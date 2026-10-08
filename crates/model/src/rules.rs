//! Board Setup's edits to the board rules (`pcbnew/dialogs/dialog_board_setup.cpp` and its pages).
//!
//! The rules a board is judged by start in the *intent* (`ConstraintModel::board`, or the project
//! an imported board brought with it) and are read-only there: the intent is a file the studio never
//! writes. Board Setup edits them anyway, the way every other `design.json` overlay does
//! (`Design::nets`, the footprint and symbol libraries): the edit is stored in `design.json`, and
//! [`RulesOverlay::apply`] lays it over the model each time a board is loaded
//! (`crates/cli/src/board.rs::load`). So the router, the gates, the derived `.kicad_pro`/`.kicad_pcb`
//! kicad-cli reads, and the studio all see one set of rules, and one Undo (the design snapshot)
//! takes an edit back.
//!
//! One field per Board Setup page, each a whole-page replace like KiCad's own "OK": `None` leaves
//! the intent's values alone, `Some` is what the user set up on that page and wins. Each page's
//! struct is the page's content and nothing else; validation ports `BOARD_DESIGN_SETTINGS::
//! ValidateDesignRules` and the page panels' own checks.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ir::Um;
use crate::{BoardRules, CheckResult, ConstraintModel, NetClass, SolderMaskRules, Stackup, StackupLayer};

/// What Board Setup changed, page by page. Absent in a `design.json` nobody edited rules in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesOverlay {
    /// Design Rules > Net Classes (`panel_setup_netclasses.cpp`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_classes: Option<NetClassSettings>,
    /// Design Rules > Constraints (`panel_setup_constraints.cpp`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraints: Option<Constraints>,
    /// Board Stackup > Solder Mask/Paste (`panel_setup_mask_and_paste.cpp`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask_paste: Option<MaskPaste>,
    /// Text & Graphics > Defaults (`panel_setup_text_and_graphics.cpp`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_graphics: Option<TextGraphicsDefaults>,
    /// Board Stackup > Physical Stackup (`panel_board_stackup.cpp`): the copper layer count and the layers' thickness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stackup: Option<StackupSettings>,
    /// Design Rules > Violation Severity (`panel_setup_severities.cpp`): DRC settings key -> `error` | `warning` | `ignore`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severities: Option<BTreeMap<String, String>>,
    /// Design Rules > Custom Rules (`panel_setup_rules.cpp`): the text of the board's `.kicad_dru`, verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_rules: Option<String>,
}

/// Design Rules > Net Classes. The Default class's sizes and every other class with the net-name
/// patterns that assign nets to it. A class's `nets` are patterns (`*` and `?`, anchored, as KiCad's
/// wildcard matcher reads them), not net names; the first class whose pattern matches a net owns it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetClassSettings {
    /// The `Default` class: every net no pattern claims. Its `name` is `Default` and its `nets` empty.
    pub default: NetClass,
    pub classes: Vec<NetClass>,
}

/// Design Rules > Constraints: the board-wide minimums DRC holds every item to
/// (`BOARD_DESIGN_SETTINGS::m_MinClearance` .. `m_MaxError`, the `rules.*` keys of the `.kicad_pro`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constraints {
    /// `rules.min_clearance`: the smallest gap between copper of different nets, on top of every net class's own.
    pub min_clearance_um: Um,
    /// `rules.min_connection`: the narrowest copper neck a connection may have.
    pub min_connection_um: Um,
    /// `rules.min_track_width`.
    pub min_track_width_um: Um,
    /// `rules.min_via_annular_width`.
    pub min_annular_width_um: Um,
    /// `rules.min_via_diameter`.
    pub min_via_diameter_um: Um,
    /// `rules.min_through_hole_diameter`.
    pub min_through_hole_um: Um,
    /// `rules.min_microvia_diameter`.
    pub min_microvia_diameter_um: Um,
    /// `rules.min_microvia_drill`.
    pub min_microvia_drill_um: Um,
    /// `rules.min_hole_to_hole`.
    pub min_hole_to_hole_um: Um,
    /// `rules.min_hole_clearance`: copper to a drilled hole.
    pub min_hole_clearance_um: Um,
    /// `rules.min_copper_edge_clearance`: copper to the board edge.
    pub min_copper_edge_clearance_um: Um,
    /// `rules.min_silk_clearance`.
    pub min_silk_clearance_um: Um,
    /// `rules.min_groove_width`.
    pub min_groove_width_um: Um,
    /// `rules.min_text_height`: the smallest silkscreen text.
    pub min_silk_text_height_um: Um,
    /// `rules.min_text_thickness`.
    pub min_silk_text_thickness_um: Um,
    /// `rules.max_error`: how closely arcs are approximated by line segments.
    pub max_error_um: Um,
    /// `rules.min_resolved_spokes`: fewer connected thermal spokes than this is a starved thermal.
    pub min_resolved_spokes: u32,
    /// `rules.use_height_for_length_calcs`.
    pub use_height_for_length_calcs: bool,
    /// `zones_allow_external_fillets`.
    pub zones_allow_external_fillets: bool,
}

