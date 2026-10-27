# Round 2 findings: board files, footprints and libraries (r2-board-io, 2026-10-27)

Reviewer: `r2-board-io`, read-only. Code read from main's checkout (`5cca5fff`). KiCad: `$KICAD_SRC_DIR` = `kicad-src-8303b2ad`, and `kicad-src-25ec2335` for KiCad 10 items. No port was used.

## How it was measured

Main's own `target/debug/eda` dates from 2026-10-23, before batches A, B, C and R6. So the binary used was `ds-agentic-eda/target/debug/eda`, built 2026-10-26 23:49 from `4c7fb03f`. That commit's tree is the same as main's (`git diff 4c7fb03f 5cca5fff` is empty). A copy is in `/tmp/findings-r2-board-io.eUqB/eda`.

Every probe went through the same round trip (`/tmp/findings-r2-board-io.eUqB/bin/rt.sh`):
1. Copy the board (and its `.kicad_pro` / `.kicad_dru`) into a scratch folder.
2. Run `eda import-kicad x.kicad_pcb -o imp`.
3. Run `eda board export -C imp --kicad pos`. This writes the derived `.kicad/intent.kicad_pcb`. `eda export` refuses imported boards; that is round 1's p22 F10.
4. Run `kicad-cli pcb upgrade --force` on a copy of the original and on a copy of ours, so kicad-cli writes both files in its own form.
5. Diff the two structurally (`bin/norm.py`). It drops uuids, `tstamp` and zone fills, and puts each footprint child and zone child on a line of its own.

Boards covered:
- all 125 top-level boards of 8303b2ad's `qa/data/pcbnew`;
- its 60 boards in subfolders;
- the 29 boards that only 25ec2335's QA corpus has;
- six small probe boards written for this review (`/tmp/findings-r2-board-io.eUqB/probe/*`).

The results are in `/tmp/findings-r2-board-io.eUqB/rt/<board>/{diff.txt,diff2.txt}`.

**Noise ignored**, either by design (batch A) or already listed in GAPS:
- Every footprint gets a new `(path)` / `(sheetname)` / `(sheetfile)` pointing at the derived schematic (batch A). This one is also finding F12 below.
- Zone outline hatch pitch: GAPS item 2.
- Board line style (`default` vs `solid`, dashes): GAPS item 2, p7-p8 F3.
- `gr_poly` arcs turned into polylines: GAPS items 27 and 31.
- Dimension text position and details: GAPS item 2.
- Pads that share a number but have different nets (stonehenge `H2`: `unconnected-(H2-Pad1)_1/_2/_3` merged into one net): GAPS item 36.
- `-0` written as `0` in model rotations.

## Findings

### F1. Every board is written as file version 20241229. A KiCad 10 board's vias then lose their covering, plugging, capping and filling, and its reference images grow. **high**

**What differs.**
- `crates/kicad/src/pcb.rs:74` always writes `(version 20241229)` and `(generator_version "9.0")`. The board can hold KiCad 10 data that the writer passes through: the setup's `(covering ..) (plugging ..) (capping ..) (filling ..)` (`import.rs:248`, written at `pcb.rs:121-123`), via-stitch generators, and images read from 20260623+ files.
- KiCad 10's parser decides what a missing token means from the file version.
- 25ec2335 `pcb_io_kicad_sexpr_parser.cpp::parsePCB_VIA`:
  ```
  // Versions before 10.0 had no protection features other than tenting, so those features must
  // be interpreted as OFF in legacy boards, not as unspecified (aka: inherit from board stackup)
  if( m_requiredVersion < 20250228 )
  {
      via->Padstack().FrontOuterLayers().has_covering = false;
      ...
      via->Padstack().Drill().is_filled = false;
      via->Padstack().Drill().is_capped = false;
  }
  ```
  A via of a 20250228+ file with no tokens of its own inherits the board's defaults. The same via in our 20241229 file is explicitly off.
