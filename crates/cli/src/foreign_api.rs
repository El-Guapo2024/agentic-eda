//! `POST /api/import_foreign`: File > Import > Non-KiCad Board File... and Non-KiCad Schematic File..., the half that needs kicad-cli.
//!
//! A browser hands the page a file's bytes and not its path (the studio never opens a path on the server for this), so the page posts the file: `{ "kind": "board" |
//! "schematic", "name": "Adafruit AHT20.brd", "data": "<base64>", "format": "auto", "layer_map": { "tNames": "F.Fab" }, "drop_layers": ["tDocu"] }` (`drop_layers`: the
//! source layers left unmatched in the layer dialog, which the import leaves out with what is on them). The server writes it to a folder of its own, has
//! kicad-cli convert it (`eda_kicad_engine::foreign`; the route runs through `studio::offload`, the one kicad-cli lane), reads what kicad-cli made with our readers to check
//! that it is a design, and answers with what the page needs to ask for the import:
//!
//! * a board: `text`, the `.kicad_pcb`, and `project`, the `.kicad_pro` kicad-cli made with it (the design rules and net classes of the application it came from; `null` when it
//!   could not be made) -- which `Cmd::ReplaceBoard` carries;
//! * a schematic: `content`, the file with its sheets and library symbols (`eda_model::sheet_file::SheetFile`) -- which `Cmd::ReplaceSchematic` carries;
//! * both: `format` (the format kicad-cli recognised), `statistics` (what it converted), `findings` (its errors and warnings, each message once with how often it was said),
//!   `layers` (a board's source layers and where each went: the `DIALOG_MAP_LAYERS` the page shows when `maps_layers` says the importer asks), `counts` (what our reader found).
//!
//! The import itself is not here: it is a command (`/api/cmd`), one undo step, so the person can take it back. Nothing on the board changes by this request.
//!
//! A reply is `{ "ok": true, ... }` or `{ "ok": false, "message": "..", "unrecognized": bool }` -- `unrecognized` when no importer of kicad-cli takes the file (as against one
//! that failed on it).

use crate::board;
use crate::import_foreign::sheet_file_of;
use eda_kicad_engine::foreign::{self, ForeignImport, ForeignOptions, Kind};
use serde_json::{json, Value};
use std::path::Path;

fn refused(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into(), "unrecognized": false })
}

