# agentic-eda vs KiCad -- Parity Report

_Generated 2026-10-24T04:08:11.165009+00:00 by `tools/parity_report.py`._

## How to reproduce

```
cargo test -p eda-connectivity --test parity_connectivity -- --ignored --nocapture
cargo test -p eda-kicad --test parity_roundtrip -- --ignored --nocapture
python3 tools/parity_report.py   # runs both above, then rewrites this file + scores.json
```

Needs `kicad-cli` on `PATH` (measured against 10.99.0). Every test above skips cleanly (prints a line, exits 0) if it's absent. The KiCad QA-corpus portions additionally skip cleanly if their corpus directory isn't present (`KICAD_QA_DATA`, default `qa/data` of the KiCad sources at `~/ws/kicad-src-8303b2ad`, else `$KICAD_SRC_DIR/qa/data` for the connectivity; `EDA_KICAD_QA_BOARDS` still wins when set) -- the examples/ladder-based measurements still run either way. `EDA_PARITY_PARTS=own,corpus,reexport` (a comma list) runs part of the round-trip harness and keeps the other parts' last results. `cargo test` (no `--ignored`) runs `crates/kicad/tests/parity_ratchet.rs`, which reads the committed `docs/parity/scores.json` below and fails if `current` has dropped below its recorded `floor` for any metric -- that test needs neither kicad-cli nor the QA corpus.

## Headline numbers

| metric | current | floor |
|---|---:|---:|
| `connectivity_boards_evaluated` | 186 | 186 |
| `connectivity_exact_match_rate` | 98.9% | 98.9% |
| `roundtrip_own_footprint_pose_survival_rate` | 100.0% | 100.0% |
| `roundtrip_own_segment_survival_rate` | 100.0% | 100.0% |
| `roundtrip_own_via_survival_rate` | 100.0% | 100.0% |
| `roundtrip_qa_pcb_import_success_rate` | 99.5% | 99.5% |
| `roundtrip_qa_sch_import_success_rate` | 100.0% | 100.0% |
| `roundtrip_reexport_fidelity_rate` | 100.0% | 100.0% |

## 1. Connectivity -- `eda_connectivity::analyze` vs KiCad's `unconnected_items`/`track_dangling`/`via_dangling`

_Measured 2026-10-24._

Boards evaluated: 186 (of 188 attempted).

Totals -- ours: {'unconnected': 287, 'track_dangling': 256, 'via_dangling': 529}, oracle: {'unconnected': 289, 'track_dangling': 254, 'via_dangling': 528}.

**Exact per-board-per-field match rate: 552/558 (98.9%).**


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


### Boards that errored (2)

