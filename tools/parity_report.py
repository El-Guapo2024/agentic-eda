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


def fmt_findings(n):
    """The number of findings (violations and unconnected items) one report has that the other does not, or n/a when the run did not measure it."""
    return "n/a" if n is None else str(n)


def type_table(totals):
    lines = ["| type | kicad | ours | matched | missing | extra |", "|---|---:|---:|---:|---:|---:|"]
    for ty, s in sorted(totals.items()):
        lines.append(f"| `{ty}` | {s['kicad']} | {s['ours']} | {s['matched']} | {s['missing']} | {s['extra']} |")
    return "\n".join(lines)


REEXPORT_NOTES = """
Every re-export parses, and kicad-cli's DRC of it says what it says of the original: the same number of violations of each type and, finding by finding, the same type, severity, items and places (to 5 micrometres; the lengths in a
description are left out, because the importer keeps micrometres and an inch board's tenths of one are rounded). The slow test `a_real_boards_reexport_is_judged_by_kicad_cli_as_the_original_is` (`crates/kicad/tests/parity_roundtrip.rs`,
`EDA_SLOW_TESTS=1`) holds this for the 12 boards as the corpus ships them and as kicad-cli 10.99.0-5334 writes them in its newest file format (`EDA_PARITY_REEXPORT_UPGRADE=1` in the harness).

What the round trip changed before, read from the two kicad-cli reports and the two files of each board (`web/studio/PARITY-pcb.md` section 37 has the ports and the tests):

- `clearance`, `shorting_items`, `solder_mask_bridge`, `copper_edge_clearance` (`complex_hierarchy`): a board text of several lines (`Actionneur\\nPiezo New Amp\\nV02`) came back as one line full of backslashes -- the reader did not take the lexer's `\\n` and the
  writer did not write it -- and its glyph width (`(size 2.032 1.524)`) was made square. `connection_width_rules`' extra `clearance`: its labels lost their `justify left bottom` and their bold.
- `clearance` (`complex_hierarchy`, 8): the two pads of a registration mark that sit on one point with one number were written with one uuid.
- `lib_footprint_issues` (`api_kitchen_sink`, `complex_hierarchy`): a footprint with no library link was written `eda:<name>`; `LIB_ID::Format` writes the bare name.
- `courtyards_overlap` (`complex_hierarchy`): a footprint with no courtyard was given the box its pads derive.
- `assertion_failure` (`component_classes_drc`): component classes -- the project's assignment rules and the footprints' own -- were not in the IR.
- `clearance` (`api_kitchen_sink`): its barcodes were not in the IR.
- `missing_tuning_profile` (`drc_missing_tuning_profile`): the reader had the tuning profiles since P9a; the harness did not read the project's Board Setup pages as `eda import-kicad` does.
- Also written back now, though no board of these 12 counts them: the outline's line width, a pad's pin type and function (free pads), the paste and mask layers a pad had and an edge connector pad's type, a pad's and a via's other tokens (a via's type and
  tenting, `remove_unused_layers`, `padstack`, teardrop settings), the arcs of a polygon (flattened as KiCad flattens them), a net only copper names (KiCad 10's files have no net table; the export failed), the nets a schematic's net class labels assign
  (`netclass_assignments`), and bold text of KiCad 10 (its thickness is the width before the bold factor 1.6).

34 more boards -- the 17 templates KiCad installs with its libraries in reach (so that `lib_footprint_mismatch` is judged too) and 17 more QA boards (vias of every kind, teardrops, padstacks, time-domain tuning, net ties) -- are judged the same way:
**29 are identical**, and each of the five others has its cause:

- `lib_footprint_mismatch` of a mounting hole, a test point, ... on every board that has one, before: the footprint's courtyard (a circle, four lines) was written as one polygon, and KiCad compares the shapes one by one with the library's copy
  (`FOOTPRINT::FootprintNeedsUpdate`). A footprint's courtyard graphics, description, tags and paste margins, and its uuid, are written back as the file had them.
- `time_calculations`: the seven net classes the schematic assigned by label (`netclass_assignments`) were not read, so its skew rule judged nothing. Now the violation is there, on another segment of the same net: the writer lists a net's tracks in a canonical
  order (net, layer, points) and kicad-cli names one of them. `issue4257`: a text that names a footprint by its uuid (`${uuid:REFERENCE}`) read `<Unknown reference>`; the footprint keeps its uuid now.
- `issue5830` (5 `clearance` of 0.1770 to 0.1773 mm against 0.178 mm) and `issue9870`: the coordinates are integer micrometres and these inch boards have them to a tenth of one (`100.295778`); a rounded track is half a micrometre closer to its neighbour.
- `issue4257` and `net_tie_drc` (`lib_footprint_issues`, `lib_footprint_mismatch` of the original): the two boards have no project file, so kicad-cli judges the original by its default severities, and the studio's derived project hides the two library
  checks unless a project says otherwise (every footprint here is embedded in the board).

What still differs in the files, with no finding of these boards to show for it: the coordinates are micrometres, so an inch board's tenths of one are rounded; a footprint's jumper pad groups, Datasheet and Description fields and `ki_*` fields, a pad's
teardrop settings and a property's `unlocked` come back as text but are not in the model, and the link to the symbol (`path`) is not written, on purpose: the derived schematic's symbols have uuids of their own; a shape's line style (dashes, dots) and a zone
outline's hatch pitch are not kept; nor are dimension details (text position mode, extension height); reference images and text boxes are written back as text, drawn by kicad-cli's plots and not by the canvas, with no editor (P16); KiCad 10's microvia stack
generators and custom user properties are not read (a top-level item of a kind nobody reads is carried as text); its line endings, grid items, via stitching and drill charts and maps are (PARITY-pcb.md section 50).
The zone fills are KiCad's: the harness exports unfilled zones and `--refill-zones` fills them, the studio keeps the file's own beside `design.json`.
"""


