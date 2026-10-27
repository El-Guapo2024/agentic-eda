# Round 3 findings: board files, footprints and libraries (r3-board-io, 2026-10-27)

**After task-r2-misc-leftovers** (tools/board-roundtrip over the 214 boards, against main's run above): 35922 -> 32362 lines that differ, 8 -> 15 boards identical, 67 better, 1 "worse" (`via_stacks` 2 -> 4: its two generators are now kept, with member uuids that differ from the source's), `plugins/fakeboard` still fails to import.

Reviewer `r3-board-io`, read-only. Code read from main's checkout at `5cac9357`. KiCad sources: `kicad-src-8303b2ad` (`$KICAD_SRC_DIR`) and `kicad-src-25ec2335` for KiCad 10 items. No port was used.

## How it was measured

- **Binary.** I built main's tree, exported with `git archive 5cac9357`, so no worktree or branch was created. The build used its own `CARGO_TARGET_DIR` under `/tmp` and ran under `nice` with `CARGO_BUILD_JOBS=2` (`cargo build -p eda-cli --bin eda`). The target dir and the source copy were deleted afterwards; the binary is kept at `/tmp/findings-r3-board-io.XmA9/eda`.
- **Round trip.** `tools/board-roundtrip/run.sh <eda> /tmp/findings-r3-board-io.XmA9/rt 3` over all 214 boards (185 from 8303b2ad, 29 only in 25ec2335). The kicad-cli rewrite of each original was reused from the round-2 cache; its `norm.py` is the same apart from comments.
- **Result.** `/tmp/findings-r3-board-io.XmA9/rt/summary.txt`, one `diff.txt` per board.
- **Residue.** `/tmp/findings-r3-board-io.XmA9/resid.txt` (`bin/resid.py`) is a second pass over the same files. It drops the footprint symbol link (F12, by design), treats `(type default)` and `(type solid)` as the same, ignores the hatch pitch, and compares the lines as multisets, so that a reordering does not count.

### Numbers (cmp.py, against round 2's final branch run `/tmp/task-r2-board-io.45zd/new2`)

| | round 2 base (`c38589fe`) | round 2 branch | main `5cac9357` |
|---|---|---|---|
| lines that differ | 41353 | 36312 | **35922** |
| boards identical | 0 | 0 | **8** |

- 199 boards are better than at the end of round 2 and 5 are worse.
- `plugins/fakeboard` still fails to import (as before).
- The 5 worse boards (`issue22751`, `issue23535_minimal`, `api_kitchen_sink`, `issue23123_minimal`, `issue23339_zone_layer_rules`) each have +2 lines, all from F11 below: the plot settings' layer selection loses the `Rescue` bit. That change came in with r2-editors-exports X3 after round 2 measured.
- The 8 identical boards: `issue23418/testing`, `issue23515`, `ScientificNotation`, `drc_missing_tuning_profile`, `issue10697`, `issue22794`, `issue7241`, `reference_images_load_save`.

### What still differs, by cause, biggest first (35922 lines)

| Lines | Cause | Status |
|---|---|---|
| 20601 | Footprint `(path)`, `(sheetname)`, `(sheetfile)` point at the derived schematic | F12, by design (batch A) |
| 10560 | Zone outline `(hatch .. pitch)`; also the teardrop zones' border style (F9) | GAPS 2 (pitch); F9 new |
| 1916 | Footprint fields: pre-v8 hidden user fields at `(at 0 0 0)` and without mirror; the legacy Description field's layer and angle (F7) | r2 F15 open; F7 new |
| 1911 | Board and footprint line style `default`/`solid`/dashes | GAPS 2, p7-p8 F3 |
| 224 | Layer table: technical layers enabled that the file had off, and long user names written on legacy boards | r2 F17 open; long names: low, F12 |
| 220 | Generators: member uuids (noise) and the `via_stack` generator lost (F5) | F5 new |
| 182 | 3D models: `-0` vs `0` (noise), and models made up for footprints that had none (F6) | F6 new |
| 78 | Groups: one bare-`locked` lock lost (F4) and one member lost (F3) | F3, F4 new |
| 52 | Zones, other: a net named `0` (F1), thieving fill lost (F2), placement areas without `(keepout)` (F8), outline arcs flattened (F10), `track_end` teardrops, per-layer hatch offsets (F13) | new |
| 48 | Pads: shared pad numbers on different nets (GAPS 36), custom-pad `gr_poly` arcs flattened (GAPS 31), a circle pad's `rect_delta` | known / low |
| 26 | Dimensions: `text_position_mode` manual becomes automatic, so the text moves | GAPS 2 |
| 26 | `pcbplotparams` layer selection: the Rescue bit (F11) | F11 new |
| 18 | `(legacy_teardrops yes)` on boards with teardrops | by design (batch C), see the notes |
| 12 | KiCad 10 vias get explicit protection `no` tokens | r2 F1 "left", same meaning |
| about 9 | Footprint `(zone_connect 0)` on a logo, empty `(embedded_files)`, stackup `addsublayer` | r2 F17 open |

