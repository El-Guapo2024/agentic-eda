//! The verbs of the embedded files (`embedded.rs`): Add, Remove, Embed fonts, Collect and Embed 3D Models, and the wire form of the command.

use super::pcb_transform_tests::{board, design, model};
use super::*;
use eda_kicad::embedded::base64_encode;
use eda_model::embedded_files::EmbeddedFileType;
use eda_model::footprint::Model3d;
use eda_model::units::mm;

const LIB: &str = "${KICAD10_3DMODEL_DIR}/Test.3dshapes/Lib.step";

fn board_files(b: &Board<'_>) -> Vec<String> {
    b.design().drawings.as_ref().map(|d| d.embedded.files.iter().map(|f| f.name.clone()).collect()).unwrap_or_default()
}

fn add(owner: EmbeddedOwner, name: &str, content: &[u8]) -> Cmd {
    Cmd::Embedded(EmbeddedCmd::Add { owner, name: name.into(), data: base64_encode(content), replace: false, kind: None })
}

fn remove(owner: EmbeddedOwner, names: &[&str]) -> Cmd {
    Cmd::Embedded(EmbeddedCmd::Remove { owner, names: names.iter().map(|s| s.to_string()).collect(), nested: false })
}

fn check(b: &mut Board<'_>, cmd: Cmd) -> String {
    let before = serde_json::to_string(b.design()).unwrap();
    let e = b.apply(&cmd).unwrap_err();
    assert_eq!(serde_json::to_string(b.design()).unwrap(), before, "a refused command leaves the design as it was");
    e[0].check.clone()
}

#[test]
fn a_file_is_added_compressed_and_checksummed_and_decodes_back() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let content: Vec<u8> = (0..20_000u32).map(|i| (i % 253_000) as u8).collect();
    b.apply(&add(EmbeddedOwner::Board, "dir/Some Part.STEP", &content)).unwrap();
    let list = &b.design().drawings.as_ref().unwrap().embedded;
    assert_eq!(list.files.len(), 1);
    let f = &list.files[0];
    assert_eq!((f.name.as_str(), f.kind), ("Some Part.STEP", EmbeddedFileType::Model), "filed under its file name, typed from its extension");
    assert_eq!(f.link(), "kicad-embed://Some Part.STEP");
    assert!(f.data.starts_with("KLUv/") && f.data.len() < content.len(), "a zstd frame, smaller than the content");
    assert_eq!(eda_kicad::embedded::decode(f).unwrap().bytes, content);
    // the same file again is nothing to change; other content needs the dialog's "Replace"
    assert_eq!(check(&mut b, add(EmbeddedOwner::Board, "Some Part.STEP", &content)), "ops_nothing_to_change");
    assert_eq!(check(&mut b, add(EmbeddedOwner::Board, "Some Part.STEP", b"other")), "ops_embedded_exists");
    b.apply(&Cmd::Embedded(EmbeddedCmd::Add { owner: EmbeddedOwner::Board, name: "Some Part.STEP".into(), data: base64_encode(b"other"), replace: true, kind: None })).unwrap();
    assert_eq!(eda_kicad::embedded::decode(&b.design().drawings.as_ref().unwrap().embedded.files[0]).unwrap().bytes, b"other");
    assert_eq!(b.design().drawings.as_ref().unwrap().embedded.files.len(), 1, "replaced, not added");
}

#[test]
fn bad_files_are_refused_with_a_reason() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    assert_eq!(check(&mut b, add(EmbeddedOwner::Board, "", b"x")), "ops_bad_embedded_name");
    assert_eq!(check(&mut b, add(EmbeddedOwner::Board, "dir/", b"x")), "ops_bad_embedded_name");
    assert_eq!(check(&mut b, Cmd::Embedded(EmbeddedCmd::Add { owner: EmbeddedOwner::Board, name: "x.bin".into(), data: "not base64!".into(), replace: false, kind: None })), "ops_bad_embedded_data");
    assert_eq!(check(&mut b, remove(EmbeddedOwner::Board, &["nothing.step"])), "ops_unknown_embedded_file");
    assert_eq!(check(&mut b, remove(EmbeddedOwner::Board, &[])), "ops_nothing_to_change");
    assert_eq!(check(&mut b, add(EmbeddedOwner::Footprint { reference: "U9".into() }, "x.bin", b"x")), "ops_not_placed");
    assert_eq!(check(&mut b, add(EmbeddedOwner::LibraryFootprint { name: "Nope".into() }, "x.bin", b"x")), "ops_unknown_footprint");
    assert_eq!(check(&mut b, add(EmbeddedOwner::LibrarySymbol { lib_id: "Nope:X".into() }, "x.bin", b"x")), "ops_unknown_symbol");
    assert_eq!(check(&mut b, add(EmbeddedOwner::Schematic, "x.bin", b"x")), "ops_no_schematic", "the studio stores the schematic first (`step_quiet`)");
}

