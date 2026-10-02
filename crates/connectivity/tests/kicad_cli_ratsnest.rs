//! Oracle verification against `kicad-cli pcb drc`: compare this crate's
//! ratsnest/dangling-copper counts to KiCad's own `unconnected_items`,
//! `track_dangling` and `via_dangling` (the same JSON fields the task
//! names, confirmed against `pcbnew/drc/drc_item.cpp`'s registered type
//! strings), on:
//! - a placement with no routing at all ("unrouted placements" -- every
//!   net's pads still need every ratsnest line);
//! - our own pipeline's routed output with some tracks deleted and others
//!   shortened into stubs ("partially routed boards ... delete some
//!   tracks", plus stubs specifically to exercise dangling detection);
//! - the same routed board round-tripped through `import_kicad_pcb` (the
//!   importer path `crates/kicad/tests/kicad_cli_import.rs` exercises),
//!   so both kicad-cli and this crate are looking at the same file.
//!
//! `#[ignore]`d like `crates/kicad/tests/kicad_cli_drc.rs`; run with
//! `cargo test -p eda-connectivity --test kicad_cli_ratsnest -- --ignored --nocapture`.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use eda_connectivity::{analyze, DanglingKind};
use eda_engine::{derive_schematic, EngineOptions};
use eda_kicad::{export_kicad_pcb, import_kicad_pcb, ExportMeta};
use eda_model::ir::{Design, Point};
use eda_model::ConstraintModel;
use eda_place::{place, PlaceOptions};

/// Our exporter writes the KiCad 9 file format; an older kicad-cli (e.g.
/// Ubuntu's 7.0) refuses to open it, so it can't serve as the oracle.
fn kicad_cli_reads_our_format(cli: &Path) -> bool {
    let v = Command::new(cli).arg("version").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let major: u32 = v.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0);
    if major < 9 {
        eprintln!("kicad-cli {v} is older than the KiCad 9 format we export; skipping");
    }
    major >= 9
}

fn find_kicad_cli() -> Option<PathBuf> {
    find_kicad_cli_any().filter(|p| kicad_cli_reads_our_format(p))
}

fn find_kicad_cli_any() -> Option<PathBuf> {
    if let Ok(out) = Command::new("which").arg("kicad-cli").output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return Some(PathBuf::from(p));
            }
        }
    }
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli");
    mac.exists().then_some(mac)
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().and_then(|p| p.parent()).expect("crates/connectivity -> repo root").to_path_buf()
}

fn load_model(name: &str) -> ConstraintModel {
    let repo = repo_root();
    let candidates = [repo.join("examples").join(format!("{name}.yaml")), repo.join("examples").join("ladder").join(format!("{name}.yaml"))];
    let path = candidates.iter().find(|p| p.exists()).unwrap_or_else(|| panic!("no example named {name} under examples/ or examples/ladder/"));
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parse {name}: {e}"))
}

