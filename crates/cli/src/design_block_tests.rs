//! Design blocks through a board directory (GAPS.md package P18): the library the board's table names, the Save as Design Block routes writing a block from the sheet, a selection,
//! the board or a selection of it, placing a block (the schematic's Import Sheet, the board's Place Design Block) as one undo step, Properties and Delete -- and (the slow
//! tier, `EDA_SLOW_TESTS=1`) what kicad-cli makes of the blocks this studio writes, and of a block made from one of KiCad's own templates.

use super::tests::{scratch, setup};
use super::{load, meta_path, save, step, undo, Meta};
use crate::design_block_api as api;
use crate::library_tables::{self, tests::machine, Env};
use eda_kicad::design_block as db;
use eda_model::ir::{PlacementSection, Point};
use eda_model::ConstraintModel;
use eda_ops::{Cmd, Domain};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn slow() -> bool {
    let off = std::env::var_os("EDA_SLOW_TESTS").is_none();
    if off {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
    }
    !off
}

/// kicad-cli on `args`, in `work`: what it said, or what it said on failure. Launched as the studio launches it (`cli_command`: an empty KiCad config, not the user's).
fn kicad_cli(work: &Path, args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let cli = eda_kicad_engine::find_cli().ok_or("no kicad-cli")?;
    let out = eda_kicad_engine::cli_command(&cli, None).args(args).current_dir(work).output().map_err(|e| e.to_string())?;
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(said)
    } else {
        Err(said)
    }
}

/// mcu30's flat schematic as the engine draws it, in the board folder `dir` (the intent is read from `examples/` and never written).
fn mcu30_flat(dir: &Path) {
    let intent_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/mcu_board_30plus.yaml");
    let model: ConstraintModel = serde_yaml::from_str(&std::fs::read_to_string(&intent_path).unwrap()).unwrap();
    let mut design = eda_engine::derive_schematic(&model, &eda_engine::EngineOptions::new(1, "t")).unwrap();
    design.placement = Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] });
    save(dir, &design).unwrap();
    let meta = Meta { intent: intent_path.display().to_string(), snap_nm: 100_000, spacing_nm: 300_000 };
    std::fs::write(meta_path(dir), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
}

fn req(v: Value) -> Vec<u8> {
    v.to_string().into_bytes()
}

/// A board folder on a machine of its own with a design block library `Power` of the board's table, made the way New Library makes one.
fn with_library(tag: &str, board: fn(&Path)) -> (library_tables::tests::Machine, Env, PathBuf) {
    let m = machine(tag, true);
    board(&m.board);
    let env = m.env();
    let r = library_tables::add_library_with(&env, &m.board, &req(json!({ "kind": "design_block", "create": true, "path": "blocks/Power", "descr": "Regulators" })));
    assert_eq!(r["ok"], true, "{r}");
    let lib = m.board.join("blocks").join("Power.kicad_blocks");
    (m, env, lib)
}

fn names_in(list: &Value, lib: &str) -> Vec<String> {
    let l = list["libraries"].as_array().unwrap().iter().find(|l| l["name"] == lib).unwrap_or_else(|| panic!("no library {lib} in {list}"));
    l["blocks"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap().to_string()).collect()
}

fn groups_of(dir: &Path) -> Vec<eda_model::ir::Group> {
    let (_, d, _) = load(dir).unwrap();
    d.schematic.map(|s| s.extras.groups).unwrap_or_default()
}

fn board_groups_of(dir: &Path) -> Vec<eda_model::ir::Group> {
    let (_, d, _) = load(dir).unwrap();
    d.drawings.map(|s| s.groups).unwrap_or_default()
}

// ------------------------------------------------------------------------------------------------ the library

