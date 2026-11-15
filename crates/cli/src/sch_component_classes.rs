//! The component classes the schematic gives each footprint: what the netlist says of a component (`(component_classes (class ..))`,
//! `NETLIST_EXPORTER_XML::getComponentClassNamesForAllSymbolUnits`, KiCad 25ec2335 `eeschema/netlist_exporters/netlist_exporter_xml.cpp`) and Update PCB from
//! Schematic makes the footprint's static class (`BOARD_NETLIST_UPDATER::updateComponentClass`). The schematic and the board are one design here, so the board
//! always has what the update would give it: [`fold`] puts the names on each link (`SymbolLink::component_classes`), which
//! `eda_model::component_classes::static_classes` reads for the writer's `(component_classes ..)`, the rule engine (`A.hasComponentClass`, `A.ComponentClass`,
//! `${Class:..}`), Multichannel and the Properties panel.
//!
//! A component's classes are the union, over every unit of the symbol (each placed symbol with its reference, on whatever sheet: `ref2.CmpNoCase( ref )`):
//!
//! - the shown text of each of its `Component Class` fields that says something (`SCH_SYMBOL::GetComponentClassNames`: `GetShownText( aPath, INTERNAL )`,
//!   not trimmed);
//! - the `Component Class` fields of the directive labels attached to each rule area the symbol is in (`m_rule_areas_cache` ->
//!   `SCH_RULE_AREA::GetDirectives`): a directive is attached when its connection point is within 5 IU of the area's edge, and a symbol is in an area its
//!   box -- body, pins and the fields it shows (`SCH_SYMBOL::GetBoundingBox`) -- collides with (`SCH_RULE_AREA::RefreshContainedItemsAndDirectives`);
//! - the same of the rule areas around a sheet symbol, for every symbol on that sheet and below it (`getSheetComponentClasses`, `IsContainedWithin`).
//!
//! **Differs**: the variant's own field texts are not read (KiCad's `m_exportVariant`; Update PCB exports the default variant's), and a symbol's box measures
//! its pins by their lines (`SCH_PIN::GetBoundingBox( false, false, false )` adds the decoration of an inverted or clocked pin).

use eda_model::component_classes::COMPONENT_CLASS_FIELD;
use eda_model::ir::{Design, Point, SchematicSection};
use eda_model::sch_extras::{LabelField, SchGraphicKind};
use eda_model::ConstraintModel;
use std::collections::{BTreeMap, BTreeSet};

/// A box in nanometres, `(x0, y0, x1, y1)`.
type Bx = (i64, i64, i64, i64);

/// Puts on every link of `model.symbol_links` the classes the schematic gives its footprint (empty when it gives none). A design with no schematic has no links.
pub(crate) fn fold(design: &Design, model: &mut ConstraintModel, project: &eda_model::text_vars::Project) {
    if model.symbol_links.is_empty() {
        return;
    }
    let classes = classes_by_reference(design, model, project);
    for (reference, link) in model.symbol_links.iter_mut() {
        link.component_classes = Some(classes.get(reference).map(|c| c.iter().cloned().collect()).unwrap_or_default());
    }
}

/// Whether any screen of the design has a field or a directive that can give a class: most designs have none, and then nothing is measured.
fn has_any_class(design: &Design) -> bool {
    let mut screens: Vec<&SchematicSection> = design.schematic.iter().collect();
    if let Some(c) = design.sheet_contents.as_ref() {
        screens.extend(c.values());
    }
    screens.iter().any(|sec| {
        sec.user_fields.values().flatten().any(|(n, _)| n == COMPONENT_CLASS_FIELD)
            || sec.extras.graphics.iter().any(|g| matches!(&g.shape, SchGraphicKind::Directive { component_class, .. } if !component_class.is_empty()))
            || sec.extras.label_fields.values().flatten().any(|f| f.name == COMPONENT_CLASS_FIELD)
    })
}

