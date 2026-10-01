#!/usr/bin/env python3
"""The one command for the agentic-eda <-> KiCad parity harness.

    python3 tools/parity_report.py

What it does:
  1. Runs the four `#[ignore]`d oracle tests (DRC, ERC, connectivity,
     round-trip -- see crates/*/tests/parity_*.rs), which each talk to
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
    ("eda-drc", "parity_drc"),
    ("eda-connectivity", "parity_connectivity"),
    ("eda-kicad", "parity_erc"),
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


def compute_metrics(drc, erc, conn, rt):
    metrics = {}

    if drc:
        metrics["drc_precision"] = drc.get("precision")
        metrics["drc_recall"] = drc.get("recall")
        boards = drc.get("boards", [])
        metrics["drc_boards_evaluated"] = float(len([b for b in boards if not b.get("error")]))

    if erc:
        metrics["erc_precision"] = erc.get("precision")
        metrics["erc_recall"] = erc.get("recall")
        boards = erc.get("boards", [])
        metrics["erc_boards_evaluated"] = float(len([b for b in boards if not b.get("error")]))

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


# UI parity (pcbnew, eeschema, the 3D viewer) is a qualitative source-reading
# audit, not something `kicad-cli` can re-measure by re-running a test --
# there's no oracle CLI for "does this menu item do the right thing". It was
# produced once by two research passes (one per editor) reading
# `web/studio/src`'s action registry/dialogs/canvas code against
# `pcbnew/tools/*`, `eeschema/tools/*`, and `common/tool|view/*` in the real
# KiCad source, cross-checked against the extracted `kicad/*.json` catalogs.
# Baked in here (rather than hand-edited into REPORT.md directly) so this
# script stays the single source of truth for the whole report: re-running
# it regenerates section 1-5 from fresh `kicad-cli` data *and* keeps this
# section, instead of a plain file overwrite silently deleting it. Refresh
# this text by re-running that audit, not by editing REPORT.md.
UI_PARITY_SECTION = """## 6. UI parity -- pcbnew, eeschema, the 3D viewer

Method: every KiCad action/menu/toolbar/dialog relevant to each editor was classified identical / partial / stub / missing against `web/studio/src`'s actual wiring (`actions/useActionRunner.ts`'s registry, dialog components, canvas interaction code), using the extracted `web/studio/src/kicad/*.json` catalogs as the ground-truth list of what KiCad exposes and the real `.cpp` source as the ground truth for *behavior*. Full per-action tables, hotkey/menu/dialog breakdowns, and mouse-semantics comparisons are in the session that produced this report; `docs/parity/GAPS.md` carries the actionable subset. Percentages below are each audit's own best estimate; see their stated method and confidence.

| editor | parity | confidence | one-line why |
|---|---:|---|---|
| eeschema | **~5%** | high | it's a read-only viewer -- 0 of 240 cataloged actions are wired; the only things that work are view-only (pan/zoom/select-one/properties-panel-read) |
| pcbnew | **~20-25%** | medium | core draw/select/route/view mostly work in simplified form; the bulk of real pcbnew (footprint editor, board setup, net classes, push-and-shove routing, most dialogs) is stub or missing |
| 3D viewer | **~30%** | low (KiCad's `3d-viewer/` source wasn't in the read snapshot) | orbit/pan/zoom/view-presets/layer-toggles work on procedural geometry; real per-footprint 3D models only load via a separate, best-effort async path |

Headline findings worth reading in full (see GAPS.md for the ranked, actionable version of each):

