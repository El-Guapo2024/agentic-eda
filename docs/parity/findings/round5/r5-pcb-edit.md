# Findings round 5: PCB editing and its UI (`r5-pcb-edit`)

Reviewer `r5-pcb-edit`. Findings only: no code was changed, nothing was built and no server was started.
- **Code read:** main's checkout `/home/team_braneboard_com/work/agentic-eda`, HEAD `8a8e47f1`.
- **KiCad compared against:** `8303b2ad` (`$KICAD_SRC_DIR`), and `25ec2335` where marked.
- **Scratch:** none kept (every finding is read from the code).
- **Paths:** ours are from the repo root; `web/` means `web/studio/src/`. KiCad's are from its source root.

Scope: GAPS items 8, 9, 10, 13 (teardrops), 14-18, 21, 22, 29 and 35.
- **Priority (a): the round-4 merges** (PARITY-pcb section 58: teardrops `a6ea19db`, Swap Layers `d977e10a`, layer defaults `2a6681ec`, Create Array `a49363b6`, Global Deletions `d8538fb5`, the drawing tools `6398b588`, locked groups `3853fa1d`, and the smaller A-5/A-6/A-7/B-2/C-7 fixes). Part A.
- **Priority (b): rows no round has reviewed.** The Draw Via tool (`DRAWING_TOOL::DrawVia`), the board's Text tool (`PlaceText`), Cleanup Graphics (`GRAPHICS_CLEANER`) and Create from Selection (`CONVERT_TOOL`). Part B.

Finding ids are round-5 ids (`A5-1`, `B5-1`, ...).

Reuse rule: no new duplicate of kicad-cli found. The teardrop generator still duplicates `TEARDROP_MANAGER::UpdateTeardrops` (already recorded, GAPS 13 / section 52); A5-1 is a defect inside it.

---

# Part A: the round-4 merges

### A5-1 (high). An arc track at a via or round pad gets no teardrop (or a stunted one): the generator takes the arc's first tessellation segment as the track
**Fixed** on task-r5-pcb-edit: a98857b6 (arc tracks as `findAnchorPointsOnTrack`; 25ec2335's arc refinements; proven point for point by `teardrops_kicad_cli.rs`'s four arc cases).
- **What differs:**
  - **Ours.** `crates/connectivity/src/teardrop.rs:445-451` takes `(near, far)` as `pts[0], pts[1]` (or the last two points). For an arc track, `Track::pts` is the arc's 32-segment tessellation (`crates/model/src/ir.rs:1313-1324`, `TRACK_ARC_SEGMENTS`), and `arc_mid_offset` is never read in `teardrop.rs`. So the "track" is the first 1/32 of the arc:
    - when that piece ends inside the anchor, `teardrop.rs:363-367` ("The track is entirely inside the via") returns `None`, and there is no teardrop at all;
    - when it does not, `stub_len` is capped by that piece's length (`teardrop.rs:385`), so the teardrop is stunted and points along the chord of one tessellation step.
  - **KiCad.** `pcbnew/teardrop/teardrop.cpp::TEARDROP_MANAGER::UpdateTeardrops` takes `PCB_ARC_T` tracks too (`if( ! ( track->Type() == PCB_TRACE_T || track->Type() == PCB_ARC_T ) ) continue;`). `teardrop_utils.cpp::findAnchorPointsOnTrack` intersects the anchor's outline with the real arc and then picks the reference segment on it:
    ```cpp
    if( aTrack->Type() == PCB_ARC_T )
    {
        SHAPE_ARC arc( aTrack->GetStart(), static_cast<PCB_ARC*>( aTrack )->GetMid(),
                       aTrack->GetEnd(), aTrack->GetWidth() );
        SHAPE_LINE_CHAIN poly = arc.ConvertToPolyline( maxError );
        pt_count = outline.Intersect( poly, pts );
    }
    ...
    // if aTrack is an arc, find the best teardrop end point on the arc
    ...
            for( int ii = poly.PointCount()-1; ii >= 0 ; ii-- )
            {
                int dist_from_start = ( poly.CPoint( ii ) - start ).EuclideanNorm();
                if( dist_from_start < actualTdLen || ii == 0 )
                {
                    start = poly.CPoint( ii );
    ```
    `computeEmergingTrackLength` does the same for arcs (`arc.ConvertToPolyline( maxError )`).
