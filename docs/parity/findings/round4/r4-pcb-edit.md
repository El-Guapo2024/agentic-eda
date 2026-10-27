# Findings round 4: PCB editing and its UI (`r4-pcb-edit`)

Reviewer `r4-pcb-edit`. This review only reports findings: no code was changed, nothing was built and no server was started.
- **Code read:** main's checkout `/home/team_braneboard_com/work/agentic-eda`, HEAD `01ffc984`.
- **KiCad compared against:** `8303b2ad`, and `25ec2335` where marked.
- **Scratch:** `/tmp/findings-r4-pcb-edit.2a3T`.
- **Paths:** ours are from the repo root; `web/` means `web/studio/src/`. KiCad's are from its source root.

Scope: GAPS items 8, 9, 10, 14-18, 21, 22, 29 and 35.
- **Priority (a): the round-3 merges** (PARITY-pcb sections 56 and 57). Part A covers Cleanup, table cells, zone-drawing Undo and Delete, Track & Via nets and Select Connection. Part B covers via rows, footprint rows, Mirror, microwave, Break Track, global text edits, Pad checks, Add Field, Swap Layers, flip and group locks.
- **Priority (b): rows no round has reviewed.** Part C covers Create Array, zone and polygon drawing, Global Deletions, Merge Zones and Teardrops. Part A also has two drawing-tool findings (A-1 to A-3) that came out of reading `drawShape` around the C-1 fix.

Finding ids are round-4 ids (`A-1`, ...), distinct from round 3's.

Reuse rule: the only active code found that duplicates what kicad-cli runs is the teardrop generator. Its rebuild duplicates `TEARDROP_MANAGER::UpdateTeardrops`, which kicad-cli's refill runs (see C-1; GAPS 521 already records this choice).

---

# Part A: the round-3 merges for Cleanup, table cells, zone-drawing undo, Track & Via nets and Select Connection

### A-1 (medium). The board's graphic Line tool draws one segment and stops; KiCad chains segments. Delete and Backspace after a committed segment do not take it back
- **Status:** **Fixed** on task-r4-pcb-edit: 6398b588 (the chain is the chain's, not the tool session's; PARITY-pcb section 58)
- **What differs:**
  - **Ours.** `web/studio/src/components/canvas/Canvas.tsx:119-124`: `shapeAutoFinishCount("segment") == 2`. At `Canvas.tsx:889-893`, the second click sends `add_shape` and clears `drawState`. So each line needs two fresh clicks.
  - **Backspace.** `pcbnew.InteractiveDrawing.deleteLastPoint` (`web/studio/src/actions/useActionRunner.ts:1957-1975`) can only drop the started point. No committed segment is ever taken back.
  - **KiCad, `DRAWING_TOOL::DrawLine`** (`pcbnew/tools/drawing_tool.cpp:386-398`) loops:
    ```cpp
    while( drawShape( aEvent, &line, startingPoint, &committedLines ) )
    {
        if( line )
        {
            commit.Add( line );
            commit.Push( _( "Draw Line" ) );
            startingPoint = VECTOR2D( line->GetEnd() );
            committedLines.push( line );
        }
    ```
    So the next segment starts at the end of the last one.
  - **KiCad, `drawShape`** (`drawing_tool.cpp:2662-2688`): with a segment started, Delete or Backspace takes the last committed segment back off the board. It pops the undo list and resumes from that segment's start:
    ```cpp
    else if( started && (   evt->IsAction( &PCB_ACTIONS::doDelete )
                         || evt->IsAction( &PCB_ACTIONS::deleteLastPoint ) ) )
    {
        if( aCommittedGraphics && !aCommittedGraphics->empty() )
        {
            twoPointMgr.SetOrigin( aCommittedGraphics->top()->GetStart() );
            twoPointMgr.SetEnd( aCommittedGraphics->top()->GetEnd() );
            aCommittedGraphics->pop();
            getViewControls()->WarpMouseCursor( twoPointMgr.GetEnd(), true );
            if( PICKED_ITEMS_LIST* undo = m_frame->PopCommandFromUndoList() )
            { m_frame->PutDataInPreviousState( undo ); ... }
        }
        else
        {
            cleanup();
            break;
        }
    ```
- **How to see it:** pick Draw Line on F.SilkS and click at (0,0), (10,0) and (10,10).
  - KiCad: two joined segments, with a third rubber-banding from (10,10). Backspace removes the (10,0)-(10,10) segment, leaving the second segment rubber-banding from (10,0).
  - Ours: one segment (0,0)-(10,0), and a new line started at (10,10).
  - Test: `web/studio/e2e`, `pcb line tool chains segments`. Three clicks give two `gr_line`s sharing (10,0). Backspace then leaves one `gr_line` and a draw state starting at (10,0).
- **Severity:** medium (the most-used drawing tool behaves differently; an undo-step difference).
- **Already known?** No. PARITY-pcb rows 128 and 2172 cover Escape and Undo only. The round-2 C6 and round-3 checks read `drawShape`'s Escape and Undo branches, not this one.

### A-2 (medium). Delete with a graphic shape started deletes the selection; KiCad cancels the shape, and the selection was cleared at the first click anyway
- **Status:** **Fixed** on task-r4-pcb-edit: 6398b588
- **What differs:**
  - **Ours.** `useActionRunner.ts:358-375` (`common.Interactive.delete`) intercepts only `drawZoneStarted` (zones and polygons) and the schematic rule area. With a line, rectangle, circle, arc or Bezier started, it falls through to `requestSelection()` and `deleteRefs`.
  - **Our first click.** The shape tool's first click (`Canvas.tsx:878-897`) does not clear the selection. So whatever was selected before the tool was picked is deleted.
  - **KiCad, first click.** `drawShape` clears the selection at its first click (`drawing_tool.cpp:2554-2556`: `if( !started ) { m_toolMgr->RunAction( ACTIONS::selectionClear ); ...`).
  - **KiCad, Delete.** Delete with the shape started is consumed by the branch quoted in A-1 (`cleanup()` drops the started shape). `drawArc` and `drawOneBezier` have their own `doDelete`/`deleteLastPoint` branches.
- **How to see it:** select footprint U1, pick Draw Rectangle, click once, press Delete.
  - KiCad: the rectangle in progress goes and U1 stays.
  - Ours: U1 is deleted and the rectangle keeps rubber-banding.
  - Unit test (`drawingCancel.test.ts`): `deleteWhileDrawing("pcb", "rect", { kind: "shape", shapeKind: "rect", pts: [[0,0]] })` should report "consumed by the tool", as `drawZoneStarted` does for zones. The fix also needs `SET_SELECTION []` on a shape tool's first click.
- **Severity:** medium (deletes items the user did not mean to; undoable).
- **Already known?** No.

### A-3 (medium). Redo, and the zone-fill actions, while an item is being drawn act on the board; KiCad refuses them (`wxBell()`)
- **Status:** **Fixed** on task-r4-pcb-edit: 6398b588 (refused silently: no bell)
- **What differs:**
  - **Ours.** `common.Interactive.redo` (`useActionRunner.ts:657-660`) checks only `boardMoving()`. With a zone outline, line, dimension, table or image in progress, Redo redoes the board under the preview.
  - **Fill All Zones (B).** It runs too.
  - **KiCad.** Every drawing loop refuses both while started (`drawShape` `drawing_tool.cpp:2713-2717`, `DrawZone` `:3629-3633`, and also `:855`, `:1158`, `:1396`, `:2017`, `:3031`, `:3337`):
    ```cpp
    else if( started && (   ZONE_FILLER_TOOL::IsZoneFillAction( evt )
                         || evt->IsAction( &ACTIONS::redo ) ) )
    {
        wxBell();
    }
    ```
