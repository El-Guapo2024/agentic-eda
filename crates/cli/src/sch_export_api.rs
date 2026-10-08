//! The schematic control actions' kicad-cli outputs: `/api/sch/bom` (the Symbol Fields Table's Export tab, `kicad-cli sch export bom`),
//! `/api/sch/bom_legacy` (Generate Legacy Bill of Materials, `kicad-cli sch export python-bom` and then one of KiCad's own generator scripts) and
//! `/api/sym/svg` (Export Symbol as SVG, `kicad-cli sym export svg`).
//!
//! Every one of them starts kicad-cli, so `studio.rs` runs each through `offload(...)`, one at a time in the lane (`crate::kicad_lane`; a test there
//! checks that no route of this module is answered inline). Nothing here lays out a BOM, a plot or a netlist itself: the dialogs' settings become
//! kicad-cli's own presets and arguments (`bom_presets`) and the files kicad-cli wrote are what the browser gets back.
//!
//! None of them edits `design.json`, so there is no `/api/cmd` verb or undo entry; output lands under the board's `export/` folder.

use crate::bom_plugins;
use crate::fab_api::{err, reply};
use crate::kicad_engine;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A generator script that has not finished after this long is stopped (a netlist of a few thousand parts is read in well under a second).
const GENERATOR_TIMEOUT: Duration = Duration::from_secs(60);
/// The biggest output file handed back inline for the browser to save.
const MAX_INLINE_BYTES: u64 = 4 << 20;

fn flag(v: &Value, key: &str, default: bool) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// The file the BOM is written to: a relative path inside the board's `export/` folder (nothing else of the board directory is the BOM's to overwrite).
pub(crate) fn export_path(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path.trim());
    if path.trim().is_empty() {
        return Err("the output file must be a path inside the board's export/ folder".into());
    }
    if p.is_absolute() || p.components().any(|c| matches!(c, Component::ParentDir | Component::Prefix(_) | Component::RootDir)) {
        return Err("the output file must be a relative path inside the board's export/ folder".into());
    }
    let mut comps = p.components();
    let first = comps.next().map(|c| c.as_os_str().to_string_lossy().to_string());
    if first.as_deref() != Some("export") || comps.next().is_none() {
        return Err("the output file must be inside the board's export/ folder (for example export/bom.csv)".into());
    }
    Ok(p.to_path_buf())
}

// ------------------------------------------------------------------------------------------------------------------------ Symbol Fields Table > Export

/// The name of the one BOM preset and BOM format preset a request carries to kicad-cli (`--preset` / `--format-preset`).
const PRESET_NAME: &str = "Studio export";

/// The Export tab of the Symbol Fields Table as KiCad's own presets: `BOM_PRESET` (the columns -- `BOM_FIELD` name, label, shown, grouped -- the sort, the
/// filter and whether symbols are grouped) and `BOM_FMT_PRESET` (the delimiters and what to keep), the JSON a project stores them as. `kicad-cli sch export bom
/// --preset` runs exactly that: its command-line options cannot say everything a preset does (a descending sort among them).
pub(crate) fn bom_presets(req: &Value) -> Result<(Value, Value), String> {
    let spec = req.get("spec").ok_or("the BOM needs the table's settings")?;
    let cols = spec.get("columns").and_then(Value::as_array).ok_or("the table has no columns")?;
    let mut fields = Vec::new();
    let mut shown = 0;
    for c in cols {
        let name = c.get("name").and_then(Value::as_str).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let label = c.get("label").and_then(Value::as_str).filter(|l| !l.is_empty()).unwrap_or(name);
        let show = flag(c, "show", true);
        shown += usize::from(show);
        fields.push(json!({ "name": name, "label": label, "show": show, "group_by": flag(c, "group_by", false) }));
    }
    if shown == 0 {
        return Err("no column is shown: there is nothing to export".into());
    }
    let preset = json!({
        "name": PRESET_NAME,
        "fields_ordered": fields,
        "sort_field": spec.get("sort_field").and_then(Value::as_str).unwrap_or("Reference"),
        "sort_asc": flag(spec, "sort_asc", true),
        "filter_string": spec.get("filter").and_then(Value::as_str).unwrap_or(""),
        "group_symbols": flag(spec, "group_symbols", true),
        "exclude_dnp": false,
        "include_excluded_from_bom": false,
    });
    let fmt = req.get("fmt").cloned().unwrap_or(Value::Null);
    let text = |key: &str, default: &str| fmt.get(key).and_then(Value::as_str).unwrap_or(default).to_string();
    let fmt = json!({
        "name": PRESET_NAME,
        "field_delimiter": text("field_delimiter", ","),
        "string_delimiter": text("string_delimiter", "\""),
        "ref_delimiter": text("ref_delimiter", ","),
        "ref_range_delimiter": text("ref_range_delimiter", "-"),
        "keep_tabs": flag(&fmt, "keep_tabs", false),
        "keep_line_breaks": flag(&fmt, "keep_line_breaks", false),
    });
    Ok((preset, fmt))
}

