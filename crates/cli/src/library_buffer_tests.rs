//! Delete, Rename, New Footprint, the modified flag and Revert of the libraries of the tables (UI-LEFT-2 LL1, LL2, LL6), through the routes and the steps the
//! studio makes: a `.kicad_sym` file library, a `.kicad_symdir` folder library and a `.pretty` folder.

use super::*;
use crate::board::tests::{scratch, setup};
use crate::library_save_api::{derived, modified, revert, save};
use serde_json::Value;

/// `Zeta` (a root), `Alpha` (derived from it) and `Other`.
const LIB: &str = "(kicad_symbol_lib\n\t(version 20251024)\n\t(generator \"kicad_symbol_editor\")\n\t(symbol \"Zeta\"\n\t\t(property \"Reference\" \"U\" (at 0 0 0))\n\t\t(property \"Value\" \"Zeta\" (at 0 0 0))\n\t\t(symbol \"Zeta_1_1\" (pin input line (at 0 0 0) (length 2.54) (name \"A\") (number \"1\")))\n\t)\n\t(symbol \"Alpha\" (extends \"Zeta\")\n\t\t(property \"Reference\" \"U\" (at 0 0 0))\n\t\t(property \"Value\" \"Alpha\" (at 0 0 0))\n\t)\n\t(symbol \"Other\"\n\t\t(property \"Reference\" \"R\" (at 0 0 0))\n\t\t(property \"Value\" \"Other\" (at 0 0 0))\n\t\t(symbol \"Other_1_1\" (pin passive line (at 0 0 0) (length 2.54) (name \"~\") (number \"1\")))\n\t)\n)\n";

/// A step as the studio's `/api/cmd` makes it: the library editors' flag noted (`note_step`).
fn ui_step(dir: &Path, cmd: Cmd) -> Result<String, Vec<CheckResult>> {
    let before = before_step(dir, None);
    let r = crate::board::step(dir, cmd, false, "ui");
    if r.is_ok() {
        note_step(dir, before);
    }
    r
}

fn ok(dir: &Path, cmd: Cmd) {
    ui_step(dir, cmd.clone()).unwrap_or_else(|e| panic!("{cmd:?}: {}", crate::board::reasons(&e)));
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(|i| i.as_str().unwrap_or_default().to_string()).collect()).unwrap_or_default()
}

fn design(dir: &Path) -> Design {
    crate::board::load(dir).unwrap().1
}

fn names_in(text: &str) -> Vec<String> {
    let mut n: Vec<String> = eda_kicad::scan_symbols(text).into_iter().map(|s| s.name).collect();
    n.sort();
    n
}

