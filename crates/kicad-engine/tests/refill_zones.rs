//! Fill All Zones by KiCad's own filler (`eda_kicad_engine::refill_zones`), against kicad-cli itself.
//!
//! The proof has three parts, on KiCad's QA boards (`qa/data/pcbnew`), whose zones are the hard cases (a net tie, drilled holes with no copper, zones on technical layers, zones on
//! several layers, two zones of one priority that yield to each other by their uuids). `REFILL_BOARDS=a.kicad_pcb,b/c.kicad_pcb` names others; `issue22826`'s inner layers differ
//! by 0.5 to 0.8% because its through-hole pads are `(remove_unused_layers yes)`, which the IR does not keep (GAPS.md item 13):
//!   1. the fill read back from the derived board is the fill kicad-cli makes of the original: per layer, the area of every ring is the same to within a fraction of a percent,
//!      which is also the proof that the derived board is the original board (a hole or a tie it lost would show here);
//!   2. the board the studio derives with those fills in it is read by kicad-cli, whose plain DRC (no `--refill-zones`: it judges the fill the studio keeps) runs on it, and a
//!      DRC asked to refill first hands back the fills it made;
//!   3. the fills are what the cache hands back, to the nanometre, and the fills a `.kicad_pcb` is saved with are the ones an import files for its zones.
//! Slow tier (kicad-cli fills each board twice): `EDA_SLOW_TESTS=1`.

use eda_kicad::zone_fills::{entries_of_imported_file, scan_zone_fills};
use eda_kicad::{import_kicad_pcb, zone_fills};
use eda_kicad_engine::{cli_command, drc, find_cli, refill_zones};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn qa_boards_dir() -> PathBuf {
    let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
    root.join("pcbnew")
}

/// The area (mm²) of a ring of nanometre points (the shoelace sum: a fractured ring's bridges cancel, so this is the island's copper).
fn ring_area_mm2(pts: &[[i64; 2]]) -> f64 {
    let twice: i128 = (0..pts.len()).map(|i| pts[i][0] as i128 * pts[(i + 1) % pts.len()][1] as i128 - pts[(i + 1) % pts.len()][0] as i128 * pts[i][1] as i128).sum();
    (twice as f64 / 2.0).abs() / 1.0e12
}

