# Findings round 2: PCB editing and its UI (`r2-pcb-edit`)

Reviewer `r2-pcb-edit`, 2026-10-27. Findings only, no code changed. Read from main's checkout (`/home/team_braneboard_com/work/agentic-eda`, HEAD 5cca5fff) against KiCad `8303b2ad` (`/home/team_braneboard_com/src/kicad-src-8303b2ad`; KiCad 10 items against `25ec2335`). Scratch dir: `/tmp/findings-r2-pcb-edit.1Opb`. Nothing was built or run (the built `target/debug/eda` predates batch F); every finding comes from reading our code against KiCad's C++.

Scope: GAPS items 8, 9, 10, 14, 15, 16, 17, 18, 21, 22, 29, 35. Priority (a): batch F (`task-fix-f-editing`, the p6 fixes F1-F18 and the group commits); (b) PARITY-pcb rows marked ported or identical that round 1 (`docs/parity/findings/p6-pcb-edit.md`, `p11-p18.md`) did not review. Paths on our side are from the repo root; `web/` means `web/studio/src/`.

Four parts: R (batch F and the edit tool, dimensions, Swap Layers), A (Properties panel, groups, Create Array, property dialogs), B (context menu, Find, Net Inspector, Appearance), C (snap, selection, edit tools per kind, footprints, smaller tools, global edits). Each part ends with its own "checked and found to match" list.

---

# Part R: batch F fixes, the edit tool, dimensions, Swap Layers

## Batch F (task-fix-f-editing) and the edit tool: findings

### R1 (high). Delete with a lone zone selected and the cursor inside one of its cutouts deletes the whole zone; KiCad removes only that cutout
- **What differs.** `EDIT_TOOL::DeleteItems` (`pcbnew/tools/edit_tool.cpp`), `case PCB_ZONE_T:`
  ```cpp
  // We process the zones special so that cutouts can be deleted when the delete
  // tool is called from inside a cutout when the zone is selected.
  // Only interact with cutouts when deleting and a single item is selected
  if( !aIsCut && aItems.GetSize() == 1 )
  {
      VECTOR2I curPos = getViewControls()->GetCursorPosition();
      ZONE*    zone = static_cast<ZONE*>( board_item );
      int outlineIdx, holeIdx;
      if( zone->HitTestCutout( curPos, &outlineIdx, &holeIdx ) )
      {
          // Remove the cutout
          commit.Modify( zone );
          zone->RemoveCutout( outlineIdx, holeIdx );
          zone->UnFill();
          ...
          // Restore the selection on the original zone
          m_toolMgr->RunAction<EDA_ITEM*>( ACTIONS::selectItem, zone );
          break;
      }
  }
  // Remove the entire zone otherwise
  commit.Remove( board_item );
  ```
  Ours: `boardDeleteCmds` (`web/studio/src/kicad-port/deleteCmds.ts:78-91`) sends `delete_zone` for every zone id, with no look at the cursor or at `zone.holes`. Nothing in `web/studio/src` or `crates/ops/src` removes a single hole (`grep -i "cutout"` finds only the Zone > Add Cutout tool and the fill). The Interactive Delete tool (`PCB_CONTROL::InteractiveDelete`, which hands one item to the same `DeleteItems`) has the same gap.
  Note on the undo step: a cutout removal does not count in `itemsDeleted`, so with nothing else deleted none of the `commit.Push` calls at the end of `DeleteItems` runs, and `COMMIT::~COMMIT` (`common/commit.cpp:39`) only frees the copies: at 8303b2ad KiCad removes the cutout with no undo entry, which looks like a KiCad bug. A port should make it one undo step (a deliberate, documented difference) rather than copy that.
- **How to see it.** Draw a zone, add a cutout (Zone > Add Cutout), select the zone, put the cursor inside the cutout, press Delete: KiCad leaves the zone with the cutout gone; we delete the zone. Test: `deleteCmds.test.ts` `a lone zone deleted from inside a cutout loses the cutout only`: `boardDeleteCmds(board, ["zone_a"], { cursor: <a point inside holes[0]> })` returns one `set_zone_outline` for `zone_a` with `holes` lacking that ring, and no `delete_zone`. With two items selected, or the cursor outside every hole, it stays `delete_zone`.
- **Severity.** High: the wrong result (the whole zone goes where KiCad keeps it).
- **Already known?** No (not in GAPS items 8, 13, 35 nor in round 1).

### R2 (high). Rotate turns a dimension's text the wrong way (dimensions with "Keep text aligned" off)
- **What differs.** `PCB_DIMENSION_BASE::Rotate` (`pcbnew/pcb_dimension.cpp`): `EDA_ANGLE newAngle = GetTextAngle() + aAngle; newAngle.Normalize(); SetTextAngle( newAngle );` with `aAngle` counter-clockwise (KiCad's angles).
  Ours: `rotate_dimension` (`crates/connectivity/src/dimension.rs:467-473`) does `dim.text_angle = (dim.text_angle + angle_millideg).rem_euclid(360_000)`, where `angle_millideg` is our clockwise-positive turn (`rotate_about` turns (1, 0) to (0, 1), the test at `dimension.rs:624` says "90 degrees clockwise"; the studio sends `angle_millideg: -turnCcw*1000`, `pcbTransform.ts`/`pcbCarry.ts`). But `Dimension::text_angle` is KiCad's counter-clockwise file angle: it is read from the label's `(at x y a)` (`crates/kicad/src/import_items.rs` `text_angle`) and written back as is (`compute_dimension_geometry` -> `pcb_items.rs:340`), and the kept-aligned angle is computed as `-angle_of(half)` (counter-clockwise, `dimension.rs:265`). A plain text does it the right way (`transform_text`: `t.angle - angle`, `crates/ops/src/pcb_transform.rs:459`); the dimension adds where it should subtract. Called from `transform_dimension` (`pcb_transform.rs:482-485`) for `rotate_items`, Move Exactly's turn and R while carrying.
  Only a dimension whose text is not kept aligned shows it (with `keep_text_aligned` the angle is recomputed from the crossbar). That is every dimension imported from a file without `(keep_text_aligned yes)` ("new format: default to keep text aligned off unless token is present", our importer too) and every leader.
- **How to see it.** A leader (or an aligned dimension with Keep text aligned off) with its text at 0 degrees, R once (90 degrees counter-clockwise): KiCad's text reads at 90 degrees and the file has `(gr_text ... (at x y 90))`; ours is at 270 (upside down relative to KiCad's) and the file says 270. Test: `pcb_transform_tests.rs` `rotating_a_dimension_turns_its_text_the_way_a_text_turns`: a `Dimension { keep_text_aligned: false, text_angle: 0, .. }` and a `Text { angle: 0 }`, `Cmd::RotateItems { angle_millideg: -90_000 }` on both; assert `dimension.text_angle == 90_000 == text.angle`.
- **Severity.** High: the file KiCad reads has the text turned the other way.
- **Already known?** No. (GAPS item 35 and PARITY section 18 still say "Rotate/Flip of a dimension" is not ported; it is, see R10.)

### R3 (high). A dimension's unit-dependent precision (V_VV .. V_VVVVV) is changed on the round trip
- **What differs.** `DIM_PRECISION` (`pcbnew/pcb_dimension.h`) has `X` .. `X_XXXXX` (0-5) and `V_VV` .. `V_VVVVV` (6-9), whose decimals depend on the units: `PCB_DIMENSION_BASE::GetValueText` `if( precision >= 6 ) { case INCH: precision = precision - 4; case MILS: precision = std::max( 0, precision - 7 ); case MM: precision = precision - 5; }` (labels "0.00 in / 0 mils / 0.0 mm" for V_VV). Ours reads 6-9 as `p - 4` (`crates/kicad/src/import_items.rs:327-331`, the inch count) and writes `d.precision` back (`crates/kicad/src/pcb_items.rs:289-293`): a `(precision 6)` mm dimension comes back as `(precision 2)` = X_XX, so KiCad shows 2 decimals where it showed 1 (and a mils one 2 where it showed 0).
- **How to see it.** Import a board with `(dimension (type aligned) ... (format (units 2) (precision 6) ...))` and export it unchanged: KiCad's file says `(precision 6)`, ours `(precision 2)`; `kicad-cli pcb export svg` of the two shows "12.3" against "12.30". Test: `crates/kicad` round trip `a_unit_dependent_dimension_precision_survives_the_round_trip`: import then write, assert `(precision 6)`.
- **Severity.** High by the brief's rule (KiCad reads a different precision back); the visible effect is the number of decimals.
- **Already known?** In part: PARITY section 18 records the collapse to a flat count as a deliberate simplification, but not that it rewrites an imported board's precision.

### R4 (medium). Mirror skips footprint fields, points and tuning patterns
- **What differs.** `EDIT_TOOL::MirrorableItems` (`edit_tool.cpp:2643`) is `{ PCB_SHAPE_T, PCB_FIELD_T, PCB_TEXT_T, PCB_TEXTBOX_T, PCB_ZONE_T, PCB_PAD_T, PCB_TRACE_T, PCB_ARC_T, PCB_VIA_T, PCB_GROUP_T, PCB_GENERATOR_T, PCB_POINT_T }`, and `EDIT_TOOL::Mirror` has `case PCB_FIELD_T: case PCB_TEXT_T: static_cast<PCB_TEXT*>( item )->Mirror( mirrorPoint, flipDirection );` (the position mirrored, the horizontal justification flipped at exactly 0 / 90 degrees), `case PCB_GENERATOR_T: ...->Mirror(...)`, `case PCB_POINT_T: ...->Mirror(...)`. Ours: `mirrorableIds` (`web/studio/src/kicad-port/pcbMirror.ts:77-91`) keeps only `track`, `via`, `zone`, `shape`, `text` (and opens groups), so a selected Reference/Value, a point or a tuning pattern is silently left alone (text boxes and images are the known PARITY section 39 gap). Slice B's B1 is the menu side of the same gap (no Mirror / Rotate submenu for these kinds).
- **How to see it.** Select a footprint's Value text, Edit > Mirror Horizontally: KiCad moves nothing (a lone item mirrors about its own position) but flips its justification left/right; select the Value and a line: KiCad mirrors both about their centre, ours only the line. Test: `pcbMirror.test.ts` `a field and a point are mirrorable`: `planMirror(board, ["R1:Value", "pt_1", "shp_a"], c, "leftRight").cmds` has an `edit_board_field` (position, justify) for R1's Value and a move of the point.
- **Severity.** Medium.
- **Already known?** No (PARITY section 39 lists text boxes and images only).

