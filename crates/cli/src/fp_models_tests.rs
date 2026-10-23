//! The 3D models of a footprint through a board directory (GAPS.md package P14, item 30): what the loader lays over the footprint, what the state sends the 3D view, what the
//! derived `.kicad_pcb` carries, and what one undo step takes back. The slow tier hands the board to kicad-cli.

use super::tests::{scratch, setup};
use super::{load, redo, step, undo};
use eda_model::footprint::Model3d;
use eda_ops::{Cmd, Domain};
use std::path::Path;

const LIB: &str = "${KICAD10_3DMODEL_DIR}/Test.3dshapes/Lib.step";

/// The two models of the proof: placed, scaled, turned, one of them half transparent and one hidden.
fn two_models() -> Vec<Model3d> {
    vec![
        Model3d { path: "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.step".into(), offset: [1.5, -0.5, 0.25], scale: [1.0, 1.25, 1.0], rotate: [0.0, 0.0, 90.0], opacity: 0.65, show: true },
        Model3d { path: "${KICAD10_3DMODEL_DIR}/Capacitor_SMD.3dshapes/C_0603_1608Metric.step".into(), offset: [-1.0, 0.5, 1.0], scale: [1.0, 1.0, 1.0], rotate: [90.0, 0.0, -30.0], opacity: 1.0, show: false },
    ]
}

/// `setup`'s board with the one model `LIB` on the footprint both parts are on, as a library footprint has it.
fn board_with_library_model(name: &str) -> std::path::PathBuf {
    let dir = scratch(name);
    setup(&dir);
    let intent = dir.join("intent.yaml");
    let mut model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent).unwrap()).unwrap();
    eda_model::fp_models::set_models(&mut model.footprints[0], &[Model3d::identity(LIB)]);
    std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
    dir
}

fn state(dir: &Path) -> serde_json::Value {
    let job: std::sync::Arc<std::sync::Mutex<String>> = Default::default();
    crate::studio::state(dir, &job, crate::text_vars_json::clock_of("/api/state")).unwrap()
}

/// The models the 3D view is sent for `reference`, or `None` when the part has no `models` key.
fn sent(dir: &Path, reference: &str) -> Option<Vec<Model3d>> {
    let s = state(dir);
    let part = s["parts"].as_array().unwrap().iter().find(|p| p["ref"] == reference).unwrap().clone();
    part.get("models").map(|list| {
        list.as_array()
            .unwrap()
            .iter()
            .map(|m| {
                let v3 = |k: &str| -> [f64; 3] { let a = m[k].as_array().unwrap(); [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()] };
                Model3d { path: m["name"].as_str().unwrap().to_string(), offset: v3("offset"), scale: v3("scale"), rotate: v3("rotate"), opacity: m["opacity"].as_f64().unwrap(), show: m["show"].as_bool().unwrap() }
            })
            .collect()
    })
}

fn derived_board(dir: &Path) -> String {
    let (_, design, model) = load(dir).unwrap();
    eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-12", title: "models" }).unwrap()
}

/// The `(footprint ..)` block of the board file that holds `(property "Reference" "<reference>"`.
fn footprint_block<'a>(pcb: &'a str, reference: &str) -> &'a str {
    let at = pcb.find(&format!("(property \"Reference\" \"{reference}\"")).unwrap_or_else(|| panic!("{reference} is written:\n{pcb}"));
    let start = pcb[..at].rfind("\n\t(footprint ").unwrap() + 1;
    let end = pcb[at..].find("\n\t(footprint ").map_or(pcb.len(), |i| at + i);
    &pcb[start..end]
}

