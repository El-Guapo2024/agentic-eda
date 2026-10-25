//! The 3D models of a footprint on the board: `SetBoardModels`, `AddBoardModel`, `RemoveBoardModel`, `MoveBoardModel` and `EditBoardModel` (`fp_models.rs`).

use super::pcb_transform_tests::{board, design, lopsided, model};
use super::*;
use eda_model::footprint::Model3d;
use eda_model::fp_edit::FootprintEdit;
use eda_model::units::mm;

const LIB: &str = "${KICAD10_3DMODEL_DIR}/Test.3dshapes/Lib.step";

fn m(path: &str) -> Model3d {
    Model3d::identity(path)
}

/// The test board's `LOPSIDED` footprint, given the one model its library gives it: the footprint both parts are on, so the list a part has by itself is `[LIB]`.
fn library_model() -> ConstraintModel {
    let mut model = model(&["F.Cu", "B.Cu"]);
    eda_model::fp_models::set_models(&mut model.footprints[0], &[m(LIB)]);
    model
}

fn listed(b: &Board<'_>, part: &str) -> Option<Vec<Model3d>> {
    b.design().footprint_edit(part).and_then(|e| e.models3d.clone())
}

fn paths(list: &[Model3d]) -> Vec<String> {
    list.iter().map(|m| m.path.clone()).collect()
}

/// The design as the file keeps it (a `Design` has no equality of its own).
fn saved(b: &Board<'_>) -> String {
    serde_json::to_string(b.design()).unwrap()
}

fn refusal(b: &mut Board<'_>, cmd: Cmd) -> String {
    let before = saved(b);
    let e = b.apply(&cmd).unwrap_err();
    assert_eq!(saved(b), before, "a refused command leaves the design as it was");
    format!("{}: {}", e[0].check, e[0].hint.clone().unwrap_or_default())
}

fn add(part: &str, model: Model3d) -> Cmd {
    Cmd::AddBoardModel { part: part.into(), model, at: None }
}

fn edit(part: &str, index: usize) -> Cmd {
    Cmd::EditBoardModel { part: part.into(), index, path: None, offset: None, scale: None, rotate: None, opacity: None, show: None }
}

#[test]
fn a_models_are_added_edited_moved_and_removed_one_verb_at_a_time_and_each_is_one_command() {
    let m0 = library_model();
    let mut b = board(&m0);
    assert_eq!(listed(&b, "U1"), None, "a footprint nothing was set on has no list of its own: it has the library's");

    // the first add starts from the library's list
    b.apply(&add("U1", m("two.step"))).unwrap();
    assert_eq!(paths(&listed(&b, "U1").unwrap()), [LIB, "two.step"]);
    assert_eq!(listed(&b, "U2"), None, "the other footprint is untouched, though it is on the same library footprint");

    // the cells of one model
    let mut cmd = edit("U1", 1);
    if let Cmd::EditBoardModel { offset, scale, rotate, opacity, show, .. } = &mut cmd {
        *offset = Some([1.5, -2.0, 0.25]);
        *scale = Some([2.0, 2.0, 1.0]);
        *rotate = Some([90.0, 0.0, -30.0]);
        *opacity = Some(0.5);
        *show = Some(false);
    }
    b.apply(&cmd).unwrap();
    let list = listed(&b, "U1").unwrap();
    assert_eq!(list[1], Model3d { path: "two.step".into(), offset: [1.5, -2.0, 0.25], scale: [2.0, 2.0, 1.0], rotate: [90.0, 0.0, -30.0], opacity: 0.5, show: false });
    assert_eq!(list[0], m(LIB), "the other model is untouched");

    // its file name alone leaves the rest
    let mut rename = edit("U1", 1);
    if let Cmd::EditBoardModel { path, .. } = &mut rename {
        *path = Some("three.step".into());
    }
    b.apply(&rename).unwrap();
    assert_eq!((listed(&b, "U1").unwrap()[1].path.clone(), listed(&b, "U1").unwrap()[1].opacity), ("three.step".to_string(), 0.5));

    // the order of the rows
    b.apply(&Cmd::MoveBoardModel { part: "U1".into(), from: 1, to: 0 }).unwrap();
    assert_eq!(paths(&listed(&b, "U1").unwrap()), ["three.step", LIB]);

    // a model leaves
    b.apply(&Cmd::RemoveBoardModel { part: "U1".into(), index: 0 }).unwrap();
    assert_eq!(listed(&b, "U1"), None, "the list is the library's again: the edit is gone, and the footprint is written as it always was");
    assert!(b.design().footprint_edit("U1").is_none());
}

