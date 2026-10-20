# agentic-eda vs KiCad -- Parity Report

_Generated 2026-10-20T07:33:23.689533+00:00 by `tools/parity_report.py`._

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
| `roundtrip_reexport_fidelity_rate` | 100.0% | 100.0% |

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

_Measured 2026-10-12._

- imported ok: 184/185
- zones skipped (no polygon, or no copper layer): 12
- track arcs kept as arcs: 10946
- Edge.Cuts items kept as shapes (arcs, circles, rectangles, curves, cutouts; none turned into chords): 513
- non-rect pad shapes approximated as rect: 0
- boards whose outline didn't close into a loop: 1
- boards whose Edge.Cuts are malformed by KiCad's own test (`invalid_outline`): 2 ['pcbnew/issue18839.kicad_pcb', 'pcbnew/issue24078/issue24078.kicad_pcb']
- outline source breakdown: {'lines': 36, 'none': 37, 'shapes': 111}

Import failures (1):

- pcbnew/plugins/fakeboard.kicad_pcb: [CheckResult { check: "kicad_import.parse", status: Fail, location: Some("file"), hint: Some("not a valid s-expression file: expected '(' at byte 0"), detail: None }]

### KiCad QA corpus: `import_kicad_sch` on all 33 real schematics

- imported ok: 33/33
- sheets not descended into: 9
- unresolved symbols dropped: 0

### Re-export fidelity on 12 real QA boards (import -> our export -> kicad-cli DRC, vs kicad-cli DRC on the original)

_Measured 2026-10-20._

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

What the round trip changed before, read from the two kicad-cli reports and the two files of each board (`web/studio/PARITY-pcb.md` section 34 has the ports and the tests):

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
outline's hatch pitch are not kept; nor are dimension details (text position mode, extension height); reference images and text boxes are written back as text, drawn by kicad-cli's plots and not by the canvas, with no editor (P16); KiCad 10's line ending
shapes, grid items, via stitching and guarding, microvia stack generators, custom user properties and drill charts are not read (no board of the QA corpus or of the installed KiCad's templates has one; a top-level item of a kind nobody reads is carried as text).
The zone fills are KiCad's: the harness exports unfilled zones and `--refill-zones` fills them, the studio keeps the file's own beside `design.json`.


## 3. Zone fill

The zone fill is KiCad's, made by kicad-cli (`kicad-cli pcb drc --refill-zones --save-board` on a copy of the derived board: Fill All Zones) and kept beside design.json; there is no other filler (docs/ARCHITECTURE.md, "Engines"; the proof is `crates/kicad-engine/tests/refill_zones.rs`). The connectivity comparison above gives the connectivity kicad-cli's own fill of the same board, so a board with a pour isn't penalized for a gap that's out of scope here.


## 4. UI parity -- KiCad actions vs the studio

Every KiCad action in `web/studio/src/kicad/actions.json` is classified against the studio's action registry by `web/studio/tools/ui-parity-audit.mjs` (it rewrites `docs/parity/UI-ACTIONS.md`). A "missing with a reason" action is one the studio deliberately does not wire, with the reason recorded in `web/studio/tools/ui-parity-missing.json`.

| editor | actions | handled | referenced | missing | missing with a reason | hotkeyed & not handled |
|---|---:|---:|---:|---:|---:|---:|
| pcbnew | 350 | 289 | 0 | 0 | 61 | 4 |
| eeschema | 233 | 174 | 0 | 0 | 59 | 8 |
| common | 183 | 150 | 0 | 0 | 33 | 2 |

Behavior (not just the presence of an action) is tracked per feature in `web/studio/PARITY-pcb.md` and the ranked, actionable gaps in `docs/parity/GAPS.md`.

