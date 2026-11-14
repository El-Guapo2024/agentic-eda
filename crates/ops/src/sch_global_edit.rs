//! Edit Text & Graphics Properties on the schematic: `DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS::TransferDataFromWindow`, `visitItem` and `processItem`
//! (eeschema/dialogs/dialog_global_edit_text_and_graphics.cpp at 8303b2ad; the sheet borders' selection test and the net name of KiCad 25ec2335) over
//! every sheet of the hierarchy, as one verb and one undo step ([`crate::sch_edit::SchCmd::GlobalEditTextGraphics`]).
//!
//! `TransferDataFromWindow` walks `Schematic().Hierarchy()` -- every sheet path, a screen shown twice visited once per path -- and `visitItem`s every item of
//! each path's screen: the symbols' fields (Reference designators, Values, Other symbol fields with the field name filter), the sheets' (Sheet titles, Other
//! sheet fields), their pins and borders, the wires, buses and their labels and junctions, the global and hierarchical labels and the labels' own fields, and
//! the schematic's texts, text boxes, shapes and graphic lines (Schematic text & graphics). The filters (by field name, by the parent's reference designator on
//! that path, by its library id, by its type -- power or not --, by net and the selection) decide which; `processItem` sets what the "Set To" box sets: the
//! font, the text colour, size, bold and italic of every text, a label's orientation, a field's alignment, visibility and shown name, the stroke's width,
//! colour and style, a shape's fill colour, a junction's size and colour. Every edit lands on the screen's item, so a screen shown on two paths is edited
//! once (its items are KiCad's same objects on both paths).
//!
//! A directive label takes the size, bold, italic, font and colour; the net filter names an item on a screen shown twice per sheet path (`ScreenFacts`).
//!
//! **Differs** (each said where it is done): a label's justification is its spin, so H and V
//! Align turn a label's spin as its `SetHorizJustify` does (`GetSpinStyle`), but a centred label keeps its side and V Align leaves a label as it is
//! ([`spin_of_justify`]); a global label's Sheet References field keeps its visibility (it follows Schematic Setup); the user fields of a power symbol keep
//! their looks (they are not placed: `PowerProps`); a field of a symbol on a sheet read from a KiCad file has no place to keep its
//! size, alignment or visibility in, and takes its font and colour only. Nothing changed is refused (KiCad pushes a commit of unchanged items).

use crate::{Board, CheckResult};
use eda_engine::fields_edit::OwnerCtx;
use eda_model::ir::{field_key, FieldPlacement, LabelKind, Point, SchematicSection, TextJustify, TextVAlign};
use eda_model::sch_extras::{FieldFontLook, JunctionLook, LabelLook, LabelSpin, SchColor, SchFill, SchGraphicKind, SchHAlign, SchLineStyle, SchStroke, SchTextJustify, SchVAlign, TextAttrs};
use eda_model::units::Nm;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// "By parent symbol type:" (`m_typeFilter`): "Non-power symbols", "Power symbols".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolTypeFilter {
    NonPower,
    Power,
}

/// The dialog's three boxes as the verb takes them. A filter is `None` when its box is unticked (or its text empty: `!m_netFilter->GetValue().IsEmpty()`);
/// a "Set To" value is `None` for "-- leave unchanged --" (`INDETERMINATE_ACTION`, an undetermined three-state box); a colour is set only with its box ticked
/// (`m_setTextColor`, `m_setColor`, `m_setFillColor`, `m_setDotColor`), `None` then being `COLOR4D::UNSPECIFIED`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalEditSpec {
    // ---- Scope (`sbScope`)
    /// "Reference designators" (`m_references`).
    pub references: bool,
    /// "Values" (`m_values`).
    pub values: bool,
    /// "Other symbol fields" (`m_otherFields`).
    pub other_fields: bool,
    /// "Wires && wire labels" (`m_wires`): wires, the labels on a wire or a pin and the junctions on a wire.
    pub wires: bool,
    /// "Buses && bus labels" (`m_buses`).
    pub buses: bool,
    /// "Global labels" (`m_globalLabels`).
    pub global_labels: bool,
    /// "Hierarchical labels" (`m_hierLabels`).
    pub hier_labels: bool,
    /// "Label fields" (`m_labelFields`).
    pub label_fields: bool,
    /// "Sheet titles" (`m_sheetTitles`).
    pub sheet_titles: bool,
    /// "Other sheet fields" (`m_sheetFields`).
    pub sheet_fields: bool,
    /// "Sheet pins" (`m_sheetPins`).
    pub sheet_pins: bool,
    /// "Sheet borders && backgrounds" (`m_sheetBorders`).
    pub sheet_borders: bool,
    /// "Schematic text && graphics" (`m_schTextAndGraphics`): free texts, text boxes, shapes and graphic lines (not rule areas or images: `visitItem` has no case
    /// for `SCH_RULE_AREA_T` or `SCH_BITMAP_T`).
    pub sch_text_graphics: bool,

    // ---- Filter Items (`sbFilters`)
    /// "By field name:" (`m_fieldnameFilter`, wildcards).
    pub field_name_filter: Option<String>,
    /// "By parent reference designator:" (`m_referenceFilter`): a symbol's reference on the sheet path visited (`GetRef( &aSheetPath )`).
    pub reference_filter: Option<String>,
    /// "By parent symbol library id:" (`m_symbolFilter`).
    pub symbol_filter: Option<String>,
    /// "By parent symbol type:".
    pub type_filter: Option<SymbolTypeFilter>,
    /// "By net:" (`m_netFilter`): the item's connection's name; an item with no connection (a symbol, a sheet, a text, a shape) is not visited at all.
    pub net_filter: Option<String>,
    /// "Selected items only" (`m_selectedFilterOpt`), with the ids selected on the sheet in view.
    pub selected_only: bool,
    pub selection: Vec<String>,

    // ---- Set To (`sbAction`)
    /// "Font:": "Default Font", "KiCad Font" or a family.
    pub font: Option<String>,
    pub set_text_color: bool,
    pub text_color: Option<SchColor>,
    pub text_size_nm: Option<Nm>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    /// "Orientation:" (labels only): the spin `SetSpinStyle( (SPIN_STYLE::SPIN) m_orientation->GetSelection() )` gives, already cast.
    pub orientation: Option<LabelSpin>,
    /// "H Align:" / "V Align:" (fields only), as the choice reads (`MapHorizJustify( GetSelection() - 1 )`).
    pub h_align: Option<SchHAlign>,
    pub v_align: Option<SchVAlign>,
    /// "Visible" / "Show field name" (fields only).
    pub visible: Option<bool>,
    pub show_field_names: Option<bool>,
    pub line_width_nm: Option<Nm>,
    pub set_line_color: bool,
    pub line_color: Option<SchColor>,
    pub line_style: Option<SchLineStyle>,
    pub set_fill_color: bool,
    pub fill_color: Option<SchColor>,
    pub junction_size_nm: Option<Nm>,
    pub set_junction_color: bool,
    pub junction_color: Option<SchColor>,
}