/// `POST /api/sch/bom`: `{spec, fmt, path?}` -- the Export tab's table settings and format, and the file to write (default `export/kicad/sch-bom/<name>.csv`).
/// Written by `kicad-cli sch export bom` from the saved design; `{ok, files, engine, text}` with the file's text for the preview line.
pub fn bom(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (preset, fmt) = match bom_presets(&req) {
        Ok(p) => p,
        Err(e) => return err(e),
    };
    let output = match req.get("path").and_then(Value::as_str) {
        Some(path) => match export_path(path) {
            Ok(rel) => Some(rel),
            Err(e) => return err(e),
        },
        None => None,
    };
    with_text(dir, reply(kicad_engine::export_sch_bom(dir, &preset, &fmt, output.as_deref())))
}

/// The text of the one file a successful export wrote, so the browser can offer it for saving.
fn with_text(dir: &Path, mut reply: Value) -> Value {
    let Some(file) = reply.get("files").and_then(Value::as_array).and_then(|f| f.first()).and_then(Value::as_str).map(str::to_string) else { return reply };
    if let Ok(text) = read_small(&dir.join(&file)) {
        reply["text"] = Value::String(text);
    }
    reply
}

fn read_small(path: &Path) -> Result<String, String> {
    let len = path.metadata().map_err(|e| e.to_string())?.len();
    if len > MAX_INLINE_BYTES {
        return Err("too big to send".into());
    }
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

// ------------------------------------------------------------------------------------------------------------------------ Generate Legacy Bill of Materials

/// `ReadyToNetlist`'s check: a BOM generator needs every symbol annotated (a reference still ending in `?` has no number).
pub(crate) fn unannotated(design: &eda_model::ir::Design) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let screens = design.schematic.iter().chain(design.sheet_contents.iter().flat_map(|c| c.values()));
    for sch in screens {
        found.extend(sch.symbols.iter().filter(|s| s.id.ends_with('?')).map(|s| s.id.clone()));
    }
    found.sort();
    found.dedup();
    found
}

/// Runs `program` and waits for it (`GENERATOR_TIMEOUT`); its output (stdout then stderr) and whether it succeeded.
fn run_generator(program: &str, args: &[String], cwd: &Path) -> Result<(bool, String), String> {
    // INPUTRC: a Python that loads `readline` prints a complaint about the person's own ~/.inputrc before the generator says anything; none of that is the generator's.
    let mut child = Command::new(program).args(args).env("INPUTRC", "/dev/null").current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("could not run {program}: {e}"))?;
    let (mut out, mut errs) = (child.stdout.take().expect("piped"), child.stderr.take().expect("piped"));
    // a generator's output is small, but a full pipe would stall it: read both while it runs
    let (o, e) = (std::thread::spawn(move || { let mut s = String::new(); let _ = out.read_to_string(&mut s); s }), std::thread::spawn(move || { let mut s = String::new(); let _ = errs.read_to_string(&mut s); s }));
    let started = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(s) => break s,
            None if started.elapsed() > GENERATOR_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} did not finish in {} seconds and was stopped", GENERATOR_TIMEOUT.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let (stdout, stderr) = (o.join().unwrap_or_default(), e.join().unwrap_or_default());
    Ok((status.success(), format!("{stdout}{stderr}")))
}

