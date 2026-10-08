# agentic-eda vs KiCad -- Parity Report

_Generated 2026-10-08T21:01:33.998681+00:00 by `tools/parity_report.py`._

## How to reproduce

```
cargo test -p eda-connectivity --test parity_connectivity -- --ignored --nocapture
cargo test -p eda-kicad --test parity_roundtrip -- --ignored --nocapture
python3 tools/parity_report.py   # runs both above, then rewrites this file + scores.json
```

Needs `kicad-cli` on `PATH` (measured against 10.99.0). Every test above skips cleanly (prints a line, exits 0) if it's absent. The KiCad QA-corpus portions additionally skip cleanly if their corpus directory isn't present (`KICAD_QA_DATA`, default `qa/data` of the KiCad sources at `~/ws/kicad-src-8303b2ad`; `EDA_KICAD_QA_BOARDS` still wins when set) -- the examples/ladder/work-based measurements still run either way. `EDA_PARITY_PARTS=own,corpus,reexport` (a comma list) runs part of the round-trip harness and keeps the other parts' last results. `cargo test` (no `--ignored`) runs `crates/kicad/tests/parity_ratchet.rs`, which reads the committed `docs/parity/scores.json` below and fails if `current` has dropped below its recorded `floor` for any metric -- that test needs neither kicad-cli nor the QA corpus.

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
| `roundtrip_reexport_fidelity_rate` | 50.0% | 33.3% |

## 1. Connectivity -- `eda_connectivity::analyze` vs KiCad's `unconnected_items`/`track_dangling`/`via_dangling`

_Measurement date not recorded._

Boards evaluated: 58 (of 59 attempted).

Totals -- ours: {'unconnected': 527, 'track_dangling': 77, 'via_dangling': 405}, oracle: {'unconnected': 76, 'track_dangling': 7, 'via_dangling': 316}.

**Exact per-board-per-field match rate: 146/174 (83.9%).**


### Boards that errored (1)

- unroutable_tiny_outline [example]: place: [CheckResult { check: "place_legalize", status: Fail, location: Some("design"), hint: Some("could not resolve all courtyard overlaps within the outline; enlarge the board or relax rules"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("C1"), hint: Some("14232600 µm² outside its bound (keepout (-3050, 76, 50, 4674), bound (0, 0, 500, 500); non-connectors keep the edge margin)"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("C2"), hint: Some("14833680 µm² outside its bound (keepout (326, -3050, 4874, 220), bound (0, 0, 500, 500); non-connectors keep the edge margin)"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("U1"), hint: Some("78455000 µm² outside its bound (keepout (200, 350, 10200, 8200), bound (0, 0, 500, 500); non-connectors keep the edge margin)"), detail: None }]

## 2. Round-trips

### Our own pipeline (`design.json` -> `.kicad_pcb` -> `import_kicad_pcb` -> `design.json`)

_Measured 2026-10-04._

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

_Measured 2026-10-08._

- imported ok: 184/185
- zones skipped (no polygon, or no copper layer): 12
- track arcs kept as arcs: 10946
- board-outline arcs approximated as straight segments: 122
- non-rect pad shapes approximated as rect: 189
- boards whose outline didn't close into a loop: 2
- outline source breakdown: {'circle': 1, 'lines': 143, 'none': 37, 'poly': 3}

Import failures (1):

- pcbnew/plugins/fakeboard.kicad_pcb: [CheckResult { check: "kicad_import.parse", status: Fail, location: Some("file"), hint: Some("not a valid s-expression file: expected '(' at byte 0"), detail: None }]

### KiCad QA corpus: `import_kicad_sch` on all 33 real schematics

- imported ok: 33/33
- sheets not descended into: 9
- unresolved symbols dropped: 0

### Re-export fidelity on 12 real QA boards (import -> our export -> kicad-cli DRC, vs kicad-cli DRC on the original)

_Measured 2026-10-08._

| board | original parses | re-export parses | identical violation-type counts | where kicad-cli's counts differ (type: original -> re-export) |
|---|---|---|---|---|
| api_kitchen_sink | True | True | False | `clearance`: 1 -> 0, `lib_footprint_issues`: 0 -> 2, `silk_overlap`: 2 -> 0 |
| bad_triangulation_case | True | True | True |  |
| complex_hierarchy | True | True | False | `clearance`: 112 -> 117, `copper_edge_clearance`: 14 -> 15, `lib_footprint_issues`: 0 -> 72, `shorting_items`: 0 -> 2, `silk_edge_clearance`: 14 -> 2, `silk_over_copper`: 104 -> 2, `silk_overlap`: 199 -> 5, `solder_mask_bridge`: 112 -> 115 |
| component_classes | True | True | False | `lib_footprint_issues`: 0 -> 2, `lib_footprint_mismatch`: 14 -> 12 |
| component_classes_drc | True | True | False | `assertion_failure`: 2 -> 4 |
| connection_width_rules | True | True | False | `clearance`: 0 -> 1 |
| custom_fields | True | True | True |  |
| custom_pads | True | True | True |  |
| drc_missing_tuning_profile | True | True | False | `missing_tuning_profile`: 1 -> 0 |
| fill_bad | True | True | True |  |
| footprints_load_save | True | True | True |  |
| graphics_load_save_v20240108 | True | True | True |  |

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


## 3. Zone fill

The exporter writes each zone's fill as computed by `crates/zone-filler` (the live zone filler the studio shows), and kicad-cli judges those fills; `--refill-zones` is opt-in (docs/ARCHITECTURE.md, "Engines"). The connectivity comparison above runs kicad-cli with `--refill-zones` so a board with a pour isn't penalized for a gap that's out of scope here.


## 4. UI parity -- KiCad actions vs the studio

Every KiCad action in `web/studio/src/kicad/actions.json` is classified against the studio's action registry by `web/studio/tools/ui-parity-audit.mjs` (it rewrites `docs/parity/UI-ACTIONS.md`). A "missing with a reason" action is one the studio deliberately does not wire, with the reason recorded in `web/studio/tools/ui-parity-missing.json`.

| editor | actions | handled | referenced | missing | missing with a reason | hotkeyed & not handled |
|---|---:|---:|---:|---:|---:|---:|
| pcbnew | 350 | 265 | 0 | 0 | 85 | 5 |
| eeschema | 233 | 165 | 0 | 0 | 68 | 10 |
| common | 183 | 143 | 0 | 0 | 40 | 4 |

Behavior (not just the presence of an action) is tracked per feature in `web/studio/PARITY-pcb.md` and the ranked, actionable gaps in `docs/parity/GAPS.md`.

