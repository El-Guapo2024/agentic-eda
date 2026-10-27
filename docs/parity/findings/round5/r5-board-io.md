# Round 5 findings: board files, footprints and libraries (r5-board-io, 2026-10-28)

Reviewer `r5-board-io`, read-only. Code read from main's checkout at `8a8e47f1`. KiCad sources: `kicad-src-8303b2ad` (`$KICAD_SRC_DIR`) and `kicad-src-25ec2335` (the box's kicad-cli 10.99.0, and the KiCad 10 items). No port was used.

## How it was measured

- **Binary.** Main at `8a8e47f1`, `git archive` into `/tmp/findings-r5-board-io.xqOv/src`, built with its own `CARGO_TARGET_DIR`, `nice`, `CARGO_BUILD_JOBS=2` (`cargo build -p eda-cli --bin eda`). Target dir and source copy deleted afterwards; the binary is kept at `/tmp/findings-r5-board-io.xqOv/eda-bin`.
- **Round trip.** `tools/board-roundtrip/run.sh <eda> /tmp/findings-r5-board-io.xqOv/rt 4` over the 214 boards, with `EDA_KICAD_API=0` (the per-run kicad-cli, so no api-server was started by the run). The kicad-cli rewrites of the originals were reused from round 4's cache (same kicad-cli 10.99.0).
- **Results.** `/tmp/findings-r5-board-io.xqOv/rt/summary.txt`, one `diff.txt` per board; the residue (round 3's `bin/resid.py`) is `/tmp/findings-r5-board-io.xqOv/resid.txt`.

### Numbers (`cmp.py` against round 4's run at `01ffc984`)

| | round 4 start (`01ffc984`) | main `8a8e47f1` (now) |
|---|---|---|
| lines that differ | 32336 | **32301** |
| boards identical | 15 | **15** |

- 7 boards better, none worse: `connect` 206 -> 190, `issue6945` 50 -> 42, `api_kitchen_sink` 49 -> 45, `issue8407` 173 -> 169, `issue25265` 14 -> 13, `issue25303` 14 -> 13, `stitch_fp_rulearea` 7 -> 6. This is the 32301 the ds/r4-board-io merge claimed, reproduced.
- `plugins/fakeboard` still fails to import (a plugin test fixture, not a board; as in rounds 2-4).

### What still differs, by cause, biggest first (32301 lines)

Measured by normalising one cause at a time (multiset diff, as `resid.py`):

| Lines | Cause | Status |
|---|---|---|
| 20005 | Footprint `(path)`, `(sheetname)`, `(sheetfile)` point at the derived schematic | by design (batch A) |
| 9820 | Zone `(hatch .. <pitch>)` written 0.5 mm, the file's pitch lost (most on `issue21482`'s teardrops) | GAPS 2, display only |
| 1774 | Line style `(type default)` vs `(type solid)` (noise) | GAPS 2 |
| 222 | Generators: member uuids (same counts) and kicad-cli's stale `target_delay` | noise |
| 176 | 3D model `(rotate (xyz -0 ..))` vs `0` (every model line checked: nothing else) | noise |
| 78 | Groups: member uuids only | noise |
| 62 | Edge.Cuts lines written with start and end swapped where the outline was rebuilt as a loop | r3 F13, left; same geometry |
| 48 | Pads: shared pad numbers on different nets (`connect`, `stonehenge`, `unconnected-netnames`, `issue23469`), custom-pad `gr_poly` arcs flattened (`issue22475`, `teardrop_custom_pad_anchor`), a circle pad's `rect_delta` (`hatch_thermal_connectivity`) | GAPS 36, GAPS 31, r3 F13 |
| ~38 | Dashed and dotted lines, arcs and rects written solid (`issue12831`, `issue22267`, `api_kitchen_sink`, `connect`) | GAPS 2, p7-p8 F3 |
| 18 | `(legacy_teardrops yes)` | by design (batch C) |
| 16 | `issue23053` H2-H5 `fp_poly`: 1 nm on some flattened-arc points | r4 N7 note, left |
| 14 | Dimensions `text_position_mode 2` read as 0 | GAPS 28 ("manual text position") |
| 12 | Vias of a pre-KiCad-10 file get explicit `(capping no) (covering ..)` | r2 F1 left, same meaning |
| 6+2 | Empty `(embedded_files)` dropped; `zone_filler`'s stackup `addsublayer` | r2 F17 open |
| 4 | `issue22597`, `api_kitchen_sink` `gr_poly` arcs flattened | GAPS 27 |
| 4 | `issue17967` `track_end` teardrops written `padvia` | PARITY-pcb |
| 4 | `issue12609` R1 MFR/MPN 1 nm | r4 N8: not ours (the 20260616 transform) |

