//! Pin Helpers: the submenu `PIN_TRICKS_MENU` (eeschema/tools/sch_edit_tool.cpp at 8303b2ad) puts on a selection of pins and sheet pins -- Wire, No Connect, Net Label,
//! Hierarchical Label and Global Label -- and what `SCH_SELECTION_TOOL::Main` does for each (`ID_POPUP_SCH_PIN_TRICKS_*`, eeschema/tools/sch_selection_tool.cpp).
//!
//! For every pin or sheet pin of the selection (anything else in it is passed over, `if( item->Type() != SCH_PIN_T && item->Type() != SCH_SHEET_PIN_T ) continue`) one
//! undo step:
//!
//! - **No Connect** puts a no-connect flag on the pin's connection point ("No Connect Pins").
//! - **Wire** draws a stub from the connection point one wire-grid step out from the symbol, along the pin's line ("Add some length to the wire as nothing in our code base
//!   handles 0 length wires very well": left of a pin that runs right, right of one that runs left, below one that runs up, above one that runs down). KiCad then hands the
//!   stubs' ends to the drag tool; here the studio selects them with their ends picked and takes the drag from there.
//! - **Net Label**, **Hierarchical Label** and **Global Label** put a label on the connection point ("Label Pins"): its text is the pin's shown name, or `<reference>_<number>`
//!   when the pin has none (a sheet pin: its name); it reads away from the pin (`SetSpinStyle`: `LEFT` for a pin that runs right, `RIGHT` for one that runs left, `BOTTOM` for
//!   one that runs up, `UP` for one that runs down); its shape follows the pin's electrical type (bidirectional, input, output, tri-state and unspecified as they are, every
//!   other type an input; a sheet pin's own shape stands for its type, a passive one being an input too).
//!
//! The wires and flags of the sheet are cleaned up and the junctions the new items need are added (`SCH_COMMIT::Push` -> `RecalculateConnections`: `CleanUp`, junctions).
//! Which way a pin runs is the symbol's own (`pinOrientation`: the library pin's orientation turned and mirrored by the symbol's, so a swapped pin runs the way the pin it
//! swapped with does); a sheet pin runs into the sheet from the side it is on (`SHEET_SIDE::LEFT` is `PIN_RIGHT`, `RIGHT` is `PIN_LEFT`, `TOP` is `PIN_DOWN`, `BOTTOM` is `PIN_UP`).

use crate::sch_labels::PlacedLabel;
use crate::sch_move::side_of;
use crate::sch_scene::{placed_lib_symbol, Matrix, Scene};
use crate::Board;
use eda_model::ir::{field_key, parse_pin_item_id, LabelKind, LabelShape, NoConnect, Point, Nm, Wire};
use eda_model::sch_extras::LabelSpin;
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};

/// What a Pin Helper puts on a pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinHelper {
    /// A wire stub (`ID_POPUP_SCH_PIN_TRICKS_WIRE`).
    Wire,
    /// A no-connect flag.
    NoConnect,
    /// A local label.
    NetLabel,
    /// A hierarchical label.
    HierLabel,
    /// A global label.
    GlobalLabel,
}

/// The default wire grid, 50 mil (`GRID_HELPER_GRIDS::GRID_WIRES` before the user picks another).
pub const DEFAULT_WIRE_GRID_NM: Nm = 1_270_000;

/// One pin or sheet pin as a helper sees it.
struct Spot {
    /// The connection point.
    at: Point,
    /// The way the pin's line runs from the connection point into the symbol (the sheet's axes, y down), a unit step.
    body: (i64, i64),
    /// The label shape its electrical type gives.
    shape: LabelShape,
    /// What a label on it says.
    text: String,
}

/// `ELECTRICAL_PINTYPE` to the shape a label on the pin takes: `PT_BIDI`, `PT_INPUT`, `PT_OUTPUT`, `PT_TRISTATE` and `PT_UNSPECIFIED` are their own, every other type an input.
fn shape_of_type(electrical_type: &str) -> LabelShape {
    match electrical_type {
        "bidirectional" => LabelShape::Bidirectional,
        "input" => LabelShape::Input,
        "output" => LabelShape::Output,
        "tri_state" => LabelShape::TriState,
        "unspecified" => LabelShape::Passive,
        _ => LabelShape::Input,
    }
}

