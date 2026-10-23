//! The `routing_clearance` gate against kicad-cli (slow tier: `EDA_SLOW_TESTS=1`, and kicad-cli installed).
//!
//! The gate (`eda_gates::check_routing`) is the studio's in-process judge of copper clearance: it runs on every edit, so a
//! `--strict` move or a shove is refused on its word. kicad-cli's `clearance` check is the engine DRC is decided by. The two
//! must not disagree in one direction: **a pair the gate fails must be a pair kicad-cli's DRC reports** on the same
//! geometry. (The other direction is allowed: kicad-cli sees holes, zone fills, per-class and custom rules the gate does not.)
//! The gate used to measure a pad by its bounding rectangle, so a track past a round pad's corner failed it with the full
//! clearance of copper to spare, and a refused shove had no violation in kicad-cli (six such pairs on `pic_programmer`,
//! three TO-92 transistors whose round pads sit a rectangle's width apart).
//!
//! Boards: the user's `work/mcu30` (a scratch copy, after the one undo its old schematic drag needs; skipped when the
//! directory is missing -- `EDA_MCU30_DIR` overrides it) and two boards of KiCad's own router corpus (`KICAD_QA_DATA`,
//! default `~/ws/kicad-src-8303b2ad/qa/data`; skipped when absent), imported the way the studio imports a `.kicad_pcb`.
//!
//! kicad-cli runs with `--all-track-errors`: by default it reports only the first violation of each track
//! (`DRC_ENGINE::GetReportAllTrackErrors`), so a pair it does not list may still be one it stopped looking for.
//!
//! The fourth is the differential pair's: pairs placed by the pair router (`eda_pns::dp_placer`) on KiCad's own pair board leave
//! kicad-cli no new `clearance` and no `diff_pair_gap_out_of_range` against a `diff_pair_gap` rule of the pair's own gap.
//!
//! The fifth is the arcs': on the two boards of KiCad's corpus that have arc tracks (`issue12609`, `issue8407`) a route across an arc track, a route that ends in the middle
//! of one (which splits it) and an arc dragged (`DM_ARC`) leave kicad-cli no new copper violation, and every arc comes back out as an `(arc ..)` of the board file.
//!
//! The third test is the board outline's: shoves toward the top edge of `mcu30` -- committed with the strict gates off, since
//! `routing_pass_through_pad` refuses some of them for a reason that is not the edge's -- leave kicad-cli no
//! `copper_edge_clearance` violation. Before the outline was a router obstacle they did (a shoved track ended 300 um from it).

#[cfg(test)]
mod tests {
    use crate::board;
    use eda_model::ir::Design;
    use eda_model::{CheckStatus, ConstraintModel};
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// A track's report ids carry a `#<segment>` suffix once a track has more than one segment (`eda_kicad::pcb`'s uuid map).
    fn item_of(id: &str) -> String {
        id.split('#').next().unwrap_or(id).to_string()
    }

    fn pair(a: &str, b: &str) -> (String, String) {
        (a.min(b).to_string(), a.max(b).to_string())
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap().flatten() {
            let (src, dst) = (entry.path(), to.join(entry.file_name()));
            if src.is_dir() {
                copy_dir(&src, &dst);
            } else {
                std::fs::copy(&src, &dst).unwrap();
            }
        }
    }

    /// What the two judges said about one board.
    struct Verdict {
        label: String,
        /// `routing_clearance` failures: the two IR ids and the gap.
        gate: Vec<(String, String, f64)>,
        /// Every unordered pair of IR ids that appears together in one copper violation of kicad-cli's.
        kicad: BTreeSet<(String, String)>,
        /// kicad-cli's copper violations (clearance, short, crossing, hole clearance).
        kicad_violations: usize,
    }

    impl Verdict {
        /// The gate's failures kicad-cli does not report.
        fn unconfirmed(&self) -> Vec<&(String, String, f64)> {
            self.gate.iter().filter(|(a, b, _)| !self.kicad.contains(&pair(a, b))).collect()
        }

        fn report(&self) {
            eprintln!("{}: the gate fails {} pair(s); kicad-cli reports {} copper violation(s), {} of the gate's pairs are not among them", self.label, self.gate.len(), self.kicad_violations, self.unconfirmed().len());
            for (a, b, gap) in self.unconfirmed() {
                eprintln!("    gate only: {a} / {b}  gap {gap:.1} um");
            }
        }
    }

