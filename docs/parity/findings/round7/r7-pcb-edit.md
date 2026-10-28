# Findings round 7: PCB editing and its UI (`r7-pcb-edit`)

Reviewer `r7-pcb-edit`. Findings only: no code was changed, nothing was built, no server was started (ports 8842-8843 unused).
- **Code read:** main's checkout `/home/team_braneboard_com/work/agentic-eda`, HEAD `f057b4ea`.
- **KiCad:** `8303b2ad` (`$KICAD_SRC_DIR`), and `25ec2335` (`~/src/kicad-src-25ec2335`) where marked. The installed kicad-cli is 25ec2335.
- **Paths:** `web/` means `web/studio/src/`.
- **Scratch:** `/tmp/findings-r7-pcb-edit.4jzp`. `td/td.kicad_pcb` is the kicad-cli teardrop case for R7-1 (after `pcb drc --refill-zones --save-board`, it holds kicad-cli's three teardrops).

Scope:
- **(a) The round-6 merges** (all dated 2026-10-27, so `git log --since=2026-10-28` lists none of them): eee102c0 (E6-1, `clears_routing_in`), 205a320c (Swap, Select/Grab Unconnected, net selection, group locks), b2fe2c35 (stroke step, polygon fill, InferBold, A6-4), 1d029c43 (teardrops on 25ec2335), 4ab0c643 (Draw Via), 810fd552 (Create Zone dialog), 475fee76 (the lows).
  - I also checked every `Cmd` that `clears_routing_in` still answers `true` for against every studio path that could send it.
- **(b) Rows no round had reviewed:**
  - Fillet / Chamfer / Dogbone / Extend Lines (`EDIT_TOOL::ModifyLines`, `item_modification_routine.cpp`);
  - Change Track Width / Via Size (`EDIT_TOOL::ChangeTrackWidth`);
  - the Draw Text Box tool's text attributes.

---

## Part A: the round-6 merges

### R7-1 (medium). Teardrops: a track whose end lies inside a via or round pad but off its centre gets no teardrop, nor does a track that crosses one. kicad-cli 25ec2335 gives both.
- **What differs.**
  - **Ours.** `crates/connectivity/src/teardrop.rs:874-881` (`generate_teardrops`) keeps a track only when its first or last point *equals* the anchor's centre (`if first == anchor.center ... else if last == anchor.center ... else { continue; }`). The module doc (`:41-46`) justifies this: "this model's router and hand-drawn tracks always land exactly on a pad/via center".
    - That claim does not hold for imported boards.
    - Nor does it hold for a via dropped onto a track with Shift (Draw Via does not split then, `viaPlacer.ts` `viaNet`'s `track: input.snap ? ... : null`), which leaves the track running straight through the via.
  - **KiCad 25ec2335** `pcbnew/teardrop/teardrop.cpp::TEARDROP_MANAGER::UpdateTeardrops` takes the track's connected pads and vias (`connectivity->GetConnectedPadsAndVias( track, &connectedPads, &connectedVias )`). It tests each end by hit-testing the shape, not by equality: `bool startHitsVia = via->HitTest( track->GetStart() ); bool endHitsVia = via->HitTest( track->GetEnd() );`. A track that crosses the anchor gets two teardrops:
    ```cpp
    // As for pads, a track that merely crosses the via earns a teardrop on each side.
    if( !startHitsVia && !endHitsVia && track->HitTest( via->GetPosition() ) )
    {
        PCB_TRACK stub( m_board );
        buildCrossingStub( stub, track, via->GetPosition() );
        stub.SetStart( track->GetEnd() );
        tryCreateTrackTeardrop( ..., &stub, track, via, via->GetPosition(), teardropUuid( track, via, 0 ) );
        stub.SetStart( track->GetStart() );
        tryCreateTrackTeardrop( ..., &stub, track, via, via->GetPosition(), teardropUuid( track, via, 1 ) );
    }
    ```
    The pad loop has the same code (`// A track crossing the pad earns one teardrop per side, not one over the whole track.`).
  - Round 6 (D6-1) moved the filters to 25ec2335 but kept the exact-centre test. So `computeChordThroughShape`'s graze filter, which is now ported, only ever sees radial tracks.
- **How to see it.** I ran this with kicad-cli (`/tmp/findings-r7-pcb-edit.4jzp/td/td.kicad_pcb`, the PARAMS of `teardrops_kicad_cli.rs`):
  - A 0.8 mm via at (100, 100) and a 0.2 mm segment (100.1, 100) -> (105, 100), which starts 0.1 mm off the centre. kicad-cli makes one teardrop, `(100.796578 100.1) (100.796578 99.9) (100.153073 99.630448) (99.995578 100) (100.153073 100.369552)`. Ours makes none.
  - A 0.8 mm via at (120, 100) and a segment (115, 100) -> (125, 100) running through it. kicad-cli makes two teardrops, one on each side. Ours makes none.
  - Test: add a third table to `crates/kicad/tests/teardrops_kicad_cli.rs` for off-centre ends, `(100.0, 100.0, 0.8, 100.1, 100.0, 105.0, 100.0, 0.2)`, and crossings, `(120.0, 100.0, 0.8, 115.0, 100.0, 125.0, 100.0, 0.2)` (expecting 2 outlines). Today the count check `kicad.len() == ours.len()` fails.
  - Unit test: `a_track_ending_off_centre_inside_the_via_gets_its_teardrop`.
- **Severity.** medium. A teardrop zone is missing where KiCad, rebuilding the same board, makes one. It is common on imported boards.
- **Already known?** No. GAPS items 8 and 13 list "round anchors only" and "one segment only". The module doc lists the exact-centre test as a deliberate scope, but GAPS does not, and the 25ec2335 crossing case is not mentioned anywhere.

### R7-2 (medium). Swap moves zones by their first corner and never swaps their layers. KiCad's `ZONE::SetPosition` is a no-op, so a zone stays where it is and swaps layers with another zone.
- **What differs.**
  - **Ours.** `web/kicad-port/pcbSwap.ts:46-51` gives a zone `pos` = its first corner (`ZONE::GetPosition`, which is right) and `layer: null`. `planSwap` then sends `move_items` for the zone (`:80-81`) and never a layer change for it.
  - **KiCad 8303b2ad** `pcbnew/zone.h:116`: `void SetPosition( const VECTOR2I& aPos ) override {}`. The same holds in 25ec2335, `zone.h:112`. So in `EDIT_TOOL::Swap` (`edit_tool_move_fct.cpp:175-178`, `a->SetPosition( aPos ); b->SetPosition( bPos );`) a zone does not move. The item it is paired with does move, to the zone's first corner.
  - Two zones are "two objects of the same type", so their layers swap: `else if( a->Type() == b->Type() && a->Type() != PCB_GROUP_T ) { ... a->SetLayer( aLayer ); b->SetLayer( bLayer ); }`. Here `ZONE::SetLayer` is `SetLayerSet( LSET( { aLayer } ) )` (`zone.cpp:550-553`).
- **How to see it.** Two single-layer pours: GND on F.Cu at x 0..10, VCC on B.Cu at x 20..30. Select both and press Alt+S.
  - KiCad: both outlines stay where they are; GND is now on B.Cu and VCC on F.Cu.
  - Ours: both outlines trade places by their first corners, and the layers are unchanged.
  - A zone with a via: KiCad moves only the via, onto the zone's first corner. Ours moves both.
  - Test (`pcbSwap.test.ts`): `planSwap(board_with_two_zones, ["z1", "z2"])` contains no `move_items`, and contains a layer change of each zone (a `set_zone_layers`-style verb, or `edit_zone` with `layer` / `layers`) to the other's layer.
- **Severity.** medium (a wrong result: zones moved that KiCad leaves in place).
- **Already known?** No. GAPS item 8's "Missing (round 6)" lists arc tracks, dimensions, groups and others, not zones. Round 6 E6-2 added zones as movable.

### R7-3 (medium). A text box drawn with the Draw Text Box tool is never bold. KiCad's tool gives it `m_textAttrs`, which `InferBold` set, so a copper text box at the defaults is bold.
- **What differs.**
  - **Ours.** `web/kicad-port/pcbTextBox.ts:381-404` (`newTextBox`) passes the class's size, glyph width, thickness, italic and mirror, but no `bold`. `boardTextBoxOf` then defaults it to `false` (`:370`). Round 6 A6-3 added `inferBold` to the Text tool only (`layerDefaults.ts:90-104`, used by `TextDialog.tsx`).
  - **KiCad 8303b2ad** `pcbnew/tools/drawing_tool.cpp`:
    - `drawShape` sets `m_textAttrs` and runs `InferBold( &m_textAttrs )` when the layer changes (`:2376-2378`) and on `layerChanged` (`:2510-2512`). `Reset` does the same (`:321-323`).
    - It then does `pcb_textbox->SetAttributes( m_textAttrs );` (`:2532`, `:2574`) on the box being drawn.
    - `InferBold` (`include/gr_text.h:82-89`) is `aAttrs->m_Bold = abs( penSize - GetPenSizeForBold( textSize ) ) < abs( penSize - GetPenSizeForNormal( textSize ) );`.
- **How to see it.** With the default Board Setup, draw a text box on F.Cu, type "X", OK, and save.
  - KiCad writes `(font (size 1.5 1.5) (thickness 0.3) (bold yes))`, and the dialog opens with Bold checked.
  - Ours has no `bold` and the dialog's Bold box (`PcbItemDialogs.tsx:230`) is unchecked.
  - Test: `newTextBox(board, "F.Cu", ...).bold === true` and `newTextBox(board, "F.SilkS", ...).bold === false` (1.0 / 0.1 mm is not bold).
- **Severity.** medium (the file differs; the dialog's initial state differs).
- **Already known?** No. A6-3 named `drawing_tool.cpp:323, 2378, 2512` but its fix covered `PlaceText` only.

### R7-4 (low). The via check judges a knockout text by the convex hull of its strokes. KiCad's knockout shape is the text's rotated bounding rectangle.
- **What differs.**
  - **Ours.** `crates/ops/src/via_place.rs:188-201` (`board_text_effective_shape`, round 6 C6-3). It grows each stroke end by `pen/2 + margin` in 16 directions, then takes `convex_hull`. The doc comment (`:155-158`) cites `buildBoundingHull` as "the convex hull of the strokes grown by ...".
  - **KiCad 8303b2ad** `pcbnew/pcb_text.cpp::PCB_TEXT::buildBoundingHull` (:582-607; 25ec2335 :759 is the same) builds a rectangle, not a hull:
    ```cpp
    SHAPE_POLY_SET poly( aRenderedText );
    poly.Rotate( -GetDrawRotation(), GetDrawPos() );
    BOX2I    rect = poly.BBox( aClearance );
    ... corners[0..3] of rect ...
    RotatePoint( corner, GetDrawPos(), GetDrawRotation() );
    ```
    `TransformTextToPolySet` calls it with `margin + aClearance` (:652-654), and `GetEffectiveShape` returns that polygon for a knockout text (:535-542).
- **How to see it.** A knockout F.Cu text "T" (1.5 mm), and a via of another net placed just below the right end of the T's bar, near the corner of the text's box but outside its strokes' hull. KiCad refuses with "Via location violates DRC."; ours places the via.
  - Test: `via_place::tests::a_knockout_text_is_judged_by_its_bounding_rectangle`. Put the via inside the box's corner and outside the hull, and assert `VIA_VIOLATES_DRC`.
- **Severity.** low (needs knockout copper text and a via near a corner of its box).
- **Already known?** No (C6-3's fix introduced it).

## Part B: rows no round reviewed

### R7-5 (medium). Change Track Width / Via Size (`changeTrackWidth`) uses the cached track width and via size, not `GetCurrentTrackWidth()` / `GetCurrentViaSize()`. This is C6-2's defect, left in this action.
- **What differs.**
  - **Ours.** `web/actions/pcbEditSweep.ts:233-234`:
    ```ts
    const width = state.currentTrackWidthNm ?? rules?.track_width ?? 250_000;
    const via = state.currentViaPreset ?? { diameter: rules?.via_diameter ?? 600_000, drill: rules?.via_drill ?? 300_000 };
    ```
    Both values are snapshots taken when an entry was picked (`pcbMenuActions.ts:53-80`, `useActionRunner.ts:455, 1097, 1128`). Draw Via (round 6 C6-2) and the router (R5-9) were moved to `currentViaSize` / `chosenTrackWidthOf` (`routerVia.ts:199`, `routerSizes.ts:58`), which read the board's list by index. This action was not; r6's C6-2 noted it in passing as "outside this slice". `microwaveActions.ts:60` has the same track-width snapshot.
  - **KiCad 8303b2ad** `pcbnew/tools/edit_tool.cpp::EDIT_TOOL::ChangeTrackWidth` (:1383-1453) asks for the size each time:
    - a via gets `new_width = board()->GetDesignSettings().GetCurrentViaSize(); new_drill = ...GetCurrentViaDrill();`, or the net class's micro-via size for a `MICROVIA`;
    - a track or arc gets `GetCurrentTrackWidth()`.
    - `GetCurrentViaSize` returns the custom size, else `m_ViasDimensionsList[ m_viaSizeIndex ]`, else the Default class's via (`board_design_settings.cpp`). `GetCurrentTrackWidth` does the same with the track list.
- **How to see it.**
  1. Pick track width entry 2 (0.3 mm) from the size menu.
  2. In Board Setup > Pre-defined Sizes, change that row to 0.4 mm, then OK.
  3. Select a track and run Change Track Width.
  - KiCad gives 0.4 mm; ours gives 0.3 mm. The same happens with via sizes, and after "Use Net Class Values" followed by an edit of the Default class.
  - Test: extract `changeTrackWidthCmd(board, state)` and assert it uses `chosenTrackWidthOf(board.routing.track_width_presets, index, ...)` and `currentViaSize(board, index, ...)` over stale snapshots, as `routerVia.test.ts` does for Draw Via.
- **Severity.** medium (a wrong size written on the board).
- **Already known?** Only as a side remark in r6 C6-2; there is no finding or GAPS entry.

### R7-6 (low). Fillet / Chamfer / Dogbone / Extend Lines: the residual differences are KiCad quirks only (listed so nobody re-reviews them)
These are recorded as matching. The two points below are KiCad oddities that are not worth porting:
- **Fillet.** `setIfPointOnSeg` writes into `t1newPoint` / `t2newPoint`, which start at `(0, 0)` (`item_modification_routine.cpp:141-171`). When both arc ends land on the same segment, KiCad moves the other line's shared end to `(0, 0)`. Ours counts that pair as a failure (`pcbModify.ts:222-225`, `if (!t1 || !t2)`). It needs a radius larger than the corner allows, and KiCad's result is garbage; this is a deliberate difference, not a defect.
- **Extend.** KiCad compares `int` distances (`const int dist_start = ( *intersection - aSeg.A ).EuclideanNorm();`), while ours uses floating `norm` (`pcbModify.ts:290-291`). They differ only on an exact tie after truncation.

---

## Checked and found to match (do not re-review)
- **E6-1 / `Cmd::clears_routing_in` (eee102c0).**
  - `SwapChain`, `MoveExact` and the item verbs answer `false`. `PlaceAt` answers `false` only for a part with no pose. `Batch` and `InGroup` recurse with the board.
  - The CLI's `step` reads `clears_routing_in(&was)` (`crates/cli/src/board.rs:1596`), and its `routing.take()` (`:1684`) is the only place a step drops the routing.
  - Every studio path to the verbs that still clear was checked; none sends one:
    - `place`, `place_edge`, `place_region`, `move_to`, `nudge`, `rotate`, `swap` and `flip` (of a part) are sent nowhere in `web/` (grep; they appear only in `api/types.ts`).
    - `place_at` is sent only by `placeArmedAt` (`store.tsx:2436-2442`). `SET_ARMED` is dispatched only from the unplaced list (`PropertiesPanel.tsx:32`, `.filter((p) => !p.placed)`) and the chooser's "-- Unplaced --" group (`PcbParityDialogs.tsx:368-389`, `!p.placed`).
    - The Pack and Move / part drop sends `move_items`, `rotate_items` and `flip_items` (`store.tsx:2393-2412`).
    - Move Exactly sends `move_items` / turns through `planMoveExact` (`MoveExactDialog.tsx:103-111`). `api.moveExact` (`store.tsx:2709`) has no caller.
    - The server has no other route that builds these `Cmd`s (grep of `crates/cli/src`: only the CLI verbs and `cmd_line`).
  - So no studio edit can wipe the routing now. The Strict gate's refusal of an edit that breaks a complete net remains, and is known (GAPS line 441).
- **Swap (205a320c), apart from R7-2.**
  - The filter order (markers, free pads to footprints, `PCB_TRACE_T` out, locked out with group inheritance) and `< 2` doing nothing.
  - The pairwise `swap(sorted[i], sorted[i+1])` of positions and of the layers of same-kind items, in selection order.
  - `EDA_SHAPE::getPosition` (arc centre, first polygon vertex, else start) and `setPosition` as a move by the delta.
  - `PCB_TEXT` layer swap through `edit_text` (`lib.rs:4690`, which keeps the glyph width when the size is unchanged).
  - 25ec2335 adds only `m_selectionTool->ReportFilteredLockedItems()` after the filter. 8303b2ad, the baseline for Swap, has no message.
- **Stroke step (A6-1, A6-4).**
  - `strokeStepKept` on arming (`useActionRunner.ts:667`, Bezier `:2250`).
  - The layer change while a stroke tool runs (`store.tsx:1518`); the polygon tool (`DrawZone`) neither reads nor resets it.
  - Board Setup's `DRAWING_TOOL::Reset` (`store.tsx:1663`; `Reset` at drawing_tool.cpp:302-319 resets `m_stroke` and `m_layer`).
  - incWidth / decWidth only with `STROKE_TOOLS` armed.
  - Matches `m_layer` / `m_stroke` semantics in every sequence I traced: step, change layer with no tool, return, re-arm.
- **Graphic polygon (A6-2).** `graphicPolygonStyle`: unfilled on Edge.Cuts / F.CrtYd / B.CrtYd, `GetLineThickness( layer )`, no `m_stroke` (`zone_create_helper.cpp:236-251`).
- **InferBold (A6-3) for the Text tool.** `inferBold` is `KiROUND( min(w,h) / 5 )` vs `/ 8` with a strict `<` (gr_text.cpp:37-64, gr_text.h:82-89). Bold comes from the class's size and thickness, not from what is typed in the dialog, as `PlaceText` infers it before the dialog.
- **Teardrops (1d029c43), apart from R7-1.** 25ec2335's `m_TdMaxWidth > 0` guard, `BestWidthRatio` / `WidthtoSizeFilterRatio`, "entirely inside", and the chord filter, as ported.
- **Draw Via (4ab0c643).**
  - `shownBy` matches `VIEW::Query`'s `!( *i )->visible` skip: a plated pad shows on its hole layers, everything else by copper layer. The backend's `hidden_layers` mirrors it, and vias are always shown.
  - `currentViaSize` matches `GetCurrentViaSize` (custom, then the list by index, then the Default class), for the via and for the ghost.
  - `NO_NET` "<no net>" is 25ec2335's `netinfo.h`.
  - A non-knockout text is judged by its strokes with glyph width, valign and bold.
- **Create Zone / Rule Area from Selection (810fd552).**
  - One dialog, titled "Convert to Copper Zone" / "Convert to Non Copper Zone" / "Convert to Rule Area".
  - The box holds "Use centerlines", "Create bounding hull", "Gap:" and "Delete source objects after conversion", and has no line width.
  - COPY_LINEWIDTH becomes CENTERLINE for zones. The settings are remembered (`rememberZoneConversion`). `getPolys` runs after OK; nothing is done when the result is empty; it is one undo step (`convert_tool.cpp::CreatePolys` :509-584).
- **Select / Grab Unconnected from pads (E6-4)** match `selectUnconnected` / `grabUnconnected` (`pcb_selection_tool.cpp:2342-2420`: footprints' pads, selected pads, `sourceParent == pad` / `targetParent == pad`, only a pad at the far end).
- **Select All Tracks in Net (E6-5).** `netsOfItems` takes pads, shapes and zones; `netItems` takes tracks, vias and copper shapes.
- **`ModifyLines` (part B).**
  - The selection filter: segment / poly / rect, locked items out.
  - Rectangle corners in `start, (end.x, start.y), end, (start.x, end.y)` order, the polygon's vertices with the closing line, and the sources removed.
  - `segmentCount` and both refusals word for word: "Exactly two lines must be selected to extend them." and "A shape with at least two lines must be selected.".
  - `for_all_pairs` over the real segments, then the decomposed lines, skipping `STRUCT_DELETED`.
  - `GetSharedEndpoints`' A.A, A.B, B.A, B.B order; the parallel skip (fillet: no failure; dogbone: a failure).
  - Chamfer's "share an endpoint" test. Dogbone's bisector sample `min( 1000, radius )`, `Contains`, and the cap segments added before the arc.
  - Extend's `Intersects`, then `IntersectLines`, `Contains`, the furthest end, and `GetClampedCoords( .., 200 mm )`.
  - `ModifyLineOrDeleteIfZeroLength`; modified lines edited in place through `reshapeCmds` (`shapeFill.ts:93-118`).
  - The four `GetStatusMessage` texts, including the dogbone's " Consider enabling the 'Add Slots' option." / " Slots were added.".
- **ChangeTrackWidth's selection** (tracks, arcs and vias, locked items out) and its one commit match; only the sizes differ (R7-5).

---

## Convergence: what a user of this area still notices in daily work, ranked
1. **Teardrops.**
   - Only round anchors (no SMD rectangle / round-rect pads, though KiCad's `m_UseRoundShapesOnly` is off by default), one segment, and global settings rather than per-pad parameters (known, GAPS 8 and 13).
   - Plus R7-1: off-centre ends and crossing tracks.
   - `computeFittedTeardropPolygon` (shrinking near other nets) is not ported (known).
   - This is the most visible difference: any board with SMD pads gets far fewer teardrops than KiCad gives it.
2. **Strict (on by default) refuses** an edit that breaks a complete net (move, swap, delete). KiCad never refuses (known, GAPS line 441).
3. **Swap** of zones (R7-2), of two footprints' orientation and side inside a mixed selection, and of arcs, dimensions, groups and other kinds (known, round 6 "Missing").
4. **Sizes from stale snapshots** in Change Track Width / Via Size (R7-5). The user sees it right after editing the pre-defined sizes.
5. **Bold:** text boxes on copper are not bold (R7-3), and the text dialog has no Bold control (known).
6. **Draw Via edge cases:** a knockout text's box (R7-4), PTH unflashed layers, text boxes, tables and dimensions on copper (C6-4, C6-5, known), and the message with Allow DRC violations on (C6-6, known).
7. **Cosmetic only:** the via ghost is not drawn at `SnapItem`'s point (C6-7); the Select Net menu's order (C6-8); R / F / E on a carried text (A6-6).

Matching now, said plainly:
- **The routing is safe.** No studio edit clears it any more.
- **Drawing defaults match.** The stroke-width step, the line modes, polygon close and fill, and the Text tool's style match KiCad.
- **Selection operations match.** Lock, Select / Grab Unconnected, Select Net and Same Sheet match.
- **Draw Via's main path matches:** snapping, net choice, split, hidden layers and size.
- **Create from Selection and its zone dialog match.**
- **Line modification matches.** Fillet, Chamfer, Dogbone and Extend Lines match, apart from KiCad quirks.

Reuse rule: no new active duplicate of kicad-cli or a linked C++ library. The teardrop generator still duplicates `TEARDROP_MANAGER` (already recorded, and accepted because the studio stores teardrops as zones); R7-1 is a defect inside it.

## Summary

| # | Finding | Severity |
|---|---|---|
| R7-1 | Teardrops: a track ending off-centre inside a via/round pad, or crossing it, gets none; kicad-cli 25ec2335 makes one (or two for a crossing) | medium |
| R7-2 | Swap moves zones by their first corner and keeps their layers; KiCad's `ZONE::SetPosition` is a no-op and two zones swap layers | medium |
| R7-3 | Draw Text Box: no `InferBold`, so a copper text box at the defaults is not bold | medium |
| R7-5 | Change Track Width / Via Size uses the cached width and via size, not `GetCurrentTrackWidth` / `GetCurrentViaSize` | medium |
| R7-4 | Via check: a knockout text is its strokes' convex hull, KiCad's is the rotated bounding rectangle | low |
| R7-6 | ModifyLines: only KiCad quirks left (fillet's `(0,0)` end, extend's int distances); recorded as matching | low |

Counts: high 0, medium 4, low 2.
