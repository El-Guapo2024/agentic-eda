//! Drill Chart Properties (`DIALOG_GENERATED_TABLE_PROPERTIES` with `PANEL_DRILL_CHART_OPTIONS`) and Drill Groups (`DIALOG_DRILL_GROUPS`) of KiCad 25ec2335 as verbs:
//! `edit_drill_chart` and `set_drill_profile`.

use super::*;
use crate::pcb_items::PcbItemCmd;
use crate::pcb_transform_tests::{design, drawings, model, p};
use crate::{Board, Cmd};
use eda_model::drill_chart::DrillChartColumn;
use eda_model::units::mm;

fn via(b: &mut Board<'_>, x: f64, drill: f64) {
    b.apply(&Cmd::AddVia { net: "GND".into(), x: mm(x), y: mm(10.0), drill: mm(drill), diameter: mm(drill + 0.3), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
}

fn place(b: &mut Board<'_>, at: (f64, f64)) -> String {
    b.apply(&Cmd::PcbItem(PcbItemCmd::PlaceDrillChart { layer: "Dwgs.User".into(), at: p(mm(at.0), mm(at.1)) })).unwrap();
    drawings(b).board_items.drill_charts.last().unwrap().table_id.clone()
}

/// What the dialog opens with for the chart `id` (`TransferDataToWindow`), as an edit that changes nothing.
fn opened(b: &Board<'_>, id: &str) -> DrillChartEdit {
    let dr = drawings(b);
    let t = dr.tables.iter().find(|t| t.id == id).unwrap();
    let c = dr.board_items.drill_charts.iter().find(|c| c.table_id == id).unwrap();
    DrillChartEdit {
        layer: t.layer.clone(),
        units: c.units,
        precision: c.precision,
        columns: c.columns.clone(),
        filter: c.filter,
        show_totals: c.show_totals,
        group_by: profile_of(b.design()).group_by,
        external_border: t.border.external,
        header_separator: t.border.header,
        row_separators: t.separators.rows,
        column_separators: t.separators.cols,
        border_width_nm: t.border.stroke.width_nm,
        separators_width_nm: t.separators.stroke.width_nm,
    }
}

fn edit(b: &mut Board<'_>, id: &str, e: DrillChartEdit) -> Result<(), Vec<eda_model::CheckResult>> {
    b.apply(&Cmd::PcbItem(PcbItemCmd::EditDrillChart { id: id.to_string(), chart: e }))
}

fn texts(b: &Board<'_>, id: &str) -> Vec<String> {
    drawings(b).tables.iter().find(|t| t.id == id).unwrap().cells.iter().map(|c| c.text.clone()).collect()
}

/// OK on Drill Chart Properties: every page's value lands where `TransferDataFromWindow` puts it, and the chart is rebuilt with the new columns.
#[test]
fn drill_chart_properties_sets_the_columns_units_strokes_and_layer_and_rebuilds_the_chart() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    via(&mut b, 10.0, 0.3);
    via(&mut b, 20.0, 0.3);
    let id = place(&mut b, (5.0, 6.0));
    let mut e = opened(&b, &id);
    let mut sym = DrillChartColumn::default_of(DrillChartColumnId::Symbol);
    sym.heading = "MARK".into();
    let mut sites = DrillChartColumn::default_of(DrillChartColumnId::SiteCount);
    sites.align = TableAlign::Left;
    sites.width_nm = mm(30.0);
    e.columns = vec![DrillChartColumn::default_of(DrillChartColumnId::DrillDiameter), sym, sites];
    e.units = TableUnits::Mils;
    e.precision = 1;
    e.layer = "Cmts.User".into();
    e.show_totals = false;
    e.external_border = false;
    e.header_separator = true;
    e.border_width_nm = mm(0.2);
    e.row_separators = false;
    e.column_separators = false;
    e.separators_width_nm = mm(0.3);
    edit(&mut b, &id, e).unwrap();
    let dr = drawings(&b);
    let t = dr.tables.iter().find(|t| t.id == id).unwrap();
    let c = &dr.board_items.drill_charts[0];
    assert_eq!((t.col_count(), t.row_count(), t.layer.as_str()), (3, 2, "Cmts.User"), "three columns, the heading and the one 0.3 mm row, no totals");
    // The test board's own via is 0.3 mm too: three sites.
    assert_eq!(texts(&b, &id), vec!["DRILL DIA", "MARK", "SITE COUNT", "12 mil", "", "3"]);
    assert_eq!(t.col_widths[2], mm(30.0), "a minimum width");
    assert_eq!((c.units, c.precision, c.show_totals, c.symbol_column), (TableUnits::Mils, 1, false, 1));
    // `aStroked ? std::max( 0, width ) : -1`: the header separator draws with the border's width; no separators, no width.
    assert_eq!((t.border.external, t.border.header, t.border.stroke.width_nm), (false, true, mm(0.2)));
    assert_eq!((t.separators.rows, t.separators.cols, t.separators.stroke.width_nm), (false, false, Nm::new(-1)));
}

#[test]
fn drill_chart_properties_refuses_what_kicad_refuses() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    via(&mut b, 10.0, 0.3);
    let id = place(&mut b, (5.0, 6.0));
    let before = b.design().clone();
    let hint = |r: Result<(), Vec<eda_model::CheckResult>>| r.unwrap_err()[0].hint.clone().unwrap_or_default();
    let mut e = opened(&b, &id);
    e.columns.clear();
    assert_eq!(hint(edit(&mut b, &id, e)), "A table needs at least one column.");
    let mut e = opened(&b, &id);
    e.columns = DrillChartColumnId::ALL.into_iter().map(|c| DrillChartColumn { width_nm: Nm::new(2_000_000_000), ..DrillChartColumn::default_of(c) }).collect();
    assert_eq!(hint(edit(&mut b, &id, e)), "The columns are too wide in total.", "twelve columns of a metre each (clamped) are more than ten metres");
    let mut e = opened(&b, &id);
    e.layer = "F.SilkS".into();
    assert!(edit(&mut b, &id, e).is_err(), "not a documentation layer");
    assert_eq!(serde_json::to_value(b.design()).unwrap(), serde_json::to_value(&before).unwrap(), "a refused edit leaves the board as it was");
    // `Validate`: a repeated column adds nothing.
    let mut e = opened(&b, &id);
    e.columns.push(DrillChartColumn::default_of(DrillChartColumnId::Symbol));
    edit(&mut b, &id, e).unwrap();
    assert_eq!(drawings(&b).board_items.drill_charts[0].columns.len(), 6);
}