- unroutable_tiny_outline [example]: place: [CheckResult { check: "place_legalize", status: Fail, location: Some("design"), hint: Some("could not resolve all courtyard overlaps within the outline; enlarge the board or relax rules"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("C1"), hint: Some("14235675000000 nm² outside its bound (keepout (-3050000, 75500, 50000, 4674500), bound (0, 0, 500000, 500000); non-connectors keep the edge margin)"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("C2"), hint: Some("14836840000000 nm² outside its bound (keepout (325500, -3050000, 4874500, 220000), bound (0, 0, 500000, 500000); non-connectors keep the edge margin)"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("U1"), hint: Some("78455000000000 nm² outside its bound (keepout (200000, 350000, 10200000, 8200000), bound (0, 0, 500000, 500000); non-connectors keep the edge margin)"), detail: None }]
- plugins/fakeboard.kicad_pcb [qa]: import_kicad_pcb failed: [CheckResult { check: "kicad_import.parse", status: Fail, location: Some("file"), hint: Some("not a valid s-expression file: expected '(' at byte 0"), detail: None }]

### The boards (188)

Each cell is ours / kicad-cli's; a **bold** cell differs. `unignored` names a check the board's project ignores, which the harness turns back on for kicad-cli.

| board | source | unconnected | track_dangling | via_dangling | unignored |
|---|---|---:|---:|---:|---|
| `all_power_ground_net` | example | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `dense_small_outline` | example | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `l1_usb_mcu` | ladder | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `l2_sensor_hub` | ladder | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `l3_motor_hub` | ladder | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `l4_control_hub` | ladder | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `ldo` | example | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `ldo_proximity_heavy` | example | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `mcu_board_30plus` | example | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `mixed_track_widths` | example | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `nc_pins` | example | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `opamp_filter` | example | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `passive_divider_ladder` | example | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `star_net` | example | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `through_hole_headers` | example | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `two_pin_nets` | example | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `unroutable_tiny_outline` | example | error | | | |
| `pcbnew/api_kitchen_sink.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/bad_triangulation_case.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/complex_hierarchy.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/component_classes.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/component_classes_drc.kicad_pcb` | qa | 0 / 0 | 4 / 4 | 0 / 0 |  |
| `pcbnew/connection_width_rules.kicad_pcb` | qa | 0 / 0 | 8 / 8 | 0 / 0 | track_dangling |
| `pcbnew/custom_fields.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/custom_pads.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/drc_missing_tuning_profile.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/fill_bad.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/footprints_load_save.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/graphics_load_save_v20240108.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/groups_load_save.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/groups_load_save_v20231212.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/incorrect_text_mirroring_drc.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/intersectingzones.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue10906.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue10916.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 1 / 1 |  |
| `pcbnew/issue11814.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue12109.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 1 / 1 |  |
| `pcbnew/issue12609.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue12831.kicad_pcb` | qa | 2 / 2 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue1358.kicad_pcb` | qa | 2 / 2 | 5 / 5 | 0 / 0 | track_dangling |
| `pcbnew/issue14008.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 1 / 1 |  |
| `pcbnew/issue14130.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue14294.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue14334.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue14412.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue14449.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue14549.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 | via_dangling |
| `pcbnew/issue14549_2.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue14559.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue15280.kicad_pcb` | qa | 4 / 4 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue16182.kicad_pcb` | qa | 16 / 16 | 20 / 20 | 0 / 0 |  |
| `pcbnew/issue16566.kicad_pcb` | qa | 0 / 0 | 2 / 2 | 0 / 0 |  |
| `pcbnew/issue17429.kicad_pcb` | qa | 35 / 35 | 91 / 91 | 2 / 2 |  |
| `pcbnew/issue17559.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue18.kicad_pcb` | qa | 0 / 0 | 2 / 2 | 0 / 0 |  |
| `pcbnew/issue18142.kicad_pcb` | qa | 0 / 0 | 2 / 2 | 0 / 0 | track_dangling, via_dangling |
| `pcbnew/issue18203.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue18839.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 0 / 0 | unconnected_items |
| `pcbnew/issue18878.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue19405_roundrect_teardrop.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 0 / 0 |  |
| `pcbnew/issue19574.kicad_pcb` | qa | 3 / 3 | 3 / 3 | 0 / 0 |  |
| `pcbnew/issue22102.kicad_pcb` | qa | **0 / 2** | 1 / 1 | 0 / 0 |  |
| `pcbnew/issue22536.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue22794.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue22798.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue23123_minimal.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue23339_zone_layer_rules.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue23690_via_in_pad.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue2512.kicad_pcb` | qa | 3 / 3 | 0 / 0 | 4 / 4 | unconnected_items, via_dangling |
| `pcbnew/issue2528.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue2568.kicad_pcb` | qa | 1 / 1 | 0 / 0 | 17 / 17 |  |
| `pcbnew/issue2904.kicad_pcb` | qa | 50 / 50 | 4 / 4 | 3 / 3 |  |
| `pcbnew/issue3812.kicad_pcb` | qa | 0 / 0 | 2 / 2 | 0 / 0 |  |
| `pcbnew/issue4139.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 0 / 0 |  |
| `pcbnew/issue4257.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 3 / 3 |  |
| `pcbnew/issue4774.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue5093.kicad_pcb` | qa | 0 / 0 | 15 / 15 | 0 / 0 |  |
| `pcbnew/issue5102.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 1 / 1 |  |
| `pcbnew/issue5313.kicad_pcb` | qa | 1 / 1 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue5320.kicad_pcb` | qa | 3 / 3 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue5567.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue5750.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue5830.kicad_pcb` | qa | 1 / 1 | 3 / 3 | 0 / 0 | track_dangling |
| `pcbnew/issue5854.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue5978.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue5990.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue6039.kicad_pcb` | qa | 3 / 3 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue6260.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue6284.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 7 / 7 |  |
| `pcbnew/issue6443.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue6879.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue6945.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue7004.kicad_pcb` | qa | 10 / 10 | 0 / 0 | 25 / 25 |  |
| `pcbnew/issue7086.kicad_pcb` | qa | 4 / 4 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue7241.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue7267.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 0 / 0 | track_dangling |
| `pcbnew/issue7325.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 | track_dangling, via_dangling |
| `pcbnew/issue7567.kicad_pcb` | qa | 1 / 1 | 0 / 0 | 0 / 0 | track_dangling, via_dangling |
| `pcbnew/issue7975.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue8003.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue832.kicad_pcb` | qa | 0 / 0 | 19 / 19 | 0 / 0 |  |
| `pcbnew/issue8407.kicad_pcb` | qa | 7 / 7 | 6 / 6 | 0 / 0 |  |
| `pcbnew/issue8883.kicad_pcb` | qa | 7 / 7 | 9 / 9 | 0 / 0 |  |
| `pcbnew/issue8909.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 1 / 1 |  |
| `pcbnew/issue9081.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/issue9870.kicad_pcb` | qa | 3 / 3 | 5 / 5 | 2 / 2 |  |
| `pcbnew/large_circle_teardrop.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 0 / 0 |  |
| `pcbnew/length_calculations.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 1 / 1 |  |
| `pcbnew/multinetclasses_drc.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/net_tie_drc.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/notched_zones.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/off_center_teardrop.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 0 / 0 |  |
| `pcbnew/orientation_drc.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/oval_teardrop.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 0 / 0 |  |
| `pcbnew/padstacks.kicad_pcb` | qa | 10 / 10 | 0 / 0 | 12 / 12 |  |
| `pcbnew/padstacks_complex.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/padstacks_normal.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/reference_images_load_save.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/reverse_via.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 1 / 1 |  |
| `pcbnew/severities.kicad_pcb` | qa | 0 / 0 | 3 / 3 | 0 / 0 | track_dangling |
| `pcbnew/skew_group_matched_drc.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/skew_within_diff_pairs_drc.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/sliver.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/solder_mask_bridge_test.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/soldermask_bridge_all_tracks.kicad_pcb` | qa | 0 / 0 | 2 / 2 | 0 / 0 |  |
| `pcbnew/soldermask_track_to_pad.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 0 / 0 |  |
| `pcbnew/stonehenge.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 39 / 39 |  |
| `pcbnew/teardrop_elongated_pad.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 0 / 0 |  |
| `pcbnew/teardrop_issue_JPC2.kicad_pcb` | qa | **1 / 3** | 5 / 5 | 0 / 0 |  |
| `pcbnew/teardrop_multi_inside_via.kicad_pcb` | qa | 0 / 0 | **6 / 4** | 1 / 1 |  |
| `pcbnew/teardrop_offcenter_two_segment.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 1 / 1 |  |
| `pcbnew/test_copper_graphics.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/test_starved_thermal.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/time_calculations.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/tracks_arcs_vias.kicad_pcb` | qa | 0 / 0 | 2 / 2 | 0 / 0 |  |
| `pcbnew/tuning_generators_load_save.kicad_pcb` | qa | 0 / 0 | 2 / 2 | 0 / 0 |  |
| `pcbnew/tuning_generators_load_save_v20231212.kicad_pcb` | qa | 0 / 0 | 2 / 2 | 0 / 0 |  |
| `pcbnew/two_segment_teardrop.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 1 / 1 |  |
| `pcbnew/via_dangling.kicad_pcb` | qa | 0 / 0 | 0 / 0 | **2 / 1** |  |
| `pcbnew/via_off_center.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `pcbnew/vme-wren.kicad_pcb` | qa | 57 / 57 | 1 / 1 | 92 / 92 |  |
| `pcbnew/zone_filler.kicad_pcb` | qa | 0 / 0 | 2 / 2 | 0 / 0 |  |
| `connect/connect.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `creepage/creepage.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `creepage_slots/creepage_slots.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `hatch_thermal_connectivity/hatch_thermal_connectivity.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 0 / 0 |  |
| `issue12964/issue12964.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 30 / 30 |  |
| `issue16658/issue16658.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue17967/issue17967.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue18045/test_proj.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue19090/issue19090.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue19325/issue19325.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue19956/issue19956.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue20480/issue20480.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue21184/issue21184.kicad_pcb` | qa | 1 / 1 | 0 / 0 | 2 / 2 |  |
| `issue21482/issue21482.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 199 / 199 |  |
| `issue21564/issue21564.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 0 / 0 |  |
| `issue21739/topology_mismatch.kicad_pcb` | qa | 7 / 7 | 0 / 0 | 0 / 0 |  |
| `issue21746/issue21746.kicad_pcb` | qa | 16 / 16 | 0 / 0 | 0 / 0 |  |
| `issue22010/issue22010.kicad_pcb` | qa | 19 / 19 | 0 / 0 | 80 / 80 |  |
| `issue22475/issue22475.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue22548/issue22548.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue22597/issue22597.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue22751/issue22751.kicad_pcb` | qa | 2 / 2 | 0 / 0 | 0 / 0 |  |
| `issue22809/issue22809.kicad_pcb` | qa | 0 / 0 | 2 / 2 | 0 / 0 |  |
| `issue22826/issue22826.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue22983/issue22983.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue23053/issue23053.kicad_pcb` | qa | 2 / 2 | 10 / 10 | 0 / 0 |  |
| `issue23332_min_width/issue23332_min_width.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue23389/issue23389.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue23418/testing.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue23418_diffnet/testing.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue23451/3Rs_bv.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue23467/issue23467.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue23469/issue23469.kicad_pcb` | qa | **5 / 4** | 0 / 0 | 0 / 0 |  |
| `issue23515/issue23515.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue23535_minimal/issue23535_minimal.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue23658/issue23658.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue23851/issue23851.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue24078/issue24078.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `issue8629/issue8629.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `keepout_disallow/keepout_disallow.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 0 / 0 |  |
| `keepout_no_clearance/keepout_no_clearance.kicad_pcb` | qa | 0 / 0 | 1 / 1 | 0 / 0 |  |
| `plugins/fakeboard.kicad_pcb` | qa | error | | | |
| `prettifier/group_and_image.kicad_pcb` | qa | 0 / 0 | 0 / 0 | 0 / 0 |  |
| `stacked/stacked.kicad_pcb` | qa | 8 / 8 | 0 / 0 | 0 / 0 |  |
| `unconnected-netnames/unconnected-netnames.kicad_pcb` | qa | **1 / 0** | 0 / 0 | 0 / 0 |  |
| `variant_test/variant_test.kicad_pcb` | qa | 2 / 2 | 0 / 0 | 0 / 0 |  |

## 2. Round-trips

### Our own pipeline (`design.json` -> `.kicad_pcb` -> `import_kicad_pcb` -> `design.json`)

_Measured 2026-10-24._

| board | footprints (pose-exact) | track segments (exact) | vias (exact) | zones before/after |
|---|---:|---:|---:|---:|
| all_power_ground_net | 3/3 | 18/18 | 0/0 | 0/0 |
| dense_small_outline | 5/5 | 18/18 | 0/0 | 0/0 |
| l1_usb_mcu | 17/17 | 157/157 | 3/3 | 0/0 |
| l2_sensor_hub | 35/35 | 408/408 | 19/19 | 0/0 |
| l3_motor_hub | 58/58 | 636/636 | 30/30 | 0/0 |
| l4_control_hub | 100/100 | 1258/1258 | 100/100 | 1/1 |
| ldo | 6/6 | 46/46 | 0/0 | 0/0 |
| ldo_proximity_heavy | 8/8 | 51/51 | 0/0 | 0/0 |
| mcu_board_30plus | 30/30 | 210/210 | 7/7 | 0/0 |
| mixed_track_widths | 6/6 | 48/48 | 0/0 | 0/0 |
| nc_pins | 3/3 | 23/23 | 0/0 | 0/0 |
| opamp_filter | 7/7 | 46/46 | 2/2 | 0/0 |
| passive_divider_ladder | 6/6 | 20/20 | 0/0 | 0/0 |
| star_net | 6/6 | 12/12 | 0/0 | 0/0 |
| through_hole_headers | 5/5 | 41/41 | 0/0 | 0/0 |
| two_pin_nets | 3/3 | 9/9 | 0/0 | 0/0 |
| unroutable_tiny_outline | ERROR: place: [CheckResult { check: "place_legalize", status: Fail, location: Some("design"), hint: Some("could not resolve all courtyard overlaps within the outline; enlarge the board or relax rules"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("C1"), hint: Some("14235675000000 nm² outside its bound (keepout (-3050000, 75500, 50000, 4674500), bound (0, 0, 500000, 500000); non-connectors keep the edge margin)"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("C2"), hint: Some("14836840000000 nm² outside its bound (keepout (325500, -3050000, 4874500, 220000), bound (0, 0, 500000, 500000); non-connectors keep the edge margin)"), detail: None }, CheckResult { check: "place_outside", status: Fail, location: Some("U1"), hint: Some("78455000000000 nm² outside its bound (keepout (200000, 350000, 10200000, 8200000), bound (0, 0, 500000, 500000); non-connectors keep the edge margin)"), detail: None }] | | | |

### KiCad QA corpus: `import_kicad_pcb` on all 185 real boards

_Measured 2026-10-24._

- imported ok: 184/185
- zones skipped (no polygon, or no copper layer): 2
- track arcs kept as arcs: 10946
- Edge.Cuts items kept as shapes (arcs, circles, rectangles, curves, cutouts; none turned into chords): 511
- non-rect pad shapes approximated as rect: 0
- boards whose outline didn't close into a loop: 3
- boards whose Edge.Cuts are malformed by KiCad's own test (`invalid_outline`): 3 ['pcbnew/issue18839.kicad_pcb', 'pcbnew/issue23658/issue23658.kicad_pcb', 'pcbnew/issue24078/issue24078.kicad_pcb']
- outline source breakdown: {'lines': 36, 'none': 37, 'shapes': 111}

Import failures (1):

- pcbnew/plugins/fakeboard.kicad_pcb: [CheckResult { check: "kicad_import.parse", status: Fail, location: Some("file"), hint: Some("not a valid s-expression file: expected '(' at byte 0"), detail: None }]

### KiCad QA corpus: `import_kicad_sch` on all 33 real schematics

- imported ok: 33/33
- sheets not descended into: 9
- unresolved symbols dropped: 0

### Re-export fidelity on 12 real QA boards (import -> our export -> kicad-cli DRC, vs kicad-cli DRC on the original)

_Measured 2026-10-24._

| board | original parses | re-export parses | identical violation-type counts | findings that differ (type, items, place) | where kicad-cli's counts differ (type: original -> re-export) |
|---|---|---|---|---:|---|
| api_kitchen_sink | True | True | True | 0 |  |
| bad_triangulation_case | True | True | True | 0 |  |
| complex_hierarchy | True | True | True | 0 |  |
| component_classes | True | True | True | 0 |  |
| component_classes_drc | True | True | True | 0 |  |
| connection_width_rules | True | True | True | 0 |  |
| custom_fields | True | True | True | 0 |  |
| custom_pads | True | True | True | 0 |  |
| drc_missing_tuning_profile | True | True | True | 0 |  |
| fill_bad | True | True | True | 0 |  |
| footprints_load_save | True | True | True | 0 |  |
| graphics_load_save_v20240108 | True | True | True | 0 |  |

Every re-export parses, and kicad-cli's DRC of it says what it says of the original: the same number of violations of each type and, finding by finding, the same type, severity, items and places (to 5 micrometres; the lengths in a
description are left out, because the importer keeps micrometres and an inch board's tenths of one are rounded). The slow test `a_real_boards_reexport_is_judged_by_kicad_cli_as_the_original_is` (`crates/kicad/tests/parity_roundtrip.rs`,
`EDA_SLOW_TESTS=1`) holds this for the 12 boards as the corpus ships them and as kicad-cli 10.99.0-5334 writes them in its newest file format (`EDA_PARITY_REEXPORT_UPGRADE=1` in the harness).

What the round trip changed before, read from the two kicad-cli reports and the two files of each board (`web/studio/PARITY-pcb.md` section 37 has the ports and the tests):

- `clearance`, `shorting_items`, `solder_mask_bridge`, `copper_edge_clearance` (`complex_hierarchy`): a board text of several lines (`Actionneur\nPiezo New Amp\nV02`) came back as one line full of backslashes -- the reader did not take the lexer's `\n` and the
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


## 3. Zone fill

The zone fill is KiCad's, made by kicad-cli (`kicad-cli pcb drc --refill-zones --save-board` on a copy of the derived board: Fill All Zones) and kept beside design.json; there is no other filler (docs/ARCHITECTURE.md, "Engines"; the proof is `crates/kicad-engine/tests/refill_zones.rs`). The connectivity comparison above gives the connectivity kicad-cli's own fill of the same board, so a board with a pour isn't penalized for a gap that's out of scope here.


## 4. UI parity -- KiCad actions vs the studio

Every KiCad action in `web/studio/src/kicad/actions.json` is classified against the studio's action registry by `web/studio/tools/ui-parity-audit.mjs` (it rewrites `docs/parity/UI-ACTIONS.md`). A "missing with a reason" action is one the studio deliberately does not wire, with the reason recorded in `web/studio/tools/ui-parity-missing.json`.

| editor | actions | handled | referenced | missing | missing with a reason | hotkeyed & not handled |
|---|---:|---:|---:|---:|---:|---:|
| pcbnew | 369 | 346 | 0 | 0 | 23 | 0 |
| eeschema | 233 | 220 | 0 | 0 | 13 | 0 |
| common | 183 | 172 | 0 | 0 | 11 | 2 |

Behavior (not just the presence of an action) is tracked per feature in `web/studio/PARITY-pcb.md` and the ranked, actionable gaps in `docs/parity/GAPS.md`.

