//! A stackup with sublayers set through `Cmd::SetStackup` (Board Setup > Physical Stackup's Add Dielectric Layer, the thickness locks, a sublayer's own material),
//! written to the derived board and rewritten by kicad-cli (`pcb upgrade --force` loads the board with `PCB_IO_KICAD_SEXPR_PARSER::parseBoardStackup` and saves
//! it with `BOARD_STACKUP::FormatBoardStackup`): every sublayer comes back as it was written, and so do the board thickness and the impedance control. Then fewer
//! copper layers with `delete_items_on_removed_layers`: the inner layer's track is gone and the 2-layer board loads in kicad-cli.
//! Slow tier: kicad-cli runs, so it needs `EDA_SLOW_TESTS=1`; skipped, loudly, without kicad-cli.

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::Design;
use eda_model::rules::{stackup_thickness_nm, RulesOverlay, StackupSettings};
use eda_model::units::mm;
use eda_model::{ConstraintModel, Stackup, StackupSublayer};
use eda_ops::{Board, Cmd};
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// A 4-layer board with an outline, a track on F.Cu and one on In1.Cu.
const BOARD: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
	(general (thickness 1.6) (legacy_teardrops no))
	(paper "A4")
	(layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
	(setup (pad_to_mask_clearance 0))
	(net 0 "") (net 1 "GND")
	(gr_rect (start 90 90) (end 130 130) (stroke (width 0.1) (type solid)) (fill no) (layer "Edge.Cuts") (uuid "11111111-1111-1111-1111-111111111111"))
	(segment (start 100 100) (end 110 100) (width 0.25) (layer "F.Cu") (net 1) (uuid "22222222-2222-2222-2222-222222222222"))
	(segment (start 100 105) (end 110 105) (width 0.25) (layer "In1.Cu") (net 1) (uuid "33333333-3333-3333-3333-333333333333"))
)
"#;

/// The model the loader would hand back after the edits: the overlay laid over the board's own.
fn effective(b: &Board, base: &ConstraintModel) -> ConstraintModel {
    let mut m = base.clone();
    b.design().drawings.as_ref().and_then(|d| d.rules.clone()).unwrap_or_else(RulesOverlay::default).apply(&mut m);
    m
}

