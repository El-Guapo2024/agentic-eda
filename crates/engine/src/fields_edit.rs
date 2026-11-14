//! The fields of one placed item as something to edit: where each is now, how a page position becomes a stored placement, and what Autoplace
//! Fields (`SCH_SYMBOL::AutoplaceFields`, `SCH_SHEET::AutoplaceFields`) makes of them.
//!
//! `fields.rs` places the fields of a derivation and says where a stored placement lands on the sheet. This is the other direction, what the
//! schematic editor needs to move a field by a page offset (`SCH_MOVE_TOOL::moveItem`: the offset taken through the inverse of the symbol's
//! transform), to set a field from the Field Properties dialog (`DIALOG_FIELD_PROPERTIES::UpdateField`: `SCH_FIELD::SetPosition`) and to run
//! Autoplace Fields on one item, in its automatic or its manual mode.
//!
//! A field is named by an id the studio selects it by, `fld:<owner key>:<field name>`; the owner key is the key of
//! `SchematicSection::field_layout` (a symbol's reference, `#<unit>` after it for any unit but the first, a power symbol's or a sheet's id).

use eda_model::ir::{field_key, FieldPlacement, NetLabel, Point, PowerSymbol, SchematicSection, SheetInstance, SymbolInstance, TextJustify, TextVAlign};
use eda_model::kicad_font::{HJustify, VJustify};
use eda_model::sch_extras::AutoplaceAlgo;
use eda_model::ConstraintModel;
use eda_model::units::Nm;

use crate::fields::{self, Collider, FieldSpec, PageField, Surroundings};
use crate::hier::kit::{self, Rect};
use crate::symgeom::{unbake, SymbolGeom};

/// The prefix of the id of a field the studio selects.
pub const ID_PREFIX: &str = "fld:";

/// The id of the field `name` of the item whose key in `field_layout` is `owner_key`.
pub fn field_id(owner_key: &str, name: &str) -> String {
    format!("{ID_PREFIX}{owner_key}:{name}")
}

/// `(owner key, field name)` of a field id; `None` for any other id.
pub fn parse_field_id(id: &str) -> Option<(&str, &str)> {
    id.strip_prefix(ID_PREFIX)?.split_once(':')
}

/// What owns a set of fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerKind {
    Symbol,
    Power,
    Sheet,
    /// A label's own fields (`SCH_LABEL_BASE::m_fields`, `crate::label_fields`): the key is the label's id, the frame the sheet's own about its anchor.
    Label,
}

/// One item with fields, as the editor sees it: its frame on the sheet, its fields' texts, and what it needs to place them.
#[derive(Debug, Clone)]
pub struct OwnerCtx {
    pub kind: OwnerKind,
    /// The key in `field_layout` (and in `SchExtras::fields_autoplaced`).
    pub key: String,
    /// The item's origin on the sheet, its turn and mirror, and the width of the box a mirror is about (0 where the origin is the anchor).
    pub at: Point,
    pub rot: u32,
    pub mirrored: bool,
    pub width: f64,
    /// The fields in KiCad's order, with the texts they show and whether they are shown until a placement says otherwise.
    pub specs: Vec<FieldSpec>,
    sym: Option<SymbolInstance>,
    geom: Option<SymbolGeom>,
    power: Option<PowerSymbol>,
    sheet: Option<SheetInstance>,
    label: Option<NetLabel>,
}

