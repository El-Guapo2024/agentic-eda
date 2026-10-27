# Findings round 6: PCB editing and its UI (`r6-pcb-edit`)

Reviewer `r6-pcb-edit`. This review only reports findings: no code was changed, nothing was built, and no server was started.
- **Code read:** main's checkout `/home/team_braneboard_com/work/agentic-eda`, HEAD `7d772e6e`.
- **KiCad:** compared against `8303b2ad` (`$KICAD_SRC_DIR`), and `25ec2335` where marked.
- **Paths:** `web/` means `web/studio/src/`.
- **Scratch:** `/tmp/findings-r6-pcb-edit.GQUt`. It holds the kicad-cli teardrop case for D6-1 in `td/`.

Scope: GAPS items 8, 9, 10, 13 (teardrops), 14-18, 21, 22, 29 and 35.
- **Priority (a):** the round-5 merges.
  - Draw Via / `Cmd::PlaceVia` (9d3a3b9d), in Part C.
  - Arc teardrops (a98857b6), Cleanup Graphics (4e6628bf) and Create from Selection (fb658ba2), in Part D.
  - The line modes and `pcbShapeSnap.ts` (9bb101cb), zone/polygon close (3ba02f35) and the Text tool / `Cmd::PlaceText` (dfd330bb), in Part A.
  - Part A also answers the open question on the stepped line width (A6-1).
- **Priority (b):** rows no round had reviewed (Swap, Move/Copy with Reference, Lock, Select/Grab Unconnected, Select All Tracks in Net, Same Hierarchical Sheet), in Part E.

**Open question, answered (A6-1).** KiCad keeps a stepped line width when a drawing tool is armed again on the same layer, and even when a different drawing tool is armed. `m_stroke` belongs to `DRAWING_TOOL` and is reset only in these cases:
- in `Reset()`, which runs on a board load, after OK in Board Setup and after the Grid Origin dialog;
- at the start of `drawShape`, `drawArc` or the Bezier tool when the active layer differs from `m_layer`;
- on a `layerChanged` event while a tool is active.

The studio resets the width every time a tool is armed (`useActionRunner.ts:656-660`), which is wrong.

**Reuse rule.** No new duplicate of kicad-cli was found. The teardrop generator still duplicates `TEARDROP_MANAGER` (already recorded); D6-1 is a defect inside it.

---

# Part A: line modes, zone/polygon close, the Text tool, the open question