#[test]
fn the_libraries_of_the_table_list_their_blocks_with_what_the_metadata_says() {
    let (m, env, lib) = with_library("db_list", setup);
    // a block written by hand, as KiCad's tools leave one: a layout and metadata, no schematic
    let folder = lib.join("Hand.kicad_block");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Hand.kicad_pcb"), "(kicad_pcb (version 20241229))").unwrap();
    std::fs::write(folder.join("Hand.json"), "{\"description\": \"by hand\", \"keywords\": \"x y\", \"fields\": {\"Rev\": \"B\", \"Maker\": \"me\"}}").unwrap();
    // a library that is no folder is a problem of its own, and a hidden library is not listed
    let r = library_tables::add_library_with(&env, &m.board, &req(json!({ "kind": "design_block", "path": m.root.join("nowhere.kicad_blocks").display().to_string() })));
    assert!(r["error"].is_string(), "a folder that is not there cannot be added: {r}");
    let list = api::list_with(&env, &m.board);
    assert_eq!(list["ok"], true);
    assert_eq!(names_in(&list, "Power"), vec!["Hand"]);
    let lib_json = &list["libraries"][0];
    assert_eq!((lib_json["scope"].clone(), lib_json["description"].clone(), lib_json["writable"].clone()), (json!("project"), json!("Regulators"), json!(true)));
    let block = &lib_json["blocks"][0];
    assert_eq!((block["description"].clone(), block["keywords"].clone(), block["schematic"].clone(), block["board"].clone()), (json!("by hand"), json!("x y"), json!(false), json!(true)));
    assert_eq!(block["fields"], json!([{ "name": "Rev", "value": "B" }, { "name": "Maker", "value": "me" }]), "the metadata's order");
    // the global table of this machine chains KiCad's, which ships no design block table: said, not a library
    assert!(list["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("does not exist")), "{}", list["notes"]);
    // a block with metadata that cannot be read is left out of the tree and reported
    std::fs::create_dir_all(lib.join("Broken.kicad_block")).unwrap();
    std::fs::write(lib.join("Broken.kicad_block").join("Broken.json"), "{").unwrap();
    let list = api::list_with(&env, &m.board);
    assert_eq!(names_in(&list, "Power"), vec!["Hand"]);
    assert!(list["problems"].as_array().unwrap().iter().any(|p| p["block"] == "Broken"), "{}", list["problems"]);
    let _ = std::fs::remove_dir_all(&m.root);
}

#[test]
fn a_block_name_that_leads_out_of_the_library_is_never_read_or_written() {
    let (m, env, lib) = with_library("db_names", setup);
    for name in ["../escape", "a/b", "..", "."] {
        assert_eq!(api::file_with(&env, &m.board, "Power", name, "pcb")["ok"], false, "{name}");
        assert_eq!(api::delete_with(&env, &m.board, &req(json!({ "lib": "Power", "name": name })))["ok"], false, "{name}");
        assert_eq!(api::properties_with(&env, &m.board, &req(json!({ "lib": "Power", "name": name, "description": "" })))["ok"], false, "{name}");
    }
    // a library the table does not name is not a place to write
    let r = api::save_with(&env, &m.board, &req(json!({ "what": "board", "lib": "Nope", "name": "X" })));
    assert_eq!(r["message"], "There is no design block library 'Nope'.");
    assert_eq!(db::enumerate(&lib).unwrap(), Vec::<String>::new());
    let _ = std::fs::remove_dir_all(&m.root);
}

// ------------------------------------------------------------------------------------------------ the board half

#[test]
fn a_board_saved_as_a_block_is_placed_as_a_linked_group_in_one_undo_step() {
    let (m, env, lib) = with_library("db_board", setup);
    let dir = m.board.clone();
    // Save Board as Design Block: the dialog opens named after the board, with no fields yet
    let p = api::prepare_with(&env, &dir, &req(json!({ "what": "board", "lib": "Power" })));
    assert_eq!(p["ok"], true, "{p}");
    assert_eq!(p["name"], "intent");
    let r = api::save_with(&env, &dir, &req(json!({ "what": "board", "lib": "Power", "name": "TwoParts", "description": "two parts", "keywords": "demo", "fields": [{ "name": "Rev", "value": "A" }] })));
    assert_eq!((r["ok"].clone(), r["created"].clone(), r["linked"].clone(), r["lib_id"].clone()), (json!(true), json!(true), json!(false), json!("Power:TwoParts")), "{r}");
    let board_file = lib.join("TwoParts.kicad_block").join("TwoParts.kicad_pcb");
    let text = std::fs::read_to_string(&board_file).unwrap();
    assert!(text.starts_with("(kicad_pcb"), "a board file");
    assert!(!lib.join("TwoParts.kicad_block").join("TwoParts.kicad_sch").exists());
    let meta = db::load(&lib, "TwoParts").unwrap();
    assert_eq!((meta.description.as_str(), meta.keywords.as_str(), meta.fields.clone()), ("two parts", "demo", vec![("Rev".to_string(), "A".to_string())]));
    assert_eq!(names_in(&api::list_with(&env, &dir), "Power"), vec!["TwoParts"]);

    // the same name again asks first; with the answer it is written over
    let again = api::save_with(&env, &dir, &req(json!({ "what": "board", "lib": "Power", "name": "TwoParts" })));
    assert_eq!((again["ok"].clone(), again["exists"].clone()), (json!(false), json!(true)), "{again}");
    assert_eq!(again["message"], "Design block 'TwoParts' already exists in library 'Power'.");
    let over = api::save_with(&env, &dir, &req(json!({ "what": "board", "lib": "Power", "name": "TwoParts", "overwrite": true })));
    assert_eq!((over["ok"].clone(), over["created"].clone()), (json!(true), json!(false)), "{over}");

    // Place Design Block: the file's text is what Append Board reads
    let file = api::file_with(&env, &dir, "Power", "TwoParts", "pcb");
    assert_eq!(file["ok"], true, "{file}");
    let (_, before, model_before) = load(&dir).unwrap();
    let placed = Cmd::PlaceDesignBlock { text: file["text"].as_str().unwrap().to_string(), at: Point { x: 30_000_000, y: 20_000_000 }, block: "Power:TwoParts".into(), as_group: true, group_name: String::new() };
    step(&dir, placed, false, "test").unwrap();
    let (_, after, model_after) = load(&dir).unwrap();
    assert_eq!(after.placement.as_ref().unwrap().footprints.len(), before.placement.as_ref().unwrap().footprints.len() + 2, "the block's two footprints are on the board");
    assert_eq!(model_after.parts.len(), model_before.parts.len() + 2);
    let groups = board_groups_of(&dir);
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert_eq!((groups[0].name.as_str(), groups[0].design_block.as_str(), groups[0].member_ids.len()), ("TwoParts", "Power:TwoParts", 2));
    // one undo takes the whole placement back, the group with it
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    let (_, back, model_back) = load(&dir).unwrap();
    assert!(back.placement.as_ref().unwrap().footprints.len() == 2 && model_back.parts.len() == 2 && back.drawings.as_ref().is_none_or(|d| d.groups.is_empty()));
    // a block with no layout says so
    std::fs::create_dir_all(lib.join("NoLayout.kicad_block")).unwrap();
    std::fs::write(lib.join("NoLayout.kicad_block").join("NoLayout.kicad_sch"), "(kicad_sch)").unwrap();
    assert_eq!(api::file_with(&env, &dir, "Power", "NoLayout", "pcb")["message"], "Design block has no layout to place.");
    let _ = std::fs::remove_dir_all(&m.root);
}

#[test]
fn a_selection_of_the_board_saved_as_a_block_is_grouped_and_linked_in_one_undo_step_and_update_writes_over_its_layout() {
    let (m, env, lib) = with_library("db_board_sel", setup);
    let dir = m.board.clone();
    // nothing selected, and a block that is not there to update
    assert_eq!(api::prepare_with(&env, &dir, &req(json!({ "what": "board_selection", "ids": [], "lib": "Power" })))["message"], "Please select some items to save as a design block.");
    assert_eq!(api::save_with(&env, &dir, &req(json!({ "what": "board_selection", "ids": ["U1"], "lib": "Power", "name": "Gone", "update": true })))["message"], "Design block 'Gone' does not exist.");
    let r = api::save_with(&env, &dir, &req(json!({ "what": "board_selection", "ids": ["U1", "U2"], "lib": "Power", "name": "Pair", "description": "the pair" })));
    assert_eq!((r["ok"].clone(), r["linked"].clone()), (json!(true), json!(true)), "{r}");
    let text = std::fs::read_to_string(lib.join("Pair.kicad_block").join("Pair.kicad_pcb")).unwrap();
    assert!(text.contains("\"U1\"") && text.contains("\"U2\""), "the selected parts are in the block's board");
    let groups = board_groups_of(&dir);
    assert_eq!((groups.len(), groups[0].name.as_str(), groups[0].design_block.as_str()), (1, "Pair", "Power:Pair"), "{groups:?}");
    assert_eq!(groups[0].member_ids.len(), 2);
    // the link is one undo step
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert!(board_groups_of(&dir).is_empty());
    // Update Design Block from Selection: the layout is written over (the dialog asks first, `has_design`), the metadata stays
    let p = api::prepare_with(&env, &dir, &req(json!({ "what": "board_selection", "ids": ["U1"], "lib": "Power", "update": { "name": "Pair" } })));
    assert_eq!((p["ok"].clone(), p["has_design"].clone(), p["name"].clone(), p["description"].clone()), (json!(true), json!(true), json!("Pair"), json!("the pair")), "{p}");
    let u = api::save_with(&env, &dir, &req(json!({ "what": "board_selection", "ids": ["U1"], "lib": "Power", "name": "Pair", "update": true })));
    assert_eq!((u["ok"].clone(), u["created"].clone(), u["linked"].clone()), (json!(true), json!(false), json!(true)), "{u}");
    let text = std::fs::read_to_string(lib.join("Pair.kicad_block").join("Pair.kicad_pcb")).unwrap();
    assert!(text.contains("\"U1\"") && !text.contains("\"U2\""), "only U1 is in it now");
    assert_eq!(db::load(&lib, "Pair").unwrap().description, "the pair");
    // a single footprint is a group of one that is linked: kept
    let g = board_groups_of(&dir);
    assert_eq!((g.len(), g[0].member_ids.clone(), g[0].design_block.as_str()), (1, vec!["U1".to_string()], "Power:Pair"));
    let _ = std::fs::remove_dir_all(&m.root);
}

// ------------------------------------------------------------------------------------------------ properties and delete

#[test]
fn properties_rename_a_block_and_delete_removes_its_folder_and_a_read_only_library_is_not_written() {
    let (m, env, lib) = with_library("db_props", setup);
    let dir = m.board.clone();
    for name in ["One", "Two"] {
        let r = api::save_with(&env, &dir, &req(json!({ "what": "board", "lib": "Power", "name": name, "description": name })));
        assert_eq!(r["ok"], true, "{r}");
    }
    // the metadata changes in place
    let r = api::properties_with(&env, &dir, &req(json!({ "lib": "Power", "name": "One", "description": "first", "keywords": "k", "fields": [{ "name": "A", "value": "1" }, { "name": "A", "value": "2" }] })));
    assert_eq!(r["message"], "Duplicate fields are not allowed.");
    let r = api::properties_with(&env, &dir, &req(json!({ "lib": "Power", "name": "One", "description": "first", "keywords": "k", "fields": [{ "name": "A", "value": "1" }] })));
    assert_eq!((r["ok"].clone(), r["renamed"].clone()), (json!(true), json!(false)), "{r}");
    assert_eq!(db::load(&lib, "One").unwrap().fields, vec![("A".to_string(), "1".to_string())]);
    // a new name saves the block under it and removes the old one; one the library has asks first
    let clash = api::properties_with(&env, &dir, &req(json!({ "lib": "Power", "name": "One", "new_name": "Two", "description": "" })));
    assert_eq!((clash["ok"].clone(), clash["exists"].clone()), (json!(false), json!(true)), "{clash}");
    let bad = api::properties_with(&env, &dir, &req(json!({ "lib": "Power", "name": "One", "new_name": "a/b", "description": "" })));
    assert_eq!(bad["message"], "Illegal character '/' in name 'a/b'.");
    let renamed = api::properties_with(&env, &dir, &req(json!({ "lib": "Power", "name": "One", "new_name": "Uno", "description": "first", "keywords": "k", "fields": [{ "name": "A", "value": "1" }] })));
    assert_eq!((renamed["ok"].clone(), renamed["lib_id"].clone()), (json!(true), json!("Power:Uno")), "{renamed}");
    assert_eq!(names_in(&api::list_with(&env, &dir), "Power"), vec!["Two", "Uno"]);
    assert!(lib.join("Uno.kicad_block").join("Uno.kicad_pcb").is_file(), "the layout went with it");
    // Delete removes the block's whole folder
    let d = api::delete_with(&env, &dir, &req(json!({ "lib": "Power", "name": "Two" })));
    assert_eq!(d["ok"], true, "{d}");
    assert!(!lib.join("Two.kicad_block").exists());
    assert_eq!(api::delete_with(&env, &dir, &req(json!({ "lib": "Power", "name": "Two" })))["ok"], false);
    // a library that is read only is not written
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&lib, std::fs::Permissions::from_mode(0o555)).unwrap();
        let ro = api::save_with(&env, &dir, &req(json!({ "what": "board", "lib": "Power", "name": "Three" })));
        assert_eq!(ro["message"], "Library 'Power' is read only.");
        assert_eq!(api::delete_with(&env, &dir, &req(json!({ "lib": "Power", "name": "Uno" })))["message"], "Library 'Power' is read only.");
        assert!(api::list_with(&env, &dir)["libraries"][0]["writable"] == json!(false));
        std::fs::set_permissions(&lib, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let _ = std::fs::remove_dir_all(&m.root);
}

// ------------------------------------------------------------------------------------------------ the schematic half

#[test]
fn a_selection_of_the_sheet_saved_as_a_block_is_grouped_and_linked_and_placed_again_through_import_sheet() {
    let (m, env, lib) = with_library("db_sch_sel", mcu30_flat);
    let dir = m.board.clone();
    let (_, design, _) = load(&dir).unwrap();
    let section = design.schematic.as_ref().unwrap();
    let pick: Vec<String> = ["U1", "C1", "R1"].iter().filter(|r| section.symbols.iter().any(|s| &s.id == *r)).map(|s| s.to_string()).collect();
    assert!(pick.len() >= 2, "mcu30 has parts to pick: {:?}", section.symbols.iter().map(|s| &s.id).collect::<Vec<_>>());
    assert_eq!(api::prepare_with(&env, &dir, &req(json!({ "what": "selection", "ids": [], "lib": "Power" })))["message"], "Please select some items to save as a design block.");
    assert_eq!(api::prepare_with(&env, &dir, &req(json!({ "what": "selection", "ids": pick, "lib": "" })))["message"], "Please select a library to save the design block to.");
    let p = api::prepare_with(&env, &dir, &req(json!({ "what": "selection", "ids": pick, "lib": "Power" })));
    assert_eq!((p["ok"].clone(), p["name"].clone()), (json!(true), json!("mcu_board_30plus")), "named after the project when the sheet is the root: {p}");
    let r = api::save_with(&env, &dir, &req(json!({ "what": "selection", "ids": pick, "lib": "Power", "name": "Trio", "description": "a few parts", "keywords": "mcu" })));
    assert_eq!((r["ok"].clone(), r["linked"].clone()), (json!(true), json!(true)), "{r}");
    let file = lib.join("Trio.kicad_block").join("Trio.kicad_sch");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("(kicad_sch") && text.contains("(lib_symbols") && text.contains("(sheet_instances"), "a schematic file");
    assert_eq!(text.matches("\n\t(symbol\n").count(), pick.len(), "the selected symbols and nothing else");
    // the selection is a group named for the block and linked to it, made in one undo step
    let groups = groups_of(&dir);
    assert_eq!((groups.len(), groups[0].name.as_str(), groups[0].design_block.as_str()), (1, "Trio", "Power:Trio"), "{groups:?}");
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert!(groups_of(&dir).is_empty());

    // Place Design Block: Import Sheet with the block's file, contents on the sheet and grouped, one undo step; the references are numbered round the design's
    let (_, before, _) = load(&dir).unwrap();
    let cmd = Cmd::ImportSheet {
        source: file.display().to_string(),
        content: None,
        as_sheet: false,
        name: String::new(),
        file: String::new(),
        at: Point { x: 250_000_000, y: 40_000_000 },
        size: (0, 0),
        keep_annotations: false,
        order: eda_ops::AnnotateOrder::XThenY,
        options: Default::default(),
        as_group: true,
        group_name: String::new(),
        block: "Power:Trio".into(),
    };
    step(&dir, cmd, false, "test").unwrap();
    let (_, after, model) = load(&dir).unwrap();
    let (n_before, n_after) = (before.schematic.as_ref().unwrap().symbols.len(), after.schematic.as_ref().unwrap().symbols.len());
    assert_eq!(n_after, n_before + pick.len(), "the block's parts came in");
    let group = groups_of(&dir).into_iter().find(|g| g.design_block == "Power:Trio").expect("a linked group");
    assert_eq!((group.name.as_str(), group.member_ids.len()), ("Trio", pick.len()));
    assert!(group.member_ids.iter().all(|id| after.schematic.as_ref().unwrap().symbols.iter().any(|s| &s.id == id)), "the group lists the new references: {:?}", group.member_ids);
    assert!(group.member_ids.iter().all(|id| !pick.contains(id)), "numbered anew: {:?}", group.member_ids);
    assert!(group.member_ids.iter().all(|id| model.part(id).is_some()), "and each is a part of the board");
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    assert!(groups_of(&dir).is_empty() && load(&dir).unwrap().1.schematic.unwrap().symbols.len() == n_before);
    let _ = std::fs::remove_dir_all(&m.root);
}

#[test]
fn a_sheet_saved_as_a_block_is_the_sheets_file_and_a_sheet_with_sheets_in_it_is_refused() {
    let (m, env, lib) = with_library("db_sch_sheet", mcu30_flat);
    let dir = m.board.clone();
    let r = api::save_with(&env, &dir, &req(json!({ "what": "sheet", "sheet": "", "lib": "Power", "name": "Whole", "fields": [{ "name": "Maker", "value": "me" }] })));
    assert_eq!((r["ok"].clone(), r["linked"].clone()), (json!(true), json!(false)), "{r}");
    let file = lib.join("Whole.kicad_block").join("Whole.kicad_sch");
    let sheet = eda_kicad::load_sheet_file(&file).unwrap();
    assert!(sheet.root.symbols.len() >= 30, "mcu30's parts are in the block: {}", sheet.root.symbols.len());
    assert!(sheet.notes.is_empty(), "{:?}", sheet.notes);
    assert_eq!(db::load(&lib, "Whole").unwrap().fields, vec![("Maker".to_string(), "me".to_string())]);
    // no group is made for a sheet, and nothing on the sheet changed
    assert!(groups_of(&dir).is_empty());
    // a sheet that places sheets cannot be a block ("Design blocks with nested sheets are not supported.")
    step(&dir, Cmd::AddSheet { name: "Sub".into(), file: "sub.kicad_sch".into(), at: Point { x: 250_000_000, y: 40_000_000 }, size: (40_000_000, 20_000_000) }, false, "test").unwrap();
    let nested = api::prepare_with(&env, &dir, &req(json!({ "what": "sheet", "sheet": "", "lib": "Power" })));
    assert_eq!(nested["message"], "Design blocks with nested sheets are not supported.");
    let nested = api::save_with(&env, &dir, &req(json!({ "what": "sheet", "sheet": "", "lib": "Power", "name": "Nested" })));
    assert_eq!(nested["message"], "Design blocks with nested sheets are not supported.");
    assert!(!lib.join("Nested.kicad_block").exists());
    // the sheet alone, selected, is saved as a sheet (`SaveSelectionAsDesignBlock` hands a lone sheet to `SaveSheetAsDesignBlock`); with another item it is refused
    let (_, d, _) = load(&dir).unwrap();
    let sheet_id = d.schematic.as_ref().unwrap().sheets[0].id.clone();
    let lone = api::prepare_with(&env, &dir, &req(json!({ "what": "selection", "sheet": "", "ids": [sheet_id], "lib": "Power" })));
    assert_eq!((lone["ok"].clone(), lone["name"].clone()), (json!(true), json!("Sub")), "{lone}");
    let two = api::prepare_with(&env, &dir, &req(json!({ "what": "selection", "sheet": "", "ids": [sheet_id, "U1"], "lib": "Power" })));
    assert_eq!(two["message"], "Design blocks with nested sheets are not supported.");
    let _ = std::fs::remove_dir_all(&m.root);
}

#[test]
fn a_block_placed_as_a_sheet_is_a_sheet_of_its_own_in_a_linked_group() {
    let (m, env, lib) = with_library("db_sch_as_sheet", mcu30_flat);
    let dir = m.board.clone();
    let (_, design, _) = load(&dir).unwrap();
    let pick: Vec<String> = design.schematic.as_ref().unwrap().symbols.iter().take(3).map(|s| s.id.clone()).collect();
    api::save_with(&env, &dir, &req(json!({ "what": "selection", "ids": pick, "lib": "Power", "name": "Trio" })));
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    let file = lib.join("Trio.kicad_block").join("Trio.kicad_sch");
    let cmd = Cmd::ImportSheet {
        source: file.display().to_string(),
        content: None,
        as_sheet: true,
        name: "Trio".into(),
        file: "trio.kicad_sch".into(),
        at: Point { x: 250_000_000, y: 40_000_000 },
        size: (40_000_000, 20_000_000),
        keep_annotations: false,
        order: eda_ops::AnnotateOrder::XThenY,
        options: Default::default(),
        as_group: true,
        group_name: String::new(),
        block: "Power:Trio".into(),
    };
    step(&dir, cmd, false, "test").unwrap();
    let (_, after, _) = load(&dir).unwrap();
    let root = after.schematic.as_ref().unwrap();
    let sheet = root.sheets.iter().find(|s| s.name == "Trio").expect("the sheet");
    assert_eq!(after.sheet_contents.as_ref().unwrap()[&sheet.file].symbols.len(), pick.len());
    assert_eq!((root.extras.groups.len(), root.extras.groups[0].design_block.as_str(), root.extras.groups[0].member_ids.clone()), (1, "Power:Trio", vec![sheet.id.clone()]), "{:?}", root.extras.groups);
    undo(&dir, "test", Some(Domain::Schematic)).unwrap();
    let (_, back, _) = load(&dir).unwrap();
    assert!(back.schematic.as_ref().unwrap().sheets.is_empty() && back.schematic.as_ref().unwrap().extras.groups.is_empty());
    let _ = std::fs::remove_dir_all(&m.root);
    let _ = scratch;
}

// ------------------------------------------------------------------------------------------------ kicad-cli (the slow tier)

/// A report file kicad-cli wrote only when it could read the file it was given.
fn report_of(work: &Path, args: &[&str], report: &str, file: &Path) -> Value {
    let report_path = work.join(report);
    let _ = std::fs::remove_file(&report_path);
    let mut all: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsStr::new).collect();
    all.extend([std::ffi::OsStr::new("--format"), std::ffi::OsStr::new("json"), std::ffi::OsStr::new("--output"), report_path.as_os_str(), file.as_os_str()]);
    let said = kicad_cli(work, &all);
    assert!(report_path.exists(), "kicad-cli did not read {}: {said:?}", file.display());
    serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap()).expect("the report is JSON")
}

