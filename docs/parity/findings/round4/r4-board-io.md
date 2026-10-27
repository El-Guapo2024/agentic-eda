# Round 4 findings: board files, footprints and libraries (r4-board-io, 2026-10-28)

Reviewer `r4-board-io`, read-only. Code read from main's checkout at `01ffc984`. KiCad sources: `kicad-src-8303b2ad` (`$KICAD_SRC_DIR`) and `kicad-src-25ec2335` for KiCad 10 items. No port was used.

## How it was measured

- **Binary.** Main's tree at `01ffc984`, exported with `git archive` into `/tmp/findings-r4-board-io.QGzO` (no worktree, no branch), built there with its own `CARGO_TARGET_DIR`, `nice`, `CARGO_BUILD_JOBS=2` (`cargo build -p eda-cli --bin eda`). The target dir and the source copy were deleted afterwards; the binary is kept at `/tmp/findings-r4-board-io.QGzO/eda-bin`.
- **Round trip.** `tools/board-roundtrip/run.sh <eda> /tmp/findings-r4-board-io.QGzO/rt 3` over all 214 boards (185 from 8303b2ad, 29 only in 25ec2335). The kicad-cli rewrites of the originals were reused from round 3's cache (same kicad-cli 10.99.0).
- **Result.** `/tmp/findings-r4-board-io.QGzO/rt/summary.txt`, one `diff.txt` per board.
- **Residue.** `/tmp/findings-r4-board-io.QGzO/resid.txt` (round 3's `bin/resid.py`: drops the footprint `(path)`/`(sheetname)`/`(sheetfile)`, treats `(type default)` as `(type solid)`, ignores the hatch pitch, compares lines as multisets). `bin/pairdiff.py` there prints, for each pair of differing lines, the tokens that differ.

### Numbers (`cmp.py`)

| | round 3 start (main `5cac9357`) | after round 3's fixes (`task-r2-misc-leftovers` final) | main `01ffc984` (now) |
|---|---|---|---|
| lines that differ | 35922 | 32362 | **32336** |
| boards identical | 8 | 15 | **15** |

- Against round 3's start: 75 boards better, 1 "worse" (`via_stacks` 2 -> 4, the kept generators' member uuids; noise, see below).
- Against round 3's end: 13 boards better (2 lines each: the plot settings' `Rescue` bit, round 3's F11, fixed on `task-r3-editors-exports` a10c9d08), none worse.
- `plugins/fakeboard` still fails to import (as in rounds 2 and 3; it is a plugin test fixture, not a board).

### What still differs, by cause, biggest first (32336 lines)