impl Constraints {
    /// The constraints a board currently has.
    pub fn of(b: &BoardRules) -> Constraints {
        Constraints {
            min_clearance_um: b.min_clearance_um,
            min_connection_um: b.min_connection_um,
            min_track_width_um: b.track_width_min_um,
            min_annular_width_um: b.annular_width_min_um,
            min_via_diameter_um: b.via_diameter_min_um,
            min_through_hole_um: b.via_drill_min_um,
            min_microvia_diameter_um: b.microvia_diameter_min_um,
            min_microvia_drill_um: b.microvia_drill_min_um,
            min_hole_to_hole_um: b.hole_to_hole_min_um,
            min_hole_clearance_um: b.hole_clearance_um,
            min_copper_edge_clearance_um: b.copper_edge_clearance_um.unwrap_or(crate::KICAD_EDGE_CLEARANCE_UM),
            min_silk_clearance_um: b.silk_clearance_um,
            min_groove_width_um: b.min_groove_width_um,
            min_silk_text_height_um: b.min_silk_text_height_um,
            min_silk_text_thickness_um: b.min_silk_text_thickness_um,
            max_error_um: b.max_error_um,
            min_resolved_spokes: b.min_resolved_spokes,
            use_height_for_length_calcs: b.use_height_for_length_calcs,
            zones_allow_external_fillets: b.zones_allow_external_fillets,
        }
    }

    fn write_to(&self, b: &mut BoardRules) {
        b.min_clearance_um = self.min_clearance_um;
        b.min_connection_um = self.min_connection_um;
        b.track_width_min_um = self.min_track_width_um;
        b.annular_width_min_um = self.min_annular_width_um;
        b.via_diameter_min_um = self.min_via_diameter_um;
        b.via_drill_min_um = self.min_through_hole_um;
        b.microvia_diameter_min_um = self.min_microvia_diameter_um;
        b.microvia_drill_min_um = self.min_microvia_drill_um;
        b.hole_to_hole_min_um = self.min_hole_to_hole_um;
        b.hole_clearance_um = self.min_hole_clearance_um;
        b.copper_edge_clearance_um = Some(self.min_copper_edge_clearance_um);
        b.silk_clearance_um = self.min_silk_clearance_um;
        b.min_groove_width_um = self.min_groove_width_um;
        b.min_silk_text_height_um = self.min_silk_text_height_um;
        b.min_silk_text_thickness_um = self.min_silk_text_thickness_um;
        b.max_error_um = self.max_error_um;
        b.min_resolved_spokes = self.min_resolved_spokes;
        b.use_height_for_length_calcs = self.use_height_for_length_calcs;
        b.zones_allow_external_fillets = self.zones_allow_external_fillets;
        // The user (or the project this came from) has said what the floors are: they are exact, not
        // lowered to the narrowest class the way the intent's unstated minimums are
        // (`eda_kicad::export_kicad_pro`).
        b.constraints_explicit = true;
    }

    /// `BOARD_DESIGN_SETTINGS::ValidateDesignRules`, the range check `PANEL_SETUP_CONSTRAINTS::TransferDataFromWindow`
    /// runs: the first value out of range, as `(setting name, message)`. The names are the `.kicad_pro` keys.
    pub fn validate(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        let mut range = |name: &'static str, v: Um, min_um: Um, max_um: Um| {
            if v < min_um || v > max_um {
                out.push((name, format!("Value must be between {} and {} mm.", fmt_mm(min_um), fmt_mm(max_um))));
            }
        };
        range("min_clearance", self.min_clearance_um, 0, 25_000);
        range("min_connection", self.min_connection_um, 0, 100_000);
        range("min_track_width", self.min_track_width_um, 0, 25_000);
        range("min_via_annular_width", self.min_annular_width_um, 0, 25_000);
        range("min_via_diameter", self.min_via_diameter_um, 0, 25_000);
        range("min_through_hole_diameter", self.min_through_hole_um, 0, 25_000);
        range("min_microvia_diameter", self.min_microvia_diameter_um, 0, 10_000);
        range("min_microvia_drill", self.min_microvia_drill_um, 0, 10_000);
        range("min_hole_to_hole", self.min_hole_to_hole_um, 0, 10_000);
        range("min_hole_clearance", self.min_hole_clearance_um, 0, 100_000);
        range("min_silk_clearance", self.min_silk_clearance_um, -10_000, 100_000);
        range("min_groove_width", self.min_groove_width_um, 0, 25_000);
        range("min_text_height", self.min_silk_text_height_um, 0, 100_000);
        range("min_text_thickness", self.min_silk_text_thickness_um, 0, 25_000);
        // `-0.01` is the flag for the pre-6.0 way of measuring the edge; a µm integer cannot hold -0.01 mm.
        range("min_copper_edge_clearance", self.min_copper_edge_clearance_um, -10, 25_000);
        range("max_error", self.max_error_um, 1, 1_000);
        if self.min_resolved_spokes > 99 {
            out.push(("min_resolved_spokes", "Value must be between 0 and 99.".into()));
        }
        out
    }
}