- 25ec2335 `parseREFERENCE_IMAGE` (around line 4399):
  ```
  // Before 20260623 the PPI was computed from the embedded resolution but pixels/cm was
  // truncated to an integer, so the stored scale compensated for the wrong PPI.  Re-scale
  // to the corrected PPI to preserve the rendered size of existing boards.
  if( m_requiredVersion < 20260623 ) { ... refImage.SetImageScale( refImage.GetImageScale() * image.GetPPI() / legacyPPI ); }
  ```
  An image from a 20260623+ file is scaled a second time when it is read back.
- Other gates newer than 20241229 in the same parser: `die_delay` (<= 20250926 is read as a length; ours matched by luck of the units), `isStrokedFill` (< 20250210), zone `thieving` (< 20260513 is a parse error, so a zone that has it would make the file unreadable; no corpus board has one).

**How to see it.**
- Probe `/tmp/findings-r2-board-io.eUqB/probe/viacov/viacov.kicad_pcb`: QA `issue23690_via_in_pad` (version 20260410) with the setup changed to `(covering (front yes) ..) (filling yes)`. kicad-cli's upgrade of the original leaves both vias with no protection tokens, so they inherit yes. kicad-cli's upgrade of our export writes `(covering (front no) (back no)) (plugging ..) (filling no) (capping no)` on both.
- Across the corpus: 25 vias of six 8303b2ad boards, 244 of `ultrasound`, and the via-stitch generators of `issue25265`, `issue25303` and `stitch_fp_rulearea`.
- Image probe `/tmp/findings-r2-board-io.eUqB/probe/img/img.kicad_pcb`: version 20261017, a 40x40 PNG at 3780 px/m (96 dpi), `(scale 1)` written as no scale. kicad-cli's upgrade of the original keeps scale 1. The upgrade of our export says `(scale 1.02128)`: the image is 2.1% larger.
- Tests to write (`crates/kicad`, slow, needs kicad-cli):
  - `a_kicad10_via_keeps_inheriting_the_board_protection`: import the viacov board, export, run `kicad-cli pcb upgrade`, and assert no via block contains `(covering`.
  - `reference_image_scale_survives_a_kicad10_round_trip`: the same for the image board, asserting no `(scale` in the image block.

**Fix direction.** Write the version that matches the tokens. When the design carries KiCad 10 data (setup via defaults, an image read from a 20260623+ file, a generator), either write a 20260623+ version and KiCad 10's forms, or write explicit per-via tokens and undo the PPI rescale.

**Already known?** No. PARITY-pcb mentions 20241229 only for the clipboard.

### F2. A track or arc with solder mask (`(layers "B.Cu" "B.Mask")`) is moved to F.Cu and loses its mask. A copper shape with mask is moved to Cmts.User. **high**

**What differs.**
- `import.rs:2064` (segments) and `import.rs:2075` (arcs) read only `(layer ..)`, with `unwrap_or("F.Cu")`.
- `import.rs:2179` (board shapes) reads only `(layer ..)`, with `unwrap_or("Cmts.User")`.
- KiCad 9 writes a masked track or shape with `(layers ..)`.
- `pcb_io_kicad_sexpr.cpp::format( PCB_TRACK )`:
  ```
  if( aTrack->GetLayerSet().count() > 1 )
      formatLayers( aTrack->GetLayerSet(), false /* enumerate layers */ );
  else
      formatLayer( aTrack->GetLayer() );
  if( aTrack->HasSolderMask() && aTrack->GetLocalSolderMaskMargin().has_value() && IsExternalCopperLayer( aTrack->GetLayer() ) )
      m_out->Print( "(solder_mask_margin %s)", ...
  ```
- The parser `parseARC` / `parsePCB_TRACK`: `case T_layers: arc->SetLayerSet( parseLayersForCuItemWithSoldermask() );`. `format( PCB_SHAPE )` does the same at line 1100.