/// A sheet pin's shape as the pin type KiCad reads from it (`LABEL_INPUT` is `PT_INPUT`, ...; a passive one is `PT_PASSIVE`, which has no label shape of its own and gives an
/// input), and then as the label shape that type gives.
fn shape_of_sheet_pin(shape: LabelShape) -> LabelShape {
    match shape {
        LabelShape::Input => LabelShape::Input,
        LabelShape::Output => LabelShape::Output,
        LabelShape::Bidirectional => LabelShape::Bidirectional,
        LabelShape::TriState => LabelShape::TriState,
        LabelShape::Passive => LabelShape::Input,
    }
}

/// The side of a label that reads away from a pin whose line runs `body` into the symbol: `pinOrientation` -> `SetSpinStyle`.
fn spin_away_from(body: (i64, i64)) -> LabelSpin {
    match body {
        (1, 0) => LabelSpin::Left,
        (-1, 0) => LabelSpin::Right,
        (0, -1) => LabelSpin::Bottom,
        (0, 1) => LabelSpin::Up,
        // `default:` is `PIN_RIGHT`
        _ => LabelSpin::Left,
    }
}

/// What a helper draws, as the places of it (a wire read from either end and its ids and nets left out, as a cleanup may merge it into another): to tell a command that changed
/// nothing from one that did.
type Drawn = (Vec<Vec<Point>>, Vec<Point>, Vec<Point>, Vec<(String, Point)>);

fn drawn_items(sch: &eda_model::ir::SchematicSection) -> Drawn {
    let mut wires: Vec<Vec<Point>> = sch
        .wires
        .iter()
        .map(|w| {
            let mut pts = w.pts.clone();
            if pts.first() > pts.last() {
                pts.reverse();
            }
            pts
        })
        .collect();
    wires.sort();
    let mut junctions: Vec<Point> = sch.junctions.iter().map(|j| j.at).collect();
    junctions.sort();
    let mut no_connects: Vec<Point> = sch.no_connects.iter().map(|n| n.at).collect();
    no_connects.sort();
    let mut labels: Vec<(String, Point)> = sch.labels.iter().map(|l| (l.net.clone(), l.at)).collect();
    labels.sort();
    (wires, junctions, no_connects, labels)
}

fn fail(check: &str, subject: &str, why: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, subject, why)]
}

impl<'a> Board<'a> {
    /// The pin or sheet pin `id` names: where it is, which way it runs, what its label says and what shape that has. `None` for anything else (KiCad passes it over).
    fn pin_spot(&self, scene: &Scene, id: &str) -> Option<Spot> {
        if let Some((key, number)) = parse_pin_item_id(id) {
            let i = scene.sch.symbols.iter().position(|s| field_key(&s.id, s.unit) == key)?;
            let sym = &scene.sch.symbols[i];
            let k = scene.shapes[i].pins.iter().position(|(n, _)| n == number)?;
            let m = Matrix::of(sym.rot, sym.mirrored, sym.mirror_y);
            let (x, y) = m.apply(scene.shapes[i].pins[k].1 .0, scene.shapes[i].pins[k].1 .1);
            let at = Point { x: sym.at.x + x, y: sym.at.y + y };
            // `RotateAndMirrorPin( dummy, parent->GetOrientation() )`: the library pin's direction through the symbol's turn and mirror; a pin with no direction known runs right
            let body = scene.shapes[i].dirs.get(k).map(|&(dx, dy)| m.apply(dx, dy)).unwrap_or((1, 0));
            // the pin's own name and type, from the symbol as it is placed (a swapped pin keeps its own, and sits where the other one was)
            let lib_pin = placed_lib_symbol(self.model, &scene.sch, sym).and_then(|l| l.pins.iter().find(|p| p.number == number && (p.unit == 0 || p.unit == sym.unit)).cloned());
            let (name, etype) = match lib_pin {
                Some(p) => (eda_model::kicad_geom::shown_name(&p.name).to_string(), p.electrical_type),
                None => (String::new(), String::new()),
            };
            // `labelText.Printf( "%s_%s", pin->GetParentSymbol()->GetRef( &sheetPath ), pin->GetNumber() )` for a pin with no name to show
            let text = if name.is_empty() { format!("{}_{}", sym.id, number) } else { name };
            return Some(Spot { at, body, shape: shape_of_type(&etype), text });
        }
        // a sheet pin: it runs into the sheet from its side
        for sheet in &scene.sch.sheets {
            if let Some(pin) = sheet.pins.iter().find(|p| !p.id.is_empty() && p.id == id) {
                let body = match side_of(sheet, pin.at) {
                    3 => (1, 0),
                    1 => (-1, 0),
                    0 => (0, 1),
                    _ => (0, -1),
                };
                return Some(Spot { at: pin.at, body, shape: shape_of_sheet_pin(pin.shape), text: pin.name.clone() });
            }
        }
        None
    }