| Lines | Cause | Status |
|---|---|---|
| 20005 | Footprint `(path)`, `(sheetname)`, `(sheetfile)` point at the derived schematic | by design (batch A; r3 F12 cleaned the duplicates: no footprint has two `(sheetfile)` now) |
| 9784 | Zone `(hatch .. <pitch>)`: written 0.5 mm, the file's pitch lost (7906 of them on `issue21482`'s teardrops at 0.1) | GAPS 2, display only |
| 1912 | Line style `(type default)` vs `(type solid)` (noise, same meaning), of which **18 lines** are dashes and dots written solid (`issue12831`, `issue22267`, `api_kitchen_sink`, `connect`) | GAPS 2, p7-p8 F3 |
| 222 | Generators: member uuids (noise; same member counts) and tuning-pattern `target_delay` where the file has none (kicad-cli's stale value; round 2's note) | noise |
| 176 | 3D model `(rotate (xyz -0 ..))` vs `0` | noise |
| 74 | Groups: member uuids only. Every group keeps its member count and its lock (checked by name, count and lock on all 12 boards) | noise |
| 54 | Edge.Cuts lines written start/end swapped where the outline was rebuilt as a loop | r3 F13 left, same geometry |
| 26 | Dimensions: `text_position_mode 2` read as 0 (known), **radial and leader text moved (N1)**, a dimension text's `justify` lost | GAPS 2 / item 35; N1 new |
| 24 | Pads: shared pad numbers on different nets (`unconnected-(H2-Pad1)_1` .. `_3`: `connect`, `stonehenge`, `unconnected-netnames`, `issue23469`), custom-pad `gr_poly` arcs flattened, a circle pad's `rect_delta` | GAPS 36, GAPS 31, r3 F13 left |
| 18 | `(legacy_teardrops yes)` on boards with teardrops | by design (batch C) |
| 16 | Footprint `fp_poly` drawn with arcs: flattened to fewer points than KiCad's own flattening (N7) | low, new |
| 12 | Vias of a pre-KiCad-10 file get explicit `(capping no) (covering ..)` tokens | r2 F1 "left", same meaning |
| 8 | Board `gr_poly` with arcs flattened (`issue22475`, `issue22597`, `api_kitchen_sink`, `teardrop_custom_pad_anchor`) | GAPS 27 |
| 4 | `track_end` teardrops written as `padvia` (`issue17967`) | PARITY-pcb, not in GAPS (r3 note) |
| 3 | `User.9` enabled on boards whose only User.9 items sit on a layer the file had off (N6) | low, new |
| 4 | 1 nm rounding: a legacy user field's position (`issue12609` R1 MFR/MPN), an arc's mid (`issue8407`) (N8) | low |
| 6+2 | Empty `(embedded_files)` dropped; stackup `addsublayer` (`zone_filler`) | r2 F17 open |

So, beyond the by-design and display-only causes, about 600 lines are left, nearly all known.

## Round-3 merges, checked (priority a)

Each fix was read against the C++ and checked on its corpus board in the round trip above:

- **F1 `(net 0)`** (`import.rs:554` `net_of`, `sexpr::is_bare_number`) against 25ec2335 `parseNet` (`IsNumber( token )` -> `getNetCode( parseInt() )`, `SetNetCode` failing leaves net 0). `issue23467` differs only by a line style now. One pad-only edge case is left: N4.
- **F2 thieving zones and the 20260513 version** (`pcb_items.rs` `write_zone`, `pcb.rs` `board_file_version` / `has_thieving_zone`) against 25ec2335 `parseZONE` (`T_mode` `T_thieving` and the `(thieving ..)` block, both gated `m_requiredVersion < 20260513`) and `format( const ZONE* )` (`(mode thieving)` after `yes`, the `(thieving (type ..) (size ..) (gap ..) (width ..) (stagger ..) (orientation ..))` block last in `(fill ..)`). `zone_thieving_track_bisection` is now identical (0 lines). I listed every `m_requiredVersion` gate of the 25ec2335 parser (lines 896, 4399, 7509, 8951, 9395, 9558, 9622) and the `PCB_PLOT_PARAMS_PARSER` gates (`< 20240819` only): writing 20260513 crosses only 20250926's `die_delay` gate (`<= 20250926`), which the doc comment already names (the one QA board with `die_delay`, `time_calculations`, is 20250926 and is written as 20241229, so it keeps its meaning). The writer's and the version's conditions agree (not a teardrop, not a rule area, not hatched).
- **F3 via uuids** (`pcb.rs:380`): `multichannel_nested` 48 vias, 48 distinct uuids; `via_stacks` 4 of 4.
- **F4 legacy group lock** (`import_items.rs` `is_locked`): `reverse_via`'s 4109-member group keeps `(locked yes)`; only member uuids differ.
- **F5 unread generators** (`DrawingsSection::raw_generators`): kicad-cli's rewrite of our `via_stacks` keeps both `via_stack` generators with 2 and 3 members.
- **F6 no guessed 3D model**: `fill_bad`, `issue25101`, `issue5830` have no model lines left beyond `-0`.
- **F7 legacy Description field**: no `property "Description"` line differs on any board.
- **F8 keepout defaults** (`import.rs` `import_zones`) against `ZONE_SETTINGS::ZONE_SETTINGS` (25ec2335 `zone_settings.cpp:92-96`) and `parseZONE` `T_keepout` ("Initialize these two because their tokens won't appear in older files"): `vme-wren`'s two placement areas write `(keepout (tracks not_allowed) (vias not_allowed) (pads not_allowed) ..)` as kicad-cli does.
- **F9 teardrop borders**: no `ZONE` line differs on any board except the pitch and `issue17967`'s `track_end`.
- **F10 zone arcs** (`DrawingsSection::zone_arcs`, `ZoneArcs::shape_of`): `issue22475` and `issue22826` have no zone line left. A footprint zone of a 20260616+ file is moved to the board frame (`fp_transform::map_pts` maps the arcs' start/mid/end too) before the rings are kept, so the kept text is in the board frame.
- **F11 Rescue bit**: the 13 boards are 2 lines better.
- **F12 sheetfile**: no footprint of any of the 214 intent boards has two `(sheetfile)` or two `(sheetname)`.
- **F13** zone hatch offsets: `api_kitchen_sink`, `Issue19775_ZoneLayers/LayerEnumerate` have no `hatch_position` line left; long user names on pre-20200922 boards: none left.

## Findings

### N1. A radial or leader dimension's text is moved to a computed place; KiCad keeps it where the file has it. **medium**

**Fixed** on ds/r4-board-io: `25a5f8d7`. `import_dimensions` keeps the `gr_text`'s `(at x y)` for a radial and a leader (`Dimension::text_pos`) and the leader's `(text_frame ..)` (`Dimension::text_frame`); the writer gives both back, Move/Rotate carry the kept position, and a radial's aligned angle follows `PCB_DIM_RADIAL::updateText`. Test: `a_leader_and_a_radial_dimension_keep_their_text_position`. `issue6945` 50 -> 42 lines, `api_kitchen_sink` 49 -> 45.

**What differs.**
- `crates/kicad/src/import_items.rs:308` (`import_dimensions`) does not read the `gr_text`'s `(at ..)` ("The dimension's measured value and text are not read: both are recomputed ... (`dim->Update()` in KiCad, `compute_dimension_geometry` here)"), and `crates/kicad/src/pcb.rs:461` writes `compute_dimension_geometry(d)`'s text position for every kind.
- For a radial and a leader dimension, KiCad never computes the text position: it is the user's (the line is drawn *to* it). 8303b2ad `pcb_dimension.cpp`:
  - `PCB_DIM_LEADER::updateGeometry` builds the line from `m_end` to the text: `SEG textSeg( m_end, GetTextPos() );` and nowhere calls `SetTextPos`.
  - `PCB_DIM_RADIAL::updateGeometry`: `SEG textSeg( arrowSeg.B, GetTextPos() );`, and `PCB_DIM_RADIAL::updateText` only turns the text toward it: `VECTOR2I textLine( GetTextPos() - GetKnee() );` (when `m_keepTextAligned`).
  - So `text_position_mode` 0 does not make KiCad move these texts; the comment's "KiCad recomputes both on load" holds for aligned and orthogonal only.
- A leader's `(text_frame 1|2)` is written `0` (`pcb_items.rs:355`). That half is known (PARITY-pcb section 18, "A leader's optional text border ... not ported"); it is listed here because the same fix (keeping the text item) carries it.

**How to see it.**
- 8303b2ad `issue6945`: four leaders "PTH Violation", "NPTH Violation", "Max Hole Violation" x2 with `text_position_mode 0`. kicad-cli's rewrite of the original keeps `(gr_text "PTH Violation" (at 176.52 92.91 0) ..)`; ours is `(at 172.300996 96.045911 0)`, 5.3 mm away, and the leader line now ends there.
- 25ec2335 `api_kitchen_sink`: the leader "Leader" goes from (119, 85.75) to (114.761, 84.440) and loses `(text_frame 1)`; the radial "R 2.1506 mm" goes from (97.450, 96.965) at 0 degrees to (109.336, 97.546) at 36 degrees.

**Test.** `a_leader_and_a_radial_dimension_keep_their_text_position`: import a board with `(dimension (type leader) (pts (xy 167.95 100.95) (xy 170.31 98.29)) .. (style .. (text_position_mode 0) (text_frame 1) ..) (gr_text "PTH" (at 176.52 92.91 0) ..))` and a radial one likewise; export; assert the written `gr_text` is `(at 176.52 92.91 0)` and the leader writes `(text_frame 1)`.

**Known?** Partly. GAPS item 35 lists "manual text position" (mode 2) and the leader's text border. The mode-0 radial/leader case, where KiCad has no automatic position at all, is not listed, and the `import_items.rs` comment claims KiCad recomputes it.

### N2. Update/Compare Footprint with Library reports board "overrides" that are the library's own values. **medium**

**Fixed** on ds/r4-board-io: `7ce2a09c`. Every override line is told only when the board's value differs from the library's (`FootprintNeedsUpdate`, `padHasOverrides`), the library's own pad margins and zone facts travel with the placed footprint (`LibraryFacts`/`PadOverrides`, `lib_keep`), and the missing footprint-level solder-paste checks and the pad's spoke-angle check are in. Test: `an_override_equal_to_the_librarys_is_not_reported` (and `a_published_library_footprints_own_overrides_are_not_differences` in the CLI). An unedited placed footprint reports nothing.

**What differs.**
- `crates/model/src/fp_compare.rs:329-341` reports "Pad clearance overridden.", "Solder mask expansion overridden.", "Zone connection overridden." whenever the board's value is set, and `pad_override_lines` (`fp_compare.rs:274`) reports each pad's when set.
- KiCad reports an override only when it differs from the library's. 8303b2ad `drc_test_provider_library_parity.cpp::FootprintNeedsUpdate`:
  ```cpp
  if( GetLocalClearance().has_value() && GetLocalClearance() != aLibFP->GetLocalClearance() )
  {
      diff = true;
      REPORT( _( "Pad clearance overridden." ) );
  }
  ```
  and `padHasOverrides`:
  ```cpp
  if( a->GetLocalZoneConnection() != ZONE_CONNECTION::INHERITED
          && a->GetLocalZoneConnection() != b->GetLocalZoneConnection() )
  {
      diff = true;
      REPORT_MSG( _( "%s has zone connection override." ), PAD_DESC( a ) );
  }
  ```
- The board's values come from `ops/src/fp_exchange.rs:190-205` (`board_footprint`) through `Design::pad_zone_facts` (`model/src/ir.rs:2914`), which for a footprint placed from a *published* library footprint takes that library footprint's own pad `zone_connection`, `clearance_override`, `thermal_gap_override` and `thermal_spoke_width_override` as the board's. For an imported KiCad board they are the file's pad tokens (`FootprintExtra`), which are the library's when the footprint is up to date. In both cases the library footprint compared against (`Footprint`, `model/src/footprint.rs:450`) has no margins or zone settings, so the comparison cannot be KiCad's.
- Also missing from KiCad's list: "Solder paste absolute clearance overridden.", "Solder paste relative clearance overridden." (footprint level) and "%s has thermal relief spoke angle override." (`a->GetThermalSpokeAngle() != b->GetThermalSpokeAngle()`, compared even without a local override).

**How to see it.** A library footprint whose pad 1 has `(zone_connect 2)` (an exposed thermal pad set to solid in the library), published to the design and placed, unedited: Tools > Update Footprints from Library / Compare Footprint with Library lists "Pad 1 has zone connection override." and `needs_update` is true. KiCad lists nothing.

**Test.** In `fp_compare.rs` tests: `an_override_equal_to_the_librarys_is_not_reported`: a library pad with `zone_connection: Some(Solid)` and the board pad with the same; assert `footprint_differences(..)` is empty. The fix needs the library's margins and zone facts in what is compared (`LibraryFootprint`'s pads have them).