**How to see it.**
- QA `issue18878` (version 20241007): segment `(start 32.249511 -85.850489) (end 32.249511 -54.014785) (layers "F.Cu" "F.Mask") (solder_mask_margin 1)` comes back `(layer "F.Cu")`, with no mask opening.
- Probe `/tmp/findings-r2-board-io.eUqB/probe/trk/trk.kicad_pcb`: a segment and an arc on `(layers "B.Cu" "B.Mask")` net A are both written `(layer "F.Cu")`. That is wrong connectivity: the copper changed side.
- Probe `/tmp/findings-r2-board-io.eUqB/probe/shp/shp.kicad_pcb`: a `gr_rect` and a `gr_line` on `(layers "B.Cu" "B.Mask")` with `(net 1)` are written on `Cmts.User`. The copper is gone.
- Test to write: `import_reads_a_masked_track_on_its_copper_layer`. Input: the probe board. Assert `routing.tracks[0].layer == "B.Cu"` and that the export contains `(layers "B.Cu" "B.Mask") (solder_mask_margin 0.2)`.

**Already known?** No.

### F3. A through-hole pad's layers are rebuilt as `*.Cu *.Mask`: paste is lost and a one-sided mask becomes two-sided. An edge-connector (`connect`) pad always gets mask on both sides. **high**

**What differs.**
- `pcb.rs:1334-1343`: a TH or NPTH pad with copper is written as `"*.Cu" "*.Mask"`, `"*.Cu"`, or that plus its art layers.
- `pcb.rs:1361-1366`: a connector pad is written as `cu` plus `["F.Mask","B.Mask"]` ("an edge connector pad has mask on both sides and no paste").
- KiCad writes the pad's own set: `pcb_io_kicad_sexpr.cpp::format( const PAD* )` line 1826, `formatLayers( aPad->GetLayerSet(), false )`. The reader keeps it (`PadMaskInfo::layers`, `ir.rs:2666`), but these branches do not use it.

**How to see it.**
- `issue8407`: `J1` pads `(layers "*.Cu" "*.Paste" "*.Mask")` come back as `"*.Cu" "*.Mask"`. The stencil loses 34 openings.
- `connect`, `issue23469`: TH pads on `F.Paste` lose it.
- `issue14559`: `J1901`'s TH pads on `"*.Cu" "F.Mask"` become `*.Mask`.
- `stacked`, `issue22475`, `issue25101`, `stickhub-extra-via`: `connect` pads on `F.Mask` (or `B.Mask`) become `"*.Mask"`. The other side's mask gets an opening over copper the pad does not have.
- Test to write: `a_through_hole_pad_keeps_its_paste_and_one_sided_mask`. A board with `(pad "1" thru_hole circle .. (layers "*.Cu" "F.Mask" "*.Paste"))` and `(pad "2" connect rect .. (layers "F.Cu" "F.Mask"))`. Assert both `(layers ..)` round-trip unchanged.

**Already known?** No. Batch B fixed SMD pads on `*.Cu` and paste-only pads (p22 F2, p7-p8 F2) only.

### F4. The footprint attribute `allow_soldermask_bridges` is read but never written: kicad-cli then reports solder-mask bridges that the original does not have. **high**

**What differs.**
- `import.rs:1775` reads it into `FootprintExtra::allow_soldermask_bridges`.
- The board writer's `fp_fields.rs:124 write_attr` writes `FootprintAttrs`, which has no such flag, and `pcb.rs` never writes the extra's.
- KiCad, `pcb_io_kicad_sexpr.cpp::format( FOOTPRINT )`: `if( aFootprint->AllowSolderMaskBridges() ) m_out->Print( " allow_soldermask_bridges" );`. The DRC uses it in `drc_test_provider_solder_mask.cpp` lines 346, 419, 493 (`if( itemFP->AllowSolderMaskBridges() )`).
- The library writer (`footprint_lib.rs:388`) does write it.

**How to see it.** `kicad-cli pcb drc` on each project's copies (`rt/<b>/drc_{orig,ours}.json`):

