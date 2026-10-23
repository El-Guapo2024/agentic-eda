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

    let content: Vec<u8> = (0..50_000u32).map(|i| (i * 7 % 251) as u8).collect();
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