impl OwnerCtx {
    /// The item with `key` on `sch`, or `None`: no such item, a symbol the model has no part for, or a sheet read from a KiCad file (whose symbols
    /// are placed by their own origin, which the fields here are not measured from).
    pub fn new(sch: &SchematicSection, model: &ConstraintModel, key: &str) -> Option<OwnerCtx> {
        // a label's fields are placed about its anchor on the sheet, whatever file the sheet came from
        if let Some(l) = sch.labels.iter().find(|l| l.id == key) {
            if !crate::label_fields::has_fields(sch, l) {
                return None;
            }
            let show = label_isr_shown(sch);
            let specs = crate::label_fields::placed(sch, l, show).into_iter().map(|f| FieldSpec { italic: f.italic, ..FieldSpec::new(&f.name, f.text, f.visible) }).collect();
            return Some(OwnerCtx { kind: OwnerKind::Label, key: key.to_string(), at: l.at, rot: 0, mirrored: false, width: 0.0, specs, sym: None, geom: None, power: None, sheet: None, label: Some(l.clone()) });
        }
        // a sheet's fields are placed from its corner on the sheet, whatever file the sheet came from (`SCH_FIELD` of an `SCH_SHEET`: the field's own position);
        // its user fields (`SchExtras::sheet_fields`) follow Sheetname and Sheetfile, as `SCH_SHEET::GetFields()` holds them (UI-LEFT-3 S3-6)
        if let Some(s) = sch.sheets.iter().find(|s| s.id == key) {
            let mut specs = vec![FieldSpec::new("Sheetname", s.name.clone(), true), FieldSpec::new("Sheetfile", s.file.clone(), true)];
            specs.extend(sch.extras.sheet_fields.get(&s.id).into_iter().flatten().map(|f| FieldSpec { size_nm: f.size_nm, bold: f.bold, italic: f.italic, name_shown: f.name_shown, ..FieldSpec::new(&f.name, f.text.clone(), f.visible) }));
            return Some(OwnerCtx { kind: OwnerKind::Sheet, key: key.to_string(), at: s.at, rot: 0, mirrored: false, width: 0.0, specs, sym: None, geom: None, power: None, sheet: Some(s.clone()), label: None });
        }
        if sch.imported_from_kicad {
            return None;
        }
        if let Some(s) = sch.symbols.iter().find(|s| field_key(&s.id, s.unit) == key) {
            let part = model.part(&s.id)?;
            let mut sym = s.clone();
            if sym.lib_id.is_empty() {
                sym.lib_id = format!("eda:{}", sym.id);
            }
            let resolved = model.real_symbol_of_instance(sch, &sym, part);
            let geom = SymbolGeom::of_in(sch, &sym, part, resolved.as_ref());
            let specs = fields::symbol_specs_in(sch, &sym, Some(part), resolved.as_ref().map(|r| r.datasheet.as_str()).unwrap_or(""));
            let (rot, mirrored) = crate::symgeom::kicad_turn(sym.rot, sym.mirrored, sym.mirror_y);
            return Some(OwnerCtx { kind: OwnerKind::Symbol, key: key.to_string(), at: sym.at, rot, mirrored, width: geom.width, specs, sym: Some(sym), geom: Some(geom), power: None, sheet: None, label: None });
        }
        if let Some(p) = sch.power_symbols.iter().find(|p| p.id == key) {
            let flag = p.lib_id == "power:PWR_FLAG";
            let mut specs = vec![FieldSpec::new("Reference", p.id.clone(), false), FieldSpec::new("Value", p.net.clone(), !flag)];
            // and its user-defined fields (`SchExtras::power_props`), items of the sheet like a symbol's (UI-LEFT-2 SS7)
            specs.extend(fields::power_user_specs(sch, p));
            return Some(OwnerCtx { kind: OwnerKind::Power, key: key.to_string(), at: p.at, rot: p.rot, mirrored: false, width: 0.0, specs, sym: None, geom: None, power: Some(p.clone()), sheet: None, label: None });
        }
        None
    }

    /// The symbol this is the fields of, with its library id filled in.
    pub fn symbol(&self) -> Option<&SymbolInstance> {
        self.sym.as_ref()
    }

    pub fn geom(&self) -> Option<&SymbolGeom> {
        self.geom.as_ref()
    }

    /// The label this is the fields of.
    pub fn label(&self) -> Option<&NetLabel> {
        self.label.as_ref()
    }

    /// The placements the fields have now, one per spec: what the section keeps, else what Autoplace Fields would give (live), else the item's default.
    pub fn current(&self, sch: &SchematicSection) -> Vec<FieldPlacement> {
        match self.kind {
            OwnerKind::Symbol => {
                let (sym, geom) = (self.sym.as_ref().expect("a symbol"), self.geom.as_ref().expect("its geometry"));
                fields::symbol_placements(sch, sym, geom, &self.specs)
            }
            OwnerKind::Power => fields::power_placements(sch, self.power.as_ref().expect("a power symbol")).into_iter().map(|(p, _)| p).collect(),
            OwnerKind::Sheet => {
                let auto = fields::sheet_placements(self.sheet.as_ref().expect("a sheet"));
                let stored = sch.field_layout.get(&self.key);
                let places = sch.extras.sheet_field_places.get(&self.key);
                self.specs
                    .iter()
                    .map(|sp| match auto.iter().find(|p| p.name == sp.name) {
                        Some(home) => stored.and_then(|s| s.iter().find(|p| p.name == sp.name)).unwrap_or(home).clone(),
                        // a user field: where Sheet Properties' grid or a move put it (`SchExtras::sheet_field_places`), styled as `sheet_fields` keeps it
                        None => user_field_placement(sp, places.and_then(|p| p.get(&sp.name)).copied().unwrap_or_default()),
                    })
                    .collect()
            }
            OwnerKind::Label => {
                let l = self.label.as_ref().expect("a label");
                let l = sch.labels.iter().find(|x| x.id == l.id).unwrap_or(l);
                crate::label_fields::placed(sch, l, label_isr_shown(sch)).iter().map(|f| crate::label_fields::placement_of(l, f)).collect()
            }
        }
    }