| board | original | our export |
|---|---|---|
| `issue1358` | `{}` | `solder_mask_bridge: 20` |
| `severities` | `copper_edge_clearance 66, isolated_copper 1` | the same plus `solder_mask_bridge: 4` |

The flag is also lost on `stonehenge`, `teardrop_elongated_pad`, `teardrop_issue_JPC2` and `net_tie_drc`.

Test to write: `allow_soldermask_bridges_survives_export`. Input: `(footprint "X" .. (attr smd allow_soldermask_bridges) ..)`. Assert the export contains `(attr smd allow_soldermask_bridges)`.

**Already known?** No.

### F5. Legacy `ki_description` and `ki_keywords` properties are passed through into a newer file: KiCad reads them as visible user fields, and the Description field comes back empty. **high**

**What differs.**
- `import.rs:1723` keeps `(property "Datasheet"|"Description"|"ki_fp_filters"|"ki_keywords"|"ki_locked"|"ki_description" ..)` as raw tokens, whatever the file version.
- `pcb.rs:1153-1155` writes them back verbatim into a 20241229 file.
- KiCad, `pcb_io_kicad_sexpr_parser.cpp::parseFOOTPRINT_unchecked`, `case T_property`:
  ```
  if( m_requiredVersion < 20230620 )
  {
      if( pName == "ki_keywords" || pName == "ki_locked" ) { NeedRIGHT(); break; }
      if( pName == "ki_description" ) { footprint->GetField( FIELD_T::DESCRIPTION )->SetText( pValue ); ... }
  ...
  // Hide the field by default if it is a legacy field that did not have text effects applied
  if( m_requiredVersion < 20230620 ) field->SetVisible( false ); else field->SetVisible( true );
  ```
  In our 20241229 file, `ki_description` and `ki_keywords` become new user fields: visible, on F.Fab, styled from the board settings. The legacy bare `(property "Datasheet" "x")` turns visible too.

**How to see it.** QA `issue14334` (version 20221018), `J1`. kicad-cli's upgrade of the original has `(property "Description" "Generic connector, single row, 01x02 ...")`. The upgrade of ours has `(property "Description" "" ..)` plus a visible `(property "ki_description" "Generic connector ..." (at 0 0 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))` and a visible `ki_keywords`. This happens on 125 footprints of `issue14549`, `issue14559`, `issue12609`, `issue14294`, `issue14334`, `issue14549_2` and `issue14130`.

Test to write: `legacy_ki_properties_become_the_description_field`. Input: a 20221018 footprint with `(property "ki_description" "D") (property "ki_keywords" "k")`. Assert the export has no `"ki_description"` or `"ki_keywords"`, and has a Description field of `D`.

**Already known?** No. `fp_fields.rs:280` handles the field side correctly; the raw token copy is the leak.

### F6. A mask-only NPTH (mounting hole) loses its solder-mask margin, zone connection and any other tokens, and is always written `(layers "*.Mask")`. **high**

**What differs.**
- `import.rs:1877-1888 layerless_hole` takes every `np_thru_hole` with no copper and keeps only the geometry and `(clearance)`.
- `pcb.rs:1460` writes `(layers "*.Mask")` and the clearance.
- What is lost: `(solder_mask_margin ..)`, `(zone_connect ..)`, paste margins, and a one-sided or empty layer set (an `F.Mask`-only hole, or `(layers)`, becomes `*.Mask`).
- KiCad's `format( PAD )` writes the set and the margins as they are.

**How to see it.**
- `issue5990`: `MH1`/`MH2`, `(pad "" np_thru_hole circle (size 5 5) (drill 2.7) (layers "*.Mask") (solder_mask_margin 1.2) (clearance 1.2))`, come back with no margin. The mask opening shrinks from 7.4 mm to 5 mm across.
- `issue14559`: `SWB101`'s holes lose `(zone_connect 0)`. This one changes nothing for a hole.
- Test to write: `a_mask_only_hole_keeps_its_margin_and_layers`. Assert `(solder_mask_margin 1.2)`, and that `(layers "F.Mask")` round-trips.