/// Reference -> the component's class names (`std::sort` of the set: `wxString::Cmp`, which a `BTreeSet` of the names is).
pub(crate) fn classes_by_reference(design: &Design, model: &ConstraintModel, project: &eda_model::text_vars::Project) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    if !has_any_class(design) {
        return out;
    }
    let subpart = eda_model::annotate::SubpartIds::of_design(design);
    let entries = eda_model::sheet_list::walk(design);
    let page_count = entries.len();
    // the sheet paths a reference's symbols are on, and the classes of the rule areas around each sheet placement (`m_sheetComponentClasses`)
    let mut on_sheets: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
    let mut sheet_classes: Vec<(Vec<String>, BTreeSet<String>)> = Vec::new();
    for e in &entries {
        let Some(sec) = eda_model::sheet_list::screen_of(design, &e.path) else { continue };
        let levels = crate::text_vars_json::sheet_levels(design, &e.path);
        let sheet = eda_model::text_vars::SchSheet {
            project: project.clone(),
            sheet_path: std::iter::once("/".to_string()).chain(levels.iter().skip(1).filter_map(|l| l.fields.first()).map(|(_, n)| format!("{n}/"))).collect(),
            levels,
            page: e.effective_page(),
            page_count,
            sheet_name: if e.path.is_empty() { "Root".to_string() } else { e.name.clone() },
            ..Default::default()
        };
        let areas = rule_areas_with_classes(sec, &sheet);
        for sym in sec.symbols.iter().filter(|s| !s.exclude_from_board) {
            let Some(part) = model.part(&sym.id) else { continue };
            let reference = eda_model::sheet_list::reference_under(sec, sym, &e.path);
            let entry = out.entry(reference.clone()).or_default();
            // the symbol's own fields (`getComponentClassFields( m_fields )`)
            let vars = crate::text_vars_json::symbol_of(sec, model, sym, &subpart);
            for (_, raw) in vars.fields.iter().filter(|(n, _)| n == COMPONENT_CLASS_FIELD) {
                let shown = if eda_model::text_vars::has_text_vars(raw) { sheet.field_shown(sheet.levels.len().saturating_sub(1), Some(&vars), raw, 0) } else { raw.clone() };
                if !shown.is_empty() {
                    entry.insert(shown);
                }
            }
            // the rule areas around it
            if !areas.is_empty() {
                let bx = symbol_box(sec, model, sym, part);
                for (poly, classes) in &areas {
                    if bx.is_some_and(|b| polygon_collides_box(poly, b)) {
                        entry.extend(classes.iter().cloned());
                    }
                }
            }
            on_sheets.entry(reference).or_default().push(e.path.clone());
        }
        // the rule areas around each sheet symbol of this screen give their classes to everything under that placement (`getSheetComponentClasses`)
        for sh in &sec.sheets {
            let mut here: BTreeSet<String> = BTreeSet::new();
            if !areas.is_empty() {
                let bx = sheet_box(sec, sh);
                for (poly, classes) in &areas {
                    if polygon_collides_box(poly, bx) {
                        here.extend(classes.iter().cloned());
                    }
                }
            }
            if !here.is_empty() {
                let mut path = e.path.clone();
                path.push(sh.id.clone());
                sheet_classes.push((path, here));
            }
        }
    }
    // `symbolSheetPath.IsContainedWithin( sheetPath )`: the sheet placement's path starts the symbol's
    for (reference, paths) in &on_sheets {
        for (sheet_path, classes) in &sheet_classes {
            if paths.iter().any(|p| p.starts_with(sheet_path)) {
                out.entry(reference.clone()).or_default().extend(classes.iter().cloned());
            }
        }
    }
    out.retain(|_, c| !c.is_empty());
    out
}

