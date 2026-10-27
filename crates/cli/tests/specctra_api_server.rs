//! File > Import > Specctra Session two ways, on every core example board: KiCad's own `ImportSpecctraSession` through `kicad-cli api-server` (the default
//! where kicad-cli has the command) against our port of `FromSESSION` (`EDA_KICAD_API=0`, the fallback, INACTIVE otherwise). Each board is placed and routed
//! by `eda pipeline` (the router is `crates/freeroute`, our port of Freerouting); kicad-cli writes its DSN (`pcb export specctra`), and the session a router
//! that keeps that routing sends back (`eda_kicad::specctra::session::echo_session`, Freerouting's shape of a session) is imported onto the board with its
//! routing taken away, once each way. The two designs must have the same tracks and vias (same count, the same points to the nanometre: both scale the
//! session's `(resolution um 10)` by `KiROUND( factor * distance / resValue )`), and the parts where they were; and each must be the routed board's routing
//! within the session's resolution (0.1 um: half of it is the most a coordinate moves).
//!
//! On the first board two more sessions: one that moves, turns and flips a part (the same placement both ways), and one naming a part the board has not
//! got, where the two differ by design: KiCad 25ec2335 skips it and imports the rest, our port of 8303b2ad refuses the session ("Reference '%s' not found.").
//!
//! `SPECCTRA_EXAMPLES=a.yaml,b.yaml` names others. Slow tier (`EDA_SLOW_TESTS=1`): it routes every board and starts kicad-cli; a kicad-cli without
//! `api-server` skips it.

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn eda(args: &[&str], envs: &[(&str, &str)]) -> (bool, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_eda"));
    cmd.args(args).env_remove("EDA_KICAD_API").env_remove("EDA_KICAD_CLI_NO_SPECCTRA");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("run eda");
    (out.status.success(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

fn ok(args: &[&str], envs: &[(&str, &str)]) -> String {
    let (good, log) = eda(args, envs);
    assert!(good, "eda {}:\n{log}", args.join(" "));
    log
}

fn design(dir: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir.join("design.json")).unwrap()).unwrap()
}

/// A track or via as a comparable key: (net, layer, width, points) and (net, at, drill, diameter, layers), in nanometres.
type TrackKey = (String, String, i64, Vec<(i64, i64)>);
type ViaKey = (String, (i64, i64), i64, i64, String, String);

fn n(v: &Value) -> i64 {
    v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64)).unwrap_or(0)
}

fn tracks(d: &Value) -> Vec<TrackKey> {
    let mut t: Vec<TrackKey> = d["routing"]["tracks"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|t| (t["net"].as_str().unwrap_or("").to_string(), t["layer"].as_str().unwrap_or("").to_string(), n(&t["width"]), t["pts"].as_array().unwrap().iter().map(|p| (n(&p["x"]), n(&p["y"]))).collect()))
        .collect();
    t.sort();
    t
}

/// The routed board's tracks cut into segments, as a session gives them back (one `PCB_TRACK` per segment of a path).
fn segments(d: &Value) -> Vec<TrackKey> {
    let mut out = Vec::new();
    for (net, layer, w, pts) in tracks(d) {
        for p in pts.windows(2) {
            out.push((net.clone(), layer.clone(), w, p.to_vec()));
        }
    }
    out.sort();
    out
}

fn vias(d: &Value) -> Vec<ViaKey> {
    let mut v: Vec<ViaKey> = d["routing"]["vias"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|v| (v["net"].as_str().unwrap_or("").to_string(), (n(&v["at"]["x"]), n(&v["at"]["y"])), n(&v["drill"]), n(&v["diameter"]), v["from_layer"].as_str().unwrap_or("").to_string(), v["to_layer"].as_str().unwrap_or("").to_string()))
        .collect();
    v.sort();
    v
}