    /// The fields on the sheet, in the order of the specs.
    pub fn page_fields(&self, sch: &SchematicSection) -> Vec<PageField> {
        self.current(sch).iter().zip(self.specs.iter()).map(|(p, sp)| self.page(p, &sp.text)).collect()
    }

    /// `SCH_FIELD::GetPosition`, `GetTextAngle`, `GetEffectiveHorizJustify`: `p` as it reads on the sheet.
    pub fn page(&self, p: &FieldPlacement, text: &str) -> PageField {
        fields::page_field(self.at, self.rot, self.mirrored, self.width, p, text)
    }

    /// `SCH_FIELD::SetPosition`: where on the sheet `page` (floating-point nanometres) is, as a stored position (from the item's origin, in its
    /// unturned frame).
    pub fn local_pos(&self, page: (f64, f64)) -> (Nm, Nm) {
        let (x, y) = unbake(self.rot, self.mirrored, self.width, page.0 - self.at.x.as_f64(), page.1 - self.at.y.as_f64());
        (Nm::kiround(x), Nm::kiround(y))
    }

    /// A page offset as the offset of a field's stored position (`SCH_MOVE_TOOL::moveItem`: the offset taken through the inverse of the symbol's
    /// transform).
    pub fn local_delta(&self, dx: Nm, dy: Nm) -> (Nm, Nm) {
        let (x, y) = unbake(self.rot, self.mirrored, 0.0, dx.as_f64(), dy.as_f64());
        (Nm::kiround(x), Nm::kiround(y))
    }

    /// `SCH_FIELD::SetPosition` and the angle and justification that read as asked once the symbol's transform has turned them: the field `old` with
    /// its anchor at `anchor` on the sheet, running up the sheet when `vertical`, justified `h` and `v` in the text's own axes. What is not place
    /// (size, bold, italic, shown name, whether it may be autoplaced, whether it is shown) stays as `old` has it.
    pub fn placed_at(&self, old: &FieldPlacement, anchor: (f64, f64), vertical: bool, h: HJustify, v: VJustify) -> FieldPlacement {
        let hj = match h {
            HJustify::Left => TextJustify::Left,
            HJustify::Center => TextJustify::Center,
            HJustify::Right => TextJustify::Right,
        };
        let vj = match v {
            VJustify::Top => TextVAlign::Top,
            VJustify::Center => TextVAlign::Center,
            VJustify::Bottom => TextVAlign::Bottom,
        };
        let mut p = fields::local_placement_on_page(self.at, self.rot, self.mirrored, self.width, &old.name, anchor, vertical, hj, vj, old.visible);
        p.size_nm = old.size_nm;
        p.bold = old.bold;
        p.italic = old.italic;
        p.name_shown = old.name_shown;
        p.no_autoplace = old.no_autoplace;
        p
    }

