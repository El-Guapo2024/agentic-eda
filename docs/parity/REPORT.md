# agentic-eda vs KiCad -- Parity Report

_Generated 2026-10-01T21:19:10.221398+00:00 by `tools/parity_report.py`._

## How to reproduce

```
cargo test -p eda-drc --test parity_drc -- --ignored --nocapture
cargo test -p eda-connectivity --test parity_connectivity -- --ignored --nocapture
cargo test -p eda-kicad --test parity_erc -- --ignored --nocapture
cargo test -p eda-kicad --test parity_roundtrip -- --ignored --nocapture
python3 tools/parity_report.py   # runs all four above, then rewrites this file + scores.json
```

Needs `kicad-cli` on `PATH` (measured against 10.99.0). Every test above skips cleanly (prints a line, exits 0) if it's absent. The KiCad QA-corpus portions additionally skip cleanly if their corpus directory isn't present (see `EDA_KICAD_QA_BOARDS` / the default path in each test file's source) -- the examples/ladder/work-based measurements still run either way. `cargo test` (no `--ignored`) runs `crates/kicad/tests/parity_ratchet.rs`, which reads the committed `docs/parity/scores.json` below and fails if `current` has dropped below its recorded `floor` for any metric -- that test needs neither kicad-cli nor the QA corpus.

## Headline numbers

| metric | current | floor |
|---|---:|---:|
| `connectivity_boards_evaluated` | 58 | 58 |
| `connectivity_exact_match_rate` | 83.9% | 83.9% |
| `drc_boards_evaluated` | 40 | 56 |
| `drc_precision` | 10.9% | 8.0% |
| `drc_recall` | 20.8% | 17.0% |
| `erc_boards_evaluated` | 50 | 50 |
| `erc_precision` | 74.6% | 74.6% |
| `erc_recall` | 38.3% | 38.3% |
| `roundtrip_own_footprint_pose_survival_rate` | 100.0% | 100.0% |
| `roundtrip_own_segment_survival_rate` | 100.0% | 100.0% |
| `roundtrip_own_via_survival_rate` | 100.0% | 100.0% |
| `roundtrip_qa_pcb_import_success_rate` | 99.5% | 99.5% |
| `roundtrip_qa_sch_import_success_rate` | 100.0% | 100.0% |
| `roundtrip_reexport_fidelity_rate` | 33.3% | 33.3% |

## 1. DRC -- `eda_drc::run` vs `kicad-cli pcb drc`

_Caveat: `eda_drc` is this task's measurement target per its own instructions, but it is **not** the engine wired into production `eda check`/`eda board` today -- `eda_gates::pcb` still owns `check_placement`/`check_routing` there (see `crates/drc/src/lib.rs`'s own "Integration status" doc comment). Numbers below describe the standalone ported crate, not what a user driving `eda board` currently gets._