- **How to see it:** add a track and Undo it. Then pick the zone tool, place two corners and press Ctrl+Y.
  - KiCad: a bell, and the track stays gone.
  - Ours: the track comes back while the outline is open.
  - Test: `undoDropsDrawing`'s sibling `redoRefused(tab, tool, drawState)` should be true for every `escapeKeepsDrawingTool` state, and the redo handler should return early on it.
- **Severity:** medium (undo-stack behaviour).
- **Already known?** No (C6 and C-1 covered Escape, Undo and Delete only).

### A-4 (low). Delete or Undo of a zone corner: the cursor is not put on the removed corner, and the last corner's removal ends the outline one press early
- **Status:** **Fixed** on task-r4-pcb-edit: 6398b588 (the last corner keeps the outline started); the cursor warp is not done (a browser cannot move the pointer)
- **What differs:**
  - **KiCad, cursor.** `DrawZone` (`drawing_tool.cpp:3608-3622`) warps the cursor to the corner it removed and holds it there:
    ```cpp
    if( std::optional<VECTOR2I> last = polyGeomMgr.DeleteLastCorner() )
    {
        cursorPos = last.value();
        getViewControls()->WarpMouseCursor( cursorPos, true );
        m_controls->ForceCursorPosition( true, cursorPos );
        polyGeomMgr.SetCursorPosition( cursorPos );
    }
    else
    {
        cleanup();
    }
    ```
  - **KiCad, last corner.** `POLYGON_GEOM_MANAGER::DeleteLastCorner` (`common/preview_items/polygon_geom_manager.cpp:133-150`) returns the point even when it removes the only corner. So the first press empties the outline but keeps the tool `started`, and only the next press reaches `cleanup()`.
  - **Ours.** `withoutLastCorner` (`web/studio/src/kicad-port/drawingCancel.ts`, at the end) returns `null` at one corner and moves no cursor.
- **How to see it:** place three zone corners and press Delete. KiCad's preview leg now runs from corner 2 to corner 3's spot, under the cursor; ours runs to wherever the mouse is.
- **Severity:** low.
- **Already known?** No.

