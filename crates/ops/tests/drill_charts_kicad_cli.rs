//! KiCad 10's drill charts built here as kicad-cli builds them: `pcb upgrade --force` rebuilds every stale chart of a board (`RefreshGeneratedTables`, `PCB_DRILL_CHART::generate`,
//! `PCB_GENERATED_TABLE::RebuildCells`), so the chart kicad-cli writes for a board is the chart `eda_ops::drill_chart` must build for it -- the rows, their text, alignment and
//! order, which cells keep their looks, the widths and heights `Autosize` gives them, the totals, the marks and the profile. KiCad 25ec2335's `api_kitchen_sink` is the board.
//! Slow tier: kicad-cli runs, so it needs `EDA_SLOW_TESTS=1`; skipped, loudly, without kicad-cli or the QA board.

use eda_model::units::Nm;
use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::drill_chart::{DrillChart, DrillChartFilter, TableUnits};
use eda_model::ir::{Design, Point};
use eda_model::table::Table;
use eda_model::ConstraintModel;
use eda_ops::pcb_items::PcbItemCmd;
use eda_ops::{Board, Cmd, TableCmd};
use std::path::{Path, PathBuf};
use std::process::Command;
use eda_model::units::mm;

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

/// The two charts are one: the same rows, cells (text, alignment, size and margins of the text), keys, marks and profile, the same corner, and the same widths and heights
/// to 2 nm (KiCad lays them out in nanometres from margins of 1.0025 mm, as `autosize` does, `TableCell::margin_nm`; the box of its text's ink is the box of an integer
/// polygon, which `autosize` measures on the curve).
fn assert_same_chart(tag: &str, ours: &Design, kicad: &Design) {
    let ((t, c), (kt, kc)) = (chart_of(ours), chart_of(kicad));
    let cells = |t: &Table| t.cells.iter().map(|x| (x.text.clone(), x.style.h_align, x.style.height_nm, x.margins)).collect::<Vec<_>>();
    assert_eq!(cells(&t), cells(&kt), "{tag}: the cells");
    assert_eq!((t.col_count(), t.row_count(), t.at), (kt.col_count(), kt.row_count(), kt.at), "{tag}: the grid");
    let near = |a: &[Nm], b: &[Nm]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (*x - *y).abs() <= Nm::new(2));
    assert!(near(&t.col_widths, &kt.col_widths), "{tag}: column widths against kicad-cli's: {:?} vs {:?}", t.col_widths, kt.col_widths);
    assert!(near(&t.row_heights, &kt.row_heights), "{tag}: row heights against kicad-cli's: {:?} vs {:?}", t.row_heights, kt.row_heights);
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
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::Table(TableCmd::Delete { id: old.id })).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().board_items.drill_charts.is_empty(), "the chart goes with its table");
    b.apply(&Cmd::PcbItem(PcbItemCmd::PlaceDrillChart { layer: "Dwgs.User".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO } })).unwrap();
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
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
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
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    let net = m.nets.first().map(|n| n.name.clone()).unwrap_or_default();
    b.apply(&Cmd::AddVia { net, x: mm(170.0), y: mm(80.0), drill: mm(0.8), diameter: mm(1.4), from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }).unwrap();
    // KiCad's: the board with the via and the chart as it was, rebuilt by kicad-cli.
    let (kicad, _) = upgraded(&cli, &export(b.design(), &m), "new_hole");
    assert!(b.refresh_drill_charts(), "the chart is stale");
    assert_same_chart("new hole", b.design(), &kicad);
    let (t, c) = chart_of(b.design());
    assert_eq!(t.cells.iter().skip(1).step_by(2).map(|c| c.text.as_str()).collect::<Vec<_>>(), ["Drill (mm)", "0.30", "0.80", "1.14", "1.52"]);
    assert_eq!(c.row_shapes.get(&2), Some(&3), "the lowest shape no group has");
    assert!(!b.refresh_drill_charts(), "a fresh chart is left be");
}

/// A chart that was read is stale whatever its cells say (`m_builtGeneration` 0 against the board's drill generation): one cell's margins changed by hand in the file, its text
/// still right, the first refresh lays the chart out again (`Autosize`) as kicad-cli's `pcb upgrade` does, and then leaves it be.
#[test]
fn a_chart_that_was_read_is_rebuilt_even_when_its_text_is_right() {
    if slow_tests_off() {
        return;
    }
    let (Some(cli), Some(text)) = (kicad_cli(), kitchen_sink()) else {
        eprintln!("skipped: needs kicad-cli and KiCad 25ec2335's api_kitchen_sink.kicad_pcb (KICAD10_QA_DATA)");
        return;
    };
    let cell = "(table_cell \"0.30\"\n\t\t\t\t(start 41 57.639294)\n\t\t\t\t(end 57 59.764294)\n\t\t\t\t(margins 0.5 0.5 0.5 0.5)";
    let edited = text.replacen(cell, &cell.replace("(margins 0.5 0.5 0.5 0.5)", "(margins 1.5 1.5 1.5 1.5)"), 1);
    assert_ne!(edited, text, "the kitchen sink's chart has this cell");
    let (kicad, _) = upgraded(&cli, &edited, "margins");
    let (d, m, _) = import_kicad_pcb(&edited).unwrap();
    let (before, _) = chart_of(&d);
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    assert!(b.refresh_drill_charts(), "a chart that was read is stale");
    assert_same_chart("margins", b.design(), &kicad);
    assert_ne!(chart_of(b.design()).0.row_heights, before.row_heights, "laid out again");
    assert!(!b.refresh_drill_charts(), "built: fresh");
}

