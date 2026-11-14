//! KiCad 10's microvia stacks (`PCB_VIA_STACK`, UI-LEFT-3 X3-1) through kicad-cli: the stacks of KiCad 25ec2335's `qa/data/pcbnew/via_stacks.kicad_pcb` read, written
//! and read back by kicad-cli as they were; stacks placed here (stacked and staggered) kept by kicad-cli with their members; and kicad-cli's DRC judging them
//! (`DRC_TEST_PROVIDER_VIA_STACK`): a stack we placed is clean, one with a hop taken out has a malformed span, and a column of loose microvias the stack would have
//! filled is "not filled". Slow tier: kicad-cli runs, so it needs `EDA_SLOW_TESTS=1`; skipped, loudly, without kicad-cli or the QA board.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::{Design, Point};
use eda_model::units::mm;
use eda_model::via_extra::ViaKind;
use eda_model::via_stack::{MicroviaStack, ViaStackStyle};
use eda_ops::pcb_items::PcbItemCmd;
use eda_ops::{Board, Cmd};
use std::path::{Path, PathBuf};
use std::process::Command;

fn kicad_cli() -> Option<PathBuf> {
    let p = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("kicad-cli"));
    Command::new(&p).arg("version").output().ok().filter(|o| o.status.success()).map(|_| p)
}