### A6-1 (medium). A stepped line width is dropped every time a drawing tool is armed; KiCad keeps it until the active layer changes (answer to the open question) **Fixed** on task-r6-pcb-edit: b2fe2c35
- **What differs:**
  - **Ours.** `web/studio/src/actions/useActionRunner.ts:656-660` (`armGraphicTool`) dispatches `drawStrokeStep: null` on every arm of Line, Arc, Rectangle, Circle and Graphic Polygon. Its comment cites `m_stroke.SetWidth( bds.GetLineThickness( m_layer ) )` as if that ran on every arm.
  - **KiCad.** `m_stroke` is a member of `DRAWING_TOOL`, shared by every drawing tool, and is reset in only three places:
    - `DRAWING_TOOL::Reset` (drawing_tool.cpp:302-319). It runs from `TOOL_MANAGER::ResetTools`, which pcbnew calls on a board load (`PCB_BASE_EDIT_FRAME::SetBoard`), after OK in `PCB_EDIT_FRAME::ShowBoardSetupDialog` (pcb_edit_frame.cpp:1770, `MODEL_RELOAD`) and after the Grid Origin dialog (`COMMON_TOOLS::GridOrigin`, `REDRAW`). It does not run when a tool is armed.
    - The start of `drawShape`, `drawArc` and `DrawBezier`, but only when the layer has changed:
      ```cpp
      if( m_layer != m_frame->GetActiveLayer() )
      {
          m_layer = m_frame->GetActiveLayer();
          m_stroke.SetWidth( bds.GetLineThickness( m_layer ) );
      ```
      (drawing_tool.cpp:2369-2372; drawArc 2792-2796; bezier 3084-3087.)
    - The `layerChanged` branch inside the loop, again only `if( m_layer != m_frame->GetActiveLayer() )` (2503-2508, 2938-2943).
  - So in KiCad: step a line to 0.3 mm with Ctrl++ (incWidth), Esc out, arm Rectangle (or Line again) on the same layer, and the rectangle is 0.3 mm. In the studio it is the class default again.
  - Related sub-case. Our step is keyed by layer (`layerDefaults.ts:99-102`, `step.layer === layer`), so the step comes back when you return to its layer. Step on F.SilkS, switch to B.SilkS inside the tool (KiCad resets `m_stroke` to B's default), switch back to F.SilkS: KiCad resets to F's default, while ours brings the old F.SilkS step back. Switching layers outside the tool gives the same result in both.
- **How to see it.** A browser check: arm Line on F.SilkS, press Ctrl++ twice, Esc, Esc, arm Rectangle, draw. Assert the rectangle's `stroke_width` is the class's 0.1 mm + 2 x `WIDTH_STEP`. Unit form: an `armGraphicTool` test asserting `drawStrokeStep` is kept when `state.activeLayer === step.layer`. The fix is to drop the reset on arm and reset only on a board load, Board Setup OK, or a layer change seen by the tool.
- **Severity:** medium (a wrong default carried into the drawn item's width).
- **Already known?** No. This was the open question from round 5's work. KiCad keeps the stepped width.

### A6-2 (medium). A board graphic polygon is always filled, and takes the stepped width; KiCad leaves it unfilled on Edge.Cuts and the courtyards, at the layer's own width with a solid line **Fixed** on task-r6-pcb-edit: b2fe2c35
- **What differs:**
  - **Ours.** `web/studio/src/components/canvas/Canvas.tsx:148`, `shapeFromPoints`' `case "polygon"`, returns `filled: true` on every layer. Canvas.tsx:669-670 passes `drawStrokeWidthNm(..., state.pcbx.drawStrokeStep)`, so a Ctrl++ step from another tool reaches the polygon.
  - **KiCad.** `ZONE_CREATE_HELPER::commitZone`, `case ZONE_MODE::GRAPHIC_POLYGON` (zone_create_helper.cpp:236-251):
    ```cpp
    poly->SetFilled( layer != Edge_Cuts && layer != F_CrtYd && layer != B_CrtYd );
    poly->SetStroke( STROKE_PARAMS( board->GetDesignSettings().GetLineThickness( layer ),
                                    LINE_STYLE::SOLID ) );
    ```
    `DrawZone` has no incWidth/decWidth branch and never reads `m_stroke`.
  - Round 4 fixed this for the Footprint Editor only (`kicad-port/fpDrawDefaults.ts`, E4-F1). The board's tool was left as it was.
- **How to see it.** Draw a polygon on Edge.Cuts with the board's polygon tool and save. Ours writes `(gr_poly ... (fill yes))` (`(fill solid)`); KiCad's tool writes `(fill no)`. A filled Edge.Cuts polygon also draws as a solid slab in KiCad. Test: `shapeFromPoints("polygon", pts, "Edge.Cuts", w).filled === false`, true on F.SilkS, false on F.CrtYd and B.CrtYd. A second test: after an incWidth step, the polygon's width equals `lineThicknessNm(d, layer)`.
- **Severity:** medium. The file differs from what KiCad's tool would write, and the shape is drawn filled. It is not high because the outline is still read as an outline.
- **Already known?** Only for the Footprint Editor (r4 E4-F1, fixed). The board's tool is not listed.

### A6-3 (medium). A new text (Text tool) is never bold: `InferBold` is not ported, and a copper text at KiCad's defaults is bold in KiCad **Fixed** on task-r6-pcb-edit: b2fe2c35 (the dialog has no Bold control: Open)
- **What differs:**
  - **Ours.** `web/studio/src/kicad-port/layerDefaults.ts:90` (`newTextStyle`) and `components/TextDialog.tsx:74-77` build the extras with the glyph width, italic and valign, but no bold. `InferBold` appears nowhere in `crates/` or `web/studio/src`. Our text dialog has no Bold control.
  - **KiCad.** `DRAWING_TOOL::PlaceText` (drawing_tool.cpp:1058-1060):
    ```cpp
    textAttrs.m_Size = bds.GetTextSize( layer );
    textAttrs.m_StrokeWidth = bds.GetTextThickness( layer );
    InferBold( &textAttrs );
    ```
    with `InferBold` from gr_text.h:82-89:
    ```cpp
    aAttrs->m_Bold = abs( penSize - GetPenSizeForBold( textSize ) )
                   < abs( penSize - GetPenSizeForNormal( textSize ) );
    ```
    `GetPenSizeForBold` is size/5 and `GetPenSizeForNormal` is size/8 (gr_text.cpp:37-64).
  - The copper class defaults are 1.5 mm and 0.30 mm (board_design_settings.h:48,52). The bold pen for 1.5 mm is 0.30 mm and the normal pen 0.1875 mm, so every copper text placed with the defaults is bold in KiCad. `DIALOG_TEXT_PROPERTIES` shows Bold checked (`m_bold->Check( m_item->IsBold() )`, :347), and OK keeps it (`SetBoldFlag`, :570).
  - The same `InferBold` runs for the text box tool's `m_textAttrs` (drawing_tool.cpp:323, 2378, 2512).
- **How to see it.** Place "X" on F.Cu with the Text tool and default Board Setup, then save. KiCad writes `(font (size 1.5 1.5) (thickness 0.3) (bold yes))`; ours has no `bold`. Test: `newTextStyle(defaults, "F.Cu").bold === true`, and `false` for F.SilkS (1.0 / 0.1: |0.1-0.2| > |0.1-0.125|). Also a `place_text` ops test that the extra carries `bold: true`.
- **Severity:** medium (the file differs; a stroke text draws the same).
- **Already known?** GAPS item 10 (line 489) lists board text bold as missing from the Properties panel. Bold at creation (`InferBold`) is not listed.

### A6-4 (low). Ctrl++ / Ctrl+- step the drawing width with no drawing tool armed **Fixed** on task-r6-pcb-edit: b2fe2c35
- **Ours.** `useActionRunner.ts:2028-2029` registers `pcbnew.InteractiveDrawing.incWidth`/`decWidth` with no condition, so in the Select tool they quietly change the width of the next drawing.
- **KiCad.** The actions are handled only inside the loops of `drawShape` (`graphic && evt->IsAction( &PCB_ACTIONS::incWidth )`, :2690), `drawArc` (:2995) and `DrawDimension` (:1712). Outside those loops nothing handles them.
- **How to see it.** In the Select tool press Ctrl++, then draw a line: ours is one step wider. Test: the handler is a no-op unless `activeTool` is a drawing tool.
- **Already known?** No.

### A6-5 (low). The line mode (free / 45 / 90) is not remembered across sessions **Fixed** on task-r6-pcb-edit: 475fee76
- **Ours.** `kicad-port/pcbParityState.ts:104` holds `angleSnapMode: "direct"` in in-memory state only. A reload starts free again.
- **KiCad.** `PCBNEW_SETTINGS::m_AngleSnapMode` is a stored setting (`editing.pcb_angle_snap_mode`, pcbnew_settings.cpp:190) that `PCB_VIEWER_TOOLS::NextLineMode` writes.
- **Already known?** No.

### A6-6 (low, behaviour inferred from the code). R / F / E on a carried text **Open**
- **KiCad.** In `PlaceText`, the text is selected while it is carried (`m_toolMgr->RunAction<EDA_ITEM*>( ACTIONS::selectItem, text )` after the dialog). Other events pass on (`evt->SetPassEvent()`), so Rotate and Flip reach `EDIT_TOOL` with the carried text as the selection. `properties` opens its dialog (an explicit branch).
- **Ours.** `Canvas.tsx` clears the selection on the first click, and the carried text lives only in `drawState`. R (`useActionRunner.ts:325`) rotates `requestSelection()`, which is empty, and E opens nothing.
- **Severity:** low. I did not follow how `EDIT_TOOL::Rotate`'s `commit->Modify` treats an `IS_NEW` item to the end. Confirm that in KiCad before fixing.

## Checked and found to match (do not re-review)
- `pcbShapeSnap.ts`:
  - `shapeAngleSnap` matches drawShape's mode choice (drawing_tool.cpp:2444-2459): rectangle and circle are free, Ctrl gives DEG45; a line follows the mode, and Ctrl frees it.
  - `snapVector45` / `snapVector90` match `GetVectorSnapped45` (with `only45` for a rectangle only) and `GetVectorSnapped90`: the 2:1 boundary and `copysign` (geometry_utils.h:112-160).
  - No second grid snap.
  - Ctrl also turns the grid off (`DisableGridSnapping` is `Modifier( MD_CTRL )`; `pcbSnap.ts` `setUseGrid(!mods.ctrl)`).
  - The preview and the click use the same function.
- The default line mode is DIRECT (pcbnew_settings.cpp:57). `lineModeNext` cycles DIRECT -> DEG45 -> DEG90 (`PCB_VIEWER_TOOLS::NextLineMode`).
- Arc tool: `arcAngleSnap` is `angleSnap != DIRECT`, with Ctrl giving DIRECT (drawArc :2851-2854, :2931).
- Zone / polygon close:
  - `zoneFinish`/`finalOutline` match `ZONE_CREATE_HELPER::OnComplete`. That covers the locked points, then in DEG45/DEG90 the leader from index 1 and the loop from 1 to n-2, `Simplify( true )` (tolerance 1, a closed chain, our `simplifyClosed` mirrors `SHAPE_LINE_CHAIN::Simplify`'s index walk), the start-point `LineDistance <= 1` removal, and fewer than 3 points scrapped.
  - `Append` deduplication of consecutive points (`append`) also matches.
  - `leaderModeOf`: Ctrl gives DIRECT.
- Text tool:
  - Undo or Esc while a text is carried drops the text and keeps the tool (`drawingCancel.ts`: "text" in `BOARD_DRAWING_KINDS`, `undoDropsDrawing`). Redo and the zone fills are refused while carried (`redoRefused`).
  - The bottom/left justification, the class glyph width and italic.
  - keep_upright is not needed for a board text: the writer emits it only for a footprint's text (pcb_io_kicad_sexpr.cpp:2340).
  - "Draw Text" selects the placed text.

---

# Part C: Draw Via

Read: `web/studio/src/kicad-port/viaPlacer.ts`, `crates/ops/src/via_place.rs`, `Cmd::PlaceVia` (`crates/ops/src/lib.rs:370`, `:2902`), the Canvas wiring (`web/studio/src/components/canvas/Canvas.tsx:744-797`), the ghost (`painter.ts:1327`), `crates/cli/src/board.rs:1750`. KiCad: `pcbnew/tools/drawing_tool.cpp::DRAWING_TOOL::DrawVia` at 8303b2ad (lines 3663-4385), `pcb_tool_base.cpp::doInteractiveItemPlacement`, `common/view/view.cpp::VIEW::Query`, `board_design_settings.cpp::GetCurrentViaSize`; 25ec2335's DrawVia (the same logic moved into `FindSnapTrack` / `CheckItemDRCViolation` helpers, plus the `<no net>` menu label).

Skipped as already listed (GAPS item 8, "Missing (round 5)"): `(free yes)`, no cut of an arc track, constraint severity, pad clearance override.

### C6-1 (medium). Hidden layers and hidden object kinds: KiCad neither snaps to, takes the net of, splits, nor DRC-checks an item that is not visible; we use every item **Fixed** on task-r6-pcb-edit: 4ab0c643 (hidden layers; a hidden object class is a level-of-detail choice `VIEW::Query` does not read, so not a difference)
- **What differs.**
  - **Ours.** `findTrack`, `findPad`, `findGraphic` (`viaPlacer.ts:72`, `:103`, `:134`) walk `queryScene(sceneOf(board), ...)`, which has every item of the board whatever the Appearance panel shows (`snapScene.ts:402`, no visibility filter). `via_violates_drc` (`via_place.rs:44-154`) walks every pad, track, via, graphic and text of the board.
  - **KiCad.** All four (`findTrack`, `findPad`, `findGraphic`, `checkDRCViolation`) collect their candidates by `view->Query( bbox, items )`, and `VIEW::Query` (`common/view/view.cpp::VIEW::Query`) skips layers that are not shown:
    ```cpp
    // ignore layers that do not contain actual items (i.e. the selection box, menus, floats)
    if( ( *i )->displayOnly || !( *i )->visible )
        continue;
    ```
    So with B.Cu hidden (or the Vias / Pads objects hidden) a B.Cu track is neither snapped to, nor gives its net, nor is split, nor refuses the via. Only `findStitchedZoneNet` reads visibility explicitly, and that part we ported (`stitchedZoneNets`'s `isLayerVisible`).
- **How to see it.** Two-layer board, a GND track on B.Cu, B.Cu hidden in Appearance, Draw Via clicked on the track's middle. KiCad: a via of no net (or the F.Cu pour's) where the grid put it, the B.Cu track whole; if the track is of another net than the via's, KiCad places it anyway (no refusal). Ours: the via snaps onto the hidden track, takes GND and cuts it. Test: `viaPlacer.test.ts` `"a track on a hidden layer is not snapped to and gives no net"`: board with one B.Cu track, `isLayerVisible: l => l !== "B.Cu"`, assert `snapViaPosition(...)` equals `gridPos` and `viaNet(...)` is `{kind:"net", net:"", track:null}` (needs `findTrack`/`findPad`/`findGraphic` to take the visibility predicate). On the backend side `place_via` needs the visible layers (or the caller's verdict) to skip hidden items in `via_violates_drc`.
- **Severity.** medium (a hidden track is split and the via gets a net the user cannot see; the DRC verdict differs).
- **Already known?** No.

### C6-2 (medium). Draw Via uses a cached via size, not the board's list entry by index: a Board Setup edit to the chosen entry (or to the net class's via) is not seen **Fixed** on task-r6-pcb-edit: 4ab0c643
- **What differs.**
  - **Ours.** `Canvas.tsx:749`: `const viaPreset = state.currentViaPreset ?? { diameter: rules?.via_diameter ..., drill: rules?.via_drill ... }`. `currentViaPreset` is a size snapshot taken when the entry was picked (`pcbMenuActions.ts:54`, `:80`, `useActionRunner.ts:1119`). The router was moved off this snapshot in round 5 (R5-9): `routerViaActions.ts:66` reads `presets[state.currentViaIndex - 1]` from the board now. The ghost (`painter.ts:1331`) has the same snapshot.
  - **KiCad.** `CreateItem` asks `bds.GetCurrentViaSize()` / `GetCurrentViaDrill()` each time a via is made (`board_design_settings.cpp::GetCurrentViaSize`):
    ```cpp
    if( m_useCustomTrackVia )
        return m_customViaSize.m_Diameter;
    else if( m_viaSizeIndex <= 0 || m_viaSizeIndex >= (int) m_ViasDimensionsList.size() )
        return m_NetSettings->GetDefaultNetclass()->GetViaDiameter();
    else
        return m_ViasDimensionsList[ m_viaSizeIndex ].m_Diameter;
    ```
    and `GetCurrentViaDrill` returns `drill > 0 ? drill : -1`.
- **How to see it.** Pick via size entry 2 (0.8/0.4) from the router's size menu, then Board Setup > Pre-defined Sizes: change that row to 1.0/0.5, OK, Draw Via, click. KiCad: a 1.0/0.5 via. Ours: 0.8/0.4. Same after "Use Net Class Values" then a change of the Default class's via. Test: a unit test of a `drawViaSize(board, state)` helper (to be extracted, as `chosenViaSize` is for the router): presets `[{0.8,0.4}]` changed to `[{1.0,0.5}]` with `currentViaIndex: 1` and a stale `currentViaPreset {0.8,0.4}`; assert `{1.0, 0.5}`.
- **Severity.** medium (wrong size on the board).
- **Already known?** No (R5-9 fixed the router only). `pcbEditSweep.ts:232` (`changeTrackWidth`'s via size) has the same snapshot; outside this slice.

### C6-3 (medium). The via's DRC check judges a board text on copper as a centred, square, normal-weight stroke text: the text's vertical justification, glyph width, bold and knockout (all in `text_extras`) are ignored **Fixed** on task-r6-pcb-edit: 4ab0c643
- **What differs.**
  - **Ours.** `via_place.rs:126-140`: `size = (t.size_nm, t.size_nm)`, `effective_pen_width(t.stroke_width, size, false)` (bold false), `valign: 0`, strokes only. The text's `TextExtra` (`crates/model/src/board_text.rs:36-45`: `valign`, `width_nm`, `bold`, `knockout`) is never read. Since round 5 B5-2 a text placed with the Text tool is bottom justified with the class's glyph width (`place_text`), so the check places its strokes about half a text height below where they are.
  - **KiCad.** `hasDRCViolation` collides with `aOther->GetEffectiveShape( layer )`, which for a text is (`pcb_text.cpp::PCB_TEXT::GetEffectiveShape`):
    ```cpp
    if( IsKnockout() )
    {
        SHAPE_POLY_SET poly;
        TransformTextToPolySet( poly, 0, GetMaxError(), ERROR_INSIDE );
        return std::make_shared<SHAPE_POLY_SET>( std::move( poly ) );
    }
    return GetEffectiveTextShape();
    ```
    with the text's real attributes (size x/y, bold, V-justify).
- **How to see it.** F.Cu text "GND" 1.5 mm high placed with the Text tool (bottom justified) at (10,10); a via of another net whose pad edge is 0.1 mm below the anchor line (at y = 10.45 for a 0.6 mm via): KiCad places it (the glyphs are above the anchor); ours refuses with "Via location violates DRC." (it judges glyphs centred on the anchor). Test: `via_place::tests::a_bottom_justified_text_is_judged_above_its_anchor` with a `TextExtra { valign: Bottom }`, asserting `place_via` succeeds.
- **Severity.** medium (a wrong refusal or a wrong placement next to copper text).
- **Already known?** No for the via check. The DRC's own text shapes (`crates/drc/src/board.rs:707-716`, `valign: 0`) have the same gap; out of this slice, worth a check by the DRC reviewer.

### C6-4 (low). An unflashed layer of a PTH pad of another net is skipped; KiCad judges the pad's hole there at the copper clearance **Open**
- **What differs.**
  - **Ours.** `via_place.rs:69-72`: `if pad.flash(layer, ..) == Flash::Never { continue; }`; the hole is then judged only at `HOLE_CLEARANCE`.
  - **KiCad.** `hasDRCViolation` loops `for( PCB_LAYER_ID layer : aOther->GetLayerSet() )` with `CLEARANCE_CONSTRAINT`, and `PAD::GetEffectiveShape` on a layer a PTH pad does not flash returns the hole:
    ```cpp
    if( !flash )
    {
        if( GetAttribute() == PAD_ATTRIB::PTH )
        {
            effective_compund->AddShape( GetEffectiveHoleShape() );
            return effective_compund;
        }
    ```
    So where the copper clearance (net class / custom rule) is larger than the hole clearance, KiCad refuses a via near the hole of a pad with "remove unconnected layers"; we place it.
- **How to see it.** A PTH pad with unconnected inner layers removed, net VCC; a rule `(constraint clearance (min 0.5mm))`, hole clearance 0.25 mm; a GND via 0.35 mm from the hole's edge. KiCad: "Via location violates DRC."; ours: placed. Test: in `via_place.rs` tests, a 4-layer board with such a pad, assert `place_via` errs with `VIA_VIOLATES_DRC`.
- **Severity.** low (needs a padstack with removed layers and a clearance above the hole clearance).
- **Already known?** No.

### C6-5 (low). Copper items the check never sees: text boxes, tables, dimensions and barcodes on a copper layer **Open**
- **What differs.** `via_violates_drc` walks pads, tracks, vias, `drawings.shapes`, `drawings.texts` and footprint texts only. KiCad's `checkDRCViolation` takes every board item from the view query (only footprints and groups as such, hidden texts and fields, and reference images are skipped), so a `PCB_TEXTBOX`, `PCB_TABLE`, `PCB_DIMENSION_*` or `PCB_BARCODE` on F.Cu is collided with its effective shape at the clearance. Our IR has them (`board_items.text_boxes`, `tables`, `dimensions`, `barcodes`).
- **How to see it.** A text box on F.Cu; Draw Via clicked on its border. KiCad refuses; ours places. Test: `via_place` test with a `TextBox` on F.Cu, assert `VIA_VIOLATES_DRC`.
- **Severity.** low.
- **Already known?** No.

### C6-6 (low). With "Allow DRC violations" on, the via is placed silently; KiCad shows the error and places it **Open**
- **What differs.** KiCad (`PlaceItem`): `ShowInfoBarError( _( "Via location violates DRC." ), true, WX_INFOBAR::MESSAGE_TYPE::DRC_VIOLATION );` comes before `if( !m_allowDRCViolations ) return false;`, so the message shows in both cases, and a clean placement dismisses a standing DRC-violation infobar. Ours (`via_place.rs:200`): with `allow_drc_violations` the check is not even run, and nothing is said.
- **How to see it.** Router in Highlight Collisions with Allow DRC violations; Draw Via next to another net's track: KiCad places it and shows "Via location violates DRC."; ours places it silently.
- **Severity.** low (message).
- **Already known?** No.

### C6-7 (low). The ghost is drawn at the grid / anchor point, not where `SnapItem` will put the via **Open**
- **What differs.** `painter.ts:1327-1331` (`drawViaGhost`) draws at `opts.snappedCursor ?? snapPoint(cursor)`. KiCad's preview item is moved by `SnapItem` on every motion (`doInteractiveItemPlacement` calls `m_placer->SnapItem( newItem.get() )` before updating the preview), so it sits on the track (`AlignToSegment` point), on the pad's centre or on the graphic as it will be placed.
- **How to see it.** Draw Via, hover 0.2 mm off a track's axis between grid points: KiCad's preview sits on the track; ours sits on the grid point and jumps on the click.
- **Severity.** low (cosmetic).
- **Already known?** No.

### C6-8 (low). The "Select Net:" menu: no-net entry blank and the order is not KiCad's **Fixed** on task-r6-pcb-edit: 4ab0c643 (the `<no net>` label; the order by net code is Open)
- **What differs.** `selectNetLabels` (`viaPlacer.ts:253`) prints a no-net pour as `"1  "`; 25ec2335's `selectPossibleNetsByPopupMenu` prints `NO_NET` (`_( "<no net>" )`, `pcbnew/netinfo.h:42`): `wxString menuText = netcode ? netInfo.GetNetItem( netcode )->GetNetname() : NO_NET;`. The entries are in fill order (`stitchedZoneNets`' insertion order); KiCad's `std::set<int> aNetcodeList` lists them by net code.
- **How to see it.** Two overlapping pours under the via, one on no net: ours shows a blank entry.
- **Severity.** low.
- **Already known?** No.

### C6-9 (low). The activity log writes `via place --net .. --at ..`, which `eda board via` does not accept **Fixed** on task-r6-pcb-edit: 475fee76
- **What differs.** `crates/cli/src/board.rs:1750` (`cmd_line`, "The command as it would be typed after `eda board`") renders `Cmd::PlaceVia` as `via place ...`; the `via` verb parses only `add|delete|move` (`board.rs:2510`: `"usage: eda board via <add|delete|move> ..."`). No CLI verb places a via the Draw Via way (snap, net choice, DRC refusal, split).
- **How to see it.** Place a via with Draw Via in the studio, copy the `cmd` from `activity.jsonl`, run `eda board via place ...`: usage error.
- **Severity.** low.
- **Already known?** No.

### Checked and found to match (do not re-review)
- `CreateItem`: `m_CurrentViaType` is only ever THROUGH (`board_design_settings.cpp:69`, nothing else assigns it), so a through via from the first to the last copper layer; the micro / blind branches are dead in 8303b2ad.
- `SnapItem`: the order tracks, pads, graphics; each gated by its magnetic setting (`NO_EFFECT` = `"never"`) and by `GetSnap()` (Shift); the cursor's raw position for the search and the grid position kept when nothing snaps; `AlignToSegment` on the track's chord (also for an arc); a pad's position; a filled graphic snapped to its position only when `viaArea * 4 > shapeArea`, else left; a segment's `AlignToSegment`; an arc's nearer end (`<` on squared distances); a polygon's nearest edge; anything else its `GetPosition()` (arc centre, polygon's first vertex, else start).
- `findTrack`: `TestSegmentHit` reach `(track width + via width) / 2`; `PCB_ARC::HitTest`'s `max_dist = accuracy + width / 2` with the end short-cut and the sweep test; the nearest by `SEG( start, end ).NearestPoint`. `findPad`: `PAD::HitTest( aPosition )` on a pad sharing a layer. `findGraphic`: copper only, `HitTest( pos, viaWidth / 2 )`, nearest by `GetPosition()`.
- `PlaceItem`'s net: track, then pad, then a graphic with a net > 0, then `findStitchedZoneNet` (the active layer's fills first in high-contrast mode, kept when one net; then the visible layers of the via, rule areas skipped; one net taken, several asked, none = no net); a cancelled menu places nothing.
- The split: only with snap on; not when the via is on the segment's start or end or off it (`SEG::Contains`, squared distance <= 3); the first part keeps the id, the second is new; the clone's lock carried; one undo step.
- `checkDRCViolation` / `hasDRCViolation`: copper zones skipped; rule areas with no vias by `Outline()->Collide( pos, width / 2 )` (area semantics); a connected item of net 0 or of the via's net skipped (a via of no net is checked against netted items); clearance per copper layer of the other item less `GetDRCEpsilon()` (500 nm, `sub_e` floored at 0), only when `clearance >= 0`; the other's hole at `HOLE_CLEARANCE`; hidden footprint texts skipped; the `DISALLOW_CONSTRAINT` at `UNDEFINED_LAYER` (implicit keepout rules include footprints' keepouts, `constraints.rs:896`).
- "Via location violates DRC." word for word; `m_allowDRCViolations` = `RM_MarkObstacles && m_allowDRCViolations` (`pns_routing_settings.h:117`), as `Canvas.tsx:776` sends it.
- `IPO_REPEAT | IPO_SINGLE_CLICK`: the tool stays armed after a placement and after a refusal; Escape leaves the tool at once (`escapeKeepsDrawingTool` false for the via tool, which has no draw state).
- 25ec2335 changes nothing else in this path (helpers moved to `FindSnapTrack` / `FindSnapPad` / `FindSnapGraphic` / `CheckItemDRCViolation`; `sizeMicrovia` only for microvias).

---

# Part D: teardrops, Cleanup Graphics, Create from Selection

Read-only. Main's checkout at `7d772e6e`. KiCad `8303b2ad` (`$KICAD_SRC_DIR`) and `25ec2335` (`~/src/kicad-src-25ec2335`).
The installed kicad-cli is `10.99.0-unknown-25ec2335f2`. Scratch: `/tmp/findings-r6-pcb-edit.GQUt/td/td.kicad_pcb` (the kicad-cli run for D6-1).

---

### D6-1 (medium). Teardrops mix two KiCad versions: 8303b2ad's filters reject tracks that 25ec2335 gives a teardrop, while the arc geometry and the proof come from 25ec2335 **Fixed** on task-r6-pcb-edit: 1d029c43 (25ec2335 throughout; `computeFittedTeardropPolygon` and the mask teardrops Open)
- **What differs:**
  - **Ours** (`crates/connectivity/src/teardrop.rs`):
    - `:557-563`: `computeEmergingTrackLength`, the 8303b2ad filter. A track whose part outside the anchor is shorter than its width gets no teardrop. Arcs are filtered too: their crossing nearest the far end is measured in a straight line.
    - `:535`: `tw >= settings.max_width_nm`, with no `> 0` guard. A Max width of 0 gives no teardrops at all.
    - Lines 588-625 port 25ec2335 for arcs: `NearestPoint`, the `maxError` arc half-width and the off-centre C/E. The module doc says this is "as the kicad-cli 25ec2335 this is proven against does".
  - **KiCad 25ec2335** (`pcbnew/teardrop/teardrop.cpp::UpdateTeardrops`): `computeEmergingTrackLength` is gone. Its replacement is `computeChordThroughShape`, which measures the chord on the *extended* centreline and leaves arcs out:
    ```
    // Reject tangential grazes, but keep short radial entries.
    if( startHitsVia != endHitsVia
        && computeChordThroughShape( track, via, track->GetLayer(), startHitsVia ? track->GetStart() : track->GetEnd() ) < track->GetWidth() )
    ```
    and in `teardrop_utils.cpp::computeChordThroughShape`:
    ```
    // Arcs are genuine entries, not the short straight grazes this filter targets.
    if( aTrack->Type() == PCB_ARC_T )
        return std::numeric_limits<int>::max();
    ```
    A radial track's chord is the anchor's diameter, so it is never rejected. The width filter is `( tdParams.m_TdMaxWidth > 0 && track->GetWidth() >= tdParams.m_TdMaxWidth )` ("A max width of 0 means no limit, not "nothing fits"").
  - **KiCad 8303b2ad** has both of our filters (`teardrop.cpp:282,301,341,359`). So the generator agrees with neither version: on a short radial track or a short arc it follows 8303b2ad, and on an arc's geometry it follows 25ec2335.
- **How to see it:** confirmed with kicad-cli 25ec2335 on `/tmp/findings-r6-pcb-edit.GQUt/td/td.kicad_pcb` (`pcb drc --refill-zones --save-board`), with the PARAMS of `teardrops_kicad_cli.rs`:
  - A 0.6 mm via at (100, 100) and a 0.2 mm segment to (100.35, 100), 0.05 mm outside the via. KiCad makes the teardrop `(100.35 100.1) (100.35 99.9) (100.182628 99.761994) (99.996433 100) (100.182628 100.238006)`. Ours returns `None` at `:561`.
  - A 0.6 mm via at (110, 100) and a 0.2 mm arc (110, 100) -> (110.25, 100.1) -> (110.4, 100.05). KiCad makes a teardrop; ours gives none.
  - Test to add, as cases in `teardrops_kicad_cli.rs`: `(100.0, 100.0, 0.6, 100.35, 100.0, 0.2)` in `CASES` and `(110.0, 100.0, 0.6, 110.25, 100.1, 110.4, 100.05, 0.2, false)` in `ARC_CASES`. Today `assert_eq!(ours, kicad)` fails (one outline short each). Also a unit test: `max_width_nm = 0` should still give teardrops.
  - The proof as it stands is sound for its 12 cases: same board, kicad-cli's own outlines, point-for-point equality, and a count check on the KiCad side. But every case emerges by more than the track width, so it cannot see this.
- **Fix direction:** choose one version. 25ec2335 is the one the proof runs against, so port `computeChordThroughShape` (arcs exempt), the `m_TdMaxWidth > 0` guard, and the other 25ec2335 filter changes. Then fix the module doc and the PARITY row (section 60 lists `computeEmergingTrackLength` as a source).
- **Severity:** medium. A teardrop zone is missing where KiCad, rebuilding the same board, makes one. It needs short or arc entries, or Max width 0.
- **Already known?** No. GAPS 13 and the round-5 "Left" list (round anchors, one segment, `CalcArcCenter`'s snap) do not cover it.

### D6-2 (medium). Create Zone / Rule Area from Selection opens two dialogs; KiCad's is one zone dialog with a "Conversion Settings" box, titled "Convert to ..." **Fixed** on task-r6-pcb-edit: 810fd552
- **What differs:**
  - **Ours:** `web/studio/src/actions/pcbConvertSweep.ts:77-117` first opens our `ConversionSettingsDialog`. Its title is "Conversion Settings", and with the hull strategy it shows Gap *and* "Line width:" (`components/PcbConvertDialogs.tsx:53-54`). The zone dialog comes after (`SET_ZONE_PENDING`), titled "Zone Properties -- New Zone", "Non-copper Zone Properties -- New Zone" or "Rule Area Properties -- New Rule Area" (`components/ZoneDialog.tsx:256-260`).
  - **KiCad** (`pcbnew/tools/convert_tool.cpp::CreatePolys`) opens only the zone editor and passes `&m_userSettings`: `InvokeRuleAreaEditor( frame, &zoneInfo, board(), &m_userSettings )`, `InvokeNonCopperZonesEditor( frame, &zoneInfo, &m_userSettings )` or `InvokeCopperZonesEditor( frame, nullptr, &zoneInfo, &m_userSettings )`. It then runs `getPolys( m_userSettings )` after OK.
  - That dialog (`dialogs/dialog_copper_zones.cpp::DIALOG_COPPER_ZONE`; the same in `dialog_non_copper_zones_properties.cpp` and `dialog_rule_area_properties.cpp`) adds a box `_( "Conversion Settings" )` with "Use centerlines", "Create bounding hull", "Gap:" and "Delete source objects after conversion". It has no line width. Its title is `_( "Convert to Copper Zone" )`, `_( "Convert to Non Copper Zone" )` or `_( "Convert to Rule Area" )`.
- **How to see it:** select a closed polygon with F.Cu active and run Create Zone from Selection. KiCad shows one dialog, "Convert to Copper Zone", with the conversion box. Ours shows "Conversion Settings" and then "Zone Properties -- New Zone".
  - Browser check: after the action, assert exactly one dialog, titled "Convert to Copper Zone", with the radio buttons "Use centerlines" and "Create bounding hull".
- **Severity:** medium (a wrong dialog sequence and wrong titles; the extra "Line width:" field does nothing for zones).
- **Already known?** No. Round 5 matched only the stand-alone "Conversion Settings" dialog, which is right for Create Polygon.

### D6-3 (low). Create Polygon / Zone from a rounded rectangle: the outline is `ROUNDRECT::TransformToPolygon`'s; KiCad's filled path is `TransformRoundChamferedRectToPolygon` **Open**
- **What differs:**
  - **Ours:** `crates/ops/src/convert.rs:185-190` calls `rounded_rect_ring` (`crates/model/src/outline.rs:146`). Each corner is flattened at `maxError / 5`, with polyline points half that error outside the arc. The doc comment cites `TransformRoundChamferedRectToPolygon` but implements `ROUNDRECT`.
  - **KiCad:** `makePolysFromClosedGraphics` sets the shape filled for every strategy except the hull (`if( aStrategy != BOUNDING_HULL ) shape->SetFilled( true );`). So `EDA_SHAPE::TransformShapeToPolygon` (`common/eda_shape.cpp`) takes the `solidFill` branch: `TransformRoundChamferedRectToPolygon( aBuffer, position, size, ANGLE_0, GetCornerRadius(), 0.0, 0, width / 2, aError, aErrorLoc )`. That function's `CornerListToPolygon` works at `aError` (`m_MaxError`), `ERROR_INSIDE`: fewer vertices, all on or inside the arc.
- **How to see it:** a 10 x 5 mm rectangle with 2 mm corners, Create Polygon (centerlines). KiCad gives about 11 segments per corner, with vertices on the circle. Ours gives about 25 per corner, slightly outside it. The written `(pts ...)` differ.
  - Test: `polys_from_items_keeps_a_rounded_rectangles_corners` should compare the ring with `pad_geom.rs`'s `corner_list_to_polygon` (the active port of `CornerListToPolygon` for rounded-rect pads) at `MAX_ERROR_NM`.
- **Severity:** low (sub-micron geometry, a different vertex count in the file).
- **Already known?** No.

### D6-4 (low). The 100 nm chaining joins a near-miss whichever way the item is drawn; KiCad's walk joins it only when the near end is the far end it enters by **Open** (a KiCad quirk)
- **What differs:**
  - **Ours:** `crates/ops/src/convert.rs:277-300`, `Chainer::new`, moves each end onto the first key within 100 nm. The comment says "KiCad's walk ... drops the near-miss point the same way".
  - **KiCad** (`convert_tool.cpp::makePolysFromChainedSegs`): `findInsertionPoint` is used only when filing an end into `connections`. The walk looks up the item's *real* end, `for( std::pair<int, EDA_ITEM*> pair : connections[nextAnchor] )`, and `insert` picks the point to append by exact equality, `VECTOR2I& point = ( aAnchor == nextSeg->A ) ? nextSeg->B : nextSeg->A;`.
  - When the near-miss end is the item's A, entering it under the key appends A (the near-miss point itself). The next lookup is then `connections[A]`, which is empty, so the chain breaks.
- **How to see it:** the triangle of `ends_within_100_nm_chain`, with the last segment reversed: `seg(off, p(5.0, 8.0))`, `off` = (50 nm, 0). Tracing KiCad's walk from each of the three candidates gives open chains: [(10,0), (0,0), (0.00005,0)], [(5,8), (10,0), (0,0), (0.00005,0)] and [(5,8), (0.00005,0)]. So KiCad says "Could not convert selection" / "Objects must form a closed shape", while ours makes the triangle.
  - Test: `a_reversed_near_miss_does_not_chain`, asserting `rings.is_empty()`.
- **Severity:** low (ends 1-100 nm apart; KiCad's own behaviour depends on the drawing direction).
- **Already known?** No. B5-13 is marked Fixed with this claim.

### D6-5 (low). Fix discontinuities in board outlines leaves out the shapes Delete redundant graphics just removed; KiCad's `fixBoardOutlines` walks them too **Open** (a KiCad quirk; kept as a difference)
- **What differs:**
  - **Ours:** `web/studio/src/kicad-port/pcbGlobalEdit.ts:311` (`shapes.filter((s) => s.layer === "Edge.Cuts" && !deleted.has(s.id))`) heals only the shapes that survive.
  - **KiCad** (`pcbnew/graphics_cleaner.cpp::fixBoardOutlines`) takes every Edge.Cuts `PCB_SHAPE` of `m_drawings`, with no `IS_DELETED` test:
    ```
    for( BOARD_ITEM* item : m_drawings ) { PCB_SHAPE* shape = dynamic_cast<PCB_SHAPE*>( item ); if( !shape || !shape->IsOnLayer( Edge_Cuts ) ) continue; shapeList.push_back( shape ); ...
    ```
    `cleanupShapes` only did `m_commit.Remove` and `SetFlags( IS_DELETED )` (null shapes are not even flagged). So the removed duplicate is still in `ConnectBoardShapes`' kd-tree.
  - `findNext` takes the 2 nearest end points: the line's own and its duplicate's, both at distance 0. The walk then hops to the duplicate, `connectPair` finds no `IntersectLines` for parallel segments, and the line's real neighbour across the gap is never reached. In KiCad, a duplicated outline line therefore blocks the healing of the gaps at its ends; ours heals them.
- **How to see it:** Edge.Cuts with segment L twice (identical), plus a neighbour whose end is 0.5 mm from L's end. Tick Delete redundant graphics and Fix discontinuities (tolerance 2 mm). KiCad removes the duplicate and leaves the gap; ours removes it and closes the gap.
  - Test (`pcbGlobalEdit.test.ts`): `the outline fix still sees a removed duplicate, as fixBoardOutlines does`, expecting no `replace` for L.
- **Severity:** low (a KiCad quirk; matching it makes the result worse, so the fixer may prefer to record it as a deliberate difference).
- **Already known?** No.

### D6-6 (low). Cleanup Graphics toasts "Nothing to clean up." and "Graphics cleaned up."; KiCad's OK says nothing **Fixed** on task-r6-pcb-edit: 475fee76
- **What differs:**
  - **Ours:** `web/studio/src/actions/pcbGlobalEditSweep.ts:76` and `:79`.
  - **KiCad:** `DIALOG_CLEANUP_GRAPHICS::doCleanup( false )` pushes the commit when it is not empty (`commit.Push( _( "Cleanup Graphics" ) )`), refreshes, and shows no message.
- **How to see it:** Cleanup Graphics with nothing to do, then OK. Ours shows the toast; KiCad closes silently.
- **Severity:** low (a message KiCad does not have).
- **Already known?** No (B5-7 dealt with the disabled OK).

---

## Checked and found to match (do not re-review)
- **Arc teardrops (a98857b6, against 25ec2335):**
  - `findAnchorPointsOnTrack`'s arc path: the crossing is the outline's `Intersect` with `SHAPE_ARC( start, mid, end, width ).ConvertToPolyline( maxError )` in the arc's own orientation (`pts[0]`). Their segments are sorted by min x and the outline edges walked in order, as `SHAPE_LINE_CHAIN::Intersect( const SHAPE_LINE_CHAIN& )`.
  - `actualTdLen = min( target, SEG( start, end ).Length() )` on the chord.
  - `arc.Reverse()` when the anchor is at the arc's end. The points are moved onto the arc by `NearestPoint`: `CIRCLE::NearestPoint`'s `Resize`, the `s_epsilon = 8` end snaps, and `sliceContainsPoint`'s normalisation.
  - The backward search for `dist_from_start < actualTdLen || ii == 0`, and the `actualTdLen` reduction and clamps.
  - `ConvertToPolyline`: the external radius, `halfMaxError`, `seg360`, `CircleToEndSegmentDeltaRadius`, the odd-index points at `r + effectiveError / 2`, and `Append`'s dropping of repeated points.
  - The arc half-width `max( w / 4, w / 2 - maxError )`.
  - D along `vecVia = vecT` with the round clamp.
  - The off-centre C/E (`perpVia`, the `0.1 * padRadius` threshold, `maxSymmetric`, `preferred_width`, `m_TdMaxWidth / 2`, `symHalfWidth > track_halfwidth`, `findCircleLineIntersection`'s nearer root, the `-sym` for C and `+sym` for E).
  - The proof test (`teardrops_kicad_cli.rs`): it compares kicad-cli's saved outlines with ours point for point, on the same imported board, with the same parameters (the defaults equal PARAMS).
- **Cleanup Graphics (4e6628bf):**
  - `m_epsilon = GetDRCEpsilon()` as 500 nm, `<` on both axes, the arc centres included.
  - `fixBoardOutlines` only outside the dry run. The shapes are moved in place (`replace_shape` keeps the id, the drawing order, the lock and, through the overlays, the group and the line style). A healed line that `mergeRects` takes is removed, not replaced.
  - `mergeRects` sees the healed coordinates and skips null shapes.
  - `ConnectBoardShapes`' walk: `findNext`'s two nearest, `SKIP_STRUCT`, `beginFromEnd`, and `connectPair` for segment, arc and Bezier pairs. It adds no shape.
  - OK always runs, and is disabled only for a bad tolerance.
  - The selection is cleared, and the edit is one step.
- **Create from Selection (fb658ba2):**
  - `CreateLines` on a rounded rectangle: `ROUNDRECT( rect, radius, true ).TransformToPolygon`, sides as lines and corners as arcs.
  - The default widths `DEFAULT_LINE_WIDTH` 0.1 mm (`PCB_SHAPE` constructor) and 0.2 mm (`PCB_TRACK` constructor), applied only when the source has no width (`aWidth && *aWidth > 0`).
  - `SegmentToArc`'s mid: `Perpendicular()` = (-y, x), `SEG::Length()` rounded, `Resize` of the truncated `offsetRatio * length`, and `SEG::Center()` truncated.
  - `EDA_SHAPE::getPosition` (an arc's centre, a polygon's first vertex, otherwise the start) for Copy line width.
  - Outset's `outset_params_pcb_edit`: 1 mm, rounded corners, source layers and widths, Edge.Cuts, `GetLineThickness( Edge_Cuts )`, no grid rounding. The corner radius becomes `max( cornerRadius + d, 0 )` when rounding.
  - The non-copper zone is chosen by `IsNonCopperLayer( destLayer )`. A zone gets no copy-line-width strategy, and the gap is not grown by the line width.
  - The zones and the removal of the sources are held as one step and closed by the removal or a bare commit.

---

# Part E: rows no round reviewed

Read from main's checkout (HEAD `7d772e6e`) against `8303b2ad`. Nothing built or run (main's `target/debug/eda` does not exist); every finding is read from the code. Ours: `web/` = `web/studio/src/`.

Features chosen (grep of rounds 2-5 and p1..p25 found no review of them): Swap (`EDIT_TOOL::Swap`, Alt+S), Move / Copy with Reference and the Interactive Offset tool's entry, Lock / Unlock / Toggle Lock (`modifyLockSelected`), Select / Grab Unconnected (`selectUnconnected`, `grabUnconnected`), Select / Deselect All Tracks in Net (`selectNet`), Items in Same Hierarchical Sheet (`selectSameSheet`, `selectAllItemsOnSheet`, `selectConnections`). On the way, the routing that the studio's Pack and Move drop and the placement of an "armed" part clear (E6-1).

---

### E6-1 (high). Swap (Alt+S), the Pack and Move drop and placing a part from the unplaced list delete the whole board's routing: every track, via and zone **Fixed** on task-r6-pcb-edit: eee102c0
- **What differs.**
  - **Ours.** The studio sends part verbs that `Cmd::clears_routing` answers `true` for:
    - Swap: `web/actions/useActionRunner.ts:1887-1894` sends `{ op: "swap_chain", parts }`.
    - Pack and Move: the move preview is `kind: "part"` (`useActionRunner.ts:2271`), so the drop goes to `commitMove`'s part branch, which sends `move_to` per footprint (`web/state/store.tsx:2375-2381`).
    - A part placed from the Properties panel's unplaced list: `placeArmedAt` sends `place_at` (`store.tsx:2414-2420`).
  - `crates/ops/src/lib.rs:2585-2600` lists `Cmd::SwapChain`, `Cmd::MoveTo`, `Cmd::PlaceAt` (and `Swap`, `Nudge`, `Rotate`, `Flip`, `MoveExact`) in `clears_routing`. `clears_routing_in` (`lib.rs:2610-2616`) exempts only `MoveItems / RotateItems / FlipItems / MirrorTracks / MirrorItems / Rip / CreateArray`. The answer does not depend on whether anything was routed to the parts.
  - `crates/cli/src/board.rs:1572` reads it and `board.rs:1660` does `design.routing.take()`. `RoutingSection` (`crates/model/src/ir.rs:1141-1150`) holds `tracks`, `vias`, `zones` and the track-width list, so all of it goes.
  - The strict gate (`board.rs:1610`, the studio's default `strict: true`, `store.tsx:1017`) compares failures with the routing still in place. So it refuses a swap that breaks a routed net ("that move takes the board from N failure(s) to M (...); refused because --strict"). A swap of two parts with nothing routed to them passes the gate, and then the saved design has no routing at all. With Strict off, every swap deletes it.
  - PARITY-pcb section 22 (row "When a footprint edit clears the routing", line 1049) says the item verbs "never clear it… as KiCad keeps every track where it is". The studio verbs above are not item verbs.
- **KiCad.** `edit_tool_move_fct.cpp::EDIT_TOOL::Swap` only does `commit->Modify( item, nullptr, RECURSE_MODE::RECURSE )` on the selection, then `a->SetPosition( bPos )` and so on, and pushes `_( "Swap" )`. No track, via or zone is touched. `PackAndMoveFootprints` runs `SpreadFootprints` and then `doMoveSelection`, again only the footprints. Placing a footprint (`BOARD_EDITOR_CONTROL::PlaceFootprint`) adds one footprint.
- **How to see it.** Take a board with routing, including a GND zone. Select two resistors with no tracks to them and press Alt+S. In KiCad the two swap. In ours they swap and every track, via and zone of the board is gone; Undo brings it back. The same happens with P (Pack and Move) and a click to drop.
  - Failing test (`crates/cli`, beside the `step` tests): a design with R1, R2 placed and unrouted, plus one track and one zone on another net. Run `step_with(dir, Cmd::SwapChain { parts: ["R1","R2"] }, Strictness::Off, "ui")`. Assert the saved design's `routing.tracks.len() == 1` and `routing.zones.len() == 1`. Add the same for `Cmd::MoveTo` and `Cmd::PlaceAt`. Better: assert `!Cmd::SwapChain{..}.clears_routing_in(&b)` next to `a_routed_footprint_moved_turned_flipped_or_deleted_leaves_the_routing_in_the_design` (`crates/ops/src/pcb_edit_details_tests.rs:321`).
- **Fix direction.** Have the studio send item verbs: Swap as `move_items` / `rotate_items` / `flip_items` per item in one batch, Pack and Move as `move_items`. Or add `SwapChain` to `clears_routing_in`'s `false` arm.
- **Severity:** high (data loss: all copper and zones; or a refusal KiCad does not make).
- **Already known?** No. PARITY line 1098 ("Moving, turning or flipping a footprint with copper routed to its pads clears the routing") is also stale for the Properties panel, which sends `move_items` (`web/kicad-port/pcbProperties.ts:394`).

### E6-2 (medium). Swap takes placed footprints only: vias, pads, graphics, texts, dimensions and groups are not swapped, and same-type items do not swap layers **Fixed** on task-r6-pcb-edit: 205a320c (arc tracks, dimensions, groups, tables, text boxes, barcodes, points and pad-as-pad swaps Open)
- **What differs.**
  - **Ours.** `useActionRunner.ts:1890` filters with `api.partByRef(id)?.placed`. Fewer than two parts means it does nothing. `Cmd::SwapChain` (`crates/ops/src/lib.rs:4319-4337`) moves only footprint poses.
  - **KiCad.** `EDIT_TOOL::Swap` filters out only markers, `PCB_TRACE_T` (straight tracks; arcs stay), locked items, and free pads through `FilterCollectorForFreePads`. Every other item swaps `GetPosition()`. Then:
    ```
    // Pads may have a copper shape offset from the anchor/hole, so swap visible shape
    // centers rather than anchor positions.  See PAD::SwapShapePositions.
    if( a->Type() == PCB_PAD_T && b->Type() == PCB_PAD_T ) { PAD::SwapShapePositions( ... ); ... swap layers; continue; }
    ...
    // We can also do a layer swap safely for two objects of the same type,
    // except groups which don't support layer swaps.
    else if( a->Type() == b->Type() && a->Type() != PCB_GROUP_T ) { ... a->SetLayer( bLayer ); b->SetLayer( aLayer ); }
    ```
- **How to see it.** Select two vias (or two texts on F.SilkS and B.SilkS) and press Alt+S. KiCad swaps their positions, and the texts' layers too. Ours does nothing.
  - Test: `useActionRunner`/e2e: two vias selected, swap. Assert each via has the other's `(x, y)`, all in one undo step "Swap".
- **Severity:** medium.
- **Already known?** No. PARITY-pcb line 309 lists the action only.

### E6-3 (medium). Swap moves locked footprints; KiCad leaves them out **Fixed** on task-r6-pcb-edit: 205a320c
- **What differs.**
  - **Ours.** `useActionRunner.ts:1890` does not look at `board.locked`, and `swap_chain` has no lock check (`lib.rs:4319`).
  - **KiCad.** `EDIT_TOOL::Swap`'s filter ends in `sTool->FilterCollectorForLockedItems( aCollector )`. That removes an item when `item->IsLocked() || lockedDescendant` (`pcb_selection_tool.cpp::FilterCollectorForLockedItems`). Then `if( selection.Size() < 2 ) return 0;`.
- **How to see it.** Lock U1, select U1 and U2, press Alt+S. KiCad does nothing, since one item is left. Ours swaps them.
- **Severity:** medium.
- **Already known?** No.

### E6-4 (medium). Select / Grab Unconnected ignore selected pads **Fixed** on task-r6-pcb-edit: 205a320c
- **What differs.**
  - **Ours.** `web/kicad-port/pcbEditActions.ts:62-66` (`farPadsOf`) and the grab loop (`:105-108`) walk `parts` whose `ref` is in the selection. A selected pad (`U1.3`) matches no part, so it is ignored.
  - **KiCad.** `PCB_SELECTION_TOOL::selectUnconnected` and `grabUnconnected` both take the selection's footprints' pads `else if( item->Type() == PCB_PAD_T ) pads.push_back( static_cast<PAD*>( item ) );`. The context menu offers both on a pad (round 2: "selectUnconnected on footprint/pad").
- **How to see it.** Click one pad of U1 and press O. KiCad selects the footprints at the far end of that pad's ratsnest lines. Ours selects nothing new. With Shift+O, ours clears the selection and starts an empty Move Individually.
  - Test: `pcbEditActions.test.ts`: `selectUnconnectedFootprints(parts, edges, ["U1.1"])` includes `R1` when an edge runs from U1.1 to R1.2.
- **Severity:** medium.
- **Already known?** No.

### E6-5 (medium). Select All Tracks in Net and the connections of Items in Same Hierarchical Sheet / Sync with nets leave out the net's copper graphics; a pad or copper shape gives no net **Fixed** on task-r6-pcb-edit: 205a320c
- **What differs.**
  - **Ours.** `web/kicad-port/pcbSelectionOps.ts:30-36` `netItems` returns tracks and vias only. Its comment says "copper shapes carry no net here", which is stale: `Shape.net` exists (`web/api/types.ts:595-596`, "The net of a shape on a copper layer").
  - `netsOfItems` (`:20-28`) takes a net from tracks, vias and zones only, not from a pad or a copper shape. `selectConnections` (`:332-333`) uses `netItems` too.
  - **KiCad.** `SelectAllItemsOnNet` and `selectConnections` both use `conn->GetNetItems( netCode, { PCB_TRACE_T, PCB_ARC_T, PCB_VIA_T, PCB_SHAPE_T } )`. `selectNet` takes the net of every `dynamic_cast<BOARD_CONNECTED_ITEM*>( i )`, which includes pads and shapes.
  - KiCad also tests `itemPassesFilter( item, true, nullptr )`. Ours tests only the Tracks/Vias rows and a direct lock (`web/actions/pcbEditSweep.ts:142-146`). It misses the Graphics row for shapes and a lock inherited from a locked group (`BOARD_ITEM::IsLocked`'s parent-group branch).
- **How to see it.** Draw a copper polygon on GND, then select a GND track and choose Select All Tracks in Net. KiCad selects the polygon too; ours does not.
  - Test: `pcbSelectionOps.test.ts`: `netItems(board_with_gnd_shape, {"GND"})` contains the shape id.
- **Severity:** medium (the pad source is low: the menu entry is on track types).
- **Already known?** No.

### E6-6 (low). Items in Same Hierarchical Sheet needs exactly one selected item, and says things KiCad does not **Fixed** on task-r6-pcb-edit: 475fee76
- **What differs.**
  - **Ours.** `web/actions/pcbSelectionSync.ts:87` returns unless `state.selection.size === 1`. `:92` toasts "`<ref>` is not on any schematic sheet.", and `:97` toasts "Could not read the schematic sheets."
  - **KiCad.** `selectSameSheet` takes `EDA_ITEM* item = m_selection.Front();`, the first selected item, whatever the size. It returns silently when that item is not a footprint, or when `footprint->GetPath().empty()`.
- **How to see it.** Select R1 and R2 (same sheet) and choose Select > Items in Same Hierarchical Sheet. KiCad selects the sheet's footprints and their connections; ours does nothing.
- **Severity:** low.
- **Already known?** No.

### E6-7 (low). Move with Reference, Copy with Reference and the Interactive Offset tool toast messages KiCad does not have; "Selection copied" is shown before the copy answers **Fixed** on task-r6-pcb-edit: 475fee76
- **What differs.**
  - **Ours.** `web/actions/pcbReferenceSweep.ts:35` "Select an item to move.", `:62` "Select something to copy.", `:93` and `:104` "Select an item first.". `:69-70` toasts "Selection copied" right after `api.copySelection(ids, at)` without awaiting it, and `copySelection` can fail with its own error toast (`store.tsx:2574-2576`).
  - **KiCad.**
    - `doMoveSelection`: `if( selection.Empty() ) { m_selectionTool->ReportFilteredLockedItems(); return false; }`. Its only message is the info bar "Selection contains locked items. Enable 'Override locks' to operate on them.", and only when locked items were filtered.
    - `copyToClipboard`: nothing when the selection is empty.
    - `POSITION_RELATIVE_TOOL::InteractiveOffset`: `if( selection.Empty() ) return 0;`.
    - None of the three strings exists in the source tree (grep).
- **Severity:** low.
- **Already known?** No.

### E6-8 (low). Toggle Lock and the locked filter of the sweep actions do not see a lock inherited from a locked group **Fixed** on task-r6-pcb-edit: 205a320c
- **What differs.**
  - **Ours.** `resolveToggleLock` (`web/kicad-port/pcbEditActions.ts:21-24`) and `requestFiltered` (`web/actions/pcbSweepKit.ts:55-60`) test `board.locked` for the item's own id only.
  - **KiCad.** `BOARD_ITEM::IsLocked`: `if( EDA_GROUP* group = GetParentGroup() ) { if( group->AsEdaItem()->IsLocked() ) return true; }`. `FilterCollectorForLockedItems` also drops an item with a `lockedDescendant`.
- **How to see it.** Lock a group, enter it, select a member and press the Toggle Lock hotkey. KiCad resolves the toggle as OFF, because the member reports locked. Ours resolves it as ON.
  - Move with Reference on an unlocked group with a locked member: KiCad filters the group out; ours moves it.
- **Severity:** low.
- **Already known?** Partly: round 2 noted `lockedDescendant` for the group itself, not for these entry points.

---

## Checked and found to match (do not re-review)
- **Swap:**
  - the cyclic result of KiCad's pairwise `swap(sorted[i], sorted[i+1])`, including orientation and side (`Flip` then `SetOrientation`);
  - selection order;
  - one undo step.
- **Move with Reference:**
  - the prompt "Select reference point for move..."; on cancel, a hover selection is cleared;
  - the picked point becomes the move origin (`selection.SetReferencePoint( pickedReferencePoint )`, `m_cursor = pickedReferencePoint`);
  - locked items are filtered.
- **Copy with Reference:**
  - "Select reference point for the copy...", "Selection copied", "Copy canceled" word for word;
  - locks not filtered (only Cut filters them);
  - the point becomes the clipboard's reference point.
- **Lock / Unlock / Toggle Lock:**
  - empty selection falls back to the item under the cursor;
  - TOGGLE becomes OFF when any selected item is locked;
  - pads skipped;
  - groups, dimensions and tables lockable;
  - one step.
- **Select / Grab Unconnected (footprints):**
  - only ratsnest edges whose far end is a pad;
  - grab skips loop edges on the same footprint and keeps the nearest by strict `<`;
  - the selection is cleared, then Move Individually runs;
  - pad anchors are `PAD::GetPosition` (`connectivity_items.cpp:68`).
- **Select Net / Deselect Net:**
  - `selectCursor` fallback;
  - adds or removes per net;
  - the Tracks/Vias filter rows.
- **`selectConnections`:**
  - `selectAllConnectedTracks( padList, STOP_AT_PAD )`;
  - a net is dropped when any of its pads is not in the list.


---

# Summary

| # | Finding | Severity |
|---|---|---|
| E6-1 | Swap, the Pack and Move drop, and placing an unplaced part all go through `clears_routing` and delete the whole board's routing (tracks, vias, zones) | high Fixed |
| A6-1 | The stepped line width is reset whenever a tool is armed; KiCad keeps it until the layer changes | medium Fixed |
| A6-2 | A board graphic polygon is always filled and takes the stepped width; KiCad leaves it unfilled on Edge.Cuts and the courtyards, at the layer width, with a solid line | medium Fixed |
| A6-3 | A new text is never bold: `InferBold` is not ported, so copper text at the defaults (1.5 mm / 0.3 mm) differs | medium Fixed |
| C6-1 | Draw Via snaps to, takes the net of, splits and is refused by items on hidden layers | medium Fixed |
| C6-2 | Draw Via uses the cached via preset, not the board's list entry by index | medium Fixed |
| C6-3 | Via DRC check ignores a copper text's valign, glyph width, bold and knockout | medium Fixed |
| D6-1 | Teardrops mix 8303b2ad's filters with 25ec2335's arc geometry; kicad-cli makes teardrops we skip (short emerging track, Max width 0) | medium Fixed |
| D6-2 | Create Zone / Rule Area from Selection opens two dialogs; KiCad opens one "Convert to ..." dialog | medium Fixed |
| E6-2 | Swap takes footprints only; no layer swap for items of the same type | medium Fixed |
| E6-3 | Swap moves locked footprints | medium Fixed |
| E6-4 | Select / Grab Unconnected ignore selected pads | medium Fixed |
| E6-5 | Select All Tracks in Net / selectConnections leave out copper shapes | medium Fixed |
| A6-4 | Ctrl++ / Ctrl+- step the drawing width with no drawing tool armed | low Fixed |
| A6-5 | The line mode is not remembered across sessions | low Fixed |
| A6-6 | R / F / E do nothing to a carried text (KiCad keeps it selected) | low Open |
| C6-4 | A PTH pad's unflashed layer is skipped by the via check (KiCad checks the hole) | low Open |
| C6-5 | Text boxes, tables, dimensions and barcodes on copper never refuse a via | low Open |
| C6-6 | With "Allow DRC violations" on, the via is placed silently; KiCad still shows the message | low Open |
| C6-7 | The via ghost is not drawn at `SnapItem`'s position | low Open |
| C6-8 | "Select Net:" menu: blank no-net entry, wrong order | low Fixed |
| C6-9 | Activity log line `via place` is not a CLI verb | low Fixed |
| D6-3 | Rounded rectangle to polygon: our corner construction differs from `TransformRoundChamferedRectToPolygon`'s | low Open |
| D6-4 | The 100 nm chaining is direction-blind; KiCad's is not | low Open |
| D6-5 | The outline fix ignores shapes just removed as duplicates (a KiCad quirk; may be a deliberate difference) | low Open |
| D6-6 | Cleanup Graphics shows toasts KiCad doesn't have | low Fixed |
| E6-6 | Same Hierarchical Sheet needs a single selection; toasts KiCad doesn't have | low Fixed |
| E6-7 | Toasts KiCad doesn't have on Move/Copy with Reference; "Selection copied" shown early | low Fixed |
| E6-8 | A lock inherited from a locked group is not seen by Toggle Lock and the sweep filter | low Fixed |

Counts: high 1, medium 12, low 16.