**Known?** No. GAPS 26 lists the attribute flags, net-tie groups, rule areas and stackup as not compared, not the override test. Round 1 (`p1-p2-footprints.md:294`) left `FootprintNeedsUpdate`/`fp_compare` unreviewed.

### N3. The library-parity report's wording and pad checks are not KiCad's. **low**

**Fixed** on ds/r4-board-io: `36319460`. KiCad's wording (`describe`: "Segment of U1 on F.SilkS differs.", "%s [<net>] of %s on %s"), courtyard graphics among the other graphics, the pad layer-set comparison, pad-to-die length, fabrication property, the copper shape's whole stroke and one primitives line for the first differing layer. Not done: "'Allow bridged solder mask apertures between pads' settings differ." and "Barcode count differs." -- the model carries neither setting (GAPS 26 already lists the attribute flags); the "Courtyard differs." fallback stays only for a footprint whose courtyard is a derived outline rather than file graphics, where KiCad has no equivalent line to copy.

`crates/model/src/fp_compare.rs` against 8303b2ad `drc_test_provider_library_parity.cpp`:
- **Shape lines.** Ours: "Line on F.SilkS differs." (`kind_name`, `fp_compare.rs:86`). KiCad: `REPORT( wxString::Format( _( "%s differs." ), ITEM_DESC( *aIt ) ) )` with `PCB_SHAPE::GetItemDescription`: `"%s of %s on %s"` (`GetFriendlyName()`, the footprint reference, the layer), and `"%s %s of %s on %s"` with the net for a copper shape; `EDA_SHAPE::getFriendlyName` says "Segment", "Rectangle", "Arc", "Circle", "Polygon", "Curve". So KiCad says "Segment of U1 on F.SilkS differs.".
- **"Courtyard differs."** (`fp_compare.rs:355-357`) is not a KiCad message. KiCad's courtyard lines are graphic items like the rest (`GraphicalItems()` filtered to `PCB_SHAPE_T`), so a courtyard change reads "Segment of U1 on F.CrtYd differs." or "Graphic item count differs.".
- **Pad layers.** `pad_lines` (`fp_compare.rs:222`) says "Pad %s layers differ." only for the side and the unconnected-layer flags. KiCad also compares the layer sets (`boardLayersMatchWithInnerLayerExpansion( getBoardNormalizedLayerSet( a, .. ), getBoardNormalizedLayerSet( bLib, .. ), allowExpansion )`), so a pad whose paste or mask layer was removed on the board is reported there and not here.
- **Not compared:** "%s pad to die length differs." (`GetPadToDieLength`), "%s fabrication property differs." (`GetProperty`), "'Allow bridged solder mask apertures between pads' settings differ.", "Barcode count differs.".
- **Primitives.** Ours writes "shape primitives differ on layer .." for each layer that differs; KiCad writes one line, for `firstDifferingLayer`.
- **Copper shape stroke.** KiCad `TEST( curr_shape.GetStroke(), ref_shape.GetStroke(), "" )` compares the whole stroke (width, line style, colour); ours the width only.

