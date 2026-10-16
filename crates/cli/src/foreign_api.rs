//! `POST /api/import_foreign`: File > Import > Non-KiCad Board File... and Non-KiCad Schematic File..., the half that needs kicad-cli.
//!
//! A browser hands the page a file's bytes and not its path (the studio never opens a path on the server for this), so the page posts the file: `{ "kind": "board" |
//! "schematic", "name": "Adafruit AHT20.brd", "data": "<base64>", "format": "auto", "layer_map": { "tNames": "F.Fab" } }`. The server writes it to a folder of its own, has
//! kicad-cli convert it (`eda_kicad_engine::foreign`; the route runs through `studio::offload`, the one kicad-cli lane), reads what kicad-cli made with our readers to check
//! that it is a design, and answers with what the page needs to ask for the import:
//!
//! * a board: `text`, the `.kicad_pcb` -- which `Cmd::ReplaceBoard` carries;
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

pub(crate) fn convert(_dir: &Path, body: &[u8]) -> Value {
    let req: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return refused(format!("not an import request: {e}")),
    };
    let kind = match req.get("kind").and_then(Value::as_str) {
        Some("board") => Kind::Board,
        Some("schematic") => Kind::Schematic,
        _ => return refused("the request names no kind of file: board or schematic"),
    };
    let name = req.get("name").and_then(Value::as_str).unwrap_or("");
    let Some(data) = req.get("data").and_then(Value::as_str) else { return refused("there is no file to import") };
    let Some(bytes) = base64_decode(data) else { return refused("the file is not base64") };
    if bytes.is_empty() {
        return refused(format!("'{name}' is empty"));
    }
    let mut options = ForeignOptions { format: req.get("format").and_then(Value::as_str).map(str::to_string), ..Default::default() };
    if let Some(map) = req.get("layer_map").and_then(Value::as_object) {
        for (from, to) in map {
            let Some(to) = to.as_str() else { return refused(format!("the layer map names '{from}' by something that is not a KiCad layer name")) };
            options.layer_map.insert(from.clone(), to.to_string());
        }
    }
    eda_kicad_engine::with_scratch(|work| {
        let input = match foreign::stage_upload(work, name, &bytes) {
            Ok(p) => p,
            Err(e) => return refused(board::reasons(&e)),
        };
        let run = match kind {
            Kind::Board => foreign::import_foreign_pcb(work, &input, &options),
            Kind::Schematic => foreign::import_foreign_sch(work, &input, &options),
        };
        // kicad-cli names the file by the path it was given, which is a folder of the server's: the person knows the file by its name.
        let (staged, scratch) = (input.display().to_string(), work.display().to_string());
        let said = |text: String| text.replace(&staged, name).replace(&scratch, "<scratch>");
        let import = match run {
            Ok(i) => i,
            Err(e) => return json!({ "ok": false, "message": said(board::reasons(&e)), "unrecognized": foreign::is_unrecognized(&e) }),
        };
        match reply(kind, name, &import) {
            Ok(mut v) => {
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

/// What the page is told of a finished conversion, after our reader has read the result.
fn reply(kind: Kind, name: &str, import: &ForeignImport) -> Result<Value, String> {
    let mut out = import.to_json();
    out["ok"] = json!(true);
    out["name"] = json!(name);
    out["maps_layers"] = json!(kind == Kind::Board && foreign::maps_layers(&import.report.source_format) && !import.report.layers.is_empty());
    match kind {
        Kind::Board => {
            let (design, model, _notes) = eda::import_kicad_pcb(&import.text).map_err(|e| format!("kicad-cli converted '{name}', but the board it made cannot be read: {}", board::reasons(&e)))?;
            let (tracks, vias, zones) = design.routing.as_ref().map_or((0, 0, 0), |r| (r.tracks.len(), r.vias.len(), r.zones.len()));
            out["counts"] = json!({ "footprints": design.placement.as_ref().map_or(0, |p| p.footprints.len()), "nets": model.nets.len(), "tracks": tracks, "vias": vias, "zones": zones });
            out["text"] = json!(import.text);
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
        // The command the page sends is a command this server's `Cmd` takes, at the size of a real board.
        let cmd: eda_ops::Cmd = serde_json::from_value(json!({ "op": "replace_board", "text": r["text"], "name": "HiFive1.B01.PcbDoc" })).expect("a board command");
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
