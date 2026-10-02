//! `/api/sch/*`: the backend for the studio's File > Plot... (eeschema's
//! `DIALOG_PLOT_SCHEMATIC`, `eeschema.EditorControl.plot`) and File >
//! Export > Netlist... (`DIALOG_EXPORT_NETLIST`,
//! `eeschema.EditorControl.exportNetlist`) dialogs
//! (`web/studio/src/components/PlotSchematicDialog.tsx`,
//! `ExportNetlistDialog.tsx`).
//!
//! Same shape as `fab_api.rs`: take the board directory every other
//! `/api/*` route does, run the ported writer (`eda_kicad::plot_schematic`
//! = `SCH_PLOTTER`, `eda_kicad::export_netlist` = `NETLIST_EXPORTER_*`),
//! write the result into `<dir>/export/` and return the paths written
//! (relative to `dir`). Both are read-only exports: nothing here edits
//! `design.json`, so there is no `/api/cmd` verb or undo entry.
//!
//! A board started from an intent alone has no `schematic` section yet:
//! like `GET /api/schematic.svg` it plots/exports the schematic the engine
//! would derive from the intent, so the dialogs work on a fresh board.

use crate::board;
use eda_model::ir::{Design, SymbolInstance};
use eda_model::{CheckResult, ConstraintModel, LibSymbol};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn err(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

fn ok(files: Vec<String>) -> Value {
    json!({ "ok": true, "files": files })
}

fn write_all(dir: &Path, files: impl IntoIterator<Item = (String, Vec<u8>)>) -> Result<Vec<String>, String> {
    let out_dir = dir.join("export");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let mut written = Vec::new();
    for (name, bytes) in files {
        std::fs::write(out_dir.join(&name), &bytes).map_err(|e| e.to_string())?;
        written.push(format!("export/{name}"));
    }
    Ok(written)
}

/// The board's design (its own schematic, or the one derived from the
/// intent), its model (with `Design::nets` already applied by
/// `board::load` -- the one netlist), and the project name.
fn load(dir: &Path) -> Result<(Design, ConstraintModel, String), Vec<CheckResult>> {
    let (meta, mut design, model) = board::load(dir)?;
    if design.schematic.is_none() {
        design = eda::prelude::derive_schematic(&model, &eda::prelude::EngineOptions::default())?;
    }
    let project = PathBuf::from(&meta.intent).file_stem().and_then(|s| s.to_str()).unwrap_or("board").to_string();
    Ok((design, model, project))
}

/// The studio's own library-symbol resolution (`schematic_json`'s
/// `lib_symbols`): a real/built-in library symbol when one resolves, else
/// the synthesized generic box for the part.
fn resolver(model: &ConstraintModel) -> impl Fn(&SymbolInstance) -> LibSymbol + '_ {
    move |s: &SymbolInstance| {
        let real = if s.lib_id.is_empty() || eda_model::is_synthetic_lib_id(&s.lib_id) { None } else { model.symbol_of(&s.lib_id) };
        real.unwrap_or_else(|| crate::studio::synthesize_generic_symbol(&format!("eda:{}", s.id), model))
    }
}

fn tool() -> String {
    format!("agentic-eda {}", env!("CARGO_PKG_VERSION"))
}

fn str_of<'a>(req: &'a Value, key: &str) -> Option<&'a str> {
    req.get(key).and_then(Value::as_str)
}