#[test]
fn a_model_can_be_added_before_a_position() {
    let m0 = library_model();
    let mut b = board(&m0);
    b.apply(&Cmd::AddBoardModel { part: "U1".into(), model: m("a.step"), at: Some(0) }).unwrap();
    b.apply(&Cmd::AddBoardModel { part: "U1".into(), model: m("b.step"), at: Some(1) }).unwrap();
    assert_eq!(paths(&listed(&b, "U1").unwrap()), ["a.step", "b.step", LIB]);
}

#[test]
fn the_whole_list_is_set_in_one_command_and_an_empty_list_is_a_footprint_with_no_model() {
    let m0 = library_model();
    let mut b = board(&m0);
    let two = vec![Model3d { offset: [1.0, 2.0, 3.0], opacity: 0.65, ..m("a.step") }, Model3d { scale: [0.5, 0.5, 0.5], rotate: [0.0, 90.0, 180.0], show: false, ..m("b.step") }];
    b.apply(&Cmd::SetBoardModels { part: "U1".into(), models: two.clone() }).unwrap();
    assert_eq!(listed(&b, "U1"), Some(two.clone()));
    assert_eq!(b.design().footprint_edit("U1").unwrap().models3d.as_ref().map(Vec::len), Some(2));

    // every model taken off: the footprint has none. The edit says so (`Some` of an empty list); it is not the same as saying nothing, which brings the library's model
    b.apply(&Cmd::SetBoardModels { part: "U1".into(), models: vec![] }).unwrap();
    assert_eq!(listed(&b, "U1"), Some(vec![]));
    assert!(!b.design().footprint_edit("U1").unwrap().is_empty(), "an edit that says the footprint has no model is an edit");

    // and the library's own list brings the edit away
    b.apply(&Cmd::SetBoardModels { part: "U1".into(), models: vec![m(LIB)] }).unwrap();
    assert!(b.design().footprint_edit("U1").is_none());
}

#[test]
fn a_list_set_with_the_fields_of_the_dialog_keeps_the_pad_and_field_edits_of_the_footprint() {
    let m0 = library_model();
    let mut b = board(&m0);
    let mut pad = eda_model::fp_edit::PadEdit::none("1", 1);
    pad.size = Some((mm(0.9), mm(0.9)));
    b.apply(&Cmd::EditBoardPad { part: "U1".into(), edit: pad.clone() }).unwrap();
    b.apply(&add("U1", m("two.step"))).unwrap();
    let e = b.design().footprint_edit("U1").unwrap();
    assert_eq!((e.pads.clone(), e.models3d.as_ref().map(Vec::len)), (vec![pad.clone()], Some(2)));
    // the models back to the library's: the pad edit stays, so the edit does
    b.apply(&Cmd::RemoveBoardModel { part: "U1".into(), index: 1 }).unwrap();
    let e = b.design().footprint_edit("U1").unwrap();
    assert_eq!((e.pads.clone(), e.models3d.clone()), (vec![pad], None));
}

