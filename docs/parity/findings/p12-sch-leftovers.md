# Findings: P12 board outline leftovers and schematic connectivity leftovers (`p12-sch-leftovers`)

Reviewer: `p12-sch-leftovers`, findings only. Main's checkout at `be562f2e` (`/home/team_braneboard_com/work/agentic-eda`), binary `target/debug/eda` (built Oct 23 11:08).
KiCad: `$KICAD_SRC_DIR` = 8303b2ad. kicad-cli on the box is 10.99.0-25ec2335, and its `convert_shape_list_to_polygon.cpp` differs from 8303b2ad (813 diff lines; see O12).
Scratch: `/tmp/findings-p12-sch-leftovers.pjob/` (`b/<board>/` QA runs, `syn/<case>/` hand-written boards, `scripts/`, `sch/` for part 2).

Part 1 is the board outline (GAPS item 27, P12; `PARITY-pcb.md` section 25). Part 2 is schematic connectivity (`PARITY-sch.md` sections 39 and 40).

---

## Part 1: the board outline

### How it was measured

- **QA boards.** 31 boards under `$KICAD_SRC_DIR/qa/data/pcbnew` have a non-trivial outline: arcs, circles, polygons, curves, footprint Edge.Cuts or several items. Boards whose outline is a single `gr_rect` are left out. The list is `/tmp/findings-p12-sch-leftovers.pjob/list.txt`.
- **Per board** (`scripts/run1.sh`), on a scratch copy:
  - `kicad-cli pcb drc --format json --severity-all`, for its `invalid_outline` findings;
  - `kicad-cli pcb export stats --format json`, with and without `--subtract-holes-from-board`. `GetBoardPolygonOutlines( false )` gives `has_outline` and the area of the outlines. The difference between the two runs, less the drill holes, is the area of the cutouts.
  - `eda import-kicad`;
  - `eda board serve` on ports 8826 and up (`scripts/poly.sh`). Its `/api/state` gives our `outline_polys` and `outline_errors`.
  - `eda board drc` (`scripts/rt.sh`): kicad-cli's DRC on the board as **we export it**. This is the round trip.
- **Comparison.** Our outer and hole areas are set against KiCad's (`scripts/compare.py`).
- **Hand-written boards.** 14 boards cover the cases the QA set lacks (`scripts/syn.py`, `scripts/syncmp.py`, results under `syn/`).

### O1. A `gr_curve` whose control point equals an end point is dropped by the reader; the slot it closes opens, and the curve is lost on export (high)

**What differs.**
- `poly_points` (`crates/kicad/src/import.rs:2411-2418`) drops a point equal to the one before it:
  ```rust
  fn push(pt: Point, out: &mut Vec<Point>) { if out.last() != Some(&pt) { out.push(pt); } }
  ```
  `graphic_shape`'s `gr_curve` arm (`import.rs:2121-2124`) then needs exactly four points (`let [start, c1, c2, end] = pts[..] else { return None };`).
- So a Bezier with `c2 == end` or `c1 == start` disappears. This is very common: KiCad's curve tool leaves a control point on the end until it is dragged.
- `gr_curve` is in `READ_ITEMS` (`import.rs:333-336`), so the item is not kept as raw text either. It is gone from the design and from every export.
- KiCad reads all four points as they are: `pcb_io_kicad_sexpr_parser.cpp::parsePCB_SHAPE`, `case T_pts` for `SHAPE_T::BEZIER`, then `SetBezierC1` / `SetBezierC2`.
- The same path reads the footprint Edge.Cuts that `footprint_edge_cuts` promotes (`import.rs:2155`), every board-level curve on other layers, and custom-pad primitives (`pad_shape.rs:125`).
- The Footprint Editor's reader (`footprint_import.rs:128-132`, `xy_points`) does not drop the point, so this is the board reader only.

**How to see it.**
- QA `issue23658`: a slot made of 2 curves and 3 lines, one curve `(xy 120.8 64.2) (xy 121.1 64.5) (xy 121.1 64.5) (xy 121.1 64.5)`.
  - kicad-cli: `has_outline` true, a 22.29 mm² cutout, no `invalid_outline`.
  - Ours: 5 of the 6 Edge.Cuts items are in `design.json`, and the live outline reports `(not a closed shape)` at (120.95, 64.35) mm. The outline falls back to the rectangle round the edges (`valid = false`), so the zone fill is not clipped to it.
  - Round trip: kicad-cli on our export says `Board has malformed outline (not a closed shape) (gap 0.3743 mm)`.
- QA `issue24078`: 2 of its 3 curves are kept. KiCad's outline is valid with a 20.46 mm² cutout. Ours is "not closed", the inferred rectangle.
  - `PARITY-pcb.md` section 25 says the two "agree (malformed)" on this board. They do not: KiCad's finding is `(self-intersecting)` from `TestBoardOutlinesGraphicItems`, with a valid outline.
- `syn/E_degenerate_curve`: the same result on a 4-item slot.
- Test to write: `crates/kicad/src/import.rs::a_curve_with_a_control_point_on_its_end_is_kept`. Import a board with `(gr_curve (pts (xy 0 0) (xy 0 0) (xy 3 2) (xy 3 0)) (layer "Edge.Cuts"))` and assert one `Shape::Bezier` with `c1 == start`.
- The fix: don't deduplicate for `gr_curve` (dedup is only right for `gr_poly`).

**Unit-sensitive:** no.

### O2. A footprint's Edge.Cuts are taken out of the footprint: our outline differs where KiCad's footprint pass matters, and the exported file is a different board to KiCad (high)

**What differs.**
- `footprint_edge_cuts` (`import.rs:2140-2169`) turns every `fp_*` Edge.Cuts item into a board-level shape. The writer emits it as a `gr_*` item. The footprint keeps none: QA `issue22826`'s card-edge connector J1 has 11 `fp_` Edge.Cuts items in the original and 0 in our export, where 32 `gr_` items stand.
- KiCad's `convert_shape_list_to_polygon.cpp::BuildBoardPolygonOutlines` first builds each footprint's own Edge.Cuts. When the footprint has copper outside them, or several contours, it keeps them apart as holes and subtracts them at the end, with no error reported:
  ```cpp
  success = doConvertOutlineToPolygon( fpSegList, fpOutlines, aErrorMax, aChainingEpsilon, false,
                                       nullptr, // don't report errors here; the second pass also
                                                // gets an opportunity to use these segments
                                       aAllowUseArcsInPolygons, cleaner );
  if( success && ( isCopperOutside( fp, fpOutlines ) || fpOutlines.OutlineCount() > 1 ) )
      fpHoles.Append( fpOutlines );
  ...
  fpHoles.Simplify();
  aOutlines.BooleanSubtract( fpHoles );
  ```