Nothing in the residue is new: every line left is a cause rounds 2-4 listed. The QA corpus has no pre-20210606 board with `~` in a name or text (F1) and no outline-font bold text in a 20260826+ file (F2), which is why neither shows here.

## Priority (a): the round-4 merges and the api-server lane

### Round-4 merges, checked

- **N1 dimension text** (`25a5f8d7`): `import_items.rs:367-398`, `pcb_items.rs:353-361`, `connectivity/src/dimension.rs` against 8303b2ad `PCB_DIM_RADIAL::updateText` (`FULL_CIRCLE - EDA_ANGLE( textLine )`, `Normalize`, fold `> 90 && <= 270`, `KiROUND( AsDegrees() )`: ours is the same), `PCB_DIM_LEADER::updateText` (`updateGeometry`, no angle recompute), `DIM_TEXT_BORDER` (NONE, RECTANGLE, CIRCLE, ROUNDRECT: our 0-3). `issue6945` and `api_kitchen_sink` have no radial or leader line left. Move and Rotate carry `text_pos`. **Flip does not: F3.** The `(text_frame N)` clamp differs for out-of-range N: F6.
- **N2/N3 fp_compare** (`7ce2a09c`, `36319460`): override lines against `FootprintNeedsUpdate`/`padHasOverrides` (each gate, word for word, the thermal gap compared on effective values, the spoke angle always): match. Wording `describe` against `PCB_SHAPE::GetItemDescription`: matches except on copper (F7). **The "pad layer-set comparison" the N3 Fixed note claims is not in the code, and die length and fabrication property are compared only when the board has a value: F4.**
- **N4 bold text in footprint files** (`bb087728`): the call sites are right (`footprint_lib.rs:73`, `footprint_import.rs:75`), but the function they call widens outline-font text that KiCad never migrates: F2 (also on boards).
- **N5 pad nets by code** (`2154aa5d`, `import.rs:555 pad_net`) against 25ec2335 `parsePAD` `case T_net` (lines 7412-7467): code first, `netName != FindNet( code )->GetNetname()` orphans, a failed `SetNetCode` falls to `FindNet( netName )` or a new net. Matches. (KiCad also requires the name after the code: `Expecting( "net name" )`; we accept `(net 3)`. Lenient, not a finding.) KiCad converts the name's overbar notation before that comparison on old files; we never do: F1.
- **N6 layers** (`7419c3fc`): `add_layers_in_use` is gone, so items keep their layer and the page stays the file's. `issue25265`/`issue25303`/`stitch_fp_rulearea` have no layer line left.
- **N7 polygon arcs at 1 um** and **N8 legacy arcs** (`7419c3fc`): `legacy_arc` against 8303b2ad `EDA_SHAPE::SetArcAngleAndEnd` (`RotatePoint( m_end, m_arcCenter, -angle.Normalize720() )`, the swap for a negative angle) and `GetArcMid`. `issue8407`'s arcs (angles -90, -300) have no arc line left. Only the Edge.Cuts start/end swaps remain (r3 F13).

### The api-server lane (`crates/kicad-engine/src/api`), checked