    /// The fields placed again by Autoplace Fields, one placement per spec, in the order of the specs. `current` are the placements now (a field that
    /// does not allow autoplacement keeps its own, the others take the style they have). A symbol's fields keep clear of what is drawn around it in
    /// the manual mode.
    pub fn autoplace(&self, sch: &SchematicSection, model: &ConstraintModel, algo: AutoplaceAlgo) -> Vec<FieldPlacement> {
        let current = self.current(sch);
        match self.kind {
            OwnerKind::Symbol => {
                let (sym, geom) = (self.sym.as_ref().expect("a symbol"), self.geom.as_ref().expect("its geometry"));
                let specs = fields::specs_styled(self.specs.clone(), Some(&current));
                match algo {
                    AutoplaceAlgo::Auto => fields::autoplace_symbol(sym, geom, &specs),
                    AutoplaceAlgo::Manual => fields::autoplace_symbol_manual(sym, geom, &specs, &current, &surroundings(sch, model, &self.key)),
                }
            }
            OwnerKind::Power => {
                let ps = self.power.as_ref().expect("a power symbol");
                // the Value and the user fields stacked as `AUTOPLACER` stacks every shown field; with no user field, the Value alone
                let styled = fields::specs_styled(self.specs.clone(), Some(&current));
                let movable: Vec<FieldSpec> = styled.iter().filter(|s| s.name != "Reference").cloned().collect();
                let home: Vec<FieldPlacement> = if movable.len() <= 1 { vec![fields::power_value_placement(ps, &ps.lib_id)] } else { fields::power_autoplaced(ps, &movable) };
                let home_of = |name: &str| movable.iter().position(|s| s.name == name).and_then(|i| home.get(i)).cloned();
                self.specs
                    .iter()
                    .zip(current.iter())
                    .map(|(sp, now)| {
                        let mut p = if sp.name == "Reference" { FieldPlacement { visible: false, ..FieldPlacement::at_origin(&sp.name) } } else { home_of(&sp.name).unwrap_or_else(|| FieldPlacement::at_origin(&sp.name)) };
                        p.name = sp.name.clone();
                        p.visible = now.visible;
                        p.size_nm = now.size_nm;
                        p.bold = now.bold;
                        p.italic = now.italic;
                        p.name_shown = now.name_shown;
                        p.no_autoplace = now.no_autoplace;
                        if now.no_autoplace {
                            now.clone()
                        } else {
                            p
                        }
                    })
                    .collect()
            }
            // `SCH_LABEL_BASE::AutoplaceFields`: every field where it puts them (a label's fields have no `do_not_autoplace`)
            OwnerKind::Label => {
                let l = self.label.as_ref().expect("a label");
                let l = sch.labels.iter().find(|x| x.id == l.id).unwrap_or(l);
                crate::label_fields::autoplaced(sch, l, label_isr_shown(sch)).iter().map(|f| crate::label_fields::placement_of(l, f)).collect()
            }
            OwnerKind::Sheet => {
                let home = fields::sheet_placements(self.sheet.as_ref().expect("a sheet"));
                self.specs
                    .iter()
                    .zip(current.iter())
                    .map(|(sp, now)| {
                        // `SCH_SHEET::AutoplaceFields` places the Sheetname and the Sheetfile only: a user field stays where it is
                        let Some(home) = home.iter().find(|p| p.name == sp.name) else { return now.clone() };
                        if now.no_autoplace {
                            return now.clone();
                        }
                        let mut p = home.clone();
                        p.visible = now.visible;
                        p.size_nm = now.size_nm;
                        p.bold = now.bold;
                        p.italic = now.italic;
                        p.name_shown = now.name_shown;
                        p
                    })
                    .collect()
            }
        }
    }
}

/// A sheet user field's placement from its place (`SchExtras::sheet_field_places`: the offset from the sheet's corner, the angle, the justification) and its
/// style (`SchExtras::sheet_fields`: shown, size, bold, italic, shown name).
fn user_field_placement(sp: &FieldSpec, at: eda_model::sch_extras::SheetFieldPlace) -> FieldPlacement {
    FieldPlacement { dx: at.dx, dy: at.dy, angle: at.angle, h: at.h, v: at.v, visible: sp.visible, size_nm: sp.size_nm, bold: sp.bold, italic: sp.italic, name_shown: sp.name_shown, ..FieldPlacement::at_origin(&sp.name) }
}

/// Keep `placements` (one per field of the item `key`, as [`OwnerCtx::current`] gives them) on `sch`: in `SchematicSection::field_layout`, except a sheet's user
/// fields, whose place is `SchExtras::sheet_field_places` and whose style (shown, size, bold, italic, shown name) is their `SchExtras::sheet_fields` entry --
/// one home for each, as Sheet Properties' grid reads and writes them (UI-LEFT-3 S3-6).
pub fn keep_placements(sch: &mut SchematicSection, key: &str, placements: Vec<FieldPlacement>) {
    if !sch.sheets.iter().any(|s| s.id == key) || !sch.extras.sheet_fields.contains_key(key) {
        sch.field_layout.insert(key.to_string(), placements);
        return;
    }
    let (own, user): (Vec<FieldPlacement>, Vec<FieldPlacement>) = placements.into_iter().partition(|p| p.name == "Sheetname" || p.name == "Sheetfile");
    sch.field_layout.insert(key.to_string(), own);
    if let Some(fields) = sch.extras.sheet_fields.get_mut(key) {
        for p in &user {
            if let Some(f) = fields.iter_mut().find(|f| f.name == p.name) {
                f.visible = p.visible;
                f.size_nm = p.size_nm;
                f.bold = p.bold;
                f.italic = p.italic;
                f.name_shown = p.name_shown;
            }
        }
    }
    let places = sch.extras.sheet_field_places.entry(key.to_string()).or_default();
    for p in user {
        let place = eda_model::sch_extras::SheetFieldPlace { dx: p.dx, dy: p.dy, angle: p.angle, h: p.h, v: p.v };
        if place.is_default() {
            places.remove(&p.name);
        } else {
            places.insert(p.name, place);
        }
    }
    if places.is_empty() {
        sch.extras.sheet_field_places.remove(key);
    }
}