#[test]
fn a_models_list_set_on_a_footprint_reaches_the_model_the_3d_view_and_the_board_file_and_one_undo_takes_it_back() {
    let dir = scratch("models_list_end_to_end");
    setup(&dir);
    assert_eq!(sent(&dir, "U1"), None, "a footprint with no model sends none: the view draws the body box");

    step(&dir, Cmd::SetBoardModels { part: "U1".into(), models: two_models() }, false, "test").unwrap();
    // the model answers with the list on that footprint, and the other footprint of the same library entry is as it was
    let (_, design, model) = load(&dir).unwrap();
    let held = |r: &str| model.footprint_of(model.part(r).unwrap()).unwrap().models3d;
    assert_eq!(held("U1"), two_models());
    assert_eq!(held("U2"), vec![]);
    assert_eq!(design.footprint_edit("U1").unwrap().models3d, Some(two_models()));
    // the 3D view is sent the list with its placement (hidden ones too: the page decides)
    assert_eq!(sent(&dir, "U1"), Some(two_models()));
    assert_eq!(sent(&dir, "U2"), None);

    // the derived board file writes each as KiCad does, in order, and reads back as the list
    let pcb = derived_board(&dir);
    let block = footprint_block(&pcb, "U1");
    assert_eq!(block.matches("(model ").count(), 2, "{block}");
    assert!(block.contains("(opacity 0.6500)") && block.contains("(hide yes)") && block.contains("(xyz 1.5 -0.5 0.25)") && block.contains("(xyz 1 1.25 1)") && block.contains("(xyz 0 0 90)"), "{block}");
    assert!(block.find("R_0603_1608Metric").unwrap() < block.find("C_0603_1608Metric").unwrap(), "in the order of the list");
    assert!(!footprint_block(&pcb, "U2").contains("(model "), "the other footprint has none");
    let (_, back, _) = eda_kicad::import_kicad_pcb(&pcb).unwrap();
    let u1 = back.parts.iter().find(|p| p.reference == "U1").unwrap();
    assert_eq!(back.footprint_of(u1).unwrap().models3d, two_models());

    // one command, one undo step: the add, the edit, the move and the remove each come back on their own
    step(&dir, Cmd::AddBoardModel { part: "U1".into(), model: Model3d::identity("third.step"), at: None }, false, "test").unwrap();
    step(&dir, Cmd::EditBoardModel { part: "U1".into(), index: 2, path: None, offset: Some([0.0, 0.0, 2.0]), scale: None, rotate: None, opacity: None, show: None }, false, "test").unwrap();
    step(&dir, Cmd::MoveBoardModel { part: "U1".into(), from: 2, to: 0 }, false, "test").unwrap();
    step(&dir, Cmd::RemoveBoardModel { part: "U1".into(), index: 1 }, false, "test").unwrap();
    let names = |dir: &Path| sent(dir, "U1").unwrap().iter().map(|m| m.path.rsplit('/').next().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(names(&dir), ["third.step", "C_0603_1608Metric.step"]);
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(names(&dir), ["third.step", "R_0603_1608Metric.step", "C_0603_1608Metric.step"], "the remove");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(names(&dir), ["R_0603_1608Metric.step", "C_0603_1608Metric.step", "third.step"], "the move");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(sent(&dir, "U1").unwrap()[2].offset, [0.0, 0.0, 0.0], "the edit");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(sent(&dir, "U1"), Some(two_models()), "the add");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(sent(&dir, "U1"), None, "the whole list");
    assert!(load(&dir).unwrap().1.footprint_edit("U1").is_none());
    redo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(sent(&dir, "U1"), Some(two_models()));
}

#[test]
fn a_refused_model_command_is_no_undo_step() {
    let dir = scratch("models_refusals_leave_no_step");
    setup(&dir);
    step(&dir, Cmd::AddBoardModel { part: "U1".into(), model: Model3d::identity("a.step"), at: None }, false, "test").unwrap();
    let before = sent(&dir, "U1");
    for (cmd, check) in [
        (Cmd::RemoveBoardModel { part: "U1".into(), index: 5 }, "ops_bad_model_index"),
        (Cmd::AddBoardModel { part: "U1".into(), model: Model3d::identity("bad?.step"), at: None }, "ops_bad_model"),
        (Cmd::SetBoardModels { part: "U1".into(), models: vec![Model3d::identity("a.step")] }, "ops_nothing_to_change"),
        (Cmd::SetBoardModels { part: "U9".into(), models: vec![] }, "ops_not_placed"),
    ] {
        let e = step(&dir, cmd, false, "test").unwrap_err();
        assert_eq!(e[0].check, check, "{e:?}");
    }
    assert_eq!(sent(&dir, "U1"), before);
    // the last step on the stack is still the add: one undo takes it, and there is nothing more
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(sent(&dir, "U1"), None);
}

#[test]
fn a_footprint_the_person_took_every_model_off_has_none_and_not_the_librarys() {
    let dir = board_with_library_model("models_emptied_stays_empty");
    assert_eq!(sent(&dir, "U1").unwrap().iter().map(|m| m.path.as_str()).collect::<Vec<_>>(), [LIB]);
    step(&dir, Cmd::SetBoardModels { part: "U1".into(), models: vec![] }, false, "test").unwrap();
    assert_eq!(sent(&dir, "U1"), Some(vec![]), "an emptied list is sent empty, not as the library's");
    assert_eq!(sent(&dir, "U2").unwrap().len(), 1, "the other footprint keeps the library's model");
    let pcb = derived_board(&dir);
    assert!(!footprint_block(&pcb, "U1").contains("(model "), "no model is written");
    assert!(footprint_block(&pcb, "U2").contains(LIB), "the library's is");
    // read back, the board's footprint has none, and the part is not given the library's again
    let (_, back, _) = eda_kicad::import_kicad_pcb(&pcb).unwrap();
    let u1 = back.parts.iter().find(|p| p.reference == "U1").unwrap();
    assert!(back.footprint_of(u1).unwrap().models3d.is_empty());
    // the library's own list brings the footprint's edit away
    step(&dir, Cmd::SetBoardModels { part: "U1".into(), models: vec![Model3d::identity(LIB)] }, false, "test").unwrap();
    assert!(load(&dir).unwrap().1.footprint_edit("U1").is_none());
}

/// Moving, turning and flipping the footprint carry its models with it (they are the footprint's), and a bottom-side footprint's are written in KiCad's frame.
#[test]
fn the_models_follow_their_footprint_and_a_bottom_one_is_written_in_kicads_frame() {
    let dir = scratch("models_follow_footprint");
    setup(&dir);
    let placed = Model3d { offset: [1.0, 2.0, 0.5], rotate: [90.0, 0.0, 30.0], ..Model3d::identity(LIB) };
    step(&dir, Cmd::SetBoardModels { part: "U1".into(), models: vec![placed.clone()] }, false, "test").unwrap();
    step(&dir, Cmd::MoveTo { part: "U1".into(), x: 8_000, y: 8_000 }, false, "test").unwrap();
    assert_eq!(sent(&dir, "U1"), Some(vec![placed.clone()]), "moved: the same placement on the footprint");
    step(&dir, Cmd::FlipItems { ids: vec!["U1".into()], pivot: eda_model::ir::Point { x: 8_000, y: 8_000 }, direction: eda_ops::FlipDirection::LeftRight }, false, "test").unwrap();
    assert_eq!(sent(&dir, "U1"), Some(vec![placed.clone()]), "flipped: the footprint's list is the same, in the frame of the footprint on the top side");
    let pcb = derived_board(&dir);
    let block = footprint_block(&pcb, "U1");
    assert!(block.contains("(xyz -1 -2 0.5)") && block.contains("(xyz 90 0 -150)"), "the file's bottom-side frame: the half turn about z: {block}");
    let (_, back, _) = eda_kicad::import_kicad_pcb(&pcb).unwrap();
    let u1 = back.parts.iter().find(|p| p.reference == "U1").unwrap();
    assert_eq!(back.footprint_of(u1).unwrap().models3d, vec![placed], "read back, the list is the one it was written from");
}

/// The Footprint Editor's list reaches the board once the footprint is updated on it (`UpdateFootprintOnBoard`: "Update Footprint from Library"), for every instance, and an
/// instance whose own list the board set keeps that one.
#[test]
fn a_list_set_in_the_footprint_editor_reaches_the_board_with_the_footprint_and_a_boards_own_list_wins() {
    let dir = scratch("models_from_the_editor");
    setup(&dir);
    step(&dir, Cmd::OpenFootprintForEdit { name: "2PAD".into() }, false, "test").unwrap();
    let list = two_models();
    step(
        &dir,
        Cmd::EditFootprintProperties {
            name: "2PAD".into(),
            description: String::new(),
            keywords: String::new(),
            attributes: Default::default(),
            reference_visible: true,
            value_visible: true,
            model: None,
            models3d: Some(list.clone()),
            zone_connection: None,
            reference: None,
            value: None,
            fields: None,
            datasheet: None,
            description_field: None,
        },
        false,
        "test",
    )
    .unwrap();
    assert_eq!(sent(&dir, "U1"), None, "the library entry is not on the board until it is published: nothing automatic");
    step(&dir, Cmd::UpdateFootprintOnBoard { name: "2PAD".into() }, false, "test").unwrap();
    assert_eq!(sent(&dir, "U1"), Some(list.clone()), "the footprint's models come with it");
    assert_eq!(sent(&dir, "U2"), Some(list.clone()));
    let (_, _, model) = load(&dir).unwrap();
    assert_eq!(model.footprint_of(model.part("U2").unwrap()).unwrap().model.as_deref(), Some(list[0].path.as_str()), "`model` is the first path");
    // the derived board file writes both models for both instances
    let pcb = derived_board(&dir);
    assert_eq!(footprint_block(&pcb, "U1").matches("(model ").count(), 2);
    assert_eq!(footprint_block(&pcb, "U2").matches("(model ").count(), 2);

    // a list the board set for one instance is that instance's, the library's is the other's
    let own = vec![Model3d::identity("mine.step")];
    step(&dir, Cmd::SetBoardModels { part: "U1".into(), models: own.clone() }, false, "test").unwrap();
    assert_eq!((sent(&dir, "U1"), sent(&dir, "U2")), (Some(own), Some(list)));
}

/// The footprint a library entry becomes when it is placed on the board from the project library keeps its models (`to_engine_footprint`): Place Footprint and Insert into PCB.
#[test]
fn a_footprint_placed_from_the_project_library_brings_its_models() {
    let dir = scratch("models_placed_from_library");
    setup(&dir);
    let mut fp = eda_model::ir::LibraryFootprint::new_empty("Test:Mounted");
    fp.pads.push(eda_model::ir::LibraryPad {
        id: String::new(),
        number: "1".into(),
        at: eda_model::ir::Point { x: 0, y: 0 },
        offset: eda_model::ir::Point::default(),
        size: (1_000, 1_000),
        shape: eda_model::ir::LibraryPadShape::Rect,
        kind: eda_model::footprint::PadKind::Smd,
        drill: None,
        drill_slot: None,
        rot: 0,
        roundrect_ratio: None,
        trapezoid_delta: None,
        chamfer_ratio: None,
        chamfer_corners: Default::default(),
        custom: None,
        layers: vec![],
        clearance_override: None,
        thermal_gap_override: None,
        thermal_spoke_width_override: None,
        zone_connection: None,
        thermal_spoke_angle_mdeg: None,
        stack: Default::default(),
        kicad: Default::default(),
    });
    fp.models3d = two_models();
    fp.model = Some(fp.models3d[0].path.clone());
    step(&dir, Cmd::PutLibraryFootprint { footprint: fp.clone(), overwrite: false }, false, "test").unwrap();
    step(&dir, Cmd::PlaceFootprint { footprint: "Test:Mounted".into(), at: eda_model::ir::Point { x: 10_000, y: 10_000 }, reference: String::new(), value: String::new(), definition: Some(fp) }, false, "test").unwrap();
    let (_, _, model) = load(&dir).unwrap();
    let placed = model.parts.iter().find(|p| p.footprint.as_deref() == Some("Test:Mounted")).expect("the footprint is a part of the board now");
    assert_eq!(model.footprint_of(placed).unwrap().models3d, two_models());
    assert_eq!(sent(&dir, &placed.reference), Some(two_models()));
}

// ---------------------------------------------------------------------------------------------------------------------------- the slow tier (kicad-cli)

/// kicad-cli on `args`, in `work`; the output on failure.
fn kicad_cli(work: &Path, args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let cli = eda_kicad_engine::find_cli().ok_or("no kicad-cli")?;
    let out = std::process::Command::new(cli).args(args).current_dir(work).output().map_err(|e| e.to_string())?;
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(said)
    } else {
        Err(said)
    }
}

