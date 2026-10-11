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

Rewritten 2026-10-09 (GAPS.md item 14) against `common/tool/grid_helper.cpp`, `pcbnew/tools/pcb_grid_helper.cpp`, `common/tool/construction_manager.cpp` and
`common/tool/tool_dispatcher.cpp`. The pure one-shot function this section used to describe (`gridSnap.ts` `bestSnapPoint`: the nearest anchor of a flat list, no state, the
Move tool only) is deleted; `gridSnap.ts` keeps the grid half (`computeNearest`, `alignToGrid`). The state a KiCad grid helper carries between mouse events (the anchor it is
snapped to, the snap line, the construction geometry) lives in one `PcbGridHelper` per tool, kept by `components/canvas/pcbSnap.ts`.

| Behavior | Status | KiCad file:function |
|---|---|---|
| **Click versus drag:** a held button is a drag after more than 8 px of travel along either axis (strict, per axis: 8 px is a click, 9 a drag, (6, 6) a click), or, on macOS only, on a motion that arrives more than 300 ms after the press; once a drag it stays one; a click reports the press position, however far the pointer wandered back. Used by all four editors (board, schematic, footprint, symbol): nothing is picked up, boxed, point-dragged or panned until the press has become a drag, and a one-grid jitter of the pointer is no longer a move | identical | `common/tool/tool_dispatcher.cpp:TOOL_DISPATCHER::handleMouseButton` (`DragDistanceThreshold`, `DragTimeThreshold`, `#ifdef __WXMAC__`) -- `kicad-port/dragThreshold.ts` (`ClickDragGesture`, 11 tests); `Canvas.tsx`, `SchematicView.tsx`, `footprint/FootprintCanvas.tsx`, `symbol/SymbolEditorCanvas.tsx` (`gestureRef`). A move drag that ends where it started commits nothing. The drag distance is the 8 px constant: a browser has no `wxSYS_DRAG_X` metric, where KiCad takes the system's and falls back to 8 |
| Grid round-off with an arbitrary origin | identical | `grid_helper.cpp:GRID_HELPER::computeNearest` / `AlignGrid` / `Align` (`kicad-port/gridHelperBase.ts`, `gridSnap.ts:computeNearest`) |
| Ctrl (Cmd on macOS) turns the grid off, Shift turns anchor snapping off | identical | `tool_event.h:TOOL_EVENT::DisableGridSnapping` (`Modifier(MD_CTRL)`), `edit_tool_move_fct.cpp` (`grid.SetUseGrid( ... && !evt->DisableGridSnapping() )`, `grid.SetSnap( !evt->Modifier( MD_SHIFT ) )`) -- **corrects an earlier brief's "Shift disables the grid"**: Ctrl gates the grid, Shift the anchors. With the grid off a point is wherever the pointer is, rounded to whole um (the verbs take integers) |
| `BestSnapAnchor` serves every placing tool: the shape tools (line, rectangle, circle, arc, Bezier, polygon: active layer, graphics grid), zone corners (active layer, graphics grid), text (text grid), vias (all layers, vias grid), dimensions (all layers, graphics grid), the ruler, picker, drill origin and local ratsnest (all layers, current grid), the point editor's zone-corner and shape-handle drags | identical | `drawing_tool.cpp:drawShape` / `drawArc` / `DrawZone` / `PlaceText` / `DrawDimension`, `pcb_picker_tool.cpp`, `pcb_point_editor.cpp` -- `PcbSnap.point( tool, ... )` (`components/canvas/pcbSnap.ts`) called on every pointer move (so the marker, snap lines and construction geometry follow the cursor) and again for the press; a new tool starts a new helper, as a new tool makes a new `PCB_GRID_HELPER`; the start of a shape is its skip point (`grid.SetSkipPoint`) |
| Route start and end: the pad, via or track under the cursor, else the wire grid | identical | the router does not use `BestSnapAnchor`: `pns_tool_base.cpp:TOOL_BASE::updateStartItem` / `updateEndItem` / `snapToItem` (a pad or via centre, a track's end, or `AlignToSegment` / `AlignToArc` on a track under the cursor) then `Align( GRID_WIRES )` (`GRID_VIAS` for a via) -- `PcbSnap.routePoint`, `PcbGridHelper.alignToSegment` / `alignToArc` / `snapToPad` |
| Anchor types: pads (centre, corners and quadrants of its shape), track ends and midpoints, via centres, footprint origins, shape corners, centres, quadrants, midpoints and ends of lines and arcs, zone corners, polygon points, dimension points, and **intersections** of any two items with idealised geometry (line x line, line x circle or arc, circle x circle); a text's origin is an anchor to pick it up by (`BestDragOrigin`), not one to snap to | identical | `pcb_grid_helper.cpp:computeAnchors( BOARD_ITEM* )` -- `kicad-port/snapAnchors.ts` (`computeItemAnchors`, the `ANCHOR` flags CORNER / OUTLINE / SNAPPABLE / ORIGIN / VERTICAL / HORIZONTAL / CONSTRUCTED, 18 tests), `snapGeom.ts` (`intersections`, a port of `INTERSECTION_VISITOR`; 12 tests), `snapScene.ts` (the board as a list of items; the layer filter on item layer sets) |
| Snap radius: 25 screen px, never more than the visible grid pitch while the grid is on | identical | `pcb_grid_helper.cpp:BestSnapAnchor` (`snapSize = 25`; `min( snapScale, GetVisibleGrid().x )`) |
| Snap hysteresis: the anchor already snapped to is kept while the cursor is within range + 5 px of it, a new one is taken within range - 5 px | identical | `BestSnapAnchor`'s `m_snapItem` with `ADVANCED_CFG::m_SnapHysteresis` (5 px) -- `pcbGridHelper.ts` (`bestSnapAnchor`; the hysteresis test slides the cursor across the boundary in both directions) |
| Snap lines: a horizontal and a vertical line (the 45-degree diagonals too in a 45-degree mode) through the last anchor snapped to; the cursor is pulled onto one within the range (1.5 x for the direction already in use) and onto the grid along it; the snap line is drawn from its origin to the cursor | identical | `grid_helper.cpp:SnapToConstructionLines`, `construction_manager.cpp:SNAP_LINE_MANAGER` -- `kicad-port/constructionManager.ts` (`SnapLineManager`, `nearestGridPointOnLine`), `components/canvas/snapOverlayPainter.ts` |
| Construction geometry: dwelling on a segment, arc or circle shows its extension (the two rays beyond a segment's ends, the rest of an arc's circle); the extension geometry of up to two temporary batches is shown (immediately while fewer than two, else after the proposal has stood for 500 ms), a persistent batch at once; "involved" items let the constructed anchors (intersections, an extension crossing a line) be snapped to | identical | `construction_manager.cpp:CONSTRUCTION_MANAGER` (`ExtensionSnapTimeoutMs`, `getMaxTemporaryBatches`, `ACTIVATION_HELPER`) -- `constructionManager.ts` (`ConstructionManager`, `ActivationHelper`, 11 tests; the clock is injected, the canvas arms a timer for the 500 ms), `pcbGridHelper.ts:addConstructionItems` |
| Magnetic points (Preferences, board editor): Snap to Pads, Snap to Tracks and Vias (Never / In track tool / Always; default In track tool), Snap to graphics (default off); Snap to Objects on the Active Layer Only / All Layers (`Shift+S`) | identical | `pcbnew_settings.cpp:m_MagneticItems`, `MAGNETIC_OPTIONS`, `pcb_control.cpp:SnapMode` -- `kicad-port/preferences.ts` (`magneticPads`, `magneticTracks`, `magneticGraphics`, `toPcbMagnetic`), `PreferencesDialog.tsx`. **A consequence worth knowing:** with the default "In track tool" a Move, a shape or a zone does not snap to pads or tracks (only the route tool does); set "Always" to get pad and track anchors in the drawing and move tools, as in KiCad |
| Move and drag: the held point is `BestDragOrigin` (the nearest origin or corner of the selection to the press; the nearest outline point when both are far), the cursor is that point plus the pointer's travel, snapped with `BestSnapAnchor` on the coarsest grid of the selection (`GetSelectionGrid`), skipping the selection's own anchors | identical, except that KiCad warps the pointer to the held point and the studio never moves the real one | `edit_tool_move_fct.cpp:EDIT_TOOL::doMoveSelection`, `pcb_grid_helper.cpp:BestDragOrigin` -- `pcbGridHelper.ts:bestDragOrigin`, `kicad-port/heldCursor.ts` (the cursor KiCad's warp would give), `Canvas.tsx` (`moveOrigin`, `moveCursor`). Multi-select moves skip every selected item's anchors |
| Grid overrides per item category: connected items (footprints, pads), wires (tracks), vias, text, graphics (shapes, dimensions); each its own grid from the editor's list, used while `Toggle Grid Overrides` (`Ctrl+Shift+G`, toolbar button) is on and the category is checked in Edit Grids... > Grid Overrides; all five start unchecked on the board, as KiCad's do | identical | `pcb_grid_helper.cpp:GetItemGrid` / `GetGridSize` / `GetSelectionGrid`, `common_tools.cpp:ToggleGridOverrides`, `panel_grid_settings.cpp` -- `kicad-port/gridOverrides.ts` (`pcbItemGrid`, `gridSizeFor`, `selectionGrid`, `defaultGridOverrides`), `state/gridOverrides.ts` (kept in the browser, per editor), `GridsDialog.tsx`, `commonGridListActions.ts`; `PARITY-common.md` section 10 |
| Placing a part (the connected-items grid) and inserting a corner with Add Corner (the current grid) | grid only, no anchors | `PcbSnap.align` (`Align( point, category )`) |
| What the server snaps: a part's placement still snaps to `meta.snap_um` (100 um) on the server, whatever grid the client showed; shapes, zones, text and tracks go as the client sends them | by design | `crates/cli/src/board.rs:step`. The client previews with the grid on screen (`state.gridUm`); before this section's rewrite the shape and zone tools snapped to the 100 um board snap instead of the displayed grid |

Pure logic: `kicad-port/{snapGeom,snapScene,snapAnchors,constructionManager,gridHelperBase,pcbGridHelper,gridOverrides,dragThreshold,heldCursor}.ts` (91 unit tests: each
anchor type, the hysteresis, the snap lines, the 500 ms dwell, the overrides, the drag rule). Wiring: `components/canvas/pcbSnap.ts` (one `PcbSnap` per canvas), `snapOverlayPainter.ts` (the
marker, snap line, guides and construction geometry), `Canvas.tsx`. Browser check: `e2e/snap-grid.check.js` (the last snap decision is `window.__eda.state().snap`).

## 3. Selection

Rewritten this session against `pcbnew/tools/pcb_selection_tool.cpp` and
`common/tool/selection_tool.cpp`'s real click/drag decision tree --
replacing a pre-existing footprints-only, Shift-only, two-tier-priority
click/box-select with the actual model: every selectable kind is a
candidate at once, `GuessSelectionCandidates` (or a clarification menu)
picks among them, and the real add/subtract/toggle/skip-heuristics
modifiers apply throughout. `common.Interactive.selectAll`/`unselectAll`
(Ctrl+A/Ctrl+Shift+A) predate this session and weren't revisited.

| Behavior | Status | KiCad file:function |
|---|---|---|
| Click modifiers: Shift=add, Ctrl/Cmd+Shift=subtract, Ctrl/Cmd=toggle | identical | `selection_tool.cpp:setModifiersState` -- `ctrlClickHighlights()` (which would turn plain Ctrl+click into "highlight this net" instead) isn't modeled; it defaults off in a stock KiCad install too, so this is the real out-of-the-box behavior, not a gap |
| Alt = skip heuristics, always disambiguate for 2+ candidates | identical | `setModifiersState`'s `m_skip_heuristics` (source's Windows-only exception for a wx/MSW menu-key conflict doesn't apply here) |
| Long-press (500ms) = same as Alt | identical | `ADVANCED_CFG::m_DisambiguationMenuDelay` (default 500) + `disambiguateCursor`'s own `m_skip_heuristics = true` |
| GuessSelectionCandidates: prefer exact hits (within 1px), then the smallest of nested items (1.5x ratio), then the active layer | 3 of the real function's passes ported | `pcb_selection_tool.cpp:GuessSelectionCandidates` -- `kicad-port/selection.ts`. Not ported: the silk/courtyard active-layer preference (no distinct silk-text item type in this app's model to special-case), the footprint coverage-ratio override (no Clipper, so no exact polygon-overlap area -- `selectionCandidates.ts` approximates per-kind area instead), `pruneObscuredSelectionCandidates` (an advanced-config-gated visibility-ratio pass, default off anyway) |
| Clarification menu for 2+ remaining candidates, "Select All" entry | same effect, simpler widget | `common/tool/selection_tool.cpp:doSelectionMenu` -- reuses `ContextMenu.tsx` rather than source's own numbered-hotkey/hover-highlight menu (no per-row highlight-on-hover) |
| A modifier+drag is always a box-select, even one starting on top of a selected item -- never an item move | identical | `pcb_selection_tool.cpp:Main`'s `hasModifier() \|\| dragAction == SELECT` branch, checked *before* any "drag the selection" path. Easy to miss: Shift/Ctrl+drag never moves anything, regardless of where the drag starts |
| A plain click on an already-multi-selected item keeps the whole group for a drag, collapsing to just that item only if the drag never actually moves | identical | `Main`'s "dragging started within the selection's bounding box" check vs. `selectPoint`'s unconditional replace -- this app defers the decision (`Canvas.tsx`'s `pendingClickRef`) until `onPointerUp` knows whether real movement happened |
| Box select: left→right = window (fully enclosed), right→left = crossing (touching) | identical (predates this session; reverified against source, unchanged) | `pcb_selection_tool.cpp:SelectRectArea`'s `greedySelection` |
| Box select across every selectable kind, with the same add/subtract/toggle modifiers as a click | identical (was footprints-only) | `SelectMultiple`'s per-item apply -- `selectionCandidates.ts:collectBoxSelection`, `kicad-port/selection.ts:applyBoxSelectionModifiers` |
| Selection Filter panel really filters (footprints/tracks/vias/zones/graphics/text) | identical for the categories this model has | `panel_selection_filter.cpp`/`itemPassesFilter` -- no keepouts/points/other-items categories (no such selectable concept exists here); was three checkboxes, two of them permanently `disabled` and doing nothing. **Status (2026-10-08): Pads, Dimensions and Locked items are categories now** (section 22 for pads) |
| A hidden layer's items are unselectable; in high-contrast mode, off-active-layer items are too | identical | `Selectable()`'s layer-visibility and `GetHighContrast()`/`GetHighContrastLayers()` checks |
| Double-click opens properties | identical (was a no-op outside active drawing) | `Main`'s `IsDblClick` handler (a single selected group is entered instead of opened: section 16) |
| Escape: cancel the active tool/drag/arm without touching the selection; else clear the selection; else clear the net highlight | identical | `Main`'s `IsCancel` handler + `m_ESCClearsNetHighlight` (default on). Replaces a flat reset that wiped the selection on every Escape, even mid-move -- `EDIT_TOOL`'s own move-cancel never touches the selection in source |
| Select/Expand Connection (U): 3-stage junction/pad/never flood, widening one stage per press | ported, net-restricted | `pcb_selection_tool.cpp:expandConnection`/`selectAllConnectedTracks` -- `kicad-port/expandConnection.ts`. Source's `IGNORE_NETS` cross-net traversal isn't modeled (two different nets touching is a DRC violation this app doesn't need to tolerate mid-traversal). Pads are start points only, never graph nodes to cross through -- which makes this model's "pad" and "never" stages compute identically (a footprint's other pads aren't bridged by copper in this model either way), documented in that file |
| Net highlight (`` ` ``): cursor-driven lookup (pads/vias/tracks preferred, zones fallback), toggles off when re-picking the same net | identical | `board_inspection_tool.cpp:highlightNet` (the `!aUseSelection` branch) -- `components/canvas/netAtCursor.ts`. Was entirely unregistered (no hotkey handler at all) |
| Net highlight dimming: brighten the matching net, darken everything else, real 0.5 factor | identical | `pcb_painter.cpp:GetColor`'s highlight branch, `COLOR4D::Brighten`/`Darken` -- `kicad-port/netHighlight.ts`. Applies only to pads/tracks/vias/zones (connected items), never a footprint's silkscreen/courtyard/reference -- matches source's `conItem` null-check. Was a flat white fill on the matching net and no dimming of anything else |
| Clear Net Highlighting (`~`) | identical | `board_inspection_tool.cpp:ClearHighlight` -- was unregistered |
| `toggleNetHighlight` (Alt+`` ` ``) | pre-existing, still simplified | real semantics are "toggle the *last* highlighted net set back on/off" (`m_lastHighlighted`), independent of selection; this app's existing handler is selection-driven instead. Not revisited this session -- a different, secondary action from the one the task names |

Pure logic: `kicad-port/selection.ts` (23 tests), `kicad-port/
expandConnection.ts` (8 tests), `kicad-port/netHighlight.ts` (4 tests).
Wiring: `components/canvas/selectionCandidates.ts`, `netAtCursor.ts`,
`properties.ts`, `Canvas.tsx`.

## 4. Edit tool

> **2026-10-08:** the first two rows describe the footprint-only Rotate, Flip and Move of the earlier sessions. Rotate, Flip, Move, drag and Align now act on every kind of item through `move_items` / `rotate_items` / `flip_items`, R turns counter-clockwise, and a carried selection turns and flips about where it was picked up as ONE selection: see section 22. The rows are kept as the record of what was there.

| Behavior | Status | KiCad file:function |
|---|---|---|
| Rotate/flip a footprint (standalone, not mid-move) | identical for 1 item (predates this session); **this session**: 2+ items now share ONE pivot -- `updateModificationPoint`'s real rule (the selection's union-bounding-box center), not each part spinning/mirroring about its own anchor the way an earlier session's docs here incorrectly called "identical" | `edit_tool.cpp:Rotate`/`Flip`, `updateModificationPoint` -- `state/store.tsx`'s `rotateSelection` (now one atomic `Cmd::MoveExact` for the whole group, same single-`BOARD_COMMIT::Push()` undo granularity as source) and `flipSelection` (mirrors each part's X about the shared center via `move_to`, then `flip`'s existing side-toggle -- **documented gap**: this is N separate Cmds, not source's one atomic commit, so undoing a group flip takes 2N Ctrl+Z's, not 1). **Status (2026-10-07): that gap is closed** -- `flipSelection` sends one `Cmd::Batch`, one undo step (`state/store.tsx`) |
| Rotate (R/Shift+R) / Flip (F) *during* an active Move | identical for the common case: a dragged footprint spins in place | `edit_tool.cpp`'s own `m_dragging` branch of `Rotate`/`Flip`: the live item is mutated directly mid-drag rather than committing a separate op, and a further rotate during the same drag reuses the *first* rotate's reference point (`updateModificationPoint`'s `m_dragging && HasReferencePoint()` guard). This app can't mutate an uncommitted backend item, so it accumulates the transform on the client-side preview instead (`MovePreview.rotateQuarterTurns`/`flipped`, applied in `painter.ts`'s preview render) and commits move+rotate+flip together as sequential Cmds on drop -- for a **single** selected part this is mathematically identical to source (rotating/flipping about a part's own anchor doesn't move it, so composing the translation and the spin in either order lands on the same pose). **Documented simplification:** a **multi-part** selection rotates/flips each part individually about its own anchor instead of the whole group swinging around one shared pivot the way source's `ROTATE_AROUND_SEL_CENTER`-style group rotation would. **Documented gap:** if R/F is pressed before the mouse has moved even once during a *click-drag* (not the M-armed path, which has no such window), there's no preview yet to attach the rotation to and the keypress is dropped -- `useActionRunner.ts`'s `tryTransformDuringMove` can only see `state.movePreview`/`state.activeTool`, not `Canvas.tsx`'s own pending-drag ref |
| Move: connected track ends follow the dragged footprint | **does not happen in source either** -- task premise corrected after reading `edit_tool_move_fct.cpp:doMoveSelection` directly: a plain (non-router) Move only ever does `item->Move(movement)` on the selection itself; it never touches a connected-but-unselected track. What source *does* do instead is redraw a live/dynamic ratsnest during the drag (`PCB_ACTIONS::updateLocalRatsnest`) | ported: `kicad-port/localRatsnest.ts:offsetRatsnestForPreview` shifts ratsnest edges touching a moving part's pads by the live preview delta, so the airwire updates every frame instead of only after the move commits and `/api/ratsnest` is re-polled |
| Router-driven drag (`D`, `pcbnew.InteractiveRouter.Drag45Degree`) of a track segment/corner or via -- collision-aware, keeps connections live via the full interactive router | **done this session** (gap #7 stage 5's frontend -- `crates/pns::dragger::Dragger` already existed; this session is wiring it in) | `router_tool.cpp:InlineDrag`/`CanInlineDrag` -- `components/canvas/dragging.ts` (`findDraggableAt`: approximates `ACTIONS::selectionCursor` + `NeighboringSegmentFilter` with the existing click-select hit-test, restricted to track/via, falling back to a lone selection; `startInlineDrag`/`finishInlineDrag`), `kicad-port/dragTool.ts` (pure preview-merge glue, mirrors `routeTool.ts`), `Canvas.tsx` (pointerdown commits, pointermove previews -- reuses the route tool's own throttle/request-guard refs since the two sessions are mutually exclusive), `useActionRunner.ts` (the `D` hotkey itself: a one-shot grab-and-go, not a toggle-arm like `X`), `painter.ts:drawInProgress`'s new `"drag"` branch (dashed live preview; a via's own attached tracks -- `fanout` -- render too, not just the via). **Update 2026-10-08 (D7):** a dragged via is a pusher now -- in Shove mode it is the head of a shove (the tracks attached to it go with it, what they hit is pushed aside), in Walk around mode it is pushed off what it lands on and its tracks are walked around -- and a corner dragged in Walk around mode is walked around what it lands on. **Scope, matching `dragger.rs`'s own documented simplifications** (see `crates/pns/PARITY.md`): corner-drag only (no segment-sideways-slide), free-angle (not 45-degree-constrained), and -- per `CanInlineDrag`'s own footprint branch being out of scope for this port's single mode -- **not** a footprint drag (only a track/via); a footprint `Move` stays the plain, non-router kind the row above already covers (confirmed, not a gap, by reading source directly) |
| Router-driven drag of a **footprint** (so its attached tracks follow via the router, distinct from the row above) | **not implemented** -- `dragger.rs` only models `DM_CORNER`/via dragging, matching the task's explicit single-item-drag scope; `CanInlineDrag`'s footprint branch (`!(aDragMode & DM_FREE_ANGLE)`) has no backend counterpart to wire | `router_tool.cpp:InlineDrag`'s `footprints` path |
| "Highlight collisions" router mode | **done this session** -- it's not a distinct concept from `Mode::MarkObstacles`, already fully implemented server-side since gap #7 stage 1 (`router_tool.cpp` literally labels `RM_MarkObstacles` as `"Highlight collisions"` in its own status-bar summary); the real gap was that nothing in the frontend ever let a person select any mode but Walkaround at all -- fixed by the settings dialog below | `router_tool.cpp`'s `RM_MarkObstacles` case, `PCB_ACTIONS::routerHighlightMode` |
| Shove mode (router mode **Shove**): push tracks and vias out of the way, walk around pads | **done 2026-10-08** -- `crates/pns/src/shove.rs` follows `pns_shove.cpp` (line stack with ranks; nearest obstacle by path length, pads then vias then tracks; `onCollidingSolid` walks the current line around the cluster of a pad; `onCollidingVia`/`pushOrShoveVia` move a via by the minimum translation vector and re-shape its tracks at 45 degrees; `runOptimizer` optimizes every shoved line; the head is walked around pads first, `rhShoveOnly`). Widths are kept (KiCad's own shove would normalise a chain of mixed widths). Replays KiCad's `simple-shove-1` (same 13 tracks pushed), `issue22749` (same 5) and `backspace1` (same run) from `qa/data/pcbnew/pns_regressions`, and 1000 random routes on four QA boards leave no new clearance violation. **Not done**: a head that ends in a via is not shoved together with it, no springback and no time limit, and dragging a *via* never shoves against the dragged via itself (`DRAGGER::dragViaWalkaround`) | `pns_shove.cpp`, `pns_walkaround.cpp`, `pns_line_placer.cpp:rhShoveOnly` -- `crates/pns/src/{shove,walkaround,node,line,optimizer,line_placer}.rs`, `crates/pns/tests/{shove_scenarios,shove_footprint,qa_regressions}.rs`; the same HTTP surface (`POST /api/route/*`, mode `"shove"`), no frontend change |
| Interactive Router Settings... (`Ctrl+<`) | **done 2026-10-08, with `dialog_pns_settings.cpp`'s rows** -- Highlight collisions (Free angle mode, Allow DRC violations), Shove (Shove vias, Jump over obstacles), Walk around; Remove redundant tracks, Optimize pad connections, Fix all segments on click: labels and tool tips as `dialog_pns_settings_base.cpp` has them, enabled as `DIALOG_PNS_SETTINGS::onModeChange` does (Free angle mode and Allow DRC violations with Highlight collisions, Shove vias and Jump over obstacles with Shove). Every one reaches `crates/pns` and is read there: Shove vias (a via is hugged, not shoved), Jump over obstacles, Optimize pad connections (`SMART_PADS`), Allow DRC violations (a colliding head can be fixed in Highlight collisions), Free angle mode (the head is a straight line at any angle, Highlight collisions only), Fix all segments on click (off: the last segment stays free after a click); plus one row KiCad has not, **Optimizer effort** (Low/Medium/Full: KiCad keeps `OptimizerEffort` in its settings file, `tools.pns.effort`). **Three of KiCad's rows are shown disabled, unchecked, with the reason in the tool tip, because nothing in this router could honour them:** Smooth dragged segments and Optimize entire track being dragged (the corner drag neither snaps nor optimizes afterwards: `dragCorner45`, `optimizeAndUpdateDraggedLine` are not ported) and Use mouse path to set track posture (no mouse trail: the router is asked one cursor position at a time). "Suggest track finish" is hidden, as KiCad hides it. OK also applies the settings to a route, drag or diff pair in progress (`POST /api/route/settings`), so a change takes effect from the next move; the settings are not kept across a page reload (KiCad keeps them in `pcbnew.json`) | `dialog_pns_settings.cpp`, `dialog_pns_settings_base.cpp` -- `components/RouterSettingsDialog.tsx`, `kicad-port/routerSettings.ts` (defaults, wire format, `onModeChange`'s rule, the reasons), `state.routerSettings` (`state/store.tsx`), `routeStart`/`routeDragStart`/`routeSetSettings` (`api/client.ts`), `POST /api/route/{start,drag_start,dp_start}`'s `settings` object and `POST /api/route/settings` (`crates/cli/src/route_api.rs`) |
| Route Differential Pair (`6`, `pcbnew.InteractiveRouter.DiffPair`) -- two parallel, gap-matched lines from one session | **done this session, significantly narrower than upstream by necessity** -- see `crates/pns/PARITY.md`'s own "Stage 7" section for the full design. Pair detection by net-name suffix (`+`/`-`/`P`/`N`, trailing digits preserved) and gap/width from the net-class diff-pair fields, both exactly as the task asked; routing itself is a direct 45-trace spine offset into two lines (collision-*reporting* only, like a plain route in `mark_obstacles` mode) rather than upstream's own coupled-shove/walkaround `DIFF_PAIR_T` item -- that whole second collision model was out of proportion to the remaining task scope. No via/layer-switch mid-pair-route either (single layer only) | `pns_diff_pair_placer.cpp`, `board.cpp:MatchDpSuffix` -- `crates/pns/src/diff_pair.rs` (8 tests), `crates/model/src/lib.rs`'s `diff_pair_{width,gap,via_gap}_of` (3 tests), `crates/cli/src/route_api.rs`'s `dp_*` endpoints, `components/canvas/diffPairRouting.ts` + `kicad-port/dpTool.ts` (3 tests, mirroring `routing.ts`/`routeTool.ts`), `Canvas.tsx`, `useActionRunner.ts`, `painter.ts:drawInProgress`'s new `"diffpair"` branch |
| Tune Length of a Single Track... (`7`, `pcbnew.LengthTuner.TuneSingleTrack`) -- lengthen a track to a target length by inserting a meander | **done this session, as a dialog rather than upstream's own live mouse-driven session** -- see `crates/pns/PARITY.md`'s own "Stage 8" section. Scoped to a straight, axis-aligned, single-segment track (refused with a message otherwise -- `generate_meander`'s own doc comment explains why diagonal baselines need a different, not-yet-derived formula); collision-*reporting* only, same as the diff-pair router above. **Diff-pair length (`8`) and skew (`9`) tuning: done, same dialog (section 20)**. The `1`/`2`/`3`/`4` live amplitude/spacing hotkeys and `Ctrl+L` settings are wired to the dialog's fields (an earlier note here saying otherwise was stale) | `pns_meander_placer.cpp`, `pns_meander.cpp` -- `crates/pns/src/meander.rs` (9 tests), `crates/cli/src/tune_api.rs`'s stateless `/api/tune_length/{preview,apply}` (reuses `Cmd::CommitRoute`, no new `Cmd`), `components/LengthTuningDialog.tsx` |

Pure logic: `kicad-port/localRatsnest.ts` (5 tests), `kicad-port/dragTool.ts`
(2 tests), `kicad-port/dpTool.ts` (3 tests). Wiring: `painter.ts`'s
`movePreview` rotate/flip transform, `useActionRunner.ts:
tryTransformDuringMove`; `components/canvas/dragging.ts`,
`kicad-port/dragTool.ts`, `Canvas.tsx`, `useActionRunner.ts` for the `D`
drag tool itself (see the new rows above); `components/RouterSettingsDialog.tsx`
for the settings dialog.

## 5. Duplicate (Cmd+D) and copy/paste (Cmd+C/V)

New backend verbs (`crates/ops/src/lib.rs`), since none existed for
"copy an item" at all before this session.

| Behavior | Status | KiCad file:function |
|---|---|---|
| Duplicate: copy in place, select the copies, immediately arm Move on them at the cursor | identical in effect | `edit_tool.cpp:Duplicate` hands straight to `doMoveSelection` -- `Cmd::Duplicate { ids }` (new, with tests) copies existing tracks/vias/zones/shapes/texts server-side; `state/store.tsx:duplicateSelection` diffs the board's id set before/after to find the new ones (the HTTP reply has no structured "here's what I made" field) and arms Move |
| Copy/Cut/Paste | Copy + Paste identical in effect; **Cut not implemented** | `common.Interactive.copy`/`paste` -- `Cmd::PasteItems { tracks, vias, zones, shapes, texts }` (new, with tests) inserts fresh copies of whole items carried with the command itself (not references), so paste survives the original being deleted first. `components/canvas/clipboard.ts` converts this app's display shapes to the Cmd's IR shape and holds the result in `state.clipboard`. `common.Interactive.cut` (Ctrl+X) -- trivially "copy then delete" -- was not wired this session; still shows "not ported yet". **Status (2026-10-07): closed** -- `common.Interactive.cut` is wired (`actions/useActionRunner.ts`; section 11) |
| Footprints | ~~explicitly out of scope~~ **done 2026-10-08** (section 22) | A copy of a footprint is a part of the board that the intent does not list (`DrawingsSection::board_parts`, folded into the model by `board::load`), under the next free reference with its pads on the same nets |

> **2026-10-08:** the clipboard is KiCad's now (`CLIPBOARD_IO`, section 22), and Duplicate, Copy and Paste take footprints, dimensions and groups too; `Cmd::PasteItems` stays for the older callers.

Rust tests: 8 new (`crates/ops/src/tests.rs`'s "duplicate / paste"
section) covering fresh-id assignment, same-position copies, a mixed
known/unknown id list, an all-unknown refusal, and an empty clipboard
no-op.

## 6. Move Exact (Shift+M)

Port of `pcbnew/dialogs/dialog_move_exact.cpp` + `edit_tool.cpp:MoveExact`.

| Behavior | Status | KiCad file:function |
|---|---|---|
| Cartesian (Move X/Move Y) or polar (Distance/Angle) offset | identical | `GetTranslationInIU`/`ToPolarDeg` -- `MoveExactDialog.tsx`, reusing `state/units.ts`'s existing `umTo`/`umFrom` |
| Independent rotation angle, applied regardless of anchor choice | identical | `TransferDataFromWindow`'s `m_rotation` |
| Anchor: item's own position (default for 1 item), selection center (default for 2+), local coordinates origin | identical for these three | `buildRotationAnchorMenu`'s `ROTATE_AROUND_ITEM_ANCHOR`/`_SEL_CENTER`/`_USER_ORIGIN`. KiCad's 4th choice, `ROTATE_AROUND_AUX_ORIGIN` (a board-wide "drill/place origin" setting), has no model equivalent in this app and is left out |
| Translate first, then rotate by the same angle around the chosen pivot (or each part's own anchor, which a pure rotation can't move) | identical | `EDIT_TOOL::MoveExact`'s `boardItem->Move(translation)` then `boardItem->Rotate(pivot, angle)` -- `Cmd::MoveExact`'s `rotate_point_about`, same matrix as `crates/model/src/footprint.rs:to_board` (no Y-axis flip either side, so a part lands exactly where its own subsequent rendering will draw it) |
| Whole batch refuses atomically if any named part is unplaced/unknown | identical in effect | source validates via the selection itself (always placed); this app's dialog can in principle be asked to move an unplaced ref, so the backend verb checks every part before moving any of them |
| Any kind of item, not footprints only | **done 2026-10-08** | `EDIT_TOOL::MoveExact` works on whatever `RequestSelection` hands it. `kicad-port/pcbTransform.ts:planMoveExact` sends one `move_items` and then a `rotate_items` per item (item anchor, about its own moved position) or one for the whole selection (centre of the box measured before the move and carried with it, or the local origin) as one batch; locked items stay, a pad stands for its footprint. `Cmd::MoveExact` is still there for the older callers, footprints only |
| Rotation sign | identical effect, bridged the same way | source: `if (!m_Display.m_DisplayInvertYAxis) rotation = -rotation;` before calling `Rotate()` -- negates the dialog's own CCW-positive, user-facing angle to match the CW-positive-in-Y-down rotation matrix underneath. This app's dialog does the same negation, landing on the same visual direction as `state/units.ts`'s existing `toPolar` (also CCW-positive) |

Rust tests: 6 new, covering the no-pivot in-place spin, translate-then-
spin, a shared pivot actually orbiting the part, one shared transform
applied to a whole batch, atomic refusal, and the empty-list refusal.

## 7. Context menus and hotkeys

`useActionRunner.ts`'s zoom-action registry was corrected/extended this
session (zoomIn/zoomOut/zoomFitObjects/zoomCenter/zoomRedraw/
resetLocalCoords -- see section 1). The right-click menu is KiCad's since
2026-10-08 (GAPS.md item 16): the next subsection. Find is section 7a.

### The right-click menu (GAPS.md item 16)

Before, a right click listed Rotate, Flip, Copy, Delete, Align and the rest whatever was selected, greyed out when they did not apply
(`actions/pcbSweepMenu.ts`, `Canvas.tsx`). Now the menu is built the way KiCad builds it: every tool adds its entries, each with the condition it shows
under and an order number, to the selection tool's `CONDITIONAL_MENU`, and the right click evaluates that against the selection.
`kicad-port/pcbContextMenu.ts` keeps the class (`ConditionalMenu`: `addItem`, `addMenu`, `addSeparator`; an entry with no order takes `entries.size()`,
goes after every entry whose order is not above its own, and a separator is drawn only when an entry or submenu came since the last one), and
`buildSelectionMenu()` adds the entries in the order `PCB_EDIT_FRAME::setupTools` initialises the tools, so the order on screen is KiCad's and not a
guess. The selection is summarised once (`kicad-port/pcbSelectionSummary.ts`: the count of each item type and shape, copper shapes, locked items,
editable corners, what `GROUP_CONTEXT_MENU::update` asks), the entries run the actions that already exist, and an entry KiCad offers whose action the
studio has not wired is listed dimmed, as the schematic's menu does (`MenuNodeView`).

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Which menu a right click opens depends on the tool that owns it | **done**: idle, the selection tool's menu; while a route is drawn or the router is armed, the router's (Cancel, Finish Route, Undo Last Segment, Route From Other End and Attempt Finish when there is something left to connect, Place Through Via and its layer-picking, blind and micro forms, Switch Track Posture, Track Corner Mode, Select Track/Via Width, Differential Pair Dimensions for a pair, Router Settings, Zoom, Grid); while a graphic, text, dimension, via or zone is being drawn, the drawing tools' (Cancel, Close Outline, Delete Last Point, Switch Arc Posture, Switch Dimension Arrows, Zones for the zone tool); the picker tools (Local Ratsnest, Set Drill Origin, Measure) offer Cancel, Zoom and Grid | `ROUTER_TOOL::Init`, `DRAWING_TOOL::Init`, `PICKER_TOOL`, `PCB_SELECTION_TOOL::Init` ("Cancel" first when a tool is active, "Don't add things like Paste when another tool is active") -- `routerMenu`, `drawingMenu`, `pickerMenu`, `components/canvas/pcbMenuBuilder.ts` |
| Only what applies is listed, in KiCad's order | **done**: Select / Cancel (order 1), Clear Net Highlighting (2), then in tool order the edit tool (Move, Flip, Break Track and the rest of the footprint and track actions, the Routing, Mirror / Rotate, Shape Modification and Position submenus, Cut ... Delete, Properties last), the pad, point-editor and board-editor entries (Cycle Arc Editing Mode, Get and Move Footprint, Locking, Zones), Net Inspection Tools, Align/Distribute, Create from Selection, Grouping, then Zoom and Grid (order 1000). An empty spot lists Select All, Unselect All, Get and Move Footprint, Paste, Paste Special, Zoom, Grid; a footprint has no Mirror or Rotate (`canMirror`); Properties is offered for one item of any kind or several tracks and vias | `CONDITIONAL_MENU::Evaluate`, `PCB_SELECTION_TOOL::Init`, `EDIT_TOOL::Init`, `PAD_TOOL::Init`, `PCB_POINT_EDITOR::Init`, `BOARD_EDITOR_CONTROL::Init`, `BOARD_INSPECTION_TOOL::Init`, `ALIGN_DISTRIBUTE_TOOL::Init`, `CONVERT_TOOL::Init`, `GROUP_TOOL::Init`, `EDA_DRAW_FRAME::AddStandardSubMenus` -- explicit orders 1, 2, 100, 150, 400, 1000, 2000 as in the C++ |
| The submenus and their conditions | **done**: Select (always the same entries; each follows its own action's enable condition), Routing (a footprint, pad, track or via), Mirror / Rotate, Shape Modification (fillet, chamfer, extend, boolean, heal, simplify by the shapes selected, the corner entries by the handle under the pointer), Position, Locking (Lock when something is unlocked, Unlock when something is locked), Zones (a selection of zones only, Merge for several, Zone Priority by what overlaps), Net Inspection Tools (every item connectable), Align/Distribute (two or more, three for Distribute), Create from Selection (`kicad-port/pcbConvert.ts` `convertAvailability`), Grouping (Group needs two items, Ungroup a group, Add Items one group and an ungrouped item, Remove Items a member), Zoom (the presets, the current one checked), Grid (Grid Origin, the grids) | the submenu makers of `edit_tool.cpp`, `board_editor_control.cpp` (`LOCK_CONTEXT_MENU`, `ZONE_CONTEXT_MENU`), `board_inspection_tool.cpp` (`NET_CONTEXT_MENU`), `group_tool.cpp` (`GROUP_CONTEXT_MENU`), `pcb_selection_tool.cpp` (`SELECT_MENU`) |
| Enter Group / Leave Group, Cancel, Clear Net Highlighting | **done**: Enter Group for one selected group, Leave Group while inside one, Cancel while a tool is active, Clear Net Highlighting first when a net is highlighted | `PCB_SELECTION_TOOL::Init` |
| Right click on a spot | **done**: with nothing selected the item under the pointer is selected first, as `selectPoint( aEvent.Position() )` does when the selection is empty; with a selection it stays (it used to retarget to the item under the pointer, which KiCad does not do) | `PCB_SELECTION_TOOL::Main` (`m_selection.Empty()` before the menu) |
| Not built, because the IR has nothing for them | table cells (Select Column(s), Row(s), Table), Swap Gate Nets, the generators' Update All Tuning Patterns | `PCB_SELECTION_TOOL::Init`, `EDIT_TOOL::Init` |
| Listed but dimmed | the pad settings (Copy / Paste / Push Pad Properties belong to the Footprint Editor tab here); most of the router's via, posture and corner-mode actions (only Place Through Via is wired, while routing; Track Corner Mode shows 45); entries whose action is unwired for a reason in `tools/ui-parity-missing.json` | `PAD_TOOL::Init`, `ROUTER_TOOL::Init` |
| Approximations | Zone Priority raise / lower is decided by the boxes of the zones overlapping, not their filled shapes; "Track Corner Mode" always shows 45 | `ZONE_PRIORITY_CONTEXT_MENU::update` |

Tests: `kicad-port/pcbContextMenu.test.ts` (36: the order for a footprint, a track, a group and an empty spot, no separator at either end or twice in a row, each submenu's conditions, the router's and the drawing tools' menus) and `pcbSelectionSummary.test.ts` (10: item counts by KiCad type, locked through a group, the group tool's facts, the point editor's, Zone Priority, Net Inspection, Convert). Seen in the browser (port 8808, a scratch copy of `mcu30`, events sent by script so the real pointer never moved): an empty spot lists exactly Select All, Unselect All, Get and Move Footprint, Paste, Paste Special, Zoom and Grid; a pad under the pointer gives the selection menu with the pad settings dimmed; a selection of footprints and then the group made from it each give their own menu (Enter Group for the group, Leave Group while inside it); and Group Items, Enter Group and Leave Group ran from the menu and left the selection and the entered group as the C++ does.


### Hotkey coverage audit

> **Historical.** This section is a snapshot from an early session and the
> tables below are stale (many rows have been implemented since). The
> current, regenerated list is `docs/parity/UI-ACTIONS.md`
> (`node web/studio/tools/ui-parity-audit.mjs`); the 14 pcbnew actions that
> list still had hotkeyed-and-missing are covered in section 20.

Of 766 extracted actions, 201 have a real default hotkey (hotkey or
macHotkey non-null). 51 of those are registered in
`useActionRunner.ts` (and therefore reachable from `useGlobalHotkeys.ts`,
the menu bar, and the toolbars) -- a prior session added
`selectAll`/`unselectAll` (Ctrl+A/Ctrl+Shift+A); a later one added
`highlightNet`/`clearHighlight` (`` ` ``/`~`), `SelectConnection` (U),
`moveExact` (Shift+M), `duplicate`/`copy`/`paste` (Cmd+D/C/V),
`layerNext`/`layerPrev` (+/-), and `layerAlphaInc`/`layerAlphaDec`
(}/{) -- 11 more, picked (per the task's item 5 instruction) for
user-visible impact once items 1-4 landed; **this session** added
`Drag45Degree` (`D`, gap #7 stage 5's frontend), `SettingsDialog`
(`Ctrl+<`, the router settings dialog -- see section 4), `DiffPair`
(`6`, gap #7 stage 7 -- see section 4), and `LengthTuner.TuneSingleTrack`
(`7`, gap #7 stage 8 -- see section 4). 150 are not registered. 55 of
those are `eeschema.*` -- out of scope for *pcbnew* parity specifically,
since this app's Schematic tab is read-only by design (no schematic
editing verbs exist in the backend yet). The remaining 95
(55 `pcbnew.*`, 40 `common.*` that apply to both editors) are genuine
pcbnew-parity gaps, listed here (alphabetically, same as before) per the
task's "explicitly listed as missing" instruction rather than left
silently unimplemented. The final report ranks what's left by
user-visible impact; this table is for lookup, not priority order.

#### `pcbnew.*` missing (55)

| Action | Hotkey | Label |
|---|---|---|
| `pcbnew.Array.createArray` | Ctrl+T | Create Array... |
| `pcbnew.Control.changeTrackLayerNext` | Ctrl++ | Switch Track to Next Layer |
| `pcbnew.Control.changeTrackLayerPrev` | Ctrl+- | Switch Track to Previous Layer |
| `pcbnew.Control.layerPairPresetCycle` | Shift+V | Cycle Layer Pair Presets |
| `pcbnew.EditorControl.EditFpInFpEditor` | Ctrl+E | Open in Footprint Editor |
| `pcbnew.EditorControl.EditLibFpInFpEditor` | Ctrl+Shift+E | Edit Library Footprint... |
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
| `pcbnew.InteractiveEdit.packAndMoveFootprints` | P | Pack and Move Footprints |
| `pcbnew.InteractiveEdit.skip` | Tab | Skip |
| `pcbnew.InteractiveEdit.swap` | Alt+S | Swap |
| `pcbnew.InteractiveMove.moveIndividually` | Ctrl+M | Move Individually |
| `pcbnew.InteractiveRouter.Autoroute` | Shift+F | Attempt Finish Selected (Autoroute) |
| `pcbnew.InteractiveRouter.ContinueFromEnd` | Ctrl+E | Route From Other End |
| `pcbnew.InteractiveRouter.DragFreeAngle` | G | Drag Free Angle |
| `pcbnew.InteractiveRouter.RouteSelected` | Shift+X | Route Selected |
| `pcbnew.InteractiveRouter.RouteSelectedFromEnd` | Shift+E | Route Selected From Other End |
| `pcbnew.InteractiveRouter.UndoLastSegment` | Backspace | Undo Last Segment |
| `pcbnew.InteractiveSelection.GrabUnconnected` | Shift+O | Grab Nearest Unconnected Footprints |
| `pcbnew.InteractiveSelection.SelectUnconnected` | O | Select All Unconnected Footprints |
| `pcbnew.InteractiveSelection.unrouteSegment` | Backspace | Unroute Segment |
| `pcbnew.lengthTuner.AmplDecrease` | 4 | Decrease Amplitude |
| `pcbnew.lengthTuner.AmplIncrease` | 3 | Increase Amplitude |
| `pcbnew.LengthTuner.Settings` | Ctrl+L | Length Tuning Settings... |
| `pcbnew.lengthTuner.SpacingDecrease` | 2 | Decrease Spacing |
| `pcbnew.lengthTuner.SpacingIncrease` | 1 | Increase Spacing |
| `pcbnew.LengthTuner.TuneDiffPair` | 8 | Tune Length of a Differential Pair |
| `pcbnew.LengthTuner.TuneDiffPairSkew` | 9 | Tune Skew of a Differential Pair |
| `pcbnew.ModuleEditor.newFootprint` | Ctrl+N | New Footprint |
| `pcbnew.PadTool.explodePad` | Ctrl+E | Edit Pad as Graphic Shapes |
| `pcbnew.PadTool.recombinePad` | Ctrl+E | Finish Pad Edit |
| `pcbnew.PointEditor.addCorner` | F1 | Create Corner |
| `pcbnew.PositionRelative.positionRelative` | Shift+P | Position Relative To... |
| `pcbnew.TableEditor.editTable` | Ctrl+E | Edit Table... |
| `pcbnew.ZoneFiller.zoneFillAll` | B | Fill All Zones |
| `pcbnew.ZoneFiller.zoneUnfillAll` | Ctrl+B | Unfill All Zones |

#### `common.*` missing, relevant to pcbnew (40)

_A snapshot from an early session: most of these are wired since. The current state of the shared actions is `docs/parity/UI-ACTIONS.md` (generated) and `PARITY-common.md`._


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
| `common.Interactive.cut`/`pasteSpecial`/`copyAsText` | Ctrl+X/Shift+V/Shift+C | `copy`/`paste`/`duplicate` done this session (section 5) -- `cut` (trivially "copy then delete") wasn't wired; `pasteSpecial`/`copyAsText` have no real analogue on the PCB (no "paste without net/position" variant, no text-serialized clipboard format); the Schematic tab has Paste Special (`PARITY-sch.md` section 15) |
| `common.Interactive.cycleArcEditMode` | Ctrl+Space | Cycle Arc Editing Mode |
| `common.Interactive.find`/`findAndReplace`/`findNext`/`findPrevious`/`findNextMarker` | Ctrl+F, ... | Find/Replace |
| `common.Interactive.finish` | End | Finish (generic "end the current interactive action") |
| `common.Interactive.measureTool` | Ctrl+Shift+M | Measure Tool |
| `common.SuiteControl.openPreferences` | Ctrl+, | Preferences... (no Preferences dialog in this app) |

## 7a. Find on the PCB (GAPS.md item 21; `dialog_find.cpp`, `eda_item.cpp`, `pcb_selection_tool.cpp`)

Find, Find Next and Find Previous were registered on the Schematic tab only; on the board Ctrl+F had nothing to open. Now the board has KiCad's Find dialog
(`components/PcbFindDialog.tsx`): a floating, modeless pane that is created once and only hidden when closed, so F3 and Shift+F3 go on through the same
hit list after it is closed. The matching and the walk are `kicad-port/pcbFind.ts` (pure), the dialog's state (options, hit list, position in it, history)
is `state/pcbFind.ts`, and what one press does is `actions/pcbFindActions.ts` (`common.Interactive.find`, `findNext`, `findPrevious`, registered on the
PCB tab).

| Behaviour | Status | KiCad file:function |
|---|---|---|
| The dialog: Search for (with the last ten texts as history), Match case, Whole words only, Wildcards, Wrap, Search footprint reference designators / footprint values / other text items / DRC markers / net names, Include hidden fields; Find Next, Find Previous, Restart Search, Close; a status line | **done**: the text takes the focus, selected, each time the dialog is shown; Enter in it is Find Next; Escape closes; the defaults are the dialog's (Wrap, references, markers, values, nets and other text on) | `DIALOG_FIND` (`dialog_find_base.cpp`, `DIALOG_FIND::Show`), `PCB_EDIT_FRAME::GetFindHistoryList` |
| Ctrl+F, F3, Shift+F3 | **done**: Ctrl+F (Cmd+F on a Mac) opens the dialog with the value of the one selected footprint or the first line of the one selected text, else the text of last time; F3 and Shift+F3 step the list, and open the dialog first when it was never shown. Find and Replace, Update Find and Find Next Marker are not board actions in KiCad either | `PCB_EDIT_FRAME::ShowFindDialog`, `FindNext`, `BOARD_EDITOR_CONTROL::Find` / `FindNext` (`ACTIONS::find`, `findNext`, `findPrevious`) |
| Matching: plain (the text occurs anywhere), Whole words only (no letter, digit or `_` just before or after an occurrence), Wildcards (`*` any run, `?` any one character, against the whole text); upper-cased on both sides unless Match case; Whole words wins when both are on | **done**; the characters a regular expression would read are plain in a mask | `EDA_ITEM::Matches`, `wxString::Matches`, `DIALOG_FIND::search` (`if( m_matchWords ) ... else if( m_wildcards )`) |
| The hit list: each placed footprint whose reference (if on) or value (if on) matches, then the board's texts, then the DRC markers whose message matches, then the nets whose name matches, each in the board's own order | **done** | `DIALOG_FIND::search` |
| Find Next / Find Previous: the selection is cleared on every press, the list is built again when an option, the board or the DRC run changed (or Restart Search), the first press shows the first hit (the last going back), a press past the end wraps with Wrap on and says "No more items to show" and stays on the last real hit with it off | **done**: status "Hit(s): 2 / 5", "No hits", "'text' not found" (also as a message) | `DIALOG_FIND::search` (`m_hitList`, `m_it`, `m_upToDate`, `endIsReached`, `ShowInfoBarMsg`) |
| Showing a hit | **done**: a footprint or a text is selected; a net selects its tracks and vias that the Selection Filter lets through (locked ones only when it allows them); a marker selects the DRC row; the view is moved to the hit only when it lies outside the middle 80 % of the view, and zoomed out (never in) until a big hit fits in half the view | `PCB_SELECTION_TOOL::FindItem` (`SelectAllItemsOnNet`), `EDA_DRAW_FRAME::FocusOnLocation` -- `kicad-port/pcbFind.ts` `focusView` |
| Find by Properties | not done: it needs the property manager and a PCBEXPR search over every board item; recorded with its reason in `tools/ui-parity-missing.json` | `PCB_ACTIONS::findByProperties`, `DIALOG_FIND_BY_PROPERTIES` |
| Include hidden fields | the box is there and does nothing: a footprint here has only its visible reference and value, so no hidden field exists to search | `PCB_FIELD::Matches` (`frd.searchAllFields`) |
| Zones, a footprint's other texts | a zone has no name in the IR, so none matches (`ZONE::Matches` reads the zone name); the footprints this studio writes carry no other text | `ZONE::Matches`, `FOOTPRINT::Matches` |
| "Show search panel" link of the dialog | not done: `common.Interactive.search` still jumps to the Activity tab | `ACTIONS::showSearch` |

Tests: `kicad-port/pcbFind.test.ts` (18: plain, whole-word and wildcard matching, every checkbox narrowing the list, the order of the list, the walk with Wrap on and off in both directions, a single hit, the status text, the preload, the view). Seen in the browser (port 8808, a scratch copy of `mcu30`): Cmd+F opened the dialog, "C1" with Enter selected C1 with "Hit(s): 1 / 4", and with the dialog closed F3 went to C10 and C11 and Shift+F3 back to C10.

## 8. Zones

Port of `pcbnew/zone_filler_tool.cpp` (fill/unfill, display mode) and
`pcbnew/dialogs/panel_zone_properties.cpp` (the settings panel). The fill
*engine* itself (`crates/zone-filler`) and the IR's `ZONE_SETTINGS` fields
(`crates/model/src/ir.rs` `Zone`) were already ported in an earlier
session (see this file's intro and `docs/parity/GAPS.md` #5) -- this
session is the UI on top of that: drawing the real computed fill, the
fill/unfill and display-mode actions, and a settings dialog that actually
reaches every one of those IR fields instead of just net/layer.

| Behavior | Status | KiCad file:function |
|---|---|---|
| B = Fill All Zones (GET /api/fill, the real ported `ZONE_FILLER`) | identical in effect | `zone_filler_tool.cpp:ZoneFillAll`/`FillAllZones` -- `useActionRunner.ts`'s `pcbnew.ZoneFiller.zoneFillAll`, `state/store.tsx`'s `zoneFill`/`fillZones`. No per-zone fill cache to mutate (this app's `/api/fill` always recomputes from scratch, cheaply) -- "fill" is "go fetch it", kept live-updated on every board-version change while any fill is showing (no "stale fill" hatch state to show, unlike source, since there's nothing cached to go stale) |
| Ctrl+B = Unfill All Zones | identical in effect | `zone_filler_tool.cpp:ZoneUnfillAll` -- `unfillZones` just discards `state.zoneFill`, same end state as source's `zone->UnFill()` |
| Per-zone Fill/Unfill (`pcbnew.ZoneFiller.zoneFill`/`zoneUnfill`, selection-scoped) | missing | only the *All* variants are wired -- this app's `/api/fill` already computes every zone each call, so a selection-scoped fetch would save nothing server-side; the real gap is no fine-grained *display* control (next row). **Status (2026-10-07): closed** -- Draft Fill and Unfill Selected Zone(s) are wired (`PARITY-boardctl.md` section 2) |
| `ZONE_DISPLAY_MODE`: solid fill vs. outline-only, independent of whether fill data exists | 2 of 4 modes ported | `pcb_control.cpp:ZoneDisplayMode` (`PCB_ACTIONS::zoneDisplayEnable`/`Disable`/`Toggle`) -- `state.zoneDisplayMode`, `painter.ts:drawZones`. Source's other two (`zoneDisplayOutlines` = fracture-borders, `zoneDisplayTesselation` = triangulation) are developer debug views of the filler's internal geometry, not ported. A zone with no fill data at all (never filled, or just Ctrl+B'd) always shows its outline regardless of this mode, matching source (nothing to fill with) |
| Filled-zone rendering: each fragment (island) as its own closed, already-`Fracture`d ring | identical | `pcb_painter.cpp`'s zone-fill paint (polygon fill, no separate even-odd holes pass needed) -- `crates/cli/src/studio.rs:fill_json`'s own doc on why fragments are pre-fractured |
| Selected zone, filled mode | simplified | source's selection-shadow layer isn't modeled; this app strokes the zone's own (unfractured) outline in the selection color on top of the solid fill instead -- same visual intent, different mechanism |
| Zone Properties dialog: net, layer, clearance, min width, priority | identical | `panel_zone_properties.cpp`'s `TransferZoneSettingsToWindow`/`AcceptOptions` -- `ZoneDialog.tsx` |
| Pad connection (Solid/Thermal/PTH-only-thermal/None), thermal gap + spoke width | identical, including source's own "never disabled by the connection choice" rule for the gap/spoke fields | same file -- `m_PadInZoneOpt`, `m_antipadClearance`/`m_spokeWidth` (explicitly never `Enable(false)`-d, per that file's own comment, since a per-pad override can still need them) |
| A pad's or footprint's own connection beats the zone's: Footprint Properties has "Pad connection" and "Clearance" for the footprint and, per pad number, "Pad connection", "Relief gap", "Spoke width", "Spoke angle" and "Clearance"; Pad Properties in the Footprint Editor has the same five and publishes them with the library footprint | identical in effect, narrower in layout | `dialog_footprint_properties.cpp` (Clearances tab) and `dialog_pad_properties.cpp` ("Clearance Overrides and Settings"): `FOOTPRINT::GetLocalZoneConnection`, `PAD::GetLocalZoneConnection`, `GetLocalThermalGapOverride`, `GetLocalThermalSpokeWidthOverride`, `GetThermalSpokeAngle`, `GetLocalClearance`. `components/FootprintPropertiesDialog.tsx` (placed footprint: two "Apply" buttons, one undo step each) and `footprint/{Pad,Footprint}PropertiesDialog.tsx`; helpers in `kicad-port/padZone.ts` (unit tested). Backend: `Cmd::SetFootprintZoneConnection` and `Cmd::SetPadZoneOverrides` (`crates/ops/src/zone_overrides.rs`, 5 tests) store the edit in `drawings.zone_overrides`, which `Design::pad_zone_facts` merges over what a board import left (`PadMaskInfo`, `FootprintExtra`) and over the published library footprint; the importer reads `zone_connect`, `thermal_bridge_angle`, `thermal_gap`, `thermal_bridge_width` and `clearance` of pads, `zone_connect` and `clearance` of footprints, and `write_footprint` writes them. The filler uses them (`ZONE_FILLER::knockoutThermalReliefs` -> `DRC_ENGINE::EvalZoneConnection`, pad over footprint over zone, plated-through-hole thermal for the "PTH only" choice). A pad's clearance override replaces the net class's and the zone's clearance for that pad but is floored at the board minimum, as `EvalRules` does; 0 is not an override. Copy, Paste and Duplicate hand them on to the new footprint (the clipboard text carries them: `write_footprint` writes the resolved facts, `ClipFootprint::zone_overrides` reads them back, `Design::zone_overrides_of`; `crates/ops/src/pcb_paste.rs`). Different from KiCad: the Pad Properties panel for a pad on the board is not editable (item 9 of `GAPS.md`), so the pad fields sit in Footprint Properties, found by pad number, and edit every pad of the footprint with that number |
| Island removal (always/never/below area limit) + area threshold | identical | same file -- `m_cbRemoveIslands`/`m_islandThreshold`, shown only in "Area" mode |
| Fill type (solid/hatched) + hatch width/gap/orientation/smoothing level/smoothing value | identical, shown only when Hatched | same file -- `m_cbHatched`'s `onHatched` enable/disable group |
| Corner smoothing: None / Chamfer / Fillet, and the radius (shown for Chamfer and Fillet, stored as 0 for None) | identical | `panel_zone_properties.cpp`'s `m_cornerSmoothingChoice`, `m_cornerRadius`, `OnCornerSmoothingSelection` and `AcceptOptions` -- `ZoneDialog.tsx`, `Cmd::EditZone`'s `smoothing` and `corner_radius` (optional: a command that leaves them out keeps them). The filler smooths the outline before it fills (`ZONE::BuildSmoothedPoly`; `crates/zone-filler/src/{smoothed,corner}.rs`), so the fill and the outline drawn under it differ the way KiCad's do; measured against kicad-cli in `crates/zone-filler/PARITY.md` |
| Zone name, Locked, outline border display style (hatched/full/invisible), per-layer hatch-offset overrides, teardrops | not in Zone Properties. The name is edited in the Properties panel (`Cmd::SetZoneName`); the IR carries the border style and the writer writes it, but no field or verb edits it; there is no per-layer hatch offset or teardrop control | `panel_zone_properties.cpp`'s `m_tcZoneName`, `m_cbLocked`, `m_outlineDisplayCtrl` and the per-layer overrides panel -- same "nothing to show, not a bug" convention as every other dialog here for a field with no editor |
| Net picker limited to nets actually present on a pad | identical in spirit, simpler widget | source's `NET_SELECTOR` (searchable combo over the whole board netlist); this app's `<select>` over `state.board.parts[].pads[].net` (unchanged from before this session) |
| Multi-copper-layer zones (one outline, several layers) | not ported -- single-layer `Zone.layer: string` | `ZONE::GetLayerSet()`; this model's zones are one layer each, unchanged scope from before this session |
| Add flow: outline first, then settings (vs. source's settings-first-then-draw) | unchanged, documented simplification from an earlier session | `ZoneDialog.tsx`'s own header comment; the dialog itself is now the full settings panel, not just net/layer |
| Add flow backend shape: `add_zone` (net/layer/outline, unchanged 3-field `Cmd`) + an immediate `edit_zone` only if any setting differs from `Zone::default()` | deliberate 2-Cmd design, not 1 atomic Cmd | keeps `Cmd::AddZone`'s wire shape stable for every existing caller (the CLI, this crate's own tests) -- `state/store.tsx`'s `addZone`, `crates/ops/src/lib.rs`'s `Cmd::EditZone`/`edit_zone` |
| Zone outline editing: drag a corner, double-click an edge to add one, right-click a corner to delete it | ported, scoped down from source -- see section 11 | `pcbnew/tools/pcb_point_editor.cpp` -- `kicad-port/zonePointEditor.ts` (corner/edge hit-testing, 9 unit tests), `Cmd::SetZoneOutline`, `Canvas.tsx`'s onPointerDown/Move/Up + onDoubleClick + onContextMenu, `painter.ts:drawZoneHandles` |
| Rule Areas (keepouts) | ported -- see section 14 | `pcbnew.InteractiveDrawing.ruleArea` |
| Zone Cutout / Similar Zone | missing, unchanged. **Status (2026-10-07): closed** -- both are registered (`actions/useActionRunner.ts`; `CODE-COMPARE-ui.md` section 1) | `pcbnew.InteractiveDrawing.zoneCutout`/`similarZone` |

Rust: `crates/ops/src/lib.rs`'s `Cmd::EditZone`/`edit_zone` (full
`ZONE_SETTINGS` replace, outline untouched; validates clearance >= 0, min
width > 0, thermal spoke >= min width, and -- matching source's own
`AcceptOptions` -- hatch thickness/gap >= min width in hatch mode), 5 new
tests in `crates/ops/src/tests.rs`. `crates/cli/src/studio.rs`'s
`fill_json`'s endpoint was already there; `state()`'s `zones` JSON and
`routing` JSON both gained the new fields (full settings; track/via preset
lists, see section 9).

## 9. Board Setup

Port of `pcbnew/dialogs/dialog_board_setup.cpp`, a tree of `panel_setup_*.cpp` pages. `BoardSetupDialog.tsx` is that treebook (Board Stackup /
Text & Graphics / Design Rules, with KiCad's own page names); the rule pages are in `components/boardSetup/*`, and what they share -- the net-class
patterns, the checks each KiCad panel makes before OK, the default stackup, the severity table -- is in `kicad-port/boardSetupRules.ts` with node
unit tests.

Every page edits. A rule page keeps a copy of what the board says and Apply sends the whole page as one undoable command (`set_net_classes`,
`set_constraints`, `set_mask_paste`, `set_text_graphics_defaults`, `set_stackup`, `set_rule_severities`, `set_custom_rules` --
`crates/ops/src/board_setup.rs`). The backend keeps the page in `design.json` (`drawings.rules`, `eda_model::rules::RulesOverlay`) and
`board::load` lays it over the intent's (or the imported project's) rules, so the router, DRC, the gates and the KiCad project kicad-cli reads
(`.kicad_pro`, the `(setup ..)` and `(stackup ..)` of the `.kicad_pcb`, the `.kicad_dru`) follow one set of rules, and one Undo takes a page back. That
overlay is the second edit/undo path into the intent's rules that this section used to say was missing. `crates/cli/src/board.rs` has the tests that a
changed clearance, a net class assigned by a pattern, a minimum track width, an ignored check and a custom rule each change what kicad-cli reports.
A page with edits not applied yet is marked in the tree; leaving the dialog asks first.

| Page | Status | KiCad file |
|---|---|---|
| Board Stackup > Physical Stackup (with Board Finish): the copper layer count (even, 2 to 32), each layer's type, material and thickness, the dielectrics' constant and loss tangent, copper finish, plated edge, edge connectors; the board thickness is what the layers add up to | editable. Not ported: colours, a dielectric's sub-layers, adding or removing a dielectric, Adjust Dielectric Thickness, impedance control. A smaller layer count is refused while a track, via or zone is on a layer that would go (KiCad deletes them) | `panel_board_stackup.cpp`, `panel_board_finish.cpp`, `board_stackup.cpp` -- the default stackup is computed in `eda_model::rules` and in `boardSetupRules.ts`, and both are held to `src/kicad/default_stackups.json` |
| Board Stackup > Solder Mask/Paste: expansion, web width, mask-to-copper clearance, bridging inside footprints, via tenting, paste clearance and ratio | editable | `panel_setup_mask_and_paste.cpp` |
| Text & Graphics > Defaults: line thickness, text size and thickness, italic and upright for each layer class | editable, and written to the derived project so KiCad uses them; this app's own drawing tools do not start new items from them yet | `panel_setup_text_and_graphics.cpp` |
| Text & Graphics > Dimensions: units, format, precision, text position and style defaults, applied once at creation, never retroactively | editable (task item 7) | `panel_setup_dimensions.cpp` |
| Design Rules > Constraints: minimum clearance, track width, connection width, annular width, via diameter, drill, micro-via size and drill, hole to hole, copper to hole, copper to edge, silk clearance, text height and thickness, arc deviation, thermal spoke count, external fillets, stackup height in lengths | editable; the ranges of `ValidateDesignRules` are checked in the page and again in the backend. An intent states no minimums, so DRC uses the narrowest sizes its classes ask for until the first Apply makes the shown ones the board's own | `panel_setup_constraints.cpp` |
| Design Rules > Pre-defined Sizes: the W/Shift+W and via-size-cycle preset lists, add/remove entries | editable | `panel_setup_tracks_and_vias.cpp` -- `Cmd::SetTrackWidthPresets`/`SetViaPresets` (whole-list replace, no per-entry Cmd, same spirit `paste_items` already uses for several items in one commit) |
| Design Rules > Teardrops | editable (task item 4) | `panel_setup_teardrops.cpp` |
| Design Rules > Net Classes: the Default class and the others (clearance, track width, via size and hole, micro-via, differential pair), and the table of patterns that assign nets to a class, with the nets a pattern matches | editable. A pattern is `*` and `?` over the whole net name; `a\|b` and `prefix(a\|b)` become one pattern per alternative, KiCad's regular-expression mode and bus notation are not read. The first class in the table whose pattern matches a net owns it (KiCad merges every matching class by priority); "priority" here is the routing order, not KiCad's. Not ported: tuning profile, PCB colour, schematic wire columns | `panel_setup_netclasses.cpp` (its Assign Netclass dialog proposes patterns the same way: `proposePattern`) |
| Assign Netclass... (right-click a track, via or zone; `pcbnew.EditorControl.assignNetclass`) | partial: the dialog (`components/AssignNetclassDialog.tsx`) proposes the pattern KiCad does for the selected nets (`GetNetclassPatternForSet`), the class (the first one besides Default) and the nets the pattern matches; OK stores the assignment with the net classes (`set_net_classes`, one undo step). The context-menu entry follows `EDIT_TOOL::Init`: shown when every selected item is a track, via or zone (a selected footprint does not get it, as in KiCad). Not ported: the canvas preview that selects the matching nets while typing | `common/dialogs/dialog_assign_netclass.cpp`, `pcbnew/tools/edit_tool.cpp` |
| Design Rules > Custom Rules: the text of the `.kicad_dru` | editable as text; "Check rule syntax" asks kicad-cli (it has no checker, and drops a file it cannot parse without a word, so the backend loads the text beside a probe board and names the first top-level rule that stops it). Not ported: the highlighting editor, Syntax Help, the compiler's extra warnings, the rule-tree designer (`drcRuleEditor`) | `panel_setup_rules.cpp` |
| Design Rules > Violation Severity: Error / Warning / Ignore for each of the 64 checks | editable; the list and the defaults come from `drc_item.cpp` (`tools/extract-drc-checks.js`). The two library-link checks start at Ignore (every footprint is embedded in the board) | `panel_setup_severities.cpp` |
| Board Editor Layers, Zone Hatch Offsets, Formatting, Text Variables, Length-tuning Patterns, Tuning Profiles, Component Classes, Embedded Files | not ported -- nothing in the model holds them | |

W/Shift+W (`pcbnew.EditorControl.trackWidthInc`/`Dec`) and the via-size
cycle (`viaSizeInc`/`Dec`) now read this page's lists -- `useActionRunner.ts`:
cycling updates `state.currentTrackWidthUm`/`currentViaPreset` (read by
Canvas.tsx's route/via tools for the *next* item) and, matching source's
own dual-purpose behavior, also applies the new size to every selected
track/via in the same keypress via `set_track_width`/`edit_via`.

Rust: `crates/cli/src/studio.rs`'s `state()` sends `board_rules` with the rules as the board is judged by them now (`default_class`,
`constraints`, `mask_paste`, `text_graphics`, `layer_names`, `severities`, `custom_rules_text`, and `overlay`, the pages an edit has replaced) beside the
older `net_classes`/`stackup` fields, and `routing.track_width_presets`/`via_presets`; `POST /api/check_rules` is Custom Rules' syntax check.

## 9a. Design Rules Checker: the review workflow (GAPS.md item 11 of the second phase; `dialog_drc.cpp`, `rc_item.cpp`, `drc_tool.cpp`, `panel_setup_severities.cpp`)

DRC is kicad-cli's (`GET /api/drc` -> `kicad-cli pcb drc --severity-all`, mapped back to our ids); this section is what the studio does around a report.
The list model is `RC_TREE_MODEL` (`kicad-port/rcItems.ts`, `components/RcList.tsx`), the dialog `DrcDialog.tsx`, what a click or a menu entry does
`actions/checkerOps.ts`; every pure rule has a node test (`rcItems.test.ts`, `markerHit.test.ts`).

| Behaviour | Status | KiCad file:function |
|---|---|---|
| The dialog: Run DRC (running state, out-of-date marking), Refill zones, Test for parity between PCB and schematic, pages Violations / Unconnected Items / Schematic Parity / Ignored Tests (+ our Lint), Show All / Errors / Warnings / Exclusions with the counts, error / warning / excluded badges | identical (2026-10-08) | `DIALOG_DRC` (`dialog_drc_base.fbp`): `updateDisplayedCounts` counts every marker whatever the boxes say, the page titles say `(n)` of the rows listed and `(not run)` for a parity test that did not run |
| A marker's rows: its own line in bold with the prefix (`Error: `, `Warning: `, `Excluded warning: `), the items it names, an exclusion's comment last; an excluded marker is dim and italic | identical | `RC_TREE_MODEL::GetValue` / `GetAttr` |
| Exclude this violation, Exclude with comment..., Edit exclusion comment..., Remove exclusion | identical, undoable | `DIALOG_DRC::OnDRCItemRClick`: `Cmd::AddDrcExclusions` / `DeleteDrcExclusions` (one undo step each), stored in `design.drawings.drc_exclusions` as the check, the uuids of the items, the comment and the marker positions worth trying (`eda_model::ir::DrcExclusion`, additive); the report on screen is patched in place. The "Exclusion Comment" prompt is `WX_TEXT_ENTRY_DIALOG` |
| Exclude all / Remove all exclusions of a check | **adapted** | KiCad's menu does it per violated custom rule (`GetViolatingRule`, DRC only), which kicad-cli's report does not name; here it is every violation of the check, board or schematic, one undo step |
| Exclusions reach kicad-cli | **partial, and why** | written to the derived `.kicad_pro` as `board.design_settings.drc_exclusions`, `PCB_MARKER::SerializeToString`'s text, one entry for each position worth trying (`crates/kicad/src/drc_exclusions.rs`). KiCad matches an exclusion to a marker by that text, **position included, exactly** (`BOARD::ResolveDRCExclusions`), and kicad-cli's report says where the *items* are, never the marker -- so the studio offers the items' own positions, a track's ends and middle (`(start + end) / 2`, `DRC_TEST_PROVIDER_TRACK_WIDTH`) and the middle of the first two items. Where the marker sits at one of those -- a via or a pad, a footprint, an unconnected pair, a narrow track, often a clearance at a track end -- kicad-cli lists the violation as `excluded` and a report that does not ask for exclusions leaves it out (slow test `a_waived_violation_reaches_kicad_cli_through_the_derived_project...`); where it sits elsewhere (the contact point of a track and a via, silkscreen over copper) only the studio's report is waived (`kicad_matched: false`, said in the row's tooltip). The studio matches by check and the items' uuids, which `eda_kicad::export_kicad_pcb_mapped` mints deterministically, so a waiver survives a restart. Not written: checks whose text carries the marker's layer (`copper_sliver`, `starved_thermal`) |
| Gates and the CLI scorecard | a waived violation is not a finding | `gate_of` and `print_drc` skip `excluded` |
| Next Marker, Previous Marker, Exclude Marker | identical, no default hotkey (KiCad has none) | `PARITY-common.md` section 5 |
| Right click a canvas marker: Exclude, Exclude all of the check, Change severity, Ignore, Edit severities, Show in the Design Rules Checker | identical in spirit | a DRC marker is a circle (`drawDrcMarkers`; a waived one is drawn dim in `LAYER_DRC_EXCLUSION`); the menu is the dialog's plus `DRC_TOOL::CrossProbe` (the dialog comes up with the marker's row selected). Not ported: selecting a marker with a left click |
| Change severity to Error / Warning for all..., Ignore all... | identical | `OnDRCItemRClick`: `set_rule_severities` (Board Setup's table, one undo step); the report on screen follows as KiCad's markers do -- an ignored check's markers go and it joins the ignored tests |
| Ignored Tests: the checks set to Ignore; right click a check for Error / Warning / Ignore | identical | `m_ignoredList`, `OnIgnoredItemRClick` -- the list is the report's `ignored_checks` (`DRC_REPORT::WriteJsonReport`) |
| Schematic Parity: board against schematic | identical, by running KiCad's own | `kicad-cli pcb drc --schematic-parity`: the derived schematic (the one the engine derives from the intent when none is stored) is written beside the derived board, which is where `JobExportDrc` looks for it. A reason kicad-cli gives for skipping the test ("Schematic parity tests require a fully annotated schematic.") is shown on the page. A design whose schematic is not connected or has no footprint links shows every footprint and pad as a difference, as kicad-cli reports them |
| Delete Marker / Delete All Markers, Save report, Report all errors for each track, cross-probe options | not ported | markers are the report's rows (Run DRC replaces them); `--all-track-errors` is not asked for |

Verified in the browser (port 8802, a scratch copy of mcu30, the real pointer never touched): `e2e/drc-review.check.js` is a 33-step check through `window.__eda`
that passes -- Run DRC, Next / Previous / Exclude Marker on both editors, the marker menu on a row and on a canvas marker (right click on the circle, Show in the
dialog), Exclude with comment (the prompt, the comment under the row, the comment back from `GET /api/drc`), Exclude all of a check, Remove, the Show boxes, Ignored Tests
(the radio menu changes a check's severity), Schematic Parity (a run with the box on fills the page), ERC's Change severity, Schematic Setup opening on Violation
Severity, and no console errors. Seen by eye: the dialogs, the Schematic Setup page, both canvas menus.

## 10. Property dialogs (GAPS.md #11)

| Dialog | Status | KiCad file |
|---|---|---|
| Footprint Properties: refdes text placement (above/below/left/right) | superseded (2026-10-09): the dialog is the field grid of section 26; `Cmd::SetLabelSide` still places the Reference of a footprint nothing was edited on | `pcb_properties_panel.cpp`'s sibling -- `Cmd::SetLabelSide` |
| Footprint Properties: zone connection and clearance of the footprint; per pad number the pad connection, relief gap, spoke width, spoke angle and clearance | editable (new, 2026-10-08); since 2026-10-09 the fields are in the same dialog as the field grid and the attributes and go out in its one OK (one undo step with them), and in Pad Properties (`BoardPadPropertiesDialog.tsx`), where the pad's clearance is the same one | `dialog_footprint_properties.cpp` (Clearances tab) and `dialog_pad_properties.cpp` -- `Cmd::SetFootprintZoneConnection` and `Cmd::SetPadZoneOverrides`, one undo step each; section 8 has the detail, and the filler reads them |
| Footprint Properties: Reference/Value/Footprint/MPN | the text of the Reference and Value, the library link and the MPN are still read-only (they live on the *intent*-derived model, `Part`, and the BOM); **what is drawn is editable (2026-10-09)**: the layout of the Reference and Value, user fields, attributes and pads are `design.json` overlay data -- section 26 | `dialog_footprint_properties.cpp`, `dialog_pad_properties.cpp`; same model-split reasoning as Board Setup's Net Classes page (section 9) for the text |
| Track/Via Properties: via diameter + drill | editable (new) | `dialog_track_via_properties.cpp` -- `Cmd::EditVia`, `ItemPropertiesDialog.tsx`'s via branch. Net, position, and layer span stay read-only, matching source's own dialog (re-netting/re-spanning an existing via isn't a field edit there either) |
| Shape Properties: layer, line width, filled | editable (new) | `pcb_shape`'s own properties -- `Cmd::EditShape`, `ItemPropertiesDialog.tsx`'s shape branch. Geometry itself has no dialog field in source either (only draggable corners, no point editor for shapes in this app) |
| Zone Properties | see section 8 -- its own full dialog, not `ItemPropertiesDialog.tsx` | `panel_zone_properties.cpp` |
| Text Properties | already editable (predates this session) | `edit_text`, `TextDialog.tsx` |

## 11. Smaller tools

| Tool | Status | KiCad file:function |
|---|---|---|
| Multi-selection Rotate/Flip around a shared pivot | now ported for the plain (not mid-drag) case -- see section 4 | `edit_tool.cpp:Rotate`/`Flip`, `updateModificationPoint` |
| Cut (Cmd+X) | ported: copy then delete, same as the honorable-mention note in this file's old hotkey audit said it trivially is | `common.Interactive.cut` -- `useActionRunner.ts` calls `copySelection()` then the same `common.Interactive.delete` handler Del uses |
| Measure tool (Ctrl+Shift+M) | ported as a client-side-only ruler -- never committed to the backend, same as KiCad's own (a measurement isn't a board item) | `common.Interactive.measureTool`, `pcb_viewer_tools.cpp` -- `state.drawState`'s new `"measure"` kind (1 point = still dragging the end, rubber-banded; 2 = a fixed ruler that stays on screen until Esc or a fresh click), `painter.ts:drawInProgress`'s distance+dx+dy label in the status bar's own unit. **Not ported**: source's temporary on-canvas unit/angle readout in a dedicated corner HUD -- this app labels the ruler itself instead |
| Align Left/Right/Top/Bottom/Center H/Center V | ported, placed footprints only | `align_distribute_tool.cpp`'s `AlignLeft`/... -- pure math in `kicad-port/alignDistribute.ts` (12 unit tests), `state/store.tsx:alignSelection`. **Not ported**: source's "prefer a locked item, else the item under the cursor" override when picking the alignment target (this app has no locked-item concept; always uses the extreme item, source's own fallback once neither override applies) and the mirrored-view Left/Right swap (this app's view never mirrors) |
| Distribute Horizontally/Vertically, by even gaps or by center spacing | ported, placed footprints only, 3+ items (source's own floor) | `align_distribute_tool.cpp:DistributeItems`/`doDistributeGaps`/`doDistributeCenters`, `libs/kimath/src/geometry/distribute.cpp`'s exact gap-math, ported verbatim including the "end-cap items never move" rule -- `kicad-port/alignDistribute.ts`, `state/store.tsx:distributeSelection` |
| Right-click context menu built from the selection (GAPS.md #30) | **done (2026-10-08)**: was partial (a static list with Copy/Cut/Duplicate/Move Exactly/Align/Distribute, greyed when they did not apply). The menu is KiCad's now, with its conditions, order and submenus: section 7 | `kicad-port/pcbContextMenu.ts`, `components/canvas/pcbMenuBuilder.ts`, `Canvas.tsx:onContextMenu` |
| Zone outline editing: drag a corner, double-click an edge to add one, right-click a corner to delete it | ported | `pcbnew/tools/pcb_point_editor.cpp` -- `kicad-port/zonePointEditor.ts`, `Cmd::SetZoneOutline` (`crates/ops`, 1 new test), `Canvas.tsx`, `painter.ts:drawZoneHandles`. Scope, matching this app's existing point-editor-less baseline rather than a full port: a single selected zone only (no multi-select point editing, no graphic-shape point editor either -- see section 10's Shape Properties row); no 45/90-degree edge-angle constraint while dragging a corner (source's Ctrl-held behavior); no "equal length" guide overlay; corner drag snaps through `BestSnapAnchor` like KiCad's point editor (`PcbSnap.point( "point_edit" )`: the zone's layer, the current grid, the zone itself skipped; section 2) |
| Array tool, grouping, dimensioning | missing, unchanged. **Status (2026-10-07): no longer missing** -- ported in sections 16, 17 and 18; what is left of each is in `docs/parity/GAPS.md` | GAPS.md #26/#27/#28 -- out of scope this session |

## 11a. Place Footprint and the Footprint Chooser (`footprint_chooser_frame.cpp`, `panel_footprint_chooser.cpp`, `footprint_info.cpp`, `footprint.cpp`, `board_editor_control.cpp::PlaceFootprint`; GAPS.md item 5)

Before this section Place Footprint (`A`) listed the schematic's unplaced parts only, so a footprint with no symbol -- a mounting hole, a fiducial, a test point -- could not be put on a board. Now it opens KiCad's Footprint Chooser over the 155 footprint libraries KiCad.app
ships (`/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints`, `EDA_KICAD_FOOTPRINTS`, about 15,000 footprints, 179 MB). Ported from commit `8303b2ad`. The tree, search, recently used and keys are the Symbol Chooser's (`PARITY-sch.md` section 3a: `kicad-port/libChooser.ts`,
`components/chooser/`, one dialog per chooser); what is the footprint chooser's own is below.

| Behavior | Status | KiCad file:function |
|---|---|---|
| `A` opens the chooser; choosing a footprint arms it and the next click on the board puts it there (Esc cancels); the part is a footprint of its own, top side, not turned, its pads on no net | **done**: `Cmd::PlaceFootprint { footprint, at }` -- a `BoardPart` (`DrawingsSection::board_parts`, the kind a Duplicate or a Paste makes) with the footprint's pads, graphics and courtyard carried in the command (the server reads them from the installed library: `crates/cli/src/library_place.rs`), so the board needs no library to be redrawn; one undo step | `BOARD_EDITOR_CONTROL::PlaceFootprint`, `PCB_BASE_FRAME::SelectFootprintFromLibrary` |
| The reference of the new part | **changed**: a library footprint's reference is `REF**` and KiCad keeps it until the user annotates; a part of this model needs a reference of its own, so it takes the usual prefix of its library with the next free number (`H1` for `MountingHole`, `TP1` for `TestPoint`, `FID1`, `R`, `C`, `L`, `D`, `J`, `SW`, `Y`, `F`, `BZ`, `BT`, `K`, `JP`, `T`, `U` for `Package_*`, `REF1` otherwise); the value is the footprint's name, as in KiCad | `FOOTPRINT` fields, `REFDES` conventions (`crates/ops/src/library_place.rs::reference_prefix_for`) |
| The unplaced parts of the schematic | **kept**: the first group of the tree, "-- Unplaced --"; choosing one arms it as before | the old Place Footprints list |
| Search over the name, `Lib:Name`, the library, the tags and the description | **done** (`FOOTPRINT_INFO::GetSearchTerms`: library 4, name 8, `Lib:Name` 16, each tag 4, the tags 1, the description 1) on the server (`GET /api/library/search?kind=footprint`) | `FOOTPRINT_INFO::GetSearchTerms`, `EDA_COMBINED_MATCHER` |
| Opened for a symbol: "Filter by pin count (N)" and "Apply footprint filters (...)" | **done**: the first shows the footprints with exactly N numbered pads (digits, or up to two letters and digits, on copper, not an NPTH: `FOOTPRINT::GetNumberedPadCount`), the second the ones a `ki_fp_filters` pattern matches (the name, or `Lib:Name` for a pattern with a colon); both start off and are remembered, as in KiCad. Applied on the server for the libraries and in the browser for the recently used | `FOOTPRINT_CHOOSER_FRAME::filterFootprint` / `filterByPinCount` / `filterByFPFilters`, `KiwayMailIn( MAIL_SYMBOL_NETLIST )` |
| Used by the symbol's Footprint field and Assign Footprints | **done**: Browse... in Symbol Properties and "From KiCad's libraries..." in Assign Footprints open it for the symbol's pin count (all units) and filters and set the field(s) | `GRID_CELL_FPID_EDITOR`, `DIALOG_FIELD_PROPERTIES` |
| The drawing, the description and the details | **done**: the footprint is drawn with the Footprint Editor's painter (`${REFERENCE}` as `REF**`), the pane shows the name, the description, Keywords, the Documentation link found in the description and the pad count | `FOOTPRINT_PREVIEW_PANEL`, `GenerateFootprintInfo`, `GetFootprintDocumentationURL` |
| The footprint of a placed symbol resolves from the footprint library, so the PCB can place it | **done**: `board::load` resolves the footprints the schematic-only parts name (`fold_unknown_symbols`) like an intent part's | `FOOTPRINT_LIBRARY_ADAPTER::LoadFootprint` |
| 3D preview, "Show 3D viewer in own window", the show/hide description button, the Footprint selector's list | **not ported** | `FOOTPRINT_CHOOSER_FRAME::build3DCanvas`, `toggleBottomSplit` |
| The tool stays active and asks for the next footprint after each click | **not done**: the armed footprint is placed once | `PlaceFootprint`'s loop |

Tests: `kicad-port/libChooser.test.ts` (the filters, the tree), `library_search.rs` (search, filters and pad counts on the installed libraries), `footprint_scan.rs`, `library_place.rs` in `eda-ops` and `eda-cli`, `board.rs::placing_an_installed_footprint_makes_a_board_part_that_undo_removes`, and, in the slow tier, `a_placed_installed_footprint_passes_kicad_clis_drc` (a mounting hole in a 20 mm board adds no kicad-cli DRC finding).
Verified in the browser (port 8806, a scratch board made from `examples/mcu_board_30plus.yaml`): `A` shows "-- Unplaced --" (30 parts) above the 155 libraries with their counts and descriptions; "MountingHole 3.2mm M3" finds 17 footprints with `MountingHole_3.2mm_M3` first, drawn with its courtyard and described; OK, a click on the board makes `H1` (31 footprints), Undo and Redo take it away and bring it back; from a symbol's Footprint chooser with both filters on, "soic 3.9x4.9" leaves exactly `SOIC-8_3.9x4.9mm_P1.27mm`; Esc cancels an armed footprint.

## 12. Cleanup Tracks & Vias

Port of `pcbnew/tracks_cleaner.cpp` (`TRACKS_CLEANER`) and its dialog
(`dialogs/dialog_cleanup_tracks_and_vias{,_base}.cpp`),
`pcbnew.GlobalEdit.cleanupTracksAndVias`.

| Behavior | Status | KiCad file:function |
|---|---|---|
| Delete redundant vias: same position+layer span, or a via sharing a position with a through-hole pad spanning the whole copper stack | identical in effect | `TRACKS_CLEANER::cleanup`'s `aDeleteDuplicateVias` branch |
| Delete zero-length tracks | identical for a straight 2-point track (see below for the granularity note) | `cleanup`'s `aDeleteNullSegments` branch (`PCB_TRACK::IsNull`) |
| Delete exact-duplicate tracks (same endpoints either direction, width, layer) | identical, and -- matching source -- runs regardless of every other checkbox | `cleanup`'s `aDeleteDuplicateSegments` branch |
| Delete tracks connecting different nets (short circuit) | identical in effect, via the real connectivity graph (`eda_connectivity::build_graph`) | `removeShortingTrackSegments` |
| Delete tracks fully inside pads | both endpoints inside the pad's shape (`PlacedPad::signed_distance`) rather than source's exact polygon boolean-subtract -- equivalent for every pad shape this model has | `deleteTracksInPads` |
| Delete tracks unconnected at one end / vias connected on only one layer | identical -- reuses the existing `dangling_tracks_and_vias` port (`connectivity/src/dangling.rs`, the same function DRC's `track_dangling`/`via_dangling` already use), iterated to a fixed point same as source's own `do`/`while` loop | `deleteDanglingTracks`, `TestTrackEndpointDangling` |
| Merge co-linear tracks | ported for the common case: two straight 2-point tracks sharing an endpoint, collinear, with nothing else touching the shared joint | `cleanup`'s merge pass, `testMergeCollinearSegments`/`mergeCollinearSegments` |
| Call order (redundant/null/duplicate, then merge if asked, then shorting, then in-pad, then dangling, then a final merge pass if dangling actually deleted anything) | identical | `TRACKS_CLEANER::CleanupBoard` |
| Dialog: six checkboxes, all unchecked by default (no `SetValue(true)` in the read snapshot), "Build Changes" (dry run, lists what would change) then "Update PCB" (commits), any checkbox edit resets back to "Build Changes" | identical | `DIALOG_CLEANUP_TRACKS_AND_VIAS` -- `CleanupTracksDialog.tsx` |
| Net/net-class/layer/"selected items only" filters; "Refill zones before and after cleanup" | not ported -- this app's cleanup always scans the whole board and never touches zone fills itself (the existing Fill All Zones action covers that separately) | `m_netFilterOpt`/`m_netclassFilterOpt`/`m_layerFilterOpt`/`m_selectedItemsFilter`/`m_cbRefillZones` |
| Cleanup Graphics... (a related but separate dialog, invalid-shape/duplicate-graphic cleanup) | missing, unchanged | `pcbnew.GlobalEdit.cleanupGraphics` -- a different dialog/engine, out of scope this item |

**Model-shape adaptation, noted once here rather than per row above:**
KiCad's `PCB_TRACK` is always a single two-point segment; this model's
`Track` is a polyline of 2+ points (`crates/kicad`'s im/exporter already
treats a multi-point `Track` as N-1 consecutive `(segment ...)`s, so this
isn't a new approximation). Zero-length/duplicate detection operates at
whole-`Track` granularity; the merge pass only ever considers straight
2-point tracks as candidates (same scope `tune_api.rs`'s length tuner
already has), so an already-merged multi-point `Track` is left alone
rather than re-walked segment by segment. The merge pass's node check
also doesn't model source's one narrow exception for a true 3-way
junction where two of three meeting tracks happen to be collinear --
this port simply declines to merge there (a missed merge, never a wrong
one). See `crates/connectivity/src/cleanup.rs`'s own header comment for
the full list.

Rust: `crates/connectivity/src/cleanup.rs` (`compute_cleanup`, 13 unit
tests) -- pure logic, no new `Cmd`: both apply and preview reuse the
existing `Cmd::CommitRoute` (remove track/via ids, add the merged
tracks) as one atomic undo step, same pattern `tune_api.rs` already
established. `crates/cli/src/cleanup_api.rs`'s stateless `POST
/api/cleanup_tracks/{preview,apply}` (same shape as `/api/tune_length`).
UI: `CleanupTracksDialog.tsx`, wired to the existing
`pcbnew.GlobalEdit.cleanupTracksAndVias` action (already present in
`menus.json`'s Edit menu from the original extraction, just unregistered
until now).

## 13. Global edits: Track/Via and Text/Graphics properties

Port of `pcbnew/dialogs/dialog_global_edit_tracks_and_vias{,_base}.cpp`
(`pcbnew.GlobalEdit.editTracksAndVias`) and `dialog_global_edit_text_
and_graphics{,_base}.cpp` (`pcbnew.GlobalEdit.editTextAndGraphics`).

| Behavior | Status | KiCad file:function |
|---|---|---|
| Edit Track & Via Properties: scope (Tracks/Vias checkboxes, both unchecked by default, matching the base dialog's own ctor) | identical | `DIALOG_GLOBAL_EDIT_TRACKS_AND_VIAS_BASE`'s ctor |
| "Set to specified values" (width/via diameter+drill/layer) vs "Set to net class / custom rule values" | identical in effect for width/via-size -- resolves each item's *own* net's class (`BoardRules::width_of`/`via_diameter_of`/`via_drill_of`, falling back to the board default), same per-item resolution `SetTrackSegmentWidth` uses | `processItem`'s `m_setToSpecifiedValues` branch |
| Filter by net, "selected items only" | ported | `visitItem`'s `m_netFilterOpt`/`m_selectedItemsFilter` |
| Filter by layer | ported for tracks; does not apply to vias (this model's `Via` has no single `GetLayer()` -- it spans `from_layer`/`to_layer`) | `visitItem`'s `m_layerFilterOpt` |
| Filter by net class, by exact track width/via size; through/micro/blind/buried via type distinction; annular-ring (`UNCONNECTED_LAYER_MODE`) and IPC4761 protection-feature presets | not ported -- no net-class-membership-of-an-item/padstack/via-type concept in this model | `m_netclassFilterOpt`, `m_filterByTrackWidth`/`m_filterByViaSize`, `m_throughVias`/`m_microVias`/`m_blindVias`/`m_buriedVias`, `m_annularRingsCtrl`/`m_protectionFeatures` |
| Edit Text & Graphics Properties: scope (board graphics/board text checkboxes), filter by layer + "selected items only" | ported, restricted to this model's two free-standing board drawing kinds (`Shape`/`Text`) | `DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS_BASE`'s ctor, `visitItem` |
| Set layer / line width (shapes) / text size + thickness (texts), "specified values" only | ported | `processItem`'s `m_setToSpecifiedValues` branch |
| Footprint reference/value/other-field scope, dimension items, tables, barcodes, bold/italic/font/auto-thickness/keep-upright, "center text on footprint", "Set to layer (and dimension) default values" | not ported -- a footprint's reference/value are the read-only intent-derived `Part` (section 10), and this model has no dimension/table/barcode item, no font/style concept on `Text`, and no per-layer-class default-style arrays (`BOARD_DESIGN_SETTINGS::m_LineThickness`/`m_TextSize`/...) to reset to | `processItem`'s `text`/`barcode`/`field`/`parentFP` branches, `onActionButtonChange`'s `else` arm |
| One atomic undo step for the whole bulk edit | identical | `SaveCopyInUndoList`/`BOARD_COMMIT::Push()` once per dialog "Apply" -- `Cmd::EditTracksAndVias`/`Cmd::EditTextAndGraphics` are each a single `Cmd`, single `board::step` call |

Rust: `crates/ops/src/lib.rs`'s `Cmd::EditTracksAndVias` (+ `SizeSpec`/
`ViaSizeSpec`) and `Cmd::EditTextAndGraphics`, 7 new tests in
`crates/ops/src/tests.rs`. No new HTTP endpoint -- both reach the backend
through the existing `POST /api/cmd`, since each is a single `Cmd` rather
than a preview/apply pair. Net/layer/selection filtering is computed
client-side (this crate has no selection/UI-filter concept of its own,
same split section 12's cleanup dialog uses) in
`GlobalEditTracksAndViasDialog.tsx`/`GlobalEditTextAndGraphicsDialog.tsx`.

## 14. Rule areas (keepout zones)

Port of `pcbnew/zone.cpp`'s `ZONE::GetIsRuleArea()`/`GetDoNotAllow*()`
flags (task item 3); the keepout DRC (`drc_test_provider_disallow.cpp`) is
kicad-cli's. A rule area shares `Zone`'s own IR struct, outline-drawing
tool and properties dialog with a copper-pour zone -- same single-dialog-
two-panels shape source itself uses -- rather than being a separate type.

| Behavior | Status | KiCad file:function |
|---|---|---|
| IR: `is_rule_area` + 5 `keepout_*` flags (tracks/vias/pads/copper pours/footprints) on `Zone`, additive | identical | `eda_model::ir::Zone` -- `ZONE::GetIsRuleArea`/`GetDoNotAllow{Tracks,Vias,Pads,ZoneFills,Footprints}` |
| A zone/rule area may have no net at all (KiCad's net code 0) | identical | `add_zone`/`edit_zone` no longer require `known_net` for an empty string -- previously impossible to create, needed for every real-world keepout |
| Drawing: outline-drawn with the same tool as a copper-pour zone | ported, one hotkey/menu difference from source rather than a separate tool -- `pcbnew.InteractiveDrawing.ruleArea` (Ctrl+Shift+K) arms the identical outline tool as `.zone`; `state.nextZoneIsRuleArea` is the one bit telling `ZoneDialog.tsx` to pre-check "Rule area" for *this* entry's outline, same end result (draw outline, dialog opens, Rule Area already ticked) with no second drawing-tool code path to maintain | `tools/drawing_tool.cpp`'s zone/keepout entry points, which upstream also funnel through one outline-drawing loop |
| Properties dialog: "Rule area" checkbox swaps the panel between fill settings and the 5 keepout checkboxes | identical in effect | `dialog_copper_zones.cpp`'s `IsRuleArea()` branch -- `ZoneDialog.tsx` |
| Zone filler honors a copper-pour keepout: every other zone's fill excludes it, any net, any priority | identical in effect | `ZONE_FILLER::fillCopperZone`'s keepout knockout -- `crates/zone-filler`'s new `FillInput::keepouts`/`FillKeepout`, 2 new tests. Previously an explicit, named gap in that crate's own doc comment ("Not ported: ... keepout zones") |
| DRC: track/via/pad/footprint landing inside a matching keepout is reported (`items_not_allowed`), and a copper pour inside one | KiCad's own: the rule area is exported as a `(zone (keepout ...) (placement ...))` and kicad-cli's DRC reports `items_not_allowed` (the Rust `disallow` provider is deleted, so there is nothing to keep in parity). **Status (2026-10-07): true now, and tested** -- until then it was not true of any board with a rule area: the writer asked its net check about the area's empty net and the whole export failed with `kicad.unknown_net`, so there was no file for kicad-cli to read (`kicad_cli_drc_rule_area_reports_items_not_allowed_and_exports_gerbers`, `crates/kicad/tests/kicad_cli_drc.rs`) | `drc_test_provider_disallow.cpp` (kicad-cli's) |
| Multi-layer rule areas (one outline, several layers); custom `(disallow ...)` DRC rules on non-keepout items; `DRCE_TEXT_ON_EDGECUTS` (a different, unrelated half of the same KiCad source file) | not ported -- this model's `Zone` is single-layer only (an existing, documented limitation predating this item), has no custom-rule language (GAPS.md #10), and text-on-Edge.Cuts is a separate check | `ZONE::GetLayerSet()`, `panel_setup_rules.cpp`, `drc_test_provider_disallow.cpp`'s `checkTextOnEdgeCuts` |
| `.kicad_pcb` export of a rule area | **ported (2026-10-07, WP5 step 1)**: a rule area is written as KiCad writes it -- no `(net ..)`, `(keepout (tracks ..) (vias ..) (pads ..) (copperpour ..) (footprints ..))`, `(placement (enabled no))`, no fill -- and the importer reads those tokens back. Before this it was written as a pour, and having no net it failed the export of the whole board | `pcb_io_kicad_sexpr.cpp` `format( const ZONE* )` -- `crates/kicad/src/pcb_items.rs::write_zone`, tests `a_rule_area_exports_as_a_keepout_zone_without_a_net` and `every_zone_is_written_with_its_own_settings_and_reads_back_the_same` |
| Canvas rendering: a rule area draws as a dashed outline with a diagonal hatch and a restriction label ("Keepout: Tracks/Vias"), never a solid fill (it never has one) | ported, a simplified stand-in for source's real cross-hatch keepout rendering | `pcb_painter.cpp`'s zone paint, keepout branch -- `painter.ts`'s new `drawRuleArea` |

Rust: `crates/model/src/ir.rs` (`Zone`'s 6 new fields), `crates/ops/src/lib.rs`
(`Cmd::EditZone` carries them; `add_zone`/`edit_zone` allow an empty net),
2 new `crates/ops/src/tests.rs` tests. `crates/zone-filler/src/lib.rs`
(`FillKeepout`, the knockout pass), 2 new tests. `crates/drc`: `DrcKeepout`
(`board.rs`, kept entirely separate from `DrcZone`/`board.zones` so the
filler and router keep treating `board.zones` as "real copper only",
unaffected by rule areas coming into existence as a concept); the DRC
provider that read it (`disallow.rs`) is gone, kicad-cli reports
`items_not_allowed` itself. Frontend: `RuleAreaFields`/`Zone`/`CmdZone` in
`api/types.ts`, `ZoneDialog.tsx`'s rule-area panel, `clipboard.ts`'s
`zoneToCmd` (copy/paste/duplicate fidelity), `painter.ts`'s `drawRuleArea`,
`useActionRunner.ts`'s `ruleArea` action, `state.nextZoneIsRuleArea`
(`store.tsx`).

## 15. Teardrops

Port of `pcbnew/teardrop/*` (task item 4): the teardrop polygon
generator, Board Setup > Teardrops' settings, and "Add All Teardrops" /
"Remove All Teardrops". A teardrop is a `Zone` with `teardrop: true`
(the task brief's own instruction -- "KiCad stores teardrops as special
zones, so store them the same way in the IR, regenerated on demand"),
computed fresh from the board's current tracks/vias/pads and replaced
wholesale on every "Add All Teardrops", never hand-edited.

| Behavior | Status | KiCad file:function |
|---|---|---|
| IR: `Zone::teardrop` marker; `RoutingSection::teardrop_settings` (`TeardropSettings`), additive | identical | `eda_model::ir::{Zone, TeardropSettings}` |
| Generated pentagon shape: 2 points on the track, 2 on the anchor's own circle, 1 behind the anchor's centre | ported via a closed-form exact-tangent-on-circle construction instead of source's general convex-hull-over-clipped-polygon algorithm -- see `eda_connectivity::teardrop`'s own doc for why (round anchors only, as a result) | `teardrop_utils.cpp`'s `computeAnchorPoints`/`findAnchorPointsOnTrack`/`computeTeardropPolygon` |
| Settings: best length/width ratio, max length/width, width-to-size filter ratio, per-target-kind (vias/PTH pads/SMD pads) enable | ported, collapsed from upstream's 3 separate `TEARDROP_PARAMETERS` (round/rect/track) into one shared block (this port never builds a rect-anchor or track-to-track teardrop at all, so there is nothing for the other two to tune separately) | `TEARDROP_PARAMETERS`/`TEARDROP_PARAMETERS_LIST` -- `BoardSetupDialog.tsx`'s new Teardrops page |
| "Add All Teardrops" / "Remove All Teardrops" | ported as whole-board regenerate/clear (`Cmd::AddAllTeardrops`/`RemoveAllTeardrops`); idempotent (a second "Add All" replaces, never duplicates, the generated set, since a teardrop's content-derived id is stable across an identical regeneration) | `TEARDROP_MANAGER::UpdateTeardrops`/`RemoveTeardrops`'s whole-board entry points (`GLOBAL_EDIT_TOOL`'s "Add/Remove Teardrops" actions) |
| Rendering: a teardrop always draws as solid copper, independent of `zoneDisplayMode` (it has no separate "fill" to toggle -- the outline already is the final shape) | ported | `pcb_painter.cpp`'s zone paint -- `painter.ts`'s new teardrop branch in `drawZones` |
| Curved (Bezier) edges (`m_CurvedEdges`) | not ported -- always the straight-edge shape, which is also upstream's own factory default | `computeCurvedForRoundShape`/`computeCurvedForRectShape` |
| Rectangular/round-rect/custom pad anchors (`TARGET_RECT`, non-round SMD pads) | not ported -- only a via or a *circular* pad (`PadShape::Circle`) is a usable anchor; this port behaves as if `m_UseRoundShapesOnly` were always on | `computeCurvedForRectShape`, `computeAnchorPoints`'s non-round branch |
| Track-to-track teardrops (`TARGET_TRACK`/`TD_TRACKEND`, two different-width tracks joined end to end) | not ported -- `m_TargetTrack2Track` defaults off upstream too | `AddTeardropsOnTracks`, `teardrop_types.h`'s `TD_TRACKEND` |
| Borrowing length from a second track segment when the first is too short (`m_AllowUseTwoTracks`) | not ported -- a track shorter than the requested teardrop length is simply skipped | `findAnchorPointsOnTrack`'s two-segment extension |
| Excluding a pad already covered by a same-net zone fill (`m_TdOnPadsInZones`) | not ported | `areItemsInSameZone` |
| Incremental updates (only regenerating teardrops near a just-edited item, `RemoveTeardrops`/`UpdateTeardrops`'s `dirtyPadsAndVias`/`dirtyTracks` lists) | not ported -- "Add All Teardrops" always recomputes the whole board from scratch, same "nothing cached, nothing stale" philosophy this app's zone fills already use | `TEARDROP_MANAGER`'s dirty-item tracking |
| `.kicad_pcb` export/import of a teardrop zone | **ported (2026-10-07)**: `(attr (teardrop (type padvia)))` plus the settings KiCad's own teardrop manager gives a teardrop zone (no clearance, 0.0254 mm minimum width, full pad connection, islands kept); read back as `Zone::teardrop`. A `track_end` teardrop imports as a `padvia` one | `teardrop.cpp` `createTeardrop`, `format( const ZONE* )` -- `crates/kicad/src/pcb_items.rs`, test `a_teardrop_zone_is_flagged_and_carries_kicads_own_teardrop_settings` |

Rust: `crates/model/src/ir.rs` (`Zone::teardrop`, `TeardropSettings`, on
`RoutingSection` -- a genuinely large mechanical ripple fixing every
`RoutingSection` literal across the workspace, since that struct has no
`Default` impl; zero behavior change to any of them). New
`crates/connectivity/src/teardrop.rs` (`generate_teardrops`, 7 unit
tests, including a shoelace point-in-polygon check that the anchor's own
centre really lands inside the generated pentagon). `crates/ops/src/lib.rs`:
`Cmd::SetTeardropSettings`/`AddAllTeardrops`/`RemoveAllTeardrops`
(new `eda-connectivity` dependency), 4 new tests. Frontend:
`TeardropSettings` in `api/types.ts`, `BoardSetupDialog.tsx`'s new
Teardrops page (settings + both buttons), `painter.ts`'s teardrop
render branch, `pcbnew.GlobalEdit.editTeardrops` wired to open Board
Setup landed on that page (`state.boardSetupInitialPage`).

## 16. Groups

Port of `common/tool/group_tool.cpp`, `pcbnew/tools/pcb_group_tool.cpp` and the entered-group half of
`pcbnew/tools/pcb_selection_tool.cpp` (task item 5): group/ungroup
(Ctrl+G / Ctrl+Shift+G), selecting a group as a single unit, and
entering/leaving one. A group is its own IR type (`Group`: id, name,
`member_ids`), not a special item flag on something else -- matching
`PCB_GROUP` being a real, separate item type upstream too.

**Update 2026-10-08 (GAPS.md item 17):** groups nest (`EDA_GROUP`'s member may be another group); move, turn, flip, delete, Duplicate, Copy and Paste take the
whole tree; the entered group is drawn as an overlay and left by a click outside it; the `.kicad_pcb` file and the clipboard text hold the tree. The tree is
read through `DrawingsSection::group_leaves` and friends (`crates/model/src/groups.rs`) and `kicad-port/groupTree.ts`; the verbs are in
`crates/ops/src/pcb_groups.rs`. The rows say what is done.

| Behavior | Status | KiCad file:function |
|---|---|---|
| IR: `Group { id, name, member_ids }`, additive, stored on `DrawingsSection` | identical in shape; stored on `DrawingsSection` purely for the lowest construction-site ripple (see `Group`'s own doc) -- a member id can name a part, track, via, zone, shape, text, dimension or **another group** | `pcbnew/pcb_group.h`'s `PCB_GROUP`, `EDA_GROUP` |
| Ctrl+G: group the current selection (2+ items) | identical in effect; the new group is selected as it is afterwards (not as the group above it) | `group_tool.cpp`'s `Group()`, `ACTIONS::group.Enable(selectionCount >= 2)`, `RunAction( ACTIONS::selectItem, group )` |
| Ctrl+Shift+G: ungroup, releasing members in place | identical in effect; the members are free of the group above it too | `GROUP_TOOL::Ungroup()` (`group->RemoveAll()`) |
| Nested groups | **done (2026-10-08)**: grouping a selection that holds groups makes them members of the new group (it used to flatten them); a group can hold neither itself nor a group that holds it (`ops_bad_group`); a group made from items that all sit in one group takes their place in it (what a group made while that group is entered does); a group left with fewer than two members dissolves and leaves the group above, which may dissolve in turn | `PCB_GROUP_TOOL::Group`, `EDA_GROUP::AddItem`, `GROUP_TOOL::RemoveFromGroup` -- `pcb_groups.rs` (`group_items`, `prune_groups`), tests in `pcb_groups_tests.rs` |
| An item belongs to at most one group; joining a new group (or `AddToGroup`) silently leaves whatever group it was already in | identical in effect | `EDA_GROUP::AddItem`, `GROUP_TOOL::AddToGroup` / `RemoveFromGroup` |
| What is taken off the board leaves its group | **done**: a deleted member is dropped from its group and a group left with fewer than two dissolves, without each delete verb knowing: `Board::apply` compares the ids before and after the command | `BOARD_COMMIT::Push` (`parentGroup->RemoveItem( boardItem )` for a removed item), `EDIT_TOOL::DeleteItems` -- `Board::group_universe`, `tidy_groups` |
| Group Items, Ungroup, Add Items, Remove Items, Group Properties | **reachable (2026-10-08)**: the Grouping submenu of the right-click menu (section 7) lists the four actions, enabled as `GROUP_CONTEXT_MENU::update` does; Group Properties is the dialog of `PARITY-common.md` section 4 and takes groups as members | `GROUP_CONTEXT_MENU`, `ACTIONS::addToGroup` / `removeFromGroup` |
| Selecting any member selects the outermost group above it; with a group entered, the group directly inside it | **done**: `withGroupSubstitution` (`state/store.tsx`) over `kicad-port/groupTree.ts` `substituteSelection`; selecting something outside the entered group leaves it | `PCB_SELECTION_TOOL::FilterCollectorForHierarchy`, `PCB_GROUP::TopLevelGroup( scope )`, `WithinScope`, `select` (`ExitGroup()`) |
| Entering a group: double-click a single selected group, or Enter Group in the right-click menu | **done**: the group's members are selected, each as it is (a group inside it stays one item), and only they can be clicked or boxed while it is entered; entering another group from inside leaves the first | `PCB_SELECTION_TOOL::EnterGroup`, `Main`'s double-click handler (`m_selection.GetSize() == 1 && Type() == PCB_GROUP_T`), `FilterCollectorForHierarchy` ("If a group is entered, disallow selections of objects outside the group") |
| Leaving a group | **done**: Escape (the first one clears the selection, the next leaves the group and **selects nothing** -- `ExitGroup( bool aSelectGroup = false )`; it used to re-select the group, wrongly), a click outside the entered group's bounding box, selecting an item outside it, and Leave Group in the menu (that one selects the group, `ExitGroup( true )`); a group dissolved or emptied while entered is left | `PCB_SELECTION_TOOL::ExitGroup`, `selectPoint` (`if( m_enteredGroup && !m_enteredGroup->GetBoundingBox().Contains( aWhere ) ) ExitGroup();`), `EDIT_TOOL::DeleteItems` ("If the entered group has been emptied then leave it") |
| Move, drag, rotate, flip of a group | **done** for the whole tree: a group moves, turns and flips every leaf below it as one `move_items` / `rotate_items` / `flip_items` (one undo step, about the selection's centre); a locked leaf makes its group locked for the tools | `PCB_GROUP::Move` / `Rotate` / `Flip` (each member's own, so a member group's in turn), `FilterCollectorForLockedItems` -- `crates/ops/src/pcb_transform.rs` (`resolve_targets`), `kicad-port/pcbTransform.ts`, `pcbCarry.ts`, `itemBoxes.ts` |
| Delete of a group | **done**: Delete (and Cut) takes the group with everything in it, nested groups included, as one undo step | `EDIT_TOOL::DeleteItems` (`case PCB_GROUP_T`: `RunOnChildren( commit.Remove, RECURSE )`, then the group itself) -- `kicad-port/deleteCmds.ts` (`boardDeleteCmds`) |
| Duplicate, Copy, Paste of a group | **done**: copies the groups with their members, inner groups first; the copy of a member joins the group copied; the pasted tree is nested again. The clipboard text is KiCad's: a `(group ...)` block per group naming its members by uuid, an inner group listed before the one that holds it | `EDIT_TOOL::Duplicate` (`DeepDuplicate`), `CLIPBOARD_IO::SaveSelection` / `Parse` -- `crates/ops/src/pcb_paste.rs` (`copy_group`), `crates/kicad/src/clipboard.rs` |
| Visual: a selected group | **done**: its enclosing box (dashed here, KiCad's solid) with the group's name on an italic tab above it when it fits; a group inside a selected group draws none | `PCB_PAINTER::draw( const PCB_GROUP* )` (`LAYER_ANCHOR`: "Selected on our own; draw enclosing box") -- `painter.ts` `drawSelectedGroups` |
| Visual: the entered group | **done**: its solid enclosing box and name in the anchor colour, and everything outside the group dimmed to 30 % (`OUTSIDE_ENTERED_GROUP_ALPHA`) so it is plain that nothing there can be picked. The dim is this port's own cue: KiCad hides the group in the main view and draws it in `m_enteredGroupOverlay`, in front of the rest, and does not dim | `PCB_PAINTER::draw( const PCB_GROUP* )` (`aGroup->IsEntered()`: "Entered group; draw enclosing box"), `PCB_SELECTION_TOOL::EnterGroup` (`view()->Hide( m_enteredGroup, true )`, `m_enteredGroupOverlay.Add`) |
| `.kicad_pcb` export/import of a `(group ...)` block | **ported (2026-10-07), nested 2026-10-08**: `(group "name" (uuid ..) [(locked yes)] (members ..))`, written last, a group inside a group named by its own uuid; a polyline track is several segments, all members. The importer resolves the uuids to ids after the items, keeps a group inside a group, and drops a group with fewer than two members on the board | `format( const PCB_GROUP* )`, `parseGROUP` -- `crates/kicad/src/pcb_items.rs::write_groups`, `import_items.rs`, tests `groups_are_written_with_their_members_uuids_and_read_back_with_their_ids` and `a_group_inside_a_group_is_written_by_its_uuid_and_read_back_nested` |
| Items drawn or pasted while a group is entered join it | not done: they are made outside the group (`BOARD_COMMIT::Push` adds them to the entered group); the Group verb does take its place in the entered group (above) | `BOARD_COMMIT::Push` (`m_enteredGroup`) |
| Create Array of a group | not done: a group id in the array's ids is skipped the way an unknown id is (section 17) | `ARRAY_TOOL::CreateArray` |
| Groups on the schematic and in the footprint editor | not done: the schematic has none (`sch_group_tool.cpp`; Group / Ungroup are registered for the board only), nor has the footprint editor (`GAPS.md` item 17) | `SCH_GROUP_TOOL`, `FOOTPRINT_EDITOR` |

Tests: Rust `crates/model/src/groups.rs` (4: leaves open nested groups in member order, a loop made by hand ends the walk, parents and the top group, expanding), `crates/ops/src/pcb_groups_tests.rs` (16: a group of groups moves, turns and flips everything below it; nesting, ungroup and remove dissolving up the tree; a group cannot hold itself or a group that holds it; deleting a member or a whole subtree leaves no group behind and a member the board never had stays; Duplicate and Paste copy the tree), `crates/ops/src/tests.rs` (2 rewritten for nesting), `crates/kicad/src/pcb.rs` and `clipboard.rs` (one nested round trip each); web `kicad-port/groupTree.test.ts` (8: parents, leaves, `TopLevelGroup`, `WithinScope`, substitution and leaving), plus nested cases in `pcbTransform.test.ts`, `pcbCarry.test.ts`, `deleteCmds.test.ts` and `groupEdit.test.ts`. Seen in the browser (port 8808, a scratch copy of `mcu30`, driven through `window.__eda`): a group holding four groups and 22 footprints turned 90 degrees as one (all 30 footprints about one centre, one Undo restored them), Flip swapped the sides of every leaf and one Undo gave them back, Delete took the 30 footprints and all five groups and one Undo brought both back; Group Items, Enter Group and Leave Group ran from the right-click menu, and Escape inside an entered group cleared the selection and then left the group without selecting it.

## 17. Create Array

Port of `pcbnew/tools/array_tool.cpp` (`ARRAY_TOOL::CreateArray`, `pcbnew.Array.createArray`, Ctrl+T), `common/array_options.cpp` (`ARRAY_GRID_OPTIONS`,
`ARRAY_CIRCULAR_OPTIONS`) and `pcbnew/dialogs/dialog_create_array{,_base}.cpp` at `8303b2ad`. `ARRAY_OPTIONS` is a dialog-session object in source too -- never written to the
board file -- so it ships as a `Cmd` payload (`ArrayGeometry`), and what an array makes is ordinary board data (a copy of a footprint is a part of the board,
`DrawingsSection::board_parts`, as Duplicate and Paste make them: section 5). **Rewritten 2026-10-09 (GAPS.md item 22)**: the first version (task item 6) could not copy
footprints, turned only footprints and text in a circle, and moved the original to the last point; the backend is now `crates/ops/src/array.rs` and the dialog is KiCad's.

| Behavior | Status | KiCad file:function |
|---|---|---|
| Grid geometry: horizontal and vertical count and spacing, an offset per row and per column (a skewed grid), "Source items remain in place" or "Centre on source items", stagger every Nth row or column (a negative one the other way; `abs(stagger) > 1` only, KiCad's truncating integer math), numbering direction and serpentine (`reverse_alternate`) | **identical** (the last two are the backend's; the board dialog never shows them, they sit in the pad numbering panel, and so keep KiCad's defaults: across then down, no serpentine) | `ARRAY_GRID_OPTIONS::GetTransform`, `getGridCoords` |
| Circular geometry: centre, count, angle between items, first item angle, clockwise or anti-clockwise, "Rotate items"; "Full circle" makes the angle 360 / count; an angle of 0 on the wire divides the circle evenly | **identical** (the angle convention is the app's: clockwise on the screen) | `ARRAY_CIRCULAR_OPTIONS::GetTransform`, `calculateCircularArrayProperties` |
| "Duplicate selection": a copy of the whole selection for every point but one -- footprints, tracks, vias, zones, graphics, text, dimensions and groups (with copies of their members) -- each moved to its point and, in a circle with "Rotate items", turned about its own position, whatever its kind; all of it one undo step | **identical** in the points taken and what is on them; **one deviation**: KiCad's reverse loop leaves the *original* on the last point and a copy on the first, here the original keeps the first, so a footprint keeps its reference and what is routed to it | `ARRAY_TOOL::CreateArray` (`TransformItem`, the reverse `ptN` loop) |
| "Arrange selection": nothing is made; the selected items, in the order they were given, go onto the points one after the other (every one first to the first item's position, then offset by the point's transform); an id that names nothing does not use up a point | **identical** | `ARRAY_TOOL::CreateArray`'s `ShouldArrangeSelection()` branch |
| A footprint copy: a new part of the board on the same nets, with the original's side, rotation, fields, attributes and pad edits; the next free reference | **done**: "Assign unique reference designators" (the default) is `ReannotateDuplicates` -- the next free number of the reference's prefix, in the order of the points; "Keep existing reference designators" leaves the *text* of every copy as the original's (the copy keeps its own id, the Reference field shows the original's text) | `BOARD_REANNOTATE_TOOL::ReannotateDuplicates`, `m_radioBtnKeepRefs` / `m_radioBtnUniqueRefs`; `crates/ops/src/pcb_paste.rs`, `array.rs` |
| A pad or a field in the selection stands for its footprint, once; a selection of a footprint and copper is arrayed together | **identical** | `GetParentFootprint`, `fpDeDupe` |
| Routing: an array of copper leaves the routing be; one of a footprint with copper routed to its pads clears the routing like every footprint edit (section 22) | as section 22 | `clears_routing_in` |
| The dialog: tabs "Grid Array" and "Circular Array", the boxes "Grid Array Size", "Items Spacing", "Stagger Settings" (Rows / Columns), "Grid Position", "Center position" (with "Select Point..." and "Select Item...", answered on the board by the picker), "Duplication Settings" (Full circle, Direction, Angle between items, Item count, First item angle, Rotate items), "Item Source" (Duplicate / Arrange selection) and "Footprint Annotation" (hidden for Arrange), in KiCad's words; the entries are text read when OK is pressed, lengths with a unit suffix; Enter is OK, Escape is Cancel | **done** | `dialog_create_array_base.cpp`, `DIALOG_CREATE_ARRAY` -- `components/CreateArrayDialog.tsx` |
| The checks of OK: "Bad numeric value for horizontal count: x" (also vertical count, stagger, point count), "horizontal delta of zero with 5 objects", "vertical delta of zero ...", "angular delta of zero ...", every complaint on a line of its own, nothing sent while there is one | **identical**, plus one of ours: a grid of less than one row or column, or a circle of no point, makes nothing and says so (the entries are text) | `DIALOG_CREATE_ARRAY::TransferDataFromWindow`, `validateLongEntry` -- `kicad-port/arrayOptions.ts:optionsFromEntries`, `checkArrayOptions`; `ArrayGeometry::validate` |
| The values are kept for the next time | **done**: the last values that went through (the angle as its entry held it, so "Full circle" keeps 360 / count), module-wide, as `s_arrayOptions`. Both position radios are restored `true` by the persister and the second stays selected, so a fresh dialog starts on "Centre on source items" | `CREATE_ARRAY_DIALOG_ENTRIES`, `WIDGET_SAVE_RESTORE` |
| The circle's centre starts at the item (a lone item) or the middle of the selection | **deviation**: `ARRAY_TOOL::CreateArray` computes that `origin` and hands it to the dialog, which stores it and never uses it (the centre starts at its last value, 0, 0 the first time); here it is the default | `ARRAY_TOOL::CreateArray`, `m_originalItemPosition` |
| A picture of where the items go, drawn from the entries as they are typed | **ours** (KiCad has none) | `previewPoints` |
| Footprint-editor pad numbering: "Renumber pads", the `ARRAY_AXIS` numbering schemes (numerals, hexadecimal, alphabet minus IOSQXZ, full alphabet), "Initial Pad Number", 2D coordinate numbering, `ARRAY_PAD_NUMBER_PROVIDER`'s skipping of used numbers | **not ported**: the library editor has no array tool and no multi-pad selection to give it, and the board editor never shows these controls (`enableArrayNumbering = m_isFootprintEditor`) | `array_axis.cpp`, `array_pad_number_provider.cpp`, `DIALOG_CREATE_ARRAY`'s numbering panels |

Rust: `crates/ops/src/array.rs` (`ArrayGeometry` -- a `Cmd` payload, not IR -- and `Cmd::CreateArray { ids, geometry, arrange, reannotate }`) and `array_tests.rs` (23 tests: the grid's order,
serpentine, skew and stagger; the circle both ways, its first-item angle and the even division; the refusals; vias, tracks, zones and shapes at every point; every kind turned in a circle; a
footprint copied with the next free references, its rotation, side and edits, the references kept; a pad or a field standing for its footprint; a group copied with its members; arranging, on a
grid and on a circle; the routing); `crates/cli/src/board.rs` `an_array_of_a_footprint_is_one_undo_step_and_its_copies_are_parts_of_the_board`. Web: `kicad-port/arrayOptions.ts` (the defaults, the
entries and how OK reads them, the geometry and command, the origin, the preview's points: 17 `node --test` cases), `components/CreateArrayDialog.tsx`, `pcbnew.Array.createArray` in
`useActionRunner.ts` (gated on a non-empty selection, as `if( selection.Empty() ) return 0;`) and "Create Array... (Ctrl+T)" in the context menu. Checked in the browser by
`e2e/footprint-edit.check.js` (section 26): a 3 x 2 grid of a footprint in place with unique references and one undo, four footprints on a circle, Arrange selection, the remembered values, the
messages of bad entries and Escape.

## 18. Dimensions

Port of `pcbnew/pcb_dimension.{h,cpp}`'s five dimension types (task item
7): Aligned, Orthogonal, Radial, Leader, Center. Collapsed into one
`Dimension` struct with a `DimensionKind` tag instead of five item
types, matching this model's existing `Shape`/`ArrayGeometry` enums'
own shape. Geometry (crossbar/extension lines/arrows/leader/centre-
cross, text position+angle, the formatted display string) is computed
fresh from `start`/`end` plus the style/format fields on every read
(`eda_connectivity::dimension::compute_dimension_geometry`), the same
"recompute, never cache" relationship source's own `Update()` has to
its stored geometry -- a renderer draws exactly what that function
returns and never re-derives any of it.

| Behavior | Status | KiCad file:function |
|---|---|---|
| Aligned: crossbar parallel to the two feature points, offset by a signed `height`; extension lines from each feature point past the crossbar by `extension_height`; inward/outward arrows; text outside (offset perpendicular) or inline (splitting the crossbar); `keep_text_aligned` rotates text to the crossbar's own angle, flipped upright when it would otherwise read upside down | identical in effect | `PCB_DIM_ALIGNED::updateGeometry`/`updateText` |
| Orthogonal: crossbar locked horizontal or vertical; only that axis of the two feature points is measured; a second, independent extension line compensates for the second feature point not lying on the (axis-locked) crossbar | identical in effect | `PCB_DIM_ORTHOGONAL::updateGeometry`/`updateText` |
| Radial: a small fixed-size (`arrow_length`) `+` mark at the centre; measures the centre-to-point distance (the radius); a leader runs outward from the point by `leader_length` to a knee, then on to the text | identical in effect | `PCB_DIM_RADIAL::updateGeometry`/`GetKnee` |
| Leader: a line from the arrow tip to a knee, then on to the text, clipped where it reaches the text (see the knockout row below) | ported; no `R=`/diameter-symbol auto-prefix convention (plain `prefix`/`suffix` strings cover it manually) | `PCB_DIM_LEADER::updateGeometry` |
| Centre: a `+` mark at `start`, sized and oriented by `end - start` (one arm along that vector, the other rotated 90°); never shows text in practice | identical in effect | `PCB_DIM_CENTER::updateGeometry` |
| Value text: measured distance, prefix/suffix, a flat 0-5 decimal-place precision, trailing-zero suppression, units (mm/mil/inch/automatic) with no/bare/parenthesized suffix, or a manual override string | ported, with `DIM_PRECISION`'s four unit-dependent "V_VVV" levels (fewer decimals for mm than inch at the "same" nominal precision) collapsed to the flat count -- one precision concept instead of two | `PCB_DIMENSION_BASE::GetValueText`/`updateText` |
| Crossbar/leader-line "knockout": a gap cut around text sitting on the line (inline text; a leader's text line stops at the text's edge either way) | ported, approximated -- this model has no real font-metrics engine anywhere, so the gap is sized from the same `0.6 * font_size`-per-character estimate `eda_engine::geometry::CHAR_WIDTH_FACTOR` already uses for every other text-overlap check in this project, not source's own exact rendered glyph bounding box | `CollectKnockedOutSegments` |
| A leader's optional text border (rectangle or circle drawn around the text) | not ported | `PCB_DIM_LEADER::m_textBorder`, `DIM_TEXT_BORDER` |
| `DIM_TEXT_POSITION::MANUAL` (freely dragging the text off its computed position) | not ported -- no point-editor-style manual sub-element dragging, same restriction this model's zone outlines already have | `DIM_TEXT_POSITION` |
| Board Setup > Dimension Properties: units/format/precision/suppress-zeroes/text-position/keep-aligned/text-size/line-thickness/arrow-length/extension-offset defaults, applied once at creation, never retroactively | identical in effect | `panel_setup_dimensions.cpp`, `BOARD_DESIGN_SETTINGS::m_Dimension*` |
| Move (drag or Move Exactly's underlying Cmd) translates both feature points | identical | `PCB_DIMENSION_BASE::Move` |
| Rotate/Flip (as a generic `BOARD_ITEM`, e.g. if ever added to a future Create Array/rotate-selection path) | not ported -- no `Cmd::RotateDimension`/flip; `crates/connectivity::dimension::rotate_dimension` exists and is tested but has no caller yet, same "built, not yet wired to a UI action" gap teardrops' settings page didn't have but groups' `AddToGroup`/`RemoveFromGroup` did until the right-click menu listed them (sections 7 and 16) | `PCB_DIMENSION_BASE::Rotate`/`Flip`/`Mirror` |
| Interactive placement: Aligned/Orthogonal's third "set height" click with a live crossbar preview; Radial/Leader/Center's own click sequences | simplified to one plain two-click (start, end) placement for every kind, with `height`/`leader_length`/orientation filled from Board Setup defaults and the just-created dimension's own Properties dialog opening immediately for fine-tuning -- a deliberate scope reduction (no live multi-step canvas preview for this item), not a missing capability: every field the live click sequence would have set is still reachable, just numerically instead of by dragging | `tools/drawing_tool.cpp::DrawDimension` |
| Interactive centre-point/-item picker buttons for Radial's own dialog-free flow | not ported -- plain numeric X/Y fields instead, same convention every other dialog in this app already uses | n/a (this port has no equivalent interactive picker tool) |
| Selecting a dimension (click, box-select, the Selection Filter's own toggle) | ported -- `components/canvas/selectionCandidates.ts` gained a `"dimension"` kind, hit-tested against the closest of its own `lines` segments (a disconnected set, not one polyline) or its text anchor | `pcb_selection_tool.cpp`'s generic item iteration, extended to this new kind |
| `.kicad_pcb` export/import of a dimension | **ported (2026-10-07)**: all five kinds, with their `(format ..)` and `(style ..)` blocks and the label as a `gr_text` (position and text from `compute_dimension_geometry`; KiCad recomputes both on load). The importer reads the new format only (the pre-6.0 `(feature1 ..)` form is skipped); a manual text position imports as `outside`. Text pen thickness is the new `Dimension::text_thickness_um` | `format( const PCB_DIMENSION_BASE* )`, `parseDIMENSION` -- `crates/kicad/src/pcb_items.rs::write_dimension`, `import_items.rs`, test `a_dimension_is_written_with_its_format_and_style_and_reads_back_the_same` |

Rust: `crates/model/src/ir.rs`'s `Dimension`/`DimensionKind`/
`DimensionUnits`/`DimensionUnitsFormat`/`DimensionTextPosition`/
`ArrowDirection`/`DimensionSettings` (on `DrawingsSection`, additive, no
ripple -- that struct already derives `Default` and every existing
literal already spreads it, same low-ripple reasoning `Group`'s own doc
gives). `crates/connectivity/src/dimension.rs` (`compute_dimension_
geometry` plus `translate_dimension`/`rotate_dimension`), 13 unit tests
covering every kind's own geometry, override text, unit-suffix
formatting, and the translate/rotate helpers. `crates/ops/src/lib.rs`:
`Cmd::AddDimension`/`DeleteDimension`/`MoveDimension`/`EditDimension`/
`SetDimensionSettings`, 8 new tests. `crates/cli/src/studio.rs`'s
`dimension_json`/`dimension_settings_json` are the one and only place
the computed geometry/formatted text are serialized -- every consumer
(the frontend) reads them, never recomputes.

Frontend: `Dimension`/`DimensionSettings`/`CmdDimension`/
`CmdDimensionKind` in `api/types.ts` (the same flat-display-vs-nested-
Cmd-payload split `Shape`/`CmdShape` already have, see either type's own
doc); `kicad-port/dimensionConvert.ts` (`toCmdDimension`/
`cmdDimensionKindOf`/`defaultDimensionPayload`, unit-tested); `state/
store.tsx`'s `nextDimensionKind`/`dimensionEditId` + the new `"dimension"`
`DrawState`/`ToolId` variant; `Canvas.tsx`'s two-click placement (`onPointerDown`),
drag-move (`DRAGGABLE_KINDS`/`commitMove`'s new `"dimension"` case),
Delete-key support, and a "Switch Dimension Arrows" context-menu entry
(a context-menu-only action in source too); `components/canvas/
selectionCandidates.ts`'s new `"dimension"` `SelectableKind` (hit-test,
box-select, the Selection Filter's own `dimensions` toggle in
`panels/SelectionFilterPanel.tsx`); `painter.ts`'s `drawDimensions`
(the in-progress two-click rubber-band reuses the existing generic
`drawState.pts` preview, no new code needed there);
`DimensionPropertiesDialog.tsx` (opens automatically right after
creation, and via "E"/double-click through `properties.ts`'s
`openPropertiesFor`, same dispatcher every other item kind already
shares); `BoardSetupDialog.tsx`'s new "Dimension Properties" page;
`useActionRunner.ts` wires all five `pcbnew.InteractiveDrawing.
*Dimension*`/`leader` toolbar actions (discoverable through the
existing, already-extracted "Dimension objects" toolbar dropdown --
`src/kicad/toolbars.json` already listed all five, so registering
handlers was the only step needed, no new toolbar UI) plus
`changeDimensionArrows`.

Rust: `crates/model/src/ir.rs`'s `Group` struct and `DrawingsSection::groups`
(chosen over `Design` or `RoutingSection` purely on construction-site count,
documented in `Group`'s own doc comment; `DrawingsSection::assign_missing_ids`
extended to id groups too, `"grp_"`-prefixed, content-derived from the
sorted member-id set). `crates/ops/src/lib.rs`: `Cmd::Group`/`Ungroup`/
`AddToGroup`/`RemoveFromGroup` (their code is in `crates/ops/src/pcb_groups.rs`
since 2026-10-08, with nesting), 6 new tests in
`crates/ops/src/tests.rs`. Frontend: `Group` in `api/types.ts`, `Group[]` on
`Drawings`, `state.enteredGroupId` + `SET_ENTERED_GROUP` + the `ESCAPE`
reducer's new tier + `withGroupSubstitution` (`state/store.tsx`),
`groupSelection`/`ungroupSelection`/`groupById` API methods (same
before/after-diff undo pattern as `duplicateSelection`), hardcoded Ctrl+G/
Ctrl+Shift+G bindings in `useGlobalHotkeys.ts` (the extraction's own
hotkeys for these two are null, same gap already noted there for Escape),
`common.Interactive.group/ungroup/groupEnter/groupLeave` in
`useActionRunner.ts`, `Canvas.tsx`'s `onDoubleClick` group-enter check,
`painter.ts`'s `drawSelectedGroups`.

## 19. Board Statistics and Swap Layers

**Board Statistics is kicad-cli's** (2026-10-03): `POST /api/board_stats` runs `kicad-cli pcb export stats` (JSON for the numbers, the text report for "Generate Report File...") on the exported board and maps it to the dialog's shape (`crates/cli/src/kicad_engine.rs`, `board_stats`); the 870-line Rust port of `ComputeBoardStatistics`/`FormatBoardStatisticsReport` is deleted, so the rows that measured it against the source are gone with it. The three checkboxes are kicad-cli's own flags (`--exclude-footprints-without-pads`, `--subtract-holes-from-board`, `--subtract-holes-from-copper`). Swap Layers is ours (below).

| Behavior | Status | KiCad file:function |
|---|---|---|
| Dialog: General page (Components/Pads/Vias/Board grids), Drill Holes page with header-click sort toggling asc/desc, three checkboxes re-running on click, session-persistent checkbox state, Close (not Cancel) | identical | `DIALOG_BOARD_STATISTICS` -- `BoardStatisticsDialog.tsx` |
| Every number | KiCad's own (kicad-cli computes it) | `ComputeBoardStatistics` |
| Number formatting (`%.4f`/`%.3f` + 2.5-digit trim for mm areas, `%.3e` fallback, ` mils`/` in` labels, `²`) | identical, in the dialog (`kicad-port/messageText.ts`) from the micrometre values the backend sends | `MessageTextFromValue`, `GetText` |
| Generate Report File... -> `<board>_report.txt` | kicad-cli's own text report (its length unit is `mm` or `in`; mils show as inches); saved through a browser download instead of a native Save dialog | `saveReportClicked`, `FormatBoardStatisticsReport` |
| Swap Layers: one row per copper layer in UI order, defaulting to itself; destination picker copper-only; map applied simultaneously (so a two-way swap works); one undo step; nothing committed when every row is identity | identical | `DIALOG_SWAP_LAYERS`, `GLOBAL_EDIT_TOOL::SwapLayers` -- `SwapLayersDialog.tsx`, `Cmd::SwapLayers` |
| Swap Layers on vias: through vias skipped, blind/buried vias get their layer pair remapped | identical (vias were never remapped before this pass) | `via->GetViaType() == VIATYPE::THROUGH`, `SetLayerPair` |

Rust: `crates/cli/src/kicad_engine.rs` (`board_stats`, 2 tests on the
kicad-cli JSON mapping), `crates/kicad-engine` (`stats`). `crates/ops/src/lib.rs`
`swap_layers` via handling + 1 new test. UI: `BoardStatisticsDialog.tsx`,
`SwapLayersDialog.tsx`, `kicad-port/messageText.ts` (+ test), wired to the
existing `pcbnew.InspectionTool.ShowBoardStatistics` /
`pcbnew.GlobalEdit.swapLayers` menu actions.

Click-through:
1. Inspect > Show Board Statistics on a routed board with a zone. Check
   the four grids fill, the drill table lists pad holes before vias at
   equal counts, and Min track clearance is a real value.
2. Tick each checkbox. The numbers update on every click (board area drops
   with "Subtract holes from board area", copper area drops with "Subtract
   holes from copper areas"). Close and reopen: the ticks are kept.
3. Drill Holes tab: click "X Size" twice; the order flips.
4. Generate Report File...: `<board>_report.txt` downloads; compare its
   layout with a KiCad-written report for the same board.
5. Switch units to mils, reopen: values read `NN.NN mils`, areas `NNN mils²`.
6. Edit > Swap Layers...: set F.Cu -> B.Cu and B.Cu -> F.Cu, OK. Tracks
   and zones trade sides; through vias stay put. Ctrl+Z restores in one step.

## 20. Hotkeyed-and-missing sweep: the 14 pcbnew actions

`docs/parity/UI-ACTIONS.md` listed 14 `pcbnew.*` actions with a hotkey and no
handler. Each one below is ported from the KiCad tool source (snapshot
commit `8303b2ad`), not just bound to a key; the five that need a subsystem
this studio does not have are recorded in `tools/ui-parity-missing.json` and
show as `missing: <reason>` in UI-ACTIONS.md (no fake handler). Every edit is
a `POST /api/cmd` verb with undo; IR additions are additive with serde
defaults.

| Action (hotkey) | Status | KiCad file:function and what was ported |
|---|---|---|
| `pcbnew.InteractiveDrawing.arcPosture` (`/`) | **done**, board and footprint editor. Registered only while an arc is being drawn (board) so `/` stays the router's posture flip the rest of the time | `drawing_tool.cpp:DrawArc`, `ARC_GEOM_MANAGER` (`kicad-port/arcGeom.ts`, 14 tests): the arc tool is now KiCad's centre -> start -> end (it was start/mid/end), `m_clockwise` starts true, `ToggleClockwise` also locks the direction, the subtended angle and the start/end swap follow `GetSubtended`/`updateArcFromConstructionMgr`, 45-degree angle snap on the end point. The committed `add_shape` arc is still start/mid/end. **Bug fixed on the way**: the canvas arc renderer drew the complementary arc (`ctx.arc`'s anticlockwise flag was inverted against `normalizeSweep`) in `painter.ts` and `footprintPainter.ts` |
| `pcbnew.InteractiveDrawing.bezier` (`Ctrl+Shift+B`) | **done**, board and footprint editor | `drawing_tool.cpp:DrawBezier`/`drawOneBezier`, `BEZIER_GEOM_MANAGER` (`kicad-port/bezierGeom.ts`): start, control 1, end, control 2 (reflected over the end), chaining (next curve starts at the end with control 1 mirrored unless the control-2 arm has zero length), double-click fills the remaining points and finishes without chaining. New `Shape::Bezier { start, c1, c2, end }` IR variant (additive); `crates/model/src/bezier.rs` + `kicad-port/bezierPoly.ts` (16 tests with `bezierGeom.ts`) are the same port of `BEZIER_POLY` flattening (Hain et al., 5 um max error) so DRC, hit-testing, 3D and the painter agree. `.kicad_pcb` `gr_curve` / `.kicad_mod` `fp_curve` written and read (round-trip test); kicad-cli 10.99 loaded the derived board and DRC'd the curve |
| `pcbnew.InteractiveEdit.duplicateIncrementPads` (`Ctrl+Shift+D`) | **done**: footprint editor (below); on the board `increment` only ever applies to pads, so it is a plain Duplicate | `edit_tool.cpp:Duplicate( increment )`. See `PARITY-fpedit.md` section 7 |
| `pcbnew.InteractiveEdit.packAndMoveFootprints` (`P`) | **done**; one approximation (the block packer) | `edit_tool_move_fct.cpp:PackAndMoveFootprints` + `spread_footprints.cpp:SpreadFootprints` (`kicad-port/packFootprints.ts`, 13 tests). Selection step: only unlocked, placed footprints (the hovered one when nothing is selected). Exact: grouping by `bbox + 1 mm gap` in a `std::map<VECTOR2I>` -- whose `operator<` compares *squared length*, so a part and its 90-degree twin share one group, reproduced -- the `optimalCountPerLine` ratio/remainder heuristics, `compareFootprintsbyRef` ordering (prefix, then trailing number), the 0.01 mm packing grid, the half-gap block inflation, and the arrangement's top-left landing on the selection's own top-left. The packed layout is a preview carried on `MovePreview.perRefOffsetUm` (painter + local ratsnest honour it) that follows the cursor like the Move tool until the drop; the drop is one `move_to` batch (one undo step); Escape reverts with nothing committed. **Approximated**: `rectpack2D::find_best_packing` (a header the pinned snapshot does not carry) packs blocks of *different* sizes -- here a shelf packer with the same "start at sqrt(area), grow by 1.2" outer loop plus a bisection of the bin side; with one block size (the common case) there is nothing to pack and the result is KiCad's. A footprint's box is anchor +- 0.25 mm merged with courtyard and pads (`FOOTPRINT::GetBoundingBox( false )` also merges silkscreen/fab drawings and zones, which the board model does not carry per footprint). The backend snaps each `move_to` to the board grid, so a packed anchor can land up to half a grid step from KiCad's |
| `pcbnew.InteractiveRouter.DragFreeAngle` (`G`) | **done** | `router_tool.cpp:InlineDrag( DM_FREE_ANGLE )`, `pns_dragger.cpp:Drag`: a free-angle drag never shoves, it marks obstacles (`crates/pns` `Dragger::free_angle`, `drag_start`'s `free_angle`, 1 test); shares `D`'s grab-and-go in `useActionRunner.ts`. **Bugs fixed on the way**: `routeDragStart/Move/Finish` sent fractional cursor coordinates the backend (integer um) read as 0,0 -- "nothing to drag there" -- now rounded (also fixed `D`) |
| `pcbnew.InteractiveDrawing.setAnchor` (`Ctrl+Shift+N`) | **done**, footprint editor | `drawing_tool.cpp:SetAnchor`; see `PARITY-fpedit.md` section 7 |
| `pcbnew.ModuleEditor.newFootprint` (`Ctrl+N`) | **done**, footprint editor | `footprint_libraries_utils.cpp:CreateNewFootprint`; see `PARITY-fpedit.md` section 7 |
| `pcbnew.LengthTuner.TuneDiffPair` (`8`) | **done**, same dialog as `7`; scoped to a straight, axis-aligned, single-segment pair | `pns_dp_meander_placer.cpp:DP_MEANDER_PLACER`. One meander along the midline of the shared stretch, each line shifted by half the pair's pitch (the pair's real spacing, kept to the micrometre), target = the pair's length `max( P, N )`, one `CommitRoute` for both lines. `crates/pns/src/dp_tune.rs` + `meander.rs:generate_dp_meander`, `tune_api.rs` (`mode: "diffpair"`), `LengthTuningDialog.tsx`. Details and differences from upstream: `crates/pns/PARITY.md` stage 8b |
| `pcbnew.LengthTuner.TuneDiffPairSkew` (`9`) | **done**, same dialog | `pns_meander_skew_placer.cpp:MEANDER_SKEW_PLACER`: the single-line placer lengthening the selected line until its net is `target skew` longer than the partner net (default 0); a line already long enough is refused. `dp_tune.rs:tune_skew`, `mode: "skew"` |
| `pcbnew.InteractiveDrawing.placeDesignBlock` (`Shift+B`) | **missing: needs a design-block library** | `drawing_tool.cpp:PlaceDesignBlock` places a block from a `.kicad_blocks` library; this studio has no block library or block IR |
| `pcbnew.InteractiveDrawing.placeImportedGraphics` (`Ctrl+Shift+F`) | **missing: needs a DXF/SVG importer** | `drawing_tool.cpp:ImportGraphics` (`GRAPHICS_IMPORTER`); none exists and no new dependencies are allowed |
| `pcbnew.TableEditor.editTable` (`Ctrl+E`) | **missing: needs table items** | `PCB_TABLE`; the design IR has no table type |
| `pcbnew.PadTool.explodePad` / `recombinePad` (`Ctrl+E`) | **missing: needs custom-shape pads** | `pad_tool.cpp:EditPad`; the footprint IR models only the six standard pad shapes (see `PARITY-fpedit.md` section 2) |

Counts after this section: 9 of the 14 are wired; the other 5 carry a recorded
reason. Verified in the browser (port 8786, scratch boards, real pointer never
touched): `G` drag of a corner and its undo; the arc tool's centre/start/end
flow, `/` taking the long way round (a 270-degree sweep checked numerically),
committed arcs rendering through their mid points; the bezier four-click flow,
its committed control points, and kicad-cli loading the derived board; Pack
and Move of six footprints (ordered columns, follows the cursor, one undo step
reverts all six, Escape leaves the board untouched); `8` on a 20 mm pair
(23 mm result within 1 um of target, both nets 23000 um, one undo step) and `9`
(a 4 mm short line matched to within 1 um).

## 21. Hotkeyed-and-missing sweep: the 8 common actions

The third group of `docs/parity/UI-ACTIONS.md`'s hotkeyed-and-missing list (the eeschema group is section 11 of
`PARITY-sch.md`, the Symbol Editor's part in `PARITY-symedit.md`). Same rule: ported from the KiCad source
(`common/tool/actions.cpp`, `common/dialogs/panel_mouse_settings.cpp`, `common/view/wx_view_controls.cpp`) or recorded in
`tools/ui-parity-missing.json` as `missing: <reason>`, no fake handler.

| Action (hotkey) | Status | KiCad file:function and what was ported |
|---|---|---|
| `common.Control.saveAs` (`Ctrl+Shift+S`) | **done, adapted**, PCB and Schematic tabs | `ACTIONS::saveAs`, "Save current document to another location". `design.json` is the only master, so what is saved is the editor's derived KiCad file, handed to the browser's Save: `GET /api/board.kicad_pcb` -> `<board>.kicad_pcb`; `GET /api/schematic.kicad_sch` -> `<board>.kicad_sch` plus each sub-sheet's own file (derived from the intent when the board stores no schematic, as every schematic output does). The PCB route refuses, with the exporter's reasons, a board KiCad would import wrongly (a track on a net that is not in the netlist). Not live on the Symbol tab (there the same key is `saveLibraryAs`; `kicad-port/actionTabGate.ts`) or the Footprint tab (it has Export .kicad_mod). `kicad-port/saveAs.ts` |
| `common.Control.updatePcbFromSchematic` (`F8`) | **done, adapted**, PCB and Schematic tabs | `DIALOG_UPDATE_PCB` / `SCH_EDITOR_CONTROL::UpdatePCB`. KiCad keeps two documents and pushes the netlist across; here every schematic edit re-derives the board's parts and nets in the same step (`reconcile_schematic`, `crates/cli/src/board.rs`), so there is nothing left to apply. F8 refetches the design and reports what an update would leave (the dialog's summary): the footprint count and which still wait to be placed (the ones an update would have added at the origin). Orphan footprints (a part whose symbol was deleted) are not listed: parts come from the intent, which no verb edits. `kicad-port/updatePcb.ts` |
| `common.SuiteControl.openPreferences` (`Ctrl+,`) | **done**: the Mouse and Touchpad page | `PANEL_MOUSE_SETTINGS` (`PreferencesDialog.tsx`, `kicad-port/preferences.ts`, 8 tests): the scroll-gesture grid (zoom / pan up-down / pan left-right x none, Ctrl, Shift, Alt) with `OnScrollRadioButton`'s one-action-per-column rule, the warning and refused OK for a remaining clash, Reset to Mouse/Trackpad Defaults, Reverse for zoom and left/right pan, "Automatically pan while moving object" and its speed, "Use zoom acceleration", zoom speed with Automatic (`WX_VIEW_CONTROLS::LoadSettings` picks the controller). Saved in the browser's local storage (UI settings, not design data) and read by every canvas: the PCB, footprint and symbol canvases already used `handleWheel`, the Schematic canvas now does too (its wheel used its own constant, so its zoom step changes to KiCad's). The status bar's autopan box is the same setting. Not offered, not a dead control: center-and-warp cursor on zoom (it moves the real pointer), the drag gestures, pan-on-movement key, and "pan left/right with horizontal movement" (KiCad stores it and never reads it) |
| `common.Control.new` (`Ctrl+N`), `common.Control.open` (`Ctrl+O`) | **missing: needs project management** | the studio serves the one project directory it was launched on and has no verb to create, open or switch projects |
| `common.Control.toggleGridOverrides` (`Ctrl+Shift+G`) | **done 2026-10-09** (GAPS.md item 14) | `common_tools.cpp:ToggleGridOverrides`, `PCB_GRID_HELPER::GetGridSize` -- the five category grids (connected items, wires, vias, text, graphics) are edited in Edit Grids... > Grid Overrides and used by every snapping tool while the switch is on (section 2); the toolbar button shows the state. `Ctrl+Shift+G` is bound to it, as in KiCad, and no longer to Ungroup (KiCad leaves Ungroup unbound; the toolbar button and the context menu still do it) |
| `common.Interactive.cycleArcEditMode` (`Ctrl+Space`) | **missing: needs the arc point editor** | `PCB_POINT_EDITOR::changeArcEditMode` only switches how arc handles behave; the studio's point editor drags zone corners only |
| `common.Interactive.pasteSpecial` (`Ctrl+Shift+V`) | **Schematic tab: wired; PCB: not done** | `PCB_CONTROL::Paste`'s dialog offers annotation modes for pasted footprints and clearing pasted nets. The PCB clipboard holds footprints now (section 22) and a plain Paste keeps nets by name and renames a taken reference (`UNIQUE_ANNOTATIONS`), but the dialog's other modes are not offered; on the Schematic tab the action is wired (`PARITY-sch.md` section 15) |

Counts after this section: 3 of the 8 are wired, the other 5 carry a reason. Across the three groups: 23 of the 43
hotkeyed-and-missing actions are wired (pcbnew 9, eeschema 11, common 3) and 20 are recorded with reasons (5, 10, 5).
Verified in the browser (port 8786, a scratch board, real pointer never touched): Save As on the PCB tab (request,
toast with the file name), on the Schematic tab (one file; with a sub-sheet the route returns a second, checked with curl), and Ctrl+Shift+S on the Symbol
tab reaching Save Library As instead; F8 on a board with six unplaced footprints; Preferences: Reset to Trackpad
Defaults, a clash giving the warning and a disabled OK, persistence across a reload, and the wheel on the PCB and
Schematic canvases (plain scroll pans up/down, Ctrl/Cmd+scroll zooms, Shift+scroll pans left/right; with a manual
zoom speed of 1 a 20-unit tick zooms by exactly 1.02).

## 22. Edit tools for every item kind (GAPS.md item 8)

Before this section Move skipped tracks and zones, Rotate and Flip took footprints and vias only, Duplicate and Copy skipped footprints, dimensions and
groups, the clipboard was the studio's own, Align and Distribute took placed footprints and ignored locks, and a pad could not be selected. Ported from
`pcbnew/tools/edit_tool.cpp`, `edit_tool_move_fct.cpp`, `align_distribute_tool.cpp`, `pcb_selection_tool.cpp` and `pcbnew/kicad_clipboard.cpp`
(snapshot `8303b2ad`). The tool half (which items, which point) is client side and pure; the item half (what a track, a zone, a dimension or a footprint does with a
move, a turn or a flip) is three backend verbs, so every action is one undo step and reaches the `.kicad_pcb` kicad-cli reads.

| Behavior | Status | KiCad file:function |
|---|---|---|
| Move and drag of footprints, tracks (their vias come along when they are selected too), zones, graphics, text, dimensions and groups | **done**: one `Cmd::MoveItems { ids, dx, dy }`, one undo step. A group moves its members; an id named twice moves once | `EDIT_TOOL::Move` / `doMoveSelection` calling `BOARD_ITEM::Move` of each class -- `crates/ops/src/pcb_transform.rs`, `kicad-port/pcbTransform.ts:planMove` / `planCarry`, `Canvas.tsx` (a plain drag of any item picks up the selection), `state/store.tsx:commitMove` (kind `"pcb"`) |
| The selection the tools work on | **done** | `PCB_SELECTION_TOOL::RequestSelection` with `FilterCollectorForLockedItems` (a locked item, a member of a locked group, a group with a locked member stays where it is; a toast says so when nothing is left) and `FilterCollectorForFreePads` (a pad stands for its footprint) -- `pcbTransform.ts:editableSelection` |
| Move Individually, Move with Reference Point, Position Relative, Move Exactly, Find and Move | **done** for every kind: they take the same selection and send the same verbs. Position Relative is one `move_items` for the whole selection; Move Exactly is one `move_items` and the `rotate_items`, one batch | `EDIT_TOOL::doMoveSelection`'s `moveIndividually`, `pickReferencePoint`, `POSITION_RELATIVE_TOOL`, `MoveExact` -- `kicad-port/pcbEditActions.ts:movableItem` (every kind's `GetPosition()`), `PcbParityDialogs.tsx`, `MoveExactDialog.tsx`, `pcbTransform.ts:planMoveExact` |
| Rotate (`R`, `Shift+R`): one item turns about its own position, several about the centre of the selection's box moved to the grid, a lone rectangle or polygon about its centre | **done**: `Cmd::RotateItems { ids, pivot, angle_millideg }`. **`R` turns counter-clockwise now**; it turned clockwise before this section | `EDIT_TOOL::Rotate` with `updateModificationPoint` and `usePcbShapeCenter` -- `pcbTransform.ts:planRotate`, `rotationPivot`, `modificationPoint` |
| What a turn does to each kind | **done**: footprints turn (their pads land where the same turn puts them); a true arc stays an arc (its mid point is turned, not resampled); a rectangle stays a normalised rectangle on a quarter turn and becomes a polygon on any other; text turns its angle (KiCad's, counter-clockwise); an orthogonal dimension swaps its axis and its side on a quarter turn; vias, zones, tracks, groups turn their points | `BOARD_ITEM::Rotate` of `PCB_TRACK`, `PCB_ARC`, `PCB_VIA`, `ZONE`, `PCB_SHAPE` (`EDA_SHAPE::rotate`), `PCB_TEXT`, `PCB_DIM_ORTHOGONAL`, `FOOTPRINT` -- `pcb_transform.rs` |
| Flip (`F`): the selection mirrored about the centre of its box (not moved to the grid), a lone item about its own position, a lone rectangle about its centre; every item to the other side | **done**: `Cmd::FlipItems { ids, pivot, direction }`. A copper layer goes to its pair and inner layers mirror through the stack (`In1` to `In(n-2)`), the front and back technical layers swap; text on a side-specific layer toggles its mirror flag; a footprint changes side, its angle becomes `-rot` (`180 - rot` top-bottom), its pads land on the mirror image (a geometry test over every pad of a part at eight pose and side combinations, both directions) and its reference text side swaps | `EDIT_TOOL::Flip`, `BOARD::FlipLayer`, `PCB_TEXT::Flip`, `EDA_SHAPE::flip`, `FOOTPRINT::Flip` -- `pcbTransform.ts:planFlip`, `flipPivot`, `pcb_transform.rs:flip_layer` |
| `R` and `F` while an item is carried | **done** for any mix of items: they act on the carried selection about the point it was picked up at (`EDIT_TOOL::Rotate` / `Flip` use the reference point that travels with the cursor), the canvas draws it through the one transform the drop sends (`kicad-port/pcbCarry.ts`, `painter.ts`), and the drop is one batch of turn, flip and move | `EDIT_TOOL::Rotate`'s and `Flip`'s `m_dragging` branch -- `pcbTransform.ts:planCarry`, `carryStart` |
| Align and Distribute for every kind, with locks | **done**: a locked item is the target and never moves (Align) or is left out (Distribute, fewer than three left means nothing to do); without one the target is the extreme item unless the cursor is inside one item's box; Distribute keeps the two end items and spaces the rest by gaps or by centres. One batch | `ALIGN_DISTRIBUTE_TOOL::GetSelections`, `selectTarget`, `DistributeItems`, `libs/kimath/.../distribute.cpp` -- `kicad-port/alignDistribute.ts` (`planAlignSelection`, `planDistributeSelection`) |
| Copy, Cut and Paste in KiCad's clipboard format | **done**: Copy asks the backend for the text (`POST /api/clipboard/copy`), puts it on the system clipboard and keeps it in the studio for the browsers that do not let a page read it back; Paste takes KiCad's text from the system clipboard first (a copy made in KiCad itself, or on another board), else the studio's own. A copy is a `(kicad_pcb ...)` fragment with the layers and only the selected items, measured from the copy's reference point, unlocked; one footprint alone is a bare `(footprint ...)` with its pads on no net. Paste maps nets by name, drops copper on layers the board lacks, makes groups again, and renames a footprint whose reference is taken to the next free one. Text that is not KiCad's pastes as a text object. Cut copies exactly what it then deletes, locked items skipped | `CLIPBOARD_IO::SaveSelection`, `CLIPBOARD_IO::Parse`, `PCB_CONTROL::Paste` / `placeBoardItems` (`PASTE_MODE::UNIQUE_ANNOTATIONS`), `EDIT_TOOL::copyToClipboard` -- `crates/kicad/src/clipboard.rs`, `crates/ops/src/pcb_paste.rs` (`Cmd::PasteClipboard`), `crates/cli/src/clipboard_api.rs`, `components/canvas/clipboard.ts`, `state/store.tsx` |
| Duplicate (`Cmd+D`) of footprints, dimensions and groups | **done**: `Cmd::Duplicate` copies in place; a footprint becomes a part of the board the intent does not list (`DrawingsSection::board_parts`, laid over the model by `board::load` like the other overlays) under the next free reference with its pads on the same nets; a group is copied with its members, the copy of a member joins its group. The copies are selected and picked up by Move. **Escape while they are carried takes them away again**, as `commit.Revert()` does (so does Escape after a Paste) | `EDIT_TOOL::Duplicate` (`DeepDuplicate` for a group), `FOOTPRINT::Duplicate` -- `pcb_paste.rs:duplicate_all`, `state/store.tsx:duplicateSelection`, `revertCarriedPlacement` |
| Deleting a copied footprint | **done**: `rip` of it takes its part away too, and Undo takes the copy and its part away together | `EDIT_TOOL::Remove` -- `Cmd::Rip` |
| When a footprint edit clears the routing | **changed**: any edit of a part clears the whole routing in this model. It now does so only when a track ends on one of the footprint's pads or a via sits on one that no staying footprint holds at the same spot (the original under a fresh copy of it). A copy made by Duplicate or Paste can be placed, turned, flipped and deleted on a routed board without losing it, also under `--strict`; copper, graphics and text never clear it. The older part verbs (`move_to`, `rotate`, `flip`, ...) keep their old answer | `Cmd::clears_routing_in(&Board)` -- `pcb_transform.rs:names_routed_part` |
| Pads are selectable | **done**: a pad is `REF.NUMBER` (`#k` after it for the k-th pad of a footprint that repeats a number); a click on it picks it over the footprint it sits in (the smaller area wins), a box takes pads only when it finds nothing else, the Selection Filter has a Pads toggle, a pad's lock is its footprint's, a selected pad is drawn with the selection outline, the Properties pane summarises it (footprint, number, net, type, shape, position, size, side), `E` or a double click reveals that pane, and the net highlight of a selected pad is its own net. A pad stands for its footprint in every edit tool; Delete on a pad only promotes the selection to the footprint and waits for a second Delete. Pad edits stay out of scope | `PCB_SELECTION_TOOL::SelectMultiple` ("if we selected nothing but pads"), `FilterCollectorForFreePads`, `EDIT_TOOL::Remove` -- `kicad-port/pcbItems.ts` (`padIds`, `padById`), `selectionCandidates.ts`, `painter.ts`, `panels/PropertiesPanel.tsx`, `kicad-port/boardControl.ts:netsOfSelection` |

**Approximations, and what is left.**
- The reference point of several items is the centre of their box moved to the nearest grid point (the grid half of `PCB_GRID_HELPER::BestSnapAnchor`; the anchor half, `kicad-port/pcbGridHelper.ts`, is not applied to this reference point), and a footprint's box is its courtyard (`FOOTPRINT::GetBoundingBox( false )` is not computed).
- A footprint edit with copper routed to its pads still clears the whole routing, as every part edit always did (`--strict` refuses it when that makes the board worse); parts land on the placement grid like every pose (at most half a snap step from where KiCad would put them).
- A copy of a footprint keeps the original's nets, as KiCad's does; moved away, its pads are unconnected and `--strict` refuses the move (`routing_connectivity`). Untick Strict for such an edit, or paste it: a lone footprint pastes with its pads on no net.
- Duplicate and Paste are a step of their own and the drop is a second one (KiCad pushes one commit when the move ends); Escape compensates.
- The clipboard text is the legacy numbered-nets form (version `20241229`, generator `pcbnew`), which KiCad 9 and 10 read; KiCad 10's own by-name form reads back here. A polyline track is written as one segment track per piece.
- Rotation step (90 degrees) and flip direction (left-right) are not settings yet, though both planners and verbs take them. `R` or `F` pressed in a click-drag before the pointer has moved once acts on the selection at once (the gap section 4 records).
- Paste Special on the PCB (annotation modes, clear nets) is not done; Paste always keeps nets by name and renames a taken reference.
- Pad edits (the Pad Properties dialog, per-pad overrides, the free-pad mode) are item 9's.

Pure logic and its tests: `kicad-port/pcbTransform.ts` (16), `alignDistribute.ts` (19), `pcbCarry.ts` (8), pad selection (`padSelection.test.ts`, 9), `localRatsnest.ts` (`carryRatsnest`), `pcbReference.ts`, `boardControl.ts`. Rust: `pcb_transform_tests.rs` (22: every kind moves, turns and flips as KiCad's class does, a footprint's pads land on the mirror image, the routing rule), `pcb_paste_tests.rs` (15), `crates/kicad/src/clipboard.rs` (5) and `crates/kicad/tests/kicad_cli_clipboard.rs` (2: kicad-cli loads our clipboard text as a board and finds the items where the copy put them; what KiCad 10 itself writes reads back), `crates/cli/src/board.rs` (6: one undo step each, the routing left alone, a duplicated footprint is a part until Undo, a copy pastes on another board through KiCad's text, a copy is placed and deleted without clearing the routing, and **a track moved with `MoveItems` is where kicad-cli's DRC finds it** in the derived `.kicad_pcb`).

Verified in the browser (port 8800, a scratch copy of `mcu30` with a zone, three graphics, a text, a dimension, a track and a via added, real pointer never touched): `R` on all six mixed items turned each about one grid-snapped pivot counter-clockwise (zone, polygon, segment, text angle 0 to 90 degrees, dimension, the rectangle staying a rectangle) and `F` mirrored them about the unsnapped centre with the layers swapped and the text mirrored, each undone by one Ctrl+Z; a synthetic pointer drag of a track and a via moved both, one undo step; `R` pressed during that drag showed the turned track and via carried, and the drop equalled the turn about the snapped centre plus the move to the micrometre; Align Left with a locked polygon left it where it was and brought the other two to its edge, Distribute Centres moved only the middle one; Duplicate of a footprint (R10), a group (a new group with copies) and Copy/Paste of a footprint, a text and a dimension, each carried and dropped, with Escape taking a carried duplicate or paste away; Cut then Paste of a footprint and a text; Move Exactly on a segment and a track with 2 mm, -1 mm and 90 degrees; Move Individually of a rectangle and a segment (each one's anchor lands on the cursor, the queue hands over to the next); Position Relative of a rectangle, a segment and a track (one vector for all three, one undo step); a click on a pad selected the pad, the Properties pane showed it, and zooming to the selection drew its outline. The scratch board's routing stayed intact (134 tracks, 13 vias, 1 zone) across the copy's move, turn, flip and delete.

## 23. The Properties panel (GAPS.md item 10)

Before this section the pane was a read-only key/value summary of one footprint or pad with Rotate and Delete buttons. It is now KiCad's property grid, ported from
`common/widgets/properties_panel.cpp`, `common/properties/property_mgr.cpp`, `pcbnew/widgets/pcb_properties_panel.cpp` and the `PROPERTY_MANAGER` registrations of
`board_item.cpp`, `board_connected_item.cpp`, `footprint.cpp`, `pad.cpp`, `pcb_track.cpp`, `zone.cpp`, `pcb_text.cpp`, `eda_text.cpp`, `pcb_shape.cpp`, `eda_shape.cpp`,
`pcb_dimension.cpp` and `pcb_group.cpp` (snapshot `8303b2ad`). The registry and the grid logic are pure (`kicad-port/propertyManager.ts`, `propertyGrid.ts`, `pcbProperties.ts`);
`components/panels/PropertyGrid.tsx` draws the grid and `PropertiesPanel.tsx` feeds it the board's selection. The schematic's pane is the same grid over its own registrations
(`PARITY-sch.md` section 17).

| Behavior | Status | KiCad file:function |
|---|---|---|
| The class registry: which properties a class has, in which order, under which group | **done**: `InheritsAfter`, `AddProperty` (the first of a name keeps it), `ReplaceProperty`, `Mask`, `OverrideAvailability` and `OverrideWriteability` (an override is for the item's own class only, which is why KiCad repeats them on each dimension class), and the depth-first walk of `collectPropsRecur` (a base class's rows come before its derived class's, the last-declared base is walked first, a class reached by two paths is walked twice and a mask only reaches what is walked after it); the group order of `rebuild()`. The registrations repeat KiCad's calls, so a footprint lists Position, Locked, Layer, Orientation, Fields ...; a track Locked, Layer, Net, Width, Start X ... End Y; a dimension shows a text colour a text does not, as KiCad's walk does | `PROPERTY_MANAGER::AddProperty`, `ReplaceProperty`, `Mask`, `OverrideAvailability`, `IsAvailableFor`, `IsWriteableFor`, `CLASS_DESC::rebuild`, `collectPropsRecur` -- `kicad-port/propertyManager.ts` |
| The grid of a selection: caption, "Basic Properties" and the other groups, a row per property common to every item | **done**: "No objects selected", the item's friendly name (`Track (arc)`, `Copper Zone`, `Rule Area` ...) or "N objects selected"; a property is shown when every item's class has it by name, it is available for every item and not hidden, and the items offer the same choices (a track's copper layers against a text's every layer: no Layer row for the mix); the value is the one they share, or `<...>` when they differ; the row is writeable only when it is for every item | `PROPERTIES_PANEL::rebuildProperties`, `extractValueAndWritability`, `GetFriendlyName` -- `kicad-port/propertyGrid.ts:buildGrid`, `extractValueAndWritability` |
| Editing: one edit sets the property on every selected item, as one undo step | **done**: `planEdit` asks each item's setter for its commands and the panel sends them as one `batch` (`Cmd::Batch`, all or nothing), so three tracks given a width are one Ctrl+Z; an item that already has the value sends nothing. A refused value (a validator, or a number that is not one) shows its message above the grid and sends nothing; a refusal by the backend (Strict, a verb's own check) is the toast, and the cell shows what the board has. Enter commits and moves to the next editor, Escape puts the old value back, Tab and a click elsewhere commit, a check box commits on a click (Space toggles it) | `PROPERTIES_PANEL::valueChanging`, `PCB_PROPERTIES_PANEL::valueChanged` (one `BOARD_COMMIT`, "Edit Properties"), `onCharHook` -- `propertyGrid.ts:planEdit`, `PropertyGrid.tsx`, `PropertiesPanel.tsx` |
| Cells: lengths and coordinates in the display unit (a unit suffix typed wins), areas in mm2, angles in degrees, enums as choices, nets as a list, colours as a swatch | **done**; the validators say what KiCad's say ("Value must be greater than or equal to 0.001 mm", "Via hole size must be smaller than via diameter", "Cannot be less than zone minimum width") | `PGPROPERTY_DISTANCE`, `_AREA`, `_ANGLE`, `_RATIO`, `PGPropertyFactory`, `PROPERTY_VALIDATORS`, `PCB_VIA::ValidateViaParameters` -- `propertyGrid.ts:formatValue`, `parseValue` |
| Footprint | **done**: Position X/Y (`move_items`), Locked, Layer (front or back: `flip_items` about its position, as `SetLayerAndFlip`), Orientation (KiCad's counter-clockwise angle; the model's `rot` runs clockwise: `rotate_items` by the difference); **Component Type** (SMD, through hole or unspecified), **Not in Schematic**, **Exclude From Position Files**, **Exclude From Bill of Materials**, **Do not Populate** and **Exempt From Courtyard Requirement** edit through `edit_board_footprint` (2026-10-09, section 26: Do not Populate and Exclude From BOM also set the schematic symbol); Reference, Value, MPN, Library Link, Block and Nets are read-only (they come from the schematic and the intent) -- "Component Type" is a row KiCad's panel does not have (its dialog has the radio) | `FOOTPRINT_DESC` -- `pcbProperties.ts` |
| Pad | **done**: Position X/Y move the footprint (`valueChanged`'s pad branch); **Pad Type**, **Pad Shape**, **Size X/Y**, **Corner Radius Ratio** and **Corner Radius Size** edit through `edit_board_pad`, and **Hole Shape** and **Hole Size** do for a through-hole pad (round or oblong; 2026-10-09, section 26); net, number and locked (the footprint's, so masked) are read-only. The clearance and mask and paste margins are in the dialog (`BoardPadPropertiesDialog.tsx`), not the panel | `PAD_DESC`, `valueChanged` |
| Footprint field (`PCB_FIELD`, 2026-10-09) | **done**: Position X/Y, Layer, Orientation, Thickness, Italic, Bold, Mirrored, Visible, Width, Height, Horizontal and Vertical Justification, Knockout and Keep Upright through `edit_board_field` (and `move_items` / `rotate_items` for the position and the angle: one undo step each); Text is writeable for a user field and read-only for the Reference and Value | `PCB_FIELD_DESC`, `EDA_TEXT_DESC` |
| Track and arc | **done**: Locked, Layer (`edit_tracks_and_vias`), Net (`set_item_net`), Width (`set_track_width`), Start X/Y and End X/Y (`edit_track`: the first and the last point; an arc keeps its mid point and is redrawn through the new ends) | `TRACK_VIA_DESC` (`PCB_TRACK`, `PCB_ARC`) |
| Via | **done**: Position X/Y, Locked, Net, Diameter and Hole (`edit_via`, each keeps the other and the validator refuses a hole as large as the pad); Layer Top, Layer Bottom and Via Type (through, blind or buried, from the span) are read-only: no verb changes a drawn via's span (KiCad's own dialog does not either) | `TRACK_VIA_DESC` (`PCB_VIA`) |
| Zone and rule area | **done**: Locked, Net (copper zones), Priority, Name (`set_zone_name`), the Keepout group (rule areas), Fill Style (fill mode, hatch rows writeable only when hatched, islands, minimum island area writeable only for "Below area limit") and Electrical (clearance, minimum width, pad connections, thermal gap and spoke width), each through `edit_zone` with every other setting as it was; no position or layer row (KiCad hides both: a zone has a layer set) | `ZONE_DESC` |
| Text | **done**: Position X/Y, Layer, Locked, Orientation, Text, Thickness, Mirrored, Width, Height (one size: this model's text is square), Horizontal Justification, all through `edit_text` | `PCB_TEXT_DESC`, `EDA_TEXT_DESC` |
| Shapes | **done**: Locked, Layer, Line Width, Fill (none or solid) through `edit_shape`; the geometry through `replace_shape` with the `EDA_SHAPE` setters' semantics: Start and End X/Y move that point alone, Center X/Y take the point on a circle with them, Radius puts it that far to the right, Width and Height put a rectangle's second corner that far from the first (the corner order stays), a polygon has Position X/Y (`move_items`); an arc's Angle and every shape's kind are read-only | `PCB_SHAPE_DESC`, `EDA_SHAPE_DESC`, `EDA_SHAPE::SetStartX`, `SetCenterX`, `SetRadius`, `SetRectangleWidth` -- `pcbProperties.ts:shapeWith` |
| Dimensions | **done** for all five kinds: Position X/Y (the first feature point alone, as `SetPosition`), Layer, Locked, Prefix, Suffix, Override Text (a leader's is its Text), Units, Units Format, Precision, Suppress Trailing Zeroes, Arrow Direction (aligned and orthogonal), Crossbar Height and Extension Line Overshoot, Leader Length, Keep Aligned, Orientation (writeable only when the text is not kept aligned), Thickness, Width, Height, all through `edit_dimension` with the rest of the dimension as it was (the label's pen included) | `DIMENSION_DESC` and the five subclasses |
| Group | **done**: Locked and Name (`edit_group` with its members as they are); no position or layer row | `PCB_GROUP_DESC` |
| Four verbs added where none fit | **done**: `edit_track { id, start?, end? }`, `set_item_net { ids, net }` (a track or via needs a net, a zone may have none), `set_zone_name { id, name }`, `replace_shape { id, shape }` (in place: id, order and lock stay). Each is one undo step alone and inside a batch | `PCB_TRACK::SetStartX`, `BOARD_CONNECTED_ITEM::SetNetCode`, `ZONE::SetZoneName`, `EDA_SHAPE` setters -- `crates/ops/src/pcb_props.rs` |

**Approximations, and what is left.**
- A footprint's position lands on the placement grid like every pose (`Board::set_pose`): a typed 12.345 mm becomes the nearest grid point, and the cell shows where it went. A pad's Position moves its
  footprint by the difference (and by it twice for two pads of one footprint selected together, which KiCad's panel does too, after a fashion).
- Moving, turning or flipping a footprint with copper routed to its pads clears the routing, as every footprint edit does (section 22).
- What this model has no place for is not registered: per-footprint clearance and paste margins, a pad's trapezoid, chamfered and custom shapes, padstacks, fabrication property, number and pin
  function, via tenting, plugging, backdrill and post-machining, a track's or shape's solder-mask expansion, teardrops per item, board text's fonts, bold, italic, vertical justification, knockout,
  keep upright, visibility and colour (a footprint's fields have them: section 26), a shape's line style, colour and corner radius, a dimension's text frame, a zone's placement area. KiCad 10's
  footprint variants are not modelled.
- Text width and height are one size. A dimension's precision is a count of decimals (0 to 5), not KiCad's unit-dependent steps.
- A group member selected by a click is the group (as KiCad's); enter the group to edit one member.
- The old pane's Rotate and Delete buttons are gone (KiCad's pane has none): `R` and `Delete`, the context menu and the Edit menu do both, and Orientation is a row.
- The coordinates are the page's: KiCad's "display origin" setting (page, auxiliary axis, grid origin) is not offered.
- The grid is not in the Footprint Editor (it has no pane there yet, `PARITY-common.md`).

Pure logic and its tests: `kicad-port/propertyManager.ts` (11 cases: order, masks, the diamond walk, overrides), `propertyGrid.ts` (18: the merge, writeability, choices, edits, cells, validators), `pcbProperties.ts` (33: every
class's list in order, every setter's commands, multi-selection). Rust: `crates/ops/src/pcb_props_tests.rs` (9) and `board.rs` `a_properties_panel_edit_of_several_items_is_one_undo_step`.

Verified in the browser (port 8805, a scratch board made with `eda board new examples/ldo.yaml` and six footprints placed -- a copy of `mcu30` was refused by the permission system -- with a track, an arc, a via, two zones (one a
rule area), a text, five shapes, three dimensions and a group added; real pointer never touched): for each kind the grid listed the rows above; every editable row was edited through the grid and each edit read back from
`/api/state` and in the cell (footprint: Position X/Y, Orientation, Layer, Locked; track: Width, Layer, Net, Start X, End Y; arc: Width, Net, End X; via: Diameter, Hole, Net, Position X, Locked; zone: Priority, Name, Net, Fill Mode,
Hatch Orientation, Clearance, Pad Connections, Remove Islands, Minimum Island Area; rule area: two keepout flags, Name; text: eight rows; segment, rectangle, circle, arc and polygon: their geometry rows, line width, fill, layer; aligned,
radial and leader dimensions; a group: Name, Locked; a pad: Position X moved its footprint), and undone one Ctrl+Z per edit, back to the exact item every time. Three tracks selected together showed `<...>` for the layer, net and width
and a width typed once changed all three as one batch, one Ctrl+Z; a footprint, a track, a via and a text selected together showed the one row they share (Locked) and a click on it locked all four, one Ctrl+Z. A hole as large as the pad, `abc` and a width of 0 each showed their message and sent nothing; Escape put the old value back. `e2e/properties-panel.check.js` repeats
this on any board.

## 24. The Appearance panel and display options (GAPS.md item 18)

Before this section the panel had a Layers tab (visibility and opacity per layer, one High contrast checkbox, Flip board view), an Objects tab (ratsnest, ratsnest mode, grid, pad numbers) and a net list that
highlighted. It is now KiCad's `APPEARANCE_CONTROLS` (`pcbnew/widgets/appearance_controls.cpp`, snapshot `8303b2ad`), with what its handlers act on: `pcb_painter.cpp` (`GetColor`: net colours, opacities, high contrast),
`ratsnest_view_item.cpp`, `pcb_selection_tool.cpp` (`Selectable`), `pcbnew_config.cpp` (`LoadProjectSettings`, `SaveProjectLocalSettings`), `project_local_settings.cpp`, `board_project_settings.cpp`,
`layer_settings_utils.cpp`, `lset.cpp`, `drc_interactive_courtyard_clearance.cpp` and `net_settings.cpp`. The model is pure (`kicad-port/appearance.ts`, `appearanceOps.ts`, `appearanceNets.ts`, `layerPresets.ts`, `appearanceFile.ts`,
`courtyardConflicts.ts`); `components/panels/AppearancePanel.tsx` and `components/panels/appearance/*` draw it, `components/canvas/appearancePaint.ts` and `painter.ts` honour it. One reducer action (`APPEARANCE`, state
`StudioState.appearance`) carries every edit, and the scripted hook has it as `studio.Appearance.op` (below).

| Behavior | Status | KiCad file:function |
|---|---|---|
| Objects tab: the rows of `s_objectSettings` with an eye, a name and, for tracks, vias, pads, zones and filled shapes, an opacity slider; Filled Shapes has the slider only | **partial**: 20 of the 22 rows. Tracks, Vias, Pads, Zones, Filled Shapes, Footprints Front and Back, Values, References, Footprint Text, Ratsnest, DRC Warnings, DRC Errors, DRC Exclusions, Anchors, Locked Item Shadow, Colliding Courtyards, Board Area Shadow, Drawing Sheet, Grid. Images (`LAYER_DRAW_BITMAPS`) and Points (`LAYER_POINTS`) are not listed: the board model has no reference images and no snap points, so the toggles would switch nothing; they stay in the model so a preset or a settings file that names them keeps them. The colour chips are read-only (the colour theme is not editable here) | `s_objectSettings`, `rebuildObjects`, `UserVisbilityLayers` |
| Footprint Text is a meta-control | **done**: switching it sets References and Values with it; switching either on puts it back on; off leaves it alone | `onObjectVisibilityChanged` |
| What a new project has on | **done**, except one: everything but DRC Exclusions and Board Area Shadow. Drawing Sheet starts **off** here (KiCad: on): the placer starts a board at the page's corner, where the sheet's frame, ten and twelve millimetres in from the paper's edge, would cut straight through it, and the studio never drew one | `GAL_SET::DefaultVisible` |
| Tracks, Vias, Pads, Zones, Filled Shapes at an opacity | **done**: the slider multiplies the item's alpha (zones 60 % in a new project, the rest 100 %); a teardrop takes the track opacity, a graphic on copper too, a filled graphic the Filled Shapes one; **an object at opacity 0 is not drawn and cannot be picked** (it counts as off for selection, its checkbox stays checked) | `onObjectOpacitySlider`, `PCB_RENDER_SETTINGS::GetColor`, `Selectable` |
| Footprints Front / Back | **done**: everything of a footprint on a hidden side -- courtyard, pads, reference, anchor, locked shadow -- is not drawn and not picked | `PAD::ViewGetLOD`, `PCB_TEXT::ViewGetLOD`, `FOOTPRINT::ViewGetLOD` |
| Pads | **done**: hidden by the object; a surface-mount pad also goes with its side's copper layer and a through-hole pad with the last copper layer on (before this, hiding F.Cu left every pad) | `PAD::ViewGetLOD`, `Selectable` |
| References, Values, Footprint Text | **partial**: the reference is drawn when its silkscreen layer, Footprint Text and References are all on. The board draws no value text (the footprint instance has no fields), so Values acts on nothing yet | `PCB_TEXT::ViewGetLOD` |
| DRC Warnings / Errors / Exclusions | **done**: a marker is on the object of its severity, a waived violation on DRC Exclusions (off in a new project: waived markers are no longer drawn). The marker the DRC dialog has selected stays on screen whatever is off | `PCB_MARKER::ViewGetLayers` |
| Anchors | **done**: a five-pixel cross at each footprint's origin, only where the footprint's side is shown and the zoom is at least 1.5 | `PCB_PAINTER::draw( FOOTPRINT )` on `LAYER_ANCHOR`, `FOOTPRINT::ViewGetLOD` |
| Locked Item Shadow | **done**: the locked footprint's box and a margin, a track a margin wider, a via ringed, a zone's outline and a graphic's stroke a margin wider, in the shadow colour; the margin is four times the silkscreen line width (Board Setup > Text & Graphics). A track's shadow goes with its layer and, in high contrast, with the dimmed layers | `PCB_PAINTER::draw` on `LAYER_LOCKED_ITEM_SHADOW`, `m_lockedShadowMargin`, `PCB_TRACK::ViewGetLOD` |
| Colliding Courtyards | **partial**: during a move, a footprint being moved and one that is not are shaded when their courtyards on the same side overlap, and so is a rule area that keeps footprints out when a moved footprint meets it; both parties are shaded, as `UpdateConflicts( view, true )` does. The courtyards are the studio's rectangles; a through-hole pad's hole inside another courtyard, and the DRC rules' courtyard clearance, are not tested | `DRC_INTERACTIVE_COURTYARD_CLEARANCE::testCourtyardClearances`, `PCB_PAINTER::draw( FOOTPRINT )` on `LAYER_CONFLICTS_SHADOW` |
| Board Area Shadow | **done**: the board outline filled with the board outline area colour | `PCB_PAINTER::draw( PCB_BOARD_OUTLINE )` |
| Drawing Sheet | **partial**: the double frame and the title block (title, file, size, date, revision, company, the first four comments) of the board's paper (Page Settings), lines only on the dark background. Not drawn: the zone-reference letters and numbers | `LAYER_DRAWINGSHEET` |
| Ratsnest, Grid | **done**: the same two switches as View and the toolbar (`showRatsnest`, `gridVisible`); a layer preset changes the grid and leaves the ratsnest alone | `onObjectVisibilityChanged`, `setVisibleObjects` |
| Pickers skip what is off | **done**: tracks, vias, pads and zones switched off or at opacity 0, a filled graphic at opacity 0, and a hidden footprint side cannot be clicked, boxed or selected with Select All; a layer that is off hides its items from the pickers by the layer's KiCad name as well as by its state key (before this, a shape on F.SilkS was still picked with F.SilkS off) | `PCB_SELECTION_TOOL::Selectable` |
| Layers tab | **done**: the copper layers front to back, then the technical layers in `LSET::TechAndUserUIOrder` (Adhesive, Paste, Silkscreen, Mask, Drawings, Comments, Eco1, Eco2, Edge.Cuts, Margin, Courtyard, Fabrication; the user layers are not modelled), each with the active-layer bar, its colour, an eye and a click to make it active. The per-layer opacity slider is the studio's own and stays. Edge.Cuts is a layer the painter honours now | `rebuildLayers` |
| The layer list's right-click menu | **done**: Show / Hide All Copper Layers, Hide All Layers But Active, Show / Hide All Non Copper Layers, Show All Layers, Hide All Layers, Show Only Front Assembly, Front, Inner (only with more than two copper layers), Back, Back Assembly Layers. The presets keep the objects and the flip as they are and leave the preset list blank; hiding the active layer's group moves the active layer to the lowest id still on | `rebuildLayerContextMenu`, `OnLayerContextMenu` |
| Layer Display Options | **done**: "Inactive layers (H)": Normal, Dim, Hide, which is `HIGH_CONTRAST_MODE`; `H` cycles all three (`common.Control.highContrastModeCycle`), the toggle action keeps Normal and Dimmed. Dim draws the other layers at a quarter of their strength (KiCad mixes 80 % into the background), Hide draws nothing of them, Edge.Cuts is pushed back to 0.3 in both. "Flip board view" is the View menu's switch | `onRatsnestMode`..`UpdateDisplayOptions`, `PCB_RENDER_SETTINGS::GetColor`'s high contrast branch, `PCB_CONTROL::HighContrastModeCycle` |
| Layer presets | **done**: the eight built-in presets in the order `m_layerPresets` lists them (a `std::map`: All Copper Layers, All Layers, Back Assembly View, Back Layers, Front Assembly View, Front Layers, Inner Copper Layers, No Layers), then the person's own after a separator, then Save preset... and Delete preset.... Picking one shows its layers, flips the board for the back presets, makes the assembly views' silkscreen the active layer (else the first layer of the set when the active one is hidden, and only a layer the board has), and a built-in preset brings back the objects as they were when a built-in preset was last left. The list shows the preset the layers, objects and flip match, else the blank entry. Save asks a name, asks before replacing one of the person's, and refuses a built-in name with KiCad's message | `loadDefaultLayerPresets`, `rebuildLayerPresetsWidget`, `onLayerPresetChanged`, `doApplyLayerPreset`, `syncLayerPresetSelection` |
| Viewports | **done**: Save viewport... keeps the part of the board on screen under a name (the same name replaces it), a saved one zooms back to it (centred, the larger ratio decides: `VIEW::SetViewport`), Delete viewport... removes one | `onViewportChanged`, `VIEW::GetViewport` / `SetViewport` |
| Nets tab: the nets, a colour, an eye, a name | **done**: sorted by name (character order), without the `unconnected-(` nets, with a filter box, buttons for the Net Inspector and for Board Setup > Net Classes. **The eye hides the net's ratsnest** ("Click to hide ratsnest for ..."): `RATSNEST_VIEW_ITEM::ViewDraw` is the one place KiCad reads the hidden nets, so a hidden net's copper stays on screen -- the set is the one `pcbnew.EditorControl.hideNet` and `showNet` edit. A click on a net highlights it (the studio's own; KiCad's click does nothing) | `NET_GRID_TABLE`, `rebuildNets`, `OnNetGridMouseEvent` |
| Net colours | **done**: double click or middle click on a swatch opens a colour dialog (a colour, an opacity, "No color"); the right-click menu has Set Net Color, Clear Net Color, Highlight, Select Tracks and Vias in, Unselect Tracks and Vias in, Show All Nets and Hide All Other Nets | `OnNetGridRightClick`, `onNetContextMenu`, `NET_GRID_TABLE::updateNetColor` |
| Where a net colour shows | **done**: "Net colors" All puts the colour on every copper item of the net (pads, tracks, vias, zones, with the highlight brightening and darkening it as it does the layer colour) and on its ratsnest, Ratsnest on the ratsnest only (KiCad's default), None nowhere. A net's own colour beats its net class's; the Default class has none. `pcbnew.Control.netColorMode` cycles the three | `PCB_RENDER_SETTINGS::GetColor` (net colour branch), `RATSNEST_VIEW_ITEM::ViewDraw`, `PCB_CONTROL::NetColorModeCycle` |
| Net Display Options | **done**: Net colors (All, Ratsnest, None) and Ratsnest display (All, Visible layers, None: the global ratsnest off) -- the second moved here from the Objects tab; with the inactive layers hidden, "visible layers" means the active one | `createControls`, `onNetColorMode`, `onRatsnestMode` |
| Net Classes tab | **done**: the Default class first, without a colour, then the others by name; each with a colour, an eye (it hides the ratsnest of every net the class owns and remembers the class), its name and how many nets it owns (the studio's own); the right-click menu has Set, Use Color from Schematic (always dimmed: the studio has no schematic net class colours) and Clear Netclass Color, Highlight Nets in, Select and Unselect Tracks and Vias in, Show All Netclasses and Hide All Other Netclasses. It is a tab of its own here; KiCad lists the classes under the nets | `buildNetClassMenu`, `showNetclass`, `onNetclassContextMenu` |
| A net belongs to one class | **simplified**: the first class whose pattern matches, else Default (`classOfNet`). KiCad builds a composite class for a net several classes claim and `ContainsNetclassWithName` is true for each of them | `NET_SETTINGS::GetEffectiveNetClass` |
| Persistence | **done**: everything above is kept per project in `appearance.json` beside `design.json`, never in the design (it does not reload the scene or enter undo). `local` is what KiCad keeps in `<project>.kicad_prl` (`board.visible_items`, `board.opacity.*`, `board.hidden_nets`, `board.hidden_netclasses`, `board.net_color_mode`, `board.high_contrast_mode`, the active layer, the layers off, the ratsnest mode, the flip), `project` what it keeps in the project file (`net_settings.net_colors`, the net classes' `pcb_color`, `board.layer_presets`, `board.viewports`). Read once when the studio opens (nothing is written before the file and the board have both been read), written 0.5 s after the last change. Nets the board no longer has are dropped when it is written (a net class's colour is kept by the class's name); the nets of a hidden class are hidden when it opens | `LoadProjectSettings`, `SaveProjectLocalSettings`, `PROJECT_LOCAL_SETTINGS`, `PARAM_LAYER_PRESET`, `PARAM_VIEWPORT`, `crates/cli/src/appearance_api.rs` |
| The derived KiCad project | **done**: the engine writes `<stem>.kicad_prl` (visible layers as KiCad's hex mask, visible items, opacities, modes, hidden nets and classes, active layer) and puts the net colours, layer presets and viewports (nanometres) into the `.kicad_pro` it derives; KiCad reads the `.kicad_prl` (kicad-cli rewrote ours as its own in a check). A net class with a colour moves **all** the board's classes into the project file (definitions and net patterns, `pcb_color` on the coloured ones) and out of the board file: `BOARD::SetProject` replaces the project's classes by the board's `(net_class ..)` blocks whenever it has any, colours included. A slow test has kicad-cli judge the same board both ways and find the same clearance violations. Boards without a class colour are written exactly as before | `crates/kicad/src/appearance.rs`, `crates/kicad-engine/src/lib.rs` (`write_project_with`, `export_board`), `BOARD::SetProject` |
| Actions | **done**: `pcbnew.Control.netColorMode` (was recorded as unwired), `common.Control.highContrastModeCycle` in three states, and `studio.Appearance.op` -- `__eda.run( "studio.Appearance.op", { op: "object", id: "tracks", visible: false } )` is a click on the eye; the ops are `kicad-port/appearanceOps.ts`'s. `__eda.state().appearance` reports what differs from a new project | `pcb_control.cpp` |

**Approximations, and what is left.**
- The colour theme is not editable: the swatches of layers and objects are read-only, and the net and net class colours are the only colours the panel changes. KiCad's `Use Color from Schematic` is dimmed for the same reason.
- Images and Points (no such items in the board model) and Values (the board draws no value text) have no effect on the picture; the first two have no row. Via types are not separate objects, as in this KiCad (Vias is one row).
- Dim is a flat quarter-strength alpha, not KiCad's mix toward the background; pads, vias and ratsnest are not dimmed by it.
- The selection filter is not part of the saved settings (`board.selection_filter` is in KiCad's `.kicad_prl`); the quick switchers on Ctrl+Tab (presets) and Alt+Tab (viewports) are not ported -- the browser keeps those keys.
- A rename of a net class in Board Setup does not carry its colour (colours are kept by class name, outside the design); the colour of a class that is gone stays in the file, unused.
- The 3D viewer has its own Appearance manager (`components/viewer3d`); this panel is the board's.

Pure logic and its tests: `kicad-port/appearance.ts` (16 cases: colour text, the 22 objects and their defaults, the rows, the Footprint Text meta-control, the modes and their numbers, the net colour rules and the highlight), `layerPresets.ts` (19: the
eight presets and their order, picking one, the active layer, the objects a built-in preset restores, the menu's presets, save and delete, viewports), `appearanceNets.ts` (12: the lists, the eyes, the classes, the menus), `appearanceOps.ts` (16: every edit
through the reducer, the object keys the pickers read), `appearanceFile.ts` (7: the file both ways, KiCad's rules for a damaged `board.visible_items`), `courtyardConflicts.ts` (9), `edaTestHook.ts`'s summary and the op parser (3). Rust:
`crates/kicad/src/appearance.rs` (12: layer ids and the hex mask against a real `.kicad_prl`, the project's additions, the class split), `crates/cli/src/appearance_api.rs` (5), `crates/kicad-engine/src/lib.rs` (2) and the slow
`a_net_class_colour_moves_the_classes_into_the_project_and_kicad_cli_judges_the_board_the_same`.

Verified in the browser (port 8811, a scratch board made with `eda place examples/mcu_board_30plus.yaml` -- 30 parts placed and routed, two net classes, a GND zone filled, a locked footprint and track added through `/api/cmd`; real pointer
never touched): every tab and menu opened; a net colour and a class colour set through the real dialog showed on pads, tracks and zone in the All mode, only on ratsnest lines in the Ratsnest mode (four nets unrouted: canvas pixels
counted) and nowhere with None, and the three-state action and the contrast cycle went round; the eye of a net and of a class hid ratsnest lines and the class eye set its eight nets; opacity 0 on vias removed them, zones took 60 %; Back Layers flipped the board
and left B.Cu only, Front Layers the other side, and a preset was saved, refused under a built-in name, asked before replacing, recalled with its objects and deleted; a viewport was saved, the view zoomed, the viewport recalled it; with
the inactive layers hidden only the active layer drew; a footprint carried over two others showed the conflict shadow; after a reload every setting (colours, hidden nets and classes, presets, viewports, the active layer) came back from
`appearance.json`, and the derived `.kicad/board.kicad_prl` and `.kicad_pro` held them. No console errors. `e2e/appearance-panel.check.js` repeats the main steps (28 checks through the panel's own controls and
`window.__eda`: objects, net colours, the two cycles, presets, viewports, net and class eyes, the file; paste it into the page of a scratch board).

## 25. The board outline: Edge.Cuts as KiCad builds it (GAPS.md Status; `pcbnew/convert_shape_list_to_polygon.cpp`, `board.cpp::GetBoardPolygonOutlines`, `drc/drc_test_provider_misc.cpp::testOutline`)

Before this section the board had one outline polygon. The importer turned an Edge.Cuts arc into its chord and a rectangle or circle into a polygon, kept the largest closed loop and dropped
every cutout, closed an open chain behind the user's back, and the writer made every board's outline a closed loop of `gr_line`s again. Everything that measures to the edge -- the zone fill,
the router, the placement gates, the 3D body, kicad-cli's `copper_edge_clearance` on the export -- worked from that polygon. Now the outline is built as KiCad builds it, from the items.

| Behavior | Status | KiCad file:function |
|---|---|---|
| Edge.Cuts items chain end to end, within 0.01 mm, into closed contours: lines, arcs and Bezier curves; a circle, a rectangle and a polygon are contours by themselves; the order and direction of the items do not matter | **done** (`crates/drc/src/outline.rs::convert_outline_to_polygon`; 18 unit tests) | `ConvertOutlineToPolygon` / `doConvertOutlineToPolygon` (`findNext` takes the nearest of the two nearest end points, `processShapeSegment`, `processClosedShape`; the closing arc is polygonised again to end on the first point) |
| Curves flattened at the board's `m_MaxError` (0.005 mm): an arc as `SHAPE_ARC::ConvertToPolyline` (its ends exact, the points between at mid-slice, the error split either side), a circle as `SHAPE_ARC( centre, start, 360 )`, a curve as `BEZIER_POLY` | **done** (`crates/model/src/outline.rs`: `Arc`, `arc_to_segment_count`, `circle_to_end_segment_delta_radius`) | `SHAPE_ARC::ConvertToPolyline`, `GetArcToSegmentCount`, `CircleToEndSegmentDeltaRadius`, `BEZIER_POLY::GetPoly` |
| Cutouts: a contour inside an odd number of others is a hole of the one around it, an island inside a cutout is an outline again, several separate outlines are allowed; contours that touch or cross take the boolean route (cutouts subtracted, islands added) | **done** | `buildContourHierarchy`, `addOutlinesToPolygon`, `addHolesToPolygon`, `hasOverlappingClosedContours` |
| Malformed outlines are reported, never closed: `(not a closed shape)` at the middle of the gap, `(self-intersecting)` where a chain runs into a used item or a contour crosses another, `(multiple board outlines not supported)` where one outline is asked for, a shape of a few nanometres | **done**. The findings are the `placement_outline_malformed` gate (a failure in the status bar's count and the problems list, located at the Edge.Cuts items it names), `outline_errors` in `/api/state` and `eda import-kicad`'s output; `kicad-cli pcb drc` reports the same `invalid_outline` on the exported file and stays the judge. On five real QA boards the two agree (`issue18839` an arc that does not close, `issue24078` a rectangle across the outline: malformed; `issue8909`, `issue12609`, `issue11814`: not), also after our export | `checkSelfIntersections` (KiCad's sweep break compares squared norms: ported as it is, so the verdict is KiCad's), `TestBoardOutlinesGraphicItems`, `testOutline` |
| An outline that does not close is the rectangle round the edges (`aInferOutlineIfNecessary`), the zone fill is then not clipped to it (`m_brdOutlinesValid`) but its islands outside are still dropped | **done** (`BoardOutline::{valid, inferred}`) | `BuildBoardPolygonOutlines`, `ZONE_FILLER::Fill` |
| Footprint Edge.Cuts graphics (a card-edge slot, a mounting slot) | **simplified**: the importer makes them board-level shapes in board space (a rectangle a tilted footprint leaves tilted is a polygon), so KiCad's per-footprint pass -- a closed contour is a hole when copper lies outside it, or when the footprint has several -- is the ordinary nesting test here. They do not follow the footprint when it is moved (the old importer baked them into the outline polygon the same way) | `BuildBoardPolygonOutlines` (`isCopperOutside`) |
| The importer keeps every Edge.Cuts item as the shape it is (`gr_line`, `gr_arc`, `gr_rect`, `gr_circle`, `gr_poly`, `gr_curve`, and the `fp_` forms) with its width; KiCad 6's `(gr_arc (start CENTRE) (end START) (angle A))` and `(width W)` are read. A board whose Edge.Cuts are one plain closed loop of lines stays `placement.outline`, as before; so does an intent's outline | **done** (`DrawingsSection::outline_is_shapes`; `placement.outline` is then only the summary, the outer ring of the largest outline, kept in step by every command that changes an Edge.Cuts shape: `crates/ops/src/outline.rs`) | `parsePCB_SHAPE` (`LEGACY_ARC_FORMATTING`) |
| The writer emits the same items: arcs, circles, rectangles and curves come back as `gr_arc`, `gr_circle`, `gr_rect`, `gr_curve`, not as a polygon, and a gap stays a gap | **done**. kicad-cli's plot of Edge.Cuts is identical before and after the round trip (slow test), and an open outline still gets `invalid_outline` | `PCB_IO_KICAD_SEXPR::format( const PCB_SHAPE* )` |
| The zone fill: clipped to the outline with its cutouts, no copper within `copper_edge_clearance` of an arc's curve, a circle's ring, a cutout's rim, or inside a solid shape; islands outside the board dropped | **done** (`FillInput::{board_outline, board_outline_invalid, edge_cuts}`); `crates/zone-filler/PARITY.md` measures it against kicad-cli | `knockoutGraphicClearance`, `ZONE::BuildSmoothedPoly` |
| The router: every side of every Edge.Cuts item is an obstacle with the board's edge clearance, a cutout's rim included | **done** (`crates/pns/src/from_ir.rs`) | `PNS_KICAD_IFACE_BASE::syncGraphicalItem` |
| `placement_within_outline` and `routing_within_outline`: a courtyard corner, a via or a track point in a cutout, or outside an outline, fails | **done** (`crates/gates/src/pcb.rs::Region`); a board whose outline is its polygon is judged as before | -- (this project's own gates) |
| The canvas draws the Edge.Cuts shapes themselves (arcs, circles) when they are the outline, not the summary polygon; the 3D board slab and solder mask are extruded from the outline with its cutouts and every separate piece | **done** (`outline_polys` and `outline_is_shapes` in `/api/state`; `kicad-port/pcbOutline.ts::boardOutlinePolygons`, `scene.ts:buildOutlineShapes`) | `board_adapter.cpp:1029` |
| Not done | the autoplacer and the placement verbs (`crates/place`, `Cmd::PlaceAt` and the rest) still work from the outline's bounding box; the Freerouting DSN boundary is the summary polygon, with no cutouts; NPTH holes are not cut out of the 3D slab (`aIncludeNPTHAsOutlines`); a rectangle's corner radius and the arcs of a `gr_poly` are not in the IR; there is no outline-specific editor (the generic shape tools draw and edit Edge.Cuts), and "Fix discontinuities in board outlines" in Update PCB from Schematic has nothing behind it | |

Verified in the browser (port 8812, a scratch board imported from a hand-written KiCad file: a 50 x 36 mm board with 5 mm rounded corners, a round hole, a square hole and a D-shaped slot made of a
curve and a line, three 0603 parts, a track and two GND zones; real pointer never touched). The canvas drew the round corners and the three holes as their own shapes, the outline once; `Fill All
Zones` left the copper 0.5 mm in from the arcs and from each hole's rim and none inside a hole; the 3D tab showed the slab with the corner arcs and the three holes cut through it. Deleting the D-slot's
closing line (`POST /api/cmd` `delete_shape`) opened the outline: the status bar's Failures went from 3 to 4, the new one `placement_outline_malformed` at the curve (`Board has malformed outline (not a closed
shape) at (36.000, 24.000) mm`), `/api/state` named it in `outline_errors`, the summary outline became the rectangle round the edges, and the page showed no error; `common.Interactive.undo` brought the board back (12 Edge.Cuts shapes,
the 80-point summary, no finding). The same file through `eda import-kicad` printed `12 Edge.Cuts item(s) kept as shapes`.

## 26. Footprints as editable objects: fields, attributes and pads (GAPS.md items 9 and 22)

Before this section a footprint on the board had a pose, a side and a four-way side for its Reference (`Cmd::SetLabelSide`); its Reference and Value were text the writer placed itself, it had no
user fields and no attributes, and its pads were a read-only summary. Now they are the objects KiCad's are. Ported from `pcbnew/pcb_field.cpp`, `dialogs/dialog_footprint_properties.cpp`,
`dialogs/dialog_pad_properties.cpp`, `footprint.cpp` (`FOOTPRINT::Add`, `Move`, `Rotate`, `Flip`, the attribute flags), `pad.cpp` and `pcb_io_kicad_sexpr.cpp` / `_parser.cpp` (`format( FOOTPRINT )`, `format( PAD )`,
`parseFOOTPRINT_unchecked`) at `8303b2ad`. Everything is an overlay on `design.json` and an undoable `/api/cmd` verb: the IR change is additive (`DrawingsSection::footprint_edits`, serde defaults, nothing
written for a footprint nothing was edited on), the intent stays the source of the Reference and Value *text*, and kicad-cli is still the only engine that judges the board.

| Behavior | Status | KiCad file:function |
|---|---|---|
| Storage: one `FootprintEdit` per footprint (`crates/model/src/fp_edit.rs`) holds the Reference and Value layouts, the user fields, the attributes and the pad edits; `None` is "as before" (the writer's legacy placement, attributes derived from the pads and the symbol's flags) | **done** | `FOOTPRINT::Fields`, `PCB_FIELD` |
| Reference and Value layout: position, width and height, thickness, layer (silk or fab, top or bottom, or any layer), visibility, rotation, horizontal and vertical justification, mirror, bold, italic, keep upright, knockout. The position and angle are in the footprint's own frame (like pads), so a footprint that moves, turns or flips takes its fields with it; the default Reference and Value reproduce what the writer always wrote, byte for byte | **done** | `EDA_TEXT`, `PCB_FIELD::Move/Rotate/Flip`, `FOOTPRINT::Move/Rotate/Flip` |
| User fields: add, delete, rename and give text, each with the same layout; names must be unique, not empty and not Reference or Value ("There are two fields called ...") | **done**; the text of the Reference and Value stays the schematic's (read-only); Datasheet and Description, KiCad's other two mandatory fields, are not fields here, and text variables (`${...}`) are not expanded | `DIALOG_FOOTPRINT_PROPERTIES`, `FOOTPRINT::AddField` |
| A field is an item of the board (`REF:Name`): selected by a click or a box (a hidden one is not under the pointer), drawn as selected, kept selected across refreshes, moved with M, dragged, moved with Move Exactly, turned about its own anchor and flipped; each one undo step; a field of a footprint that is moved too is not moved twice; locked with its footprint | **done** | `EDIT_TOOL::Move`, `PCB_FIELD`, `FilterCollectorForLockedItems` -- `move_items` / `rotate_items` / `flip_items` take field ids |
| The Appearance panel (section 24): a footprint's fields follow the Objects rows as `PCB_FIELD::ViewGetLOD` and `PCB_SELECTION_TOOL::Selectable` have them: the Reference needs References, the Value Values, every field Footprint Text, and the footprint's side must show (Footprints Front or Back); the field's own layer must be on too; selecting the footprint shows its fields whatever those rows say ("Force show fields when footprint selected", on by default). A field those rows hide cannot be picked | **done** | `PCB_FIELD::ViewGetLOD`, `PCB_TEXT::ViewGetLOD`, `PCB_SELECTION_TOOL::Selectable` -- `painter.ts:drawFields`, `selectionCandidates.ts:fieldShown` |
| Flip: the footprint's fields go to the other side's layers (F.SilkS to B.SilkS, F.Fab to B.Fab, ...) and are mirrored, in one undo step with the footprint | **done** | `FOOTPRINT::Flip`, `PCB_FIELD::Flip` |
| Attributes: component type (SMD, through hole, unspecified), not in schematic (board only), exclude from position files, exclude from bill of materials, do not populate, exempt from courtyard requirement | **done**; written as `(attr ...)`; an unedited footprint gets `(attr smd)` or `(attr through_hole)` from its pads, so `--smd-only` and `--exclude-fp-th` work | `FOOTPRINT::SetAttributes`, `FP_SMD`, `FP_THROUGH_HOLE`, `FP_BOARD_ONLY`, `FP_EXCLUDE_FROM_POS_FILES`, `FP_EXCLUDE_FROM_BOM`, `FP_DNP`, `FP_ALLOW_MISSING_COURTYARD` |
| Do not populate and exclude from BOM are the symbol's flags too: setting one on the footprint sets the schematic symbol and the other way round (the last edit wins), so the BOM of the derived schematic agrees with the board | **done**; kicad-cli's position export honours "exclude from position files" and DNP (`--exclude-dnp`, a new checkbox of Generate Placement Files, and the fab package's position file), its BOM export honours DNP (the JLC BOM passes `--exclude-dnp` now) | `PLACE_FILE_EXPORTER`, `m_excludeDNP` |
| Pads, per pad by number (and by "the k-th pad with that number"): type (SMD, through hole, NPTH), shape (circle, oval, rectangle, rounded rectangle), size, shape offset, rotation, corner radius ratio, hole (round or oblong), solder mask margin, solder paste margin and ratio | **done**; the type, shape, size, hole, rotation and ratio are folded into the instance's footprint the model sees (placement, courtyards and the canvas use them); the offset and the margins reach the board file and kicad-cli (our own checks do not read them). The pad's local clearance is the zone filler's overlay (below): a pad has one | `PAD::SetShape/SetSize/SetDrillSize/SetOffset`, `SetLocalSolderMaskMargin`, `SetLocalSolderPasteMargin(Ratio)` |
| Not in the pad edit | **not ported**: trapezoid, chamfered and custom shapes, padstacks (front, inner and back differing), fabrication property, pad number and pin function, pad-to-die length, backdrill and post-machining; the zone filler's overlay is not repeated here (next row) | `dialog_pad_properties.cpp` |
| The zone filler's overlay (`DrawingsSection::zone_overrides`, `Cmd::SetFootprintZoneConnection`, `Cmd::SetPadZoneOverrides`; section 8): a footprint's zone connection and clearance, and per pad number the pad connection, relief gap, spoke width, spoke angle and clearance | **done by the zone filler** (GAPS.md item 13); it sits beside `footprint_edits` and the two are never repeated: a pad has one clearance, the overlay's. The same dialogs below edit both | `FOOTPRINT::SetLocalZoneConnection`, `SetLocalClearance`, `PAD::SetLocalZoneConnection`, `SetLocalThermalGapOverride`, `SetLocalThermalSpokeWidthOverride`, `SetThermalSpokeAngle` |
| The Footprint Properties dialog: the field grid (Name, Value, Show, Width, Height, Thickness, Layer, and with "Show all columns" the italic, bold, mirrored, keep upright, knockout, justification and offset), Add Field and Delete Field, placement (position, orientation, side, locked), component type and the attributes, and the zone connection fields of the Clearances tab and of Pad Properties (the footprint's connection and clearance; per pad number the pad connection, relief gap, spoke width, spoke angle and clearance); one OK is one undo step (`edit_board_footprint`, then the move, turn, flip and lock, then `set_footprint_zone_connection` and `set_pad_zone_overrides` for what was changed) | **done**; orientation is KiCad's counter-clockwise (the model's `rot` runs clockwise); a position lands on the placement grid like every pose | `DIALOG_FOOTPRINT_PROPERTIES::TransferDataFromWindow`, `Validate` |
| The Pad Properties dialog (a double click, or Properties on a pad): type, shape, size, offset, rotation, corner radius, hole, the mask and paste overrides (an override that is switched off puts the library's value back) and the zone filler's fields of the same panel (the clearance, pad connection, relief gap, spoke width and angle: those of every pad that carries the number); one OK is one undo step (`edit_board_pad` and `set_pad_zone_overrides` in one batch); the checks are `PAD::CheckPad`'s ("Error: (Pad must have a positive size)", "Through hole pad has no hole") | **done** | `DIALOG_PAD_PROPERTIES::TransferDataFromWindow`, `padValuesOK` |
| The Properties panel: the rows of a `PCB_FIELD`, the footprint's attribute rows and the pad's type, shape, size, corner radius and hole rows (section 23) | **done** | `PCB_FIELD_DESC`, `FOOTPRINT_DESC`, `PAD_DESC` |
| The `.kicad_pcb`: `(property "Name" "text" (at x y a) (layer ...) [(hide yes)] (effects ...))` for every field (bottom-side fields in the file's mirrored frame), `(attr ...)`, the pad offset inside `(drill ...)`, the margins; read back by the importer into the same `FootprintEdit` (the old `fp_text` form too, and a reference shared with another footprint keeps the text the file has). One `(pad ..)` form carries the pad edit's extras and the zone filler's fields (`clearance`, `zone_connect`, `thermal_bridge_width`, `thermal_bridge_angle`, `thermal_gap`), in KiCad's order, and each half is read back into its own overlay | **done**; kicad-cli loads it | `format( FOOTPRINT )`, `parseFOOTPRINT_unchecked` -- `crates/kicad/src/fp_fields.rs`, `pcb.rs`, `import.rs` |
| Duplicate, Copy, Paste and Create Array take a footprint's edit and its zone overrides with them (a copy under its new reference); Rip up removes the edit and the zone overrides; the clipboard text carries both (its one `(pad ..)` form has the extras and the zone fields), and a paste of a footprint the model knows by name keeps the pads that are not the library's (a shape, size or hole edited where it was copied) as the copy's pad edits | **done** | `kicad_clipboard.cpp` -- `ClipFootprint.edit`, `ClipFootprint.zone_overrides`, `pad_edits_between` |
| Change Footprint(s), Update Footprints from Library, geographical reannotate | **not ported** (a part's footprint comes from the intent, which has no verb for it) | `dialog_exchange_footprints.cpp`, `dialog_update_pcb.cpp`, `board_reannotate_tool.cpp` |

Rust (69 new tests): `crates/model/src/fp_edit.rs` (16: the frames, the defaults, pad lookup, hole edits, derived attributes, JSON, the pad edits between two footprints), `crates/ops/src/fp_edit_tests.rs` (11: the verbs and their refusals, the symbol sync,
fields that move, turn and flip on their own and with their footprint), `crates/kicad/src/fp_fields.rs` and `pcb.rs` (13: the text written and read back on either side, the old form, the attributes in KiCad's order,
a shared reference, one pad form with both overlays), `crates/ops/src/array_tests.rs` (24, section 17) and five in `crates/cli/src/board.rs` (an edit is one undo step and reaches the model and the file; a duplicate keeps it and the
zone overrides; a copy and paste keeps both, here and on another board; an array is one undo step; **slow tier**: `kicad_cli_loads_the_edited_board_and_its_position_and_bom_exports_honour_the_attributes` runs real kicad-cli on the written board). Web (39 new `node --test` cases): `kicad-port/fpFields.ts`
(ids, layouts, commands, the checks), `pcbProperties.ts` (the new rows), `arrayOptions.ts`, `padZone.ts` (the commands the dialogs' OK sends for the zone fields); the painter, hit testing and carry draw, pick and move a field.

Verified in the browser (port 8809, a scratch board made with `eda board new examples/mcu_board_30plus.yaml` and eight footprints placed with `eda board place`; the page's pointer events were dispatched in the page, the real
pointer never touched): `e2e/footprint-edit.check.js` runs 19 scenarios through `window.__eda`, each undone and the board compared with how it was -- a field selected as an item, moved with Move Exactly, carried with M and
dragged by the pointer (one undo step, its footprint not moved); the Appearance panel's References, Values and Footprint Text rows hiding fields, and a hidden field not picked; the panel edits of a field (visible, thickness, justification, layer) and of a footprint (do not populate, exclude from BOM and from position files, component type); the Footprint Properties
dialog (a reference size, a new user field, an orientation and an attribute in one OK, one undo; a user field added, renamed and deleted; a reserved name refused with nothing sent; E on a field opens it on that field's row); Flip (fields on the
back layers, mirrored); the Footprint Properties dialog's zone connection fields beside the field grid and the attributes (one OK, one undo for all of it); the Pad Properties dialog (shape, size, offset, clearance and the zone connection in one OK, one undo; a pad with no size refused in the check's words); Copy and Paste of a footprint with a pad edit and a zone override (the paste has both); Create Array as a 3 x 2 grid with unique references, four
footprints on a circle with "Full circle", and Arrange selection, each one undo step, the dialog's remembered values, the messages of bad entries and Escape. With the real kicad-cli on the scratch board after edits sent to
`/api/cmd`: `/api/fab/pos` with `exclude_dnp` left out the footprint marked do not populate and the one excluded from position files, and `/api/fab/bom` left the do-not-populate one out of the BOM.
A hidden Browser pane stops the page's timers, which `__eda.run` waits on: the script then drives them from a fetch loop (its header says how to read a run that outlasts a tool call).

**Approximations, and what is left.**
- A footprint's position lands on the placement grid like every pose (`Board::set_pose`); the field positions do not (they are the dialog's numbers).
- The pad offset and the margins are written to the board file and read by kicad-cli, and drawn by the canvas (offset), but our own placement and courtyard checks read the pad as the engine model has it (type, shape,
  size, hole, rotation, ratio).
- A field's text is literal: KiCad's text variables are not expanded, and the Reference and Value text is the schematic's.
- Per-footprint mask and paste margin overrides, trapezoid, chamfered and custom pads, padstacks, Datasheet and Description fields, Change Footprint, Update from Library, geographical reannotate and the footprint
  editor's array of pads (section 17) are left.
- The zone filler's fields address a pad by its number: the zone fields of a pad that shares its number with others are those of all of them, as the overlay keeps them.
