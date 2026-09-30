//! `eda board serve`: the board in a browser, editable.
//!
//! A person and a model work the same board through the same verbs. The
//! page shows what is on the board -- parts, pads by net, the airwires
//! still to route, tracks once routed, the gates failing -- and turns a
//! drag, a key press or a click into a board command, which runs through
//! [`crate::board::step`] exactly as the CLI's does. Commands typed at
//! the CLI show up on the page within a second, and the page's show up
//! in `activity.jsonl` for the CLI side to read: one board, one set of
//! verbs, two hands.
//!
//! A local tool, so a small one: the standard library's TCP listener,
//! one request at a time, bound to 127.0.0.1. Routing, the one slow
//! command, runs on a thread so the page keeps answering meanwhile.

use crate::board;
use eda_model::footprint::{placed_courtyard, placed_pads};
use eda_model::ir::{LabelSide, Shape, Side};
use eda_model::{CheckResult, CheckStatus};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const PAGE: &str = include_str!("studio.html");

/// What the routing thread is doing: "idle", "running", or how the last
/// run ended.
type Job = Arc<Mutex<String>>;

/// Where the built React UI lives, in precedence order: `--ui <dir>`,
/// then `EDA_STUDIO_UI`, then the workspace-relative `web/studio/dist`
/// this binary was compiled from. `None` from [`built_ui`] means none of
/// those has an `index.html` yet, so `serve` falls back to the embedded
/// single-file page.
fn ui_dir(cli_ui: Option<&Path>) -> PathBuf {
    if let Some(p) = cli_ui {
        return p.to_path_buf();
    }
    if let Ok(p) = std::env::var("EDA_STUDIO_UI") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/studio/dist")
}

fn built_ui(cli_ui: Option<&Path>) -> Option<PathBuf> {
    let dir = ui_dir(cli_ui);
    dir.join("index.html").is_file().then_some(dir)
}

pub fn serve(dir: &Path, port: u16, ui: Option<PathBuf>) -> Result<(), Vec<CheckResult>> {
    // Fail now, not on the first request, if this is no board.
    board::load(dir)?;
    let ui_root = built_ui(ui.as_deref());
    // A taken port is usually another studio already showing a board;
    // take the next free one rather than refuse.
    let (listener, got) = (port..port.saturating_add(20))
        .find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok().map(|l| (l, p)))
        .ok_or_else(|| vec![CheckResult::fail("serve_bind", format!("127.0.0.1:{port}"), format!("ports {port}-{} are all taken", port.saturating_add(19)))])?;
    if got != port {
        eprintln!("board: port {port} is taken (another studio open there?), using {got}");
    }
    eprintln!(
        "board: serving {} at http://127.0.0.1:{got}/ ({})",
        dir.display(),
        match &ui_root {
            Some(d) => format!("ui: {}", d.display()),
            None => "ui: embedded studio.html".to_string(),
        }
    );
    let job: Job = Arc::new(Mutex::new("idle".into()));
    let schematic: Mutex<Option<(std::time::SystemTime, String)>> = Mutex::new(None);
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        if let Err(e) = handle(&mut stream, dir, &job, &schematic, ui_root.as_deref()) {
            let _ = respond(&mut stream, "500 Internal Server Error", "text/plain", e.as_bytes());
        }
    }
    Ok(())
}

/// A file under the built UI's directory, or 404 if it does not resolve
/// to one (missing, or outside `root` -- no serving `../../etc/passwd`
/// through a crafted path).
fn serve_file(stream: &mut TcpStream, root: &Path, rel: &str) -> Result<(), String> {
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let candidate = root.join(rel);
    let resolved = candidate.canonicalize().ok().zip(root.canonicalize().ok()).filter(|(p, r)| p.starts_with(r)).map(|(p, _)| p);
    match resolved.and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
        Some((p, bytes)) => respond(stream, "200 OK", mime_of(&p), &bytes),
        // A client-side route (no file extension) falls back to index.html,
        // like any single-page app; a genuinely missing asset still 404s.
        None if !rel.contains('.') => {
            let index = root.join("index.html");
            match std::fs::read(&index) {
                Ok(bytes) => respond(stream, "200 OK", mime_of(&index), &bytes),
                Err(_) => respond(stream, "404 Not Found", "text/plain", b"not found"),
            }
        }
        None => respond(stream, "404 Not Found", "text/plain", b"not found"),
    }
}

fn mime_of(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
}

