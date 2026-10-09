#!/usr/bin/env python3
"""The one command for the agentic-eda <-> KiCad parity harness.

    python3 tools/parity_report.py

What it does:
  1. Runs the two `#[ignore]`d oracle tests (connectivity, round-trip --
     see crates/*/tests/parity_*.rs), which each talk to
     `kicad-cli` and KiCad's own QA board corpus and write their raw
     findings to docs/parity/raw/*.json. Every one of them skips cleanly
     (prints a line, exits 0) if kicad-cli isn't on PATH; the QA-corpus
     portions additionally skip cleanly if that corpus isn't present (see
     EDA_KICAD_QA_BOARDS in each test file). Pass --skip-run to reuse
     whatever is already sitting in docs/parity/raw/ instead of
     re-running the tests (useful while iterating on this script itself).
  2. Folds docs/parity/raw/*.json into docs/parity/REPORT.md (human-
     readable) and docs/parity/scores.json (a flat list of named metrics
     with a `current` value and a `floor`).
  3. The ratchet: a metric's `floor` is never silently lowered. If
     docs/parity/scores.json is already committed, this run's `floor` for
     each metric is max(old_floor, ...) is NOT what happens -- it is left
     at the *old* floor untouched, whatever `current` does, so a
     regression shows up as current < floor instead of being erased. A
     brand-new metric (no prior floor on record) seeds floor = current.
     Raising a floor to lock in a real improvement is a deliberate,
     separate edit to scores.json, not something this script does for you.
     `crates/kicad/tests/parity_ratchet.rs` is the test that actually
     fails the suite on a regression; this script exits non-zero too, for
     CI that runs it directly.

DRC and ERC are not measured here any more: kicad-cli is the only DRC/ERC
engine (docs/ARCHITECTURE.md, "Engines"), so there is no Rust port to hold up
against it. What stays measured is the live connectivity port (the ratsnest)
and the exporter/importer round trip.

No third-party packages; stdlib only.
"""
import json
import subprocess
import sys
import datetime
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
RAW_DIR = REPO_ROOT / "docs/parity/raw"
SCORES_PATH = REPO_ROOT / "docs/parity/scores.json"
REPORT_PATH = REPO_ROOT / "docs/parity/REPORT.md"

TESTS = [
    ("eda-connectivity", "parity_connectivity"),
    ("eda-kicad", "parity_roundtrip"),
]


def run_tests():
    for package, test in TESTS:
        cmd = ["cargo", "test", "--release", "-p", package, "--test", test, "--", "--ignored", "--nocapture"]
        print(f"\n$ {' '.join(cmd)}", flush=True)
        result = subprocess.run(cmd, cwd=REPO_ROOT)
        if result.returncode != 0:
            print(f"warning: {test} exited {result.returncode} (a parity test only hard-fails if the harness itself is broken -- e.g. it processed zero boards; see its own stdout above)", file=sys.stderr)


def load(name):
    path = RAW_DIR / f"{name}.json"
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text())
    except json.JSONDecodeError as e:
        print(f"warning: could not parse {path}: {e}", file=sys.stderr)
        return None


def pct(n, d):
    return (n / d) if d else None


