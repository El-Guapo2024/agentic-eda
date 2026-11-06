//! The embedded files through a board directory (GAPS.md items 24 and 30, package P26): what the Embedded Files panel reads (`GET /api/embedded`), the file it extracts
//! (`GET /api/embedded/file`), a footprint's model named `kicad-embed://` found by the 3D model route, and Collect and Embed 3D Models (`PCB_CONTROL::CollectAndEmbed3DModels`)
//! as one undo step that the derived `.kicad_pcb` carries.

use super::tests::{scratch, setup};
use super::{load, step, undo};
use eda_kicad::embedded::base64_encode;
use eda_model::embedded_files::{EmbeddedFileType, EmbeddedOwner};
use eda_model::footprint::Model3d;
use eda_ops::{Cmd, Domain, EmbeddedCmd};
use serde_json::Value;
use std::path::Path;

const VRML: &[u8] = b"#VRML V2.0 utf8\nShape { geometry Box { size 1 1 1 } }\n";
const STEP: &[u8] = b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n";

fn json_of(r: &crate::model3d_api::Reply) -> Value {
    serde_json::from_slice(&r.body).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&r.body)))
}

fn add(owner: EmbeddedOwner, name: &str, content: &[u8]) -> Cmd {
    Cmd::Embedded(EmbeddedCmd::Add { owner, name: name.into(), data: base64_encode(content), replace: false, kind: None })
}

fn derived_board(dir: &Path) -> String {
    let (_, design, model) = load(dir).unwrap();
    eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-10-23", title: "embedded" }).unwrap()
}