    /// kicad-cli's copper violations on the design as the studio would hand it over, as the IR ids of the items each names.
    fn kicad_violations(design: &Design, model: &ConstraintModel, work: &Path) -> Vec<Vec<String>> {
        // The engine's own run writes `board.kicad_pcb` and its project into `work`; the second run below reads them.
        eda_kicad_engine::drc(design, model, work, false).unwrap_or_else(|e| panic!("kicad-cli: {e:?}"));
        let date = eda_kicad_engine::today();
        let (_, map) = eda_kicad::export_kicad_pcb_mapped(design, model, &eda_kicad::ExportMeta { date: &date, title: "board" }).expect("export");
        let report = work.join("drc_all.json");
        let _ = std::fs::remove_file(&report);
        let out = eda_kicad_engine::cli_command(&eda_kicad_engine::find_cli().expect("kicad-cli"), None)
            .args(["pcb", "drc", "--format", "json", "--severity-all", "--all-track-errors", "--units", "mm", "-o"])
            .arg(&report)
            .arg(work.join("board.kicad_pcb"))
            .output()
            .expect("kicad-cli runs");
        let text = std::fs::read_to_string(&report).unwrap_or_else(|_| panic!("no report: {}", String::from_utf8_lossy(&out.stderr)));
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json["violations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| matches!(v["type"].as_str(), Some("clearance" | "shorting_items" | "tracks_crossing" | "hole_clearance")))
            .map(|v| v["items"].as_array().unwrap().iter().filter_map(|i| map.get(i["uuid"].as_str().unwrap_or("")).map(|id| item_of(id))).collect())
            .collect()
    }

    fn judge(label: &str, design: &Design, model: &ConstraintModel, work: &Path) -> Verdict {
        let gate: Vec<(String, String, f64)> = eda_gates::check_routing(design, model)
            .into_iter()
            .filter(|c| c.check == "routing_clearance" && c.status == CheckStatus::Fail)
            .map(|c| {
                let d = c.detail.expect("a routing_clearance failure names its items");
                (d["items"][0].as_str().unwrap().to_string(), d["items"][1].as_str().unwrap().to_string(), d["gap_nm"].as_f64().unwrap())
            })
            .collect();
        let violations = kicad_violations(design, model, work);
        let mut kicad = BTreeSet::new();
        for ids in &violations {
            for (i, a) in ids.iter().enumerate() {
                for b in &ids[i + 1..] {
                    kicad.insert(pair(a, b));
                }
            }
        }
        Verdict { label: label.into(), gate, kicad, kicad_violations: violations.len() }
    }

    fn kicad_available() -> bool {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() {
            eprintln!("skipped: slow test; set EDA_SLOW_TESTS=1 to run it");
            return false;
        }
        if eda_kicad_engine::find_cli().is_none() {
            eprintln!("kicad-cli not found; skipping");
            return false;
        }
        true
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eda_cli_gate_vs_kicad_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A board of KiCad's router corpus, imported with its ids assigned.
    fn qa_board(name: &str) -> Option<(Design, ConstraintModel)> {
        let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data")).join("pcbnew/pns_regressions/boards");
        let text = std::fs::read_to_string(root.join(format!("{name}.kicad_pcb"))).ok()?;
        let (mut design, model, _) = eda_kicad::import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import {name}: {e:?}"));
        design.assign_missing_ids();
        Some((design, model))
    }

    /// The counts of the violation kinds a pair can cause in kicad-cli's report for `design`.
    fn pair_counts(design: &Design, model: &ConstraintModel, work: &Path) -> std::collections::BTreeMap<String, usize> {
        let report = eda_kicad_engine::drc(design, model, work, false).unwrap_or_else(|e| panic!("kicad-cli: {e:?}"));
        let mut out = std::collections::BTreeMap::new();
        for v in report.violations.iter().filter(|v| !v.excluded) {
            if matches!(v.kind.as_str(), "clearance" | "shorting_items" | "tracks_crossing" | "hole_clearance" | "copper_edge_clearance" | "diff_pair_gap_out_of_range" | "track_width") {
                *out.entry(v.kind.clone()).or_insert(0) += 1;
            }
        }
        out
    }

    /// The design after a finished route's commit, as `Cmd::CommitRoute` applies it.
    fn apply_commit(design: &Design, commit: &eda_pns::router::RouteCommit) -> Design {
        let mut d = design.clone();
        let rt = d.routing.as_mut().unwrap();
        rt.tracks.retain(|t| !commit.remove_track_ids.contains(&t.id));
        rt.vias.retain(|v| !commit.remove_via_ids.contains(&v.id));
        rt.tracks.extend(commit.tracks.iter().cloned());
        rt.vias.extend(commit.vias.iter().cloned());
        d.assign_missing_ids();
        d
    }

    /// `dp_test.kicad_pcb` (a connector and a micro-BGA with a D+/D- pair between them), the pair's net class a KiCad project would give it
    /// (0.2 mm tracks, 0.25 mm gap), a `diff_pair_gap` rule that asks for that gap and no less, and three GND tracks across the open board.
    fn pair_board() -> Option<(Design, ConstraintModel)> {
        let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data"));
        let text = std::fs::read_to_string(root.join("pcbnew/pns_regressions/boards/dp_test.kicad_pcb")).ok()?;
        let (mut design, mut model, _) = eda_kicad::import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import: {e:?}"));
        model.board.net_classes.push(eda_model::NetClass { name: "usb".into(), nets: vec!["/D+".into(), "/D-".into()], track_width: None, clearance: None, via_diameter: None, via_drill: None, microvia_diameter: None, microvia_drill: None, diff_pair_width: Some(200_000), diff_pair_gap: Some(250_000), diff_pair_via_gap: Some(250_000), priority: 0 });
        // coupled stretches of the pair may not come closer than 0.24 mm (the pair's gap, less the rounding of a micrometre grid)
        model.board.custom_rules_text = Some("(version 1)\n(rule \"pair gap\"\n  (condition \"A.inDiffPair('/D')\")\n  (constraint diff_pair_gap (min 0.24mm)))\n".into());
        let p = |x, y| eda_model::ir::Point { x, y };
        let rt = design.routing.as_mut().unwrap();
        for (id, pts) in [("gnd_east", vec![p(128_000_000, 90_000_000), p(128_000_000, 93_000_000), p(130_000_000, 95_000_000), p(130_000_000, 100_000_000)]), ("gnd_north", vec![p(109_000_000, 92_500_000), p(114_000_000, 91_500_000), p(119_000_000, 91_500_000), p(121_000_000, 89_500_000)])] {
            rt.tracks.push(eda_model::ir::Track { id: id.into(), net: "GND".into(), pins: vec![], layer: "F.Cu".into(), width: 200_000, pts, arc_mid_offset: None });
        }
        design.assign_missing_ids();
        Some((design, model))
    }

    /// A pair placed by the pair router -- from the connector's pads into the open board, walked round or pushing the GND tracks in its way,
    /// and from the neck-down stubs' side meeting them -- is clean for kicad-cli: no new `clearance` (or any other copper violation), and no
    /// `diff_pair_gap_out_of_range` against a rule that asks for the pair's own gap. (Slow tier: four kicad-cli runs.)
    #[test]
    fn a_placed_pair_passes_kicad_clis_clearance_and_diff_pair_gap_checks() {
        if !kicad_available() {
            return;
        }
        let Some((design, model)) = pair_board() else {
            eprintln!("KiCad's QA corpus not found; skipping");
            return;
        };
        let dir = scratch("pair_drc");
        let baseline = pair_counts(&design, &model, &dir);
        eprintln!("the board before the pair: {baseline:?}");
        let mut placed = 0;
        // (the pad or track end the pair starts on, the cursor it goes to, the mode)
        let p = |x, y| eda_model::ir::Point { x, y };
        let runs = [
            (p(124_700_000, 95_900_000), p(135_000_000, 95_265_000), eda_pns::settings::Mode::Walkaround),
            (p(124_700_000, 95_900_000), p(135_000_000, 99_000_000), eda_pns::settings::Mode::Shove),
            (p(124_700_000, 95_900_000), p(133_000_000, 91_000_000), eda_pns::settings::Mode::Shove),
            (p(118_950_000, 95_075_000), p(118_050_000, 95_300_000), eda_pns::settings::Mode::Walkaround),
        ];
        for (start, at, mode) in runs {
            let mut router = eda_pns::router::Router::new(&design, &model);
            router.settings.mode = mode;
            router.start_diff_pair(start, "F.Cu").unwrap_or_else(|e| panic!("{mode:?} from {start:?}: {e}"));
            let pv = router.diff_pair_preview(at).expect("a pair is running");
            assert!(!pv.colliding, "{mode:?} from {start:?} to {at:?}: the pair does not fit: {pv:?}");
            let shoved = !pv.displaced_lines.is_empty();
            let commit = router.finish_diff_pair(at).expect("a pair that fits commits");
            let after = apply_commit(&design, &commit);
            let counts = pair_counts(&after, &model, &dir);
            eprintln!("{mode:?} {start:?} -> {at:?} ({} tracks, {} shoved): {counts:?}", commit.tracks.len(), commit.remove_track_ids.len());
            for kind in ["clearance", "shorting_items", "tracks_crossing", "hole_clearance", "copper_edge_clearance", "diff_pair_gap_out_of_range", "track_width"] {
                let (was, now) = (baseline.get(kind).copied().unwrap_or(0), counts.get(kind).copied().unwrap_or(0));
                assert!(now <= was, "{mode:?} {start:?} -> {at:?}: kicad-cli reports {now} {kind} (the board had {was}) after the pair (shoved: {shoved})");
            }
            placed += 1;
        }
        assert_eq!(placed, runs.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_gate_never_fails_a_pair_kicad_cli_reports_clean_on_mcu30() {
        if !kicad_available() {
            return;
        }
        // `work/` is the user's, and not in a git worktree: look beside this checkout, then at the main one.
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let candidates = [std::env::var_os("EDA_MCU30_DIR").map(PathBuf::from), Some(manifest.join("../../work/mcu30")), Some(manifest.join("../../../../../work/mcu30"))];
        let Some(src) = candidates.into_iter().flatten().find(|p| p.join("design.json").exists()) else {
            eprintln!("work/mcu30 not found (EDA_MCU30_DIR); skipping");
            return;
        };
        let dir = scratch("mcu30");
        copy_dir(&src, &dir);
        // The copy of the board in `work/` carries the nets of a schematic drag (`NET_nn`): one undo takes them back.
        let (_, design, _) = board::load(&dir).unwrap();
        if design.nets.as_ref().is_some_and(|n| n.iter().any(|n| n.name.starts_with("NET_"))) {
            board::undo(&dir, "test", None).expect("one undo");
        }
        let (_, design, model) = board::load(&dir).unwrap();
        let v = judge("mcu30", &design, &model, &dir.join(".kicad"));
        v.report();
        assert!(v.unconfirmed().is_empty(), "{}: the gate fails pairs kicad-cli reports clean: {:?}", v.label, v.unconfirmed());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_gate_never_fails_a_pair_kicad_cli_reports_clean_on_two_qa_boards() {
        if !kicad_available() {
            return;
        }
        let mut bad = Vec::new();
        // `pic_programmer`: three TO-92 transistors with round pads a pad-rectangle apart (the old gate failed six pairs of
        // them); `backspace1`: SMD parts, two real violations that kicad-cli does report.
        for name in ["pic_programmer", "backspace1"] {
            let Some((design, model)) = qa_board(name) else {
                eprintln!("{name}: KiCad's QA corpus not found; skipping");
                continue;
            };
            let dir = scratch(name);
            let v = judge(name, &design, &model, &dir);
            v.report();
            bad.extend(v.unconfirmed().into_iter().map(|(a, b, gap)| format!("{name}: {a} / {b} (gap {gap:.1} um)")));
            let _ = std::fs::remove_dir_all(&dir);
        }
        assert!(bad.is_empty(), "the gate fails pairs kicad-cli reports clean: {bad:?}");
    }

    /// `copper_edge_clearance` violations and the other copper kinds in kicad-cli's report for the board in `dir`.
    fn edge_and_copper_counts(dir: &Path) -> (u64, u64) {
        let report = crate::kicad_engine::drc(dir, false).expect("kicad-cli ran");
        let count = |kind: &str| report["counts"][kind].as_u64().unwrap_or(0);
        (count("copper_edge_clearance"), count("clearance") + count("shorting_items") + count("tracks_crossing") + count("hole_clearance"))
    }

    #[test]
    fn a_shove_toward_the_board_edge_leaves_kicad_cli_no_copper_edge_clearance_violation_on_mcu30() {
        if !kicad_available() {
            return;
        }
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let candidates = [std::env::var_os("EDA_MCU30_DIR").map(PathBuf::from), Some(manifest.join("../../work/mcu30")), Some(manifest.join("../../../../../work/mcu30"))];
        let Some(src) = candidates.into_iter().flatten().find(|p| p.join("design.json").exists()) else {
            eprintln!("work/mcu30 not found (EDA_MCU30_DIR); skipping");
            return;
        };
        let dir = scratch("mcu30_shove");
        copy_dir(&src, &dir);
        let (_, design, _) = board::load(&dir).unwrap();
        if design.nets.as_ref().is_some_and(|n| n.iter().any(|n| n.name.starts_with("NET_"))) {
            board::undo(&dir, "test", None).expect("one undo");
        }
        let baseline = edge_and_copper_counts(&dir);
        eprintln!("mcu30: {} copper_edge_clearance, {} copper clearance violations before", baseline.0, baseline.1);

        // LED8 leaves D8.1 at (9875, 7200) straight toward the top edge (y = 4544), pushing the GND track along y = 5619 ahead of it.
        let mut pushed = 0;
        for tip_y in [6_300_000, 5_900_000, 5_700_000, 5_600_000, 5_500_000, 5_400_000, 5_000_000, 4_700_000] {
            let (_, design, model) = board::load(&dir).unwrap();
            let mut router = eda_pns::router::Router::new(&design, &model);
            router.settings.mode = eda_pns::settings::Mode::Shove;
            router.start(eda_model::ir::Point { x: 9_875_000, y: 7_200_000 }, "F.Cu", 200_000).expect("D8.1 starts a route");
            let at = eda_model::ir::Point { x: 9_875_000, y: tip_y };
            let Some(commit) = router.finish(at) else {
                eprintln!("tip {tip_y}: the router refuses it");
                continue;
            };
            pushed += commit.remove_track_ids.len().min(1);
            let cmd = eda_ops::Cmd::CommitRoute { remove_track_ids: commit.remove_track_ids, remove_via_ids: commit.remove_via_ids, tracks: commit.tracks, vias: commit.vias, lock_tracks: commit.lock_tracks, via_kinds: commit.via_kinds };
            board::step(&dir, cmd, false, "test").expect("the commit applies");
            let after = edge_and_copper_counts(&dir);
            eprintln!("tip {tip_y}: {} copper_edge_clearance, {} copper clearance violations after", after.0, after.1);
            assert!(after.0 <= baseline.0, "a shove to y = {tip_y} leaves {} copper_edge_clearance violation(s) (there were {})", after.0, baseline.0);
            assert!(after.1 <= baseline.1, "a shove to y = {tip_y} leaves {} copper clearance violation(s) (there were {})", after.1, baseline.1);
            board::undo(&dir, "test", None).expect("undo the commit");
        }
        assert!(pushed > 0, "none of the routes pushed a track: the test is not testing a shove");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One of KiCad's own boards with arc tracks (`pcbnew/issue12609.kicad_pcb`: 47 of them, `issue8407.kicad_pcb`: 40), imported the way the studio imports a `.kicad_pcb`.
    fn arc_board(name: &str) -> Option<(Design, ConstraintModel)> {
        let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data")).join("pcbnew");
        let text = std::fs::read_to_string(root.join(format!("{name}.kicad_pcb"))).ok()?;
        let (mut design, model, _) = eda_kicad::import_kicad_pcb(&text).unwrap_or_else(|e| panic!("import {name}: {e:?}"));
        design.assign_missing_ids();
        Some((design, model))
    }

    /// The `(arc ..)` tracks of the board file `design` is written as (a track's own line, one tab in: not a polygon's or a footprint's).
    fn arcs_in_file(design: &Design, model: &ConstraintModel) -> usize {
        let text = eda_kicad::export_kicad_pcb(design, model, &eda_kicad::ExportMeta { date: "2026-01-01", title: "t" }).expect("the board file");
        text.lines().filter(|l| l.starts_with("\t(arc (start ")).count()
    }

    /// kicad-cli's copper violations of `design` with `--all-track-errors` -- by default it reports only the first violation of each track, and which one is first moves with the
    /// geometry, so the counts of a board with violations already would wander -- as (kind, the description and the items each names, in order).
    fn all_track_report(design: &Design, model: &ConstraintModel, work: &Path, kinds: &[&str]) -> Vec<(String, String)> {
        // the engine's own run writes `board.kicad_pcb` and its project into `work`; the run below reads them
        eda_kicad_engine::drc(design, model, work, false).unwrap_or_else(|e| panic!("kicad-cli: {e:?}"));
        let report = work.join("drc_all_tracks.json");
        let _ = std::fs::remove_file(&report);
        let out = eda_kicad_engine::cli_command(&eda_kicad_engine::find_cli().expect("kicad-cli"), Some(&work.join("board.kicad_pro")))
            .args(["pcb", "drc", "--format", "json", "--severity-all", "--all-track-errors", "--units", "mm", "-o"])
            .arg(&report)
            .arg(work.join("board.kicad_pcb"))
            .output()
            .expect("kicad-cli runs");
        let text = std::fs::read_to_string(&report).unwrap_or_else(|_| panic!("no report: {}", String::from_utf8_lossy(&out.stderr)));
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json["violations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["severity"].as_str() != Some("exclusion") && kinds.contains(&v["type"].as_str().unwrap_or("")))
            .map(|v| {
                let mut items: Vec<String> = v["items"].as_array().unwrap().iter().map(|i| i["description"].as_str().unwrap_or("").to_string()).collect();
                items.sort();
                (v["type"].as_str().unwrap().to_string(), format!("{} {}", v["description"].as_str().unwrap_or(""), items.join(" / ")))
            })
            .collect()
    }

    fn counts_of(report: &[(String, String)]) -> std::collections::BTreeMap<String, usize> {
        let mut out = std::collections::BTreeMap::new();
        for (kind, _) in report {
            *out.entry(kind.clone()).or_insert(0) += 1;
        }
        out
    }

    /// Arcs against kicad-cli's DRC (slow tier), on KiCad's own boards with arc tracks. Three kinds of edit -- a route that crosses an arc track (Shove mode pushes it
    /// away), a route that ends in the middle of a track of its net that is an arc (which splits the arc there: `SplitAdjacentArcs`), and an arc track dragged (`DM_ARC`: the
    /// curvature follows the cursor, the tangent neighbours follow the arc) -- each add no `clearance`, `track_width` or other copper violation to kicad-cli's report of the
    /// board, and every arc the commit leaves is an `(arc ..)` of the board file, not a polyline of chords.
    #[test]
    fn routes_across_and_onto_an_arc_track_and_a_dragged_arc_add_no_violation_to_kicad_clis_drc() {
        use eda_pns::dragger::DragKind;
        use eda_pns::item::Item;
        use eda_pns::router::Router;
        use eda_pns::settings::Mode;
        if !kicad_available() {
            return;
        }
        let kinds = ["clearance", "shorting_items", "tracks_crossing", "hole_clearance", "copper_edge_clearance", "track_width"];
        let dist = |a: eda_model::ir::Point, b: eda_model::ir::Point| (((a.x - b.x) as f64).powi(2) + ((a.y - b.y) as f64).powi(2)).sqrt();
        let mut judged = 0;
        for name in ["issue12609", "issue8407"] {
            let Some((design, model)) = arc_board(name) else {
                eprintln!("{name}: KiCad's QA corpus not found; skipping");
                continue;
            };
            let dir = scratch(name);
            let before = all_track_report(&design, &model, &dir, &kinds);
            let baseline = counts_of(&before);
            let arcs_before = design.routing.as_ref().unwrap().tracks.iter().filter(|t| t.arc().is_some()).count();
            let (node, layers) = eda_pns::from_ir::build_node(&design, &model);
            let mut arcs: Vec<(String, eda_pns::item::Net, i32, eda_pns::arcs::ArcPts)> = node
                .iter()
                .filter_map(|(_, it)| match it {
                    Item::Arc(a) => a.source_track.clone().map(|t| (t, a.net.clone(), a.layer, a.arc)),
                    _ => None,
                })
                .collect();
            arcs.sort_by(|a, b| a.0.cmp(&b.0));
            assert_eq!(arcs.len(), arcs_before, "{name}: every arc track is an arc item of the world");
            assert!(arcs.len() > 20, "{name} has arc tracks: {}", arcs.len());
            let pads: Vec<(eda_model::ir::Point, eda_pns::item::Net, eda_pns::layer::LayerRange)> = node
                .iter()
                .filter_map(|(_, it)| match it {
                    Item::Solid(s) if !s.edge && s.net.is_some() => Some((s.pos, s.net.clone(), s.layers)),
                    _ => None,
                })
                .collect();
            let judge = |label: &str, commit: &eda_pns::router::RouteCommit| {
                let after = apply_commit(&design, commit);
                let now = all_track_report(&after, &model, &dir, &kinds);
                let counts = counts_of(&now);
                let arc_tracks = after.routing.as_ref().unwrap().tracks.iter().filter(|t| t.arc().is_some()).count();
                eprintln!("{name} {label}: {} tracks put, {} taken, {} of the new ones arcs; {arc_tracks} arc tracks now (were {arcs_before}): {counts:?} (before {baseline:?})", commit.tracks.len(), commit.remove_track_ids.len(), commit.tracks.iter().filter(|t| t.arc().is_some()).count());
                if counts != baseline {
                    eprintln!("    new: {:?}", now.iter().filter(|v| !before.contains(v)).collect::<Vec<_>>());
                    eprintln!("    gone: {:?}", before.iter().filter(|v| !now.contains(v)).collect::<Vec<_>>());
                }
                for kind in kinds {
                    let (was, got) = (baseline.get(kind).copied().unwrap_or(0), counts.get(kind).copied().unwrap_or(0));
                    assert!(got <= was, "{name} {label}: kicad-cli reports {got} {kind} (the board had {was})");
                }
                assert_eq!(arcs_in_file(&after, &model), arc_tracks, "{name} {label}: every arc track is written as an (arc ..)");
            };
            // how many of each kind were judged, how many tries were spent on them
            let (mut across, mut onto, mut dragged, mut pushed_arcs, mut split_arcs) = (0, 0, 0, 0, 0);
            let mut tries = [0usize; 3];
            for (track, net, layer, arc) in &arcs {
                if across >= 2000 && onto >= 2000 && dragged >= 2000 {
                    break;
                }
                let layer_name = layers.name_of(*layer).to_string();
                // across: from the nearest pad of another net, 2 to 9 mm from the arc's middle, straight at the arc and 0.15 mm past its middle, where the head's end sits on the
                // arc's stroke: Shove mode has to push the arc away (a try in which it does not, the router walked or the arc was not in the way, is not judged)
                if across < 2000 && tries[0] < 30_000 {
                    let from = pads.iter().filter(|(p, n, l)| n != net && l.overlaps_layer(*layer) && (2_000.0..9_000.0).contains(&dist(*p, arc.mid))).min_by(|a, b| dist(a.0, arc.mid).total_cmp(&dist(b.0, arc.mid)));
                    if let Some((pad, _, _)) = from {
                        tries[0] += 1;
                        let d = dist(*pad, arc.mid);
                        let target = eda_model::ir::Point { x: arc.mid.x + (((arc.mid.x - pad.x) as f64 / d) * 150.0).round() as i64, y: arc.mid.y + (((arc.mid.y - pad.y) as f64 / d) * 150.0).round() as i64 };
                        let mut router = Router::new(&design, &model);
                        router.settings.mode = Mode::Shove;
                        if router.start(*pad, &layer_name, model.board.track_width).is_ok() && router.preview(target).is_some_and(|pv| !pv.colliding) {
                            if let Some(commit) = router.finish(target) {
                                if commit.remove_track_ids.contains(track) {
                                    pushed_arcs += 1;
                                    judge(&format!("across {track}"), &commit);
                                    across += 1;
                                    judged += 1;
                                }
                            }
                        }
                    }
                }
                // onto: from a pad of the arc's own net to the middle of the arc, which the route ends on (and so splits there)
                if onto < 2000 && tries[1] < 16_000 {
                    let from = pads.iter().filter(|(p, n, l)| n == net && l.overlaps_layer(*layer) && (1_500.0..12_000.0).contains(&dist(*p, arc.mid))).min_by(|a, b| dist(a.0, arc.mid).total_cmp(&dist(b.0, arc.mid)));
                    if let Some((pad, _, _)) = from {
                        tries[1] += 1;
                        let mut router = Router::new(&design, &model);
                        router.settings.mode = Mode::Shove;
                        if router.start(*pad, &layer_name, model.board.track_width).is_ok() && router.preview(arc.mid).is_some_and(|pv| !pv.colliding) {
                            if let Some(commit) = router.finish(arc.mid) {
                                let split = commit.remove_track_ids.contains(track);
                                split_arcs += usize::from(split && commit.tracks.iter().filter(|t| t.arc().is_some()).count() >= 2);
                                judge(&format!("onto {track} (split: {split})"), &commit);
                                onto += 1;
                                judged += 1;
                            }
                        }
                    }
                }
                // dragged: the middle of the arc taken hold of and moved 0.4 mm away from its centre
                if dragged < 2000 && tries[2] < 16_000 {
                    tries[2] += 1;
                    let (c, r) = (arc.center_f(), arc.radius());
                    let to = eda_model::ir::Point { x: arc.mid.x + (((arc.mid.x as f64 - c.0) / r) * 400.0).round() as i64, y: arc.mid.y + (((arc.mid.y as f64 - c.1) / r) * 400.0).round() as i64 };
                    let mut router = Router::new(&design, &model);
                    router.settings.mode = Mode::Shove;
                    if router.drag_start(arc.mid, &layer_name).is_ok() && matches!(router.drag_kind(), Some(DragKind::Arc)) && router.drag_preview(to).is_some_and(|pv| !pv.colliding) {
                        if let Some(commit) = router.drag_finish(to) {
                            judge(&format!("dragged {track}"), &commit);
                            dragged += 1;
                            judged += 1;
                        }
                    }
                }
            }
            eprintln!("{name}: {across} routes across (each pushed the arc: {pushed_arcs}), {onto} onto ({split_arcs} split it into two arcs), {dragged} arcs dragged; tries {tries:?}");
            assert!(across > 0 && onto > 0 && dragged > 0, "{name}: every kind of edit was judged at least once: {across}, {onto}, {dragged}");
            let _ = std::fs::remove_dir_all(&dir);
        }
        assert!(judged == 0 || judged >= 6000, "{judged} edits judged by kicad-cli");
    }

    /// Springback (`SHOVE::reduceSpringback` / `pushSpringback`, `FixRoute`'s locked frame, `UnfixRoute`'s rewind) against kicad-cli's DRC
    /// (slow tier): KiCad's own recorded session `issue22749-shove-weird-drag-track-end` replayed through the router, and routes on
    /// `pic_programmer` whose cursor pushes into the tracks, draws back, pushes again, clicks, goes on and Backspaces -- each commit,
    /// with every track the session's shoves left moved, adds no copper violation to kicad-cli's report of the board.
    #[test]
    fn springback_sessions_add_no_violation_to_kicad_clis_drc() {
        use eda_model::ir::Point;
        use eda_pns::item::Item;
        use eda_pns::router::Router;
        use eda_pns::settings::Mode;
        if !kicad_available() {
            return;
        }
        let kinds = ["clearance", "shorting_items", "tracks_crossing", "hole_clearance", "copper_edge_clearance", "track_width"];
        let Some((design, model)) = qa_board("pic_programmer") else {
            eprintln!("KiCad's QA corpus not found; skipping");
            return;
        };
        let dir = scratch("springback");
        let before = all_track_report(&design, &model, &dir, &kinds);
        let baseline = counts_of(&before);
        let judge = |label: &str, commit: &eda_pns::router::RouteCommit| {
            let after = apply_commit(&design, commit);
            let now = all_track_report(&after, &model, &dir, &kinds);
            let counts = counts_of(&now);
            eprintln!("pic_programmer {label}: {} tracks put, {} taken, {} vias moved: {counts:?} (before {baseline:?})", commit.tracks.len(), commit.remove_track_ids.len(), commit.remove_via_ids.len());
            if counts != baseline {
                eprintln!("    new: {:?}", now.iter().filter(|v| !before.contains(v)).collect::<Vec<_>>());
            }
            for kind in kinds {
                let (was, got) = (baseline.get(kind).copied().unwrap_or(0), counts.get(kind).copied().unwrap_or(0));
                assert!(got <= was, "{label}: kicad-cli reports {got} {kind} (the board had {was})");
            }
        };
        let mut judged = 0;

        // KiCad's recorded session: a route from a track end on B.Cu, pushing its neighbour round the head
        let root = std::env::var_os("KICAD_QA_DATA").map(PathBuf::from).filter(|p| p.exists()).unwrap_or_else(|| PathBuf::from("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data")).join("pcbnew/pns_regressions/issue22749-shove-weird-drag-track-end");
        if let Ok(text) = std::fs::read_to_string(root.join("pns.log")) {
            let log: serde_json::Value = serde_json::from_str(&text).unwrap();
            let pt = |v: &serde_json::Value| Point { x: (v["x"].as_f64().unwrap() / 1000.0).round() as i64, y: (v["y"].as_f64().unwrap() / 1000.0).round() as i64 };
            let events = log["events"].as_array().unwrap();
            let mut router = Router::new(&design, &model);
            router.settings.mode = Mode::Shove;
            let start = pt(&events[0]["position"]);
            let (node, _) = eda_pns::from_ir::build_node(&design, &model);
            let net = node.nearest_anchor(start, eda_pns::layer::LayerRange::single(1), 500_000, None).and_then(|(id, _)| node.get(id).and_then(|i| i.net().clone())).expect("a track end with a net");
            router.start(start, "B.Cu", model.board.width_of(&net)).expect("starts on the track end");
            for e in &events[1..] {
                router.preview(pt(&e["position"]));
            }
            let commit = router.finish(pt(&events.last().unwrap()["position"])).expect("the recorded route finishes");
            assert!(!commit.remove_track_ids.is_empty(), "the session pushed tracks");
            judge("issue22749 replayed", &commit);
            judged += 1;
        }

        // pushing into the tracks east of a pad, drawing back, pushing again, a click, a second leg, a Backspace and the leg again
        let (node, layers) = eda_pns::from_ir::build_node(&design, &model);
        let mut pads: Vec<(Point, String, i32)> = node
            .iter()
            .filter_map(|(_, it)| match it {
                Item::Solid(s) if !s.edge && s.net.is_some() && s.layers.overlaps_layer(0) => Some((s.pos, s.source.clone(), 0)),
                _ => None,
            })
            .collect();
        pads.sort_by(|a, b| a.1.cmp(&b.1));
        let mut sessions = 0;
        for (pad, name, layer) in &pads {
            if sessions >= 4000 {
                break;
            }
            let layer_name = layers.name_of(*layer).to_string();
            let at = |dx: i64, dy: i64| Point { x: pad.x + dx, y: pad.y + dy };
            let mut router = Router::new(&design, &model);
            router.settings.mode = Mode::Shove;
            router.settings.fix_all_segments = true;
            if router.start(*pad, &layer_name, model.board.track_width).is_err() {
                continue;
            }
            let mut pushed = false;
            for step in [600_000, 1_500_000, 2_500_000, 3_500_000, 2_000_000, 800_000, 2_000_000, 3_500_000, 4_500_000] {
                if let Some(pv) = router.preview(at(step, 0)) {
                    pushed |= !pv.displaced_lines.is_empty();
                }
            }
            if !pushed || !router.preview(at(4_500_000, 0)).is_some_and(|pv| !pv.colliding) {
                continue;
            }
            if !matches!(router.fix(at(4_500_000, 0)), Some(eda_pns::line_placer::FixOutcome::Fixed { real_end: false })) {
                continue;
            }
            for step in [500_000, 1_500_000, 2_500_000] {
                router.preview(at(4_500_000, -step));
            }
            router.undo_last_segment();
            for step in [600_000, 1_500_000, 2_500_000, 3_500_000, 4_500_000] {
                router.preview(at(4_500_000 + step / 2, step));
            }
            let end = at(6_750_000, 4_500_000);
            if !router.preview(end).is_some_and(|pv| !pv.colliding) {
                continue;
            }
            let Some(commit) = router.finish(end) else { continue };
            if commit.remove_track_ids.is_empty() {
                continue;
            }
            judge(&format!("session from {name}"), &commit);
            sessions += 1;
            judged += 1;
        }
        eprintln!("{judged} springback sessions judged ({sessions} on pic_programmer's pads)");
        assert!(judged >= 3000, "{judged} sessions judged by kicad-cli");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The Track Corner Mode (`CORNER_MODE`) against kicad-cli's DRC (slow tier): routes on `pic_programmer` in each of the four modes, in Shove and
    /// Walk around, from pads out into the board with a click and a second leg -- each commit adds no copper violation to kicad-cli's report (those with
    /// the board's text on copper included: four of them before the router saw copper text, `syncTextItem`), and in the two rounded modes the corners are `(arc ..)` tracks of the board file.
    #[test]
    fn routes_in_every_corner_mode_add_no_violation_to_kicad_clis_drc() {
        use eda_model::ir::Point;
        use eda_pns::direction45::CornerMode;
        use eda_pns::item::Item;
        use eda_pns::router::Router;
        use eda_pns::settings::Mode;
        if !kicad_available() {
            return;
        }
        let kinds = ["clearance", "shorting_items", "tracks_crossing", "hole_clearance", "copper_edge_clearance", "track_width"];
        let Some((design, model)) = qa_board("pic_programmer") else {
            eprintln!("KiCad's QA corpus not found; skipping");
            return;
        };
        let dir = scratch("corner_modes");
        // a board text on copper is an obstacle of the router (`syncTextItem`): what a route does to the text of `pic_programmer` counts too
        let before = all_track_report(&design, &model, &dir, &kinds);
        let baseline = counts_of(&before);
        let (node, _) = eda_pns::from_ir::build_node(&design, &model);
        let mut pads: Vec<(Point, String)> = node
            .iter()
            .filter_map(|(_, it)| match it {
                Item::Solid(s) if !s.edge && s.net.is_some() && s.layers.overlaps_layer(0) => Some((s.pos, s.source.clone())),
                _ => None,
            })
            .collect();
        pads.sort_by(|a, b| a.1.cmp(&b.1));
        let mut judged = 0;
        let mut arcs_written = 0;
        let mut pushed = 0;
        for (c, corner) in [CornerMode::Mitered45, CornerMode::Rounded45, CornerMode::Mitered90, CornerMode::Rounded90].into_iter().enumerate() {
            for (m, mode) in [Mode::Shove, Mode::Walkaround].into_iter().enumerate() {
                let mut done = 0;
                // each mode from pads of its own, round the board
                let offset = (c * 2 + m) * pads.len() / 8;
                for (pad, name) in pads.iter().cycle().skip(offset).take(pads.len()) {
                    if done >= 2000 {
                        break;
                    }
                    let mut router = Router::new(&design, &model);
                    router.settings.mode = mode;
                    router.settings.corner_mode = corner;
                    if router.start(*pad, "F.Cu", model.board.track_width).is_err() {
                        continue;
                    }
                    let (mid, end) = (Point { x: pad.x + 3_000_000, y: pad.y - 1_300_000 }, Point { x: pad.x + 4_300_000, y: pad.y - 4_500_000 });
                    for k in 1..=4 {
                        router.preview(Point { x: pad.x + 750_000 * k, y: pad.y - 325_000 * k });
                    }
                    if !router.preview(mid).is_some_and(|pv| !pv.colliding) || !matches!(router.fix(mid), Some(eda_pns::line_placer::FixOutcome::Fixed { real_end: false })) {
                        continue;
                    }
                    if !router.preview(end).is_some_and(|pv| !pv.colliding) {
                        continue;
                    }
                    let Some(commit) = router.finish(end) else { continue };
                    let arcs = commit.tracks.iter().filter(|t| t.arc().is_some() && t.net != "").count();
                    let after = apply_commit(&design, &commit);
                    let now = all_track_report(&after, &model, &dir, &kinds);
                    let counts = counts_of(&now);
                    eprintln!("pic_programmer {corner:?} {mode:?} from {name}: {} tracks put ({arcs} arcs), {} taken: {counts:?} (before {baseline:?})", commit.tracks.len(), commit.remove_track_ids.len());
                    if counts != baseline {
                        eprintln!("    new: {:?}", now.iter().filter(|v| !before.contains(v)).collect::<Vec<_>>());
                    }
                    for kind in kinds {
                        let (was, got) = (baseline.get(kind).copied().unwrap_or(0), counts.get(kind).copied().unwrap_or(0));
                        assert!(got <= was, "{corner:?} {mode:?} from {name}: kicad-cli reports {got} {kind} (the board had {was})");
                    }
                    if matches!(corner, CornerMode::Rounded45 | CornerMode::Rounded90) {
                        assert!(arcs > 0, "{corner:?} {mode:?} from {name}: the corners are arcs");
                        assert_eq!(arcs_in_file(&after, &model), arcs_in_file(&design, &model) + commit.tracks.iter().filter(|t| t.arc().is_some()).count(), "every arc is an (arc ..) of the board file");
                        arcs_written += arcs;
                    }
                    pushed += usize::from(!commit.remove_track_ids.is_empty());
                    done += 1;
                    judged += 1;
                }
            }
        }
        eprintln!("{judged} routes judged by kicad-cli, {arcs_written} arcs written, {pushed} of them pushed tracks");
        assert!(judged >= 12_000, "{judged} routes judged");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Virtual vias (`NODE::FixupVirtualVias`, `DRAGGER::checkVirtualVia`) against kicad-cli's DRC (slow tier): on `pic_programmer` and
    /// `backspace1`, the joints where the width changes, with no pad or via (KiCad's `FixupVirtualVias` gives a T of one width none), taken hold of at the end of one of their
    /// segments and dragged 0.3 mm in each mode -- every track there follows, no via is committed, and kicad-cli finds no new copper violation and no
    /// new dangling track (the junction stays one joint).
    #[test]
    fn dragging_a_junction_as_a_virtual_via_adds_no_violation_to_kicad_clis_drc() {
        use eda_model::ir::Point;
        use eda_pns::item::Item;
        use eda_pns::router::Router;
        use eda_pns::settings::Mode;
        if !kicad_available() {
            return;
        }
        let kinds = ["clearance", "shorting_items", "tracks_crossing", "hole_clearance", "copper_edge_clearance", "track_width", "track_dangling", "unconnected_items"];
        let mut judged = 0;
        for board in ["pic_programmer", "backspace1"] {
            let Some((design, model)) = qa_board(board) else {
                eprintln!("KiCad's QA corpus not found; skipping");
                return;
            };
            let dir = scratch(&format!("vvia_{board}"));
            let before = all_track_report(&design, &model, &dir, &kinds);
            let baseline = counts_of(&before);
            let (node, layers) = eda_pns::from_ir::build_node(&design, &model);
            // the junctions: segment ends whose joint has a virtual via
            let mut junctions: Vec<(Point, i32, Point)> = Vec::new();
            for (_, it) in node.iter() {
                let Item::Segment(s) = it else { continue };
                for (end, other) in [(s.a, s.b), (s.b, s.a)] {
                    let has = node.joint_at(end, &s.net).is_some_and(|j| j.links.iter().any(|&id| node.get(id).is_some_and(Item::is_virtual)));
                    if has && !junctions.iter().any(|(p, _, _)| *p == end) {
                        // the grab point: on the segment, a micrometre from the end
                        let len = (((other.x - end.x) as f64).hypot((other.y - end.y) as f64)).max(1.0);
                        let at = Point { x: end.x + ((other.x - end.x) as f64 / len).round() as i64, y: end.y + ((other.y - end.y) as f64 / len).round() as i64 };
                        junctions.push((end, s.layer, at));
                    }
                }
            }
            junctions.sort_by_key(|(p, _, _)| (p.x, p.y));
            eprintln!("{board}: {} junctions with a virtual via", junctions.len());
            let mut done = 0;
            for (k, (end, layer, at)) in junctions.iter().enumerate() {
                if done >= 6000 {
                    break;
                }
                let mode = [Mode::Shove, Mode::Walkaround, Mode::MarkObstacles][k % 3];
                let mut router = Router::new(&design, &model);
                router.settings.mode = mode;
                if router.drag_start(*at, layers.name_of(*layer)).is_err() || !router.drag_virtual_via() {
                    continue;
                }
                let to = Point { x: end.x + 300_000, y: end.y + 300_000 };
                if !router.drag_preview(to).is_some_and(|pv| !pv.colliding) {
                    continue;
                }
                let Some(commit) = router.drag_finish(to) else { continue };
                assert!(commit.vias.iter().all(|v| v.at != to), "{board}: no via is committed at the junction");
                let after = apply_commit(&design, &commit);
                let now = all_track_report(&after, &model, &dir, &kinds);
                let counts = counts_of(&now);
                eprintln!("{board} {mode:?} junction {end:?}: {} tracks put, {} taken: {counts:?} (before {baseline:?})", commit.tracks.len(), commit.remove_track_ids.len());
                if counts != baseline {
                    eprintln!("    new: {:?}", now.iter().filter(|v| !before.contains(v)).collect::<Vec<_>>());
                }
                for kind in kinds {
                    let (was, got) = (baseline.get(kind).copied().unwrap_or(0), counts.get(kind).copied().unwrap_or(0));
                    assert!(got <= was, "{board} {mode:?} junction {end:?}: kicad-cli reports {got} {kind} (the board had {was})");
                }
                assert!(commit.remove_track_ids.len() >= 3 || commit.tracks.len() >= 2, "{board}: the tracks at the junction moved: {commit:?}");
                done += 1;
                judged += 1;
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
        eprintln!("{judged} junction drags judged by kicad-cli");
        assert!(judged >= 6000, "{judged} junction drags judged");
    }

    /// The router's blind/buried via and microvia (`ROUTER_TOOL::handleLayerSwitch` with `VIA_ACTION_FLAGS::BLIND_VIA` / `MICROVIA`, the studio's
    /// `pcbnew.InteractiveRouter.PlaceBlindVia` / `PlaceMicroVia`) against kicad-cli (slow tier): on a 4-layer scratch board, SIG is routed from a stub on
    /// F.Cu through a blind via (F.Cu .. In1.Cu, the net class's via size) to a stub on In1.Cu, and GND from a stub on B.Cu through a microvia
    /// (In2.Cu .. B.Cu, the net class's microvia size) to a stub on In2.Cu, each through the route API and committed as one `Cmd::CommitRoute`. The
    /// board file has `(via blind ..)` and `(via micro ..)` on those layers, and kicad-cli's DRC finds nothing on the board but the stubs' outer ends,
    /// which lead nowhere.
    #[test]
    fn a_route_with_a_blind_via_and_one_with_a_microvia_are_clean_for_kicad_clis_drc_on_four_layers() {
        use eda_model::ir::{PlacementSection, Point, Provenance, RoutingSection, Track};
        use serde_json::json;
        if !kicad_available() {
            return;
        }
        let dir = scratch("typed_vias");
        let p = |x, y| Point { x, y };
        let mut model = ConstraintModel { nets: ["SIG", "GND"].iter().map(|n| eda_model::Net { name: n.to_string(), pins: vec![] }).collect(), ..ConstraintModel::default() };
        model.board.layers = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"].iter().map(|l| l.to_string()).collect();
        // a build-up stackup: prepreg under each outer layer, the core in the middle -- a microvia goes through one prepreg (kicad-cli's
        // `microvia_crosses_core`, a check newer than 8303b2ad, refuses one through a core, and KiCad's default stackup has a core under F.Cu and B.Cu)
        let mut stackup = eda_model::rules::default_stackup(4, 1_600_000);
        for l in stackup.layers.iter_mut().filter(|l| matches!(l.kind.as_deref(), Some("core" | "prepreg"))) {
            l.kind = Some(if l.name == "dielectric 2" { "core" } else { "prepreg" }.into());
        }
        model.stackup = Some(stackup);
        let intent_path = dir.join("intent.yaml");
        std::fs::write(&intent_path, serde_yaml::to_string(&model).unwrap()).unwrap();
        let stub = |id: &str, net: &str, layer: &str, a: Point, b: Point| Track { id: id.into(), net: net.into(), pins: vec![], layer: layer.into(), width: 250_000, pts: vec![a, b], arc_mid_offset: None };
        let tracks = vec![
            stub("sig_f", "SIG", "F.Cu", p(2_000_000, 6_000_000), p(4_000_000, 6_000_000)),
            stub("sig_in1", "SIG", "In1.Cu", p(16_000_000, 6_000_000), p(18_000_000, 6_000_000)),
            stub("gnd_b", "GND", "B.Cu", p(2_000_000, 14_000_000), p(4_000_000, 14_000_000)),
            stub("gnd_in2", "GND", "In2.Cu", p(16_000_000, 14_000_000), p(18_000_000, 14_000_000)),
        ];
        let design = Design {
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "t".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![p(0, 0), p(20_000_000, 0), p(20_000_000, 20_000_000), p(0, 20_000_000)], footprints: vec![], modules: vec![] }),
            routing: Some(RoutingSection { tracks, vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
            drawings: None,
        };
        board::save(&dir, &design).unwrap();
        let meta = board::Meta { intent: intent_path.display().to_string(), snap_nm: 100_000, spacing_nm: 300_000 };
        std::fs::write(dir.join("board.json"), serde_json::to_string_pretty(&meta).unwrap()).unwrap();
        let body = |v: serde_json::Value| v.to_string().into_bytes();
        // (from, layer, the via's type, its span, the layer the route goes on on, its size, the far stub)
        let routes = [
            (p(4_000_000, 6_000_000), "F.Cu", "blind", ["F.Cu", "In1.Cu"], "In1.Cu", (model.board.via_diameter, model.board.via_drill), p(16_000_000, 6_000_000)),
            (p(4_000_000, 14_000_000), "B.Cu", "micro", ["In2.Cu", "B.Cu"], "In2.Cu", (300_000, 100_000), p(16_000_000, 14_000_000)),
        ];
        for (from, layer, kind, span, to_layer, (diameter, drill), to) in routes {
            let cell: crate::route_api::RouteCell = std::sync::Mutex::new(None);
            let started = crate::route_api::start(&dir, &cell, &body(json!({ "x": from.x, "y": from.y, "layer": layer, "width": 250_000, "settings": { "mode": "walkaround" } })));
            assert_eq!(started["ok"], json!(true), "{started}");
            let armed = crate::route_api::via(&cell, &body(json!({ "enabled": true, "diameter": diameter, "drill": drill, "to_layer": to_layer, "via_type": kind, "span": span })));
            assert_eq!(armed["ok"], json!(true), "{armed}");
            let fixed = crate::route_api::fix(&cell, &body(json!({ "x": 9_000_000, "y": from.y })));
            assert_eq!(fixed["blocked"], json!(false), "{fixed}");
            let finished = crate::route_api::finish(&dir, &cell, &body(json!({ "x": to.x, "y": to.y })));
            assert_eq!(finished["ok"], json!(true), "{finished}");
        }
        let (_, design, model) = board::load(&dir).unwrap();
        let text = eda_kicad::export_kicad_pcb(&design, &model, &eda_kicad::ExportMeta { date: "2026-01-01", title: "t" }).expect("the board file");
        let vias: Vec<&str> = text.lines().map(str::trim_start).filter(|l| l.starts_with("(via ")).collect();
        eprintln!("{vias:#?}");
        assert_eq!(vias.len(), 2, "{vias:?}");
        assert!(vias.iter().any(|l| l.starts_with("(via blind") && l.contains("(layers \"F.Cu\" \"In1.Cu\")")), "{vias:?}");
        assert!(vias.iter().any(|l| l.starts_with("(via micro") && l.contains("(layers \"In2.Cu\" \"B.Cu\")") && l.contains("(size 0.3)") && l.contains("(drill 0.1)")), "{vias:?}");
        let work = dir.join("drc");
        std::fs::create_dir_all(&work).unwrap();
        let report = eda_kicad_engine::drc(&design, &model, &work, false).unwrap_or_else(|e| panic!("kicad-cli: {e:?}"));
        let found: Vec<(String, String)> = report.violations.iter().filter(|v| !v.excluded).map(|v| (v.kind.clone(), v.description.clone())).collect();
        eprintln!("kicad-cli: {found:?}");
        // the four stubs' outer ends lead nowhere (`track_dangling`); a route that did not reach its stub would leave two more, and the via one
        let dangling = found.iter().filter(|(k, _)| k == "track_dangling").count();
        assert_eq!(dangling, 4000, "only the stubs' outer ends dangle: {found:?}");
        let other: Vec<&(String, String)> = found.iter().filter(|(k, _)| k != "track_dangling").collect();
        assert!(other.is_empty(), "kicad-cli finds {other:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