#[test]
fn a_position_the_list_has_none_at_is_refused_with_its_reason_and_nothing_changes() {
    let m0 = library_model();
    let mut b = board(&m0);
    let e = refusal(&mut b, Cmd::RemoveBoardModel { part: "U1".into(), index: 1 });
    assert_eq!(e, "ops_bad_model_index: there is no model 2: the footprint has 1 3D model");
    assert!(refusal(&mut b, Cmd::MoveBoardModel { part: "U1".into(), from: 0, to: 4 }).starts_with("ops_bad_model_index: there is no model 5"));
    assert!(refusal(&mut b, Cmd::AddBoardModel { part: "U1".into(), model: m("x.step"), at: Some(2) }).starts_with("ops_bad_model_index: a model cannot be added at position 3"));
    let mut cmd = edit("U1", 3);
    if let Cmd::EditBoardModel { show, .. } = &mut cmd {
        *show = Some(false);
    }
    assert!(refusal(&mut b, cmd).starts_with("ops_bad_model_index: there is no model 4"));
    assert!(b.design().footprint_edit("U1").is_none(), "no edit was made");
    // the same on a footprint with no models at all
    let mut bare = model(&["F.Cu", "B.Cu"]);
    bare.footprints[0].models3d.clear();
    bare.footprints[0].model = None;
    let mut b = board(&bare);
    assert_eq!(refusal(&mut b, Cmd::RemoveBoardModel { part: "U1".into(), index: 0 }), "ops_bad_model_index: there is no model 1: the footprint has no 3D models");
}

#[test]
fn a_file_name_or_a_number_the_panel_would_not_take_is_refused_in_its_words() {
    let m0 = library_model();
    let mut b = board(&m0);
    assert_eq!(refusal(&mut b, add("U1", m(""))), "ops_bad_model: No filename entered");
    assert_eq!(refusal(&mut b, add("U1", m("a?.step"))), "ops_bad_model: Illegal filename: \"a?.step\"");
    assert_eq!(refusal(&mut b, add("U1", m("AL.IAS:a.step"))), "ops_bad_model: Illegal filename: \"AL.IAS:a.step\"");
    assert!(refusal(&mut b, add("U1", Model3d { scale: [0.0, 1.0, 1.0], ..m("a.step") })).contains("the scale 0 is out of range"));
    assert!(refusal(&mut b, add("U1", Model3d { offset: [2_000.0, 0.0, 0.0], ..m("a.step") })).contains("the offset 2000 mm is out of range"));
    assert!(refusal(&mut b, add("U1", Model3d { opacity: -0.1, ..m("a.step") })).contains("the opacity -0.1 is out of range"));
    // an edit is judged as the model it makes
    let mut cmd = edit("U1", 0);
    if let Cmd::EditBoardModel { path, .. } = &mut cmd {
        *path = Some(String::new());
    }
    assert_eq!(refusal(&mut b, cmd), "ops_bad_model: No filename entered");
    // a whole list names the row
    let list = vec![m("a.step"), m("b.step"), m("c|d?.step")];
    assert_eq!(refusal(&mut b, Cmd::SetBoardModels { part: "U1".into(), models: list }), "ops_bad_model: model 3: Illegal filename: \"c|d?.step\"");
    assert!(b.design().footprint_edit("U1").is_none());
}

#[test]
fn a_command_that_changes_nothing_is_refused_so_that_no_empty_undo_step_is_pushed() {
    let m0 = library_model();
    let mut b = board(&m0);
    assert_eq!(refusal(&mut b, Cmd::SetBoardModels { part: "U1".into(), models: vec![m(LIB)] }), "ops_nothing_to_change: the 3D models are as they were");
    assert!(refusal(&mut b, edit("U1", 0)).starts_with("ops_nothing_to_change"), "a patch that says nothing");
    let mut same = edit("U1", 0);
    if let Cmd::EditBoardModel { show, scale, .. } = &mut same {
        *show = Some(true);
        *scale = Some([1.0, 1.0, 1.0]);
    }
    assert_eq!(refusal(&mut b, same), "ops_nothing_to_change: model 1 is as it was");
    b.apply(&add("U1", m("b.step"))).unwrap();
    assert!(refusal(&mut b, Cmd::MoveBoardModel { part: "U1".into(), from: 1, to: 1 }).starts_with("ops_nothing_to_change"));
}

