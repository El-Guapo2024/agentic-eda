//! KiCad 10's via stitching template as Via Stitching Properties' Configure sets it, written here and read by kicad-cli: the template's type and annular rings and every via
//! the stitching makes a copy of the whole template (`PCB_VIA_STITCH::Update`'s `m_viaTemplate->Clone()`). KiCad 25ec2335's `stitch_fp_rulearea` (four copper layers, a GND
//! stitching over its pours) is the board. Slow tier: kicad-cli runs, so it needs `EDA_SLOW_TESTS=1`; skipped, loudly, without kicad-cli or the QA board.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::padstack::UnconnectedLayers;
use eda_model::units::mm;
use eda_model::via_extra::ViaKind;
use eda_ops::pcb_items::{PcbItemCmd, PlannedVia};
use eda_ops::{Board, Cmd};
use std::path::PathBuf;
use std::process::Command;

const QA_DEFAULT: &str = "/home/team_braneboard_com/src/kicad-src-25ec2335/qa/data";

fn kicad_cli() -> Option<PathBuf> {
    let p = std::env::var_os("EDA_KICAD_CLI").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("kicad-cli"));
    Command::new(&p).arg("version").output().ok().filter(|o| o.status.success()).map(|_| p)
}

/// The stitching's template set to connected layers only (Configure's Annular rings) and its vias made again from it: kicad-cli's `pcb upgrade --force` keeps the template and
/// the vias as we wrote them: the same vias where we put them, each with the template's `remove_unused_layers`.
#[test]
fn a_configured_template_and_its_vias_are_kept_by_kicad_cli() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let qa = std::env::var_os("KICAD10_QA_DATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(QA_DEFAULT));
    let (Some(cli), Ok(text)) = (kicad_cli(), std::fs::read_to_string(qa.join("pcbnew/stitch_fp_rulearea.kicad_pcb"))) else {
        eprintln!("skipped: needs kicad-cli and KiCad 25ec2335's stitch_fp_rulearea.kicad_pcb (KICAD10_QA_DATA)");
        return;
    };
    let (design, model, notes) = import_kicad_pcb(&text).unwrap();
    let entries = eda_kicad::zone_fills::entries_of_imported_file(&design, notes.zone_fills.clone());
    let fills = eda_kicad::zone_fills::KicadFills::new("file".into(), String::new(), 0, entries).fills_nm(&design);
    let mut stitch = design.drawings.as_ref().unwrap().board_items.via_stitches[0].clone();
    stitch.via.annular = Some(UnconnectedLayers::RemoveAll);
    let plan = eda_drc::via_stitch::plan(&eda_drc::via_stitch::PlanInputs { design: &design, model: &model, fills: &fills, current_via_size_nm: mm(0.6), current_via_drill_nm: mm(0.3) }, &stitch);
    let n = plan.vias.len();
    assert!(n > 0, "the stitching places vias");
    let id = stitch.id.clone();
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    b.apply(&Cmd::PcbItem(PcbItemCmd::SetViaStitch { id: Some(id), stitch: plan.stitch, vias: Some(plan.vias.into_iter().map(|(cell, at)| PlannedVia { cell, at }).collect()) })).unwrap();
    let ours = b.design().clone();
    let written = export_kicad_pcb(&ours, &model, &ExportMeta { date: "2026-11-06", title: "via stitch template" }).unwrap();
    assert!(written.contains("(templates (template (name \"via\") (via (at 0 0) (size 0.6) (drill 0.3) (layers \"F.Cu\" \"B.Cu\") (remove_unused_layers yes) (keep_end_layers no)"), "the template as format( PCB_TRACK ) writes it");

    let dir = std::env::temp_dir().join(format!("eda_via_stitch_template_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("board.kicad_pcb");
    std::fs::write(&path, &written).unwrap();
    let out = Command::new(&cli).args(["pcb", "upgrade", "--force"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "kicad-cli pcb upgrade: {}", String::from_utf8_lossy(&out.stderr));
    let (kicad, _, _) = import_kicad_pcb(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let st = &kicad.drawings.as_ref().unwrap().board_items.via_stitches[0];
    let template = eda_kicad::via_template_extra(&st.via);
    assert_eq!((template.kind, template.unconnected_layers()), (ViaKind::Through, UnconnectedLayers::RemoveAll), "kicad-cli kept the template's annular rings");
    assert_eq!(st.member_ids.len(), n, "and the {n} vias");
    let dr = kicad.drawings.as_ref().unwrap();
    for m in &st.member_ids {
        let x = dr.via_extras.iter().find(|x| &x.id == m).expect("a member via says more than a plain via");
        assert_eq!(x.unconnected_layers(), UnconnectedLayers::RemoveAll, "each via is a copy of the template");
    }
    let pos = |d: &eda_model::ir::Design, ids: &[String]| {
        let mut v: Vec<(i64, i64)> = d.routing.as_ref().unwrap().vias.iter().filter(|v| ids.contains(&v.id)).map(|v| (v.at.x.get(), v.at.y.get())).collect();
        v.sort();
        v
    };
    let ours_ids = &ours.drawings.as_ref().unwrap().board_items.via_stitches[0].member_ids;
    assert_eq!(pos(&kicad, &st.member_ids), pos(&ours, ours_ids), "where we put them");
    eprintln!("stitch_fp_rulearea: {n} vias of connected layers only, kept by kicad-cli");
}
