//! What the multichannel tool reads of the footprints of a board: the sheet each is on, its component classes, its group, its pads and their nets, its box.
//!
//! `MULTICHANNEL_TOOL` works on `FOOTPRINT*`s and asks each for `GetSheetname()`, `GetSheetfile()`, `GetComponentClass()`, `GetParentGroup()`, `Pads()` and
//! `GetBoundingBox( false )`. A footprint here is a pose of a part, so those answers are put together from the places this model keeps them:
//!
//! * **The sheet.** An imported board has what its file said (`FootprintExtra::sheetname`, `sheetfile`: the netlist update's `Sheetname` and `Sheetfile`). A board made
//!   here has its schematic, stored or -- until an edit stores it -- the module sheets the studio derives from the intent (`eda_model::modules::infer_modules`, the
//!   names `derive_hierarchy` gives the sheets): the sheet path of a symbol is `SCH_SHEET_PATH::PathHumanReadable`, `/` and the name of each sheet down to it, each followed
//!   by a `/`. A sheet placed more than once has the reference of each symbol per placement (`SymbolPathOverride`), and the symbol the footprint stands for is identified
//!   by the sheet's file and the symbol's place in it, which is what the tail of `FOOTPRINT::GetPath()` is to KiCad's tie breaker.
//! * **The component classes.** The footprint's own (`FootprintExtra::component_classes`, `GetStaticComponentClass`) and the ones the project's assignment rules give it
//!   (`COMPONENT_CLASS_MANAGER::GetDynamicComponentClassesForFootprint`: each rule is a DRC-language condition, `COMPONENT_CLASS_ASSIGNMENT_DATA::
//!   GetAssignmentInDRCLanguage`, here evaluated for the footprint), and a class per sheet when the project asks.
//! * **The box.** `FOOTPRINT::GetBoundingBox( false )`: the footprint's position grown by 0.25 mm, its pads, its courtyard and its graphics -- not its texts, and not the
//!   annotations on the user layers of a footprint that has a side.

use super::Board;
use eda_model::component_classes::{ClassAssignment, ComponentClassSettings, ConditionKind};
use eda_model::footprint::{placed_pads, to_board, Model3d};
use eda_model::fp_edit::{self, FieldLayout, UserField};
use eda_model::ir::{Design, FootprintExtra, FootprintInstance, Point, Side, Nm};
use eda_model::multichannel::Box4;
use eda_model::{ConstraintModel, Footprint, Part};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Where a part is in the schematic's hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PartSheet {
    /// `SCH_SHEET_PATH::PathHumanReadable`: `/` for the root, `/MCU (U1)/` for a sheet below it.
    pub name: String,
    /// The sheet's file (`SCH_SHEET::GetFileName`); empty for the root, whose file this model does not name.
    pub file: String,
    /// The symbol the part stands for, identified within its file: two placements of one sheet have the same.
    pub symbol: Option<String>,
}

/// A footprint of the board as the tool reads it.
#[derive(Debug, Clone)]
pub(crate) struct McFootprint {
    pub reference: String,
    /// `FOOTPRINT::GetFPIDAsString`: the library link the part names, "" when it names none.
    pub fpid: String,
    pub pose: FootprintInstance,
    /// The pads in the footprint's own order: the number and the net's name ("" for none).
    pub pads: Vec<(String, String)>,
    pub sheetname: String,
    pub sheetfile: String,
    /// The names of the classes of `GetComponentClass()`.
    pub classes: BTreeSet<String>,
    /// `GetParentGroup()`: the id of the group that lists the footprint.
    pub group: Option<String>,
    /// The names of the footprint's group and of the groups above it, the innermost first (`memberOfGroup` walks up).
    pub groups_up: Vec<String>,
    pub locked: bool,
    pub symbol: Option<String>,
    /// `GetBoundingBox( false )`.
    pub bbox: Box4,
    /// The Reference and Value fields as laid out (in the footprint's own frame), and the user fields: what Repeat Layout copies onto the footprint that stands for this one.
    pub reference_layout: Option<FieldLayout>,
    pub value_layout: Option<FieldLayout>,
    pub user_fields: Vec<UserField>,
    /// `FOOTPRINT::Models()`.
    pub models3d: Vec<Model3d>,
}