fn names_listed(dir: &Path, target: &str) -> Vec<String> {
    let r = crate::embedded_api::list(dir, target);
    assert_eq!(r.status, "200 OK", "{}", String::from_utf8_lossy(&r.body));
    json_of(&r)["files"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn a_file_added_to_the_board_is_listed_extracted_byte_for_byte_and_one_undo_takes_it_back() {
    let dir = scratch("embedded_add_list_extract");
    setup(&dir);
    assert!(names_listed(&dir, "/api/embedded?owner=board").is_empty());

    let content: Vec<u8> = (0..50_000u32).map(|i| (i * 7 % 251_000) as u8).collect();
    step(&dir, add(EmbeddedOwner::Board, "datasheet.pdf", &content), false, "test").unwrap();
    let r = crate::embedded_api::list(&dir, "/api/embedded?owner=board");
    let j = json_of(&r);
    let f = &j["files"][0];
    assert_eq!(f["name"], "datasheet.pdf");
    assert_eq!(f["type"], "datasheet");
    assert_eq!(f["link"], "kicad-embed://datasheet.pdf");
    assert_eq!(f["status"], "ok");
    assert_eq!(f["size"], content.len() as u64);
    assert_eq!(j["fonts"], false);

    // Extract: the bytes that went in, by name or by link
    for name in ["datasheet.pdf", "kicad-embed%3A%2F%2Fdatasheet.pdf"] {
        let got = crate::embedded_api::file(&dir, &format!("/api/embedded/file?owner=board&name={name}"));
        assert_eq!(got.status, "200 OK");
        assert_eq!(got.body, content, "{name}");
    }
    assert_eq!(crate::embedded_api::file(&dir, "/api/embedded/file?owner=board&name=nothing.pdf").status, "404 Not Found");
    assert_eq!(crate::embedded_api::list(&dir, "/api/embedded?owner=footprint").status, "400 Bad Request", "a footprint is named by its reference");
    assert_eq!(crate::embedded_api::list(&dir, "/api/embedded?owner=footprint&id=U9").status, "404 Not Found");

    // the derived board file carries it, and reads back with the same data
    let pcb = derived_board(&dir);
    assert!(pcb.contains("(embedded_files") && pcb.contains("(name \"datasheet.pdf\")"), "{}", &pcb[pcb.len().saturating_sub(600)..]);
    let (back, _, _) = eda_kicad::import_kicad_pcb(&pcb).unwrap();
    let again = back.drawings.unwrap().embedded;
    assert_eq!(eda_kicad::embedded::decode(again.get("datasheet.pdf").unwrap()).unwrap().bytes, content);

    // Embed fonts is a step of its own
    step(&dir, Cmd::Embedded(EmbeddedCmd::SetFonts { owner: EmbeddedOwner::Board, embed: true }), false, "test").unwrap();
    assert_eq!(json_of(&crate::embedded_api::list(&dir, "/api/embedded?owner=board"))["fonts"], true);
    assert!(derived_board(&dir).contains("(embedded_fonts yes)"));
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(json_of(&crate::embedded_api::list(&dir, "/api/embedded?owner=board"))["fonts"], false);

    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert!(names_listed(&dir, "/api/embedded?owner=board").is_empty(), "one undo takes the add back");
    assert!(!derived_board(&dir).contains("embedded_files"));
}

#[test]
fn a_model_named_kicad_embed_is_found_by_the_3d_route_from_the_boards_files() {
    let dir = scratch("embedded_model_route");
    setup(&dir);
    step(&dir, add(EmbeddedOwner::Board, "part.wrl", VRML), false, "test").unwrap();
    step(&dir, Cmd::SetBoardModels { part: "U1".into(), models: vec![Model3d::identity("kicad-embed://part.wrl")] }, false, "test").unwrap();

    let names = crate::library_tables::model_names(&dir);
    let roots = crate::model3d_api::board_roots(&dir, &names);
    let found = crate::model3d_api::resolve("kicad-embed://part.wrl", &roots, &|_| None).unwrap_or_else(|_| panic!("the embedded model resolves"));
    assert_eq!(std::fs::read(&found.path).unwrap(), VRML, "extracted as it was embedded");
    assert!(found.path.starts_with(dir.canonicalize().unwrap().join(".kicad").join("embed")), "{}", found.path.display());
    assert!(crate::model3d_api::finds_file("kicad-embed://part.wrl", &roots, &|_| None));
    assert!(!crate::model3d_api::finds_file("kicad-embed://other.wrl", &roots, &|_| None));
    assert!(crate::model3d_api::resolve("kicad-embed://other.wrl", &roots, &|_| None).is_err());

    // the route itself serves it
    let store = crate::model3d_api::Store::new(dir.join("cache"), std::sync::Arc::new(|ps: &[std::path::PathBuf]| ps.iter().map(|_| Err("no conversion in this test".to_string())).collect()), std::time::Duration::from_millis(1));
    let r = crate::model3d_api::reply(&dir, "/api/3dmodel?name=kicad-embed%3A%2F%2Fpart.wrl", &store);
    assert_eq!((r.status, r.body.as_slice()), ("200 OK", VRML));

    // a footprint on the board lists the file in its own block, and the board's list has the data
    let pcb = derived_board(&dir);
    assert!(pcb.contains("(model \"kicad-embed://part.wrl\""), "{pcb}");
    assert_eq!(pcb.matches("(name \"part.wrl\")").count(), 1, "only the board's block: U1 has no list of its own yet");
}

#[test]
fn collect_and_embed_3d_models_embeds_the_files_renames_the_models_and_is_one_undo_step() {
    let dir = scratch("embedded_collect_models");
    setup(&dir);
    std::fs::create_dir_all(dir.join("models")).unwrap();
    std::fs::write(dir.join("models/part.wrl"), VRML).unwrap();
    std::fs::write(dir.join("models/part.step"), STEP).unwrap();
    let path = "${KIPRJMOD}/models/part.wrl";
    step(&dir, Cmd::SetBoardModels { part: "U1".into(), models: vec![Model3d::identity(path), Model3d::identity("${KIPRJMOD}/models/missing.wrl")] }, false, "test").unwrap();
    step(&dir, Cmd::SetBoardModels { part: "U2".into(), models: vec![Model3d::identity(path)] }, false, "test").unwrap();

    // what the action would do
    let plan = json_of(&crate::embedded_api::collect_plan(&dir));
    assert_eq!(plan["count"], 2, "{plan}");
    assert_eq!(plan["models"][0]["path"], path);
    assert_eq!(plan["models"][0]["files"], serde_json::json!(["part.wrl", "part.step"]), "the STEP beside the VRML, for the STEP exporter");
    assert_eq!(plan["models"][0]["footprints"], serde_json::json!(["U1", "U2"]));
    assert_eq!(plan["skipped"].as_array().unwrap().len(), 1, "{plan}");

    // the command as the studio sends it carries no files: the server reads them in
    step(&dir, Cmd::Embedded(EmbeddedCmd::CollectModels { models: vec![] }), false, "test").unwrap();
    let (_, design, model) = load(&dir).unwrap();
    let list = &design.drawings.as_ref().unwrap().embedded;
    assert_eq!(list.files.iter().map(|f| (f.name.as_str(), f.kind)).collect::<Vec<_>>(), [("part.step", EmbeddedFileType::Model), ("part.wrl", EmbeddedFileType::Model)]);
    assert_eq!(eda_kicad::embedded::decode(list.get("part.step").unwrap()).unwrap().bytes, STEP);
    let paths = |r: &str| model.footprint_of(model.part(r).unwrap()).unwrap().models3d.iter().map(|m| m.path.clone()).collect::<Vec<_>>();
    assert_eq!(paths("U1"), ["kicad-embed://part.wrl", "${KIPRJMOD}/models/missing.wrl"], "a model that names no file is left as it was");
    assert_eq!(paths("U2"), ["kicad-embed://part.wrl"]);
    // the journal kept the files: the step replays where the folder is not
    let pcb = derived_board(&dir);
    assert!(pcb.contains("(name \"part.wrl\")") && pcb.contains("(name \"part.step\")") && pcb.contains("(model \"kicad-embed://part.wrl\""));
    // the model now comes out of the board, not the folder
    std::fs::remove_dir_all(dir.join("models")).unwrap();
    let names = crate::library_tables::model_names(&dir);
    let found = crate::model3d_api::resolve("kicad-embed://part.wrl", &crate::model3d_api::board_roots(&dir, &names), &|_| None).ok().expect("served from the board");
    assert_eq!(std::fs::read(found.path).unwrap(), VRML);

    // nothing left to collect
    assert_eq!(json_of(&crate::embedded_api::collect_plan(&dir))["count"], 0);
    let e = step(&dir, Cmd::Embedded(EmbeddedCmd::CollectModels { models: vec![] }), false, "test").unwrap_err();
    assert_eq!(e[0].check, "ops_nothing_to_change");

    // one undo: the files and the names together
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    let (_, design, model) = load(&dir).unwrap();
    assert!(design.drawings.as_ref().is_none_or(|d| d.embedded.files.is_empty()));
    let paths = |r: &str| model.footprint_of(model.part(r).unwrap()).unwrap().models3d.iter().map(|m| m.path.clone()).collect::<Vec<_>>();
    assert_eq!(paths("U2"), [path]);
}

#[test]
fn a_footprint_lists_its_own_files_and_board_setup_shows_the_ones_only_footprints_hold() {
    let dir = scratch("embedded_footprint_owner");
    setup(&dir);
    let u1 = EmbeddedOwner::Footprint { reference: "U1".into() };
    step(&dir, add(u1.clone(), "U1.step", STEP), false, "test").unwrap();
    assert_eq!(names_listed(&dir, "/api/embedded?owner=footprint&id=U1"), ["U1.step"]);
    assert!(names_listed(&dir, "/api/embedded?owner=footprint&id=U2").is_empty());
    let board = json_of(&crate::embedded_api::list(&dir, "/api/embedded?owner=board"));
    assert!(board["files"].as_array().unwrap().is_empty());
    assert_eq!(board["inherited"][0]["name"], "U1.step", "{board}");
    assert_eq!(board["inherited"][0]["used_by"], serde_json::json!(["U1"]));
    let got = crate::embedded_api::file(&dir, "/api/embedded/file?owner=footprint&id=U1&name=U1.step");
    assert_eq!(got.body, STEP);

    // the board file: the board's block has the data, the footprint's names it
    let pcb = derived_board(&dir);
    assert_eq!(pcb.matches("(name \"U1.step\")").count(), 2, "the board's list and U1's");
    let (back, _, _) = eda_kicad::import_kicad_pcb(&pcb).unwrap();
    assert_eq!(back.drawings.as_ref().unwrap().embedded.files.len(), 1);

    step(&dir, Cmd::Embedded(EmbeddedCmd::Remove { owner: u1, names: vec!["kicad-embed://U1.step".into()], nested: false }), false, "test").unwrap();
    assert!(names_listed(&dir, "/api/embedded?owner=footprint&id=U1").is_empty());
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert_eq!(names_listed(&dir, "/api/embedded?owner=footprint&id=U1"), ["U1.step"]);
}

/// `PCB_CONTROL::CollectAndEmbed3DModels` goes through the footprints in their order and `AddFile( fname, false )` keeps the first file of a name: two models of the same file name
/// in two folders are one file, the one of the first footprint, whatever the paths sort as. A model file of any extension that is there is embedded (`fname.Exists()`), and
/// an empty one is not (`AddFile` fails on it: no data to read).
#[test]
fn collect_takes_the_first_footprints_file_of_a_name_any_model_file_and_no_empty_one() {
    let dir = scratch("embedded_collect_order");
    setup(&dir);
    for (sub, content) in [("b", &b"ISO-10303-21; from b"[..]), ("a", b"ISO-10303-21; from a")] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        std::fs::write(dir.join(sub).join("part.step"), content).unwrap();
    }
    std::fs::write(dir.join("a/zipped.stpz"), b"\x1f\x8b not really gzip").unwrap();
    std::fs::write(dir.join("a/empty.step"), b"").unwrap();
    // U1 is placed before U2 and names the folder that sorts last
    step(&dir, Cmd::SetBoardModels { part: "U1".into(), models: vec![Model3d::identity("${KIPRJMOD}/b/part.step"), Model3d::identity("${KIPRJMOD}/a/zipped.stpz")] }, false, "test").unwrap();
    step(&dir, Cmd::SetBoardModels { part: "U2".into(), models: vec![Model3d::identity("${KIPRJMOD}/a/part.step"), Model3d::identity("${KIPRJMOD}/a/empty.step")] }, false, "test").unwrap();
    let plan = json_of(&crate::embedded_api::collect_plan(&dir));
    assert_eq!(plan["count"], 3, "{plan}");
    step(&dir, Cmd::Embedded(EmbeddedCmd::CollectModels { models: vec![] }), false, "test").unwrap();
    let (_, design, model) = load(&dir).unwrap();
    let list = &design.drawings.as_ref().unwrap().embedded;
    assert_eq!(list.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["part.step", "zipped.stpz"]);
    assert_eq!(eda_kicad::embedded::decode(list.get("part.step").unwrap()).unwrap().bytes, b"ISO-10303-21; from b", "the first footprint's file");
    let paths = |r: &str| model.footprint_of(model.part(r).unwrap()).unwrap().models3d.iter().map(|m| m.path.clone()).collect::<Vec<_>>();
    assert_eq!(paths("U1"), ["kicad-embed://part.step", "kicad-embed://zipped.stpz"]);
    assert_eq!(paths("U2"), ["kicad-embed://part.step", "${KIPRJMOD}/a/empty.step"]);
}

/// Board Setup's Export writes every file of the panel's list (`onExportFiles` walks `m_localFiles`, which holds the files only footprints hold too); the list tells the content's
/// checksum KiCad compares a file added again with (`AddEmbeddedFile`: `existingFile->data_hash == newFileHash`, the current hash once the file is read).
#[test]
fn board_setup_extracts_an_inherited_file_and_the_listing_tells_the_contents_checksum() {
    let dir = scratch("embedded_inherited_extract");
    setup(&dir);
    step(&dir, add(EmbeddedOwner::Footprint { reference: "U1".into() }, "U1.step", STEP), false, "test").unwrap();
    let got = crate::embedded_api::file(&dir, "/api/embedded/file?owner=board&name=U1.step");
    assert_eq!(got.status, "200 OK", "{}", String::from_utf8_lossy(&got.body));
    assert_eq!(got.body, STEP);
    let board = json_of(&crate::embedded_api::list(&dir, "/api/embedded?owner=board"));
    assert_eq!(board["inherited"][0]["content_checksum"], eda_kicad::embedded::checksum_of(STEP), "{board}");
}

/// A board text set in `face` (bold when `bold`), written into the board's `design.json` as an import would leave it (`TextExtra::face`).
fn add_face_text(dir: &Path, id: &str, face: &str, bold: bool) {
    let path = dir.join("design.json");
    let mut d: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let dr = d.as_object_mut().unwrap().entry("drawings").or_insert_with(|| serde_json::json!({}));
    let dr = dr.as_object_mut().unwrap();
    dr.entry("texts").or_insert_with(|| serde_json::json!([])).as_array_mut().unwrap().push(serde_json::json!({ "id": id, "content": "FONT", "at": { "x": 1_000_000, "y": 1_000_000 }, "layer": "F.SilkS", "size_nm": 1_000_000, "stroke_width": 150_000 }));
    dr.entry("text_extras").or_insert_with(|| serde_json::json!([])).as_array_mut().unwrap().push(serde_json::json!({ "id": id, "face": face, "bold": bold }));
    std::fs::write(&path, serde_json::to_string_pretty(&d).unwrap()).unwrap();
}

/// "Embed fonts" ticked (`onFontEmbedClick`): `GET /api/embedded/fonts` lists the fonts the board's texts use (`BOARD::GetFonts`, fontconfig's match), and the panel's OK
/// (`apply`, with no font data) has the files read in by the server, typed font, one undo step; unticked, the fonts go. Needs fontconfig with DejaVu Sans (the box has it).
#[test]
fn the_fonts_box_takes_in_the_fonts_the_boards_texts_use() {
    let dir = scratch("embedded_text_fonts");
    setup(&dir);
    if eda_kicad_engine::fonts::find_font("DejaVu Sans", false, false).is_none_or(|f| !f.path.ends_with("DejaVuSans.ttf")) {
        eprintln!("skipped: fontconfig has no DejaVu Sans here");
        return;
    }
    add_face_text(&dir, "txt_font1", "DejaVu Sans", false);
    add_face_text(&dir, "txt_font2", "DejaVu Sans", true);
    add_face_text(&dir, "txt_font3", "KiCad Font", false);
    let fonts = json_of(&crate::embedded_api::fonts(&dir, "/api/embedded/fonts?owner=board"));
    let names: Vec<&str> = fonts["fonts"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["DejaVuSans-Bold.ttf", "DejaVuSans.ttf"], "{fonts}");
    assert_eq!(fonts["fonts"][1]["permission"], "installable");
    let file = crate::embedded_api::font_file(&dir, "/api/embedded/fonts/file?owner=board&name=DejaVuSans.ttf");
    assert_eq!(file.status, "200 OK");
    assert_eq!(&file.body[..4], &[0, 1, 0, 0], "the font's own bytes");
    assert_eq!(crate::embedded_api::font_file(&dir, "/api/embedded/fonts/file?owner=board&name=passwd").status, "404 Not Found", "only a font the texts use");

    // the panel's OK: only the rows it still lists come in
    let cmd: Cmd = serde_json::from_value(serde_json::json!({ "op": "embedded", "verb": "apply", "owner": { "kind": "board" }, "fonts": true, "refresh_fonts": true, "text_fonts": ["DejaVuSans.ttf"] })).unwrap();
    step(&dir, cmd, false, "test").unwrap();
    let (_, design, _) = load(&dir).unwrap();
    let list = &design.drawings.as_ref().unwrap().embedded;
    assert!(list.fonts);
    assert_eq!(list.files.iter().map(|f| (f.name.as_str(), f.kind)).collect::<Vec<_>>(), [("DejaVuSans.ttf", EmbeddedFileType::Font)]);
    assert_eq!(eda_kicad::embedded::decode(&list.files[0]).unwrap().bytes, std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf").unwrap_or_else(|_| eda_kicad::embedded::decode(&list.files[0]).unwrap().bytes));
    // the derived board (the save) adds the bold one the texts use as well (`EmbedFonts`, `AddFile( .., false )`)
    let saved = eda_kicad_engine::fonts::with_board_fonts(&design);
    assert_eq!(saved.drawings.as_ref().unwrap().embedded.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["DejaVuSans-Bold.ttf", "DejaVuSans.ttf"]);
    undo(&dir, "test", Some(Domain::Pcb)).unwrap();
    assert!(names_listed(&dir, "/api/embedded?owner=board").is_empty(), "one undo");
}

/// `GetAssociatedDocument`'s `kicad-embed://` branch: the first list of the stack that has the file gives it, served with its type; no list, or none that has it, is a 404.
#[test]
fn a_datasheet_named_by_kicad_embed_is_served_from_the_first_list_of_the_stack_that_has_it() {
    let dir = scratch("embedded_datasheet_doc");
    setup(&dir);
    step(&dir, add(EmbeddedOwner::Footprint { reference: "U1".into() }, "u1.pdf", b"%PDF-1.4 from U1"), false, "test").unwrap();
    step(&dir, add(EmbeddedOwner::Board, "ds.pdf", b"%PDF-1.4 from the board"), false, "test").unwrap();
    let enc = |s: &str| s.replace('{', "%7B").replace('}', "%7D").replace('"', "%22").replace(':', "%3A").replace(',', "%2C").replace('[', "%5B").replace(']', "%5D");
    let doc = |name: &str, stack: &str| crate::embedded_api::document(&dir, &format!("/api/embedded/doc?name=kicad-embed%3A%2F%2F{name}&stack={}", enc(stack)));
    let stack = r#"[{"kind":"footprint","reference":"U1"},{"kind":"board"}]"#;
    let r = doc("u1.pdf", stack);
    assert_eq!((r.status, r.kind, r.body.as_slice()), ("200 OK", "application/pdf", &b"%PDF-1.4 from U1"[..]));
    let r = doc("ds.pdf", stack);
    assert_eq!(r.body, b"%PDF-1.4 from the board", "the next list of the stack");
    assert_eq!(doc("u1.pdf", r#"[{"kind":"board"}]"#).status, "404 Not Found");
    assert_eq!(doc("ds.pdf", r#"[{"kind":"footprint","reference":"U2"}]"#).status, "404 Not Found");
    assert_eq!(doc("ds.pdf", "[]").status, "404 Not Found");
    assert_eq!(doc("ds.pdf", "not json").status, "400 Bad Request");
    let probe = |name: &str| json_of(&crate::embedded_api::document(&dir, &format!("/api/embedded/doc?probe=1&name={name}&stack={}", enc(stack))))["found"].clone();
    assert_eq!((probe("u1.pdf"), probe("none.pdf")), (Value::Bool(true), Value::Bool(false)), "a probe answers whether it is found");
}