- **Specctra session import** (`api/specctra.rs`) against 25ec2335 `SPECCTRA_DB::FromSESSION` (specctra_import.cpp:340-555) and `makeVIA` (186-296):
  - unlocked tracks and vias removed, locked kept: `read_back` reports only the KIIDs that were not there before, and `ops/src/specctra.rs` removes the unlocked ones itself. Consistent.
  - a `place` of an unknown reference or with no vertex is skipped and counted (`m_skippedCount`); `skipOnError` drops a bad wire or via; `(polygon ..)` wires are ignored. The lane reports KiCad's count and words (`cli/src/specctra_api.rs`). Matches.
  - via drill: `makeVIA` sets `UNDEFINED_DRILL_DIAMETER` when the padstack name's drill equals the Default class's; `PCB_VIA::Serialize` hands the padstack's `-1`, and the lane falls back to the via's net class drill (`via_drill_of`), as `GetDrillValue` does. Via type `VT_BLIND_BURIED` split by whether it reaches an outer layer. Matches.
  - placements: back side `place.m_rotation + 180`, read back by `rot_of_file_orientation` and `to_design_frame`, the same frame handling as the INACTIVE port.
  - `layer_name` (`BL_In10_Cu` -> `In10.Cu`) and the `ImportSpecctraSession` and `GetItems` messages: as `api/proto`.
- **Export jobs** (`api/jobs.rs`) against `kicad/cli/command_pcb_export_drill.cpp`, `command_pcb_export_pos.cpp`, `command_pcb_export_ipcd356.cpp` and `api_handler_pcb.cpp::handleRunBoardJobExportDrill/Position/IpcD356` (2625-2730), and the job defaults (`JOB_EXPORT_PCB_DRILL`: inch, map off, PDF; `JOB_EXPORT_PCB_POS`: drill-origin on, board edge on):
  - every kicad-cli default is set explicitly in the message: units mm, origin absolute, zeros decimal, oval alternate, map only with `--generate-map`, precision 6, `use_drill_place_file_origin` from the flag, `gerber_board_edge` from the flag, `single_file`/`naked_filename` true. Matches.
  - unmapped arguments (`--generate-report`, `--variant`, `--define-var`, `-D`) and values kicad-cli refuses (`--side both` with gerber, a bad format) go per-run, so kicad-cli says the words.
  - Proof: `through_hole_headers` (scratch `eda board new`), `eda board export --kicad pos|drill|ipcd356` through the server and with `EDA_KICAD_API=0`. The outputs are byte-identical apart from the creation dates (`/tmp/findings-r5-board-io.xqOv/api/srv-*-out`, `run-*-out`).
- **Server lifecycle** (`api/server.rs`, `api/sp.rs`): the nng `req0` framing (8-byte header, type 1 + 64-bit length, request id with top bit), stale replies dropped, restart after a transport error, `falls_back` only for unavailable/unhandled/busy. `open_board_kept` hashes the board and its `.kicad_pro`, which is everything drill, pos and ipcd356 read. No finding.

## Findings

### F1. A board older than 20210606 keeps the old overbar notation: its net names and texts are not KiCad's. **high** **Fixed** on task-r5-board-io: e3154817

**What differs.**
- `crates/kicad/src/import.rs:521` (`import_net_names`), `:543` (`net_of`), `:555` (`pad_net`), the net class `add_net` names, and every text reader take the strings as the file has them. No function in the crate converts `~...~` (grep: no overbar converter in `crates/`).
- KiCad converts them on read for every file before 20210606. 8303b2ad `pcb_io_kicad_sexpr_parser.cpp`:
  - `parseEDA_TEXT` (line 749): `// In version 20210606 the notation for overbars was changed from `~...~` to `~{...}`. We need to convert the old syntax to the new one.` `if( m_requiredVersion < 20210606 ) aText->SetText( ConvertToNewOverbarNotation( aText->GetText() ) );` This covers `gr_text`, `fp_text`, fields and dimension text.
  - `parseNETINFO_ITEM` (328), `parseNETCLASS` (3079), `T_add_net` (3162), `parsePAD` `T_net` (6253): `if( m_requiredVersion < 20210606 ) netName = ConvertToNewOverbarNotation( netName );`
  - `common/string_utils.cpp::ConvertToNewOverbarNotation`: `~X~` -> `~{X}`, `~~` -> `~`, `~~{` -> `~~{}`, a lone `~` kept, and a string that already has `~{` is returned unchanged.
- The writer then writes `(version 20241229)`, so kicad-cli does not convert either. KiCad reads net `~RESET~` as the literal text, not an overbarred `RESET`. The schematic's `~{RESET}` no longer matches it (Update PCB from Schematic renames or re-nets), and a silkscreen `~CS~` prints its tildes.