/// Whether a global label's Sheet References are shown (Schematic Setup > Formatting, kept on the root sheet: a sheet's own section says so only on the root).
fn label_isr_shown(sch: &SchematicSection) -> bool {
    sch.extras.intersheet_refs.as_ref().is_some_and(|r| r.show)
}

/// `AUTOPLACER::getDrawableArea`: the paper inside the drawing sheet's 10 mm margins.
fn drawable_area(sch: &SchematicSection) -> Option<Rect> {
    let paper = sch.title_block.as_ref().map(|t| t.paper.as_str()).unwrap_or("");
    let (w, h) = eda_model::page::PageSettings::of_sheet(sch.extras.page.as_ref(), paper).size_nm().ok()?;
    Some(Rect { x0: kit::FRAME_OUTER, y0: kit::FRAME_OUTER, x1: w - kit::FRAME_OUTER, y1: h - kit::FRAME_OUTER })
}

/// Everything drawn on the sheet but the item `exclude_key`'s own fields, as the manual Autoplace Fields keeps clear of it
/// (`AUTOPLACER::getPossibleCollisions`): the other symbols by their bodies and pins and by their shown fields, the wires, labels, power symbols,
/// no-connects, junctions, texts and sheets.
pub fn surroundings(sch: &SchematicSection, model: &ConstraintModel, exclude_key: &str) -> Surroundings {
    let mut colliders: Vec<Collider> = Vec::new();
    // a symbol is one box: its body and its pins (`GetBodyAndPinsBoundingBox`); each unit of a part is a symbol of its own (r2 C-M5: the obstacle's owner
    // is the unit's field key, `U1#2`), and only the one being placed is left out
    let mut by_owner: std::collections::BTreeMap<String, Rect> = std::collections::BTreeMap::new();
    for o in crate::obstacles::of_section_but_wires(sch, model) {
        if o.owner.is_empty() {
            colliders.push(Collider { rect: o.rect, wire: None });
        } else if o.owner != exclude_key {
            let e = by_owner.entry(o.owner.clone()).or_insert(o.rect);
            *e = e.union(o.rect);
        }
    }
    for (_, rect) in by_owner {
        colliders.push(Collider { rect, wire: None });
    }
    for w in &sch.wires {
        for pair in w.pts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            // `SCH_LINE::GetBoundingBox`: the segment grown by half its pen (`GetPenWidth() / 2`), the default wire's 6 mils here (**Differs**: a wire's own
            // stroke width is not read)
            colliders.push(Collider { rect: Rect::of(a, b).inflate(Nm::new(76_200)), wire: Some((a.y, b.y)) });
        }
    }
    for t in &sch.texts {
        let size_iu = t.size_nm.get() / eda_model::units::SCH_IU_NM;
        let style = eda_model::kicad_geom::TextStyle { size_iu, thickness_iu: 0, h: HJustify::Left, v: VJustify::Center };
        let vertical = (t.angle / 90_000) % 2 == 1;
        colliders.push(Collider { rect: kit::text_rect_styled(&t.content, (t.at.x.as_f64(), t.at.y.as_f64()), &style, vertical), wire: None });
    }
    for s in &sch.sheets {
        colliders.push(Collider { rect: Rect::sized(s.at, s.size.0, s.size.1), wire: None });
        if s.id != exclude_key {
            for f in fields::sheet_fields(sch, s) {
                if f.visible && !f.text.is_empty() {
                    colliders.push(Collider { rect: fields::field_rect(&f), wire: None });
                }
            }
        }
    }
    // the shown fields of every other item
    for sym in &sch.symbols {
        let key = field_key(&sym.id, sym.unit);
        if key == exclude_key {
            continue;
        }
        if let Some(ctx) = OwnerCtx::new(sch, model, &key) {
            for f in ctx.page_fields(sch) {
                if f.visible && !f.text.is_empty() {
                    colliders.push(Collider { rect: fields::field_rect(&f), wire: None });
                }
            }
        }
    }
    for p in &sch.power_symbols {
        if p.id != exclude_key {
            for f in fields::power_fields(sch, p) {
                if f.visible && !f.text.is_empty() {
                    colliders.push(Collider { rect: fields::field_rect(&f), wire: None });
                }
            }
        }
    }
    Surroundings { colliders, drawable: drawable_area(sch) }
}

#[cfg(test)]
mod tests;
