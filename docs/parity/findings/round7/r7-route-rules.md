# Round 7 findings: router, zones, rules, DRC, connectivity and Board Setup (`r7-route-rules`, 2026-10-28)

Read-only review. The code was read from main's checkout (`/home/team_braneboard_com/work/agentic-eda`, at f057b4ea). KiCad is `/home/team_braneboard_com/src/kicad-src-8303b2ad`, and `kicad-src-25ec2335` where the box's kicad-cli is newer (the DRC item list, via stitching). Nothing was built or run: main's `target/debug/eda` is missing, so every finding comes from reading the code against KiCad's C++. Each finding names the test that would show it.

Already-known items were skipped: GAPS (items 3, 7, 11, 13, 26, 36), `crates/pns/PARITY.md` Stages 18-20, PARITY-pcb sections 9, 59 and 63, and rounds 1-6 under `docs/parity/findings/`. R6-R8 is still open and is not repeated here.

The file has three sections:
- **R**: the router;
- **D**: rules, DRC and Board Setup;
- **Z**: zones, connectivity and Cleanup.

Each section ends with what was checked and found to match. After them come the convergence summary and the summary table.

---

## Section R: the router and its studio side

Read-only. Code read from main's checkout (`/home/team_braneboard_com/work/agentic-eda`, at f057b4ea). KiCad: `/home/team_braneboard_com/src/kicad-src-8303b2ad` (`pcbnew/router/router_tool.cpp`, `pns_line_placer.cpp`, `pns_diff_pair_placer.cpp`, `pns_router.cpp`, `pns_kicad_iface.cpp`, `pcbnew/layer_pairs.cpp`, `pcbnew/sel_layer.cpp`, `pcbnew/tools/pcb_control.cpp`, `common/tool/tool_manager.cpp`). Nothing was built or run.

