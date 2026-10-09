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
        let out = std::process::Command::new(eda_kicad_engine::find_cli().expect("kicad-cli"))
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
                (d["items"][0].as_str().unwrap().to_string(), d["items"][1].as_str().unwrap().to_string(), d["gap_um"].as_f64().unwrap())
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
        for tip_y in [6_300, 5_900, 5_700, 5_600, 5_500, 5_400, 5_000, 4_700] {
            let (_, design, model) = board::load(&dir).unwrap();
            let mut router = eda_pns::router::Router::new(&design, &model);
            router.settings.mode = eda_pns::settings::Mode::Shove;
            router.start(eda_model::ir::Point { x: 9_875, y: 7_200 }, "F.Cu", 200).expect("D8.1 starts a route");
            let at = eda_model::ir::Point { x: 9_875, y: tip_y };
            let Some(commit) = router.finish(at) else {
                eprintln!("tip {tip_y}: the router refuses it");
                continue;
            };
            pushed += commit.remove_track_ids.len().min(1);
            let cmd = eda_ops::Cmd::CommitRoute { remove_track_ids: commit.remove_track_ids, remove_via_ids: commit.remove_via_ids, tracks: commit.tracks, vias: commit.vias };
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
}