/// The footprints of a board with the nets numbered the way the matcher needs them.
pub(crate) struct McBoard {
    pub fps: Vec<McFootprint>,
    /// A net's code: 1 and up by name, 0 for none.
    pub codes: BTreeMap<String, i64>,
}

/// `A.memberOfSheet( 'x' )`'s test, which `wxString::Matches` makes a wildcard one: the sheet of the footprint without its closing `/` against the pattern without its closing `/`;
/// the root (`/`, or no sheet at all) is what an empty pattern and `/` name.
pub(crate) fn member_of_sheet(sheet: &str, wanted: &str) -> bool {
    let (sheet, wanted) = (sheet.strip_suffix('/').unwrap_or(sheet), wanted.strip_suffix('/').unwrap_or(wanted));
    if eda_drc::pcbexpr::wild_compare(wanted, sheet, true) {
        return true;
    }
    // `refName.Matches( "/" )`: the pattern, once its closing `/` is off, is "/" itself only if it was "//"; an empty one names the root
    (eda_drc::pcbexpr::wild_compare("/", wanted, true) || wanted.is_empty()) && sheet.is_empty()
}

/// `memberOfSheetOrChildrenFunc`: the sheet of the footprint, or any sheet below it, is the pattern -- compared level by level, each a wildcard.
pub(crate) fn member_of_sheet_or_children(sheet: &str, wanted: &str) -> bool {
    let (sheet, wanted) = (sheet.strip_suffix('/').unwrap_or(sheet), wanted.strip_suffix('/').unwrap_or(wanted));
    let sheet_path: Vec<&str> = sheet.split('/').collect();
    let wanted_path: Vec<&str> = wanted.split('/').collect();
    if wanted_path.len() > sheet_path.len() {
        return false;
    }
    if (eda_drc::pcbexpr::wild_compare("/", wanted, true) || wanted.is_empty()) && sheet.is_empty() {
        return true;
    }
    wanted_path.iter().zip(&sheet_path).all(|(w, s)| eda_drc::pcbexpr::wild_compare(w, s, true))
}

/// `MULTICHANNEL_TOOL::queryComponentsInSheet`'s test: the footprint's sheet is the one named or starts with it and a `/`.
pub(crate) fn in_sheet_or_below(sheetname: &str, wanted: &str) -> bool {
    let wanted = wanted.strip_suffix('/').unwrap_or(wanted);
    let sn = sheetname.strip_suffix('/').unwrap_or(sheetname);
    sn == wanted || sn.starts_with(&format!("{wanted}/"))
}

/// `memberOfGroupFunc`: some group the footprint is in, or one above that, is named as the pattern says (a wildcard).
pub(crate) fn member_of_group(groups_up: &[String], wanted: &str) -> bool {
    groups_up.iter().any(|g| eda_drc::pcbexpr::wild_compare(wanted, g, true))
}

/// The tests of `COMPONENT_CLASS_ASSIGNMENT_DATA::GetAssignmentInDRCLanguage`'s expressions, as one footprint answers them.
struct ClassFacts<'a> {
    reference: &'a str,
    fpid: &'a str,
    front: bool,
    /// `A.Orientation`, KiCad's counter-clockwise degrees in 0..360.
    orientation: f64,
    fields: &'a [(String, String)],
    sheet: &'a str,
}

/// A string `==` of the expression language: case-insensitive, and a wildcard when the literal has a `*` or a `?` (`VALUE::EqualTo`).
fn lib_eval_eq(value: &str, literal: &str) -> bool {
    if literal.contains('*') || literal.contains('?') {
        eda_drc::pcbexpr::wild_compare(literal, value, false)
    } else {
        value.to_lowercase() == literal.to_lowercase()
    }
}