- Ours sends them all through the one nesting pass (`crates/drc/src/outline.rs:15-18` says so). The two agree only while the footprint contour lies wholly inside the board.

**How to see it.**
- `syn/A_fp_notch_on_edge`: a 50×40 mm board, and a footprint with an `fp_rect` 6×4 mm notch across the top edge and its pad outside the notch.
  - kicad-cli: `invalid_outline (self-intersecting)` once (from `TestBoardOutlinesGraphicItems`). But `has_outline` is true, with an area of 1988 mm²: the notch is cut out.
  - Ours: the outline is invalid (3 × `(self-intersecting)`). We use the 2104.6 mm² rectangle round the edges, so the zone fill is not clipped and the 3D slab is the box.
  - kicad-cli on our export: 3 × `(self-intersecting)`, and KiCad's outline is now invalid too. The file means something else to KiCad.
- `syn/B_fp_contour_outside`: the footprint's closed Edge.Cuts lies outside the board, with its pad outside it.
  - KiCad: one outline of 2000 mm², no error (the subtraction removes nothing).
  - Ours: **two** outlines (2024 mm²), a 24 mm² second board piece. It is drawn in 3D, and placement and routing gates treat it as board.
- QA `issue22826` and `issue14559` (slots inside the board) agree in area. Our export's DRC names "Segment on Edge.Cuts" where KiCad named "Segment of J1 on Edge.Cuts", and the slot no longer moves with J1 in KiCad.
- Tests to write:
  - `crates/drc/src/outline.rs::a_footprint_hole_across_the_board_edge_is_subtracted_without_error`, with the A case: assert `valid` and an area of 1988 mm².
  - `crates/kicad/src/pcb.rs::a_footprints_edge_cuts_are_written_inside_the_footprint`: the round trip of `issue22826` keeps 11 `fp_` Edge.Cuts items in J1.

**Unit-sensitive:** no.

### O3. A rectangle's corner radius is dropped (outline and file) (high; already listed as a gap, now measured)

**What differs.**
- `graphic_shape`'s `gr_rect` arm (`import.rs:2117`) does not read `(radius R)`, and the IR's `Shape::Rect` has no field for it.
- KiCad's `processClosedShape`, `case SHAPE_T::RECTANGLE`:
  ```cpp
  if( aShape->GetCornerRadius() > 0 )
  {
      ROUNDRECT rr( SHAPE_RECT( aShape->GetStart(), aShape->GetRectangleWidth(), aShape->GetRectangleHeight() ),
                    aShape->GetCornerRadius(), true /* normalize */ );
      ...
      rr.TransformToPolygon( poly, aShape->GetMaxError() );
  ```

**How to see it.**
- `syn/D_rounded_rect` (a 50×40 mm rectangle, `(radius 5)`): KiCad's area is 1978.535 mm², ours 2000. Our export has no `radius`, so kicad-cli then sees a sharp rectangle.
- QA `pns_regressions/boards/ultrasound.kicad_pcb` has one: KiCad's outline is 18678.535 mm², ours 18700.000 mm² (four 5 mm corners lost). kicad-cli on our export finds 2 `copper_edge_clearance` instead of 3, because the corners got sharper.

GAPS item 27 lists it ("A rectangle's corner radius ... not in the IR"). It belongs among the high ones, because the save silently changes the board.

**Unit-sensitive:** no.

### O4. A KiCad 10 `gr_ellipse` / `gr_ellipse_arc` on Edge.Cuts is not part of our outline (medium)

**What differs.**
- `gr_ellipse` is not in `READ_ITEMS`. It is kept as raw text: our export still carries it, so kicad-cli's verdict on the saved file is unchanged.
- But it is not in `edge_cuts_shapes`, so the live outline, zone-fill clip, router edges, 3D slab and placement gates ignore it.
- `eda import-kicad` says "not read (... they are not in the design nor in what is exported from it): gr_ellipse x1". The second half of that sentence is wrong for this item: the export does keep it.
- KiCad 25ec2335's `processClosedShape` has `case SHAPE_T::ELLIPSE` (`SHAPE_ELLIPSE::ConvertToPolyline( aErrorMax )`), and `processShapeSegment` has `case SHAPE_T::ELLIPSE_ARC`. This is the kicad-cli the project judges with. 8303b2ad has neither.

**How to see it.** `syn/F_ellipse_cutout`: an ellipse with radii 5 and 3 mm inside the rectangle. KiCad: 2000 mm² of outline, net 1952.92 mm² (the 47.1 mm² ellipse is a hole). Ours: no hole.

**Unit-sensitive:** no.

### O5. A board with no Edge.Cuts: no live finding, and no inferred outline (medium)

**What differs.**
- KiCad's `BuildBoardPolygonOutlines` starts with `bool success = false;`. With an empty `segList` it stays false, so `testOutline` reports:
  ```cpp
  drcItem->SetErrorDetail( _( "(no edges found on Edge.Cuts layer)" ) );
  ```
- KiCad's inference then falls back to the bounding box of everything on the board:
  ```cpp
  BOX2I bbbox = aBoard->GetBoardEdgesBoundingBox();
  // If null area, uses the global bounding box.
  if( ( bbbox.GetWidth() ) == 0 || ( bbbox.GetHeight() == 0 ) )
      bbbox = aBoard->ComputeBoundingBox( false, true );
  // Ensure non null area. If happen, gives a minimal size.
  if( ( bbbox.GetWidth() ) == 0 || ( bbbox.GetHeight() == 0 ) )
      bbbox.Inflate( pcbIUScale.mmToIU( 1.0 ) );
  ```
- Ours:
  - `check_edge_cuts` (`crates/drc/src/outline.rs:1122-1125`) returns nothing for no shapes;
  - `outline_errors_json` (`crates/cli/src/studio.rs:1491-1494`) returns `[]` unless there are Edge.Cuts shapes;
  - `build_board_polygon_outlines` (`outline.rs:874-892`) infers only from the Edge.Cuts' own box, and inflates a zero-area box by 1 mm without first trying the board's items (`:885`).
- The doc comment of `check_board_outline` says kicad-cli "has its own" message for this case. So the live view and the judge disagree by design here. The no-edges case only reaches this code when there is no `placement.outline` polygon either, which is the case for an imported board with no Edge.Cuts.

**How to see it.** `syn/C_no_edges` (one footprint, no Edge.Cuts). kicad-cli: `Board has malformed outline (no edges found on Edge.Cuts layer)`, `has_outline` false. Ours: `outline_polys` is empty, `outline_errors` is empty, and the import prints "outline: none".

Test to write: `crates/drc/src/outline.rs::no_edge_cuts_is_reported_as_kicad_reports_it`. `check_edge_cuts(&[])` should hold one error, "(no edges found on Edge.Cuts layer)", when the design has items.