/// `FOOTPRINT::GetComponentClass()`'s constituent names of every footprint, sorted: its static classes (`eda_model::component_classes::static_classes`) and,
/// when the project has an assignment rule or sheet classes, the ones the rule engine gives it (`GetDynamicComponentClassesForFootprint`, the rule board's
/// footprint items). Footprints in no class are left out.
pub(crate) fn effective_classes(design: &Design, model: &ConstraintModel) -> BTreeMap<String, Vec<String>> {
    if !model.board.component_classes.is_default() {
        let board = eda_drc::board::build(design, model);
        let rb = eda_drc::rule_board::RuleBoard::new(design, model, &board, eda_drc::constraints::Units::Mm, None);
        return model
            .parts
            .iter()
            .filter_map(|p| {
                let classes = rb.footprint_item(&p.reference)?.footprint.as_ref()?.classes.clone();
                (!classes.is_empty()).then(|| (p.reference.clone(), classes))
            })
            .collect();
    }
    let extras: BTreeMap<&str, &eda_model::ir::FootprintExtra> = design.drawings.iter().flat_map(|d| d.footprint_extras.iter()).map(|e| (e.id.as_str(), e)).collect();
    model
        .parts
        .iter()
        .filter_map(|p| {
            let classes = eda_model::component_classes::static_classes(extras.get(p.reference.as_str()).copied(), model.symbol_links.get(&p.reference));
            (!classes.is_empty()).then(|| (p.reference.clone(), classes))
        })
        .collect()
}

/// The `Component Class` fields of a directive label that say something on `sheet` (`getComponentClassFields( label->GetFields() )`): its own (the flag's
/// second field, `SchGraphicKind::Directive::component_class`) and any further field of that name.
fn directive_classes(sheet: &eda_model::text_vars::SchSheet, own: &str, fields: &[LabelField]) -> Vec<String> {
    std::iter::once(own)
        .chain(fields.iter().filter(|f| f.name == COMPONENT_CLASS_FIELD).map(|f| f.text.as_str()))
        .map(|raw| crate::board::label_field_shown(sheet, fields, raw, true))
        .filter(|c| !c.is_empty())
        .collect()
}

/// Each rule area of the screen with the classes of the directives attached to it (`RefreshContainedItemsAndDirectives`: the connection point within 5 IU of an
/// edge, `CollideEdge`); the areas that get none are left out.
fn rule_areas_with_classes(sec: &SchematicSection, sheet: &eda_model::text_vars::SchSheet) -> Vec<(Vec<Point>, BTreeSet<String>)> {
    let directives: Vec<(Point, Vec<String>)> = sec
        .extras
        .graphics
        .iter()
        .filter_map(|g| match &g.shape {
            SchGraphicKind::Directive { at, component_class, .. } => {
                let fields = sec.extras.label_fields.get(&g.id).map(Vec::as_slice).unwrap_or(&[]);
                let classes = directive_classes(sheet, component_class, fields);
                (!classes.is_empty()).then_some((*at, classes))
            }
            _ => None,
        })
        .collect();
    if directives.is_empty() {
        return Vec::new();
    }
    sec.extras
        .graphics
        .iter()
        .filter_map(|g| match &g.shape {
            SchGraphicKind::RuleArea { pts, .. } if pts.len() >= 3 => {
                let classes: BTreeSet<String> = directives.iter().filter(|(at, _)| crate::board::polygon_edge_distance(pts, *at) <= crate::board::RULE_AREA_EDGE_NM).flat_map(|(_, c)| c.iter().cloned()).collect();
                (!classes.is_empty()).then(|| (pts.clone(), classes))
            }
            _ => None,
        })
        .collect()
}

fn merge(a: Option<Bx>, b: Bx) -> Bx {
    match a {
        None => b,
        Some(a) => (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)),
    }
}