#[test]
fn kicad_cli_loads_the_schematic_blocks_this_studio_writes() {
    if !slow() || eda_kicad_engine::find_cli().is_none() {
        return;
    }
    let (m, env, lib) = with_library("db_cli_sch", mcu30_flat);
    let dir = m.board.clone();
    let (_, design, _) = load(&dir).unwrap();
    let pick: Vec<String> = design.schematic.as_ref().unwrap().symbols.iter().take(4).map(|s| s.id.clone()).collect();
    let a = api::save_with(&env, &dir, &req(json!({ "what": "selection", "ids": pick, "lib": "Power", "name": "Selected" })));
    let b = api::save_with(&env, &dir, &req(json!({ "what": "sheet", "sheet": "", "lib": "Power", "name": "Sheet" })));
    assert!(a["ok"] == true && b["ok"] == true, "{a} {b}");
    for name in ["Selected", "Sheet"] {
        let file = lib.join(format!("{name}.kicad_block")).join(format!("{name}.kicad_sch"));
        // `sch export svg`: the file loads and is drawn
        let out = m.root.join(format!("svg_{name}"));
        kicad_cli(&m.root, &["sch".as_ref(), "export".as_ref(), "svg".as_ref(), "--output".as_ref(), out.as_os_str(), file.as_os_str()]).unwrap_or_else(|e| panic!("sch export svg {name}: {e}"));
        let svg = std::fs::read_to_string(out.join(format!("{name}.svg"))).expect("the svg");
        assert!(svg.contains("<svg") && svg.contains("<path"), "{name}: drawn");
        // and `sch erc` reads it (a report exists only when the file loaded)
        let erc = report_of(&m.root, &["sch", "erc"], &format!("erc_{name}.json"), &file);
        assert!(erc.get("sheets").is_some(), "{name}: {erc}");
    }
    // the selection's block has just its symbols; the sheet's has them all
    let count = |name: &str| std::fs::read_to_string(lib.join(format!("{name}.kicad_block")).join(format!("{name}.kicad_sch"))).unwrap().matches("(lib_id \"").count();
    assert!(count("Selected") == 4 && count("Sheet") >= 30, "{} {}", count("Selected"), count("Sheet"));
    // the panel's own preview of a block is the same run
    let preview = api::preview_with(&env, &dir, "Power", "Selected", "sch");
    assert_eq!(preview["ok"], true, "{preview}");
    assert!(preview["svg"].as_str().unwrap().contains("<svg"));
    let again = api::preview_with(&env, &dir, "Power", "Selected", "sch");
    assert_eq!(again["cached"], true, "a second look does not run kicad-cli again");
    let _ = std::fs::remove_dir_all(&m.root);
}