/// A board whose project symbol table has `Mine` at `libs/Mine.kicad_sym`, or a folder `libs/Mine.kicad_symdir` when `folder` (Zeta and Alpha in one file,
/// Other in its own).
fn board_with(name: &str, folder: bool) -> (PathBuf, PathBuf) {
    let dir = scratch(name);
    setup(&dir);
    let libs = dir.join("libs");
    std::fs::create_dir_all(&libs).unwrap();
    let path = if folder {
        let p = libs.join("Mine.kicad_symdir");
        std::fs::create_dir_all(&p).unwrap();
        let (zeta_alpha, other) = LIB.split_at(LIB.find("\t(symbol \"Other\"").unwrap());
        std::fs::write(p.join("Zeta.kicad_sym"), format!("{zeta_alpha})\n")).unwrap();
        std::fs::write(p.join("Other.kicad_sym"), format!("(kicad_symbol_lib (version 20251024)\n{other}")).unwrap();
        p
    } else {
        let p = libs.join("Mine.kicad_sym");
        std::fs::write(&p, LIB).unwrap();
        p
    };
    let row = format!(r#"{{"kind":"symbol","path":"${{KIPRJMOD}}/libs/{}"}}"#, path.file_name().unwrap().to_string_lossy());
    let added = library_tables::add_library(&dir, row.as_bytes());
    assert_eq!(added["ok"], true, "{added}");
    (dir, path)
}

#[test]
fn deleting_a_library_symbol_takes_it_and_its_children_out_of_the_file_at_the_next_save() {
    let (dir, path) = board_with("libbuf_delete_file", false);
    // KiCad lists the derived symbols Delete takes with it
    assert_eq!(strings(&derived(&dir, "Mine:Zeta")["names"]), ["Alpha"]);
    // the design has no copy of Zeta: the library's symbol is deleted all the same, and Alpha with it
    ok(&dir, Cmd::DeleteLibrarySymbol { lib_id: "Mine:Zeta".into() });
    assert_eq!(design(&dir).symbol_library.unwrap().deleted, ["Mine:Alpha", "Mine:Zeta"]);
    let names = crate::studio::symbol_editor_names_json(&dir).unwrap();
    assert_eq!(strings(&names["deleted"]), ["Mine:Alpha", "Mine:Zeta"], "the tree hides them");
    assert_eq!(strings(&modified(&dir, "symbol")["libraries"]), ["Mine"], "the library has something to save");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), LIB, "the file keeps them until Save");
    let r = save(&dir, br#"{"kind":"symbol","lib":"Mine"}"#);
    assert_eq!(r["ok"], true, "{r}");
    let mut gone = strings(&r["deleted"]);
    gone.sort();
    assert_eq!(gone, ["Mine:Alpha", "Mine:Zeta"]);
    assert_eq!(names_in(&std::fs::read_to_string(&path).unwrap()), ["Other"]);
    assert!(strings(&modified(&dir, "symbol")["libraries"]).is_empty(), "saved: nothing left to save");
    // a name neither the design nor the file has is refused
    let e = ui_step(&dir, Cmd::DeleteLibrarySymbol { lib_id: "Mine:Nobody".into() }).unwrap_err();
    assert_eq!(e[0].hint.as_deref().unwrap_or_default(), "Symbol library 'Mine' has no symbol 'Nobody'.");
    // a symbol made again under a deleted name is the library's again
    ok(&dir, Cmd::NewSymbol { lib_id: "Mine:Zeta".into() });
    assert_eq!(design(&dir).symbol_library.unwrap().deleted, ["Mine:Alpha"], "Zeta's mark went with its new copy");
}