/// `POST /api/sch/bom_legacy`: `{plugin: "bom_csv_grouped_by_value.py"}` -- `DIALOG_BOM`'s Generate: the intermediate XML netlist
/// (`kicad-cli sch export python-bom`) and then the chosen script over it, both into `export/kicad/sch-python-bom/`. Only a script of KiCad's own
/// plugins folder can be named. `{ok, files, messages, output: {name, text}}`.
pub fn bom_legacy(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let wanted = req.get("plugin").and_then(Value::as_str).unwrap_or("");
    let Some(plugins_dir) = eda_kicad_engine::bom_plugins_dir() else { return err("KiCad's plugins folder was not found (set EDA_KICAD_PLUGINS)") };
    // the file name only, matched against what the folder holds: no request can name a path
    let Some((plugin, script)) = bom_plugins::list(&plugins_dir).into_iter().find(|(p, _)| p.file == wanted) else {
        return err(format!("'{wanted}' is not one of KiCad's BOM generator scripts"));
    };
    let (design, _) = match kicad_engine::load_with_schematic(dir) {
        Ok(v) => v,
        Err(e) => return err(crate::board::reasons(&e)),
    };
    let open = unannotated(&design);
    if !open.is_empty() {
        return err(format!("Generating BOM requires a fully annotated schematic. Not annotated: {}", open.join(", ")));
    }
    let xml_files = match kicad_engine::export_sch(dir, "python-bom", &[]) {
        Ok(v) => v,
        Err(e) => return err(crate::board::reasons(&e)),
    };
    let Some(xml_rel) = xml_files["files"].as_array().and_then(|f| f.first()).and_then(Value::as_str) else { return err("kicad-cli wrote no netlist") };
    let xml = dir.join(xml_rel);
    let out_base = xml.with_extension("");
    let python = std::env::var("EDA_PYTHON").unwrap_or_else(|_| "python3".to_string());
    let (program, args) = plugin.invocation(&script, &python, &xml, &out_base);
    let out_dir = xml.parent().unwrap_or(dir).to_path_buf();
    // a generator that leaves an earlier run's file behind must not be taken for having written it
    let output = PathBuf::from(format!("{}{}", out_base.display(), plugin.ext));
    let _ = std::fs::remove_file(&output);
    let (ok, messages) = match run_generator(&program, &args, &out_dir) {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let rel_of = |p: &Path| p.strip_prefix(dir).map(|r| r.to_string_lossy().to_string()).unwrap_or_else(|_| p.to_string_lossy().to_string());
    let written = output.is_file();
    let mut files: Vec<String> = xml_files["files"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if written {
        files.push(rel_of(&output));
    }
    let mut reply = json!({ "ok": ok && written, "files": files, "messages": messages, "command": plugin.command_template(&script), "engine": xml_files["engine"] });
    if ok && written {
        if let Ok(text) = read_small(&output) {
            reply["output"] = json!({ "name": output.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), "text": text });
        }
    } else {
        reply["message"] = Value::String(if ok { format!("{} wrote no output file", plugin.file) } else { format!("{} failed", plugin.file) });
    }
    reply
}

// ------------------------------------------------------------------------------------------------------------------------ Export Symbol as SVG

/// The SVG kicad-cli writes for a unit and body style (`doSymExportSvg`): `<symbol>_unit<N>` with `_demorgan` for the second body style of a symbol
/// that has one; path characters in the name become spaces (`wxFileName::GetForbiddenChars`).
pub(crate) fn svg_file_name(symbol: &str, unit: u32, demorgan: bool) -> String {
    let name: String = symbol.chars().map(|c| if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { ' ' } else { c }).collect();
    format!("{name}_unit{unit}{}.svg", if demorgan { "_demorgan" } else { "" })
}

/// `POST /api/sym/svg`: `{lib_id, unit?, body_style?, black_and_white?, include_hidden_pins?, include_hidden_fields?}` -- the symbol (a project library entry, else
/// the resolved one) written out as a one-symbol library and plotted by `kicad-cli sym export svg`. `{ok, files, svg: {name, text}}`: `svg` is the unit
/// and body style asked for (`SYMBOL_EDITOR_CONTROL::ExportSymbolAsSVG` plots the unit and style being edited).
pub fn symbol_svg(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let lib_id = req.get("lib_id").and_then(Value::as_str).unwrap_or("");
    if lib_id.is_empty() {
        return err("no symbol to export");
    }
    let unit = req.get("unit").and_then(Value::as_u64).unwrap_or(1).max(1) as u32;
    let demorgan = req.get("body_style").and_then(Value::as_u64).unwrap_or(1) == 2;
    let text = match crate::library_api::symbol_kicad_sym_any(dir, lib_id) {
        Ok(t) => t,
        Err(e) => return err(crate::board::reasons(&e)),
    };
    let bare = lib_id.rsplit(':').next().unwrap_or(lib_id);
    let mut args: Vec<String> = Vec::new();
    for (key, option) in [("black_and_white", "--black-and-white"), ("include_hidden_pins", "--include-hidden-pins"), ("include_hidden_fields", "--include-hidden-fields")] {
        if flag(&req, key, false) {
            args.push(option.to_string());
        }
    }
    let mut reply = reply(kicad_engine::export_symbol_svg(dir, bare, &text, &args));
    let wanted = svg_file_name(bare, unit, demorgan);
    let file = reply.get("files").and_then(Value::as_array).and_then(|f| f.iter().filter_map(Value::as_str).find(|f| f.ends_with(&wanted))).map(str::to_string);
    match file {
        Some(file) => {
            if let Ok(svg) = read_small(&dir.join(&file)) {
                reply["svg"] = json!({ "name": format!("{bare}.svg"), "text": svg });
            }
        }
        None if reply["ok"] == Value::Bool(true) => {
            reply["ok"] = Value::Bool(false);
            reply["message"] = Value::String(format!("kicad-cli plotted no unit {unit} for '{bare}'"));
        }
        None => {}
    }
    reply
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> Value {
        json!({
            "spec": {
                "columns": [
                    { "name": "Reference", "label": "Refs", "show": true, "group_by": false },
                    { "name": "Value", "label": "Value", "show": true, "group_by": true },
                    { "name": "Footprint", "label": "Footprint", "show": true, "group_by": true },
                    { "name": "Datasheet", "label": "Datasheet", "show": false, "group_by": false },
                    { "name": "${QUANTITY}", "label": "Qty", "show": true, "group_by": false },
                ],
                "group_symbols": true, "sort_field": "Reference", "sort_asc": true, "filter": ""
            },
            "fmt": { "field_delimiter": ",", "string_delimiter": "\"", "ref_delimiter": ",", "ref_range_delimiter": "-", "keep_tabs": false, "keep_line_breaks": false }
        })
    }

    #[test]
    fn the_table_becomes_a_bom_preset_and_a_format_preset() {
        let (preset, fmt) = bom_presets(&spec()).unwrap();
        assert_eq!(preset["name"], PRESET_NAME);
        // every column in order, with its label, whether it shows and whether it groups -- a hidden one stays in, for sorting and grouping
        assert_eq!(
            preset["fields_ordered"],
            json!([
                { "name": "Reference", "label": "Refs", "show": true, "group_by": false },
                { "name": "Value", "label": "Value", "show": true, "group_by": true },
                { "name": "Footprint", "label": "Footprint", "show": true, "group_by": true },
                { "name": "Datasheet", "label": "Datasheet", "show": false, "group_by": false },
                { "name": "${QUANTITY}", "label": "Qty", "show": true, "group_by": false },
            ])
        );
        assert_eq!((preset["sort_field"].as_str(), preset["sort_asc"].as_bool(), preset["group_symbols"].as_bool()), (Some("Reference"), Some(true), Some(true)));
        assert_eq!(preset["filter_string"], "");
        // KiCad's reader wants every key of both presets
        for key in ["name", "fields_ordered", "sort_field", "sort_asc", "filter_string", "group_symbols", "exclude_dnp"] {
            assert!(preset.get(key).is_some(), "{key}");
        }
        assert_eq!(fmt, json!({ "name": PRESET_NAME, "field_delimiter": ",", "string_delimiter": "\"", "ref_delimiter": ",", "ref_range_delimiter": "-", "keep_tabs": false, "keep_line_breaks": false }));
    }

    #[test]
    fn a_descending_sort_a_filter_and_the_format_options_follow_the_dialog() {
        let mut r = spec();
        r["spec"]["group_symbols"] = json!(false);
        r["spec"]["sort_asc"] = json!(false);
        r["spec"]["filter"] = json!("R*");
        r["fmt"]["keep_tabs"] = json!(true);
        r["fmt"]["keep_line_breaks"] = json!(true);
        r["fmt"]["ref_range_delimiter"] = json!("");
        r["fmt"]["field_delimiter"] = json!(";");
        let (preset, fmt) = bom_presets(&r).unwrap();
        assert_eq!((preset["group_symbols"].as_bool(), preset["sort_asc"].as_bool(), preset["filter_string"].as_str()), (Some(false), Some(false), Some("R*")));
        assert_eq!(fmt["ref_range_delimiter"], "", "an empty range delimiter means no ranges");
        assert_eq!((fmt["field_delimiter"].as_str(), fmt["keep_tabs"].as_bool(), fmt["keep_line_breaks"].as_bool()), (Some(";"), Some(true), Some(true)));
    }

    #[test]
    fn a_table_with_nothing_shown_is_refused() {
        let mut r = spec();
        for c in r["spec"]["columns"].as_array_mut().unwrap() {
            c["show"] = json!(false);
        }
        assert!(bom_presets(&r).unwrap_err().contains("nothing to export"));
        assert!(bom_presets(&json!({})).is_err());
    }

    #[test]
    fn the_output_file_stays_inside_export() {
        assert_eq!(export_path("export/bom.csv").unwrap(), PathBuf::from("export/bom.csv"));
        assert!(export_path("export/sub/bom.csv").is_ok());
        for bad in ["", "  ", "bom.csv", "design.json", "../export/bom.csv", "export/../design.json", "/etc/bom.csv", "export", "export/"] {
            assert!(export_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn an_unannotated_reference_is_found_on_any_sheet() {
        use eda_model::ir::{Provenance, SchematicSection, SymbolInstance};
        use eda_model::ir::Design;
        let sym = |id: &str| SymbolInstance { id: id.into(), at: eda_model::ir::Point { x: 0, y: 0 }, rot: 0, mirrored: false, mirror_y: false, lib_id: String::new(), unit: 1, value: String::new(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false };
        let sch = |ids: &[&str]| SchematicSection {
            symbols: ids.iter().map(|i| sym(i)).collect(),
            wires: vec![], labels: vec![], texts: vec![], power_symbols: vec![], no_connects: vec![], bus_entries: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(),
            title_block: None, sheets: vec![], instance_overrides: vec![], junctions: vec![], lines: vec![], imported_from_kicad: false, extras: Default::default(),
        };
        let mut d = Design {
            schema: 1,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: Some(sch(&["R1", "C?"])),
            nets: None,
            placement: None,
            routing: None,
            drawings: None,
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
        };
        d.sheet_contents = Some([("a.kicad_sch".to_string(), sch(&["U?", "C?"]))].into_iter().collect());
        assert_eq!(unannotated(&d), vec!["C?", "U?"]);
        d.schematic = Some(sch(&["R1"]));
        d.sheet_contents = None;
        assert!(unannotated(&d).is_empty());
    }

    #[test]
    fn the_svg_of_a_unit_is_named_the_way_kicad_cli_names_it() {
        assert_eq!(svg_file_name("R", 1, false), "R_unit1.svg");
        assert_eq!(svg_file_name("LM358", 2, true), "LM358_unit2_demorgan.svg");
        assert_eq!(svg_file_name("A/B:C", 1, false), "A B C_unit1.svg", "path characters become spaces");
    }

    #[test]
    fn a_generator_that_runs_long_or_fails_is_reported() {
        let dir = std::env::temp_dir();
        let (ok, out) = run_generator("sh", &["-c".into(), "echo hi; echo oops 1>&2; exit 3".into()], &dir).unwrap();
        assert!(!ok);
        assert!(out.contains("hi") && out.contains("oops"));
        assert!(run_generator("definitely-not-a-program-eda", &[], &dir).is_err());
    }
}