/// `kicad-cli pcb export vrml` of `board`, the file's own name and the path of the models taken out of it, so two boards' exports can be compared.
fn vrml_of(work: &Path, board: &Path, name: &str) -> String {
    let out = work.join(format!("{name}.wrl"));
    let (units, models) = (std::ffi::OsStr::new("mm"), format!("{name}_models"));
    kicad_cli(work, &["pcb".as_ref(), "export".as_ref(), "vrml".as_ref(), "--units".as_ref(), units, "--models-dir".as_ref(), models.as_ref(), "-f".as_ref(), "-o".as_ref(), out.as_os_str(), board.as_os_str()]).unwrap_or_else(|e| panic!("kicad-cli export vrml {name}: {e}"));
    let text = std::fs::read_to_string(&out).unwrap();
    text.lines().filter(|l| !l.trim_start().starts_with("title ")).map(|l| if l.trim_start().starts_with("url ") { "url \"MODEL\"".to_string() } else { l.to_string() }).collect::<Vec<_>>().join("\n")
}

/// The numbers after `key` on the first line of `text` that starts with it.
fn numbers_after(text: &str, key: &str) -> Vec<f64> {
    text.lines().find(|l| l.trim_start().starts_with(key)).map(|l| l.trim_start()[key.len()..].split_whitespace().filter_map(|n| n.parse().ok()).collect()).unwrap_or_default()
}

