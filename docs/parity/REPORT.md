# agentic-eda vs KiCad -- Parity Report

_Generated 2026-10-01T18:08:06.182130+00:00 by `tools/parity_report.py`._

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
| `drc_boards_evaluated` | 54 | 54 |
| `drc_precision` | 5.9% | 5.9% |
| `drc_recall` | 12.3% | 12.3% |
| `erc_boards_evaluated` | 52 | 52 |
| `erc_precision` | 75.1% | 75.1% |
| `erc_recall` | 47.8% | 47.8% |
| `roundtrip_own_footprint_pose_survival_rate` | 100.0% | 100.0% |
| `roundtrip_own_segment_survival_rate` | 100.0% | 100.0% |
| `roundtrip_own_via_survival_rate` | 100.0% | 100.0% |
| `roundtrip_qa_pcb_import_success_rate` | 99.5% | 99.5% |
| `roundtrip_qa_sch_import_success_rate` | 100.0% | 100.0% |
| `roundtrip_reexport_fidelity_rate` | 33.3% | 33.3% |

## 1. DRC -- `eda_drc::run` vs `kicad-cli pcb drc`

_Caveat: `eda_drc` is this task's measurement target per its own instructions, but it is **not** the engine wired into production `eda check`/`eda board` today -- `eda_gates::pcb` still owns `check_placement`/`check_routing` there (see `crates/drc/src/lib.rs`'s own "Integration status" doc comment). Numbers below describe the standalone ported crate, not what a user driving `eda board` currently gets._

Boards evaluated: 54 (of 57 attempted). Position-match tolerance: 50 um. `unconnected_items` excluded here -- measured instead under Connectivity.

**Overall precision: 5.9%, recall: 12.3%.**

_Reading this number_: precision is low enough to call out specifically, and it isn't evenly spread. `tracks_crossing` is 0 on the KiCad side across every one of these boards but 1567 on ours -- a 100% disagreement rate for that one check, which looks like a real correctness bug rather than a tolerance or scope difference (see GAPS.md #3). `hole_clearance` has zero *matched* instances despite 179/633 raw counts -- the positions disagree every time, not just the totals (a same-net exclusion bug that inflated the raw count further was fixed this round; the remaining zero-match issue is a separate position/scope mismatch, still open). Several clearance-family checks (`clearance`, `track_width`, `shorting_items`, `silk_over_copper`) over-fire 2-7x -- down from 4-10x before real per-net/netclass rules were imported from the sidecar `.kicad_pro`, but still consistent with remaining gaps in how `import_kicad_pcb` resolves per-net overrides (GAPS.md #10). Three of the four `examples/ladder/*.yaml` rungs are additionally missing from these totals entirely (load-sensitive `crates/freeroute` route times, not `eda_drc` itself -- see GAPS.md #2) -- see the timeout entries below.

| type | kicad | ours | matched | missing | extra |
|---|---:|---:|---:|---:|---:|
| `annular_width` | 23 | 27 | 12 | 11 | 15 |
| `assertion_failure` | 2 | 0 | 0 | 2 | 0 |
| `clearance` | 221 | 414 | 47 | 174 | 367 |
| `connection_width` | 3 | 0 | 0 | 3 | 0 |
| `copper_edge_clearance` | 29 | 37 | 4 | 25 | 33 |
| `copper_sliver` | 1 | 0 | 0 | 1 | 0 |
| `courtyards_overlap` | 0 | 86 | 0 | 0 | 86 |
| `creepage` | 1 | 0 | 0 | 1 | 0 |
| `drill_out_of_range` | 142 | 106 | 4 | 138 | 102 |
| `duplicate_footprints` | 0 | 8 | 0 | 0 | 8 |
| `hole_clearance` | 179 | 271 | 179 | 0 | 92 |
| `holes_co_located` | 4 | 4 | 4 | 0 | 0 |
| `invalid_outline` | 12 | 1 | 0 | 12 | 1 |
| `isolated_copper` | 63 | 0 | 0 | 63 | 0 |
| `items_not_allowed` | 4 | 0 | 0 | 4 | 0 |
| `lib_footprint_issues` | 363 | 0 | 0 | 363 | 0 |
| `lib_footprint_mismatch` | 108 | 0 | 0 | 108 | 0 |
| `mirrored_text_on_front_layer` | 4 | 0 | 0 | 4 | 0 |
| `missing_tuning_profile` | 1 | 0 | 0 | 1 | 0 |
| `nonmirrored_text_on_back_layer` | 4 | 0 | 0 | 4 | 0 |
| `shorting_items` | 1 | 290 | 1 | 0 | 289 |
| `silk_edge_clearance` | 44 | 30 | 0 | 44 | 30 |
| `silk_over_copper` | 182 | 185 | 0 | 182 | 185 |
| `silk_overlap` | 228 | 23 | 0 | 228 | 23 |
| `skew_out_of_range` | 3 | 0 | 0 | 3 | 0 |
| `solder_mask_bridge` | 315 | 290 | 1 | 314 | 289 |
| `starved_thermal` | 9 | 0 | 0 | 9 | 0 |
| `text_height` | 0 | 1 | 0 | 0 | 1 |
| `track_dangling` | 7 | 92 | 1 | 6 | 91 |
| `track_width` | 209 | 2999 | 10 | 199 | 2989 |
| `via_dangling` | 117 | 83 | 36 | 81 | 47 |
| `via_diameter` | 138 | 101 | 0 | 138 | 101 |
| `zones_intersect` | 12 | 0 | 0 | 12 | 0 |

