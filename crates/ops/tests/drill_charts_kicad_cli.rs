//! KiCad 10's drill charts built here as kicad-cli builds them: `pcb upgrade --force` rebuilds every stale chart of a board (`RefreshGeneratedTables`, `PCB_DRILL_CHART::generate`,
//! `PCB_GENERATED_TABLE::RebuildCells`), so the chart kicad-cli writes for a board is the chart `eda_ops::drill_chart` must build for it -- the rows, their text, alignment and
//! order, which cells keep their looks, the widths and heights `Autosize` gives them, the totals, the marks and the profile. KiCad 25ec2335's `api_kitchen_sink` is the board.
//! Slow tier: kicad-cli runs, so it needs `EDA_SLOW_TESTS=1`; skipped, loudly, without kicad-cli or the QA board.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::drill_chart::{DrillChart, DrillChartFilter, TableUnits};
use eda_model::ir::{Design, Point};
use eda_model::table::Table;
use eda_model::ConstraintModel;
use eda_ops::pcb_items::PcbItemCmd;
use eda_ops::{Board, Cmd, TableCmd};
use std::path::{Path, PathBuf};
use std::process::Command;

const QA_DEFAULT: &str = "/home/team_braneboard_com/src/kicad-src-25ec2335/qa/data";

fn slow_tests_off() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    off
}

fn kicad_cli() -> Option<PathBuf> {
    let p = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("kicad-cli"));
    Command::new(&p).arg("version").output().ok().filter(|o| o.status.success()).map(|_| p)
}

fn kitchen_sink() -> Option<String> {
    let qa = std::env::var_os("KICAD10_QA_DATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(QA_DEFAULT));
    std::fs::read_to_string(qa.join("pcbnew/api_kitchen_sink.kicad_pcb")).ok()
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("eda_drill_charts_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The board kicad-cli writes for `text` (`pcb upgrade --force`, which rebuilds the stale charts), read back.
fn upgraded(cli: &Path, text: &str, tag: &str) -> (Design, ConstraintModel) {
    let path = scratch(tag).join("board.kicad_pcb");
    std::fs::write(&path, text).unwrap();
    let out = Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "{tag}: kicad-cli pcb upgrade: {}", String::from_utf8_lossy(&out.stderr));
    let (d, m, _) = import_kicad_pcb(&std::fs::read_to_string(&path).unwrap()).unwrap();
    (d, m)
}

fn chart_of(d: &Design) -> (Table, DrillChart) {
    let dr = d.drawings.as_ref().unwrap();
    let c = dr.board_items.drill_charts.first().expect("a drill chart").clone();
    let t = dr.tables.iter().find(|t| t.id == c.table_id).unwrap().clone();
    (t, c)
}

/// The two charts are one: the same rows, cells (text, alignment, size and margins of the text), keys, marks and profile, the same corner, and widths and heights within 2 µm
/// (KiCad lays them out from the margins in nanometres, 1.0025 mm, this port from the micrometre the IR holds them to, 1.003 mm).
fn assert_same_chart(tag: &str, ours: &Design, kicad: &Design) {
    let ((t, c), (kt, kc)) = (chart_of(ours), chart_of(kicad));
    let cells = |t: &Table| t.cells.iter().map(|x| (x.text.clone(), x.style.h_align, x.style.height_um, x.margins)).collect::<Vec<_>>();
    assert_eq!(cells(&t), cells(&kt), "{tag}: the cells");
    assert_eq!((t.col_count(), t.row_count(), t.at), (kt.col_count(), kt.row_count(), kt.at), "{tag}: the grid");
    let near = |a: &[i64], b: &[i64]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 2);
    assert!(near(&t.col_widths, &kt.col_widths), "{tag}: column widths {:?} against kicad-cli's {:?}", t.col_widths, kt.col_widths);
    assert!(near(&t.row_heights, &kt.row_heights), "{tag}: row heights {:?} against kicad-cli's {:?}", t.row_heights, kt.row_heights);
    assert_eq!((&c.row_keys, &c.row_shapes, c.symbol_column), (&kc.row_keys, &kc.row_shapes, kc.symbol_column), "{tag}: the keys and marks");
    let profile = |d: &Design| d.drawings.as_ref().unwrap().board_items.drill_profile.clone();
    assert_eq!(profile(ours), profile(kicad), "{tag}: the profile");
    eprintln!("{tag}: {} x {} cells as kicad-cli builds them", t.col_count(), t.row_count());
}

fn export(d: &Design, m: &ConstraintModel) -> String {
    export_kicad_pcb(d, m, &ExportMeta { date: "2026-10-23", title: "drill charts" }).unwrap()
}

/// A chart placed on a board (`PlaceDrillChart`: the default template, the board's own profile) is the chart kicad-cli builds from a chart that has nothing but its
/// layer: the six default columns, a row for each hole size in the order the model sorts them, the shapes the frozen profile gave them, the totals row.
#[test]
fn a_placed_drill_chart_is_the_one_kicad_builds() {
    if slow_tests_off() {
        return;
    }
    let (Some(cli), Some(text)) = (kicad_cli(), kitchen_sink()) else {
        eprintln!("skipped: needs kicad-cli and KiCad 25ec2335's api_kitchen_sink.kicad_pcb (KICAD10_QA_DATA)");
        return;
    };
    // KiCad's: the chart with no cells and no settings, rebuilt.
    let start = text.find("\t(drill_chart").unwrap();
    let end = start + text[start..].find("\t(drill_map").unwrap();
    let bare = format!("{}\t(drill_chart (uuid \"5383ee38-a9d4-45be-94ed-27ce389fc0fc\") (layer \"Dwgs.User\") (column_count 6))\n{}", &text[..start], &text[end..]);
    let (kicad, _) = upgraded(&cli, &bare, "placed");

    // Ours: the board's chart taken away, a new one placed at the origin.
    let (d, m, _) = import_kicad_pcb(&text).unwrap();
    let (old, _) = chart_of(&d);
    let mut b = Board::new(d, &m, 100, 300);
    b.apply(&Cmd::Table(TableCmd::Delete { id: old.id })).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().board_items.drill_charts.is_empty(), "the chart goes with its table");
    b.apply(&Cmd::PcbItem(PcbItemCmd::PlaceDrillChart { layer: "Dwgs.User".into(), at: Point { x: 0, y: 0 } })).unwrap();
    assert_same_chart("placed", b.design(), &kicad);
    let (t, _) = chart_of(b.design());
    assert_eq!(t.cells.iter().take(6).map(|c| c.text.as_str()).collect::<Vec<_>>(), ["SYM", "DRILL DIA", "SLOT W x L", "PLATED", "OPS", "FROM / TO"]);
    assert_eq!(t.cells[6 * 4].text, "7 OPS / 7 SITES");
}