#[test]
fn renaming_a_library_symbol_moves_it_and_its_children_under_the_new_name_at_the_next_save() {
    let (dir, path) = board_with("libbuf_rename_file", false);
    // a name the file already holds, in any case, is refused without Overwrite
    let e = ui_step(&dir, Cmd::RenameLibrarySymbol { lib_id: "Mine:Zeta".into(), new_lib_id: "Mine:OTHER".into(), overwrite: false }).unwrap_err();
    assert_eq!(e[0].hint.as_deref().unwrap_or_default(), "Symbol 'OTHER' already exists in library 'Mine'.");
    ok(&dir, Cmd::RenameLibrarySymbol { lib_id: "Mine:Zeta".into(), new_lib_id: "Mine:Omega".into(), overwrite: false });
    let d = design(&dir).symbol_library.unwrap();
    assert_eq!(d.deleted, ["Mine:Zeta"]);
    assert_eq!(d.by_lib_id("Mine:Alpha").and_then(|s| s.extends.clone()).as_deref(), Some("Mine:Omega"), "the child follows its parent");
    assert_eq!(d.by_lib_id("Mine:Omega").unwrap().fields.iter().find(|f| f.name == "Value").map(|f| f.text.as_str()), Some("Omega"), "a Value that was the name follows it");
    let r = save(&dir, br#"{"kind":"symbol","lib":"Mine"}"#);
    assert_eq!(r["ok"], true, "{r}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(names_in(&text), ["Alpha", "Omega", "Other"]);
    let alpha = eda_kicad::scan_symbols(&text).into_iter().find(|s| s.name == "Alpha").unwrap();
    assert_eq!(alpha.extends.as_deref(), Some("Omega"));
    assert_eq!(std::fs::read_to_string(path.with_extension("bak")).unwrap(), LIB, "the file was backed up first");
}

#[test]
fn a_folder_library_loses_a_deleted_symbols_file_and_renames_a_renamed_ones() {
    let (dir, path) = board_with("libbuf_folder", true);
    ok(&dir, Cmd::DeleteLibrarySymbol { lib_id: "Mine:Other".into() });
    ok(&dir, Cmd::RenameLibrarySymbol { lib_id: "Mine:Alpha".into(), new_lib_id: "Mine:Beta".into(), overwrite: false });
    let r = save(&dir, br#"{"kind":"symbol","lib":"Mine"}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(strings(&r["removed"]), [path.join("Other.kicad_sym").display().to_string()]);
    let mut files: Vec<String> = std::fs::read_dir(&path).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.')).collect();
    files.sort();
    assert_eq!(files, ["Beta.kicad_sym", "Zeta.kicad_sym"], "Other's file went; Beta has a file of its own");
    assert_eq!(names_in(&std::fs::read_to_string(path.join("Zeta.kicad_sym")).unwrap()), ["Zeta"], "Alpha left Zeta's file");
    assert_eq!(names_in(&eda_kicad::read_symbol_library_text(&path).unwrap()), ["Beta", "Zeta"]);
}

#[test]
fn an_item_edited_and_edited_back_stays_modified_until_it_is_saved_or_reverted() {
    let (dir, _) = board_with("libbuf_dirty", false);
    // opening is no modification
    ok(&dir, Cmd::OpenSymbolForEdit { lib_id: "Mine:Other".into() });
    assert!(strings(&modified(&dir, "symbol")["items"]).is_empty());
    let sym = design(&dir).symbol_library.unwrap().by_lib_id("Mine:Other").unwrap().clone();
    let mut edited = sym.clone();
    edited.description = "changed".into();
    ok(&dir, Cmd::PutLibrarySymbol { symbol: edited, overwrite: true });
    ok(&dir, Cmd::PutLibrarySymbol { symbol: sym, overwrite: true });
    assert_eq!(strings(&modified(&dir, "symbol")["items"]), ["Mine:Other"], "edited back by hand: still modified (KiCad's flag)");
    let r = revert(&dir, br#"{"kind":"symbol","id":"Mine:Other"}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert!(strings(&modified(&dir, "symbol")["items"]).is_empty(), "Revert clears it");
    // an undo is an edit too: an edit whose flag is cleared (as a save would), undone back to the file's text
    let mut edited = design(&dir).symbol_library.unwrap().by_lib_id("Mine:Other").unwrap().clone();
    edited.description = "again".into();
    ok(&dir, Cmd::PutLibrarySymbol { symbol: edited, overwrite: true });
    clear(&dir, Kind::Symbol, &["Mine:Other".to_string()], false);
    let before = before_step(&dir, Some("symbol_editor"));
    crate::board::undo(&dir, "ui", Some(eda_ops::Domain::SymbolEditor)).unwrap();
    note_step(&dir, before);
    assert!(crate::library_save_api::symbol_matches_file(&dir, design(&dir).symbol_library.unwrap().by_lib_id("Mine:Other").unwrap(), None), "the file's text again");
    assert_eq!(strings(&modified(&dir, "symbol")["items"]), ["Mine:Other"]);
    assert_eq!(save(&dir, br#"{"kind":"symbol","lib":"Mine"}"#)["ok"], true);
    assert!(strings(&modified(&dir, "symbol")["items"]).is_empty(), "Save clears it");
}

#[test]
fn revert_of_a_symbol_never_saved_puts_back_the_symbol_as_it_was_made_and_revert_of_the_library_brings_back_what_delete_took() {
    let (dir, _) = board_with("libbuf_revert", false);
    ok(&dir, Cmd::NewSymbol { lib_id: "Mine:Fresh".into() });
    let made = design(&dir).symbol_library.unwrap().by_lib_id("Mine:Fresh").unwrap().clone();
    let mut edited = made.clone();
    edited.description = "edited".into();
    ok(&dir, Cmd::PutLibrarySymbol { symbol: edited, overwrite: true });
    let r = revert(&dir, br#"{"kind":"symbol","id":"Mine:Fresh"}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(design(&dir).symbol_library.unwrap().by_lib_id("Mine:Fresh").unwrap().description, made.description, "as it was made (`m_original`)");
    // the library: the delete is undone, the new symbol goes (`RevertLibrary`: the buffer and its deleted list)
    ok(&dir, Cmd::DeleteLibrarySymbol { lib_id: "Mine:Other".into() });
    let r = revert(&dir, br#"{"kind":"symbol","lib":"Mine"}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(strings(&r["gone"]), ["Mine:Fresh"]);
    let d = design(&dir).symbol_library.unwrap();
    assert!(d.deleted.is_empty() && d.by_lib_id("Mine:Fresh").is_none(), "{:?}", d.deleted);
    assert!(strings(&crate::studio::symbol_editor_names_json(&dir).unwrap()["deleted"]).is_empty());
    // one undo step takes the revert back
    crate::board::undo(&dir, "ui", Some(eda_ops::Domain::SymbolEditor)).unwrap();
    assert_eq!(design(&dir).symbol_library.unwrap().deleted, ["Mine:Other"]);
    assert_eq!(revert(&dir, br#"{"kind":"symbol","id":"Mine:Nobody"}"#)["nothing"], true);
}

/// UI-LEFT-3 L3-1: `RevertSymbol` of a renamed symbol goes back to the original under its own name (`UpdateSymbolAfterRename( &original, .. )`), the
/// symbols derived from it following it back; a renamed and saved symbol goes back to its saved name; a symbol made, renamed and never saved goes back to
/// the made one.
#[test]
fn revert_of_a_renamed_symbol_goes_back_to_the_name_it_had() {
    let (dir, path) = board_with("libbuf_revert_rename", false);
    ok(&dir, Cmd::RenameLibrarySymbol { lib_id: "Mine:Zeta".into(), new_lib_id: "Mine:Omega".into(), overwrite: false });
    ok(&dir, Cmd::RenameLibrarySymbol { lib_id: "Mine:Omega".into(), new_lib_id: "Mine:Psi".into(), overwrite: false });
    assert_eq!(design(&dir).symbol_library.unwrap().renamed.get("Mine:Psi").cloned().unwrap_or_default(), ["Mine:Zeta", "Mine:Omega"]);
    let r = revert(&dir, br#"{"kind":"symbol","id":"Mine:Psi"}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["lib_id"], "Mine:Zeta", "the tree selects the original");
    assert_eq!(strings(&r["gone"]), ["Mine:Psi"]);
    let d = design(&dir).symbol_library.unwrap();
    assert!(d.by_lib_id("Mine:Psi").is_none() && d.by_lib_id("Mine:Zeta").is_some());
    assert_eq!(d.by_lib_id("Mine:Alpha").and_then(|s| s.extends.clone()).as_deref(), Some("Mine:Zeta"), "the child follows it back");
    assert!(d.deleted.iter().all(|x| x != "Mine:Zeta") && d.renamed.is_empty(), "{:?} {:?}", d.deleted, d.renamed);
    assert!(crate::library_save_api::symbol_matches_file(&dir, d.by_lib_id("Mine:Zeta").unwrap(), Some(&d)), "the file's symbol");
    assert!(strings(&modified(&dir, "symbol")["items"]).iter().all(|i| i != "Mine:Zeta" && i != "Mine:Psi"));
    // one undo step takes the revert back, the names it went by with it
    crate::board::undo(&dir, "ui", Some(eda_ops::Domain::SymbolEditor)).unwrap();
    assert_eq!(design(&dir).symbol_library.unwrap().renamed.get("Mine:Psi").cloned().unwrap_or_default(), ["Mine:Zeta", "Mine:Omega"]);
    // saved under its new name, renamed again: Revert goes back to the saved name (`SetOriginal` after `SaveBuffer`)
    ok(&dir, Cmd::RenameLibrarySymbol { lib_id: "Mine:Other".into(), new_lib_id: "Mine:Kept".into(), overwrite: false });
    assert_eq!(save(&dir, br#"{"kind":"symbol","lib":"Mine"}"#)["ok"], true);
    assert!(names_in(&std::fs::read_to_string(&path).unwrap()).contains(&"Kept".to_string()));
    ok(&dir, Cmd::RenameLibrarySymbol { lib_id: "Mine:Kept".into(), new_lib_id: "Mine:Later".into(), overwrite: false });
    let r = revert(&dir, br#"{"kind":"symbol","id":"Mine:Later"}"#);
    assert_eq!(r["lib_id"], "Mine:Kept", "{r}");
    // made, renamed, never saved: back to the made symbol, under its first name
    ok(&dir, Cmd::NewSymbol { lib_id: "Mine:Fresh".into() });
    ok(&dir, Cmd::RenameLibrarySymbol { lib_id: "Mine:Fresh".into(), new_lib_id: "Mine:Stale".into(), overwrite: false });
    let r = revert(&dir, br#"{"kind":"symbol","id":"Mine:Stale"}"#);
    assert_eq!(r["lib_id"], "Mine:Fresh", "{r}");
    assert!(design(&dir).symbol_library.unwrap().by_lib_id("Mine:Fresh").is_some());
}

/// UI-LEFT-3 L3-2: a derived symbol renamed while its parent is not in the design is saved derived from that parent (`LIB_BUFFER::SaveBuffer` of a derived
/// buffer), and a child of a renamed parent derives from the parent's new name.
#[test]
fn a_renamed_derived_symbol_stays_derived_when_saved() {
    let (dir, path) = board_with("libbuf_rename_derived", false);
    ok(&dir, Cmd::RenameLibrarySymbol { lib_id: "Mine:Alpha".into(), new_lib_id: "Mine:Beta".into(), overwrite: false });
    assert!(design(&dir).symbol_library.unwrap().by_lib_id("Mine:Zeta").is_none(), "the parent is not in the design");
    let r = save(&dir, br#"{"kind":"symbol","lib":"Mine"}"#);
    assert_eq!(r["ok"], true, "{r}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(names_in(&text), ["Beta", "Other", "Zeta"]);
    let beta = eda_kicad::scan_symbols(&text).into_iter().find(|s| s.name == "Beta").unwrap();
    assert_eq!(beta.extends.as_deref(), Some("Zeta"), "written derived, as KiCad's buffer is");
    // the folder library too, the parent in another file
    let (dir, path) = board_with("libbuf_rename_derived_folder", true);
    ok(&dir, Cmd::RenameLibrarySymbol { lib_id: "Mine:Alpha".into(), new_lib_id: "Mine:Beta".into(), overwrite: false });
    assert_eq!(save(&dir, br#"{"kind":"symbol","lib":"Mine"}"#)["ok"], true);
    let beta = eda_kicad::scan_symbols(&std::fs::read_to_string(path.join("Beta.kicad_sym")).unwrap()).into_iter().find(|s| s.name == "Beta").unwrap();
    assert_eq!(beta.extends.as_deref(), Some("Zeta"));
}

/// UI-LEFT-3 L3-4: the modified flag and the symbol as it was made outlast the server (a restart reads them back from the design's history).
#[test]
fn the_modified_flag_and_a_made_symbol_outlast_a_restart() {
    let (dir, _) = board_with("libbuf_persist", false);
    ok(&dir, Cmd::OpenSymbolForEdit { lib_id: "Mine:Other".into() });
    let sym = design(&dir).symbol_library.unwrap().by_lib_id("Mine:Other").unwrap().clone();
    let mut edited = sym.clone();
    edited.description = "changed".into();
    ok(&dir, Cmd::PutLibrarySymbol { symbol: edited, overwrite: true });
    ok(&dir, Cmd::PutLibrarySymbol { symbol: sym, overwrite: true });
    ok(&dir, Cmd::NewSymbol { lib_id: "Mine:Fresh".into() });
    // the server restarts: what it held in memory is gone
    state().lock().unwrap().remove(&dir);
    assert_eq!(strings(&modified(&dir, "symbol")["items"]), ["Mine:Fresh", "Mine:Other"], "edited back by hand, still modified after a restart");
    assert!(original(&dir, "Mine:Fresh").is_some(), "the made symbol is kept");
    assert_eq!(revert(&dir, br#"{"kind":"symbol","id":"Mine:Other"}"#)["ok"], true);
    state().lock().unwrap().remove(&dir);
    assert_eq!(strings(&modified(&dir, "symbol")["items"]), ["Mine:Fresh"], "the cleared flag is kept cleared");
}

const CHIP: &str = "(footprint \"Chip\" (version 20240108) (generator \"eda\") (layer \"F.Cu\")\n\t(attr through_hole exclude_from_bom)\n\t(pad \"1\" thru_hole circle (at 0 0) (size 1.5 1.5) (drill 0.8) (layers \"*.Cu\" \"*.Mask\"))\n)\n";

fn board_with_footprints(name: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(name);
    setup(&dir);
    let added = library_tables::add_library(&dir, br#"{"kind":"footprint","path":"${KIPRJMOD}/libs/Fp","create":true}"#);
    assert_eq!(added["ok"], true, "{added}");
    let pretty = dir.join("libs").join("Fp.pretty");
    std::fs::write(pretty.join("Chip.kicad_mod"), CHIP).unwrap();
    std::fs::write(pretty.join("Other.kicad_mod"), CHIP.replace("\"Chip\"", "\"Other\"")).unwrap();
    (dir, pretty)
}

#[test]
fn deleting_and_renaming_a_library_footprint_reach_its_pretty_folder_when_saved() {
    let (dir, pretty) = board_with_footprints("libbuf_fp");
    // Delete of a footprint the design has no copy of: marked, and the library's own save deletes its file (`DeleteFootprintFromLibrary`)
    ok(&dir, Cmd::DeleteLibraryFootprint { name: "Fp:Other".into() });
    assert_eq!(strings(&modified(&dir, "footprint")["libraries"]), ["Fp"]);
    let r = save(&dir, br#"{"kind":"footprint","lib":"Fp"}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["message"], "Footprint 'Other' deleted from library 'Fp'");
    assert!(!pretty.join("Other.kicad_mod").exists());
    // Rename of a footprint the design has no copy of starts from its file; a name the folder has is refused without Overwrite
    std::fs::write(pretty.join("Taken.kicad_mod"), CHIP.replace("\"Chip\"", "\"Taken\"")).unwrap();
    let e = ui_step(&dir, Cmd::RenameLibraryFootprint { name: "Fp:Chip".into(), new_name: "Fp:Taken".into(), overwrite: false }).unwrap_err();
    assert_eq!(e[0].hint.as_deref().unwrap_or_default(), "Footprint 'Taken' already exists in library 'Fp'.");
    ok(&dir, Cmd::RenameLibraryFootprint { name: "Fp:Chip".into(), new_name: "Fp:Chip2".into(), overwrite: false });
    let fl = design(&dir).footprint_library.unwrap();
    assert_eq!(fl.deleted, ["Fp:Chip", "Fp:Other"]);
    assert_eq!(fl.by_name("Fp:Chip2").unwrap().pads.len(), 1, "the file's footprint, renamed");
    // `SaveFootprint` of the renamed footprint: the old file goes, the new one is written
    let r = save(&dir, br#"{"kind":"footprint","id":"Fp:Chip2"}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert!(!pretty.join("Chip.kicad_mod").exists() && pretty.join("Chip2.kicad_mod").is_file());
    assert!(std::fs::read_to_string(pretty.join("Chip2.kicad_mod")).unwrap().starts_with("(footprint \"Chip2\""));
    assert!(strings(&modified(&dir, "footprint")["libraries"]).is_empty());
}

#[test]
fn a_new_footprint_in_a_library_takes_the_attributes_of_its_last_footprint() {
    let (dir, _) = board_with_footprints("libbuf_newfp");
    ok(&dir, Cmd::NewFootprint { name: "Fp:Untitled".into() });
    let fp = design(&dir).footprint_library.unwrap().by_name("Fp:Untitled").unwrap().clone();
    // `fpnames.back()` is Other, a through-hole footprint left out of the BOM
    assert!(fp.attributes.through_hole && fp.attributes.exclude_from_bom && !fp.attributes.smd, "{:?}", fp.attributes);
    // and saved at once into the library (`tryToSaveFootprintInLibrary`)
    assert_eq!(save(&dir, br#"{"kind":"footprint","id":"Fp:Untitled"}"#)["ok"], true);
    assert!(dir.join("libs").join("Fp.pretty").join("Untitled.kicad_mod").is_file());
    assert!(strings(&modified(&dir, "footprint")["items"]).is_empty(), "made and saved: not modified");
}

#[test]
fn save_library_as_onto_a_footprint_library_that_is_there_asks_and_overwrite_replaces_it() {
    let (dir, pretty) = board_with_footprints("libbuf_fp_save_as");
    let other = dir.join("libs").join("Old.pretty");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("Stale.kicad_mod"), CHIP.replace("\"Chip\"", "\"Stale\"")).unwrap();
    // `createNewLibrary`: "Library %s already exists." with Overwrite
    let asked = crate::library_save_api::save_library_as(&dir, br#"{"kind":"footprint","lib":"Fp","path":"${KIPRJMOD}/libs/Old"}"#);
    assert_eq!((asked["error"].clone(), asked["confirm"].clone()), (Value::from(format!("Library {} already exists.", other.display())), Value::from("Overwrite")), "{asked}");
    assert!(other.join("Stale.kicad_mod").is_file(), "nothing changed before the answer");
    let done = crate::library_save_api::save_library_as(&dir, br#"{"kind":"footprint","lib":"Fp","path":"${KIPRJMOD}/libs/Old","overwrite":true}"#);
    assert_eq!(done["ok"], true, "{done}");
    let mut files: Vec<String> = std::fs::read_dir(&other).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    files.sort();
    assert_eq!(files, ["Chip.kicad_mod", "Other.kicad_mod"], "the old folder's footprints went (`DeleteLibrary`), the library's are copied in");
    assert!(pretty.join("Chip.kicad_mod").is_file());
}

#[test]
fn a_footprint_file_the_reader_refuses_is_not_listed_and_its_library_says_why() {
    let (dir, pretty) = board_with_footprints("libbuf_fp_refused");
    std::fs::write(pretty.join("Broken.kicad_mod"), "(module_that_is_no_footprint)\n").unwrap();
    // `FP_CACHE::Load`: "Queue I/O errors so only files that fail to parse don't get loaded"
    let v = crate::library_index::items(&dir, Kind::Footprint, "Fp");
    assert_eq!(strings(&v["names"]), ["Chip", "Other"], "{v}");
    let why = v["load_error"].as_str().unwrap_or_default();
    assert!(why.starts_with(&format!("Unable to read file '{}'\n", pretty.join("Broken.kicad_mod").display())), "{why}");
}

/// UI-LEFT-3 L3-6, slow tier (kicad-cli): a footprint file and a symbol file this crate's reader takes but KiCad's parser refuses (an unknown token) are not in
/// their libraries -- the tree, the chooser's items and its search -- and the libraries say which file (`FP_CACHE::Load`, the folder `Load`); a file library
/// KiCad's parser refuses lists nothing.
#[test]
fn a_file_only_kicads_parser_refuses_is_not_listed_and_its_library_says_so() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() || eda_kicad_engine::find_cli().is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let (dir, pretty) = board_with_footprints("libbuf_fp_kicad_refuses");
    crate::library_verdict::tests::check_with_kicad_cli(&dir);
    let strict = CHIP.replace("\"Chip\"", "\"Strict\"").replace("(attr", "(frobnicate 1)\n\t(attr");
    assert!(eda_kicad::parse_library_footprint(&strict).is_ok(), "this crate's reader takes it");
    std::fs::write(pretty.join("Strict.kicad_mod"), &strict).unwrap();
    let v = crate::library_index::items(&dir, Kind::Footprint, "Fp");
    assert_eq!(strings(&v["names"]), ["Chip", "Other"], "{v}");
    assert_eq!(v["load_error"].as_str().unwrap_or_default(), format!("Unable to read file '{}'\n{}", pretty.join("Strict.kicad_mod").display(), crate::library_verdict::KICAD_REFUSES));
    let e = crate::library_search::entries(&dir, Kind::Footprint, "Fp", &crate::library_search::Filter::default());
    let listed: Vec<String> = e["entries"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(listed, ["Chip", "Other"], "the chooser lists it no more");
    let found = loop {
        let r = crate::library_search::search(&dir, Kind::Footprint, "Strict", &crate::library_search::Filter::default(), 50);
        if r["status"] == "ready" {
            break r;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert!(!found.to_string().contains("\"Strict\""), "the search finds it no more: {found}");
    // a symbol folder library: the refused file's symbols are left out, the others listed
    let (dir, path) = board_with("libbuf_sym_kicad_refuses", true);
    crate::library_verdict::tests::check_with_kicad_cli(&dir);
    let other = std::fs::read_to_string(path.join("Other.kicad_sym")).unwrap().replace("(property \"Reference\" \"R\"", "(frobnicate 1)\n\t\t(property \"Reference\" \"R\"");
    std::fs::write(path.join("Other.kicad_sym"), other).unwrap();
    let v = crate::library_index::items(&dir, Kind::Symbol, "Mine");
    assert_eq!(strings(&v["names"]), ["Alpha", "Zeta"], "{v}");
    let why = v["load_error"].as_str().unwrap_or_default();
    assert!(why.starts_with(&format!("Unable to read file '{}'", path.join("Other.kicad_sym").display())) && why.ends_with("fixed manually."), "{why}");
    // a file library: nothing loads
    let (dir, path) = board_with("libbuf_symfile_kicad_refuses", false);
    crate::library_verdict::tests::check_with_kicad_cli(&dir);
    std::fs::write(&path, LIB.replace("(property \"Reference\" \"R\"", "(frobnicate 1)\n\t\t(property \"Reference\" \"R\"")).unwrap();
    let v = crate::library_index::items(&dir, Kind::Symbol, "Mine");
    assert!(strings(&v["names"]).is_empty(), "{v}");
}
