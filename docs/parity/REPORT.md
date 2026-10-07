# agentic-eda vs KiCad -- Parity Report

_Generated 2026-10-04T00:40:09.281258+00:00 by `tools/parity_report.py`._

## How to reproduce

```
cargo test -p eda-connectivity --test parity_connectivity -- --ignored --nocapture
cargo test -p eda-kicad --test parity_roundtrip -- --ignored --nocapture
python3 tools/parity_report.py   # runs both above, then rewrites this file + scores.json
```

Needs `kicad-cli` on `PATH` (measured against 10.99.0). Every test above skips cleanly (prints a line, exits 0) if it's absent. The KiCad QA-corpus portions additionally skip cleanly if their corpus directory isn't present (see `EDA_KICAD_QA_BOARDS` / the default path in each test file's source) -- the examples/ladder/work-based measurements still run either way. `cargo test` (no `--ignored`) runs `crates/kicad/tests/parity_ratchet.rs`, which reads the committed `docs/parity/scores.json` below and fails if `current` has dropped below its recorded `floor` for any metric -- that test needs neither kicad-cli nor the QA corpus.

## Headline numbers

| metric | current | floor |
|---|---:|---:|
| `connectivity_boards_evaluated` | 58 | 58 |
| `connectivity_exact_match_rate` | 83.9% | 83.9% |
| `roundtrip_own_footprint_pose_survival_rate` | 100.0% | 100.0% |
| `roundtrip_own_segment_survival_rate` | 100.0% | 100.0% |
| `roundtrip_own_via_survival_rate` | 100.0% | 100.0% |
| `roundtrip_qa_pcb_import_success_rate` | 99.5% | 99.5% |
| `roundtrip_qa_sch_import_success_rate` | 100.0% | 100.0% |
| `roundtrip_reexport_fidelity_rate` | 33.3% | 33.3% |

## 1. Connectivity -- `eda_connectivity::analyze` vs KiCad's `unconnected_items`/`track_dangling`/`via_dangling`

Boards evaluated: 58 (of 59 attempted).

Totals -- ours: {'unconnected': 527, 'track_dangling': 77, 'via_dangling': 405}, oracle: {'unconnected': 76, 'track_dangling': 7, 'via_dangling': 316}.

**Exact per-board-per-field match rate: 146/174 (83.9%).**


### Boards that errored (1)

- unroutable_tiny_outline [example]: place: [CheckResult { check: "place_legalize", status: Fail, location: Some("design"), hint: Some("could not resolve all courtyard overlaps within the outline; enlarge the board or relax rules"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("C1"), hint: Some("14232600 µm² outside its bound (keepout (-3050, 76, 50, 4674), bound (0, 0, 500, 500); non-connectors keep the edge margin)"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("C2"), hint: Some("14833680 µm² outside its bound (keepout (326, -3050, 4874, 220), bound (0, 0, 500, 500); non-connectors keep the edge margin)"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("U1"), hint: Some("78455000 µm² outside its bound (keepout (200, 350, 10200, 8200), bound (0, 0, 500, 500); non-connectors keep the edge margin)"), detail: None }]

## 2. Round-trips

### Our own pipeline (`design.json` -> `.kicad_pcb` -> `import_kicad_pcb` -> `design.json`)

| board | footprints (pose-exact) | track segments (exact) | vias (exact) | zones before/after |
|---|---:|---:|---:|---:|
| all_power_ground_net | 3/3 | 16/16 | 0/0 | 0/0 |
| dense_small_outline | 5/5 | 21/21 | 0/0 | 0/0 |
| l1_usb_mcu | 17/17 | 158/158 | 3/3 | 0/0 |
| l2_sensor_hub | 35/35 | 354/354 | 16/16 | 0/0 |
| l3_motor_hub | 58/58 | 618/618 | 36/36 | 0/0 |
| l4_control_hub | 100/100 | 1273/1273 | 87/87 | 1/0 |
| ldo | 6/6 | 33/33 | 0/0 | 0/0 |
| ldo_proximity_heavy | 8/8 | 52/52 | 0/0 | 0/0 |
| mcu_board_30plus | 30/30 | 221/221 | 7/7 | 0/0 |
| mixed_track_widths | 6/6 | 35/35 | 0/0 | 0/0 |
| nc_pins | 3/3 | 20/20 | 0/0 | 0/0 |
| opamp_filter | 7/7 | 43/43 | 0/0 | 0/0 |
| passive_divider_ladder | 6/6 | 22/22 | 0/0 | 0/0 |
| star_net | 6/6 | 14/14 | 0/0 | 0/0 |
| through_hole_headers | 5/5 | 39/39 | 0/0 | 0/0 |
| two_pin_nets | 3/3 | 9/9 | 0/0 | 0/0 |
| unroutable_tiny_outline | ERROR: place: [CheckResult { check: "place_legalize", status: Fail, location: Some("design"), hint: Some("could not resolve all courtyard overlaps within the outline; enlarge the board or relax rules"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("C1"), hint: Some("14232600 µm² outside its bound (keepout (-3050, 76, 50, 4674), bound (0, 0, 500, 500); non-connectors keep the edge margin)"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("C2"), hint: Some("14833680 µm² outside its bound (keepout (326, -3050, 4874, 220), bound (0, 0, 500, 500); non-connectors keep the edge margin)"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("U1"), hint: Some("78455000 µm² outside its bound (keepout (200, 350, 10200, 8200), bound (0, 0, 500, 500); non-connectors keep the edge margin)"), detail: None }] | | | |

### KiCad QA corpus: `import_kicad_pcb` on all 185 real boards

- imported ok: 184/185
- zones skipped (not imported): 5847
- track arcs approximated as straight segments: 3120
- non-rect pad shapes approximated as rect: 157
- boards whose outline didn't close into a loop: 6
- outline source breakdown: {'lines': 143, 'none': 38, 'poly': 3}

Import failures (1):

- pcbnew/plugins/fakeboard.kicad_pcb: [CheckResult { check: "kicad_import.parse", status: Fail, location: Some("file"), hint: Some("not a valid s-expression file: expected '(' at byte 0"), detail: None }]

### KiCad QA corpus: `import_kicad_sch` on all 33 real schematics

- imported ok: 33/33
- sheets not descended into: 9
- unresolved symbols dropped: 0

### Re-export fidelity on 12 real QA boards (import -> our export -> kicad-cli DRC, vs kicad-cli DRC on the original)

| board | original parses | re-export parses | identical violation-type counts |
|---|---|---|---|
| api_kitchen_sink | True | True | False |
| bad_triangulation_case | True | True | False |
| complex_hierarchy | True | True | False |
| component_classes | True | True | False |
| component_classes_drc | True | True | False |
| connection_width_rules | True | True | False |
| custom_fields | True | True | True |
| custom_pads | True | True | True |
| drc_missing_tuning_profile | True | True | False |
| fill_bad | True | True | False |
| footprints_load_save | True | True | True |
| graphics_load_save_v20240108 | True | True | True |

## 3. Zone fill

The exporter writes each zone's fill as computed by `crates/zone-filler` (the live zone filler the studio shows), and kicad-cli judges those fills; `--refill-zones` is opt-in (docs/ARCHITECTURE.md, "Engines"). The connectivity comparison above runs kicad-cli with `--refill-zones` so a board with a pour isn't penalized for a gap that's out of scope here.


## 4. UI parity -- pcbnew, eeschema, the 3D viewer

Method: every KiCad action/menu/toolbar/dialog relevant to each editor was classified identical / partial / stub / missing against `web/studio/src`'s actual wiring (`actions/useActionRunner.ts`'s registry, dialog components, canvas interaction code), using the extracted `web/studio/src/kicad/*.json` catalogs as the ground-truth list of what KiCad exposes and the real `.cpp` source as the ground truth for *behavior*. Full per-action tables, hotkey/menu/dialog breakdowns, and mouse-semantics comparisons are in the session that produced this report; `docs/parity/GAPS.md` carries the actionable subset. Percentages below are each audit's own best estimate; see their stated method and confidence.

| editor | parity | confidence | one-line why |
|---|---:|---|---|
| eeschema | **~5%** | high | it's a read-only viewer -- 0 of 240 cataloged actions are wired; the only things that work are view-only (pan/zoom/select-one/properties-panel-read) |
| pcbnew | **~20-25%** | medium | core draw/select/route/view mostly work in simplified form; the bulk of real pcbnew (footprint editor, board setup, net classes, push-and-shove routing, most dialogs) is stub or missing |
| 3D viewer | **~30%** | low (KiCad's `3d-viewer/` source wasn't in the read snapshot) | orbit/pan/zoom/view-presets/layer-toggles work on procedural geometry; real per-footprint 3D models only load via a separate, best-effort async path |

Headline findings worth reading in full (see GAPS.md for the ranked, actionable version of each):

- **eeschema has no edit commands at all**, confirmed three ways in the source: `SchematicView.tsx`'s own header comment ("Read-only for now"), zero `eeschema.*` entries in the action registry, and no mutating ops in `api/types.ts`'s `Schematic` interface (compare to the PCB `Cmd` union's ~20 mutating ops). The schematic data model also has no sheet/hierarchy concept at all.
- **ERC now runs from the UI** (closed 2026-10-03): Inspect > Electrical Rules Checker runs `kicad-cli sch erc` on the exported schematic, with a running state, a Lint tab for our own readability checks, and markers on the sheet.
- **A genuine correctness bug, not just a gap**: `common.Interactive.undo`/`redo` are wired without the `pcbOnly()` guard every sibling action uses, so pressing Ctrl+Z while viewing the Schematic tab silently undoes the last *PCB* edit.
- **A systemic hotkey-extraction bug**: every KiCad action whose default hotkey is behind a `#ifdef __WXMAC__`/`#else` platform conditional extracts wrong (Ctrl+Y doesn't redo, Home doesn't zoom-fit, F1/F2 zoom in/out don't exist as hotkeys at all -- the studio authors noticed and worked around that last one by excluding both rather than fixing the extractor).
- **pcbnew's box-select direction rule is correctly ported** (left-right drag = fully-enclosed, right-left = crossing, matching `pcb_selection_tool.cpp`'s exact comment) -- but scoped to footprints only; tracks/vias/zones/shapes/text are never box-selectable.
- **No click-vs-drag threshold anywhere**: KiCad promotes a mouse-down to a drag only past 8px or 300ms (`tool_dispatcher.cpp`); studio flags "moved" the instant a grid-snapped delta is non-zero, so a sub-pixel jitter on a click can silently nudge a part by one grid step.
- Dialogs are the starkest surface-area gap: roughly **6 of pcbnew's ~75 dialogs** and **1 of eeschema's ~45** (the shared Hotkeys list) have any counterpart at all, and most of those that exist are explicitly read-only by their own code comments.