#[test]
fn a_footprint_that_is_not_on_the_board_has_no_models_to_edit() {
    let m0 = library_model();
    let mut b = board(&m0);
    for cmd in [add("U9", m("a.step")), Cmd::RemoveBoardModel { part: "U9".into(), index: 0 }, Cmd::SetBoardModels { part: "U9".into(), models: vec![m("a.step")] }] {
        assert!(refusal(&mut b, cmd).starts_with("ops_not_placed"));
    }
}

#[test]
fn several_model_edits_in_one_batch_are_one_command_and_a_refusal_undoes_them_all() {
    let m0 = library_model();
    let mut b = board(&m0);
    let mut hide = edit("U1", 0);
    if let Cmd::EditBoardModel { show, .. } = &mut hide {
        *show = Some(false);
    }
    b.apply(&Cmd::Batch { cmds: vec![add("U1", m("a.step")), add("U1", m("b.step")), hide, Cmd::MoveBoardModel { part: "U1".into(), from: 2, to: 0 }] }).unwrap();
    let list = listed(&b, "U1").unwrap();
    assert_eq!(paths(&list), ["b.step", LIB, "a.step"]);
    assert!(!list[1].show);

    let before = saved(&b);
    let e = b.apply(&Cmd::Batch { cmds: vec![add("U1", m("c.step")), Cmd::RemoveBoardModel { part: "U1".into(), index: 9 }] }).unwrap_err();
    assert_eq!(e[0].check, "ops_bad_model_index");
    assert_eq!(saved(&b), before, "the first command of a refused batch is taken back with the second");
}