/// A length in µm as the millimetres a message shows.
fn fmt_mm(um: Um) -> String {
    let s = format!("{:.4}", um as f64 / 1000.0);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.to_string() }
}

/// Board Stackup > Solder Mask/Paste. `SolderMaskRules` is the model's own copy of the same values (the mask half
/// is read by the solder-mask checks and written to the `.kicad_pcb`); the paste half is new.
pub type MaskPaste = SolderMaskRules;

/// `ValidateDesignRules`' solder mask and paste ranges: `(setting name, message)` for each value out of range.
pub fn validate_mask_paste(m: &MaskPaste) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let mut range = |name: &'static str, v: Um, min_um: Um, max_um: Um| {
        if v < min_um || v > max_um {
            out.push((name, format!("Value must be between {} and {} mm.", fmt_mm(min_um), fmt_mm(max_um))));
        }
    };
    range("solder_mask_expansion", m.expansion_um, -25_000, 25_000);
    range("solder_mask_min_width", m.min_width_um, 0, 25_000);
    range("solder_mask_to_copper_clearance", m.to_copper_clearance_um, 0, 25_000);
    range("solder_paste_margin", m.paste_margin_um, -25_000, 25_000);
    if !m.paste_margin_ratio.is_finite() || !(-1.0..=1.0).contains(&m.paste_margin_ratio) {
        out.push(("solder_paste_margin_ratio", "Value must be between -1.000 and 1.000.".into()));
    }
    out
}

/// One row of the Text & Graphics Defaults grid: a layer class's line thickness and, for the classes that
/// carry text, the default text.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerClassDefaults {
    /// `defaults.<class>_line_width`.
    pub line_width_um: Um,
    /// `defaults.<class>_text_size_h`.
    pub text_width_um: Um,
    /// `defaults.<class>_text_size_v`.
    pub text_height_um: Um,
    /// `defaults.<class>_text_thickness`.
    pub text_thickness_um: Um,
    /// `defaults.<class>_text_italic`.
    pub italic: bool,
    /// `defaults.<class>_text_upright` (keep upright).
    pub upright: bool,
}

/// Text & Graphics > Defaults: `BOARD_DESIGN_SETTINGS::m_LineThickness`/`m_TextSize`/`m_TextThickness`/
/// `m_TextItalic`/`m_TextUpright` per layer class. The board outline and courtyard classes carry a line
/// thickness only (`ROW_EDGES`/`ROW_COURTYARD`: their text cells are disabled in the grid).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextGraphicsDefaults {
    pub silk: LayerClassDefaults,
    pub copper: LayerClassDefaults,
    /// `defaults.board_outline_line_width`.
    pub edge_cuts_line_width_um: Um,
    /// `defaults.courtyard_line_width`.
    pub courtyard_line_width_um: Um,
    pub fab: LayerClassDefaults,
    pub others: LayerClassDefaults,
}

impl Default for TextGraphicsDefaults {
    /// `BOARD_DESIGN_SETTINGS::BOARD_DESIGN_SETTINGS()`: silk 0.1 / 1x1 / 0.1, copper 0.2 / 1.5x1.5 / 0.3,
    /// outline 0.05, courtyard 0.05, fab and others 0.1 / 1x1 / 0.15.
    fn default() -> Self {
        let class = |line, size, thickness| LayerClassDefaults { line_width_um: line, text_width_um: size, text_height_um: size, text_thickness_um: thickness, italic: false, upright: true };
        TextGraphicsDefaults {
            silk: class(100, 1000, 100),
            copper: class(200, 1500, 300),
            edge_cuts_line_width_um: 50,
            courtyard_line_width_um: 50,
            fab: class(100, 1000, 150),
            others: class(100, 1000, 150),
        }
    }
}