**Unit-sensitive:** no.

### O6. DRC message and marker: no "(gap X)", and the marker is the handler's point, not `testOutline`'s (medium)

**What differs.**
- `OutlineError::describe` (`outline.rs:46-48`) gives `Board has malformed outline {message}`, and `at` is the handler's `pt`.
- `drc_test_provider_misc.cpp::testOutline` (8303b2ad and 25ec2335) rewrites both whenever the two items are shapes:
  ```cpp
  if( overlap.GetWidth() > 0 && overlap.GetHeight() > 0 )
  {   markerPos = overlap.Centre(); usedGap = false; }
  else
  {   ...
      if( effectiveShapeA->NearestPoints( effectiveShapeB.get(), ptA, ptB ) )
      {   gap = ( ptA - ptB ).EuclideanNorm(); markerPos = ( ptA + ptB ) / 2; usedGap = true; }
  }
  ...
  if( itemA && itemB && usedGap )
      drcItem->SetErrorDetail( wxString::Format( _( "%s (gap %s)" ), msg, MessageTextFromValue( gap ) ) );
  ```
  When neither item is a shape, `findClosestOutlineGap` (`drc_test_provider_misc.cpp:55-111`) looks for the culprits over all Edge.Cuts shapes.
- So the status bar, the problems list and `outline_errors` show a different text and point from kicad-cli for the same finding. Seen on the round trips:
  - `issue23658` export: kicad-cli says `(not a closed shape) (gap 0.3743 mm)`, ours `(not a closed shape)`;
  - `issue24078` original: kicad-cli says `(self-intersecting) (gap 1.4000 mm)`.

**How to see it.** `syn/J_gap_outside_eps` (a 0.02 mm gap): set our `outline_errors[0].message` against the kicad-cli description on the same file.

Test to write: `outline.rs::a_gap_is_reported_with_its_width`. The J case should describe as `... (not a closed shape) (gap 0.0200 mm)`, at the middle of the nearest points.

**Unit-sensitive:** the gap's value and the midpoint's rounding are (`(ptA + ptB) / 2` in nm). The missing text is not.

### O7. The outline is flattened at a fixed 5 µm, not at Board Setup's "Maximum allowed deviation" (medium)

**What differs.**
- `board_outline_of` (`outline.rs:909-914`) and the importer (`import.rs:2190`) pass `MAX_ERROR_UM` (`crates/model/src/outline.rs:29`, 5).
- KiCad's `BOARD::GetBoardPolygonOutlines` passes `GetDesignSettings().m_MaxError`. That is Board Setup > Constraints > "Maximum allowed deviation" (`panel_setup_constraints.cpp:133`, `rules.max_error`, file `(setup (max_error ..))`).
- We read it into `board.max_error_um` (`import.rs:1109-1110`) and edit it in `ConstraintsPage.tsx:111`, but the outline never sees it.
- Also, `.max(1)` clamps to 1 µm, where KiCad's minimum is 0.0001 mm (`board_design_settings.cpp:1027`: `pcbIUScale.mmToIU( 0.0001 )`).
- The zone fill is kicad-cli's, so this touches the live outline: the router's edge obstacles, the gates and the 3D slab.

**How to see it.** A board with `max_error 0.05` and a 10 mm-radius corner: KiCad's arc has the segment count `GetArcToSegmentCount( 10mm, 0.05mm, 90° )`, ours that of 5 µm.

Test to write: `outline.rs::the_board_max_error_flattens_its_arcs`.

**Unit-sensitive:** the clamp is. The constant is not.

### O8. A closed shape's ends are not chaining candidates: messages differ where a line ends on a circle's rim or a rectangle's corner (medium)

**What differs.**
- KiCad's `PCB_SHAPE_ENDPOINTS_ADAPTOR` (8303b2ad, `convert_shape_list_to_polygon.cpp:139-149`) holds `GetStart()` / `GetEnd()` of **every** shape in the list: a circle's centre and rim point, a rectangle's corners, a polygon's.
- `findNext`, and the two shapes `report_gap` names, therefore see closed shapes too.
- Ours, `Endpoints::new` (`outline.rs:368-376`), holds only open shapes' ends. The module doc admits this (`outline.rs:19`).

**How to see it.** `syn/G_line_to_circle_rim`: a line starting at a circle's `end` point.
- kicad-cli: `(self-intersecting)` [Segment, Circle] **and** `(not a closed shape)` [Circle, Segment].
- Ours: only `(not a closed shape)`.
- Both say the outline is invalid. Only the findings differ.

Test to write: `outline.rs::a_line_from_a_circles_rim_is_reported_as_kicad_does`.

**Unit-sensitive:** no.

### O9. The closing arc is polygonised again; KiCad's arcs have been cleared and it just moves the last point (low)

**What differs.**
- `convert` (`outline.rs:719-733`): when the last item is an arc that ends within the epsilon of the first point, the arc is flattened again so that it ends on that point (`contour.last_arc`). `PARITY-pcb.md` section 25 describes this as KiCad's behaviour ("the closing arc is polygonised again to end on the first point").
- In KiCad, `processShapeSegment` has already done `if( !aAllowUseArcsInPolygons ) arcChain.ClearArcs();` before `aContour.Append( arcChain )`. Every board-outline caller passes `false` there, so `currContour.IsArcEnd(...)` is false and the `else` branch runs:
  ```cpp
  currContour.SetPoint( -1, currContour.CPoint( 0 ) );
  ```
- The vertices of the last arc differ, by up to the 10 µm epsilon.

**How to see it.** A square with one rounded corner whose arc ends 4 µm off the first line's start (the line first in the file). Assert that the contour's points equal the arc's own polyline, with only its last point moved.

**Unit-sensitive:** partly (the size of the move).

### O10. Placement and the placement verbs use the outline's bounding box (high, already listed; confirmed)
- **Fixed in part** on task-fix-f-editing: b1eebd75 (the placement verbs test the real outlines with their cutouts). The annealing placer (`crates/place`) still works from the box.