pub(crate) fn convert(dir: &Path, body: &[u8]) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return refused(format!("not an import request: {e}")),
    };
    // "project": an import of a project's files together (an Eagle `.brd` and its `.sch`, an EasyEDA Pro project): the board as "board", and the schematic the project command
    // made with it, which the page replaces in the same step (KiCad's "Import Non-KiCad Project", `IMPORT_PROJ_HELPER::ImportFiles`).
    let project = req.get("kind").and_then(Value::as_str) == Some("project");
    let kind = match req.get("kind").and_then(Value::as_str) {
        Some("board") | Some("project") => Kind::Board,
        Some("schematic") => Kind::Schematic,
        _ => return refused("the request names no kind of file: board, schematic or project"),
    };
    // The files: `name` and `data`, or a project's `files: [{ name, data }, ..]`.
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    if let Some(list) = req.get("files").and_then(Value::as_array) {
        for f in list {
            let name = f.get("name").and_then(Value::as_str).unwrap_or("");
            let Some(bytes) = f.get("data").and_then(Value::as_str).and_then(base64_decode) else { return refused(format!("'{name}' is not base64")) };
            files.push((name.to_string(), bytes));
        }
    } else {
        let name = req.get("name").and_then(Value::as_str).unwrap_or("");
        let Some(data) = req.get("data").and_then(Value::as_str) else { return refused("there is no file to import") };
        let Some(bytes) = base64_decode(data) else { return refused("the file is not base64") };
        files.push((name.to_string(), bytes));
    }
    // An Altium project file (`.PrjPcb`) chosen with its documents: KiCad 25ec2335's project import hands it to the schematic importer as `project_file`, which reads its
    // `[ProjectVariantN]` sections (`ParseAltiumProjectVariants`) into the schematic's design variants. kicad-cli takes no `.PrjPcb`, so it is read here and laid over
    // the converted schematic as `eda import-kicad Project.PrjPcb` lays it (`altium_variants`).
    let mut altium_variants: Vec<crate::altium_variants::ProjectVariant> = Vec::new();
    files.retain(|(n, b)| {
        if n.to_ascii_lowercase().ends_with(".prjpcb") {
            altium_variants.extend(crate::altium_variants::parse_project_variants(&String::from_utf8_lossy(b)));
            false
        } else {
            true
        }
    });
    if files.is_empty() {
        return refused("there is no file to import");
    }
    if let Some((name, _)) = files.iter().find(|(_, b)| b.is_empty()) {
        return refused(format!("'{name}' is empty"));
    }
    let name = files[0].0.clone();
    let name = name.as_str();
    // A board is imported with the project kicad-cli makes of it: its design rules and net classes are in that file and in no other.
    let mut options = ForeignOptions { format: req.get("format").and_then(Value::as_str).map(str::to_string), project: kind == Kind::Board, ..Default::default() };
    // The board of a project file of several the person chose (`DIALOG_IMPORT_CHOOSE_PROJECT`): its place in the list `boards` gave.
    options.board = req.get("board").and_then(Value::as_u64).map(|i| i as usize);
    if let Some(map) = req.get("layer_map").and_then(Value::as_object) {
        for (from, to) in map {
            let Some(to) = to.as_str() else { return refused(format!("the layer map names '{from}' by something that is not a KiCad layer name")) };
            options.layer_map.insert(from.clone(), to.to_string());
        }
    }
    // The layers left in the Unmatched list of the layer dialog: `DIALOG_MAP_LAYERS` answers `UNDEFINED_LAYER` for them, and the importer leaves out what is on them.
    if let Some(drop) = req.get("drop_layers").and_then(Value::as_array) {
        options.drop_layers = drop.iter().filter_map(Value::as_str).map(str::to_string).collect();
    }
    eda_kicad_engine::with_scratch(|work| {
        let mut staged: Vec<std::path::PathBuf> = Vec::new();
        for (n, bytes) in &files {
            match foreign::stage_upload(work, n, bytes) {
                Ok(p) => staged.push(p),
                Err(e) => return refused(board::reasons(&e)),
            }
        }
        // A KiCad 5 schematic is KiCad's own legacy format, which no importer of kicad-cli takes: read as `OpenProjectFiles` reads it (`crate::legacy_import`)
        if kind == Kind::Schematic && staged.iter().any(|p| crate::legacy_import::is_legacy_schematic_file(p)) {
            return legacy_reply(dir, work, &staged);
        }
        // A project: the board is the first file a board importer takes (by the names, those only a board has first), the others go with it to the project command.
        let mut order: Vec<usize> = (0..staged.len()).collect();
        if project {
            order.sort_by_key(|&i| !crate::import_foreign::kinds_for(&files[i].0).first().is_some_and(|k| *k == Kind::Board));
        }
        let mut run = Err(Vec::new());
        let mut input = staged[order[0]].clone();
        for &i in &order {
            input = staged[i].clone();
            let mut opts = options.clone();
            if project {
                opts.partners = staged.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, p)| p.clone()).collect();
            }
            run = match kind {
                Kind::Board => foreign::import_foreign_pcb(work, &input, &opts),
                Kind::Schematic => foreign::import_foreign_sch(work, &input, &opts),
            };
            match &run {
                Err(e) if project && foreign::is_unrecognized(e) => continue,
                _ => break,
            }
        }
        let name = input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| name.to_string());
        let name = name.as_str();
        // kicad-cli names the file by the path it was given, which is a folder of the server's: the person knows the file by its name.
        let (staged, scratch) = (input.display().to_string(), work.display().to_string());
        let said = |text: String| text.replace(&staged, name).replace(&scratch, "<scratch>");
        let import = match run {
            Ok(i) => i,
            Err(e) => return json!({ "ok": false, "message": said(board::reasons(&e)), "unrecognized": foreign::is_unrecognized(&e) }),
        };
        // A project file of several boards: `DIALOG_IMPORT_CHOOSE_PROJECT` asks which, and the page asks again with `board`.
        if import.needs_board_choice() {
            return json!({ "ok": true, "choose_board": true, "boards": import.boards, "name": name, "format": import.report.source_format, "kind": "board" });
        }
        match reply(kind, name, &import, project) {
            Ok(mut v) => {
                if !altium_variants.is_empty() {
                    with_altium_variants(&mut v, &altium_variants);
                }
                // the findings are said the same way: a path of the server's is not the person's
                if let Some(findings) = v.get_mut("findings").and_then(Value::as_array_mut) {
                    for f in findings {
                        if let Some(m) = f["message"].as_str().map(|m| said(m.to_string())) {
                            f["message"] = json!(m);
                        }
                    }
                }
                v
            }
            Err(message) => json!({ "ok": false, "message": said(message), "unrecognized": false, "findings": import.to_json()["findings"] }),
        }
    })
}

