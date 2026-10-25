//! The shapes of a board's barcodes, for the studio's canvas (GAPS.md item 24, package P16, step 4).
//!
//! KiCad works the modules of a barcode out with zint (`PCB_BARCODE::ComputeBarcode`: `ZBarcode_Encode`, `ZBarcode_Buffer_Vector`) and keeps the polygons of the symbol and of the text
//! under it (`m_poly`, `AssembleBarcode`: scaled to the barcode's size, the text one millimetre under it, inverted into a rectangle for a knockout, mirrored on a back layer, turned).
//! The `.kicad_pcb` has the parameters and none of that: the file's data cannot say what the code looks like. This studio has no zint and may add no dependency, so the shape comes
//! from kicad-cli, which has it: a board of nothing but the barcode -- at the origin, turned as it is, on its layer -- is plotted to SVG (`pcb export svg`) and the filled polygons of
//! the plot are the shape. The same code kicad-cli itself would plot for the real board, whatever KiCad version or zint it was built with.
//!
//! A plot takes a fraction of a second and the studio's request loop does not wait for kicad-cli, so [`lookup`] answers from a cache keyed by every parameter the shape depends on
//! (not the barcode's place: the polygons are kept with its centre at the origin) and starts the plot in a thread the first time a shape is asked for; the browser asks again
//! while the answer is `pending` (`GET /api/barcode_shape?id=..`, `POST /api/barcode_preview` for the dialog's barcode that is not on the board yet).

use eda_model::units::{mm, Nm};
use eda_model::barcode::Barcode;
use eda_model::ir::{Design, DrawingsSection, PlacementSection, Point, Provenance};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// What a barcode is drawn as: the filled polygons of the plot, micrometres, the centre of the barcode at the origin. Each path is one `<path>` of the plot (filled even-odd) and
/// holds its contours; the paths are separate so that two that overlap are not cut out of one another.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BarcodeShape {
    pub paths: Vec<Vec<Vec<[eda_model::units::Nm; 2]>>>,
}

/// What the cache holds for a barcode's parameters.
#[derive(Debug, Clone)]
enum Slot {
    Pending,
    Ready(Arc<BarcodeShape>),
    Failed(String),
}

/// The answer to a request for a shape.
#[derive(Debug, Clone)]
pub enum Reply {
    Pending,
    Ready(Arc<BarcodeShape>),
    Failed(String),
}

impl Reply {
    /// The JSON of the route: `{"status": "pending"}`, `{"status": "ready", "paths": [[[[x, y], ..], ..], ..]}` or `{"status": "failed", "error": ".."}`.
    pub fn json(&self) -> Value {
        match self {
            Reply::Pending => json!({ "status": "pending" }),
            Reply::Ready(shape) => json!({ "status": "ready", "paths": shape.paths }),
            Reply::Failed(error) => json!({ "status": "failed", "error": error }),
        }
    }

    /// The HTTP status the route answers with: 202 while the plot runs.
    pub fn http_status(&self) -> &'static str {
        match self {
            Reply::Pending => "202 Accepted",
            _ => "200 OK",
        }
    }
}

/// Every parameter the shape depends on and nothing it does not: not the id, not the place. `text` is the text the code encodes (its variables expanded).
pub fn key(b: &Barcode, text: &str) -> String {
    let ecc = if b.kind.has_error_correction() { b.ecc.file_name() } else { "-" };
    format!(
        "{}|{}|{}|{}x{}|{}|{}|{}|{},{}|{}|{}|{}",
        b.kind.file_name(),
        ecc,
        b.layer,
        b.width_nm,
        b.height_nm,
        b.text_height_nm,
        b.show_text,
        b.knockout,
        b.margin_x_nm,
        b.margin_y_nm,
        b.angle,
        text.len(),
        text
    )
}

fn cache() -> &'static Mutex<HashMap<String, Slot>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Slot>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// One plot at a time: they are short and each makes a scratch directory, and the lane kicad-cli's longer runs go through is not the place for them.
fn plot_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Default::default)
}