def compute_metrics(conn, rt):
    metrics = {}

    if conn:
        metrics["connectivity_exact_match_rate"] = conn.get("exact_match_rate")
        boards = conn.get("boards", [])
        metrics["connectivity_boards_evaluated"] = float(len([b for b in boards if not b.get("error")]))

    if rt:
        own = rt.get("own_pipeline_roundtrip", []) or []
        seg_before = sum(b["survival"]["segments_before"] for b in own if not b.get("error"))
        seg_after = sum(b["survival"]["segments_after_exact_matches"] for b in own if not b.get("error"))
        via_before = sum(b["survival"]["vias_before"] for b in own if not b.get("error"))
        via_after = sum(b["survival"]["vias_after_exact_matches"] for b in own if not b.get("error"))
        fp_before = sum(b["survival"]["footprints_before"] for b in own if not b.get("error"))
        fp_after = sum(b["survival"]["footprint_pose_exact_matches"] for b in own if not b.get("error"))
        metrics["roundtrip_own_segment_survival_rate"] = pct(seg_after, seg_before)
        metrics["roundtrip_own_via_survival_rate"] = pct(via_after, via_before)
        metrics["roundtrip_own_footprint_pose_survival_rate"] = pct(fp_after, fp_before)

        pcb_notes = rt.get("qa_corpus_pcb_import_notes")
        if pcb_notes:
            metrics["roundtrip_qa_pcb_import_success_rate"] = pct(pcb_notes.get("imported_ok", 0), pcb_notes.get("total", 0))
        sch_notes = rt.get("qa_corpus_sch_import_notes")
        if sch_notes:
            metrics["roundtrip_qa_sch_import_success_rate"] = pct(sch_notes.get("imported_ok", 0), sch_notes.get("total", 0))

        subset = rt.get("reexport_fidelity_subset", []) or []
        if subset:
            ok = len([b for b in subset if not b.get("error") and b.get("reexport_parses") and b.get("identical_type_counts")])
            metrics["roundtrip_reexport_fidelity_rate"] = pct(ok, len(subset))

    return metrics