/// `m_textSize.Validate( 1.0, 10000.0, EDA_UNITS::MILS )`: "1 mil .. 10 inches".
pub const TEXT_SIZE_MIN_NM: Nm = Nm::new(25_400);
pub const TEXT_SIZE_MAX_NM: Nm = Nm::new(254_000_000);

/// One item of a screen `processItem` reaches.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    /// A field of a symbol or a power symbol (`key`: its `field_key`, a power symbol's id), with whether the parent's transform flips the alignment
    /// (`GetTransform().x1 < 0`, `.y1 < 0`).
    SymbolField { key: String, name: String, flip_h: bool, flip_v: bool },
    /// A sheet's Sheetname or Sheetfile.
    SheetField { sheet: String, name: String },
    /// A sheet's user field (`SchExtras::sheet_fields`).
    SheetUserField { sheet: String, name: String },
    SheetBorder(String),
    SheetPin(String),
    Label(String),
    /// A directive label (an `SchGraphic`).
    Directive(String),
    /// A label's own field (`SchExtras::label_fields`).
    LabelField { label: String, name: String },
    Wire(String),
    Junction(String),
    Text(String),
    /// A shape or a text box.
    Graphic(String),
    Line(String),
}

fn wild(pattern: &str, text: &str) -> bool {
    // `WildCompareString( pattern, text, false )`: the whole text, case folded
    crate::search::wild_match(&pattern.to_lowercase(), &text.to_lowercase())
}

fn non_empty(f: &Option<String>) -> Option<&str> {
    f.as_deref().filter(|s| !s.is_empty())
}

/// What one screen holds for `visitItem`, worked out once: the segments of its wires and buses, and where its pins are.
struct ScreenFacts {
    wires: Vec<(Point, Point, String)>,
    buses: Vec<(Point, Point, String)>,
    /// Each pin's point, with the net of `REF.PIN` on the path (`None` for one the model has no net for); a power symbol's pin with its net.
    pins: Vec<(Point, Option<String>)>,
    /// The stored net name of a wire -> its name on this path, for a screen shown more than once.
    per_path: BTreeMap<String, String>,
}

/// The sheet path names of one placement and of the other placements of its screen (`SCH_SHEET_PATH::PathHumanReadable`: "/", "/A/", "/A/B/").
#[derive(Debug, Default, Clone)]
struct PathPrefixes {
    here: String,
    others: Vec<String>,
}

impl PathPrefixes {
    fn shared(&self) -> bool {
        !self.others.is_empty()
    }

    /// A local net named under another placement's path, named under this one (`CONNECTION_GRAPH` names a local net by the path it is on).
    fn on_this_path(&self, net: &str) -> String {
        if net.starts_with(&self.here) {
            return net.to_string();
        }
        let mut others: Vec<&String> = self.others.iter().collect();
        others.sort_by_key(|p| std::cmp::Reverse(p.len()));
        match others.into_iter().find(|p| net.starts_with(p.as_str())) {
            Some(p) => format!("{}{}", self.here, &net[p.len()..]),
            None => net.to_string(),
        }
    }
}