CONNECTIVITY_NOTES = """
The boards are a fixed set: every `examples/*.yaml` and `examples/ladder/*.yaml` board (placed and routed best-effort, then exported), and every
`qa/data/pcbnew/*.kicad_pcb` of the KiCad sources (8303b2ad) plus the first board of each `qa/data/pcbnew/*/` directory, imported. Never the user's
`work/` boards. kicad-cli runs on a copy of each board in a directory of its own; a check the board's project sets to `ignore` is turned back on there
(the board's `unignored`), and our count is capped at kicad-cli's error limit for the check (199, and 499 for `unconnected_items`), past which
kicad-cli reports nothing more.

Why the floor changed (2026-10-23). The old floor, 58 boards and 83.9%, was set on the Mac on a set that counted the user's `work/l1-order` and
`work/mcu30`, which only some checkouts have (a box without them evaluated 56 and failed the ratchet), and a 40-board sample of the QA corpus. Those
two boards are gone, the sample is the whole fixed set above, and two fields that measured nothing are measured now: a check the project ignores was
compared with kicad-cli's zero, and a count past kicad-cli's error limit with the limit. On the new set the port matched 539 of 558 (96.6%, 186
boards); the floor was reset to that, then raised with the fixes of `task-connectivity-parity` (an arc is one item, a pad turned off a multiple of
90 degrees is measured by its copper, a pad or via that removes its unused copper is its hole on those layers) to 546 of 558 (97.8%).
`web/studio/PARITY-pcb.md` section 51 lists the 12 differences left, board by board, with their reasons.
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
        "sources at `~/ws/kicad-src-8303b2ad`, else `$KICAD_SRC_DIR/qa/data` for the connectivity; `EDA_KICAD_QA_BOARDS` still wins when set) -- the "
        "examples/ladder-based measurements still run either way. `EDA_PARITY_PARTS=own,corpus,reexport` "
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
        lines.append(CONNECTIVITY_NOTES)
        if conn.get("import_failures"):
            lines.append(f"\n### Boards that errored ({len(conn['import_failures'])})\n")
            for f in conn["import_failures"][:20]:
                lines.append(f"- {f}")
        lines.append(f"\n### The boards ({len(conn['boards'])})\n")
        lines.append("Each cell is ours / kicad-cli's; a **bold** cell differs. `unignored` names a check the board's project ignores, which the harness turns back on for kicad-cli.\n")
        lines.append("| board | source | unconnected | track_dangling | via_dangling | unignored |")
        lines.append("|---|---|---:|---:|---:|---|")
        for b in conn["boards"]:
            if b.get("error"):
                lines.append(f"| `{b['board']}` | {b['source']} | error | | | |")
                continue

            def cell(field):
                o, k = b["ours"][field], b["oracle"][field]
                return f"{o} / {k}" if o == k else f"**{o} / {k}**"

            lines.append(f"| `{b['board']}` | {b['source']} | {cell('unconnected')} | {cell('track_dangling')} | {cell('via_dangling')} | {', '.join(b.get('unignored', []))} |")
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
            lines.append("| board | original parses | re-export parses | identical violation-type counts | findings that differ (type, items, place) | where kicad-cli's counts differ (type: original -> re-export) |")
            lines.append("|---|---|---|---|---:|---|")
            for b in subset:
                if b.get("error"):
                    lines.append(f"| {b['board']} | ERROR: {b['error']} | | | | |")
                    continue
                ot, rt_ = b.get("original_types", {}), b.get("reexport_types", {})
                diff = ", ".join(f"`{k}`: {ot.get(k, 0)} -> {rt_.get(k, 0)}" for k in sorted(set(ot) | set(rt_)) if ot.get(k, 0) != rt_.get(k, 0))
                findings = fmt_findings(b.get("finding_differences"))
                lines.append(f"| {b['board']} | {b['original_parses']} | {b['reexport_parses']} | {b['identical_type_counts']} | {findings} | {diff} |")
            lines.append(REEXPORT_NOTES)
    else:
        lines.append("_No data -- `docs/parity/raw/roundtrip.json` not found. Run the round-trip parity test._")
    lines.append("")

    lines.append("## 3. Zone fill\n")
    lines.append(
        "The zone fill is KiCad's, made by kicad-cli (`kicad-cli pcb drc --refill-zones --save-board` on a copy of "
        "the derived board: Fill All Zones) and kept beside design.json; there is no other filler "
        "(docs/ARCHITECTURE.md, \"Engines\"; the proof is `crates/kicad-engine/tests/refill_zones.rs`). "
        "The connectivity comparison above gives the connectivity kicad-cli's own fill of the same board, so a "
        "board with a pour isn't penalized for a gap that's out of scope here.\n"
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