/// "Grouping is shared by every drill chart and drill map on the board": a chart's new grouping rebuilds the other charts too.
#[test]
fn a_new_grouping_from_one_charts_dialog_rebuilds_every_chart() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    via(&mut b, 10.0, 0.3);
    via(&mut b, 20.0, 0.4);
    let first = place(&mut b, (5.0, 6.0));
    let second = place(&mut b, (5.0, 40.0));
    assert_eq!(drawings(&b).tables.iter().find(|t| t.id == second).unwrap().row_count(), 4, "two sizes, two rows");
    let mut e = opened(&b, &first);
    e.group_by.retain(|k| *k != DrillGroupKey::Size);
    edit(&mut b, &first, e).unwrap();
    assert!(!profile_of(b.design()).is_grouped_by(DrillGroupKey::Size));
    for id in [&first, &second] {
        assert_eq!(drawings(&b).tables.iter().find(|t| t.id == *id).unwrap().row_count(), 3, "not grouped by size: one row for both vias");
    }
}

/// Drill Groups: a group's mark (`commitDetail`) and Freeze assignments (`onFreezeChanged`) are the board's profile, and the charts show the new mark.
#[test]
fn a_groups_new_mark_is_the_boards_and_every_chart_shows_it() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    via(&mut b, 10.0, 0.3);
    let id = place(&mut b, (5.0, 6.0));
    let key = drawings(&b).board_items.drill_charts[0].row_keys[&1].clone();
    let mark = DrillSymbolAssignment { mode: DrillMarkMode::Letter, shape_index: 0, letter: "Q".into(), description: "stitching".into() };
    let mut e = opened(&b, &id);
    e.columns.push(DrillChartColumn::default_of(DrillChartColumnId::Description));
    edit(&mut b, &id, e).unwrap();
    let set = |b: &mut Board<'_>, profile: DrillProfileEdit| b.apply(&Cmd::PcbItem(PcbItemCmd::SetDrillProfile { profile }));
    set(&mut b, DrillProfileEdit { assignment: Some(DrillGroupMark { key: key.clone(), mark: mark.clone() }), ..Default::default() }).unwrap();
    assert_eq!(profile_of(b.design()).assignments[&key], mark);
    let t = texts(&b, &id);
    assert_eq!((t[7].as_str(), t[13].as_str()), ("Q", "stitching"), "the symbol cell and the Description column: {t:?}");
    assert!(drawings(&b).board_items.drill_charts[0].row_shapes.is_empty(), "a letter draws no shape");
    // "same": nothing to do, no undo step.
    assert_eq!(set(&mut b, DrillProfileEdit { assignment: Some(DrillGroupMark { key: key.clone(), mark }), ..Default::default() }).unwrap_err()[0].check, "ops_nothing_to_do");
    set(&mut b, DrillProfileEdit { freeze_assignments: Some(false), ..Default::default() }).unwrap();
    assert!(!profile_of(b.design()).freeze_assignments);
    // Not frozen: a rebuild assigns afresh, from the first curated shape.
    assert_eq!(drawings(&b).board_items.drill_charts[0].row_shapes.get(&1), Some(&0));
    // Board Setup's Drill Chart page: the default marks, the symbol size and width.
    set(&mut b, DrillProfileEdit { mark_policy: Some(DrillMarkPolicy::SizeText), size_nm: Some(mm(2.0)), width_nm: Some(mm(0.2)), ..Default::default() }).unwrap();
    let pr = profile_of(b.design());
    assert_eq!((pr.mark_policy, pr.size_nm, pr.width_nm), (DrillMarkPolicy::SizeText, mm(2.0), mm(0.2)));
    assert_eq!(texts(&b, &id)[7], "0.300", "the hole size as the mark");
}