#[test]
fn kicad_cli_fills_the_derived_board_as_it_fills_the_original() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let boards = std::env::var("REFILL_BOARDS").map(|v| v.split(',').map(String::from).collect::<Vec<_>>()).unwrap_or_else(|_| ["issue5990.kicad_pcb", "issue14008.kicad_pcb", "issue6260.kicad_pcb", "issue5750.kicad_pcb"].map(String::from).to_vec());
    let root = std::env::var_os("PARITY_TMP").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("eda_refill_zones"));
    let mut table = String::from("| board | layer | kicad-cli on the original mm² | Fill All Zones mm² | diff % | rings (original / ours) |\n|---|---|---|---|---|---|\n");
    let mut times = String::from("| board | Fill All Zones (B) seconds | rings |\n|---|---|---|\n");
    let mut bad = Vec::new();
    for rel in &boards {
        let text = std::fs::read_to_string(qa_boards_dir().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let board = root.join(rel.replace(['/', '.'], "_"));
        let _ = std::fs::remove_dir_all(&board);
        std::fs::create_dir_all(board.join(".kicad")).unwrap();

        // 1. The oracle: kicad-cli refills the original board.
        let original = board.join("original.kicad_pcb");
        std::fs::write(&original, &text).unwrap();
        // The reference run is started as every other one is (`cli_command`: none of the user's KiCad settings), so it and the refill under test see the same KiCad.
        let out = cli_command(&cli, None).args(["pcb", "drc", "--refill-zones", "--save-board", "--format", "report", "--output"]).arg(board.join("original.report")).arg(&original).output().expect("run kicad-cli");
        assert!(out.status.success(), "{rel}: {}", String::from_utf8_lossy(&out.stderr));
        let mut oracle: BTreeMap<String, (f64, usize)> = BTreeMap::new();
        // (a fill with no points marks a zone filled with nothing: no ring)
        for f in scan_zone_fills(&std::fs::read_to_string(&original).unwrap()).into_iter().filter(|f| !f.pts.is_empty()) {
            let e = oracle.entry(f.layer).or_default();
            e.0 += ring_area_mm2(&f.pts);
            e.1 += 1;
        }

        // The studio's Fill All Zones on the same board as the studio has it.
        let (mut design, model, _) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import {rel}: {e:?}"));
        design.routing.as_mut().expect("a routing section").assign_missing_ids();
        let work = board.join(".kicad");
        let fills = refill_zones(&design, &model, &work).unwrap_or_else(|e| panic!("{rel}: {e:?}"));
        let (current, stale) = fills.tally(&design);
        assert!(current >= 1 && stale == 0, "{rel}: the fill is for the zones it was made of: {current} current, {stale} stale");
        assert!(fills.engine.starts_with("kicad-cli "), "{}", fills.engine);
        let mut ours: BTreeMap<String, (f64, usize)> = BTreeMap::new();
        for entry in fills.zones.values() {
            for (layer, rings) in &entry.layers {
                let e = ours.entry(layer.clone()).or_default();
                e.0 += rings.iter().map(|r| ring_area_mm2(&r.pts)).sum::<f64>();
                e.1 += rings.len();
            }
        }
        assert!(!ours.is_empty(), "{rel}: kicad-cli left no zone with a fill");
        times.push_str(&format!("| {rel} | {:.1} | {} |\n", fills.seconds(), fills.counts().1));
        for layer in oracle.keys().chain(ours.keys()).cloned().collect::<std::collections::BTreeSet<_>>() {
            let (a, b) = (oracle.get(&layer).copied().unwrap_or_default(), ours.get(&layer).copied().unwrap_or_default());
            let pct = if a.0 > 0.0 { 100.0 * (a.0 - b.0).abs() / a.0 } else if b.0 > 0.0 { 100.0 } else { 0.0 };
            table.push_str(&format!("| {rel} | {layer} | {:.3} | {:.3} | {pct:.3} | {} / {} |\n", a.0, b.0, a.1, b.1));
            if pct > 0.5 || a.1 != b.1 {
                bad.push(format!("{rel} {layer}: {:.3} mm² in {} rings against {:.3} mm² in {} rings", a.0, a.1, b.0, b.1));
            }
        }

        // 2 and 3. Kept beside the design, the fills are what the derived board is written with, and kicad-cli's plain DRC reads it.
        zone_fills::store(&board, &fills).unwrap();
        assert_eq!(zone_fills::load(&board).as_ref(), Some(&fills), "{rel}: the cache gives back what was kept, to the nanometre");
        let report = drc(&design, &model, &work, false).unwrap_or_else(|e| panic!("{rel}: DRC of the board with kicad-cli's fills: {e:?}"));
        assert!(!report.zones_refilled_by_kicad, "the DRC of the board as it is does not refill");
        let derived = std::fs::read_to_string(work.join("board.kicad_pcb")).unwrap();
        let (_, rings) = fills.counts();
        assert_eq!(derived.matches("(filled_polygon").count(), rings, "{rel}: the derived board holds kicad-cli's fills, ring for ring");
        assert_eq!(derived.matches("(island yes)").count(), fills.zones.values().flat_map(|e| e.layers.values()).flatten().filter(|r| r.island).count(), "{rel}: and which of them are islands");
        // A DRC asked to refill first makes the fills itself, on a copy, and checks the board with them.
        let again = drc(&design, &model, &work, true).unwrap_or_else(|e| panic!("{rel}: DRC with a refill first: {e:?}"));
        assert!(again.zones_refilled_by_kicad);
        assert_eq!(again.zone_fills.as_ref().map(|f| f.counts()), Some(fills.counts()), "{rel}: the refill is the same one");

        // The fills a file is saved with are the ones an import files for its zones, found by the uuid the zones have in the file: kicad-cli saved the refilled original, and
        // the zones of the board imported from the original file are the zones of that one.
        let saved = std::fs::read_to_string(&original).unwrap();
        let mut fresh = design.clone();
        fresh.routing.as_mut().unwrap().assign_missing_ids();
        let filed = entries_of_imported_file(&fresh, scan_zone_fills(&saved));
        let filed_rings: usize = filed.values().flat_map(|e| e.layers.values()).map(Vec::len).sum();
        assert_eq!(filed_rings, oracle.values().map(|v| v.1).sum::<usize>(), "{rel}: an import files every ring the file was saved with");
    }
    println!("\n{table}\n{times}");
    assert!(bad.is_empty(), "the fill of the derived board is not the original's:\n{}\n{table}", bad.join("\n"));
}

/// The F.Cu Gerber kicad-cli plots of `design` with `fills` in it, in bytes.
fn front_copper_gerber_bytes(cli: &std::path::Path, design: &eda_model::ir::Design, model: &eda_model::ConstraintModel, fills: &zone_fills::KicadFills, dir: &std::path::Path, tag: &str) -> u64 {
    let pcb = dir.join(format!("{tag}.kicad_pcb"));
    std::fs::write(&pcb, eda_kicad::export_kicad_pcb_with(design, model, &eda_kicad::ExportMeta { date: "2026-01-01", title: tag }, Some(fills)).expect("export")).unwrap();
    let out_dir = dir.join(format!("{tag}_gerbers"));
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).unwrap();
    let out = cli_command(cli, None).args(["pcb", "export", "gerbers", "--layers", "F.Cu", "--output"]).arg(&out_dir).arg(&pcb).output().expect("run kicad-cli");
    assert!(out.status.success(), "{tag}: {}", String::from_utf8_lossy(&out.stderr));
    std::fs::read_dir(&out_dir).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| x != "gbrjob")).map(|e| e.metadata().unwrap().len()).sum()
}