impl ScreenFacts {
    /// `path`'s facts. `prefixes`: the sheet path names (`SCH_SHEET_PATH::PathHumanReadable`, "/A/B/") of `path` and of the other placements of its screen.
    fn new(sch: &SchematicSection, model: &eda_model::ConstraintModel, path: &[String], prefixes: &PathPrefixes) -> ScreenFacts {
        let pin_net: BTreeMap<&str, &str> = model.nets.iter().flat_map(|n| n.pins.iter().map(move |p| (p.as_str(), n.name.as_str()))).collect();
        let mut pins: Vec<(Point, Option<String>)> = Vec::new();
        // "REF.PIN" as the screen names it -> as this placement names it (`GetRef( &aSheetPath )`)
        let mut pin_on_path: BTreeMap<String, String> = BTreeMap::new();
        for sym in &sch.symbols {
            let Some(part) = model.part(&sym.id) else { continue };
            let resolved = model.real_symbol_of_instance(sch, sym, part);
            let reference = eda_model::sheet_list::reference_under(sch, sym, path);
            for (number, at) in eda_engine::placed::pin_points_in(sch, sym, part, resolved.as_ref()) {
                pins.push((at, pin_net.get(format!("{reference}.{number}").as_str()).map(|s| s.to_string())));
                pin_on_path.insert(format!("{}.{number}", sym.id), format!("{reference}.{number}"));
            }
        }
        pins.extend(sch.power_symbols.iter().map(|p| (p.at, Some(p.net.clone()))));
        // `SCH_CONNECTION::Name()` on this path (`aItem->Connection( &aSheetPath )`): the model keeps one name per wire, the last placement traced, so a screen
        // shown twice names each wire again here -- the net of a pin of its net on this placement (its wires' pins, or a pin on one of them), else the stored
        // name with this placement's sheet path in place of another placement's (a local net, "/A/NET" -> "/B/NET")
        let mut per_path: BTreeMap<String, String> = BTreeMap::new();
        if prefixes.shared() {
            for w in sch.wires.iter().filter(|w| !w.net.is_empty()) {
                if per_path.contains_key(&w.net) {
                    continue;
                }
                let group: Vec<&eda_model::ir::Wire> = sch.wires.iter().filter(|o| o.net == w.net && o.bus == w.bus).collect();
                let by_pin = group
                    .iter()
                    .flat_map(|o| o.pins.iter())
                    .find_map(|p| pin_net.get(pin_on_path.get(p).map_or(p.as_str(), String::as_str)).map(|n| n.to_string()))
                    .or_else(|| pins.iter().find(|(at, n)| n.is_some() && group.iter().any(|o| o.pts.windows(2).any(|s| crate::sch_scene::on_segment(s[0], s[1], *at)))).and_then(|(_, n)| n.clone()));
                per_path.insert(w.net.clone(), by_pin.unwrap_or_else(|| prefixes.on_this_path(&w.net)));
            }
        }
        let segs = |bus: bool| -> Vec<(Point, Point, String)> {
            sch.wires.iter().filter(|w| w.bus == bus).flat_map(|w| { let net = per_path.get(&w.net).cloned().unwrap_or_else(|| w.net.clone()); w.pts.windows(2).map(move |s| (s[0], s[1], net.clone())).collect::<Vec<_>>() }).collect()
        };
        ScreenFacts { wires: segs(false), buses: segs(true), pins, per_path }
    }

    /// The name of wire `w`'s connection on this path.
    fn wire_net(&self, w: &eda_model::ir::Wire) -> String {
        self.per_path.get(&w.net).cloned().unwrap_or_else(|| w.net.clone())
    }

    fn on(segs: &[(Point, Point, String)], at: Point) -> Option<&str> {
        segs.iter().find(|(a, b, _)| crate::sch_scene::on_segment(*a, *b, at)).map(|(_, _, n)| n.as_str())
    }

    /// `SCH_LABEL_LOCATE_WIRE_T`: a label connected to a wire or a pin.
    fn on_wire(&self, at: Point) -> bool {
        Self::on(&self.wires, at).is_some() || self.pins.iter().any(|(p, _)| *p == at)
    }

    /// `SCH_LABEL_LOCATE_BUS_T`.
    fn on_bus(&self, at: Point) -> bool {
        Self::on(&self.buses, at).is_some()
    }

    /// The name of the connection of whatever sits at `at` (`GetConnectionName( &aSheetPath )`): the wire's or bus's under it, else the pin's.
    fn net_at(&self, at: Point) -> Option<String> {
        Self::on(&self.wires, at).or_else(|| Self::on(&self.buses, at)).map(str::to_string).filter(|n| !n.is_empty()).or_else(|| self.pins.iter().find(|(p, _)| *p == at).and_then(|(_, n)| n.clone()))
    }
}