### A-5 (low). Cleanup's merge refuses two collinear segments that meet inside a zone, overlap, or fold back; KiCad merges them
- **Status:** **Fixed** in part on task-r4-pcb-edit: 45dcab1a (the zone: `ZONE::HitTest` of the track's net); overlap, fold-back and the 1 nm tolerance left
- **What differs:**
  - **Zones.** `is_node` (`crates/connectivity/src/cleanup.rs:564-575`) takes any zone whose raw outline holds the joint as a node, so the merge is refused. The module doc (`cleanup.rs:40-44`) calls this "the safe direction". KiCad's `testMergeCollinearSegments` (`pcbnew/tracks_cleaner.cpp`) tests a zone with `citem->HitTest( aSeg1->GetStart(), ( aSeg1->GetWidth() + 1 ) / 2 )`. That is `ZONE::HitTest` (`pcbnew/zone.cpp:734-741`), which only hits the outline's corners and edges:
    ```cpp
    int accuracy = std::max( aAccuracy, pcbIUScale.mmToIU( 0.1 ) );
    return HitTestForCorner( aPosition, accuracy * 2 ) || HitTestForEdge( aPosition, accuracy );
    ```
    So two segments of a GND trace meeting in the middle of a GND pour merge in KiCad and stay two here.
  - **Overlap and fold-back.** `shared_endpoint` (`cleanup.rs:542-559`) needs an exact shared end, and the fold-back test (`cleanup.rs:599-606`) refuses a segment doubling back. KiCad's candidates are any connected `PCB_TRACE_T` that is `ApproxCollinear` (`SEG::ApproxCollinear`, 1 IU). The merged segment is the bounding span of the four ends. So collinear overlapping segments (A (0,0)-(2,0), B (1,0)-(3,0)) become (0,0)-(3,0). A fold-back (A (0,0)-(2,0), B (2,0)-(1,0)) becomes (0,0)-(2,0).
  - **Tolerance.** The exact cross product (`collinear`, `cleanup.rs:535-538`) is stricter than `ApproxCollinear`'s 1 nm. Off-axis segments whose rounded points are 1 nm off line do not merge.
- **How to see it:** test `merge_collinear_inside_a_zone`. Put an N1 zone over the whole board and add two N1 segments (0,0)-(5,0) and (5,0)-(10,0) with nothing else at (5,0). With `merge_segments`, expect one `MergedTracks` change; ours gives none.
- **Severity:** low (a missed merge; connectivity is unchanged).
- **Already known?** The zone outline is in the module doc as a deliberate choice, but its reason ("is the shared joint a node") does not match KiCad's code. The overlap, fold-back and 1 nm cases are not documented.

### A-6 (low). Track & Via Properties' short question: the net names differ from KiCad's for no net and for an indeterminate selector, and an OK with no edits asks nothing
- **Status:** **Fixed** in part on task-r4-pcb-edit: ca34d02e (the names); an OK with nothing edited still asks nothing
- **What differs:**
  - **KiCad's message.** `confirmShortingNets` (`pcbnew/dialogs/dialog_track_via_properties.cpp:908-929`) formats `"Applying these changes will short net %s with %s."` from `m_netSelector->GetValue()` and `FindNet( *shortingNets.begin() )->GetNetname()`. For a short to a no-net pad that second string is net 0's name, `""`, giving "... with .". For an indeterminate selector, `GetValue()` is the indeterminate string `INDETERMINATE_STATE`, "-- mixed values --" (`include/widgets/ui_common.h:46`, set at `dialog_track_via_properties.cpp:747`).
  - **Ours.** `crates/ops/src/pad_nets.rs` (`track_via_nets`, the `shown` and `selector` closures) prints `<no net>` for the shorting net and `""` for an indeterminate selector ("short net  with GND.").
  - **No edits.** KiCad runs the short check on every OK (`TransferDataFromWindow` reaches `shortingNets` whatever changed). Our `trackViaCmds` (`web/studio/src/kicad-port/trackViaDialog.ts`) sends `track_via_nets` only `if (cmds.length > 0 || changed("net"))`. So OK on a track that already overlaps another net asks in KiCad and is silent here.
- **How to see it:** in `pad_nets_tests.rs`, `a_short_to_a_no_net_pad_names_the_empty_net` widens an N1 track over a no-net pad. The hint should be "Applying these changes will short net N1 with ."
- **Severity:** low (message).
- **Already known?** No. C-2 was fixed and its known difference is the zone flood, not this.

### A-7 (low). Select Connection with a tuning pattern selected discards it and picks under the cursor; KiCad expands from the pattern
- **Status:** **Fixed** on task-r4-pcb-edit: 357b9cea
- **What differs:**
  - **KiCad.** `PCB_SELECTION_TOOL::expandConnection` (`pcbnew/tools/pcb_selection_tool.cpp:1808-1816`) counts a generator in `initialCount`:
    ```cpp
    if(   item->Type() == PCB_FOOTPRINT_T
       || item->Type() == PCB_GENERATOR_T
       || ( static_cast<const BOARD_ITEM*>( item )->IsConnected() ) )
        initialCount++;
    ```
    So `selectCursor` is not called, and the stages start from the generator's board items (`:1856-1862`).
  - **Ours.** `STARTS` in `web/studio/src/kicad-port/pcbConnection.ts` (`part`, `track`, `via`, `zone`, `pad`) leaves out `group`, which a tuning pattern is (`itemKind`). `connectedStartCount` is then 0, and `planSelectConnection` replaces the selection with the item under the cursor, or with nothing. `expandedSelection`'s `startsOf` would have handled the generator.
- **How to see it:** in `pcbConnection.test.ts`, select only a tuning pattern `tp1` with nothing under the cursor. `planSelectConnection(board, {"tp1"}, [])` should return the pattern plus its connected tracks; ours returns `{ refs: [] }`.
- **Severity:** low.
- **Already known?** No.

### Part A: checked and found to match (do not re-review)
- **`TRACKS_CLEANER::CleanupBoard`** (round 3 D-1..D-5, 19e8c01f). Checked:
  - the call order;
  - `removeNullSegments = aMergeSegments || aRemoveMisConnected`;
  - duplicates always run;
  - the final merge only after a deletion.
- **Redundant vias.** Checked:
  - `QueryColliding` against the vias not yet `SKIP_STRUCT`, so the earlier of two goes;
  - a locked via, never flagged, still counts as a twin;
  - position, `GetViaType` and `GetLayerSet` compared;
  - `GetConnectedPads( via )` with `( pad->GetLayerSet() & all_cu ) == all_cu`, any net.
- **Duplicate segments.** `PCB_TRACE_T` only, non-null, `IsPointOnEnds` both ways, width and layer, the earlier goes.
- **`removeShortingTrackSegments`.** Checked:
  - pads and tracks of another net, net 0 included;
  - zones not looked at;
  - a via as `CLEANUP_SHORTING_VIA`;
  - only the item at fault goes.
- **`deleteTracksInPads`.** Both ends in the pad, then the width-inflated track against the pad's copper. Only the oval approximation is documented.
- **`deleteDanglingTracks`.** Checked:
  - iterated to a fixed point;
  - `TestTrackEndpointDangling( track, false )`;
  - KiCad skips `IS_DELETED` items in the test, so dry run and apply agree.
- **`TestTrackEndpointDangling` port** (`crates/connectivity/src/dangling.rs`). Checked:
  - `accuracy = KiROUND( width / 2.0 )`;
  - both ends in a zone means not dangling;
  - `getMinDist` for a pad or via (its position) and for a track (the nearer end), truncated;
  - the via branch: a no-net via with no connections is not dangling, one layer means dangling.
- **`explode` / `finish`.** Only the faulty segment goes; the runs that stay come back. Locked and filtered polylines are not cut.
- **Table cells (291dfd71).**
  - Cut and Delete of a cell: `DeleteItems`' `PCB_TABLECELL_T` "Clear contents of table cell". Delete's `RequestSelection` in `EDIT_TOOL::Remove` has no `FilterCollectorForTableCells`, and ours sends `edit_cells` text "" for both.
  - `SaveSelection`'s `promotedTables`, and `deleteUnselectedCells` for a block selection: row heights and column widths from the min row and column, `SetColCount`, and the origin at the first kept cell's corner via `Normalize`.
- **Zone drawing (5f553a9d).** Delete, Backspace and Undo with a corner placed remove the last corner. Not started, they pass to the editor. Undo in the measure tool undoes the board (it handles only `IsCancelInteractive`). The zone tool pops after a finished outline (`setActiveTool select`, matching `PopTool`).
- **Track & Via nets (3d208446).** Checked:
  - the order: the other edits first, then the shorts question, then the pads question;
  - `other->GetNetCode() == track->GetNetCode() || == newNetCode` skipped;
  - pads of `newNetCode` skipped;
  - a pad of the track's own net goes to `changingPads`, any other net to `shortingNets`;
  - the 1, 2 and n pad messages word for word;
  - the button labels;
  - Cancel Changes reverts everything; Leave Nets Unchanged keeps every net;
  - only the selected tracks and the changing pads get the new net;
  - the shape-collision graph here (`geom::touches`) collides whole shapes, as the dialog's `collide()` does, not only anchors.
- **Select Connection (586d125c).**
  - `selectAllConnectedTracks` line by line: `trackMap` by both ends; the exact via hit, then the off-centre one in `std::less<VECTOR2I>` order (x then y); `gotPad` and `gotNonStartPad`; `pt_count` over non-degenerate tracks on the point's copper; STOP_AT_JUNCTION and STOP_AT_PAD; the via pushing every track point inside it with the via's layers; `SKIP_STRUCT` across start items.
  - `expandConnection`'s stages: they stop when the selection outgrows `initialCount`, which is counted before the cursor pick. A-7 is the exception.

---

# Part B: section 56 via rows, footprint panel rows, Mirror; section 57 microwave, Break Track, Edit Text & Graphics, Pad checks, Add Field, Swap Layers, flip mirror, group lock

---

### B-1 (medium). Swap Layers and a tuning pattern: we take the pattern's stored layer as its own, KiCad takes its first track's layer (`GetLayer` override). A two-way swap therefore gives a different result, and leaves our pattern on one layer with its tracks on another
- **Status:** **Fixed** on task-r4-pcb-edit: d977e10a (tests `swap_layers_both_ways_puts_a_tuning_patterns_tracks_back_as_set_layer_does`, `swap_layers_chained_moves_a_pattern_twice`)
- **What differs.** `crates/ops/src/lib.rs:4368-4404` (`swap_generator_layers`) builds the set as `{p.layer} ∪ member layers`. `p.layer` is the stored layer, which is still the old one. KiCad's generator set is `PCB_GENERATOR::GetLayerSet()` = `PCB_GROUP::GetLayerSet() | LSET( { GetLayer() } )`. `PCB_TUNING_PATTERN` overrides `GetLayer`:
  ```cpp
  // pcbnew/generators/pcb_tuning_pattern.h
  PCB_LAYER_ID GetLayer() const override
  {
      for( BOARD_ITEM* item : GetBoardItems() )
      {
          if( PCB_TRACK* track = dynamic_cast<PCB_TRACK*>( item ) )
              return track->GetLayer();
      }
      return PCB_GENERATOR::GetLayer();
  }
  void SetLayer( PCB_LAYER_ID aLayer ) override
  {
      PCB_GENERATOR::SetLayer( aLayer );
      for( BOARD_ITEM* item : GetBoardItems() )
          item->SetLayer( aLayer );
  }
  ```
  In `GLOBAL_EDIT_TOOL::SwapLayers` (`pcbnew/tools/global_edit_tool.cpp:149-176`) the tracks are swapped first. Then `swapBoardItem( generator, layerMap )` runs. It sees only the tracks' *new* layer, and with one layer it calls `SetLayerSet` → `SetLayer`, which **moves the member tracks too**.
  - F.Cu ↔ B.Cu, pattern on F.Cu:
    - KiCad: the tracks go to B.Cu. The generator's set is {B.Cu}, mapped to {F.Cu}, so `SetLayer( F_Cu )` puts the tracks back on F.Cu. Result: pattern and tracks both on F.Cu.
    - Ours: the set is {F.Cu, B.Cu}, which maps onto itself, so nothing changes. The tracks stay on **B.Cu** while the pattern says **F.Cu**.
    - `crates/ops/src/tuning_tests.rs:171-177` (`swap_layers_both_ways_leaves_a_tuning_pattern_where_it_was`) asserts this mixed state.
  - Chained map F→In1, In1→In2:
    - KiCad: the tracks go to In1. The generator's set is {In1}, mapped to {In2}, so the tracks end on In2.
    - Ours: {F, In1} maps to {In1, In2}, two layers, so nothing changes. The tracks stay on In1 and the pattern stays on F.
  - KiCad's writer writes `(generated .. (layer %s))` from `GetLayer()` (`pcb_io_kicad_sexpr.cpp`, `format( PCB_GENERATOR )`), so KiCad never writes a pattern on a layer other than its tracks'. We write `p.layer`.
  - The PARITY-pcb §57 row (line 2226), "F.Cu with B.Cu leaves it (KiCad's quirk)", misdescribes KiCad.
- **How to see it.** On a 4-layer board, take a tuning pattern on F.Cu and run Swap Layers F.Cu↔B.Cu.
  - Test `swap_layers_both_ways_puts_a_tuning_patterns_tracks_back_as_set_layer_does`: assert that every member track and `tuning_patterns[0].layer` are on `F.Cu`.
  - Test `swap_layers_chained_moves_a_pattern_twice`: map F→In1 and In1→In2; assert the tracks and the pattern are on `In2`.
  - Fix: use the first member track's post-swap layer as "own" (fall back to `p.layer` when there is no track). When the result is a single layer, set the pattern's layer *and* its member tracks.
- **Severity:** medium. The state is inconsistent, and the result differs from KiCad's.
- **Already known?** No. r3 A-2 is marked Fixed, but the fix is wrong for the two-way and chained cases.

### B-2 (medium). Mirror, Rotate (and Create Array's origin) of one selected tuning pattern turn about its bounding-box centre; KiCad uses `PCB_GENERATOR::GetPosition()` = `m_origin`
- **Status:** **Fixed** on task-r4-pcb-edit: 1f93df15
- **What differs.**
  - `web/studio/src/kicad-port/pcbItems.ts:127-131` (`itemPosition`, `case "group"`) returns the centre of the box for every group, generators included.
  - `EDIT_TOOL::updateModificationPoint` (`pcbnew/tools/edit_tool.cpp:3563-3569`) takes "the reference point is its position" for a single item: `aSelection.SetReferencePoint( item->GetPosition() )`.
  - `pcbnew/pcb_generator.h:77`: `VECTOR2I GetPosition() const override { return m_origin; }`.
  - So Mirror Horizontally of a lone tuning pattern mirrors about the pattern's origin, its first base-line point, in KiCad. Ours mirrors about the middle of the meander's box, and the pattern lands somewhere else.
  - The same `itemPosition` feeds `modificationPoint` (`pcbTransform.ts:106`, Rotate), `mirrorReferencePoint` (`pcbMirror.ts:72`), `CreateArrayDialog.tsx:72,78` and `PcbParityDialogs.tsx:194`.
- **How to see it.** Select one tuning pattern whose origin is at (0,0) and whose meander runs to x = 10 mm. Mirror it horizontally.
  - KiCad: the base line goes from (0,0) to (-10,0).
  - Ours: it goes from (10,0) to (0,0).
  - Unit test in `pcbMirror.test.ts`: `mirrorReferencePoint(board, [patternGroupId])` equals the pattern's `origin` (from `drawings.tuning_patterns`, matched by id).
- **Severity:** medium (a wrong result of an edit).
- **Already known?** No.

### B-3 (medium). Edit Text & Graphics "Set to layer default values": the text is made square at the class's height, and italic is not reset; `StyleFromSettings` sets the class's (width, height) and italic
- **Status:** **Fixed** on task-r4-pcb-edit: 2a6681ec (texts, text boxes, cells; a dimension has one size and no italic in this model)
- **What differs.** `crates/ops/src/pcb_global_edit.rs`:
  - texts: `:107` sets `size_nm` to the height; `:125` drops the width (`x.width_nm = None`);
  - text boxes: `:136-137`;
  - cells: `:166`, `cell.style.width_nm = cell.style.height_nm`;
  - dimensions: `:195`, one size.
  - `text_size()` (`:33-35`) deliberately returns only `.1`. Yet `TextGraphicsDefaults::text_size_nm` returns `(width, height)` (`crates/model/src/rules.rs:409-411`), and Board Setup's rows have separate width and height cells.

  KiCad:
  ```cpp
  // pcbnew/pcb_text.cpp:350 PCB_TEXT::StyleFromSettings (PCB_TEXTBOX::StyleFromSettings, pcb_textbox.cpp:172, the same after PCB_SHAPE's)
  SetTextSize( settings.GetTextSize( GetLayer() ) );
  SetTextThickness( settings.GetTextThickness( GetLayer() ) );
  SetItalic( settings.GetTextItalic( GetLayer() ) );
  ```
  `GetTextSize` is a `VECTOR2I` (width, height). Italic now has a place to land: `TextExtra::italic` (`crates/model/src/board_text.rs:42`) and `TextBox::italic` (`crates/model/src/pcb_items.rs`). `GetTextItalic` is `text_italic_upright` (`rules.rs:419`).
- **How to see it.** In Board Setup, set Silk text to 1.2 mm wide, 1.5 mm high, italic. Put a non-italic 1×1 mm text and a text box on F.Silkscreen, then run Set to layer default values.
  - KiCad: 1.2×1.5, italic.
  - Ours: 1.5×1.5, not italic.
  - Test in `pcb_global_edit_tests.rs`, `layer_defaults_take_the_classs_width_height_and_italic`: assert the text extra's `width_nm == Some(1.2 mm)`, `italic == true`, and the same on the text box.
- **Severity:** medium (wrong default).
- **Already known?** In part. PARITY-pcb line 676 lists bold, italic and font as not ported "this model has no font or style flag on `Text`". That reason is stale: the extras carry italic now. The width/height loss is not listed anywhere.

### B-4 (low). Pad Properties checks for a custom pad and for a pad with no copper layer
- **Status:** **Fixed** in part on task-r4-pcb-edit: 41e138cf (custom pads); the no-copper-layer case does not arise (the dialog's type picks the layers)
- **What differs.** `web/studio/src/kicad-port/fpFields.ts:262-293` (`padCheck`) uses `pad.size` for every shape and skips the mask check for `custom`.

  `PAD::doCheckPad` (`pcbnew/pad.cpp:2962-2965, 2970, 3034-3048`):
  ```cpp
  if( GetShape( aLayer ) == PAD_SHAPE::CUSTOM )
      pad_size = GetBoundingBox().GetSize();
  ...
  if( IsOnCopperLayer() && GetDrillSize().x > 0 )   // the hole checks
  ...
  if( GetShape( aLayer ) == PAD_SHAPE::CUSTOM )
  {   for( const std::shared_ptr<PCB_SHAPE>& shape : GetPrimitives( aLayer ) )
      {   BOX2I shapeBBox = shape->GetBoundingBox();
          if( absMargin > shapeBBox.GetWidth() || absMargin > shapeBBox.GetHeight() )
          {   aErrorHandler( DRCE_PADSTACK, _( "(negative solder mask clearance is larger "
                                             "than some shape primitives; results may be "
                                             "surprising)" ) );
              break; } } }
  ```
  - A custom pad with a negative mask margin larger than one of its primitives gets no warning from us.
  - The paste check uses the anchor size instead of the bounding box.
  - The hole checks run on a pad with no copper layer (an NPTH on `*.Mask` only), where KiCad skips them because `IsOnCopperLayer()` is false. That gives a spurious "(pad hole not inside pad shape)" when the NPTH's size is smaller than its offset.
- **How to see it.** Take a custom pad: a 0.5 mm anchor plus a 0.2×0.2 mm rectangle primitive. Set the mask margin to −0.3 mm.
  - Test `padCheck({shape:"custom", maskMargin:-300_000, primitives:[..]})`: expect the "some shape primitives" warning.
- **Severity:** low.
- **Already known?** No (r3 C-4 covered the plain shapes).

### B-5 (low). The writer marks only the group locked; KiCad writes `(locked yes)` on every item whose group above is locked
- **Status:** **Fixed** on task-r4-pcb-edit: 3853fa1d
- **What differs.**
  - `crates/kicad/src/pcb_items.rs:381-431`: since 8b3a2a07 the group's lock takes the group above into account, but only for groups.
  - Every `format()` in `pcb_io_kicad_sexpr.cpp` writes `IsLocked()` (tracks :2882, vias :2763, shapes :1092, footprints :1209, texts :2332, text boxes :2374, zones :2917, ...). `BOARD_ITEM::IsLocked` (`pcbnew/board_item.cpp:125-137`) is true when `GetParentGroup()->IsLocked()`.
  - So KiCad saves each member of a locked group as `(locked yes)`; we don't. On reading, both behave the same while grouped. After Ungroup, a KiCad-saved board keeps the members locked and ours frees them.
- **How to see it.** Lock a group containing a track and a footprint, then save.
  - Test: the written `(segment ..)` and `(footprint ..)` contain `(locked yes)`.
- **Severity:** low (a textual difference; the same reading while grouped).
- **Already known?** No.

---

### Checked and found to match (do not re-review)
- **Track & Via Properties' via rows (`crates/ops/src/via_layers.rs`, `web/kicad-port/trackViaDialog.ts`):**
  - `TransferDataFromWindow`'s order: `SetViaType`, `SetTopLayer` / `SetBottomLayer`, `SanitizeLayers` (through → F_Cu..B_Cu; a back-to-front pair swapped by `IsCopperLayerLowerThan`), `SetUnconnectedLayerMode`.
  - The four ring choices and their order (`m_annularRingsCtrlChoices`).
  - `ValidateViaParameters`' "Via layer must be a copper layer.", "Via layer is outside the board stack." and "Via start layer and end layer cannot be the same", word for word.
  - `onViaEdit` sets F.Cu/B.Cu and disables both layer choices for Through.
  - The rings row shows on `getLayerDepth() > 1`. Ours counts in stack positions, as 25ec2335's fixed `BOARD::LayerDepth` (`LAYER_RANGE(..).size() - 1`). 8303b2ad's `aEndLayer - aStartLayer` on the new IDs (B_Cu=2, In1=4) is a KiCad bug, rightly not copied.
  - The indeterminate rings row is left alone. That matches 25ec2335. 8303b2ad selects index 3 (`START_END_ONLY`) for mixed vias, a bug fixed in 25ec2335.
  - Deliberately not copied: KiCad's `SetPadstack( *m_viaStack )` copies the *first* via's padstack to every via whose layers change. Our refusal when a via's pair collapses (KiCad's `SetTopLayer` silently returns, "refuse invalid via") is stricter and harmless.
- **`FOOTPRINT_DESC` rows:**
  - Names, groups and order: Reference (Fields); Library Link, Library Description, Keywords, Component Class (Footprint Properties); Not in Schematic, Exclude From Position Files, Exclude From Bill of Materials, Do not Populate (Attributes); Exempt From Courtyard Requirement, Clearance Override, Solderpaste Margin Override, Solderpaste Margin Ratio Override, Zone Connection Style (Overrides).
  - `ENUM_MAP<ZONE_CONNECTION>` labels.
  - Component Class = `GetFullClassNameForConstituents` (`sortClassNames`, `,`-joined).
  - The documented differences (Reference read-only, Component Type row, paste rows read-only, assignment-rule classes) are as PARITY says. Track width 0 is taken: `PGPROPERTY_SIZE` is a `wxUIntProperty`.
- **Mirror:**
  - `PCB_TEXTBOX::Mirror` (`PCB_SHAPE::Mirror`, then `ANGLE_180 - a` / `-a`).
  - `PCB_TUNING_PATTERN::baseMirror`: base line and coupled base line mirrored; origin and end from the base line; RIGHT→LEFT, otherwise RIGHT.
  - `MirrorableItems` against the action's filter.
  - KiCad mirrors `m_origin` twice when a pattern has no base line, so it stays put; ours mirrors it once. Every pattern read from a file has a base line, so this is not reported.
- **Microwave (d7082fdb):**
  - `ADD_MODE::INSERT` is `m_pads.push_front` (`footprint.cpp:1429-1433`), so `Pads().begin()` is pad 2.
  - Gap: pad 2 on the left.
  - Stub: the first pad renamed "1", the second sized `(tw, gap_size)` at `-(gap+tw)/2`.
  - Polygonal Shape: pad 2 at `-size/2`, pad 1 at `+size/2`.
- **Break Track on a locked track (04a71b87):** "The selected item is locked." / "Confirmation" / OK "Break Track"; the selection cleared first; the halves keep the lock (`lock_tracks` handled in `commit_route`). Two small differences, not reported: KiCad asks before trying the break (ours asks after the break point is found), and the do-not-show box is known.
- **Edit Text & Graphics, specified values (ed91d108):**
  - Text boxes as text and shape.
  - Barcode `SetTextSize( height )`, with no line width (not a `PCB_SHAPE`).
  - `PCB_BARCODE::StyleFromSettings` = `GetTextSize( layer ).y`.
  - `PCB_SHAPE::StyleFromSettings` = `GetLineThickness`.
  - Dimension defaults.
- **Pad Properties (8aefbd9e), plain shapes:**
  - "(negative local clearance values have no effect)".
  - "(negative solder mask clearance is larger than pad; no solder mask will be generated)".
  - "(negative solder paste margin is larger than pad; no solder paste mask will be generated)", with `KiROUND( pad_size * mratio )` and the ratio as a fraction.
  - "(pad hole not inside pad shape)" at the pad's position against the copper at its offset.
  - The min-drill text via `StringFromValue`.
  - Corner, chamfer and trapezoid limits are clamped by `padShape.ts`.
- **Add Field (eb2172ab):** `StyleFromSettings( .., true )` on F.Fab/B.Fab: size (w,h), thickness, italic, keep-upright (the field has a parent footprint), mirrored on B.Fab.
- **Flip mirror on typed user layers (904bc77e):** `IsSideSpecific()` evaluated after `SetLayer( FlipLayer(..) )` (`pcb_text.cpp:473-490`, `pcb_textbox.cpp:574-585`); ours tests the new layer's type.
- **Group lock (8b3a2a07):** `BOARD_ITEM::IsLocked`'s parent-group chain in Group Items, Group Properties (shown) and the group's own lock on OK.

---

# Part C: ported or identical rows no round reviewed (Create Array, zone and polygon drawing, Global Deletions, Merge Zones, Teardrops)

Rows rounds 1-3 covered were not re-read: p6, r2, r3, p13, P24 and P25.


---

### C-1 (high). Teardrops are measured from the pad or via centre, not from its edge: with KiCad's default ratio the teardrop ends on the via's rim and adds almost no copper
- **Status:** **Fixed** on task-r4-pcb-edit: a6ea19db (and the `>=` width filters); kicad-cli's own teardrops equal ours on 8 vias (`crates/kicad/tests/teardrops_kicad_cli.rs`)
- **What differs.** `crates/connectivity/src/teardrop.rs:197-199` puts the track-side corners A and B at `near + dir * target_len`, where `near` is the anchor's centre (`:157`, `near != center` returns None) and `target_len = anchor_diameter * best_length_ratio` (`:121`). KiCad measures the length from where the track leaves the pad/via outline: `pcbnew/teardrop/teardrop_utils.cpp::TEARDROP_MANAGER::findAnchorPointsOnTrack`:
  ```cpp
  int targetLength = KiROUND( GetWidth( aOther, layer ) * aParams.m_BestLengthRatio );
  ...
  pt_count = outline.Intersect( SEG( start, end ), pts );
  ...
  aIntersection = pts[0].p;
  start = aIntersection;      // This is currently the reference point of the teardrop length
  actualTdLen = std::min( targetLength, SEG( start, end ).Length() );
  ```
  With the defaults (`m_BestLengthRatio( 0.5)`, `teardrop_parameters.h:53`, which ours copies) `target_len` equals the radius, so our A and B land on the via's circle: the pentagon is the via plus two slivers the track's width already covers. KiCad's tip is `r + 0.5 * d` from the centre (0.6 mm for a 0.6 mm via).
- **How to see it.** Board with a 0.6 mm via at (0,0), a 0.2 mm track (0,0)-(5 mm,0) on its net, Teardrops on with the defaults, Add All Teardrops. Test `teardrop::tests::the_teardrop_length_starts_at_the_anchor_edge`: `generate_teardrops(..)` gives one zone; assert its largest x is 600_000 nm (ours: 300_000). The max-length clamp (1 mm) applies to the length past the edge too.
- **Severity.** high: wrong copper, and the zones are written to the `.kicad_pcb` (`(legacy_teardrops yes)`, GAPS 519-529), so KiCad shows the same stunted teardrops.
- **Already known?** No. GAPS 519-521 and PARITY-pcb section 15 list the documented cuts (round anchors only, one segment, no curves), not this.
- **Same area (low):** the width filter is only `track_width > d * filter_ratio` (`teardrop.rs:117`); `teardrop.cpp:282-284` (`TEARDROP_MANAGER::UpdateTeardrops`'s pad and via loops) skips `track->GetWidth() >= tdParams.m_TdMaxWidth || >= annularWidth * m_BestWidthRatio || >= annularWidth * m_WidthtoSizeFilterRatio`, all three with `>=`.
- **Reuse rule.** The whole generator duplicates C++ that kicad-cli runs: `zone_filler_tool.cpp:135` (`teardropMgr.UpdateTeardrops( commit, nullptr, nullptr, true /* forceFullUpdate */ )`) rebuilds every teardrop from the pads' and vias' teardrop parameters on a refill, the run `fill_api.rs` already makes. Writing the parameters (`(teardrops (best_length_ratio ..) ..)` per pad/via) and not `(legacy_teardrops yes)` would give KiCad's own shapes, rect pads and two-track teardrops included. GAPS 521 states this as left ("KiCad's own teardrop rebuild ... is not what the studio asks for"), so it is a known choice. It is reported here because this finding is in the duplicated code.

### C-2 (medium). Create Array leaves out text boxes, barcodes, points and reference images (and tables, tuning patterns): alone it refuses, in a mixed selection it drops them without a word
- **Status:** **Fixed** in part on task-r4-pcb-edit: a49363b6 (text boxes, barcodes, points, images, tables); tuning patterns left
- **What differs.** `crates/ops/src/array.rs:264-277` (`array_selection`) keeps an id only if `item_position` finds it. `item_position` (`:230-260`) knows footprints, tracks, vias, zones, shapes, texts, dimensions and groups only. So a text box, barcode, point, image, table or tuning pattern is dropped before `collect_copies` (which *can* copy the first four, `pcb_paste.rs:317`). Alone, it fails with "none of the given ids name a footprint, track, via, zone, shape, text, dimension or group on the board". KiCad's `pcbnew/tools/array_tool.cpp::ARRAY_TOOL::onDialogClosed` duplicates all of them:
  ```cpp
  case PCB_BARCODE_T: case PCB_REFERENCE_IMAGE_T: case PCB_TEXT_T: case PCB_TEXTBOX_T: case PCB_TABLE_T:
  ... case PCB_POINT_T: case PCB_TARGET_T: case PCB_ZONE_T:
      this_item = item->Duplicate( true, &commit );
  case PCB_GENERATOR_T:
      this_item = static_cast<PCB_GENERATOR*>( item )->DeepClone();
  ```
  Arrange selection has the same gap. Inside a group these items are copied (through `copy_group`), but they are left out of the group's position (`middle` over `item_position`, see C-3).
- **How to see it.** Draw a text box, select it, Ctrl+T, a 3 x 1 grid: nothing is made and an error toast appears. Test `array_tests::a_text_box_barcode_point_and_image_are_arrayed`: a board with one of each, `Cmd::CreateArray { ids: [the four], Grid nx 3, dx 5 mm }`; assert there are 3 of each kind, at x, x+5, x+10 mm.
- **Severity.** medium (a missing result, no data loss).
- **Already known?** No. GAPS line 406 says "Paste, Duplicate and Create Array land text boxes and images", which is untrue for Create Array.

### C-3 (medium). A group's position in Create Array is the middle of its members' anchor points, not the centre of its bounding box: circular arrays without "Rotate items", and Arrange selection, put groups elsewhere
- **Status:** **Fixed** on task-r4-pcb-edit: a49363b6 (the studio sends the box centre, `group_positions`)
- **What differs.** `array.rs:255-257` (`item_position` of a group) and `:463-465` (`transform_block`) take `middle(points)`, the box of the members' *positions* (a footprint's origin, a track's start, ...). KiCad's `pcbnew/pcb_group.cpp::PCB_GROUP::GetPosition` is `return GetBoundingBox().Centre();`, the full item boxes. `TransformItem` (`array_tool.cpp:44-50`) feeds that position to `GetTransform( aIndex, aItem.GetPosition() )`. For a circular array the offset is `RotatePoint( new_pos, m_centre, angle ) - aPos` (`array_options.cpp::ARRAY_CIRCULAR_OPTIONS::GetTransform`), so it depends on the position whenever "Rotate items" is off. Arrange selection aligns every item to `firstItem->GetPosition()` through `PCB_GROUP::SetPosition`, so two groups whose boxes are not centred on their anchors land offset from KiCad's. A grid, and a circle with Rotate items on, are the same either way: the motion is rigid.
- **How to see it.** A group of a footprint at (0,0) whose courtyard spans x 0..10 mm, plus a track starting at (0,0). Circular array, centre (20 mm, 0), 2 points, 90 degrees, Rotate items off. KiCad moves the copy by `R(5,0) - (5,0)`; ours by `R(0,0) - (0,0)`. Test `array_tests::a_group_turns_about_its_bounding_box_centre_in_a_circle`: assert the copied footprint's `at` is 5 mm off ours.
- **Severity.** medium (wrong placement).
- **Already known?** No. p6's "checked" list has "group box centre" for the TS transform code. `array.rs` is a separate implementation.

### C-4 (medium). Drawing a zone, rule area or graphic polygon: a click on the first corner does not close the outline, and the line mode (45 / 90 degrees) is ignored
- **Status:** **Fixed** on task-r4-pcb-edit: 6398b588
- **What differs.**
  - `web/components/canvas/Canvas.tsx:793-797` appends every click to the zone's points. Graphic polygons (`:878-896`) do the same. `pcbnew/tools/drawing_tool.cpp::DrawZone` (which runs `drawPolygon`, `drawZone`, `drawRuleArea`, `drawZoneCutout` and `drawSimilarZone`, `:4402-4416`) ends the outline when the click is on the first corner: `const bool endPolygon = evt->IsDblClick( BUT_LEFT ) || evt->IsAction( &PCB_ACTIONS::closeOutline ) || polyGeomMgr.NewPointClosesOutline( cursorPos );`, with `POLYGON_GEOM_MANAGER::NewPointClosesOutline` being `m_lockedPoints.PointCount() > 0 && m_lockedPoints.CPoint( 0 ) == aPt`. Ours adds a duplicate corner and keeps drawing.
  - `DrawZone` sets `polyGeomMgr.SetLeaderMode( angleSnap )` from `GetAngleSnapMode()` (Ctrl gives `LEADER_MODE::DIRECT`). In 45-degree mode `AddPoint` locks in two points per click (`m_leaderPts`' elbow and end). Ours constrains only segments and rectangles (`Canvas.tsx:882-886`, `constrainByAngleMode`). Zones and polygons are always free-angle, whatever `state.pcbx.angleSnapMode` says.
- **How to see it.** Zone tool: click (0,0), (10,0), (10,10), then (0,0) again. KiCad finishes and opens the zone dialog. Ours has 4 corners with a duplicate and waits. With Shift+Space on "Constrain to H, V, 45", a click at (10,3) from (0,0) gives KiCad two corners ((7,0) then (10,3)); ours gives one diagonal edge. A unit test belongs next to `drawingCancel.test.ts`: a `zoneClick(pts, p)` helper returns `finish` when `p` equals `pts[0]`.
- **Severity.** medium (a wrong outline and a wrong flow, the most-used way of closing a zone).
- **Already known?** No.

### C-5 (low). A finished graphic polygon leaves its drawing tool armed; KiCad's DrawZone pops back to the selection tool
- **Status:** **Fixed** on task-r4-pcb-edit: 6398b588
- **What differs.** `Canvas.tsx:637-650` (`finishDraw`, `draw.kind === "shape"`) only clears `drawState`. The zone branch does `SET_ACTIVE_TOOL select` (`:629`), as KiCad does. KiCad's graphic polygon runs in `DrawZone` too (`Go( &DRAWING_TOOL::DrawZone, PCB_ACTIONS::drawPolygon.MakeEvent() )`), and `if( endPolygon ) { ...; cleanup(); m_frame->PopTool( aEvent ); break; }`.
- **How to see it.** Draw a polygon, double-click: KiCad is back in selection; ours waits for the next polygon.
- **Already known?** No. r3's "Noted, not checked" item (the zone tool) is answered under "checked" below: zones match.

### C-6 (medium). Global Deletions: "Clear board" leaves text boxes, tables, barcodes, images, points and tuning patterns; "Text" leaves text boxes
- **Status:** **Fixed** on task-r4-pcb-edit: d8538fb5 (and the three lows; the Text deviation is in PARITY-pcb section 58)
- **What differs.** `web/kicad-port/pcbGlobalEdit.ts:83-135` (`planGlobalDeletions`) with `all` deletes zones, shapes, texts, dimensions, footprints, tracks and vias only. `pcbnew/dialogs/dialog_global_deletion.cpp::DIALOG_GLOBAL_DELETION::DoGlobalDeletions` removes everything in `board->Drawings()` (`if( delete_all ) processItem( item, all_layers );`), which holds text boxes, tables, barcodes, reference images and points. It also removes tuning patterns left with no tracks (`if( pattern->GetBoardItems().empty() ) commit.Remove( pattern );`). The Text option is `if( item->Type() == PCB_TEXT_T || item->Type() == PCB_TEXTBOX_T )`, but ours walks `drawings.texts` only.
- **How to see it.** A board with a text box and a barcode, Global Deletions > "Clear board" > OK: both stay. Test in `pcbGlobalEdit.test.ts`: `planGlobalDeletions(b with text_boxes:[tb], barcodes:[bc], opts({ all: true }))`; assert `removed` includes `tb` and `bc`.
- **Severity.** medium.
- **Already known?** No.
- **Same function (low).**
  - Zones are matched on `z.layer` alone (`:97`). KiCad tests `( item->GetLayerSet() & layers_mask ).any()`, so a F.Cu+B.Cu zone is deleted with "Current layer (B.Cu) only"; ours keeps it.
  - The lock filters read `board.locked` by id. KiCad's `IsLocked()` also counts the group above (`BOARD_ITEM::IsLocked`, r3 B-3), so a shape in a locked group counts as locked in KiCad and as unlocked here.
  - The deliberate deviation in the comment (`:79-81`, Text deleted even with Graphics ticked: KiCad's `else if` chain only deletes texts when no graphics option is set) is not in PARITY-pcb. It should be listed there as a difference.

### C-7 (low). Merge Zones: refusals KiCad does not make, and the intersection test ignores holes
- **Status:** **Fixed** on task-r4-pcb-edit: a4f1a061 (holes; the log lines as the refusal); no self-intersection check (a simplified union never self-intersects)
- **What differs.**
  - `crates/ops/src/board_control.rs:123-125` refuses with "the selected zones do not touch, so there is nothing to merge" when only the first zone qualifies. `pcbnew/tools/board_editor_control.cpp::ZoneMerge` only `wxLogMessage( _( "Some zones did not intersect and were not merged." ) )` (likewise "Some zone netcodes did not match and were not merged.", "Some zones were rule areas and were not merged.", "Some zone layer sets did not match and were not merged."). It still runs `mergeZones` on the one zone (a `Simplify` and a "Merge Zones" commit). Ours shows an error KiCad has no words for, and never shows the four log lines.
  - `zones_intersect` (`board_control.rs:94-96`) uses the outer outline only. `edit_zone_helpers.cpp::BOARD::TestZoneIntersection` iterates `IterateSegmentsWithHoles()` and tests corners with `poly1->Contains( *iter )`, which excludes holes. A zone lying wholly inside another's hole is "not intersecting" in KiCad (skipped, the rest merged); ours takes it in, and the union gives two outlines and "Zones have insufficient overlap for merging.", so nothing merges.
  - `mergeZones` also refuses on `IsSelfIntersecting()`; ours does not check.
- **Already known?** No.

### C-8 (low). Create Array, circular: the angle is rounded to a millidegree before it is multiplied
- **Status:** Open (not fixed: KiCad reads its full-circle angle back from the entry's formatted text, so sending the exact double is not KiCad's either; 2.3 um at 50 mm)
- **What differs.** `array.rs:141-142`: `angle_millideg * n` on an integer millidegree angle, and the even division `.round()`. KiCad: `EDA_ANGLE( m_angle.AsDegrees() * n, DEGREES_T )` on the double the entry held (`DoubleValueFromString( m_entryCircAngle->GetValue() )`). "Full circle" with 7 points gives 51.428571 degrees in KiCad. Ours sends 51429 mdeg, so point 6 is 0.0026 degrees off: 2.3 µm at a 50 mm radius.
- **Already known?** No.

---

### Checked and found to match (do not re-review)
- **`ARRAY_GRID_OPTIONS`:**
  - `getGridCoords` (axis size, serpentine).
  - `gtItemPosRelativeToItem0`'s axis swap and skew offsets.
  - The stagger: `VECTOR2I * double` gives `VECTOR2<double>`, `/ stagger` is not rounded, and the `VECTOR2I` conversion is `static_cast`, a truncation toward zero, equal to our `i64` division.
  - `m_centred`'s extent with `m_nx`/`m_ny` whatever the numbering axis.
- **`ARRAY_CIRCULAR_OPTIONS::GetTransform`:** the zero-angle even division, the offset, `m_clockwise` negating, the rotation dropped when not Rotate items.
- **`DIALOG_CREATE_ARRAY::TransferDataFromWindow`:** "horizontal delta of zero with %ld objects", "vertical ...", "angular delta of zero with %ld objects" (KiCad refuses a zero angle even though `GetTransform` could divide evenly; ours too); `calculateCircularArrayProperties`' 360/n.
- **`onDialogClosed`:**
  - Arrange: `fpDeDupe`, the first item's position, items past the point count left alone, a pad or field standing for its footprint.
  - Duplicate: the reverse `ptN` loop's points and references (the original-on-last-point difference is in GAPS 22).
- **`BOARD_REANNOTATE_TOOL::ReannotateDuplicates`:** the increment from the reference's own number (`value < 0 ? 1 : ++value`) equals `pcb_paste.rs::unique_reference`.
- **`TransformItem`:** move, then rotate about the new `GetPosition()`. Positions: a track's `m_Start`, an arc's centre (`PCB_ARC::GetPosition`, only matters for rigid motions), `EDA_SHAPE::getPosition`, `ZONE::GetPosition` (first corner), a dimension's `m_start`.
- **`DRAWING_TOOL::DrawZone` for zones and rule areas** (r3's "Noted, not checked"): finishing pops the tool. Ours sets the select tool after the zone dialog, cutout, similar zone and via stitching (`Canvas.tsx:629`). See C-5 for polygons.
- **`ZoneMerge` / `mergeZones`:** the first zone's settings kept; net, rule-area-ness and layer-set filters; the "intersects any already taken" chain in selection order; holes kept, not fractured; the highest priority adopted; "Zones have insufficient overlap for merging." on more than one outline.
- **`DoGlobalDeletions`:**
  - Zones against teardrops.
  - The drawing layer mask (`AllNonCuMask` without Edge.Cuts; Edge.Cuts for Board outlines), and only `PCB_SHAPE_T` under Graphics.
  - The footprint, track and via lock filters (vias separately).
  - Footprints by their copper side; markers cleared.
  - The dialog's benign defaults.
- **`TEARDROP_PARAMETERS` defaults:** 1 mm / 2 mm / 0.5 / 1.0 / 0.9; every target on.

---

# Summary

| # | Finding | Severity |
|---|---|---|
| C-1 | Teardrop length measured from the pad or via centre, not its edge: with the default ratio the teardrop ends on the via's rim (written to the file) | high |
| A-1 | The board's Line tool draws one segment per two clicks; KiCad chains segments, and Delete or Backspace takes back the last committed one | medium |
| A-2 | Delete with a shape started deletes the (never cleared) selection; KiCad cancels the shape | medium |
| A-3 | Redo and zone fills while drawing act on the board; KiCad refuses them (`wxBell`) | medium |
| B-1 | Swap Layers and tuning patterns: the round-3 A-2 fix is wrong both ways and when chained (KiCad's `GetLayer`/`SetLayer` follow the tracks) | medium |
| B-2 | A lone tuning pattern turns and mirrors about its box centre, not `m_origin` | medium |
| B-3 | Set to layer default values: the text is made square at the class height, and italic is not reset | medium |
| C-2 | Create Array refuses or drops text boxes, barcodes, points, images, tables and tuning patterns | medium |
| C-3 | Create Array: a group's position is the middle of its anchors, not its bounding-box centre | medium |
| C-4 | Zone and polygon drawing: a click on the first corner does not close it, and the line mode is ignored | medium |
| C-6 | Global Deletions' Clear board and Text leave text boxes, tables, barcodes, images, points and tuning patterns | medium |
| A-4 | Delete or Undo of a zone corner: no cursor warp, and the last corner ends the outline one press early | low |
| A-5 | Cleanup merge refuses joints inside zones, overlaps and fold-backs, and has no 1 nm tolerance | low |
| A-6 | Track & Via short question: no-net and indeterminate names; an OK with no edits asks nothing | low |
| A-7 | Select Connection with a tuning pattern selected picks under the cursor instead | low |
| B-4 | Pad checks: custom-pad mask and paste sizes; hole checks on a pad with no copper | low |
| B-5 | Members of a locked group are not written `(locked yes)` | low |
| C-5 | A finished graphic polygon leaves its tool armed | low |
| C-7 | Merge Zones: an error KiCad does not show, holes ignored, no self-intersection check | low |
| C-8 | Circular array angle rounded to a millidegree before it is multiplied | low |

Counts: high 1, medium 10, low 9.

These round-3 fixes came out right; each part's "checked" list gives the detail:
- Cleanup (D-1 to D-5, D-12 (a)(c)(d));
- table cells (B-1) and the group of one footprint (B-2);
- zone-drawing Delete and Undo (C-1, apart from A-4);
- measure-tool Undo (C-6);
- Track & Via nets (C-2, apart from A-6) and the width message (C-3);
- Pad checks for plain shapes (C-4);
- Add Field (C-5);
- Select Connection (D-6, apart from A-7);
- microwave (D-7);
- Break Track (D-8);
- Edit Text & Graphics' specified values (D-9);
- flip mirror (A-1);
- group lock (B-3; B-5 is the writer's other half);
- the section 56 via rows, `FOOTPRINT_DESC` rows and Mirror.

Round 3's A-2 (Swap Layers and generators), marked Fixed, is wrong: see B-1.