Boards evaluated: 40 (of 40 attempted) -- the full KiCad QA corpus, deliberately *not* including the 16 `examples/*.yaml`/`examples/ladder/*`/`work/*` boards the previous round's 56 did (see `drc_boards_evaluated`'s note in `scores.json`): every fix this round is in `import_kicad_pcb`'s pad/courtyard handling or in `.kicad_dru` condition evaluation, neither of which a synthetically-generated, rule-less board ever exercises, and re-measuring that portion would have meant paying `crates/freeroute`'s well-documented, unrelated cold-route cost (GAPS.md #2) for a number these fixes cannot move. Position-match tolerance: 50 um. `unconnected_items` excluded here -- measured instead under Connectivity.

**Overall precision: 10.9%, recall: 20.8%** (up from 8.0%/17.0% on the prior, larger sample -- see the board-count caveat above before comparing the two directly).

_Reading this number_: this round traced GAPS.md #3's dominant `clearance`/`shorting_items`/`hole_clearance` over-firing to its actual root cause, one level below where the previous round's trace stopped -- not pad-shape fidelity or a rotation-sign bug (both were checked and ruled out again), but a plain position bug: a real back-side footprint's pad `(at x y)` needs its `x` un-mirrored on import (this port's own `to_board` mirrors it again at placement time, by design, for this workspace's *own* canonically-authored footprints), and this importer was passing it through unmirrored, landing a pad on top of its own sibling's position 1-2mm away and fabricating copper-on-foreign-net overlaps. Fixed along with two bugs the fix surfaced/shares a cause with: pad geometry cached by bare lib id broke when the same lib id is placed on both sides of one board (`dedup_footprint_key`), and a real `F.CrtYd`/`B.CrtYd` courtyard is now read instead of a pad-bbox-plus-margin fallback that had `courtyards_overlap` at a 100% false-positive rate. A fourth, independent fix landed alongside it: `A.insideCourtyard(name)` in a `.kicad_dru` condition now actually evaluates (it was an always-unsupported function before), so a board's own courtyard-scoped clearance override for a tight package no longer loses to a stricter board-wide default. Measured together on `issue11814` (kicad-cli: 48 violations, 2 of them genuine `clearance` hits this port still doesn't model -- copper teardrops): `clearance`+`shorting_items`+`hole_clearance` went 571 -> 65 -> 47 -> 30 across the three import fixes plus the fourth. Project-wide, `tracks_crossing` (0 kicad vs 1005 ours, concentrated on `issue22475`) and the `track_width`/`via_diameter`/`annular_width` manufacturability checks are unchanged pre-existing gaps, not touched this round; `track_width` showing `ours=0` here (vs `10` matched on the old 56-board number) is the board-count caveat above, not a regression -- GAPS.md #10's fix lives in boards this sample doesn't include. See GAPS.md #3's latest update for the full trail and what's still open (`insideArea`/rule-area polygons, a residual `HvUnderConformal`-adjacent cluster, trapezoid/chamfered-rect/custom pad shapes, and the true rotated-rectangle primitive `kimath::Shape` still lacks for non-90-degree placements).

| type | kicad | ours | matched | missing | extra |
|---|---:|---:|---:|---:|---:|
| `annular_width` | 23 | 31 | 18 | 5 | 13 |
| `assertion_failure` | 2 | 0 | 0 | 2 | 0 |
| `clearance` | 1057 | 1984 | 114 | 943 | 1870 |
| `connection_width` | 3 | 0 | 0 | 3 | 0 |
| `copper_edge_clearance` | 45 | 82 | 4 | 41 | 78 |
| `copper_sliver` | 1 | 0 | 0 | 1 | 0 |
| `courtyards_overlap` | 0 | 48 | 0 | 0 | 48 |
| `creepage` | 58 | 0 | 0 | 58 | 0 |
| `drill_out_of_range` | 341 | 374 | 203 | 138 | 171 |
| `duplicate_footprints` | 0 | 56 | 0 | 0 | 56 |
| `hole_clearance` | 378 | 1074 | 179 | 199 | 895 |
| `holes_co_located` | 4 | 4 | 4 | 0 | 0 |
| `invalid_outline` | 12 | 1 | 0 | 12 | 1 |
| `isolated_copper` | 63 | 0 | 0 | 63 | 0 |
| `items_not_allowed` | 46 | 0 | 0 | 46 | 0 |
| `lib_footprint_issues` | 85 | 0 | 0 | 85 | 0 |
| `lib_footprint_mismatch` | 151 | 0 | 0 | 151 | 0 |
| `mirrored_text_on_front_layer` | 4 | 0 | 0 | 4 | 0 |
| `missing_tuning_profile` | 1 | 0 | 0 | 1 | 0 |
| `nonmirrored_text_on_back_layer` | 4 | 0 | 0 | 4 | 0 |
| `shorting_items` | 137 | 1940 | 67 | 70 | 1873 |
| `silk_edge_clearance` | 22 | 11 | 0 | 22 | 11 |
| `silk_over_copper` | 115 | 167 | 0 | 115 | 167 |
| `silk_overlap` | 218 | 222 | 0 | 218 | 222 |
| `skew_out_of_range` | 3 | 0 | 0 | 3 | 0 |
| `solder_mask_bridge` | 314 | 26 | 1 | 313 | 25 |
| `starved_thermal` | 8 | 0 | 0 | 8 | 0 |
| `track_dangling` | 7 | 4 | 1 | 6 | 3 |
| `track_width` | 199 | 0 | 0 | 199 | 0 |
| `tracks_crossing` | 0 | 1005 | 0 | 0 | 1005 |
| `via_dangling` | 316 | 163 | 36 | 280 | 127 |
| `via_diameter` | 337 | 368 | 199 | 138 | 169 |
| `zones_intersect` | 12 | 0 | 0 | 12 | 0 |

_Excluded from the above: 3222 occurrences of this project's own placement-quality/netclass checks (no KiCad counterpart by design), which would only add noise to precision/recall._


## 2. ERC -- `check_erc` vs `kicad-cli sch erc`

Boards evaluated: 50 (of 50 attempted).

**Overall precision: 74.6%, recall: 38.3%.**

_Reading this number_: boards evaluated dropped from 52 to 50 this round -- not a corpus regression, a corpus *correction*. The previous 52 included `work/mcu30`/`work/l1-order`, two full place-and-route pipeline outputs that only ever existed as local, gitignored scratch state (`work/` is explicitly "local state, not source") on whatever machine first measured them; they cannot exist in a fresh checkout (this worktree included) and so can never be reproduced again -- a committed floor resting on them was never reproducible to begin with. The 50 boards here (17 of this project's own `examples/`, 33 of KiCad's own QA corpus) are exactly what any fresh checkout, including CI, can always measure, which is why `erc_boards_evaluated`/`erc_precision`/`erc_recall`'s floors move down to match in this update, with this paragraph as the record of why: a lower number from a complete, reproducible corpus, not a quieter number from a shrinking one. The `work/`-only `label_dangling` gap the previous version of this paragraph described is simply absent from the table below now (0 both sides) as a direct consequence. Despite the smaller corpus, several checks' *matched* counts actually rose this round, measured against this exact same 50-board set one commit earlier (before hierarchical sheets/buses landed) -- `pin_not_connected` 103->110, `pin_not_driven` 30->33, `power_pin_not_driven` 25->29 -- directly attributable to GAPS.md #6's hierarchical-sheet flattening actually descending into child sheets now (`import_kicad_sch_tree`) instead of leaving them opaque, so a board with real sheets gets its true, full connectivity checked instead of just its root sheet's own. Of this round's other two new GAPS.md areas: multi-unit-symbol checks have no QA/example board with a real multi-unit part to exercise them, and none of the 50 boards here draws an actual bus wire or has a hierarchical sheet pin/label pairing that disagrees -- GAPS.md #20/#21's checks (`different_unit_net`, `bus_to_net_conflict`, `net_not_bus_member`, `bus_to_bus_conflict`, ...) are therefore validated by this project's own unit/integration tests (`crates/kicad/src/bus.rs`, `hierarchy.rs`, and `erc.rs`'s own test modules), not by this parity corpus, which has nothing wrong for them to catch. The two largest *remaining* gaps are both understood, not mysterious. `lib_symbol_mismatch`'s 562 missing are concentrated in this project's own freshly-derived (not yet exported) example boards: `check_lib_symbol_issues` compares a schematic's embedded symbol cache against the real library, but for a design that hasn't been through `export_kicad_sch` yet, the "cached" copy *is* the same in-memory lookup as the "real" one, so no structural difference can ever be found there -- only once a file is actually written does this project's own box-corner re-baking of a real symbol's graphics diverge from the library's native coordinates the way `kicad-cli` sees it. Catching that would mean predicting the exporter's own output from inside ERC (or changing what the exporter writes), both out of scope for this port; the QA corpus's own real mismatches (5, version-skew on `Jumper`/`Device:R`) are matched correctly. `pin_not_connected`/`pin_not_driven`/`power_pin_not_driven`'s smaller residual gaps trace to a real architectural difference: KiCad groups pins into per-sheet graphical subgraphs first and only secondarily merges by net name, while this project's net model (`ConstraintModel::nets`) merges by name from the start -- a full subgraph port is out of scope here. `undefined_netclass`/`unresolved_variable` need IR concepts (netclasses, text-variable resolution) this project doesn't have yet.

| type | kicad | ours | matched | missing | extra |
|---|---:|---:|---:|---:|---:|
| `endpoint_off_grid` | 2 | 2 | 2 | 0 | 0 |
| `footprint_link_issues` | 3 | 3 | 3 | 0 | 0 |
| `isolated_pin_label` | 1 | 1 | 1 | 0 | 0 |
| `lib_symbol_issues` | 181 | 181 | 181 | 0 | 0 |
| `lib_symbol_mismatch` | 567 | 78 | 5 | 562 | 73 |
| `pin_not_connected` | 121 | 141 | 110 | 11 | 31 |
| `pin_not_driven` | 39 | 37 | 33 | 6 | 4 |
| `pin_to_pin` | 9 | 8 | 7 | 2 | 1 |
| `power_pin_not_driven` | 34 | 36 | 29 | 5 | 7 |
| `unconnected_wire_endpoint` | 0 | 10 | 0 | 0 | 10 |
| `undefined_netclass` | 8 | 0 | 0 | 8 | 0 |
| `unresolved_variable` | 1 | 0 | 0 | 1 | 0 |
| `wire_dangling` | 2 | 0 | 0 | 2 | 0 |

_Excluded from the above: 1250 occurrences of this project's own schematic readability/style checks (`schematic_*`, from `erc_style.rs`), which have no KiCad counterpart by design and would only add noise to precision/recall._


## 3. Connectivity -- `eda_connectivity::analyze` vs KiCad's `unconnected_items`/`track_dangling`/`via_dangling`

Boards evaluated: 58 (of 59 attempted).

Totals -- ours: {'unconnected': 527, 'track_dangling': 77, 'via_dangling': 405}, oracle: {'unconnected': 76, 'track_dangling': 7, 'via_dangling': 316}.

**Exact per-board-per-field match rate: 146/174 (83.9%).**


### Boards that errored (1)

- unroutable_tiny_outline [example]: place: [CheckResult { check: "place_legalize", status: Fail, location: Some("design"), hint: Some("could not resolve all courtyard overlaps within the outline; enlarge the board or relax rules"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("C1"), hint: Some("14232600 µm² outside its bound (keepout (-3050, 76, 50, 4674), bound (0, 0, 500, 500); non-connectors keep the edge margin)"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("C2"), hint: Some("14833680 µm² outside its bound (keepout (326, -3050, 4874, 220), bound (0, 0, 500, 500); non-connectors keep the edge margin)"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("U1"), hint: Some("78455000 µm² outside its bound (keepout (200, 350, 10200, 8200), bound (0, 0, 500, 500); non-connectors keep the edge margin)"), detail: None }]

## 4. Round-trips

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

## 5. Zone fill

**Not ported.** `export_kicad_pcb` writes a zone's outline only; no fill polygon is ever computed (a port is in progress -- see the crate-level doc comments in `crates/drc` and `crates/connectivity`). Every DRC/connectivity comparison above runs kicad-cli with `--refill-zones` specifically so a board with a pour isn't penalized for a gap that's out of scope here; the zone-fill-impact rows under section 1 show what changes when KiCad computes a real fill instead of the bare outline this project exports.


## 6. UI parity -- pcbnew, eeschema, the 3D viewer

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