**Already known?** GAPS item 27 says the hole "is kept and written". The margin and layer loss are not listed.

### F7. A group of one item is dropped, so a placement rule area names a group that is no longer there. A group's `(locked yes)` is always lost. **high / medium**

**What differs.**
- `import_items.rs:211-212`:
  ```
  // A group of one is not a group, unless it is linked to a design block
  let built = if members.len() < 2 && !(members.len() == 1 && !g.lib_id.is_empty()) { Built::Dropped(members) } ...
  ```
- KiCad's parser (`resolveGroups`, `pcb_io_kicad_sexpr_parser.cpp:1530`) keeps every group. The `< 2` rule is only in `group_tool.cpp` (Remove from Group: `if( group->GetItems().size() < 2 ) { group->RemoveAll(); m_commit->Remove( ... ) }`), an edit, not a load.
- `import_items.rs:154`: `let (groups, _locked_groups) = build_groups(...)`. The ids `Walk` collects for `(locked yes)` groups are thrown away, so a locked group is unlocked on export.

**How to see it.**
- 25ec2335 QA `issue22983`: groups `DestA` and `DestB` (one member each) are gone from our file. The rule areas `RuleAreaDestA/B` still say `(placement (enabled yes) (group "DestA"))`, so KiCad's Repeat Layout finds no target. High: data loss, and KiCad reads the file differently.
- `groups_load_save`, `reverse_via`, `issue22475`, `issue22826`: `(group "GroupName" (locked yes) ..)` comes back with no lock. Medium.
- Tests to write:
  - `a_group_of_one_survives_import`: `(group "G" (uuid ..) (members "<one uuid>"))`, assert one group.
  - `a_locked_group_stays_locked`: assert `drawings.locked_ids` contains the group's id, and the export has `(locked yes)` in the group.

**Already known?** No. GAPS item 17 lists the `< 2` rule for verbs only (review M1).

### F8. A copper zone is always written `(fill yes ..)`, even when the file's zone was unfilled. **medium**

**What differs.**
- `pcb_items.rs:153-156` writes `(fill yes` for every zone that is not a rule area. The IR has no filled flag.
- KiCad, `pcb_io_kicad_sexpr.cpp::format( ZONE )`:
  ```
  m_out->Print( "(fill" );
  // Default is not filled.
  if( aZone->IsFilled() ) m_out->Print( " yes" );
  ```
- KiCad uses `IsFilled()` in two places:
  - `PCB_CONTROL::unfilledZoneCheck` shows the "zones are not filled" info bar: `if( !zone->GetIsRuleArea() && !zone->IsFilled() )`.
  - `ZONE_FILLER_TOOL::ZoneFillDirty` refills dirty zones: `if( !zone->IsFilled() || m_dirtyZoneIDs.count( zone->m_Uuid ) )`.

  So an unfilled zone written "filled with nothing" gets no warning and is skipped by the automatic refill.

**How to see it.** `vme-wren` has 125 unfilled zones: no `filled_polygon`, `(fill (thermal_gap ..) ..)`. Our export writes 147 `(fill yes ..)` and no fill. The same goes for `issue14559`'s unfilled zones.

Test to write: `an_unfilled_zone_is_written_unfilled`. Assert `(fill (thermal_gap` and no `(fill yes`.

**Already known?** No.

### F9. A footprint with no `(attr ..)` gets one made up from its pads (`smd` or `through_hole`). KiCad leaves it "unspecified". **medium**

**What differs.**
- `fp_fields.rs:304`: `edit.attrs = sexpr::find(fp, "attr").map(parse_attrs)` gives `None` when there is no attr.
- `fp_edit.rs:507-513 effective_attrs` then derives the kind from the pads (`FootprintAttrs::derived`).
- KiCad (`parseFOOTPRINT_unchecked`) sets attributes 0 for a 20200826+ file with no `(attr)`. It forces `FP_THROUGH_HOLE` only for older files: `if( m_requiredVersion < 20200826 && attributes == 0 ) attributes |= FP_THROUGH_HOLE;`.

