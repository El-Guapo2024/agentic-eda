//! The library editors' Save, Save Library As and Revert routes (`crate::library_save_api`) on a board with libraries of its own in its project tables: a
//! `.kicad_sym` file, a `.kicad_symdir` folder and a `.pretty` folder. The writers themselves are `eda_kicad::library_save`'s tests (with kicad-cli).

use super::*;
use crate::board::tests::{scratch, setup};
use eda_ops::Cmd;
use serde_json::json;

const LIB: &str = "(kicad_symbol_lib\n\t(version 20251024)\n\t(generator \"kicad_symbol_editor\")\n\t(symbol \"Zeta\"\n\t\t(property \"Reference\" \"U\" (at 0 0 0))\n\t\t(property \"Value\" \"Zeta\" (at 0 0 0))\n\t\t(property \"Description\" \"the root\" (at 0 0 0))\n\t\t(symbol \"Zeta_1_1\" (pin input line (at 0 0 0) (length 2.54) (name \"A\") (number \"1\")))\n\t)\n\t(symbol \"Alpha\" (extends \"Zeta\")\n\t\t(property \"Reference\" \"U\" (at 0 0 0))\n\t\t(property \"Value\" \"Alpha\" (at 0 0 0))\n\t\t(property \"Description\" \"derived\" (at 0 0 0))\n\t)\n)\n";

fn put(dir: &Path, symbol: &LibrarySymbol) {
    crate::board::step(dir, Cmd::PutLibrarySymbol { symbol: symbol.clone(), overwrite: true }, false, "ui").unwrap_or_else(|e| panic!("put {}: {}", symbol.lib_id, crate::board::reasons(&e)));
}

fn opened(dir: &Path, lib_id: &str) -> LibrarySymbol {
    serde_json::from_value(saved(dir, "symbol", lib_id, "")["symbol"].clone()).unwrap_or_else(|e| panic!("{lib_id} is in the file: {e}"))
}

fn in_design(dir: &Path, lib_id: &str) -> LibrarySymbol {
    let (_, design, _) = board::load(dir).unwrap();
    design.symbol_library.unwrap().by_lib_id(lib_id).unwrap().clone()
}