- **eeschema has no edit commands at all**, confirmed three ways in the source: `SchematicView.tsx`'s own header comment ("Read-only for now"), zero `eeschema.*` entries in the action registry, and no mutating ops in `api/types.ts`'s `Schematic` interface (compare to the PCB `Cmd` union's ~20 mutating ops). The schematic data model also has no sheet/hierarchy concept at all.
- **The ERC engine exists and is unexposed.** `crates/kicad/src/erc.rs` is a real, working 808-line implementation (see section 2's measured precision/recall) with zero UI path to run it -- no action, no route, no results dialog.
- **A genuine correctness bug, not just a gap**: `common.Interactive.undo`/`redo` are wired without the `pcbOnly()` guard every sibling action uses, so pressing Ctrl+Z while viewing the Schematic tab silently undoes the last *PCB* edit.
- **A systemic hotkey-extraction bug**: every KiCad action whose default hotkey is behind a `#ifdef __WXMAC__`/`#else` platform conditional extracts wrong (Ctrl+Y doesn't redo, Home doesn't zoom-fit, F1/F2 zoom in/out don't exist as hotkeys at all -- the studio authors noticed and worked around that last one by excluding both rather than fixing the extractor).
- **pcbnew's box-select direction rule is correctly ported** (left-right drag = fully-enclosed, right-left = crossing, matching `pcb_selection_tool.cpp`'s exact comment) -- but scoped to footprints only; tracks/vias/zones/shapes/text are never box-selectable.
- **No click-vs-drag threshold anywhere**: KiCad promotes a mouse-down to a drag only past 8px or 300ms (`tool_dispatcher.cpp`); studio flags "moved" the instant a grid-snapped delta is non-zero, so a sub-pixel jitter on a click can silently nudge a part by one grid step.
- Dialogs are the starkest surface-area gap: roughly **6 of pcbnew's ~75 dialogs** and **1 of eeschema's ~45** (the shared Hotkeys list) have any counterpart at all, and most of those that exist are explicitly read-only by their own code comments.
"""


def render_report(drc, erc, conn, rt, scores):
    lines = []
    lines.append("# agentic-eda vs KiCad -- Parity Report\n")
    lines.append(f"_Generated {datetime.datetime.now(datetime.timezone.utc).isoformat()} by `tools/parity_report.py`._\n")
    lines.append("## How to reproduce\n")
    lines.append("```")
    for package, test in TESTS:
        lines.append(f"cargo test -p {package} --test {test} -- --ignored --nocapture")
    lines.append("python3 tools/parity_report.py   # runs all four above, then rewrites this file + scores.json")
    lines.append("```")
    lines.append(
        "\nNeeds `kicad-cli` on `PATH` (measured against 10.99.0). Every test above skips cleanly "
        "(prints a line, exits 0) if it's absent. The KiCad QA-corpus portions additionally skip "
        "cleanly if their corpus directory isn't present (see `EDA_KICAD_QA_BOARDS` / the default "
        "path in each test file's source) -- the examples/ladder/work-based measurements still run "
        "either way. `cargo test` (no `--ignored`) runs `crates/kicad/tests/parity_ratchet.rs`, which "
        "reads the committed `docs/parity/scores.json` below and fails if `current` has dropped below "
        "its recorded `floor` for any metric -- that test needs neither kicad-cli nor the QA corpus.\n"
    )

    lines.append("## Headline numbers\n")
    lines.append("| metric | current | floor |")
    lines.append("|---|---:|---:|")
    for m in scores["metrics"]:
        lines.append(f"| `{m['name']}` | {fmt(m['current'])} | {fmt(m['floor'])} |")
    lines.append("")

    lines.append("## 1. DRC -- `eda_drc::run` vs `kicad-cli pcb drc`\n")
    lines.append(
        "_Caveat: `eda_drc` is this task's measurement target per its own instructions, but it is **not** the engine "
        "wired into production `eda check`/`eda board` today -- `eda_gates::pcb` still owns `check_placement`/"
        "`check_routing` there (see `crates/drc/src/lib.rs`'s own \"Integration status\" doc comment). Numbers below "
        "describe the standalone ported crate, not what a user driving `eda board` currently gets._\n"
    )
    if drc:
        lines.append(f"Boards evaluated: {len([b for b in drc['boards'] if not b.get('error')])} "
                      f"(of {len(drc['boards'])} attempted). Position-match tolerance: {drc.get('tolerance_um')} um. "
                      f"`{', '.join(drc.get('not_in_scope', []))}` excluded here -- measured instead under Connectivity.\n")
        lines.append(f"**Overall precision: {fmt(drc['precision'])}, recall: {fmt(drc['recall'])}.**\n")
        lines.append(
            "_Reading this number_: `track_width` was last round's single largest false-positive source "
            "(~2900 extra, almost all on one board, `issue11814`) and is fixed this round (GAPS.md #10): this "
            "port was checking every track against its *net class's nominal width* as if that were KiCad's "
            "real minimum-width constraint, when `DRC_ENGINE::loadImplicitRules`'s `TRACK_WIDTH_CONSTRAINT` "
            "floor is always the board-wide `m_TrackMinWidth` (`.kicad_pro`'s `rules.min_track_width`), "
            "independent of net class -- a net class's own width only ever sets that constraint's advisory "
            "`Opt`, which the check never reads. `track_width` now matches every one of the 10 it reports, "
            "0 extra (same nominal-vs-minimum bug and fix applied to `via_diameter`/hole-size minimums too). "
            "Two QA boards that previously timed out (`issue21482`, `issue22475`) now complete -- a real "
            "improvement, but it also surfaces pre-existing, already-documented bugs at a larger scale than "
            "before: `tracks_crossing` is 0 on the KiCad side across every one of these boards but 1005 on "
            "ours, concentrated on `issue22475` (a same-net-but-still-flagged pattern not yet root-caused, "
            "see GAPS.md #3). `clearance`/`shorting_items`/`hole_clearance` are now the dominant over-firing "
            "group (2230/2203/1008 extra) -- diffed item-by-item this round on `issue11814` (GAPS.md #3's "
            "latest update): violations cluster just under the clearance threshold rather than being wildly "
            "wrong, which rules out footprint net-tie exclusion and 90-degree rotation-sign bugs and points "
            "at a smaller-scale geometry fidelity gap (pad shape/size import precision, possibly compounded "
            "by `kimath::Shape` having no rotated-rectangle primitive for non-90-degree placements) that is "
            "diagnosed but not yet fixed.\n"
        )
        lines.append(type_table(drc["totals"]))
        non_kicad = drc.get("non_kicad_totals") or {}
        if non_kicad:
            lines.append(f"\n_Excluded from the above: {sum(non_kicad.values())} occurrences of this project's own placement-quality/netclass checks "
                          f"(no KiCad counterpart by design), which would only add noise to precision/recall._\n")
        if drc.get("zone_fill_impact"):
            lines.append("\n### Zone fill impact (KiCad with vs without `--refill-zones`, same file)\n")
            for z in drc["zone_fill_impact"]:
                lines.append(f"- **{z['board']}**: refilled={z['refill']}, unfilled={z['no_refill']}")
        if drc.get("import_failures"):
            lines.append(f"\n### Boards that errored ({len(drc['import_failures'])}, excluded from totals above)\n")
            for f in drc["import_failures"][:20]:
                lines.append(f"- {f}")
            if len(drc["import_failures"]) > 20:
                lines.append(f"- ... and {len(drc['import_failures']) - 20} more (see docs/parity/raw/drc.json)")
    else:
        lines.append("_No data -- `docs/parity/raw/drc.json` not found. Run the DRC parity test (needs kicad-cli)._")
    lines.append("")

    lines.append("## 2. ERC -- `check_erc` vs `kicad-cli sch erc`\n")
    if erc:
        lines.append(f"Boards evaluated: {len([b for b in erc['boards'] if not b.get('error')])} (of {len(erc['boards'])} attempted).\n")
        lines.append(f"**Overall precision: {fmt(erc['precision'])}, recall: {fmt(erc['recall'])}.**\n")
        lines.append(
            "_Reading this number_: boards evaluated dropped from 52 to 50 this round -- not a corpus "
            "regression, a corpus *correction*. The previous 52 included `work/mcu30`/`work/l1-order`, two "
            "full place-and-route pipeline outputs that only ever existed as local, gitignored scratch state "
            "(`work/` is explicitly \"local state, not source\") on whatever machine first measured them; "
            "they cannot exist in a fresh checkout (this worktree included) and so can never be reproduced "
            "again -- a committed floor resting on them was never reproducible to begin with. The 50 boards "
            "here (17 of this project's own `examples/`, 33 of KiCad's own QA corpus) are exactly what any "
            "fresh checkout, including CI, can always measure, which is why `erc_boards_evaluated`/"
            "`erc_precision`/`erc_recall`'s floors move down to match in this update, with this paragraph as "
            "the record of why: a lower number from a complete, reproducible corpus, not a quieter number "
            "from a shrinking one. The `work/`-only `label_dangling` gap the previous version of this "
            "paragraph described is simply absent from the table below now (0 both sides) as a direct "
            "consequence. Despite the smaller corpus, several checks' *matched* counts actually rose this "
            "round, measured against this exact same 50-board set one commit earlier (before hierarchical "
            "sheets/buses landed) -- `pin_not_connected` 103->110, `pin_not_driven` 30->33, "
            "`power_pin_not_driven` 25->29 -- directly attributable to GAPS.md #6's hierarchical-sheet "
            "flattening actually descending into child sheets now (`import_kicad_sch_tree`) instead of "
            "leaving them opaque, so a board with real sheets gets its true, full connectivity checked "
            "instead of just its root sheet's own. Of this "
            "round's other two new GAPS.md areas: multi-unit-symbol checks have no QA/example board with a "
            "real multi-unit part to exercise them, and none of the 50 boards here draws an actual bus wire "
            "or has a hierarchical sheet pin/label pairing that disagrees -- GAPS.md #20/#21's checks "
            "(`different_unit_net`, `bus_to_net_conflict`, `net_not_bus_member`, `bus_to_bus_conflict`, ...) "
            "are therefore validated by this project's own unit/integration tests (`crates/kicad/src/bus.rs`, "
            "`hierarchy.rs`, and `erc.rs`'s own test modules), not by this parity corpus, which has nothing "
            "wrong for them to catch. The two largest *remaining* gaps are both understood, not mysterious. "
            "`lib_symbol_mismatch`'s 562 missing are concentrated in this project's own freshly-derived "
            "(not yet exported) example boards: `check_lib_symbol_issues` compares a schematic's embedded "
            "symbol cache against the real library, but for a design that hasn't been through `export_kicad_sch` "
            "yet, the \"cached\" copy *is* the same in-memory lookup as the \"real\" one, so no structural "
            "difference can ever be found there -- only once a file is actually written does this project's "
            "own box-corner re-baking of a real symbol's graphics diverge from the library's native coordinates "
            "the way `kicad-cli` sees it. Catching that would mean predicting the exporter's own output from "
            "inside ERC (or changing what the exporter writes), both out of scope for this port; the QA "
            "corpus's own real mismatches (5, version-skew on `Jumper`/`Device:R`) are matched correctly. "
            "`pin_not_connected`/`pin_not_driven`/`power_pin_not_driven`'s smaller residual gaps trace to a "
            "real architectural difference: KiCad groups pins into per-sheet graphical subgraphs first and "
            "only secondarily merges by net name, while this project's net model (`ConstraintModel::nets`) "
            "merges by name from the start -- a full subgraph port is out of scope here. "
            "`undefined_netclass`/`unresolved_variable` need IR concepts (netclasses, text-variable "
            "resolution) this project doesn't have yet.\n"
        )
        lines.append(type_table(erc["totals"]))
        style_only = erc.get("style_only_totals") or {}
        if style_only:
            lines.append(f"\n_Excluded from the above: {sum(style_only.values())} occurrences of this project's own schematic readability/style checks "
                          f"(`schematic_*`, from `erc_style.rs`), which have no KiCad counterpart by design and would only add noise to precision/recall._\n")
        if erc.get("import_failures"):
            lines.append(f"\n### Boards that errored ({len(erc['import_failures'])}, excluded from totals above)\n")
            for f in erc["import_failures"][:20]:
                lines.append(f"- {f}")
            if len(erc["import_failures"]) > 20:
                lines.append(f"- ... and {len(erc['import_failures']) - 20} more (see docs/parity/raw/erc.json)")
    else:
        lines.append("_No data -- `docs/parity/raw/erc.json` not found. Run the ERC parity test (needs kicad-cli)._")
    lines.append("")

    lines.append("## 3. Connectivity -- `eda_connectivity::analyze` vs KiCad's `unconnected_items`/`track_dangling`/`via_dangling`\n")
    if conn:
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

    lines.append("## 4. Round-trips\n")
    if rt:
        lines.append("### Our own pipeline (`design.json` -> `.kicad_pcb` -> `import_kicad_pcb` -> `design.json`)\n")
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
            lines.append(f"- imported ok: {pcb_notes['imported_ok']}/{pcb_notes['total']}")
            lines.append(f"- zones skipped (not imported): {pcb_notes['zones_skipped']}")
            lines.append(f"- track arcs approximated as straight segments: {pcb_notes['track_arcs_approximated']}")
            lines.append(f"- non-rect pad shapes approximated as rect: {pcb_notes['non_rect_pad_shapes_approximated']}")
            lines.append(f"- boards whose outline didn't close into a loop: {pcb_notes['outline_open']}")
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
            lines.append("| board | original parses | re-export parses | identical violation-type counts |")
            lines.append("|---|---|---|---|")
            for b in subset:
                if b.get("error"):
                    lines.append(f"| {b['board']} | ERROR: {b['error']} | | |")
                    continue
                lines.append(f"| {b['board']} | {b['original_parses']} | {b['reexport_parses']} | {b['identical_type_counts']} |")
    else:
        lines.append("_No data -- `docs/parity/raw/roundtrip.json` not found. Run the round-trip parity test._")
    lines.append("")

    lines.append("## 5. Zone fill\n")
    lines.append(
        "**Not ported.** `export_kicad_pcb` writes a zone's outline only; no fill polygon is ever "
        "computed (a port is in progress -- see the crate-level doc comments in `crates/drc` and "
        "`crates/connectivity`). Every DRC/connectivity comparison above runs kicad-cli with "
        "`--refill-zones` specifically so a board with a pour isn't penalized for a gap that's out of "
        "scope here; the zone-fill-impact rows under section 1 show what changes when KiCad computes "
        "a real fill instead of the bare outline this project exports.\n"
    )
    lines.append("")

    lines.append(UI_PARITY_SECTION)

    return "\n".join(lines)


def main():
    skip_run = "--skip-run" in sys.argv
    if not skip_run:
        run_tests()

    RAW_DIR.mkdir(parents=True, exist_ok=True)
    drc = load("drc")
    erc = load("erc")
    conn = load("connectivity")
    rt = load("roundtrip")

    metrics = compute_metrics(drc, erc, conn, rt)
    scores = merge_scores(metrics)
    SCORES_PATH.write_text(json.dumps(scores, indent=2) + "\n")
    print(f"wrote {SCORES_PATH}")

    report = render_report(drc, erc, conn, rt, scores)
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