#[test]
fn a_footprint_edit_without_a_list_reads_as_before_and_the_list_goes_in_and_out_of_the_design_file() {
    // a design.json from before the list existed
    let old: FootprintEdit = serde_json::from_str(r#"{"id":"U1","pads":[{"number":"1","size":[900,900]}]}"#).unwrap();
    assert_eq!(old.models3d, None);
    assert!(serde_json::to_string(&old).unwrap().find("models3d").is_none(), "nothing to write when nothing was set");

    let mut with = FootprintEdit::new("U1");
    with.models3d = Some(vec![Model3d { offset: [1.5, 0.0, 0.25], rotate: [90.0, 0.0, 0.0], opacity: 0.5, ..m("a.step") }, m("b.step")]);
    let text = serde_json::to_string(&with).unwrap();
    assert!(text.contains("\"models3d\""), "{text}");
    assert_eq!(serde_json::from_str::<FootprintEdit>(&text).unwrap(), with);
    // and an empty list is written, not dropped: it says the footprint has no model
    let mut none = FootprintEdit::new("U1");
    none.models3d = Some(vec![]);
    let text = serde_json::to_string(&none).unwrap();
    assert!(text.contains("\"models3d\":[]"), "{text}");
    assert_eq!(serde_json::from_str::<FootprintEdit>(&text).unwrap().models3d, Some(vec![]));
}

#[test]
fn the_commands_read_from_the_json_the_studio_sends() {
    // the studio reads a command from a parsed body (`serde_json::from_value`, as `/api/cmd` does)
    let read = |v: serde_json::Value| -> Cmd { serde_json::from_value(v).unwrap() };
    let m0 = library_model();
    let mut b = board(&m0);
    b.apply(&read(serde_json::json!({ "op": "add_board_model", "part": "U1", "model": { "path": "a.step", "offset": [1, 2, 0.5], "rotate": [90, 0, 0], "opacity": 0.5 } }))).unwrap();
    b.apply(&read(serde_json::json!({ "op": "edit_board_model", "part": "U1", "index": 1, "scale": [2, 2, 2], "show": false }))).unwrap();
    b.apply(&read(serde_json::json!({ "op": "move_board_model", "part": "U1", "from": 1, "to": 0 }))).unwrap();
    let list = listed(&b, "U1").unwrap();
    assert_eq!(list[0], Model3d { path: "a.step".into(), offset: [1.0, 2.0, 0.5], scale: [2.0, 2.0, 2.0], rotate: [90.0, 0.0, 0.0], opacity: 0.5, show: false });
    assert_eq!(list[1], m(LIB));
    b.apply(&read(serde_json::json!({ "op": "remove_board_model", "part": "U1", "index": 0 }))).unwrap();
    assert_eq!(listed(&b, "U1"), None);
    b.apply(&read(serde_json::json!({ "op": "set_board_models", "part": "U1", "models": [{ "path": "x.step" }, { "path": "y.step", "show": false }] }))).unwrap();
    assert_eq!(paths(&listed(&b, "U1").unwrap()), ["x.step", "y.step"]);
}

/// U1 with its own list already set, as the board has it after a load: the edit in the design and laid on the footprint in the model.
fn set_up_with(list: &[Model3d]) -> (ConstraintModel, Design) {
    let mut m = library_model();
    let mut d = design();
    let mut edit = FootprintEdit::new("U1");
    edit.models3d = Some(list.to_vec());
    let lib = m.library_footprint_of(m.part("U1").unwrap()).unwrap();
    m.instance_footprints.insert("U1".into(), eda_model::fp_edit::edited_footprint(&lib, &edit));
    m.edited_models.insert("U1".into());
    d.drawings.as_mut().unwrap().footprint_edits.push(edit);
    (m, d)
}

fn exchange_for_other(b: &mut Board<'_>, options: ExchangeOptions) {
    let mut other = lopsided();
    other.name = "Test:OTHER".into();
    eda_model::fp_models::set_models(&mut other, &[m("other.step")]);
    b.apply(&Cmd::ExchangeFootprints { references: vec!["U1".into()], footprint: Some("Test:OTHER".into()), options, library: [("Test:OTHER".to_string(), other)].into() }).unwrap();
}

#[test]
fn a_footprint_put_on_another_keeps_the_list_the_board_set_unless_the_models_are_reset() {
    let list = vec![Model3d { offset: [1.0, 0.0, 0.5], ..m("a.step") }, m("b.step")];
    // "Update/reset 3D models" off: the footprint keeps its models, on the new footprint's pads
    let (model, design) = set_up_with(&list);
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    exchange_for_other(&mut b, ExchangeOptions { reset_3d_models: false, ..ExchangeOptions::change() });
    assert_eq!(listed(&b, "U1"), Some(list.clone()), "the list the board set stays set");
    assert_eq!(b.design().footprint_assignment("U1").unwrap().definition.as_ref().unwrap().models3d, list, "and the board's copy of the new footprint carries it");

    // on: they are the new footprint's own, and the board no longer sets any
    let (model, design) = set_up_with(&list);
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    exchange_for_other(&mut b, ExchangeOptions { reset_3d_models: true, ..ExchangeOptions::change() });
    assert_eq!(listed(&b, "U1"), None);
    assert_eq!(paths(&b.design().footprint_assignment("U1").unwrap().definition.as_ref().unwrap().models3d), ["other.step"]);
    assert!(b.design().footprint_edit("U1").is_none_or(|e| e.models3d.is_none()));

    // a footprint the person took every model off keeps having none when the models are kept
    let (model, design) = set_up_with(&[]);
    let mut b = Board::new(design, &model, mm(0.1), mm(0.3));
    exchange_for_other(&mut b, ExchangeOptions { reset_3d_models: false, ..ExchangeOptions::change() });
    assert_eq!(listed(&b, "U1"), Some(vec![]), "no model is still no model: it is not the new footprint's");
    // and one nothing was set on has no list of its own after either
    let m0 = library_model();
    let mut b = board(&m0);
    exchange_for_other(&mut b, ExchangeOptions { reset_3d_models: false, ..ExchangeOptions::change() });
    assert_eq!(listed(&b, "U1"), None);
}

const OLD: &str = "${KICAD6_3DMODEL_DIR}/Test.3dshapes/Lib.wrl";
const NEW: &str = "${KICAD10_3DMODEL_DIR}/Test.3dshapes/Lib.step";

/// A board whose two footprints are on a library footprint with an old VRML model.
fn old_model() -> ConstraintModel {
    let mut model = model(&["F.Cu", "B.Cu"]);
    eda_model::fp_models::set_models(&mut model.footprints[0], &[Model3d { offset: [1.0, 0.0, 0.5], rotate: [0.0, 0.0, 90.0], opacity: 0.5, ..m(OLD) }]);
    model
}

fn replacing(old: &str, new: &str) -> Cmd {
    Cmd::MigrateBoardModels { replacements: [(old.to_string(), new.to_string())].into() }
}

#[test]
fn migrating_a_model_gives_every_footprint_that_has_it_the_new_name_with_its_placement_in_one_command() {
    let m0 = old_model();
    let mut b = board(&m0);
    b.apply(&replacing(OLD, NEW)).unwrap();
    let want = vec![Model3d { path: NEW.into(), offset: [1.0, 0.0, 0.5], rotate: [0.0, 0.0, 90.0], opacity: 0.5, ..m(NEW) }];
    assert_eq!((listed(&b, "U1"), listed(&b, "U2")), (Some(want.clone()), Some(want)), "both footprints are on the footprint that has it: the placement is kept");
}

#[test]
fn migrating_changes_only_the_models_under_the_old_names_and_leaves_a_footprints_other_models_alone() {
    let m0 = old_model();
    let mut b = board(&m0);
    // U1 has its own list: the old model with another placement, another model, the old one again
    let own = vec![Model3d { scale: [2.0, 2.0, 2.0], ..m(OLD) }, m("keep.step"), Model3d { show: false, ..m(OLD) }];
    b.apply(&Cmd::SetBoardModels { part: "U1".into(), models: own }).unwrap();
    b.apply(&Cmd::MigrateBoardModels { replacements: [(OLD.to_string(), NEW.to_string()), ("never-used.wrl".to_string(), "x.step".to_string())].into() }).unwrap();
    let u1 = listed(&b, "U1").unwrap();
    assert_eq!(paths(&u1), [NEW, "keep.step", NEW]);
    assert_eq!((u1[0].scale, u1[2].show), ([2.0, 2.0, 2.0], false), "each model keeps its placement");
    assert_eq!(paths(&listed(&b, "U2").unwrap()), [NEW], "and U2, which had the library's, now has its own");
    // a second migration finds nothing under those names
    let e = refusal(&mut b, replacing(OLD, NEW));
    assert_eq!(e, "ops_nothing_to_change: no footprint on the board has a model under those names");
}

#[test]
fn a_migration_the_tab_would_not_take_is_refused_and_changes_nothing() {
    let m0 = old_model();
    let mut b = board(&m0);
    assert_eq!(refusal(&mut b, Cmd::MigrateBoardModels { replacements: Default::default() }), "ops_nothing_to_change: no model is to be replaced");
    assert!(refusal(&mut b, replacing(OLD, "bad?.step")).starts_with(&format!("ops_bad_model: the new name for {OLD}: Illegal filename")));
    assert!(refusal(&mut b, replacing(OLD, "")).contains("No filename entered"));
    assert_eq!(refusal(&mut b, replacing("other.wrl", NEW)), "ops_nothing_to_change: no footprint on the board has a model under those names");
    assert!(b.design().footprint_edit("U1").is_none() && b.design().footprint_edit("U2").is_none());
}

#[test]
fn the_command_reads_from_the_json_the_studio_sends_for_a_migration() {
    let read = |v: serde_json::Value| -> Cmd { serde_json::from_value(v).unwrap() };
    let m0 = old_model();
    let mut b = board(&m0);
    b.apply(&read(serde_json::json!({ "op": "migrate_board_models", "replacements": { OLD: NEW } }))).unwrap();
    assert_eq!(paths(&listed(&b, "U1").unwrap()), [NEW]);
}