#[test]
fn files_are_removed_by_name_or_by_link_and_the_fonts_box_drops_the_fonts() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    for (name, content) in [("a.pdf", &b"%PDF a"[..]), ("b.step", b"ISO-10303-21;"), ("c.ttf", b"font")] {
        b.apply(&add(EmbeddedOwner::Board, name, content)).unwrap();
    }
    assert_eq!(board_files(&b), ["a.pdf", "b.step", "c.ttf"]);
    b.apply(&remove(EmbeddedOwner::Board, &["kicad-embed://b.step"])).unwrap();
    assert_eq!(board_files(&b), ["a.pdf", "c.ttf"], "the panel's Remove takes the link as well as the name");
    // the box: switching it either way removes the fonts that are in the list
    b.apply(&Cmd::Embedded(EmbeddedCmd::SetFonts { owner: EmbeddedOwner::Board, embed: true })).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().embedded.fonts);
    assert_eq!(board_files(&b), ["a.pdf"]);
    assert_eq!(check(&mut b, Cmd::Embedded(EmbeddedCmd::SetFonts { owner: EmbeddedOwner::Board, embed: true })), "ops_nothing_to_change");
    b.apply(&Cmd::Embedded(EmbeddedCmd::SetFonts { owner: EmbeddedOwner::Board, embed: false })).unwrap();
    assert!(!b.design().drawings.as_ref().unwrap().embedded.fonts);
    b.apply(&remove(EmbeddedOwner::Board, &["a.pdf"])).unwrap();
    assert!(b.design().drawings.as_ref().unwrap().embedded.is_empty(), "an empty list is nothing at all");
}

#[test]
fn a_footprint_on_the_board_has_a_list_of_its_own_and_the_board_can_take_a_file_out_of_all_of_them() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    let u1 = EmbeddedOwner::Footprint { reference: "U1".into() };
    let u2 = EmbeddedOwner::Footprint { reference: "U2".into() };
    b.apply(&add(u1.clone(), "part.step", b"model")).unwrap();
    b.apply(&add(u2.clone(), "part.step", b"model")).unwrap();
    let edit = |b: &Board<'_>, r: &str| b.design().footprint_edit(r).and_then(|e| e.embedded.clone());
    assert_eq!(edit(&b, "U1").unwrap().files.len(), 1);
    assert!(board_files(&b).is_empty(), "the footprint's file is the footprint's");
    // the list the board writes holds it once, with the data
    let all = eda_model::embedded_files::board_file_list(b.design());
    assert_eq!(all.files.len(), 1);
    // the board holds the same file: Board Setup > Embedded Files removes it, and with `nested` takes it out of the footprints
    b.apply(&add(EmbeddedOwner::Board, "part.step", b"model")).unwrap();
    b.apply(&Cmd::Embedded(EmbeddedCmd::Remove { owner: EmbeddedOwner::Board, names: vec!["part.step".into()], nested: true })).unwrap();
    assert!(board_files(&b).is_empty());
    assert!(edit(&b, "U1").is_none() && edit(&b, "U2").is_none(), "an emptied list is no edit when the board file gave the footprint none");
    assert!(b.design().drawings.as_ref().unwrap().footprint_edits.is_empty());
}

#[test]
fn library_footprints_and_symbols_keep_their_files_and_move_them_with_a_rename() {
    let m = ConstraintModel::default();
    let mut b = Board::new(design(), &m, mm(0.1), mm(0.3));
    b.apply(&Cmd::NewFootprint { name: "Mine".into() }).unwrap();
    let owner = EmbeddedOwner::LibraryFootprint { name: "Mine".into() };
    b.apply(&add(owner.clone(), "m.step", b"model")).unwrap();
    assert_eq!(b.design().footprint_library.as_ref().unwrap().embedded["Mine"].files.len(), 1);
    b.apply(&Cmd::RenameLibraryFootprint { name: "Mine".into(), new_name: "Yours".into(), overwrite: false }).unwrap();
    let lib = b.design().footprint_library.as_ref().unwrap();
    assert!(lib.embedded.contains_key("Yours") && !lib.embedded.contains_key("Mine"));
    b.apply(&Cmd::DeleteLibraryFootprint { name: "Yours".into() }).unwrap();
    assert!(b.design().footprint_library.as_ref().unwrap().embedded.is_empty());

    b.apply(&Cmd::NewSymbol { lib_id: "Lib:S".into() }).unwrap();
    let owner = EmbeddedOwner::LibrarySymbol { lib_id: "Lib:S".into() };
    b.apply(&add(owner.clone(), "d.pdf", b"%PDF")).unwrap();
    b.apply(&Cmd::RenameLibrarySymbol { lib_id: "Lib:S".into(), new_lib_id: "Lib:T".into(), overwrite: false }).unwrap();
    assert!(b.design().symbol_library.as_ref().unwrap().embedded.contains_key("Lib:T"));
    // the whole list replaced (a footprint or a symbol imported from a file brings its files)
    let list = b.design().symbol_library.as_ref().unwrap().embedded["Lib:T"].clone();
    let owner = EmbeddedOwner::LibrarySymbol { lib_id: "Lib:T".into() };
    assert_eq!(check(&mut b, Cmd::Embedded(EmbeddedCmd::SetList { owner: owner.clone(), list: list.clone() })), "ops_nothing_to_change");
    b.apply(&Cmd::Embedded(EmbeddedCmd::SetList { owner, list: Default::default() })).unwrap();
    assert!(b.design().symbol_library.as_ref().unwrap().embedded.is_empty());
}