**What differs.**
- `crates/place/src/lib.rs:972-988` takes the intent's `model.board.outline` (or `opts.outline`) and works in its bbox.
- `crates/ops/src/lib.rs:3158-3166` (`board_bbox`) is used by `place_at` (`:3419-3429`, "that position puts the part's courtyard on or past the board edge") and four other verbs (`:3193, :3231, :3268, :3316`). It works from `placement.outline`, the summary ring: the **largest** outline only, with no holes.
- So a part can be placed in a cutout, and a board of two separate pieces (`syn/H_two_boards`, which matches KiCad's 3600 mm²) cannot get a part on its smaller piece.
- KiCad's autoplacer works from the real polygon: `ar_autoplacer.cpp:96` `m_board->GetBoardPolygonOutlines( m_boardShape, true );` and `m_topFreeArea = m_boardShape;`. `autoplace_tool.cpp:135` does the same.

**How to see it.**
- `Cmd::PlaceAt` of a 2×2 mm part into the middle of `issue12609`'s cutout succeeds.
- Test to write: `crates/ops/src/tests.rs::place_at_refuses_a_cutout`.

**Unit-sensitive:** no.

### O11. `placement_within_outline` tests only the courtyard's four corners (low; project gate)

`crates/gates/src/pcb.rs:274-279` tests `r.corners().iter().all(|c| region.inside(*c))`. A cutout wholly inside a courtyard (a mounting hole under a large part), or a notch that crosses one side between two corners, passes. KiCad has no such gate, so this is not a parity item, but the gate's claim "a courtyard ... in a cutout ... fails" holds only at the corners.

Test to write: `gates::pcb::a_courtyard_over_a_cutout_fails` (a round 3 mm cutout centred under a 10×10 mm courtyard).

**Unit-sensitive:** no.

### O12. Our port follows 8303b2ad's chaining; the judge, kicad-cli 25ec2335, chains differently (medium; context for every outline verdict)

25ec2335's `convert_shape_list_to_polygon.cpp` changes:
- `findNext` became `findNeighbours`, a `radiusSearch` that returns the nearest **available** and the nearest **consumed** shape. A chain closes when `chain.size() > 1 && close_enough( prev, chainPt, ... )`, whatever shape was found. `(self-intersecting)` needs a consumed neighbour.
- The arc direction is chosen by `closer_to_first( aPrevPt, pend, pstart )`, not "start first".
- `free_end()` picks the start point.
- Seeds come in list order.
- `findCrossingContours` replaces `hasOverlappingClosedContours`: only the crossing contours take the boolean path, and "A nested contour crossing another is a cutout wall, parent parity lies for it".
- `report_gap` searches 8 neighbours for a second, different shape.
- Ellipses (O4).

None of these changed a verdict on the 31 QA boards (every disagreement there is O1, O2 or O3). They can on T-junctions, arcs shorter than 10 µm, and nested contours that cross. `PARITY-pcb.md` section 25 says kicad-cli "stays the judge", so the port should either follow 25ec2335 or say which one it follows.

**Unit-sensitive:** no.

### O13. Rounding to the micrometre shows in the messages and areas (low; the nm change fixes it)

- `syn/M_tiny_segment` (a 0.0005 mm segment): kicad-cli says `(segment has null or very small length: 500 nm)`. Ours says `1000 nm`: the reader rounds 0.5 µm to 1 µm, and `nm = |um| um * 1000` (`outline.rs:941`).
- Areas against kicad-cli: `issue8909` outer 23186.197 vs 23186.278 mm², holes 70.210 vs 70.197; `issue7325` holes 253.711 vs 253.679; `issue5093` 9690.471 vs 9690.455.
- The circle radius is rounded to µm (`model/src/outline.rs::circle_radius`).
- `bezier_polyline`'s fallback of "10 when max_error <= 0" (`model/src/bezier.rs:213`) was 10 nm in the C++ and is 10 µm here.

**Unit-sensitive:** yes, all of these.

### Known, re-confirmed from the code, not re-measured

- The Freerouting DSN boundary is `placement.outline` (`crates/freeroute/src/design.rs:294-299`), with no cutouts. KiCad's `specctra_export.cpp::fillBOUNDARY` (`:1021-1055`) takes every outline of `GetBoardPolygonOutlines( m_brd_outlines, true )` and makes each hole a signal-layer `keepout` ("emit a signal layers keepout for every interior polygon left..."). (GAPS 27; high for the autorouter.)
- NPTH holes are not cut from the 3D slab. `web/studio/src/components/viewer3d/scene.ts` has no drill at all. KiCad's `board_adapter.cpp:1029` calls `GetBoardPolygonOutlines( m_board_poly, true, nullptr, false, true )`, the last `true` being `aIncludeNPTHAsOutlines`, which does `aOutlines.BooleanSubtract( hole )` per NPTH pad (`board.cpp:3298-3318`). (GAPS 27; low, a picture.)
- The arcs of a `gr_poly` are flattened by the reader (`poly_points`, `import.rs:2427-2434`) and written back as points. The geometry is kept to 5 µm, but the item changes. (GAPS 27; low.)
- "Fix discontinuities in board outlines" (Update PCB from Schematic) has nothing behind it; there is no outline-specific editor.
- From reading, not run: a plain intent polygon (`placement.outline`, no Edge.Cuts shapes) is never checked live. `outline_errors_json` (`studio.rs:1492`) returns `[]` before calling `check_board_outline`, and `board_outline` (`outline.rs:896-905`) returns the ring as valid. A self-intersecting or zero-length side would get `invalid_outline` from kicad-cli on the export but no live finding. Medium if intents can carry such polygons; I did not check whether intent validation prevents them.

### Checked and found to match (do not re-review)

**Geometry on the QA boards** (kicad-cli 25ec2335 against ours; same outline count, same holes, area within 0.1 mm²):
- `api_kitchen_sink`, `connect`, `creepage`, `issue11814`, `issue12609` (3 holes), `issue14559` (fp_ Edge.Cuts slots), `issue17429`, `issue18203`, `issue21482`, `issue21746`, `issue22475`, `issue22826` (fp card-edge slot), `issue23053`, `issue5093`, `issue5102`, `issue5830`, `issue5854` and `issue7267` (fp_rect hole), `issue5990`, `issue6260`, `issue7325` (13 holes), `issue832`, `issue8883`, `issue8909` (fp_circle and 5 circles, 6 holes).
- `issue18839` (one arc that does not close): both report `(not a closed shape)` naming the arc twice, and both infer the box.

**`invalid_outline` on the round trip** (kicad-cli on the original against kicad-cli on our export) is the same on every QA board except `issue23658` (O1). `issue24078`'s finding changes from `(self-intersecting)` to `(not a closed shape)` (O1).

Two of those exports get more `copper_edge_clearance` findings than the originals. Neither comes from the outline:
- `issue22475`: +1, from track arcs on In1.Cu (B1_OUT) whose lengths shift by 0.1 µm after the round trip. That looks like the track-arc µm rounding (unit-sensitive) and is for the router/IO reviewer.
- `issue22826`: +1, an Edge.Cuts arc against the GND zone fill. It may follow from O2 (the slot is no longer J1's). I did not take it further.

**Hand-written cases that match:**
- `B2` (an NPTH footprint whose Edge.Cuts circle holds its pad: a hole in both);
- `H` (two separate outlines, 3600 mm², valid);
- `I` (a 5 µm gap closes in both);
- `J` (a 20 µm gap is `(not a closed shape)` in both);
- `K` (an arc that closes within the epsilon: valid in both, 1994.629 vs 1994.634 mm²);
- `L` (a footprint whose Edge.Cuts is the whole board outline: 2000 mm², valid in both).

**From reading, ported as KiCad has it** (8303b2ad):
- `close_enough`, `closer_to_first`, `rescale`;
- `findNext` as "the two nearest, the nearer that is not the shape itself, strictly within ε²";
- `processShapeSegment` for lines, arcs and Bezier curves (the start-point choice, skipping a repeated point, `aPrevPt = nextPt`);
- `processClosedShape` for a polygon, a sharp rectangle and a circle starting at `center + (r, 0)`;
- the start point (`first->GetStart()` against the second's ends);
- `report_gap`'s pair key;
- `buildContourHierarchy` (`PointInside( firstPt, 0, true )`, even-odd, bbox prefilter);
- `addOutlinesToPolygon` / `addHolesToPolygon`, including the boolean path;
- `checkSelfIntersections` (the lexicographic sort and the norm-based break);
- `TestBoardOutlinesGraphicItems`: its four size messages word for word, the 0.001 mm minimum, closed contours plus `buildChainedClosedContour` loops, the overlap-box centre;
- `testOutline`'s 0.05 mm `maxError`, ε = 0.01 mm (`DEFAULT_CHAINING_EPSILON_MM`), and `(no edges found on Edge.Cuts layer)` when building fails with no handler call;
- the `Simplify()` at the end of `GetBoardPolygonOutlines`;
- a footprint's Edge.Cuts turned into board space (`RotatePoint`, a tilted `fp_rect` as a polygon).

**Board Setup:** the "Edge Cuts" line-thickness default of Text & Graphics (`kicad-port/boardSetupRules.ts:277, 294`, `layerDefaults.ts`) and "Copper to edge clearance" with its legacy −0.01 mm flag (`boardSetupRules.ts:238-239`) are there.

### Coverage note

- `pns_regressions/boards/stickhub-extra-via` (605.284 vs 605.286 mm²) and `vme-wren` (35820.488 vs 35820.45 mm²) match too, with the same DRC on the round trip.
- `plugins/allegro/.../VCU118_REV2-0`: kicad-cli finds no `invalid_outline`, and our import builds a valid 58-point outline. Its kicad-cli `stats` (over 5 minutes) and our serve were not finished, so its area is not compared.

---

## Part 2: schematic connectivity

The leftovers named in `web/studio/PARITY-sch.md` section 39 ("Left", line 1181) and `crates/cli/src/sch_hierarchy_tests.rs:1105-1112` (`QA_TRACED_OTHERWISE`).
KiCad is quoted from `$KICAD_SRC_DIR` (8303b2ad); the same code is in 25ec2335, the tree of the box's kicad-cli 10.99 (`generateBusAliasMembers`, `processSubGraphs`, the bus-parents pass and `propagateToNeighbors` are the same apart from one priority clause).

How it was measured: the QA projects copied to `/tmp/findings-p12-sch-leftovers.pjob/sch/`; `kicad-cli sch export netlist --format kicadsexpr` on each copy; ours from `target/debug/eda import-kicad <root>.kicad_sch -o out_<x>`, comparing `design.json`'s `nets` pin sets (nets of two pins or more, `#` references left out, as `qa_traced` does). The comparison script is `/tmp/findings-p12-sch-leftovers.pjob/cmp.py`.
On `netlists/video` this gives exactly the "20 of 371" of the slow test. On `bus_connection` and `issue17771`, `eda import-kicad` also loses the bus aliases (finding S4), so for those the aliases were written inline in scratch copies (`bcx/`, `i17x/`: `{a_xyz}` -> `{x y z}` and so on). kicad-cli's nets are the same either way.

### S1. Two buses on one sheet that share a member name are not joined (high)

**What differs.** In KiCad, every **bus label** on a sheet makes one virtual net subgraph per net member, named by the member's full local name (`test.x`, `DQ0`). It is a strong driver. Two such virtual subgraphs with the same name on one sheet merge, and that merged net then has two bus parents. A later pass renames the matching member of every parent bus to one final name, and then renames every subgraph that still carries the old name. The result: two buses that never touch, on one sheet, are one net per shared member name, with no wire or net label needed.

Our tracer makes "one node per member of a bus and of every bus joined to it" (`crates/engine/src/nets.rs:427-482`, the node pushed at `:450` and joined at `:474-478`). A member node is joined only to (a) the members of buses that are already one bus, through wires, local bus labels of the same name, or the hierarchy, and (b) real nets on the sheet whose label or global name equals the member name (`local` / `global_here`, `:475`). Two separate buses that share a member name get two member nodes, and nothing unites them. The case only works when a real net carrying that name happens to sit on the sheet (`issue17771`'s explicit `W.X.Y.Z1` label is why Z1 matches and Z2 does not).

KiCad, `eeschema/connection_graph.cpp::generateBusAliasMembers` (1762-1818):
```cpp
for( CONNECTION_SUBGRAPH* subgraph : m_driver_subgraphs )
{
    SCH_ITEM_VEC vec = subgraph->GetAllBusLabels();
    for( SCH_ITEM* item : vec )
    {
        ...
        dummy.ConfigureFromLabel( label->GetShownText( &subgraph->m_sheet, false ) );
        for( const auto& conn : dummy.Members() )
        {
            // Only create subgraphs for NET members, not nested buses
            if( !conn->IsNet() )
                continue;
            wxString name = conn->FullLocalName();
            CONNECTION_SUBGRAPH* new_sg = new CONNECTION_SUBGRAPH( this );
            // This connection cannot form a part of the item because the item is not, itself
            // connected to this subgraph.  It exists as part of a virtual item that may be
            // connected to other items but is not in the schematic.
            ...
            new_sg->m_is_bus_member = true;
            new_sg->m_strong_driver = true;
```
`GetAllBusLabels` (`connection_graph.cpp:364-388`) takes only label drivers (`SCH_LABEL_T`, `SCH_GLOBAL_LABEL_T`, `SCH_HIER_LABEL_T`) whose connection is `BUS` or `BUS_GROUP`. A bus named only by a sheet pin makes no virtual members.

Then `processSubGraphs` (2178-2240) works on candidates on the same sheet. Two nets with equal `Name( true )` merge ("absorbs neighbor"). A bus whose member name equals a net's name gets that net as a neighbour:
```cpp
if( connection->IsBus() && candidate->m_driver_connection->IsNet() )
{
    subgraph->m_bus_neighbors[member].insert( candidate );
    candidate->m_bus_parents[member].insert( subgraph );
}
```
Then `buildConnectionGraph` (2548-2622) runs this pass:
```cpp
// Handle buses that have been linked together somewhere by member (net) connections.
// This feels a bit hacky, perhaps this algorithm should be revisited in the future.
...
if( subgraph->m_bus_parents.size() < 2 )
    continue;
...
SCH_CONNECTION* match = matchBusMember( parent->m_driver_connection, link_member );
...
if( conn->Name() != match->Name() )
{
    wxString old_name = match->Name();
    match->Clone( *conn );
    auto jj = m_net_name_to_subgraphs_map.find( old_name );
    ...
    for( CONNECTION_SUBGRAPH* old_sg : old_subgraphs )
    {
        ...
        old_sg->m_driver_connection->Clone( *conn );
```
The netlist groups subgraphs by `{ GetNetName(), NetCode() }` (`connection_graph.cpp:2693-2701`, `netlist_exporter_xml.cpp:1132`), and `getOrCreateNetCode` gives one code per name. Every subgraph renamed this way is therefore one net.

**How to see it.**
- `bus_connection` (aliases written inline, `bcx/`). Sheet `a2` has the bus `test{x y z}`, which goes up to the root's `top{x y z}`; `test{x}`, which goes to sheet `b`; and `test{y z}`, which goes to sheet `b`. kicad-cli gives `/top.x [J1.1 J2.1]`, `/top.y [J1.2 J2.2]`, `/top.z [J1.3 J2.3]`. Ours gives `top.x [J1.1]`, `test.x [J2.1]`, `top.y [J1.2 J2.2]` (joined only because `a2` has a real `test.y` label), `top.z [J1.3]`, `test.z [J2.3]`.
- Control: rename the two `test{...}` labels to `other{...}` (`bc3/`) or to `{...}` (`bc4/`). kicad-cli then parts them (`/a2/other.x`, `/top.x`), which shows the join is by the shared member name on the sheet. Removing the `test.y` label (`bc2/`) keeps all three joined in kicad-cli.
- `issue17771` (`i17x/`): kicad-cli gives `/m.X.Y.Z1 [TP101 TP301 TP401]` and `/m.X.Y.Z2 [TP102 TP302 TP402]`. Ours gives Z1 right, but Z2 as `m.X.Y.Z2 [TP102]` and `W.X.Y.Z2 [TP302 TP402]`. The sheet `sub` has `W{A}` and `W.X{B}`, both with a member `W.X.Y.Z2`, and no net of that name.
- `netlists/video`: 18 of the 20 differing nets are this case. They are `/DQ0`..`/DQ15` and `/PCA0`, `/PCA1`. The root carries the separate buses `DQ[0..15]`, `DQ[0..31]` (two labels), `DQ[0..7]` (two labels), `PCA[0..2]` (two) and `PCA[0..1]`, and no `DQn` or `PCAn` net label. KiCad's `/DQ0` is 8 pins (RR7.2 U10.92 U11.100 U23.13 U24.21 U3.2 U8.16 U9.14). Ours is 3 nets: `/DQ0` [U10.92 U8.16 U9.14], `/DQ0_2` [U23.13 U24.21], `DQ0` [RR7.2 U11.100 U3.2]. `/PCA0` is 4 pins in KiCad and ours leaves out U10.84.
- Test to write, in `crates/engine/src/nets.rs`: `two_buses_on_one_sheet_join_by_a_shared_member_name`. On one screen, bus A labelled `D[0..1]` with a bus entry and a wire to `R1.1` labelled nothing (joined through a child sheet's hierarchical bus `D[0..1]` whose `D0` wire reaches R1.1). Bus B, separate, labelled `D[0..3]`, with a sheet pin to another child whose `D0` reaches R2.1. No net label `D0` on the parent. Assert R1.1 and R2.1 are one net, named `D0`. Promote `bus_connection` and `issue17771` from `QA_TRACED_OTHERWISE` to `QA_TRACED_ALIKE` once S4 is fixed (or inline their aliases).

**What `nets.rs` needs.** In the member loop (`:452-479`), for every bus group `g` on screen `s`, and only for names that come from a **label** on the bus (GetAllBusLabels: not a sheet pin's name), also index each member node by `(s, m)`, where `m` is the expanded member's full local name: `test.x` for a group with a prefix, `DQ0` for a vector. That is a map `virtual_member: BTreeMap<(usize, String), Vec<usize>>`, filled where `node` is computed. After the loop, union all nodes of each key. The member node's driver (the best bus's member name, `:450`) then names the joined net, as `match->Clone( *conn )` does. The existing `local` / `global_here` joins stay: they are the "virtual sg absorbs a real net of that name" half. Nested buses inside a group are skipped (`if( !conn->IsNet() ) continue;`).

**Unit-sensitive:** no.

### S2. Hidden power-input pins of ordinary symbols do not connect by name (high)

**What differs.** In KiCad, a `power_in` pin that is **invisible**, on a symbol that is not a power symbol, is a global power pin. That is the "legacy" implicit power connection of 74xx, RAM and FPGA symbols. Its net is named by the pin's name, and it joins every other net of that name in the design. It is skipped only when something is wired to it.

Our tracer gives a symbol pin only the `PIN` priority with no name (`crates/engine/src/nets.rs:314-320`). Global power names come only from power symbols (`:338-347`, `Naming::global_power`). Pin visibility is not even in the model the tracer reads: `eda_model::symbol::LibPin` (`crates/model/src/symbol.rs:133`) has no `hidden` field, and `crates/kicad/src/symbol_lib.rs:370::parse_pin` reads none. The design's `symbol_library` does keep it (`LibrarySymbolPin::hidden`: video's has 91 hidden `power_in` pins), but `trace_design` (`crates/cli/src/board.rs:472`) builds pins from `model`.

KiCad, `eeschema/sch_pin.cpp::SCH_PIN::IsGlobalPower` (439-455):
```cpp
if( GetType() != ELECTRICAL_PINTYPE::PT_POWER_IN )
    return false;
...
// Local power symbols are never global, even with invisible pins
if( parent->IsLocalPower() )
    return false;
// Legacy support: invisible power-in pins on non-power symbols act as global power
return !IsVisible();
```
`connection_graph.cpp::generateGlobalPowerPinSubGraphs` (1830-1885):
```cpp
// These are NOT limited to power symbols, we support legacy invisible + power-in pins
// on non-power symbols.
...
if( !pin->ConnectedItems( sheet ).empty()
        && ( !libParent || !libParent->IsGlobalPower() ) )
{
    // ERC will warn about this: user has wired up an invisible pin
    continue;
}
...
if( libParent && libParent->IsGlobalPower() )
    connection->SetName( pin->GetParentSymbol()->GetValue( true, &sheet, false ) );
else
    connection->SetName( pin->GetShownName() );
```

**How to see it.** On `netlists/video`, the other 2 of the 20 nets:
- KiCad's `+5V` has 140 pins and ours has 78. The 62 missing are the hidden `VCC` pins of U2, U3-U6, U11-U19, U21-U24. Each is a net of its own here, `unconnected-(U11-VCC-Pad11)` and so on.
- KiCad's `GND` has 237 pins and ours has 167. The 70 missing are the hidden `GND`/`VSS` pins (`unconnected-(U11-VSS-Pad10)` ...).

The file declares them as `(pin power_in line (at -6.35 19.05 90) (length 0) (name "VCC") (number "10") hide)` (`graphic.kicad_sch:37-40`).

Test to write, in `crates/cli/src/sch_hierarchy_tests.rs`: `a_hidden_power_input_pin_joins_the_net_of_its_name`. A library symbol U1 with a hidden `power_in` pin named `VCC` and nothing wired to it, plus a power symbol `VCC` on a wire to R1.1. Assert `U1.<n>` and `R1.1` are one net named `VCC`. A second case wires the hidden pin to R2.1 and asserts it stays on R2's net, as KiCad's `continue` does. Then move `netlists/video` to `QA_TRACED_ALIKE` once S1 and S2 are fixed.

**What is needed.** Carry `hidden` into `eda_model::symbol::LibPin`, or pass it through `Naming`. Then, in `trace_nets_with`, give each such unwired pin a `GLOBAL_POWER_PIN` driver named by the pin's shown name, and push it into `global_names`, but only when it is the only thing at its point (`ConnectedItems( sheet ).empty()`) and the symbol is neither a power symbol nor a local power symbol.

**Unit-sensitive:** no.

### S3. A power symbol's value is not resolved per placement: `${#}` shorts nets together (high)

**What differs.** KiCad names a global power pin's net by the symbol's Value with text variables resolved **for that sheet path**: `GetValue( true, &sheet, false )` (`connection_graph.cpp:1882`, quoted in S2). That goes through `SCH_FIELD::GetShownText( aPath )` (`sch_symbol.cpp::GetValue`, 1452-1458), and `${#}` is the page number of the path (`sch_sheet.cpp::SCH_SHEET::ResolveTextVar`, 404-407):
```cpp
if( token->IsSameAs( wxT( "#" ) ) )
{
    *token = wxString::Format( "%s", aPath->GetPageNumber() );
    return true;
}
```
Our importer stores the raw Value text as the power symbol's net (`crates/kicad/src/sch_import.rs:196`: `PowerSymbol { ..., net: value, ... }`). The tracer uses `ps.net` as the global name as it is (`crates/engine/src/nets.rs:345-346`). So `REF${#}` on a sheet placed three times is one global net `REF${#}` joining all three placements, where KiCad makes three nets. The project already has the resolver it needs, `eda_model::text_vars` (`SchSheet::resolve_level`, `"#" => self.page`, `crates/model/src/text_vars.rs:598-600`). It is just not used for connectivity.

**How to see it.** `ERC_dynamic_power_symbol_test` (copy in `dyn/`): the sub-sheet `erc_test_dynamic_power_symbol_subsheet.kicad_sch` is placed three times (pages 2, 3 and 4), each with a power symbol `.Project_Library:REF${#}` on R's pin 2. kicad-cli gives `REF2 [R201.2]`, `REF3 [R301.2]`, `REF4 [R401.2]`. Ours gives one net `REF${#}` [R201.2 R301.2 R401.2]. That is a short the board would inherit.

Test to write, in `crates/engine/src/nets.rs` or `sch_hierarchy_tests.rs`: `a_power_symbols_page_variable_is_resolved_per_placement`. One screen shown twice (page "2" and "3"), holding a power symbol of value `P${#}` on R1.1. Assert two nets `P2` and `P3` with one pin each.

**What is needed.** Resolve `ps.net` per `ScreenIn` (the placement's page and sheet fields) before it becomes a driver. This could be a `Naming::power_value: Option<&dyn Fn(&ScreenIn, &str) -> String>` backed by `text_vars`. It should resolve every variable, not only `#`: `${SHEETNAME}` and project variables too, as `GetShownText` does.

**Unit-sensitive:** no.

### S4. Bus aliases are lost on import: `SheetFile` has no aliases, and a KiCad 10 project's aliases (`.kicad_pro`) are never read (high)

**What differs.**
- (a) `eda_kicad::load_sheet_file` (`crates/kicad/src/sheet_file.rs:28-52`) reads the tree with `import_kicad_sch_tree_with`, which does collect `design.bus_aliases` (`sch_import.rs:452-459`, `:760-763`), and then throws them away. `SheetFile` (`crates/model/src/sheet_file.rs:14-25`) has no alias field. Every route that goes through it reaches the tracer with no alias, so any bus written with `{ALIAS}` has no members and joins nothing. Those routes are `eda import-kicad x.kicad_sch` and File > Import > Non-KiCad Schematic (`Cmd::ReplaceSchematic`, `crates/ops/src/import_foreign.rs:57-105`, which keeps the *old* design's aliases), and `Cmd::ImportSheet` (Import Sheet, a schematic drop, a design block: `crates/ops/src/lib.rs:1261`).
- (b) KiCad 10 keeps bus aliases in the project file, not the schematic. `SCHEMATIC::Reset` and `SetProject` call `loadBusAliasesFromProject()` (`eeschema/schematic.cpp:180, 211`), the parameter is `"schematic.bus_aliases"` (`common/project/project_file.cpp:120`), and the `.kicad_sch` writer no longer emits `bus_alias` (only the parser accepts it, `sch_io_kicad_sexpr_parser.cpp:3024`). Our reader looks only at `(bus_alias ...)` in the `.kicad_sch` (`sch_import.rs:441-459`, whose comment says the project file has no reader here). Nothing reads `.kicad_pro`'s `schematic.bus_aliases`.

**How to see it.**
- (a) `eda import-kicad bus_connection/bus_connection.kicad_sch -o out` (QA original, aliases in the `.kicad_sch`): `design.json` has no `bus_aliases`, and its nets are `x [J1.1]`, `/a2/b/x [J2.1]` and so on, six one-pin nets. kicad-cli gives three two-pin nets. `issue17771` likewise gives six one-pin nets.
- (b) `bcp/`: the aliases moved into `bus_connection.kicad_pro` as `"schematic": {"bus_aliases": {"a_xyz": ["x","y","z"], ...}}` and removed from the `.kicad_sch`. kicad-cli still gives `/top.x [J1.1 J2.1]` and the rest; ours gives six one-pin nets.
- Tests to write: `crates/kicad/src/sheet_file.rs::a_sheet_files_bus_aliases_come_with_it` (load the QA `bus_connection` and assert three aliases), and `crates/kicad/src/sch_import.rs::a_projects_bus_aliases_are_read_from_the_kicad_pro` (a root with no `bus_alias` and a `.kicad_pro` that has them; assert `design.bus_aliases`).

**Unit-sensitive:** no.

**Note for the slow test.** `qa_traced` reads with `import_kicad_sch_tree`, which keeps the `.kicad_sch` aliases, so the slow test's numbers for `bus_connection` and `issue17771` are S1 alone. The studio's own import paths are worse (S4).

### Checked and found to match

- `netlists/video`: 351 of the 371 KiCad nets of two pins or more have exactly KiCad's pins, among them all the `DQ16`..`DQ31` nets and every other bus member net (one bus, or buses joined by a label of the same name). The 20 that differ are S1 (18) and S2 (2); nothing else is unexplained.
- `bus_connection` with the aliases inline: the member joined through a real net label on the sheet (`test.y`) matches kicad-cli. This is the existing `local` join at `nets.rs:475`, KiCad's virtual member absorbing a real net. The same holds for `issue17771`'s Z1 through the explicit `W.X.Y.Z1` label.
- `ERC_dynamic_power_symbol_test`: the `GND` net [R201.1 R301.1 R401.1] matches. The `PWR_FLAG`s name no net, as `IsGlobalPower` (power output) says.
- The match of a group member by local name across the hierarchy (`m{A}` over `W{A}`, `W.X{B}` over `{B}`, `Y{C}` over `V{C}`) gives KiCad's members wherever a member node exists (Z1 path in `issue17771`).
- The reader keeps a pin's bare `hide` atom of 2020-format files (`(pin power_in line ... hide)`), so S2 is the tracer's alone, not the reader's.

### Also noticed (outside the asked scope; for the record)

- Names: ours leaves the sheet path off a root local label's net (`ACCES_RAM-` where KiCad says `/ACCES_RAM-`; 278 of video's nets differ only so). This is deliberate and documented (`nets.rs:514`, PARITY-sch row "Nets across sheets"). It matters only where a name has to equal KiCad's (a `.kicad_pcb` net name after Update PCB). Low.
- `PARITY-sch.md:543` ("Not ported: buses are traced as plain wires, a screen placed by two sheet symbols is traced once") is stale since sections 38 and 39. Low (doc).

### Summary rows

| # | Finding | Severity | Unit-sensitive |
|---|---|---|---|
| S1 | Two buses on one sheet sharing a member name are not one net (`generateBusAliasMembers` + the multiple-bus-parents pass); `bus_connection`, `issue17771` Z2, 18 nets of `video` | high | no |
| S2 | Hidden `power_in` pins of non-power symbols are not global power (`SCH_PIN::IsGlobalPower` legacy rule); `video` `+5V`/`GND` miss 132 pins | high | no |
| S3 | Power symbol Value not resolved per placement (`GetValue( true, &sheet )`, `${#}`); `ERC_dynamic_power_symbol_test` shorts REF2/REF3/REF4 | high | no |
| S4 | Bus aliases dropped by `load_sheet_file`/`SheetFile` (import-kicad, Import Sheet, ReplaceSchematic), and `.kicad_pro` `schematic.bus_aliases` never read | high | no |
| -- | Root local net names without `/` (documented choice); stale PARITY-sch row 543 | low | no |

---

## Summary

| # | Finding | Severity | Unit-sensitive |
|---|---|---|---|
| O1 | `gr_curve` with a control point on an end point is dropped by `poly_points` dedup: the slot opens (QA `issue23658`, `issue24078`), and the curve is lost on export | high | no |
| O2 | Footprint Edge.Cuts become board items: no `BuildBoardPolygonOutlines` footprint pass (a notch across the edge makes the outline invalid; a contour outside the board becomes a second board), and the export moves them out of the footprint | high | no |
| O3 | `gr_rect` corner radius dropped from the outline and the file (QA `ultrasound`: 18700 vs 18678.5 mm²) | high | no |
| O4 | KiCad 10 `gr_ellipse` / `gr_ellipse_arc` are not in the live outline (kept in the file) | medium | no |
| O5 | No Edge.Cuts: no live "(no edges found on Edge.Cuts layer)", no inference from the board's items | medium | no |
| O6 | Message lacks "(gap X)", and the marker is not `testOutline`'s overlap centre or nearest-points midpoint | medium | partly |
| O7 | Outline flattened at a fixed 5 µm, not the board's `m_MaxError`; `max_error` clamped to 1 µm | medium | partly |
| O8 | Closed shapes' end points are not chaining candidates (a line from a circle's rim: KiCad also says self-intersecting) | medium | no |
| O9 | The closing arc is re-polygonised; KiCad (arcs cleared) moves the last point; the PARITY claim is wrong | low | partly |
| O10 | Placement and the `Cmd::PlaceAt` family use the outline's bbox (largest outline only) | high (known) | no |
| O11 | `placement_within_outline` tests the courtyard's corners only | low | no |
| O12 | Our port is 8303b2ad's chaining; the judge, kicad-cli 25ec2335, chains differently (`findNeighbours`, crossing contours, ellipses) | medium | no |
| O13 | µm rounding in messages ("1000 nm" vs "500 nm"), areas, circle radius, and the Bezier error fallback | low | yes |
| -- | Known and re-confirmed: DSN boundary without cutouts (high), NPTH not cut from the 3D slab (low), `gr_poly` arcs flattened (low), no outline editor or "Fix discontinuities"; a plain intent polygon never checked live (medium, from reading) | -- | no |
| S1 | Two buses on one sheet sharing a member name are not one net (`generateBusAliasMembers` + multiple-bus-parents pass); `bus_connection`, `issue17771` Z2, 18 nets of `video` | high | no |
| S2 | Hidden `power_in` pins of non-power symbols are not global power (`SCH_PIN::IsGlobalPower` legacy rule); `video` `+5V`/`GND` miss 132 pins | high | no |
| S3 | Power symbol Value not resolved per placement (`${#}`); `ERC_dynamic_power_symbol_test` shorts REF2/REF3/REF4 | high | no |
| S4 | Bus aliases dropped by `load_sheet_file`/`SheetFile`; `.kicad_pro` `schematic.bus_aliases` never read | high | no |
| -- | Root local net names without `/` (documented choice); stale PARITY-sch row 543 | low | no |

Counts:
- Part 1, outline: 4 high (O1, O2, O3, O10), 5 medium (O4, O5, O6, O7, O8; plus O12 as context), 3 low (O9, O11, O13).
- Part 2, schematic: 4 high, 1 low.
