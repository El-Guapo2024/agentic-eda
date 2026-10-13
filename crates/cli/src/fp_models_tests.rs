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