/// `visitItem` over one sheet path's screen: the items `processItem` reaches, put in `out`. `in_view`: the screen is the sheet in view's, whose items
/// the selection names.
fn visit(sch: &SchematicSection, model: &eda_model::ConstraintModel, path: &[String], prefixes: &PathPrefixes, spec: &GlobalEditSpec, in_view: bool, out: &mut BTreeSet<Target>) {
    let facts = ScreenFacts::new(sch, model, path, prefixes);
    let selected: BTreeSet<&str> = spec.selection.iter().map(String::as_str).collect();
    // `processItem`'s first test: the item or its parent selected (only items of the screen in view are)
    let take = |id: &str, parent: Option<&str>| !spec.selected_only || (in_view && (selected.contains(id) || parent.is_some_and(|p| selected.contains(p))));
    let net_ok = |net: Option<String>| match non_empty(&spec.net_filter) {
        None => true,
        Some(f) => net.is_some_and(|n| wild(f, &n)),
    };
    // a symbol, a sheet, a text or a shape has no connection: the net filter leaves it out
    let connected_only = non_empty(&spec.net_filter).is_some();
    let field_named = |name: &str| match (spec.field_name_filter.as_deref()).filter(|s| !s.is_empty()) {
        None => true,
        Some(f) => wild(f, name),
    };

    // ---- symbols (`SCH_SYMBOL_T`), and power symbols, which are symbols too
    if !connected_only {
        let sym_ok = |reference: &str, lib_id: &str, power: bool| {
            non_empty(&spec.reference_filter).is_none_or(|f| wild(f, reference))
                && non_empty(&spec.symbol_filter).is_none_or(|f| wild(f, lib_id))
                && spec.type_filter.is_none_or(|t| (t == SymbolTypeFilter::Power) == power)
        };
        for sym in &sch.symbols {
            let reference = eda_model::sheet_list::reference_under(sch, sym, path);
            let lib_id = if sym.lib_id.is_empty() { format!("eda:{}", sym.id) } else { sym.lib_id.clone() };
            if !sym_ok(&reference, &lib_id, false) {
                continue;
            }
            let key = field_key(&sym.id, sym.unit);
            let m = crate::sch_scene::Matrix::of(sym.rot, sym.mirrored, sym.mirror_y);
            let names: Vec<String> = match OwnerCtx::new(sch, model, &key) {
                Some(ctx) => ctx.specs.iter().map(|s| s.name.clone()).collect(),
                // a sheet read from a KiCad file: the four and the user fields, by name
                None => ["Reference", "Value", "Footprint", "Datasheet"].iter().map(|s| s.to_string()).chain(sch.user_fields.get(&sym.id).into_iter().flat_map(|u| u.keys().cloned())).collect(),
            };
            for name in names {
                let wanted = match name.as_str() {
                    "Reference" => spec.references,
                    "Value" => spec.values,
                    other => spec.other_fields && field_named(other),
                };
                if wanted && take(&eda_engine::fields_edit::field_id(&key, &name), Some(&sym.id)) {
                    out.insert(Target::SymbolField { key: key.clone(), name, flip_h: m.x1 < 0, flip_v: m.y1 < 0 });
                }
            }
        }
        for p in &sch.power_symbols {
            let reference = eda_model::sheet_list::power_reference_under(sch, p, path);
            if !sym_ok(&reference, &p.lib_id, true) {
                continue;
            }
            let m = crate::sch_scene::Matrix::of(p.rot, false, false);
            for (name, wanted) in [("Reference", spec.references), ("Value", spec.values)] {
                if wanted && take(&eda_engine::fields_edit::field_id(&p.id, name), Some(&p.id)) {
                    out.insert(Target::SymbolField { key: p.id.clone(), name: name.to_string(), flip_h: m.x1 < 0, flip_v: m.y1 < 0 });
                }
            }
        }

        // ---- sheets (`SCH_SHEET_T`)
        for s in &sch.sheets {
            if spec.sheet_titles && take(&eda_engine::fields_edit::field_id(&s.id, "Sheetname"), Some(&s.id)) {
                out.insert(Target::SheetField { sheet: s.id.clone(), name: "Sheetname".into() });
            }
            if spec.sheet_fields {
                if field_named("Sheetfile") && take(&eda_engine::fields_edit::field_id(&s.id, "Sheetfile"), Some(&s.id)) {
                    out.insert(Target::SheetField { sheet: s.id.clone(), name: "Sheetfile".into() });
                }
                for f in sch.extras.sheet_fields.get(&s.id).into_iter().flatten() {
                    if field_named(&f.name) && take(&eda_engine::fields_edit::field_id(&s.id, &f.name), Some(&s.id)) {
                        out.insert(Target::SheetUserField { sheet: s.id.clone(), name: f.name.clone() });
                    }
                }
            }
            // KiCad 25ec2335: `if( !m_selectedFilterOpt->GetValue() || sheet->IsSelected() )`
            if spec.sheet_borders && take(&s.id, None) {
                out.insert(Target::SheetBorder(s.id.clone()));
            }
            if spec.sheet_pins {
                for pin in &s.pins {
                    if take(&pin.id, Some(&s.id)) {
                        out.insert(Target::SheetPin(pin.id.clone()));
                    }
                }
            }
        }
    }

    // ---- wires and buses (`SCH_LINE_T` on LAYER_WIRE / LAYER_BUS)
    for w in &sch.wires {
        if (if w.bus { spec.buses } else { spec.wires }) && take(&w.id, None) && net_ok(Some(facts.wire_net(w)).filter(|n| !n.is_empty())) {
            out.insert(Target::Wire(w.id.clone()));
        }
    }

    // ---- labels (`SCH_LABEL_T`, `SCH_GLOBAL_LABEL_T`, `SCH_HIER_LABEL_T`)
    for l in &sch.labels {
        if !net_ok(facts.net_at(l.at).or_else(|| Some(facts.per_path.get(&l.net).cloned().unwrap_or_else(|| l.net.clone())))) {
            continue;
        }
        let kind_ticked = match l.kind {
            LabelKind::Global { .. } => spec.global_labels,
            LabelKind::Hierarchical { .. } => spec.hier_labels,
            LabelKind::Local => false,
        };
        let reached = (spec.wires && facts.on_wire(l.at)) || (spec.buses && facts.on_bus(l.at)) || kind_ticked;
        if reached && take(&l.id, None) {
            out.insert(Target::Label(l.id.clone()));
        }
        if spec.label_fields {
            for f in sch.extras.label_fields.get(&l.id).into_iter().flatten() {
                if field_named(&f.name) && take(&eda_engine::fields_edit::field_id(&l.id, &f.name), Some(&l.id)) {
                    out.insert(Target::LabelField { label: l.id.clone(), name: f.name.clone() });
                }
            }
        }
    }
    // ---- directive labels (`SCH_DIRECTIVE_LABEL_T`): on a wire or a bus
    for g in &sch.extras.graphics {
        if let SchGraphicKind::Directive { at, .. } = &g.shape {
            if net_ok(facts.net_at(*at)) && ((spec.wires && facts.on_wire(*at)) || (spec.buses && facts.on_bus(*at))) && take(&g.id, None) {
                out.insert(Target::Directive(g.id.clone()));
            }
        }
    }

    // ---- junctions (`SCH_JUNCTION_T`): the first item connected to it decides, a bus with Buses ticked, a wire with Wires
    for j in &sch.junctions {
        let on_bus = facts.on_bus(j.at);
        let on_wire = ScreenFacts::on(&facts.wires, j.at).is_some();
        if ((on_bus && spec.buses) || (on_wire && spec.wires)) && take(&j.id, None) && net_ok(facts.net_at(j.at)) {
            out.insert(Target::Junction(j.id.clone()));
        }
    }

    // ---- `SCH_TEXT_T`, `SCH_TEXTBOX_T`, `SCH_SHAPE_T` and `SCH_LINE_T` on LAYER_NOTES
    if spec.sch_text_graphics && !connected_only {
        for t in &sch.texts {
            if take(&t.id, None) {
                out.insert(Target::Text(t.id.clone()));
            }
        }
        for g in &sch.extras.graphics {
            let shape = matches!(g.shape, SchGraphicKind::Rectangle { .. } | SchGraphicKind::Circle { .. } | SchGraphicKind::Arc { .. } | SchGraphicKind::Bezier { .. } | SchGraphicKind::Polygon { .. } | SchGraphicKind::TextBox { .. });
            if shape && take(&g.id, None) {
                out.insert(Target::Graphic(g.id.clone()));
            }
        }
        for l in &sch.lines {
            if take(&l.id, None) {
                out.insert(Target::Line(l.id.clone()));
            }
        }
    }
}

