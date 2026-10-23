//! Carried placements (`crates/cli/src/carry.rs`): Duplicate and Paste and the drop that ends them are one undo step, and a cancelled carry leaves nothing.

use super::super::tests::{scratch, setup};
use super::super::{load, redo, step, step_carrying, undo, Strictness};
use super::{cancel, commit, is_carrying, Carry};
use eda_model::ir::Point;
use eda_ops::{Cmd, Domain};
use std::path::Path;

fn begin(dir: &Path, cmd: Cmd) {
    step_carrying(dir, cmd, Strictness::Off, "test", Some(Carry::Begin)).expect("the carried command runs");
}

fn drop_with(dir: &Path, cmd: Cmd) -> Result<String, Vec<eda_model::CheckResult>> {
    step_carrying(dir, cmd, Strictness::Off, "test", Some(Carry::Commit))
}

fn footprints(dir: &Path) -> Vec<(String, Point)> {
    load(dir).unwrap().1.placement.as_ref().unwrap().footprints.iter().map(|f| (f.id.clone(), f.at)).collect()
}

fn undo_pcb(dir: &Path) -> Result<String, Vec<eda_model::CheckResult>> {
    undo(dir, "test", Some(Domain::Pcb))
}

fn duplicate_u1() -> Cmd {
    Cmd::Duplicate { ids: vec!["U1".into()] }
}

fn add_text(dir: &Path) {
    let text = eda_model::ir::Text { id: String::new(), content: "T".into(), at: Point { x: 1_000, y: 1_000 }, angle: 0, layer: "F.SilkS".into(), size_nm: 1_000, stroke_width: 150, justify: eda_model::ir::TextJustify::Center, mirror: false };
    step(dir, Cmd::AddText { text }, false, "test").unwrap();
}

fn texts(dir: &Path) -> usize {
    load(dir).unwrap().1.drawings.as_ref().map_or(0, |d| d.texts.len())
}

#[test]
fn a_duplicate_and_the_drop_that_ends_it_are_one_undo_step() {
    let dir = scratch("carry_one_step");
    setup(&dir);
    begin(&dir, duplicate_u1());
    assert!(is_carrying(&dir));
    assert_eq!(footprints(&dir).len(), 3, "the copy is on the board while it is carried");

    drop_with(&dir, Cmd::MoveItems { ids: vec!["U3".into()], dx: 3_000, dy: 4_000 }).unwrap();
    assert!(!is_carrying(&dir), "the drop ends the carry");
    assert_eq!(footprints(&dir).iter().find(|(id, _)| id == "U3").unwrap().1, Point { x: 8_000, y: 9_000 }, "the copy is where it was dropped");

    undo_pcb(&dir).unwrap();
    assert_eq!(footprints(&dir).len(), 2, "one undo takes the copy and its move away together");
    assert!(undo_pcb(&dir).is_err(), "and that was the only step: nothing is left to undo");

    redo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(footprints(&dir).iter().find(|(id, _)| id == "U3").unwrap().1, Point { x: 8_000, y: 9_000 }, "one redo brings both back");
    assert!(redo(&dir, "test", Some(Domain::Pcb)).is_err());
}

#[test]
fn cancelling_a_carry_removes_the_items_and_leaves_the_undo_and_redo_stacks_as_they_were() {
    let dir = scratch("carry_cancel");
    setup(&dir);
    // An edit that was undone: it waits on the redo stack, as it does while a carried command is only a preview.
    add_text(&dir);
    undo_pcb(&dir).unwrap();
    assert_eq!(texts(&dir), 0);

    begin(&dir, duplicate_u1());
    assert_eq!(footprints(&dir).len(), 3);
    cancel(&dir, "test").unwrap();
    assert!(!is_carrying(&dir));
    assert_eq!(footprints(&dir).len(), 2, "`commit.Revert()`: the copy is gone");
    assert!(cancel(&dir, "test").is_err(), "a second Escape has nothing to cancel");

    // The redo stack still has the text; the undo stack has nothing the carry put there.
    redo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(texts(&dir), 1, "the earlier redo is still there");
    undo_pcb(&dir).unwrap();
    assert!(undo_pcb(&dir).is_err(), "and the carry left no undo entry");
}