/// A chart whose settings change is built again, and its rows keep their looks (`migrateRows`): two places more than the file's, as kicad-cli builds it.
#[test]
fn a_drill_chart_with_new_settings_is_rebuilt_as_kicad_rebuilds_it() {
    if slow_tests_off() {
        return;
    }
    let (Some(cli), Some(text)) = (kicad_cli(), kitchen_sink()) else {
        eprintln!("skipped: needs kicad-cli and KiCad 25ec2335's api_kitchen_sink.kicad_pcb (KICAD10_QA_DATA)");
        return;
    };
    let (kicad, _) = upgraded(&cli, &text.replacen("(precision 2)", "(precision 4)", 1), "precision");
    let (d, m, _) = import_kicad_pcb(&text).unwrap();
    let (t, c) = chart_of(&d);
    let mut b = Board::new(d, &m, 100, 300);
    let filter: DrillChartFilter = c.filter;
    b.apply(&Cmd::PcbItem(PcbItemCmd::SetDrillChart { id: t.id.clone(), units: TableUnits::Mm, precision: 4, show_totals: c.show_totals, filter })).unwrap();
    assert_same_chart("precision", b.design(), &kicad);
    assert_eq!(chart_of(b.design()).0.cells[3].text, "0.3000");
}

/// A hole of a new size on the board: the chart, stale, is rebuilt by the step that made it (`refresh_drill_charts`, KiCad's `OnModify`) with a row between the others, the
/// rows that still report what they did keeping their cells, the new one new cells -- as kicad-cli rebuilds the stale chart of the same board -- and the new size's mark is
/// the board's profile's from then on.
#[test]
fn a_new_hole_size_gets_a_row_as_kicad_gives_it_one() {
    if slow_tests_off() {
        return;
    }
    let (Some(cli), Some(text)) = (kicad_cli(), kitchen_sink()) else {
        eprintln!("skipped: needs kicad-cli and KiCad 25ec2335's api_kitchen_sink.kicad_pcb (KICAD10_QA_DATA)");
        return;
    };
    let (d, m, _) = import_kicad_pcb(&text).unwrap();
    let mut b = Board::new(d, &m, 100, 300);
    let net = m.nets.first().map(|n| n.name.clone()).unwrap_or_default();
    b.apply(&Cmd::AddVia { net, x: 170_000, y: 80_000, drill: 800, diameter: 1_400, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    // KiCad's: the board with the via and the chart as it was, rebuilt by kicad-cli.
    let (kicad, _) = upgraded(&cli, &export(b.design(), &m), "new_hole");
    assert!(b.refresh_drill_charts(), "the chart is stale");
    assert_same_chart("new hole", b.design(), &kicad);
    let (t, c) = chart_of(b.design());
    assert_eq!(t.cells.iter().skip(1).step_by(2).map(|c| c.text.as_str()).collect::<Vec<_>>(), ["Drill (mm)", "0.30", "0.80", "1.14", "1.52"]);
    assert_eq!(c.row_shapes.get(&2), Some(&3), "the lowest shape no group has");
    assert!(!b.refresh_drill_charts(), "a fresh chart is left be");
}