/// `SCH_LABEL_BASE::GetSpinStyle` of a label whose `SetHorizJustify( h )` H Align set: `GR_TEXT_H_ALIGN_RIGHT` is LEFT (BOTTOM for a vertical label), anything
/// else RIGHT (UP). **Differs**: a centred label's text stays at its side of the anchor (the model keeps a label's justification as its spin only), and
/// V Align does not reach a label (its `SetVertJustify` has no place: a label is always bottom justified here, as `SetSpinStyle` leaves it).
fn spin_of_justify(h: SchHAlign, vertical: bool) -> LabelSpin {
    match (h, vertical) {
        (SchHAlign::Right, false) => LabelSpin::Left,
        (SchHAlign::Right, true) => LabelSpin::Bottom,
        (_, false) => LabelSpin::Right,
        (_, true) => LabelSpin::Up,
    }
}

/// `COLOR4D::UNSPECIFIED` (all zeroes) is no colour.
fn specified(c: Option<SchColor>) -> Option<SchColor> {
    c.filter(|c| *c != SchColor { r: 0, g: 0, b: 0, a: 0 })
}

fn h_of(a: SchHAlign) -> TextJustify {
    match a {
        SchHAlign::Left => TextJustify::Left,
        SchHAlign::Center => TextJustify::Center,
        SchHAlign::Right => TextJustify::Right,
    }
}

fn v_of(a: SchVAlign) -> TextVAlign {
    match a {
        SchVAlign::Top => TextVAlign::Top,
        SchVAlign::Center => TextVAlign::Center,
        SchVAlign::Bottom => TextVAlign::Bottom,
    }
}

/// A field's size as the model keeps it: 0 for the default 50 mil.
fn field_size(size: Nm) -> Nm {
    if size == eda_model::ir::DEFAULT_FIELD_SIZE_NM {
        Nm::ZERO
    } else {
        size
    }
}

/// `processItem` on a field's `EDA_TEXT` and `SCH_FIELD` parts that its placement keeps: size, alignment (flipped as the parent's transform asks: "if(
/// parentSymbol && parentSymbol->GetTransform().x1 < 0 )"), italic, bold, visibility and shown name.
fn edit_placement(p: &mut FieldPlacement, spec: &GlobalEditSpec, flip_h: bool, flip_v: bool) {
    if let Some(s) = spec.text_size_nm {
        p.size_nm = field_size(s);
    }
    if let Some(h) = spec.h_align {
        let h = h_of(h);
        p.h = match (flip_h, h) {
            (true, TextJustify::Left) => TextJustify::Right,
            (true, TextJustify::Right) => TextJustify::Left,
            _ => h,
        };
    }
    if let Some(v) = spec.v_align {
        let v = v_of(v);
        p.v = match (flip_v, v) {
            (true, TextVAlign::Top) => TextVAlign::Bottom,
            (true, TextVAlign::Bottom) => TextVAlign::Top,
            _ => v,
        };
    }
    if let Some(i) = spec.italic {
        p.italic = i;
    }
    if let Some(b) = spec.bold {
        p.bold = b;
    }
    if let Some(v) = spec.visible {
        p.visible = v;
    }
    if let Some(n) = spec.show_field_names {
        p.name_shown = n;
    }
}

/// The font and colour a field (or a label's field) keeps in `SchExtras::field_looks`.
fn edit_field_look(sch: &mut SchematicSection, key: &str, name: &str, spec: &GlobalEditSpec) -> bool {
    if spec.font.is_none() && !spec.set_text_color {
        return false;
    }
    let mut look: FieldFontLook = sch.extras.field_look(key, name).cloned().unwrap_or_default();
    if let Some(f) = &spec.font {
        look.font = crate::sch_symbol_props::font_of(f);
    }
    if spec.set_text_color {
        look.color = specified(spec.text_color);
    }
    sch.extras.set_field_look(key, name, look)
}

