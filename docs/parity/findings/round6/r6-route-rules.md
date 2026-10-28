# Round 6 findings: router, zones, rules, DRC, connectivity and Board Setup (`r6-route-rules`, 2026-10-28)

Read-only review. Code was read from main's checkout (`/home/team_braneboard_com/work/agentic-eda`, at 7d772e6e). KiCad: `/home/team_braneboard_com/src/kicad-src-8303b2ad`, and `kicad-src-25ec2335` for items newer than that (the box's kicad-cli 10.99.0, via stitching, the DRC item list). Nothing was built.

Already-known items were skipped: GAPS 3, 7, 11, 13, 29, 36, `crates/pns/PARITY.md` Stages 18-19, PARITY-pcb section 59, and rounds 1-5 under `docs/parity/findings/`.

The file has three sections:
- **R**: the router and its studio side;
- **D**: rules, DRC and the Board Setup rule pages;
- **Z**: zones, connectivity and Cleanup.

Each section ends with what was checked and found to match. The summary table is at the end.

---

## Section R: the router and its studio side

Read-only. Code read from main's checkout (`/home/team_braneboard_com/work/agentic-eda` at 7d772e6e). KiCad: `/home/team_braneboard_com/src/kicad-src-8303b2ad`.

Scope:
- (a) the round-5 router merges: dc58601d (`pickSingleItem` / `getStartLayer`, `CommitRoute`'s `track_sources` / `group`, the pair's user dimensions), a023fbf8 (`routerSizes.ts`, the message panel, W / Shift+W), 8df3629b (the project's lists);
- (b) router behaviours not yet ported, or never reviewed: the router tool's layer commands, V's via size, `m_startWithVia`, Route Selected / Autoroute;
- (c) rows no round reviewed: Board Setup > Length-tuning Patterns and the tuner's 1-4 keys.

### High

#### R6-R1. V while routing arms a via of the board's default size, not the net class's or the chosen Pre-defined Sizes via

**Fixed** on task-r6-router: d3af3fce.

- **What differs.**
  - Ours: `web/studio/src/actions/useActionRunner.ts:421` (`pcbnew.Control.layerToggle` while a route is drawn; `pcbMenuActions.ts:114-115` makes the menu's Place Through Via the same handler):
    ```ts
    routeToggleVia(!draw.placingVia, rules?.via_diameter ?? 600_000, rules?.via_drill ?? 300_000, toLayer)
    ```
    `rules` is `board.board_rules`: the board's default via. `crates/cli/src/route_api.rs:306-308` (`via`) uses the sent `diameter` / `drill` and asks the rules (`net_class_via_size`) only when none is sent. So the net's class, a `.kicad_dru` `via_diameter` / `hole_size` rule and the chosen list entry (`currentViaIndex`, custom sizes) are all ignored for V.
  - The other via commands of the same file do it right: `routerViaActions.ts:62-67` sends nothing on "use net class" and the list's entry otherwise (`chosenViaSize`).
  - KiCad: V while routing is `ACT_PlaceThroughVia` -> `ROUTER_TOOL::onViaCommand` -> `handleLayerSwitch( aEvent, true )` (`router_tool.cpp`), which sizes every via the same way:
    ```cpp
    sizes.SetViaDiameter( bds.m_ViasMinSize );
    sizes.SetViaDrill( bds.m_MinThroughDrill );

    if( bds.UseNetClassVia() || viaType == VIATYPE::MICROVIA )
    {
        PCB_VIA dummyVia( board() );
        dummyVia.SetViaType( viaType );
        dummyVia.SetLayerPair( currentLayer, targetLayer );

        if( !m_router->GetCurrentNets().empty() )
            dummyVia.SetNet( static_cast<NETINFO_ITEM*>( m_router->GetCurrentNets()[0] ) );

        DRC_CONSTRAINT constraint;

        constraint = bds.m_DRCEngine->EvalRules( VIA_DIAMETER_CONSTRAINT, &dummyVia, nullptr,
                                                 currentLayer );

        if( !constraint.IsNull() )
            sizes.SetViaDiameter( constraint.m_Value.Opt() );
        ...
    }
    else
    {
        sizes.SetViaDiameter( bds.GetCurrentViaSize() );
        sizes.SetViaDrill( bds.GetCurrentViaDrill() );
    }
    ```
- **How to see it.** A board whose Default class has a 0.6 / 0.3 mm via and a class `Power` with 0.8 / 0.4 mm, a net `+5V` in `Power`. Route from a `+5V` pad, press V, click, finish. KiCad places a 0.8 / 0.4 via; ours places 0.6 / 0.3, which kicad-cli's DRC then flags against the class (or a `via_diameter` rule). The same happens with a Pre-defined Sizes via chosen in the via menu: Alt+Shift+V uses it, V does not.
  - Test (studio, `routerVia.test.ts` style, after moving V's size choice into `kicad-port/routerVia.ts`): `v_while_routing_sizes_the_via_as_the_other_via_commands` -- with `currentViaIndex` 0 the request has no `diameter` / `drill`; with index 2 it has `via_presets[1]`; with custom sizes on, the custom size.
  - Server-side proof: `route_api` `a_v_via_of_a_power_net_has_the_class_size` -- `start` on the `+5V` pad, `via` with only `to_layer`, `fix`, `finish`; the committed via is 0.8 / 0.4.
- **Severity.** High: the wrong via size is committed on the most common via command.
- **Already known?** No. Known only: V's target layer is the layer pair's other layer, not KiCad's nearest-ratsnest-anchor layer (`crates/pns/PARITY.md` Stage 11, GAPS item 7). The size is not listed.

#### R6-R2. A chosen track width below the board's minimum track width is routed as it is; KiCad routes at the minimum

**Fixed** on task-r6-router: f365ded0.

- **What differs.**
  - Ours: `crates/cli/src/route_api.rs:215-219` (`start`):
    ```rust
    (None, Some(w)) => (Nm::new(w), "user choice".to_string()),
    ```
    The width the studio sends (a list entry or the custom width, `chosenTrackWidthOf`) is taken as it is. Mid-route W / the width menu goes through `Router::set_width` (`crates/pns/src/router.rs:498-502`), also unclamped.
  - KiCad: `PNS_KICAD_IFACE_BASE::ImportSizes` (`pns_kicad_iface.cpp`) starts from the board minimum and only ever raises it:
    ```cpp
    int  trackWidth = bds.m_TrackMinWidth;
    bool found = false;
    aSizes.SetWidthSource( _( "board minimum track width" ) );
    ...
    if( !found )
    {
        trackWidth = std::max( trackWidth, bds.GetCurrentTrackWidth() );

        if( bds.UseNetClassTrack() )
            aSizes.SetWidthSource( _( "netclass 'Default'" ) );
        else if( trackWidth == bds.GetCurrentTrackWidth() )
            aSizes.SetWidthSource( _( "user choice" ) );
    }
    ```
    `onTrackViaSizeChanged` calls `ImportSizes` again after W or a menu choice, so the mid-route change is clamped too. Neither `DIALOG_TRACK_VIA_SIZE::TransferDataFromWindow` nor `PANEL_SETUP_TRACKS_AND_VIAS` stops a width below the minimum, so the case is reachable in KiCad. (The connected-width path, `inheritTrackWidth`, is not clamped in KiCad either, and ours matches it there.)
- **How to see it.** Board Setup > Constraints: minimum track width 0.2 mm. Custom Track/Via Size: track 0.1 mm, "Use Starting Track Width" off. Route from a pad. KiCad routes 0.2 mm, and the message panel says "Track Width: 0.2 mm (from board minimum track width)". Ours routes 0.1 mm "(from user choice)", and kicad-cli's DRC reports `track_width` on every new segment.
  - Test: `route_api` `a_chosen_width_below_the_board_minimum_routes_at_the_minimum` -- `start` with `width: 100_000`, `auto_track_width: false`, on a board with `track_width_min_nm` 200 000; assert the reply's `width == 200_000` and `sizes.width_source == "board minimum track width"`. Then `move` with `width: 100_000`; assert the preview's `width` stays 200 000.
- **Severity.** High: wrong result (tracks narrower than the board allows). Only reached with a chosen width below the minimum.
- **Already known?** No.

### Medium

#### R6-R3. During a route, the layer keys (PgUp / PgDn, + / -, the layerInner*N* actions) only change the studio's active layer; KiCad switches the route's layer or drops a via there (`ROUTER_TOOL::onLayerCommand`)

**Fixed** on task-r6-router: 9a88f288.

- **What differs.**
  - Ours: `useActionRunner.ts:1009-1010` (`layerNext` / `layerPrev`: `cycleActiveLayer`) and `:1912-1929` (`layerTop` / `layerBottom` / `layerInner1..30`: `switchLayer`) are registered whatever the draw state. They dispatch `SET_ACTIVE_LAYER` and never call the router. The route goes on, on its own layer.
  - KiCad: while the router tool runs, these actions are the router's (`ROUTER_TOOL::setTransitions`: `Go( &ROUTER_TOOL::onLayerCommand, PCB_ACTIONS::layerTop.MakeEvent() )` .. `layerBottom`, `layerNext`, `layerPrev`, `layerToggle`). `onLayerCommand` is `handleLayerSwitch( aEvent, false )`:
    ```cpp
    if( targetLayer != UNDEFINED_LAYER )
    {
        if( targetLayer == currentLayer )
            return 0;

        if( !aForceVia && m_router && m_router->SwitchLayer( m_iface->GetPNSLayerFromBoardLayer( targetLayer ) ) )
        {
            updateEndItem( aEvent );
            updateSizesAfterRouterEvent( m_iface->GetPNSLayerFromBoardLayer( targetLayer ), m_endSnapPoint );
            m_router->Move( m_endSnapPoint, m_endItem );        // refresh
            return 0;
        }
    }
    ```
    and, when `SwitchLayer` refuses, falls through to arm a through via to `targetLayer` (sized as R6-R1 quotes). `LINE_PLACER::SetLayer` accepts the switch only while nothing is fixed from a via or through pad (`!m_startItem || ( m_startItem->OfKind( ITEM::VIA_T ) && m_startItem->Layers().Overlaps( aLayer ) ) || ( m_startItem->OfKind( ITEM::SOLID_T ) && ... )`).
  - `layerNext` / `layerPrev` there step through the copper layers in UI order from the router's current layer, skipping invisible ones (issue 14480).
- **How to see it.** 4-layer board. Start a route on a through-hole pad on F.Cu, move away, press PgDn (layerBottom). KiCad: the head jumps to B.Cu (`SetLayer` from a through pad). Then click to fix a segment and press `+` (layerNext): KiCad arms a via to In1.Cu, and the next click places it. Ours: both presses only change the Appearance panel's active layer, and the route stays on F.Cu.
  - Test (browser check, `router-leftovers.check.js`): start on a THT pad, `__eda.run("pcbnew.Control.layerBottom")`; assert `drawState.layer === "B.Cu"`. Fix, run `pcbnew.Control.layerNext`; assert `drawState.placingVia` and `pendingViaLayer === "In1.Cu"`.
- **Severity.** Medium: a missing router behaviour on common keys. No data is wrong, but the route does not do what the key asks.
- **Already known?** No. CODE-COMPARE-ui.md:39/:147 call these actions FAITHFUL, but only for the idle editor.

#### R6-R4. Route Selected's Autoroute does not use the ported `ROUTER::Finish`, and each autorouted connection is its own undo step

**Fixed** on task-r6-router: 9a88f288.

- **What differs.**
  - Ours: `useActionRunner.ts:2161-2176` (`routeSelected("auto")`). It does one `routeMove` to the ratsnest line's far end (`a.target`). It keeps the route only if `!head.colliding && head.snapped_end` and then `routeFinish` commits it. Each `routeFinish` is its own `board::step`, so each connection is one undo step.
  - The server already has KiCad's `ROUTER::Finish`: `route_api::attempt_finish` / `Router::attempt_finish` (`router.rs:618-641`: the nearest ratsnest anchor, up to five Moves until the end stops moving, then the layer check and `FixRoute`). The studio's F key uses it; Autoroute does not.
  - KiCad: `ROUTER_TOOL::RouteSelected` posts `routerAttemptFinish` for each anchor and then calls `performRouting`, so each connection goes through `ROUTER::Finish`:
    ```cpp
    // Keep moving until we don't change position or hit the limit
    int      triesLeft = 5;
    ...
    } while( placer->CurrentEnd() != moveResultPoint && triesLeft );

    // If we've made it, fix the route and we're done
    if( moveResultPoint == otherEnd && otherEndLayers.Overlaps( GetCurrentLayer() ) )
    ```
    The undo steps are grouped:
    ```cpp
    // We want autorouted tracks to all be in one undo group except for
    // any tracks that need to be manually finished.
    ...
    if( groupStart )
        groupStart = false;
    else
        m_iface->SetCommitFlags( APPEND_UNDO );
    ...
    if( !autoRouted )
        groupStart = true;
    ```
    When `Finish` fails, the route stays live with its head where `Finish` left it. Ours cancels the session and starts a fresh one at the anchor (`handOver( a.at, layer )`), so that head is lost.
  - Ours also shows toasts KiCad does not have: "Nothing to route: the selection has no unrouted connections." (`:2150`), "Autorouted %d of %d connections; finish the next one by hand." and "Autorouted %d of %d connections." (`RouteSelected` shows no message at all).
- **How to see it.** Select a footprint with three unrouted pads whose connections are free of obstacles. Run Autoroute (Shift+F), then Ctrl+Z. KiCad: one undo takes all three away. Ours: one undo per connection. A connection whose walkaround settles only on a second `Move` (springback or the optimizer moving the end) completes in KiCad and is handed back by hand in ours.
  - Test: `routeSelectedLoop` / route API test `autoroute_of_three_free_connections_is_one_undo_step` -- after the run, `undo` once and assert that no new track is left.
- **Severity.** Medium: wrong undo steps and a different success test.
- **Already known?** No (no earlier round reviewed Route Selected).

#### R6-R5. V pressed with the route tool armed but not yet routing does nothing; KiCad flips the active layer and starts the next route with a via (`m_startWithVia`)

**Fixed** on task-r6-router: 9a88f288 (the via's target is the pair's other layer, as V's, not the nearest ratsnest anchor's).

- **What differs.**
  - Ours: `pcbnew.Control.layerToggle` is registered only while a route or pair is drawn (`useActionRunner.ts:415`, `:497`). With the tool armed and idle, V has no handler. Nothing sets a "start with a via" flag either: `grep -ri startWithVia web/studio/src` finds nothing.
  - KiCad: `ROUTER_TOOL::MainLoop`:
    ```cpp
    else if( evt->IsAction( &ACT_PlaceThroughVia ) )
    {
        m_startWithVia = true;
        m_toolMgr->RunAction( PCB_ACTIONS::layerToggle );
    }
    ```
    and `performRouting`:
    ```cpp
    // If the user pressed 'V' before starting to route, enable via placement now
    if( m_startWithVia )
    {
        m_startWithVia = false;
        handleLayerSwitch( ACT_PlaceThroughVia.MakeEvent(), true );
    }
    ```
- **How to see it.** Press X, hover a pad, press V, click the pad, move. KiCad: the active layer has flipped to the pair's other layer, and the head starts with a via armed, so the first fix drops a via at the pad end of the leg. Ours: nothing changes until the click, and the route starts with no via.
- **Severity.** Medium (a missing interaction; no data is wrong).
- **Already known?** Partly. CODE-COMPARE-ui.md:105 lists `layerToggle` idle as DIVERGENT for the idle editor (`PCB_CONTROL::LayerToggle`). The router's `m_startWithVia` is not listed.

### Low

#### R6-R6. The active layer does not follow the route through a via (`syncRouterAndFrameLayer`) and is given back only after a start on another layer

**Fixed** on task-r6-router: 9a88f288 (GAPS item 7 corrected).

- **What differs.**
  - Ours: the only `SET_ACTIVE_LAYER` of the route tool is at the start (`routing.ts:41-45`, `diffPairRouting.ts:26-30`). `store.tsx:1624-1629` gives `originalActiveLayer` back only when the start switched the layer. A via placed mid-route changes `draw.layer` (`drawStateFromPreview`) but not `state.activeLayer`.
  - KiCad: `performRouting`'s `syncRouterAndFrameLayer` sets the frame's active layer to `m_router->GetCurrentLayer()` after each via or layer switch, and makes that layer visible:
    ```cpp
    editFrame->SetActiveLayer( pcbLayer );

    if( !getView()->IsLayerVisible( pcbLayer ) )
    ```
    `finishInteractive` always restores `frame()->SetActiveLayer( m_originalActiveLayer )`, which `prepareInteractive` saved whatever the start layer was. `onViaCommand` does the same when a via is taken back (`frame()->SetActiveLayer( m_iface->GetBoardLayerFromPNSLayer( m_router->GetCurrentLayer() ) )`).
  - GAPS item 7 (round 5, "the active layer follows it while it runs") claims more than the code does.
- **How to see it.** Start on an F.Cu pad with F.Cu active, V, click (the route goes on on B.Cu). KiCad: the layer selector and high contrast show B.Cu while routing, and F.Cu again after the route ends. Ours: F.Cu all along, so high contrast dims the layer being routed.
- **Severity.** Low (display and high contrast; the route itself is right).
- **Already known?** No. GAPS item 7 should be corrected.

#### R6-R7. Board Setup > Length-tuning Patterns refuses values KiCad accepts, and an imported project with such a value loses all three groups

**Fixed** on task-r6-router: ae2c616a (the reader takes each group as it is; the page's own refusals stay, documented as beyond KiCad's panel).

- **What differs.**
  - Ours: `crates/model/src/setup_pages.rs:230-252` (`TuningPatterns::validate`) refuses an amplitude or spacing <= 0 or above `MAX_MEANDER_NM`, a minimum amplitude above the maximum, and a radius > 100 %. It does so in our own words ("The minimum amplitude cannot be above the maximum amplitude.", ...).
  - The importer (`crates/kicad/src/import.rs:743`) clamps `corner_radius_percentage` to 0..100. At `:755-757` it drops all three read groups when any one fails `validate`, keeping the previous ones.
  - KiCad: `PANEL_SETUP_TUNING_PATTERNS::TransferDataFromWindow` (`panel_setup_tuning_patterns.cpp`) copies the twelve unit binders into the settings with no check at all. `BOARD_DESIGN_SETTINGS`' `tuning_pattern_settings` reader (`board_design_settings.cpp`, `read_settings`) takes each key as it is (`settings.m_cornerRadiusPercentage = entry["corner_radius_percentage"].get<int>();`), with no clamp.
- **How to see it.** A `.kicad_pro` with `"diff_pair_defaults": { "min_amplitude": 1.2, "max_amplitude": 1.0, ... }`, as made in KiCad by typing a minimum above the maximum. Import it: ours keeps the defaults for all three groups, so the single-track spacing the user set is lost too.
  - Test: `merge_project_setup_pages_keeps_each_group_as_kicad_reads_it` -- the single-track group's spacing of 0.9 mm survives next to an inverted diff-pair group.
- **Severity.** Low.
- **Already known?** No.

#### R6-R8. A shoved or dragged line made of several board tracks: each new segment takes its own track's mask and group, where KiCad gives the whole line the seed segment's

**Open** (task-r6-router): partly as described -- a shoved line of several tracks is already committed under one track (`ShoveOutcome`: the first of `source_tracks`), so its segments share one mask and group; but that track is the one whose id sorts first, not the seed. Fixing it needs the seed carried through `Line` (crates/pns/PARITY.md Stage 20).

- **What differs.**
  - Ours: `RouteCommit::push_tracks( .., Some(track_id) )` is called per board track of `displaced_tracks()` (`router.rs:944-948`, `:1373-1377`). So every replacement keeps the mask and group of the track it came from.
  - KiCad: a shoved line is assembled from many segments (`NODE::AssembleLine`, which gives the `LINE` its seed segment's source item). Its new segments are `SEGMENT( const LINE& aParentLine, const SEG& aSeg )` with `m_sourceItem = aParentLine.GetSourceItem();`. So every segment of the shoved line takes the seed's mask (`createBoardItem`) and group (`m_replacementMap[src]`).
  - Ours matches KiCad when all the line's tracks share a mask and group, which is the usual case.
- **How to see it.** Two collinear tracks of one net joined end to end: A with a solder-mask opening, inside group G; B with neither. Shove the pair of them with a route, hitting A. KiCad: both new segments have the opening and are in G. Ours: only A's replacement does.
- **Severity.** Low (an edge case of R5-2's fix).
- **Already known?** No (Stage 18 says the line placer's `displaced_tracks` carry the source, which is right per track).

### Checked and found to match (no need to review again)

**dc58601d.**
- `TOOL_BASE::pickSingleItem( pos, nullptr, -1 )`:
  - the slop loop `{ 0, maxSlopRadius }`, which ends on the first pass with any candidate;
  - `prioritized[0..3]` (pad/via on `tl`, track on `tl`, pad/via on any layer, track on any layer);
  - a track's distance is that of its nearer anchor;
  - the end pick's `aLayer >= 0` filter (`routePick.ts`'s `only`).
  - Server: `Router::pick_start`, `track_pick`. The documented differences (visibility, high contrast, layer order on another layer) are as stated.
- `ROUTER_TOOL::getStartLayer`: the active layer when the item overlaps it, else `ls.Start()` (`start_layer_of`, `start_diff_pair`).
- `PNS_KICAD_IFACE::createBoardItem` / `Commit`:
  - a segment or arc with a track source takes `HasSolderMask` and `GetLocalSolderMaskMargin` (ops `commit_route`: the entry and its margin);
  - a source that is in a group puts the new item in that group; a source in no group puts it in none (`if( m_itemGroups.contains( src ) )`);
  - an item with no source and a new via join the entered group;
  - the removed tracks' mask entries go.
  - A displaced via keeps its id, so its group and tenting stay. This is the same result as KiCad's `sourceVia->GetFrontTentingMode()` and the replacement map.
- `SplitAdjacentSegments` / `SplitAdjacentArcs` halves keep the split track as their source (`SEGMENT::Clone` copies `m_sourceItem`; `pns_arc.cpp:39`).
- `simplify_new_lines`: a merged line takes its seed's source (`AssembleLine`).
- `ImportSizes`' pair branch: with user dimensions, the width, gap and via gap all come from the user, over an inherited width (`else { diffPairWidth = bds.GetCurrentDiffPairWidth(); ... "user choice" }`), and the start checks judge them (`UserDpSizes`).

**a023fbf8.**
- `BOARD_EDITOR_CONTROL::TrackWidthInc` / `Dec` while routing (`stepTrackWidth`):
  - the first press with `m_UseConnectedTrackWidth && !m_TempOverrideTrackWidth` sets only the override;
  - otherwise the index steps, wrapping at both ends;
  - `UseCustomTrackViaSize( false )`.
- The pair branch while the pair tool is active, routing or not (`stepDiffPairIndex`, `UseCustomDiffPairDimensions( false )`).
- The source text after a change: "user choice" for an entry, the rule's at entry 0.
- `ROUTER_TOOL::UpdateMessagePanel`, word for word:
  - "Routing Track: %s" / "Resolved Netclass: %s";
  - "Routing Track" / "(no net)";
  - "Routing Diff Pair: %s" with the two classes joined by ", " when they differ;
  - "Corner Style" with "Free-angle", "45-degree", "45-degree rounded", "90-degree", "90-degree rounded";
  - "Mode" with "Highlight collisions", "Walk around", "Shove";
  - "Track Width: %s" / "(from %s)", "Min Clearance: %s", "Diff Pair Gap: %s".
  - The known differences: "DP Max Uncoupled-length", and the clearance not re-imported after a layer change.
- `NETCLASS::GetHumanReadableName`: "%s and %s", "%s, %s and %s", "%s, %s and %d more" (`human_readable_class`).
- `DIFF_PAIR_MENU` labels (`diffPairLabel`).

**8df3629b.** The `track_widths` / `via_dimensions` / `diff_pair_dimensions` `PARAM_LAMBDA`s:
- written in mm with the index-0 dummy first;
- on read, empty, non-object and key-less rows are skipped, then the dummy is dropped;
- an absent list leaves the design's own.

**(b), (c).**
- `ROUTER::Finish` as `Router::attempt_finish`: the nearest ratsnest anchor, five tries, `moveResultPoint == otherEnd && otherEndLayers.Overlaps( GetCurrentLayer() )`. The F key uses it.
- `MEANDER_SETTINGS::MEANDER_SETTINGS()` defaults (0.2 / 1.0 / 0.6 / 0.05 mm, round corners at 80 %, left side) and `SetTargetLength` / `SetTargetLengthDelay`.
- `MEANDER_PLACER_BASE::AmplitudeStep` / `SpacingStep` for a single track and a pair.
- `PlaceTuningPattern`'s 1-4 keys ("Select a track to tune first." without a pattern), and the stepped value carried to the next pattern of the session (`meanderSettings.m_spacing = ...`).
- `PCB_TUNING_PATTERN::CreateNew`'s Board Setup defaults per mode.
- The `tuning_pattern_settings` reader starting each group from `MEANDER_SETTINGS()`, including the 0.6 mm spacing for a diff-pair group that names none. The exceptions are R6-R7.
- `routeSelectedAnchors`: the selection order, a footprint's pads, the ratsnest edges whose end is the item.

---

## Section D: rules, DRC, Board Setup rule pages (round 6)

Read-only. Code read from main's checkout at 7d772e6e. KiCad: `kicad-src-8303b2ad`, and `kicad-src-25ec2335` where the box's kicad-cli (10.99.0, 25ec2335) is newer.
Scope: the round-5 merges 2e51b45e (sheet, fields, component classes on the rule board), 28cbdf9d (Barcode / Text Box / Table / Leader to the router's rules), d9a5c7f1;
then the DRC dialog's rows, Board Setup > Violation Severity and Custom Rules' syntax check, and active code that duplicates the ported rule engine or kicad-cli.

### High

#### R6-D1. `A.Orientation` of a footprint and of a pad has the wrong sign on the rule board (the router, Inspect, via placement and the component-class Rotation condition)

**Fixed** on task-r6-router: 30f07089.

- **What differs.**
  - Footprint: `crates/drc/src/rule_board.rs:326` `it.set("Orientation", Prop::Double(fp.rot as f64 / 1000.0));`.
    - `FootprintInstance::rot` is this model's clockwise angle. The importer stores `(at x y 90)` as `(-90000).rem_euclid(360000)` = 270000 (`crates/kicad/src/import.rs:441` `import_rot_millideg`).
    - The writer turns it back with `normalize180(-(rot + half))` (`crates/kicad/src/pcb.rs:1119` `kicad_footprint_orientation`, which also adds the half turn of a back footprint made here).
    - So a footprint KiCad has at 90° reads as `A.Orientation == 270` here.
  - Pad: `rule_board.rs:455` `it.set("Orientation", Prop::Double(-(p.orientation_mdeg as f64) / 1000.0));`.
    - `DrcPad::orientation_mdeg` is already in KiCad's sign, in 0..360 (`crates/drc/src/board.rs:154-155` doc, and line 634 negates the model's angle).
    - Negating it again gives -90 for a pad KiCad has at 90, and -180 for one at 180.
  - Both lines are from 6f9fce47 (2026-10-12), so they are not round-5 regressions.
  - KiCad:
    - `pcbnew/footprint.cpp:4910` registers `PROPERTY<FOOTPRINT, double>( _HKI( "Orientation" ), &FOOTPRINT::SetOrientationDegrees, &FOOTPRINT::GetOrientationDegrees, PROPERTY_DISPLAY::PT_DEGREE )`.
    - `GetOrientationDegrees()` is `return m_orient.AsDegrees();`, and `FOOTPRINT::SetOrientation` keeps it in (-180, 180] (`m_orient.Normalize180();`).
    - `pad.cpp:3390` does the same for the pad. Its `GetOrientationDegrees()` is `return m_padStack.GetOrientation().AsDegrees();`, and `PADSTACK::SetOrientation` keeps it in [0, 360) (`m_orientation.Normalize();`).
  - The R5-D1 fix makes this reachable from Board Setup. A component-class assignment with a Rotation condition compiles to `( A.Orientation == %s deg )` (`component_class_settings.cpp::GetAssignmentInDRCLanguage`, `getRotationExpr`). `rule_board.rs::dynamic_component_classes` evaluates that on these footprint items, so a "Rotation 90" class goes to the footprints KiCad has at -90 / 270.
- **How to see it.**
  - Test `rule_board_orientation_is_kicads`: build `hv_board` (rule_board.rs tests) with R5 at `rot: 270_000` and Top, which is KiCad's `(at x y 90)`.
  - Assert `holds(&rb, "A.Orientation == 90 deg", &r5)`.
  - Assert the same for R5's pad, with the pad at 0 in the footprint.
  - Both fail today: the footprint reads 270 and the pad -90.
  - Router scenario: a custom rule `(rule r (condition "A.Orientation == 90 deg") (constraint clearance (min 1mm)))` holds a route off a rotated footprint's pads in kicad-cli's DRC but not in the router.
- **Severity.** High. Rules and component classes match the wrong items in the router, Inspect (Clearance and Constraints Resolution), Draw Via and the via stitcher, all of which build `RuleBoard`.
- **Already known?** No.

### Medium

#### R6-D2. Multichannel's component classes are a second, divergent evaluator of the assignment rules; the rule board now runs KiCad's real one

**Fixed** on task-r6-router: 291b3dc7.

- **What differs.**
  - Since 2e51b45e, `rule_board.rs::dynamic_component_classes` compiles `GetAssignmentInDRCLanguage`'s text and evaluates it with the expression engine, as `COMPONENT_CLASS_ASSIGNMENT_RULE::Matches` does (`m_condition->EvaluateFor( aFootprint, nullptr, 0, aFootprint->GetSide(), nullptr )`).
  - `crates/ops/src/multichannel_facts.rs:131-194` (`condition_holds`, `assignment_matches`, `dynamic_classes`) is still active and is a hand-written imitation. It feeds Generate Placement Rule Areas, a rule area's component-class source and Repeat Layout. It disagrees with the rule board in these ways:
    - **Custom condition.** `ConditionKind::Custom => (!primary.is_empty()).then_some(false)`: a class assigned by a custom expression never applies. KiCad compiles `( <expr> )` and evaluates it.
    - **Side condition with an empty primary.** Ours gives `None` (dropped). KiCad's `getSideExpr` drops only `"Any"` and turns anything else that is not "Front" into `( A.Layer == 'B.Cu' )`. `rule_board.rs` does this right.
    - **Fields.** Multichannel's fields include `("Footprint", fpid)` (`multichannel_facts.rs:283`) and leave out Description. KiCad's `FOOTPRINT::GetField` has no Footprint field. That is the very difference R5-D1 fixed in `rule_board.rs::footprint_fields`.
    - **Sheet.** Multichannel takes the sheet from the stored schematic or `infer_modules`. The rule board takes it from `FootprintExtra::sheetname` or `symbol_links`. One footprint can be on two different sheets for `memberOfSheet` and for the sheet classes.
  - So the same footprint can have class X in the router and Inspect and not in Generate Placement Rule Areas, or the other way round.
- **How to see it.**
  - Add an assignment `{ class: "PWR", conditions: [Custom "A.Reference == 'R5'"] }`.
  - `rb.footprint_item("R5").footprint.classes` contains "PWR".
  - `Board::mc_board().fps[R5].classes` does not.
  - Test `multichannel_and_rule_board_agree_on_component_classes`.
- **Severity.** Medium: rule areas are generated by a component class KiCad would assign differently. Fix by reusing `RuleBoard`'s evaluator rather than keeping two.
- **Already known?** The Custom case is admitted in a code comment (`multichannel_facts.rs:157-158`), but not in GAPS or in a findings file.

#### R6-D3. Find by Properties' Query tab gives every footprint no sheet and no component class

**Fixed** on task-r6-router: 55418603.

- **What differs.**
  - `crates/drc/src/query.rs:146` builds the item's footprint as `FootprintFacts { reference, fpid, fields, ..Default::default() }`: `sheet: ""` and `classes: []`.
  - The studio's request has no field for either (`web/studio/src/kicad-port/pcbQuery.ts:35,196`), and the module doc still says "the studio sends no sheet path, so a footprint ... is on the root sheet" (`query.rs:12`).
  - The result:
    - `A.memberOfSheet('/HV/')` is false for every item;
    - `A.hasComponentClass('PWR')` is false;
    - `A.ComponentClass == 'PWR'` is false.
  - The rule board, since R5-D1, answers all three for the router and Inspect.
  - KiCad's `DIALOG_FIND_BY_PROPERTIES::selectMatchingFromQuery` evaluates the same `memberOfSheetFunc` / `hasComponentClassFunc` / `PCBEXPR_COMPONENT_CLASS_REF` on the board's real footprints (`fp->GetSheetname()`, `footprint->GetComponentClass()`).
- **How to see it.**
  - On a board imported with `(sheetname "/HV/")` and `(component_classes (class "PWR"))` on R5, Find by Properties > Query `A.hasComponentClass('PWR')` selects nothing; KiCad selects R5 and its pads.
  - Test in `query.rs`: a request whose R5 item carries those facts. The request has nowhere to put them, so the fix needs the field first.
- **Severity.** Medium (wrong selection; no data harmed).
- **Already known?** No. R5-D1 fixed only the rule board.

#### R6-D4. Violation Severity lists the checks of 8303b2ad, not those of the kicad-cli that runs the DRC: 12 checks cannot be set, and one listed check no longer exists

**Fixed** on task-r6-router: f67b52c0.

- **What differs.**
  - `web/studio/src/kicad/drc_checks.json` is generated from 8303b2ad (`"commit": "8303b2ada..."`).
  - The box's kicad-cli (25ec2335) has these in `DRC_ITEM::allItemTypes`, and so in `PANEL_SETUP_SEVERITIES` (which shows one row per item with a message):
    - `copper_outside_board` ("Copper outside board outline", default warning);
    - `malformed_microvia_stack_span`, `microvia_stack_not_filled` (warning), `microvia_stack_depth`;
    - `hole_aspect_ratio` ("Hole too deep for its diameter");
    - `microvia_crosses_core` (warning);
    - `via_stitch_overlap` ("Via-stitching zones overlap for same net");
    - `net_chain_stub_length`, `net_chain_return_path`, `net_chain_tuning_profiles`;
    - `footprint_scaled_with_pads` (warning).
    - The defaults are from 25ec2335 `board_design_settings.cpp:179-188`.
  - `zones_intersect` is listed here but is gone from 25ec2335's list.
  - kicad-cli reports these 12 checks in the DRC dialog, and the dialog's marker menu offers "Change severity" for them. The Board Setup page that the menu's "Edit violation severities..." opens has no row for any of them, so they cannot be set to Ignore there.
- **How to see it.**
  - A board with a via-stitching area overlapping another of the same net: kicad-cli reports `via_stitch_overlap`, and Board Setup > Violation Severity has no "Via-stitching zones overlap for same net" row.
  - Test in `boardSetupRules.test.ts`: every key kicad-cli's `--severity-all` report can give is a row of `drc_checks.json`.
- **Severity.** Medium (a missing setting).
- **Already known?** p13-p16 F4 lists the same keys for the exclusion table `drc_exclusions.rs::ERROR_TYPES`, not for the severity page.

#### R6-D5. Board Setup > Custom Rules' "Check rule syntax" does not say what KiCad says; the ported reporter is not used

**Fixed** on task-r6-router: 72c7b52e (`ExpandTextVars` / `ConvertCrossReferencesToKIIDs` not run first; `checkPlausibility` still not ported).

- **What differs.**
  - KiCad `panel_setup_rules.cpp::PANEL_SETUP_RULES::OnCompile`:
    - expands the text (`ConvertCrossReferencesToKIIDs`, `ExpandTextVars`);
    - runs `DRC_RULES_PARSER parser( rulesText, _( "DRC rules" ) ); parser.Parse( dummyRules, m_errorsReport ); checkPlausibility( dummyRules );`;
    - catches a lexer `PARSE_ERROR` as `"%s <a href='%d:%d'>%s</a>%s"` with `_( "ERROR:" )`, line, offset and `pe.ParseProblem()`;
    - says "No errors found." (`DRC_RULES_PARSER::Parse`) for a clean file.
  - Ours (`CustomRulesPage.tsx` -> `POST /api/check_rules` -> `crates/kicad-engine/src/lib.rs:1501` `check_rules`):
    - bisects the top-level forms with up to log2(n)+1 kicad-cli DRC runs on a probe board;
    - shows our own message and the first form that fails, with no reason and no column.
  - Yet `crates/drc/src/dru.rs:332` `parse_reporting` is an active port of `Parse( rules, reporter )`. It reports every error at its line:offset and "No errors found.", and the DRC Rule Editor already uses it (`rule_editor.rs:1893`).
  - The page's note "kicad-cli ignores a rules file it cannot parse without saying so" (`CustomRulesPage.tsx`) is stale for the box's kicad-cli. Its own `rules_load` (`lib.rs:1491-1493`) handles the "DRC incomplete: could not compile custom design rules" that this kicad-cli stops with.
- **How to see it.**
  - Text `(version 1)\n(rule a (constraint clearance (min 1mm))` (missing `)`):
    - KiCad: `ERROR: <line:offset> Missing ')'.` (or the lexer's problem text);
    - ours: a generic verdict with "Go to line 2".
  - Text `(version 1)\n(rule a (constraint clearance (min 1mm)))`:
    - KiCad: "No errors found.";
    - ours: a kicad-cli verdict string.
- **Severity.** Medium (messages not KiCad's; slow, with several kicad-cli runs). The plausibility warnings (`checkPlausibility`) are listed as not ported in PARITY-pcb section 9 and are not counted here.
- **Already known?** No. GAPS line 240 records the kicad-cli route as a choice, not as a difference.

#### R6-D6. The DRC dialog's "Refill all zones before performing DRC" starts unchecked; KiCad's starts checked

**Fixed** on task-r6-router: 0c4efd45.

- **What differs.**
  - `web/studio/src/state/store.tsx:1039` has `drcRefillZones: false`, and the box's tooltip says "Off by default" (`DrcDialog.tsx:229`).
  - KiCad `dialog_drc_base.cpp:46` (8303b2ad and 25ec2335): `m_cbRefillZones->SetValue(true);`. `DIALOG_DRC` never reads it from settings.
  - So a first Run DRC in KiCad judges freshly filled zones, and ours judges the stored (possibly stale) fills.
- **How to see it.** Open Inspect > Design Rules Checker on a new session: the box is unticked. Test: the store's initial state has `drcRefillZones === true`.
- **Severity.** Medium (wrong default).
- **Already known?** No.

#### R6-D7. `A.Locked` is always false on the rule board

**Fixed** on task-r6-router: e550feb3.

- **What differs.**
  - `rule_board.rs:328` (footprint), `:457` (pad), `:505` (track) and `:565` (via) all set `Prop::Bool(false)`, though `design.drawings.locked_ids` holds the locked footprints, tracks and vias.
  - KiCad: `BOARD_ITEM`'s "Locked" property is `IsLocked()`. Pads mask it: `pad.cpp:3388` `propMgr.Mask( TYPE_HASH( PAD ), TYPE_HASH( BOARD_ITEM ), _HKI( "Locked" ) );`, so for a pad ours should not answer at all.
- **How to see it.** Lock a track, then add the rule `(rule l (condition "A.Locked == true") (constraint clearance (min 1mm)))`. Clearance Resolution on that track shows the board default, while KiCad shows rule `l`. Test: `holds(&rb, "A.Locked", &locked_fp)`.
- **Severity.** Medium (wrong rule match; such rules are rare).
- **Already known?** No.

### Low

#### R6-D8. Component-class assignment rules: an empty-class row or a `"` in a value is not refused as KiCad refuses it

**Fixed** on task-r6-router: 5d0378d9.

- **What differs.**
  - `SyncDynamicComponentClassAssignments` takes `CompileAssignmentRule` returning null as a failure:
    - `if( rule ) rules.emplace_back(..) else success = false;`
    - then `if( success ) m_assignmentRules = std::move( rules );` after `m_assignmentRules.clear();`.
  - `CompileAssignmentRule` returns null for an empty class (`if( ruleSource.empty() ) return nullptr;`) and for a text the DRC lexer cannot parse. A field value or reference with a `"` ends the `(condition "...")` string early.
  - So in KiCad one such row leaves the board with no dynamic classes at all.
  - Ours:
    - `rule_board.rs` filters out empty-class rows (`.filter(|a| !a.class.is_empty())`) and keeps the others;
    - it compiles the bare expression, so a `"` in a value is no error.
  - KiCad's own panel refuses such a row (`PANEL_ASSIGN_COMPONENT_CLASSES::Validate`: "Error with conditions for component class assignment %s"). Only a hand-edited or foreign `.kicad_pro` gets here.
- **How to see it.** A project with assignments `[{class:"", ...}, {class:"PWR", conditions:[]}]`: kicad-cli gives no footprint PWR, while ours gives every footprint PWR.
- **Severity.** Low.
- **Already known?** No.

#### R6-D9. The DRC dialog's "All" box is derived from the other three; KiCad's is a box of its own

**Fixed** on task-r6-router: 5752f359.

- **What differs.**
  - Ours: `DrcDialog.tsx:260` `checked={allShown(view.filter)}`.
  - KiCad: `DIALOG_DRC::OnSeverity` only pushes All's state into the others (`m_showErrors->SetValue( true ); m_showWarnings->SetValue( aEvent.IsChecked() ); m_showExclusions->SetValue( aEvent.IsChecked() );`). Nothing ever sets `m_showAll` back.
  - So unticking Warnings leaves All ticked in KiCad and unticks it in ours, and ticking the three by hand leaves KiCad's All unticked.
- **Severity.** Low (cosmetic).
- **Already known?** No.

### Checked and found to match (no need to review again)

- **2e51b45e, `PCBEXPR_COMPONENT_CLASS_VALUE`** (`pcbexpr_evaluator.cpp:242-340`) against `pcbexpr.rs` `Special::ComponentClass`:
  - two class values compare by class;
  - a string matches a constituent by `WildCompareString( .., false )` when it is a wildcard, else `IsSameAs( .., false )`, and falls through to the full name (`,`-joined, `GetFullClassNameForConstituents`);
  - `NotEqualTo` is `!isInConstituents && !isFullName`;
  - `PCBEXPR_COMPONENT_CLASS_REF::GetValue` resolves a pad or graphic through `GetParentFootprint()` and gives no value for an item with none.
- **`hasComponentClassFunc`:** the footprint or its parent footprint, `ContainsClassName` (exact `==` on the constituent names; a single class is its own constituent, `getOrCreateConstituentClass` `AddConstituentClass( newClass.get() )`).
- **`GetAssignmentInDRCLanguage`, each condition:**
  - Reference: trim, `wxSplit` on `,`, untrimmed elements, an empty text giving nothing;
  - Footprint: `A.Library_Link`;
  - Side: only "Any" dropped;
  - Rotation: trim, "Any" or not an int dropped;
  - Footprint field: both trimmed, both needed;
  - Custom and Sheet name: trimmed;
  - joined by `&&` / `||`, and no condition left means it always applies.
- **Sheet classes:** each footprint's `GetSheetname()`, skipping "" and "/", with `"` and `'` taken out of the name.
- **A rule that does not compile** leaves no dynamic rule at all (ours returns an empty list).
- **`sortClassNames`:** `wxString::Cmp`, which is a BTreeSet.
- **`getFieldFunc`:** an answer for a `PCB_FOOTPRINT_T` item only, `GetField` by exact name, first match, Reference, Value, Datasheet, Description, user fields, no Footprint field.
- **`memberOfSheet`'s source:** `GetSheetname()`, "" for a hand-placed footprint.
- **28cbdf9d:** `ENUM_MAP<KICAD_T>` (`common/eda_item.cpp:429-452`) names PCB_TEXTBOX_T "Text Box", PCB_TABLE_T "Table", PCB_DIM_LEADER_T "Leader", the other dimensions "Dimension", and PCB_BARCODE_T "Barcode".
  - `PNS_KICAD_IFACE_BASE::syncWorld` sends text boxes to `syncGraphicalItem`, tables to `syncTextItem`, barcodes to `syncBarcode` and dimensions to `syncDimension`, each with the item as parent.
  - The barcode's implicit physical clearance is counted in the search margin, as `QueryWorstConstraint` would count it.
  - Not covered: a footprint's own text boxes and barcodes (`syncWorld:2230-2243`), which `fp_art::footprint_copper_of` does not model; noted, not judged.
- **d9a5c7f1:**
  - the missing-argument texts "Missing netclass name argument to hasNetclass()" / "...hasExactNetclass()" / "Missing component class name argument to hasComponentClass()" (no period), as in `pcbexpr_functions.cpp:1367,1406,1467`;
  - `existsOnLayerFunc` reads the item before the argument;
  - `checkFootprint` (`board_inspection_tool.cpp:1007-1024`) keeps the last of the same logical pads, or the footprint.
- **DRC dialog:**
  - "Test for parity between PCB and schematic" starts unchecked (no `SetValue` in the base);
  - the Errors and Warnings boxes start on, Exclusions off;
  - All sets Errors on and Warnings / Exclusions to its own state (`OnSeverity`);
  - the Ignored Tests list is the checks at Ignore.
  - The menu, Delete, Save and exclusions were checked in p13-p16. "Report all errors for each track" is known as not ported (PARITY-pcb 9a).
- **Violation Severity page against 8303b2ad:** one row per item with a message, grouped by the `errorCode == 0` headings, Error / Warning / Ignore radios, and the defaults of `BOARD_DESIGN_SETTINGS`' constructor. The table sends only what differs.
- **Constraints page:** checked in round 2.
- **Rule Editor:** checked in round 3. Not reviewed again.

---

## Section Z: zones, connectivity and Cleanup (round 6)

Read-only. Code read from main's checkout (`/home/team_braneboard_com/work/agentic-eda`, at 7d772e6e). KiCad: `/home/team_braneboard_com/src/kicad-src-8303b2ad` (via stitching: `kicad-src-25ec2335`, which is where it lives).

Scope:
- the round-5 merges ed941af7 (R5-Z1, crossing fills), f17789de (R5-Z2, fills kept through moves), 90fad5a1 (R5-Z4/Z6, Cleanup's merge), 08d8e21e (R5-Z5, Auto-Assign Priorities) and 21761e94 (R5-Z3, Zone Properties on no net);
- rows no round had reviewed: via stitching's placement (`crates/drc/src/via_stitch.rs` against `PCB_VIA_STITCH::Update` / `buildPlacementCells`), the Zone Fill Strategy rows of Board Setup > Constraints, and the zone-fill edges of the edit tools (Duplicate, Paste, Flip).

---

### High

None.

### Medium

#### R6-Z1. A duplicated or pasted zone has no fill; in KiCad the copy carries the original's fill

**Fixed** on task-r6-router: 21a343f3.

- **What differs.**
  - KiCad: `EDIT_TOOL::Duplicate` (`pcbnew/tools/edit_tool.cpp`, `case PCB_ZONE_T: ... dupe_item = orig_item->Duplicate( true, &commit );`) clones the zone. The copy constructor `ZONE::InitDataFromSrcInCopyCtor` (`pcbnew/zone.cpp`) copies the fill and the flag:
    ```cpp
    m_isFilled                = aZone.m_isFilled;
    ...
    std::shared_ptr<SHAPE_POLY_SET> fill = aZone.m_FilledPolysList.at( layer );
    if( fill )
        m_FilledPolysList[layer] = std::make_shared<SHAPE_POLY_SET>( *fill );
    ```
    No `UnFill()` follows: the only `UnFill()` calls in pcbnew's tools are the point editor, Delete of a cutout, `ZONE_CREATE_HELPER` and `BOARD_EDITOR_CONTROL::ZoneDuplicate` (Duplicate Zone onto Layer, `newZone->UnFill();`). The interactive move after the duplicate then moves the fill with the copy (`ZONE::Move`). Copy/Paste is the same: the clipboard is written by `PCB_IO_KICAD_SEXPR::format( const ZONE* )`, which writes every `(filled_polygon ..)` whatever the control flags ("// Save the PolysList (filled areas)"), and the paste parses them back.
  - Ours: the fill cache is keyed by zone id (`crates/kicad/src/zone_fills.rs:114`, `KicadFills::current`: `let entry = self.zones.get(&zone.id)?;`), and Duplicate and Paste mint a new id for every copied zone (`crates/ops/src/pcb_paste.rs:651-656`, `z.id = mint("zone", ...)`). Nothing copies the original's entry to the new id (`crates/cli/src/fill_api.rs` has no hook for it). The copy is drawn as an outline, has no copper in live connectivity (`KicadFills::copper`), and is written `(fill)` with no `(filled_polygon ..)` to every derived `.kicad_pcb` (the DRC's, Save As) until the next B.
- **How to see it.** Fill a GND pour (B), select it, Ctrl+D and drop the copy elsewhere (or Ctrl+C, Ctrl+V). KiCad: the copy shows the original's copper, moved with it (out of date, so KiCad's DRC says the fills are out of date). Ours: the copy is an empty outline; a pad that only the copy's copper reaches is unconnected in our ratsnest. Test: `zone_fills.rs` `a_duplicated_zone_carries_its_fill`: a `KicadFills` with zone `z` filled, then the design after `Cmd::Duplicate { ids: [z] }` and a move of 5 mm; assert `fills.rings(&copy, "F.Cu")` equals the original's rings moved by 5 mm (today it is empty).
- **Severity.** Medium: the same stale-fill semantics R5-Z2 ported for a moved zone, missing for its copies (display and live connectivity until a refill; a refill makes the two the same).
- **Already known?** No. GAPS 13 (l.576-590) lists move, turn, mirror, flip and Zone Properties only.

#### R6-Z2. Auto-Assign Priorities: a no-net copper zone never counts the unconnected pads and NPTH holes in its overlap; KiCad counts them as its own

**Fixed** on task-r6-router: b8ce9d43.

- **What differs.**
  - KiCad `computeConstraint` (`pcbnew/zone_utils.cpp`) compares net codes, and a no-net zone and an unconnected pad (or an NPTH pad) are both net code 0:
    ```cpp
    int netCodeA = aPair.zoneA->GetNetCode();
    ...
    if( intersection.Contains( aPos ) )
    {
        if( aNetCode == netCodeA )
            countA++;
        else if( aNetCode == netCodeB )
            countB++;
    }
    ```
    The pads loop takes every pad on a shared layer (`pad->IsOnLayer( layer )`), NPTH pads included (their layer set is `*.Cu *.Mask`).
  - Ours (`crates/ops/src/zone_priority.rs:114-122`): `match net { Some(n) if n == net_a => .., Some(n) if n == net_b => .., _ => {} }`. An unconnected or NPTH pad has `net: None` in `DrcBoard` (`crates/drc/src/board.rs`, `pin_net.get(..)`), and a no-net zone has `net == ""`, so neither ever matches: the no-net zone's count is always 0.
- **How to see it.** A no-net copper pour (thieving, shield) overlapping a GND pour, with three mounting holes (NPTH) in the overlap and no GND pad there. KiCad: 3 against 0, ratio 1 >= 0.2, the no-net pour goes above GND. Ours: 0 against 0, decided by area (the smaller one wins). The fill then differs. Test: `zone_priority.rs` `a_no_net_zone_counts_the_unconnected_pads_in_its_overlap`: zones `gnd` (GND, 10 mm square) and `thief` (net "", 20 mm square) overlapping, three NPTH pads of no net in the overlap; assert `thief` gets the higher priority (today `gnd`, the smaller).
- **Severity.** Medium: a wrong result (which pour wins the overlap) in a real, if uncommon, layout.
- **Already known?** No (R5-Z5 and the round-5 "checked" list judged the counts for nets only).

### Low

#### R6-Z3. A flipped zone keeps `(fill yes)`; KiCad's `ZONE::Flip` leaves it filled-polygons-but-not-filled

**Fixed** on task-r6-router: e7182c29.

- **What differs.** `ZONE::Flip` (`pcbnew/zone.cpp`) calls `SetLayerSet( flipped )`, which, when the set changes (a one-sided zone), runs `unFillLocked()` (`m_isFilled = false;`) and then puts the mirrored polygons back with `SetFilledPolysList( newLayer, shape )`, which does not set `m_isFilled`. So a flipped F.Cu zone has its fill on B.Cu but `IsFilled()` is false: the writer prints `(fill` without ` yes` (`if( aZone->IsFilled() ) m_out->Print( " yes" );`), `PCB_CONTROL::unfilledZoneCheck` shows its infobar, `ZoneFillDirty` refills it, `intersectsArea` and the disallow test (`drc_test_provider_disallow.cpp:128`, `if( copperZone->IsFilled() && ..`) skip it. Ours keeps the entry's flag (`crates/kicad/src/zone_fills.rs` `current`: `unfilled: entry.unfilled`), so `crates/kicad/src/pcb.rs:334` writes `(fill yes)`.
- **How to see it.** Fill a one-sided F.Cu pour, flip it (F), export: ours `(fill yes (thermal_gap ..`; KiCad's save `(fill (thermal_gap ..`, the `(filled_polygon (layer "B.Cu") ..)` the same in both. Test: `zone_fills.rs` `a_flipped_one_sided_zone_is_written_not_filled`: `filled_rect()`, the zone mirrored onto B.Cu; assert `fills.current(&flipped).unwrap().unfilled` (today false).
- **Severity.** Low (a flag; the copper is the same; the derived board is refilled before DRC on request).
- **Already known?** No. (A zone on both outer layers is GAPS 13's known "taken as mirrored"; there `SetLayerSet` sees the same set and keeps `m_isFilled`, so only the one-sided case is this.)

#### R6-Z4. Cleanup's merge: a different-width track that the dialog's filter leaves out still blocks the merge

**Fixed** on task-r6-router: 6c9ae9be.

- **What differs.** `TRACKS_CLEANER::cleanup`'s `mergeSegments` collects the different-width neighbours only among items the filter keeps:
  ```cpp
  if( candidate->Type() == PCB_TRACE_T && !candidate->HasFlag( IS_DELETED )
      && !filterItem( candidate ) )
  ```
  Ours (`crates/connectivity/src/cleanup.rs:623-634`, `has_different_width_attached`) tests every track on the layer and takes no `Protected` argument, so a filtered-out neck-down track still stops the merge (a locked one should still block, as `filterItem` is not the lock: that part is right).
- **How to see it.** Two collinear 0.25 mm segments A-B of net N1, a 0.15 mm track of net N2 touching A's far end; Cleanup with "Filter items" on net N1 and Merge on. KiCad merges A and B; ours does not. Test: `cleanup.rs` `merge_ignores_a_filtered_out_different_width_track`, `compute_cleanup_filtered` with a net filter; assert one `MergedTracks` change.
- **Severity.** Low (only with the filter on).
- **Already known?** No.

#### R6-Z5. Board Setup > Constraints: Minimum thermal relief spoke count accepts 11-99; KiCad's spin control stops at 10

**Fixed** on task-r6-router: 80556d92.

- **What differs.** `panel_setup_constraints_base.cpp` (8303b2ad and 25ec2335): `m_minResolvedSpokeCountCtrl = new wxSpinCtrl( .., wxSP_ARROW_KEYS, 0, 10, 0 );`: the field cannot go above 10, and says nothing. The 0..99 range is only the project file's (`PARAM<int>( "rules.min_resolved_spokes", .., DEFAULT_MINRESOLVEDSPOKES, 0, 99 )`). Ours (`web/studio/src/kicad-port/boardSetupRules.ts:253`, `crates/model/src/rules.rs:223`) takes up to 99 and shows "Value must be between 0 and 99." beyond, a message KiCad never shows for this field.
- **How to see it.** Type 25 in the field: KiCad shows 10; ours keeps 25 and Apply saves it.
- **Severity.** Low.
- **Already known?** Round 2 (r2-route-rules l.161) judged the 0..99 range against the loader only, not the control.

---

### Checked and found to match (no need to review again)

- **ed941af7 against `CN_VISITOR::checkZoneZoneConnection`** (`connectivity_algo.cpp:861-911`): the valid-outline guard (fewer than 3 points), the layer test (a zone item is one layer), the box test, A's vertices against B behind B's box then B's against A behind A's box, and `CN_ZONE_LAYER::ContainsPoint` with the edge counting (`SHAPE::Collide( p )` at 0 clearance; ours `point_in_polygon || edge distance <= 0`). The fragments are the fractured `filled_polygon` rings, as `m_outline` is (`items.rs:333-365`).
- **f17789de against `ZONE::Move` / `Rotate` / `Mirror` / `Flip`, `ZONE::SetLayerSet` and `PCB_EDIT_FRAME::Edit_Zone_Params`:**
  - `Edit_Zone_Params` does `zoneInfo.ExportSetting( *aZone )` (full export) and no `UnFill()`; `ExportSetting` calls `SetLayerSet( m_Layers )`, which unfills only when the set changes (`if( m_layerSet != aLayerSet )`): ours keeps the fill on a settings or net edit and drops it on a layer change.
  - `Move` / `Rotate` / `Mirror` transform `m_FilledPolysList`; `Flip` is `Mirror` plus every fill to `FlipLayer( oldLayer )`. The layer map in `follow` (copper reversed, `F.`/`B.` swapped) gives `FlipLayer`'s answer for any contiguous or non-contiguous copper set.
  - The point editor (`ZONE_POINT_EDIT_BEHAVIOR::UpdateItem`: `m_zone.UnFill()`) and Delete of a cutout (`zone->RemoveCutout(..); zone->UnFill();`) change the vertex or hole count or break rigidity, so `follow` returns `None`: emptied, as KiCad.
  - KiCad's `m_AutoRefillZones` defaults to false (`pcbnew_settings.cpp:64`), so a stale fill is what KiCad shows too.
  - Duplicate Zone onto Layer (`BOARD_EDITOR_CONTROL::ZoneDuplicate`: `newZone->UnFill();`) gives an unfilled copy in both.
- **21761e94 against `PANEL_ZONE_PROPERTIES::updateInfoBar`:** "<no net> will result in an isolated copper island." word for word, a warning only (OK stays enabled); `Edit_Zone_Params` keeps net 0 (`FindNet( 0 )` is the unconnected net item).
- **90fad5a1 against `mergeSegments` / `testMergeCollinearSegments`:** the different-width rule (straight `PCB_TRACE_T` neighbours only, arcs and vias not counted, no net test since tracks can change net in `CN_VISITOR`, the rule judged on the segment the pair is tried from, `candidate < segment` stood for by board order) and the lock and candidate filter, apart from R6-Z4. The overlap merge, the 1 nm `ApproxCollinear` tolerance and the kept identity of `aSeg1` are already listed as left (round 4 A-5, R5-2's note).
- **08d8e21e against `AutoAssignZonePriorities` (8303b2ad = 25ec2335 here):** `findOverlappingPairs` (edges of outlines and holes, `Contains` of the first vertex excluding holes), `computeConstraint`'s intersection with holes, `SHAPE_POLY_SET::Area` less the holes, the pad loop (`IsOnLayer` then `break`) and the via loop, the 20 % threshold, `assignPrioritiesFromGraph`, the union-find groups and the group maximum, the "changed?" return. Apart from R6-Z2. (KiCad's `std::sort` on priority alone is unstable; ours breaks ties by index.)
- **Via stitching placement (`via_stitch.rs` against 25ec2335 `pcb_via_stitch.cpp`):**
  - `cellForPosition` / `positionForCell` (`roundDiv`, the staggered half-pitch on odd rows), `Update`'s origin re-anchor from the first off-grid member (the stagger shift taken out), `childrenAreOnThisGrid` by layout, mode and pitch;
  - `InitializeDefaults` / `defaultPitch` (max of 2 mm and twice the via) / `defaultViaSize` 0.6 mm / `defaultViaDrill` 0.3 mm from the current sizes;
  - `buildPlacementCells`: net 0 makes nothing; the same-net fills (outlines in Guard mode) clipped to the outline's box, footprint zones included (`collectAllZones`); at least two via layers with copper; the "two layers overlap" accumulation; the intersection with the outline; the deflate by half the via (`ROUND_ALL_CORNERS`, 5 µm);
  - the obstacles: holes once each (hole-to-hole on `UNDEFINED_LAYER`, the hole clearance for another net), pads by `FlashLayer` whatever their net, same-net tracks and shapes skipped but same-net vias kept, member vias skipped, the margin `max( via/2 + clearance, drill/2 + hole clearance )` for another net, via keepouts inflated by half the via, a different-net zone's fill grown by its margin less its own outline;
  - the grid walk from `floor( (bbox - offset) / pitch )`, the Poisson tile (`mt19937` seed, two uniforms, the tile range), the guard's envelope margin (`via/2 + clearance + 5 µm + 2 µm`), the guard's quarter-pitch exclusion.
  - Not judged again: `SampleGuardEnvelope` and the Poisson tile's numbers (task-kicad10-review, PARITY-pcb section 50).
- **Fill / Unfill:** `ZoneFill` takes only the selected zones, `ZoneUnfill` / `ZoneUnfillAll` empty them. That they are undo steps in KiCad (`commit.Push( _( "Unfill All Zones" ), ZONE_FILL_OP )`) is GAPS 13's known "a fill is derived data, not an undo step".
- **Board Setup > Constraints, Zone Fill Strategy:** "Allow fillets/chamfers outside zone outline" (`zones_allow_external_fillets`, default false) and the spoke count's label and default, apart from R6-Z5.

---

## Summary

| ID | What | Severity | Known before? | task-r6-router |
|---|---|---|---|---|
| R6-R1 | V while routing arms a via of the board's default size, not the net class's / rule's / chosen list entry (`handleLayerSwitch`'s sizing) | high | no (only V's layer choice is listed) | Fixed d3af3fce |
| R6-R2 | A chosen track width below the board minimum is routed as it is; `ImportSizes` takes `max( m_TrackMinWidth, GetCurrentTrackWidth() )` | high | no | Fixed f365ded0 |
| R6-R3 | Mid-route layer keys (PgUp/PgDn, +/-, layerInnerN) only change the active layer; KiCad switches the route's layer or arms a via (`onLayerCommand`) | medium | no | Fixed 9a88f288 |
| R6-R4 | Autoroute does one Move rather than `ROUTER::Finish`, and makes one undo step per connection (KiCad: `APPEND_UNDO` across autorouted ones); our own toasts | medium | no | Fixed 9a88f288 |
| R6-R5 | V with the route tool armed and idle: no layer toggle and no `m_startWithVia` | medium | partly (CODE-COMPARE-ui: idle layerToggle) | Fixed 9a88f288 |
| R6-R6 | The active layer does not follow the route through vias (`syncRouterAndFrameLayer`); restored only after a start on another layer | low | no (GAPS 7 overstates) | Fixed 9a88f288 |
| R6-R7 | Length-tuning Patterns: our refusals where KiCad checks nothing; the importer clamps and drops all three groups on one bad value | low | no | Fixed ae2c616a |
| R6-R8 | A shoved multi-track line: each segment takes its own track's mask/group, KiCad the seed's for the whole line | low | no | Open |
| R6-D1 | `A.Orientation` of footprints and pads has the wrong sign on the rule board (rules, component-class Rotation) | high | no | Fixed 30f07089 |
| R6-D2 | Multichannel's component classes are a second, divergent evaluator (Custom, empty Side, Footprint field, sheet source) | medium | partly (code comment) | Fixed 291b3dc7 |
| R6-D3 | Find by Properties' Query gives footprints no sheet and no component class | medium | no | Fixed 55418603 |
| R6-D4 | Violation Severity lists 8303b2ad's checks, not those of the 25ec2335 kicad-cli that runs the DRC | medium | no | Fixed f67b52c0 |
| R6-D5 | Custom Rules' "Check rule syntax" is not KiCad's `OnCompile` output; the ported `dru.rs::parse_reporting` is unused there | medium | no | Fixed 72c7b52e |
| R6-D6 | DRC dialog's "Refill all zones before performing DRC" starts unchecked; KiCad's starts checked | medium | no | Fixed 0c4efd45 |
| R6-D7 | `A.Locked` is always false on the rule board | medium | no | Fixed e550feb3 |
| R6-D8 | Component-class rules: an empty class or a `"` makes KiCad drop every assignment rule; ours keeps the others | low | no | Fixed 5d0378d9 |
| R6-D9 | DRC dialog's "All" box is derived from the other three; KiCad's is its own | low | no | Fixed 5752f359 |
| R6-Z1 | A duplicated or pasted zone has no fill (the cache is keyed by id); KiCad's copy carries the fill | medium | no | Fixed 21a343f3 |
| R6-Z2 | Auto-Assign Priorities: a no-net zone never counts unconnected/NPTH pads (net 0) in its overlap | medium | no | Fixed b8ce9d43 |
| R6-Z3 | A flipped one-sided zone is written `(fill yes)`; KiCad's `Flip` leaves `IsFilled()` false | low | no | Fixed e7182c29 |
| R6-Z4 | Cleanup's merge: a filtered-out different-width track still blocks the merge | low | no | Fixed 6c9ae9be |
| R6-Z5 | Min thermal relief spoke count accepts 11-99 with a message; KiCad's spin control stops at 10 | low | partly (r2 judged the loader's range) | Fixed 80556d92 |

Counts: high 3, medium 11, low 8 (22 in all).