**How to see it.**
- 25ec2335 `8851_HW-U1-VCU118_REV2-0_071417` (version 20260101): 2055 footprints with no `(attr)` are written `(attr through_hole)`.
- Others: `issue12609` `TW1-3` get `(attr smd)`, `large_circle_teardrop` `P1` gets `(attr smd)`, `padstacks_complex` gets `through_hole`.
- This changes Footprint Properties' "Component type", which footprints `kicad-cli pcb export pos --smd-only` takes, and whether the footprint type check runs.
- Test to write: `no_attr_means_no_attr`. A 20240108 footprint with no `(attr)` exports with no `(attr` line.

**Already known?** No.

### F10. Legacy `(pcbplotparams (viasonmask true))` is not turned into via tenting off. **medium**

**What differs.**
- `import.rs:818-822` reads only `(setup (tenting ..))`.
- KiCad, `parseSETUP`, `case T_pcbplotparams`:
  ```
  if( plotParams.GetLegacyPlotViaOnMaskLayer().has_value() )
  {
      bool tent = !( *plotParams.GetLegacyPlotViaOnMaskLayer() );
      m_board->GetDesignSettings().m_TentViasFront = tent;
      m_board->GetDesignSettings().m_TentViasBack = tent;
  }
  ```

**How to see it.** `issue5320` (version 20230620, `(viasonmask true)`). kicad-cli's upgrade of the original says `(tenting (front no) (back no))`. Ours says `(tenting (front yes) (back yes))`, so every via's mask opening is gone from the Gerbers.

Test to write: `viasonmask_true_untents_the_vias`.

**Already known?** No.

### F11. The board's plot settings are thrown away and written as fixed defaults. **medium**

**What differs.**
- `pcb.rs:147-160` and on write a fixed `(pcbplotparams (layerselection 0x00010fc_ffffffff) ... (svgprecision 4) ...)`. Nothing is read from the file.
- Compared with kicad-cli's view of the original, across 117 of 125 boards: `layerselection` differs on 117, `svgprecision` on 54, `drillshape` on 47, `outputdirectory` on 34, `creategerberjobfile` on 31, the Gerber attribute flags on 22-26, `subtractmaskfromsilk` on 15, `useauxorigin` on 6.
- An imported project's Plot dialog (and `kicad-cli pcb export gerbers --board-plot-params`) then uses settings the user never chose: another output directory, the aux origin off, `subtractmaskfromsilk` off.

Test to write: `plot_params_round_trip`. Read `(pcbplotparams (outputdirectory "gerbers/") (useauxorigin yes) ..)` and assert both are written back.

**Already known?** No.

### F12. `eda import-kicad x.kicad_pcb` does not take the `x.kicad_sch` beside it, and the export points every footprint at a schematic the project never had. **medium (possibly by design)**

**What differs.**
- `crates/cli/src/import_kicad.rs` pairs only foreign formats (Eagle, CADSTAR, Altium; lines 26 and 173). A KiCad board is read alone.
- The export then writes `(path "/<derived uuid>")`, `(sheetfile "intent.kicad_sch")` and `(sheetname "/")`, or made-up sheets like `"/Misc/"`, for every footprint (`pcb.rs:1175-1183`).
- KiCad's Open Project loads both files and keeps the link.

**How to see it.** `custom_fields` (it has a `.kicad_sch`): `R1` goes from `(path "/8e57...")`, sheet file `custom_fields.kicad_sch`, to `(path "/b6c4...") (sheetfile "intent.kicad_sch") (sheetname "/")`. Opening our export beside the user's own schematic and running Update PCB (F8, matching by path) would match no footprint. This is the batch A symptom again, now for imported projects.

**Already known?** Not listed. Batch A's notes say "a footprint read from a file keeps the sheet the file named", but the path is replaced anyway.

### F13. A footprint with an empty Reference is renamed `FP<n>`, and its Reference and Value texts are re-laid out. **medium**