#[test]
fn kicad_cli_loads_the_board_blocks_this_studio_writes() {
    if !slow() || eda_kicad_engine::find_cli().is_none() {
        return;
    }
    let (m, env, lib) = with_library("db_cli_pcb", setup);
    let dir = m.board.clone();
    // a track and a via so the layout has copper as well as footprints
    let (_, d, _) = load(&dir).unwrap();
    assert_eq!(d.placement.as_ref().unwrap().footprints.len(), 2);
    let whole = api::save_with(&env, &dir, &req(json!({ "what": "board", "lib": "Power", "name": "Whole" })));
    let part = api::save_with(&env, &dir, &req(json!({ "what": "board_selection", "ids": ["U1"], "lib": "Power", "name": "One" })));
    assert!(whole["ok"] == true && part["ok"] == true, "{whole} {part}");
    for name in ["Whole", "One"] {
        let file = lib.join(format!("{name}.kicad_block")).join(format!("{name}.kicad_pcb"));
        let drc = report_of(&m.root, &["pcb", "drc"], &format!("drc_{name}.json"), &file);
        assert!(drc.get("violations").is_some(), "{name}: {drc}");
        let out = m.root.join(format!("{name}.svg"));
        kicad_cli(&m.root, &["pcb".as_ref(), "export".as_ref(), "svg".as_ref(), "--mode-single".as_ref(), "--layers".as_ref(), "F.Cu".as_ref(), "--output".as_ref(), out.as_os_str(), file.as_os_str()]).unwrap_or_else(|e| panic!("pcb export svg {name}: {e}"));
        assert!(out.exists());
    }
    // the preview of the layout
    let preview = api::preview_with(&env, &dir, "Power", "Whole", "pcb");
    assert_eq!(preview["ok"], true, "{preview}");
    assert!(preview["svg"].as_str().unwrap().contains("<svg"));
    let _ = std::fs::remove_dir_all(&m.root);
}