fn handle(stream: &mut TcpStream, dir: &Path, job: &Job, schematic: &Mutex<Option<(std::time::SystemTime, String)>>, ui_root: Option<&Path>) -> Result<(), String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).map_err(|e| e.to_string())?;
    let mut parts = request_line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let mut length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).map_err(|e| e.to_string())? == 0 || header == "\r\n" || header == "\n" {
            break;
        }
        if let Some((k, v)) = header.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                length = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; length.min(1 << 20)];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    let path = target.split('?').next().unwrap_or("/");

    match (method, path) {
        ("GET", "/") => match ui_root {
            Some(root) => serve_file(stream, root, "index.html"),
            None => respond(stream, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
        },
        ("GET", "/api/version") => respond(stream, "200 OK", "application/json", version(dir, job).to_string().as_bytes()),
        ("GET", "/api/state") => {
            let v = state(dir, job).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("GET", "/api/schematic.svg") => match schematic_svg(dir, schematic) {
            Ok(svg) => respond(stream, "200 OK", "image/svg+xml", svg.as_bytes()),
            Err(e) => respond(stream, "404 Not Found", "text/plain", board::reasons(&e).as_bytes()),
        },
        ("GET", "/api/schematic") => {
            let v = schematic_json(dir).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("POST", "/api/cmd") => {
            let req: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
            let strict = req.get("strict").and_then(Value::as_bool).unwrap_or(true);
            let reply = match serde_json::from_value::<eda_ops::Cmd>(req.get("cmd").cloned().unwrap_or(Value::Null)) {
                Err(e) => json!({ "ok": false, "message": format!("not a board command: {e}") }),
                Ok(cmd) => match board::step(dir, cmd, strict, "ui") {
                    Ok(summary) => json!({ "ok": true, "message": summary }),
                    Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
                },
            };
            respond(stream, "200 OK", "application/json", reply.to_string().as_bytes())
        }
        ("POST", "/api/undo") => {
            let reply = match board::undo(dir, "ui") {
                Ok(summary) => json!({ "ok": true, "message": summary }),
                Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
            };
            respond(stream, "200 OK", "application/json", reply.to_string().as_bytes())
        }
        ("POST", "/api/redo") => {
            let reply = match board::redo(dir, "ui") {
                Ok(summary) => json!({ "ok": true, "message": summary }),
                Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
            };
            respond(stream, "200 OK", "application/json", reply.to_string().as_bytes())
        }
        ("POST", "/api/route") => {
            let mut j = job.lock().map_err(|e| e.to_string())?;
            if *j == "running" {
                return respond(stream, "409 Conflict", "application/json", json!({ "ok": false, "message": "already routing" }).to_string().as_bytes());
            }
            *j = "running".into();
            drop(j);
            let (dir, job) = (dir.to_path_buf(), job.clone());
            std::thread::spawn(move || {
                let end = match board::route_board(&dir, "ui") {
                    Ok(s) => s,
                    Err(e) => format!("route failed: {}", board::reasons(&e)),
                };
                if let Ok(mut j) = job.lock() {
                    *j = end;
                }
            });
            respond(stream, "200 OK", "application/json", json!({ "ok": true, "message": "routing" }).to_string().as_bytes())
        }
        ("GET", p) if ui_root.is_some() && !p.starts_with("/api/") => serve_file(stream, ui_root.unwrap(), p.trim_start_matches('/')),
        _ => respond(stream, "404 Not Found", "text/plain", b"not found"),
    }
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> Result<(), String> {
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
    stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body)).map_err(|e| e.to_string())
}

fn stamp(p: PathBuf) -> u128 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos())
}

/// Changes whenever the board, its activity or the routing job does, so
/// the page knows to fetch the state again.
fn version(dir: &Path, job: &Job) -> Value {
    let j = job.lock().map(|j| j.clone()).unwrap_or_default();
    json!(format!("{}-{}-{}", stamp(dir.join("design.json")), stamp(dir.join("activity.jsonl")), j.len() + j.bytes().map(|b| b as usize).sum::<usize>()))
}