#[test]
fn collect_and_embed_puts_the_models_in_the_board_and_names_them_by_link() {
    let mut m = model(&["F.Cu", "B.Cu"]);
    eda_model::fp_models::set_models(&mut m.footprints[0], &[Model3d::identity(LIB)]);
    let mut b = board(&m);
    let step = eda_kicad::embedded::encode_new("Lib.step", b"ISO-10303-21; step").unwrap();
    let wrl = eda_kicad::embedded::encode_new("Lib.wrl", b"#VRML V2.0 utf8").unwrap();
    // nothing found: nothing happens
    assert_eq!(check(&mut b, Cmd::Embedded(EmbeddedCmd::CollectModels { models: vec![] })), "ops_nothing_to_change");
    b.apply(&Cmd::Embedded(EmbeddedCmd::CollectModels { models: vec![CollectedModel { path: LIB.into(), files: vec![wrl.clone(), step.clone()] }] })).unwrap();
    // both files are in the board (the VRML model and the STEP beside it), and both footprints name the model by its link
    assert_eq!(board_files(&b), ["Lib.step", "Lib.wrl"]);
    for part in ["U1", "U2"] {
        let list = b.design().footprint_edit(part).and_then(|e| e.models3d.clone()).unwrap();
        assert_eq!(list.iter().map(|m| m.path.as_str()).collect::<Vec<_>>(), ["kicad-embed://Lib.wrl"], "{part}");
    }
    // a second run finds a model that is a link already: nothing to embed
    assert_eq!(check(&mut b, Cmd::Embedded(EmbeddedCmd::CollectModels { models: vec![CollectedModel { path: LIB.into(), files: vec![step] }] })), "ops_nothing_to_change");
}

#[test]
fn the_command_has_a_wire_form_and_a_domain_for_the_undo_scope() {
    let cmd: Cmd = serde_json::from_str(r#"{"op":"embedded","verb":"add","owner":{"kind":"footprint","reference":"U1"},"name":"a.step","data":"QQ=="}"#).unwrap();
    assert_eq!(cmd, Cmd::Embedded(EmbeddedCmd::Add { owner: EmbeddedOwner::Footprint { reference: "U1".into() }, name: "a.step".into(), data: "QQ==".into(), replace: false, kind: None }));
    assert_eq!(serde_json::to_value(&cmd).unwrap()["verb"], "add");
    let cmd: Cmd = serde_json::from_str(r#"{"op":"embedded","verb":"collect_models"}"#).unwrap();
    assert!(matches!(cmd, Cmd::Embedded(EmbeddedCmd::CollectModels { ref models }) if models.is_empty()));
    assert_eq!(Cmd::Embedded(EmbeddedCmd::SetFonts { owner: EmbeddedOwner::Schematic, embed: true }).domain(), Domain::Schematic);
    assert_eq!(Cmd::Embedded(EmbeddedCmd::SetFonts { owner: EmbeddedOwner::Board, embed: true }).domain(), Domain::Pcb);
    assert_eq!(Cmd::Embedded(EmbeddedCmd::SetFonts { owner: EmbeddedOwner::LibraryFootprint { name: "X".into() }, embed: true }).domain(), Domain::FootprintEditor);
    assert_eq!(Cmd::Embedded(EmbeddedCmd::SetFonts { owner: EmbeddedOwner::LibrarySymbol { lib_id: "L:X".into() }, embed: true }).domain(), Domain::SymbolEditor);
    assert!(!Cmd::Embedded(EmbeddedCmd::SetFonts { owner: EmbeddedOwner::Schematic, embed: true }).edits_connectivity());
}

/// `EMBEDDED_FILES::AddFile` fails on an empty file (`decompressedData.data()` is null), and the panel says "Failed to add file '%s'." (`AddEmbeddedFile`).
#[test]
fn an_empty_file_is_not_added_as_kicad_does_not_add_one() {
    let m = model(&["F.Cu", "B.Cu"]);
    let mut b = board(&m);
    assert_eq!(check(&mut b, add(EmbeddedOwner::Board, "empty.txt", b"")), "ops_bad_embedded_data");
    let e = b.apply(&add(EmbeddedOwner::Board, "empty.txt", b"")).unwrap_err();
    assert_eq!(e[0].hint.as_deref(), Some("Failed to add file 'empty.txt'."));
}