/// A KiCad 5 legacy schematic and the files that came with it (its sheets' `.sch`, the cache library `<project>-cache.lib`, the project's `.lib`s and
/// `.pro`): the root is the legacy schematic no other one places (the first of them when each is placed), read by `crate::legacy_import::load`. A browser
/// hands the page the files and not the folder, so they are what the folder is. What the legacy rescue reads comes along into the board's folder `dir`
/// (`carry_project_files`), named after the board's project. The reply is a schematic's (`content`, `counts`, `notes`), with `format` "KiCad legacy
/// schematic" and `remap` when no symbol names a library (KiCad opens Remap Symbols as such a project loads).
fn legacy_reply(dir: &Path, work: &Path, staged: &[std::path::PathBuf]) -> Value {
    let legacy: Vec<&std::path::PathBuf> = staged.iter().filter(|p| crate::legacy_import::is_legacy_schematic_file(p)).collect();
    let placed: std::collections::BTreeSet<String> = legacy
        .iter()
        .filter_map(|p| std::fs::read(p).ok())
        .filter_map(|b| eda_kicad::legacy_sch::parse_screen(&String::from_utf8_lossy(&b)).ok())
        .flat_map(|s| s.items.into_iter().filter_map(|it| if let eda_kicad::legacy_sch::Item::Sheet(sh) = it { Some(sh.file) } else { None }))
        .filter_map(|f| Path::new(&f.replace('\\', "/")).with_extension("sch").file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    let root = legacy.iter().find(|p| p.file_name().is_some_and(|n| !placed.contains(&*n.to_string_lossy()))).or(legacy.first()).map(|p| (*p).clone());
    let Some(root) = root else { return refused("there is no legacy schematic among the files") };
    let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let loaded = match crate::legacy_import::load(&root, work) {
        Ok(l) => l,
        Err(e) => return json!({ "ok": false, "message": board::reasons(&e).replace(&work.display().to_string(), "<scratch>"), "unrecognized": false }),
    };
    let mut notes = loaded.content.notes.clone();
    if let Ok((meta, _, _)) = board::load(dir) {
        notes.extend(crate::legacy_import::carry_project_files(&loaded, dir, &crate::rescue_api::project_name(&meta)));
    }
    let content = &loaded.content;
    let symbols: usize = content.all_screens().map(|(_, s)| s.symbols.len() + s.power_symbols.len()).sum();
    let notes: Vec<String> = notes.into_iter().map(|n| n.replace(&work.display().to_string(), "<scratch>")).collect();
    json!({
        "ok": true,
        "kind": "schematic",
        "name": name,
        "format": "KiCad legacy schematic",
        "engine": "eda (KiCad's legacy reader, ported)",
        "counts": { "symbols": symbols, "sheets": 1 + content.screens.len(), "labels": content.all_screens().map(|(_, s)| s.labels.len()).sum::<usize>(), "wires": content.all_screens().map(|(_, s)| s.wires.len()).sum::<usize>() },
        "notes": notes,
        "remap": loaded.remap,
        "content": serde_json::to_value(content).unwrap_or(Value::Null),
    })
}

/// What the page is told of a finished conversion, after our reader has read the result. `project`: the schematic the project command made with the board comes too
/// (`content`, `schematic_counts`), and the board is the project command's, which has the schematic's net names.
fn reply(kind: Kind, name: &str, import: &ForeignImport, project: bool) -> Result<Value, String> {
    let mut out = import.to_json();
    out["ok"] = json!(true);
    out["name"] = json!(name);
    out["maps_layers"] = json!(kind == Kind::Board && foreign::maps_layers(&import.report.source_format) && !import.report.layers.is_empty());
    if !import.boards.is_empty() {
        out["boards"] = json!(import.boards);
    }
    match kind {
        Kind::Board => {
            let text = import.project_board.as_deref().filter(|_| project || !import.boards.is_empty()).unwrap_or(&import.text);
            let (design, model, notes) = eda::import_kicad_pcb(text).map_err(|e| format!("kicad-cli converted '{name}', but the board it made cannot be read: {}", board::reasons(&e)))?;
            let (tracks, vias, zones) = design.routing.as_ref().map_or((0, 0, 0), |r| (r.tracks.len(), r.vias.len(), r.zones.len()));
            out["counts"] = json!({ "footprints": design.placement.as_ref().map_or(0, |p| p.footprints.len()), "nets": model.nets.len(), "tracks": tracks, "vias": vias, "zones": zones });
            out["text"] = json!(text);
            // a footprint at a scale other than 1:1 (KiCad 10's `(transform .. (scale ..))`): the reader bakes the scale into it, and the board file written back holds no scale
            if !notes.footprints_scaled.is_empty() {
                out["notes"] = json!([eda_kicad::scaled_footprints_note(&notes.footprints_scaled)]);
            }
            if project {
                let Some(sch) = import.project_schematic.as_deref() else {
                    return Err(format!("kicad-cli converted the board of '{name}', but made no schematic of the project's files"));
                };
                let content = project_sheet_file(sch, import.project.as_deref()).map_err(|e| format!("kicad-cli converted '{name}', but the schematic it made cannot be read: {}", board::reasons(&e)))?;
                let symbols: usize = content.all_screens().map(|(_, s)| s.symbols.len() + s.power_symbols.len()).sum();
                out["schematic_counts"] = json!({ "symbols": symbols, "sheets": 1 + content.screens.len(), "labels": content.all_screens().map(|(_, s)| s.labels.len()).sum::<usize>(), "wires": content.all_screens().map(|(_, s)| s.wires.len()).sum::<usize>() });
                out["schematic_notes"] = json!(content.notes);
                out["content"] = serde_json::to_value(&content).map_err(|e| e.to_string())?;
                out["kind"] = json!("project");
            }
            // `Cmd::ReplaceBoard` carries it too, and the server sets the board's rules and net classes from it.
            out["project"] = json!(import.project);
            // And the custom rules its importer wrote (`.kicad_dru`), which the server sets as the board's.
            out["rules"] = json!(import.rules);
        }
        Kind::Schematic => {
            let content = sheet_file_of(import).map_err(|e| format!("kicad-cli converted '{name}', but the schematic it made cannot be read: {}", board::reasons(&e)))?;
            let symbols: usize = content.all_screens().map(|(_, s)| s.symbols.len() + s.power_symbols.len()).sum();
            out["counts"] = json!({ "symbols": symbols, "sheets": 1 + content.screens.len(), "labels": content.all_screens().map(|(_, s)| s.labels.len()).sum::<usize>(), "wires": content.all_screens().map(|(_, s)| s.wires.len()).sum::<usize>() });
            out["notes"] = json!(content.notes);
            out["content"] = serde_json::to_value(&content).map_err(|e| e.to_string())?;
        }
    }
    Ok(out)
}

/// The Altium project's variants laid over the reply's schematic (`content`, a [`eda_model::sheet_file::SheetFile`]), and said in its notes; a reply with no schematic (a
/// board alone) is left as it is.
fn with_altium_variants(v: &mut Value, variants: &[crate::altium_variants::ProjectVariant]) {
    let Some(mut content) = v.get("content").cloned().and_then(|c| serde_json::from_value::<eda_model::sheet_file::SheetFile>(c).ok()) else { return };
    let made = crate::altium_variants::apply(&mut content, variants);
    let Ok(json) = serde_json::to_value(&content) else { return };
    v["content"] = json;
    let note = format!("{} design variant(s) of the Altium project ({}), {made} symbol record(s).", variants.len(), variants.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "));
    let key = if v.get("schematic_notes").is_some() { "schematic_notes" } else { "notes" };
    match v.get_mut(key).and_then(Value::as_array_mut) {
        Some(list) => list.push(json!(note)),
        None => v[key] = json!([note]),
    }
}

/// The schematic a project import made (`<stem>.kicad_sch`, its sheets beside it) as a [`eda_model::sheet_file::SheetFile`]: a KiCad 10 project of several top-level sheets
/// (`schematic.top_level_sheets`, an Eagle file of several sheets) placed on a root as `eda import-kicad` places them (`combine_top_level_sheets`).
fn project_sheet_file(sch: &Path, pro: Option<&str>) -> Result<eda_model::sheet_file::SheetFile, Vec<eda_model::CheckResult>> {
    let listed = pro.map(eda_kicad::project_schematic::top_level_sheets).unwrap_or_default();
    let dir = sch.parent().unwrap_or(Path::new("."));
    if listed.len() > 1 {
        let mut parts = Vec::new();
        for (file, sheet_name) in listed {
            parts.push((sheet_name, file.clone(), eda_kicad::load_sheet_file(&dir.join(&file))?));
        }
        return Ok(crate::import_foreign::combine_top_level_sheets(parts));
    }
    eda_kicad::load_sheet_file(sch)
}

/// Standard base64 (RFC 4648, with or without padding; white space is ignored). `None` for anything else.
pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    let (mut padding, mut data_chars) = (0, 0usize);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padding += 1;
                continue;
            }
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        if padding > 0 {
            return None; // data after the padding
        }
        data_chars += 1;
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // What is left over must be the unused bits of the last character, all zero; padding, when there is any, completes the last group of four.
    (bits < 6 && acc == 0 && (padding == 0 || (padding <= 2 && (data_chars + padding) % 4 == 0))).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An Altium project file chosen with the documents: its variants are laid over the schematic the reply carries and said in its notes (KiCad 25ec2335's project
    /// import, `ParseAltiumProjectVariants`); a reply with no schematic is left alone.
    #[test]
    fn an_altium_project_files_variants_are_laid_over_the_replys_schematic() {
        let prj = "[Design]\r\n[ProjectVariant1]\r\nDescription=Lite\r\nVariationCount=1\r\nVariation1=Designator=R2|Kind=1|AlternatePart=\r\n";
        let variants = crate::altium_variants::parse_project_variants(prj);
        let mut file = eda_model::sheet_file::SheetFile::default();
        file.root.symbols = vec![serde_json::from_value(json!({ "id": "R2", "lib_id": "Device:R", "at": { "x": 0, "y": 0 }, "rot": 0 })).unwrap()];
        let mut v = json!({ "ok": true, "kind": "project", "content": serde_json::to_value(&file).unwrap(), "schematic_notes": [] });
        with_altium_variants(&mut v, &variants);
        let back: eda_model::sheet_file::SheetFile = serde_json::from_value(v["content"].clone()).unwrap();
        assert!(back.root.extras.variants.contains_key("Lite"));
        assert_eq!(back.root.extras.symbol_variants["R2"]["Lite"].dnp, Some(true));
        assert_eq!(v["schematic_notes"], json!(["1 design variant(s) of the Altium project (Lite), 1 symbol record(s)."]));
        let mut board_only = json!({ "ok": true, "kind": "board" });
        with_altium_variants(&mut board_only, &variants);
        assert_eq!(board_only, json!({ "ok": true, "kind": "board" }));
    }

    #[test]
    fn base64_is_the_standard_alphabet_with_or_without_padding() {
        assert_eq!(base64_decode("").unwrap(), b"");
        assert_eq!(base64_decode("TWFu").unwrap(), b"Man");
        assert_eq!(base64_decode("TWE=").unwrap(), b"Ma");
        assert_eq!(base64_decode("TWE").unwrap(), b"Ma");
        assert_eq!(base64_decode("TQ==").unwrap(), b"M");
        assert_eq!(base64_decode("TQ").unwrap(), b"M");
        assert_eq!(base64_decode("TW\nFu\r\n").unwrap(), b"Man", "a browser may fold lines");
        assert_eq!(base64_decode("+/8=").unwrap(), vec![0xfb, 0xff]);
        // Every byte value survives, which a binary Altium file needs.
        let all: Vec<u8> = (0..=255).collect();
        let encoded = {
            const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            for chunk in all.chunks(3) {
                let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
                s.push(A[(n >> 18) as usize & 63] as char);
                s.push(A[(n >> 12) as usize & 63] as char);
                s.push(if chunk.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
                s.push(if chunk.len() > 2 { A[n as usize & 63] as char } else { '=' });
            }
            s
        };
        assert_eq!(base64_decode(&encoded).unwrap(), all);
        for bad in ["T*Fu", "TWFu=Zm", "TWE==", "T", "TQ=x", "TR=="] {
            assert!(base64_decode(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn a_request_that_names_no_file_is_refused_before_kicad_cli_is_asked() {
        let ask = |v: Value| convert(Path::new("."), v.to_string().as_bytes());
        for (req, said) in [
            (json!({ "name": "a.brd", "data": "AAAA" }), "no kind"),
            (json!({ "kind": "pcb", "name": "a.brd", "data": "AAAA" }), "no kind"),
            (json!({ "kind": "board", "name": "a.brd" }), "no file"),
            (json!({ "kind": "board", "name": "a.brd", "data": "***" }), "not base64"),
            (json!({ "kind": "board", "name": "a.brd", "data": "" }), "is empty"),
            (json!({ "kind": "board", "name": "..", "data": "AAAA" }), "no usable name"),
            (json!({ "kind": "board", "name": "a.brd", "data": "AAAA", "layer_map": { "x": 3 } }), "not a KiCad layer name"),
        ] {
            let r = ask(req.clone());
            assert_eq!(r["ok"], false, "{req}: {r}");
            assert!(r["message"].as_str().unwrap().contains(said), "{req}: {r}");
        }
        let r = convert(Path::new("."), b"not json");
        assert_eq!(r["ok"], false);
    }

    // ------------------------------------------------------- the slow tier: the real kicad-cli on KiCad's QA files, through the route

    fn encoded(path: &Path) -> String {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bytes = std::fs::read(path).unwrap();
        let mut s = String::new();
        for chunk in bytes.chunks(3) {
            let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
            s.push(A[(n >> 18) as usize & 63] as char);
            s.push(A[(n >> 12) as usize & 63] as char);
            s.push(if chunk.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
            s.push(if chunk.len() > 2 { A[n as usize & 63] as char } else { '=' });
        }
        s
    }

    fn qa(rel: &str) -> Option<std::path::PathBuf> {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() {
            eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
            return None;
        }
        if eda_kicad_engine::find_cli().is_none() {
            eprintln!("kicad-cli not found; skipping");
            return None;
        }
        let root = std::env::var_os("KICAD_QA_DATA").map(std::path::PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| std::path::PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
        let path = root.join(rel);
        assert!(path.is_file(), "no QA file {}", path.display());
        Some(path)
    }

    /// KiCad's "Import Non-KiCad Project" (`IMPORT_PROJ_HELPER::ImportFiles`): an Eagle `.brd` and its `.sch` together, the board with the project the schematic made (its net
    /// names), the schematic with it; and the layers left unmatched go, with what is on them.
    #[test]
    fn an_eagle_board_and_its_schematic_come_in_as_one_project() {
        let Some(brd) = qa("pcbnew/plugins/eagle/Adafruit-AHT20-PCB/Adafruit AHT20 Temperature & Humidity.brd") else { return };
        let sch = brd.with_extension("sch");
        let file = |p: &Path| json!({ "name": p.file_name().unwrap().to_string_lossy(), "data": encoded(p) });
        // the schematic first: the board is the file a board importer takes
        let r = convert(Path::new("."), json!({ "kind": "project", "files": [file(&sch), file(&brd)], "drop_layers": ["tDocu"] }).to_string().as_bytes());
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!((r["kind"].as_str(), r["format"].as_str(), r["name"].as_str()), (Some("project"), Some("Eagle"), Some("Adafruit AHT20 Temperature & Humidity.brd")), "{}", r["kind"]);
        assert_eq!(r["schematic_counts"]["symbols"], 36, "the schematic the project command made: {}", r["schematic_counts"]);
        assert!(r["counts"]["footprints"].as_u64().unwrap() > 10 && r["project"].is_string(), "{}", r["counts"]);
        assert!(r["layers"].as_array().unwrap().iter().any(|l| l["source"] == "tDocu" && l["kicad"].is_null()), "tDocu went nowhere: {}", r["layers"]);
        let board: eda_ops::Cmd = serde_json::from_value(json!({ "op": "replace_board", "text": r["text"], "name": "x.brd", "project": r["project"] })).expect("a board command");
        let schematic: eda_ops::Cmd = serde_json::from_value(json!({ "op": "replace_schematic", "content": r["content"], "name": "x.sch" })).expect("a schematic command");
        assert!(board.replaces_board() && schematic.replaces_schematic());
    }

    /// `DIALOG_IMPORT_CHOOSE_PROJECT`: an EasyEDA Pro project of several boards stops `pcb import` (the chooser callback kicad-cli does not set); the route names the boards and
    /// imports the one asked for.
    #[test]
    fn a_project_file_of_several_boards_names_them_and_imports_the_one_chosen() {
        let Some(epro) = qa("pcbnew/plugins/easyedapro/ProProject_Yuzuki Chameleon_2023-09-02.epro") else { return };
        let ask = |extra: Value| {
            let mut req = json!({ "kind": "board", "name": "Yuzuki.epro", "data": encoded(&epro) });
            if let (Some(o), Some(e)) = (req.as_object_mut(), extra.as_object()) {
                o.extend(e.clone());
            }
            convert(Path::new("."), req.to_string().as_bytes())
        };
        let r = ask(json!({}));
        assert_eq!((r["ok"].as_bool(), r["choose_board"].as_bool()), (Some(true), Some(true)), "{r}");
        let boards: Vec<String> = r["boards"].as_array().unwrap().iter().map(|b| b.as_str().unwrap().to_string()).collect();
        assert_eq!(boards, ["PCB", "树莓派A版形状", "点位图", "100ASK", "散热器外形"], "in the project's order, by the boards' ids");
        let i = boards.iter().position(|b| b == "100ASK").unwrap();
        let r = ask(json!({ "board": i }));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["format"].as_str(), Some("EasyEDA / JLCEDA Pro"));
        assert_eq!(r["counts"]["footprints"].as_u64(), Some(205), "{}", r["counts"]);
        assert!(r["text"].as_str().is_some_and(|t| t.starts_with("(kicad_pcb")));
        let r = ask(json!({ "board": 9 }));
        assert_eq!(r["ok"], false, "{r}");
    }

    /// An Altium board and an Eagle schematic through the route: what the page is told, and a command that takes the reply as it is.
    #[test]
    fn the_route_converts_a_qa_board_and_a_qa_schematic_and_the_commands_take_the_reply_as_it_is() {
        let Some(pcb) = qa("pcbnew/plugins/altium/HiFive/HiFive1.B01.PcbDoc") else { return };
        let r = convert(Path::new("."), json!({ "kind": "board", "name": "HiFive1.B01.PcbDoc", "data": encoded(&pcb) }).to_string().as_bytes());
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!((r["format"].as_str(), r["maps_layers"].as_bool()), (Some("Altium Designer"), Some(true)), "an Altium board asks which KiCad layer each layer goes to");
        assert_eq!(r["counts"], json!({ "footprints": 116, "nets": 78, "tracks": 1156, "vias": 247, "zones": r["counts"]["zones"].clone() }), "kicad-cli's own 116 footprints and 247 vias; our reader's 78 nets");
        assert_eq!(r["statistics"]["footprints"], 116);
        assert!(r["layers"].as_array().unwrap().iter().any(|l| l["kicad"] == "F.Cu"), "{}", r["layers"]);
        // The command the page sends is a command this server's `Cmd` takes, at the size of a real board, with the project kicad-cli made with it (the importer's rules).
        let pro: Value = serde_json::from_str(r["project"].as_str().expect("the project kicad-cli made with the board")).expect("a project file");
        assert!(pro["board"]["design_settings"]["rules"]["min_clearance"].as_f64().is_some_and(|c| c > 0.0), "Altium's clearance rule: {}", pro["board"]["design_settings"]["rules"]);
        let cmd: eda_ops::Cmd = serde_json::from_value(json!({ "op": "replace_board", "text": r["text"], "name": "HiFive1.B01.PcbDoc", "project": r["project"] })).expect("a board command");
        assert!(cmd.replaces_board());

        let sch = qa("pcbnew/plugins/eagle/Adafruit-AHT20-PCB/Adafruit AHT20 Temperature & Humidity.sch").unwrap();
        let r = convert(Path::new("."), json!({ "kind": "schematic", "name": "Adafruit AHT20 Temperature & Humidity.sch", "data": encoded(&sch) }).to_string().as_bytes());
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!((r["format"].as_str(), r["counts"]["symbols"].as_u64(), r["maps_layers"].as_bool()), (Some("EAGLE"), Some(36), Some(false)));
        let cmd: eda_ops::Cmd = serde_json::from_value(json!({ "op": "replace_schematic", "content": r["content"], "name": "x.sch" })).expect("a schematic command: the sheet file as the server sent it");
        assert!(cmd.replaces_schematic());
        // A file no importer recognises, in kicad-cli's words.
        let r = convert(Path::new("."), json!({ "kind": "board", "name": "notes.txt", "data": "aGVsbG8gd29ybGQ=" }).to_string().as_bytes());
        assert_eq!(r["ok"], false);
        assert!(!r["message"].as_str().unwrap().is_empty(), "{r}");
    }
}
