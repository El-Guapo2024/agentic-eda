//! A dielectric's sublayers through the board writer and reader (round 2 F17): `BOARD_STACKUP::FormatBoardStackup` writes each sublayer after the first behind an
//! `addsublayer`, with its colour, its thickness (and ` locked`), its material and its epsilon R and loss tangent as its type has them, and
//! `PCB_IO_KICAD_SEXPR_PARSER::parseBoardStackup` reads them back one by one. KiCad's own QA board with an `addsublayer`, `zone_filler`, is read from
//! `$KICAD_QA_DATA` (or `$KICAD_SRC_DIR/qa/data`) and skipped without it. That kicad-cli reads and rewrites them as written is proved in `eda-ops`'s
//! `stackup_kicad_cli` (slow).

use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::rules::{default_stackup, stackup_thickness_nm};
use eda_model::units::mm;
use eda_model::{Stackup, StackupSublayer};
use std::path::PathBuf;

fn qa(name: &str) -> Option<String> {
    let dir = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).or_else(|| std::env::var_os("KICAD_SRC_DIR").map(|d| PathBuf::from(d).join("qa/data")));
    let Some(text) = dir.and_then(|d| std::fs::read_to_string(d.join("pcbnew").join(name)).ok()) else {
        eprintln!("skipped: {name} needs KICAD_QA_DATA (or KICAD_SRC_DIR)");
        return None;
    };
    Some(text)
}

fn write(design: &eda_model::ir::Design, model: &eda_model::ConstraintModel) -> String {
    export_kicad_pcb(design, model, &ExportMeta { date: "2026-11-06", title: "stackup" }).expect("export")
}

fn stackup_of(text: &str) -> Stackup {
    import_kicad_pcb(text).expect("imports").1.stackup.expect("a stackup")
}

/// The `(stackup ..)` block of a written board, on one line.
fn block(text: &str) -> String {
    let at = text.find("(stackup").expect("a stackup block");
    text[at..].lines().take_while(|l| !l.trim_start().starts_with("(pad_to_mask_clearance")).collect::<Vec<_>>().join(" ")
}

#[test]
fn zone_fillers_sublayer_is_read_written_and_read_again_as_kicad_has_it() {
    let Some(text) = qa("zone_filler.kicad_pcb") else { return };
    let (design, model, _) = import_kicad_pcb(&text).expect("imports");
    let stackup = model.stackup.clone().expect("a stackup");
    let diel = stackup.layers.iter().find(|l| l.name == "dielectric 1").unwrap();
    // `(thickness 1.51) (material "FR4") (epsilon_r 4.5) (loss_tangent 0.02) addsublayer (thickness 0)`
    assert_eq!((diel.thickness_mm, diel.material.as_deref(), diel.epsilon_r, diel.loss_tangent), (Some(1.51), Some("FR4"), Some(4.5), Some(0.02)));
    assert_eq!(diel.sublayers, vec![StackupSublayer { thickness_mm: Some(0.0), ..Default::default() }], "one sublayer after the first, with a thickness of 0 and nothing else");
    let out = write(&design, &model);
    let b = block(&out);
    assert!(b.contains(r#"(layer "dielectric 1" (type "core") (thickness 1.51) (material "FR4") (epsilon_r 4.5) (loss_tangent 0.02) addsublayer (thickness 0))"#), "{b}");
    let again = stackup_of(&out);
    assert_eq!(again, stackup, "read, written and read again, every sublayer is as it was");
    assert_eq!(stackup_thickness_nm(&again), mm(0.01 + 0.035 + 1.51 + 0.035 + 0.01));
}

/// A stackup as Board Setup leaves it: a locked dielectric, sublayers with and without a material and a colour, a locked sublayer. Each sublayer writes what its type
/// has (a sublayer without a material writes no epsilon R or loss tangent, `HasMaterialValue`), and every value and lock reads back as it was.
#[test]
fn every_sublayers_values_and_lock_come_back() {
    let (design, mut model, _) = import_kicad_pcb(r#"(kicad_pcb (version 20241229) (generator "pcbnew") (layers (0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user)) (net 0 ""))"#).expect("imports");
    let mut stackup = default_stackup(4, mm(1.6));
    {
        let d1 = stackup.layers.iter_mut().find(|l| l.name == "dielectric 1").unwrap();
        d1.thickness_locked = true;
        d1.color = Some("FR4 natural".into());
        d1.sublayers = vec![
            StackupSublayer { material: Some("PTFE".into()), thickness_mm: Some(0.125), thickness_locked: true, epsilon_r: Some(2.1), loss_tangent: Some(0.0002), color: Some("#E0E0E0FF".into()) },
            StackupSublayer::added(),
        ];
        let d2 = stackup.layers.iter_mut().find(|l| l.name == "dielectric 2").unwrap();
        d2.sublayers = vec![StackupSublayer { material: Some("Not specified".into()), thickness_mm: Some(0.05), ..StackupSublayer::added() }];
    }
    model.board.board_thickness_nm = stackup_thickness_nm(&stackup);
    model.stackup = Some(stackup.clone());
    let out = write(&design, &model);
    let b = block(&out);
    for want in [
        r##"(layer "dielectric 1" (type "core") (color "FR4 natural") (thickness 0.48 locked) (material "FR4") (epsilon_r 4.5) (loss_tangent 0.02) addsublayer (color "#E0E0E0FF") (thickness 0.125 locked) (material "PTFE") (epsilon_r 2.1) (loss_tangent 0.0002) addsublayer (thickness 0))"##,
        r#"(layer "dielectric 2" (type "prepreg") (thickness 0.48) (material "FR4") (epsilon_r 4.5) (loss_tangent 0.02) addsublayer (thickness 0.05))"#,
    ] {
        assert!(b.contains(want), "missing {want}:\n{b}");
    }
    assert!(out.contains(&format!("(thickness {})", eda_model::units::to_mm(model.board.board_thickness_nm))), "the board's thickness counts the sublayers");
    let again = stackup_of(&out);
    let d1 = again.layers.iter().find(|l| l.name == "dielectric 1").unwrap();
    assert!(d1.thickness_locked);
    assert_eq!(
        d1.sublayers,
        vec![
            StackupSublayer { material: Some("PTFE".into()), thickness_mm: Some(0.125), thickness_locked: true, epsilon_r: Some(2.1), loss_tangent: Some(0.0002), color: Some("#E0E0E0FF".into()) },
            // `added()`'s epsilon R 1 and loss tangent 0 are not written without a material: they read back as not set, as KiCad's `DIELECTRIC_PRMS` defaults
            StackupSublayer { thickness_mm: Some(0.0), ..Default::default() },
        ]
    );
    let d2 = again.layers.iter().find(|l| l.name == "dielectric 2").unwrap();
    assert_eq!(d2.sublayers, vec![StackupSublayer { thickness_mm: Some(0.05), ..Default::default() }], "a material that is not specified is not written");
    assert_eq!(stackup_thickness_nm(&again), stackup_thickness_nm(&stackup));
    // written again, the file is the same
    let mut model2 = model.clone();
    model2.stackup = Some(again);
    assert_eq!(block(&write(&design, &model2)), b);
}