fn qa_board() -> Option<String> {
    let qa = std::env::var_os("KICAD10_QA_DATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("KICAD10_SRC_DIR").map(|d| PathBuf::from(d).join("qa/data")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("src/kicad-src-25ec2335/qa/data")))?;
    std::fs::read_to_string(qa.join("pcbnew/via_stacks.kicad_pcb")).ok()
}

fn setup() -> Option<(PathBuf, String)> {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return None;
    }
    match (kicad_cli(), qa_board()) {
        (Some(cli), Some(text)) => Some((cli, text)),
        _ => {
            eprintln!("skipped: needs kicad-cli and KiCad 25ec2335's via_stacks.kicad_pcb (KICAD10_QA_DATA or KICAD10_SRC_DIR)");
            None
        }
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("eda_via_stack_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `kicad-cli pcb upgrade --force` on the board we wrote, read back.
fn through_kicad(cli: &Path, dir: &Path, text: &str) -> (Design, String) {
    let path = dir.join("board.kicad_pcb");
    std::fs::write(&path, text).unwrap();
    let out = Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "kicad-cli pcb upgrade: {}", String::from_utf8_lossy(&out.stderr));
    let back = std::fs::read_to_string(&path).unwrap();
    (import_kicad_pcb(&back).unwrap().0, back)
}

/// kicad-cli's DRC of the board we wrote: each violation's type and the uuids of its items.
fn drc(cli: &Path, dir: &Path, text: &str) -> Vec<(String, Vec<String>)> {
    let path = dir.join("drc.kicad_pcb");
    std::fs::write(&path, text).unwrap();
    let report = dir.join("drc.json");
    let out = Command::new(cli).args(["pcb", "drc", "--format", "json", "-o"]).arg(&report).arg(&path).output().unwrap();
    assert!(report.exists(), "kicad-cli pcb drc: {}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    json["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v["type"].as_str().unwrap_or("").to_string(), v["items"].as_array().map(|i| i.iter().filter_map(|x| x["uuid"].as_str().map(str::to_string)).collect()).unwrap_or_default()))
        .collect()
}

fn stacks(d: &Design) -> Vec<MicroviaStack> {
    d.drawings.as_ref().map(|d| d.board_items.via_stacks.clone()).unwrap_or_default()
}

/// What a stack is, its ids aside: the settings and where its members are (each via's place and span, each trace's ends and layer).
fn shape_of(d: &Design, s: &MicroviaStack) -> (MicroviaStack, Vec<String>) {
    let rt = d.routing.as_ref().unwrap();
    let mut members: Vec<String> = rt.vias.iter().filter(|v| s.member_ids.contains(&v.id)).map(|v| format!("via {:?} {}..{} {}x{}", v.at, v.from_layer, v.to_layer, v.diameter, v.drill)).collect();
    members.extend(rt.tracks.iter().filter(|t| s.member_ids.contains(&t.id)).map(|t| format!("trace {:?} {} {}", t.pts, t.layer, t.width)));
    members.sort();
    (MicroviaStack { id: String::new(), member_ids: Vec::new(), net: String::new(), ..s.clone() }, members)
}

fn p(x: f64, y: f64) -> Point {
    Point { x: mm(x), y: mm(y) }
}

/// The QA board's two stacks (a stacked F.Cu..In2.Cu one, a staggered In1.Cu..In3.Cu one with its trace) read here, written as `format( PCB_GENERATOR )` writes them
/// and read back by kicad-cli: the same stacks, the same members, every hop still a microvia.
#[test]
fn the_qa_boards_stacks_round_trip_through_kicad_cli() {
    let Some((cli, text)) = setup() else { return };
    let (design, model, _) = import_kicad_pcb(&text).unwrap();
    let ours = stacks(&design);
    assert_eq!(ours.len(), 2, "both stacks read");
    assert_eq!(ours.iter().map(|s| s.member_ids.len()).sum::<usize>(), 5);
    let written = export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-11-14", title: "via stacks" }).unwrap();
    assert_eq!(written.matches("(type via_stack)").count(), 2);
    assert!(!design.drawings.as_ref().unwrap().raw_generators.iter().any(|g| g.text.contains("via_stack")), "no longer kept as text");
    let dir = scratch("roundtrip");
    let (back, back_text) = through_kicad(&cli, &dir, &written);
    assert_eq!(back_text.matches("(type via_stack)").count(), 2, "kicad-cli kept both generators");
    let mut a: Vec<_> = ours.iter().map(|s| shape_of(&design, s)).collect();
    let mut b: Vec<_> = stacks(&back).iter().map(|s| shape_of(&back, s)).collect();
    a.sort_by(|x, y| x.0.start_layer.cmp(&y.0.start_layer));
    b.sort_by(|x, y| x.0.start_layer.cmp(&y.0.start_layer));
    assert_eq!(a, b, "kicad-cli read the stacks as we wrote them");
    let dr = back.drawings.as_ref().unwrap();
    for s in stacks(&back) {
        for v in back.routing.as_ref().unwrap().vias.iter().filter(|v| s.member_ids.contains(&v.id)) {
            assert_eq!(dr.via_extras.iter().find(|x| x.id == v.id).map(|x| x.kind), Some(ViaKind::Micro));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Stacks placed here (Place Microvia Stack's `placeStack`), a stacked F.Cu..In3.Cu one and a staggered In4.Cu..B.Cu... one on the QA board's six layers, are kept by
/// kicad-cli with their members; kicad-cli's DRC finds nothing wrong with their spans or fill; a hop taken out of a stack is a malformed span, reported on the stack; a
/// column of loose unfilled microvias is "Microvia under microvia not filled", where the stacked stack in the same place is not.
#[test]
fn placed_stacks_are_kept_and_judged_by_kicad_cli() {
    let Some((cli, text)) = setup() else { return };
    let (design, model, _) = import_kicad_pcb(&text).unwrap();
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    let place = |s: MicroviaStack| Cmd::PcbItem(PcbItemCmd::SetViaStack { id: None, stack: s, current: Default::default(), allow_drc_violations: true, hidden_layers: vec![] });
    let mut stacked = MicroviaStack::new(p(60.0, 20.0));
    stacked.start_layer = "F.Cu".into();
    stacked.end_layer = "In3.Cu".into();
    stacked.via_size_nm = mm(0.3);
    stacked.via_drill_nm = mm(0.1);
    stacked.capped = true;
    b.apply(&place(stacked)).unwrap();
    let mut staggered = MicroviaStack::new(p(60.0, 40.0));
    staggered.start_layer = "B.Cu".into();
    staggered.end_layer = "In2.Cu".into();
    staggered.style = ViaStackStyle::Staggered;
    staggered.pitch_nm = mm(0.6);
    staggered.filled = false;
    staggered.via_size_nm = mm(0.3);
    staggered.via_drill_nm = mm(0.1);
    staggered.hops = Some(vec![p(60.0, 40.0), p(60.6, 40.0), p(61.2, 40.0)]);
    b.apply(&place(staggered)).unwrap();
    let design = b.design().clone();
    let placed: Vec<MicroviaStack> = stacks(&design).into_iter().filter(|s| s.preset.is_empty()).collect();
    assert_eq!(placed.len(), 2);
    let written = export_kicad_pcb(&design, &model, &ExportMeta { date: "2026-11-14", title: "placed stacks" }).unwrap();
    let dir = scratch("placed");
    let (back, _) = through_kicad(&cli, &dir, &written);
    let mut a: Vec<_> = placed.iter().map(|s| shape_of(&design, s)).collect();
    let mut kept: Vec<_> = stacks(&back).iter().filter(|s| s.preset.is_empty()).map(|s| shape_of(&back, s)).collect();
    a.sort_by(|x, y| x.0.start_layer.cmp(&y.0.start_layer));
    kept.sort_by(|x, y| x.0.start_layer.cmp(&y.0.start_layer));
    assert_eq!(a, kept, "kicad-cli kept the placed stacks and their members");
    assert_eq!(a.iter().map(|x| x.1.len()).collect::<Vec<_>>(), [5, 3], "three hops on F.Cu..In3.Cu; three hops and two traces on B.Cu..In2.Cu");

    let stack_kinds = ["malformed_microvia_stack_span", "microvia_stack_not_filled"];
    let ours = |v: &[(String, Vec<String>)]| v.iter().filter(|(t, _)| stack_kinds.contains(&t.as_str())).cloned().collect::<Vec<_>>();
    let clean = ours(&drc(&cli, &dir, &written));
    assert!(clean.is_empty(), "the stacks placed here are well formed and filled: {clean:?}");

    // a hop taken out of the stacked stack: its span is no longer tiled
    let st = stacks(&design).into_iter().find(|s| s.start_layer == "F.Cu" && s.end_layer == "In3.Cu").unwrap();
    let middle = design.routing.as_ref().unwrap().vias.iter().find(|v| st.member_ids.contains(&v.id) && v.from_layer == "In1.Cu").unwrap().id.clone();
    let mut broken = Board::new(design.clone(), &model, mm(0.1), mm(0.3));
    broken.apply(&Cmd::DeleteVia { id: middle }).unwrap();
    let broken_text = export_kicad_pcb(broken.design(), &model, &ExportMeta { date: "2026-11-14", title: "broken stack" }).unwrap();
    let found = ours(&drc(&cli, &dir, &broken_text));
    let stack_uuid = broken_text.split("(generated").find(|g| g.contains("(start_layer \"F.Cu\")") && g.contains("(end_layer \"In3.Cu\")")).and_then(|g| g.split("(uuid \"").nth(1)).and_then(|u| u.split('"').next()).unwrap().to_string();
    assert!(found.iter().any(|(t, items)| t == "malformed_microvia_stack_span" && items.contains(&stack_uuid)), "kicad-cli reports the stack's malformed span on the stack: {found:?}");

    // loose microvias in a column, unfilled: kicad-cli's fill check (the stack would have filled them)
    let mut loose = Board::new(design.clone(), &model, mm(0.1), mm(0.3));
    for (a, z) in [("F.Cu", "In1.Cu"), ("In1.Cu", "In2.Cu")] {
        let via = Cmd::PlaceVia { net: String::new(), x: mm(80.0), y: mm(20.0), drill: mm(0.1), diameter: mm(0.3), from_layer: a.into(), to_layer: z.into(), split_track: None, allow_drc_violations: true, hidden_layers: vec![], barcode_shapes: Default::default() };
        loose.apply(&via).unwrap();
    }
    let mut d = loose.design().clone();
    let ids: Vec<String> = d.routing.as_ref().unwrap().vias.iter().filter(|v| v.at == p(80.0, 20.0)).map(|v| v.id.clone()).collect();
    for id in &ids {
        d.drawings.as_mut().unwrap().via_extras.push(eda_model::via_extra::ViaExtra { id: id.clone(), kind: ViaKind::Micro, tokens: vec!["(filling no)".into()] });
    }
    let loose_text = export_kicad_pcb(&d, &model, &ExportMeta { date: "2026-11-14", title: "loose column" }).unwrap();
    let found = ours(&drc(&cli, &dir, &loose_text));
    assert!(found.iter().any(|(t, _)| t == "microvia_stack_not_filled"), "kicad-cli reports the unfilled column: {found:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