/// The largest distance (nm, per coordinate) between two equally long lists of points, or `None` when they do not pair up (count, net, layer, width).
fn max_track_shift(a: &[TrackKey], b: &[TrackKey]) -> Option<i64> {
    if a.len() != b.len() {
        return None;
    }
    // Paired by net, layer and width, then by nearest points: sort each group and walk.
    let group = |v: &[TrackKey]| -> BTreeMap<(String, String, i64), Vec<Vec<(i64, i64)>>> {
        let mut m: BTreeMap<(String, String, i64), Vec<Vec<(i64, i64)>>> = BTreeMap::new();
        for (net, layer, w, pts) in v {
            // A segment is the same either way round.
            let mut p = pts.clone();
            if p.first() > p.last() {
                p.reverse();
            }
            m.entry((net.clone(), layer.clone(), *w)).or_default().push(p);
        }
        for g in m.values_mut() {
            g.sort();
        }
        m
    };
    let (ga, gb) = (group(a), group(b));
    if ga.keys().ne(gb.keys()) {
        return None;
    }
    let mut worst = 0;
    for (k, pa) in &ga {
        let pb = &gb[k];
        if pa.len() != pb.len() {
            return None;
        }
        for (x, y) in pa.iter().zip(pb) {
            if x.len() != y.len() {
                return None;
            }
            for (p, q) in x.iter().zip(y) {
                worst = worst.max((p.0 - q.0).abs()).max((p.1 - q.1).abs());
            }
        }
    }
    Some(worst)
}

fn max_via_shift(a: &[ViaKey], b: &[ViaKey]) -> Option<i64> {
    if a.len() != b.len() {
        return None;
    }
    let mut worst = 0;
    for (x, y) in a.iter().zip(b) {
        if (&x.0, x.2, x.3, &x.4, &x.5) != (&y.0, y.2, y.3, &y.4, &y.5) {
            return None;
        }
        worst = worst.max((x.1 .0 - y.1 .0).abs()).max((x.1 .1 - y.1 .1).abs());
    }
    Some(worst)
}

fn core_examples(root: &Path) -> Vec<PathBuf> {
    if let Ok(v) = std::env::var("SPECCTRA_EXAMPLES") {
        return v.split(',').map(|f| root.join(f)).collect();
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(root).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "yaml")).collect();
    files.sort();
    files
}