fn bool_of(req: &Value, key: &str, default: bool) -> bool {
    req.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// `POST /api/sch/plot`: `{"format": "svg"|"pdf", "color": bool,
/// "plot_drawing_sheet": bool, "background": bool, "plot_all": bool,
/// "sheet_path": "<placement id>/<id>/..." (when `plot_all` is false),
/// "pages": ["1", "3"], "page_size": "auto"|"a4"|"a"}` -- the
/// `DIALOG_PLOT_SCHEMATIC` options (`SCH_PLOT_OPTS`). Omitted fields take
/// `SCH_PLOT_OPTS`'s own defaults (colour, drawing sheet, background,
/// every sheet).
pub fn plot(dir: &Path, body: &[u8]) -> Value {
    use eda_kicad::{PageSizeSelect, PlotFormat, PlotMeta, SchPlotOpts};
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let format = match str_of(&req, "format") {
        Some("pdf") => PlotFormat::Pdf,
        Some("svg") | None => PlotFormat::Svg,
        Some(other) => return err(format!("unknown plot format {other:?} (expected \"svg\" or \"pdf\")")),
    };
    let page_size_select = match str_of(&req, "page_size") {
        Some("a4") => PageSizeSelect::A4,
        Some("a") => PageSizeSelect::A,
        _ => PageSizeSelect::Auto,
    };
    let opts = SchPlotOpts {
        plot_all: bool_of(&req, "plot_all", true),
        plot_drawing_sheet: bool_of(&req, "plot_drawing_sheet", true),
        plot_pages: req.get("pages").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        black_and_white: !bool_of(&req, "color", true),
        page_size_select,
        use_background_color: bool_of(&req, "background", true),
        current_sheet: str_of(&req, "sheet_path").unwrap_or("").split('/').filter(|s| !s.is_empty()).map(str::to_string).collect(),
    };
    let (design, model, project) = match load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let now = eda::now_rfc3339();
    let meta = PlotMeta { iso_date: now.clone(), tool: tool(), project: project.clone(), fallback_title: project, fallback_date: now.split('T').next().unwrap_or("").to_string() };
    let r = resolver(&model);
    let files = match eda_kicad::plot_schematic(&design, &model, format, &opts, &meta, &r) {
        Ok(f) => f,
        Err(e) => return err(board::reasons(&e)),
    };
    match write_all(dir, files.into_iter().map(|f| (f.filename, f.bytes))) {
        Ok(written) => ok(written),
        Err(e) => err(e),
    }
}

/// `POST /api/sch/netlist`: `{"format": "kicad"|"xml"}` -- KiCad's `.net`
/// (`NETLIST_EXPORTER_KICAD`) or generic `.xml` (`NETLIST_EXPORTER_XML`).
pub fn netlist(dir: &Path, body: &[u8]) -> Value {
    use eda_kicad::{NetlistFormat, NetlistMeta};
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let format = match str_of(&req, "format") {
        Some("xml") => NetlistFormat::Xml,
        Some("kicad") | None => NetlistFormat::Kicad,
        Some(other) => return err(format!("unknown netlist format {other:?} (expected \"kicad\" or \"xml\")")),
    };
    let (design, model, project) = match load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    let lib_root = eda_kicad::default_symbol_library_root();
    let meta = NetlistMeta {
        source: dir.join(format!("{project}.kicad_sch")).to_string_lossy().to_string(),
        date: eda::now_rfc3339(),
        tool: tool(),
        project: project.clone(),
        fallback_title: project.clone(),
        symbol_library_root: lib_root.is_dir().then_some(lib_root),
    };
    let text = match eda_kicad::export_netlist(&design, &model, format, &meta) {
        Ok(t) => t,
        Err(e) => return err(board::reasons(&e)),
    };
    match write_all(dir, [(format!("{project}.{}", format.extension()), text.into_bytes())]) {
        Ok(written) => ok(written),
        Err(e) => err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::{Net, Part, Pin, PinKind};

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eda_cli_sch_api_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A board directory holding an LDO-ish intent with no schematic
    /// section yet (so the API derives one, like a fresh board).
    fn setup(dir: &Path) {
        let part = |r: &str, pins: &[(&str, &str, PinKind)]| Part {
            reference: r.into(),
            mpn: None,
            lcsc: None,
            value: Some(format!("{r}_val")),
            package: None,
            footprint: Some("Foo:Bar".into()),
            symbol: None,
            datasheet: None,
            pins: pins.iter().map(|(n, name, k)| Pin { number: n.to_string(), name: Some(name.to_string()), kind: *k }).collect(),
            body_um: None,
            edge: None,
        };
        let model = ConstraintModel {
            parts: vec![part("U1", &[("1", "VIN", PinKind::Power), ("2", "GND", PinKind::Ground)]), part("CIN", &[("1", "1", PinKind::Passive), ("2", "2", PinKind::Ground)])],
            nets: vec![Net { name: "VIN".into(), pins: vec!["U1.1".into(), "CIN.1".into()] }, Net { name: "GND".into(), pins: vec!["U1.2".into(), "CIN.2".into()] }],
            ..Default::default()
        };
        let intent = dir.join("ldo.yaml");
        std::fs::write(&intent, serde_yaml::to_string(&model).unwrap()).unwrap();
        let mut design = eda::prelude::derive_schematic(&model, &eda::prelude::EngineOptions::default()).unwrap();
        design.schematic = None;
        board::save(dir, &design).unwrap();
        std::fs::write(dir.join("board.json"), json!({ "intent": intent.display().to_string(), "snap_um": 100, "spacing_um": 300 }).to_string()).unwrap();
    }

    #[test]
    fn plot_endpoint_writes_svg_and_pdf_into_export() {
        let dir = scratch("plot");
        setup(&dir);
        let r = plot(&dir, br#"{"format": "svg"}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["files"], json!(["export/ldo.svg"]));
        let svg = std::fs::read_to_string(dir.join("export/ldo.svg")).unwrap();
        assert!(svg.contains("<svg") && svg.contains(">U1</text>") && svg.contains("stroke:#009600"));

        let r = plot(&dir, br#"{"format": "pdf", "color": false, "plot_drawing_sheet": false, "page_size": "a4"}"#);
        assert_eq!(r["ok"], true, "{r}");
        let pdf = std::fs::read(dir.join("export/ldo.pdf")).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));

        // B&W: no colour in the SVG.
        let r = plot(&dir, br#"{"format": "svg", "color": false}"#);
        assert_eq!(r["ok"], true);
        let bw = std::fs::read_to_string(dir.join("export/ldo.svg")).unwrap();
        assert!(!bw.contains("stroke:#009600"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plot_rejects_an_unknown_format() {
        let dir = scratch("plot_bad");
        setup(&dir);
        let r = plot(&dir, br#"{"format": "dxf"}"#);
        assert_eq!(r["ok"], false);
        assert!(r["message"].as_str().unwrap().contains("dxf"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn netlist_endpoint_writes_net_and_xml_from_the_one_netlist() {
        let dir = scratch("netlist");
        setup(&dir);
        let r = netlist(&dir, br#"{"format": "kicad"}"#);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["files"], json!(["export/ldo.net"]));
        let net = std::fs::read_to_string(dir.join("export/ldo.net")).unwrap();
        assert!(net.starts_with("(export\n\t(version \"E\")"));
        assert!(net.contains("(name \"GND\")") && net.contains("(name \"VIN\")"));

        let r = netlist(&dir, br#"{"format": "xml"}"#);
        assert_eq!(r["files"], json!(["export/ldo.xml"]));
        let xml = std::fs::read_to_string(dir.join("export/ldo.xml")).unwrap();
        assert!(xml.contains("<net code=\"1\" name=\"GND\" class=\"Default\">"));

        // An edit to the one netlist (design.nets, as a schematic edit writes it) shows up in the export.
        let (_, mut design, _) = board::load(&dir).unwrap();
        design.nets = Some(vec![Net { name: "RAIL".into(), pins: vec!["U1.1".into(), "CIN.1".into()] }]);
        board::save(&dir, &design).unwrap();
        let r = netlist(&dir, br#"{"format": "kicad"}"#);
        assert_eq!(r["ok"], true);
        let net = std::fs::read_to_string(dir.join("export/ldo.net")).unwrap();
        // GND/VIN are gone from `nets`; the now-unnamed CIN pin 2 is its own default-named net.
        assert!(net.contains("(name \"RAIL\")") && net.contains("Net-(CIN-Pad2)") && !net.contains("(name \"VIN\")\n\t\t\t(class"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_board_is_an_error_not_a_panic() {
        let dir = scratch("nobody");
        let r = netlist(&dir, b"{}");
        assert_eq!(r["ok"], false);
        let r = plot(&dir, b"{}");
        assert_eq!(r["ok"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