impl TextGraphicsDefaults {
    /// Whether this is the factory set (nothing to write to a project file).
    pub fn is_default(&self) -> bool {
        *self == TextGraphicsDefaults::default()
    }

    /// `PANEL_SETUP_TEXT_AND_GRAPHICS::TransferDataFromWindow`'s checks, as `(row label, message)`: a line width
    /// of 0.005 to 100 mm, a text size of 0.001 to 250 mm, a text thickness of at least 0.005 mm and, to stay
    /// readable, at most a quarter of the text size.
    pub fn validate(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        let line = |label: &'static str, w: Um, out: &mut Vec<(&'static str, String)>| {
            if !(5..=100_000).contains(&w) {
                out.push((label, "Incorrect line width. It must be between 0.005 and 100 mm.".into()));
            }
        };
        let text = |label: &'static str, c: &LayerClassDefaults, out: &mut Vec<(&'static str, String)>| {
            line(label, c.line_width_um, out);
            if !(1..=250_000).contains(&c.text_width_um) || !(1..=250_000).contains(&c.text_height_um) {
                out.push((label, "Text size is incorrect. Size must be between 0.001 and 250 mm.".into()));
            } else {
                let max_thickness = (c.text_width_um.min(c.text_height_um) / 4).min(100_000);
                if c.text_thickness_um > max_thickness {
                    out.push((label, format!("Text thickness is too large. It must be at most {} mm.", fmt_mm(max_thickness))));
                } else if c.text_thickness_um < 5 {
                    out.push((label, "Text thickness is too small. It must be at least 0.005 mm.".into()));
                }
            }
        };
        text("Silk Layers", &self.silk, &mut out);
        text("Copper Layers", &self.copper, &mut out);
        line("Edge Cuts", self.edge_cuts_line_width_um, &mut out);
        line("Courtyards", self.courtyard_line_width_um, &mut out);
        text("Fab Layers", &self.fab, &mut out);
        text("Other Layers", &self.others, &mut out);
        out
    }
}

/// Board Stackup > Physical Stackup: how many copper layers the board has and the layers between them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackupSettings {
    /// 2, 4, ... 32 (`PANEL_SETUP_BOARD_STACKUP::m_choiceCopperLayers`).
    pub copper_layers: u8,
    /// Top to bottom, silkscreen to silkscreen.
    pub stackup: Stackup,
}

/// Copper layer names in stack order for an `n`-layer board: `F.Cu`, `In1.Cu` .. `In{n-2}.Cu`, `B.Cu`.
pub fn copper_layer_names(n: usize) -> Vec<String> {
    let n = n.max(2);
    let mut v = vec!["F.Cu".to_string()];
    v.extend((1..n - 1).map(|i| format!("In{i}.Cu")));
    v.push("B.Cu".to_string());
    v
}

/// `BOARD_STACKUP::BuildDefaultStackupList` (`pcbnew/board_stackup_manager/board_stackup.cpp`) for a board of
/// `copper_layers` and `board_thickness_um`: silkscreen, paste and mask on each face, the copper layers with a
/// dielectric (alternating core and prepreg, FR4) between each pair, the dielectrics sharing what is left of the
/// thickness after the copper (0.035 mm a layer) and the two masks (0.01 mm each).
pub fn default_stackup(copper_layers: usize, board_thickness_um: Um) -> Stackup {
    const COPPER_MM: f64 = 0.035;
    const MASK_MM: f64 = 0.01;
    let n = copper_layers.max(2);
    let board_mm = board_thickness_um as f64 / 1000.0;
    let dielectric_mm = ((board_mm - COPPER_MM * n as f64 - MASK_MM * 2.0) / (n as f64 - 1.0).max(1.0)).max(0.0);
    let layer = |name: &str, kind: &str, material: Option<&str>, thickness: Option<f64>, epsilon: Option<f64>, tangent: Option<f64>| StackupLayer {
        name: name.into(),
        material: material.map(String::from),
        thickness_mm: thickness,
        kind: Some(kind.into()),
        epsilon_r: epsilon,
        loss_tangent: tangent,
    };
    let mut layers = vec![
        layer("F.SilkS", "Top Silk Screen", None, None, None, None),
        layer("F.Paste", "Top Solder Paste", None, None, None, None),
        layer("F.Mask", "Top Solder Mask", None, Some(MASK_MM), None, None),
    ];
    for (i, name) in copper_layer_names(n).iter().enumerate() {
        layers.push(layer(name, "copper", None, Some(COPPER_MM), None, None));
        if i + 1 < n {
            let kind = if i % 2 == 0 { "core" } else { "prepreg" };
            layers.push(layer(&format!("dielectric {}", i + 1), kind, Some("FR4"), Some(dielectric_mm), Some(4.5), Some(0.02)));
        }
    }
    layers.extend([
        layer("B.Mask", "Bottom Solder Mask", None, Some(MASK_MM), None, None),
        layer("B.Paste", "Bottom Solder Paste", None, None, None, None),
        layer("B.SilkS", "Bottom Silk Screen", None, None, None, None),
    ]);
    Stackup { layers, copper_finish: None, dielectric_constraints: false, edge_connector: 0, edge_plating: false }
}