## Findings

### F1. A no-net zone in a KiCad 10 file becomes a zone on a new net named "0". **high**
**Fixed** on task-r2-misc-leftovers: b52602d9 (`net_of` goes by `sexpr::is_bare_number`, as `parseNet`'s `IsNumber( token )`).

**What differs.**
- `crates/kicad/src/import.rs:520` (`net_of`): when the file has no net table (`net_names.is_empty()`), the one argument of `(net ..)` is taken as a name, quoted or not. The comment says the reader "does not keep whether an argument was quoted".
- That comment is stale. `sexpr.rs:23-29` keeps the `QUOTED` mark on quoted atoms for exactly this reason ("KiCad's lexer takes `1` and `"1"` for two different tokens").
- KiCad 25ec2335 `pcb_io_kicad_sexpr_parser.cpp::parseNet`:
  ```cpp
  // Legacy files (pre-10.0) will have a netcode instead of a netname.  This netcode
  // is authoratative (though may be mapped by getNetCode() to prevent collisions).
  if( IsNumber( token ) )
  {
      if( !aItem->SetNetCode( std::max( 0, getNetCode( parseInt() ) ), /* aNoAssert */ true ) )
  ```
  An unquoted `0` is net code 0, which is no net.

**How to see it.** 25ec2335 `qa/data/pcbnew/issue23467.kicad_pcb` (version 20260206) has `(zone (net 0) (net_name "") (layer "F.Cu") ..)`.
- Our export writes `(net 1 "0")` and `(zone (net 1) (net_name "0") ..)`.
- kicad-cli's rewrite of ours has `(zone (net "0") ..)`.
- The zone is now on a net with no pads, and with `island_removal_mode 0` its fill is removed. The original fills as a netless pour.

**Test.** `import_reads_an_unquoted_zero_net_as_no_net`: import a KiCad 10 file holding a zone `(net 0) (net_name "")` and a pad `(net "0")`. Assert that the zone's net is `""` and that the pad's net is `"0"`.

**Known?** No.

### F2. A copper-thieving zone loses its fill mode and is written as a solid pour. **high**
**Fixed** on task-r2-misc-leftovers: 9e1c1ed1 (`DrawingsSection::zone_thieving`; the board is written as 20260513; slow `kicad_cli_reads_the_thieving_zone_back`; PARITY-pcb section 54's claim corrected).

**What differs.**
- `import.rs:2488-2492` reads `(fill (mode ..))` and keeps only `hatch`. `(mode thieving)` and its `(thieving (type ..) (size ..) (gap ..) (width ..) (stagger ..) (orientation ..))` block are dropped silently, with no `ImportNotes` entry.
- KiCad 25ec2335 `parseZONE`:
  ```cpp
  case T_thieving:
      if( m_requiredVersion < 20260513 )
      {
          Expecting( "segment, hatch or polygon "
                     "(thieving requires file version >= 20260513)" );
      }
      zone->SetFillMode( ZONE_FILL_MODE::COPPER_THIEVING );
  ```
- PARITY-pcb §54 says "A zone with `thieving` (20260513) would still make an unreadable 20241229 file (no corpus board has one)". Both halves are wrong today:
  - a corpus board does have one;
  - the zone is not kept, so the file stays readable, but the copper changes.

**How to see it.** 25ec2335 `zone_thieving_track_bisection.kicad_pcb`:
- orig: `(fill yes (mode thieving) .. (thieving (type hatch) (size 5) (gap 2) (width 0.3) (stagger yes) (orientation 33)))`;
- ours: `(fill yes (thermal_gap 0.5) ..)`, a solid pour over the whole area.

**Test.** `a_thieving_zone_is_kept_or_reported`. Either keep the mode (writing 20260513 for such a board, as F1 of round 2 does for the via protection), or at least name the zone in `ImportNotes` and refuse silent solid copper. Assert that the export never writes a solid `(fill yes` for a zone read as thieving.

**Known?** No (the §54 claim is wrong).

### F3. Three vias of one net at one point: two get the same uuid, and the group of the third loses it. **medium**
**Fixed** on task-r2-misc-leftovers: f7f5adac (a count after the layers while the seed is taken).

**What differs.**
- `crates/kicad/src/pcb.rs:370-384`: the via uuid seed is `via:{net}:{x}:{y}`, and on a clash it is extended once with `:{from}:{to}`.
- A third via at the same point on the same layers clashes again. `duid_for` then hands out the second via's uuid.
- KiCad keeps both items, but `resolveGroups` finds one item per uuid, so a group (or a DRC exclusion) naming the via loses it.

**How to see it.** 25ec2335 `multichannel_nested`:
- three VCC vias at (38.2875, 70.59) in the source, each with its own uuid;
- our `intent.kicad_pcb` has two of them as `50c1b170-fc48-4356-9d4a-db27c0b7e5b8`;
- group "Common emitter amplifier11 (/SecondChannel/)" goes from 52 members to 51 (one via).

**Test.** `vias_at_one_point_get_distinct_uuids`: three identical vias. Assert that the export has three distinct `(uuid ..)` values, and that a group of all three keeps three members after `import_kicad_pcb(export(..))`. Fix: seed with the via id, or loop until the seed is unused.

**Known?** No. The comment at `pcb.rs:370` handles only the two-via microvia case.

### F4. A legacy group lock (`(group "" locked (id ..))`) is lost. **medium**
**Fixed** on task-r2-misc-leftovers: 5d26ee10 (`import_groups` uses `is_locked`, which now takes the bare keyword only, not a quoted string saying `locked`).

**What differs.**
- `crates/kicad/src/import_items.rs:278` (`import_groups`): `locked = sexpr::find(g, "locked").is_some_and(..)` sees only the `(locked yes)` list form.
- The bare `locked` atom that files before 20221130 write after the name is ignored.
- `import_items::is_locked` (line 70) handles both forms, and the footprint groups use it (`import.rs`, 40c2df39). The board groups do not.
- KiCad `pcb_io_kicad_sexpr_parser.cpp::parseGROUP_members` / `parseGROUP`:
  ```cpp
  else if( token == T_locked )
      groupInfo.locked = true;
  ```

**How to see it.** 8303b2ad `reverse_via.kicad_pcb` (20220914): `(group "" locked (id 7dfc5707-..) (members ..4109 items))`. kicad-cli's rewrite of the original has `(locked yes)`; ours has no lock.

**Test.** Extend `kicad10_items.rs`'s group-lock test with `(group "g" locked (id ..) (members ..))`. Assert that the group id is in `locked_ids`.

**Known?** Round-2 F7's lock half is marked Fixed; that fix is incomplete.

### F5. The KiCad 10 microvia-stack generator is "kept as text" but its members name uuids the vias no longer have, so KiCad drops it. **medium**
**Fixed** on task-r2-misc-leftovers: 24fc35aa (`DrawingsSection::raw_generators`: members by our ids, written with the uuids the items were written as; kicad-cli's rewrite keeps both stacks; the board-roundtrip counts `via_stacks` 2 -> 4 lines, the member uuids now being the written ones, which norm.py does not drop).

**What differs.**
- `import.rs:383`: "A generator of a kind nothing here reads (KiCad 10's microvia stack, `via_stack`) goes on as text".
- The text keeps `(members "<file uuid>" ..)`, but the export writes the vias with new uuids (`pcb.rs:384` `duid_for`).
- KiCad's `resolveGroups` then finds no member, and kicad-cli's rewrite of our file has no `(generated (type via_stack) ..)` at all.
- GAPS item 2 says such a generator "is kept as text". That is not what happens.

**How to see it.** 25ec2335 `via_stacks`:
- orig: two `(generated (type via_stack) ..)` with 2 and 3 members;
- ours: `intent.kicad_pcb` holds both with the old member uuids, and kicad-cli's rewrite has none.

**Test.** `an_unread_generator_keeps_its_members`: after export, every uuid in a raw generator's `(members ..)` is the uuid of an item in the file. The fix is the uuid remapping the tuning patterns already use (`written`).

**Known?** No (GAPS says it is kept).

### F6. A footprint read from a file that has no 3D model gets one made up from its name. Pin-header models are guessed from any name with "1x" and trailing digits. **medium**
**Fixed** on task-r2-misc-leftovers: f2521f9f (an explicit footprint with no model has none; `kicad_footprint_for`'s header rule takes `PINHEADER-n`, `PIN-HEADER-n` and names ending in `1X<n>` only).

**What differs.**
- `crates/kicad/src/model3d.rs:231-262` (`default_part_models_in`): an explicit footprint (`explicit == true`) with no `models3d` and no `model` falls through to `kicad_footprint_of_part(part)`.
- `crates/model/src/footprint.rs:1024-1033` reads any key that contains `1X` and ends in digits as `PinHeader_1x<digits>`.
- KiCad writes only the models the footprint has (`format( FOOTPRINT )`: `for( const FP_3DMODEL& model : aFootprint->Models() )`).

**How to see it.**
- `fill_bad`: J1 `lib:SolderWirePad_1x01_SMD_2.9x7.8mm#2`, no model, gets `PinHeader_1x02_P2.54mm_Vertical.step`. The `2` comes from the `#2` variant suffix.
- `issue25101`: `Hera_Kicad_C-Group:C-Group_1x_0603` gets `PinHeader_1x603_..` (a file that does not exist).
- `issue5830`: `programming_header:Pin_Header_Straight_2x03` gets the 1x03 header.
- 6 footprints on 3 boards in all. The STEP/GLB export shows parts the board never had.

**Test.** `an_imported_footprint_without_models_stays_without`: import a board whose footprint `X:Foo_1x03` has no `(model)`, export it, and assert that it has no `(model`. Also check `kicad_footprint_for("LIB:C-GROUP_1X_0603", ..)` is `None` (the generic table should not match full library ids).

**Known?** No. Round-2 F18 is about the installed-art tolerance, not this.

### F7. The round-2 F5 fix puts a legacy footprint's Description field on B.Fab and at angle 0. KiCad has it on F.Fab at the footprint's angle. **low**
**Fixed** on task-r2-misc-leftovers: 20b60bc8 (F.Fab, `(at 0 0 <the footprint's angle, 0..360>)`).

**What differs.**
- `import.rs:1808,1815`: `fab = B.Fab` for a `B.Cu` footprint, and `(at 0 0 0)`.
- KiCad: the Description field is the one `FOOTPRINT::FOOTPRINT` creates (`addField( FIELD_T::DESCRIPTION, F_Fab, false )`), before `(layer ..)` is parsed. `parseFOOTPRINT_unchecked`'s `ki_description` branch only does `footprint->GetField( FIELD_T::DESCRIPTION )->SetText( pValue )`.
- So the field stays on F.Fab. `(at x y a)` then rotates it with the footprint (`SetOrientation`).

**How to see it.**
- `issue12609` C1 (back, 270°): orig `(property "Description" "Unpolarized capacitor" (at 0 0 270) (layer "F.Fab") ..)`, ours `(at 0 0 0) (layer "B.Fab")`.
- Also on `issue14559` and four other boards: 74 lines. The field is hidden, so nothing shows.

**Test.** In `legacy_ki_properties_become_the_description_field`, add a `B.Cu` footprint at 90° and assert `(layer "F.Fab")` and an angle of 90.

**Known?** No: §54 claims "on its Fab layer", which is the wrong layer for back-side footprints.

### F8. A rule area with `(placement ..)` and no `(keepout ..)` is read as allowing everything. KiCad reads it as keeping out tracks, vias and pads. **high (rare)**
**Fixed** on task-r2-misc-leftovers: e4d2db42.

**What differs.**
- `import.rs:2548-2556` sets the keepout flags only from a `(keepout ..)`, so a zone made a rule area by `(placement ..)` alone keeps all-allowed.
- In KiCad, `ZONE::ZONE` copies the default settings, and `ZONE_SETTINGS::ZONE_SETTINGS` sets:
  ```cpp
  SetDoNotAllowZoneFills( false );
  SetDoNotAllowVias( true );
  SetDoNotAllowTracks( true );
  SetDoNotAllowPads( true );
  SetDoNotAllowFootprints( false );
  ```
  `case T_placement: zone->SetIsRuleArea( true );` leaves these flags alone.
- Inside `(keepout ..)`, KiCad resets only pads and footprints ("Initialize these two because their tokens won't appear in older files"), so a missing `tracks`, `vias` or `copperpour` keeps the default. We read a missing key as allowed.

**How to see it.** `vme-wren`: 2 zones `auto-placement-area-/io_drivers_fp/bank0/io01/` and `../io34/` have `(placement (enabled yes) ..)` and no keepout.
- kicad-cli's rewrite of the original: `(keepout (tracks not_allowed) (vias not_allowed) (pads not_allowed) (copperpour allowed) (footprints allowed))`;
- ours: everything `allowed`. KiCad's DRC and router treat the original's areas as keepouts.
- No other corpus board has this.

**Test.** `a_placement_area_without_keepout_keeps_kicads_defaults`: assert `keepout_tracks`, `keepout_vias` and `keepout_pads` are true and `keepout_copper_pour` is false. Also `(keepout (pads allowed))` alone gives tracks and vias kept out.

**Known?** No.

### F9. Teardrop zones are always written `(hatch none ..)`; the file's border style is lost. **low**
**Fixed** on task-r2-misc-leftovers: 5c1af957 (a teardrop with the file's uuid keeps its style; the ones made here are `none`).

**What differs.**
- `crates/kicad/src/pcb_items.rs:95-97` writes `none` for every teardrop.
- `import.rs:2468` reads the style, but the writer ignores it.
- KiCad's `createTeardrop` sets `INVISIBLE_BORDER` only on the teardrops it makes. A file's `(hatch full|edge ..)` is kept (`parseZONE`).

**How to see it.** `issue11814` has 547 teardrop zones going from `full` to `none`, `issue14559` 339 from `edge`, `issue14549` 43. Teardrops of `teardrop_issue_JPC2`, `issue18142` and `issue17967` too: 938 zones in all. This changes the display only.

**Known?** No.

### F10. Arcs in a zone outline are flattened to polylines. **medium**
**Fixed** on task-r2-misc-leftovers: 98b84624 (`DrawingsSection::zone_arcs`: the rings as the file drew them, written while the outline is the one read; an edited outline is its points).

**What differs.**
- `import.rs:2602-2635` (`poly_points`) turns `(pts (arc ..))` into a 5 µm polyline for zones as well as for `gr_poly`.
- KiCad keeps the arcs in the zone's `SHAPE_LINE_CHAIN` and writes them back as `(arc ..)`.

**How to see it.**
- `issue22475`: the GND zone on In1.Cu loses its 6 outline arcs (about 230 points in ours).
- `issue22826`: ten rule areas that are full circles (`(arc (start ..) (mid ..) (end <start>))`) become 11-point polygons.
- The outline moves by up to the 5 µm error, and the file grows.

**Known?** Partly: round 2 and GAPS 27/31 list arcs in `gr_poly`, not in zone outlines.

### F11. The plot settings' layer selection drops the Rescue layer bit. **low**
**Not fixed** on task-r2-misc-leftovers: skipped at the lead's request (the plot settings are task-r3-editors-exports').
**Fixed** on task-r3-editors-exports: a10c9d08 (`Rescue`, id 37, is read and written back; the Plot dialog's checklist never lists it, as `GetEnabledLayers` never has it)

**What differs.**
- `crates/model/src/plot_params.rs:36-45` (`all_layers`) has no `Rescue` (layer id 37), so `decode_layer_selection` drops bit 37 and `encode_layer_selection` cannot write it.
- KiCad's `PCB_PLOT_PARAMS_PARSER` keeps the whole `LSET` it reads (`m_layerSelection.ParseHex`).

**How to see it.** 13 boards. `video-v10`: `0x00000000_00000000_000010fc_ffffffff` becomes `..._000010dc_..`; `pic_programmer`: `..._00000030_..` becomes `..._00000010_..`. These are the 5 boards worse than round 2.

**Known?** No (it came in with X3).

### F12. A footprint whose file names a `(sheetfile)` and no `(sheetname)` is written with two `(sheetfile ..)`. **low**
**Fixed** on task-r2-misc-leftovers: 5c1af957.

**What differs.**
- `pcb.rs:1170-1175` writes the file's own `sheetname`/`sheetfile` from `FootprintExtra`.
- Then `pcb.rs:1181-1187` adds the link's `(sheetname)` and `(sheetfile)` whenever the extra has no `sheetname`.
- KiCad reads the last one, so the file's sheet file is overridden. That is the derived one, and harmless, but the comment's "keeps the sheet the file named" is not what happens.

**How to see it.** `issue11814`: 133 of 134 footprints have `(sheetfile "A05-attenuator.kicad_sch")` and then `(sheetfile "vca821id_u2.kicad_sch")`. There are such footprints on 40 boards.

**Known?** No.

### F13. Smaller differences. **low**
**Fixed in part** on task-r2-misc-leftovers: 9746d13a (a zone's per-layer hatch offsets), e33afc48 (the long user names of a pre-20200922 board: its layers keep their file names, `parseLayer`). Left: a circle pad's `rect_delta` (no effect on a circle), the Edge.Cuts loop's direction (same geometry).

- **A zone's per-layer hatch offsets are dropped.** The offset is `(property (layer "F.Cu") (hatch_position (xy 1 1)))` inside a zone, not in `zone_defaults`. Seen on `api_kitchen_sink` and `Issue19775_ZoneLayers/LayerEnumerate`. Only a hatched fill changes.
- **A circle pad's `rect_delta` is dropped.** `hatch_thermal_connectivity` (`(size 2 2) (rect_delta 0.5 0)` on a circle): KiCad keeps the delta, which has no effect on a circle.
- **Edge.Cuts lines are written with start and end swapped** where the outline was rebuilt as a loop: `simple`, `issue8407`, `issue17559`, `3Rs_bv`, `video-v10`, `issue16658`, `issue23469`. Same geometry.
- **Legacy boards get the long user layer names written.** `custom_pads`, `issue4139`, `issue4257` get `(5 "F.SilkS" user "F.Silkscreen")` where kicad-cli writes none.

## Notes on known items (not counted)

- **`(legacy_teardrops yes)` on 9 boards** whose files say `no` (`issue21482`, `issue11814`, `issue18142`, `issue24649`, `issue25580`, the teardrop QA boards). This is batch C by design, so that kicad-cli's refill keeps the teardrops (`pcb.rs:82`).
  - Side effect: KiCad's `TEARDROP_MANAGER::UpdateTeardrops` (`teardrop.cpp:234`, `if( m_board->LegacyTeardrops() ) return;`) and the pad and via dialogs then treat the user's board as legacy. They disable the teardrop settings and show the "legacy teardrops" warning (`dialog_pad_properties.cpp:1429-1435`).
  - Worth a line in GAPS, since it changes how KiCad edits the board.
- **`track_end` teardrops written as `padvia`** (`issue17967`). This is documented in PARITY-pcb ("A `track_end` teardrop imports as a `padvia` one") but not in GAPS.
- **Tuning pattern `target_delay`.** These differ only where KiCad's own value is garbage (round 2's note).

## Checked and found to match (do not review again)

- **Round-2 fixes, by round trip and by reading against KiCad:**
  - **F1, the file version gate** (`pcb.rs::board_file_version`), against every `m_requiredVersion` gate in 25ec2335's parser between 20241229 and 20261017: 20250210 stroked fill, 20250228 via protection, 20250926 `die_delay` units, 20260513 thieving (F2 above), 20260616, 20260623, 20260826.
  - **F1, the image scale** (`pcb_image::scale_from_file`/`file_scale`) against `parsePCB_REFERENCE_IMAGE` and `BITMAP_BASE::GetLegacyPPI`/`updatePPI` (truncation then `KiROUND( dpiX * 2.54 )`).
  - **F2, masked tracks and shapes** (`cu_item_layers`, `cu_item_layer_words`) against `parseLayersForCuItemWithSoldermask`, `PCB_TRACK::SetLayerSet`/`GetLayerSet` and `PCB_SHAPE::SetLayerSet`/`GetLayerSet`. No track or shape differs in the round trip.
  - **F3 and F6, pad layer sets**, including the pre-9.0 "0 means inherit" filter (`parsePAD`'s `m_requiredVersion <= 20240201` branches). No pad differs except the known causes.
  - **F4** `allow_soldermask_bridges`: no `attr` differences.
  - **F8 zone fill flags:** no `(fill` differences apart from F1 and F2.
  - **F9:** checked against the `m_requiredVersion < 20200826 && attributes == 0` rule. The edge case of an empty `(attr)` in a pre-2020 file is not handled, but no corpus board has one.
  - **F10:** `viasonmask` in file order, as in `case T_pcbplotparams`.
  - **F13:** the empty Reference and Value, as in `FOOTPRINT::FOOTPRINT` and `PCB_TEXT::PCB_TEXT( FOOTPRINT* )` (1.27 mm, F.SilkS and F.Fab at the footprint's position).
  - **F14:** footprint groups (only uuids differ on `issue14559` ICON101).
  - **F16** `gr_bbox`, **F17** the target length.
- **Footprint art, footprint zones, pads, vias, tracks, texts, tables, images and barcodes:** nothing differs beyond the causes listed above.
- **Net chains** (`net_chain_bridging_tuning_profiles`): kept, terminal pads included.
- **Library tables (GAPS item 5, PARITY-common §14, the rows marked identical that no round had reviewed):**
  - `Vars::expand_with` (`library_table.rs:446-560`) against `common.cpp::KIwxExpandEnvVars`: the bracket kinds, `:` in names, the project-then-environment order, the versioned `KICAD*_` fallbacks, the unexpanded-wildcard text, a missing closing bracket, `\$`, and the re-expansion only when the result holds `{(%` and `})%`, with loop detection.
  - `effective_libs` (`library_table.rs:917`) against `LIBRARY_MANAGER::Rows`: chained tables in place, a disabled chained table skipped, the parent's hidden flag passed down, a later nickname winning in the first one's place.
  - One small difference, not counted: KiCad's `GetRow` uses `aIncludeInvalid = true`, so a later invalid row named like an earlier valid one shadows it in KiCad's lookups. Ours skips rows that did not load.
- **Footprint library writer:** `crates/kicad/tests/library_roundtrip.rs` (kicad-cli `fp upgrade` on both sides, `KNOWN` empty) was read; it was not re-run.

## Summary

| # | Finding | Severity | Known? |
|---|---|---|---|
| F1 | Unquoted `(net 0)` in a KiCad 10 file becomes a net named "0" | high | no |
| F2 | Copper thieving zone becomes a solid pour, silently | high | no (§54 claim wrong) |
| F8 | Placement area without `(keepout)` read as allowing all; KiCad keeps out tracks, vias and pads | high (rare: vme-wren) | no |
| F3 | Three vias at one point: duplicate uuid, group member lost | medium | no |
| F4 | Bare `locked` on a legacy group lost | medium | r2 F7 fix incomplete |
| F5 | `via_stack` generator kept with stale member uuids; KiCad drops it | medium | no (GAPS claim wrong) |
| F6 | Imported footprint without a model gets a guessed one (`PinHeader_1x603`) | medium | no |
| F10 | Zone outline arcs flattened | medium | partly |
| F7 | Legacy Description field on B.Fab at 0°; KiCad has F.Fab at the footprint angle | low | r2 F5 fix detail |
| F9 | Teardrop zones' border style always `none` | low | no |
| F11 | Plot layer selection loses Rescue (the 5 boards worse than round 2) | low | no |
| F12 | Duplicate `(sheetfile)` on footprints read from older files | low | no |
| F13 | Zone hatch offsets, circle `rect_delta`, Edge.Cuts direction, long user layer names | low | no |

Counts: **high 3** (F1, F2, F8), **medium 5** (F3, F4, F5, F6, F10), **low 5** (F7, F9, F11, F12, F13).