/// The shape of `b` (with `text` its encoded text, variables expanded) if it is known; else a plot is started and the reply is [`Reply::Pending`]. A plot that failed stays failed until
/// the process restarts (the reason is the same every time).
pub fn lookup(b: &Barcode, text: &str) -> Reply {
    let k = key(b, text);
    {
        let mut slots = cache().lock().unwrap_or_else(PoisonError::into_inner);
        match slots.get(&k) {
            Some(Slot::Ready(shape)) => return Reply::Ready(shape.clone()),
            Some(Slot::Failed(e)) => return Reply::Failed(e.clone()),
            Some(Slot::Pending) => return Reply::Pending,
            None => {
                slots.insert(k.clone(), Slot::Pending);
            }
        }
    }
    let (barcode, text) = (b.clone(), text.to_string());
    std::thread::spawn(move || {
        let slot = match compute(&barcode, &text) {
            Ok(shape) => Slot::Ready(Arc::new(shape)),
            Err(e) => Slot::Failed(e),
        };
        cache().lock().unwrap_or_else(PoisonError::into_inner).insert(k, slot);
    });
    Reply::Pending
}

/// [`lookup`] that waits for the plot: what the tests ask for.
#[cfg(test)]
pub fn shape_blocking(b: &Barcode, text: &str) -> Result<Arc<BarcodeShape>, String> {
    loop {
        match lookup(b, text) {
            Reply::Ready(shape) => return Ok(shape),
            Reply::Failed(e) => return Err(e),
            Reply::Pending => std::thread::sleep(std::time::Duration::from_millis(40)),
        }
    }
}