**How to see it.** A scratch board `/tmp/findings-r5-board-io.xqOv/overbar/o.kicad_pcb` (version 20210228, `(net 1 "~RESET~")`, two pads and a segment on it, `gr_text "~CS~"`):
- `kicad-cli pcb upgrade` writes `(net "~{RESET}")` and `(gr_text "~{CS}" ..)`;
- `eda import-kicad` + `eda board export --kicad pos` writes `(net 1 "~RESET~")`, `(add_net "~RESET~")` and `(gr_text "~CS~" ..)` in a 20241229 file.

The QA corpus has no pre-20210606 board with a `~`, so the round trip does not show it.

**Test.** `an_old_boards_overbars_are_kicads_new_notation`: import the board above; assert the model's net is `~{RESET}` with pins R1.1 and R1.2, the track's net is `~{RESET}`, and the text is `~{CS}`. The same file with `(version 20211014)` keeps `~RESET~`. Unit cases for the converter: `~~` -> `~`, `a~b~c` -> `a~{b}c`, `~` -> `~`, `~{x}` unchanged.

**Known?** No (no overbar row in GAPS or findings). The schematic reader (`sch_import.rs`) has the same gate in KiCad's `SCH_IO_KICAD_SEXPR_PARSER` (`m_requiredVersion < 20210606`); I did not check it, since it belongs to the schematic lane.

### F2. Bold text in an outline font is widened 1.6x on read of a 20260826+ file; KiCad migrates stroke-font text only. **medium** **Fixed** on task-r5-board-io: fcfc782e

**What differs.**
- `crates/kicad/src/bold_text.rs:21` `normalize` / `:31` `widen` multiply the `(thickness ..)` of every bold `(font ..)` by 1.6. It runs on boards (`import.rs:151`) and, since r4 N4, on footprint files (`footprint_lib.rs:73`, `footprint_import.rs:75`).
- 25ec2335 `common/eda_text.cpp::MigrateLegacyBoldStrokeWidth` (called from `parseEDA_TEXT`, parser line 896, `if( m_requiredVersion < 20260826 )`):
  ```cpp
  // Outline faces never baked bold into the width; skip them.
  if( !IsBold() || thickness <= 1 || !isStrokeFont() )
      return;
  SetTextThickness( std::max( 2, KiROUND( thickness / BOLD_STROKE_MULTIPLIER ) ) );
  ```
  `isStrokeFont()` is false for any `(face ..)` other than the KiCad font (`FONT::IsStroke`: `aFontName == _( "Default Font" ) || aFontName == KICAD_FONT_NAME`). So an outline-font bold text has the same thickness in both file versions, and multiplying it back is wrong.
- Minor: KiCad also leaves a thickness of 1 nm or less alone, and floors the result at 2 nm. Neither matters in practice.

**How to see it.** `/tmp/findings-r5-board-io.xqOv/bold/b.kicad_pcb` (20241229) has two bold texts of `(thickness 0.2)`, "STROKE" (KiCad font) and "OUTLINE" (`(face "DejaVu Sans")`). `kicad-cli pcb upgrade` writes 20261017 with STROKE `0.125` and OUTLINE `0.2`. Our import and export of that upgraded file writes STROKE `0.2` (right) and OUTLINE **`0.32`** (KiCad has 0.2).

**Test.** In `bold_text.rs`: `an_outline_fonts_bold_thickness_is_left_as_it_is`: `(kicad_pcb (version 20260901) (gr_text "a" (effects (font (face "Arial") (size 1 1) (thickness 0.2) (bold yes)))))`; assert `["0.2"]`. Also `(face "KiCad Font")` stays a stroke font and is widened. The same through `parse_footprint_file`.

**Known?** No. The N4 fix carried it into the footprint readers.

### F3. Flipping a radial or leader dimension leaves its kept text position behind. **medium** **Fixed** on task-r5-board-io: e9726153

