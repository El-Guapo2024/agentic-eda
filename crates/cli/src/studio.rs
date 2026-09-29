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
use eda_model::ir::{LabelSide, Side};
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

pub fn serve(dir: &Path, port: u16) -> Result<(), Vec<CheckResult>> {
    // Fail now, not on the first request, if this is no board.
    board::load(dir)?;
    // A taken port is usually another studio already showing a board;
    // take the next free one rather than refuse.
    let (listener, got) = (port..port.saturating_add(20))
        .find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok().map(|l| (l, p)))
        .ok_or_else(|| vec![CheckResult::fail("serve_bind", format!("127.0.0.1:{port}"), format!("ports {port}-{} are all taken", port.saturating_add(19)))])?;
    if got != port {
        eprintln!("board: port {port} is taken (another studio open there?), using {got}");
    }
    eprintln!("board: serving {} at http://127.0.0.1:{got}/", dir.display());
    let job: Job = Arc::new(Mutex::new("idle".into()));
    let schematic: Mutex<Option<(std::time::SystemTime, String)>> = Mutex::new(None);
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        if let Err(e) = handle(&mut stream, dir, &job, &schematic) {
            let _ = respond(&mut stream, "500 Internal Server Error", "text/plain", e.as_bytes());
        }
    }
    Ok(())
}

fn handle(stream: &mut TcpStream, dir: &Path, job: &Job, schematic: &Mutex<Option<(std::time::SystemTime, String)>>) -> Result<(), String> {
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
        ("GET", "/") => respond(stream, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
        ("GET", "/api/version") => respond(stream, "200 OK", "application/json", version(dir, job).to_string().as_bytes()),
        ("GET", "/api/state") => {
            let v = state(dir, job).unwrap_or_else(|e| json!({ "error": board::reasons(&e) }));
            respond(stream, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("GET", "/api/schematic.svg") => match schematic_svg(dir, schematic) {
            Ok(svg) => respond(stream, "200 OK", "image/svg+xml", svg.as_bytes()),
            Err(e) => respond(stream, "404 Not Found", "text/plain", board::reasons(&e).as_bytes()),
        },
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
            "tracks": r.tracks.iter().map(|t| json!({ "net": t.net, "layer": t.layer, "width": t.width, "pts": t.pts.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>() })).collect::<Vec<_>>(),
            "vias": r.vias.iter().map(|v| json!({ "net": v.net, "x": v.at.x, "y": v.at.y, "d": v.diameter })).collect::<Vec<_>>(),
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
        "routing": routing,
        "checks": checks,
        "activity": activity,
        "job": job.lock().map(|j| j.clone()).unwrap_or_default(),
    }))
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