/// The text attributes (`SchExtras::text_attrs`) of a free text, a text box or a label: font and colour, and for a free text bold and italic.
fn edit_text_attrs(sch: &mut SchematicSection, id: &str, spec: &GlobalEditSpec, bold_italic: bool) -> bool {
    let before: TextAttrs = sch.extras.text_attrs.get(id).cloned().unwrap_or_default();
    let mut a = before.clone();
    if let Some(f) = &spec.font {
        a.font = crate::sch_symbol_props::font_of(f);
    }
    if spec.set_text_color {
        a.color = specified(spec.text_color);
    }
    if bold_italic {
        if let Some(i) = spec.italic {
            a.italic = i;
        }
        if let Some(b) = spec.bold {
            a.bold = b;
        }
    }
    if a == before {
        return false;
    }
    if a.is_default() {
        sch.extras.text_attrs.remove(id);
    } else {
        sch.extras.text_attrs.insert(id.to_string(), a);
    }
    true
}

/// `processItem`'s stroke (`HasLineStroke`): width, style and colour of a wire, a bus or a graphic line's `SchExtras::strokes` entry.
fn edit_stroke(sch: &mut SchematicSection, id: &str, spec: &GlobalEditSpec, width: bool) -> bool {
    let before: SchStroke = sch.extras.strokes.get(id).copied().unwrap_or_default();
    let mut s = before;
    if let (true, Some(w)) = (width, spec.line_width_nm) {
        s.width_nm = w.max(Nm::ZERO);
    }
    if let Some(st) = spec.line_style {
        s.style = st;
    }
    if spec.set_line_color {
        s.color = specified(spec.line_color);
    }
    if s == before {
        return false;
    }
    if s.is_default() {
        sch.extras.strokes.remove(id);
    } else {
        sch.extras.strokes.insert(id.to_string(), s);
    }
    true
}