/// p13-p16 P13-1: Fill All (and every refill before a DRC or a plot) kept no teardrop fill: the derived board said `(legacy_teardrops no)`, so kicad-cli's
/// `TEARDROP_MANAGER::UpdateTeardrops( .., forceFullUpdate )` deleted every teardrop and rebuilt only those of pads with `(teardrops .. (enabled yes))`, and wrote the
/// ones it rebuilt with no uuid, which matched no zone. Now the board says `yes` while it has teardrops (they are the design's zones), kicad-cli fills them as zones,
/// and a teardrop that comes back without a uuid is found by its layer and outline. Every teardrop of `issue14549` (43, a file of 20221018) and of
/// `teardrop_issue_JPC2` (pads with enabled teardrops) has a fill after the refill, with the area it had when imported, and the F.Cu Gerber keeps its copper.
#[test]
fn refill_keeps_teardrop_fills() {
    if std::env::var_os("EDA_SLOW_TESTS").is_none() {
        eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
        return;
    }
    let Some(cli) = find_cli() else {
        eprintln!("kicad-cli not found; skipping");
        return;
    };
    let root = std::env::var_os("PARITY_TMP").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("eda_refill_teardrops"));
    for rel in ["issue14549.kicad_pcb", "teardrop_issue_JPC2.kicad_pcb"] {
        let text = std::fs::read_to_string(qa_boards_dir().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let dir = root.join(rel.replace('.', "_"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".kicad")).unwrap();
        let (mut design, model, notes) = import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import {rel}: {e:?}"));
        design.routing.as_mut().expect("a routing section").assign_missing_ids();
        let imported = zone_fills::KicadFills::new("import".into(), String::new(), 0, entries_of_imported_file(&design, notes.zone_fills.clone()));
        let teardrops: Vec<&eda_model::ir::Zone> = design.routing.as_ref().unwrap().zones.iter().filter(|z| z.teardrop).collect();
        assert!(!teardrops.is_empty(), "{rel} has teardrops");
        let area = |fills: &zone_fills::KicadFills, z: &eda_model::ir::Zone| fills.current(z).map_or(0.0, |e| e.layers.values().flatten().map(|r| ring_area_mm2(&r.pts)).sum::<f64>());
        assert!(teardrops.iter().all(|z| area(&imported, z) > 0.0), "{rel}: every teardrop has the fill the file saved");

        let refilled = refill_zones(&design, &model, &dir.join(".kicad")).unwrap_or_else(|e| panic!("{rel}: {e:?}"));
        let unfilled: Vec<&str> = teardrops.iter().filter(|z| area(&refilled, z) <= 0.0).map(|z| z.id.as_str()).collect();
        assert!(unfilled.is_empty(), "{rel}: {} of {} teardrops lost their fill: {unfilled:?}", unfilled.len(), teardrops.len());
        let (before, after): (f64, f64) = (teardrops.iter().map(|z| area(&imported, z)).sum(), teardrops.iter().map(|z| area(&refilled, z)).sum());
        assert!((after - before).abs() <= 0.02 * before, "{rel}: teardrop copper {before:.4} mm^2 as saved, {after:.4} after the refill");
        let (_, stale) = refilled.tally(&design);
        assert_eq!(stale, 0);

        // The plot keeps the teardrop copper: the F.Cu Gerber of the refilled board is the one of the board as saved, to a few percent (was 35% smaller).
        let g_before = front_copper_gerber_bytes(&cli, &design, &model, &imported, &dir, "imported");
        let g_after = front_copper_gerber_bytes(&cli, &design, &model, &refilled, &dir, "refilled");
        assert!(g_after as f64 >= 0.97 * g_before as f64, "{rel}: F.Cu Gerber {g_before} bytes as saved, {g_after} after the refill");
    }

    // A board kicad-cli itself saved after rebuilding teardrops (JPC2's pads say `(enabled yes)`): its teardrops have no uuid, and the import still files their fills.
    let original = root.join("jpc2_saved.kicad_pcb");
    std::fs::write(&original, std::fs::read_to_string(qa_boards_dir().join("teardrop_issue_JPC2.kicad_pcb")).unwrap()).unwrap();
    let out = cli_command(&cli, None).args(["pcb", "drc", "--refill-zones", "--save-board", "--format", "report", "--output"]).arg(root.join("jpc2_saved.report")).arg(&original).output().expect("run kicad-cli");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let saved = std::fs::read_to_string(&original).unwrap();
    let (mut design, _model, notes) = import_kicad_pcb(&saved).expect("import");
    design.routing.as_mut().unwrap().assign_missing_ids();
    let teardrops: Vec<&eda_model::ir::Zone> = design.routing.as_ref().unwrap().zones.iter().filter(|z| z.teardrop).collect();
    assert!(!teardrops.is_empty() && teardrops.iter().all(|z| z.uuid.is_empty()), "kicad-cli wrote its rebuilt teardrops without a uuid");
    let fills = zone_fills::KicadFills::new("import".into(), String::new(), 0, entries_of_imported_file(&design, notes.zone_fills));
    assert!(teardrops.iter().all(|z| fills.current(z).is_some()), "every uuid-less teardrop is found by its outline");
}