### R5 (medium). Position Relative forgets its offsets, the polar box and the picked item between openings
- **What differs.** `POSITION_RELATIVE_TOOL::PositionRelative`: "The dialog is not modal and not deleted between calls." (`if( !m_dialog ) m_dialog = new DIALOG_POSITION_RELATIVE( editFrame ); m_dialog->Show( true );`; it is rebuilt only when the units changed). So the X/Y (or distance/angle) entries, the Polar coordinates box and `m_anchorItemPosition` (the item or point picked last time, used by `getAnchorPos()` for `ANCHOR_ITEM` and `ANCHOR_POINT`) are as the user left them. Ours keeps only the anchor type (`lastAnchorKind`, `web/studio/src/components/PcbParityDialogs.tsx:115`); `polar`, `a`, `b`, `pickedItem`, `pointX/Y` are `useState` defaults on every open (`:157-215`). Since `lastAnchorKind` is kept but the picked item is not, reopening on "Item" anchors at (0, 0) ("<none selected>") where KiCad anchors at the item picked before.
- **How to see it.** Position Relative, Select Item... (pick U1), offset (5, 0), OK; select another part, Position Relative again, OK: KiCad puts it 5 mm right of U1 again; ours opens with 0, 0 and no item, and OK moves the part's anchor to (0, 0). Test: a dialog test `Position Relative reopens with the last offsets and the picked item`.
- **Severity.** Medium (a wrong default; with "Item" kept it moves the selection to the origin).
- **Already known?** No (round 1 F14 fixed `s_anchorType` and Move Exactly's memory only).

### R6 (low). R pressed right after M, before the pointer moves, turns about the selection's own reference point, not the grab point
- **What differs.** `EDIT_TOOL::doMoveSelection`'s first event is the Move action itself (`evt->IsAction( &PCB_ACTIONS::move )` is in the motion branch), so dragging starts at once and the reference point is set before any motion: `m_cursor = grid.BestDragOrigin( originalMousePos, sel_items, ... ); ... selection.SetReferencePoint( dragOrigin );`. An R then turns about that anchor (the item anchor nearest the cursor: a pad, a line end). Ours: `tryTransformDuringMove` (`web/studio/src/actions/useActionRunner.ts:249-283`) uses `base.grabNm ?? base.pivotNm ?? rotationPivot(...)`; `grabNm` is set only by the first pointer move (`Canvas.tsx:1200`), so a press before any motion turns about `rotationPivot` (a lone item's position, or the snapped centre). The code comment says so ("Before the pointer has moved the grab point is not known yet").
- **How to see it.** Hover over pad 2 of R1, press M, press R without moving the mouse: KiCad turns R1 about pad 2; we turn it about R1's origin. Test: `useActionRunner`/store test: with `moveOriginNm` on pad 2 and no `movePreview`, R gives an op whose pivot is pad 2 (`dragOriginFor(state, refs, moveOriginNm)`).
- **Severity.** Low.
- **Already known?** In the code comment only; not in PARITY or GAPS.