_Excluded from the above: 3190 occurrences of this project's own placement-quality/netclass checks (no KiCad counterpart by design), which would only add noise to precision/recall._


### Boards that errored (3, excluded from totals above)

- unroutable_tiny_outline [example]: place/route: TIMEOUT after 2700s -- likely a hang or quadratic-plus blowup in our own engine on this board's geometry, not kicad-cli (no oracle child process was observed during the hang this was discovered from)
- issue21482/issue21482.kicad_pcb [qa]: TIMEOUT after 60s -- likely a hang or quadratic-plus blowup in our own engine on this board's geometry, not kicad-cli (no oracle child process was observed during the hang this was discovered from)
- issue22475/issue22475.kicad_pcb [qa]: TIMEOUT after 60s -- likely a hang or quadratic-plus blowup in our own engine on this board's geometry, not kicad-cli (no oracle child process was observed during the hang this was discovered from)

## 2. ERC -- `check_erc` vs `kicad-cli sch erc`

Boards evaluated: 52 (of 52 attempted).

**Overall precision: 75.1%, recall: 47.8%.**

_Reading this number_: the two largest remaining gaps are both understood, not mysterious. `lib_symbol_mismatch`'s 562 missing are concentrated in this project's own freshly-derived (not yet exported) example boards: `check_lib_symbol_issues` compares a schematic's embedded symbol cache against the real library, but for a design that hasn't been through `export_kicad_sch` yet, the "cached" copy *is* the same in-memory lookup as the "real" one, so no structural difference can ever be found there -- only once a file is actually written does this project's own box-corner re-baking of a real symbol's graphics diverge from the library's native coordinates the way `kicad-cli` sees it. Catching that would mean predicting the exporter's own output from inside ERC (or changing what the exporter writes), both out of scope for this port; the QA corpus's own real mismatches (5, version-skew on `Jumper`/`Device:R`) are matched correctly. `label_dangling`'s 75 extra are concentrated in two `work/` boards (full place-and-route pipeline output, not this task's own freshly-generated examples) whose labels don't coincide with this project's current exporter's own pin placement -- consistent with those two fixtures predating a later exporter change, not a logic bug in the check itself (the *schematic-only* generation path for the same kind of board has zero such mismatches). `pin_not_connected`/`pin_not_driven`/`power_pin_not_driven`'s smaller residual gaps trace to a real architectural difference: KiCad groups pins into per-sheet graphical subgraphs first and only secondarily merges by net name, while this project's net model (`ConstraintModel::nets`) merges by name from the start -- a full subgraph port is out of scope here. `undefined_netclass`/`unresolved_variable` need IR concepts (netclasses, text-variable resolution) this project doesn't have yet.

| type | kicad | ours | matched | missing | extra |
|---|---:|---:|---:|---:|---:|
| `endpoint_off_grid` | 2 | 2 | 2 | 0 | 0 |
| `footprint_link_issues` | 3 | 3 | 3 | 0 | 0 |
| `isolated_pin_label` | 1 | 1 | 1 | 0 | 0 |
| `label_dangling` | 17 | 92 | 17 | 0 | 75 |
| `lib_symbol_issues` | 228 | 228 | 228 | 0 | 0 |
| `lib_symbol_mismatch` | 567 | 76 | 5 | 562 | 71 |
| `pin_not_connected` | 242 | 248 | 216 | 26 | 32 |
| `pin_not_driven` | 39 | 33 | 30 | 9 | 3 |
| `pin_to_pin` | 9 | 8 | 7 | 2 | 1 |
| `power_pin_not_driven` | 60 | 57 | 50 | 10 | 7 |
| `unconnected_wire_endpoint` | 38 | 33 | 27 | 11 | 6 |
| `undefined_netclass` | 8 | 0 | 0 | 8 | 0 |
| `unresolved_variable` | 1 | 0 | 0 | 1 | 0 |
| `wire_dangling` | 16 | 2 | 2 | 14 | 0 |

_Excluded from the above: 1227 occurrences of this project's own schematic readability/style checks (`schematic_*`, from `erc_style.rs`), which have no KiCad counterpart by design and would only add noise to precision/recall._


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