/// A four-layer board whose copper layers have names of their own (`Top`, `GND plane`), a via of an imperial drill (0.3048 mm, 12 mil), vias with a backdrill from either side
/// and a tertiary drill, vias with front and back post-machining, a buried via, and a chart of ten columns (the operation, the protection, the backdrill stub and the aspect ratio
/// among them): KiCad 25ec2335's `EnumerateDrillSpans` gives the backdrill spans, `enumerateVia` their operations (kind 1 or 2, not plated, the stub from `GetDrillDepth`) and the
/// primary drills' post-machining, `GroupKeyString` the `|k` and `|m` of each, `spanText` the board's layer names.
const BACKDRILL_BOARD: &str = include_str!("drill_chart_backdrills.kicad_pcb");

/// The board's chart refreshed here, against the chart kicad-cli's `pcb upgrade` builds for the same file (a chart read from a file is always stale: `m_builtGeneration` 0).
fn refreshed_against_kicad(cli: &Path, text: &str, tag: &str) -> Design {
    let (kicad, _) = upgraded(cli, text, tag);
    let (d, m, _) = import_kicad_pcb(text).unwrap();
    let mut b = Board::new(d, &m, mm(0.1), mm(0.3));
    assert!(b.refresh_drill_charts(), "{tag}: a chart that was read is stale");
    assert_same_chart(tag, b.design(), &kicad);
    b.design().clone()
}

fn rows(d: &Design) -> Vec<Vec<String>> {
    let (t, _) = chart_of(d);
    t.cells.chunks(t.col_count()).map(|r| r.iter().map(|c| c.text.clone()).collect()).collect()
}

#[test]
fn backdrills_post_machining_layer_names_and_imperial_holes_are_charted_as_kicad_charts_them() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = kicad_cli() else {
        eprintln!("skipped: needs kicad-cli");
        return;
    };
    let d = refreshed_against_kicad(&cli, BACKDRILL_BOARD, "backdrills");
    let r = rows(&d);
    let col = |r: &[String], i: usize| r[i].clone();
    // The imperial drill keyed and printed from its nanometres.
    assert_eq!(chart_of(&d).1.row_keys.get(&2).map(String::as_str), Some("v1|d304800|s-|p1|L0-31|k0|m0:0:0:0/0:0:0:0"));
    assert_eq!((col(&r[2], 1), col(&r[2], 5)), ("0.305".to_string(), "Top / B.Cu".to_string()));
    // The post-machined vias: a row each, keyed by their post-machining.
    let keys: Vec<String> = chart_of(&d).1.row_keys.values().cloned().collect();
    assert!(keys.contains(&"v1|d400000|s-|p1|L0-31|k0|m3:900000:0:900/0:0:0:0".to_string()) && keys.contains(&"v1|d400000|s-|p1|L0-31|k0|m0:0:0:0/2:1000000:300000:0".to_string()), "{keys:?}");
    // The backdrills: not plated, after the plated rows, their stub and the "Backdrill" operation.
    assert_eq!(r[6][1..].to_vec(), ["0.600", "No", "1", "1", "Top / GND plane", "Backdrill", "\u{2014}", "0.790", "0.3:1"]);
    assert_eq!((col(&r[8], 1), col(&r[8], 5), col(&r[8], 8)), ("0.650".to_string(), "In2.Cu / B.Cu".to_string(), "1.495".to_string()));
    assert_eq!(r[9][0], "9 OPS / 6 SITES");

    // Without a stackup in the file: KiCad's default stackup gives the stubs and the aspect ratios.
    let start = BACKDRILL_BOARD.find("\t\t(stackup").unwrap();
    let end = BACKDRILL_BOARD.find("\t\t(pad_to_mask_clearance").unwrap();
    let bare = format!("{}{}", &BACKDRILL_BOARD[..start], &BACKDRILL_BOARD[end..]);
    refreshed_against_kicad(&cli, &bare, "backdrills_default_stackup");

    // Backdrills filtered out (`(backdrill no)`): only the primary drills.
    let filtered = BACKDRILL_BOARD.replacen("(column_count 10)", "(filter (plated yes) (npth yes) (vias yes) (slots yes) (backdrill no) (castellated yes))\n\t\t(column_count 10)", 1);
    let d = refreshed_against_kicad(&cli, &filtered, "backdrills_filtered");
    assert_eq!(rows(&d).len(), 1 + 5 + 1);
}