/// The board thickness a stackup adds up to (`BOARD_STACKUP::BuildBoardThicknessFromStackup`): every layer's
/// thickness, in µm.
pub fn stackup_thickness_um(s: &Stackup) -> Um {
    (s.layers.iter().filter_map(|l| l.thickness_mm).sum::<f64>() * 1000.0).round() as Um
}

impl StackupSettings {
    /// The default stackup for `copper_layers` and `board_thickness_um`.
    pub fn new(copper_layers: usize, board_thickness_um: Um) -> StackupSettings {
        StackupSettings { copper_layers: copper_layers.clamp(2, 32) as u8, stackup: default_stackup(copper_layers, board_thickness_um) }
    }

    /// A stackup for a different copper layer count, keeping what this one says about every layer the new one
    /// still has (`BOARD_STACKUP::SynchronizeWithBoard`): a copper or dielectric layer that exists in both keeps its
    /// thickness and material, a new one gets the default. The board thickness is kept.
    pub fn with_copper_layers(&self, copper_layers: usize) -> StackupSettings {
        let mut next = StackupSettings::new(copper_layers, stackup_thickness_um(&self.stackup).max(1));
        for l in next.stackup.layers.iter_mut() {
            if let Some(old) = self.stackup.layers.iter().find(|o| o.name == l.name && o.kind == l.kind) {
                *l = old.clone();
            }
        }
        next.stackup.copper_finish = self.stackup.copper_finish.clone();
        next.stackup.dielectric_constraints = self.stackup.dielectric_constraints;
        next.stackup.edge_connector = self.stackup.edge_connector;
        next.stackup.edge_plating = self.stackup.edge_plating;
        next
    }

    /// Whether this is a stackup the model can hold: an even copper layer count from 2 to 32 whose copper layers
    /// are the ones that count names, in order, and nothing with a negative thickness. Returns the first problem.
    pub fn validate(&self) -> Result<(), String> {
        let n = self.copper_layers as usize;
        if !(2..=32).contains(&n) || n % 2 != 0 {
            return Err(format!("A board has 2, 4, 6 .. 32 copper layers, not {n}."));
        }
        let copper: Vec<&str> = self.stackup.layers.iter().filter(|l| l.kind.as_deref() == Some("copper")).map(|l| l.name.as_str()).collect();
        let expect = copper_layer_names(n);
        if copper != expect.iter().map(String::as_str).collect::<Vec<_>>() {
            return Err(format!("The stackup's copper layers {copper:?} are not the {n} layers {expect:?}."));
        }
        if let Some(l) = self.stackup.layers.iter().find(|l| l.thickness_mm.is_some_and(|t| !t.is_finite() || t < 0.0)) {
            return Err(format!("Layer {:?} has a negative or invalid thickness.", l.name));
        }
        Ok(())
    }
}

pub use crate::drc_checks::DRC_CHECKS;

/// A severity word KiCad writes (`SeverityToString`): `error`, `warning` or `ignore`.
pub fn is_severity(s: &str) -> bool {
    matches!(s, "error" | "warning" | "ignore")
}