#[test]
fn another_command_settles_a_carry_as_a_step_of_its_own() {
    let dir = scratch("carry_settled_by_a_command");
    setup(&dir);
    begin(&dir, duplicate_u1());
    add_text(&dir);
    assert!(!is_carrying(&dir));
    assert_eq!((footprints(&dir).len(), texts(&dir)), (3, 1));

    undo_pcb(&dir).unwrap();
    assert_eq!((footprints(&dir).len(), texts(&dir)), (3, 0), "the text goes first");
    undo_pcb(&dir).unwrap();
    assert_eq!(footprints(&dir).len(), 2, "then the duplicate, a step of its own");
    assert!(undo_pcb(&dir).is_err());
}

#[test]
fn undo_while_items_are_carried_undoes_the_duplicate() {
    let dir = scratch("carry_undo");
    setup(&dir);
    begin(&dir, duplicate_u1());
    undo_pcb(&dir).unwrap();
    assert!(!is_carrying(&dir));
    assert_eq!(footprints(&dir).len(), 2);
    redo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(footprints(&dir).len(), 3, "the duplicate was a step of its own, so it can be redone");
}

#[test]
fn a_drop_that_changes_nothing_ends_the_carry_and_the_duplicate_stands_as_one_step() {
    let dir = scratch("carry_commit_alone");
    setup(&dir);
    begin(&dir, duplicate_u1());
    commit(&dir).unwrap();
    assert!(!is_carrying(&dir));
    assert_eq!(footprints(&dir).len(), 3);
    assert!(commit(&dir).is_err(), "nothing is carried now");
    undo_pcb(&dir).unwrap();
    assert_eq!(footprints(&dir).len(), 2);
    assert!(undo_pcb(&dir).is_err());
}

#[test]
fn a_drop_with_nothing_carried_is_an_ordinary_step() {
    let dir = scratch("carry_drop_alone");
    setup(&dir);
    drop_with(&dir, Cmd::MoveItems { ids: vec!["U1".into()], dx: 1_000, dy: 0 }).unwrap();
    assert_eq!(footprints(&dir)[0].1, Point { x: 6_000, y: 5_000 });
    undo_pcb(&dir).unwrap();
    assert_eq!(footprints(&dir)[0].1, Point { x: 5_000, y: 5_000 });
}

#[test]
fn a_refused_drop_leaves_the_carry_open_to_be_dropped_again_or_cancelled() {
    let dir = scratch("carry_refused_drop");
    setup(&dir);
    begin(&dir, duplicate_u1());
    assert!(drop_with(&dir, Cmd::MoveItems { ids: vec!["nope".into()], dx: 1, dy: 1 }).is_err());
    assert!(is_carrying(&dir), "the carry is still open");
    drop_with(&dir, Cmd::MoveItems { ids: vec!["U3".into()], dx: 2_000, dy: 0 }).unwrap();
    undo_pcb(&dir).unwrap();
    assert_eq!(footprints(&dir).len(), 2, "and the retried drop is still one step with the duplicate");
}

#[test]
fn a_paste_and_its_drop_are_one_step_too_and_cancelling_removes_what_the_paste_brought() {
    let dir = scratch("carry_paste");
    setup(&dir);
    let reply = crate::clipboard_api::copy(&dir, serde_json::json!({ "ids": ["U1", "U2"], "reference": { "x": 0, "y": 0 } }).to_string().as_bytes());
    let text = reply["text"].as_str().unwrap().to_string();

    begin(&dir, Cmd::PasteClipboard { text: text.clone(), at: Point { x: 10_000, y: 12_000 } });
    assert_eq!(footprints(&dir).len(), 4);
    cancel(&dir, "test").unwrap();
    assert_eq!(footprints(&dir).len(), 2, "Escape takes the pasted footprints away");

    begin(&dir, Cmd::PasteClipboard { text, at: Point { x: 10_000, y: 12_000 } });
    drop_with(&dir, Cmd::MoveItems { ids: vec!["U3".into(), "U4".into()], dx: 500, dy: 0 }).unwrap();
    assert_eq!(footprints(&dir).len(), 4);
    undo_pcb(&dir).unwrap();
    assert_eq!(footprints(&dir).len(), 2, "the paste and the drop are undone together");
    assert!(undo_pcb(&dir).is_err());
}