Scope:
- (a) the round-6 router merges: d3af3fce (R6-R1, V's via size, `chosenViaOf`), f365ded0 (R6-R2, the board-minimum width), 9a88f288 (R6-R3 layer keys and `LINE_PLACER::SetLayer`, R6-R4 Autoroute through `ROUTER::Finish`, R6-R5 `m_startWithVia`, R6-R6 `syncRouterAndFrameLayer`);
- (b) router behaviours still not ported: what `performRouting`'s event loop does with Undo, Delete, Redo, Properties and the zone-fill actions; the pair tool's share of the round-6 fixes;
- (c) rows no round reviewed: Set Layer Pair (`SelectCopperLayerPair`, `LAYER_PAIR_SETTINGS`), Shift+V (`CycleLayerPresets`), the router-mode and corner-mode actions.

### High

#### R7-R1. Ctrl+Z, Delete and Redo while a route is drawn (or a track dragged) act on the board; KiCad takes back the last segment, or does nothing

- **What differs.**
  - Ours:
    - `web/studio/src/actions/useActionRunner.ts:692-700` (`common.Interactive.undo`). The only exceptions are a board Move, a zone or polygon with a corner placed, and the drawing tools (`kicad-port/drawingCancel.ts:16-21`, `BOARD_DRAWING_KINDS = shape, zone, dimension, table, measure, text`). A route (`drawState.kind === "route"`), a pair (`"diffpair"`) or a drag (`"drag"`) is none of these, so Ctrl+Z goes to `api.undo()`. That undoes the board's last step, with the route session still open on the server.
    - `crates/cli/src/studio.rs:952-958` (`POST /api/undo`) does not look at the route session.
    - `useActionRunner.ts:373-392` (`common.Interactive.delete`): while routing, Delete is `deleteRefs( requestSelection() )`. `requestSelection` (`:185-200`) falls back to the item under the cursor, so Delete removes the selection or the pad, track or via the head is over.
    - Redo (`:701-706`, `redoRefused`) and Fill / Fill All / Unfill / Unfill All (`:2440-2443`) are refused only for `BOARD_DRAWING_KINDS`. So they run while a route, a pair or a drag is live.
  - KiCad:
    - `ROUTER_TOOL::prepareInteractive` blocks the frame's undo and redo: `frame()->UndoRedoBlock( true );` (`router_tool.cpp:1486`). `finishInteractive` lifts the block (`:1504`). `InlineDrag` / `performDragging` set the same block (`:2158`, `:2624`, `:2912`), and `PCB_BASE_EDIT_FRAME::RestoreCopyFromUndoList` / `...RedoList` return at once while it is set (`undo_redo.cpp:227`, `:265`: `if( UndoRedoBlocked() ) return;`).
    - `performRouting`'s loop takes Undo and Delete as Undo Last Segment:
      ```cpp
      else if( evt->IsAction( &PCB_ACTIONS::routerUndoLastSegment )
                  || evt->IsAction( &ACTIONS::doDelete )
                  || evt->IsAction( &ACTIONS::undo ) )
      {
          if( std::optional<VECTOR2I> last = m_router->UndoLastSegment() )
      ```
    - The zone-fill actions ring the bell:
      ```cpp
      else if( ZONE_FILLER_TOOL::IsZoneFillAction( evt ) )
      {
          wxBell();
      }
      ```
  - So in our studio the board changes under a live route. The router's world and its displaced tracks (`remove_track_ids`) are from before the undo. The route's commit (`Cmd::CommitRoute`) then lands on the undone board. A track the undone step had removed can come back as a shoved copy, and removes are issued for ids that no longer exist. A Delete while routing deletes copper the user never meant to touch.
- **How to see it.**
  1. Route A from R1.1 to R2.1 and finish.
  2. Start route B on another net, click once to fix a segment, and press Ctrl+Z.
     - KiCad: B's fixed segment is taken back, and route A stays on the board.
     - Ours: route A is undone while B is still drawn. Finishing B commits B against a world that still has A, so in Shove mode A's shoved replacement comes back.
  3. Start a route, hover a pad of another net, and press Delete.
     - KiCad: the last segment is taken back.
     - Ours: the hovered item is deleted (one undo step), and the route goes on.
  - Test (studio, `drawingCancel.test.ts` or a new `routerKeys.test.ts`): `undo_and_delete_while_routing_take_back_the_last_segment`.
    - With `drawState = { kind: "route", .. }`, running `common.Interactive.undo` and `common.Interactive.delete` calls `routeUndoSegment` and never `api.undo` / `deleteRefs`.
    - With `kind: "drag"`, Ctrl+Z calls neither.
    - With `kind: "route" | "diffpair" | "drag"`, `pcbnew.ZoneFiller.zoneFillAll` does nothing.
  - Server guard test (`route_api`): `an_undo_is_refused_while_a_route_session_is_open`.
- **Severity.** High. The board changes under a live session, and the route's commit is computed against a world the undo has changed (wrong connectivity, data loss with Delete).
- **Already known?** No. CODE-COMPARE-ui.md:104 marks `UndoLastSegment` "UNVERIFIABLE", and no file lists Undo, Delete, Redo or the fill actions while routing.

#### R7-R2. A differential pair's vias ignore the chosen via size (Pre-defined Sizes entry or custom size): R6-R1's fix covers the single track only

- **What differs.**
  - Ours: the pair is started with no via size (`web/studio/src/api/client.ts:695` `dpStart`: width, gap and via gap only). Its V (`useActionRunner.ts:503-509` -> `components/canvas/diffPairRouting.ts:35-41` `toggleDiffPairVia` -> `dpToggleVia( enabled, toLayer )`) sends no diameter and no drill. `crates/pns/src/dp_placer.rs:495-505` keeps `self.sizes.via_diameter` / `via_drill` from the start's `import_sizes`, which are the rules' (`dp_placer.rs:136`). So with the via list on entry 2, or custom sizes on, the pair's vias are still the net class's.
  - KiCad: the pair placer makes its vias from the router's sizes (`pns_diff_pair_placer.cpp:78` `VIA v( aP, layers, m_sizes.ViaDiameter(), m_sizes.ViaDrill(), aNet, m_sizes.ViaType() );`). Those sizes come from:
    - `ImportSizes` at the start: `if( bds.UseNetClassVia() && aStartItem ) { .. } else { viaDiameter = bds.GetCurrentViaSize(); viaDrill = bds.GetCurrentViaDrill(); }`;
    - `handleLayerSwitch` on V: `else { sizes.SetViaDiameter( bds.GetCurrentViaSize() ); sizes.SetViaDrill( bds.GetCurrentViaDrill() ); }`.
  - Both are mode-independent (the pair goes through the same `performRouting` and `onViaCommand`).
- **How to see it.**
  - Setup: Board Setup > Pre-defined Sizes has a via of 0.8 / 0.4 mm. Choose it in the via-size menu (Default class via 0.6 / 0.3 mm). Route the `USB_D+` / `USB_D-` pair (6), press V, then click.
  - KiCad: two 0.8 / 0.4 mm vias. Ours: two 0.6 / 0.3 mm vias.
  - Test: (studio) `a_pair_via_takes_the_chosen_via_size` -- with `currentViaIndex = 1`, `toggleDiffPairVia` sends `chosenViaOf("through", viaChoiceOf(state))`. (server, `route_api`) `dp_via` with `diameter: 800_000, drill: 400_000`, then `dp_fix`; the pair's committed vias are 0.8 / 0.4 (the server side already takes them, `dp_placer.rs:498-503`).
- **Severity.** High (by this review's scale: the wrong via size is committed), but only with a list entry or custom size chosen.
- **Already known?** Partly. GAPS item 7 (Stage 11) lists "the via commands in the pair tool" as left. That item covers the blind, micro and Select Layer commands, not V's size. R6-R1 named only the single track.

### Medium

#### R7-R3. The pair tool has none of R6-R3 / R6-R5: the layer keys while a pair is drawn change only the active layer, and V with the pair tool armed does nothing

- **What differs.**
  - Ours:
    - `web/studio/src/actions/routerViaActions.ts:95-123` registers the router's layer commands only `if (board && draw?.kind === "route")`.
    - `:131-139` registers V-before-a-route only `if (board && !draw && state.activeTool === "route")`.
    - So with a pair drawn, PgUp / PgDn, + / - and `layerInnerN` are the board editor's (`useActionRunner.ts:1018`, `:1933-1964`, `SET_ACTIVE_LAYER`). Then `store.tsx`'s `syncRouterAndFrameLayer` puts the active layer straight back on the next `SET_DRAW_STATE`, since the pair stays on its layer.
    - With the pair tool (`activeTool === "diffpair"`) armed and idle, V has no handler at all.
  - KiCad:
    - `ROUTER_TOOL::MainLoop` is the loop of both modes (`Go( &ROUTER_TOOL::MainLoop, PCB_ACTIONS::routeDiffPair.MakeEvent() )`), so `evt->IsAction( &ACT_PlaceThroughVia )` sets `m_startWithVia` for a pair too, and `performRouting` (both modes) arms the via.
    - `onLayerCommand` -> `handleLayerSwitch` -> `ROUTER::SwitchLayer` -> `DIFF_PAIR_PLACER::SetLayer` (`pns_diff_pair_placer.cpp:437-459`):
      ```cpp
      else if( m_chainedPlacement || !m_prevPair )
          return false;
      else if( !m_prevPair->PrimP() || ( m_prevPair->PrimP()->OfKind( ITEM::VIA_T ) &&
                  m_prevPair->PrimP()->Layers().Overlaps( aLayer ) ) )
      {
          m_currentLayer = aLayer;
          m_start = *m_prevPair;
          initPlacement();
          Move( m_currentEnd, nullptr );
          return true;
      }
      ```
      When it refuses, a pair of through vias to the layer is armed, as for a track.
- **How to see it.**
  - 4-layer board: start a pair on two vias, then press PgDn. KiCad moves the pair to B.Cu; ours flickers the layer selector and stays on F.Cu.
  - Press 6, hover a pad pair, press V, then click. KiCad starts the pair with its vias armed on the pair's other layer; ours starts with no via, and the active layer did not flip.
  - Test (browser, `diff-pairs.check.js`): after `dp_start` on vias, `__eda.run("pcbnew.Control.layerBottom")`, then `drawState.layer === "B.Cu"`. With the pair tool idle, run `pcbnew.InteractiveRouter.PlaceVia`, then `state.pcbx.startWithVia === true` and the active layer is flipped.
  - The server needs `DIFF_PAIR_PLACER::SetLayer` (`dp_placer.rs` has no `set_layer`).
- **Severity.** Medium (missing interaction; no wrong data).
- **Already known?** No. Stage 20 and R6-R3 / R6-R5 name the single track only.

#### R7-R4. Autoroute: a connection `Finish` cannot make, finished by hand, joins the autorouted undo group; KiCad makes it a step of its own

- **What differs.**
  - Ours: `crates/cli/src/route_api.rs:390-394` (`attempt_finish`) sets `router.append_undo = append` before `Router::attempt_finish`. On `Err(end)` (`:405-408`) the session stays with `append_undo` still true, so the user's later `finish` / Esc commit (`route_api::finish`, `commit_route`) calls `board::append_to_previous_undo`.
  - The studio sets `groupStart = true` only for the next connection (`useActionRunner.ts`, `routeSelected`: "`if( !autoRouted ) groupStart = true;`").
  - `crates/pns/PARITY.md` Stage 20 states the opposite as the intent: "(its commit, finished by hand, joins the group too, as KiCad's flags are set before `performRouting`)".
  - KiCad: `performRouting`'s `routerAttemptFinish` branch clears the flag when `Finish` fails during Autoroute (`router_tool.cpp:1587-1613`):
    ```cpp
    if( m_router->Finish() )
    {
        // When we're routing a group of signals automatically we want
        // to break up the undo stack every time we have to manually route
        // so the user gets nice checkpoints. Remove the APPEND_UNDO flag.
        if( autoRouted != nullptr )
            *autoRouted = true;
        break;
    }
    else
    {
        // This acts as check if we were called by the autorouter; we don't want
        // to reset APPEND_UNDO if we're auto finishing after route-other-end
        if( autoRouted != nullptr )
        {
            *autoRouted = false;
            m_iface->SetCommitFlags( 0 );
        }
    ```
- **How to see it.**
  - Select a footprint with three connections: two free, then one blocked so that `Finish` stops short. Autoroute, finish the third by hand, then Ctrl+Z once.
  - KiCad: only the hand-finished route goes. Ours: all three go.
  - Test (`route_api`): `a_hand_finished_autoroute_connection_is_its_own_undo_step`.
    1. Route connection 1 with `attempt_finish { append_undo: false }`.
    2. Route connection 2 with `attempt_finish { append_undo: true }`, against an obstacle, so it is not finished.
    3. Finish connection 2 with `fix` / `finish`.
    4. `undo` once and assert that connection 1's tracks are still on the board.
- **Severity.** Medium (wrong undo step).
- **Already known?** No. R6-R4 was fixed with this reading, and Stage 20 documents it as matching.

#### R7-R5. E (Properties) while routing opens the hovered item's properties; KiCad opens Custom Track/Via Size

- **What differs.**
  - Ours: `useActionRunner.ts:945-957` (`pcbnew.InteractiveEdit.properties`) has no routing branch: `openPropertiesFor( requestSelection()[0] )`, which is the hovered pad's, track's or footprint's dialog, while the route session is open. Applying it edits the board under the session (as R7-R1).
  - KiCad: `performRouting`:
    ```cpp
    else if( evt->IsAction( &PCB_ACTIONS::properties ) )
    {
        frame()->GetCanvas()->SetCurrentCursor( KICURSOR::ARROW );
        controls()->SetAutoPan( false );
        {
            m_toolMgr->RunAction( ACT_CustomTrackWidth );
        }
    ```
    `ACT_CustomTrackWidth` -> `CustomTrackWidthDialog` -> `DIALOG_TRACK_VIA_SIZE`, then `onTrackViaSizeChanged`.
- **How to see it.** Start a route, hover a pad, then press E. KiCad opens the Custom Track/Via Size dialog; ours opens Pad Properties.
  - Test: with `drawState.kind === "route"`, `pcbnew.InteractiveEdit.properties` opens `TrackViaSizeDialog` (`routerViaActions.ts` already has it for Q, `pcbnew.InteractiveRouter.CustomTrackViaSize`).
- **Severity.** Medium (wrong dialog; editing under a live route).
- **Already known?** No.

### Low

#### R7-R6. V-before-a-route (`startWithVia`) outlives the tool: KiCad clears it every time the router tool starts

- **What differs.**
  - Ours: `pcbx.startWithVia` is set by V (`routerViaActions.ts:132-136`). It is cleared only when a route starts from a click (`Canvas.tsx:821-823`). Leaving the tool (Esc, another tool) keeps it, so a route started much later, after X again, starts with a via armed.
  - KiCad: `ROUTER_TOOL::MainLoop` resets it on entry (`router_tool.cpp:2001`: `m_startWithVia = false;`). Then `performRouting` spends it.
- **How to see it.** X, V, Esc, Esc (back to the selection tool), then X and click a pad. KiCad routes with no via; ours shows a via armed at the head.
  - Test: `SET_ACTIVE_TOOL` away from `"route"` clears `pcbx.startWithVia`.
- **Severity.** Low (visible in the preview before any via is placed).
- **Already known?** No.

#### R7-R7. Autoroute leaves the active layer on the last connection's start layer; KiCad gives back the layer that was active when it began

- **What differs.**
  - Ours: Autoroute's `handOver( a.at, layer )` calls `startInteractiveRoute( .., layer, .. )` with `layer = routeStartLayer( a, copperLayers, state.activeLayer )`. That is the anchor's side for an SMD pad on the other side. `components/canvas/routing.ts:46` then saves `draw.originalActiveLayer = layer`, the start layer and not the active one, and `store.tsx` restores that at the end.
  - KiCad:
    - `RouteSelected` restores `originalLayer` before each anchor ("Try to return to the original layer as indicating the user's preferred layer for autorouting tracks ... should be changed back after.": `if( frame->GetActiveLayer() != originalLayer ) frame->SetActiveLayer( originalLayer );`);
    - `prepareInteractive` saves `m_originalActiveLayer = editFrame->GetActiveLayer()` before `SetActiveLayer( pcbLayer )`;
    - `finishInteractive` restores it.
- **How to see it.** F.Cu active, select a footprint on B.Cu (SMD), then Shift+F. When the run ends KiCad's active layer is F.Cu; ours is B.Cu.
  - Test: the `routeSelected("auto")` loop passes the active layer as the original layer, not `routeStartLayer`'s result (`startInteractiveRoute` takes both).
- **Severity.** Low (the active layer; no data).
- **Already known?** No.

#### R7-R8. The router-mode and corner-mode actions show toasts KiCad does not show (cosmetic)

- **What differs.**
  - Ours: `actions/pcbEditSweep.ts:90` toasts `Router mode: %s` and `:113` toasts `Track corner mode: %s`.
  - KiCad: `ROUTER_TOOL::ChangeRouterMode` / `CycleRouterMode` (`settings.SetMode( mode ); UpdateMessagePanel();`) and `handlePnSCornerModeChange` only update the message panel and refresh the head. No infobar, no popup.
- **Severity.** Low, cosmetic.
- **Already known?** No.

#### R7-R9. GAPS item 7 says "nothing of the router is left but the tuner's gaps"; the component and multi-segment drags are not ported (doc)

- **What differs.**
  - `docs/parity/GAPS.md:399` ("Hit: nothing of the router is left but the tuner's gaps").
  - KiCad: `ROUTER_TOOL::InlineDrag` drags a footprint with its tracks following (`footprints.insert( .. )`, `itemsToDrag.Add( solid )` for each pad, `PNS::DM_COMPONENT` -> `COMPONENT_DRAGGER`), and several selected tracks (`MULTI_DRAGGER`).
  - Ours: `components/canvas/dragging.ts:26-60` (`findDraggableAt`) finds a track or via only, and D on a footprint says our own "Nothing to drag there -- hover a track or via first." `crates/pns/src` has no component or multi dragger.
  - `docs/parity/findings/p5-router.md:687` lists both as known, but neither GAPS item 7's "Left" line nor the ranked list carries them, and D on a footprint is daily use in KiCad.
- **Severity.** Low (the doc). The behaviour itself is a known gap; it is ranked in the convergence list below.
- **Already known?** The behaviour yes (p5); the GAPS wording no.

### Checked and found to match (no need to review again)

**d3af3fce (R6-R1).**
- `chosenViaOf` / `chosenViaSize` against `BOARD_DESIGN_SETTINGS::UseNetClassVia()` / `GetCurrentViaSize()`: no size on entry 0 without custom sizes; the list's entry for index > 0; the custom size while custom sizes are on; never a size for a microvia.
- The server's `Router::net_class_via_size` against `handleLayerSwitch`: it starts from `bds.m_ViasMinSize` / `bds.m_MinThroughDrill`, then takes each rule's opt without a max (`sizes.SetViaDiameter( constraint.m_Value.Opt() )`), for a dummy via of the route's net, its type and its layers, on the layer being routed.
- Place Through Via in the menu goes through the same handler (`pcbMenuActions.ts:114-115`).

**f365ded0 (R6-R2).**
- `settings::chosen_track_width` is `ImportSizes`' `!found` branch, word for word: "board minimum track width", then "user choice" only when the chosen width is the one routed.
- The connected-track width is not clamped (`found = inheritTrackWidth(..)` skips the max), and the rule width is `std::max( trackWidth, constraint.m_Value.Opt() )` with the rule's name only when the opt is routed (`import_rule_sizes`).
- `onTrackViaSizeChanged` imports the sizes again mid-route, so `Router::set_width` raising the width is right.

**9a88f288 (R6-R3 .. R6-R6), single track.**
- `LinePlacer::set_layer` against `LINE_PLACER::SetLayer`: idle, any layer; `m_chainedPlacement` refuses; a via or `SOLID` start item whose layers overlap; no start item.
- `fix` sets `m_startItem = nullptr` and `m_chainedPlacement = !pl.EndsWithVia()` (`chained = true` after a plain fix, false after a via fix).
- `UnfixRoute` clears `m_startItem` and leaves `m_chainedPlacement`.
- The head restarts from the leg's start in the initial direction (`m_p_start = m_currentStart; m_direction = m_initial_direction; m_mouseTrailTracer.Clear()`).
- `ROUTER::SwitchLayer` only in `ROUTE_TRACK`.
- `routerLayerTarget` against `handleLayerSwitch`'s first half:
  - UI order of the board's copper (`LSET::AllCuMask( n ).UIOrder()`);
  - next / prev from the router's layer, wrapping, at most `n - 1` steps past invisible layers (issue 14480), nothing when none is visible;
  - a direct layer only when `enabledLayers.test( targetLayer )`;
  - the current layer is nothing to do.
- Refused, a through via to the target is armed, sized as V's.
- R6-R4: `groupStart` / `APPEND_UNDO` for the routes `Finish` makes; the run's `originalLayer` read once; no message when there is nothing to route or after a run (`RouteSelected` shows none). R7-R4 is the exception.
- R6-R5: `MainLoop`'s `m_startWithVia = true; RunAction( layerToggle )` flips the active layer to the pair's other layer (`PCB_CONTROL::LayerToggle`), and `performRouting` arms the via before its loop. R7-R3 (pair) and R7-R6 are the exceptions.
- R6-R6: `syncRouterAndFrameLayer` (the active layer is the router's, made visible), `m_originalActiveLayer` saved at every start and given back by `finishInteractive`; Route From Other End keeps it (`continueInteractiveRouteFromEnd` spreads the draw state). R7-R7 is the exception.

**(c).**
- `LAYER_PAIR_SETTINGS` (`layer_pairs.cpp`) against `kicad-port/layerPairs.ts`: `addLayerPairInternal` (the duplicate test by `HasSameLayers`, and the manual pair dropped when added), `RemoveLayerPair`, `SetLayerPairs` (duplicates skipped), `SetCurrentLayerPair` (manual unless an enabled preset), `GetEnabledLayerPairs` ("Manual" first, then the enabled presets, the current index).
- `PCB_CONTROL::CycleLayerPresets`: fewer than two does nothing; `currentIndex < 0` is taken as 0; `( i + 1 ) % n`.
- `SelectCopperLayerPair`: "Warning: top and bottom layers are same." (a toast here, a `DisplayInfoMessage` there).
- `CycleRouterMode`'s order (Highlight collisions -> Shove -> Walk around -> Highlight collisions) and `ChangeRouterMode`.
- `handlePnSCornerModeChange`'s cycle (45 -> arc 45 -> 90 -> arc 90 -> 45), live only in the router tool (`AS_CONTEXT`).
- The `pcbnew.InteractiveRouter.*` action names: every one KiCad registers (`router_tool.cpp`, `pcb_actions.cpp`) has a handler in the studio.

---

## Section D: rules, DRC, Board Setup rule pages (round 7)

Read-only. Code read from main's checkout (`/home/team_braneboard_com/work/agentic-eda`, at f057b4ea). KiCad: `kicad-src-8303b2ad`, and `kicad-src-25ec2335` for the DRC item list. Nothing was built or run (main's `target/debug/eda` is not there).

Scope:
- (a) the round-6 fixes 30f07089, 291b3dc7, 55418603, f67b52c0, 72c7b52e, 0c4efd45, e550feb3, 5d0378d9, 5752f359, and the round-6 board-io fix b4ae2230 (it touches the `.kicad_dru` reader the length tuner uses);
- (b) the rule board's item facts no round has compared with KiCad's property getters (`Library Link`, `Via Type`, `isMicroVia`), and the length tuner's rule lookup;
- active code that duplicates the rule engine.

### High

None.

### Medium

#### R7-D1. `A.Locked` (the R6-D7 fix) ignores a locked group, and zones, graphics and texts have no "Locked" at all

- **What differs.**
  - Ours: `crates/drc/src/rule_board.rs:498` `is_locked` answers from the item's own id only (`locked_ids.iter().any(|l| l == id)`). It is set for footprints (`:336`), tracks (`:519`) and vias (`:579`). `zone_item` (`:591`), `shape_item` (`:646`) and `text_item` (`:668`) set no "Locked" property.
  - KiCad `board_item.cpp::BOARD_ITEM::IsLocked` (8303b2ad:125):
    ```cpp
    if( EDA_GROUP* group = GetParentGroup() )
    {
        if( group->AsEdaItem()->IsLocked() )
            return true;
    }
    ...
    return m_isLocked;
    ```
    The "Locked" property is registered on `BOARD_ITEM` (`board_item.cpp:483`, `PROPERTY<BOARD_ITEM, bool>( _HKI( "Locked" ), .., &BOARD_ITEM::IsLocked )`). It is masked only for `PAD` (`pad.cpp:3388`) and `PCB_MARKER` (`pcb_marker.cpp:467`). So zones, shapes and texts have it, and a track, via, zone or graphic inside a locked group reads as locked. `FOOTPRINT::IsLocked` (`footprint.h:622`) is its own flag alone, which ours matches.
  - Zones can be locked here (`locked_ids` holds zone ids; `pcb.rs` round-trips a locked zone).
- **How to see it.**
  - Test `rule_board.rs` `a_member_of_a_locked_group_and_a_locked_zone_are_locked`: `hv_board` with a group `G` (locked, `locked_ids: ["G"]`) holding track `t1`, and a zone `z1` in `locked_ids`. Assert `holds(&rb, "A.Locked", &track_item("t1"))` and `holds(&rb, "A.Locked", &rb.zone_item_of(z1))`. Both fail today.
  - Scenario: lock a group of tracks, add `(rule l (condition "A.Locked") (constraint clearance (min 1mm)))`. Inspect > Clearance Resolution on a member shows the board default; KiCad shows rule `l`. kicad-cli's DRC applies `l`.
- **Severity.** Medium (wrong rule match; rare rules).
- **Already known?** No. R6-D7 listed footprints, pads, tracks and vias only.

#### R7-D2. A microvia is "Blind" on the rule board: `A.isMicroVia()` is false, `A.isBlindVia()` true, `A.Via_Type` is never `'Micro'`

- **What differs.**
  - Ours: `rule_board.rs:545-554` (`via_item`) works the kind out from the span alone ("Through" / "Blind" / "Buried"). `pcbexpr.rs:1014-1017` tests `it.via_type == Some("Micro")`, which the rule board never gives. The model does know the type: `eda_model::via_extra::ViaKind::Micro`, read and written by `crates/kicad/src/via_io.rs`, and committed by the router's Place Microvia (GAPS item 7). `crates/drc` never reads `ViaExtra`.
  - The studio's Find by Properties request does the same (`web/studio/src/kicad-port/pcbQuery.ts` `viaType`: "(no micro vias)").
  - KiCad: `pcb_track.cpp` `ENUM_MAP<VIATYPE>`: `.Map( VIATYPE::MICROVIA, _HKI( "Micro" ) )`; the "Via Type" property is `GetViaType()`. `pcbexpr_functions.cpp::isMicroVia`: `item->Type() == PCB_VIA_T && static_cast<PCB_VIA*>( item )->IsMicroVia()`; `PCB_VIA::IsMicroVia`: `m_viaType == VIATYPE::MICROVIA`. `PCB_VIA::IsBlindVia` is true only for `VIATYPE::BLIND || VIATYPE::BURIED` (`pcb_track.cpp:1915`), so a microvia is not blind.
  - So an HDI rule such as `(rule uvia (condition "A.isMicroVia()") (constraint hole_size (min 0.075mm)))` is never applied by the router, Draw Via, the via stitcher or Inspect. A `isBlindVia()` rule is applied to microvias, which KiCad does not do. kicad-cli's DRC applies them as KiCad does, so the router places what the DRC then flags.
- **How to see it.**
  - Test `rule_board.rs` `a_micro_via_is_micro_on_the_rule_board`: a 4-layer board with a via F.Cu-In1.Cu whose `ViaExtra.kind` is `Micro`. Assert `holds(&rb, "A.isMicroVia()", &via)`, `!holds(&rb, "A.isBlindVia()", &via)` and `holds(&rb, "A.Via_Type == 'Micro'", &via)`. All three fail today.
- **Severity.** Medium (wrong rule match on HDI boards).
- **Already known?** Partly. GAPS line 903 (item 29) says "micro vias (the IR has no via type: a via is through, blind or buried by its layers)". That reason is stale: the IR has `ViaKind` since GAPS item 7's router work. The rule board was not listed.

#### R7-D3. `A.Library_Link` (and `memberOfFootprint('lib:name')`, `intersectsCourtyard('lib:name')`, the Footprint condition of component classes) carries the importer's `#2` slot

- **What differs.**
  - Ours: `rule_board.rs:290` `let fpid = part.map(eda_model::footprint::footprint_lib_id)`. The R6-D2 fix changed it to say "the link the derived board writes". But `footprint_lib_id` returns `part.footprint` as it is. For an imported board, that is the importer's key, `"Lib:Name#2"`, whenever two instances of one library footprint differ in pads, 3D models or art (`crates/kicad/src/import.rs:1862` `dedup_footprint_key`, `:2201` `footprint: Some(key)`).
  - The writer takes the slot off (`crates/kicad/src/pcb.rs:1183-1186`: "A `#2`, `#3` ... slot is the importer's way to keep apart instances ...: in the file it is still that library's footprint"). So kicad-cli sees `Lib:Name`, and the rule board sees `Lib:Name#2`.
  - The same `fpid` is `FootprintFacts::fpid`. It feeds `memberOfFootprint` / `intersectsCourtyard` (`footprint_selected`), and through R6-D2 Multichannel's component classes (`fpid_of` strips only `eda:`). Find by Properties sends `part.footprint` too (`pcbQuery.ts:196`).
  - KiCad: "Library Link" is `FOOTPRINT::GetFPIDAsString()`. `testFootprintSelector` (`pcbexpr_functions.cpp:245`): `aSelector.Find( ':' ) != wxNOT_FOUND && aFp->GetFPIDAsString().Matches( aSelector )`. A component-class Footprint condition is `( A.Library_Link == '%s' )`.
- **How to see it.**
  - Test `rule_board.rs` `library_link_has_no_import_slot`: a part with `footprint: Some("Resistor_SMD:R_0603#2")` and `model.footprints` holding `Resistor_SMD:R_0603`. Assert `holds(&rb, "A.Library_Link == 'Resistor_SMD:R_0603'", &fp)` and `holds(&rb, "A.memberOfFootprint('Resistor_SMD:R_0603')", &pad)`. Today it reads `...#2` and fails.
  - Scenario: import a board where one 0603 resistor's 3D model offset differs from the others. A component class "Footprint = Resistor_SMD:R_0603..." (exact) leaves that resistor out in the router, Inspect and Multichannel. kicad-cli includes it.
- **Severity.** Medium (wrong rule and class match on imported boards; exact library-link conditions only, a trailing `*` hides it).
- **Already known?** No.

#### R7-D4. The length tuner's target is a second rule lookup: a second `.kicad_dru` reader, on stand-in tracks with no properties, and with B set to the partner where KiCad has none

- **What differs.**
  - Ours: `crates/cli/src/tune_edit.rs:316` `constraint_for`.
    - It reads `rules.custom_rules`: `crates/kicad/src/custom_rules.rs`, a second hand-written `.kicad_dru` reader beside the port in `crates/drc/src/dru.rs`. r6-board-io F4 patched the two places where it differed (b4ae2230).
    - It evaluates each condition on two bare `RuleItem::new(id, Class::Track)` stand-ins (`:330-338`). They have only a net and a layer: no "Width", "Layer"-property, "Net Class" property, groups, parent footprint or sheet.
    - B is the partner's stand-in, or the same net (`track("b", partner.unwrap_or(net))`).
    - A condition this evaluator cannot compile skips that one rule (`continue`).
  - KiCad `generators/pcb_tuning_pattern.cpp::PCB_TUNING_PATTERN::CreateNew` (8303b2ad:447 / :478):
    ```cpp
    constraint = bds.m_DRCEngine->EvalRules( LENGTH_CONSTRAINT, aStartItem, nullptr, layer );
    ...
    constraint = bds.m_DRCEngine->EvalRules( SKEW_CONSTRAINT, aStartItem, nullptr, layer );
    ```
    That is the real start track as A and no B, through the same compiled rules the DRC uses. A rules file that does not compile gives no rules at all.
  - Results that differ:
    - `(condition "A.NetClass == 'DDR' && A.Width < 0.15mm")`: no target here; KiCad has one.
    - `(condition "A.memberOfGroup('bus') ...")`: no target here; KiCad has one.
    - `(condition "B.NetName == 'CLK_N'")` on a single track: here B is a stand-in of the same net; in KiCad B is null.
  - The rule board (`eda_drc::rule_board::RuleBoard`, already built for the router and Inspect) has the real track items and the ported compiler (`constraints.rs` knows `CType::Length` / `Skew`).
- **How to see it.**
  - Test `tune_edit.rs` `a_length_rule_on_track_width_gives_the_target`: rules `(rule w (condition "A.Width == 0.2mm") (constraint length (min 40mm)))`, a 0.2 mm track of net SIG. Assert `constraint_for(..)` is `Some` with min 40 mm. Today it is `None`: the stand-in has no Width.
- **Severity.** Medium (a wrong or missing tuning target for any length rule whose condition asks more than the net and class).
- **Already known?** Partly. r6-board-io noted "custom_rules.rs is a second, partial reader of `.kicad_dru` beside the rule engine port", and fixed the two differences F4 named. The stand-in items and B are not listed. This is the active duplicate of the ported rule engine the brief asks about.

### Low

#### R7-D5. Multichannel's own sheet (`memberOfSheet` stand-ins, rule-area sources) still has a different source from the rule board's sheet classes

- **What differs.**
  - `crates/ops/src/multichannel_facts.rs:172`: the sheet is `FootprintExtra::sheetname`, else `mc_part_sheets()`. That is the stored schematic, or `infer_modules` when there is none.
  - The rule board (`rule_board.rs:311`), which since R6-D2 gives Multichannel its component classes (sheet classes included), takes `FootprintExtra::sheetname`, else `symbol_links[..].sheetname`. That is what the writer puts in `(sheetname ..)` (`pcb.rs:1248-1249`) and what KiCad's `GetSheetname()` then is.
  - So on a board made here with modules but no Update-PCB symbol links, Generate Placement Rule Areas groups footprints by the inferred `/<module>/` sheets. The "sheet" component classes, and kicad-cli, have no sheet at all.
- **How to see it.** Test `multichannel_tests.rs` `mc_sheet_is_the_written_sheetname`: a design with two inferred modules and empty `symbol_links`. Assert `mc_board().fps[i].sheetname == ""`, or that the rule board gives the same as mc. They disagree today.
- **Severity.** Low (R6-D2's sheet point, half fixed).
- **Already known?** R6-D2 named it. The fix's commit message claims one evaluator, but the sheet source stayed two.

### Checked and found to match (no need to review again)

- **30f07089 (R6-D1).**
  - A footprint's `A.Orientation` is `kicad_footprint_orientation`: `normalize180(-(rot + half))`, the writer's own `(at x y a)` angle (the code is shared now). This is `FOOTPRINT::GetOrientationDegrees`, kept in (-180, 180] by `SetOrientation`.
  - A pad's is `orientation_mdeg` in [0, 360), with the half turn of a back footprint made here added exactly when `half_turn_pad` keeps the pad's own angle (`pad.ext.is_default()`). That equals the writer's `pad_file_angle` of the half-turned footprint modulo 360.
  - `orientation_mdeg` has no other reader (`drill_chart.rs` keeps its own copy for slot angles, where a half turn is no change).
- **291b3dc7 (R6-D2).**
  - The hand-written condition evaluator is gone. Classes come from `RuleBoard::footprint_item(..).footprint.classes`: the file's classes, the compiled `GetAssignmentInDRCLanguage`, and the sheet classes.
  - Custom conditions, an empty Side and the absent Footprint field now follow KiCad. `A.Library_Link` is `footprint_lib_id`, apart from R7-D3.
- **55418603 (R6-D3).**
  - `/api/pcb/query` fills each footprint's sheet and classes from the rule board when the request has none; a request's own wins.
  - A hand-placed footprint's sheet is "" (`GetSheetname()`).
  - The pad and the field items reach the footprint's facts.
- **f67b52c0 (R6-D4).** `drc_checks.json` was compared with 25ec2335 `drc_item.cpp` `allItemTypes` item by item (script: 74 items with a severity = 77 less the three of `heading_internal`):
  - every settings key, every title word for word, the `DRCE_*` code and the order;
  - every default against `BOARD_DESIGN_SETTINGS::BOARD_DESIGN_SETTINGS` (25ec2335:167-217): all error, then the warning and ignore overrides, including `copper_outside_board`, `microvia_crosses_core` and `microvia_stack_not_filled` as warnings, `footprint_scaled_with_pads` warning, `tuning_profile_track_geometries` and `track_not_centered_on_via` ignore.
  - `zones_intersect` is gone from 25ec2335's list.
- **72c7b52e (R6-D5).**
  - `compile_report` is `OnCompile` less `ExpandTextVars` / `checkPlausibility` (both documented). `DRC_RULES_PARSER::Parse( rules, reporter )` reports `"ERROR: <a href='%d:%d'>%s</a>%s"`, and gives "No errors found." only when the reporter has no message.
  - A thrown `PARSE_ERROR` gives `"%s <a href='%d:%d'>%s</a>%s"` with `_( "ERROR:" )` and `ParseProblem()`.
  - The lexer's "Unterminated delimited string" (`dsnlexer.cpp` L_unterminated, non-specctra mode) matches, with its offset `cur - start + curText.length()`.
  - `OnErrorLinkClicked` puts the caret at `PositionFromLine( line - 1 ) + ( offset - 1 )`; ours does the same (`goTo`).
- **0c4efd45 (R6-D6).** `m_cbRefillZones->SetValue(true)` in 8303b2ad and 25ec2335 `dialog_drc_base.cpp`.
- **e550feb3 (R6-D7).** Footprints, tracks (the `#k` piece of a polyline mapped to its track) and vias: own flag. The pad mask is right (`pad.cpp:3388`). Missing parts: R7-D1.
- **5d0378d9 (R6-D8).** `CompileAssignmentRule` returns null for an empty class (`if( ruleSource.empty() ) return nullptr;`), for a text the parser cannot read, and for any reporter error. One null makes `success = false` and no rules. Ours gives none on an empty class or a `"` in class or condition, and the round-6 "checked" list covers a condition that does not compile.
- **5752f359 (R6-D9).** `OnSeverity` pushes All into the other three and nothing sets it back. KiCad's All starts unchecked (`dialog_drc_base.cpp` has no `SetValue` for `m_showAll` or `m_showExclusions`). Ours starts on `SHOW_ALL` (all four ticked): this is GAPS item 11's "Noticed, not changed: KiCad's Show boxes start with Exclusions and All off". Known, so not counted. (Round 6's "checked" line saying ours starts with Exclusions off was wrong.)
- **b4ae2230 (r6-board-io F4)** against `DRC_RULES_PARSER::parseLayer` (8303b2ad:831): `T_outer` = `ExternalCuMask()`, `T_inner` = `InternalCuMask()`, else `ENUM_MAP<PCB_LAYER_ID>` names matched with `wxString::Matches` (case-sensitive). Every `(constraint ..)` of a rule is its own entry (`parseDRC_RULE`). The rest of that reader is R7-D4.
- **Functions of the expression language**, against `pcbexpr_functions.cpp` (8303b2ad):
  - the registered names and deprecated aliases (`RegisterAllFunctions`);
  - `isPlatedFunc` (a PTH pad or any via);
  - `isBlindVia` / `isBuriedVia` / `isBlindBuriedViaFunc` by span for blind and buried vias (apart from microvias, R7-D2);
  - `inDiffPairFunc` (the complement must exist on the board; the base name or the base less its trailing `_`, `Matches`);
  - `isCoupledDiffPairFunc` (the DIFF_PAIR_GAP / LENGTH / SKEW shortcut);
  - `memberOfFootprintFunc` (a footprint itself is never its own member: `GetParentFootprint()`);
  - the selector forms of `testFootprintSelector` (`${CLASS:..}`, reference, a lib id with `:`).

---

## Section Z: zones, connectivity and Cleanup (round 7)

Read-only, as above. Scope: the round-6 fixes 21a343f3, b8ce9d43, e7182c29, 6c9ae9be, 80556d92; then the fill cache's model of `UnFill()`, and the unfilled-zone notice.

### High

None.

### Medium

None.

### Low

#### R7-Z1. An unfill does not stick: a zone that KiCad has emptied gets its old fill back when a later edit returns it to the filled shape or layers

- **What differs.**
  - Ours: the fill cache is stateless with respect to edits. `crates/kicad/src/zone_fills.rs:114` `KicadFills::current` decides from the entry made at fill time and the zone as it is now (`follow`, `:297`). An edit that empties the zone (another layer set; the point editor's reshape) only makes `current` return `None`. The entry stays (`fill_api.rs` removes entries only on Unfill and on a refill).
  - So after a later edit makes the zone the shape and layers it was filled with again, the old fill is "current" again:
    - Zone Properties F.Cu → B.Cu → F.Cu;
    - a corner dragged away and dragged back;
    - a one-sided zone flipped twice (R6-Z3's `unfilled` comes from `!layer_of.is_empty()`, which is empty again after the second flip, so it is even written `(fill yes)`).
  - KiCad: `ZONE::SetLayerSet` (8303b2ad `zone.cpp:556`) on another set runs `unFillLocked()`. That replaces every `m_FilledPolysList` entry with an empty set and does `m_isFilled = false`, and then `m_FilledPolysList.clear()`. The point editor does `m_zone.UnFill()`. The copper is gone until the next fill, whatever later edits do. (Undo is the exception in both: KiCad's undo copy holds the fill.)
- **How to see it.** Test `zone_fills.rs` `a_layer_change_and_back_leaves_the_zone_unfilled`: `filled_rect()` on F.Cu with its entry; the zone moved to B.Cu (assert `current` is `None`), then back to F.Cu. Assert `fills.current(&back).is_none()`. Today it is `Some` with the old rings and `unfilled == false`. A second test: flip twice, assert `current(..).unwrap().unfilled`.
- **Severity.** Low (display, live connectivity and the derived board's `(fill yes)` until the next B; a refill makes the two the same).
- **Already known?** No. GAPS 13 notes only that undo gives the fill back "because the cache still holds the old zone's entry".

#### R7-Z2. "N zones not filled (B)" is a standing status-bar note; KiCad says it in a 5-second infobar, and only when the zone display is switched to filled (cosmetic)

- **What differs.**
  - Ours: `web/studio/src/kicad-port/checkRevision.ts:83` pushes `` `${n} zone(s) not filled (B)` `` into the status notes whenever a copper zone has no fill.
  - KiCad `tools/pcb_control.cpp::PCB_CONTROL::unfilledZoneCheck` is called from `ZoneDisplayMode` (on `zoneDisplayFilled`) only. Unless "Don't show again" was clicked, it shows:
    ```cpp
    msg.Printf( _( "Not all zones are filled. Use Edit > Fill All Zones (%s) "
                   "if you wish to see all fills." ), KeyNameFromKeyCode( PCB_ACTIONS::zoneFillAll.GetHotKey() ) );
    infobar->ShowMessageFor( msg, 5000, wxICON_WARNING );
    ```
    with a "Don't show again" link.
- **How to see it.** Open a board with an unfilled pour: ours shows the note at once and keeps it. KiCad shows nothing until View > Draw Zone Fills, then its sentence for 5 s.
- **Severity.** Low, cosmetic.
- **Already known?** No (GAPS 13 describes our note as a feature).

### Checked and found to match (no need to review again)

- **21a343f3 (R6-Z1).**
  - Duplicate, Create Array, Paste / Paste Special / PasteItems carry the source's current fill, `unfilled` included (`InitDataFromSrcInCopyCtor` copies `m_FilledPolysList` and `m_isFilled`; no `UnFill()` in `EDIT_TOOL::Duplicate` or the array tool).
  - Copy writes every layer's current rings moved into the clipboard frame and `(fill yes)` from `IsFilled` (`format( const ZONE* )`).
  - A source with no current fill gives an unfilled copy.
  - Matching a copy to a source of the same net cannot cross layers or mirror: `follow` needs equal layers unless the motion is a mirror, and a mirror reverses the vertex order's winding, which a copy keeps.
  - Duplicate Zone onto Layer goes through Zone Properties (a new zone), so it stays unfilled (`ZoneDuplicate`'s `newZone->UnFill()`).
- **b8ce9d43 (R6-Z2).** `computeConstraint` compares net codes. A no-net zone (""), an NPTH, a pad no pin names and a no-net via are net 0. A pin alone is on its `unconnected-(..)` net, as the derived board writes it.
- **e7182c29 (R6-Z3).** A one-sided flip is `unfilled` with the mirrored copper kept (`SetLayerSet` then `SetFilledPolysList`). A zone on both outer layers stays filled (`m_layerSet == aLayerSet`). Apart from the double flip, R7-Z1.
- **6c9ae9be (R6-Z4).** `mergeSegments`' different-width candidates are `!filterItem( candidate )`. A filtered-out track does not block; a locked one that the filter keeps does.
- **80556d92 (R6-Z5).** `wxSpinCtrl( .., 0, 10, 0 )`: a typed value is clamped to 0..10 with no message. (A project value of 11..99 is shown as read rather than as the spin control's 10. Not counted: Apply would need an edit to save it.)
- **Swap Pad Nets through a fill** (`pad_cluster.rs` with no zone copper): known (p22-p24-p25 line 479, GAPS 26).
- **`zone_copper.rs`**: a plain container of the fills by (zone, layer); nothing to compare.

---

---

## Convergence: what still differs that a user would notice in daily work (ranked)

**Router**
1. **Ctrl+Z, Delete and Redo while routing or dragging act on the board** (R7-R1, new). KiCad blocks undo and redo during a route (`UndoRedoBlock`), and there Undo or Delete takes back the last segment. This is the router difference most likely to cost work today.
2. **D on a footprint (`COMPONENT_DRAGGER`) and D on several tracks (`MULTI_DRAGGER`) are not ported.** This is known (p5-router), but GAPS item 7 says nothing is left (R7-R9). Dragging a footprint with its tracks following is daily use in KiCad.
3. **The pair tool misses round 6's single-track fixes**: the chosen via size (R7-R2), the layer keys while routing, and V before a route (R7-R3).
4. **V's implicit target is the layer pair's other layer, not the nearest ratsnest anchor's layer**, and the clearance is not re-imported after a layer switch. Both are known (GAPS 7, Stage 20).
5. **E while routing opens the wrong dialog** (R7-R5). A hand-finished Autoroute connection lands in the wrong undo step (R7-R4).
6. Cosmetic or near-cosmetic: the router-mode and corner-mode toasts (R7-R8); the active layer after an Autoroute (R7-R7); a stale V-before-a-route flag (R7-R6).

Otherwise the single-track router of round 6 matches KiCad: V's size, the board-minimum width, `LINE_PLACER::SetLayer`, Autoroute through `ROUTER::Finish`, and the active layer following the route.

**Rules, DRC, zones:** see the ranked list at the end of the D/Z part above. In short: on HDI or imported boards, rule conditions on microvias, locked groups or zones, and deduplicated library links match different items than kicad-cli does. The length tuner's target is the one active second rule evaluator left. The rest of the round-6 merges in this area match KiCad.

---

## Summary

| ID | What | Severity | Known before? |
|---|---|---|---|
| R7-R1 | Ctrl+Z / Delete / Redo / zone fill while a route, pair or drag is live act on the board (KiCad: `UndoRedoBlock`, Undo/Delete = Undo Last Segment, fill = `wxBell`) | high | no |
| R7-R2 | A pair's vias ignore the chosen Pre-defined / custom via size (`ImportSizes`, `handleLayerSwitch` use `GetCurrentViaSize()`) | high | partly (GAPS 7: pair via commands) |
| R7-R3 | Pair tool: layer keys mid-route (`DIFF_PAIR_PLACER::SetLayer`) and V before a pair (`m_startWithVia`) not ported | medium | no |
| R7-R4 | Autoroute: a hand-finished connection joins the autorouted undo group (KiCad `SetCommitFlags( 0 )` when `Finish` fails); Stage 20 documents the opposite | medium | no |
| R7-R5 | E while routing opens the hovered item's properties, not Custom Track/Via Size (`PCB_ACTIONS::properties` -> `ACT_CustomTrackWidth`) | medium | no |
| R7-R6 | `startWithVia` survives leaving the tool (`MainLoop`: `m_startWithVia = false`) | low | no |
| R7-R7 | Autoroute leaves the active layer on the last start layer, not `originalLayer` | low | no |
| R7-R8 | Router-mode / corner-mode toasts KiCad does not show | low (cosmetic) | no |
| R7-R9 | GAPS 7 "nothing left" omits COMPONENT_DRAGGER / MULTI_DRAGGER (doc) | low | behaviour yes (p5) |
| R7-D1 | `A.Locked`: no group lock (`BOARD_ITEM::IsLocked`), and zones, shapes and texts have no "Locked" | medium | no (R6-D7 partial) |
| R7-D2 | A microvia reads "Blind" on the rule board: `isMicroVia()` false, `isBlindVia()` true, no `'Micro'` Via Type; the IR's `ViaKind` is unused | medium | partly (GAPS 903, stale reason) |
| R7-D3 | `A.Library_Link` / footprint selectors carry the importer's `#2` slot that the writer strips | medium | no |
| R7-D4 | The length tuner's rule lookup: second `.kicad_dru` reader, property-less stand-in tracks, B = partner (KiCad: start item, B null) | medium | partly (r6-board-io) |
| R7-D5 | Multichannel's own sheet source still differs from the rule board's / the written `(sheetname)` | low | partly (R6-D2) |
| R7-Z1 | A zone KiCad has unfilled gets its old fill back when an edit returns it to the filled shape or layers (double flip writes `(fill yes)`) | low | no |
| R7-Z2 | Standing "N zones not filled (B)" note vs KiCad's 5 s infobar on switching to filled display | low (cosmetic) | no |