impl RulesOverlay {
    /// Lay this overlay over `model`: each page that is `Some` replaces what the intent (or the imported
    /// project) said about that page. The custom rules' text is stored on the model; turning it into the
    /// parsed [`crate::CustomRule`]s is the loader's job (`eda_kicad::merge_custom_rules`), which this crate
    /// cannot call.
    pub fn apply(&self, model: &mut ConstraintModel) {
        if let Some(nc) = &self.net_classes {
            let b = &mut model.board;
            if let Some(v) = nc.default.track_width {
                b.track_width = v;
            }
            if let Some(v) = nc.default.clearance {
                b.clearance = v;
            }
            if let Some(v) = nc.default.via_diameter {
                b.via_diameter = v;
            }
            if let Some(v) = nc.default.via_drill {
                b.via_drill = v;
            }
            b.default_class = Some(nc.default.clone());
            b.net_classes = nc.classes.clone();
        }
        if let Some(c) = &self.constraints {
            c.write_to(&mut model.board);
        }
        if let Some(m) = &self.mask_paste {
            model.board.solder_mask = m.clone();
        }
        if let Some(t) = &self.text_graphics {
            model.board.text_graphics = *t;
        }
        if let Some(s) = &self.stackup {
            model.board.layers = copper_layer_names(s.copper_layers as usize);
            model.board.board_thickness_um = stackup_thickness_um(&s.stackup);
            model.stackup = Some(s.stackup.clone());
        }
        if let Some(sev) = &self.severities {
            model.board.rule_severities = sev.clone();
        }
        if let Some(text) = &self.custom_rules {
            model.board.custom_rules_text = if text.trim().is_empty() { None } else { Some(text.clone()) };
            model.board.custom_rules.clear();
        }
    }

    /// Whether any page is set.
    pub fn is_empty(&self) -> bool {
        *self == RulesOverlay::default()
    }
}

/// `MAXIMUM_CLEARANCE`: 500 mm.
pub const MAXIMUM_CLEARANCE_UM: Um = 500_000;