/// Apply `processItem` to every target of one screen; returns whether anything changed and whether a connectable item did (its commit cleans up).
fn apply_screen(sch: &mut SchematicSection, model: &eda_model::ConstraintModel, targets: &BTreeSet<Target>, spec: &GlobalEditSpec) -> (bool, bool) {
    let mut changed = false;
    let mut connectable = false;

    // the fields with places: each owner's placements read once, edited, written once
    let mut by_owner: BTreeMap<String, Vec<(&str, bool, bool)>> = BTreeMap::new();
    for t in targets {
        match t {
            Target::SymbolField { key, name, flip_h, flip_v } => by_owner.entry(key.clone()).or_default().push((name, *flip_h, *flip_v)),
            Target::SheetField { sheet, name } => by_owner.entry(sheet.clone()).or_default().push((name, false, false)),
            _ => {}
        }
    }
    let places_set = spec.text_size_nm.is_some() || spec.h_align.is_some() || spec.v_align.is_some() || spec.italic.is_some() || spec.bold.is_some() || spec.visible.is_some() || spec.show_field_names.is_some();
    // one snapshot for every owner: an owner's placements are its own `field_layout` entry, which no other owner's edit touches
    let snapshot = (places_set && !by_owner.is_empty()).then(|| sch.clone());
    for (key, fields) in &by_owner {
        if let Some(snapshot) = &snapshot {
            if let Some(ctx) = OwnerCtx::new(snapshot, model, key) {
                let before = ctx.current(snapshot);
                let mut now = before.clone();
                for (name, flip_h, flip_v) in fields {
                    if let Some(i) = ctx.specs.iter().position(|s| s.name == *name) {
                        edit_placement(&mut now[i], spec, *flip_h, *flip_v);
                    }
                }
                if now != before {
                    sch.field_layout.insert(key.clone(), now);
                    changed = true;
                }
            }
        }
        for (name, ..) in fields {
            changed |= edit_field_look(sch, key, name, spec);
        }
    }

    for t in targets {
        match t {
            Target::SymbolField { .. } | Target::SheetField { .. } => {}
            Target::SheetUserField { sheet, name } => {
                // a sheet's user field keeps its text, whether it is shown and italic (r2 B-L1: no place, size or look of its own)
                if let Some(f) = sch.extras.sheet_fields.get_mut(sheet).and_then(|v| v.iter_mut().find(|f| f.name == *name)) {
                    let before = f.clone();
                    if let Some(v) = spec.visible {
                        f.visible = v;
                    }
                    if let Some(i) = spec.italic {
                        f.italic = i;
                    }
                    changed |= *f != before;
                }
            }
            Target::SheetBorder(id) => {
                let before = sch.extras.sheet_props_of(id);
                let mut p = before.clone();
                if let Some(w) = spec.line_width_nm {
                    p.border_width_nm = w.max(Nm::ZERO);
                }
                if spec.set_line_color {
                    p.border_color = specified(spec.line_color);
                }
                if spec.set_fill_color {
                    p.fill_color = specified(spec.fill_color);
                }
                if p != before {
                    changed |= sch.extras.set_sheet_props(id, p);
                }
            }
            Target::SheetPin(id) => {
                // its `EDA_TEXT`'s size, bold and italic (`SchExtras::sheet_pin_looks`), and its font and colour (`SchExtras::text_attrs`, UI-LEFT-2 SS15)
                if edit_text_attrs(sch, id, spec, false) {
                    changed = true;
                    connectable = true;
                }
                let before: LabelLook = sch.extras.sheet_pin_looks.get(id).copied().unwrap_or_default();
                let mut l = before;
                if let Some(s) = spec.text_size_nm {
                    l.size_nm = field_size(s);
                }
                if let Some(i) = spec.italic {
                    l.italic = i;
                }
                if let Some(b) = spec.bold {
                    l.bold = b;
                }
                if l != before {
                    changed = true;
                    connectable = true;
                    if l.is_default() {
                        sch.extras.sheet_pin_looks.remove(id);
                    } else {
                        sch.extras.sheet_pin_looks.insert(id.clone(), l);
                    }
                }
            }
            Target::Label(id) => {
                let before: LabelLook = sch.extras.label_looks.get(id).copied().unwrap_or_default();
                let mut l = before;
                if let Some(s) = spec.text_size_nm {
                    l.size_nm = field_size(s);
                }
                if let Some(i) = spec.italic {
                    l.italic = i;
                }
                if let Some(b) = spec.bold {
                    l.bold = b;
                }
                let mut here = false;
                if l != before {
                    here = true;
                    if l.is_default() {
                        sch.extras.label_looks.remove(id);
                    } else {
                        sch.extras.label_looks.insert(id.clone(), l);
                    }
                }
                here |= edit_text_attrs(sch, id, spec, false);
                // `SCH_LABEL_BASE::SetSpinStyle`: a spin the label already shows (its own, or the one its wire gives it) changes nothing. Before it (`processItem`
                // sets the `EDA_TEXT` first), H Align is the label's `SetHorizJustify`, which is its spin as `GetSpinStyle` reads it back: Right justified is
                // LEFT (BOTTOM when vertical), Left or Center RIGHT (UP); the Orientation, set after, wins
                let shows = sch.labels.iter().find(|x| x.id == *id).map(|label| eda_engine::label_fields::label_spin(sch, label));
                let wanted = spec.orientation.or_else(|| spec.h_align.zip(shows).map(|(h, s)| spin_of_justify(h, matches!(s, LabelSpin::Up | LabelSpin::Bottom))));
                if let Some(spin) = wanted {
                    if shows.is_some_and(|s| s != spin) {
                        sch.extras.label_spins.insert(id.clone(), spin);
                        here = true;
                    }
                }
                if here {
                    changed = true;
                    connectable = true;
                }
            }
            Target::Directive(id) => {
                // a directive label is an `EDA_TEXT` too: its size (`SchGraphicKind::Directive::text_size_nm`, 50 mil kept as none), bold and italic (its
                // `text_attrs`, which the Properties panel masks as `SCH_DIRECTIVE_LABEL_DESC` does but `processItem` sets), font and colour
                let mut here = false;
                if let Some(size) = spec.text_size_nm {
                    if let Some(g) = sch.extras.graphics.iter_mut().find(|g| g.id == *id) {
                        if let SchGraphicKind::Directive { text_size_nm, .. } = &mut g.shape {
                            let size = field_size(size);
                            if *text_size_nm != size {
                                *text_size_nm = size;
                                here = true;
                            }
                        }
                    }
                }
                here |= edit_text_attrs(sch, id, spec, true);
                if here {
                    changed = true;
                    connectable = true;
                }
            }
            Target::LabelField { label, name } => {
                if let Some(f) = sch.extras.label_fields.get_mut(label).and_then(|v| v.iter_mut().find(|f| f.name == *name)) {
                    let before = f.clone();
                    if let Some(v) = spec.visible {
                        f.visible = v;
                    }
                    if let Some(i) = spec.italic {
                        f.italic = i;
                    }
                    // `processItem`: `SetTextSize`, `SetBold` on the `EDA_TEXT`, `SetNameShown` on the `SCH_FIELD` (the field's own since UI-LEFT-2 SS12)
                    if let Some(size) = spec.text_size_nm {
                        f.size_nm = if size == eda_model::ir::DEFAULT_FIELD_SIZE_NM { Nm::ZERO } else { size };
                    }
                    if let Some(b) = spec.bold {
                        f.bold = b;
                    }
                    if let Some(n) = spec.show_field_names {
                        f.name_shown = n;
                    }
                    changed |= *f != before;
                }
                changed |= edit_field_look(sch, label, name, spec);
            }
            Target::Wire(id) => {
                if edit_stroke(sch, id, spec, true) {
                    changed = true;
                    connectable = true;
                }
            }
            Target::Junction(id) => {
                let before: JunctionLook = sch.extras.junction_looks.get(id).copied().unwrap_or_default();
                let mut j = before;
                if let Some(d) = spec.junction_size_nm {
                    j.diameter_nm = d.max(Nm::ZERO);
                }
                if spec.set_junction_color {
                    j.color = specified(spec.junction_color);
                }
                if j != before {
                    changed = true;
                    connectable = true;
                    if j.is_default() {
                        sch.extras.junction_looks.remove(id);
                    } else {
                        sch.extras.junction_looks.insert(id.clone(), j);
                    }
                }
            }
            Target::Text(id) => {
                if let Some(t) = sch.texts.iter_mut().find(|t| t.id == *id) {
                    if let Some(s) = spec.text_size_nm {
                        if t.size_nm != s {
                            t.size_nm = s;
                            changed = true;
                        }
                        // `SetTextSize( VECTOR2I( size, size ) )`: as wide as it is high
                        if sch.extras.text_widths.remove(id).is_some() {
                            changed = true;
                        }
                    }
                }
                if spec.h_align.is_some() || spec.v_align.is_some() {
                    let before = sch.extras.text_justify_of(id);
                    let now = SchTextJustify { h: spec.h_align.map(h_of).unwrap_or(before.h), v: spec.v_align.map(v_of).unwrap_or(before.v) };
                    changed |= sch.extras.set_text_justify(id, now);
                }
                changed |= edit_text_attrs(sch, id, spec, true);
            }
            Target::Graphic(id) => {
                let Some(g) = sch.extras.graphics.iter_mut().find(|g| g.id == *id) else { continue };
                let before = g.clone();
                if let SchGraphicKind::TextBox { size_nm, bold, italic, h_align, v_align, .. } = &mut g.shape {
                    if let Some(s) = spec.text_size_nm {
                        *size_nm = s;
                    }
                    if let Some(h) = spec.h_align {
                        *h_align = h;
                    }
                    if let Some(v) = spec.v_align {
                        *v_align = v;
                    }
                    if let Some(i) = spec.italic {
                        *italic = i;
                    }
                    if let Some(b) = spec.bold {
                        *bold = b;
                    }
                }
                if let Some(w) = spec.line_width_nm {
                    g.width_nm = w.max(Nm::ZERO);
                }
                if let Some(st) = spec.line_style {
                    g.line_style = st;
                }
                if spec.set_line_color {
                    g.color = specified(spec.line_color);
                }
                // `SetFillColor`, then `FILLED_WITH_COLOR`, or `NO_FILL` for an unspecified colour
                if spec.set_fill_color {
                    g.fill_color = specified(spec.fill_color);
                    g.fill = if g.fill_color.is_some() { SchFill::Color } else { SchFill::None };
                }
                let text_box = matches!(g.shape, SchGraphicKind::TextBox { .. });
                changed |= *g != before;
                if text_box {
                    changed |= edit_text_attrs(sch, id, spec, false);
                }
            }
            Target::Line(id) => {
                if let Some(l) = sch.lines.iter_mut().find(|l| l.id == *id) {
                    if let Some(w) = spec.line_width_nm {
                        let w = w.max(Nm::ZERO);
                        if l.width_nm != w {
                            l.width_nm = w;
                            changed = true;
                        }
                    }
                }
                changed |= edit_stroke(sch, id, spec, false);
            }
        }
    }
    (changed, connectable)
}

