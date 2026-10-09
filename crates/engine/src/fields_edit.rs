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

use eda_model::ir::{field_key, FieldPlacement, Point, PowerSymbol, SchematicSection, SheetInstance, SymbolInstance, TextJustify, TextVAlign};
use eda_model::kicad_font::{HJustify, VJustify};
use eda_model::sch_extras::AutoplaceAlgo;
use eda_model::ConstraintModel;

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
}

impl OwnerCtx {
    /// The item with `key` on `sch`, or `None`: no such item, a symbol the model has no part for, or a sheet read from a KiCad file (whose symbols
    /// are placed by their own origin, which the fields here are not measured from).
    pub fn new(sch: &SchematicSection, model: &ConstraintModel, key: &str) -> Option<OwnerCtx> {
        if sch.imported_from_kicad {
            return None;
        }
        if let Some(s) = sch.symbols.iter().find(|s| field_key(&s.id, s.unit) == key) {
            let part = model.part(&s.id)?;
            let mut sym = s.clone();
            if sym.lib_id.is_empty() {
                sym.lib_id = format!("eda:{}", sym.id);
            }
            let resolved = model.real_symbol_of(&sym.lib_id, part);
            let geom = SymbolGeom::of(&sym, part, resolved.as_ref());
            let specs = fields::symbol_specs(&sym, Some(part), resolved.as_ref().map(|r| r.datasheet.as_str()).unwrap_or(""));
            return Some(OwnerCtx { kind: OwnerKind::Symbol, key: key.to_string(), at: sym.at, rot: sym.rot, mirrored: sym.mirrored, width: geom.width, specs, sym: Some(sym), geom: Some(geom), power: None, sheet: None });
        }
        if let Some(p) = sch.power_symbols.iter().find(|p| p.id == key) {
            let flag = p.lib_id == "power:PWR_FLAG";
            let specs = vec![FieldSpec::new("Reference", p.id.clone(), false), FieldSpec::new("Value", p.net.clone(), !flag)];
            return Some(OwnerCtx { kind: OwnerKind::Power, key: key.to_string(), at: p.at, rot: p.rot, mirrored: false, width: 0.0, specs, sym: None, geom: None, power: Some(p.clone()), sheet: None });
        }
        if let Some(s) = sch.sheets.iter().find(|s| s.id == key) {
            let specs = vec![FieldSpec::new("Sheetname", s.name.clone(), true), FieldSpec::new("Sheetfile", s.file.clone(), true)];
            return Some(OwnerCtx { kind: OwnerKind::Sheet, key: key.to_string(), at: s.at, rot: 0, mirrored: false, width: 0.0, specs, sym: None, geom: None, power: None, sheet: Some(s.clone()) });
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
                self.specs.iter().map(|sp| stored.and_then(|s| s.iter().find(|p| p.name == sp.name)).or_else(|| auto.iter().find(|p| p.name == sp.name)).cloned().expect("a placement for each sheet field")).collect()
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

    /// `SCH_FIELD::SetPosition`: where on the sheet `page` is, as a stored position (micrometres from the item's origin, in its unturned frame).
    pub fn local_pos(&self, page: (f64, f64)) -> (i64, i64) {
        let (x, y) = unbake(self.rot, self.mirrored, self.width, page.0 - self.at.x as f64, page.1 - self.at.y as f64);
        (x.round() as i64, y.round() as i64)
    }

    /// A page offset as the offset of a field's stored position (`SCH_MOVE_TOOL::moveItem`: the offset taken through the inverse of the symbol's
    /// transform).
    pub fn local_delta(&self, dx: i64, dy: i64) -> (i64, i64) {
        let (x, y) = unbake(self.rot, self.mirrored, 0.0, dx as f64, dy as f64);
        (x.round() as i64, y.round() as i64)
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
        p.size_um = old.size_um;
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
                let home = fields::power_value_placement(ps, &ps.lib_id);
                self.specs
                    .iter()
                    .zip(current.iter())
                    .map(|(sp, now)| {
                        let mut p = if sp.name == "Value" { home.clone() } else { FieldPlacement { visible: false, ..FieldPlacement::at_origin(&sp.name) } };
                        p.visible = now.visible;
                        p.size_um = now.size_um;
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
            OwnerKind::Sheet => {
                let home = fields::sheet_placements(self.sheet.as_ref().expect("a sheet"));
                self.specs
                    .iter()
                    .zip(current.iter())
                    .map(|(sp, now)| {
                        if now.no_autoplace {
                            return now.clone();
                        }
                        let mut p = home.iter().find(|p| p.name == sp.name).cloned().expect("a home for each sheet field");
                        p.visible = now.visible;
                        p.size_um = now.size_um;
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

/// `AUTOPLACER::getDrawableArea`: the paper inside the drawing sheet's 10 mm margins.
fn drawable_area(sch: &SchematicSection) -> Option<Rect> {
    let paper = sch.title_block.as_ref().map(|t| t.paper.as_str()).unwrap_or("");
    let (w, h) = eda_model::page::PageSettings::of_sheet(sch.extras.page.as_ref(), paper).size_um().ok()?;
    Some(Rect { x0: kit::FRAME_OUTER, y0: kit::FRAME_OUTER, x1: w - kit::FRAME_OUTER, y1: h - kit::FRAME_OUTER })
}

/// Everything drawn on the sheet but the item `exclude_key`'s own fields, as the manual Autoplace Fields keeps clear of it
/// (`AUTOPLACER::getPossibleCollisions`): the other symbols by their bodies and pins and by their shown fields, the wires, labels, power symbols,
/// no-connects, junctions, texts and sheets.
pub fn surroundings(sch: &SchematicSection, model: &ConstraintModel, exclude_key: &str) -> Surroundings {
    let mut colliders: Vec<Collider> = Vec::new();
    // a symbol is one box: its body and its pins (`GetBodyAndPinsBoundingBox`)
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
            colliders.push(Collider { rect: Rect::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)), wire: Some((a.y, b.y)) });
        }
    }
    for t in &sch.texts {
        let size_iu = t.size_um * 10;
        let style = eda_model::kicad_geom::TextStyle { size_iu, thickness_iu: 0, h: HJustify::Left, v: VJustify::Center };
        let vertical = (t.angle / 90_000) % 2 == 1;
        colliders.push(Collider { rect: kit::text_rect_styled(&t.content, (t.at.x as f64, t.at.y as f64), &style, vertical), wire: None });
    }
    for s in &sch.sheets {
        colliders.push(Collider { rect: Rect::new(s.at.x, s.at.y, s.at.x + s.size.0, s.at.y + s.size.1), wire: None });
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