/// Validate a Net Classes page: the names and the sizes.
///
/// `panel_setup_netclasses.cpp` refuses an empty or duplicate class name, a class named like the
/// Default one, a pattern with no class, and a size that is not positive; the checks are the same.
pub fn validate_net_classes(s: &NetClassSettings) -> Result<(), CheckResult> {
    let bad = |what: &str, why: String| Err(CheckResult::fail("ops_bad_netclasses", what, why));
    if s.default.name != "Default" || !s.default.nets.is_empty() {
        return bad("Default", "the Default class is named Default and has no net patterns of its own".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for c in std::iter::once(&s.default).chain(&s.classes) {
        if c.name.trim().is_empty() {
            return bad("(unnamed)", "a net class needs a name".into());
        }
        // `validateNetclassName` compares without regard to case ("Power" and "power" are one name).
        if !seen.insert(c.name.trim().to_lowercase()) {
            return bad(&c.name, format!("there are two net classes named {:?}", c.name));
        }
        if c.nets.iter().any(|p| p.trim().is_empty()) {
            return bad(&c.name, "a net pattern cannot be empty".into());
        }
        for (what, v) in [("track width", c.track_width), ("clearance", c.clearance), ("via diameter", c.via_diameter), ("via drill", c.via_drill), ("microvia diameter", c.microvia_diameter), ("microvia drill", c.microvia_drill), ("differential pair width", c.diff_pair_width), ("differential pair gap", c.diff_pair_gap), ("differential pair via gap", c.diff_pair_via_gap)] {
            if let Some(v) = v {
                if v < 0 || (v == 0 && what != "clearance" && what != "differential pair gap" && what != "differential pair via gap") {
                    return bad(&c.name, format!("{what} is {v} um; sizes cannot be negative, and only a clearance can be zero"));
                }
            }
        }
        // `MAXIMUM_CLEARANCE` (board_design_settings.h): a larger clearance overflows the integer maths in KiCad's engines.
        if c.clearance.is_some_and(|v| v > MAXIMUM_CLEARANCE_UM) {
            return bad(&c.name, format!("clearance is larger than {} mm", MAXIMUM_CLEARANCE_UM / 1000));
        }
        if let (Some(d), Some(drill)) = (c.via_diameter, c.via_drill) {
            if drill >= d {
                return bad(&c.name, format!("via drill {drill} um is not smaller than the {d} um via, which leaves no annular ring"));
            }
        }
    }
    Ok(())
}

/// Map a `BoardRules` back to the page content the dialog starts from, for a board whose intent holds the rules.
pub fn net_class_settings_of(b: &BoardRules) -> NetClassSettings {
    let mut default = b.default_class.clone().unwrap_or_else(|| NetClass {
        name: "Default".into(),
        nets: vec![],
        track_width: None,
        clearance: None,
        via_diameter: None,
        via_drill: None,
        microvia_diameter: None,
        microvia_drill: None,
        diff_pair_width: None,
        diff_pair_gap: None,
        diff_pair_via_gap: None,
        priority: 0,
    });
    default.track_width = Some(b.track_width);
    default.clearance = Some(b.clearance);
    default.via_diameter = Some(b.via_diameter);
    default.via_drill = Some(b.via_drill);
    NetClassSettings { default, classes: b.net_classes.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(name: &str, nets: &[&str]) -> NetClass {
        NetClass { name: name.into(), nets: nets.iter().map(|s| s.to_string()).collect(), track_width: Some(300), clearance: Some(250), via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: None, diff_pair_gap: None, diff_pair_via_gap: None, priority: 0 }
    }

    #[test]
    fn a_default_stackup_adds_up_to_the_board_thickness_with_copper_and_dielectrics_in_order() {
        for n in [2usize, 4, 6] {
            let s = default_stackup(n, 1_600);
            let copper: Vec<&str> = s.layers.iter().filter(|l| l.kind.as_deref() == Some("copper")).map(|l| l.name.as_str()).collect();
            assert_eq!(copper, copper_layer_names(n).iter().map(String::as_str).collect::<Vec<_>>());
            assert_eq!(s.layers.iter().filter(|l| matches!(l.kind.as_deref(), Some("core" | "prepreg"))).count(), n - 1, "one dielectric between each pair of copper layers");
            assert!((stackup_thickness_um(&s) - 1_600).abs() <= 2, "{n} layers add up to {}", stackup_thickness_um(&s));
        }
        let four = default_stackup(4, 1_600);
        let kinds: Vec<&str> = four.layers.iter().filter_map(|l| l.kind.as_deref()).filter(|k| matches!(*k, "core" | "prepreg")).collect();
        assert_eq!(kinds, vec!["core", "prepreg", "core"], "KiCad alternates core and prepreg, starting with core");
    }

    #[test]
    fn changing_the_copper_layer_count_keeps_the_thickness_of_the_layers_that_remain() {
        let mut two = StackupSettings::new(2, 1_600);
        two.stackup.layers.iter_mut().find(|l| l.name == "F.Cu").unwrap().thickness_mm = Some(0.07);
        let four = two.with_copper_layers(4);
        assert_eq!(four.copper_layers, 4);
        assert_eq!(four.stackup.layers.iter().find(|l| l.name == "F.Cu").unwrap().thickness_mm, Some(0.07), "F.Cu keeps what was set");
        assert!(four.stackup.layers.iter().any(|l| l.name == "In2.Cu"), "the new inner layers appear");
        assert!(four.validate().is_ok());
        assert!(StackupSettings { copper_layers: 3, ..four.clone() }.validate().is_err(), "an odd copper layer count is refused");
    }

    /// The Physical Stackup page rebuilds the table in the browser when the copper layer count changes
    /// (`web/studio/src/kicad-port/boardSetupRules.ts`, a port of `default_stackup` and `with_copper_layers`).
    /// Both languages are held to the same cases in `web/studio/src/kicad/default_stackups.json`, so they cannot drift:
    /// this test fails when the file is not what this code produces (`EDA_WRITE_FIXTURES=1` rewrites it), and
    /// `boardSetupRules.test.ts` fails when the TypeScript does not produce what the file says.
    #[test]
    fn the_web_pages_default_stackups_are_the_ones_computed_here() {
        let defaults: Vec<serde_json::Value> = [(2usize, 1_600), (4, 1_600), (4, 1_000), (6, 1_600), (8, 2_400)]
            .iter()
            .map(|&(n, t)| serde_json::json!({ "copper_layers": n, "board_thickness_um": t, "stackup": default_stackup(n, t) }))
            .collect();
        let mut edited = StackupSettings::new(4, 1_600);
        for (name, t) in [("F.Cu", 0.07), ("dielectric 1", 0.2), ("B.Mask", 0.02)] {
            edited.stackup.layers.iter_mut().find(|l| l.name == name).unwrap().thickness_mm = Some(t);
        }
        edited.stackup.layers.iter_mut().find(|l| l.name == "dielectric 2").unwrap().material = Some("Polyimide".into());
        edited.stackup.copper_finish = Some("ENIG".into());
        edited.stackup.edge_plating = true;
        let resizes: Vec<serde_json::Value> = [6usize, 2, 4]
            .iter()
            .map(|&to| serde_json::json!({ "from": edited, "to_copper_layers": to, "expected": edited.with_copper_layers(to) }))
            .collect();
        let fixture = serde_json::json!({ "defaults": defaults, "resizes": resizes });

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/studio/src/kicad/default_stackups.json");
        if std::env::var_os("EDA_WRITE_FIXTURES").is_some() {
            std::fs::write(&path, serde_json::to_string_pretty(&fixture).unwrap() + "\n").unwrap();
        }
        let on_disk: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_default()).unwrap_or(serde_json::Value::Null);
        assert_eq!(on_disk, fixture, "{} is stale: run `EDA_WRITE_FIXTURES=1 cargo test -p eda-model default_stackups` and commit it", path.display());
    }

    #[test]
    fn the_overlay_replaces_a_page_at_a_time_and_leaves_the_rest_to_the_intent() {
        let mut model = ConstraintModel::default();
        model.board.net_classes.push(class("power", &["VBUS"]));
        let before = model.board.clone();
        RulesOverlay::default().apply(&mut model);
        assert_eq!(model.board, before, "an empty overlay changes nothing");

        let overlay = RulesOverlay {
            constraints: Some(Constraints { min_clearance_um: 300, ..Constraints::of(&before) }),
            net_classes: Some(NetClassSettings { default: NetClass { track_width: Some(250), clearance: Some(180), ..net_class_settings_of(&before).default }, classes: vec![class("signal", &["SPI_*", "I2C_?"])] }),
            ..Default::default()
        };
        overlay.apply(&mut model);
        assert_eq!(model.board.min_clearance_um, 300);
        assert!(model.board.constraints_explicit, "the floors are exact once they are set");
        assert_eq!((model.board.track_width, model.board.clearance), (250, 180), "the Default class's sizes are the board's defaults");
        assert_eq!(model.board.net_classes.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["signal"], "the classes are replaced wholesale");
        assert_eq!(model.board.annular_width_min_um, before.annular_width_min_um, "a constraint the page does not change keeps its value");
        assert_eq!(model.board.via_drill, before.via_drill, "a Default size the page leaves unset keeps the board's");
    }

    #[test]
    fn constraints_are_range_checked_like_validate_design_rules() {
        let ok = Constraints::of(&BoardRules::default());
        assert!(ok.validate().is_empty(), "{:?}", ok.validate());
        let bad = Constraints { min_clearance_um: 30_000, min_resolved_spokes: 100, max_error_um: 0, ..ok.clone() };
        let names: Vec<&str> = bad.validate().iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["min_clearance", "max_error", "min_resolved_spokes"]);
        assert!(bad.validate()[0].1.contains("between 0 and 25"), "{:?}", bad.validate()[0]);
        assert!(Constraints { min_silk_clearance_um: -5_000, ..ok }.validate().is_empty(), "the silk clearance may be negative, down to -10 mm");
    }

    #[test]
    fn text_defaults_follow_the_grids_own_limits() {
        assert!(TextGraphicsDefaults::default().validate().is_empty());
        let mut t = TextGraphicsDefaults::default();
        t.silk.text_thickness_um = 400; // a quarter of the 1 mm text is 0.25
        t.copper.line_width_um = 2;
        t.fab.text_width_um = 300_000;
        let rows: Vec<&str> = t.validate().iter().map(|(r, _)| *r).collect();
        assert_eq!(rows, vec!["Silk Layers", "Copper Layers", "Fab Layers"], "{:?}", t.validate());
    }

    #[test]
    fn net_class_pages_refuse_what_the_panel_refuses() {
        let settings = |classes: Vec<NetClass>| NetClassSettings { default: NetClass { name: "Default".into(), nets: vec![], ..class("x", &[]) }, classes };
        assert!(validate_net_classes(&settings(vec![class("power", &["VBUS"]), class("signal", &["SPI_*"])])).is_ok());
        for (bad, why) in [
            (vec![class("a", &["X"]), class("a", &["Y"])], "duplicate"),
            (vec![class("Power", &["X"]), class("power", &["Y"])], "duplicate without regard to case"),
            (vec![NetClass { clearance: Some(500_001), ..class("huge", &["X"]) }], "a clearance over 500 mm"),
            (vec![class("", &["X"])], "unnamed"),
            (vec![class("Default", &["X"])], "named like the Default class"),
            (vec![class("a", &[""])], "empty pattern"),
        ] {
            assert!(validate_net_classes(&settings(bad)).is_err(), "{why}");
        }
        let mut wide_drill = class("vias", &["V*"]);
        wide_drill.via_diameter = Some(400);
        wide_drill.via_drill = Some(400);
        assert!(validate_net_classes(&settings(vec![wide_drill])).is_err(), "a drill as wide as its via leaves no annular ring");
    }

    #[test]
    fn the_known_drc_checks_have_unique_keys_and_valid_default_severities() {
        let mut seen = std::collections::BTreeSet::new();
        for (key, sev) in DRC_CHECKS {
            assert!(seen.insert(*key), "{key} twice");
            assert!(is_severity(sev), "{key}: {sev}");
        }
    }
}