/// A design of nothing but the barcode `b`, at the origin: the board kicad-cli is asked to plot.
fn lone_barcode(b: &Barcode, text: &str) -> Design {
    let mut barcode = b.clone();
    barcode.id = "bar_plot".into();
    barcode.at = Point { x: Nm::ZERO, y: Nm::ZERO };
    barcode.text = text.to_string();
    Design {
        footprint_library: None,
        sheet_contents: None,
        bus_aliases: vec![],
        symbol_library: None,
        schema: eda_model::ir::SCHEMA_VERSION,
        provenance: Provenance { engine_version: "barcode".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
        schematic: None,
        nets: None,
        routing: None,
        placement: Some(PlacementSection { outline: vec![Point { x: mm(-1.0), y: mm(-1.0) }, Point { x: mm(1.0), y: mm(-1.0) }, Point { x: mm(1.0), y: mm(1.0) }, Point { x: mm(-1.0), y: mm(1.0) }], footprints: vec![], modules: vec![] }),
        drawings: Some(DrawingsSection { barcodes: vec![barcode], ..Default::default() }),
    }
}

/// Plot the barcode with kicad-cli and read the polygons back.
fn compute(b: &Barcode, text: &str) -> Result<BarcodeShape, String> {
    let Some(cli) = eda_kicad_engine::find_cli() else { return Err("kicad-cli is not installed, so the barcode cannot be drawn".to_string()) };
    let _one_at_a_time = plot_lock().lock().unwrap_or_else(PoisonError::into_inner);
    let design = lone_barcode(b, text);
    let model = eda_model::ConstraintModel::default();
    let board = eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-01-01", title: "barcode" }).map_err(|e| crate::board::reasons(&e))?;
    let dir = std::env::temp_dir().join(format!("eda_barcode_{}_{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())));
    std::fs::create_dir_all(&dir).map_err(|e| format!("scratch directory: {e}"))?;
    let (file, out) = (dir.join("barcode.kicad_pcb"), dir.join("barcode.svg"));
    let result = (|| {
        std::fs::write(&file, &board).map_err(|e| format!("scratch board: {e}"))?;
        let done = eda_kicad_engine::cli_command(&cli, None)
            .args(["pcb", "export", "svg", "--mode-single", "--exclude-drawing-sheet", "--page-size-mode", "2", "--layers", &b.layer, "-o"])
            .arg(&out)
            .arg(&file)
            .output()
            .map_err(|e| format!("kicad-cli did not run: {e}"))?;
        if !done.status.success() || !out.exists() {
            return Err(format!("kicad-cli did not plot the barcode: {}{}", String::from_utf8_lossy(&done.stdout), String::from_utf8_lossy(&done.stderr)));
        }
        let svg = std::fs::read_to_string(&out).map_err(|e| format!("plot: {e}"))?;
        Ok(BarcodeShape { paths: filled_paths(&svg) })
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// The filled polygons of a kicad-cli SVG plot (`<path style=".. stroke:none;fill-rule:evenodd;" d="M x,y x,y .. Z ..">`), one entry per path, each a list of contours in micrometres.
pub fn filled_paths(svg: &str) -> Vec<Vec<Vec<[eda_model::units::Nm; 2]>>> {
    let mut out = Vec::new();
    for path in svg.split("<path ").skip(1) {
        let Some((head, rest)) = path.split_once("d=\"") else { continue };
        if !head.contains("stroke:none") || !head.contains("fill-rule:evenodd") {
            continue;
        }
        let d = rest.split('"').next().unwrap_or("");
        let contours: Vec<Vec<[eda_model::units::Nm; 2]>> = d
            .split('M')
            .filter(|s| !s.trim().is_empty())
            .map(|sub| {
                sub.split(|c: char| c.is_whitespace() || c == 'Z' || c == 'L')
                    .filter(|t| t.contains(','))
                    .filter_map(|t| t.split_once(',').and_then(|(x, y)| Some([eda_model::units::from_mm(x.parse::<f64>().ok()?), eda_model::units::from_mm(y.parse::<f64>().ok()?)])))
                    .collect::<Vec<_>>()
            })
            .filter(|c| c.len() >= 3)
            .collect();
        if !contours.is_empty() {
            out.push(contours);
        }
    }
    out
}

/// The text a barcode encodes: its text with `${NAME}` expanded as `PCB_BARCODE::GetShownText` expands it (the board's variables, the project's, the title block's).
fn shown_text(dir: &Path, b: &Barcode, target: &str) -> Result<String, String> {
    let (meta, design, model) = crate::board::load(dir).map_err(|e| crate::board::reasons(&e))?;
    let tv = crate::text_vars_json::PcbShown::new(dir, &design, &model, crate::text_vars_json::project(dir, &meta.intent, crate::text_vars_json::clock_of(target)));
    Ok(tv.text_shown(&b.text, &b.layer))
}

/// What a route answers: the HTTP status and the JSON body.
pub(crate) struct Answer {
    pub status: &'static str,
    pub body: String,
}

fn answer(reply: &Reply) -> Answer {
    Answer { status: reply.http_status(), body: reply.json().to_string() }
}

fn failed(status: &'static str, error: &str) -> Answer {
    Answer { status, body: json!({ "status": "failed", "error": error }).to_string() }
}

/// `GET /api/barcode_shape?id=..`: the shape of the barcode of the board `id` names, `pending` (202) while kicad-cli plots it.
pub(crate) fn shape_route(dir: &Path, target: &str) -> Answer {
    let id = crate::image_api::query_value(target, "id");
    let Ok((_, design, _)) = crate::board::load(dir) else { return failed("404 Not Found", "the board could not be read") };
    let Some(b) = design.drawings.as_ref().and_then(|d| d.barcodes.iter().find(|b| b.id == id)).cloned() else { return failed("404 Not Found", "no barcode with this id") };
    match shown_text(dir, &b, target) {
        Ok(text) if text.is_empty() => Answer { status: "200 OK", body: json!({ "status": "ready", "paths": [] }).to_string() },
        Ok(text) => answer(&lookup(&b, &text)),
        Err(e) => failed("404 Not Found", &e),
    }
}

/// `POST /api/barcode_preview` with `{ "barcode": { .. } }`: the same for a barcode that is not on the board yet (the dialog's preview, `DIALOG_BARCODE_PROPERTIES::refreshPreview`).
pub(crate) fn preview_route(dir: &Path, body: &[u8]) -> Answer {
    let parsed: Result<Value, _> = serde_json::from_slice(body);
    let Ok(req) = parsed else { return failed("400 Bad Request", "the request is not JSON") };
    let b: Barcode = match serde_json::from_value(req.get("barcode").cloned().unwrap_or(Value::Null)) {
        Ok(b) => b,
        Err(e) => return failed("400 Bad Request", &format!("not a barcode: {e}")),
    };
    match shown_text(dir, &b, "/api/barcode_preview") {
        Ok(text) if text.is_empty() => Answer { status: "200 OK", body: json!({ "status": "ready", "paths": [] }).to_string() },
        Ok(text) => answer(&lookup(&b, &text)),
        Err(e) => failed("404 Not Found", &e),
    }
}

#[cfg(test)]
mod tests {
    use eda_model::units::{mm, Nm};
    use super::*;
    use eda_model::barcode::BarcodeKind;

    fn qr(text: &str) -> Barcode {
        Barcode {
            id: "bar_a".into(),
            at: Point { x: mm(5.0), y: mm(5.0) },
            angle: 0,
            layer: "F.SilkS".into(),
            width_nm: mm(10.0),
            height_nm: mm(10.0),
            text: text.into(),
            text_height_nm: mm(1.5),
            kind: BarcodeKind::Qr,
            ecc: eda_model::barcode::EccLevel::L,
            show_text: true,
            knockout: false,
            margin_x_nm: Nm::ZERO,
            margin_y_nm: Nm::ZERO,
        }
    }

    #[test]
    fn the_key_has_what_the_shape_depends_on_and_not_the_place_or_the_id() {
        let a = qr("HELLO");
        let mut moved = a.clone();
        moved.at = Point { x: mm(9.0), y: mm(-3.0) };
        moved.id = "bar_b".into();
        assert_eq!(key(&a, "HELLO"), key(&moved, "HELLO"));
        for change in [
            Box::new(|b: &mut Barcode| b.width_nm = mm(12.0)) as Box<dyn Fn(&mut Barcode)>,
            Box::new(|b| b.height_nm = mm(12.0)),
            Box::new(|b| b.angle = 90_000),
            Box::new(|b| b.layer = "B.SilkS".into()),
            Box::new(|b| b.show_text = false),
            Box::new(|b| b.knockout = true),
            Box::new(|b| b.margin_x_nm = mm(0.5)),
            Box::new(|b| b.kind = BarcodeKind::DataMatrix),
            Box::new(|b| b.ecc = eda_model::barcode::EccLevel::H),
            Box::new(|b| b.text_height_nm = mm(2.0)),
        ] {
            let mut c = a.clone();
            change(&mut c);
            assert_ne!(key(&a, "HELLO"), key(&c, "HELLO"));
        }
        assert_ne!(key(&a, "HELLO"), key(&a, "WORLD"));
        // The error correction means nothing to a code that has none.
        let (mut d1, mut d2) = (a.clone(), a.clone());
        d1.kind = BarcodeKind::Code128;
        d2.kind = BarcodeKind::Code128;
        d2.ecc = eda_model::barcode::EccLevel::H;
        assert_eq!(key(&d1, "X"), key(&d2, "X"));
    }

    #[test]
    fn the_filled_paths_of_a_plot_are_read_in_nanometres() {
        let svg = "<g>\n<path style=\"fill:none; stroke:#AFAFAF; stroke-width:0.2000;\" d=\"M 1.0000,1.0000\n2.0000,1.0000\n2.0000,2.0000\nZ\" /> \n\
                   <path style=\"fill:#F2EDA1; fill-opacity:1.0000; stroke:none;fill-rule:evenodd;\"\nd=\"M -0.8389,6.0018\n-0.8051,6.0158\n-0.7792,6.0417\nZ\" /> \n\
                   <path style=\"fill:#F2EDA1; fill-opacity:1.0000; stroke:none;fill-rule:evenodd;\"\nd=\"M 0.0000,0.0000\n1.0000,0.0000\n1.0000,1.0000\nZ M 0.2500,0.2500\n0.5000,0.2500\n0.5000,0.5000\nZ\" /> \n</g>";
        let paths = filled_paths(svg);
        assert_eq!(paths.len(), 2, "the stroked outline is not a shape: {paths:?}");
        assert_eq!(paths[0], vec![vec![[Nm::new(-838_900), Nm::new(6_001_800)], [Nm::new(-805_100), Nm::new(6_015_800)], [Nm::new(-779_200), Nm::new(6_041_700)]]]);
        assert_eq!(paths[1].len(), 2, "a path may have a hole");
        assert_eq!(paths[1][1], vec![[mm(0.25), mm(0.25)], [mm(0.5), mm(0.25)], [mm(0.5), mm(0.5)]]);
    }

    #[test]
    fn a_reply_has_the_json_and_the_status_of_the_route() {
        assert_eq!(Reply::Pending.json(), json!({ "status": "pending" }));
        assert_eq!(Reply::Pending.http_status(), "202 Accepted");
        let ready = Reply::Ready(Arc::new(BarcodeShape { paths: vec![vec![vec![[Nm::ZERO, Nm::ZERO], [Nm::new(1), Nm::ZERO], [Nm::new(1), Nm::new(1)]]]] }));
        assert_eq!(ready.json(), json!({ "status": "ready", "paths": [[[[0, 0], [1, 0], [1, 1]]]] }));
        assert_eq!(ready.http_status(), "200 OK");
        assert_eq!(Reply::Failed("no kicad-cli".into()).json()["error"], "no kicad-cli");
    }
}