**What differs.** `import.rs:1571` and `:2288`: `footprint_field(fp, "Reference").filter(|s| !s.is_empty()).unwrap_or_else(|| format!("FP{}", idx + 1))`. The export writes Reference and Value as `"FP1"`, at default places and sizes (`(at 0 -1.8)`, 1 mm, 0.15 mm thick), instead of the file's `""` at `(0 0)`, 1.27 mm.

**How to see it.** `issue23123_minimal`, `issue22751`, `issue23467`, `issue23535_minimal`. KiCad keeps an empty reference; a footprint without one is legal (a logo, a mounting hole).

**Already known?** No.

### F14. A footprint's own groups are dropped, and a board group loses its tuning pattern member. **medium**

**What differs.**
- `import.rs:1718` lists `"group"` among the footprint children it owns (`OWN`), but no reader takes it in, so `(footprint .. (group "" (members ..)))` is lost. Seen on `issue14559` `ICON101`, two groups.
- `write_groups` (`pcb_items.rs:352`) names only items in `written`, and tuning patterns (`(generated ..)`) are not among them. In 25ec2335 `multichannel_nested`, each of the nine design-block groups loses one member, the tuning pattern (for example "Common emitter amplifier2 (/FirstChannel/)": 47 members become 46).
- KiCad's `resolveGroups` adds generators as group members (`groupTypeObjects` holds both kinds).

**Already known?** Partly: GAPS item 24 says Repeat Layout does not copy tuning patterns. The file-level membership loss is not listed.

### F15. Pre-version-8 user fields lose their position and their mirror justification. **low**

- `simple` (20220914): 427 hidden fields are written `(at 0 0 0)`; kicad-cli's view of the original has them at the footprint's place.
- `stonehenge` (B.Fab): fields lose `(justify mirror)`.
- They are hidden, so nothing shows.

### F16. A pad's `gr_bbox` (the number box, a proxy item) is written with no `(fill ..)`, and KiCad reads it as filled. **low**

- `pad_shape.rs:228` writes `" (gr_bbox (start {}) (end {}))"`.
- KiCad's `formatPrimitives` writes `(fill yes|no)` for rectangles, including the proxy one.
- `issue14559`: `J1001` and others go from `(gr_bbox .. (fill no))` to `(fill yes)`.

### F17. Smaller losses. **low**

- A footprint-level `(zone_connect 0)` on a padless footprint (a logo: `issue14549` `Logo1`).
- An empty footprint `(embedded_files)` (`issue25265`, `issue25303`).
- A stackup `addsublayer` (`zone_filler`).
- The board's enabled technical layers: every Adhes, Paste, Mask, SilkS and Fab layer is written even when the original had them off (`issue18839`, `issue14130`).
- A tuning pattern's `target_length` of 1000000 mm (the "unconstrained" value of the skew patterns in 25ec2335 `test_proj`, `diff_pair_uncoupled_tuning_drc`, `multichannel_nested`) is clamped to 2147.483637 mm. KiCad keeps a `long long` (`pcb_tuning_pattern.cpp`: `long long int val; aProps.get_to_iu( "target_length", val )`).

### F18. Note on the R6 change (installed footprints carried whole). **low (design note)**

- `fp_art_io.rs:265 same_frame` accepts a library footprint whose pads are within 0.1 mm of the built-in's.
- Routing that already exists in a `design.json` made with the built-in pads then meets pads moved by up to 100 um.
- Whether the installed footprint is used depends on the KiCad libraries being found when the model is loaded (`attach_installed_art` is best-effort). The same `design.json` therefore exports different pads on a machine without the libraries.
- Through-hole pads of an installed footprint go through F3's writer, so an installed THT footprint with paste or one-sided mask would fail kicad-cli's library parity (`lib_footprint_mismatch`).
- Not measured on a real part: no built-in maps to such a footprint today.

## Checked and found to match (do not review again)

