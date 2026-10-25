# Nanometres in the IR (task-nanometres, 2026-10-23)

The user's decision of 2026-10-23: full parity, so the lengths of the design file go from micrometres to the integer nanometres KiCad holds.
This is the plan, written before the code changed; "Result" at the end says what landed.

## Why

`pub type Um = i64` (`crates/model/src/ir.rs:18`) rounded every KiCad nanometre to a micrometre. That cost:

- sub-µm segments: 6 of the 186 connectivity boards (`issue17967`, `vme-wren`, ... in `docs/parity/REPORT.md`);
- imperial drills: 0.3048 mm stored as 0.305;
- the library round trip: 70 of 164 KiCad footprints up to 0.5 µm off (`crates/kicad/tests/library_roundtrip.rs`);
- table cells about 1 µm off (`crates/kicad/tests/kicad10_items.rs`);
- the tolerances written to hide those.

## Key decisions

1. **One unit, `Nm`, everywhere in the IR.** `pub type Nm = i64` (nanometres), in `crates/model/src/units.rs`, re-exported as `eda_model::ir::Nm`. `Um` is removed: nothing in the IR is a micrometre. The few things that truly are micrometres keep a name that says so (the Specctra DSN's `(resolution um 10)`, FreeRouting's 0.1 µm board unit, the intent file's v1 unit; see below).
2. **The schematic side holds nanometres too, on KiCad's 100 nm grid.** KiCad's schematic internal unit is 100 nm (`include/base_units.h`: `SCH_IU_PER_MM = 1e4`, "Schematic internal units 1=100nm", in both 8303b2ad and 25ec2335, the box's kicad-cli), the board's 1 nm (`PCB_IU_PER_MM = 1e6`).
   - We keep one unit in the IR rather than two (a symbol placed on a sheet, a sheet shared by both editors, cross-probing and the clipboard all mix the two today, and a second unit type would be a second source of x100 mistakes).
   - The schematic reader rounds as KiCad's does, to its unit: `KiROUND( mm * schIUScale.IU_PER_MM ) * 100` nm (`sch_io_kicad_sexpr_parser.cpp::parseInternalUnits`). So every schematic length read from a file is a whole number of 100 nm, exactly what KiCad holds.
   - The schematic writer formats as KiCad's does: `FormatInternalUnits( schIUScale, KiROUND( nm / 100 ) )`, i.e. at most four decimals of a millimetre. A schematic length our own code computes (a division by two, a rotation) is rounded to KiCad's grid when written, as KiCad itself would hold it.
   - What can still differ: KiCad does its schematic arithmetic in 100 nm integers, we do it in 1 nm integers, so an integer division (centring) can land up to 50 nm apart before the writer rounds it. Listed under "What still differs".
   - Library symbols (`LibrarySymbol*`, `symbol::SPoint`) are millimetres in `f64` today, as the `.kicad_sym` file writes them, not micrometres; they had none of the µm IR's rounding and stay as they are (renaming them is not part of this change).
3. **Conversions live at boundaries only, and are KiCad's.** `crates/model/src/units.rs` holds the scales and the exact conversions, ported from `base_units.h` / `eda_units.cpp`:
   - `PCB_IU_PER_MM = 1e6`, `SCH_IU_PER_MM = 1e4`, `NM_PER_MM`, `NM_PER_MIL = 25_400`, `NM_PER_UM = 1000`, `SCH_IU_NM = 100`;
   - `from_mm(f64) -> Nm`: `KiROUND( std::clamp( mm * IU_PER_MM, -INT_LIMIT, INT_LIMIT ) )` (`pcb_io_kicad_sexpr_parser.cpp::parseBoardUnits`), half away from zero (`std::llround`);
   - `sch_from_mm(f64) -> Nm`: the schematic's, above;
   - `to_mm(Nm) -> f64`;
   - `format_mm(Nm) -> String`: `EDA_UNIT_UTILS::FormatInternalUnits( pcbIUScale, v )` exactly (`{:.10g}`, and `{:.10f}` with the zeros taken off at or below 0.0001 mm);
   - `sch_format_mm(Nm) -> String`: the same over `schIUScale` after `KiROUND( nm / 100 )`.
   The board/footprint reader and writer go through `from_mm` / `format_mm`, the schematic and symbol ones through the `sch_` pair. KiCad files read and written are then exact: a `.kicad_pcb` read and written back gives the same numbers, character for character.
4. **A format version in `design.json`.** `Design::schema` exists (always 1, never read). It becomes the version: `SCHEMA_VERSION = 2` means nanometres. A file whose `schema` is missing or below 2 is migrated on load, every length x1000, exactly once (see "Migration"). Saving always writes 2. The migration is one way: a board saved by this code and read by an older binary would read every length 1000x too big. Old code reading a new file is not supported (the user's decision); `docs/HANDOFF.md` says so.
5. **Names say the unit.** Every `_um` / `Um` / `um_` name becomes `_nm` / `Nm` / `nm_`, in Rust and in the studio's TypeScript (`sizeUm` -> `sizeNm`, `umTo` -> `nmTo`, `UM_PER_MM` -> `NM_PER_MM`). A serialized field renamed this way keeps its old key as a `#[serde(alias = "..._um")]`, so a v1 file still reads; a v2 file writes the new key. The type of a value that was typed `Um` but is not a length gets its own name: `TuningPattern::target_delay` / `target_skew_delay` (attoseconds), `TableText::line_spacing_permille` (permille).

## The boundaries

Each is changed to convert exactly, or to stop converting where the other side was already nm.

| Boundary | Where | Today | After |
|---|---|---|---|
| `.kicad_pcb` / `.kicad_mod` reader | `crates/kicad/src/import.rs::mm_to_um` (~158 calls), `pcb_table.rs::um`, `import_items.rs::um_of`, `pcb_generated.rs::prop_um`, `footprint_lib.rs`, ... | `(mm*1000).round()` | `units::from_mm` (KiCad's `parseBoardUnits`) |
| `.kicad_pcb` / `.kicad_mod` writer | `crates/kicad/src/lib.rs::mm` (~300 calls), `fmt_mm_f` on lengths, `.kicad_pro` closures (`pcb.rs:652`, `:796`, `kicad-engine/src/lib.rs:569`) | um/1000 to 4 decimals | `units::format_mm` (`FormatInternalUnits`) |
| `.kicad_sch` / `.kicad_sym` reader and writer | `sch_import.rs`, `sch_extras_io.rs`, `sch_table_io.rs`, `label_fields.rs`, `sch_clipboard.rs`, `symbol_lib.rs` (lengths only) | um | `sch_from_mm` / `sch_format_mm` (100 nm) |
| `.kicad_dru` | `crates/drc/src/rule_editor.rs`, `custom_rules.rs` | already nm inside (`to_nm`) | the x1000 bridges go |
| zone fills | `crates/kicad/src/zone_fills.rs` (`mm_to_nm`, `fmt_nm`, `nm_to_um`) | nm read, rounded to um | `nm_to_um` goes: the IR keeps KiCad's rings exactly |
| Specctra DSN/SES | `crates/kicad/src/specctra/*` | nm inside, x1000 at the IR (`shape_in_nm`, `session.rs::um`) | the bridges go; the DSN's own `(resolution um 10)` stays micrometres, named so |
| kicad-cli JSON (DRC, ERC, board stats, drill charts) | `crates/kicad-engine/src/lib.rs::to_um` / `to_nm`, `crates/cli/src/kicad_engine.rs::quantity_um` | um | nm |
| DXF/SVG graphics import | `crates/kicad/src/dxf_import.rs`, `svg_import.rs`, `crates/ops/src/import_graphics*.rs` | mm f64 -> um | mm f64 -> nm (`from_mm`) |
| intent YAML | `ConstraintModel` (`crates/model/src/lib.rs`) | um integers, no version | v1 (no `schema`) is um, read x1000; what we write gets `schema: 2` (nm) |
| the CLI's arguments | `crates/cli/src/main.rs`, `board.rs` (`--at 17.5,17.5`, `--width 0.2`, `--pts`) | mm f64 -> um | mm -> nm |
| messages | every `x as f64 / 1000.0` that prints millimetres | /1000 | `units::to_mm` |
| the studio's API | `crates/cli/src/*_api.rs`, `studio.rs`; `/api/inspect/clearance` takes mm | um JSON | nm JSON |
| the studio (web/studio/src) | `state/units.ts` (`UM_PER_MM`), `api/types.ts` (`type Um`), `kicad-port/pcbGeom.ts` (`IU_PER_UM`, `toIU`, `fromIU`), `kicad-port/view.ts` (`MIN_SCALE`, `MAX_SCALE`), `kicad-port/zoomFit.ts`, grids (`grid.ts`, `gridSettings.ts`), hit-test floors (`Math.max(150, 6/scale)`), default widths, 3D (`scene.ts` `/1000` -> mm), library symbols (`transform.ts` `*1000`) | um | nm; the bridges to KiCad's nm (`toIU`) become the identity and go |
| FreeRouting port | `crates/freeroute/src/design.rs` (`UNITS_PER_UM = 10`) | board unit 0.1 um | the same board unit, from nm (`/100`) |
| Bookshelf / DREAMPlace | `crates/interchange/src/bookshelf.rs` (`unit_um`) | um per unit | nm per unit |

The router (`crates/pns`) already computes in nanometres inside: `kigeom::to_nm` / `to_um`, `arcs.rs`, `direction45.rs`, `dp_meander_placer.rs`, `meander_placer.rs`, `mouse_trail.rs` convert at its edge. Those go, and the tolerances KiCad states in nm that were restated for a µm grid go back to KiCad's values (`diff_pair.rs`: `checkGap`'s 100 nm slack, the gateway's 5 nm alignment, `BuildGeneric`'s +-100 nm). The DRC (`crates/drc`: `constraints.rs::nm`, `rule_board.rs::to_nm`, `via_stitch.rs::nm` / `um_of`, `outline.rs`) likewise. `crates/model/src/table_text.rs::nm`, `crates/ops/src/microwave.rs::nm` / `um_of`, `crates/pns/src/kigeom.rs::NM_PER_UM` are the other bridges.

## Constants

Every length constant is restated in nm, by name: grids (`GRID = 1270` -> `1_270_000`, the studio's `DEFAULT_PCB_GRIDS`, `EESCHEMA_GRIDS`), default widths and clearances (`rules.rs`, `ir.rs` zone defaults, teardrop defaults, `DEFAULT_FIELD_SIZE`, `DEFAULT_TEXT_*`, `pcb_items::DEFAULT_POINT_SIZE`, page limits, `LENGTH_UNCONSTRAINED`), stub lengths, the placement gates' thresholds (`crates/gates`, `crates/lint`, `crates/place`), the router grid (`crates/grid`), the renderers' scales (`crates/render`, `crates/judge`) and the studio's hit-test floors and zoom limits. `Zone::min_island_area` is an area: x1e6. Constants that KiCad states in nm and we rounded to µm get KiCad's value back (for example `pcbPointEdit`'s 1 mil minimum, 25 400 nm instead of 26 µm).

## Overflow

Coordinates up to 1e9 nm (1 m) give one product up to 1e18, below i64's 9.2e18; a triple product does not fit. The audit (read-only, grep-based) found every exact triple product already in i128 (`kimath` / `kigeom` `rescale`, segment intersection, projections) or f64 (circle through three points, line intersections, Clipper2). To widen, as KiCad's `ecoord`/`rescale` does:

- `crates/connectivity/src/cleanup.rs:401` (an i64 cross product used as a predicate) -> i128;
- `crates/pns/src/shove.rs:174` (an i64 sum of squares) -> i128;
- area sums in i64: `crates/engine/src/lib.rs:861`, `:993`, `crates/engine/src/fields.rs:869` -> i128 or f64;
- `crates/drc/src/shape_poly.rs:28,41,68` (`r as i32`) -> clamped like the ops callers;
- `pns/src/meander.rs` `as i32` narrowings copy KiCad's `int` on purpose (already nm).

Every other `.pow(2)` / `x*x + y*y` on coordinate differences stays i64, as `SEG::ecoord` does. The web's areas in nm^2 exceed 2^53 (a 300 mm board: 9e16): areas are shown in mm^2 and computed in f64, as today.

## Migration

### design.json and its history

- `Design` gets a hand-written `Deserialize` (the derived one kept as `#[serde(remote = "Self")]`): the document is read into a `serde_json::Value`, its `schema` read (missing = 0), and the derived reader runs with a thread-local **length scale** of 1000 when `schema < 2`, 1 otherwise. Every length field carries `#[serde(deserialize_with = "crate::units::de")]`, which multiplies what it read by the scale (`units::Lengths` is implemented for `i64`, `Option`, `Vec`, tuples and arrays of them); `Point`'s `x` and `y` carry it, so every point, outline and polyline follows. `Zone::min_island_area` (µm^2) carries `units::de_area` (scale squared). A field absent from an old file takes its serde default, which is already nm, so nothing is scaled twice and nothing is missed by being absent.
- After reading, `schema` is set to 2, and `Design`'s `Serialize` always writes `SCHEMA_VERSION`: a design in memory is always nm, so every save writes 2, and a second load of what was saved never scales again.
- Reading through a `Value` also sidesteps the `arbitrary_precision` problem with tagged enums (checked: an internally tagged enum with an `f64` field reads from a `Value` with the feature on, and fails from a string).
- The completeness of the annotations is checked three ways: a source test in `crates/model` (every `Nm`-typed field of a `Deserialize` type in the crate has `units::de`); a unit test per kind of length field (points, widths, sizes, drills, zone outlines, every overlay); and, during the work, the design files the OLD binary made from KiCad's QA boards and schematics compared leaf by leaf with the new binary's: every number equal or x1000 (within the old rounding), and the keys of each kind reviewed by hand.
- **`.history/`** holds whole designs (`head.json`, `undo/*.json`, `redo/*.json`, `pending/design.json`), each with its own `schema`, and every reader of them goes through `Design`'s reader, so each snapshot is migrated when it is read, on its own stamp. Undo across the migration restores an old snapshot as nm, and `restore_domain` overlays nm on nm: units never mix. The outside-edit check compares raw bytes of `head.json` and `design.json`; an old pair is still byte-equal (no false "file" step), and the first save writes both new.
- No file is rewritten just because it was read: a board opened and closed without an edit stays as it was on disk (still readable by the old binary).

### The other files

| File | Lengths | Migration |
|---|---|---|
| `board.json` (`Meta`) | `snap_um`, `spacing_um` | renamed `snap_nm`, `spacing_nm`; the old keys are read x1000 |
| `view.json` | `center` (µm), `scale` (px per µm) | gets `"units": "nm"`; one without it has `center` x1000 and `scale` /1000 when read (`view_api::get`) |
| `appearance.json` | `project.viewports[]` `{x, y, w, h}` (µm) | `VERSION` 1 -> 2; a version-1 file's viewports x1000 when read |
| intent YAML (`ConstraintModel`) | `board.grid`, `track_width`, `clearance`, `outline`, net classes, footprints (an imported board's intent holds its footprints) ... | `ConstraintModel` gets `schema` like `Design`: absent (every hand-written intent and `examples/*.yaml`) is v1, micrometres, read x1000 every time; what our code writes (`eda import-kicad`, `imports/*.intent.yaml`) carries `schema: 2` and nm |
| `.kicad/zone_fills.json` | KiCad's nm rings (already), and a signature over the zone's fields | `KicadFills::SCHEMA` 2 -> 3; a schema-2 cache's signatures are compared with the zone's fields taken back to µm, so a migrated board keeps its fills (and the same DRC) |
| `activity.jsonl` | mm text | none (it is text, and stays right) |
| browser `localStorage` (`eda-studio.grid-settings`, `grid-overrides`) | grid sizes in µm | read under a new key; the old key's sizes x1000 once |
| `EDA_EPISODE` / `EDA_VIEW` recordings | µm | none (debug recordings) |

### The migration test

- Unit tests in `crates/model` (`tests/nanometre_migration.rs`): a v1 document per kind of length field, read, every length x1000 and nothing else changed; a v2 document read unchanged; read, written and read again never scales twice.
- `crates/cli`: a board directory written by the OLD binary (`eda board new examples/mcu_board_30plus.yaml`, placed and routed, two zones filled, with undo snapshots) committed as a fixture; the new code loads it, every length x1000, undo goes back across the migration in nm, and the outside-edit check makes no false step.
- By hand on copies (never `work/`): the same board saved by the OLD binary, loaded by the new one: the same `.kicad_pcb` numbers and the same kicad-cli DRC.

## Tests

Literal lengths in tests are restated in nm. No assertion is loosened. The tolerances the µm IR needed are removed and listed under "Result: tolerances removed".

## Risks

1. A length field the migration misses: an old board loads with that length 1000x too small. Covered by the source test, the per-kind tests and the old/new binary comparison.
2. A non-length scaled by mistake (a count, an angle, an index typed `i64`): the same comparison, both ways.
3. A `/ 1000` or `* 1000` meaning mm left in place: lengths 1000x off in a message, a dialog or a file. Every `1000`, `1_000`, `1e3`, `25.4`, `254`, `127`, `1270`, `2540`, `635` in the crates and the studio is reviewed, not replaced blindly (angles are millidegrees, time is milliseconds, ratios are permille).
4. Overflow in nm (above).
5. Float precision in the studio: Canvas2D runs in doubles; float32 inside the browser is relative, so x1000 coordinates lose nothing. Zoom limits and hit-test floors are restated.
6. The schematic's 100 nm grid against our 1 nm arithmetic (decision 2).
7. Old snapshots read by something that does not go through `Design`'s reader (a `Value` read of `design.json`): the tests that read the file as JSON are updated, and nothing in production does.
8. Sheer size: about 170 Rust files use `Um`, about 270 TypeScript files touch lengths. Done crate by crate, in dependency order, each step committed with `cargo check` clean.

## Order of work

1. This plan.
2. `crates/model`: `units.rs`, the renames, the migration, the model's constants and tests.
3. `crates/kicad` (readers, writers, specctra, zone fills), `crates/drc`, `crates/pns`, `crates/engine`, `crates/ops`, the rest of the crates, `crates/cli`.
4. `web/studio`.
5. The gates: `cargo check` (no warnings), `tools/check.sh fast`, the slow tier, `tools/parity_report.py` (12/12, connectivity at least 97.8%, the floor raised), `tools/run-all-checks.sh -j 3` (42/42), the migration test.

## Result

Landed on `task-nanometres` (2026-10-23 to 10-24). The plan held; what follows is what came out of it.

### Decisions as built

- One unit, `Nm = i64`, in the whole IR, the schematic included. The schematic reader and writer round to KiCad's 100 nm unit (`sch_from_mm`, `sch_format_mm`); our arithmetic between them is in 1 nm.
- `design.json` `schema: 2` is nm. A missing or lower `schema` is migrated on read (x1000 per length field, x1e6 for `Zone::min_island_area`), by the thread-local length scale under `Design`'s reader. Every save writes 2.
- `.history/` snapshots are migrated on their own stamp when read, so undo and redo across the migration land in nm. A board only read is not rewritten.
- `board.json` (`snap_nm`, `spacing_nm`; the old `_um` keys x1000), `view.json` (`"units": "nm"`), `appearance.json` (`VERSION` 2) and the intents (no `schema` = µm) are migrated the same way. So are `.kicad/zone_fills.json` (schema 3; a schema-2 cache's signatures compared in µm, so a migrated board keeps its fills) and the studio's `localStorage` grids (a new key).

### The migration test

- `crates/model/tests/nanometre_migration.rs`: one v1 document per kind of length field. Each is x1000, a v2 document is unchanged, and read-write-read never scales twice.
- `crates/cli/tests/nanometre_migration.rs`, on the fixture `crates/cli/tests/fixtures/old_board_um/`. The fixture is a board the old binary (be562f2) made from `examples/opamp_filter.yaml`, placed and routed, with a pour, a text and three undo snapshots.
  - `board status` leaves `design.json`'s bytes alone.
  - Undo gives the old snapshot x1000 (schema 2), and redo gives the old head x1000.
  - Two undos and a redo walk the snapshots in nm.
  - In the slow tier, `eda board drc` gives the same kicad-cli findings, with the same positions in nm, as the old binary gave in µm (`old_board_um.drc.json`).

### Tolerances removed

| Where | Was (µm IR) | Now |
|---|---|---|
| `model/kicad_geom.rs` label and pin text boxes | 0.2 µm | 1 nm (exact on the 100 nm grid) |
| `model/rules.rs` stackup thickness | 2 µm | exact |
| `model/table_text.rs` cell text | 1.5 µm | exact |
| `model/fp_exchange.rs` pads of a footprint turned to the back | 2 µm | 2 nm |
| `model/outline.rs` quarter arc: radius, centre | 0.8 µm, 0.5 µm | 1 nm |
| `engine/fields` sheet field margins (712/585 µm), label anchors | 1 µm, 0.2 µm | KiCad's 711.6/584.6 µm within 1 nm; 1 nm |
| `drc/hatch.rs` sides of the box round a turned text | 2 µm | 2 nm |
| `drc/pad_shape.rs` the probe's gap to a custom pad's ring | 1 µm | 1 nm |
| `pns` board edge: the router's clearance epsilon | 1 µm | KiCad's DRC epsilon, 500 nm (499_499 collides, 499_500 is clear) |
| `pns/diff_pair.rs` `checkGap` slack; the gateway's alignment | 1 µm; 1 µm | KiCad's 100 nm; KiCad's 5 nm (`make_gap_vector` is KiCad's: 106_066) |
| `pns` direction45 oracle: arc mid | 1 µm | exact |
| `pns` qa_regressions recorded segments | 3 µm | 10 nm |
| `pns` pair pitch (router, dp_boards) | -1/+3 µm | +-10 nm |
| `pns` dp_placer gap; pad clearance; via to pad | 1.5 µm; 1 µm; 2 µm | 100 nm; 500 nm; 500 nm |
| `pns/diff_pair.rs` coupled length and skew | 1 µm | 1 nm |
| `pns/arcs.rs` polyline radius | 1.6 µm | 1 µm (the flattening error) |
| `kicad-engine` tables: text ink vs kicad-cli's plot; table borders | 8 µm; 3 µm | 1 µm (measured 0.7); 100 nm |
| `ops` rotation in 15 degree presses | 1 µm per press | 1 nm per press |
| `ops/pcb_tables.rs` cell margins | 1003 µm (rounded) | KiCad's 1_002_500 nm exact |
| `kicad` tests: text thickness, outline width, a legacy arc's mid | 0.305, 0.203, 18.536 mm | the file's 0.3048 and 0.2032, and KiCad's 18.535534 |
| `cli` microwave arc stub | 1.061 mm | 1.06066 mm (as KiCad) |
| `cli` Altium 4 mil / Eagle 12 and 8 mil rules | 102 / 305 / 203 µm | 101_600 / 304_800 / 203_200 nm |

Two tolerances stay or got looser, each for a reason:
- `ops` convert polygonises at KiCad's `m_MaxError` (5 µm; the port used 1 µm), so a circle's polygon area is now within 0.2 % (was 0.1 %).
- `pns/tests/dp_boards.rs`, the cursor sweep: a coupled stretch's gap keeps its old 1.5 µm slack (1500 nm, not `checkGap`'s 100 nm). A 45 degree pair's offsets round to about 1.2 µm short of the gap (walkaround, the J1 sweep).

### Bugs found on the way (non-test code)

- `kicad/barcode_io.rs`: the default size was 40 µm (KiCad: 40 mm) and the default text 1.5 µm.
- `kicad/pcb_textbox.rs`: the legacy text size was 1.524 µm.
- `kicad/label_fields.rs`: the field size was 1.27 µm.
- `kicad/sch_table_io.rs`: a cell's default text was 1.27 µm.
- `kicad/via_stitch_io.rs`: the writer's origin offset and pitch were /1000.
- `ops/sch_net_collision.rs`: `MIN_RADIUS` was 800 µm, where KiCad's is 800 schematic IU, i.e. 80 µm.
- `ops` `place_beside`: a stand-off exactly half way between two grid points now takes the one nearer the anchor. The tie only exists in nm.
- `place`: thresholds in µm (`+ 500`, `.max(6_000)`, `snap.max(100)`).
- `pns/from_ir.rs`: KiCad keeps an Edge.Cuts arc a `SHAPE_ARC` (`syncGraphicalItem`), and ours were chords up to 5 µm inside it. A route on the arc's outer side came 2.1 µm under the edge clearance for kicad-cli (issue12609). A chord of a flattened curve now has the flattening error as its radius.
- The studio:
  - Board Setup's tuning-pattern fields wrote `_um` keys.
  - The text-size bounds of the schematic dialogs were in µm.
  - The barcode default was in µm.
  - The dimension's `extension_height` default is now 586_421 nm (arrow length x sin 27.5 degrees).
  - The context menu's default widths were in µm.
  - The symbol editor's painter and snaps took mm as µm.
  - The reference image's 25.4 mm was wrong.
- `bench` corpus: `opamp_filter` completes cleanly at every seed, with the old binary too. It is promoted to Clean, as `corpus.rs` asks.

### Before and after

- `tools/parity_report.py`:
  - connectivity went from 546/558 (97.8%) to 552/558 (98.9%) per board and field, and the floor is raised to 98.9%;
  - the sub-µm segments of `vme-wren` (199 -> 1 dangling, as KiCad), `issue17967`, `issue18045`, `issue22826`, `issue6284` and `issue9870` now match;
  - re-export 12/12 (`finding_differences` 0 on each);
  - pcb import 184/185, sch 33/33 (unchanged).
- The library round trip (`crates/kicad/tests/library_roundtrip.rs`, kicad-cli): the sampled 87 footprints and 99 symbols come back identical, 0 differing. Before, 70 of 164 footprints were up to 0.5 µm off.
- Imperial drills are exact (0.3048 mm).

### What still differs from KiCad

- The schematic: KiCad does its arithmetic in 100 nm integers and we do ours in 1 nm, so a centring division can land up to 50 nm apart before the writer rounds it to KiCad's grid.
- Convert (`ops`): chaining shapes is exact here, where KiCad joins within 100 nm.
- The router's Edge.Cuts curves are chords with a 5 µm band, not `SHAPE_ARC`s: near a curved edge the router keeps up to 5 µm more than the clearance.
- The `dp_boards` sweep's coupled-gap slack (above).
- Six connectivity boards still differ: `issue22102`, `teardrop_issue_JPC2`, `teardrop_multi_inside_via`, `via_dangling`, `issue23469`, `unconnected-netnames` (`docs/parity/REPORT.md`).
- Known and not from this change:
  - a via drag on the `simple` board;
  - `crates/render`'s `to_abs` mixes mm and nm in one place;
  - `connectivity`'s ignored `kicad_cli_ratsnest_oracle` fails on `l1_usb_mcu`'s footprint, which place cannot find. It fails on main too.
- Some unit tests that were self-consistent at a thousandth of their size are left so where the geometry does not care: `freeroute` directions, and `pns` direction45, line, line_walk and hull.

### Gates (2026-10-24, on the box)

1. `cargo check --workspace --tests --keep-going`: no warnings.
2. `tools/check.sh fast` (4m44s): Rust (debug) 3642 passed, 0 failed, 25 ignored (main: 3627); web typecheck ok, 2449 unit tests.
3. The slow tier: every crate in release, with `EDA_SLOW_TESTS=1`, `KICAD_QA_DATA=$KICAD_SRC_DIR/qa/data` (the pns replays run) and `EDA_KICAD_CLI=/usr/local/bin/kicad-cli`. 3642 passed, 0 failed.
   - The `#[ignore]`d kicad-cli tests also pass: `kicad_cli_drc`, `kicad_cli_erc`, `kicad_cli_import`, and `hier_erc` (11 in all).
   - `kicad_cli_ratsnest_oracle` fails, as it does on main (above).
   - `EDA_KICAD_CLI` is needed on Linux by the GLB export test: `studio::kicad_cli_path` defaults to the macOS bundle.
4. `tools/parity_report.py`: re-export 12/12, connectivity 98.9% (552/558), and the floor raised to it.
5. `tools/run-all-checks.sh -j 3`: 42/42.
6. The migration test (`crates/cli/tests/nanometre_migration.rs`, `crates/model/tests/nanometre_migration.rs`): passes, fast and slow.

## The length type (task-length-newtype, 2026-10-24)

The user's decision of 2026-10-24: lengths are a type of their own, so the compiler catches unit mixing. A tuner default board thickness written
`1_600` (1.6 µm) for `1_600_000` was the bug of this kind that prompted it: with `Nm = i64` nothing stopped a millimetre float, a micrometre literal
or a count from being taken for a length.

### `eda_model::units::Nm`

`#[repr(transparent)]` over `i64`, `#[serde(transparent)]`: `design.json`, the intents, the API's JSON and `schema` 2 are the bytes they were, and the
schema-1 x1000 migration (`units::de`, `Lengths`) reads into it as it read into the `i64`.

| | |
|---|---|
| build | `Nm::new(i64)` (a count of nanometres, `const`), `Nm::from_mm(f64)` (KiCad's `parseBoardUnits`: `KiROUND` of the clamp to `INT_LIMIT`), `Nm::kiround(f64)` (a length computed in floating-point nanometres), `units::mm(f64)` / `um(f64)` / `mil(f64)` (a typed or test value, rounded half away from zero, no clamp), `Nm::ZERO`, `Nm::MAX`, `Nm::MIN` |
| read | `nm.get()` (the count), `nm.mm()`, `nm.um()`, `nm.mils()`, `nm.as_f64()` (nanometres as a float, for geometry), `a.ratio(b)` (`a / b` as a float), `a.area(b)` (nm^2 as an `i128`) |
| arithmetic | `Nm ± Nm` (also by reference), `-Nm`, `Nm * i64`, `i64 * Nm`, `Nm / i64`, `Nm % Nm`, `Nm / Nm -> i64` (a count, truncated), `+=`, `-=`, `*= i64`, `/= i64`, `Sum`, `abs`, `signum -> i64`, `min` / `max` / `clamp` (`Ord`), `rem_euclid(Nm) -> Nm`, `div_euclid(Nm) -> i64`, `checked_add` / `_sub`, `saturating_add` / `_sub`, `scale(f64)` (`KiROUND( v * k )`), `sch_round()` |
| not there | `Nm * Nm` (an area is not a length: `area`), `Nm + i64`, `as` casts, `From<i64>` (no silent conversion either way) |
| printed | `Debug` and `Display` print the bare count, as the `i64` did (messages, `{:?}` keys and snapshots are unchanged); `FromStr` parses a count; `serde_json::Value::from(nm)` is the count |

The ratios `NM_PER_MM`, `NM_PER_UM`, `NM_PER_MIL`, `NM_PER_INCH`, `SCH_IU_NM` stay `i64`: they are counts per unit, not lengths.

### Wave 1: the IR, and shims everywhere else

`crates/model` is converted: `Point`, every length field of the IR (footprints, rules, stackup, outlines, groups, tables, tuning, the overlays),
and the model's own helpers (`table_text`, `reannotate`, `annotate`'s positions, the floorplan, the board trim). Length fields that were bare `i64`
became `Nm`: `ErcMarker::at_nm`, `DrcExclusion::positions_nm`, `TuningPattern::last_tuning_length`, `ReannotateInput` / `Listed` / `Bad` `x`,
`y`, `annotate::Entry::pos`. Three of those held nanometres even in the micrometre IR and were never scaled by the migration, rightly; the source test
of `tests/nanometre_migration.rs` now takes a field marked `// nanometres since schema 1` as one that is not scaled. What stays `i64` on purpose:
`TargetRange` (nanometres or attoseconds, by the pattern's domain), `kicad_font` / `kicad_geom` (KiCad's schematic IU, 100 nm), areas
(`min_island_area`, nm^2), counts and angles.

The other crates compile through **shims**, kept in each crate's own files so the crates can be converted one by one:

- `nm.raw_shim()`: the same as `nm.get()`, where a length meets code that still holds lengths as `i64`;
- `Nm::from_raw_shim(v)`: the same as `Nm::new(v)`, where an `i64` (a literal, a value from code not converted yet) becomes a length.

Both are `#[doc(hidden)]` and named for `grep -rn raw_shim crates/<crate>`. A literal `0` became `Nm::ZERO` and a type annotation that held an IR
length became `Nm` where that was the smaller change; neither is a shim. Wave 2 replaces every shim with the typed form: a literal with `mm(..)`,
`Nm::from_mm`, or a typed constant (`const X: Nm = Nm::new(..)`), and an `i64` local, field, parameter or return type that holds a length with `Nm`.
Kernels on raw integers (clipper2, shape-poly-set, the Specctra `V`, the router's force vectors) keep their `i64` and convert at their boundary with
`get` / `new`. The counts per crate at the end of wave 1 are in the table below (`.raw_shim()` / `from_raw_shim(`).

### Proof of no behaviour change

Every shim and every conversion is the identity on the bits, and every operator of `Nm` is the `i64` operator: a program that compiles computes the
same numbers. Checked by: every test passes with its assertions unchanged (only the types and constructors of test literals changed);
`tools/same_design.sh <main's eda> <this eda>` (every example through `eda pipeline` at seed 0 and every KiCad QA board through `eda import-kicad`,
every file compared with `cmp`); `tools/parity_report.py` re-export and connectivity as on main; `tools/run-all-checks.sh`.
