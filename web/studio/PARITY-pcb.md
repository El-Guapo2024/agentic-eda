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
| Selection Filter panel really filters (footprints/tracks/vias/zones/graphics/text) | identical for the categories this model has | `panel_selection_filter.cpp`/`itemPassesFilter` -- no pads/keepouts/dimensions/points/locked-items/other-items categories (no such selectable concept exists here); was three checkboxes, two of them permanently `disabled` and doing nothing |
| A hidden layer's items are unselectable; in high-contrast mode, off-active-layer items are too | identical | `Selectable()`'s layer-visibility and `GetHighContrast()`/`GetHighContrastLayers()` checks |
| Double-click opens properties | identical (was a no-op outside active drawing) | `Main`'s `IsDblClick` handler -- this app has no groups, so it's always "open properties," never "enter group" |
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

| Behavior | Status | KiCad file:function |
|---|---|---|
| Rotate/flip a footprint (standalone, not mid-move) | identical for 1 item (predates this session); **this session**: 2+ items now share ONE pivot -- `updateModificationPoint`'s real rule (the selection's union-bounding-box center), not each part spinning/mirroring about its own anchor the way an earlier session's docs here incorrectly called "identical" | `edit_tool.cpp:Rotate`/`Flip`, `updateModificationPoint` -- `state/store.tsx`'s `rotateSelection` (now one atomic `Cmd::MoveExact` for the whole group, same single-`BOARD_COMMIT::Push()` undo granularity as source) and `flipSelection` (mirrors each part's X about the shared center via `move_to`, then `flip`'s existing side-toggle -- **documented gap**: this is N separate Cmds, not source's one atomic commit, so undoing a group flip takes 2N Ctrl+Z's, not 1) |
| Rotate (R/Shift+R) / Flip (F) *during* an active Move | identical for the common case: a dragged footprint spins in place | `edit_tool.cpp`'s own `m_dragging` branch of `Rotate`/`Flip`: the live item is mutated directly mid-drag rather than committing a separate op, and a further rotate during the same drag reuses the *first* rotate's reference point (`updateModificationPoint`'s `m_dragging && HasReferencePoint()` guard). This app can't mutate an uncommitted backend item, so it accumulates the transform on the client-side preview instead (`MovePreview.rotateQuarterTurns`/`flipped`, applied in `painter.ts`'s preview render) and commits move+rotate+flip together as sequential Cmds on drop -- for a **single** selected part this is mathematically identical to source (rotating/flipping about a part's own anchor doesn't move it, so composing the translation and the spin in either order lands on the same pose). **Documented simplification:** a **multi-part** selection rotates/flips each part individually about its own anchor instead of the whole group swinging around one shared pivot the way source's `ROTATE_AROUND_SEL_CENTER`-style group rotation would. **Documented gap:** if R/F is pressed before the mouse has moved even once during a *click-drag* (not the M-armed path, which has no such window), there's no preview yet to attach the rotation to and the keypress is dropped -- `useActionRunner.ts`'s `tryTransformDuringMove` can only see `state.movePreview`/`state.activeTool`, not `Canvas.tsx`'s own pending-drag ref |
| Move: connected track ends follow the dragged footprint | **does not happen in source either** -- task premise corrected after reading `edit_tool_move_fct.cpp:doMoveSelection` directly: a plain (non-router) Move only ever does `item->Move(movement)` on the selection itself; it never touches a connected-but-unselected track. What source *does* do instead is redraw a live/dynamic ratsnest during the drag (`PCB_ACTIONS::updateLocalRatsnest`) | ported: `kicad-port/localRatsnest.ts:offsetRatsnestForPreview` shifts ratsnest edges touching a moving part's pads by the live preview delta, so the airwire updates every frame instead of only after the move commits and `/api/ratsnest` is re-polled |
| Router-driven drag (`D`, `pcbnew.InteractiveRouter.Drag45Degree`) of a track segment/corner or via -- collision-aware, keeps connections live via the full interactive router | **done this session** (gap #7 stage 5's frontend -- `crates/pns::dragger::Dragger` already existed; this session is wiring it in) | `router_tool.cpp:InlineDrag`/`CanInlineDrag` -- `components/canvas/dragging.ts` (`findDraggableAt`: approximates `ACTIONS::selectionCursor` + `NeighboringSegmentFilter` with the existing click-select hit-test, restricted to track/via, falling back to a lone selection; `startInlineDrag`/`finishInlineDrag`), `kicad-port/dragTool.ts` (pure preview-merge glue, mirrors `routeTool.ts`), `Canvas.tsx` (pointerdown commits, pointermove previews -- reuses the route tool's own throttle/request-guard refs since the two sessions are mutually exclusive), `useActionRunner.ts` (the `D` hotkey itself: a one-shot grab-and-go, not a toggle-arm like `X`), `painter.ts:drawInProgress`'s new `"drag"` branch (dashed live preview; a via's own attached tracks -- `fanout` -- render too, not just the via). **Scope, matching `dragger.rs`'s own documented simplifications** (see `crates/pns/PARITY.md`): corner-drag only (no segment-sideways-slide), free-angle (not 45-degree-constrained), and -- per `CanInlineDrag`'s own footprint branch being out of scope for this port's single mode -- **not** a footprint drag (only a track/via); a footprint `Move` stays the plain, non-router kind the row above already covers (confirmed, not a gap, by reading source directly) |
| Router-driven drag of a **footprint** (so its attached tracks follow via the router, distinct from the row above) | **not implemented** -- `dragger.rs` only models `DM_CORNER`/via dragging, matching the task's explicit single-item-drag scope; `CanInlineDrag`'s footprint branch (`!(aDragMode & DM_FREE_ANGLE)`) has no backend counterpart to wire | `router_tool.cpp:InlineDrag`'s `footprints` path |
| "Highlight collisions" router mode | **done this session** -- it's not a distinct concept from `Mode::MarkObstacles`, already fully implemented server-side since gap #7 stage 1 (`router_tool.cpp` literally labels `RM_MarkObstacles` as `"Highlight collisions"` in its own status-bar summary); the real gap was that nothing in the frontend ever let a person select any mode but Walkaround at all -- fixed by the settings dialog below | `router_tool.cpp`'s `RM_MarkObstacles` case, `PCB_ACTIONS::routerHighlightMode` |
| Interactive Router Settings... (`Ctrl+<`) | **done this session, narrower than upstream's own dialog by necessity** -- only `Mode` (Highlight Collisions/Shove/Walk Around) and Remove Redundant Tracks have any real effect in `crates/pns` (confirmed by grepping every other `RoutingSettings` field for a reader outside `settings.rs` itself: none -- `ShoveVias`/`JumpOverObstacles`/`SmartPads`/`SmoothDraggedSegments`/`OptimizeEntireDraggedTrack`/`AutoPosture`/`FixAllSegments`/`AllowDrcViolations` either don't exist on this port's settings struct or exist but are never read), so only those two are real, working controls -- every other upstream field is left out entirely (same "nothing to show, not a bug" convention Board Setup/Zone dialogs already use), except Free Angle Mode, shown **disabled** with its reason rather than omitted since the task asked for it by name and this port's router only ever builds 45-degree traces (a scope decision from gap #7's very first session, not something a checkbox could flip). A setting change here takes effect on the next `X`/`D` session start, not live mid-route -- this app starts a brand-new backend session per route/drag (no persistent one upstream's live dialog could push an update into) | `dialog_pns_settings.cpp` -- `components/RouterSettingsDialog.tsx`, `state.routerSettings` (`state/store.tsx`), `POST /api/route/start`'s new `remove_loops` field and `POST /api/route/drag_start`'s new `mode` field (`crates/cli/src/route_api.rs`) |
| Route Differential Pair (`6`, `pcbnew.InteractiveRouter.DiffPair`) -- two parallel, gap-matched lines from one session | **done this session, significantly narrower than upstream by necessity** -- see `crates/pns/PARITY.md`'s own "Stage 7" section for the full design. Pair detection by net-name suffix (`+`/`-`/`P`/`N`, trailing digits preserved) and gap/width from the net-class diff-pair fields, both exactly as the task asked; routing itself is a direct 45-trace spine offset into two lines (collision-*reporting* only, like a plain route in `mark_obstacles` mode) rather than upstream's own coupled-shove/walkaround `DIFF_PAIR_T` item -- that whole second collision model was out of proportion to the remaining task scope. No via/layer-switch mid-pair-route either (single layer only) | `pns_diff_pair_placer.cpp`, `board.cpp:MatchDpSuffix` -- `crates/pns/src/diff_pair.rs` (8 tests), `crates/model/src/lib.rs`'s `diff_pair_{width,gap,via_gap}_of` (3 tests), `crates/cli/src/route_api.rs`'s `dp_*` endpoints, `components/canvas/diffPairRouting.ts` + `kicad-port/dpTool.ts` (3 tests, mirroring `routing.ts`/`routeTool.ts`), `Canvas.tsx`, `useActionRunner.ts`, `painter.ts:drawInProgress`'s new `"diffpair"` branch |
| Tune Length of a Single Track... (`7`, `pcbnew.LengthTuner.TuneSingleTrack`) -- lengthen a track to a target length by inserting a meander | **done this session, as a dialog rather than upstream's own live mouse-driven session** -- see `crates/pns/PARITY.md`'s own "Stage 8" section. Scoped to a straight, axis-aligned, single-segment track (refused with a message otherwise -- `generate_meander`'s own doc comment explains why diagonal baselines need a different, not-yet-derived formula); collision-*reporting* only, same as the diff-pair router above. **Not implemented**: diff-pair length tuning (`8`) and skew tuning (`9`), the `1`/`2`/`3`/`4` live amplitude/spacing hotkeys (plain dialog number fields instead), and the separate `Ctrl+L` settings dialog | `pns_meander_placer.cpp`, `pns_meander.cpp` -- `crates/pns/src/meander.rs` (9 tests), `crates/cli/src/tune_api.rs`'s stateless `/api/tune_length/{preview,apply}` (reuses `Cmd::CommitRoute`, no new `Cmd`), `components/LengthTuningDialog.tsx` |

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
| Copy/Cut/Paste | Copy + Paste identical in effect; **Cut not implemented** | `common.Interactive.copy`/`paste` -- `Cmd::PasteItems { tracks, vias, zones, shapes, texts }` (new, with tests) inserts fresh copies of whole items carried with the command itself (not references), so paste survives the original being deleted first. `components/canvas/clipboard.ts` converts this app's display shapes to the Cmd's IR shape and holds the result in `state.clipboard`. `common.Interactive.cut` (Ctrl+X) -- trivially "copy then delete" -- was not wired this session; still shows "not ported yet" |
| Footprints | **explicitly out of scope, needs a design decision** | Duplicating a footprint would add a part instance the intent/BOM doesn't have -- `Cmd::Duplicate`'s own doc comment states this; an id naming a footprint just never matches anything duplicable rather than being specially rejected |

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
| Rotation sign | identical effect, bridged the same way | source: `if (!m_Display.m_DisplayInvertYAxis) rotation = -rotation;` before calling `Rotate()` -- negates the dialog's own CCW-positive, user-facing angle to match the CW-positive-in-Y-down rotation matrix underneath. This app's dialog does the same negation, landing on the same visual direction as `state/units.ts`'s existing `toPolar` (also CCW-positive) |

Rust tests: 6 new, covering the no-pivot in-place spin, translate-then-
spin, a shared pivot actually orbiting the part, one shared transform
applied to a whole batch, atomic refusal, and the empty-list refusal.

## 7. Context menus and hotkeys

`useActionRunner.ts`'s zoom-action registry was corrected/extended this
session (zoomIn/zoomOut/zoomFitObjects/zoomCenter/zoomRedraw/
resetLocalCoords -- see section 1). The context menu itself
(`Canvas.tsx:onContextMenu`) wasn't otherwise revisited.

### Hotkey coverage audit

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
| `common.Interactive.cut`/`pasteSpecial`/`copyAsText` | Ctrl+X/Shift+V/Shift+C | `copy`/`paste`/`duplicate` done this session (section 5) -- `cut` (trivially "copy then delete") wasn't wired; `pasteSpecial`/`copyAsText` have no real analogue in this app (no "paste without net/position" variant, no text-serialized clipboard format) |
| `common.Interactive.cycleArcEditMode` | Ctrl+Space | Cycle Arc Editing Mode |
| `common.Interactive.find`/`findAndReplace`/`findNext`/`findPrevious`/`findNextMarker` | Ctrl+F, ... | Find/Replace |
| `common.Interactive.finish` | End | Finish (generic "end the current interactive action") |
| `common.Interactive.measureTool` | Ctrl+Shift+M | Measure Tool |
| `common.SuiteControl.openPreferences` | Ctrl+, | Preferences... (no Preferences dialog in this app) |

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
| Per-zone Fill/Unfill (`pcbnew.ZoneFiller.zoneFill`/`zoneUnfill`, selection-scoped) | missing | only the *All* variants are wired -- this app's `/api/fill` already computes every zone each call, so a selection-scoped fetch would save nothing server-side; the real gap is no fine-grained *display* control (next row) |
| `ZONE_DISPLAY_MODE`: solid fill vs. outline-only, independent of whether fill data exists | 2 of 4 modes ported | `pcb_control.cpp:ZoneDisplayMode` (`PCB_ACTIONS::zoneDisplayEnable`/`Disable`/`Toggle`) -- `state.zoneDisplayMode`, `painter.ts:drawZones`. Source's other two (`zoneDisplayOutlines` = fracture-borders, `zoneDisplayTesselation` = triangulation) are developer debug views of the filler's internal geometry, not ported. A zone with no fill data at all (never filled, or just Ctrl+B'd) always shows its outline regardless of this mode, matching source (nothing to fill with) |
| Filled-zone rendering: each fragment (island) as its own closed, already-`Fracture`d ring | identical | `pcb_painter.cpp`'s zone-fill paint (polygon fill, no separate even-odd holes pass needed) -- `crates/cli/src/studio.rs:fill_json`'s own doc on why fragments are pre-fractured |
| Selected zone, filled mode | simplified | source's selection-shadow layer isn't modeled; this app strokes the zone's own (unfractured) outline in the selection color on top of the solid fill instead -- same visual intent, different mechanism |
| Zone Properties dialog: net, layer, clearance, min width, priority | identical | `panel_zone_properties.cpp`'s `TransferZoneSettingsToWindow`/`AcceptOptions` -- `ZoneDialog.tsx` |
| Pad connection (Solid/Thermal/PTH-only-thermal/None), thermal gap + spoke width | identical, including source's own "never disabled by the connection choice" rule for the gap/spoke fields | same file -- `m_PadInZoneOpt`, `m_antipadClearance`/`m_spokeWidth` (explicitly never `Enable(false)`-d, per that file's own comment, since a per-pad override can still need them) |
| Island removal (always/never/below area limit) + area threshold | identical | same file -- `m_cbRemoveIslands`/`m_islandThreshold`, shown only in "Area" mode |
| Fill type (solid/hatched) + hatch width/gap/orientation/smoothing level/smoothing value | identical, shown only when Hatched | same file -- `m_cbHatched`'s `onHatched` enable/disable group |
| Zone name, Locked, corner smoothing (chamfer/fillet) + radius, outline border display style (hatched/full/invisible), per-layer hatch-offset overrides, teardrops | not ported -- no IR field | this model's `Zone` has no `name`/`locked`/corner-smoothing/border-display-style/per-layer-override/teardrop concept at all (single-layer outline + the fill-engine fields only) -- same "nothing to show, not a bug" convention as every other dialog here for a field with no backing data |
| Net picker limited to nets actually present on a pad | identical in spirit, simpler widget | source's `NET_SELECTOR` (searchable combo over the whole board netlist); this app's `<select>` over `state.board.parts[].pads[].net` (unchanged from before this session) |
| Multi-copper-layer zones (one outline, several layers) | not ported -- single-layer `Zone.layer: string` | `ZONE::GetLayerSet()`; this model's zones are one layer each, unchanged scope from before this session |
| Add flow: outline first, then settings (vs. source's settings-first-then-draw) | unchanged, documented simplification from an earlier session | `ZoneDialog.tsx`'s own header comment; the dialog itself is now the full settings panel, not just net/layer |
| Add flow backend shape: `add_zone` (net/layer/outline, unchanged 3-field `Cmd`) + an immediate `edit_zone` only if any setting differs from `Zone::default()` | deliberate 2-Cmd design, not 1 atomic Cmd | keeps `Cmd::AddZone`'s wire shape stable for every existing caller (the CLI, this crate's own tests) -- `state/store.tsx`'s `addZone`, `crates/ops/src/lib.rs`'s `Cmd::EditZone`/`edit_zone` |
| Zone outline editing: drag a corner, double-click an edge to add one, right-click a corner to delete it | ported, scoped down from source -- see section 11 | `pcbnew/tools/pcb_point_editor.cpp` -- `kicad-port/zonePointEditor.ts` (corner/edge hit-testing, 9 unit tests), `Cmd::SetZoneOutline`, `Canvas.tsx`'s onPointerDown/Move/Up + onDoubleClick + onContextMenu, `painter.ts:drawZoneHandles` |
| Rule Areas (keepouts) | ported -- see section 14 | `pcbnew.InteractiveDrawing.ruleArea` |
| Zone Cutout / Similar Zone | missing, unchanged | `pcbnew.InteractiveDrawing.zoneCutout`/`similarZone` |

Rust: `crates/ops/src/lib.rs`'s `Cmd::EditZone`/`edit_zone` (full
`ZONE_SETTINGS` replace, outline untouched; validates clearance >= 0, min
width > 0, thermal spoke >= min width, and -- matching source's own
`AcceptOptions` -- hatch thickness/gap >= min width in hatch mode), 5 new
tests in `crates/ops/src/tests.rs`. `crates/cli/src/studio.rs`'s
`fill_json`'s endpoint was already there; `state()`'s `zones` JSON and
`routing` JSON both gained the new fields (full settings; track/via preset
lists, see section 9).

## 9. Board Setup

Port of `pcbnew/dialogs/dialog_board_setup.cpp`, which is really a tree of
~15 `panel_setup_*.cpp` pages. `BoardSetupDialog.tsx` only builds the pages
this app's constraint model (`crates/model/src/lib.rs` `BoardRules`) has
real data for; a page with no IR backing at all is left out.

A hard split runs through every page: `BoardRules` (net classes, hole/
clearance/text defaults, stackup) lives on the *intent*-derived
`ConstraintModel`, loaded read-only (`crates/cli/src/board.rs::load`) --
there is no `Cmd` that can change it without a second edit/undo path into
the intent file, which this session did not build (GAPS.md #10 sizes that
"L", same size the custom-rule-language page would be). Only the new
`RoutingSection.track_width_presets`/`via_presets` live on the editable
`design.json` IR, so only that one page is genuinely editable -- every
other page is a read-only mirror.

| Page | Status | KiCad file |
|---|---|---|
| Net Classes: name, net-pattern list, track width, clearance, via size/drill, priority | read-only (no edit command -- see this section's intro) | `panel_setup_rules.cpp`'s net-class grid (the pattern-assignment side of it; `dialog_copper_zones.cpp`'s own net-class picker is the same gap) |
| Track Widths & Vias: the W/Shift+W and via-size-cycle preset lists, add/remove entries | editable | `panel_setup_tracks_and_vias.cpp` -- `Cmd::SetTrackWidthPresets`/`SetViaPresets` (whole-list replace, no per-entry Cmd, same spirit `paste_items` already uses for several items in one commit) |
| Design Rules: the custom per-net/per-item constraint expression language | not ported -- no IR concept at all | `panel_setup_rules.cpp`'s actual subject (a small expression language over `DRC_ENGINE::EvalRules`) -- shown instead: the board-wide numeric defaults `eda_drc` does check (clearance, track width, annular ring, hole-to-hole, hole clearance, silk clearance), read-only |
| Text & Graphics Defaults: refdes font size, minimum silk text height/thickness | read-only | `panel_setup_text_and_graphics.cpp` |
| Layer Stackup: name/material/thickness per layer | read-only, and usually empty (most intents never set one) | `panel_setup_layers.cpp` -- `crates/model/src/lib.rs` `Stackup`/`StackupLayer` already existed on `ConstraintModel`, just never exposed in `/api/state` before this session |
| Constraints / Teardrops / Tuning Patterns / Mask & Paste / Formatting / Zones defaults / Severities | not ported -- no IR concept | no model field for any of these; left out entirely rather than faked |

W/Shift+W (`pcbnew.EditorControl.trackWidthInc`/`Dec`) and the via-size
cycle (`viaSizeInc`/`Dec`) now read this page's lists -- `useActionRunner.ts`:
cycling updates `state.currentTrackWidthUm`/`currentViaPreset` (read by
Canvas.tsx's route/via tools for the *next* item) and, matching source's
own dual-purpose behavior, also applies the new size to every selected
track/via in the same keypress via `set_track_width`/`edit_via`.

Rust: `crates/cli/src/studio.rs`'s `state()` gained `board_rules.
net_classes`/`stackup`/the hole-clearance-and-text-default fields (plain
JSON exposure, no new endpoint) and `routing.track_width_presets`/
`via_presets`.

## 10. Property dialogs (GAPS.md #11)

| Dialog | Status | KiCad file |
|---|---|---|
| Footprint Properties: refdes text placement (above/below/left/right) | editable (new) | `pcb_properties_panel.cpp`'s sibling -- `Cmd::SetLabelSide`, a dropdown in `FootprintPropertiesDialog.tsx` |
| Footprint Properties: Reference/Value/Footprint/MPN | still read-only | these live on the *intent*-derived model (`crates/model/src/lib.rs` `Part`, the BOM), not the editable `design.json` IR -- same model-split reasoning as Board Setup's Net Classes page (section 9) |
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
| Right-click context menu built from the selection (GAPS.md #30) | partial: Copy/Cut/Duplicate/Move Exactly/Align/Distribute added to the existing static list, and a real per-kind Delete (`common.Interactive.delete`) replaces a footprint-only `ripSelection()` call that silently did nothing for a track/via/zone/shape/text right-click before this session | `Canvas.tsx:onContextMenu` -- still not source's fully dynamic per-item-type tool menu (GAPS.md #30's larger ask), but a meaningfully less-static list than before |
| Zone outline editing: drag a corner, double-click an edge to add one, right-click a corner to delete it | ported | `pcbnew/tools/pcb_point_editor.cpp` -- `kicad-port/zonePointEditor.ts`, `Cmd::SetZoneOutline` (`crates/ops`, 1 new test), `Canvas.tsx`, `painter.ts:drawZoneHandles`. Scope, matching this app's existing point-editor-less baseline rather than a full port: a single selected zone only (no multi-select point editing, no graphic-shape point editor either -- see section 10's Shape Properties row); no 45/90-degree edge-angle constraint while dragging a corner (source's Ctrl-held behavior); no "equal length" guide overlay; corner drag snaps to the plain grid only, same as every other zone/route/shape placement click in this app (`gridHelper.ts:snapPoint`, not the anchor-aware `snapWithAnchors` the Move tool uses) |
| Array tool, grouping, dimensioning | missing, unchanged | GAPS.md #26/#27/#28 -- out of scope this session |

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
flags (task item 3) and `pcbnew/drc/drc_test_provider_disallow.cpp`'s
keepout half. A rule area shares `Zone`'s own IR struct, outline-drawing
tool and properties dialog with a copper-pour zone -- same single-dialog-
two-panels shape source itself uses -- rather than being a separate type.

| Behavior | Status | KiCad file:function |
|---|---|---|
| IR: `is_rule_area` + 5 `keepout_*` flags (tracks/vias/pads/copper pours/footprints) on `Zone`, additive | identical | `eda_model::ir::Zone` -- `ZONE::GetIsRuleArea`/`GetDoNotAllow{Tracks,Vias,Pads,ZoneFills,Footprints}` |
| A zone/rule area may have no net at all (KiCad's net code 0) | identical | `add_zone`/`edit_zone` no longer require `known_net` for an empty string -- previously impossible to create, needed for every real-world keepout |
| Drawing: outline-drawn with the same tool as a copper-pour zone | ported, one hotkey/menu difference from source rather than a separate tool -- `pcbnew.InteractiveDrawing.ruleArea` (Ctrl+Shift+K) arms the identical outline tool as `.zone`; `state.nextZoneIsRuleArea` is the one bit telling `ZoneDialog.tsx` to pre-check "Rule area" for *this* entry's outline, same end result (draw outline, dialog opens, Rule Area already ticked) with no second drawing-tool code path to maintain | `tools/drawing_tool.cpp`'s zone/keepout entry points, which upstream also funnel through one outline-drawing loop |
| Properties dialog: "Rule area" checkbox swaps the panel between fill settings and the 5 keepout checkboxes | identical in effect | `dialog_copper_zones.cpp`'s `IsRuleArea()` branch -- `ZoneDialog.tsx` |
| Zone filler honors a copper-pour keepout: every other zone's fill excludes it, any net, any priority | identical in effect | `ZONE_FILLER::fillCopperZone`'s keepout knockout -- `crates/zone-filler`'s new `FillInput::keepouts`/`FillKeepout`, 2 new tests. Previously an explicit, named gap in that crate's own doc comment ("Not ported: ... keepout zones") |
| DRC: track/via/pad/footprint landing inside a matching keepout is reported (`items_not_allowed`) | ported for the explicit-keepout-zone path; a track/via/pad is layer-matched exactly, a footprint ignores the keepout's own layer (a component exclusion zone's layer is cosmetic -- see below) | `drc_test_provider_disallow.cpp`'s `DISALLOW_CONSTRAINT`/`antiTrackKeepouts` paths -- new `crates/drc/src/providers/disallow.rs`, 8 tests |
| DRC: copper-pour-in-keepout cross-check (belt-and-suspenders over the filler's own exclusion) | ported, with the same epsilon-deflate trick source's own `query_areas` uses -- otherwise the filler's own zero-gap cut would always register as "touching" and false-positive | `drc_test_provider_disallow.cpp`'s `query_areas` cached-intersection test |
| Multi-layer rule areas (one outline, several layers); custom `(disallow ...)` DRC rules on non-keepout items; `DRCE_TEXT_ON_EDGECUTS` (a different, unrelated half of the same KiCad source file) | not ported -- this model's `Zone` is single-layer only (an existing, documented limitation predating this item), has no custom-rule language (GAPS.md #10), and text-on-Edge.Cuts is a separate check | `ZONE::GetLayerSet()`, `panel_setup_rules.cpp`, `drc_test_provider_disallow.cpp`'s `checkTextOnEdgeCuts` |
| `.kicad_pcb` export of a rule area | not ported -- `crates/kicad`'s exporter was out of this item's explicit scope; a rule area round-tripped through export today would still be written as (and read back as) a zone, net/fill fields included, which is not what a real `.kicad_pcb`'s `(zone (keepout ...))` block looks like | `crates/kicad/src/pcb.rs`'s zone writer -- flagged here for a follow-up, not silently dropped |
| Canvas rendering: a rule area draws as a dashed outline with a diagonal hatch and a restriction label ("Keepout: Tracks/Vias"), never a solid fill (it never has one) | ported, a simplified stand-in for source's real cross-hatch keepout rendering | `pcb_painter.cpp`'s zone paint, keepout branch -- `painter.ts`'s new `drawRuleArea` |

Rust: `crates/model/src/ir.rs` (`Zone`'s 6 new fields), `crates/ops/src/lib.rs`
(`Cmd::EditZone` carries them; `add_zone`/`edit_zone` allow an empty net),
2 new `crates/ops/src/tests.rs` tests. `crates/zone-filler/src/lib.rs`
(`FillKeepout`, the knockout pass), 2 new tests. `crates/drc`: `DrcKeepout`
(`board.rs`, kept entirely separate from `DrcZone`/`board.zones` so every
*other* existing provider -- `copper_clearance` in particular -- keeps
treating `board.zones` as "real copper only", unaffected by rule areas
coming into existence as a concept); `ErrorType::ItemsNotAllowed`
(`item.rs`, appended, matching KiCad's own `items_not_allowed` settings
key); the new `providers/disallow.rs` (8 tests); one new line in `lib.rs`'s
`run()` registering it. Frontend: `RuleAreaFields`/`Zone`/`CmdZone` in
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
| `.kicad_pcb` export/import of a teardrop zone | not ported, same documented gap as section 14's rule areas | `crates/kicad/src/pcb.rs` |

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

Port of `common/tool/group_tool.cpp` and the entered-group half of
`pcbnew/tools/pcb_selection_tool.cpp` (task item 5): group/ungroup
(Ctrl+G / Ctrl+Shift+G), selecting a group as a single unit, and
entering/leaving one. A group is its own IR type (`Group`: id, name,
`member_ids`), not a special item flag on something else -- matching
`PCB_GROUP` being a real, separate item type upstream too.

| Behavior | Status | KiCad file:function |
|---|---|---|
| IR: `Group { id, name, member_ids }`, additive, stored on `DrawingsSection` | identical in shape; stored on `DrawingsSection` purely for the lowest construction-site ripple (see `Group`'s own doc) -- a member id can name a part, track, via, zone, shape or text, not just a drawing | `pcbnew/pcb_group.h`'s `PCB_GROUP` |
| Ctrl+G: group the current selection (2+ items) | identical in effect | `group_tool.cpp`'s `Group()`, `ACTIONS::group.Enable(selectionCount >= 2)` |
| Ctrl+Shift+G: ungroup, releasing members in place | identical in effect | `group_tool.cpp`'s `Ungroup()` |
| Grouping a selection that includes an existing group flattens that group's members into the new one instead of nesting | ported as the model's defined behavior, not a compatibility shim -- this IR has no group-of-groups concept at all (`EDA_GROUP` allows nesting upstream; out of scope here) | `eda_model::ir::Group`'s own doc, `eda_ops`'s `group_items` |
| An item belongs to at most one group; joining a new group (or `AddToGroup`) silently leaves whatever group it was already in; a group left with fewer than 2 members is dissolved | identical in effect | `GROUP_TOOL::Group`/`AddToGroup`/`RemoveFromGroup`'s own `if (group->GetItems().size() < 2) group->RemoveAll()` rule |
| `AddToGroup`/`RemoveFromGroup` as `Cmd`s | ported in the ops layer (both tested) but not reachable from the UI -- no context-menu entry or hotkey calls them; only `Group`/`Ungroup` (hotkeys) and whole-group selection/dissolution are wired today | `ACTIONS::addToGroup`/`removeFromGroup` -- normally a context-menu-only pair upstream too, just not ported here |
| Selecting any member selects the whole group instead (unless you're inside it) | ported as one substitution choke point in the `SET_SELECTION` reducer case (`withGroupSubstitution`, `state/store.tsx`) rather than touching every selection call site in `Canvas.tsx` | `PCB_SELECTION_TOOL::SelectPoint`'s own promote-to-top-level-group step |
| Entering a group (double-click a single selected group) lets you select/edit its members directly | ported: `Canvas.tsx`'s `onDoubleClick` checks the clicked/selected id against `board.drawings.groups` (by own id or membership) and dispatches `SET_ENTERED_GROUP` instead of opening properties, mirroring source's `m_selection.GetSize() == 1 && Type() == PCB_GROUP_T -> EnterGroup()` ordering exactly | `pcb_selection_tool.cpp`'s `Main()` dblclick handler, `EnterGroup()` |
| Leaving a group: Escape | ported as a new tier in the existing `ESCAPE` reducer case, slotted exactly where source puts it -- selection-non-empty still wins first, then entered-group-exit, then idle net-highlight-clear last | `pcb_selection_tool.cpp`'s `IsCancel()` handler, `ExitGroup()`'s `aSelectGroup` default (re-selects the group) |
| Leaving a group: click/click-elsewhere outside its bounding box; explicit "Leave Group" context-menu action | not ported -- `common.Interactive.groupLeave` exists and is correctly wired in `useActionRunner.ts` (dispatches the same re-select-the-group behavior Escape now also triggers) but nothing in the UI calls it yet, since there's no group entry in the right-click menu; Escape is the only exit path today | `pcb_selection_tool.cpp`'s bounding-box auto-exit check, `PCB_ACTIONS::groupLeave` |
| Visual: dashed bounding-box outline around a selected group | ported, a simplified stand-in for source's real selection-halo rendering | `painter.ts`'s `drawSelectedGroups` (`boundsOfPoints` over every member's own points) |
| Visual: "entered group" dimming/overlay of everything outside it; a named group's own label | not ported -- entering a group changes selection/selectability semantics only, with no distinct rendering of the entered state yet | `pcb_selection_tool.cpp`'s `m_enteredGroupOverlay`, `pcb_painter.cpp`'s group name paint |
| Group-aware move/rotate/flip/delete (acting on every member as a unit when the group itself is "selected") | not ported -- today's whole-group "selection" is a set of individual member ids under the hood (via substitution at read time), and the existing per-kind edit `Cmd`s have no group-aware bulk path; moving/rotating/deleting "a group" in this app means doing so to each member id already present in `state.selection`, not a single group-level operation | `GROUP_TOOL`'s interaction with `EDIT_TOOL`/`PCB_ACTIONS::move` et al. acting on `PCB_GROUP` as one item |
| Nested groups (a group containing another group) | not ported, by design -- see the flattening row above | `EDA_GROUP`'s recursive member list |
| `.kicad_pcb` export/import of a `(group ...)` block | not ported, same documented gap as sections 14-15 | `crates/kicad/src/pcb.rs` |

## 17. Create Array

Port of `pcbnew/tools/array_tool.cpp` (`ARRAY_TOOL::CreateArray`,
`pcbnew.Array.createArray`, Ctrl+T) and `include`/`common/array_options.cpp`'s
`ARRAY_GRID_OPTIONS`/`ARRAY_CIRCULAR_OPTIONS` geometry (task item 6).
`ARRAY_OPTIONS` is a dialog-session object in source too -- never written
to the board file -- so this ships as a `Cmd` payload (`ArrayGeometry`),
not a new IR field.

| Behavior | Status | KiCad file:function |
|---|---|---|
| Grid geometry: Nx/Ny, X/Y spacing, X/Y offset (an oblique/skewed grid), centred-on-original vs corner-anchored, stagger (brick/honeycomb pattern) every Nth row or column | identical | `ARRAY_GRID_OPTIONS::GetTransform`/`gtItemPosRelativeToItem0`/`getGridCoords` |
| Circular geometry: centre, point count, angle between points (typed directly, plus a "divide evenly" checkbox that pre-fills 360/count -- a dialog-only convenience, same as source's own `calculateCircularArrayProperties`), angle offset, clockwise/counterclockwise, "rotate items" (spin each item in place as well as moving it along the circle) | identical in effect | `ARRAY_CIRCULAR_OPTIONS::GetTransform` |
| "Duplicate" (default): create `size - 1` new copies of each resolved track/via/zone/shape/text; the *original* item is also transformed, into the array's own last slot -- not left at slot 0 -- so a centred array's slots are evenly occupied | identical in effect | `ARRAY_TOOL::CreateArray`'s reverse `ptN` loop, `TransformItem` |
| "Arrange selection": reposition the given items into the array's own slots instead of creating anything; a placed part is a valid target here (only `Cmd::MoveExact`-style `set_pose`, nothing created) | identical in effect, including that an id naming nothing arrayable is skipped *without* consuming a slot (source's own inner `selectionIndex` cursor advancing independently of the outer `arrayIndex`) | `ARRAY_TOOL::CreateArray`'s `ShouldArrangeSelection()` branch |
| A track/via/zone/shape only ever *translates* in a circular array, never spins in place, even with "rotate items" on; a `Part` or `Text` does spin (the two kinds with a simple scalar position+orientation this model can rotate without a generic per-point shape-rotation capability) | ported with this one explicit narrowing -- see `ArrayGeometry::Circular::rotate_items`'s own doc | `TransformItem`'s unconditional `aItem.Rotate(aItem.GetPosition(), transform.m_rotation)` -- source rotates every kind the same way, since every `BOARD_ITEM` has a generic `Rotate()` |
| A multi-point item (a track or a zone) is translated rigidly by reading just its first point as the position `GetTransform` expects, same as a via/text's single point -- its own shape is never otherwise altered | ported, a model-shape adaptation rather than a behavior gap (same "one position, whole item moves together" result `PCB_TRACK`/`ZONE`'s real `GetPosition()`-based move already has) | `BOARD_ITEM::Move` |
| Validation: a grid needs 1+ rows/columns and a nonzero spacing wherever there's more than one of them; a circular array needs 1+ points and a nonzero angle wherever there's more than one | identical | `DIALOG_CREATE_ARRAY::TransferDataFromWindow`'s own zero-delta checks |
| Arraying a footprint (placed part), in either mode other than a plain "Arrange selection" reposition | not ported -- `FootprintInstance::id` *is* its schematic symbol's id (see that struct's own doc); there is no "conjure a new placed copy" operation this model's ops layer has, the same reason `Cmd::Duplicate` already excludes footprints. Source has no such restriction (a `PCB_FOOTPRINT` duplicates like anything else) | `ARRAY_TOOL::CreateArray`'s `PCB_FOOTPRINT_T` branch |
| Grouping all of one array "block" together automatically (`PCB_GROUP_T`/`PCB_GENERATOR_T` deep-duplication, so an arrayed sub-assembly stays one unit) | not ported -- a group id in `ids` is skipped the same way an unknown id is (no group-aware duplicate/move yet, PARITY-pcb.md section 16) | `ARRAY_TOOL::CreateArray`'s `PCB_GROUP_T`/`PCB_GENERATOR_T` branches |
| Footprint reannotation ("Unique references" -- assign fresh R/C/U numbers to the new copies) | not ported, and not a scope gap so much as a structural mismatch -- see the footprint row above; a PCB-side "rename this reference" has nothing to write without a matching schematic symbol to rename too | `BOARD_REANNOTATE_TOOL::ReannotateDuplicates`, `m_radioBtnKeepRefs`/`m_radioBtnUniqueRefs` |
| Footprint-editor pad numbering: "Renumber pads" checkbox (grid) / always-on (circular), the `ARRAY_AXIS` numeric/hex/alphabetic-minus-IOSQXZ/full-alphabetic numbering schemes, `ARRAY_PAD_NUMBER_PROVIDER`'s skip-already-used-numbers logic, 2D (primary+secondary axis) numbering | not ported -- source's own `enableArrayNumbering = m_isFootprintEditor` means this entire half of the dialog never shows in the board editor either; this app's footprint editor (`LibraryFootprint`'s pads) has no multi-pad array/selection tooling of its own yet to extend with it (it does already have a simpler, non-array `Cmd::RenumberPads` -- reading-order renumber, no scheme/skip logic -- for the existing "Renumber Pads" tool, a different KiCad feature) | `DIALOG_CREATE_ARRAY`'s numbering panels, `array_pad_number_provider.cpp`, `include/array_axis.h` |
| Interactive "select centre point/item" buttons for the circular centre | not ported -- the dialog's centre fields are plain numeric inputs, pre-filled with the selection's own average reference point, same numeric-entry convention every other dialog in this app uses (no canvas-picker mode) | `DIALOG_CREATE_ARRAY::OnSelectCenterButton`, `PCB_PICKER_TOOL` |
| A live canvas preview of the array before committing | not ported -- same "fill the form, Create, see the result" shape every dialog in this app other than section 12's cleanup preview already has | n/a -- source has no such preview either; this row exists only to note it was considered |

Rust: `crates/ops/src/lib.rs`'s `ArrayGeometry` (`Grid`/`Circular`, a `Cmd`
payload type, not IR) and `Cmd::CreateArray`, `create_array`/
`arrange_into_array`/`duplicate_into_array`, 10 new tests. No IR/model
changes. Frontend: `ArrayGeometry` in `api/types.ts`,
`CreateArrayDialog.tsx`, `pcbnew.Array.createArray` wired in
`useActionRunner.ts` (gated on a non-empty selection, matching source's
own `if (selection.Empty()) return 0;`) and a "Create Array... (Ctrl+T)"
context-menu entry in `Canvas.tsx`.

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
| Rotate/Flip (as a generic `BOARD_ITEM`, e.g. if ever added to a future Create Array/rotate-selection path) | not ported -- no `Cmd::RotateDimension`/flip; `crates/connectivity::dimension::rotate_dimension` exists and is tested but has no caller yet, same "built, not yet wired to a UI action" gap teardrops' settings page didn't have but groups' `AddToGroup`/`RemoveFromGroup` do (section 16) | `PCB_DIMENSION_BASE::Rotate`/`Flip`/`Mirror` |
| Interactive placement: Aligned/Orthogonal's third "set height" click with a live crossbar preview; Radial/Leader/Center's own click sequences | simplified to one plain two-click (start, end) placement for every kind, with `height`/`leader_length`/orientation filled from Board Setup defaults and the just-created dimension's own Properties dialog opening immediately for fine-tuning -- a deliberate scope reduction (no live multi-step canvas preview for this item), not a missing capability: every field the live click sequence would have set is still reachable, just numerically instead of by dragging | `tools/drawing_tool.cpp::DrawDimension` |
| Interactive centre-point/-item picker buttons for Radial's own dialog-free flow | not ported -- plain numeric X/Y fields instead, same convention every other dialog in this app already uses | n/a (this port has no equivalent interactive picker tool) |
| Selecting a dimension (click, box-select, the Selection Filter's own toggle) | ported -- `components/canvas/selectionCandidates.ts` gained a `"dimension"` kind, hit-tested against the closest of its own `lines` segments (a disconnected set, not one polyline) or its text anchor | `pcb_selection_tool.cpp`'s generic item iteration, extended to this new kind |
| `.kicad_pcb` export/import of a dimension | not ported, same documented gap as sections 14-17's own new item kinds | `crates/kicad/src/pcb.rs` |

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
`AddToGroup`/`RemoveFromGroup`, `group_items`/`ungroup_items`/`add_to_group`/
`remove_from_group`/`prune_empty_groups`, 6 new tests in
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
