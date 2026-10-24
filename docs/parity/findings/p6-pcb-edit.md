# Findings: P6, PCB editing details and the board editor's edit tool (`p6-pcb-edit`)

Reviewer: `p6-pcb-edit`, 2026-10-23. Findings only, no code changed. Read from main's checkout (`/home/team_braneboard_com/work/agentic-eda`, HEAD `be562f2e`) against KiCad `8303b2ad` (`$KICAD_SRC_DIR`). Scratch dir: `/tmp/findings-p6-pcb-edit.bVaa`.

Paths on our side are relative to the repo root; `web/` means `web/studio/src/`. KiCad paths are relative to `$KICAD_SRC_DIR`.

Not re-reported: Move Exactly snapping to the placer's 0.1 mm grid (fixed in P20). Known and already listed in GAPS/PARITY, so not counted here either: `--strict` refusing an edit that breaks a complete net (GAPS item 8), the footprint box missing silkscreen and fab graphics beyond courtyard and body (item 8), no `gr_rect` corner radius in the IR (so no `RECT_RADIUS` handle), no aux-origin / `ROTATE_AROUND_AUX_ORIGIN` *as documented* (but see F14: the board does have one now), Duplicate/Paste/Array/Mirror of text boxes and images (PARITY section 39), the arc point editor's Ctrl+Space, the dimension point editor (PARITY section 18, "not ported").

---

## High

