# PCB editor parity with KiCad pcbnew

One row per behavior/action. Status is **identical** (same constants/logic,
adapted only where the browser platform genuinely requires it, noted
inline), **partial** (core behavior ported, a real gap remains, noted),
or **missing** (not started).

KiCad source snapshot: commit `8303b2ad` (see the scratchpad `kicad-src`
copy's `COMMIT` file), version 10.99.

Pure-logic ports live in `web/studio/src/kicad-port/*.ts`, unit-tested
with `npm run test:unit` (compiles with `tsc -p tsconfig.test.json` to a
temp dir, runs with `node --test`; see `tools/run-unit-tests.sh`).

## 1. View controls

| Behavior | Status | KiCad file:function |
|---|---|---|
| Wheel zoom about the cursor | identical | `common/view/wx_view_controls.cpp:onWheel` (via `kicad-port/viewControls.ts:handleWheel` + `view.ts:zoomAbout`, which implements `view.cpp:VIEW::SetScale`'s anchor-preserving math) |
| Zoom step size/acceleration (mouse wheel) | identical | `common/view/zoom_controller.cpp` (`CONSTANT_ZOOM_CONTROLLER`/`ACCELERATING_ZOOM_CONTROLLER`, ported in `kicad-port/zoomController.ts`); default is `CONSTANT_ZOOM_CONTROLLER(MAC_SCALE)` on macOS, `CONSTANT_ZOOM_CONTROLLER(MSW_SCALE)` elsewhere, matching source's actual *default settings* (zoom_acceleration=false) -- `ACCELERATING_ZOOM_CONTROLLER` is ported and available but only KiCad's real opt-in path, not the out-of-box default |
| "Center on zoom"/pointer-warp when zooming | missing (by design) | `wx_view_controls.cpp:onWheel`'s `IsCursorWarpingEnabled()`/`CenterOnCursor()` branch -- requires warping the real OS pointer; this app never moves the real pointer (hard rule, and impossible from a web page anyway), so only the non-warping anchor-preserving branch is used, unconditionally. Documented in `kicad-port/viewControls.ts`'s header comment |
| Ctrl+wheel = horizontal pan, Shift/Alt+wheel = vertical pan | identical | `wx_view_controls.cpp:onWheel` (`scroll_modifier_zoom`="none", `scroll_modifier_pan_h`="ctrl" defaults) |
| Native horizontal wheel (trackpad/tilt-wheel) = horizontal pan | identical, with a noted DOM/wx translation | `wx_view_controls.cpp:onWheel`'s `axis == wxMOUSE_WHEEL_HORIZONTAL` branch; DOM has no axis flag, so a dominant `deltaX` is used as the proxy -- see `viewControls.ts:handleWheel`'s comment |
| Middle-drag pan | identical | `wx_view_controls.cpp:onMotion`/`onButton` (`drag_middle` default PAN) |
| Right-drag pan (vs. right-click = context menu) | identical | `wx_view_controls.cpp:onButton` (`drag_right` default PAN); `Canvas.tsx`'s `justPannedRef` distinguishes a real drag from a click so the native `contextmenu` event still fires for a plain right-click |
| Edge auto-pan while dragging | identical, default off | `wx_view_controls.cpp:handleAutoPanning`/`onTimer` (border math, acceleration formula) ported in `kicad-port/viewControls.ts:computeAutoPanDirection`/`computeAutoPanStep`, driven by a `requestAnimationFrame` loop in `Canvas.tsx` (source uses a ~4.17ms one-shot `wxTimer`, documented as `AUTO_PAN_TIMER_MS` but not used as a literal delay). Off by default (`input.auto_pan` defaults `false` in source too) -- toggle in the status bar ("autopan" checkbox, since KiCad's own toggle lives in a Preferences dialog this app doesn't have) |
| Home = Zoom to Fit | identical (board outline stands in for KiCad's "worksheet page") | `common/tool/actions.cpp:zoomFitScreen` (`WXK_HOME` non-Mac, `Cmd+0` on Mac -- `useActionRunner.ts`) |
| Ctrl+Home = Zoom to All Objects | identical (same fit target as Home -- no separate "page" concept in this app) | `actions.cpp:zoomFitObjects` |
| F1 / F2 = Zoom In/Out at Cursor | identical factor and anchor; **not** snapped to a zoom-percent preset list | `common/tool/common_tools.cpp:doZoomInOut` (factor exactly `1.3`/`1/1.3`; anchored at the cursor's current world position). Source then snaps to the nearest entry in a separate `zoom_factors` preset list (`doZoomToPreset`) -- not ported, no such preset list exists in this app yet. Mac hotkey is `Cmd+'+'`/`Cmd+'-'`, not F1/F2 -- a real macOS-only override in source, see "Hotkey extraction" below |
| Zoom In/Out (menu, no default hotkey) | identical | `common_tools.cpp:doZoomInOutCenter` (same `1.3` factor, anchored at the view center instead of the cursor) |
| F4 = Center on Cursor | partial (pan only, no pointer warp) | `common_tools.cpp:ZoomCenter` → `view_controls.cpp:CenterOnCursor` (pans so the cursor's world point becomes the new center; source also warps the real pointer to screen-center, not done here -- same "never move the pointer" rule as above) |
| Ctrl+F5 = Zoom to Selection Area (drag-to-zoom-box tool) | missing | `actions.cpp:zoomTool` |
| F5 / Cmd+R = Refresh | identical (a true no-op) | `common_tools.cpp:ZoomRedraw` (`HardRedraw()`); this app has no GAL render cache to invalidate, so there's nothing to do -- registered as a real no-op rather than left unimplemented |
| Full-window vs. small crosshair cursor | identical | `common.Control.cursorFullCrosshairs`/`cursorSmallCrosshairs`, `state.fullscreenCrosshair` (pre-existing) |
| Grid drawing: dots, with coarsen-when-too-dense | identical algorithm, approximated rendering | `common/gal/graphics_abstraction_layer.h:GAL::GetVisibleGridSize` (the coarsen-by-`gridTick`-until-visible loop, exact thresholds) ported as `kicad-port/grid.ts:computeVisibleGridSize`, used by `painter.ts:drawGrid`. The *coarsening threshold* is exact; the *dot shape* is a simplification of source's two-pass stencil-masked-lines trick (no stencil buffer in Canvas2D) -- see `painter.ts:drawGrid`'s header comment |
| Grid drawing: lines / small-cross styles | missing (no style switcher in the UI) | `opengl_gal.cpp:DrawGrid`'s `GRID_STYLE::LINES`/`SMALL_CROSS` branches; `kicad-port/grid.ts` computes visible spacing for any style, but `painter.ts` only ever renders DOTS (KiCad's own shipped default) |
| Major grid line/dot every 10th line | identical | `graphics_abstraction_layer.h:GAL::SetCoarseGrid(10)` (`kicad-port/grid.ts:DEFAULT_GRID_TICK`/`isMajorGridLine`) |
| N / Shift+N grid cycling, real default grid list | identical | `common/settings/app_settings.cpp:APP_SETTINGS_BASE::DefaultGridSizeList` (the Pcbnew/footprint-editor branch -- 22 entries, mil block then mm block, **not** sorted by size; a literal jump from 1 mil to 5.0 mm is in source and preserved) ported as `kicad-port/grid.ts:DEFAULT_PCB_GRIDS_UM`, used by `Toolbar.tsx`'s grid dropdown and `useActionRunner.ts`'s `gridNext`/`gridPrev` |
| Cmd+U / Ctrl+U units toggle | identical | `common.Control.toggleUnits` (pre-existing; this app folds a third "mil" unit into the metric/imperial toggle, noted in `useActionRunner.ts`) |
| Status bar X/Y (absolute cursor position) | identical | `pcbnew/pcb_base_frame.cpp:UpdateStatusBar` (field 2) |
| Status bar dX/dY/dist relative to a Space-settable origin | identical | `common/base_screen.cpp` (`m_LocalOrigin`, default `(0,0)`) + `common/tool/common_tools.cpp:ResetLocalCoords` (Space) + `pcb_base_frame.cpp:UpdateStatusBar` (field 3). Previously this app's dx/dy was (incorrectly) tied to the move tool's own drag anchor; now a separate `state.localOriginUm`, matching source exactly (the move tool never touches `m_LocalOrigin`) |
| Polar coordinate status bar mode | identical (pre-existing) | `common_tools.cpp:ZoomCenter`-adjacent `SwitchPolarMode`/`DisplayGridMsg` family; `state.polar` |
| Zoom gesture (trackpad pinch) / pan gesture | missing | `wx_view_controls.cpp:onZoomGesture`/`onPanGesture` (wx-specific touch gesture events; not planned -- wheel/trackpad scroll already covers the common case) |
| Click-and-drag zoom (hold middle/right + drag configured to ZOOM instead of PAN) | missing | `wx_view_controls.cpp:onMotion`'s `DRAG_ZOOMING` state (`drag_middle`/`drag_right` = ZOOM is a non-default KiCad setting; this app only implements the default PAN behavior for those buttons) |

### Hotkey extraction fix (foundational, affects every area below too)

`tools/lib/actionsParser.js` mis-parsed every KiCad action whose default
hotkey is wrapped in `#if defined( __WXMAC__ ) ... #else ... #endif`: the
old regex greedily spanned both branches and spliced one platform's
modifier onto the other platform's key. Fixed (`extractPlatformRaw`, with
a regression test in `tools/lib/actionsParser.test.js`) and
`src/kicad/actions.json` regenerated. Corrected entries: `zoomIn`
("Ctrl+F1" → **F1**, mac **Cmd+'+'**), `zoomOut` ("Ctrl+F2" → **F2**, mac
**Cmd+'-'**), `zoomRedraw` ("Ctrl+F5" → **F5**, mac **Cmd+R**),
`zoomFitScreen` ("Ctrl+Home" → **Home**, mac **Cmd+0**),
`common.Interactive.redo` (**Ctrl+Y**, mac **Cmd+Shift+Z** -- was
"Ctrl+Shift+Z" on *every* platform before), `common.Interactive.delete`
(**Del**, mac **Backspace** -- was plain "Del" everywhere, no mac
override at all before). Also fixed: a bare `' '` (space) char hotkey
normalizing to the literal string `" "` instead of `"Space"`
(`common.Control.resetLocalCoords` and two others), which meant Space
could never actually match a real keydown. New `KicadAction.macHotkey`/
`macAltHotkey` fields (`src/kicad/types.ts`) carry a *genuine* macOS-only
override (not just the usual Cmd-for-Ctrl substitution);
`actions/hotkeys.ts:effectiveHotkey()` is the one place every reader
(global hotkey dispatch, menu bar, toolbar tooltips, the hotkeys list
dialog) resolves `hotkey` vs. `macHotkey` the same way. Also fixed in the
same file: `displayHotkey`'s Mac symbol conversion left a stray "+" before
the final key (e.g. "Ctrl+Shift+Z" → "⌘⇧+Z" instead of "⌘⇧Z"), and would
have swallowed a literal "+"/"-" key entirely under a naive fix -- replaced
with an anchored-prefix regex, tested against exactly that case.

## 2. Grid snapping

| Behavior | Status | KiCad file:function |
|---|---|---|
| Grid round-off with an arbitrary origin | identical | `common/tool/grid_helper.cpp:GRID_HELPER::computeNearest`/`Align` (`kicad-port/gridSnap.ts:computeNearest`/`alignToGrid`) |
| Ctrl disables the grid round-off (move at full precision) | identical | `include/tool/tool_event.h:TOOL_EVENT::DisableGridSnapping` (`Modifier(MD_CTRL)`), `pcbnew/tools/edit_tool_move_fct.cpp` (`grid.SetUseGrid(... && !evt->DisableGridSnapping())`) -- **corrects the task brief's "Shift disables the grid":** source shows Ctrl gates the grid, Shift gates anchor snapping (next row); verified directly against `edit_tool_move_fct.cpp` lines 1048-1049, quoted in `gridSnap.ts`'s header comment |
| Shift disables anchor snapping (grid round-off unaffected) | identical | `edit_tool_move_fct.cpp` (`grid.SetSnap(!evt->Modifier(MD_SHIFT))`) |
| Anchor snap to pad centers | identical | `pcbnew/tools/pcb_grid_helper.cpp:computeAnchors`'s pad case (`kicad-port/gridSnap.ts:collectAnchors`) |
| Anchor snap to track endpoints and segment midpoints | identical | `pcb_grid_helper.cpp:computeAnchors`'s track case |
| Anchor snap to via centers | identical (addition beyond the task's literal list, for consistency with `routing.ts`'s own route-anchor treatment of vias) | `pcb_grid_helper.cpp:computeAnchors`'s via case |
| Anchor snap to footprint origins | identical | `pcb_grid_helper.cpp:computeAnchors`'s footprint-anchor case |
| Snap radius: 25 screen px, clamped to the visible grid pitch when grid snapping is on | identical | `pcb_grid_helper.cpp:BestSnapAnchor` (`snapSize = 25`; the `min(snapScale, GetVisibleGrid().x)` clamp, cited there against gitlab#5638/#7125/#12303) |
| Magnetic pads/tracks on/off | partial (plain on/off; always on for Move) | `pcbnew_settings.cpp` (`m_MagneticItems`) default `CAPTURE_CURSOR_IN_TRACK_TOOL` for both pads and tracks -- KiCad's real 3-state per-tool gating (never/route-tool-only/always) is simplified to the two-state `MagneticSettings` in `gridSnap.ts`, always on; graphics/allLayers magnetic settings not modeled |
| Snap hysteresis (avoids flicker right at the snap-radius boundary) | missing | `advanced_config.cpp:m_SnapHysteresis` (default 5px) -- needs state carried across calls; `bestSnapPoint` is a pure per-call function |
| Snap lines / construction geometry (intersections, extension lines, reference-only points) | missing | `pcb_grid_helper.cpp:BestSnapAnchor`'s `SNAP_MANAGER`/`SNAP_LINE_MANAGER` path (~150 of its ~250 lines) -- a much newer, larger KiCad feature, out of scope this session |
| Wired into the Move tool's live preview (drag and the M-armed move) | identical | `components/canvas/Canvas.tsx` (`snapRef`, both `drag.kind === "move"` and the `moveMode` branch of `onPointerMove`) -- the preview's delta is "(snapped cursor now) − (snapped cursor at drag start)", matching source's own `BestDragOrigin`-then-per-frame-`BestSnapAnchor` model, not a plain grid-rounded delta |
| Wired into route/zone/shape placement clicks | missing (still plain grid snap) | `Canvas.tsx`'s `onPointerDown` route/zone/shape-tool branches still call `snapPoint`, not `snapWithAnchors` |
| Multi-select drag excludes every selected item's own anchors | partial | only a single-item drag passes `excludeOwnerId`; a multi-select drag passes none (KiCad's `BestSnapAnchor` is given the whole selection to skip via its `aSkip` parameter) |

Pure logic: `src/kicad-port/gridSnap.ts`, 12 unit tests. Wiring:
`components/canvas/gridHelper.ts:snapWithAnchors`, `Canvas.tsx:snapRef`.

## 3. Selection

Mostly not started this session (click/box-select/clarification-menu/net-
highlight predate this session -- see Canvas.tsx's own header comments).
Added: `common.Interactive.selectAll`/`unselectAll` (Ctrl+A/Ctrl+Shift+A
-- `useActionRunner.ts`, every placed footprint plus every track/via/
zone/shape/text id; Unselect All only clears the selection, unlike Escape
which also cancels whatever tool/drawing is in progress). Port target for
everything else: `pcbnew/tools/pcb_selection_tool.cpp`, `common/tool/
selection_tool.cpp`.

## 4. Edit tool

Move's snap behavior got real anchor-snapping this session (section 2);
move/rotate/flip/delete/the live preview-on-commit pattern predate this
session and weren't otherwise touched. Not started: `pcbnew.
InteractiveEdit.moveExact` (**Shift+M**, "Move Exactly..." -- a dialog
for an exact relative offset) and `common.Interactive.duplicate`
(**Cmd+D**/Ctrl+D) -- both explicitly named in this task and both
confirmed **missing** (no handler anywhere in this app, not even a
backend `Cmd` verb for "duplicate an item"; see the audit below). Adding
`duplicate` properly needs a new backend verb per the task's hard rule
("add it in Rust with a test") -- `crates/ops`'s `Cmd` enum
(`web/studio/src/api/types.ts`'s `Cmd` union mirrors it 1:1) has no
duplicate-anything op today, for parts, vias, shapes, or text. Not
attempted this session; ranked as the top follow-up in the final report.
Port target: `pcbnew/tools/edit_tool.cpp`, `edit_tool_move_fct.cpp`.

## 5. Context menus and hotkeys

`useActionRunner.ts`'s zoom-action registry was corrected/extended this
session (zoomIn/zoomOut/zoomFitObjects/zoomCenter/zoomRedraw/
resetLocalCoords -- see section 1). The context menu itself
(`Canvas.tsx:onContextMenu`) wasn't otherwise revisited.

### Hotkey coverage audit

Of 766 extracted actions, 201 have a real default hotkey (hotkey or
macHotkey non-null). 36 of those are registered in
`useActionRunner.ts` (and therefore reachable from `useGlobalHotkeys.ts`,
the menu bar, and the toolbars) -- including `selectAll`/`unselectAll`
(Ctrl+A/Ctrl+Shift+A), added this session since they were trivial, safe,
and directly in item 3's (Selection) territory. 165 are not registered.
55 of those are `eeschema.*` -- out of scope for *pcbnew* parity
specifically, since this app's Schematic tab is read-only by design (no
schematic editing verbs exist in the backend yet). The remaining 110 (67
`pcbnew.*`, 43 `common.*` that apply to both editors) are genuine
pcbnew-parity gaps, listed here per the task's "explicitly listed as
missing" instruction rather than left silently unimplemented:

#### `pcbnew.*` missing (67)

| Action | Hotkey | Label |
|---|---|---|
| `pcbnew.Array.createArray` | Ctrl+T | Create Array... |
| `pcbnew.Control.changeTrackLayerNext` | Ctrl++ | Switch Track to Next Layer |
| `pcbnew.Control.changeTrackLayerPrev` | Ctrl+- | Switch Track to Previous Layer |
| `pcbnew.Control.layerAlphaDec` | { | Decrease Layer Opacity |
| `pcbnew.Control.layerAlphaInc` | } | Increase Layer Opacity |
| `pcbnew.Control.layerNext` | + | Switch to Next Layer |
| `pcbnew.Control.layerPairPresetCycle` | Shift+V | Cycle Layer Pair Presets |
| `pcbnew.Control.layerPrev` | - | Switch to Previous Layer |
| `pcbnew.EditorControl.clearHighlight` | ~ | Clear Net Highlighting |
| `pcbnew.EditorControl.EditFpInFpEditor` | Ctrl+E | Open in Footprint Editor |
| `pcbnew.EditorControl.EditLibFpInFpEditor` | Ctrl+Shift+E | Edit Library Footprint... |
| `pcbnew.EditorControl.highlightNet` | \` | Highlight Net |
| `pcbnew.EditorControl.lineModeNext` | Shift+Space | Line Modes |
| `pcbnew.EditorControl.placeFootprint` | A | Place Footprints |
| `pcbnew.EditorControl.toggleLock` | L | Toggle Lock |
| `pcbnew.EditorControl.trackWidthDec` | Shift+W | Switch Track Width to Previous |
| `pcbnew.EditorControl.trackWidthInc` | W | Switch Track Width to Next |
| `pcbnew.EditorControl.viaSizeInc` | \\ | Increase Via Size |
| `pcbnew.InteractiveDrawing.arcPosture` | / | Switch Arc Posture |
| `pcbnew.InteractiveDrawing.bezier` | Ctrl+Shift+B | Draw Bezier Curve |
| `pcbnew.InteractiveDrawing.decWidth` | Ctrl+- | Decrease Line Width |
| `pcbnew.InteractiveDrawing.deleteLastPoint` | Backspace | Delete Last Point |
| `pcbnew.InteractiveDrawing.incWidth` | Ctrl++ | Increase Line Width |
| `pcbnew.InteractiveDrawing.orthogonalDimension` | Ctrl+Shift+H | Draw Orthogonal Dimensions |
| `pcbnew.InteractiveDrawing.placeDesignBlock` | Shift+B | Place Design Block |
| `pcbnew.InteractiveDrawing.placeImportedGraphics` | Ctrl+Shift+F | Import Graphics... |
| `pcbnew.InteractiveDrawing.ruleArea` | Ctrl+Shift+K | Draw Rule Areas |
| `pcbnew.InteractiveDrawing.setAnchor` | Ctrl+Shift+N | Place the Footprint Anchor |
| `pcbnew.InteractiveDrawing.similarZone` | Ctrl+Shift+. | Add a Similar Zone |
| `pcbnew.InteractiveDrawing.zoneCutout` | Shift+C | Add a Zone Cutout |
| `pcbnew.InteractiveEdit.deleteFull` | Shift+Del | Delete Full Track |
| `pcbnew.InteractiveEdit.duplicateIncrementPads` | Ctrl+Shift+D | Duplicate and Increment |
| `pcbnew.InteractiveEdit.FindMove` | T | Get and Move Footprint |
| `pcbnew.InteractiveEdit.moveExact` | Shift+M | Move Exactly... |
| `pcbnew.InteractiveEdit.packAndMoveFootprints` | P | Pack and Move Footprints |
| `pcbnew.InteractiveEdit.skip` | Tab | Skip |
| `pcbnew.InteractiveEdit.swap` | Alt+S | Swap |
| `pcbnew.InteractiveMove.moveIndividually` | Ctrl+M | Move Individually |
| `pcbnew.InteractiveRouter.Autoroute` | Shift+F | Attempt Finish Selected (Autoroute) |
| `pcbnew.InteractiveRouter.ContinueFromEnd` | Ctrl+E | Route From Other End |
| `pcbnew.InteractiveRouter.DiffPair` | 6 | Route Differential Pair |
| `pcbnew.InteractiveRouter.Drag45Degree` | D | Drag 45 Degree Mode |
| `pcbnew.InteractiveRouter.DragFreeAngle` | G | Drag Free Angle |
| `pcbnew.InteractiveRouter.RouteSelected` | Shift+X | Route Selected |
| `pcbnew.InteractiveRouter.RouteSelectedFromEnd` | Shift+E | Route Selected From Other End |
| `pcbnew.InteractiveRouter.SettingsDialog` | Ctrl+< | Interactive Router Settings... |
| `pcbnew.InteractiveRouter.UndoLastSegment` | Backspace | Undo Last Segment |
| `pcbnew.InteractiveSelection.GrabUnconnected` | Shift+O | Grab Nearest Unconnected Footprints |
| `pcbnew.InteractiveSelection.SelectConnection` | U | Select/Expand Connection |
| `pcbnew.InteractiveSelection.SelectUnconnected` | O | Select All Unconnected Footprints |
| `pcbnew.InteractiveSelection.unrouteSegment` | Backspace | Unroute Segment |
| `pcbnew.lengthTuner.AmplDecrease` | 4 | Decrease Amplitude |
| `pcbnew.lengthTuner.AmplIncrease` | 3 | Increase Amplitude |
| `pcbnew.LengthTuner.Settings` | Ctrl+L | Length Tuning Settings... |
| `pcbnew.lengthTuner.SpacingDecrease` | 2 | Decrease Spacing |
| `pcbnew.lengthTuner.SpacingIncrease` | 1 | Increase Spacing |
| `pcbnew.LengthTuner.TuneDiffPair` | 8 | Tune Length of a Differential Pair |
| `pcbnew.LengthTuner.TuneDiffPairSkew` | 9 | Tune Skew of a Differential Pair |
| `pcbnew.LengthTuner.TuneSingleTrack` | 7 | Tune Length of a Single Track |
| `pcbnew.ModuleEditor.newFootprint` | Ctrl+N | New Footprint |
| `pcbnew.PadTool.explodePad` | Ctrl+E | Edit Pad as Graphic Shapes |
| `pcbnew.PadTool.recombinePad` | Ctrl+E | Finish Pad Edit |
| `pcbnew.PointEditor.addCorner` | F1 | Create Corner |
| `pcbnew.PositionRelative.positionRelative` | Shift+P | Position Relative To... |
| `pcbnew.TableEditor.editTable` | Ctrl+E | Edit Table... |
| `pcbnew.ZoneFiller.zoneFillAll` | B | Fill All Zones |
| `pcbnew.ZoneFiller.zoneUnfillAll` | Ctrl+B | Unfill All Zones |

#### `common.*` missing, relevant to pcbnew (43)

| Action | Hotkey | Label |
|---|---|---|
| `common.Control.cursorClick` | Enter | Click |
| `common.Control.cursorDblClick` | End | Double-click |
| `common.Control.cursorDown`/`Up`/`Left`/`Right`(`Fast`) | arrows, Ctrl+arrows | Cursor movement -- KiCad's keyboard-driven cursor, a different interaction model this app doesn't have (mouse-only cursor positioning) |
| `common.Control.gridFast1`/`gridFast2`/`gridFastCycle` | Alt+1/2/4 | Two "fast grid" presets independent of the main grid list |
| `common.Control.libraryTreeSearch` | Ctrl+L | Focus Library Tree Search Field (no library tree in this app) |
| `common.Control.magneticSnapToggle` | Shift+S | Toggle Snapping Between Active and All Layers |
| `common.Control.new`/`open`/`print`/`save`/`saveAs` | Ctrl+N/O/P/S/Shift+S | File operations -- this app has no file model (the backend persists every command immediately) |
| `common.Control.panDown`/`Up`/`Left`/`Right` | Shift+arrows | Keyboard panning |
| `common.Control.showDatasheet` | D | Show Datasheet |
| `common.Control.toggleGridOverrides` | Ctrl+Shift+G | Grid Overrides |
| `common.Control.updatePcbFromSchematic` | F8 | Update PCB from Schematic... (no schematic editing to update from) |
| `common.Control.zoomTool` | Ctrl+F5 | Zoom to Selection Area (drag-to-zoom-box; see section 1) |
| `common.Interactive.copy`/`cut`/`paste`/`pasteSpecial`/`copyAsText` | Ctrl+C/X/V/Shift+V/Shift+C | Clipboard -- no backend verb for any of these yet |
| `common.Interactive.cycleArcEditMode` | Ctrl+Space | Cycle Arc Editing Mode |
| `common.Interactive.duplicate` | **Ctrl+D** | **Duplicate -- explicitly named in this task's Edit tool item; see section 4** |
| `common.Interactive.find`/`findAndReplace`/`findNext`/`findPrevious`/`findNextMarker` | Ctrl+F, ... | Find/Replace |
| `common.Interactive.finish` | End | Finish (generic "end the current interactive action") |
| `common.Interactive.measureTool` | Ctrl+Shift+M | Measure Tool |
| `common.SuiteControl.openPreferences` | Ctrl+, | Preferences... (no Preferences dialog in this app) |