def merge_scores(metrics):
    old = {}
    if SCORES_PATH.exists():
        try:
            old_json = json.loads(SCORES_PATH.read_text())
            old = {m["name"]: m for m in old_json.get("metrics", [])}
        except (json.JSONDecodeError, KeyError):
            pass

    out = []
    for name, current in sorted(metrics.items()):
        prior = old.get(name)
        if current is None:
            # No data this run (e.g. kicad-cli unavailable) -- keep
            # whatever was on record rather than erasing history.
            floor = prior["floor"] if prior else None
        elif prior and prior.get("floor") is not None:
            floor = prior["floor"]  # never auto-lowered; see module docstring
        else:
            floor = current  # first time this metric is seen: seed the floor at today's level
        entry = {"name": name, "current": current, "floor": floor}
        # An informational metric (`"ratchet": false`) stays on the record
        # with its reason; parity_ratchet.rs doesn't gate on it.
        for key in ("ratchet", "note"):
            if prior and key in prior:
                entry[key] = prior[key]
        out.append(entry)
    return {"generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(), "metrics": out}


def fmt(v):
    if v is None:
        return "n/a"
    if isinstance(v, float):
        if v.is_integer() and v > 1:
            return f"{v:.0f}"  # a count (e.g. "boards_evaluated"), not a rate
        return f"{v * 100:.1f}%" if 0 <= v <= 1 else f"{v:.3f}"
    return str(v)


def type_table(totals):
    lines = ["| type | kicad | ours | matched | missing | extra |", "|---|---:|---:|---:|---:|---:|"]
    for ty, s in sorted(totals.items()):
        lines.append(f"| `{ty}` | {s['kicad']} | {s['ours']} | {s['matched']} | {s['missing']} | {s['extra']} |")
    return "\n".join(lines)


REEXPORT_NOTES = """
What the remaining differences are (read from the two kicad-cli reports of each board; every re-export parses):

- `lib_footprint_issues` / `lib_footprint_mismatch` (`api_kitchen_sink`, `component_classes`): the writer names a footprint `eda:<name>` when the source
  gave it no library (`D5`, `bornier2`), and the board's own project reports library links as warnings, so kicad-cli flags a library that does not
  exist. The studio's derived project ignores both checks unless the project sets them (`effective_rule_severities`).
- `silk_overlap`, `silk_over_copper`, `silk_edge_clearance`, `solder_mask_bridge`, `clearance` (`complex_hierarchy`, `api_kitchen_sink`): footprint-local
  graphics (`fp_line`, `fp_circle`: 512 and 6 on `complex_hierarchy`) are imported into `drawings.footprint_extras`, which only the in-house DRC reads,
  and the writer does not write them back. `api_kitchen_sink` also has a barcode item, which has no IR item.
- `assertion_failure` (`component_classes_drc`): its rules test `A.Component_Class`; component classes are project data this importer does not read.
- `missing_tuning_profile` (`drc_missing_tuning_profile`): tuning profiles are not read either.
- `connection_width_rules`: one extra `clearance` violation, not analysed.
"""


def ui_parity_section():
    """Section 4: the counts come from the generated action audit (docs/parity/UI-ACTIONS.md, written by web/studio/tools/ui-parity-audit.mjs)."""
    lines = ["## 4. UI parity -- KiCad actions vs the studio\n"]
    audit = REPO_ROOT / "docs/parity/UI-ACTIONS.md"
    rows = []
    if audit.exists():
        for line in audit.read_text().splitlines():
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if len(cells) == 7 and cells[0] in ("pcbnew", "eeschema", "common") and cells[1].isdigit():
                rows.append(cells)
    if rows:
        lines.append("Every KiCad action in `web/studio/src/kicad/actions.json` is classified against the studio's action registry by `web/studio/tools/ui-parity-audit.mjs` (it rewrites `docs/parity/UI-ACTIONS.md`). A \"missing with a reason\" action is one the studio deliberately does not wire, with the reason recorded in `web/studio/tools/ui-parity-missing.json`.\n")
        lines.append("| editor | actions | handled | referenced | missing | missing with a reason | hotkeyed & not handled |")
        lines.append("|---|---:|---:|---:|---:|---:|---:|")
        for c in rows:
            lines.append("| " + " | ".join(c) + " |")
        lines.append("")
    else:
        lines.append("_`docs/parity/UI-ACTIONS.md` not found. Run `node web/studio/tools/ui-parity-audit.mjs`._\n")
    lines.append("Behavior (not just the presence of an action) is tracked per feature in `web/studio/PARITY-pcb.md` and the ranked, actionable gaps in `docs/parity/GAPS.md`.\n")
    return "\n".join(lines)


def render_report(conn, rt, scores):
    lines = []
    lines.append("# agentic-eda vs KiCad -- Parity Report\n")
    lines.append(f"_Generated {datetime.datetime.now(datetime.timezone.utc).isoformat()} by `tools/parity_report.py`._\n")
    lines.append("## How to reproduce\n")
    lines.append("```")
    for package, test in TESTS:
        lines.append(f"cargo test -p {package} --test {test} -- --ignored --nocapture")
    lines.append("python3 tools/parity_report.py   # runs both above, then rewrites this file + scores.json")
    lines.append("```")
    lines.append(
        "\nNeeds `kicad-cli` on `PATH` (measured against 10.99.0). Every test above skips cleanly "
        "(prints a line, exits 0) if it's absent. The KiCad QA-corpus portions additionally skip "
        "cleanly if their corpus directory isn't present (`KICAD_QA_DATA`, default `qa/data` of the KiCad "
        "sources at `~/ws/kicad-src-8303b2ad`; `EDA_KICAD_QA_BOARDS` still wins when set) -- the "
        "examples/ladder/work-based measurements still run either way. `EDA_PARITY_PARTS=own,corpus,reexport` "
        "(a comma list) runs part of the round-trip harness and keeps the other parts' last results. "
        "`cargo test` (no `--ignored`) runs `crates/kicad/tests/parity_ratchet.rs`, which "
        "reads the committed `docs/parity/scores.json` below and fails if `current` has dropped below "
        "its recorded `floor` for any metric -- that test needs neither kicad-cli nor the QA corpus.\n"
    )

    lines.append("## Headline numbers\n")
    lines.append("| metric | current | floor |")
    lines.append("|---|---:|---:|")
    for m in scores["metrics"]:
        lines.append(f"| `{m['name']}` | {fmt(m['current'])} | {fmt(m['floor'])} |")
    lines.append("")

    lines.append("## 1. Connectivity -- `eda_connectivity::analyze` vs KiCad's `unconnected_items`/`track_dangling`/`via_dangling`\n")
    if conn:
        lines.append(f"_Measured {conn['measured_at']}._\n" if conn.get("measured_at") else "_Measurement date not recorded._\n")
        lines.append(f"Boards evaluated: {len([b for b in conn['boards'] if not b.get('error')])} (of {len(conn['boards'])} attempted).\n")
        lines.append(f"Totals -- ours: {conn['totals_ours']}, oracle: {conn['totals_oracle']}.\n")
        lines.append(f"**Exact per-board-per-field match rate: {conn['exact_matches']}/{conn['total_checks']} ({fmt(conn['exact_match_rate'])}).**\n")
        if conn.get("import_failures"):
            lines.append(f"\n### Boards that errored ({len(conn['import_failures'])})\n")
            for f in conn["import_failures"][:20]:
                lines.append(f"- {f}")
    else:
        lines.append("_No data -- `docs/parity/raw/connectivity.json` not found. Run the connectivity parity test (needs kicad-cli)._")
    lines.append("")

    lines.append("## 2. Round-trips\n")
    if rt:
        measured = rt.get("measured_at", {})

        def when(part):
            return f"_Measured {measured[part]}._\n" if part in measured else "_Measurement date not recorded._\n"

        lines.append("### Our own pipeline (`design.json` -> `.kicad_pcb` -> `import_kicad_pcb` -> `design.json`)\n")
        lines.append(when("own"))
        own = rt.get("own_pipeline_roundtrip", [])
        lines.append("| board | footprints (pose-exact) | track segments (exact) | vias (exact) | zones before/after |")
        lines.append("|---|---:|---:|---:|---:|")
        for b in own:
            if b.get("error"):
                lines.append(f"| {b['board']} | ERROR: {b['error']} | | | |")
                continue
            s = b["survival"]
            lines.append(f"| {b['board']} | {s['footprint_pose_exact_matches']}/{s['footprints_before']} | {s['segments_after_exact_matches']}/{s['segments_before']} | {s['vias_after_exact_matches']}/{s['vias_before']} | {s['zones_before']}/{s['zones_after']} |")

        pcb_notes = rt.get("qa_corpus_pcb_import_notes")
        if pcb_notes:
            lines.append(f"\n### KiCad QA corpus: `import_kicad_pcb` on all {pcb_notes['total']} real boards\n")
            lines.append(when("corpus"))
            lines.append(f"- imported ok: {pcb_notes['imported_ok']}/{pcb_notes['total']}")
            lines.append(f"- zones skipped (no polygon, or no copper layer): {pcb_notes['zones_skipped']}")
            lines.append(f"- track arcs kept as arcs: {pcb_notes.get('track_arcs_kept', 'n/a')}")
            if pcb_notes.get("outline_shapes") is not None:
                # Since the board outline is built as KiCad builds it (2026-10-09) an Edge.Cuts arc is kept, never approximated.
                lines.append(f"- Edge.Cuts items kept as shapes (arcs, circles, rectangles, curves, cutouts; none turned into chords): {pcb_notes['outline_shapes']}")
            else:
                lines.append(f"- board-outline arcs approximated as straight segments: {pcb_notes['track_arcs_approximated']}")
            lines.append(f"- non-rect pad shapes approximated as rect: {pcb_notes['non_rect_pad_shapes_approximated']}")
            lines.append(f"- boards whose outline didn't close into a loop: {pcb_notes['outline_open']}")
            if pcb_notes.get("outline_malformed") is not None:
                lines.append(f"- boards whose Edge.Cuts are malformed by KiCad's own test (`invalid_outline`): {pcb_notes['outline_malformed']} {pcb_notes.get('outline_malformed_boards', [])}")
            lines.append(f"- outline source breakdown: {pcb_notes['outline_source_counts']}")
            if pcb_notes.get("failures"):
                lines.append(f"\nImport failures ({len(pcb_notes['failures'])}):\n")
                for f in pcb_notes["failures"][:20]:
                    lines.append(f"- {f}")
                if len(pcb_notes["failures"]) > 20:
                    lines.append(f"- ... and {len(pcb_notes['failures']) - 20} more (see docs/parity/raw/roundtrip.json)")

        sch_notes = rt.get("qa_corpus_sch_import_notes")
        if sch_notes:
            lines.append(f"\n### KiCad QA corpus: `import_kicad_sch` on all {sch_notes['total']} real schematics\n")
            lines.append(f"- imported ok: {sch_notes['imported_ok']}/{sch_notes['total']}")
            lines.append(f"- sheets not descended into: {sch_notes['sheets_not_descended']}")
            lines.append(f"- unresolved symbols dropped: {sch_notes['unresolved_symbols']}")
            if sch_notes.get("failures"):
                lines.append(f"\nImport failures ({len(sch_notes['failures'])}):\n")
                for f in sch_notes["failures"][:20]:
                    lines.append(f"- {f}")

        subset = rt.get("reexport_fidelity_subset", [])
        if subset:
            lines.append(f"\n### Re-export fidelity on {len(subset)} real QA boards (import -> our export -> kicad-cli DRC, vs kicad-cli DRC on the original)\n")
            lines.append(when("reexport"))
            lines.append("| board | original parses | re-export parses | identical violation-type counts | where kicad-cli's counts differ (type: original -> re-export) |")
            lines.append("|---|---|---|---|---|")
            for b in subset:
                if b.get("error"):
                    lines.append(f"| {b['board']} | ERROR: {b['error']} | | | |")
                    continue
                ot, rt_ = b.get("original_types", {}), b.get("reexport_types", {})
                diff = ", ".join(f"`{k}`: {ot.get(k, 0)} -> {rt_.get(k, 0)}" for k in sorted(set(ot) | set(rt_)) if ot.get(k, 0) != rt_.get(k, 0))
                lines.append(f"| {b['board']} | {b['original_parses']} | {b['reexport_parses']} | {b['identical_type_counts']} | {diff} |")
            lines.append(REEXPORT_NOTES)
    else:
        lines.append("_No data -- `docs/parity/raw/roundtrip.json` not found. Run the round-trip parity test._")
    lines.append("")

    lines.append("## 3. Zone fill\n")
    lines.append(
        "The exporter writes each zone's fill as computed by `crates/zone-filler` (the live zone filler the "
        "studio shows), and kicad-cli judges those fills; `--refill-zones` is opt-in "
        "(docs/ARCHITECTURE.md, \"Engines\"). The connectivity comparison above runs kicad-cli with "
        "`--refill-zones` so a board with a pour isn't penalized for a gap that's out of scope here.\n"
    )
    lines.append("")

    lines.append(ui_parity_section())

    return "\n".join(lines)


def main():
    skip_run = "--skip-run" in sys.argv
    if not skip_run:
        run_tests()

    RAW_DIR.mkdir(parents=True, exist_ok=True)
    conn = load("connectivity")
    rt = load("roundtrip")

    metrics = compute_metrics(conn, rt)
    scores = merge_scores(metrics)
    SCORES_PATH.write_text(json.dumps(scores, indent=2) + "\n")
    print(f"wrote {SCORES_PATH}")

    report = render_report(conn, rt, scores)
    REPORT_PATH.write_text(report + "\n")
    print(f"wrote {REPORT_PATH}")

    regressions = [m for m in scores["metrics"] if m["current"] is not None and m["floor"] is not None and m["current"] < m["floor"] - 1e-9]
    if regressions:
        print(f"\nRATCHET FAILURE: {len(regressions)} metric(s) below their recorded floor:", file=sys.stderr)
        for m in regressions:
            print(f"  {m['name']}: current={m['current']:.4f} floor={m['floor']:.4f}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