/// `text` as kicad-cli writes it back, read again.
fn kicad_rewrite(cli: &Path, text: &str, tag: &str) -> (Design, ConstraintModel, String) {
    let dir = std::env::temp_dir().join(format!("eda_stackup_kicad_cli_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("board.kicad_pcb");
    std::fs::write(&path, text).unwrap();
    let out = Command::new(cli).args(["pcb", "upgrade", "--force"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "{tag}: kicad-cli pcb upgrade: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let theirs = std::fs::read_to_string(&path).unwrap();
    let (d, m, _) = import_kicad_pcb(&theirs).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    (d, m, theirs)
}

fn export(d: &Design, m: &ConstraintModel) -> String {
    export_kicad_pcb(d, m, &ExportMeta { date: "2026-11-06", title: "stackup" }).unwrap()
}

/// What the page sends after Add Dielectric Layer on Dielectric 1 (twice) and Dielectric 2, a lock and a sublayer's own material and colour.
fn with_sublayers() -> StackupSettings {
    let mut s = StackupSettings::new(4, mm(1.6));
    s.stackup.dielectric_constraints = true;
    let d1 = s.stackup.layers.iter_mut().find(|l| l.name == "dielectric 1").unwrap();
    d1.thickness_locked = true;
    d1.sublayers = vec![
        StackupSublayer { material: Some("PTFE".into()), thickness_mm: Some(0.125), thickness_locked: true, epsilon_r: Some(2.1), loss_tangent: Some(0.0002), color: Some("PTFE natural".into()) },
        StackupSublayer { thickness_mm: Some(0.05), ..StackupSublayer::added() },
    ];
    let d2 = s.stackup.layers.iter_mut().find(|l| l.name == "dielectric 2").unwrap();
    d2.sublayers = vec![StackupSublayer { material: Some("FR4".into()), thickness_mm: Some(0.2), epsilon_r: Some(4.6), loss_tangent: Some(0.015), ..Default::default() }];
    s
}

/// Each layer as the file says it: its name, type, and every sublayer's material, thickness (to the nanometre), lock, epsilon R and loss tangent when a material
/// is set (they are written only then), and colour.
fn as_written(s: &Stackup) -> Vec<(String, Option<String>, Vec<(Option<String>, i64, bool, Option<(f64, f64)>, Option<String>)>)> {
    s.layers
        .iter()
        .map(|l| {
            let subs = (0..l.sublayer_count())
                .filter_map(|i| l.sublayer(i))
                .map(|x| {
                    let material = x.material.clone().filter(|m| eda_model::stackup_prm_specified(Some(m)));
                    let values = material.as_ref().filter(|_| l.has_epsilon_r_value()).map(|_| (x.epsilon_r.unwrap_or(1.0), x.loss_tangent.unwrap_or(0.0)));
                    (material, (x.thickness_mm.unwrap_or(0.0) * 1e6).round() as i64, x.thickness_locked, values, x.color.clone())
                })
                .collect();
            (l.name.clone(), l.kind.clone(), subs)
        })
        .collect()
}

#[test]
fn kicad_cli_rewrites_the_sublayers_set_through_the_verb_as_they_were_written() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = kicad_cli() else {
        eprintln!("skipped: needs kicad-cli");
        return;
    };
    let (design, base, _) = import_kicad_pcb(BOARD).unwrap();
    let mut b = Board::new(design, &base, mm(0.1), mm(0.3));
    let settings = with_sublayers();
    b.apply(&Cmd::SetStackup { settings: settings.clone(), delete_items_on_removed_layers: false }).unwrap();
    let model = effective(&b, &base);
    let ours = export(b.design(), &model);
    assert!(ours.contains("addsublayer (color \"PTFE natural\") (thickness 0.125 locked) (material \"PTFE\") (epsilon_r 2.1) (loss_tangent 0.0002) addsublayer (thickness 0.05))"), "{ours}");

    let (_, kicad, theirs) = kicad_rewrite(&cli, &ours, "sublayers");
    let back = kicad.stackup.clone().expect("kicad-cli kept the stackup");
    assert_eq!(as_written(&back), as_written(&settings.stackup), "kicad-cli's rewrite:\n{}", &theirs[..theirs.len().min(4000)]);
    assert!(back.dielectric_constraints, "(dielectric_constraints yes)");
    assert_eq!(kicad.board.board_thickness_nm, stackup_thickness_nm(&settings.stackup), "the board thickness counts every sublayer, in KiCad too");
    let d1 = back.layers.iter().find(|l| l.name == "dielectric 1").unwrap();
    assert_eq!((d1.sublayer_count(), d1.thickness_locked, d1.sublayers[0].thickness_locked), (3, true, true));
    eprintln!("kicad-cli kept {} sublayers", back.layers.iter().map(|l| l.sublayers.len()).sum::<usize>());
}

#[test]
fn fewer_copper_layers_with_the_flag_delete_the_inner_track_and_kicad_cli_loads_the_board() {
    if slow_tests_off() {
        return;
    }
    let Some(cli) = kicad_cli() else {
        eprintln!("skipped: needs kicad-cli");
        return;
    };
    let (design, base, _) = import_kicad_pcb(BOARD).unwrap();
    assert_eq!(base.board.layers.len(), 4);
    let mut b = Board::new(design, &base, mm(0.1), mm(0.3));
    let two = StackupSettings::new(2, mm(1.6));
    let refused = b.apply(&Cmd::SetStackup { settings: two.clone(), delete_items_on_removed_layers: false }).unwrap_err();
    assert_eq!(refused[0].location.as_deref(), Some("In1.Cu"));
    b.apply(&Cmd::SetStackup { settings: two, delete_items_on_removed_layers: true }).unwrap();
    let tracks: Vec<String> = b.design().routing.as_ref().unwrap().tracks.iter().map(|t| t.layer.clone()).collect();
    assert_eq!(tracks, ["F.Cu"], "the track on In1.Cu is deleted, the one on F.Cu stays");
    let model = effective(&b, &base);
    let (d, m, theirs) = kicad_rewrite(&cli, &export(b.design(), &model), "fewer");
    assert_eq!(m.board.layers, ["F.Cu", "B.Cu"], "kicad-cli's board has two copper layers:\n{}", &theirs[..theirs.len().min(2000)]);
    assert_eq!(d.routing.as_ref().unwrap().tracks.iter().map(|t| t.layer.as_str()).collect::<Vec<_>>(), ["F.Cu"]);
}