/// Slow tier (kicad-cli, `EDA_SLOW_TESTS=1`): the board of a footprint that has two models -- offsets, scales, rotations, an opacity, one of them hidden -- on a footprint that is turned.
/// KiCad reads the `(model ...)` entries this studio writes the way it reads its own: its own copy of the board (`kicad-cli pcb upgrade` writes the board out again as KiCad does)
/// has the same entries, character for character, and the VRML that `kicad-cli pcb export vrml` makes of the two boards is the same. And what that VRML puts on the board is
/// what the models say: the shown model once, with its scale, turned by the footprint's turn and its own, at the footprint's place moved by its offset, and not the hidden one.
#[test]
fn kicad_cli_reads_the_models_this_studio_writes_as_it_reads_its_own() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let models = eda_kicad_engine::find_cli().map(|_| crate::model3d_api::models_dir());
    let Some(library) = models.filter(|m| m.join("Resistor_SMD.3dshapes/R_0603_1608Metric.step").is_file() && m.join("Capacitor_SMD.3dshapes/C_0603_1608Metric.step").is_file()) else {
        eprintln!("skipped: no kicad-cli or no 3D model library");
        return;
    };
    let _ = library;
    let dir = scratch("models_kicad_cli_proof");
    setup(&dir);
    // U1 turned a third of a quarter, with the two models
    step(&dir, Cmd::RotateItems { ids: vec!["U1".into()], pivot: eda_model::ir::Point { x: 5_000, y: 5_000 }, angle_millideg: 30_000 }, false, "test").unwrap();
    step(&dir, Cmd::SetBoardModels { part: "U1".into(), models: two_models() }, false, "test").unwrap();
    let ours = derived_board(&dir);

    let work = dir.join("kicad");
    std::fs::create_dir_all(&work).unwrap();
    let (ours_path, theirs_path) = (work.join("ours.kicad_pcb"), work.join("theirs.kicad_pcb"));
    std::fs::write(&ours_path, &ours).unwrap();
    std::fs::write(&theirs_path, &ours).unwrap();
    kicad_cli(&work, &["pcb".as_ref(), "upgrade".as_ref(), theirs_path.as_os_str()]).expect("kicad-cli upgrades the board");
    let theirs = std::fs::read_to_string(&theirs_path).unwrap();

    // 1. KiCad's own text of the two models is ours, entry for entry
    let entries = |text: &str| -> String {
        let block = footprint_block(text, "U1");
        let from = block.find("\t\t(model ").expect("a model is written");
        let to = block.rfind("\n\t)").map_or(block.len(), |i| i + 1);
        block[from..to].to_string()
    };
    assert_eq!(entries(&theirs), entries(&ours), "KiCad writes the entries back as this studio wrote them");
    assert_eq!(entries(&ours).matches("(model ").count(), 2);
    assert!(entries(&theirs).contains("(opacity 0.6500)") && entries(&theirs).contains("(hide yes)"), "KiCad kept the opacity and the hide switch");

    // 2. the same VRML from both
    let ours_wrl = vrml_of(&work, &ours_path, "ours");
    let theirs_wrl = vrml_of(&work, &theirs_path, "theirs");
    assert_eq!(ours_wrl, theirs_wrl, "kicad-cli exports the two boards the same");

    // 3. the shown model once, where the models say: translation from the footprint's place and the offset turned by the footprint's turn, in the units of the file (2.54 mm)
    assert_eq!(ours_wrl.matches("Inline {").count(), 1, "the hidden model is not exported: {ours_wrl}");
    let at_line = footprint_block(&ours, "U1").lines().find(|l| l.trim_start().starts_with("(at ")).expect("the footprint's place").trim().trim_start_matches("(at ").trim_end_matches(')').to_string();
    let at: Vec<f64> = at_line.split_whitespace().filter_map(|n| n.parse().ok()).collect();
    let (angle, shown) = (at[2].to_radians(), &two_models()[0]);
    // the board's centre: the box of Edge.Cuts in `setup` (0..20 mm)
    let (cx, cy) = (10.0, 10.0);
    let (s, c) = angle.sin_cos();
    let want = [(at[0] - cx + shown.offset[0] * c - shown.offset[1] * s) / 2.54, (-(at[1] - cy) + shown.offset[0] * s + shown.offset[1] * c) / 2.54, (0.8 + shown.offset[2]) / 2.54];
    let got = numbers_after(&ours_wrl, "translation");
    assert!(got.len() == 3 && got.iter().zip(want).all(|(g, w)| (g - w).abs() < 2e-4), "translation {got:?}, wanted {want:?}\n{ours_wrl}");
    assert_eq!(numbers_after(&ours_wrl[ours_wrl.find("Inline").map_or(0, |i| ours_wrl[..i].rfind("Transform").unwrap_or(0))..], "scale"), shown.scale.to_vec(), "its own scale");
    // the turn: about z, the footprint's counter-clockwise turn plus the model's own, which the file counts the other way
    let rotation = numbers_after(&ours_wrl[ours_wrl.find("rotation").unwrap()..], "rotation");
    let turned = rotation[2] * rotation[3]; // the axis is (0, 0, -1) for a clockwise turn
    let want_turn = (at[2] - shown.rotate[2]).to_radians();
    assert!((turned - want_turn).abs() < 2e-4, "rotation {rotation:?} is {turned} rad, wanted {want_turn}");
}