/// One condition of an assignment as the DRC-language expression it becomes: `None` for the ones that make no expression (a blank one, side `Any`, a rotation that is no
/// whole number), which `GetAssignmentInDRCLanguage` leaves out.
fn condition_holds(kind: ConditionKind, primary: &str, secondary: &str, f: &ClassFacts) -> Option<bool> {
    let (primary, secondary) = (primary.trim(), secondary.trim());
    match kind {
        ConditionKind::Reference => {
            let refs: Vec<&str> = primary.split(',').collect();
            if primary.is_empty() {
                return None;
            }
            Some(refs.iter().any(|r| lib_eval_eq(f.reference, r)))
        }
        ConditionKind::Footprint => (!primary.is_empty()).then(|| lib_eval_eq(f.fpid, primary)),
        ConditionKind::Side => match primary {
            "Any" | "" => None,
            "Front" => Some(f.front),
            _ => Some(!f.front),
        },
        ConditionKind::Rotation => match primary.parse::<i64>() {
            Ok(deg) if primary != "Any" => Some((f.orientation - deg as f64).abs() < 1e-6),
            _ => None,
        },
        ConditionKind::FootprintField => {
            if primary.is_empty() || secondary.is_empty() {
                return None;
            }
            Some(f.fields.iter().find(|(n, _)| n == primary).is_some_and(|(_, v)| lib_eval_eq(v, secondary)))
        }
        // A DRC expression of the user's own: the expression language is evaluated for the inspection commands, on rule items; a class assigned by one is not
        // worked out here, and no footprint is given it.
        ConditionKind::Custom => (!primary.is_empty()).then_some(false),
        ConditionKind::SheetName => (!primary.is_empty()).then(|| member_of_sheet(f.sheet, primary)),
    }
}

/// `COMPONENT_CLASS_ASSIGNMENT_RULE::Matches`: the conditions, joined by `&&` or `||`; no condition at all assigns the class to every footprint.
fn assignment_matches(a: &ClassAssignment, f: &ClassFacts) -> bool {
    let results: Vec<bool> = a.conditions.iter().filter_map(|c| condition_holds(c.kind, &c.primary, &c.secondary, f)).collect();
    if results.is_empty() {
        return true;
    }
    if a.all {
        results.iter().all(|r| *r)
    } else {
        results.iter().any(|r| *r)
    }
}

/// The classes the project's rules give a footprint (`GetDynamicComponentClassesForFootprint`), with a class for each sheet when the project asks
/// (`SyncDynamicComponentClassAssignments`: not for the root and not for a footprint on no sheet; quotes are taken out of the name).
fn dynamic_classes(settings: &ComponentClassSettings, f: &ClassFacts) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for a in &settings.assignments {
        if !a.class.is_empty() && assignment_matches(a, f) {
            out.insert(a.class.clone());
        }
    }
    if settings.sheet_classes && !f.sheet.is_empty() && f.sheet != "/" {
        let name = f.sheet.replace(['"', '\''], "");
        if member_of_sheet(f.sheet, &name) {
            out.insert(name);
        }
    }
    out
}

/// `FOOTPRINT::GetFPIDAsString`: the library link a part names. A footprint no library holds is written into a file under the library `eda` (`eda_kicad`'s writer), and read
/// back under it: the link of one and the same footprint is not told apart by that.
pub(crate) fn fpid_of(name: &str) -> String {
    name.strip_prefix("eda:").unwrap_or(name).to_string()
}

fn norm360(a: i64) -> i64 {
    a.rem_euclid(360_000)
}

/// The box of a point list.
fn grow(b: Box4, p: (Nm, Nm)) -> Box4 {
    (b.0.min(p.0), b.1.min(p.1), b.2.max(p.0), b.3.max(p.1))
}

/// The stroke width of the courtyard graphics a footprint of a board file carries (`(width ..)` of its `fp_*` lines), half of it: the box of a courtyard is the box of its
/// stroke. 0.05 mm, the width of KiCad's libraries, when the file says none.
fn courtyard_half_stroke(extra: Option<&FootprintExtra>) -> Nm {
    let widest = extra
        .into_iter()
        .flat_map(|e| e.courtyard_items.iter())
        .filter_map(|item| {
            let at = item.find("(width ")? + "(width ".len();
            let end = item[at..].find(')')?;
            item[at..at + end].trim().parse::<f64>().ok()
        })
        .fold(0.0_f64, f64::max);
    let width_nm = if widest > 0.0 { eda_model::units::from_mm(widest) } else { Nm::new(50_000) };
    width_nm / 2
}