Round trip through import and export of all 214 corpus boards, judged by kicad-cli's own rewrite. Apart from the findings above and the known gaps listed at the top, nothing differed in:
- **Footprints:** placement, side and turn, including back-side footprints in the library frame (`f9b2dc52`) and KiCad 10 `(transform ..)` boards.
- **Footprint art:** every `fp_*` graphic and text, the courtyard written item by item (R6), and texts with knockout, bold and italic.
- **Pads:** SMD pads (including multi-layer SMD, batch B), paste-only and mask-only aux pads (batch B), custom pads and their primitives, padstacks (`padstacks`, `padstacks_normal`, `padstacks_complex`), pin function and pin type, `die_delay` (also from a 20261017 copy), and pad `(teardrops ..)` tokens.
- **Vias:** type, padstack, tenting, `zone_layer_connections`. Only F1's version effect differs.
- **Zones:** holes (batch B), keepouts, priorities, thermal settings, island modes, teardrop zones and their attributes (batch C), placement rule areas, a footprint's own zones.
- **Board shapes:** `gr_curve` with a control point on an end (batch B), rectangle corner radius (batch B), the footprint's Edge.Cuts kept in the footprint (batch B).
- **Board items:** text, text boxes, tables, points, barcodes, reference images (pixels byte for byte; the scale only as in F1), and dimensions (except the known details).
- **3D models:** offset, scale, rotation, hide and opacity; the Extruded 3D Body is kept (batch B).
- **Footprint data:** component classes, units, net-tie pad groups, jumper pad groups, `duplicate_pad_numbers_are_jumpers`.
- **Generators:** via-stitch and tuning generators, apart from F1 and F17.
- **KiCad's own default for a missing `target_delay`:** kicad-cli takes it from a stale variable in `PCB_TUNING_PATTERN::SetProperties` (the previous `get_to_iu` value). Our 1000000 / 0 differs only because KiCad's value is garbage, so this is not reported.
- **Library tables (item 5):** `library_table.rs`'s `expand_with` handles `$NAME`, `${NAME}` and `$(NAME)` and the versioned wildcard names, the same as `ExpandEnvVarSubstitutions`. Spot-checked only.

## Summary

| # | Finding | Severity | Known? |
|---|---|---|---|
| F1 | File version 20241229 with KiCad 10 data: vias lose protection, images rescale | high | no |
| F2 | A masked track or arc moves to F.Cu; masked copper shapes move to Cmts.User | high | no |
| F3 | TH pad layers rebuilt: paste lost, one-sided mask made two-sided; `connect` pads get both masks | high | no |
| F4 | `allow_soldermask_bridges` not written: +20 and +4 `solder_mask_bridge` in kicad-cli | high | no |
| F5 | Legacy `ki_description` and `ki_keywords` written as visible fields; Description emptied | high | no |
| F6 | Mask-only NPTH loses its mask margin and layer set | high | no |
| F7 | One-member groups dropped (rule area orphaned: high); group locks lost (medium) | high / medium | no |
| F8 | Unfilled zones written `(fill yes)` | medium | no |
| F9 | `(attr)` invented for footprints without one | medium | no |
| F10 | Legacy `viasonmask true` not turned into tenting off | medium | no |
| F11 | Plot settings replaced by fixed defaults | medium | no |
| F12 | Board import ignores the project's schematic; every path rewritten | medium | no |
| F13 | Empty references renamed `FP<n>`, texts re-laid out | medium | no |
| F14 | Footprint groups dropped; tuning pattern left out of its group | medium | partly |
| F15 | Legacy hidden field positions and mirror | low | no |
| F16 | `gr_bbox` fill flag | low | no |
| F17 | Small losses (footprint zone_connect, empty embedded_files, sublayer, enabled layers, target length clamp) | low | no |
| F18 | R6 design note (0.1 mm frame tolerance, depends on the libraries being found) | low | no |

Counts: **high 7** (F1-F6 and F7's group half), **medium 8** (F7's lock half and F8-F14), **low 4** (F15-F18).