### R7 (low). A via-stitching area with vias follows the Vias row of the Selection Filter, not Other items (KiCad 10)
- **What differs.** `PCB_SELECTION_TOOL::itemPassesFilter` (both 8303b2ad and 25ec2335): `if( itemType == PCB_GENERATOR_T ) { if( GetItems().empty() ) { if( !m_filter.otherItems ) ... return false; } else { itemType = ( *GetItems().begin() )->Type(); } }`. A generator with members is filtered as its first member: a via stitch (`PCB_VIA_STITCH`, `AddItem( via )`) by `m_filter.vias`, a tuning pattern by `m_filter.tracks`. Ours: `filterAllows` (`web/studio/src/components/canvas/selectionCandidates.ts:122-125`) sends `via_stitch` to `otherItems` always, though `BoardViaStitch.member_ids` is known. (Round 1's "checked" line "barcodes and generators under Other items" is right only for an empty generator.)
- **How to see it.** A via stitch with vias; untick Vias: KiCad no longer picks it, we do; untick Other items instead: KiCad still picks it, we do not. Test: `selectionCandidates.test.ts` `a via stitch with vias is gated by vias, an empty one by other items`.
- **Severity.** Low.
- **Already known?** No.

### R8 (low). The courtyard part of a footprint's box leaves out half the courtyard line width
- **What differs.** `FOOTPRINT::GetBoundingBox` merges each drawing's `GetBoundingBox()`, and `PCB_SHAPE`'s box is inflated by half its stroke width. Ours: `placed_courtyard_box` (`crates/model/src/footprint.rs:307-322`, 5066c1b1, the F3 fix) is the box of the courtyard outline points only; `footprintBox` (`web/studio/src/kicad-port/footprintBox.ts:41`) merges it as is (the art goes through `artBounds`, which does add `stroke_width / 2`). With the usual 0.05 mm courtyard line, the box is 0.025 mm short on each side where the courtyard is the outermost drawing, so Align and the pivot of several footprints are off by that.
- **How to see it.** Two parts whose courtyard is their outermost drawing, Align Left: KiCad lines up the outer edges of the courtyard lines; we line up their centrelines, 0.025 mm apart from KiCad's. Test: `footprint.rs` `the courtyard box includes half the line width` (the header test with a 50 000 nm stroke: x0 = -1 795 000).
- **Severity.** Low.
- **Already known?** No (the F3 fix is otherwise right: off-centre courtyards now give KiCad's box).

### R9 (low). Swap Layers leaves tables (and their cells) on the old copper layer
- **What differs.** `GLOBAL_EDIT_TOOL::SwapLayers` runs `swapBoardItem` over `Tracks()`, `Generators()`, `Zones()` and every item of `Drawings()`, which includes `PCB_TABLE`. Ours, `swap_layers` (`crates/ops/src/lib.rs:4202-4255`), maps tracks, zones, vias, shapes, texts, dimensions and `board_items`, not `dr.tables`.
- **How to see it.** A table on F.Cu, Swap Layers F.Cu -> B.Cu: KiCad's table is on B.Cu; ours stays on F.Cu. Test: `swap_layers_moves_a_table`.
- **Severity.** Low (a table on copper is rare).
- **Already known?** No.

### R10 (low). Doc drift
- GAPS item 35 ("Dimensions: ... Rotate/Flip of a dimension") and PARITY section 18 ("Rotate/Flip ... not ported -- no `Cmd::RotateDimension`/flip; `rotate_dimension` ... has no caller yet") are stale: `transform_dimension` (`crates/ops/src/pcb_transform.rs:479-505`) turns and flips dimensions through `rotate_items` / `flip_items`, with `PCB_DIM_ORTHOGONAL::Rotate`'s quarter-turn rule and `PCB_DIM_ALIGNED::Mirror` / `PCB_DIM_ORTHOGONAL::Mirror`'s height rule (both checked, they match), and R2's text-angle bug.
- `rotate_dimension`'s doc and the aligned text angle: `angle_of` rounds to a whole degree (`dimension.rs:93-95`, `.round() as i64 * 1000`), so a kept-aligned label at 33.69 degrees is written at 34; KiCad recomputes it on load (`Update()`), so only our own canvas and the file's text differ. Cosmetic.
- `extension_height`'s default is `Nm::kiround( arrow * sin 27.5 )` (`crates/model/src/ir.rs:2444`); KiCad's `static_cast<int>( m_arrowLength * s_arrowAngle.Sin() )` truncates (586 421 nm, ours 586 422). Cosmetic.

### Batch F fixes checked against KiCad and found right (do not re-review)
- F1 (9b6b6694): `toPolarDeg` / `polarTranslation` negate the angle as `PCB_ORIGIN_TRANSFORMS::ToDisplay/FromDisplay( const EDA_ANGLE&, REL_Y_COORD )` do with the Y axis not inverted (`return -aValue.AsDegrees();`); Clear's offset is `GetSelectionAnchorPosition() - getAnchorPos()`; the selection anchor is `GetTopLeftItem( true )`, then the top-left pad, then the top-left item (`PCB_SELECTION::GetTopLeftItem`, ties by smaller y); `RelativeItemSelectionMove` is one vector, one step. Move Exactly's polar angle uses the same `REL_Y_COORD` negation (`dyNm = -d sin a`).
- F2 (85a39921): `replace_shape` keeps the id, lock and drawing order; `mirror_tracks` mirrors start, end and an arc's mid in place with the layer kept (`PCB_TRACK::Mirror` / `PCB_ARC::Mirror`).
- F3 (5066c1b1): the box starts at the position grown by 0.25 mm, merges courtyard, fab body, art (with half the stroke), pads; without text drops Cmts/Dwgs/Eco1/Eco2 on a sided footprint; with text the shown fields and the footprint texts (`FOOTPRINT::GetBoundingBox`). Left: R8 and round 1 F21.
- F4 (55547f28): R turns about the grab point (the cursor in `doMoveSelection`, i.e. `BestDragOrigin` in the frame before the move); F of a lone item (not a rectangle, not a table) about the reference point, of several about `selection.GetCenter()` of the items as the presses left them; presses are applied in order and then the move -- equivalent to KiCad's interleaved move/turn sequence. Left: R6.
- F5 / F7 (38d681a8): Undo while a board move runs cancels it and Redo is refused (`doMoveSelection`: `restore_state = true; // Perform undo locally`, `ACTIONS::redo` -> `wxBell()`); Shift+Del expands tracks/vias with `selectConnection` and makes one `DeleteItems` batch with locks, groups, tables, fields and patterns (`EDIT_TOOL::Remove` with `REMOVE_FLAGS::ALT`).
- F6 (d99c8195): Move Individually's drops are one held step and Escape reverts all; the next item is picked up with its position on the cursor (`nextItem->Move( cursor - nextItem->GetPosition() )`); Skip puts the current item back. (The double-click finish is the known leftover.)
- F9 (57388c01): a shown field is hidden, an already hidden one gives "Use the Footprint Properties dialog to remove fields." only when nothing else was deleted or hidden (`DeleteItems`' `itemsDeleted == 0` branch); a field of a footprint deleted with it is not touched.
- F10 (31169eb0): Mirror filters with the group lock both ways; L takes groups, dimensions and tables and skips pads (`modifyLockSelected`: "Disallow locking free pads"); toggle resolves to Unlock if anything is locked.
- F11 (f6a731d3): `itemPassesFilter`'s lock test includes a locked group above and the parent footprint, with the lone-pad exception on a click only; zones vs rule areas (`m_filter.keepouts`).
- F12 (f6e2506d): only Align Left/Right swap in a mirrored view (`align_distribute_tool.cpp:306, 356`; nothing else there tests `IsMirroredX`).
- F14 (4b2a4b72): Move Exactly's menu is `{ aAnchor, ROTATE_AROUND_USER_ORIGIN, ROTATE_AROUND_AUX_ORIGIN }` with KiCad's four labels word for word, the saved state keyed like `m_hash_key` ("|with_selection" for several items); Position Relative's grid origin is the board's `GetGridOrigin()` and `s_anchorType` starts on Item.
- F15 (33dd31d4): Copy's reference point is `BestDragOrigin( cursor, items )` over the copied items (origin, then corner, then outline beyond 50 px).
- F16 (0c5e670a): a lone text box / table turns about its centre; a lone table flips about its box centre; a lone non-rectangle item flips about its position (`EDIT_TOOL::Rotate` / `Flip`, `updateModificationPoint`).
- Dimensions under Rotate/Flip (not batch F, stale docs): `PCB_DIM_ORTHOGONAL::Rotate`'s quarter-turn rule (Normalize180, 45/135 boundaries rounded towards zero), `PCB_DIM_ALIGNED::Mirror` (height negated), `PCB_DIM_ORTHOGONAL::Mirror` (height negated only when aligned with the flip), the flipped text angle negated, the layer flipped. Board Setup dimension defaults (X_XXXX, automatic units, no suffix, suppress zeroes, outside, keep aligned, 50 mil arrows, 0.5 mm offset) match `BOARD_DESIGN_SETTINGS`; the units and units-format numbers in the file match `DIM_UNITS_MODE` / `DIM_UNITS_FORMAT`.
- Swap Layers: through vias skipped, blind/buried vias remapped as `SetLayerPair( layerMap[top], layerMap[bottom] )`, zones by their whole layer set, drawings and board items (left: R9 tables).

---

# Part A: Properties panel, groups, Create Array, property dialogs

---

### A1 (high). A one-member group is dropped on import, Paste, Duplicate and Create Array. Batch F's premise is that such groups are kept.
- **What differs.** Batch F (52f2626d, 868d845f) made the board keep a group of one, as KiCad does. Its commit message says such a group is "kept after a delete since p6 F8, or read from a KiCad file". Four paths still drop it:
  - **Import:** `crates/kicad/src/import_items.rs:211-213` (`build_groups`): "A group of one is not a group, unless it is linked to a design block". The `Built::Dropped` arm hands the one member to the group above. The test `crates/kicad/src/pcb.rs:2888` asserts this ("a group of one item is dropped on the way in").
  - **Paste / Duplicate / Create Array:** `crates/ops/src/pcb_paste.rs:699-702` (`if member_ids.len() < 2 && ... { group_ids.push(None); continue; }`). Array copies go through `collect_copies` and this same landing code.
  - KiCad keeps every group it parses. `pcb_io_kicad_sexpr_parser.cpp::resolveGroups` makes a `PCB_GROUP` for each `GROUP_INFO` (`group = new PCB_GROUP( groupInfo->parent ); ... Add( group, ADD_MODE::INSERT, true )`) and then only `group->AddItem( item )` for each member it finds. There is no size test, and a group with zero members even survives in memory.
  - KiCad's Duplicate and Create Array copy the group whole: `case PCB_GROUP_T: this_item = static_cast<PCB_GROUP*>( item )->DeepDuplicate( true, &commit );` in `array_tool.cpp::onDialogClosed` and `EDIT_TOOL::Duplicate`.
- **How to see it.**
  1. Group a line and a text, then delete the text. Our board now keeps `grp_x = {line}`, and the `.kicad_pcb` we write has `(group "" ... (members <line uuid>))`.
  2. Re-import that file, or append it as a board: the group is gone.
  3. Select the group and press Ctrl+D: the copy has no group either.
  - Tests to write:
    - `crates/kicad/src/pcb.rs` `a_group_of_one_item_is_read_back`: write a design whose group holds one track, import it, and assert `dr.groups.len() == 1` and that the group's members are that track.
    - `pcb_groups_tests.rs` `duplicating_a_one_member_group_copies_the_group`: `Cmd::Duplicate` of `grp_one`, then assert two groups of one member each.
  - Fix the existing assertion at `pcb.rs:2888` (it asserts KiCad's opposite).
- **Severity:** high. A group (its name, uuid, lock and design-block link) is lost on the round trip of a KiCad file.
- **Already known?** No. GAPS item 17 now says "a group of one dissolves" is gone. The schematic has the same rule in `crates/kicad/src/sch_import.rs:661` and `crates/kicad/src/sch_clipboard.rs:208` (`prune_groups`); that is outside this slice, but it is the same fix.

### A2 (high). Footprint Orientation in the Properties panel and in Footprint Properties is 180 degrees off for a footprint flipped in the studio
- **What differs.**
  - The panel's Orientation row reads `fpOrientation = norm360(-(part.rot ?? 0))` (`web/kicad-port/pcbProperties.ts:340`, `:638`), and its setter rotates by the difference from that value (`:640-645`). `FootprintPropertiesDialog.tsx:92` and `:169` do the same.
  - The model keeps a back footprint mirrored in x with `rot` unchanged. A left-right flip does `rot = norm360(-rot)` (`crates/ops/src/pcb_transform.rs:316`).
  - The writer puts that footprint in KiCad's frame by adding half a turn: `turned_fp = FootprintInstance { rot: (fp.rot + 180_000) % 360_000, .. }` (`crates/kicad/src/pcb.rs:1067-1072`). It does this whenever the footprint was not read in the file's frame. The file's angle is therefore `-(rot + 180)`.
  - KiCad's `FOOTPRINT::Flip` gives the same angle: `EDA_ANGLE newOrientation = -m_orient; ... if( aFlipDirection == FLIP_DIRECTION::LEFT_RIGHT ) Rotate( aCentre, ANGLE_180 );`. The panel shows that value: `FOOTPRINT_DESC` registers `PROPERTY<FOOTPRINT, double>( _HKI( "Orientation" ), &FOOTPRINT::SetOrientationDegrees, &FOOTPRINT::GetOrientationDegrees, PT_DEGREE )`.
  - So for any bottom footprint made or flipped here, the studio shows `-rot` while KiCad and our own file have `-rot + 180`. Typing a value lands the footprint half a turn from what was typed.
  - A footprint read from a KiCad file in the file's frame (`FootprintExtra::library_frame == false`) is right, so the two kinds of bottom footprint disagree with each other.
  - Lesser, on every footprint: KiCad's value is `Normalize180`d, so it is in (-180, 180]. Ours is in [0, 360): KiCad shows -90 where we show 270.
- **How to see it.** Place R1 at rot 0 on the front and flip it (F, or Layer = B.Cu in the panel). The panel shows Orientation 0; KiCad, and the `(at x y 180)` we write, say 180. Type 0: we leave it as it is, and the file still says 180.
  - Test (`pcbProperties.test.ts`) `a back footprint's orientation is KiCad's`: a part with `side: "bottom"`, `rot: 0` and no `library_frame` extra, and assert that the Orientation row reads 180. The state JSON (`crates/cli/src/studio.rs:1201`) needs to say whether the footprint is in the library frame, or to send KiCad's angle.
  - Second test: Orientation = 180 on that part sends no command.
- **Severity:** high. Wrong value, and an edit leaves the wrong pose in the file KiCad reads.
- **Already known?** No. Position Relative and Move Exactly may share the convention; they are outside this slice.

### A3 (medium). Text boxes, barcodes, reference images and tuning patterns are outside the group bookkeeping (`universe`)
- **What differs.** `universe()` (`crates/ops/src/pcb_groups.rs:42-57`) lists footprints, tracks, vias, zones, shapes, texts, dimensions, tables and groups. It leaves out `dr.barcodes`, `dr.board_items` (text boxes, reference images, points) and `dr.tuning_patterns`. Yet these can be group members: Ctrl+G does not check the ids, and the writer resolves them (`crates/kicad/src/pcb.rs:446-453` puts them in `written`). Three effects:
  1. **Drawing while a group is entered.** A text box, barcode or reference image drawn while a group is entered does not join it. `join_group` diffs `universe` before and after the command (`pcb_groups.rs:126-144`). KiCad's `BOARD_COMMIT::Push` does `if( enteredGroup && boardItem->IsGroupableType() && !boardItem->GetParentGroup() ) enteredGroup->AddItem( boardItem );`, and `BOARD_ITEM::IsGroupableType` (`pcbnew/board_item.cpp:41-66`) is true for `PCB_REFERENCE_IMAGE_T`, `PCB_TEXTBOX_T`, `PCB_GENERATOR_T` and `PCB_BARCODE_T`. Points (`PCB_POINT_T`) are not in that list.
  2. **Deleting a member.** A deleted text box or barcode stays a member id (`tidy_groups` diffs `universe`), so a group of {text box, line} still counts two members after the text box is gone.
  3. **Save Selection as Design Block.** It fails when the selection has one of these. `link_design_block_items` (`:105-111`) refuses any id not in `universe` with "no item of the board has this id", after the block has already been written (`crates/cli/src/design_block_api.rs:473-478`).
- **How to see it.** Enter a group and draw a text box: the group's `member_ids` do not change, where KiCad's group gains it.
  - Test: `pcb_groups_tests.rs` `a_text_box_drawn_in_an_entered_group_joins_it`: `Cmd::InGroup { group, cmd: <add text box> }`, then assert that the box's id is in the group.
  - Test: `deleting_a_grouped_barcode_drops_it_from_the_group`.
- **Severity:** medium (a wrong edge case, plus a refused save).
- **Already known?** No.

### A4 (medium). Group Items does not lock the new group when a member is locked; the Group Properties dialog has no Locked box
- **What differs.**
  - KiCad's `pcb_group_tool.cpp::PCB_GROUP_TOOL::Group` locks the new group when any item in the selection is locked:
    ```cpp
    for( EDA_ITEM* eda_item : selection )
        if( eda_item->IsBOARD_ITEM() )
            if( static_cast<BOARD_ITEM*>( eda_item )->IsLocked() )
                group->SetLocked( true );
    ```
    Ours never adds the new group to `locked_ids`: `GroupHost::make_group` (`crates/model/src/groups.rs`) and `Board::group_items` (`crates/ops/src/pcb_groups.rs:66`).
  - The tools still treat the group as locked through its locked leaf (`FilterCollectorForLockedItems`' `lockedDescendant`). But the file has no `(locked yes)` on the group, and the Properties panel's Locked row for the group is unticked where KiCad's is ticked. Unlocking the member later frees our group; KiCad's stays locked.
  - `DIALOG_GROUP_PROPERTIES` has `m_locked` (`m_locked->SetValue( aGroup->AsEdaItem()->IsLocked() )`, then `SetLocked( m_locked->GetValue() )` in `TransferDataFromWindow`), shown for `PCB_GROUP_T`. `web/components/CommonDialogs.tsx:20` says "Not here: the dialog's "Locked" box, which this model has no counterpart for". That reason is stale: `locked_ids` holds groups (p6 F10 fixed, `set_locked` on a group id), and the writer emits `(locked yes)` (`write_group`).
- **How to see it.** Lock a line, select it with a text, press Ctrl+G, and save. KiCad writes `(group "" (uuid ..) (locked yes) (members ..))`; we write no `(locked yes)`.
  - Test: `pcb_groups_tests.rs` `grouping_a_locked_item_locks_the_group`: assert that the new group's id is in `drawings.locked_ids`.
- **Severity:** medium (a wrong default and a wrong file flag).
- **Already known?** No.

### A5 (medium). Track & Via Properties and Shape Properties (`ItemPropertiesDialog.tsx`) lack fields KiCad's dialogs have, give a false reason, and edit only one item
- **What differs.**
  - `PARITY-pcb.md` section 10 (lines 561-562) and the header of `web/components/ItemPropertiesDialog.tsx:1-9` say net, position and the via's layer span "stay read-only, matching source's own dialog", and that a shape's "Geometry itself has no dialog field in source either". The dialog's text says: "Net and layer span have no edit command yet, same as real KiCad's own dialog -- delete and redraw to change either." (`:157`). The track branch says "Net and layer have no edit command yet" (`:156`).
  - KiCad's `pcbnew/dialogs/dialog_track_via_properties.cpp` has a net selector (`m_netSelector`, `onNetSelector`), track start/end and via position binders (`m_trackStartX`, `m_viaX`, ...), the via's layer pair (`m_ViaStartLayer`, `m_ViaEndLayer`) and the via type.
  - `dialog_shape_properties.cpp` edits the geometry (the `GEOM_SYNCER` / `RECTANGLE_GEOM_SYNCER` binders) and the net (`m_item->SetNetCode( m_netSelector->GetSelectedNetcode() )`).
  - `EDIT_TOOL::Properties` opens the track/via dialog on the whole selection when it holds only tracks, arcs and vias: `if( ( SELECTION_CONDITIONS::OnlyTypes( { PCB_TRACE_T, PCB_ARC_T, PCB_VIA_T } ) )( selection ) ) { DIALOG_TRACK_VIA_PROPERTIES dlg( editFrame, selection ); ...`. Ours (`web/components/canvas/properties.ts:61`) opens the dialog on one id.
  - The verbs to do all of this exist now (`edit_track`, `set_item_net`, `edit_tracks_and_vias`, `replace_shape`, from the Properties panel's `pcb_props.rs`).
  - The track's width is a separate "Apply Width" button, not OK, and the dialog has a Delete button KiCad's does not.
- **How to see it.** Select three tracks and press E: KiCad edits all three (and their net); ours opens on one, with the net read-only.
  - Test (`useActionRunner` / e2e): `E on several tracks opens one dialog over all of them`.
- **Severity:** medium. The message and the reason are wrong; it is an edit gap, not a data error.
- **Already known?** No. GAPS has no Track/Via dialog row.

### A6 (low). The footprint's Properties panel rows differ from `FOOTPRINT_DESC`
- **What differs.** KiCad's rows after Orientation (`pcbnew/footprint.cpp::FOOTPRINT_DESC`): Reference (writeable, `SetReference`); Library Link, Library Description, Keywords and Component Class under "Footprint Properties"; the four Attributes; then the Overrides: "Exempt From Courtyard Requirement", "Clearance Override", "Solderpaste Margin Override", "Solderpaste Margin Ratio Override" and "Zone Connection Style".
  - Ours (`pcbProperties.ts:647-700`) adds Value, MPN, Block and Nets, none of which KiCad has. `PARITY-pcb.md` section 23 names only "Component Type" as an extra.
  - Ours lacks Library Description, Keywords and Component Class.
  - Ours lacks Clearance Override and Zone Connection Style. Section 23 says "per-footprint clearance ... not registered" because the model has no place for them, but it does: `Cmd::SetFootprintZoneConnection` holds both (section 10, line 6).
- **How to see it.** Select a footprint and compare the row list with KiCad 10's.
  - Test: `pcbProperties.test.ts` `a footprint lists FOOTPRINT_DESC's rows`, comparing the names in order.
- **Severity:** low.
- **Already known?** Reference read-only, yes (GAPS item 10 and item 26). The extra and missing rows, no.

### A7 (low). Validators KiCad does not have: a length of 0 is refused for track width, pad size, text size, rectangle side and circle radius
- **What differs.**
  - KiCad registers these with no validator. Their grid cell is `PGPROPERTY_SIZE` (a `wxUIntProperty`, which only refuses negatives):
    - `PCB_TRACK` "Width": `PROPERTY<PCB_TRACK, int>( _HKI( "Width" ), &PCB_TRACK::SetWidth, &PCB_TRACK::GetWidth, PROPERTY_DISPLAY::PT_SIZE )`, no `SetValidator` (`pcb_track.cpp` ~3133);
    - `PAD` "Size X" / "Size Y" (`pad.cpp:3461-3467`), no validator;
    - `EDA_TEXT` "Width" / "Height" (`eda_text.cpp:1436-1442`);
    - `EDA_SHAPE` "Width" / "Height" / "Radius" (`eda_shape.cpp:2958`, `:2975-2985`).
  - "Hole Size X" uses `PositiveIntValidator`, which allows 0 (`if( val < 0 ) ... TOO_SMALL( val, 0 )`).
  - Ours refuses `<= 0` (or `< 1 µm`) with "Value must be greater than or equal to 0.001 mm": `pcbProperties.ts:765`, `:870`, `:848`, `:1192`, `:1311`, `:1331`.
  - Also, KiCad's rectangle Width and Height are signed (`GetRectangleWidth`: `return GetEndX() - GetStartX();`, and `SetRectangleWidth` puts `End = Start + w`). Ours reads `Math.abs` and keeps the side (`:1328-1336`).
- **How to see it.** Track Width = 0: KiCad takes it (DRC then flags it); we refuse it.
- **Severity:** low (stricter than KiCad). The `abs` difference is visible only on a rectangle drawn right to left.
- **Already known?** No.

### A8 (low). Via Type ignores the via's own type
- **What differs.** The Via Type row works out Through, Blind or Buried from the span (`pcbProperties.ts:921-934`) and ignores `Via.type` (`web/api/types.ts:351`: `"blind" | "buried" | "micro"`). KiCad shows `GetViaType()`, and its `ENUM_MAP<VIATYPE>` includes `VIATYPE::MICROVIA, _HKI( "Micro" )` (`pcb_track.cpp:2878-2881`). So a microvia F.Cu-In1.Cu shows "Blind" here and "Micro" in KiCad.
- **How to see it.** Place a microvia from the router and select it.
  - Test: `pcbProperties.test.ts` `a microvia's type is Micro`.
- **Severity:** low.
- **Already known?** No.

### A9 (low). Group Properties: a one-member result, and the library link check
- **What differs.**
  - `CommonDialogs.tsx:57-58` selects nothing when the group is left with one member: `refs: dlg.members.length >= 2 ? [dlg.id] : []`, with the comment "a group left with fewer than two members is gone". Since 868d845f, `set_group` dissolves nothing (`drop_empty_groups`), so the group of one is still there. KiCad selects it (`m_toolMgr->RunAction<EDA_ITEM*>( ACTIONS::selectItem, m_group->AsEdaItem() )`).
  - The "Library link:" box: KiCad checks it with `libId.Parse( m_libraryLink->GetValue(), true ) >= 0`. `LIB_ID::Parse` with no colon takes the whole text as the item name and succeeds, and `aFix = true` repairs the item name with `FixIllegalChars` instead of failing. Ours (`check_design_block_id`, `crates/model/src/groups.rs`) refuses a text with no colon, an empty part, or a backslash, all with "Invalid library link: '...'".
- **How to see it.** Type `MyBlock` (no library) and OK: KiCad accepts it; we show "Invalid library link: 'MyBlock'".
- **Severity:** low.
- **Already known?** No.

### A10 (low). Doc drift after batch F's group changes
- `web/studio/PARITY-pcb.md:784` ("a group left with fewer than two members dissolves and leaves the group above, which may dissolve in turn") and `:786` ("a group left with fewer than two dissolves"). Since 52f2626d and 868d845f, only an empty group goes, and only Remove Items applies the `< 2` rule. Same for the tests paragraph at `:801` ("deleting a member or a whole subtree leaves no group behind").
- `PARITY-pcb.md:798`: "Create Array of a group | not done: a group id in the array's ids is skipped". Section 17 and GAPS 22 say groups are copied, and `array.rs` uses `collect_copies`. Round 1 F22 fixed GAPS 17 but missed this row.
- `crates/ops/src/pcb_groups.rs:13` and `:27` (module doc) still say a group of fewer than two dissolves on delete.
- `PARITY-pcb.md` section 10 rows 561-562: see A5.
- `CommonDialogs.tsx:20` and `:57`: see A4 and A9.
- **Severity:** low.

### A11 (low). Group Items with members that share a group, and with pads
- **What differs.**
  - `GroupHost::make_group` puts the new group into the group its members shared, whether or not that group is entered (`crates/model/src/groups.rs`, the `shared` branch). KiCad's `PCB_GROUP_TOOL::Group` takes each member out of its group (`group->AddItem( eda_item )`, which `EDA_GROUP::AddItem` does) and adds the new group to the commit. Only `BOARD_COMMIT::Push`'s `enteredGroup->AddItem` nests it, and only when a group is entered. The studio wraps commands made inside an entered group in `in_group`, so the GUI agrees; the backend verb used by itself (`eda board`, agents) nests where KiCad would not.
  - KiCad's selection filter for Group removes pads and fields (`if( item->GetParentFootprint() ) aCollector.Remove( item );`) and then needs two or more items. `groupSelection` (`web/state/store.tsx:2463-2469`) sends any ids, a pad's included.
- **Severity:** low.

---

### Checked and found to match (do not re-review)
- `GROUP_TOOL::RemoveFromGroup` after 868d845f. Only the groups that lost a member are tested against `< 2` (`affectedGroups`). The dissolved group's remaining member goes free, not into the group above. The group above is not looked at again (`groups_holding` + `prune_affected_groups`).
- `GROUP_TOOL::Ungroup`. The group is removed, its members go free (not into the group above), and the released members are selected (`ungroupSelection`). Nothing else dissolves.
- `GROUP_TOOL::AddToGroup` and `GROUP_CONTEXT_MENU::update`. Exactly one group plus items in no group are needed (`planAddToGroup`). Enabling: Group at `selectionCount >= 2`, Ungroup with a group, Remove Items with a member, Add Items with `onlyOneGroup && hasUngroupedItems`.
- `DIALOG_GROUP_PROPERTIES::TransferDataFromWindow`. Members are taken from their old groups, an old group is not dissolved, and an empty one is not written. `DoAddMember` refuses the group itself and duplicates.
- `BOARD_COMMIT::Push` after 52f2626d. A removed item leaves its group and a group of one stays (`tidy_groups`). Multichannel `pruneExistingGroups` is limited to the groups it pruned (`multichannel.rs:482-489`, `multichannel_copy.rs:707-712`). Save Selection as Design Block's cut uses the same rule (`design_block_api.rs:391-396`).
- `PCB_GROUP_DESC`: Position X/Y and Layer masked; Name under "Group Properties".
- `PROPERTIES_PANEL::rebuildProperties` / `extractValueAndWritability`. Caption "No objects selected", "%d objects selected", or the friendly name. A row needs the property on every item, available and not hidden, with the same choices. `<...>` shows when values differ; a row read-only for one item is read-only for all.
- `PCB_PROPERTIES_PANEL::valueChanged`. A pad's Position X/Y moves its footprint by the delta. The edit is one commit.
- `TRACK_VIA_DESC`. Start X/Y replace Position X/Y; End X/Y; the via masks Layer; Diameter and Hole with `ValidateViaParameters`' messages word for word ("Via diameter is too small.", "Via drill is too small.", "Via hole size must be smaller than via diameter"). Via Type labels Through, Blind, Buried (but see A8).
- `FOOTPRINT_DESC` Layer: `SetLayerAndFlip` is `Flip( GetPosition(), FLIP_DIRECTION::LEFT_RIGHT )`, which matches `flip_items` with `pivot = at` and `left_right`.
- `EDA_SHAPE` setters: `SetStartX`/`SetEndX` move one point; `SetCenterX` moves the circle with its point (`GetStartX` as the getter); `SetRadius` is a size. `PCB_SHAPE::GetPosition` is the arc's centre, the polygon's first vertex, otherwise `m_start`.
- `ARRAY_GRID_OPTIONS::getGridCoords`, `gtItemPosRelativeToItem0` (the stagger's `VECTOR2D` result truncated by the `VECTOR2I` converting constructor: same as our integer division), `GetTransform`'s centring by half the extent.
- `ARRAY_CIRCULAR_OPTIONS::GetTransform`: angle × n, or 360·n/count when the angle is 0, plus the offset, negated when clockwise (`RotatePoint` positive is counter-clockwise on screen), rotation only with `m_rotateItems`; `TransformItem` moves, then turns about the item's own position.
- The Arrange branch: selection order, a footprint once per `fpDeDupe`, each item first set to the first item's position. "Arrange selection" is the commit text.
- `CREATE_ARRAY_DIALOG_ENTRIES` defaults: 5 × 5 at 2.54 mm, stagger 1 rows, 90°, 4 points, clockwise, not full circle, no rotation, Duplicate, unique references. Both position radios are `true`, and the persister adds Centre on source items second.
- `TransferDataFromWindow`'s messages: "horizontal delta of zero with %ld objects", "vertical ...", "angular delta of zero with %ld objects", and `validateLongEntry`'s "Bad numeric value for %s: %s" with "horizontal count", "vertical count", "stagger", "point count".
- The original keeping the first point, and the circle centre starting at the selection, are the known deviations of GAPS item 22 (KiCad's references by point come out the same: the copy at point 0 is not reannotated, the original at the last point is).

---

# Part B: context menu, Find, inspection, Net Inspector, Appearance


## B1 (medium). The context menu's selection summary does not count fields, text boxes, barcodes, points, reference images or generators. Net Inspection Tools shows for them, and Mirror / Rotate does not
- **What differs.** `summarizePcbSelection` (`web/kicad-port/pcbSelectionSummary.ts:52-115`) has a `switch (kind)` with cases for part, pad, track, via, zone, shape, text, dimension, tablecell, table and group only. `itemKind` (`web/kicad-port/pcbItems.ts:59-80`) also returns `"field"` (a footprint's Reference, Value or user field) and the extra kinds (text box, image, point, barcode, `via_stitch`). Those kinds raise `s.total` and nothing else. The menu's conditions then read the type counts (`web/kicad-port/pcbContextMenu.ts:159-200` `count`, `hasTypes`, `onlyTypes`):
  - **Net Inspection Tools.** `showNetMenu` (`pcbContextMenu.ts:794`) is `s.total > 0 && s.footprints === 0 && s.texts === 0 && ... && <all shapes copper>`, so it is true for a lone field, text box, barcode, point or image. KiCad's `BOARD_INSPECTION_TOOL::Init` `showNetMenuFunc` accepts only these types and returns false for everything else:
    ```cpp
    case PCB_TRACE_T: case PCB_ARC_T: case PCB_VIA_T: case PCB_PAD_T: case PCB_ZONE_T:
        continue;
    case PCB_SHAPE_T: { if( !static_cast<const PCB_SHAPE*>( item )->IsOnCopperLayer() ) return false; else continue; }
    default:
        return false;
    ```
  - **Mirror / Rotate.** `canMirror` (`pcbContextMenu.ts:469-473`) is `hasTypes(s, MIRRORABLE_TYPES)`, which never sees these kinds, so a lone text box, field or point gets no Mirror / Rotate submenu. KiCad's `EDIT_TOOL::MirrorableItems` (`edit_tool.cpp:2643`) is `{ PCB_SHAPE_T, PCB_FIELD_T, PCB_TEXT_T, PCB_TEXTBOX_T, PCB_ZONE_T, PCB_PAD_T, PCB_TRACE_T, PCB_ARC_T, PCB_VIA_T, PCB_GROUP_T, PCB_GENERATOR_T, PCB_POINT_T }`, and `EDIT_TOOL::Init` adds the submenu under `canMirror`. So KiCad offers Rotate CCW/CW and Mirror H/V there.
  - **Copy as Text.** See B2.
- **How to see it.** Select a footprint's Value text (or a text box) and right-click. KiCad has Mirror / Rotate and no Net Inspection Tools; we show Net Inspection Tools and no Mirror / Rotate. Test: `pcbContextMenu.test.ts`, `a lone field or text box gets Mirror / Rotate and no Net Inspection Tools`. Summarise a board with one text box selected, and assert that the labels of `pcbSelectionMenu(s)` contain "Mirror / Rotate" and not "Net Inspection Tools". Do the same with `["R1:Value"]`.
- **Severity.** Medium: the wrong entries are listed.
- **Already known?** No. GAPS 16 and PARITY section 7 do not mention it (the summary predates P16i's item kinds).

## B2 (medium). Copy as Text on the board: the menu leaves out fields, text boxes, tables and cells, the action copies only texts and dimensions, and it copies the raw text, not the shown text
- **What differs.** The menu entry's condition is `COPY_AS_TEXT_TYPES = ["text", "dimension"]` (`web/kicad-port/pcbContextMenu.ts:218`, used at `:747`). KiCad's `canCopyAsText` (`EDIT_TOOL::Init`) is `NotEmpty && OnlyTypes( { PCB_FIELD_T, PCB_TEXT_T, PCB_TEXTBOX_T, PCB_DIM_ALIGNED_T, PCB_DIM_LEADER_T, PCB_DIM_CENTER_T, PCB_DIM_RADIAL_T, PCB_DIM_ORTHOGONAL_T, PCB_TABLE_T, PCB_TABLECELL_T } )`.
  The action (`web/actions/useActionRunner.ts:1616-1631`) maps each id to `api.textById(id)?.content ?? api.dimensionById(id)?.text`. It does not take a field, a text box, a table cell or a table, and it copies the stored text. `EDIT_TOOL::copyToClipboardAsText` (`edit_tool.cpp:3775`) does this instead:
  ```cpp
  case PCB_TEXT_T: case PCB_FIELD_T: case PCB_DIM_*: ... return text.GetShownText( true );
  case PCB_TEXTBOX_T: case PCB_TABLECELL_T: ... return textBox.GetShownText( true );
  case PCB_TABLE_T: ... s << cell->GetShownText( true ); if( col < table.GetColCount() - 1 ) s << '\t'; ... if( row < table.GetRowCount() - 1 ) s << '\n';
  ```
  So a text `${TITLE}`, or a footprint text `${REFERENCE}`, is copied resolved in KiCad and literally by us.
- **How to see it.** Select a table and press Ctrl+Shift+C: KiCad puts the cells on the clipboard, tab- and newline-separated; we put nothing. Do the same with a board text `${REVISION}` on a board whose revision is "B": KiCad copies "B", we copy "${REVISION}". Tests: `pcbContextMenu.test.ts`, `Copy as Text is offered for a field, a text box, a table and a cell`; and a unit test of the board text collector, `a table copies its cells tab and newline separated, and a text its shown text`.
- **Severity.** Medium: the wrong text on the clipboard, and a missing entry.
- **Already known?** No.

## B3 (low). Net Inspector: Name and Netclass sort by `localeCompare`, not `ValueStringCompare`
- **What differs.** `netInspectorRows` (`web/kicad-port/netInspector.ts:96-101`) compares text columns with `String(x).localeCompare(String(y))`. KiCad's data model `compareItems` (`pcbnew/widgets/pcb_net_inspector_panel_data_model.h`, `COLUMN_NAME` / `COLUMN_NETCLASS`) uses `ValueStringCompare( s1, s2 )` (`common/string_utils.cpp:1158`). That function splits off the trailing number and compares it as a number (`strFWordMid.ToCDouble( &lFirstNumber )`), so NET2 sorts before NET10. With `localeCompare`, "NET10" sorts before "NET2". Equal values: KiCad breaks the tie by item pointer, we break it by name (harmless).
- **How to see it.** Nets `D2` and `D10`, sorted by Name: KiCad gives D2, D10; we give D10, D2. Test: `netInspector.test.ts`, `names sort by their numbers as ValueStringCompare does`, asserting `netInspectorRows([D10, D2], opts).map(r => r.name)` equals `["D2", "D10"]`.
- **Severity.** Low: the order on screen.
- **Already known?** No.

## B4 (low). Net Inspector: "Filter by Netclass" matches the human-readable class name; KiCad matches `NETCLASS::GetName()`
- **What differs.** `netShown` (`web/kicad-port/netInspector.ts:87`) tests `row.netclass` (the column text, `GetHumanReadableName`: "A and B"). `PCB_NET_INSPECTOR_PANEL::netFilterMatches` tests `const wxString netClassName = UnescapeString( netClass->GetName() ).Upper();`. For a net that one class claims, the two are the same. For a net that several classes claim, `NET_SETTINGS::GetEffectiveNetClass` names the class `name.Printf( "Effective for net: %s", aNetName )` (`common/project/net_settings.cpp:827`). In KiCad, a filter on "A" does not find that net through its class (it may still match by net name), and a filter on "EFFECTIVE" finds every multi-class net.
- **How to see it.** Classes A and B both claim net `X`, the filter is "AND", Filter by Net Name is off: we list X ("A and B"); KiCad lists nothing. Test: `netInspector.test.ts`, `the netclass filter reads the effective class's name`.
- **Severity.** Low: a KiCad quirk. Copy it or record it as a deliberate difference.
- **Already known?** No.

## B5 (low). Net Inspector report: rows in the on-screen sort, not the model's net-code order
- **What differs.** The dialog passes `shown` (sorted by the chosen column) to `netInspectorReport` (`web/components/NetInspectorDialog.tsx:110`). KiCad's `generateReport` walks `m_dataModel->itemAt( row )` for `row < itemCount()`, which is `m_items`, the model's own list (filled by `addItems` / `addItem`). That list does not follow the view's sort. The header and the cell formats match.
- **How to see it.** Sort by Total Length descending and save the report. KiCad's rows stay in model order; ours follow the screen. Test: none needed beyond a note. This is about where the rows come from, not the CSV format.
- **Severity.** Low.
- **Already known?** No.

## B6 (low). Appearance net names: a colour saved under both the old and the new name of a net; duplicate hidden nets
- **What differs.** `with_board_net_names` (`crates/kicad/src/appearance.rs:169-195`, e1a3c2b5) rebuilds `net_colors` by renaming every key through a `Map` collect. If the file holds both `"FB"` (the intent's name) and `"/FB"` (the board's name), both keys become `/FB`, and whichever comes last in the map's order wins. The serde `Map` is ordered, `"/FB" < "FB"`, so the stale `FB` colour overwrites the newer `/FB` one. `hidden_nets` can likewise end up with `"/FB"` twice. KiCad has no counterpart: it stores colours by net name only (`net_settings.net_colors`). In normal use the studio's GET renames the keys and a POST then replaces the whole `project` section (`crates/cli/src/appearance_api.rs:72-79`), so the two names seldom coexist. It can happen when the file was written by an older studio after the rename.
- **How to see it.** Unit test in `appearance.rs`: `a colour under the board's name wins over one left under the intent's`. Input `{"project":{"net_colors":{"FB":"rgb(1,1,1)","/FB":"rgb(2,2,2)"}}}` with intent `/FB` ↔ `FB`; assert that `/FB` is `rgb(2,2,2)` and that `hidden_nets` has no duplicates.
- **Severity.** Low.
- **Already known?** No.

---

### Checked and found to match (do not re-review)
- **Net Inspector numbers** (`crates/cli/src/net_inspector_api.rs` against `length_delay_calculation.cpp` and `pcb_net_inspector_panel.cpp::calculateNets`):
  - `optimiseVias`: the exact-position lines and the lines inside the via's radius, the coincident pad's side (`SMD`/`CONN` take the first copper layer; otherwise the footprint's layer), the stub case and the "nothing connects" case (`pcbVia->GetLayer()` twice).
  - `mergeLines`: one-to-one ends only, no merge into an arc, the same layer only; `MERGED_IN_USE` / `RETIRED`; `mergeShapeLineChains` at the start and at the end, in both orientations.
  - The via clip after the merge, on `MERGED_IN_USE` lines only, start first or else the end; `clipLineToVia` / `clipLineToPad` build center → crossing → the rest.
  - `OptimiseTraceInPad`: only lines that end on the pad's position, and only on a layer the pad flashes on.
  - The sums: `TrackLength` and `LayerLengths` per start layer; `ViaLength` and `NumVias` only with `m_UseHeightForLengthCalcs`; the pads' pad-to-die length and count.
  - `GetTotalLength = GetBoardWireLength + GetViaLength + GetPadDieLength`.
  - Rows only for nets with tracks, arcs, vias or pads (`foundNets`); zero-pad nets left out unless Show Zero Pad Nets; never net code 0.
  - `GetHumanReadableName` ("%s and %s", "%s, %s and %s", "%s, %s and %d more").
- **Net Inspector page.**
  - `buildColumns`: the names and CSV names, the units flags, one column per enabled copper layer in `GetEnabledLayers().Seq()` order (F.Cu, B.Cu, In1...).
  - `netFilterMatches`: an empty filter matches everything; class match before name match; the upper-cased `UNCONNECTED-(` test only when matched.
  - `generateReport`: the header `"%s (%s)";` for length columns, `"...";` for quoted columns, `;` after the others, lengths without their unit label; the unit labels mm, mils, in.
  - The defaults: name and class filters on, zero-pad and unconnected nets off.
- **Context menu** (`web/kicad-port/pcbContextMenu.ts` against `PCB_SELECTION_TOOL::Init`, `EDIT_TOOL::Init`, `BOARD_EDITOR_CONTROL::Init`, `BOARD_INSPECTION_TOOL::Init`, `GROUP_CONTEXT_MENU::update`, `pcb_edit_frame.cpp::setupUIConditions`):
  - The entries, their order numbers and their conditions for: Select, Cancel, Enter/Leave Group, the design-block entries, Clear Net Highlighting, the table-cell entries, Select All / Unselect All (`noItemsCondition`), Skip, Move, Drag 45 / Free Angle (`DraggableItems` = track, via, footprint, arc), Flip, Swap, Swap Pad Nets, Swap Gate Nets (and its submenu), Break Track, Fillet Tracks, Pack and Move, Assign Netclass (`connectedTypes`), Inspect Clearance (count 2), the five footprint actions, the Routing / Mirror-Rotate / Shape Modification / Position submenus at 100, Cut ... Delete at 150 (Paste and Paste Special only with no active tool), and Properties at 2000 (one item, or tracks and vias only).
  - The submenus' contents and conditions:
    - Mirror/Rotate: rotate on NotEmpty, mirror on canMirror.
    - Routing: `isRoutable`.
    - Position: Move Individually on MoreThan(1).
    - Shape Modification: heal and simplify by HasTypes; fillet, chamfer and dogbone by OnlyTypes; extend on two segments; the point-editor entries on Count(1); editVertices by `selectionHasEditableCorners` (size 1); booleans on MoreThan(1).
    - Locking: Lock when something is unlocked, Unlock when something is locked, Toggle always.
    - Zones: the fixed list; Merge on MoreThan(1) of zones; Duplicate, Cutout and Similar on a single zone.
    - Net Inspection Tools: show/hide enabled only when an item has a net.
    - Select: selectNet on OnlyTypes(trackTypes), selectSameSheet on footprints, selectOnSchematic on pad/footprint/group, selectUnconnected on footprint/pad; deselectNet left out, as KiCad comments it out.
    - Grouping's enable rules.
    - Create from Selection's `canCreateArray = MoreThan(0)`.
    - Align/Distribute: MoreThan(1) / MoreThan(2).
- **Appearance file side** (`crates/kicad/src/appearance.rs`, `web/kicad-port/appearance.ts`, `appearanceFile.ts`, `appearanceNets.ts`):
  - `PCB_LAYER_ID` numbers (F.Cu 0, F.Mask 1, B.Cu 2, In<n> 2+2n, ... F.Fab 35, User.<n> 37+2n).
  - The `VISIBILITY_LAYER` strings (lower-cased enum names; "none" for an empty set; damaged-array restore).
  - The opacity keys and defaults (zones and images 0.6, the rest 1.0), `board.net_color_mode` default Ratsnest.
  - `NET_GRID_TABLE::Rebuild`: net code above 0, case-sensitive `unconnected-(` left out, `a.name < b.name` sort.
  - `rebuildNets`: the Default class first and without a swatch, then the classes `std::sort`ed.
  - e1a3c2b5's renaming of colours and hidden nets from the intent's name to the board's (apart from B6).

---

# Part C: snapping, selection, edit tools per kind, footprints, smaller tools, global edits


## High

### C1. Rotate by a non-quarter angle drops a rounded rectangle's corner radius
- **What differs.** `EDA_SHAPE::rotate` (`common/eda_shape.cpp:966-981`) turns a rectangle that is not turned by a cardinal angle into the polygon of its *rounded* rectangle:
  ```cpp
  case SHAPE_T::RECTANGLE:
      if( aAngle.IsCardinal() ) { RotatePoint( m_start, ... ); RotatePoint( m_end, ... ); }
      else
      {
          // Convert non-cardinally-rotated rect to a diamond
          ROUNDRECT rr( SHAPE_RECT( GetStart(), GetRectangleWidth(), GetRectangleHeight() ), m_cornerRadius );
          m_shape = SHAPE_T::POLY;
          rr.TransformToPolygon( GetPolyShape(), getMaxError() );
          GetPolyShape().Rotate( aAngle, aRotCentre );
      }
  ```
  Ours, `crates/ops/src/pcb_transform.rs:409-419` (`transform_shape`, `Shape::Rect`): a non-cardinal turn makes a `Shape::Polygon` of the **four sharp corners**. The rectangle's corner radius lives in `BoardItems::shape_styles` (`crates/model/src/pcb_items.rs:351`, `corner_radius_of`), keyed by the id the polygon keeps, and is read only for `Shape::Rect` (`crates/model/src/outline.rs:76-82`, `crates/drc/src/hatch.rs:378`), so the rounding is gone and a stale radius stays on the polygon's style.
- **How to see it.** Draw a rectangle on Edge.Cuts, give it a 2 mm corner radius (Properties), select it, Rotate by 45 degrees (Move Exactly's rotation, or a 45-degree rotation step): KiCad's board outline still has rounded corners (a polygon with arc-approximating vertices); ours has a sharp diamond, so the board outline and the copper-to-edge DRC change. Test: `pcb_transform_tests.rs` `a_rounded_rectangle_turned_45_degrees_is_the_polygon_of_its_rounded_rectangle` -- a 10 x 6 mm rect with `set_corner_radius(id, mm(2))`, `RotateItems { angle_millideg: 45_000 }`; assert the result is a `Polygon` with more than 4 points, every point within `MAX_ERROR_NM` of the rotated rounded-rect outline, and `corner_radius_of(id) == 0`.
- **Severity.** high (the board outline is a different shape; a file KiCad reads differently).
- **Already known?** No. Round 1 noted "no `gr_rect` corner radius in the IR" (since added by batch B/C, `1bb6f8c0`); the rotate path was not updated.

### C2. Cut deletes items that the Copy did not put on the clipboard; Copy of a field, barcode, point, text box or image fails the whole copy; Copy of a pad copies its whole footprint
- **What differs.**
  - `EDIT_TOOL::cutToClipboard` (`pcbnew/tools/edit_tool.cpp:3869`) deletes "exactly that; no more, no fewer" of what `copyToClipboard` put on the clipboard (`Remove` with `REMOVE_FLAGS::CUT`). `CLIPBOARD_IO::SaveSelection` (`pcbnew/kicad_clipboard.cpp:340-440`) writes every `BOARD_ITEM`: a `PCB_FIELD` becomes a free `PCB_TEXT` with the field's text (`copy = new PCB_TEXT( m_board ); ... textItem->SetText( field->GetText() )`), a pad becomes a new footprint holding just that pad (`if( copy->Type() == PCB_FIELD_T || copy->Type() == PCB_PAD_T ) { FOOTPRINT* footprint = new FOOTPRINT( m_board ); ... footprint->Add( copy ); }`), barcodes, points, text boxes, images and tables are `Clone()`d. `copyToClipboard`'s `RequestSelection` filter has no `FilterCollectorForFreePads` (`edit_tool.cpp:3723-3731`), so a selected pad is copied as a pad.
  - Ours: `Picked::add_item` (`crates/kicad/src/clipboard.rs:131-154`) knows footprints, tracks, vias, zones, shapes, texts and dimensions only; any other id fails the whole copy with `clipboard.unknown_item` ("no placed part, track, via, zone, shape, text, dimension or group with this id"). `copySelection` (`web/state/store.tsx:2515-2530`) toasts the error and returns normally, and Cut (`web/actions/useActionRunner.ts:332-338`) is `api.copySelection(refs).then(() => deleteRefs(refs))`: the delete runs even when nothing reached the clipboard. `copySelection` runs `editableSelection` (`web/kicad-port/pcbTransform.ts:72`), which turns a pad into its footprint, so Copy of one pad copies the whole footprint (a second footprint under a new reference on Paste).
- **How to see it.** Select a barcode (or a footprint's Value field, or a point) together with a track, Ctrl+X: KiCad has both on the clipboard and both removed; we toast "no placed part, track, via, zone, shape, text, dimension or group with this id", leave the clipboard as it was and delete the track and the barcode (the field is hidden). Select one pad of U1, Ctrl+C, Ctrl+V: KiCad pastes a footprint with that single pad; we paste all of U1. Tests: `clipboard.rs` `a_field_copies_as_a_text_and_a_barcode_and_a_point_copy` (`export_pcb_clipboard(&d, &m, &["R1:Value", "<barcode id>"], ..)` is `Ok` and contains `(gr_text "<value text>"` and `(barcode`); web `useActionRunner` test `cut does not delete when the copy failed` (stub `postClipboardCopy` to `{ok:false}`, run `common.Interactive.cut`, assert no `cmdBatch` was sent).
- **Severity.** high (Cut destroys items that are not on the clipboard; Undo brings them back, but the user believes they were cut).
- **Already known?** Only in part: PARITY section 39 lists "Duplicate, Paste, Create Array and Mirror of a text box or an image" and GAPS item 24 (line 662) Copy of tables. Not listed: fields, barcodes and points on the clipboard (batch F added them to Duplicate only, `38f95a64`), a pad copied as its footprint, and Cut deleting after a failed copy.

### C3. Edit Track & Via Properties' "Set to net class / custom rule values" ignores custom rules and diff-pair widths, and sets a width KiCad leaves alone
- **What differs.** `DIALOG_GLOBAL_EDIT_TRACKS_AND_VIAS::processItem`'s else arm is `m_parent->SetTrackSegmentWidth( aItem, aUndoList, true )`, and `PCB_EDIT_FRAME::SetTrackSegmentWidth` (`pcbnew/edit_track_width.cpp:34-60`) with `aUseDesignRules` reads the DRC engine:
  ```cpp
  MINOPTMAX<int> constraint = aItem->GetWidthConstraint();
  if( constraint.HasOpt() )            new_width = constraint.Opt();
  else if( constraint.Min() > 0 )      new_width = constraint.Min();
  if( via ) { constraint = via->GetDrillConstraint(); ... }
  ...
  if( new_width <= 0 ) new_width = aItem->GetWidth();
  ```
  `PCB_TRACK::GetWidthConstraint` is `EvalRules( TRACK_WIDTH_CONSTRAINT, this, nullptr, m_layer )` (`pcbnew/pcb_track.cpp:549-564`; a via's is `VIA_DIAMETER_CONSTRAINT`, its drill `HOLE_SIZE_CONSTRAINT`), so a `.kicad_dru` rule's `track_width (opt ..)` wins, and a net in a diff pair gets the class's diff-pair width: `drc_engine.cpp:313-327` adds the item-specific rule `A.hasExactNetclass('%s') && A.inDiffPair('*')` with `SetOpt( nc->GetDiffPairWidth() )` after the plain class rule. The tuning-profile rules (`drc_engine.cpp:465-512`) set a per-layer width too.
  Ours, `crates/ops/src/lib.rs:3786-3804` (`edit_tracks_and_vias`): `SizeSpec::NetClass => model.board.width_of(&t.net)`, which is the class's track width or the board default (`crates/model/src/lib.rs:911-913`); `ViaSizeSpec::NetClass` likewise. No custom rule, no diff-pair width, no tuning profile, and a width is always written.
- **How to see it.** A class `HS` with track width 0.2 mm and diff-pair width 0.12 mm, nets `USB_P`/`USB_N` in it routed at 0.3 mm; Edit Track & Via Properties, Tracks, "Set to net class / custom rule values", Apply: KiCad sets 0.12 mm; we set 0.2 mm. Or a custom rule `(rule w (condition "A.NetName == 'VBUS'") (constraint track_width (opt 0.5mm)))`: KiCad 0.5 mm, ours the class width. Test: `tests.rs` `edit_tracks_and_vias_net_class_takes_the_diff_pair_width_and_custom_rules`.
- **Severity.** high (tracks end up at a width the rules do not want; a pair's impedance changes).
- **Already known?** No (PARITY section 13 claims "identical in effect"; GAPS 35 lists only microvias and the via presets).

## Medium

### C4. Flip leaves an item on a front/back user layer where it is; KiCad moves it to the paired layer
- **What differs.** Every item's `Flip` calls `GetBoard()->FlipLayer( GetLayer() )` (`pcb_shape.cpp:581`, `pcb_text.cpp:486`, `pcb_track.cpp:1136`, `zone.cpp:1114`, `footprint.cpp:2983`, ...), which is `m_layers[...].m_opposite` (`pcbnew/board.cpp:930-933`). `BOARD::recalcOpposites` (`board.cpp:869-926`) pairs user layers typed `front`/`back`: first by the name after the first dot ("Match up similary-named front/back user layers"), then consecutive `User.n` (front) / `User.n+1` (back) pairs with custom names.
  Ours: `flip_layer` (`crates/ops/src/pcb_transform.rs:83-102`) maps the seven fixed technical pairs and the inner copper only; a `User.*` layer always stays. The IR has the types (`LayerType::Front | Back`, `crates/model/src/layers.rs:131-142`, whose doc says such a layer "does" flip, and `crates/kicad/src/pcb.rs:579` writes them), so a board with `User.1 "F.Assembly" front` and `User.2 "B.Assembly" back` flips its assembly drawings in KiCad and not here (same for footprint art flipped through `fp_edit.rs:350,367`, `pcb_items.rs:511,521`, `pcb_tables.rs:165`).
- **How to see it.** Board Setup > Board Editor Layers: User.1 named `F.Notes` type front, User.2 `B.Notes` type back; draw a line on User.1, F: KiCad puts it on User.2; we keep User.1. Test: `pcb_transform_tests.rs` `flip_takes_a_front_user_layer_to_its_back_pair` (layer setup with the two types; `FlipItems` on a line on `User.1`; assert its layer is `User.2`).
- **Severity.** medium (an edge case, but the flipped item is on the wrong layer in the file).
- **Already known?** No.

### C5. Pad Properties refuses a plated pad whose hole reaches the pad's edge; KiCad warns and accepts it, in other words
- **What differs.** `DIALOG_PAD_PROPERTIES::padValuesOK` (`pcbnew/dialogs/dialog_pad_properties.cpp`) only blocks OK on `DRCE_PADSTACK_INVALID` and `DRCE_PAD_TH_WITH_NO_HOLE`; `DRCE_PADSTACK` messages are prefixed `_( "Warning: " )` and shown, and OK goes through (`return error_msgs.GetCount() == 0;`). `PAD::doCheckPad` (`pcbnew/pad.cpp:2987-3009`) reports `_( "(PTH pad hole leaves no copper)" )` when the copper minus the hole is empty, and with `aForPadProperties` `_( "(PTH pad hole not fully inside copper)" )` when the hole pokes out -- both `DRCE_PADSTACK` (warnings). The only size error besides "(Pad must have a positive size)" is `_( "(PTH pad hole size must be larger than %s)" )` for a hole of 4 nm or less.
  Ours, `web/kicad-port/fpFields.ts:205` (`checkPadValues`): `if (pad.kind === "through_hole" && (hw >= pw || hh >= ph)) return \`Error: (PTH pad hole leaves no copper): the ${...} hole is as big as the pad.\`` -- an error, so the dialog refuses, with a sentence KiCad does not have, for a hole that merely reaches the edge along one axis (KiCad: a warning, or nothing when copper remains all round). Also `"Corner radius ratio must be between 0 and 50%."` and `"Solder paste ratio must be between -50% and 100%."` (`:208-209`) are not KiCad's: KiCad clamps the corner ratio in `onCornerSizePercentChange` and has no such messages.
- **How to see it.** A 2 x 1 mm rectangular PTH pad, drill 1.2 mm, OK: KiCad shows "Warning: (PTH pad hole not fully inside copper)" and applies; we refuse. A 1 mm round pad with a 1 mm hole: KiCad "Warning: (PTH pad hole leaves no copper)" and applies; we refuse. Test: `fpFields.test.ts` `a hole wider than the pad is a warning, not an error` (`checkPadValues({kind:"through_hole", shape:"rect", size:[2e6,1e6], drill:1.2e6}, fmt)` returns no error; a new warnings list holds `"Warning: (PTH pad hole not fully inside copper)"`).
- **Severity.** medium (a refused edit KiCad allows, and messages not KiCad's).
- **Already known?** No.

### C6. Escape in a drawing tool with a shape started leaves the tool; KiCad cancels the shape and keeps the tool
- **What differs.** `DRAWING_TOOL::drawShape` (`pcbnew/tools/drawing_tool.cpp`, the `IsCancelInteractive()` branch):
  ```cpp
  if( evt->IsCancelInteractive() || ( started && evt->IsAction( &ACTIONS::undo ) ) )
  {
      cleanup();
      if( !started )
      {
          // We've handled the cancel event.  Don't cancel other tools
          evt->SetPassEvent( false );
          m_frame->PopTool( aTool );
          cancelled = true;
      }
      break;
  }
  ```
  and `DrawLine` loops `while( drawShape( ... ) )`: the first Escape drops the line (or rectangle, circle, arc, polygon) being drawn and the tool stays armed; a second Escape leaves it. Undo with a shape started is the same cancel. `DrawZone`, `DrawDimension` and `DrawArc` follow the same pattern.
  Ours: `common.Interactive.cancel` (`web/actions/useActionRunner.ts:853-875`) dispatches `ESCAPE`, whose reducer (`web/state/store.tsx:1365-1401`) sets `activeTool: "select"` and `drawState: null` whenever anything is in progress, so one Escape both drops the shape and leaves the tool. PARITY section 3's row "Escape: cancel the active tool/drag/arm ..." is marked identical.
- **How to see it.** Press the line tool, click a start point, Escape: KiCad's line tool is still active (the next click starts a new line); ours is back to Select. Test: a `store` reducer test `ESCAPE with a line started keeps the line tool` (state `activeTool: "line"`, `drawState: {kind: "line", start: ...}`; after `ESCAPE`, `activeTool === "line"` and `drawState == null`; a second `ESCAPE` gives `"select"`).
- **Severity.** medium.
- **Already known?** No.

### C7. Paste of plain text: trimmed, and 1 mm / 0.15 mm where KiCad's text is 1.27 mm with a default pen
- **What differs.** `PCB_CONTROL::Paste` (`pcbnew/tools/pcb_control.cpp:1185-1206`): "If it wasn't content, then paste as a text object": `item->SetText( clipText ); item->SetLayer( m_frame->GetActiveLayer() );` -- the text as it is, the `EDA_TEXT` defaults (`common/eda_text.cpp:107`: `Mils2IU( DEFAULT_SIZE_TEXT )`, `DEFAULT_SIZE_TEXT 50` mils = 1.27 mm; `TEXT_ATTRIBUTES` `m_StrokeWidth( 0 )`), no `StyleFromSettings`; longer than `m_MaxPastedTextLength` asks "Pasting a long text text string may be very slow.  Do you want to continue?"; the commit is "Paste Text" and the item is carried by `placeBoardItems`.
  Ours, `web/state/store.tsx:2543-2544`: `content: system.trim()`, `size_nm: 1_000_000`, `stroke_width: 150_000`, layer `activeLayerName(st.activeLayer, "F.SilkS")`; no long-text question.
- **How to see it.** Copy "  hello\n" from another program, Ctrl+V on the board: KiCad's text is "  hello\n" (two lines), 1.27 mm high; ours "hello", 1 mm. Test: `store` test `a plain-text paste is the clipboard text as is, 1.27 mm`.
- **Severity.** medium (a wrong default).
- **Already known?** No.

## Low

### C8. Footprint Properties: a new field is `Field2` (KiCad `Field4`); duplicate and reserved names are refused where KiCad refuses neither
- **What differs.** `DIALOG_FOOTPRINT_PROPERTIES::OnAddField` names the field `GetUserFieldName( m_fields->size(), DO_TRANSLATE )` ("Field%d"), and `m_fields` holds the four mandatory fields (Reference, Value, Datasheet, Description), so the first is `Field4`; `OnDeleteField` says `"The first %d fields are mandatory."` with 4. `Validate` (`dialog_footprint_properties.cpp:395-500`) checks only an empty name and the sizes; no uniqueness or reserved-name check.
  Ours: `newFieldName(rs.map(...), rs.length)` with two mandatory rows (`web/components/FootprintPropertiesDialog.tsx:345`, `web/kicad-port/fpFields.ts:176-184`, whose own comment says "`Field4`, `Field5` ..."), and it skips names already used; `submit` refuses `"\"${r.name}\" is the name of a mandatory field."` and `"There are two fields called \"${r.name}\"."` (`FootprintPropertiesDialog.tsx:144-145`), and the delete button's tooltip is "The first two fields are mandatory.". (The refusals may be needed by our name-keyed fields; then they are a documented difference, not identical as PARITY section 26 says.)
- **How to see it.** Open Footprint Properties on R1, Add Field: KiCad's row is `Field4`; ours `Field2`. Test: `fpFields.test.ts` `the first added field is Field4`.
- **Severity.** low.
- **Already known?** No.

### C9. GuessSelectionCandidates' silk and courtyard preferences: not ported, for a reason that no longer holds
- **What differs.** `PCB_SELECTION_TOOL::GuessSelectionCandidates` first prefers, with a silk layer active, "single-layer items on *either* silk layer" (`PCB_FIELD_T, PCB_TEXT_T, PCB_TEXTBOX_T, PCB_TABLE_T, PCB_TABLECELL_T, PCB_SHAPE_T, PCB_BARCODE_T`), and with a courtyard layer active in high contrast, footprints. Ours (`web/kicad-port/selection.ts:159-195`) starts at the slop pass. PARITY section 3 says "no distinct silk-text item type in this app's model to special-case": fields, texts, shapes, text boxes and barcodes on F/B.SilkS are all items now.
- **How to see it.** Active layer F.SilkS, click where a footprint's Reference overlaps one of its pads: KiCad picks the Reference; ours the smaller pad. Test: `selection.test.ts` `a silk layer in front prefers silk items`.
- **Severity.** low.
- **Already known?** As "not ported" in PARITY section 3, with a stale reason.

### Checked and found to match (do not re-review)
- Click versus drag: per-axis strict 8 px from the press point, the macOS-only 300 ms rule evaluated on a motion, a click reported at the press position (`tool_dispatcher.cpp::handleMouseButton` vs `web/kicad-port/dragThreshold.ts`).
- `PCB_GRID_HELPER::BestSnapAnchor` line by line (`pcbGridHelper.ts:383-512`): 25 px range clamped to the visible grid while the grid is on, hysteresis in/out, snap lines before anchors with "prefer actual anchors" inside `snapIn`, reference-only points, the kept `m_snapItem` within `snapOut`, construction proposals for non-constructed anchors, hover activation, the point-on-element snap only with the grid off, the grid fallback cancelling the proposal.
- `GetItemGrid` mapping (footprint/pad connectable, text/field text, shape/dimension/image/textbox/barcode graphics, track wires, via vias, else current), `GetGridSize` override rule, `GetSelectionGrid` (coarsest), the PCB and schematic override defaults of `app_settings.cpp:495-545`.
- `::FlipLayer` for the fixed pairs and inner copper (`common/layer_id.cpp:173-215`) vs `flip_layer` (except C4's user layers).
- `PCB_TEXT::Flip` (left-right negates, top-bottom 180 - angle, mirror toggled on a side-specific layer) and `PCB_TEXT::Rotate`; `FOOTPRINT::Flip`'s orientation (`-rot`, plus the half turn for LEFT_RIGHT) against `part_pose`; `EDA_SHAPE::rotate` of segments, circles, arcs, Béziers, polygons and cardinal rectangles.
- Pack and Move's `std::map<VECTOR2I>` ordering by squared length (`VECTOR2::operator<`, `vector2d.h:570-573`).
- Escape's idle tiers: clear the selection, else leave the entered group without selecting it, else clear the net highlight (`m_ESCClearsNetHighlight` default true) -- `pcb_selection_tool.cpp:537-565` vs the `ESCAPE` reducer.
- Footprint Properties `Validate`: "Fields must have a name.", "Text width/height must be at least/at most %s.", "Text thickness is too large for the text size.\nIt will be clamped at %s.", in KiCad's order; the dialog's apply order (orientation before the side flip, flip in `m_FlipDirection`).
- Pad Properties: "Error: (Pad must have a positive size)" (a circle needs only x) and "Error: Through hole pad has no hole." for PTH and NPTH.
- Global edit tracks and vias, "specified values": a track width, a via size and a layer per item; the layer applies to tracks and arcs only.
- `copyToClipboard`'s reference point `BestDragOrigin` and `cutToClipboard` filtering locked items (batch F, `33dd31d4`).


---

# Summary

| # | Finding | Severity |
|---|---|---|
| R1 | Delete of a lone zone from inside a cutout deletes the zone; KiCad removes the cutout only | high |
| R2 | Rotate turns a dimension's (not kept aligned) text angle the wrong way: `rotate_dimension` adds a clockwise angle to a counter-clockwise one | high |
| R3 | Dimension precision V_VV..V_VVVVV read as a flat count and written back changed | high |
| A1 | One-member group dropped on import, Paste, Duplicate, Create Array | high |
| A2 | Footprint Orientation 180 degrees off for a back footprint flipped here; 0..360 shown | high |
| C1 | Non-cardinal Rotate of a rounded rectangle drops its corner radius | high |
| C2 | Cut deletes what the failed Copy did not copy; fields/barcodes/points/text boxes/images fail Copy; a pad copies its footprint | high |
| C3 | Global edit "net class / custom rule values" ignores custom rules, diff-pair and tuning widths | high |
| R4 | Mirror skips footprint fields, points and tuning patterns | medium |
| R5 | Position Relative forgets offsets, polar box and the picked item between openings | medium |
| A3 | Text boxes, barcodes, images, tuning patterns outside the group bookkeeping | medium |
| A4 | Group Items does not lock the group of a locked item; no Locked box | medium |
| A5 | Track/Via and Shape Properties dialogs lack KiCad's fields and edit one item | medium |
| B1 | Menu summary ignores fields, text boxes, barcodes, points, images, generators (Net Inspection shown, Mirror / Rotate missing) | medium |
| B2 | Copy as Text: no fields, text boxes, tables; raw text, not `GetShownText` | medium |
| C4 | Flip ignores front/back user layer pairs | medium |
| C5 | Pad Properties refuses a hole at the pad's edge (KiCad warns); invented messages | medium |
| C6 | Escape with a shape started leaves the drawing tool | medium |
| C7 | Plain-text paste trimmed, 1 mm / 0.15 mm instead of 1.27 mm / default pen | medium |
| R6 | R right after M (no motion) turns about the selection's point, not the grab point | low |
| R7 | A via stitch with vias is gated by Other items, not Vias | low |
| R8 | Courtyard box misses half the courtyard line width | low |
| R9 | Swap Layers leaves tables on the old layer | low |
| R10 | Doc drift: dimension Rotate/Flip marked not ported; whole-degree aligned text angle; extension height rounding | low |
| A6-A11 | Panel rows, zero-size validators, via type, Group Properties, doc drift, Group verb nesting | low |
| B3-B6 | Net Inspector sort, class filter, report order; appearance colour rename collision | low |
| C8-C9 | New field name Field2; silk/courtyard selection preference | low |

Counts: high 8, medium 11, low 17 (R 5, A 6, B 4, C 2). Batch F's p6 fixes F1-F18 and the group fixes were checked against KiCad and are right, except the leftovers named above (R6, R8) and the ones batch F itself lists as not done.