**What differs.**
- `crates/ops/src/pcb_transform.rs:590-605` (`transform_dimension`, `Xform::Flip`) mirrors `start`, `end`, the text angle and the height. Since r4 N1 a radial or leader keeps `Dimension::text_pos`, and Flip does not mirror it. Move and Rotate do (`connectivity/src/dimension.rs` `translate_dimension`, `rotate_dimension`).
- 8303b2ad `pcb_dimension.cpp::PCB_DIMENSION_BASE::Mirror` (called by `Flip`):
  ```cpp
  VECTOR2I newPos = GetTextPos();
  MIRROR( newPos, axis_pos, aFlipDirection );
  SetTextPos( newPos );
  // invert angle
  SetTextAngle( -GetTextAngle() );
  MIRROR( m_start, axis_pos, aFlipDirection );
  MIRROR( m_end, axis_pos, aFlipDirection );
  ```
- So after a flip (F, or Flip on a selection), the leader line runs from the mirrored arrow to the old text position on the other side of the board. A radial's text and its angle (`updateText` aims at `text_pos`) are wrong in the same way, and the file keeps them.

**How to see it.** Import `issue6945` (four leaders), select one, flip it about its own position: the `gr_text (at ..)` written is unchanged while the `pts` are mirrored.

**Test.** In `pcb_transform_tests.rs`: `a_flipped_leader_mirrors_its_text_position`: a leader with `text_pos (10, 5)`, flipped left-right about x = 0; assert `text_pos == (-10, 5)` and the start and end mirrored. Same for a radial.

**Known?** No (new with N1).

### F4. Library parity: the pad layer set is not compared, though r4 N3 is marked Fixed for it; die length and fabrication property only when the board has one. **medium** **Fixed** on task-r5-board-io: 62a0d4de

**What differs.**
- `crates/model/src/fp_compare.rs:288-292` reports "Pad %s layers differ." only for `opposite_side` and the remove-unconnected/keep-outer flags. The r4 N3 Fixed note says "the pad layer-set comparison" was added; commit `36319460` has no such code, and the engine `Pad` carries no layer set (the library's `LibraryPad::layers` and the board's `PadMaskInfo::layers` exist but are not passed in).
- 8303b2ad `drc_test_provider_library_parity.cpp::padNeedsUpdate`:
  ```cpp
  if( layerSettingsDiffer
         || !boardLayersMatchWithInnerLayerExpansion( getBoardNormalizedLayerSet( a, a->GetBoard() ),
                                                     getBoardNormalizedLayerSet( bLib, a->GetBoard() ),
                                                     allowExpansion ) )
  ...
          aReporter->Report( wxString::Format( _( "%s layers differ." ), PAD_DESC( a ) ) );
  ```
- `fp_compare.rs:282` and `:297` claim die length and fabrication property only `if o.pad_to_die_length.is_some()` / `o.fabrication_property.is_some()`. KiCad always tests them: `TEST( a->GetPadToDieLength(), bLib->GetPadToDieLength(), .. "%s pad to die length differs." )` and `TEST( a->GetProperty(), bLib->GetProperty(), .. "%s fabrication property differs." )`. The board side's values exist only for a board read from a file (`ops/src/fp_exchange.rs:205-215`, `file_pad`); for a footprint placed here they are `None`. So a library that later adds a die length or a heatsink property is not reported.
- Effect: Update Footprints from Library (only changed footprints) and the DRC-free Compare skip a footprint whose library copy changed only in a pad's paste or mask layers, its die length or its fabrication property.

**How to see it.** A library footprint with pad 1 `(layers "F.Cu" "F.Paste" "F.Mask")`, placed, then the library's pad 1 changed to `(layers "F.Cu" "F.Mask")`: Compare Footprint with Library says nothing; KiCad says "Pad 1 layers differ.".

**Test.** `fp_compare.rs`: `a_pad_whose_paste_layer_was_removed_reports_layers_differ` (needs the layer sets in `InstanceFacts`/`LibraryFacts`); `a_library_die_length_the_board_lacks_is_reported`: library pad `die_length 0.5`, board pad none; assert "Pad 1 pad to die length differs.".

**Known?** The layer set was round-4 N3; its Fixed mark is wrong. GAPS 26 does not list it.