/// KiCad's own `Arduino_Nano` template (a schematic with two 15-pin headers, five power symbols and 28 labels; a board of six footprints on 59 nets) made into a block of
/// a library, and placed on the studio's own boards.
#[test]
fn a_block_made_from_one_of_kicads_own_templates_places_with_its_parts_and_nets() {
    if !slow() || eda_kicad_engine::find_cli().is_none() {
        return;
    }
    let template = Path::new("/Applications/KiCad/KiCad.app/Contents/SharedSupport/template/Arduino_Nano");
    if !template.join("Arduino_Nano.kicad_sch").exists() {
        eprintln!("skipped: no KiCad template folder");
        return;
    }
    let (m, env, lib) = with_library("db_template", mcu30_flat);
    let dir = m.board.clone();
    // the block as KiCad's tools would leave it: the template's own two files and a metadata file
    let block = db::DesignBlock {
        name: "Nano".into(),
        description: "An Arduino Nano's headers".into(),
        keywords: "arduino nano".into(),
        fields: vec![("Family".into(), "Arduino".into())],
        schematic_file: Some(template.join("Arduino_Nano.kicad_sch")),
        board_file: Some(template.join("Arduino_Nano.kicad_pcb")),
    };
    db::save(&lib, &block).unwrap();
    let list = api::list_with(&env, &dir);
    assert_eq!(names_in(&list, "Power"), vec!["Nano"]);
    let json_block = &list["libraries"][0]["blocks"][0];
    assert_eq!((json_block["schematic"].clone(), json_block["board"].clone()), (json!(true), json!(true)));
    let sch_path = json_block["schematic_path"].as_str().unwrap().to_string();

    // the schematic: the block is read through Import Sheet and its parts and nets are the design's
    let (_, before, _) = load(&dir).unwrap();
    let cmd = Cmd::ImportSheet {
        source: sch_path.clone(),
        content: None,
        as_sheet: false,
        name: String::new(),
        file: String::new(),
        at: Point { x: 250_000_000, y: 40_000_000 },
        size: (0, 0),
        keep_annotations: false,
        order: eda_ops::AnnotateOrder::XThenY,
        options: Default::default(),
        as_group: true,
        group_name: String::new(),
        block: "Power:Nano".into(),
    };
    step(&dir, cmd, false, "test").unwrap();
    let (_, after, model) = load(&dir).unwrap();
    let new_symbols: Vec<&eda_model::ir::SymbolInstance> = after.schematic.as_ref().unwrap().symbols.iter().filter(|s| !before.schematic.as_ref().unwrap().symbols.iter().any(|b| b.id == s.id && b.unit == s.unit)).collect();
    assert_eq!(new_symbols.len(), 2, "J1 and J2 of the template: {:?}", new_symbols.iter().map(|s| (&s.id, &s.lib_id)).collect::<Vec<_>>());
    assert!(new_symbols.iter().all(|s| s.lib_id == "Connector_Generic:Conn_01x15"));
    assert!(new_symbols.iter().all(|s| model.part(&s.id).is_some()), "each is a part of the board");
    let group = groups_of(&dir).into_iter().find(|g| g.design_block == "Power:Nano").expect("the linked group");
    assert_eq!(group.name, "Nano");
    // the nets: the labels of the template name the nets its headers' pins are on
    let nets = after.nets.as_ref().expect("a schematic edit stores the net list");
    let on_net = |net: &str| nets.iter().find(|n| n.name == net).map(|n| n.pins.iter().filter(|p| new_symbols.iter().any(|s| p.starts_with(&format!("{}.", s.id)))).count()).unwrap_or(0);
    for net in ["D2", "A0", "D13{slash}SCK"] {
        assert!(on_net(net) >= 1, "{net}: {:?}", nets.iter().filter(|n| n.name.contains(net.trim_end_matches("{slash}SCK"))).collect::<Vec<_>>());
    }

    // the layout on a board of the studio's own: six footprints with the template's pad nets
    let m2 = machine("db_template_pcb", true);
    setup(&m2.board);
    let r = library_tables::add_library_with(&m2.env(), &m2.board, &req(json!({ "kind": "design_block", "path": lib.display().to_string() })));
    assert_eq!(r["ok"], true, "{r}");
    let env2 = m2.env();
    let file = api::file_with(&env2, &m2.board, "Power", "Nano", "pcb");
    assert_eq!(file["ok"], true, "{file}");
    let (_, board_before, _) = load(&m2.board).unwrap();
    let cmd = Cmd::PlaceDesignBlock { text: file["text"].as_str().unwrap().to_string(), at: Point { x: 100_000_000, y: 60_000_000 }, block: "Power:Nano".into(), as_group: true, group_name: String::new() };
    step(&m2.board, cmd, false, "test").unwrap();
    let (_, board_after, model2) = load(&m2.board).unwrap();
    let added = board_after.placement.as_ref().unwrap().footprints.len() - board_before.placement.as_ref().unwrap().footprints.len();
    assert_eq!(added, 6000, "the template's six footprints");
    assert!(model2.parts.len() >= 8);
    let parts = &board_after.drawings.as_ref().unwrap().board_parts;
    let netted: usize = parts.iter().map(|p| p.pad_nets.len()).sum();
    assert!(netted >= 30, "the headers' thirty pads bring their nets: {netted}");
    assert!(parts.iter().any(|p| p.pad_nets.iter().any(|(_, n)| n == "/D2")), "and the template's net names (the board's own, with the sheet's slash)");
    assert!(board_groups_of(&m2.board).iter().any(|g| g.design_block == "Power:Nano"));
    // the link is in the files the studio derives, and kicad-cli reads them and writes the link back (`sch upgrade` / `pcb upgrade` are its own writers)
    let meta = eda_kicad::ExportMeta { date: "2026-10-20", title: "blocks" };
    let sch_files = eda_kicad::export_kicad_sch_tree(&after, &model, &meta, "board.kicad_sch").unwrap();
    let sch_text = &sch_files.iter().find(|(n, _)| n == "board.kicad_sch").unwrap().1;
    assert!(sch_text.contains("(lib_id \"Power:Nano\")"), "the derived schematic carries the group's link");
    let work = m.root.join("derived");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("board.kicad_sch"), sch_text).unwrap();
    let erc = report_of(&work, &["sch", "erc"], "erc.json", &work.join("board.kicad_sch"));
    assert!(erc.get("sheets").is_some(), "{erc}");
    kicad_cli(&work, &["sch".as_ref(), "upgrade".as_ref(), "--force".as_ref(), work.join("board.kicad_sch").as_os_str()]).unwrap();
    assert!(std::fs::read_to_string(work.join("board.kicad_sch")).unwrap().contains("(lib_id \"Power:Nano\")"), "kicad-cli kept the schematic group's link");
    let pcb_text = eda_kicad::export_kicad_pcb(&board_after, &model2, &meta).unwrap();
    assert!(pcb_text.contains("(lib_id \"Power:Nano\")"), "the derived board carries the group's link");
    std::fs::write(work.join("board.kicad_pcb"), &pcb_text).unwrap();
    let drc = report_of(&work, &["pcb", "drc"], "drc.json", &work.join("board.kicad_pcb"));
    assert!(drc.get("violations").is_some(), "{drc}");
    kicad_cli(&work, &["pcb".as_ref(), "upgrade".as_ref(), "--force".as_ref(), work.join("board.kicad_pcb").as_os_str()]).unwrap();
    assert!(std::fs::read_to_string(work.join("board.kicad_pcb")).unwrap().contains("(lib_id \"Power:Nano\")"), "kicad-cli kept the board group's link");
    // both previews are kicad-cli's drawing of the template's files
    for what in ["sch", "pcb"] {
        let p = api::preview_with(&env, &dir, "Power", "Nano", what);
        assert_eq!(p["ok"], true, "{what}: {p}");
        assert!(p["svg"].as_str().unwrap().len() > 2_000, "{what}");
    }
    let _ = std::fs::remove_dir_all(&m.root);
    let _ = std::fs::remove_dir_all(&m2.root);
}