/// Migrate 3D Models, through a board directory: the `.wrl` references the footprints on the board name that no search finds, with their candidates among the STEP files of the
/// folders searched (a library of a test's own here), and the replacement one undo step that keeps each model's placement.
#[test]
fn migrate_3d_models_lists_the_missing_vrml_models_with_their_candidates_and_replaces_them_in_one_undo_step() {
    use crate::model3d_api::Roots;
    use crate::model3d_tab_api::{migrate_in, Root};
    let dir = scratch("migrate_models");
    setup(&dir);
    // the library the test searches: KiCad's layout, STEP files only
    let lib = scratch("migrate_models_library");
    for f in ["Resistor_SMD.3dshapes/R_0603_1608Metric.step", "Resistor_SMD.3dshapes/R_0603_1608Metric_Pad.step", "Capacitor_SMD.3dshapes/R_0603_1608Metric.step", "Resistor_SMD.3dshapes/R_1206_3216Metric.step", "Connector.3dshapes/header.stp"] {
        std::fs::create_dir_all(lib.join(f).parent().unwrap()).unwrap();
        std::fs::write(lib.join(f), "ISO-10303-21;").unwrap();
    }
    // U1 and U2 are on a footprint with the old VRML model, placed and half transparent, and a STEP that is there
    let old = "${KICAD6_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.wrl";
    let there = "${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_1206_3216Metric.step";
    let placed = Model3d { offset: [0.5, 0.0, 0.25], rotate: [0.0, 0.0, 90.0], scale: [1.0, 1.5, 1.0], opacity: 0.5, ..Model3d::identity(old) };
    let intent = dir.join("intent.yaml");
    let mut model: eda_model::ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent).unwrap()).unwrap();
    eda_model::fp_models::set_models(&mut model.footprints[0], &[placed.clone(), Model3d::identity(there)]);
    std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();

    let roots = Roots { models: lib.clone(), project: dir.clone(), extra: vec![] };
    let listing = vec![Root { name: "KIPRJMOD".into(), path: dir.canonicalize().unwrap() }, Root { name: "KICAD10_3DMODEL_DIR".into(), path: lib.canonicalize().unwrap() }];
    let answer = migrate_in(&dir, "", &roots, &listing, &|_| None);
    assert_eq!(answer.status, "200 OK", "{}", String::from_utf8_lossy(&answer.body));
    let v: serde_json::Value = serde_json::from_slice(&answer.body).unwrap();
    let missing = v["missing"].as_array().unwrap();
    assert_eq!(missing.len(), 1, "only the VRML that is not there: the STEP model is there and is not a VRML: {v}");
    let m = &missing[0];
    assert_eq!((m["name"].as_str(), m["rep"].as_str()), (Some(old), Some("U1")));
    assert_eq!(m["footprints"], serde_json::json!(["U1", "U2"]));
    assert_eq!((m["model"]["offset"].clone(), m["model"]["scale"].clone(), m["model"]["rotate"].clone(), m["model"]["opacity"].clone()), (serde_json::json!([0.5, 0.0, 0.25]), serde_json::json!([1.0, 1.5, 1.0]), serde_json::json!([0.0, 0.0, 90.0]), serde_json::json!(0.5)), "the first footprint's placement of it");
    let c: Vec<(&str, i64)> = m["candidates"].as_array().unwrap().iter().map(|c| (c["path"].as_str().unwrap(), c["score"].as_i64().unwrap())).collect();
    assert_eq!(
        c,
        [
            ("${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.step", 1000),
            ("${KICAD10_3DMODEL_DIR}/Capacitor_SMD.3dshapes/R_0603_1608Metric.step", 800),
            ("${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric_Pad.step", 460),
        ],
        "the same stem in the same folder, the same stem in another, the one that is four edits away; the other resistor and the connector are not candidates"
    );
    assert_eq!(v["scanned"], 5, "the STEP files of the library");

    // Replace: one command, the placement kept, the STEP model it was not touching left alone
    let new = c[0].0.to_string();
    step(&dir, Cmd::MigrateBoardModels { replacements: [(old.to_string(), new.clone())].into() }, false, "test").unwrap();
    let now = sent(&dir, "U1").unwrap();
    assert_eq!(now.iter().map(|m| m.path.as_str()).collect::<Vec<_>>(), [new.as_str(), there]);
    assert_eq!((now[0].offset, now[0].rotate, now[0].scale, now[0].opacity), (placed.offset, placed.rotate, placed.scale, placed.opacity), "the placement stays");
    assert_eq!(sent(&dir, "U2"), Some(now.clone()), "every footprint that had it");
    assert!(derived_board(&dir).contains("R_0603_1608Metric.step"), "and the board file names the new model");
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(sent(&dir, "U1").unwrap()[0].path, old, "one undo takes the whole migration back");
    // a folder added by hand is searched; one the route may not read is refused
    let extra = dir.join("mine");
    std::fs::create_dir_all(&extra).unwrap();
    assert_eq!(listing.len(), 2);
    std::fs::write(extra.join("R_0603_1608Metric.step"), "ISO-10303-21;").unwrap();
    let with = migrate_in(&dir, &format!("/api/3dmodel/migrate?dirs={}", extra.display()), &roots, &listing, &|_| None);
    let w: serde_json::Value = serde_json::from_slice(&with.body).unwrap();
    assert_eq!(w["scanned"], 6, "{w}");
    assert_eq!(w["missing"][0]["candidates"].as_array().unwrap().len(), 4);
    let refused = migrate_in(&dir, "/api/3dmodel/migrate?dirs=/etc", &roots, &listing, &|_| None);
    assert_eq!(refused.status, "403 Forbidden");
}