fn state(dir: &Path, job: &Job) -> Result<Value, Vec<CheckResult>> {
    let (meta, design, model) = board::load(dir)?;
    let b = eda_ops::Board::new(design.clone(), &model, meta.snap_um, meta.spacing_um);
    let view = eda_ops::view::view(&b, &model)?;
    let block_of: std::collections::HashMap<String, String> = view["parts"]
        .as_array()
        .map(|ps| ps.iter().filter_map(|p| Some((p["ref"].as_str()?.to_string(), p["block"].as_str().unwrap_or("").to_string()))).collect())
        .unwrap_or_default();
    let net_of: std::collections::HashMap<&str, &str> = model.nets.iter().flat_map(|n| n.pins.iter().map(move |p| (p.as_str(), n.name.as_str()))).collect();
    let pl = design.placement.as_ref();
    let mut parts = Vec::new();
    for part in &model.parts {
        let fp = pl.and_then(|pl| pl.footprints.iter().find(|f| f.id == part.reference));
        let size = model.footprint_of(part).map(|f| f.courtyard_half()).map(|(w, h)| [w * 2, h * 2]);
        let mut p = json!({
            "ref": part.reference,
            "value": part.value,
            "package": part.package,
            "mpn": part.mpn,
            "block": block_of.get(&part.reference),
            "placed": fp.is_some(),
            "size": size,
        });
        if let Some(fp) = fp {
            let pads: Vec<Value> = placed_pads(&model, part, fp)
                .unwrap_or_default()
                .iter()
                .map(|q| {
                    let net = net_of.get(format!("{}.{}", part.reference, q.number).as_str()).copied();
                    json!({ "num": q.number, "net": net, "x": q.center.x, "y": q.center.y, "w": q.size.0, "h": q.size.1, "round": q.is_round(), "th": q.through_hole })
                })
                .collect();
            p["at"] = json!([fp.at.x, fp.at.y]);
            p["rot"] = json!(fp.rot as f64 / 1000.0);
            p["side"] = json!(if fp.side == Side::Bottom { "bottom" } else { "top" });
            p["label"] = json!(match fp.label {
                LabelSide::Above => "above",
                LabelSide::Below => "below",
                LabelSide::Left => "left",
                LabelSide::Right => "right",
            });
            p["courtyard"] = json!(placed_courtyard(&model, part, fp).map(|c| [c.0, c.1, c.2, c.3]));
            p["pads"] = json!(pads);
        }
        parts.push(p);
    }
    let mut checks: Vec<&CheckResult> = Vec::new();
    let placement_checks = b.checks();
    let routing_checks = if design.routing.is_some() { eda_gates::check_routing(&design, &model) } else { Vec::new() };
    checks.extend(placement_checks.iter().chain(routing_checks.iter()).filter(|c| !matches!(c.status, CheckStatus::Pass)));
    let checks: Vec<Value> = checks
        .iter()
        .map(|c| json!({ "check": c.check, "fail": matches!(c.status, CheckStatus::Fail), "at": c.location, "hint": c.hint }))
        .collect();
    let activity: Vec<Value> = std::fs::read_to_string(dir.join("activity.jsonl"))
        .unwrap_or_default()
        .lines()
        .rev()
        .take(60)
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let routing = design.routing.as_ref().map(|r| {
        json!({
            "tracks": r.tracks.iter().map(|t| json!({
                "id": t.id, "net": t.net, "layer": t.layer, "width": t.width,
                "pts": t.pts.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "vias": r.vias.iter().map(|v| json!({
                "id": v.id, "net": v.net, "x": v.at.x, "y": v.at.y, "d": v.diameter,
                "drill": v.drill, "from": v.from_layer, "to": v.to_layer,
            })).collect::<Vec<_>>(),
            "zones": r.zones.iter().map(|z| json!({
                "id": z.id, "net": z.net, "layer": z.layer,
                "outline": z.outline.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    });
    let drawings = design.drawings.as_ref().map(|d| {
        json!({
            "shapes": d.shapes.iter().map(shape_json).collect::<Vec<_>>(),
            "texts": d.texts.iter().map(|t| json!({
                "id": t.id, "content": t.content, "x": t.at.x, "y": t.at.y, "angle": t.angle,
                "layer": t.layer, "size": t.size_um, "stroke_width": t.stroke_width,
                "justify": match t.justify {
                    eda_model::ir::TextJustify::Left => "left",
                    eda_model::ir::TextJustify::Center => "center",
                    eda_model::ir::TextJustify::Right => "right",
                },
                "mirror": t.mirror,
            })).collect::<Vec<_>>(),
        })
    });
    Ok(json!({
        "name": Path::new(&meta.intent).file_stem().and_then(|s| s.to_str()).unwrap_or("board"),
        "dir": dir.display().to_string(),
        "outline": pl.map(|pl| pl.outline.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()),
        "layers": model.board.layers,
        "snap": meta.snap_um,
        "parts": parts,
        "rules": model.placement_rules,
        // Read-only board-wide defaults for the auxiliary toolbar's
        // track-width/via-size indicators. `/api/cmd` takes any `Cmd` as
        // JSON, including `add_track`/`add_via`, which can draw copper at
        // other widths/sizes than these -- these are only the defaults a
        // new track/via would start from.
        "board_rules": {
            "track_width": model.board.track_width,
            "via_drill": model.board.via_drill,
            "via_diameter": model.board.via_diameter,
            "clearance": model.board.clearance,
        },
        "routing": routing,
        "drawings": drawings,
        "checks": checks,
        "activity": activity,
        "job": job.lock().map(|j| j.clone()).unwrap_or_default(),
    }))
}

/// One `Shape` as JSON: every variant carries `id`/`kind`/`layer`/
/// `stroke_width`/`filled` plus its own geometry, points as `[x, y]` pairs
/// -- the same convention `outline`/`pts` already use elsewhere in this
/// API, so a `(add|move|delete)_shape` command and this read use the same
/// shape.
fn shape_json(s: &Shape) -> Value {
    let pt = |p: eda_model::ir::Point| json!([p.x, p.y]);
    match s {
        Shape::Segment { id, layer, stroke_width, filled, start, end } => {
            json!({ "id": id, "kind": "segment", "layer": layer, "stroke_width": stroke_width, "filled": filled, "start": pt(*start), "end": pt(*end) })
        }
        Shape::Arc { id, layer, stroke_width, filled, start, mid, end } => {
            json!({ "id": id, "kind": "arc", "layer": layer, "stroke_width": stroke_width, "filled": filled, "start": pt(*start), "mid": pt(*mid), "end": pt(*end) })
        }
        Shape::Rect { id, layer, stroke_width, filled, start, end } => {
            json!({ "id": id, "kind": "rect", "layer": layer, "stroke_width": stroke_width, "filled": filled, "start": pt(*start), "end": pt(*end) })
        }
        Shape::Circle { id, layer, stroke_width, filled, center, end } => {
            json!({ "id": id, "kind": "circle", "layer": layer, "stroke_width": stroke_width, "filled": filled, "center": pt(*center), "end": pt(*end) })
        }
        Shape::Polygon { id, layer, stroke_width, filled, pts } => {
            json!({ "id": id, "kind": "polygon", "layer": layer, "stroke_width": stroke_width, "filled": filled, "pts": pts.iter().map(|p| pt(*p)).collect::<Vec<_>>() })
        }
    }
}

/// The design's schematic drawn, or, for a board started from an intent
/// alone, one derived from the intent (cached until the intent changes).
fn schematic_svg(dir: &Path, cache: &Mutex<Option<(std::time::SystemTime, String)>>) -> Result<String, Vec<CheckResult>> {
    let (meta, design, model) = board::load(dir)?;
    if design.schematic.is_some() {
        return eda::render_schematic(&design, &model);
    }
    let when = std::fs::metadata(&meta.intent).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
    if let Ok(c) = cache.lock() {
        if let Some((t, svg)) = c.as_ref() {
            if *t == when {
                return Ok(svg.clone());
            }
        }
    }
    let derived = eda::prelude::derive_schematic(&model, &eda::prelude::EngineOptions::default())?;
    let svg = eda::render_schematic(&derived, &model)?;
    if let Ok(mut c) = cache.lock() {
        *c = Some((when, svg.clone()));
    }
    Ok(svg)
}

/// The same schematic `schematic_svg` draws, but as structured JSON for
/// the browser UI's own KiCad-style renderer instead of one baked image:
/// each symbol instance plus its part's pins (a `SymbolInstance` alone
/// says nothing about what it draws), every wire, and every net label.
/// No second cache next to `schematic_svg`'s -- these boards are small,
/// so deriving again on a cache miss costs nothing worth guarding.
fn schematic_json(dir: &Path) -> Result<Value, Vec<CheckResult>> {
    let (_, design, model) = board::load(dir)?;
    let sch = match design.schematic {
        Some(s) => s,
        None => eda::prelude::derive_schematic(&model, &eda::prelude::EngineOptions::default())?.schematic.unwrap_or(eda_model::ir::SchematicSection { symbols: Vec::new(), wires: Vec::new(), labels: Vec::new() }),
    };
    let symbols: Vec<Value> = sch
        .symbols
        .iter()
        .map(|s| {
            let part = model.part(&s.id);
            json!({
                "id": s.id,
                "at": [s.at.x, s.at.y],
                // Millideg -> plain degrees, same convention `state()` uses for a PCB part's `rot`.
                "rot": s.rot as f64 / 1000.0,
                "mirrored": s.mirrored,
                "value": part.and_then(|p| p.value.clone()),
                "mpn": part.and_then(|p| p.mpn.clone()),
                "package": part.and_then(|p| p.package.clone()),
                "pins": part.map(|p| p.pins.iter().map(|pin| json!({ "number": pin.number, "name": pin.name, "kind": pin.kind })).collect::<Vec<_>>()).unwrap_or_default(),
            })
        })
        .collect();
    let wires: Vec<Value> = sch.wires.iter().map(|w| json!({ "net": w.net, "pins": w.pins, "pts": w.pts.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>() })).collect();
    let labels: Vec<Value> = sch.labels.iter().map(|l| json!({ "net": l.net, "at": [l.at.x, l.at.y] })).collect();
    Ok(json!({ "symbols": symbols, "wires": wires, "labels": labels }))
}