#[test]
fn kicads_session_import_and_ours_put_the_same_routing_on_every_example_board() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let cli = ["/usr/local/bin/kicad-cli", "/usr/bin/kicad-cli", "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"].into_iter().find(|c| Path::new(c).is_file());
    let Some(cli) = std::env::var("EDA_KICAD_CLI").ok().or(cli.map(String::from)) else {
        eprintln!("skipped: no kicad-cli");
        return;
    };
    let help = Command::new(&cli).arg("--help").output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    if !help.contains("api-server") {
        eprintln!("skipped: kicad-cli {cli} has no api-server");
        return;
    }
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let root = std::env::temp_dir().join(format!("eda_specctra_api_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut rows: Vec<String> = Vec::new();
    let mut compared = 0;
    let mut failures: Vec<String> = Vec::new();
    for yaml in core_examples(&examples) {
        let name = yaml.file_stem().unwrap().to_string_lossy().to_string();
        let dir = root.join(&name);
        std::fs::create_dir_all(&dir).unwrap();
        let pipe = dir.join("pipeline");
        let (routed_ok, log) = eda(&["pipeline", yaml.to_str().unwrap(), "-o", pipe.to_str().unwrap(), "--seed", "0"], &[]);
        if !pipe.join("design.json").is_file() {
            rows.push(format!("{name:28} no design (pipeline: {})", log.lines().last().unwrap_or("")));
            continue;
        }
        let routed = design(&pipe);
        let (want_tracks, want_vias) = (segments(&routed), vias(&routed));
        if want_tracks.is_empty() {
            rows.push(format!("{name:28} not routed (pipeline ok={routed_ok})"));
            continue;
        }
        let board = |tag: &str| -> PathBuf {
            let b = dir.join(tag);
            ok(&["board", "new", yaml.to_str().unwrap(), "-o", b.to_str().unwrap()], &[]);
            std::fs::copy(pipe.join("design.json"), b.join("design.json")).unwrap();
            b
        };
        let src = board("routed");
        let dsn = dir.join("b.dsn");
        ok(&["board", "export", "specctra", "-o", dsn.to_str().unwrap(), "-C", src.to_str().unwrap()], &[]);
        let text = std::fs::read_to_string(&dsn).unwrap();
        let pcb = eda_kicad::specctra::parse::load_pcb(&text, "b.dsn").expect("the DSN reads back");
        let ses = dir.join("b.ses");
        std::fs::write(&ses, eda_kicad::specctra::session::echo_session(&pcb, "b.ses")).unwrap();

        let mut stripped = routed.clone();
        stripped["routing"]["tracks"] = Value::Array(vec![]);
        stripped["routing"]["vias"] = Value::Array(vec![]);
        let import = |tag: &str, envs: &[(&str, &str)]| -> (Value, String, f64) {
            let b = board(tag);
            std::fs::write(b.join("design.json"), serde_json::to_string_pretty(&stripped).unwrap()).unwrap();
            let t = std::time::Instant::now();
            let log = ok(&["board", "import-ses", ses.to_str().unwrap(), "-C", b.to_str().unwrap()], envs);
            (design(&b), log, t.elapsed().as_secs_f64())
        };
        let (kicad, klog, kt) = import("kicad", &[("EDA_SAY_SESSION_ENGINE", "1")]);
        assert!(klog.contains("session engine: kicad-cli api-server"), "{name}: the import went through the api-server: {klog}");
        let (ours, olog, ot) = import("eda", &[("EDA_SAY_SESSION_ENGINE", "1"), ("EDA_KICAD_API", "0")]);
        assert!(olog.contains("session engine: eda"), "{name}: EDA_KICAD_API=0 takes our port: {olog}");

        let (kt_, kv) = (tracks(&kicad), vias(&kicad));
        let (ot_, ov) = (tracks(&ours), vias(&ours));
        let same = kt_ == ot_ && kv == ov && kicad["placement"]["footprints"] == ours["placement"]["footprints"];
        // The pipeline's design names a sheet's nets as the intent does (`FB`), the board the studio derives as KiCad does (`/FB`, the net names decision of
        // 2026-10-23), and both imports give the board's names: the routed board is compared with the leading `/` dropped.
        let bare_t = |v: &[TrackKey]| -> Vec<TrackKey> { let mut v: Vec<TrackKey> = v.iter().map(|(n, l, w, p)| (n.trim_start_matches('/').to_string(), l.clone(), *w, p.clone())).collect(); v.sort(); v };
        let bare_v = |v: &[ViaKey]| -> Vec<ViaKey> { let mut v: Vec<ViaKey> = v.iter().map(|(n, a, d, w, f, t)| (n.trim_start_matches('/').to_string(), *a, *d, *w, f.clone(), t.clone())).collect(); v.sort(); v };
        let shift_t = max_track_shift(&bare_t(&kt_), &bare_t(&want_tracks));
        let shift_v = max_via_shift(&bare_v(&kv), &bare_v(&want_vias));
        rows.push(format!(
            "{name:28} tracks {:4} / {:4} vias {:3} / {:3}  kicad==eda: {}  vs routed: max shift tracks {} nm, vias {} nm  import {kt:.2} s (api-server) / {ot:.2} s (eda)",
            kt_.len(),
            ot_.len(),
            kv.len(),
            ov.len(),
            if same { "yes" } else { "NO" },
            shift_t.map_or("unpaired".to_string(), |s| s.to_string()),
            shift_v.map_or("unpaired".to_string(), |s| s.to_string())
        ));
        if !same {
            let diff_t: Vec<_> = kt_.iter().filter(|t| !ot_.contains(t)).take(3).collect();
            let diff_o: Vec<_> = ot_.iter().filter(|t| !kt_.contains(t)).take(3).collect();
            failures.push(format!("{name}: kicad-only {diff_t:?} / eda-only {diff_o:?} / vias {} vs {}", kv.len(), ov.len()));
        }
        if !shift_t.is_some_and(|s| s <= 50) || !shift_v.is_some_and(|s| s <= 50) {
            failures.push(format!("{name}: the imported routing is not the routed board's within the session's resolution ({shift_t:?}, {shift_v:?})"));
        }
        if compared == 0 {
            // The placement: a session that moves, turns and flips a part (the first `(place ..)`: 1 mm right, 90 degrees more, the other side) puts it
            // in the same place both ways.
            let text = std::fs::read_to_string(&ses).unwrap();
            let at = text.find("(place ").expect("a placement");
            let end = at + text[at..].find(')').unwrap();
            let words: Vec<&str> = text[at + 7..end].split_whitespace().collect();
            let (x, side, rot) = (words[1].parse::<f64>().unwrap(), words[3], words[4].parse::<f64>().unwrap());
            let moved = format!("(place {} {} {} {} {}", words[0], x + 10000.0, words[2], if side == "front" { "back" } else { "front" }, (rot + 90.0) % 360.0);
            let moved_ses = dir.join("moved.ses");
            std::fs::write(&moved_ses, format!("{}{}{}", &text[..at], moved, &text[end..])).unwrap();
            let import_moved = |tag: &str, envs: &[(&str, &str)]| -> Value {
                let b = board(tag);
                std::fs::write(b.join("design.json"), serde_json::to_string_pretty(&stripped).unwrap()).unwrap();
                ok(&["board", "import-ses", moved_ses.to_str().unwrap(), "-C", b.to_str().unwrap()], envs);
                design(&b)
            };
            let k = import_moved("kicad-moved", &[]);
            let o = import_moved("eda-moved", &[("EDA_KICAD_API", "0")]);
            let fp = |d: &Value| d["placement"]["footprints"].as_array().unwrap().iter().find(|f| f["id"] == words[0]).cloned().unwrap();
            assert_ne!(fp(&k), fp(&routed), "{name}: the session moved {}", words[0]);
            assert_eq!(fp(&k), fp(&o), "{name}: {} is where KiCad's import and ours put it", words[0]);
            assert_eq!(k["placement"]["footprints"], o["placement"]["footprints"], "{name}: every part");
            rows.push(format!("{name:28} moved {}: {} (both ways)", words[0], fp(&k)));

            // A part the board has not got: KiCad 25ec2335 skips it and imports the rest (`m_skippedCount`); our port of 8303b2ad refuses the session.
            let other = text.replacen("(place ", "(place NOPE_", 1);
            let bad = dir.join("other.ses");
            std::fs::write(&bad, other).unwrap();
            let b = board("kicad-other");
            std::fs::write(b.join("design.json"), serde_json::to_string_pretty(&stripped).unwrap()).unwrap();
            let log = ok(&["board", "import-ses", bad.to_str().unwrap(), "-C", b.to_str().unwrap()], &[]);
            assert!(log.contains("1 session item(s) were skipped due to unresolved reference, layer, or padstack."), "{name}: KiCad skips the part it has not got: {log}");
            assert_eq!(tracks(&design(&b)), kt_, "{name}: and imports the routing");
            let b = board("eda-other");
            std::fs::write(b.join("design.json"), serde_json::to_string_pretty(&stripped).unwrap()).unwrap();
            let (good, log) = eda(&["board", "import-ses", bad.to_str().unwrap(), "-C", b.to_str().unwrap()], &[("EDA_KICAD_API", "0")]);
            assert!(!good && log.contains("Reference 'NOPE_"), "{name}: our port refuses it in 8303b2ad's words: {log}");
        }
        compared += 1;
    }
    eprintln!("\n{}", rows.join("\n"));
    if failures.is_empty() {
        let _ = std::fs::remove_dir_all(&root);
    } else {
        eprintln!("boards kept in {}", root.display());
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(compared > 0, "no example was routed");
}