/// `SCH_SYMBOL::GetBoundingBox` (`doGetBoundingBox( true, true )`): the body (`LIB_SYMBOL::GetBodyBoundingBox` with the pins: each shown pin's line), then the
/// fields it shows. `None` for a symbol that draws nothing.
fn symbol_box(sec: &SchematicSection, model: &ConstraintModel, sym: &eda_model::ir::SymbolInstance, part: &eda_model::Part) -> Option<Bx> {
    let mut sym = sym.clone();
    if sym.lib_id.is_empty() {
        sym.lib_id = format!("eda:{}", sym.id);
    }
    let resolved = model.real_symbol_of_instance(sec, &sym, part);
    let geom = eda_engine::symgeom::SymbolGeom::of_in(sec, &sym, part, resolved.as_ref());
    let mut bx: Option<Bx> = geom.body.map(|b| (b.x0.floor() as i64, b.y0.floor() as i64, b.x1.ceil() as i64, b.y1.ceil() as i64));
    for p in geom.pins.iter().filter(|p| !p.hidden) {
        let d = p.orient.dir();
        let end = (p.tip.0 + d.0 * p.length, p.tip.1 + d.1 * p.length);
        let b = (p.tip.0.min(end.0).floor() as i64, p.tip.1.min(end.1).floor() as i64, p.tip.0.max(end.0).ceil() as i64, p.tip.1.max(end.1).ceil() as i64);
        bx = Some(merge(bx, b));
    }
    for f in eda_engine::fields::symbol_fields(sec, &sym, Some(part), resolved.as_ref(), &geom).iter().filter(|f| f.visible && !f.text.is_empty()) {
        let r = eda_engine::fields::field_rect(f);
        bx = Some(merge(bx, (r.x0.get(), r.y0.get(), r.x1.get(), r.y1.get())));
    }
    bx
}

/// `SCH_SHEET::GetBoundingBox`: the sheet's body and the fields it shows.
fn sheet_box(sec: &SchematicSection, sheet: &eda_model::ir::SheetInstance) -> Bx {
    let mut bx = (sheet.at.x.get(), sheet.at.y.get(), (sheet.at.x + sheet.size.0).get(), (sheet.at.y + sheet.size.1).get());
    for f in eda_engine::fields::sheet_fields(sec, sheet).iter().filter(|f| f.visible && !f.text.is_empty()) {
        let r = eda_engine::fields::field_rect(f);
        bx = merge(Some(bx), (r.x0.get(), r.y0.get(), r.x1.get(), r.y1.get()));
    }
    bx
}

/// `SHAPE_POLY_SET::Collide( SHAPE_RECT )` with no clearance: the outline and the box share a point -- a corner of the box in the outline (or on its edge), a
/// vertex of the outline in the box, or two edges crossing.
fn polygon_collides_box(poly: &[Point], (x0, y0, x1, y1): Bx) -> bool {
    let pt = |x: i64, y: i64| Point { x: eda_model::units::Nm::new(x), y: eda_model::units::Nm::new(y) };
    let corners = [pt(x0, y0), pt(x1, y0), pt(x1, y1), pt(x0, y1)];
    if corners.iter().any(|c| crate::board::point_in_polygon(poly, *c)) {
        return true;
    }
    if poly.iter().any(|p| (x0..=x1).contains(&p.x.get()) && (y0..=y1).contains(&p.y.get())) {
        return true;
    }
    (0..4).any(|i| crate::board::segment_collides_polygon(poly, corners[i], corners[(i + 1) % 4]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point { x: eda_model::units::Nm::new(x), y: eda_model::units::Nm::new(y) }
    }

    /// A box collides with an outline it overlaps, lies in, holds or touches, and not with one beside it.
    #[test]
    fn a_box_collides_with_an_outline_it_shares_a_point_with() {
        let square = vec![p(0, 0), p(100, 0), p(100, 100), p(0, 100)];
        assert!(polygon_collides_box(&square, (10, 10, 20, 20)), "inside");
        assert!(polygon_collides_box(&square, (-50, -50, 200, 200)), "around");
        assert!(polygon_collides_box(&square, (90, 40, 150, 60)), "across an edge");
        assert!(polygon_collides_box(&square, (-10, 40, 110, 60)), "through, no corner of either in the other");
        assert!(polygon_collides_box(&square, (100, 10, 120, 20)), "touching the edge");
        assert!(!polygon_collides_box(&square, (101, 10, 120, 20)), "beside");
    }
}

#[cfg(test)]
#[path = "sch_component_classes_tests.rs"]
mod sch_component_classes_tests;