impl<'a> Board<'a> {
    /// [`crate::sch_edit::SchCmd::GlobalEditTextGraphics`]: `TransferDataFromWindow` over `Schematic().Hierarchy()`, one commit.
    pub(crate) fn global_edit_text_graphics(&mut self, spec: &GlobalEditSpec) -> Result<(), Vec<CheckResult>> {
        if let Some(size) = spec.text_size_nm {
            if size < TEXT_SIZE_MIN_NM {
                return Err(vec![CheckResult::fail("ops_bad_text_size", "text_size", "Text size must be at least 0.0254 mm.")]);
            }
            if size > TEXT_SIZE_MAX_NM {
                return Err(vec![CheckResult::fail("ops_bad_text_size", "text_size", "Text size must be less than 254 mm.")]);
            }
        }
        self.schematic()?;
        let model = self.model;
        // the screen in view, whose items the selection names
        let in_view = self.focus.clone().unwrap_or_default();
        // `for( const SCH_SHEET_PATH& sheetPath : m_parent->Schematic().Hierarchy() )`: every path visits its screen; a screen's items are edited once
        let mut targets: BTreeMap<String, BTreeSet<Target>> = BTreeMap::new();
        let entries = eda_model::sheet_list::walk(&self.design);
        // each placement's sheet path name ("/A/B/"), by the placement names from the root down
        let names: BTreeMap<&[String], &str> = entries.iter().map(|e| (e.path.as_slice(), e.name.as_str())).collect();
        let human = |path: &[String]| -> String { (1..=path.len()).map(|n| format!("{}/", names.get(&path[..n]).copied().unwrap_or(""))).fold("/".to_string(), |a, b| a + &b) };
        for entry in &entries {
            let Some(screen) = eda_model::sheet_list::screen_of(&self.design, &entry.path) else { continue };
            let prefixes = PathPrefixes { here: human(&entry.path), others: entries.iter().filter(|o| o.file == entry.file && o.path != entry.path).map(|o| human(&o.path)).collect() };
            visit(screen, model, &entry.path, &prefixes, spec, entry.file == in_view, targets.entry(entry.file.clone()).or_default());
        }
        let saved_focus = self.focus.clone();
        let mut changed = false;
        let mut result = Ok(());
        for (file, items) in &targets {
            if items.is_empty() {
                continue;
            }
            self.focus = (!file.is_empty()).then(|| file.clone());
            let sch = match self.schematic_mut() {
                Ok(s) => s,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            };
            let (here, connectable) = apply_screen(sch, model, items, spec);
            changed |= here;
            // `SCH_COMMIT::pushSchEdit`: a commit that holds a connectable item cleans its screen up (a stroke may make two wires one)
            if here && connectable {
                if let Err(e) = self.commit_clean_up() {
                    result = Err(e);
                    break;
                }
            }
        }
        self.focus = saved_focus;
        result?;
        if !changed {
            return Err(vec![CheckResult::fail("ops_nothing_changed", "edit_text_graphics", "nothing the dialog reaches changes")]);
        }
        Ok(())
    }
}