### F5. A copper graphic's line in the library-parity report: KiCad's footprint copy has no board. **low** **Fixed** on task-r5-board-io: 62a0d4de

`fp_compare.rs:229` (`describe`) writes `"Segment [<no net>] of R1 on F.Cu differs."`. In 8303b2ad and 25ec2335 `FootprintNeedsUpdate` the shapes described are those of `FOOTPRINT dummy( *this ); dummy.SetParent( nullptr );`. `PCB_SHAPE::GetItemDescription` on copper calls `GetNetnameMsg()`, which returns `wxT( "[** NO BOARD DEFINED **]" )` when `!GetBoard()`. So KiCad's line is `Segment [** NO BOARD DEFINED **] of R1 on F.Cu differs.` (and the layer is `LayerName()`'s canonical name, not the board's user name). This is a KiCad quirk; copy it or keep ours, but the doc comment's claim is not what KiCad prints. Test: extend `the_lines_follow_kicads_order_and_wording` with an `F.Cu` segment.

### F6. A leader's `(text_frame N)` outside 0-3 is read as no frame; KiCad clamps. **low** **Fixed** on task-r5-board-io: 32912d58

`import_items.rs:367-372`: `_ => DimensionTextBorder::None`. 8303b2ad `parseDIMENSION` `T_text_frame`: `textFrame = std::clamp( textFrame, 0, 3 );`, so `(text_frame 7)` is ROUNDRECT and `-1` is NONE. Hand-edited files only. Test: `(text_frame 7)` reads back as `(text_frame 3)`.

## Reuse (active code that duplicates kicad-cli or a linked C++ library)

- Nothing new. The Specctra session import and the drill/pos/ipcd356 exports now go to KiCad, with the ports INACTIVE as fallback (checked above). `fp_compare.rs` is still a re-implementation of `FootprintNeedsUpdate`, as round 4 noted; the dialog's lines need it.

## Checked and found to match (do not review again)

- Everything under "Round-4 merges, checked" and "The api-server lane, checked" above, except F2-F4 and F6.
- `footprint_scan.rs` (never reviewed): `is_numbered_pad` and the pad filter against 8303b2ad `FOOTPRINT::GetNumberedPadCount` (`isElectricalPadNumber`: 0-2 letters then digits only, copper only, not NPTH, distinct numbers). Matches.
- `line_ending_io.rs` and `eda_model::line_ending` (never reviewed) against 25ec2335 `parseLineEnding` and `LINE_ENDING::Format`: defaults 0, `length`/`width`/`stroke` written only when > 0, no ending written on a closed shape. Matches.
- `via_io.rs` (never reviewed): the tokens a via does not model are kept verbatim; `(backdrill ..)` is its own list in 25ec2335 `format( PCB_TRACK* )` (line 3270), so it survives.
- `FootprintNeedsUpdate`'s shape sort: `cmp_drawings` ends with an `m_Uuid` tiebreak, so equal shapes are not collapsed by the `std::set`. Our count comparison is the same.
- The residue of all 214 boards (table above): every line left belongs to a cause already listed.

## Summary

| # | Finding | Severity | Known? |
|---|---|---|---|
| F1 | Pre-20210606 boards: `~...~` net names and texts not converted to `~{...}` (nets, net classes, pads, all texts) | high | no |
| F2 | `bold_text::normalize` widens outline-font bold text (boards and, since N4, footprint files); KiCad skips non-stroke fonts | medium | no |
| F3 | Flip of a radial or leader dimension leaves `text_pos` unmirrored | medium | no (new with N1) |
| F4 | Library parity: pad layer sets not compared (r4 N3 marked Fixed wrongly); die length and fab property gated on the board having one | medium | partly (r4 N3) |
| F5 | Copper graphic's parity line: KiCad prints `[** NO BOARD DEFINED **]` | low | no |
| F6 | `(text_frame N)` not clamped to 0-3 | low | no |

Counts: **high 1** (F1), **medium 3** (F2, F3, F4), **low 2** (F5, F6).

Round trip: 32336 (round 4 start) -> **32301** now, 15 boards identical, 7 better, none worse. Outside the footprint links (20005), the hatch pitch (9820) and default/solid (1774), 702 lines are left, all of known causes.