/// A board whose project symbol table has `Mine` (a `.kicad_sym` file holding `LIB`).
fn board_with_symbol_library(name: &str) -> PathBuf {
    let dir = scratch(name);
    setup(&dir);
    let added = library_tables::add_library(&dir, br#"{"kind":"symbol","path":"${KIPRJMOD}/libs/Mine","create":true}"#);
    assert_eq!(added["ok"], true, "{added}");
    std::fs::write(dir.join("libs").join("Mine.kicad_sym"), LIB).unwrap();
    dir
}

fn items(v: &Value) -> Vec<String> {
    v["items"].as_array().unwrap().iter().map(|i| i.as_str().unwrap().to_string()).collect()
}

#[test]
fn save_writes_the_modified_symbols_into_the_library_file_and_nothing_else() {
    let dir = board_with_symbol_library("libsave_file");
    let file = dir.join("libs").join("Mine.kicad_sym");
    let mod_ = || items(&modified(&dir, "symbol"));
    assert!(mod_().is_empty());
    // opened as it is: not modified; edited: modified; a new symbol of the library: modified
    let zeta = opened(&dir, "Mine:Zeta");
    put(&dir, &zeta);
    assert!(mod_().is_empty(), "an opened symbol is the file's");
    let mut edited = in_design(&dir, "Mine:Zeta");
    edited.reference_prefix = "Q".into();
    put(&dir, &edited);
    let mut beta = LibrarySymbol::new_empty("Mine:Beta");
    beta.reference_prefix = "R".into();
    put(&dir, &beta);
    assert_eq!(mod_(), ["Mine:Beta", "Mine:Zeta"]);
    assert_eq!(modified(&dir, "symbol")["libraries"], json!(["Mine"]));

    let r = save(&dir, br#"{"kind":"symbol","lib":"Mine"}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["message"], format!("Symbol library file '{}' saved.", file.display()));
    assert_eq!(std::fs::read_to_string(dir.join("libs").join("Mine.bak")).unwrap(), LIB, "the file as it was is kept as Mine.bak (`backupFile`)");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\t(symbol \"Alpha\" (extends \"Zeta\")\n\t\t(property \"Reference\" \"U\" (at 0 0 0))"), "Alpha is kept as the file had it: {text}");
    assert!(text.contains("\"Reference\" \"Q\""), "{text}");
    assert_eq!(eda_kicad::scan_symbols(&text).into_iter().map(|s| s.name).collect::<Vec<_>>(), ["Beta", "Zeta", "Alpha"]);
    assert!(mod_().is_empty(), "saved: nothing is modified");
    // a second save with nothing modified writes nothing
    let again = save(&dir, br#"{"kind":"symbol","lib":"Mine"}"#);
    assert_eq!(again["written"], json!([]));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
}

#[test]
fn a_derived_symbol_opened_flat_is_saved_derived_while_only_its_fields_change() {
    let dir = board_with_symbol_library("libsave_derived");
    let file = dir.join("libs").join("Mine.kicad_sym");
    let alpha = opened(&dir, "Mine:Alpha");
    assert!(alpha.extends.is_none(), "the editor opens it flat");
    put(&dir, &alpha);
    let mut edited = in_design(&dir, "Mine:Alpha");
    edited.description = "derived, edited".into();
    put(&dir, &edited);
    assert_eq!(save(&dir, br#"{"kind":"symbol","lib":"Mine"}"#)["ok"], true);
    let text = std::fs::read_to_string(&file).unwrap();
    let alpha_form = eda_kicad::scan_symbols(&text).into_iter().find(|s| s.name == "Alpha").unwrap();
    assert_eq!(alpha_form.extends.as_deref(), Some("Zeta"), "{text}");
    assert_eq!(alpha_form.description, "derived, edited");
    assert!(items(&modified(&dir, "symbol")).is_empty());
    // a drawing of its own cannot be a derived symbol's: it is saved flat
    let mut drawn = in_design(&dir, "Mine:Alpha");
    let mut pin = drawn.pins[0].clone();
    pin.id.clear();
    pin.number = "2".into();
    drawn.pins.push(pin);
    put(&dir, &drawn);
    assert_eq!(save(&dir, br#"{"kind":"symbol","lib":"Mine"}"#)["ok"], true);
    let text = std::fs::read_to_string(&file).unwrap();
    let alpha_form = eda_kicad::scan_symbols(&text).into_iter().find(|s| s.name == "Alpha").unwrap();
    assert_eq!(alpha_form.extends, None);
    assert_eq!(alpha_form.pin_count(), 2);
}

#[test]
fn save_refuses_a_library_that_cannot_be_written_and_one_the_tables_do_not_have() {
    let dir = board_with_symbol_library("libsave_readonly");
    assert_eq!(save(&dir, br#"{"kind":"symbol","lib":"Nowhere"}"#)["error"], "No library specified.");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file = dir.join("libs").join("Mine.kicad_sym");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();
        let root = std::fs::read_to_string("/proc/self/status").is_ok_and(|s| s.lines().any(|l| l.starts_with("Uid:") && l.split_whitespace().nth(2) == Some("0")));
        if !root {
            let r = save(&dir, br#"{"kind":"symbol","lib":"Mine"}"#);
            assert_eq!(r["error"], "Symbol library 'Mine' is not writable.");
            assert_eq!(r["detail"], "You must save to a different location.");
            assert_eq!(r["read_only"], true);
            let w = writable(&dir, "symbol");
            let mine = w["libraries"].as_array().unwrap().iter().find(|l| l["name"] == "Mine").unwrap().clone();
            assert_eq!(mine["writable"], false);
            assert_eq!(mine["scope"], "project");
        }
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
}

#[test]
fn a_folder_library_gets_a_file_for_a_new_symbol() {
    let dir = scratch("libsave_folder");
    setup(&dir);
    let folder = dir.join("libs").join("Folder.kicad_symdir");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Zeta.kicad_sym"), LIB).unwrap();
    let added = library_tables::add_library(&dir, br#"{"kind":"symbol","path":"${KIPRJMOD}/libs/Folder.kicad_symdir"}"#);
    assert_eq!(added["ok"], true, "{added}");
    let mut gamma = LibrarySymbol::new_empty("Folder:Gamma");
    gamma.reference_prefix = "D".into();
    put(&dir, &gamma);
    let r = save(&dir, br#"{"kind":"symbol","lib":"Folder"}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["written"], json!([folder.join("Gamma.kicad_sym")]));
    assert_eq!(std::fs::read_to_string(folder.join("Zeta.kicad_sym")).unwrap(), LIB);
    // the tables' reader finds it, and it is not modified any more
    let back = opened(&dir, "Folder:Gamma");
    assert_eq!(back.reference_prefix, "D");
    assert!(items(&modified(&dir, "symbol")).is_empty());
}

#[test]
fn revert_reads_what_the_file_holds() {
    let dir = board_with_symbol_library("libsave_revert");
    let mut zeta = opened(&dir, "Mine:Zeta");
    zeta.reference_prefix = "X".into();
    put(&dir, &zeta);
    put(&dir, &LibrarySymbol::new_empty("Mine:Never"));
    let one = saved(&dir, "symbol", "Mine:Zeta", "");
    assert_eq!(one["symbol"]["reference_prefix"], "U");
    let all = saved(&dir, "symbol", "", "Mine");
    assert_eq!(all["symbols"]["Mine:Never"], Value::Null, "a symbol never saved has nothing to go back to");
    assert_eq!(all["symbols"]["Mine:Zeta"]["reference_prefix"], "U");
    assert_eq!(saved(&dir, "symbol", "", "Nowhere")["error"], "Library 'Nowhere' not found.");
}

#[test]
fn save_library_as_writes_every_symbol_into_a_new_file_and_puts_it_in_the_table() {
    let dir = board_with_symbol_library("libsave_as");
    let mut zeta = opened(&dir, "Mine:Zeta");
    zeta.reference_prefix = "Q".into();
    put(&dir, &zeta);
    let r = save_library_as(&dir, br#"{"kind":"symbol","lib":"Mine","path":"${KIPRJMOD}/libs/Copy","table":"project"}"#);
    assert_eq!(r["ok"], true, "{r}");
    let copy = dir.join("libs").join("Copy.kicad_sym");
    assert_eq!(r["path"], copy.display().to_string());
    assert_eq!(r["nickname"], "Copy");
    assert_eq!(r["uri"], "${KIPRJMOD}/libs/Copy.kicad_sym");
    let text = std::fs::read_to_string(&copy).unwrap();
    assert_eq!(eda_kicad::scan_symbols(&text).into_iter().map(|s| s.name).collect::<Vec<_>>(), ["Zeta", "Alpha"]);
    assert!(text.contains("\"Reference\" \"Q\""));
    assert_eq!(std::fs::read_to_string(dir.join("libs").join("Mine.kicad_sym")).unwrap(), LIB, "the library saved from keeps its file");
    assert_eq!(items(&modified(&dir, "symbol")), ["Mine:Zeta"], "and its edit");
    // the file is there now: replaced only when the person says so; a nickname taken gets a number
    let again = save_library_as(&dir, br#"{"kind":"symbol","lib":"Mine","path":"${KIPRJMOD}/libs/Copy.kicad_sym","table":"project"}"#);
    assert_eq!(again["error"], "A file named \"Copy.kicad_sym\" already exists. Do you want to replace it?");
    assert_eq!(again["confirm"], "Replace");
    let replaced = save_library_as(&dir, br#"{"kind":"symbol","lib":"Mine","path":"${KIPRJMOD}/libs/Copy.kicad_sym","table":"project","overwrite":true}"#);
    assert_eq!(replaced["nickname"], "Copy1", "{replaced}");
    // "Update existing library table entry": the row of Mine now names the new file, whose symbols are the design's
    let moved = save_library_as(&dir, br#"{"kind":"symbol","lib":"Mine","path":"${KIPRJMOD}/libs/Moved.kicad_sym","table":"replace"}"#);
    assert_eq!(moved["nickname"], "Mine", "{moved}");
    let table = std::fs::read_to_string(dir.join("sym-lib-table")).unwrap();
    assert!(table.contains("(name \"Mine\") (type \"KiCad\") (uri \"${KIPRJMOD}/libs/Moved.kicad_sym\")"), "{table}");
    assert!(items(&modified(&dir, "symbol")).is_empty(), "the library is the new file now");
    // only a symbol library file name
    assert!(save_library_as(&dir, br#"{"kind":"symbol","lib":"Mine","path":"${KIPRJMOD}/notes.txt"}"#)["error"].as_str().unwrap().contains("is not a symbol library file name"));
}

#[test]
fn footprints_save_into_their_pretty_folder_and_save_library_as_copies_the_folder() {
    let dir = scratch("libsave_fp");
    setup(&dir);
    let added = library_tables::add_library(&dir, br#"{"kind":"footprint","path":"${KIPRJMOD}/libs/Mine","create":true}"#);
    assert_eq!(added["ok"], true, "{added}");
    let pretty = dir.join("libs").join("Mine.pretty");
    let mut fp = LibraryFootprint::new_empty("Mine:R1");
    fp.description = "a footprint".into();
    crate::board::step(&dir, Cmd::PutLibraryFootprint { footprint: fp.clone(), overwrite: false }, false, "ui").unwrap();
    assert_eq!(items(&modified(&dir, "footprint")), ["Mine:R1"], "never saved");
    let r = save(&dir, br#"{"kind":"footprint","id":"Mine:R1"}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["path"], pretty.join("R1.kicad_mod").display().to_string());
    assert!(items(&modified(&dir, "footprint")).is_empty(), "saved");
    assert_eq!(saved(&dir, "footprint", "Mine:R1", "")["footprint"]["description"], "a footprint");
    assert_eq!(save(&dir, br#"{"kind":"footprint","id":"Nowhere:R1"}"#)["error"], "Library 'Nowhere' not found.");

    let copy = save_library_as(&dir, br#"{"kind":"footprint","lib":"Mine","path":"${KIPRJMOD}/libs/Copy"}"#);
    assert_eq!(copy["ok"], true, "{copy}");
    assert_eq!(copy["nickname"], "Copy");
    assert_eq!(copy["copied"], json!(["R1"]));
    assert_eq!(copy["message"], format!("Footprint library '{}' saved as '{}'.", pretty.display(), dir.join("libs").join("Copy.pretty").display()));
    assert_eq!(std::fs::read_to_string(dir.join("libs").join("Copy.pretty").join("R1.kicad_mod")).unwrap(), std::fs::read_to_string(pretty.join("R1.kicad_mod")).unwrap());
    assert!(std::fs::read_to_string(dir.join("fp-lib-table")).unwrap().contains("(name \"Copy\")"));
}

/// A footprint of `Mine` (the project table's `.pretty`) with what a blank footprint has none of: two pads, a silkscreen line and a description.
const CHIP: &str = r#"(footprint "Chip" (version 20240108) (generator "eda") (layer "F.Cu") (descr "the file's")
	(attr smd)
	(fp_line (start -2 -1) (end 2 -1) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
	(pad "1" smd rect (at -1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
	(pad "2" smd rect (at 1 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask"))
)
"#;

/// A board whose project footprint table has `Mine` (`libs/Mine.pretty`) holding `Chip.kicad_mod`.
fn board_with_footprint_library(name: &str) -> PathBuf {
    let dir = scratch(name);
    setup(&dir);
    let added = library_tables::add_library(&dir, br#"{"kind":"footprint","path":"${KIPRJMOD}/libs/Mine","create":true}"#);
    assert_eq!(added["ok"], true, "{added}");
    std::fs::write(dir.join("libs").join("Mine.pretty").join("Chip.kicad_mod"), CHIP).unwrap();
    dir
}

fn design_footprint(dir: &Path, name: &str) -> Option<LibraryFootprint> {
    let (_, design, _) = board::load(dir).unwrap();
    design.footprint_library.and_then(|l| l.by_name(name).cloned())
}

/// The Footprint Editor opens a library's footprint in two requests: `GET /api/library/footprint` (`library_api::footprint_any`: the design's copy, else
/// the file's, which the editor then puts in) and `open_footprint_for_edit`. A command can land between them -- in the fpedit-fields run that failed, the
/// Save As dialog's own open read the design's copy, the check deleted it, and the open came after -- and the open verb, which reads no library, started a
/// blank footprint under the library's name that then stood in for the file. The interleaving, forced: the open reads the file itself now.
#[test]
fn a_footprint_whose_design_copy_goes_between_the_editors_read_and_its_open_opens_from_its_file() {
    let dir = board_with_footprint_library("libsave_reopen_race");
    let (fp, in_project) = crate::library_api::footprint_any(&dir, "Mine:Chip").unwrap();
    assert!(!in_project);
    crate::board::step(&dir, Cmd::PutLibraryFootprint { footprint: fp, overwrite: true }, false, "ui").unwrap();
    assert!(items(&modified(&dir, "footprint")).is_empty(), "the copy the editor put in is the file's");
    // the editor looks: the design's copy is there, so it puts nothing in ...
    assert!(crate::library_api::footprint_any(&dir, "Mine:Chip").unwrap().1, "in the project");
    // ... another client forgets the design's copy ...
    crate::board::step(&dir, Cmd::DeleteLibraryFootprint { name: "Mine:Chip".into() }, false, "check").unwrap();
    // ... and the editor's open lands
    crate::board::step(&dir, Cmd::OpenFootprintForEdit { name: "Mine:Chip".into() }, false, "ui").unwrap();
    let opened = design_footprint(&dir, "Mine:Chip").expect("opened");
    assert_eq!((opened.pads.len(), opened.graphics.len(), opened.description.as_str()), (2, 1, "the file's"), "the file's footprint, not a blank one");
    assert!(items(&modified(&dir, "footprint")).is_empty(), "and not modified: the design's copy is the file's");
    // one undo step, as the editor's own put is: undone, the design has no copy again
    crate::board::undo(&dir, "ui", None).unwrap();
    assert!(design_footprint(&dir, "Mine:Chip").is_none(), "undone: the copy the open put in is gone");
}

/// A library's file that is there and cannot be parsed is not "no footprint with this name" (for which the editor starts a blank one): the read says why,
/// and the open is refused -- `PCB_BASE_FRAME::LoadFootprint` gives nothing on the `IO_ERROR`, `LoadFootprintFromLibrary` leaves the editor as it was.
#[test]
fn a_library_footprint_whose_file_cannot_be_parsed_is_not_opened_blank() {
    let dir = board_with_footprint_library("libsave_reopen_broken");
    let broken = dir.join("libs").join("Mine.pretty").join("Broken.kicad_mod");
    std::fs::write(&broken, "(footprint \"Broken\" (layer \"F.Cu\") (pad \"1\" smd rect").unwrap();
    let why = crate::board::reasons(&crate::library_api::footprint_any(&dir, "Mine:Broken").unwrap_err());
    assert!(why.contains(&broken.display().to_string()) && !why.contains("no footprint with this name"), "{why}");
    let refused = crate::board::step(&dir, Cmd::OpenFootprintForEdit { name: "Mine:Broken".into() }, false, "ui").unwrap_err();
    assert!(crate::board::reasons(&refused).contains(&broken.display().to_string()), "{}", crate::board::reasons(&refused));
    assert!(design_footprint(&dir, "Mine:Broken").is_none(), "nothing opened");
    // a name no library has is KiCad's New Footprint still: a blank one
    let missing = crate::board::reasons(&crate::library_api::footprint_any(&dir, "Mine:Fresh").unwrap_err());
    assert!(missing.contains("no footprint with this name"), "{missing}");
    crate::board::step(&dir, Cmd::OpenFootprintForEdit { name: "Mine:Fresh".into() }, false, "ui").unwrap();
    assert!(design_footprint(&dir, "Mine:Fresh").is_some_and(|f| f.pads.is_empty() && f.graphics.is_empty()));
}