### F1. Position Relative's polar angle runs the wrong way (and its "Clear" shows the mirrored angle)
- **Fixed** on task-fix-f-editing: 9b6b6694 (`polarTranslation`/`toPolarDeg` negate, Offset Item too; the test asserts KiCad's value).
- **What differs.** `DIALOG_POSITION_RELATIVE` gives the Y/angle entry the coordinate type `REL_Y_COORD` (`pcbnew/dialogs/dialog_position_relative.cpp:59`: `m_yOffset.SetCoordType( ORIGIN_TRANSFORMS::REL_Y_COORD );`). In polar mode the entry is an angle read with `UNIT_BINDER::GetAngleValue()`, which is `m_originTransforms.FromDisplay( EDA_ANGLE( GetDoubleValue(), DEGREES_T ), m_coordType )`, and `PCB_ORIGIN_TRANSFORMS::FromDisplay( const EDA_ANGLE&, ... )` negates it when the Y axis is not inverted (the default):
  ```cpp
  if( !invertYAxis() && ( aCoordType == REL_X_COORD || aCoordType == REL_Y_COORD ) )
      return -aValue;
  ```
  `getTranslationInIU` then takes `val.y = KiROUND( r * q.Sin() )` of the negated angle, so a typed 90 degrees moves **up** the screen (counter-clockwise), and `OnClear`'s `SetAngleValue( q )` goes through `ToDisplay`, which negates again, so the field shows a counter-clockwise angle.
  Ours: `web/kicad-port/pcbEditActions.ts:345-349` `polarTranslation(r, deg)` is `y = r sin(deg)` with no negation, and `toPolarDeg` (`:340`) shows `atan2(y, x)` in Y-down coordinates. 90 degrees moves **down**. The Move Exactly dialog in the same app negates (`web/components/MoveExactDialog.tsx:73`, `dyUm = -dist * sin(angle)`), which is right for `DIALOG_MOVE_EXACT` (same `REL_Y_COORD` on `m_moveY`), so the two dialogs disagree.
- **How to see it.** Select a footprint, Position Relative, anchor the local origin at the footprint, tick Polar, Distance 10 mm, Angle 90, OK: KiCad puts it 10 mm above; we put it 10 mm below. Test: `pcbEditActions.test.ts` `polarTranslation follows DIALOG_POSITION_RELATIVE's REL_Y_COORD angle`: `assert.deepEqual(polarTranslation(1000, 90), { x: 0, y: -1000 })` and `toPolarDeg(0, -1000).deg === 90`. (The present test at `pcbEditActions.test.ts:154` asserts the wrong `y: 1000`.)
- **Unit-sensitive:** no.

### F2. A shape whose handles are dragged, and a shape or track that is mirrored, is deleted and re-added: it leaves its group (the group can dissolve) and gets a new id
- **Fixed** on task-fix-f-editing: 85a39921 (`reshapeCmds` always `replace_shape`; new verb `mirror_tracks`).
- **What differs.** KiCad's point editor and `EDIT_TOOL::Mirror` change the item in place: `commit.Modify( item )` then `UpdateItem` / `PCB_SHAPE::Mirror` / `PCB_TRACK::Mirror`, so the item keeps its UUID and its parent group (`pcbnew/tools/pcb_point_editor.cpp::OnSelectionChange`, `commit.Push( _( "Move Point" ) )`; `pcbnew/tools/edit_tool.cpp::Mirror`).
  Ours:
  - shape handle drop: `web/components/canvas/Canvas.tsx:1303-1310` sends `reshapeCmds`, which for any shape without hatch, line ending or net is `[{ op: "delete_shape", id }, { op: "add_shape", shape }]` (`web/kicad-port/shapeFill.ts:81-86`); `add_shape` throws the id away and assigns a new one (`crates/ops/src/lib.rs:4206`).
  - Mirror: `web/kicad-port/pcbMirror.ts:95-133` re-creates shapes the same way and tracks through `commit_route` (`remove_track_ids` + new `tracks`).
  - `Board::apply` then runs `tidy_groups` (`crates/ops/src/pcb_groups.rs:157-173`): the old id is gone, so it is dropped from its group, and a group left with one member is dissolved.
- **How to see it.** Draw two lines, group them (Ctrl+G), enter the group, drag one line's end handle: the group is gone from the board and from the saved `.kicad_pcb`. The same happens after Mirror Horizontally of one grouped track. Test: an ops or cli test `point_edit_of_a_grouped_shape_keeps_it_in_its_group`: board with group `{s1, s2}`, apply the studio's batch for a handle drag of `s1`, assert the group still exists with two members, one of them the edited shape. The fix needs `replace_shape` (it exists, `Cmd` `replace_shape`) for every shape, and an in-place track edit.
- **Severity:** high (data loss: the group, and the item's UUID in the file).
- **Unit-sensitive:** no.

### F3. A footprint's box (Rotate/Flip pivot, Align/Distribute, click hit-test) uses the symmetric courtyard enclosure, not KiCad's box
- **Fixed** on task-fix-f-editing: 5066c1b1 (the studio's `courtyard` is `placed_courtyard_box`: the real outlines put on the board; the placer keeps its symmetric box).
- **What differs.** `FOOTPRINT::GetBoundingBox( bool aIncludeText )` (`pcbnew/footprint.cpp:1731`) starts from `BOX2I bbox( m_pos ); bbox.Inflate( pcbIUScale.mmToIU( 0.25 ) );` and merges the real drawings (courtyard lines included), pads, zones and points. Ours, `web/kicad-port/footprintBox.ts:46`, merges `part.courtyard`, which the backend sends as `placed_courtyard` (`crates/cli/src/studio.rs:1198`, `crates/model/src/footprint.rs:300-305`): `courtyard_half()`, a box **symmetric about the footprint origin** ("a library footprint loaded with an asymmetric courtyard is conservatively *enclosed* by the smallest symmetric box", `footprint.rs:313-320`), or, with no courtyard, the pad box plus 0.25 mm made symmetric (`footprint.rs:715-723`); and for a rotation that is not a multiple of 90 degrees `rotated_extent` grows it further.
  For any footprint whose origin is not its centre (pin headers and connectors with the origin at pin 1, most THT parts) the box is up to twice as long as KiCad's on that axis.
- **How to see it.** Import a board with a `PinHeader_1x04_P2.54mm_Vertical` (origin at pin 1; courtyard y from -1.77 to 9.39 mm). Our box is y -9.39 .. 9.39, KiCad's -1.77 .. 9.39 (plus text). Align Top of that header with a resistor moves the header by 7.62 mm too little; Rotate of the header with another part turns about a different centre; a click 5 mm above pin 1 selects the header (`selectionCandidates.ts:233-243` also tests `p.courtyard`). Test: `footprintBox.test.ts` `an off-centre courtyard gives KiCad's box, not a symmetric one`: a part with `at [0,0]`, courtyard lines from (-1770,-1770) to (1770, 9390): assert `footprintBox(p, false)` is `[-1770, -1770, 1770, 9390]`. The data fix is to send the real courtyard box (the outlines exist: `Footprint::courtyard_outlines`).
- **Severity:** high (wrong result of Align/Distribute and of a multi-item Rotate/Flip).
- **Unit-sensitive:** no.

### F4. R during a carried Move turns about the wrong point; R after F turns the wrong way
- **Fixed** on task-fix-f-editing: 55547f28 (the presses kept in order with their pivots, `MovePreview.ops`, `pcbCarry.ts carryPress`; R about the grab point; F of several about the box centre of the moment).
- **What differs.** In `EDIT_TOOL::doMoveSelection` the selection's reference point is the cursor: it starts at `grid.BestDragOrigin( originalMousePos, sel_items, ... )` (the nearest anchor of the picked-up items to the mouse, e.g. the pad you grabbed) and is updated on every motion (`selection.SetReferencePoint( m_cursor );`, `edit_tool_move_fct.cpp` ~1158). `EDIT_TOOL::Rotate` during a drag does not replace it (`updateModificationPoint`: `if( ( m_dragging || aSelection[0]->IsMoving() ) && aSelection.HasReferencePoint() ) return false;`, and `usePcbShapeCenter` requires `!m_dragging`), so it turns about the cursor. `Flip` during a drag mirrors about `selection.GetCenter()` of the selection **as it is now** (several items) or the reference point (one item). Each press is applied to the items in the order pressed.
  Ours: the carry keeps one quarter-turn count and one `flipped` boolean (`web/actions/useActionRunner.ts:244-265`) and `carryMatrix` always applies turn first, then flip, about pivots fixed at pick-up: `pivotUm = rotationPivot(...)` (the item's own position, or the snapped centre of the box) and `flipPivotUm = flipPivot(...)` (`web/kicad-port/pcbTransform.ts:268-274`, `web/kicad-port/pcbCarry.ts:62-75`, `planCarry` `pcbTransform.ts:208-225`). So:
  1. A footprint grabbed by a pad turns about its origin, not about the pad under the cursor; a multi-item carry turns about the box centre, not the grab point. The item lands somewhere else than in KiCad.
  2. F then R: KiCad gives Rot(+step) after Flip; we give Flip after Rot(+step) = Rot(-step) after Flip. After a flip, R visibly turns the carried items clockwise.
- **How to see it.** Grab a 2-pin resistor by pad 2 (2 mm right of its origin), press R, drop without moving: KiCad turns about pad 2, which stays where it was; we turn about the footprint origin, so pad 2 ends 2 mm above the origin, about 2.8 mm from where it was. Test: `pcbCarry.test.ts` `a carried turn is about the grab point`: `planCarry` given a grab point `[2000, 0]` and one R press must rotate about `(2000, 0)`; and `F then R equals KiCad's order`: the transform for presses `[F, R]` must equal `rot(90) ∘ flip`, not `flip ∘ rot(90)` (needs the presses kept as an ordered list, as `tryHeldTurn` does on the schematic).
- **Severity:** high for (2) (a wrong orientation for a common key sequence); (1) medium.
- **Unit-sensitive:** no.

---

## Medium

### F5. Undo during a Move undoes the previous edit instead of cancelling the Move; during a carried Duplicate/Paste it leaves a Redo entry
- **Fixed** on task-fix-f-editing: 38d681a8 (Undo during a board Move cancels it, Redo does nothing; the backend's carry is not asked).
- **What differs.** In `doMoveSelection`: `else if( evt->IsAction( &ACTIONS::undo ) ) { restore_state = true; // Perform undo locally break; }` and Redo is refused (`... || evt->IsAction( &ACTIONS::redo ) ) { wxBell(); }`). The move's commit is reverted; nothing on the undo stack is touched. For Duplicate/Paste the whole commit is `Revert()`ed (no undo, no redo entry).
  Ours: `common.Interactive.undo` (`web/actions/useActionRunner.ts:602`) always calls `api.undo()` (`web/state/store.tsx:2383-2388`), which neither ends the Move tool nor reverts a carry. With a carry pending, `crates/cli/src/carry.rs` "settles" it as its own step first, then undoes it: the copies vanish but a Redo brings them back, and the Move tool is still armed on ids that no longer exist. Redo during a move runs too.
- **How to see it.** Draw a line, select a footprint, press M, move the mouse, Ctrl+Z: KiCad drops the move and the line stays; we remove the line and keep carrying the footprint. Ctrl+D then Ctrl+Z: KiCad leaves no redo entry; we leave one. Test (cli + web): `undo_during_a_carry_reverts_it_without_a_redo_entry` (begin a carry, POST undo, assert the redo stack is unchanged); and a `useActionRunner` test that undo with `activeTool === "move"` dispatches the move cancel instead of `api.undo`.
- **Unit-sensitive:** no.

### F6. Move Individually commits each drop as its own undo step, and Escape keeps the items already dropped
- **Fixed** on task-fix-f-editing: d99c8195 (new carry step `hold`; the drops are one step, Escape or Undo reverts every drop). Not done: the double click that finishes and puts the current item back.
- **What differs.** `doMoveSelection`'s `moveIndividually` branch picks up the next item into the **same** `aCommit` (`aCommit->Modify( nextItem, ... )`, `edit_tool_move_fct.cpp` ~1405) and `EDIT_TOOL::Move` pushes it once (`localCommit.Push( _( "Move" ) )`); Escape (`IsCancelInteractive`) sets `restore_state` and the whole commit is reverted, so every item goes back. A double-click finishes and puts the current item back (`orig_items[itemIdx]->SetPosition( originalPos )`).
  Ours: each drop is a separate `commitMove` → `runPlan` → one `/api/cmd` batch, then `advanceMoveQueue` (`web/state/store.tsx:2159-2183`, `:2300-2311`); Escape only clears the queue (`store.tsx:1357`, `:1392`).
- **How to see it.** Select three footprints, Ctrl+M, drop two, Escape: KiCad has all three where they were and no undo step; we have two moved and two undo steps. Test: `store` or e2e `move individually is one undo step and Escape reverts every drop`.
- **Unit-sensitive:** no.

### F7. Shift+Del (`deleteFull`) is several undo steps, ignores locks, groups, tables and tuning patterns
- **Fixed** on task-fix-f-editing: 38d681a8 (Shift+Del hands the expanded ids to `deleteRefs`: one batch, locks, groups, tables, fields, tuning patterns).
- **What differs.** `EDIT_TOOL::Remove` with `REMOVE_FLAGS::ALT` runs the same `RequestSelection` (with `FilterCollectorForLockedItems`), expands tracks with `selectConnection`, then the one `DeleteItems` → one `commit.Push( _( "Delete" ) )`.
  Ours, `web/actions/useActionRunner.ts:1683-1706`: the copper goes in one `commit_route`, then **one `api.cmd` per other item** (each its own undo step); no `isLocked` check at all (a locked zone, shape, footprint or text is deleted); a group, a table, a table cell, a dimension of an entered group and a tuning pattern are not handled (`boardDeleteCmds` in `kicad-port/deleteCmds.ts` is not used).
- **How to see it.** Select a track, a zone and a text, Shift+Del: three undo steps (KiCad: one). Lock the zone first: we delete it anyway (KiCad keeps it and says "Selection contains locked items. Enable 'Override locks' to operate on them." only if nothing is left). Test: a `useActionRunner` test that `deleteFull` issues exactly one `cmdBatch` built by `boardDeleteCmds` over the expanded ids.
- **Unit-sensitive:** no.

### F8. Delete of a member keeps a one-member group in KiCad; ours dissolves it
- **Fixed** on task-fix-f-editing: 52f2626d (`tidy_groups` drops only empty groups; the sheet still dissolves a group of one, GAPS item 17).
- **What differs.** `BOARD_COMMIT::Push` (`pcbnew/board_commit.cpp:372-373`) only does `parentGroup->RemoveItem( boardItem )` for a removed item; nothing dissolves the group. A one-member group stays on the board and is saved (`PCB_IO_KICAD_SEXPR::format( const PCB_GROUP* )` skips only an **empty** group: `if( memberIds.empty() ) return;`). The `< 2` rule belongs to `GROUP_TOOL::RemoveFromGroup` ("Remove Items") only (`common/tool/group_tool.cpp:273`).
  Ours: `tidy_groups` (`crates/ops/src/pcb_groups.rs:157-173`) runs after every command and calls `prune_groups`, which dissolves any group under two members (`crates/model/src/groups.rs:247-258`). GAPS item 17 records this for the sheet only ("a group of one member dissolves (KiCad keeps it)"); the board has the same divergence.
- **How to see it.** Group a line and a text, delete the text: KiCad's file still has `(group "" ... (members <line uuid>))`; ours has no group. Test: `pcb_groups_tests.rs` `deleting_one_of_two_members_keeps_a_one_member_group`.
- **Unit-sensitive:** no.

### F9. Delete on a selected footprint field does nothing (KiCad hides it)
- **Fixed** on task-fix-f-editing: 57388c01.
- **What differs.** `EDIT_TOOL::DeleteItems`, `case PCB_FIELD_T:` hides a visible field (`field->SetVisible( false )`, commit "Hide Field" / "Hide Fields") and, for one already hidden, says `_( "Use the Footprint Properties dialog to remove fields." )`.
  Ours: `boardDeleteCmds` (`web/kicad-port/deleteCmds.ts:46-71`) has no field case; a `REF:Name` id is not `known` and is skipped, so nothing happens and nothing is said.
- **How to see it.** Click a footprint's Value text, Del: KiCad hides it (one undo step). Test: `deleteCmds.test.ts` `a field is hidden, not deleted`: `boardDeleteCmds(board, ["R1:Value"]).cmds` is one `edit_board_field` with `visible: false`.
- **Unit-sensitive:** no.

### F10. Locked items: a locked zone's corners can be dragged; Mirror moves a locked group; L cannot lock a group, dimension, table or field; there is no "Override locks"
- **Fixed in part** on task-fix-f-editing: 31169eb0 (a locked zone has no handles, Mirror filters locked groups, L locks groups/dimensions/tables). Not done: the toolbar's Override locks (and so KiCad's longer message).
- **What differs.**
  - `PCB_POINT_EDITOR::OnSelectionChange`: `if( !item || item->IsLocked() ) return 0;` — no handles on a locked item. Ours: the zone-corner drag (`web/components/canvas/Canvas.tsx:910-919`) and the zone's double-click Add Corner (`:1455-1468`) never check the lock (the shape branch at `:931` does).
  - `EDIT_TOOL::Mirror` uses `FilterCollectorForLockedItems` (an item whose group is locked, or a group with a locked descendant, is left out). Ours, `web/actions/pcbEditSweep.ts:196-210`, filters `lockedIds.has(id)` on the expanded **members** only, so a locked group (or a member of one) is mirrored.
  - `BOARD_EDITOR_CONTROL::modifyLockSelected` locks every `BOARD_ITEM` but pads (groups, dimensions, tables, fields included). Ours, `web/actions/useActionRunner.ts:1670-1677`, keeps only parts, tracks, vias, zones, shapes, texts and extras; a group or dimension is silently dropped, although `Cmd::SetLocked` accepts both (`crates/ops/src/lib.rs:4054-4061`).
  - KiCad's toolbar "Override locks" (`EDA_DRAW_FRAME::GetOverrideLocks`) lifts `FilterCollectorForLockedItems`; ours has none, and our toast is "Selection contains locked items." where KiCad's is `"Selection contains locked items. Enable 'Override locks' to operate on them."`.
- **How to see it.** Lock a zone (L), select it, drag a corner: it moves. Select a group or a dimension and press L: nothing happens. Tests: Canvas/e2e `a locked zone shows no handles`; `pcbEditSweep` `mirror leaves a locked group alone`; `modifyLock locks a dimension and a group`.
- **Unit-sensitive:** no.

### F11. Selection Filter: no "Rule Areas" row; a single pad of a locked footprint cannot be clicked; a member of a locked group is pickable; "Only ..." and the rejected-row flash are missing
- **Fixed in part** on task-fix-f-editing: f6a731d3 (Rule Areas row, a locked group's members locked, a lone pad of a locked footprint on a click, KiCad's rows and order). Not done: "Only ..." and the flash of the rejecting row.
- **What differs** (`pcbnew/tools/pcb_selection_tool.cpp::itemPassesFilter`, `pcbnew/widgets/panel_selection_filter.cpp`):
  - Zones are `m_filter.zones` and rule areas `m_filter.keepouts` (`( !m_filter.zones && !zone->GetIsRuleArea() ) || ( !m_filter.keepouts && zone->GetIsRuleArea() )`), row "Rule Areas". Ours (`web/components/canvas/selectionCandidates.ts:76-77`) gates every zone, rule area or not, by `zones`; the panel (`web/components/panels/SelectionFilterPanel.tsx:10-21`) has no Rule Areas row.
  - Locked filter exception: `if( aItem->Type() == PCB_PAD_T && !aMultiSelect ) { // allow a single pad to be selected }`. Ours rejects the pad with its footprint's lock (`selectionCandidates.ts:219`, `lockedId = p.ref`).
  - `BOARD_ITEM::IsLocked()` is true for a member of a locked group (`if( group->AsEdaItem()->IsLocked() ) return true;`), so with "Locked items" off nothing of a locked group can be clicked. Ours tests `locked.has(lockedId)` on the item's own id only, then substitutes the group, so a locked group is selected.
  - The panel's order and labels: KiCad "All items", "Locked items", Footprints, Text, Tracks, Vias, Pads, Graphics, Zones, Rule Areas, Dimensions, Points, Other items; ours "All Items" and a different order. KiCad has the right-click `"Only %s"` menu and flashes the row that rejected a click (`HighlightSelectionFilter( rejected )` in `selectPoint`); ours has neither.
- **How to see it.** Untick Zones with a keepout on the board: KiCad still selects the keepout; we cannot. Lock a footprint, click one of its pads with "Locked items" off: KiCad selects the pad. Tests: `selectionCandidates` `a rule area follows keepouts, not zones`; `a lone pad of a locked footprint passes the filter on a click, not in a box`.
- **Unit-sensitive:** no.

### F12. Align Left/Right in a flipped board view align the wrong side
- **Fixed** on task-fix-f-editing: f6e2506d.
- **What differs.** `ALIGN_DISTRIBUTE_TOOL::AlignLeft`: "Because this tool uses bounding boxes and they aren't mirrored even when the view is mirrored, we need to call the other one if mirrored." `if( getView()->IsMirroredX() ) return doAlignRight();` (and the reverse in `AlignRight`).
  Ours: `web/kicad-port/alignDistribute.ts:12-13` says "Source's mirrored-view swap of Align Left/Right is not ported: this app's view never mirrors a PCB" — stale: Flip Board View exists (`state.bcx.boardFlipped`, used by `Canvas.tsx:364`, `:451` and by Rotate in `store.tsx:2256-2258`). `alignSelection` (`store.tsx:2276-2282`) never looks at it.
- **How to see it.** View > Flip Board View, select two parts, Align Left: they line up on the screen's right edge. Test: `alignDistribute.test.ts` `a flipped view swaps left and right` (with a `flipped` flag passed to `planAlignSelection`).
- **Unit-sensitive:** no.

### F13. Distribute promotes pads to footprints; Position Relative anchors a lone pad's footprint, not the pad
- **Fixed** on task-fix-f-editing: 6b39b82b.
- **What differs.**
  - `ALIGN_DISTRIBUTE_TOOL::DistributeItems`: "Don't filter for free pads. We want to allow for distributing other items (such as a via) between two pads." The pads keep their own boxes. Ours: `planDistributeSelection` runs `editableSelection`, which turns each pad into its footprint (`web/kicad-port/alignDistribute.ts:224-227`), so a via is spaced between two footprint boxes.
  - `POSITION_RELATIVE_TOOL::PositionRelative`'s `RequestSelection` has no `FilterCollectorForFreePads`; the anchor is "We prefer footprints, then pads, then anything else": with a pad (no footprint) selected, `m_selectionAnchor` is the pad's position; `moveSelectionBy` moves its parent. Ours promotes the pad first (`web/components/PcbParityDialogs.tsx:124-131`), so the anchor is the footprint origin and the footprint origin, not the pad, lands on the target (the comment at `pcbEditActions.ts:321-323`, "This app has no free pads, so the middle tier never applies", is wrong: the middle tier is a selected pad).
- **How to see it.** Select a pad of U1, Position Relative to a point (10, 10): KiCad puts that pad at (10, 10); we put U1's origin there. Test: `pcbEditActions.test.ts` `positionRelativeSelectionAnchor prefers a pad over other items` (needs pads passed as their own tier).
- **Unit-sensitive:** no.

### F14. Position Relative's "Grid origin" is (0, 0); Move Exactly has no "drill/place origin" and offers both item and centre anchors
- **Fixed** on task-fix-f-editing: 4b2a4b72 (grid origin, `s_anchorType`, Move Exactly's menu with the drill/place origin, the values of last time).
- **What differs.** `DIALOG_POSITION_RELATIVE::getAnchorPos`: `case ANCHOR_GRID_ORIGIN: return ... GetDesignSettings().GetGridOrigin();`. Ours: `web/components/PcbParityDialogs.tsx:186-187` returns `{ x: 0, y: 0 }` with the comment "this model has no grid-origin setting" — stale: the board has `grid_origin` (`web/api/types.ts:1017`, verb `set_grid_origin`).
  `DIALOG_MOVE_EXACT` lists exactly `m_menuIDs = { aAnchor, ROTATE_AROUND_USER_ORIGIN, ROTATE_AROUND_AUX_ORIGIN }` with `aAnchor` = item anchor for one item, selection centre for several, in the words "Rotate around item anchor", "Rotate around selection center", "Rotate around local coordinates origin", "Rotate around drill/place origin". Ours (`web/components/MoveExactDialog.tsx:120-126`) offers item, centre and local origin always, with other labels ("Each item's own anchor", "Selection center", "Local coordinates origin"), and no drill/place origin, although the board has `aux_origin` (`types.ts:1015`, `set_aux_origin`). PARITY section 6 says the aux origin "has no model equivalent": stale.
  Also the dialog remembers its values (DIALOG_SHIM state, `TransferDataToWindow`: "Force the evaluation when setting previous values"); ours resets every field to 0 on open (`MoveExactDialog.tsx:55-66`); Position Relative's anchor type is static (`s_anchorType`, starts `ANCHOR_ITEM`), ours starts on "origin" every time.
- **How to see it.** Set a grid origin at (50, 50) mm, Position Relative to Grid origin with offset 0: KiCad moves the selection anchor to (50, 50), we to (0, 0). Test: a dialog test that `referenceAnchor()` for "grid" returns `board.grid_origin`.
- **Unit-sensitive:** no.

### F15. Copy's reference point is the cursor's grid point, not `BestDragOrigin`
- **Fixed** on task-fix-f-editing: 33dd31d4 (`dragOriginAnchor`, BestDragOrigin of the copied items).
- **What differs.** `EDIT_TOOL::copyToClipboard`: `refPoint = grid.BestDragOrigin( getViewControls()->GetCursorPosition(), items );` — the nearest anchor of the copied items to the cursor, so a paste puts that anchor (a footprint origin, a pad, a line end) on the cursor and on the grid. Ours: `web/state/store.tsx:2494` uses `snapPoint(cursor, gridUm)` (no grid origin either), so the pasted items hang at an arbitrary offset from the cursor and land off the anchors they were on.
- **How to see it.** Copy a footprint with the cursor 0.3 mm from its origin on a 1 mm grid, paste: KiCad's footprint origin is on the cursor grid point; ours is 0.3 mm (or so) away from it. Test: `store`/cli test that the clipboard text's origin is the footprint's position when the cursor is near it.
- **Unit-sensitive:** no.

### F16. Lone text box: Rotate turns about its corner; lone table: Rotate/Flip about its corner
- **Fixed** on task-fix-f-editing: 0c5e670a.
- **What differs.** `EDIT_TOOL::Rotate`: `if( selection.Size() == 1 && !m_dragging && dynamic_cast<PCB_TEXTBOX*>( selection.Front() ) ) selection.SetReferencePoint( ... ->GetCenter() );`; a lone table sets `usePcbShapeCenter` and `updateModificationPoint` excludes `PCB_TABLE_T` from the single-item rule. `EDIT_TOOL::Flip`: a lone table flips about the box centre (`if( !table && ( !rect || ... ) ) refPt = selection.GetReferencePoint();`). Ours: `rotationPivot` / `flipPivot` (`web/kicad-port/pcbTransform.ts:118-135`) special-case only shapes `rect`/`polygon`; a text box's position is its first corner (`web/kicad-port/pcbExtraItems.ts:102`), a table's its first cell's corner (`pcbItems.ts:133-136`). PARITY section 39 records "R turns it about its position" as if it were KiCad's.
- **How to see it.** Draw a text box, R: KiCad keeps its centre in place; ours swings it around its top-left corner. Test: `pcbTransform.test.ts` `a lone text box turns about its centre`.
- **Unit-sensitive:** no.

### F17. Duplicate skips barcodes and points (and text boxes, images: known), and copies a locked item as unlocked
- **Fixed** on task-fix-f-editing: 38f95a64 (barcodes and points in `Copies`, Create Array too; a locked item's copy is locked, a duplicate with nothing carriable is reverted). A lone tuning pattern and tables are not refused/skipped as KiCad's are.
- **What differs.** `EDIT_TOOL::Duplicate` duplicates `PCB_TEXTBOX_T`, `PCB_BARCODE_T`, `PCB_REFERENCE_IMAGE_T`, `PCB_POINT_T` like any item (`orig_item->Duplicate( true, &commit )`); `BOARD_ITEM::Duplicate` is a `Clone()` with a new UUID, so the lock is kept, and the following `doMoveSelection` then filters the locked copies (`FilterCollectorForLockedItems`); if nothing is left it returns false and the commit is reverted. A lone tuning pattern is refused (`if( selection.Size() == 1 && selection.CountType( PCB_GENERATOR_T ) ) return 0;`); tables are skipped (`case PCB_TABLE_T: // JEY TODO`).
  Ours: `collect_copies` / `copy_of` (`crates/ops/src/pcb_paste.rs:251-360`) know footprints, tracks, vias, zones, shapes, texts, dimensions, groups only; a barcode or a point in the selection is dropped (a lone one: "none of the given ids name ..."). `duplicateSelection` (`web/state/store.tsx:2414-2415`) says "the copy is not locked".
- **How to see it.** Select a barcode, Ctrl+D: KiCad carries a copy; we refuse. Test: `pcb_paste_tests.rs` `duplicate_copies_barcodes_and_points`.
- **Unit-sensitive:** no.

### F18. Double-click on a selected zone's edge adds a corner (KiCad opens Zone Properties); zone edges have no drag handle
- **Fixed in part** on task-fix-f-editing: 31169eb0 (a double click on a zone edge opens Zone Properties; corners are Create Corner, Insert). Not done: the zone's edge handles.
- **What differs.** KiCad's point editor has no double-click handling; `PCB_SELECTION_TOOL::Main`'s `IsDblClick` opens the item's properties. Corners are added with "Create Corner" (`pcbnew.PointEditor.addCorner`, hotkeys Ctrl+Insert / Insert in `pcb_actions.cpp` ~2318). A zone's edit points include an `EDIT_LINE` per edge (`ZONE_POINT_EDIT_BEHAVIOR` via `POLYGON_POINT_EDIT_BEHAVIOR::BuildForPolyOutline`), which drags the edge (Ctrl toggles converging / fixed length).
  Ours: `web/components/canvas/Canvas.tsx:1452-1468` adds a corner on a double-click on the edge, so the Zone Properties dialog cannot be opened by a double-click there; the zone drag (`:910-919`, `findNearestCorner`) has corners only — no edge handles, unlike our polygon shapes (`ringEditPoints` has the midpoints).
- **How to see it.** Select a zone, double-click its outline: KiCad shows the zone dialog; we add a vertex. Test: e2e `a double-click on a zone edge opens its properties`.
- **Unit-sensitive:** no.

---

## Low

### F19. Rectangle edge handles clamp to a minimum size; the minimum is 26 µm
- **Not a bug** (checked by the fix batch F): `PinEditedCorner` clamps the edges too at 8303b2ad (`aTopLeft.y = std::min( aTopLeft.y, aBotRight.y - minHeight )` for `RECT_TOP`, pcb_point_editor.cpp:503), and the minimum is 25 400 nm since the nanometre merge.
- `PinEditedCorner` clamps only the four corners to `Mils2IU( 1 )`; an edge line (`RECT_TOP` ... `RECT_RIGHT`) is set as is (`setTop( topLeft.y )`). Ours clamps edges too (`web/kicad-port/pcbPointEdit.ts:493-503`) and uses `MIN_RECT_SIDE_UM = 26` where KiCad's is 25.4 µm (`:25`). **Unit-sensitive: yes** — in nm it should be 25 400.

### F20. Rotation step: 0 and > 360 handled differently in the session
- `PANEL_EDIT_OPTIONS::TransferDataFromWindow` sets `cfg->m_RotationAngle = m_rotationAngle.GetAngleValue();` as typed (0 makes R do nothing, 450 turns 450 degrees); only the JSON load ignores 0 and reduces values over 360. Ours reduces and refuses 0 at once (`web/kicad-port/preferences.ts:67-72`) and rounds to tenths. Same visible turn except for 0. Unit-sensitive: no.

### F21. Reference/Value join the footprint box whatever the layer and Appearance rows
- `FOOTPRINT::GetBoundingBox( true )` adds Value and Reference only when visible **and** `board->IsLayerVisible( ... ) && IsElementVisible( LAYER_FP_VALUES/REFERENCES ) && IsElementVisible( LAYER_FP_TEXT )`. Ours tests `field.visible` only (`web/kicad-port/footprintBox.ts:52-56`), so with F.SilkS hidden the Rotate/Flip pivot of several footprints still counts their references. Unit-sensitive: no.

### F22. Doc drift found on the way
- **Fixed** on task-fix-f-editing: 12dc2e61 (GAPS 17, PARITY 6), 6b39b82b and f6e2506d (the two code comments); PARITY 6's aux origin note in 4b2a4b72.
- GAPS item 17 "Create Array skips groups (item 22)": `array.rs` uses `collect_copies`, which copies groups (PARITY section 17 says so too).
- PARITY section 6 "ROTATE_AROUND_AUX_ORIGIN ... no model equivalent" and `PcbParityDialogs.tsx` "no grid-origin setting": both exist now (F14).
- PARITY section 6, rotation-sign row: KiCad's `if( !m_DisplayInvertYAxis ) rotation = -rotation;` negates a variable it never uses afterwards (it rotates by `angle`, copied before); the net direction (counter-clockwise) matches ours, but the stated reason does not.
- `web/kicad-port/alignDistribute.ts:12` "this app's view never mirrors a PCB" (F12); `pcbEditActions.ts:322` "no free pads, so the middle tier never applies" (F13).

---

## Checked and found to match (do not re-review)
- `EDIT_TOOL::Remove`'s pad promotion: a selected pad is promoted to its footprint and waits for a second Delete; a hovered pad deletes the footprint at once (`!selectionCopy.IsHover()`), `deleteCmds.ts` / `useActionRunner.ts:326-341`.
- Delete of a footprint leaves every connected track and via (`rip` touches no routing; `clears_routing_in` false) — KiCad's `DeleteItems` removes only the footprint. (The `--strict` refusal is the known GAPS item 8 entry.)
- Delete of a group takes its members (`RunOnChildren` + `commit.Remove`), a table cell only clears its text, a tuning pattern is removed with its track put back; locked items are skipped; Delete is one undo step; the Interactive Delete tool deletes one item per click and says "Item locked.".
- Rotate: one item about `GetPosition()`, a lone rectangle/polygon about its centre, several about `BestSnapAnchor( GetCenter() )`; the step from Preferences; the angle negated in a flipped view (`IsFlippedX`); `rotateCcw` positive.
- Flip: about the box centre (not snapped), a lone item about its position except a rectangle; direction from `m_FlipDirection`; one undo step.
- Item positions (`GetPosition`) per kind: track start, arc/circle centre, polygon first vertex, zone first corner, group box centre, dimension start.
- Move Exactly: translation then rotation, selection centre measured before the move and moved with it, the default anchor by selection size, the polar angle's sign (`REL_Y_COORD`), one undo step.
- Align: locked items are targets, cursor-in-box target, extreme item otherwise, `GetBoundingBox( false )` for footprints, a pad aligns by its own box and moves its footprint; Distribute's gap and centre maths (`GetDeltasForDistributeByGaps/Points`), end caps fixed, locked items out; one undo step each.
- Paste Special: the three choices and their tooltips word for word, opening on "Keep existing reference designators" (`PASTE_MODE::KEEP_ANNOTATIONS`), "Clear net assignments" hidden for a lone footprint (`HideClearNets`), `REF**`; plain Paste keeps references; Duplicate/Paste and the drop are one step and Escape reverts (except Undo, F5).
- Point editor handle sets: segment (start, end), circle (centre, end), Bezier (start, c1, c2, end), arc (start, mid, end, centre), rectangle (4 corners, centre, 4 edges; no radius: known), polygon (vertices + edge midpoints); a locked shape has none.
- Box selection direction (window/crossing) and its swap in a flipped view (screen-based here, world-based plus `IsMirroredX` in KiCad: same result).
- Selection Filter kind mapping (fields, tables, cells, text boxes under Text; images under Graphics; points; barcodes and generators under Other items); "Locked items" outside "All items".
- `PCB_TEXT::Mirror` justification rule (only at exactly 0 / 90 degrees) in `pcbMirror.ts`.

---

## Summary

| # | Finding | Severity | Unit-sensitive |
|---|---|---|---|
| F1 | Position Relative polar angle (and Clear) mirrored vs `REL_Y_COORD` | high | no |
| F2 | Point edit / Mirror re-create shapes and tracks: item leaves its group, new id | high | no |
| F3 | Footprint box from the symmetric courtyard enclosure (Align, Distribute, Rotate/Flip pivot, hit test) | high | no |
| F4 | R during a carry turns about the pick-up pivot, not the grab point (medium part); F then R turns the wrong way (high part) | high | no |
| F5 | Undo during Move undoes the previous edit; during a carry leaves a Redo | medium | no |
| F6 | Move Individually: one step per drop, Escape keeps earlier drops | medium | no |
| F7 | Shift+Del: several undo steps, ignores locks, groups, tables, patterns | medium | no |
| F8 | Deleting a member dissolves a two-member group (KiCad keeps one member) | medium | no |
| F9 | Delete on a field does nothing (KiCad hides it, or says to use the dialog) | medium | no |
| F10 | Locks: locked zone corners draggable, Mirror moves a locked group, L skips groups/dimensions, no Override locks | medium | no |
| F11 | Selection Filter: no Rule Areas row, lone pad of a locked footprint, locked group pickable, no "Only ..." | medium | no |
| F12 | Align Left/Right not swapped in a flipped view | medium | no |
| F13 | Distribute and Position Relative promote pads to footprints | medium | no |
| F14 | Position Relative grid origin is (0,0); Move Exactly lacks drill/place origin, wrong menu, forgets values | medium | no |
| F15 | Copy's reference point is the grid point, not `BestDragOrigin` | medium | no |
| F16 | Lone text box / table turn (and table flip) about a corner | medium | no |
| F17 | Duplicate drops barcodes and points; locked copies unlocked | medium | no |
| F18 | Double-click on a zone edge adds a corner; no zone edge handles | medium | no |
| F19 | Rectangle edge handles clamped; 26 µm minimum | low | yes |
| F20 | Rotation step 0 / > 360 in session | low | no |
| F21 | Ref/Value in the footprint box regardless of layer visibility | low | no |
| F22 | Doc drift (GAPS 17, PARITY 6, two code comments) | low | no |

Counts: high 4, medium 14, low 4.