impl Board<'_> {
    /// The sheet of each part: see the module doc.
    pub(crate) fn mc_part_sheets(&self) -> BTreeMap<String, PartSheet> {
        if self.design.schematic.is_some() {
            return sheets_of_schematic(&self.design);
        }
        // No schematic stored: the module sheets the studio shows (`infer_modules`, and `derive_hierarchy`'s names for their sheets and files). One module, or none, is
        // one flat sheet.
        let recorded: Vec<eda_model::ir::ModuleRegion> = self.design.placement.as_ref().map(|p| p.modules.clone()).unwrap_or_default();
        let modules = eda_model::modules::infer_modules(self.model, &recorded);
        let mut out = BTreeMap::new();
        if modules.len() < 2 {
            return out;
        }
        let mut taken: BTreeSet<String> = BTreeSet::new();
        for m in &modules {
            let file = eda_engine::hier::sheet_file_name(&m.name, &taken);
            taken.insert(file.clone());
            for r in &m.refs {
                out.insert(r.clone(), PartSheet { name: format!("/{}/", m.name), file: file.clone(), symbol: None });
            }
        }
        out
    }

    /// The footprints of the board, by reference.
    pub(crate) fn mc_board(&self) -> McBoard {
        let design = &self.design;
        let model: &ConstraintModel = self.model;
        let sheets = self.mc_part_sheets();
        let mut codes: BTreeMap<String, i64> = BTreeMap::new();
        let mut names: Vec<&str> = model.nets.iter().map(|n| n.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        for (i, n) in names.into_iter().enumerate() {
            codes.insert(n.to_string(), i as i64 + 1);
        }
        let extras: HashMap<&str, &FootprintExtra> = design.drawings.iter().flat_map(|d| d.footprint_extras.iter()).map(|e| (e.id.as_str(), e)).collect();
        let locked: BTreeSet<&str> = design.drawings.iter().flat_map(|d| d.locked_ids.iter().map(String::as_str)).collect();
        let settings = &model.board.component_classes;
        let mut fps: Vec<McFootprint> = Vec::new();
        let mut poses: Vec<&FootprintInstance> = self.placement().footprints.iter().collect();
        poses.sort_by(|a, b| a.id.cmp(&b.id));
        for pose in poses {
            let Some(part) = model.part(&pose.id) else { continue };
            let extra = extras.get(pose.id.as_str()).copied();
            let (sheetname, sheetfile, symbol) = match (extra.and_then(|e| e.sheetname.clone()), sheets.get(&pose.id)) {
                (Some(name), _) => (name, extra.and_then(|e| e.sheetfile.clone()).unwrap_or_default(), None),
                (None, Some(s)) => (s.name.clone(), s.file.clone(), s.symbol.clone()),
                (None, None) => (String::new(), String::new(), None),
            };
            let footprint = model.footprint_of(part);
            let net_of = pad_nets(model, &pose.id);
            let mut pads: Vec<(String, String)> = footprint.iter().flat_map(|f| f.pads.iter()).map(|p| (p.number.clone(), net_of.get(p.number.as_str()).cloned().unwrap_or_default())).collect();
            // the pads of a footprint that sit on no copper layer (a mounting hole): pins without a net
            if let Some(e) = extra {
                pads.extend(e.holes.iter().map(|h| (h.pad.number.clone(), String::new())));
            }
            // the link the board file gives the footprint (`footprint_lib_id`: a package's installed KiCad footprint), which a design block saved from it has
            let fpid = fpid_of(&eda_model::footprint::footprint_lib_id(part));
            let mut fields: Vec<(String, String)> = vec![("Reference".into(), pose.id.clone()), ("Value".into(), part.value.clone().unwrap_or_default()), ("Footprint".into(), fpid.clone())];
            if let Some(d) = &part.datasheet {
                fields.push(("Datasheet".into(), d.clone()));
            }
            if let Some(edit) = design.footprint_edit(&pose.id) {
                fields.extend(edit.fields.iter().map(|u| (u.name.clone(), u.text.clone())));
            }
            let orientation = norm360(-(pose.rot as i64)) as f64 / 1000.0;
            let facts = ClassFacts { reference: &pose.id, fpid: &fpid, front: pose.side == Side::Top, orientation, fields: &fields, sheet: &sheetname };
            let mut classes: BTreeSet<String> = extra.map(|e| e.component_classes.iter().cloned().collect()).unwrap_or_default();
            classes.extend(dynamic_classes(settings, &facts));
            let dr = design.drawings.as_ref();
            let group = dr.and_then(|d| d.parent_group(&pose.id)).map(|g| g.id.clone());
            let mut groups_up: Vec<String> = Vec::new();
            if let Some(d) = dr {
                let mut at = d.parent_group(&pose.id);
                let mut hops = 0;
                while let Some(g) = at {
                    groups_up.push(g.name.clone());
                    hops += 1;
                    at = if hops > d.groups.len() { None } else { d.parent_group(&g.id) };
                }
            }
            let bbox = self.mc_footprint_box(pose, part, footprint.as_ref(), extra);
            let edit = design.footprint_edit(&pose.id);
            let reference_layout = footprint.as_ref().map(|fp| fp_edit::effective_reference(design, pose, fp));
            let value_layout = Some(fp_edit::effective_value(design, pose));
            let user_fields = edit.map(|e| e.fields.clone()).unwrap_or_default();
            let models3d = edit.and_then(|e| e.models3d.clone()).unwrap_or_else(|| footprint.as_ref().map(|f| f.models3d.clone()).unwrap_or_default());
            fps.push(McFootprint {
                reference: pose.id.clone(),
                fpid,
                pose: pose.clone(),
                pads,
                sheetname,
                sheetfile,
                classes,
                group,
                groups_up,
                locked: locked.contains(pose.id.as_str()),
                symbol,
                bbox,
                reference_layout,
                value_layout,
                user_fields,
                models3d,
            });
        }
        McBoard { fps, codes }
    }

    /// `FOOTPRINT::GetBoundingBox( false )`: see the module doc.
    fn mc_footprint_box(&self, pose: &FootprintInstance, part: &Part, footprint: Option<&Footprint>, extra: Option<&FootprintExtra>) -> Box4 {
        let r = Nm::new(250_000);
        let mut b: Box4 = (pose.at.x - r, pose.at.y - r, pose.at.x + r, pose.at.y + r);
        let Some(footprint) = footprint else { return b };
        if let Some(pads) = placed_pads(self.model, part, pose) {
            for p in pads {
                b = grow(b, (p.center.x - p.size.0 / 2, p.center.y - p.size.1 / 2));
                b = grow(b, (p.center.x + p.size.0 / 2, p.center.y + p.size.1 / 2));
            }
        }
        // the courtyard, with the stroke it is drawn with
        if !extra.is_some_and(|e| e.no_courtyard) {
            let half = courtyard_half_stroke(extra);
            let outlines: Vec<Vec<(Nm, Nm)>> = if !footprint.courtyard_outlines.is_empty() {
                footprint.courtyard_outlines.clone()
            } else if let Some((hw, hh)) = footprint.courtyard {
                vec![vec![(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]]
            } else {
                Vec::new()
            };
            for o in &outlines {
                for &(x, y) in o {
                    let p = to_board(pose, (x, y));
                    b = grow(b, (p.x - half, p.y - half));
                    b = grow(b, (p.x + half, p.y + half));
                }
            }
        }
        // its graphics (the art), but for the notes on the user layers
        for shape in &footprint.art.shapes {
            let placed = shape.placed(pose);
            if matches!(placed.shape.layer(), "Cmts.User" | "Dwgs.User" | "Eco1.User" | "Eco2.User") {
                continue;
            }
            if let Some((x0, y0, x1, y1)) = placed.bbox() {
                b = grow(b, (x0, y0));
                b = grow(b, (x1, y1));
            }
        }
        // the zones the footprint holds
        if let Some(rt) = &self.design.routing {
            for z in rt.zones.iter().filter(|z| z.parent_footprint.as_deref() == Some(pose.id.as_str())) {
                if let Some(zb) = eda_model::multichannel::polygon_box(&z.outline) {
                    b = grow(b, (zb.0, zb.1));
                    b = grow(b, (zb.2, zb.3));
                }
            }
        }
        b
    }
}

/// Pad number -> net name for the pads of `reference` the model puts on a net.
fn pad_nets(model: &ConstraintModel, reference: &str) -> HashMap<String, String> {
    let prefix = format!("{reference}.");
    let mut out = HashMap::new();
    for n in &model.nets {
        for pin in n.pins.iter().filter(|p| p.starts_with(&prefix)) {
            out.insert(pin[prefix.len()..].to_string(), n.name.clone());
        }
    }
    out
}

/// The sheet of each part of a stored schematic (`SCH_SHEET_LIST` and the per-placement references).
fn sheets_of_schematic(design: &Design) -> BTreeMap<String, PartSheet> {
    let mut out: BTreeMap<String, PartSheet> = BTreeMap::new();
    for entry in eda_model::sheet_list::walk(design) {
        let Some(screen) = eda_model::sheet_list::screen_of(design, &entry.path) else { continue };
        // PathHumanReadable: "/" and the name of every sheet down to this one, each with its "/".
        let mut name = "/".to_string();
        for depth in 1..=entry.path.len() {
            if let Some(p) = eda_model::sheet_list::placement_at(design, &entry.path[..depth]) {
                name.push_str(&p.name);
                name.push('/');
            }
        }
        for sym in &screen.symbols {
            let reference = eda_model::sheet_list::reference_under(screen, sym, &entry.path);
            out.entry(reference).or_insert_with(|| PartSheet { name: name.clone(), file: entry.file.clone(), symbol: Some(format!("{}|{},{}", entry.file, sym.at.x, sym.at.y)) });
        }
    }
    out
}

impl McBoard {
    pub(crate) fn index_of(&self, reference: &str) -> Option<usize> {
        self.fps.iter().position(|f| f.reference == reference)
    }
}

/// A point rotated by `angle` (clockwise millidegrees) about the origin, as `Xform::Rotate` does.
pub(crate) fn rotate_about_origin(p: Point, angle: i64) -> Point {
    super::rotate_point_about(p, Point { x: Nm::ZERO, y: Nm::ZERO }, angle)
}

/// A footprint of a design block's board file as the tool reads it (`dbRA.m_components`): its pose, its pads and their nets in the block's own names.
pub(crate) fn block_footprint(f: &eda_kicad::ClipFootprint) -> McFootprint {
    let pose = FootprintInstance { id: f.reference.clone(), at: f.at, rot: f.rot, side: f.side, label: f.label };
    let nets: HashMap<&str, &str> = f.pad_nets.iter().map(|(n, net)| (n.as_str(), net.as_str())).collect();
    let pads: Vec<(String, String)> = f.definition.iter().flat_map(|d| d.pads.iter()).map(|p| (p.number.clone(), nets.get(p.number.as_str()).map(|n| n.to_string()).unwrap_or_default())).collect();
    let reference_layout = f.definition.as_ref().map(|d| f.edit.as_ref().and_then(|e| e.reference.clone()).unwrap_or_else(|| fp_edit::default_reference_layout(&pose, d)));
    let value_layout = Some(f.edit.as_ref().and_then(|e| e.value.clone()).unwrap_or_else(|| fp_edit::default_value_layout(&pose)));
    let models3d = f.edit.as_ref().and_then(|e| e.models3d.clone()).unwrap_or_else(|| f.definition.as_ref().map(|d| d.models3d.clone()).unwrap_or_default());
    McFootprint {
        reference: f.reference.clone(),
        fpid: fpid_of(&f.name),
        pose,
        pads,
        sheetname: String::new(),
        sheetfile: String::new(),
        classes: BTreeSet::new(),
        group: None,
        groups_up: Vec::new(),
        locked: false,
        symbol: None,
        bbox: (f.at.x - Nm::new(250_000), f.at.y - Nm::new(250_000), f.at.x + Nm::new(250_000), f.at.y + Nm::new(250_000)),
        reference_layout,
        value_layout,
        user_fields: f.edit.as_ref().map(|e| e.fields.clone()).unwrap_or_default(),
        models3d,
    }
}

/// The items of a design block's board file (`findRoutingInRuleArea` and `findOtherItemsInRuleArea` for `PLACEMENT_SOURCE_T::DESIGN_BLOCK`): all of them, whatever is where.
pub(crate) fn block_items(clip: &eda_kicad::Clipboard) -> super::multichannel_copy::RefItems {
    use super::multichannel_copy::{Item, RefItems};
    let mut items: Vec<Item> = Vec::new();
    items.extend(clip.tracks.iter().cloned().map(Item::Track));
    items.extend(clip.vias.iter().cloned().map(Item::Via));
    items.extend(clip.zones.iter().filter(|z| !z.teardrop).cloned().map(Item::Zone));
    for (i, s) in clip.shapes.iter().enumerate() {
        items.push(Item::Shape(s.clone(), clip.shape_style.get(i).cloned().flatten()));
    }
    items.extend(clip.texts.iter().cloned().map(|t| Item::Text(t, None)));
    items.extend(clip.dimensions.iter().cloned().map(Item::Dimension));
    RefItems { items, ..Default::default() }
}