**Test.** `the_lines_follow_kicads_order_and_wording` already exists; extend it with a silkscreen segment ("Segment of R1 on F.SilkS differs.") and a courtyard change.

**Known?** Partly: GAPS 26 names the attribute flags, not the wording or the pad layer sets.

### N4. A footprint file of version 20260826 or later is read with its bold text 1.6 times too thin. **medium**

**Fixed** on ds/r4-board-io: `bb087728`. `parse_footprint_file` and `footprint_import` run `bold_text::normalize` against the footprint file's own `(version ..)`, as `parseEDA_TEXT`'s `m_requiredVersion < 20260826` gate does; test `a_kicad_11_footprint_files_bold_text_keeps_its_width`.

**What differs.**
- The board reader widens a bold text's thickness back to the older meaning (`crates/kicad/src/bold_text.rs::normalize`, called only from `import.rs:151`).
- The footprint readers do not: `crates/kicad/src/footprint_lib.rs:69` (`parse_footprint_file`, the library footprints the board places, their art read by `fp_art_io::read_art`) and `crates/kicad/src/footprint_import.rs:78` (the Footprint Editor's `.kicad_mod` import, which reads `(version ..)` but only for the field and margin gates).
- KiCad applies the same gate to footprint files. 25ec2335 `parseEDA_TEXT`:
  ```cpp
  if( m_requiredVersion < 20260826 )
      aText->MigrateLegacyBoldStrokeWidth();
  ```
  and `parseFOOTPRINT_unchecked` sets `m_requiredVersion = std::max( m_requiredVersion, this_version )` from the footprint's own `(version ..)`.
- The footprint is then written as 20240108 (`export_kicad_mod_with`, `footprint_lib.rs:350`) or inside a 20241229 board, so KiCad draws the base width.

**How to see it.** Save a footprint with a bold `(fp_text user "X" .. (effects (font (size 1 1) (thickness 0.125) (bold yes))))` from the box's KiCad 10.99 Footprint Editor (it writes 20261017), place it from the project's library: kicad-cli draws its strokes at 0.125 mm where KiCad drew 0.2 mm. The corpus has no such footprint file.

**Test.** `a_kicad_11_footprint_files_bold_text_keeps_its_width`: `parse_footprint_file` of `(footprint "X" (version 20261017) .. (fp_text user "X" (at 0 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.125) (bold yes)))) (pad ..))`; assert the art text's thickness is 0.2 mm; same through `footprint_import`.

**Known?** No (`bold_text.rs`'s header says "every reader of text in this crate works on either", which is true for boards only).

### N5. A legacy pad whose `(net N "name")` disagrees with the net table keeps the table's net; KiCad leaves it on no net. **low**

**Fixed** on ds/r4-board-io: `2154aa5d`. `parsePAD`'s `case T_net`: a code the table has is on the table's net only when the line's name is that net's name, else no net (`NETINFO_LIST::ORPHANED`); a code the table lacks makes the line's name a net whatever the table (the `SetNetCode` failure path). Test: `a_pad_whose_net_name_disagrees_with_its_code_is_on_no_net` and the no-table case.

**What differs.**
- `crates/kicad/src/import.rs:554` (`net_of`): a bare code found in the table gives the table's name; a code not found gives the line's name only when the table is not empty.
- 25ec2335 `parsePAD` `case T_net`:
  ```cpp
  if( foundNetcode )
  {
      if( netName != m_board->FindNet( pad->GetNetCode() )->GetNetname() )
      {
          pad->SetNetCode( NETINFO_LIST::ORPHANED, /* aNoAssert */ true );
          wxLogTrace( traceKicadPcbPlugin,
                      _( "Net name doesn't match ID in\nfile: %s\nline: %d offset: %d" ),
  ```
  and when `SetNetCode` fails (a code the board does not have) the name is used whatever the table: `FindNet( netName )`, else a new `NETINFO_ITEM`.
- So: (a) a pad `(net 3 "B")` with `(net 3 "A")` in the table is on no net in KiCad and on "A" here; (b) a pad `(net 5 "X")` in a file with no net table is on "X" in KiCad and on no net here. Tracks, vias and zones go through `parseNet`, which takes the code alone, so they agree.

**How to see it.** Hand-edited or merged files; no corpus board has either.

**Test.** `a_pad_whose_net_name_disagrees_with_its_code_is_on_no_net`: legacy table `(net 3 "A")`, pad `(net 3 "B")`; assert the pad's net is "". And a 20260206 file with a pad `(net 5 "X")`; assert "X".

**Known?** No.

### N6. A layer the file has off is turned on when an item sits on it. **low**

**Fixed** on ds/r4-board-io: `7419c3fc`. `add_layers_in_use` is gone: an item on a layer the file's `(layers ..)` does not name keeps its layer, and the page stays the file's, so `write_layers` lists only what the file listed; the item is kept and hidden. The test is now `a_layer_the_items_are_on_but_the_file_has_off_stays_off` (the old one asserted the divergence), and it also checks that the export carries both items and reads back as the same page. `issue25265` 14 -> 13, `issue25303` 14 -> 13, `stitch_fp_rulearea` 7 -> 6.

**What differs.**
- `crates/kicad/src/import.rs` `add_layers_in_use` ("A layer an imported item is on is a layer the board has") enables it, and `pcb.rs` `write_layers` lists it.
- KiCad's parser finds the item's layer by name among all standard names (`m_layerIndices`), puts the item there, and leaves the board's enabled set as the file's `(layers ..)` says; `formatBoardLayers` writes only the enabled ones. The item is kept and hidden.

**How to see it.** 25ec2335 `issue25265`, `issue25303`, `stitch_fp_rulearea`: two `gr_text` holding a via-stitching plugin's JSON on `User.9`, which the file does not list. kicad-cli's rewrite of ours adds `(55 "User.9" user)`, so the JSON text becomes a visible layer in the Appearance panel and in plots that take all user layers.

**Known?** The behaviour is deliberate (for minimal fixtures), but not listed as a difference from KiCad. r2-board-io F17's fix covered technical layers only.

### N7. A footprint polygon drawn with arcs is flattened to fewer points than KiCad flattens it to. **low**

**Fixed** on ds/r4-board-io: `7419c3fc`. A polygon's arcs are flattened at `getArcPolygonizationMaxError()` -- `SHAPE_ARC::DefaultAccuracyForPCB() / 5`, 0.001 mm (`eda_model::outline::POLYGON_ARC_ERROR_NM`) -- in the footprint art reader, the board `gr_poly`/zone `(pts ..)` reader and so in the writer's points: `connect`'s J102 `fp_poly` has kicad-cli's 42 points (was 22) and compares identical (`connect` 206 -> 190 lines); `issue23053`'s H2-H5 now have kicad-cli's 60 points too, with 1 nm on a few coordinates left (our `Arc::polyline` rounds where `SHAPE_ARC::ConvertToPolyline` rounds slightly differently -- the N8 family; a faithful port of that function would touch every caller of `polyline`, so it is not done here).

**How to see it.** 8303b2ad `connect`, J102's four `fp_poly` of two arcs each: kicad-cli's rewrite of the original has 42 points per polygon, ours 22 (`issue23053` likewise). The outline moves by up to the flattening error. kicad-cli itself writes these as points, so the arcs themselves are not lost by us alone.

**Known?** The board `gr_poly` case is GAPS 27 and the custom-pad case GAPS 31; the footprint `fp_poly` case is not named.

### N8. One-nanometre drifts. **low**

**Fixed** (arc) and **not a bug** (legacy field) on ds/r4-board-io: `7419c3fc`.
- `issue8407`: **fixed**. A legacy `(start CENTRE) (end ARC_START) (angle A)` arc now mirrors `SetArcAngleAndEnd( A, true )` (the end is the start turned by -A; a negative A swaps them) and `GetArcMid` (the rounded start turned by half the angle its rounded radials make, `CalcArcAngles`), with KiCad's integer `RotatePoint`/`KIROUND` and its exact `EDA_ANGLE::Sin`/`Cos` and `EDA_ANGLE( VECTOR2D )` values. `issue8407` 173 -> 169.
- `issue12609` R1 MFR/MPN (back side, legacy file): **not our import's rounding**. Our export writes `(at 275.824062 210.835224 0)`, byte for byte what kicad-cli's own rewrite of the original 20220914 file has -- the import's `p - R(a) p` for a field with no `(at)` reproduces KiCad's value exactly. The 1 nm appears only when kicad-cli re-parses our version-20241229 export: a pre-20260616 file's board-frame `(at)` is converted into the footprint frame and written back, with integer rounding each way, and no value near 210.835224 lands back on it (222 -> 223, 223 -> 223, 224 -> 223, 225 -> 225; a 5x7 probe around the pair found no preimage). The last nanometre therefore needs the 20260616 transform format (a KiCad 10 file version), not a different number in ours.

## Reuse (active code that duplicates kicad-cli or a linked C++ library)

- `crates/model/src/fp_compare.rs` re-implements `FOOTPRINT::FootprintNeedsUpdate`. kicad-cli's DRC runs the same function (`DRC_TEST_PROVIDER_LIBRARY_PARITY`, `lib_footprint_mismatch`) when the project has an `fp-lib-table`, but its report carries only "Footprint '%s' does not match copy in library", not the reporter's lines the Compare dialog shows. So the port is still needed for the dialog's text; the yes/no answer (Update Footprints from Library's list) could come from kicad-cli. Worth a row in the port-vs-reuse audit; not a finding.
- `crates/kicad/src/embedded.rs` ports MurmurHash3 x64-128 (`mmh3_x64_128`, `mmh3_v1_x64_128`). zstd is already the C library; MurmurHash3 is public-domain C++ (smhasher) and KiCad's `mmh3_hash.h` is header-only. Small and tested, so low value; listed for completeness.
- `dxf_import.rs` (dxflib half) is already in the audit (KEEP-PORT for now).

## Checked and found to match (do not review again)

- Everything under "Round-3 merges, checked" above.
- `ExchangeFootprint`'s pad matching (8303b2ad `pcb_edit_frame.cpp`: `matchItemsBySimilarity<PAD>`, unmatched new pads `NETINFO_LIST::UNCONNECTED`): GAPS 26 already lists "kept by pad number only".
- `FootprintNeedsUpdate`'s shape comparison (`shapeNeedsUpdate`): sort order (`cmp_drawings`: layer, then `SHAPE_T` order segment, rectangle, arc, circle, poly, bezier), `EPSILON` 10 nm, the arc mid at `mmToIU( 0.0005 )`, the polygon `CompareGeometry( .., true, EPSILON )`, the stroke compared on copper only, fill and layer: as `fp_compare.rs:113-168`. Pads: `cmp_pads` order, position, type, orientation (`TEST_D` 1e-5), per unique layer shape, size, delta, round-rect and chamfer ratios, chamfer positions, offset, drill shape and size: as `pad_lines`, apart from N3.
- The 3D models, groups (counts and locks) and generators of all 214 boards: only uuid and `-0` noise.
- Footprint checker (`kicad-engine/footprint_check.rs`) and foreign imports (`kicad-engine/foreign.rs`) run kicad-cli; no Rust re-implementation is active.

## Summary

| # | Finding | Severity | Known? |
|---|---|---|---|
| N1 | Radial and leader dimension text moved to a computed place; leader text frame lost | medium | partly (mode 2 and the frame are; mode 0 is not) |
| N2 | Library parity reports the library's own pad/footprint settings as board overrides; paste and spoke-angle overrides missing | medium | no |
| N4 | `.kicad_mod` of 20260826+ read without the bold-text migration: bold text 1.6x thinner | medium | no |
| N3 | Library parity wording ("Line on .." vs "Segment of U1 on ..", "Courtyard differs."), pad layer sets, die length, fab property not compared | low | partly |
| N5 | Pad `(net N "name")` mismatches: KiCad orphans / uses the name; we take the table / no net | low | no |
| N6 | A layer the file has off is enabled when an item sits on it (`User.9` on 3 boards) | low | no (deliberate) |
| N7 | Footprint `fp_poly` arcs flattened to fewer points than KiCad | low | no (board/pad cases known) |
| N8 | 1 nm drifts (legacy field position, arc mid) | low | no |

Counts: **high 0**, **medium 3** (N1, N2, N4), **low 5** (N3, N5, N6, N7, N8).

Round trip: 35922 (round 3 start) -> 32362 (after round 3's fixes) -> **32336** now (13 boards 2 lines better from the Rescue fix, none worse); 15 boards identical; about 600 lines left beyond the by-design footprint links (20005), the hatch pitch (9784) and the default/solid line style.