- **How to see it:** a 0.6 mm via at (100, 100) mm and a 0.2 mm arc track from (100, 100) through (101, 100.4) to (102, 100) on F.Cu, with teardrops on. Add All Teardrops.
  - Ours: the first tessellation point is about 0.07 mm from the centre, inside the via, so no teardrop.
  - KiCad (`kicad-cli pcb drc --refill-zones --save-board` on the same board with the via's `(teardrops (enabled yes) ..)` and no `(legacy_teardrops yes)`, as `crates/kicad/tests/teardrops_kicad_cli.rs` already does): one teardrop.
  - Test: `teardrop.rs`, `an_arc_track_gets_a_teardrop_along_the_arc`. Build the arc with `Track::arc`/`tessellate_arc` and assert one teardrop zone; extend `teardrops_kicad_cli.rs` with an arc case and compare the outline point for point.
- **Severity:** high. The teardrop zones are written to the file (`(legacy_teardrops yes)`, so kicad-cli keeps ours), and the copper is wrong for every arc track at a round anchor.
- **Already known?** No. GAPS 13/35 lists curved edges, non-round anchors, track-to-track, two-segment borrowing, `m_TdOnPadsInZones` and incremental updates, not arc tracks. The module doc says "Straight edges only", which is about `m_CurvedEdges`.

### A5-2 (high). A rectangle is constrained by the line mode (45 / 90 degrees, the studio's default 45), so it comes out flat or square; KiCad draws a rectangle free, and Ctrl gives a square
**Fixed** on task-r5-pcb-edit: 9bb101cb (`kicad-port/pcbShapeSnap.ts`).
- **What differs:**
  - **Ours.** `web/components/canvas/Canvas.tsx:906-910` runs `constrainByAngleMode(state.pcbx.angleSnapMode, last, [sx, sy])` on the second corner of a **rectangle** as on a line, and `painter.ts:1285-1287` does the same for the preview. In 45 mode, `posture45` (`web/components/canvas/routing.ts:188-197`) turns the diagonal onto the nearest 45-degree ray, keeping its length. So a 10 x 3 mm drag (16.7 degrees) becomes a 10.4 x 0 mm rectangle, and 10 x 7 mm becomes a square of 8.6 mm. In 90 mode every rectangle is flat.
  - **KiCad.** `pcbnew/tools/drawing_tool.cpp::DRAWING_TOOL::drawShape`:
    ```cpp
    // Drawing rectangles and circles ignore the snap behavior by default, but constrains
    // when the modifier key is pressed
    if( shape == SHAPE_T::RECTANGLE || shape == SHAPE_T::CIRCLE)
    {
        if( evt->Modifier( MD_CTRL ) )
            angleSnap = LEADER_MODE::DEG45;
        else
            angleSnap = LEADER_MODE::DIRECT;
    }
    ```
    and with Ctrl, `newEnd = GetVectorSnapped45( lineVector, ( shape == SHAPE_T::RECTANGLE ) )`: `only45` true, so a square on the diagonal.
- **How to see it:** with the studio as it starts (line mode 45), Draw Rectangle, click (0,0), click (10,3) mm.
  - KiCad: a 10 x 3 mm rectangle.
  - Ours: a degenerate rectangle (0,0)-(10.44,0), or (10,0) after the grid snap.
  - Test (`node --test`): extract the click's constraint into a function (`shapeSecondPoint(kind, mode, ctrl, from, to)`); `("rect", "45", false, [0,0], [10e6,3e6])` must give `[10e6,3e6]`, and `ctrl: true` must give `[10e6,10e6]`.
- **Severity:** high (the most-used shape tool gives a wrong shape under the default settings).
- **Already known?** No. Round 4 C-4 named `constrainByAngleMode` only as the reason zones ignored the line mode.

### A5-3 (medium). The line tool's 45-degree mode turns the segment onto the nearest ray keeping its length; KiCad keeps the cursor's x (or y), and the boundary is 2:1, not 22.5 degrees. Ctrl does not free the angle
**Fixed** on task-r5-pcb-edit: 9bb101cb.
- **What differs:**
  - **Ours.** `posture45` (`web/components/canvas/routing.ts:188-197`) rounds the angle to a multiple of 45 degrees (boundary at 22.5 degrees) and keeps the length, then `Canvas.tsx:908-909` puts the result back on the grid. `constrainByAngleMode` takes no modifier, so Ctrl changes nothing.
  - **KiCad.** `drawShape` sets `newEnd = GetVectorSnapped45( lineVector, false )` (`libs/kimath/include/geometry/geometry_utils.h`):
    ```cpp
    if( !only45 && absVec.x > ext_type( absVec.y ) * 2 )
        newVec.y = 0;          // snap along x-axis
    else if( !only45 && absVec.y > ext_type( absVec.x ) * 2 )
        newVec.x = 0;          // snap onto y-axis
    else if( absVec.x > absVec.y )
        newVec.y = std::copysign( aVec.x, aVec.y );   // snap away from x-axis towards 45
    else
        newVec.x = std::copysign( aVec.y, aVec.x );
    ```
    So the end keeps the cursor's grid x (or y). Ctrl is free angle for lines (`if( evt->Modifier( MD_CTRL ) ) angleSnap = LEADER_MODE::DIRECT;`).
- **How to see it:** in 45 mode on a 1 mm grid, draw a line from (0,0) and click at (10,4) mm.
  - KiCad: end (10,0).
  - Ours: end (11,0) (length 10.77 mm on the ray, then the grid).
  - A click at (10,7): KiCad (10,10); ours (9,9).
  - Test: `constrainByAngleMode("45", [0,0], [10e6,4e6])` should be `[10e6,0]`, and `[10e6,7e6]` should give `[10e6,10e6]`; with Ctrl the cursor itself.
- **Severity:** medium.
- **Already known?** No.

### A5-4 (medium). The studio starts in the 45-degree line mode; KiCad's default is free angle
**Fixed** on task-r5-pcb-edit: 9bb101cb.
- **What differs:**
  - **Ours.** `web/kicad-port/pcbParityState.ts:101`: `angleSnapMode: "45"`.
  - **KiCad.** `pcbnew/pcbnew_settings.cpp` (8303b2ad and 25ec2335): `m_AngleSnapMode( LEADER_MODE::DIRECT )` and `new PARAM<int>( "editing.pcb_angle_snap_mode", reinterpret_cast<int*>( &m_AngleSnapMode ), static_cast<int>( LEADER_MODE::DIRECT ) )`.
  - This default makes A5-2, A5-3 and A5-5 the out-of-the-box behaviour of every line, rectangle, zone and polygon.
- **How to see it:** a fresh studio, Draw Line, a click at (0,0) and a move to (10,3) mm: KiCad's rubber band goes to the cursor; ours snaps to (10.4,0).
  - Test: `DEFAULT_PCB_PARITY.angleSnapMode === "direct"`.
- **Severity:** medium (a wrong default).
- **Already known?** No.

### A5-5 (medium). Closing a zone, rule area or graphic polygon: the outline is the clicked corners only; KiCad adds the bent leader's and loop's corners in the 45/90 modes and simplifies the outline
**Fixed** on task-r5-pcb-edit: 3ba02f35 (`pcbDrawZone.ts::zoneFinish`, and the loop in the preview).
- **What differs:**
  - **Ours.** `finishDraw` (`web/components/canvas/Canvas.tsx:614-633` for zones, cutouts, similar zones and stitchings; `:652-656` for a graphic polygon) uses `draw.pts`, the locked corners. `kicad-port/polygonGeom.ts::finalOutline` already ports `OnComplete` (leader, loop, `Simplify`, the start point dropped when collinear), but only the schematic's rule area calls it (`components/schematic/schShapeTools.ts:69`). Our preview (`painter.ts:1290-1296`) closes the outline straight from the cursor to the first corner, with no loop line.
  - **KiCad.** `pcbnew/tools/zone_create_helper.cpp::ZONE_CREATE_HELPER::OnComplete`, which zones, rule areas and graphic polygons all go through:
    ```cpp
    // In DEG45 mode, we may have intermediate points in the leader that should be included
    // as they are shown in the preview.  These typically maintain the 45 constraint
    if( aMgr.GetLeaderMode() == LEADER_MODE::DEG45 || aMgr.GetLeaderMode() == LEADER_MODE::DEG90 )
    {
        const SHAPE_LINE_CHAIN leaderPts = aMgr.GetLeaderLinePoints();
        for( int i = 1; i < leaderPts.PointCount(); i++ )
            outline->Append( leaderPts.CPoint( i ) );
        const SHAPE_LINE_CHAIN loopPts = aMgr.GetLoopLinePoints();
        for( int i = 1; i < loopPts.PointCount() - 1; i++ )
            outline->Append( loopPts.CPoint( i ) );
    }
    SHAPE_LINE_CHAIN& chain = outline->Outline( 0 );
    chain.SetClosed( true );
    chain.Simplify( true );
    // Remove the start point if it lies on the line between neighbouring points.
    ```
    `POLYGON_GEOM_MANAGER::updateTemporaryLines` builds the loop as a 45/90 path from the cursor back to the first corner (`m_loopPts = build45DegLeader( aEndPoint, m_lockedPoints.Reverse() ).Reverse();`). So a zone closed in 45 mode gets a bent closing edge, not an arbitrary-angle one.
  - In free-angle mode, `Simplify( true )` still drops repeated and collinear corners, which ours keeps (for example three collinear clicks, or a corner clicked twice).
- **How to see it:** in 45 mode, draw a zone (0,0), (10,0), (10,10), then (3,7) (two corners locked: the bend and the end), and double-click there.
  - KiCad: the closing edge from (3,7) to (0,0) is bent onto 45/90 legs, with an extra corner.
  - Ours: a straight edge (3,7)-(0,0).
  - Test (`pcbDrawZone.test.ts`): `zoneFinish(pts, cursor, "deg45")` (a new function) returns `finalOutline` of the geometry, and `finishDraw` uses it. Assert the extra corner, and that `[[0,0],[5,0],[10,0],[10,10]]` in direct mode closes to three corners.
- **Severity:** medium (the zone outline written to the board differs).
- **Already known?** No. Round 4 C-4's fix (`6398b588`) ported the click and the leader, not the completion.

### A5-6 (medium). A graphic polygon emptied by Delete: the next Delete deletes the selection. A zone or polygon's first corner does not clear the selection
**Fixed** on task-r5-pcb-edit: 3ba02f35.
- **What differs:**
  - **Ours.** `drawZoneStarted` (`web/kicad-port/drawingCancel.ts:37-41`) keeps a zone `started` with no corner, but a graphic polygon only while `pts.length > 0`. After the first Delete has taken its only corner (`withoutLastCorner` keeps `{ shapeKind: "polygon", pts: [] }`), the second Delete:
    - is not `drawZoneStarted`;
    - is not `drawShapeStarted` either (polygon is not in `DRAW_SHAPE_KINDS`, `drawingCancel.ts`);
    - so it reaches `requestSelection()` and `deleteRefs` (`web/actions/useActionRunner.ts:378-379`).
  - Neither the zone tool nor the polygon tool clears the selection at the first corner (`Canvas.tsx:801-806`, `:893-899`), so whatever was selected before is deleted.
  - **KiCad.** `DRAWING_TOOL::DrawZone` keeps `started` until `cleanup()`, so the second Delete is still the tool's:
    ```cpp
    else if( started && (   evt->IsAction( &PCB_ACTIONS::deleteLastPoint )
                         || evt->IsAction( &ACTIONS::doDelete )
                         || evt->IsAction( &ACTIONS::undo ) ) )
    {
        if( std::optional<VECTOR2I> last = polyGeomMgr.DeleteLastCorner() ) { ... }
        else
        {
            cleanup();
        }
    }
    ```
    And the selection is cleared when the first corner goes in (`zone_create_helper.cpp::ZONE_CREATE_HELPER::OnFirstPoint`: `m_tool.GetManager()->RunAction( ACTIONS::selectionClear );`).
- **How to see it:** select footprint U1, pick Draw Polygon, click once, press Delete twice.
  - KiCad: the outline ends, and U1 stays.
  - Ours: U1 is deleted (undoable).
  - Test (`drawingCancel.test.ts`): `drawZoneStarted("pcb", "draw_polygon", { kind: "shape", shapeKind: "polygon", pts: [] })` should be true. Also add a Canvas-level check that the first zone or polygon corner dispatches `SET_SELECTION []`.
- **Severity:** medium (deletes items the user did not mean to).
- **Already known?** No. Round 4 A-4's fix made the zone kind keep `started`, but not the polygon kind.

### A5-7 (low). Docs and a test left stale by the teardrop rewrite
**Fixed** on task-r5-pcb-edit: a98857b6 (the test) and 5720d03e (PARITY-pcb sections 12, 15; round 5 is section 60).
- `web/studio/PARITY-pcb.md:740` still says the pentagon is "ported via a closed-form exact-tangent-on-circle construction instead of source's general convex-hull-over-clipped-polygon algorithm". Since `a6ea19db` it is the hull construction.
- `PARITY-pcb.md:747` says "a track shorter than the requested teardrop length is simply skipped". It now gets a shorter teardrop (`actualTdLen`), as section 58 says.
- `crates/connectivity/src/teardrop.rs:505-517` (`a_track_too_short_for_the_requested_length_is_skipped`): the comment says the 50 um track "must be skipped, not clipped". The test passes only because the track ends inside the 0.8 mm via (`inside(end)`). A 0.5 mm track would now get a clipped teardrop, so the name describes the old rule.
- **Severity:** low.

### Part A: checked and found to match (do not re-review)
- **Teardrops (`a6ea19db`):**
  - `UpdateTeardrops`' three `>=` width filters, for pads and vias.
  - The "entirely inside" test by `PCB_VIA::HitTest` (`dist <= max_dist^2`).
  - `computeEmergingTrackLength`'s nearest crossing from the outside end, `KiROUND`.
  - `GetWidth`: a via's width, a pad's `min( x, y )`.
  - `IsRound`: a circle, or an oval with x == y.
  - `findAnchorPointsOnTrack`'s target length `KiROUND( width * ratio )` capped by `m_TdMaxLen > 0`, then `min( target, SEG( start, end ).Length() )` from the crossing.
  - `computeAnchorPoints`' `force_clip = m_BestWidthRatio < 1.0`, the clip rectangle, `halfsize = min( maxW, preferred ) / 2`, and the hull neighbours with the area swap.
  - `setTeardropPriorities` (the writer's `teardrop_priorities`).
  - The anchors' layer reach (a PTH pad's whole stack, an SMD pad's side, a via's span).
- **Swap Layers (`d977e10a`):**
  - `DIALOG_SWAP_LAYERS::TransferDataFromWindow` maps copper layers only; ours lists `board.layers`.
  - The track pass, then the generators (`swapBoardItem` on `PCB_GROUP::GetLayerSet() | LSET( { GetLayer() } )`, `SetLayerSet` only for one layer).
  - `PCB_TUNING_PATTERN::GetLayer` / `SetLayer`; the both-ways and chained results.
  - Blind/buried vias by `SetLayerPair`, through vias skipped; zones, drawings, tables and barcodes by `swapBoardItem`; footprints untouched.
- **Set to layer default values (`2a6681ec`):** `PCB_TEXT::StyleFromSettings` (size w,h; thickness; italic; keep-upright only with a parent footprint, which these texts have not) and `PCB_TEXTBOX::StyleFromSettings` (the shape's stroke first). The text box and cell fields are as described.
- **Create Array (`a49363b6`):**
  - The type list of `onDialogClosed`'s duplicate switch: footprints, shapes, barcodes, images, texts, text boxes, tables, tracks, arcs, vias, dimensions, points, targets, zones; generators by `DeepClone`, which is still left; groups by `DeepDuplicate`.
  - `TransformItem`: `Move`, then `Rotate( GetPosition() )`.
  - `PCB_TABLE::GetPosition` (cell 0's), `EDA_SHAPE::getPosition` for text boxes, and `PCB_GROUP::GetPosition` (the box centre, from `itemBounds`' union of the leaves).
  - Reannotation: KiCad leaves the copies of point 0 un-reannotated (`ptN != arraySize - 1`) and the original renamed last, so the references by point match ours (GAPS 22).
- **Global Deletions (`d8538fb5`):**
  - `DoGlobalDeletions`' zone pass (teardrops apart, `GetLayerSet() & layers_mask`).
  - The `Drawings()` pass with `PCB_SHAPE_T`-only Graphics filters and `PCB_TEXT_T || PCB_TEXTBOX_T` Text.
  - Clear board over every drawing; empty tuning patterns removed.
  - The footprint, track and via lock filters; `BOARD_ITEM::IsLocked` through groups.
  - The Text/Graphics `else if` deviation is documented.
- **The drawing tools (`6398b588`):**
  - `DrawLine`'s chain: `commit.Push( "Draw Line" )`, `startingPoint = line->GetEnd()`.
  - The three ends of a chain: same spot (`twoPointMgr.IsEmpty()`), a double-click, a snapped `PCB_SHAPE`. The new graphic not being selected is documented.
  - `drawShape`'s Delete/Backspace branch (pop the top line, undo its commit, resume from its start; else `cleanup()` with the tool kept), and the `selectionClear` at the first click of a line, rectangle, circle, arc and Bezier.
  - Redo and the zone-fill actions refused while started.
  - `POLYGON_GEOM_MANAGER::DeleteLastCorner`, `NewPointClosesOutline` and `AddPoint`'s two locked points in 45 mode.
  - `endPolygon`'s `PopTool` for zones and polygons.
- **Locked groups (`3853fa1d`):** `BOARD_ITEM::IsLocked`'s parent-group chain, and every `format()` writing `IsLocked()`: segments, vias, zones, shapes, texts, text boxes, tables, barcodes, images, dimensions, footprints, groups and generators.
- **Cleanup merge at a zone (`45dcab1a`):**
  - `ZONE::HitTest`'s `max( accuracy, 0.1 mm )`, with corner `2 * accuracy` and edge `accuracy`.
  - `testMergeCollinearSegments`' flag logic: three flagged points or a flagged joint refuse; `testTrackEndpointIsNode` only fires with both ends tested, so it never refuses a plain merge.
- **Merge Zones (`a4f1a061`):** `TestZoneIntersection`'s `IterateSegmentsWithHoles` and `IterateWithHoles` with `Contains`; the shared-layer test `( GetLayerSet() & GetLayerSet() ).any()`.
- **Track & Via short question (`ca34d02e`):** `confirmShortingNets`' two messages, "Confirmation", "Apply Anyway" / "Cancel Changes".

---

# Part B: rows no round reviewed

### B5-1 (high). Draw Via (Ctrl+Shift+X) only places a via within 0.5 mm of a pad, via or track end, at the click point and with that item's net: a free via, a stitching via in a pour, a via on a track's middle are refused; no snap, no track split, no DRC check, always through
**Fixed** on task-r5-pcb-edit: 9d3a3b9d (`kicad-port/viaPlacer.ts`, `crates/ops/src/via_place.rs`). Left: `(free yes)` (no flag in the IR), an arc track is not split.
- **What differs:**
  - **Ours.** `web/components/canvas/Canvas.tsx:726-737`:
    ```ts
    const anchor = findRouteAnchor(board, sx, sy, ANCHOR_SNAP_NM);
    if (!anchor) { dispatch({ type: "TOAST", message: "Click a pad, via, or track end -- a via needs a net.", kind: "error" }); return; }
    ...
    api.cmd({ op: "add_via", net: anchor.net, x: sx, y: sy, drill: viaPreset.drill, diameter: viaPreset.diameter, from_layer: "F.Cu", to_layer: "B.Cu" });
    ```
    - `findRouteAnchor` (`web/components/canvas/routing.ts:141-166`) knows only pad centres, vias and track **ends**, within `ANCHOR_SNAP_NM` = 0.5 mm (`Canvas.tsx:115`).
    - The via goes at the click (`sx, sy`), not at the anchor (`anchor.at` is ignored).
    - `add_via` (`crates/ops/src/lib.rs:3807-3815`) needs a known net (`known_net`).
  - **KiCad.** `pcbnew/tools/drawing_tool.cpp::DRAWING_TOOL::DrawVia`, `VIA_PLACER`:
    - `SnapItem` snaps to the nearest track under the via (`m_gridHelper.AlignToSegment( position, trackSeg )`), else a pad's position (`aItem->SetPosition( pad->GetPosition() )`), else a graphic.
    - `PlaceItem` takes the net from that track, pad or copper graphic. Else it takes the net of the zone fill under it (`findStitchedZoneNet`: `z->HitTestFilledArea( layer, position )`, with a "Select Net:" popup when several nets are there). Else no net: `return NETINFO_LIST::ORPHANED;` and `via->SetIsFree( via->GetNetCode() > 0 )`.
    - It refuses a via that violates DRC: `m_frame->ShowInfoBarError( _( "Via location violates DRC." ), true, WX_INFOBAR::MESSAGE_TYPE::DRC_VIOLATION ); if( !m_allowDRCViolations ) return false;` (`checkDRCViolation`: clearance and hole clearance to every item near it, keepouts with `GetDoNotAllowVias`, `DISALLOW_CONSTRAINT`).
    - It splits the track it lands on:
      ```cpp
      if( track && m_gridHelper.GetSnap() )
      { ...
          track->SetEnd( viaPos );
          PCB_TRACK* newTrack = dynamic_cast<PCB_TRACK*>( track->Clone() );
          newTrack->SetStart( viaPos );
          newTrack->SetEnd( trackEnd );
          aCommit.Add( newTrack );
      ```
    - `CreateItem` uses `bds.m_CurrentViaType` (through, blind/buried or micro: the active layer and the route layer pair; a microvia takes the net class's µvia sizes).
- **How to see it:**
  1. A GND pour on F.Cu, filled. Draw Via, click in the middle of the pour.
     - KiCad: a GND stitching via.
     - Ours: the toast "Click a pad, via, or track end -- a via needs a net.", and nothing placed.
  2. Click 0.4 mm beside the end of a 0.2 mm track of N1 that runs next to an N2 pad.
     - KiCad: snapped onto the track's line, split there, and refused with "Via location violates DRC." if it would touch N2.
     - Ours: an N1 via 0.4 mm off the end, possibly not touching the track and possibly touching N2: a short no check stops.
  3. Click on the middle of a track: KiCad places it on the track and splits it; ours refuses (no end within 0.5 mm).
  - Tests: a `kicad-port/viaPlacer.ts` port of `SnapItem` and `PlaceItem`'s net choice, with `node --test` cases for each of the three. A Rust `add_via` with an empty net (a free via) must be accepted.
- **Severity:** high: wrong connectivity, no DRC guard, and the tool's main use (stitching vias in a pour) is impossible.
- **Already known?** No. PARITY-pcb has no row for the Draw Via tool. The router's Place Via (V while routing) is a different action.

### B5-2 (medium). The board's Text tool places the text where it was first clicked and goes back to Select; KiCad carries it to a second click and stays armed. A new text ignores the class's width and italic
**Fixed** on task-r5-pcb-edit: dfd330bb (`place_text`, the carried text). Left: the canvas does not draw the bottom justification.
- **What differs:**
  - **Ours.** `web/components/canvas/Canvas.tsx:939-943`: the click opens `SET_TEXT_DIALOG { mode: "add", at: [sx, sy] }` and at once sets the Select tool. `TextDialog.tsx:71-72` adds the text at `dialog.at`. `newTextStyle` (`web/kicad-port/layerDefaults.ts:86-89`) gives one size (the class's height) and no italic, and `add_text` has no width or italic.
  - **KiCad.** `drawing_tool.cpp::DRAWING_TOOL::PlaceText`:
    - It clears the selection when the tool starts.
    - The first click builds the text from `bds.GetTextSize( layer )` (width and height), `GetTextThickness`, `GetTextItalic`, `GetTextUpright`, `m_Mirrored = IsBackLayer`, `GR_TEXT_H_ALIGN_LEFT`, `GR_TEXT_V_ALIGN_BOTTOM`, then opens `DIALOG_TEXT_PROPERTIES`.
    - The text then follows the cursor (`text->SetPosition( cursorPos )` on motion) until the next click commits "Draw Text" and selects it, unless the dialog moved it: `else if( text->GetTextPos() != cursorPos ) { // If the user modified the location then go ahead and place it there. placing = true; }`.
    - The loop goes on, so the tool stays. Escape drops a carried text, and a second Escape leaves.
  - The footprint editor's Text tool was brought to this in round 4 (E4-F2); the board's was not.
- **How to see it:** set F.SilkS text to 1.5 x 1.0 mm, italic, in Board Setup. Pick Draw Text, click at (10,10), type "A", OK, then move the mouse.
  - KiCad: the text, 1.5 wide x 1.0 high and italic, follows the cursor until a click; the tool is still armed.
  - Ours: a 1.0 x 1.0 upright text fixed at (10,10), and the Select tool.
  - Test: a browser check `pcb text tool carries and stays` and a `node --test` case for `newTextStyle` returning width and italic.
- **Severity:** medium.
- **Already known?** In part. `PARITY-fpedit.md:64` records "Not modelled: bottom justification, italic, keep-upright" for the footprint editor. The board tool's single click, its leaving the tool and the class width are not recorded anywhere.

## Cleanup Graphics (`pcbnew/graphics_cleaner.cpp`, `dialogs/dialog_cleanup_graphics.cpp`; ours `web/kicad-port/pcbGlobalEdit.ts`, `web/actions/pcbGlobalEditSweep.ts`, `web/components/PcbGlobalEditDialogs.tsx`)

### B5-3 (medium). The cleaner's 0.5 um tolerance became exact equality with the move to nanometres: near-duplicates and near-zero graphics are kept
**Fixed** on task-r5-pcb-edit: 4e6628bf.
- **What differs:**
  - **Ours.** `web/kicad-port/pcbGlobalEdit.ts:216-217`: `const same = (a, b) => Math.abs(a[0] - b[0]) < 0.5 && Math.abs(a[1] - b[1]) < 0.5;`, commented "`m_DRCEpsilon` = 0.5 um: on integer micrometres, equal". The file header (`:10`) still says "Units: micrometres". The shapes are nm now (`api/types.ts:598-605`, `Nm`; `pcbGeom.ts:29`), so `same` is exact equality. The arc branch of `areEquivalent` already uses `< 500` (`:252`).
  - **KiCad.** `GRAPHICS_CLEANER::CleanupBoard`: `m_epsilon = m_commit.GetBoard()->GetDesignSettings().GetDRCEpsilon();`, which is `ADVANCED_CFG::m_DRCEpsilon = 0.0005; // 0.5um`, 500 nm. Then:
    ```cpp
    bool equivalent( const VECTOR2I& a, const VECTOR2I& b, int epsilon )
    { return abs( a.x - b.x ) < epsilon && abs( a.y - b.y ) < epsilon; };
    ```
    This is used by `isNullShape` (segment, rectangle, arc, Bezier) and by `areEquivalent`.
- **How to see it:** three F.SilkS segments of one width: (0,0)-(10 mm,0), then (300 nm,0)-(10 mm,200 nm), then (0,0)-(400 nm,0). Run Delete redundant graphics.
  - KiCad: "Remove duplicated graphic" and "Remove zero-size graphic".
  - Ours: nothing.
  - Test: `pcbGlobalEdit.test.ts`, `cleanup graphics uses KiCad's 500 nm epsilon`. Expect `["duplicate", "null"]`.
- **Severity:** medium.
- **Already known?** No.

### B5-4 (medium). Fix discontinuities in board outlines replaces each healed Edge.Cuts shape with a new one (id, group, lock, line style lost), and Merge lines into rectangles then never sees the healed lines
**Fixed** on task-r5-pcb-edit: 4e6628bf.
- **What differs:**
  - **Ours.** `pcbGlobalEdit.ts:302-310` takes `healShapes(edge, tol)`'s `remove`/`add` as they come, and `pcbGlobalEditSweep.ts:72` sends `delete_shape` + `add_shape`. The new shape copies only `layer`, `stroke_width` and `filled`. The healed ids are then marked deleted, so `mergeRects` (`:317`, `:331`) skips them.
    - Edit > Heal Shapes on the same shapes does keep ids, through `modifyResultCmds`/`reshapeCmds`; this path bypasses that.
  - **KiCad.** `GRAPHICS_CLEANER::fixBoardOutlines` edits the shapes in place (`shapeList.push_back( shape ); if( !m_dryRun ) m_commit.Modify( shape ); ... ConnectBoardShapes( shapeList, m_outlinesTolerance );`). `CleanupBoard` then runs `mergeRects()` on the healed coordinates (`if( aFixBoardOutlines ) fixBoardOutlines(); if( aMergeRects ) mergeRects();`).
- **How to see it:** put four Edge.Cuts lines in a group, making a 20 x 10 mm rectangle with 0.1 mm gaps at the corners. Tick both options.
  - KiCad: one `gr_rect`.
  - Ours: four new, ungrouped lines with new uuids, and no rectangle.
  - Test: the plan's healed lines are reshapes of the same ids; with `mergeRects` there is one `rect`.
- **Severity:** medium.
- **Already known?** No.

### B5-5 (low). A merged rectangle takes only the stroke width; KiCad copies the whole stroke
**Not a bug** here: board graphics have no line style or colour in this model (written `(type solid)`), so the width is the whole stroke.
- **What differs:**
  - **Ours.** `pcbGlobalEdit.ts:350`: `{ kind: "rect", layer, stroke_width: top.shape.stroke_width, filled: false, ... }`.
  - **KiCad.** `GRAPHICS_CLEANER::mergeRects`: `rect->SetStroke( top->shape->GetStroke() );`, which carries the width, the line style and the colour.
- **How to see it:** merge four dashed lines. KiCad's `gr_rect` is `(type dash)`; ours is solid.
- **Severity:** low.

### B5-6 (low). Text boxes are left out of Delete redundant graphics
**Open** (recorded in GAPS item 8).
- **What differs:**
  - **Ours.** `pcbGlobalEditSweep.ts:63` passes only `board.drawings?.shapes`.
  - **KiCad.** The cleaner walks `board->Drawings()` with `dynamic_cast<PCB_SHAPE*>( *it )`, and a `PCB_TEXTBOX` is a `PCB_SHAPE`. So a zero-size text box is "Remove zero-size graphic", and a text box whose rectangle, layer and border width equal an earlier one is "Remove duplicated graphic", whatever its text.
- **How to see it:** two text boxes with the same corners and different text. KiCad removes the second; ours keeps both. Match it, or record the deviation.
- **Severity:** low.

### B5-7 (low). Cleanup Graphics: a stale PARITY row and two dialog differences
**Fixed** in part on task-r5-pcb-edit: 4e6628bf (Update PCB always runs) and 5720d03e (the stale row). Left: the change list is not clickable (GAPS item 8).
- `web/studio/PARITY-pcb.md:622` still says Cleanup Graphics is "missing ... out of scope". It is implemented (`planCleanupGraphics`, `CleanupGraphicsDialog`), and no row describes the port.
- Dialog (`web/components/PcbGlobalEditDialogs.tsx:92-117`):
  - "Update PCB" is disabled when nothing is ticked. KiCad's OK always works, and `doCleanup( false )` then pushes nothing.
  - The changes list shows one line per kind with a count. KiCad's `RC_TREE_MODEL` lists each item, and a click selects it (`OnSelectItem`).
- **Severity:** low.

## Create from Selection and Outset (`pcbnew/tools/convert_tool.cpp`, `item_modification_routine.cpp::OUTSET_ROUTINE`; ours `crates/ops/src/convert.rs`, `web/kicad-port/pcbConvert.ts`, `web/actions/pcbConvertSweep.ts`)

### B5-8 (medium). A rounded rectangle loses its corner radius in Create Polygon / Zone / Rule Area, Create Lines / Tracks and Outset
**Fixed** on task-r5-pcb-edit: fb658ba2.
- **What differs:**
  - **Ours.** Each of these treats a rectangle as four sharp corners:
    - `crates/ops/src/convert.rs:187` (`closed_ring`);
    - `web/kicad-port/pcbConvert.ts:140` (`sourceRings`, for lines and tracks);
    - `pcbConvert.ts:382` (`const radius = params.roundCorners ? Math.max(d, 0) : 0;`).
  - The stated reason, "the IR rectangle has none" (`convert.rs:17`, `pcbConvert.ts:381`), is out of date. The model keeps `ShapeStyle::corner_radius` (`crates/model/src/pcb_items.rs:351`), and the TS `Shape` has `corner_radius?` (`api/types.ts:601`).
  - **KiCad, `CreateLines`.** `SHAPE_RECT rect( graphic->GetStart(), graphic->GetEnd() ); ROUNDRECT rrect( rect, graphic->GetCornerRadius(), true ); ... rrect.TransformToPolygon( poly, graphic->GetMaxError() );`
  - **KiCad, `makePolysFromClosedGraphics`.** `shape->TransformShapeToPolygon(...)`, which rounds the corners.
  - **KiCad, `OUTSET_ROUTINE::ProcessItem`.** `int cornerRadius = pcb_shape.GetCornerRadius(); if( m_params.roundCorners ) cornerRadius = std::max( cornerRadius + m_params.outsetDistance, 0 );`.
- **How to see it:** a 10 x 5 mm rectangle with a 2 mm radius.
  - Create Polygon: KiCad's corners are rounded; ours has 4 vertices.
  - Create Lines: KiCad gives 4 segments and 4 arcs; ours gives 4 segments.
  - Outset 1 mm, rounded: KiCad's radius is 3 mm; ours is 1 mm.
  - Tests: `convert.rs` `polys_from_items_keeps_a_rounded_rectangles_corners`, and `pcbConvert.test.ts` for the lines and the outset.
- **Severity:** medium.

### B5-9 (medium). Create Zone / Rule Area from Selection refuses a non-copper active layer; KiCad makes a non-copper zone there
**Fixed** on task-r5-pcb-edit: fb658ba2.
- **What differs:**
  - **Ours.** `web/actions/pcbConvertSweep.ts:66-67`: `if (kind !== "poly" && !board.layers.includes(layer)) { toast("Zones and rule areas are made on copper layers: choose a copper layer first.", "error"); return; }`. The message is our own.
  - **KiCad.** `CONVERT_TOOL::CreatePolys`:
    ```cpp
    bool nonCopper = IsNonCopperLayer( destLayer );
    ...
    else if( nonCopper )
        ret = InvokeNonCopperZonesEditor( frame, &zoneInfo, &m_userSettings );
    ```
  - Non-copper zones exist in the studio (P13; the zone tool draws them).
- **How to see it:** select a closed polygon on F.SilkS, with F.SilkS active, and Create Zone from Selection. KiCad opens Non-copper Zone Properties; ours shows the toast.
- **Severity:** medium.

### B5-10 (medium). Create Zone / Rule Area from Selection takes several undo steps; KiCad's is one ("Convert to Zone")
**Fixed** on task-r5-pcb-edit: fb658ba2 (the zones and the removal held in one step, `carry`).
- **What differs:**
  - **Ours.** `web/components/ZoneDialog.tsx:225-230` adds each zone with `await api.addZone(...)`, which `store.tsx:2518-2541` makes a `runCmd(add_zone)` plus a separate `runBatch(follow)`. The originals are then deleted by a separate `api.cmdBatch(convert.deleteCmds)`.
  - **KiCad.** `CreatePolys` puts every zone in `commit.Add( zone )` and every original in `commit.Remove( item )`, then makes one `commit.Push( _( "Convert to Zone" ) )` ("Create Zone" when the originals are kept).
- **How to see it:** convert two closed rectangles with Delete source objects ticked. In KiCad one Undo restores the board; ours needs three or more.
- **Severity:** medium (undo steps).

### B5-11 (medium). Create Lines and Create Tracks from a source with no width use the wrong default width
**Fixed** on task-r5-pcb-edit: fb658ba2.
- **What differs:**
  - **Ours.**
    - Lines: the layer class's line thickness (`pcbConvertSweep.ts:138`).
    - Tracks: the current track width (`pcbConvertSweep.ts:175`).
  - **KiCad.** `CreateLines`' `addGraphicChain` / `addTrackChain` set a width only `if( aWidth && *aWidth > 0 )`. Otherwise the constructor's default stays:
    - `PCB_SHAPE`'s `pcbIUScale.mmToIU( DEFAULT_LINE_WIDTH )`, where `DEFAULT_LINE_WIDTH` is 0.10 (`board_design_settings.h:45`);
    - `PCB_TRACK`'s `m_width = pcbIUScale.mmToIU( 0.2 );` (`pcb_track.cpp:68`).
- **How to see it:** Create Lines from a filled zone on Edge.Cuts: KiCad's lines are 0.1 mm, ours 0.05 mm. Create Tracks from a filled polygon with a 0.3 mm current width: KiCad's tracks are 0.2 mm, ours 0.3 mm.
- **Severity:** medium (a wrong default).

### B5-12 (low). "Copy line width of first object" orders an arc by its start, not its centre
**Fixed** on task-r5-pcb-edit: fb658ba2.
- **What differs:**
  - **Ours.** `pcbConvert.ts:67-75` (`shapePosition`): `case "arc": ... return pt(s.start)`.
  - **KiCad.** `CreatePolys` uses `candidate->GetPosition()`, which is `EDA_SHAPE::getPosition` (`if( m_shape == SHAPE_T::ARC ) return getCenter();`).
- **How to see it:** an arc with its start at (0,5), its centre at (10,0) and a 0.5 mm width, closing a loop with a 0.2 mm segment from (2,0). KiCad copies 0.2 mm; ours copies 0.5 mm.
- **Severity:** low.

### B5-13 (low). Create Polygon chains ends that meet exactly; KiCad joins ends within 100 nm
**Fixed** on task-r5-pcb-edit: fb658ba2.
- **What differs:**
  - **Ours.** `convert.rs:48` says it ("here only exact meetings chain"), and the `Chainer` keys points exactly (`convert.rs:264`).
  - **KiCad.** `makePolysFromChainedSegs`: `int chainingEpsilon = 100; // max dist from one endPt to next startPt in IU`, used by `findInsertionPoint` and the closing test (`closeEnough( outline.GetPoint( 0 ), outline.GetPoint( -1 ), chainingEpsilon )`).
- **How to see it:** a triangle with one end 50 nm off (common after a DXF import). KiCad makes the polygon; ours says "Could not convert selection" / "Objects must form a closed shape".
- **Severity:** low.
- **Already known?** Only in the code comment, not in GAPS.

### B5-14 (low). Create Arc from a segment: the mid point can be 1 nm off
**Fixed** on task-r5-pcb-edit: fb658ba2.
- **What differs:**
  - **Ours.** `pcbConvert.ts:260-268` uses `Math.trunc((A+B)/2)` for the centre and an unrounded `0.1 * hypot` for the offset.
  - **KiCad.** `SegmentToArc`: `mid = seg->Center() + ( seg->B - seg->A ).Perpendicular().Resize( offsetRatio * seg->Length() )`. Here `SEG::Center()` is `A + ( B - A ) / 2`, and `Resize` takes an `int`, so the offset is truncated.
- **How to see it:** `arcMidOfSegment([0,0],[10000005,0])` should be `[5000002, 1000000]`.
- **Severity:** low.

### B5-15 (low). Outset's default line width is a fixed 0.05 mm, not the board's Edge.Cuts thickness
**Fixed** on task-r5-pcb-edit: fb658ba2.
- **What differs:**
  - **Ours.** `pcbConvert.ts:317`: `lineWidthNm: 50_000`.
  - **KiCad.** `CONVERT_TOOL::OutsetItems`: `outset_params_pcb_edit { pcbIUScale.mmToIU( 1 ), true, true, true, Edge_Cuts, frame.GetDesignSettings().GetLineThickness( Edge_Cuts ), std::nullopt, false }`.
- **Severity:** low.

### B5-16 (low). Create Tracks has a dialog and a delete flag of its own, and forces a net
**Open** (recorded in GAPS item 8).
- **What differs:**
  - **Ours.** Our own "Create Tracks from Selection" dialog (copper layer, net, delete originals) and a remembered flag of its own (`pcbConvertSweep.ts:31`). The net is required by `commit_route` (`known_net`), which is noted only in a code comment.
  - **KiCad.** `CreateLines` has no dialog. It uses the active layer if that is copper, else `SelectOneLayer( F_Cu, LSET::AllNonCuMask() )`. The tracks have no net, and the flag is the shared `m_userSettings.m_DeleteOriginals`.
- **Severity:** low. Record it in GAPS if it is kept.

### Part B: checked and found to match (do not re-review)
- **Cleanup Graphics:**
  - The labels "Remove zero-size graphic", "Remove duplicated graphic" and "Convert lines to rectangle".
  - `CleanupBoard`'s order, with the outline fix skipped in the dry run.
  - `isNullShape` (circle radius 0, polygon with no points, the Bezier in every non-degenerate case).
  - `areEquivalent`: kind, layer and width first; then the points per kind; polygons never; no reversed match.
  - `cleanupShapes` keeps the first of a pair.
  - `mergeRects`: the `SIDE_CANDIDATE` swap, the `ptMap` order, `viable`, the left/top/right/bottom search, and the rectangle from `top->start` to `bottom->end`, unfilled.
  - The 2 mm tolerance and the remembered options; one "Cleanup Graphics" step, with the selection cleared.
- **Create from Selection:**
  - `convertAvailability` against `CONVERT_TOOL::Init`, and `initUserSettings`' defaults.
  - `resolvedSettings`; the three strategies, including bounding hull's union and inflation.
  - `SKIP_STRUCT` for Delete source objects; the error messages "Could not convert selection", "Resulting polygon would be empty" and "Objects must form a closed shape".
  - `makePolysFromChainedSegs`' walk; the dialog's labels and "Conversion Settings"; `GetBoardItemWidth`.
  - `SegmentToArc`'s kinds. The three-point arc is rightly preferred over `CalcArcAngles`, which goes the long way round.
  - `OUTSET_ROUTINE::ProcessItem` (circles, segments, arcs with radius >= d, grid rounding outwards) and its two messages.
- **Draw Via:** nothing matches beyond the via size, which is the current via preset (`bds.GetCurrentViaSize()`). See B5-1.
- **Board Text tool:** the class's height and thickness, mirrored on a back layer, left aligned, and an empty text dropped (`NoPrintableChars`).

---

# Summary

| # | Finding | Severity |
|---|---|---|
| A5-1 | Teardrops: an arc track at a via or round pad gets none (or a stunted one); its first tessellation step is taken as the track | high |
| A5-2 | A rectangle is constrained by the line mode (default 45), so it comes out flat or square; KiCad leaves it free, and Ctrl gives a square | high |
| B5-1 | Draw Via: only near a pad, via or track end, placed at the click with that net; no free or stitching vias, no snap, no track split, no DRC refusal, always through | high |
| A5-3 | 45-degree line mode: length-preserving rotation at 22.5 degrees instead of `GetVectorSnapped45`; Ctrl does not free it | medium |
| A5-4 | The studio starts in 45-degree mode; KiCad's default is free angle | medium |
| A5-5 | Closing a zone or polygon: no leader or loop corners in 45/90 mode, no `Simplify` (`ZONE_CREATE_HELPER::OnComplete`) | medium |
| A5-6 | A graphic polygon emptied by Delete: the next Delete deletes the selection, which the first corner never cleared | medium |
| B5-2 | Board Text tool: placed at the first click, the tool left, the class width and italic ignored | medium |
| B5-3 | Cleanup Graphics: the 0.5 um epsilon is exact equality in nm | medium |
| B5-4 | Cleanup Graphics: the outline fix replaces shapes (id, group, style lost), and the merge into rectangles misses them | medium |
| B5-8 | Convert and Outset ignore a rectangle's corner radius | medium |
| B5-9 | Create Zone / Rule Area refused on a non-copper layer | medium |
| B5-10 | Create Zone from Selection takes N+1 undo steps | medium |
| B5-11 | Create Lines / Tracks default widths (0.1 mm / 0.2 mm in KiCad) | medium |
| A5-7 | Stale teardrop PARITY rows and test comment | low |
| B5-5 | A merged rectangle keeps only the stroke width | low |
| B5-6 | Text boxes left out of Delete redundant graphics | low |
| B5-7 | Cleanup Graphics: stale PARITY row; OK disabled; grouped change list | low |
| B5-12 | Copy line width: an arc ordered by its start, not its centre | low |
| B5-13 | No 100 nm chaining tolerance in Create Polygon | low |
| B5-14 | Create Arc mid point 1 nm off | low |
| B5-15 | Outset's default width fixed at 0.05 mm | low |
| B5-16 | Create Tracks: own dialog, own delete flag, net forced | low |

Counts: high 3, medium 11, low 9.

All the round-4 fixes in section 58 were re-read against the C++. Apart from what A5-1, A5-5, A5-6 and A5-7 add, they are right; Part A's "checked" list gives the detail.