    /// [`SchCmd::PinHelper`](crate::sch_edit::SchCmd::PinHelper): `what` on every pin and sheet pin of `ids`, with `grid` the wire grid a stub is one step of (0 is 50 mil).
    pub(crate) fn pin_helper(&mut self, what: PinHelper, ids: &[String], grid: Nm) -> Result<(), Vec<CheckResult>> {
        let model = self.model;
        let spots: Vec<Spot> = {
            let scene = Scene::new(self.schematic()?, model);
            ids.iter().filter_map(|id| self.pin_spot(&scene, id)).collect()
        };
        if spots.is_empty() {
            return Err(fail("ops_nothing_selected", ids.first().map(String::as_str).unwrap_or("pins"), "select a pin or a sheet pin"));
        }
        let step = if grid > 0 { grid } else { DEFAULT_WIRE_GRID_NM };
        let was = drawn_items(self.schematic()?);
        match what {
            PinHelper::NoConnect => {
                let sch = self.schematic_mut()?;
                for s in &spots {
                    sch.no_connects.push(NoConnect { id: String::new(), at: s.at, pin: String::new() });
                }
            }
            PinHelper::Wire => {
                let sch = self.schematic_mut()?;
                for s in &spots {
                    // out from the symbol: the other way from the pin's line
                    let end = Point { x: s.at.x - s.body.0 * step, y: s.at.y - s.body.1 * step };
                    sch.wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts: vec![s.at, end], bus: false });
                }
            }
            PinHelper::NetLabel | PinHelper::HierLabel | PinHelper::GlobalLabel => {
                for s in &spots {
                    let kind = match what {
                        PinHelper::HierLabel => LabelKind::Hierarchical { shape: s.shape },
                        PinHelper::GlobalLabel => LabelKind::Global { shape: s.shape },
                        _ => LabelKind::Local,
                    };
                    self.place_label(&s.text, s.at, &kind, &PlacedLabel { spin: Some(spin_away_from(s.body)), ..Default::default() })?;
                }
            }
        }
        // `RecalculateConnections( LOCAL_CLEANUP )`: the junctions the new items need at the pins, and the sheet cleaned up
        let sch = self.schematic_mut()?;
        sch.assign_missing_ids();
        let mut scene = Scene::new(sch, model);
        for s in &spots {
            if scene.is_explicit_junction_needed(s.at) {
                scene.add_junction(s.at);
            }
        }
        scene.clean_up();
        *self.schematic_mut()? = scene.finish();
        // a no-connect on a pin that has one, a stub along a wire that is there: the cleanup took it back, and a step that changed nothing is no step
        if drawn_items(self.schematic()?) == was {
            return Err(fail("ops_pin_helper_unchanged", ids.first().map(String::as_str).unwrap_or("pins"), "the pins have it already"));
        }
        Ok(())
    }
}