/// FreeRouting's best effort on `design` -- may leave nets unrouted; we
/// only need *some* copper to damage for the partial-routing scenario; a
/// board this port can't fully route is still a fine source of tracks.
fn route_best_effort(design: &Design, model: &ConstraintModel) -> Design {
    let mut out = design.clone();
    match eda_freeroute::design::route_design(design, model, &model.board, 20, 10) {
        Ok(routed) => {
            if !routed.unrouted.is_empty() {
                eprintln!("route_best_effort: left unrouted: {:?}", routed.unrouted);
            }
            out.routing = Some(routed.routing);
        }
        Err(e) => panic!("route_design failed outright: {e}"),
    }
    out
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
struct Counts {
    unconnected: usize,
    track_dangling: usize,
    via_dangling: usize,
}

fn our_counts(design: &Design, model: &ConstraintModel) -> Counts {
    let report = analyze(design, model);
    let track_dangling = report.dangling.iter().filter(|d| d.kind == DanglingKind::Track).count();
    let via_dangling = report.dangling.iter().filter(|d| d.kind == DanglingKind::Via).count();
    Counts { unconnected: report.ratsnest.len(), track_dangling, via_dangling }
}

/// Run `kicad-cli pcb drc` on `pcb` and pull out the three counts this
/// crate is checked against. `--refill-zones`: we export a zone's outline
/// but not its fill, so without a refill kicad-cli would DRC an empty
/// plane and call every stitching via dangling -- a false mismatch that
/// says nothing about this crate's own logic (same reasoning
/// `kicad_cli_drc.rs` documents at its own `--refill-zones` use).
fn oracle_counts(cli: &Path, pcb: &Path) -> Counts {
    let report_path = pcb.with_extension("drc.json");
    let out = Command::new(cli)
        .args(["pcb", "drc", "--refill-zones", "--format", "json", "--severity-all", "--exit-code-violations", "--output"])
        .arg(&report_path)
        .arg(pcb)
        .output()
        .expect("failed to run kicad-cli pcb drc");
    eprintln!("kicad-cli drc({}) stdout:\n{}\nstderr:\n{}", pcb.display(), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(report_path.exists(), "kicad-cli did not write a DRC report for {}", pcb.display());
    let text = std::fs::read_to_string(&report_path).unwrap();
    let report: serde_json::Value = serde_json::from_str(&text).expect("parse DRC json");

    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    if let Some(vs) = report.get("violations").and_then(|v| v.as_array()) {
        for v in vs {
            let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("unknown").to_string();
            *by_type.entry(ty).or_default() += 1;
        }
    }
    let unconnected = report.get("unconnected_items").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
    Counts { unconnected, track_dangling: by_type.get("track_dangling").copied().unwrap_or(0), via_dangling: by_type.get("via_dangling").copied().unwrap_or(0) }
}

fn write_pcb(design: &Design, model: &ConstraintModel, dir: &Path, name: &str) -> PathBuf {
    let meta = ExportMeta { date: "2026-01-01", title: name };
    let text = export_kicad_pcb(design, model, &meta).unwrap_or_else(|e| panic!("export_kicad_pcb {name}: {e:?}"));
    let path = dir.join(format!("{name}.kicad_pcb"));
    std::fs::write(&path, text).unwrap();
    path
}

/// Damage a routed design the way a half-finished layout actually looks:
/// delete every third track outright (its net now needs a fresh ratsnest
/// line), and shorten every other-third track to 40% of its length (its
/// far end now dangles, but its near end still lands exactly where it
/// always did, on a pad or via).
fn damage_routing(design: &mut Design) {
    let Some(rt) = design.routing.as_mut() else { return };
    let mut kept = Vec::with_capacity(rt.tracks.len());
    for (i, mut t) in rt.tracks.drain(..).enumerate() {
        match i % 3 {
            0 => continue, // deleted outright
            1 => {
                if let [a, b] = t.pts[..] {
                    let short = Point { x: a.x + (b.x - a.x) * 2 / 5, y: a.y + (b.y - a.y) * 2 / 5 };
                    t.pts = vec![a, short];
                }
                kept.push(t);
            }
            _ => kept.push(t), // untouched
        }
    }
    rt.tracks = kept;
}

struct Scenario {
    label: &'static str,
    ours: Counts,
    oracle: Counts,
}

fn check_board(cli: &Path, name: &str, seed: u64) -> Vec<Scenario> {
    let model = load_model(name);
    let opts = EngineOptions { seed, intent_hash: "connectivity_oracle".into(), ..Default::default() };
    let design = derive_schematic(&model, &opts).unwrap_or_else(|e| panic!("derive_schematic {name}: {e:?}"));
    let placed = place(&design, &model, &PlaceOptions { seed, ..Default::default() }).unwrap_or_else(|e| panic!("place {name}: {e:?}"));

    let dir = std::env::temp_dir().join("eda_connectivity_oracle").join(name);
    std::fs::create_dir_all(&dir).unwrap();

    let mut scenarios = Vec::new();

    // 1. Placement only: no copper anywhere.
    let pcb = write_pcb(&placed, &model, &dir, "placement_only");
    scenarios.push(Scenario { label: "placement-only", ours: our_counts(&placed, &model), oracle: oracle_counts(cli, &pcb) });

    // 2. Fully (best-effort) routed, then round-tripped through the
    //    importer -- both kicad-cli and this crate see the same file.
    let routed = route_best_effort(&placed, &model);
    let full_pcb = write_pcb(&routed, &model, &dir, "routed_full");
    let oracle_full = oracle_counts(cli, &full_pcb);
    let text = std::fs::read_to_string(&full_pcb).unwrap();
    let (imported_design, imported_model, notes) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import_kicad_pcb {name}: {e:?}"));
    eprintln!("{name}: import notes: zones_skipped={} track_arcs_approximated={}", notes.zones_skipped, notes.track_arcs_approximated);
    scenarios.push(Scenario { label: "imported-round-trip", ours: our_counts(&imported_design, &imported_model), oracle: oracle_full });

    // 3. Damage the routing: deleted tracks need a fresh ratsnest line;
    //    shortened tracks dangle at their new, unconnected end.
    let mut damaged = routed;
    damage_routing(&mut damaged);
    let damaged_pcb = write_pcb(&damaged, &model, &dir, "partially_routed");
    scenarios.push(Scenario { label: "partially-routed", ours: our_counts(&damaged, &model), oracle: oracle_counts(cli, &damaged_pcb) });

    scenarios
}

/// Compare every scenario's three counts for `name` against kicad-cli;
/// prints a line per check and returns `(exact_matches, total, mismatch
/// descriptions)`.
fn compare_board(cli: &Path, name: &str, seed: u64) -> (usize, usize, Vec<String>) {
    let (mut matches, mut total, mut mismatches) = (0usize, 0usize, Vec::new());
    for scenario in check_board(cli, name, seed) {
        for (field, ours, oracle) in [
            ("unconnected_items", scenario.ours.unconnected, scenario.oracle.unconnected),
            ("track_dangling", scenario.ours.track_dangling, scenario.oracle.track_dangling),
            ("via_dangling", scenario.ours.via_dangling, scenario.oracle.via_dangling),
        ] {
            total += 1;
            if ours == oracle {
                matches += 1;
            } else {
                mismatches.push(format!("{name}/{}: {field} ours={ours} oracle={oracle}", scenario.label));
            }
            println!("{name:16} {:20} {field:18} ours={ours:<4} oracle={oracle:<4} {}", scenario.label, if ours == oracle { "match" } else { "DIFF" });
        }
    }
    (matches, total, mismatches)
}

#[test]
#[ignore]
fn kicad_cli_ratsnest_oracle() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };

    // A spread of board shapes: two simple two-pin nets, a star topology
    // (several clusters on one net -> a multi-edge ratsnest), and a real
    // multi-part ladder board. None of these declare a copper pour, so the
    // zone-outline approximation (see crates/connectivity/src/items.rs)
    // never comes into play: every count is expected to match exactly.
    // (`kicad_cli_ratsnest_zone_approximation`, below, covers a board that
    // does have one, and does not assert -- see its own doc comment.)
    let boards = ["two_pin_nets", "star_net", "l1_usb_mcu"];

    let mut total_checks = 0usize;
    let mut exact_matches = 0usize;
    let mut mismatches = Vec::new();
    for name in boards {
        let (m, t, mut miss) = compare_board(&cli, name, 0);
        exact_matches += m;
        total_checks += t;
        mismatches.append(&mut miss);
    }

    println!("\nmatch rate: {exact_matches}/{total_checks} ({:.1}%)", 100.0 * exact_matches as f64 / total_checks as f64);
    assert!(mismatches.is_empty(), "{}/{} checks mismatched kicad-cli:\n{}", mismatches.len(), total_checks, mismatches.join("\n"));
}

/// `examples/ladder/l4_control_hub.yaml` declares a copper pour
/// (`board.pours`), so this board's zones are exactly where the
/// outline-only approximation (see `crates/connectivity/src/items.rs`)
/// diverges from KiCad's real, filled-polygon connectivity. Not asserted
/// on -- the point is to see and report the gap, not fail the suite over
/// a documented, intentional limitation that zone fill will close.
#[test]
#[ignore]
fn kicad_cli_ratsnest_zone_approximation() {
    let Some(cli) = find_kicad_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let (matches, total, mismatches) = compare_board(&cli, "l4_control_hub", 0);
    println!("\nzone-pour board match rate: {matches}/{total} ({:.1}%)", 100.0 * matches as f64 / total as f64);
    if !mismatches.is_empty() {
        println!("mismatches (expected -- outline-only zone approximation, no fill yet):\n{}", mismatches.join("\n"));
    }
}
