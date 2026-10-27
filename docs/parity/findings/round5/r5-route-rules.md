# Round 5 findings: router, zones, rules, DRC, connectivity and Board Setup (`r5-route-rules`, 2026-10-28)

Reviewer: `r5-route-rules`, read-only. Code read from main's checkout (`/home/team_braneboard_com/work/agentic-eda`, at 8a8e47f1). KiCad: `/home/team_braneboard_com/src/kicad-src-8303b2ad`.

Nothing was built. Every finding comes from reading both sides, except R5-Z1, which kicad-cli confirmed on scratch boards in `/tmp/findings-r5-route-rules.3iyj/zz/`. The tests named are the ones I would write.

Scope, in priority order:
- (a) The round-4 merges: 747bbd9a / 3f855aa1 (`inheritTrackWidth`, `auto_track_width`, the head via's `IsFlashedOnLayer`, via ids kept on commit, the drag's via-first pick, the pair's start gap on arcs, `ImportSizes`' hole to hole), cca40225 (Board Setup's Differential Pairs list, `ValidateViaParameters`, the size choice by index), 919021d7, 45dcab1a.
- (b) Router behaviours next to them that are not ported: the start layer, the router's commit of replaced tracks, the width menu's flags, mid-route `W`, the pair's sizes at the start check, the message panel.
- (c) Rows no round reviewed: the DRC providers (section D below), zones and connectivity (section Z below).

---

## High

### R5-1. A route that starts on a pad or track of another layer than the active one starts with no net on the active layer (`ROUTER_TOOL::getStartLayer`)

- **What differs.**
  - Ours:
    - `web/studio/src/components/canvas/Canvas.tsx:746` sends the active layer: `const layer = state.activeLayer ?? board.layers[0] ?? "F.Cu";`.
    - `crates/pns/src/router.rs:362-399` (`pick_start`) looks only on that layer: `let layers = LayerRange::single(layer);` for `nearest_anchor`, and `segment_at(at, layer, ..)`.
    - So an SMD pad or a track on B.Cu, clicked with F.Cu active, is not found. `Router::start` (`router.rs:244-249`) then starts in empty space: `net` is `None`, and the route is on F.Cu.
  - KiCad:
    - `pns_tool_base.cpp::TOOL_BASE::pickSingleItem` collects candidates on every layer. `prioritized[0]` / `[1]` are the via/pad and the track on the top layer `tl`. `prioritized[2]` / `[3]` are the same kinds on any layer: `if( d < dist[2] ) { prioritized[2] = item; .. }`. The first non-null entry wins.
    - `TOOL_BASE::updateStartItem` drops an off-layer item only with grid snapping off: `if( !m_gridHelper->GetUseGrid() && m_startItem && !m_startItem->Layers().Overlaps( tl ) ) m_startItem = nullptr;`. With the grid on (the default), it keeps it.
    - `router_tool.cpp::ROUTER_TOOL::getStartLayer` then starts on the item's layer:
      ```cpp
      if( ls.Overlaps( startLayer ) )
          return tl;
      else
          return m_iface->GetBoardLayerFromPNSLayer( ls.Start() );
      ```
    - `prepareInteractive` makes that the active layer (`editFrame->SetActiveLayer( pcbLayer );`) and starts there.
- **How to see it.** A two-layer board with R1 on the bottom side, so its SMD pads are on B.Cu. Make F.Cu active, press X and click R1.1.
  - KiCad starts a route of R1.1's net on B.Cu and switches the active layer to B.Cu.
  - Ours starts a route with no net on F.Cu, where the click was. Finishing it commits a no-net F.Cu track over the bottom pad.
  - The same happens when starting on a B.Cu track end.
  - Test (pns): `a_route_started_on_a_pad_of_the_other_side_starts_on_its_layer`. Board: an SMD pad `R1.1` of `SIG` on B.Cu at (0, 0). Call `router.start(p(0, 0), "F.Cu", 250 µm)`. Assert `router.current_net() == Some("SIG")` and that the session's layer is B.Cu.
- **Severity.** High: wrong connectivity, because the route has no net. Bottom-side SMD parts are common.
- **Already known?** No. Neither GAPS item 7 nor CODE-COMPARE-router names `getStartLayer`.

### R5-2. The router's commit drops a replaced track's solder-mask opening and its group; new tracks are not put in the entered group

- **What differs.**
  - Ours:
    - `crates/ops/src/lib.rs:4679` (`commit_route`) removes every track in `remove_track_ids` and adds the router's tracks with cleared ids (`t.id.clear()`). `rt.assign_missing_ids()` then gives each a new content-derived id.
    - Nothing carries a removed track's `drawings.track_masks` entry (`crates/model/src/track_mask.rs`, keyed by track id) or its group membership (`drawings.groups[].member_ids`) to the tracks that replace it. A `grep` for `track_masks` finds no use in `crates/ops`, `crates/cli` or `crates/pns`.
    - So a track that is shoved or dragged loses its mask opening and leaves its group. A group of one then dissolves (`tidy_groups`).
    - A route committed while a group is entered is in no group.
  - KiCad:
    - Each new segment of a moved line keeps the source item (`pns_segment.h`, `SEGMENT( const LINE& aParentLine, .. )`: `m_sourceItem = aParentLine.GetSourceItem();`; `NODE::AssembleLine`: `pl.SetSourceItem( aSeg->GetSourceItem() );`).
    - `pns_kicad_iface.cpp::PNS_KICAD_IFACE::createBoardItem` copies the source's mask onto every new track and arc:
      ```cpp
      if( aItem->GetSourceItem() && aItem->GetSourceItem()->IsType( { PCB_TRACE_T, PCB_ARC_T } ) )
      {
          PCB_TRACK* sourceTrack = static_cast<PCB_TRACK*>( aItem->GetSourceItem() );
          track->SetHasSolderMask( sourceTrack->HasSolderMask() );
          track->SetLocalSolderMaskMargin( sourceTrack->GetLocalSolderMaskMargin() );
      }
      ```
    - It also files each new item under its source's group, or under the entered group when it has no source:
      ```cpp
      if( BOARD_ITEM* src = aItem->GetSourceItem() )
      {
          if( m_itemGroups.contains( src ) )
              m_replacementMap[src].push_back( newBoardItem );
      }
      else
      {
          // This is a new item, which goes in the entered group (if any)
          m_replacementMap[ENTERED_GROUP_MAGIC_NUMBER].push_back( newBoardItem );
      }
      ```
      `PNS_KICAD_IFACE::RemoveItem` records `m_itemGroups[parent] = group;`.
    - `PNS_KICAD_IFACE::Commit` puts them in: `group->AddItem( bi );` under `m_commit->Modify( group->AsEdaItem(), .. )`.
- **How to see it.** Import a board with a track that has `(layers "F.Cu" "F.Mask")`, as an exposed power trace, and is in a group with a footprint. Shove that track with a route next to it, or drag it with D.
  - KiCad: the moved track is still `(layers "F.Cu" "F.Mask")` and still in the group.
  - Ours: `(layer "F.Cu")`. The `track_masks` entry names an id that no longer exists, and the group lost the track.
  - Test (ops): `commit_route_carries_a_replaced_tracks_mask_and_group`. A design with track `trk_a` in group `{trk_a, R1}` and with `track_masks: [{id: trk_a}]`. Apply `CommitRoute { remove_track_ids: [trk_a], tracks: [moved] }` with the replacement's source named. Assert the new track has a mask entry and is a member of the group. This needs `CommitRoute` to carry each new track's source id, which `pns` has as `source_track`.
- **Severity.** High: data loss in the file. KiCad's DRC and fabrication outputs read the mask opening.
- **Already known?** No. r2-editors-exports D2 covers the Specctra import, where KiCad does *not* keep groups. p6 F2 covers handle drags and mirrors. The router's commit is not named anywhere.

### R5-3. The `.kicad_pro` we write has no `track_widths` and no `via_dimensions`: the user's pre-defined track widths and via sizes are lost in KiCad

- **What differs.**
  - Ours: `crates/kicad/src/pcb.rs:988-995` writes only the Differential Pairs list (`project["board"]["design_settings"]["diff_pair_dimensions"]`, new in cca40225). `grep -rn "track_widths\|via_dimensions" crates/kicad/src` finds nothing, so `RoutingSection::track_width_presets` and `via_presets` never reach the project file. The reader is missing too (`merge_project_predefined_sizes`, `import.rs:828`, reads only `diff_pair_dimensions`).
  - KiCad: `board_design_settings.cpp` has a `PARAM_LAMBDA` for each list. It writes `"track_widths"` and `"via_dimensions"` (`entry["diameter"] = pcbIUScale.IUTomm( via.m_Diameter ); entry["drill"] = ..`), each with the index-0 net-class dummy first, as for the pairs.
- **How to see it.** In the studio, Board Setup > Pre-defined Sizes: add track width 0.4 mm and via 0.8/0.4 mm, Apply, and export the KiCad project. Open it in KiCad: the track width and via size drop-downs hold only the net-class entries. The pair list added in cca40225 survives.
  - Test (kicad): `the_project_carries_the_predefined_track_widths_and_via_sizes`. A design with `track_width_presets [400 µm]` and `via_presets [{800, 400}]` gives `.kicad_pro` `track_widths == [0.0, 0.4]` and `via_dimensions == [{0,0},{0.8,0.4}]`. Reading that back with the merge gives the same lists.
- **Severity.** High: data loss, and a file KiCad reads differently.
- **Already known?** The reader half is p22 F5 (medium, for the importer). The writer half is not listed. cca40225 added the pair list to the writer but not the other two.

---

## Medium

### R5-4. A pair's start checks use the class sizes when the pair is routed with custom dimensions

- **What differs.**
  - Ours:
    - `crates/cli/src/route_api.rs:612-616` (`dp_start`) calls `router.start_diff_pair(at, layer)`. Only after it succeeds does it apply the request's `width` / `gap` / `via_gap` (`router.set_diff_pair_sizes(..)`).
    - The checks inside `start_diff_pair` (`router.rs:1156-1162`) and `DiffPairPlacer::start` (`dp_placer.rs:361-392`) use `ImportSizes`' class sizes: "Diff pair gap is less than board minimum clearance.", the start-gap check and "The routing start point violates DRC.".
    - The studio sends the custom dimensions while "Use Custom Values..." is on (`Canvas.tsx:766-767`).
  - KiCad:
    - `ROUTER_TOOL::prepareInteractive` calls `m_iface->ImportSizes( sizes, m_startItem, .. )` and then `m_router->UpdateSizes( sizes )` before `m_router->StartRouting(..)`.
    - `ImportSizes` takes the user's dimensions when the pair is not on the net class: `else { diffPairWidth = bds.GetCurrentDiffPairWidth(); diffPairGap = bds.GetCurrentDiffPairGap(); .. }`, and `GetCurrentDiffPairGap` returns `m_customDiffPair.m_Gap` while `m_useCustomDiffPair`.
    - So `ROUTER::isStartingPointRoutable`'s `if( m_sizes.DiffPairGap() < m_sizes.MinClearance() )` and `int configuredGap = m_sizes.DiffPairGap() + m_sizes.DiffPairWidth();` judge the custom sizes.
- **How to see it.** Class: 0.2 mm width, 0.15 mm gap. Existing P/N tracks: 0.1 mm wide at a 0.3 mm pitch (a neckdown). Custom dimensions: 0.1 / 0.2. Start the pair on the tracks.
  - KiCad: |0.3 − 0.3| ≤ 0.03, so the pair starts.
  - Ours: the configured pitch is 0.1 + 0.15 = 0.25 mm. The stubs' own width wins through `inheritTrackWidth`, so the class width plays no part. |0.3 − 0.25| > 0.025, so it refuses with "The differential pair gap at the start point does not match the configured gap. ...".
  - The reverse case: a custom gap below the board minimum is refused by KiCad and accepted by ours.
  - Test (cli route_api): `a_pair_with_custom_dimensions_is_checked_with_them_at_the_start`. With the board above, `dp_start {x, y, layer, width: 100_000, gap: 200_000}` replies `ok`.
- **Severity.** Medium: a wrong refusal or acceptance at the start.
- **Already known?** No. R4-4 fixed the arc case of the same check.

### R5-5. The Differential Pairs list of Pre-defined Sizes cannot be chosen: no entries in the pair menu, and W does not step it while routing a pair

- **What differs.**
  - Ours:
    - `web/studio/src/components/canvas/pcbMenuBuilder.ts:111-118` (`diffPairMenu`) still has only "Use Net Class Values" and "Use Custom Values...". Its doc says "this studio keeps no list of pair sizes besides the custom one", which is stale since cca40225.
    - `useActionRunner.ts:463-464` says of the pair: "no width cycling".
    - `dpStart` (`api/client.ts:682`) sends only the custom values.
    - So `routing.diff_pair_presets` is written and read, but nothing routes with it.
  - KiCad:
    - `router_tool.cpp::DIFF_PAIR_MENU::update` lists every entry after index 0, with these labels:
      - `_( "Width %s" )` when the gap and the via gap are both ≤ 0;
      - `_( "Width %s, via gap %s" )` when only the via gap is set;
      - `_( "Width %s, gap %s" )` when only the gap is set;
      - `_( "Width %s, gap %s, via gap %s" )` when both are set.
      
      Each entry is checked when `!bds.UseCustomDiffPairDimensions() && bds.GetDiffPairIndex() == (int) i`. Choosing one runs `SetDiffPairIndex`, which also turns custom off.
    - `board_editor_control.cpp::TrackWidthInc` steps the pair list while a pair is being routed:
      ```cpp
      if( routerTool && routerTool->IsToolActive()
          && routerTool->Router()->Mode() == PNS::PNS_MODE_ROUTE_DIFF_PAIR )
      {
          int widthIndex = bds.GetDiffPairIndex() + 1;
          ..
          bds.SetDiffPairIndex( widthIndex );
          bds.UseCustomDiffPairDimensions( false );
      ```
    - `ImportSizes` then takes `GetCurrentDiffPairWidth/Gap/ViaGap()`, which is that list's entry.
- **How to see it.** Add a pair 0.1 / 0.1 / 0.15 mm in Pre-defined Sizes. Right-click while routing a pair: KiCad offers "Width 0.1 mm, gap 0.1 mm, via gap 0.15 mm"; ours offers only the two fixed items. Pressing W while routing a pair changes nothing in ours.
  - Test (web): `diffPairMenu lists the board's pair sizes with KiCad's labels`. With `diff_pair_presets [{200k, 0, 0}, {100k, 100k, 150k}]`, assert the labels "Width 0.2 mm" and "Width 0.1 mm, gap 0.1 mm, via gap 0.15 mm".
- **Severity.** Medium: the list exists but cannot be used, so one of KiCad's routes to pair sizes is missing.
- **Already known?** No. R4-3 asked for the list. Its fix added the page and the file I/O, not the choosing.

### R5-6. W / Shift+W while routing steps a fixed ladder of ten widths, not the board's list, and the first press does not keep the index

- **What differs.**
  - Ours:
    - While a route is live, `web/studio/src/actions/useActionRunner.ts:446-456` (`cycleWidth`) steps `WIDTH_PRESETS_NM = [100_000, 150_000, .., 1_000_000]` (`:128`) from the head's width.
    - It does not use `[class width, ...track_width_presets]` or `currentTrackWidthIndex`. Those are used only when no route is live (`:1065`).
    - The ladder's comment, and `crates/pns/PARITY.md:240`, say: "this project has no per-board 'preferred widths' list". That is no longer true: `routing.track_width_presets` is the list.
  - KiCad, `board_editor_control.cpp::TrackWidthInc`:
    ```cpp
    if( routerTool && routerTool->IsToolActive()
        && routerTool->Router()->GetState() == PNS::ROUTER::RouterState::ROUTE_TRACK
        && bds.m_UseConnectedTrackWidth && !bds.m_TempOverrideTrackWidth )
    {
        bds.m_TempOverrideTrackWidth = true;
    }
    else
    {
        widthIndex++;
    }
    ```
    - It sets `SetTrackWidthIndex( widthIndex )` and runs `trackViaSizeChanged`. `onTrackViaSizeChanged` re-runs `ImportSizes`, so the route takes `m_TrackWidthList[index]`, or the net class's width at index 0.
    - So the first W of a route that inherited a stub's width goes to the current list entry, and only later presses step the board's list.
- **How to see it.** Pre-defined widths [0.3, 0.6]. Start a route on a pad with a 0.15 mm stub, so it inherits 0.15. Press W.
  - KiCad: the class width (index 0), then 0.3, then 0.6.
  - Ours: 0.2, then 0.25, then 0.3.
- **Severity.** Medium: wrong widths on a common key.
- **Already known?** In part. PARITY.md:240 documents the ladder, but the reason it gives no longer holds. GAPS does not list it.

### R5-7. The router's width menu turns "Use Starting Track Width" off where KiCad only overrides it for the current route

- **What differs.**
  - Ours:
    - `web/studio/src/actions/pcbMenuActions.ts:52-73`: "Track …" (`studio.Router.setTrackWidth`) and "Via …" (`studio.Router.setViaSize`) both dispatch `autoTrackWidth: false`.
    - `routerViaActions.ts:117`: Custom Track/Via Size's OK does the same.
    - `boardControlActions.ts:206`: the Auto Track Width toggle only flips the flag, and never clears custom sizes.
    - `useActionRunner.ts:1062`: W / Shift+W with no live route steps the index but leaves `useCustomTrackVia` on.
  - KiCad, `router_tool.cpp::TRACK_WIDTH_MENU::eventHandler` (the menu's event is consumed there, so `PCB_EDIT_FRAME::Tracks_and_Vias_Size_Event` does not run):
    ```cpp
    else if( id >= ID_POPUP_PCB_SELECT_VIASIZE1 && id <= ID_POPUP_PCB_SELECT_VIASIZE16 )
    {
        bds.UseCustomTrackViaSize( false );
        bds.SetViaSizeIndex( id - ID_POPUP_PCB_SELECT_VIASIZE1 );
    }
    else if( id >= ID_POPUP_PCB_SELECT_WIDTH1 && id <= ID_POPUP_PCB_SELECT_WIDTH16 )
    {
        bds.UseCustomTrackViaSize( false );
        bds.m_TempOverrideTrackWidth = true;
        bds.SetTrackWidthIndex( id - ID_POPUP_PCB_SELECT_WIDTH1 );
    }
    ```
    - Neither branch touches `m_UseConnectedTrackWidth`. `CustomTrackWidthDialog` sets only `m_TempOverrideTrackWidth = true; bds.UseCustomTrackViaSize( true );`.
    - `prepareInteractive` clears the override at the next start, so the next route that starts on a stub inherits its width again.
    - `BOARD_EDITOR_CONTROL::AutoTrackWidth`: `if( bds.UseCustomTrackViaSize() ) { bds.UseCustomTrackViaSize( false ); bds.m_UseConnectedTrackWidth = true; } else { .. toggle .. }`.
    - `SetTrackWidthIndex` and `SetViaSizeIndex` set `m_useCustomTrackVia = false`.
- **How to see it.** Choose "Track 0.4 mm" from the router's menu, then start a route on a pad with a 0.15 mm stub.
  - KiCad: 0.15 mm, and "Use Starting Track Width" is still checked.
  - Ours: 0.4 mm, and "Use Starting Track Width" is unchecked.
  - The same happens after choosing a via size, which in KiCad has nothing to do with the width.
  - Test (web): `pcbMenuActions: choosing a width keeps autoTrackWidth`.
- **Severity.** Medium: a wrong default width after a common menu choice.
- **Already known?** No. R4-1's fix made the server inherit the width, but the studio's flags still turn inheritance off.

### R5-8. The router's message panel is not ported (`ROUTER_TOOL::UpdateMessagePanel`): the width, clearance and gap with the rule they came from

- **What differs.**
  - Ours: `web/studio/src/components/MessagePanel.tsx` shows the selection only, and its header says the fields are "this app's own reasonable approximation". `ImportedSizes::width_source` (`crates/pns/src/settings.rs`) is computed but nothing shows it: `grep width_source web/studio/src` finds nothing, and `/api/route/start` does not send it.
  - KiCad, while routing:
    - `_( "Routing Track: %s" )` with `_( "Resolved Netclass: %s" )`, or `_( "Routing Diff Pair: %s" )`;
    - `_( "Corner Style" )` with "45-degree", "45-degree rounded", "90-degree", "90-degree rounded" or "Free-angle";
    - `_( "Mode" )` with "Highlight collisions", "Walk around" or "Shove";
    - `_( "Track Width: %s" )` / `_( "(from %s)" )` with the width source ("existing track", "netclass 'Default'", "user choice", a rule's name);
    - `_( "Min Clearance: %s" )` (from source);
    - for a pair, `_( "Diff Pair Gap: %s" )` and `_( "DP Max Uncoupled-length: %s" )`.
- **How to see it.** Start a route on a pad with a stub. KiCad shows "Track Width: 0.15 mm (from existing track)"; the studio shows nothing about the route.
- **Severity.** Medium: in KiCad this panel is how a user sees which rule set the width or clearance.
- **Already known?** No. GAPS item 7 lists only the tuner's panel lines.

---

## Low

### R5-9. The chosen width and via are kept by value: a Board Setup edit to the list does not reach them

- **What differs.**
  - Ours: `state/store.tsx:1638` stores `currentTrackWidthNm` with the index, and `chosenTrackWidth` (`kicad-port/routerVia.ts:162`) returns that stored value. The via preset is the same (`currentViaPreset`).
  - KiCad: `BOARD_DESIGN_SETTINGS::GetCurrentTrackWidth` reads the list at route time: `return m_TrackWidthList[ m_trackWidthIndex ];`. `GetCurrentViaSize` is the same.
- **How to see it.** Choose preset 1 (0.3 mm). In Board Setup, change it to 0.35 mm and Apply. KiCad routes at 0.35 mm; ours still routes at 0.3 mm. After a removal, ours can route at a width that is no longer in the list.
- **Severity.** Low.
- **Already known?** No (R4-8's fix moved to indices but kept the value).

### R5-10. Pre-defined Sizes' Apply: no sort, Add rows pre-filled, and track widths refused with our own words

- **What differs.**
  - `PANEL_SETUP_TRACKS_AND_VIAS::TransferDataFromWindow` sorts all three lists (`// Sort lists by increasing value`: `sort( trackWidths.begin(), .. ); sort( vias.. ); sort( diffPairs.. );`, by `VIA_DIMENSION::operator<` diameter then drill, `DIFF_PAIR_DIMENSION::operator<` width, gap, via gap). Ours keeps the user's order (`BoardSetupDialog.tsx:218-225`, `ops set_*_presets`), and it is the order of the menu and of W.
  - `OnAddTrackWidthsClick` / `OnAddViaSizesClick` / `OnAddDiffPairsClick` add a 0 width or diameter with empty gap / drill cells (`AppendViaSize( 0, 0 )`, `AppendDiffPairs( 0, 0, 0 )`). Ours pre-fills the Default class's values (`BoardSetupDialog.tsx:337, 365, 399-409`).
  - `PANEL_SETUP_TRACKS_AND_VIAS::Validate` checks no track width. Ours refuses a width ≤ 0 with "every preset width must be positive" (`crates/ops/src/lib.rs:3864-3868`).
- **How to see it.** Add 0.6 mm and then 0.3 mm and Apply. KiCad's menu reads 0.3 then 0.6; ours reads 0.6 then 0.3.
- **Severity.** Low.
- **Already known?** No.

### R5-11. `merge_project_predefined_sizes` drops raw row 0 before it filters out malformed rows

- **What differs.**
  - Ours: `crates/kicad/src/import.rs:833` drops the first raw row (`.skip(1)`) and only then filters out the malformed ones.
  - KiCad: `board_design_settings.cpp`'s `diff_pair_dimensions` loader filters first (`if( entry.empty() || !entry.is_object() ) continue;`, then the missing keys), keeping every well-formed row. Index 0 of what is left becomes the net-class entry.
- **How to see it.** `[{}, {width 0, gap 0, via_gap 0}, {0.2, 0.15, 0.15}]`. KiCad keeps 0.2/0.15 as the only preset. Ours also keeps the all-zero row as a preset, which shows as a "Width 0 mm" entry.
- **Severity.** Low; only a hand-edited or damaged project has such rows.
- **Already known?** No.

---

## Checked and found to match (no need to review again)

**Round-4 router fixes (747bbd9a, 3f855aa1).**
- `settings::inherit_track_width` against `PNS_KICAD_IFACE_BASE::inheritTrackWidth`:
  - a track's or arc's own width first;
  - for a via or pad, the joint's linked segments and arcs less the item;
  - with a start position not (0, 0), the one on `m_startLayer` whose far end (the anchor farther from the pad or via centre) is nearest the cursor;
  - else the narrowest on the start layer, else the narrowest of all;
  - `false` for any other kind.
- `import_sizes`: the inheritance is applied over the rule sizes with source "existing track", for the route and the pair alike.
- The pair's inheritance ignores `m_TempOverrideTrackWidth` in KiCad (`if( bds.m_UseConnectedTrackWidth && aStartItem )`). The custom pair width the studio sends then overrides it, as KiCad's `else` branch does (apart from R5-4's timing).
- `route_api::start`: the connected width wins over a sent width, as `prepareInteractive` clears `m_TempOverrideTrackWidth` before `ImportSizes`. The default is on, as `m_AutoTrackWidth( true )`.
- `Node::collides_item`'s `head_flashed`, against `ITEM::collideSimple`:
  - the head's `IsFlashedOnLayer( aHead, Layers() )` turns off both the copper-to-copper clearance and the item-hole-to-head test (`holeI->collideSimple( aHead )` asks the same question with `this` = the hole);
  - the head's own hole still collides.
- `RuleResolver::is_via_flashed_on`: the range form, over the intersection of the two layer ranges.
- A via with no board source is flashed on every layer it spans. That matches `PNS::VIA::ConnectsLayer` with `KEEP_ALL`, the mode a router-made via gets.
- The flashed layers come from the board via's connectivity at its original place, as `PCB_VIA::FlashLayer` reads the unmoved parent.
- `ROUTER::CommitRouting` keeps a dragged via as an update of the same parent (`changed`), so keeping its id and extras (`commit_route`'s `remove_via_ids.contains(&v.id)`) is right.
- `DiffPairPlacer::start`:
  - the start-gap check runs for `SEGMENT_T | ARC_T`;
  - `actualGap` is `|AnchorP − AnchorN|`, `configuredGap` is `DiffPairGap() + DiffPairWidth()`, and the tolerance is `configuredGap / 10` in integers;
  - the message matches word for word.
- `import_rule_sizes`:
  - `CT_HOLE_TO_HOLE` of `dummyVia, dummyVia, UNDEFINED_LAYER` and of the coupled via, asked outside `if( aStartItem )`;
  - `SetDiffPairHoleToHole( max(..) )`;
  - the pair's dummies at `Anchor( 0 )`, the single track's at the anchor nearer the cursor, which is chosen for `SEGMENT_T` only (not `ARC_T`, as in KiCad).
- **Not judged:** `drag_start`'s via-first pick. KiCad's D goes through `EDIT_TOOL::Drag`'s selection filter (`GuessSelectionCandidates`, the "via attached to two tracks" reduction), not `pickSingleItem`. A via with two tracks resolves to the via in both. A via with one stub goes through KiCad's area heuristic (a via counts as r², not πr²), which ours does not model. This is not reported as a defect.

**Board Setup, cca40225.**
- `set_via_presets` / `validateViaParameters` against `PCB_VIA::ValidateViaParameters`:
  - `GEOMETRY_MIN_SIZE` is 1 µm;
  - the order is diameter, then drill, then the two "not defined" messages, then `aDiameter <= aPrimaryDrill`;
  - the words match: "Via diameter is too small.", "Via drill is too small.", "No via hole size defined.", "No via diameter defined.", "Via hole size must be smaller than via diameter".
- The pairs: `ValidateDesignRules`' `m_Width > 0 && m_Gap <= 0`, giving "No differential pair gap defined.". The via gap and zero-width rows are not checked.
- The project's `diff_pair_dimensions` is written in mm with the `{0, 0, 0}` dummy first.
- `chosenTrackWidth` / `chosenViaSize` follow `GetCurrentTrackWidth` / `GetCurrentViaSize`'s range check (index ≤ 0 or past the end is the net class). A microvia ignores the list.
- `BOARD_DESIGN_SETTINGS::GetCurrentDiffPairWidth/Gap/ViaGap`'s fallbacks were read for R5-5: the Default class's pair width or track width, gap or clearance, and via gap or gap.

**Router rows read for (b) and found to match.**
- `TOOL_BASE::snapToItem`:
  - a pad snaps to its nearest `AnchorPoints()` entry, or `Anchor( 0 )` when it has none (`node.rs:390`);
  - a via snaps to its centre;
  - a track or arc snaps to an end within half its width, else to its nearest point (`router.rs:369-392`).
- `PNS_KICAD_IFACE::createBoardItem` for vias: type, unconnected-layer mode and layer pair. This is Stage 11's `via_kinds`, plus R4-2's kept extras for a moved via.

---

## Section D: the DRC slice (crates/drc, the rule engine as the router and Inspect use it)


Read-only. Main's checkout (`/home/team_braneboard_com/work/agentic-eda`), KiCad `kicad-src-8303b2ad`. Nothing was built or run.

Scope: `crates/drc` (the rule engine, `pcbexpr`, `rule_board`, `inspect`, `query`), as the router (`crates/pns/src/rule_resolver.rs`), the Clearance/Constraints Resolution reports (`crates/cli/src/inspect_api.rs`) and Find by Properties' Query (`POST /api/pcb/query`) use it. DRC itself is kicad-cli's (`crates/drc/src/lib.rs:10-13`), so no test provider was reviewed. Earlier rounds had not reviewed `pcbexpr.rs`, `inspect.rs`, `query.rs`, `rule_board.rs` or `rule_item.rs`.

---

### High

#### R5-D1. The rule engine's footprints have no sheet path, component classes or fields, though the IR now carries them. kicad-cli's DRC sees them, but the router and Inspect do not.

- **What differs.**
  - Ours, `crates/drc/src/rule_board.rs:237-245` (`build_footprints`):
    ```rust
    fields: vec![("Reference".into(), fp.id.clone()), ("Value".into(), value.clone()), ("Footprint".into(), fpid.clone())],
    ..
    sheet: String::new(),
    ```
    - `hasComponentClass` is a constant `0` (`crates/drc/src/pcbexpr.rs:1015-1018`: `if f == Func::HasComponentClass { ctx.push(Value::number(0.0)); return; }`).
    - `fromTo` is a constant `0` (`pcbexpr.rs:962-966`).
  - The data exists now:
    - `crates/model/src/lib.rs:1085-1094` (`FromSchematic`) has `sheetname` ("`/` for the root, `/LED channels/` below it") and `fields` ("Datasheet, Description, the simulation fields, the user's").
    - `crates/kicad/src/pcb.rs:1224-1239` writes `(sheetname ..)` / `(sheetfile ..)` into the derived `.kicad_pcb`.
    - `FootprintExtra::component_classes` holds the component classes (GAPS line 207).
    - So kicad-cli's DRC evaluates `memberOfSheet`, `getField` and `hasComponentClass` on real values, while ours evaluates them on empty ones.
  - KiCad, `pcbexpr_functions.cpp::memberOfSheetFunc`:
    ```cpp
    wxString sheetName = fp->GetSheetname();
    ..
    if( sheetName.Matches( refName ) ) return 1.0;
    ```
  - KiCad, `getFieldFunc`: `PCB_FIELD* field = fp->GetField( arg->AsString() ); if( field ) return field->GetText();`. `FOOTPRINT::GetField( const wxString& )` (`footprint.cpp:695`) walks `m_fields`: Reference, Value, Datasheet, Description and the user fields. **No "Footprint" field** (`FOOTPRINT::FOOTPRINT` adds only `REFERENCE`, `VALUE`, `DATASHEET`, `DESCRIPTION`, `footprint.cpp:114-117`). So ours answers `getField('Footprint')` with the library id, where KiCad answers `''`.
  - `hasComponentClassFunc` reads the footprint's component class. `fromToFunc` reads `GetConnectivity()->GetFromToCache()`.
  - Since round 2 the router's clearance, width and `ImportSizes` all come from this engine (`crates/pns/src/rule_resolver.rs:327`: `RuleBoard::new(..)`). So a custom rule keyed on any of these is enforced by kicad-cli and ignored by the router.
  - `crates/drc/src/query.rs:12` has the same gap for Find by Properties: "the studio sends no sheet path, so a footprint ... is on the root sheet".
- **How to see it.**
  - Setup: a two-sheet project, with R5 on sheet `/HV/`. `.kicad_dru`: `(rule hv (condition "A.memberOfSheet('/HV/')") (constraint clearance (min 1mm)))`.
  - Route a track of another net past R5's pad at 0.3 mm (board clearance 0.2 mm). Our router accepts it, and kicad-cli `pcb drc` reports `clearance` (1.0 mm). Clearance Resolution shows "board setup constraints" or "netclass 'Default'" instead of rule `hv`.
  - The same happens with `A.getField('Voltage') == '400V'` (a user field) and with `A.hasComponentClass('HV')`.
  - Test (drc): `rule_board_footprint_has_its_sheet_and_fields`. Design: R1 placed from sheet `/HV/` with a user field `Voltage=400V`. Assert:
    - `RuleBoard::new(..).footprint_item("R1")` has `sheet == "/HV/"`;
    - `getField('Voltage')` evaluates to `"400V"`;
    - `getField('Footprint')` evaluates to `""`;
    - `memberOfSheet('/HV/')` is 1 for R1's pad.
- **Severity.** High: the router places copper that kicad-cli flags. Inspect also names the wrong rule.
- **Already known?** Partly. GAPS line 836 lists "component classes and sheet paths in a condition (`hasComponentClass`, `memberOfSheet` say false); `fromTo`; a footprint's user fields in `getField` (only Reference, Value and Footprint)" under the Clearance Resolution report. That was written when the IR had none of it. Not listed:
  - that the IR now has sheet paths, fields and component classes;
  - that the router uses this engine, so the gap now produces DRC violations;
  - the extra "Footprint" field, which KiCad does not have.

#### R5-D2. The router's copper barcodes, text boxes and tables are typed as "Graphic" to the rules, so the implicit 1 mm barcode clearance is never applied

- **What differs.**
  - Ours, `crates/pns/src/from_ir.rs`:
    - the barcode obstacle is `add_obstacle(.., Class::Shape)` (around line 450);
    - the table is `Class::Shape` (around line 463);
    - the text box goes through `add_shape` and `rules.add_graphic(.., Class::Shape)` (line 385).
  - `Class::Shape.type_name()` is `"Graphic"` (`crates/drc/src/rule_item.rs:55`). Every dimension is `Class::Dimension`, a leader included, where KiCad says "Leader".
  - `crates/pns/src/rule_resolver.rs:391-411` then drops the barcode rule on purpose: "the barcodes' implicit 1 mm physical clearance, `A.Type == 'Barcode'`, is left out: it never holds for the router's items" and `ImplicitSource::BarcodeDefaults => true` in the `plain` shortcut. That stopped being true when R3-2 (f44f6bdd) put barcodes into the router's world.
  - KiCad:
    - `pns_kicad_iface.cpp::syncBarcode`: `solid->SetParent( aBarcode );`.
    - `syncGraphicalItem` (`:1811`, also for `PCB_TEXTBOX_T`) and `syncTextItem` (`:1923`, `PCB_TABLE_T`) do `solid->SetParent( aItem );`.
    - `PNS_PCBNEW_RULE_RESOLVER::QueryConstraint` evaluates the parent board item, so `A.Type` is `ENUM_MAP<KICAD_T>`'s "Barcode", "Text Box" or "Table" (`common/eda_item.cpp:432-452`).
    - `PNS_PCBNEW_RULE_RESOLVER::Clearance` always also asks `CT_PHYSICAL_CLEARANCE` ("if( QueryConstraint( PNS::CONSTRAINT_TYPE::CT_PHYSICAL_CLEARANCE, aA, aB, layer, &constraint ) ) .. rv = constraint.m_Value.Min()").
    - `DRC_ENGINE::loadImplicitRules` 2b: `createImplicitRule( _( "barcode visual separation default" ), DRC_IMPLICIT_SOURCE::BARCODE_DEFAULTS )` with `PHYSICAL_CLEARANCE_CONSTRAINT` min `mmToIU( 1.0 )` and condition `A.Type == 'Barcode'`.
    - So KiCad's router keeps a track 1 mm (less the epsilon) from a copper barcode.
- **How to see it.**
  - A QR barcode on F.Cu and a SIG track routed past it at 0.3 mm (board clearance 0.2 mm). Ours lets it through. KiCad's router holds it 1 mm off, and kicad-cli `pcb drc` reports the 1 mm physical clearance.
  - A custom rule `(condition "A.Type == 'Text Box'")` is likewise missed by our router.
  - Test (pns): `a_copper_barcode_keeps_its_1mm_physical_clearance`. A world with one barcode on F.Cu: assert `RuleResolver::clearance(track, barcode_solid) >= 1 mm - epsilon`. Second test: `add_graphic` for a text box gives `A.Type == 'Text Box'`.
- **Severity.** High: the router places copper that DRC flags. Copper barcodes are rare.
- **Already known?** No. The comment in `rule_resolver.rs` and `crates/pns/PARITY.md:1104` predate the barcodes in the world.

---

### Low

#### R5-D3. `existsOnLayer('')` is a compile error in ours and compiles silently in KiCad

- **What differs.**
  - Ours, `pcbexpr.rs` `call_func`'s `_` arm (around line 1047) checks the argument before the item: `if arg.as_string(ctx).is_empty() { ctx.report_error(&missing(..)) ..}` and only then `let Some(it) = item else ..`. The preflight run has no items, so the message is told at compile.
  - KiCad, `existsOnLayerFunc` checks the item first: `if( !item ) return;` and only then `if( !arg || arg->AsString().IsEmpty() ) { .. ReportError( MISSING_LAYER_ARG( wxT( "existsOnLayer()" ) ) ) }`. The preflight (`libeval_compiler.cpp:951-970`) runs with no item, so nothing is reported at compile. Every other one-argument function checks the argument first, as ours does.
- **How to see it.** Find by Properties > Check Syntax on `A.existsOnLayer('')`: KiCad shows "Syntax OK", and ours shows "Syntax error: Missing layer name argument to existsOnLayer().". The rule editor's own compile does the same.
  - Test: `compile("A.existsOnLayer('')")` has no errors.
- **Severity.** Low.
- **Already known?** No.

#### R5-D4. The missing-argument messages of `hasNetclass`, `hasExactNetclass` and `hasComponentClass` are not KiCad's

- **What differs.**
  - Ours, `pcbexpr.rs:778-788` `missing()`: these three names fall to `_ => format!("Missing {what} argument to {name}")` with `what = "name"`, which gives "Missing name argument to hasNetclass()".
  - KiCad:
    - `hasNetclassFunc`: `_( "Missing netclass name argument to hasNetclass()" )`;
    - `hasExactNetclassFunc`: `_( "Missing netclass name argument to hasExactNetclass()" )`;
    - `hasComponentClassFunc`: `_( "Missing component class name argument to hasComponentClass()" )` (none of the three has a final period).
  - `getField` with an empty name reports nothing in either (KiCad reports only `if( !arg )`).
- **How to see it.** Check Syntax on `A.hasNetclass('')`.
  - Test: `compile` errors equal "Missing netclass name argument to hasNetclass()".
- **Severity.** Low (message).
- **Already known?** No.

#### R5-D5. Clearance Resolution of a footprint: the stand-in pad differs for unnumbered pads, and ours takes the first pad where KiCad takes the last

- **What differs.**
  - Ours, `rule_board.rs:773-782` (`footprint_stand_in`): `if pads.iter().all(|(_, i)| self.board.pads[*i].number == first.number) { return pad_item(first) }`.
  - KiCad, `board_inspection_tool.cpp::reportClearance`'s `checkFootprint`: `if( !foundPad || pad->SameLogicalPadAs( foundPad ) ) foundPad = pad; else return footprint;`.
    - `PAD::SameLogicalPadAs` (`pad.h:160`) is `GetParentFootprint() == aOther->GetParentFootprint() && !m_number.IsEmpty() && m_number == aOther->m_number`.
    - So two unnumbered pads (a mounting-hole pair) keep the **footprint**, which gives a different set of pages (courtyard instead of copper).
    - When all pads share a number, the stand-in is the **last** pad, not the first.
- **How to see it.**
  - Inspect Clearance between a footprint with two unnumbered NPTH holes and a track. KiCad treats the footprint as a footprint: no copper page, courtyard pages if both have courtyards. Ours uses the first hole as a pad.
  - Test: `footprint_stand_in` of a footprint with pads `["", ""]` returns the footprint.
- **Severity.** Low.
- **Already known?** No.

---

### Checked and found to match (no need to review again)

- **`DRC_ENGINE::loadImplicitRules` sections 1, 2a and 2b** (`constraints.rs:764-818`):
  - the names "board setup constraints", "zone fill strategy", "silk", "silk text height", "silk text thickness", "hole", "edge", "courtyard", "micro-via" and "barcode visual separation default";
  - the constraints and minimums, the silk layer conditions, the micro-via condition `A.Via_Type == 'Micro'`, and the barcode's 1 mm `PHYSICAL_CLEARANCE` with `A.Type == 'Barcode'`.
  - (Section 3 was checked in round 4.)
- **`PCBEXPR_BUILTIN_FUNCTIONS`:**
  - the table of names, the deprecated aliases included (`insideCourtyard`, `insideFrontCourtyard`, `insideBackCourtyard`, `insideArea`, `memberOf`);
  - `isPlated` (a PTH pad or any via);
  - `isMicroVia`, `isBlindVia`, `isBuriedVia`, `isBlindBuriedVia`;
  - `memberOfGroup` (its group, else its footprint's, walked up, `Matches`);
  - `memberOfFootprint` (`GetParentFootprint`, so not a footprint itself);
  - `inDiffPair` (the suffix, the complement on the board, the base name or the base before a trailing `_`);
  - `isCoupledDiffPair` (the `DIFF_PAIR_GAP` / `LENGTH` / `SKEW` shortcut, then B's net);
  - `hasNetclass` / `hasExactNetclass` (connected items only);
  - `memberOfSheet` / `memberOfSheetOrChildren` matching (trailing `/`, root, level-by-level wildcards). This is on the values given, apart from R5-D1. Ours trims every trailing `/`, KiCad only one, which only matters for a `//` path.
  - The missing-argument texts of `existsOnLayer`, the courtyard functions, the area functions, `memberOfGroup`, the sheet functions, `memberOfFootprint` and `inDiffPair` are word for word.
- **`PCBEXPR_UCODE::CreateVarRef`:** `null`, then `NetClass` / `ComponentClass` / `NetName` / `Type`, then `A`/`AB`/`B`/`L`. `true` / `false` are a deliberate, documented extension for Find by Properties only (`pcbexpr.rs:150-153`). KiCad's compiler says "Unrecognized item 'false'" even though its own `formatValueForExpression` writes `false`.
- **`ENUM_MAP<KICAD_T>` names** for the classes the rule board builds: Footprint, Pad, Track (arcs too), Via, Zone, Text (fields too), Graphic, Dimension, Group. The exceptions are in R5-D2.
- **`DIALOG_FIND_BY_PROPERTIES`:**
  - `selectMatchingFromQuery`: the compile first, then each item on each layer of its layer set, matched when non-zero;
  - `onCheckSyntaxClick`: "Syntax OK", "Syntax error: ", "Expression is empty";
  - `queryUsesUnsupportedPairwiseSyntax`'s "B. expressions are not supported.".
  - These are split between `query.rs` and `web/studio/src/kicad-port/findByProperties.ts`.
- **`BOARD_INSPECTION_TOOL::reportClearance`** (`inspect.rs:207-622`). Apart from R5-D5, these match line for line:
  - the zone/pad swap;
  - the Zone page (thermal gap, spoke width, minimum spoke count; the "None" branch with the zone's clearance, physical and physical-hole overrides and their texts);
  - the same-net short cut (`GetNetCode() > 0`);
  - the per-layer Clearance page, active layer first;
  - the Diff Pair page and its "No 'diff_pair_uncoupled' constraints defined.";
  - the silkscreen pages with the tented-mask note;
  - the courtyard pages;
  - the Hole page (hole clearance layer choice, hole to hole on `UNDEFINED_LAYER`);
  - the Edge.Cuts and Margin pages;
  - Physical Clearances (`PhysicalLayersMask` = board tech layers plus copper; CrtYd for a footprint against an edge; "Items share no relevant layers.  No 'physical_clearance' constraints will be applied.");
  - the physical hole clearance.
- **`InspectConstraints`** (`inspect.rs:626-770`):
  - the pages and the item types they apply to:
    - Track Width: `PCB_TRACE_T` only, not arcs;
    - Via Diameter and Via Annular Width: on `UNDEFINED_LAYER`;
    - Hole Size: a pad with a drill, or a via;
    - Solder Mask: pad, shape, or any `PCB_TRACK` (vias included);
    - Solder Paste: abs and rel, with KiCad's three blank lines in its odd place;
    - Text Size;
    - Keepouts: "Item <b>disallowed</b> at current location." / "Item allowed at current location.";
    - Assertions;
  - each message word for word.

---


---

## Section Z: zones and connectivity


Read-only. Code read from main's checkout (`/home/team_braneboard_com/work/agentic-eda`, at 8a8e47f1). KiCad: `/home/team_braneboard_com/src/kicad-src-8303b2ad`. kicad-cli 10.99.0 (25ec2335) was run on scratch boards in `/tmp/findings-r5-route-rules.3iyj/zz/`.

Scope:
- the round-4 merge 45dcab1a (Cleanup's merge asks `ZONE::HitTest`);
- `crates/connectivity` (`algo.rs`, `items.rs`, `geom.rs`, `ratsnest.rs`, `cleanup.rs`);
- `crates/ops/src/zone_priority.rs` (Auto-Assign Priorities: marked ported in GAPS 13, no earlier round reviewed it);
- the zone fill cache's invalidation rule (`crates/kicad/src/zone_fills.rs::zone_signature`);
- Zone Properties (`web/studio/src/components/ZoneDialog.tsx`, `ops::edit_zone`).

---

### High

#### R5-Z1. Two same-net zone fills that cross without a vertex of one inside the other are connected in ours; KiCad leaves them unconnected

- **What differs.**
  - Ours: `crates/connectivity/src/geom.rs:287`, `(ItemShape::Zone { outline: oa }, ItemShape::Zone { outline: ob }) => polygons_overlap(oa, ob)`. `polygons_overlap` (`geom.rs:120-140`) also returns true when any edge of one crosses an edge of the other (`segments_intersect`).
  - KiCad, `connectivity_algo.cpp::CN_VISITOR::checkZoneZoneConnection` tests only vertices:
    ```cpp
    const SHAPE_LINE_CHAIN& outlineA = aZoneLayerA->GetOutline();

    for( int i = 0; i < outlineA.PointCount(); i++ )
    {
        const VECTOR2I& pt = outlineA.CPoint( i );

        if( !boxB.Contains( pt ) )
            continue;

        if( aZoneLayerB->ContainsPoint( pt ) )
        {
            aZoneLayerA->Connect( aZoneLayerB );
            aZoneLayerB->Connect( aZoneLayerA );
            return;
        }
    }
    ```
    The same loop then runs over `outlineB` against A. Edge crossings are never tested.
- **How to see it (run).** The scratch board is `/tmp/findings-r5-route-rules.3iyj/zz/zz.kicad_pcb`. It has two GND fills on F.Cu forming a "+": a horizontal bar (100..110, 104..106) and a vertical bar (104..106, 100..110). Each bar has a GND pad outside the crossing.
  - `kicad-cli pcb drc` reports `Missing connection between items: Zone [GND] on F.Cu, priority 0 / Zone [GND] on F.Cu, priority 0`.
  - Control case: shorten the vertical bar to end at y = 105, so one of its vertices lies inside the other fill (`zz2.kicad_pcb`). kicad-cli then reports 0 unconnected items.
  - Ours joins the two fills in one cluster, so the studio shows no ratsnest line and the net counts as routed.
  - Test: `two_crossing_fills_without_a_vertex_inside_are_not_connected` in `algo.rs`. Use two `ZoneCopper` fills shaped as above, each with an N1 pad. Assert `search_clusters` gives 2 clusters for N1. The control case (one vertex inside) gives 1.
- **Severity.** High: wrong connectivity, and the studio disagrees with kicad-cli's `unconnected_items`. It is rare, because it needs two same-net pours that cross like this.
- **Already known?** No. GAPS 36's "left" list does not name it.

#### R5-Z2. Moving, rotating, flipping or mirroring a zone, or OK in Zone Properties, throws away the fill; KiCad keeps it (moved with the zone, or as it was)

- **What differs.**
  - Ours: a fill is kept only while `zone_signature` (`crates/kicad/src/zone_fills.rs:196-228`) still matches. The signature hashes the outline, net, layers, clearance, minimum width, thermal settings, islands, hatch and smoothing. Its doc says "Editing any of them takes the fill away (KiCad unfills a zone that is edited)". GAPS 13 says the same ("an edit to a zone (outline, layers, net, a setting) empties that zone", "as KiCad does with auto-refill off"). Nothing translates a fill when its zone moves: `grep` finds no transform of cached rings outside the importer's footprint frame.
  - KiCad:
    - `zone.cpp::ZONE::Move` moves the fill with the outline: `for( std::pair<..>& pair : m_FilledPolysList ) pair.second->Move( offset );`. `ZONE::Rotate`, `ZONE::Flip` and `ZONE::Mirror` likewise transform `m_FilledPolysList`.
    - `edit_zone_helpers.cpp::PCB_EDIT_FRAME::Edit_Zone_Params` (Zone Properties OK) does `commit.Modify( aZone ); zoneInfo.ExportSetting( *aZone ); .. commit.Push( _( "Edit Zone Properties" ), SKIP_CONNECTIVITY ); rebuildConnectivity();`. It does not call `UnFill()`, so the old fill stays, along with the connectivity built from it.
    - KiCad calls `UnFill()` only for point edits (`pcb_point_editor.cpp` `m_zone.UnFill()`, `vertex_editor_pane.cpp`), for a cutout deleted from inside (`edit_tool.cpp:3035`), and in the filler and zone tools (`zone_filler_tool.cpp:415,434`, `board_editor_control.cpp:1789`, `zone_create_helper.cpp:192`).
- **How to see it.** Take a routed board whose GND pour joins its GND pads. Select the pour together with everything inside it (or the whole layout block) and move it 5 mm.
  - KiCad: the fill moves along, the pads stay connected, and the saved file has the moved `(filled_polygon ..)`.
  - Ours: the zone has no fill until the next Fill All. Live connectivity, the ratsnest and the status bar show GND unrouted. The `.kicad_pcb` the studio writes (DRC, plots, Save As) carries no fill for that zone, so kicad-cli's DRC without `--refill-zones` reports the GND pads unconnected.
  - The same happens after changing a pour's clearance in Zone Properties: KiCad keeps showing the old copper until a refill; ours drops it at once.
  - Test: `a_moved_zone_keeps_its_fill_moved` in `zone_fills.rs`. Store a fill for zone `z`, translate `z.outline` by (5 mm, 0), and assert `fills.rings(&moved, "F.Cu")` equals the old rings shifted by 5 mm (not empty).
- **Severity.** High: live connectivity and the written file differ from KiCad's after an ordinary move. A refill puts it right.
- **Already known?** No. GAPS 13 states the opposite as parity; the claim does not hold against `ZONE::Move` / `Edit_Zone_Params`.

#### R5-Z3. OK in Zone Properties on a copper zone with no net gives it the first net in alphabetical order

- **What differs.**
  - Ours: `web/studio/src/components/ZoneDialog.tsx:185` sets `const effectiveNet = kind === "copper" ? net || nets[0] || "" : ..`. The net `<select>` (`:277-284`) has no "no net" entry. The edit path sends `edit_zone` with `net: effectiveNet` (`:218`). A copper zone whose net is `""` (an imported no-net pour, or a thieving pour) therefore comes back on `nets[0]` after any OK, even one with no edits. The server accepts `""` (`crates/ops/src/lib.rs:4082-4086`); it is the dialog that changes the net.
  - KiCad: `panel_zone_properties.cpp` offers `<no net>` in `m_netSelector`. The panel only warns: `m_copperZoneInfoBar->ShowMessage( _( "<no net> will result in an isolated copper island." ), wxICON_WARNING );` (`updateInfoBar`). `Edit_Zone_Params` keeps net 0 (`if( NETINFO_ITEM* net = GetBoard()->FindNet( zoneInfo.m_Netcode ) ) aZone->SetNetCode( .. )`).
- **How to see it.** Import a board with a no-net copper pour (for example a thieving or shielding pour). Open its properties (E) and press OK without touching anything. The pour is now on, say, `+3V3`, and shorts whatever `+3V3`-free copper it covers once filled.
  - Test (web): open ZoneDialog in edit mode on a zone `{net: "", layer: "F.Cu"}` with board nets `["+3V3", "GND"]`, submit, and assert the `edit_zone` cmd has `net: ""`.
- **Severity.** High: a silent net change, so wrong connectivity after one OK.
- **Already known?** In part. `findings/p13-p16.md` P13-4 notes `net || nets[0]` and the missing "no net" entry for a **new** zone, and asks for the highlighted net. The edit path, where an existing zone's net is overwritten, is not named there.

---

### Medium

#### R5-Z4. Cleanup's merge ignores KiCad's "different width attached" rule: a segment with a narrower or wider track on either end is never merged in KiCad

- **What differs.**
  - Ours: `crates/connectivity/src/cleanup.rs:590-616` (`merge_collinear`) only requires the two candidates to have the same net, layer and width, a shared end and collinearity. It then asks `is_node` about the joint. Nothing looks at the other tracks connected to either segment.
  - KiCad, `tracks_cleaner.cpp::TRACKS_CLEANER::cleanup` (the `mergeSegments` lambda):
    ```cpp
    // Do not merge an end which has different width tracks attached -- it's a
    // common use-case for necking-down a track between pads.
    ..
    for( CN_ITEM* connected : citem->ConnectedItems() )
    {
        ..
        if( candidate->Type() == PCB_TRACE_T && !candidate->HasFlag( IS_DELETED )
            && !filterItem( candidate ) )
        {
            ..
            if( candidateSegment->GetWidth() == segment->GetWidth() )
                sameWidthCandidates.push_back( candidateSegment );
            else
            {
                differentWidthCandidates.push_back( candidateSegment );
                break;
            }
        }
    }

    if( !differentWidthCandidates.empty() )
        continue;
    ```
    `citem->ConnectedItems()` is everything connected to the segment, at both ends and anywhere along it. So one connected `PCB_TRACE_T` of another width means that segment takes part in no merge at all.
- **How to see it.** On one net, lay out a collinear chain C1 (0.15 mm), A (0.25 mm), B (0.25 mm), C2 (0.15 mm), with A and B meeting at a free point. Run Cleanup with "Merge co-linear tracks".
  - KiCad merges nothing: A has C1 attached and B has C2.
  - Ours merges A and B into one segment.
  - Test: `merge_skips_a_segment_with_a_different_width_attached` in `cleanup.rs`, built from that chain with `merge_segments: true`. Assert no `CleanupKind::MergedTracks`.
- **Severity.** Medium: Cleanup changes the board where KiCad does not. Connectivity is unchanged and the step can be undone.
- **Already known?** No. Round 4 A-5 covers the zone, overlap and tolerance cases only.

#### R5-Z5. Auto-Assign Priorities ignores the zones' holes (overlap, intersection, area)

- **What differs.**
  - Ours: `crates/ops/src/zone_priority.rs` builds each zone from its outer ring only:
    - `ring(&zones[i].outline)` (`:52-54`) and `polygons_overlap(&zones[i].outline, &zones[j].outline)` (`:77`) for the overlap and the intersection;
    - `area(&zones[..].outline)` (`:47-50`) for the size rule.
    `Zone::holes` (the cutouts the IR keeps since p22 F1) are never read.
  - KiCad, `zone_utils.cpp`, uses the whole outline set, holes included:
    - `findOverlappingPairs`: `a->Outline()->Collide( b->Outline() ) || .. Contains( .. CVertex( 0 ) )`;
    - `computeConstraint`: `SHAPE_POLY_SET polyA = aPair.zoneA->Outline()->CloneDropTriangulation(); .. intersection.BooleanIntersection( polyA, polyB );`;
    - the size rule: `aPair.zoneA->Outline()->Area()`, which subtracts the holes (`SHAPE_POLY_SET::Area`: `area -= Hole( i, j ).Area( true );`).
- **How to see it.** Take a large GND pour with a cutout, and a small `VBUS` pour that lies entirely inside the cutout.
  - KiCad: the pours do not overlap, so they get no edge, and Kahn's order keeps their current order.
  - Ours: they overlap; with no pads in the "overlap", the smaller-area rule puts `VBUS` above GND.
  - A big cutout can also make the pour with the larger outer ring the smaller one by area. KiCad then flips the size rule, and ours does not.
  - Test: `auto_priorities_respect_a_cutout` in `zone_priority.rs`. Two pours as above, current priorities GND 1, VBUS 0. Assert `auto_assign_zone_priorities` returns `None` (nothing changes).
- **Severity.** Medium: a wrong priority, and so a different fill once the zones are refilled.
- **Already known?** No. GAPS 13 lists the places that ignore holes (Convert to shapes, Simplify, rule_board, the hit test, the Zone Manager preview, Repeat Layout); `zone_priority` is not among them.

---

### Low

#### R5-Z6. Cleanup's `is_node`: pads, vias and zones at the joint are not asked as `testMergeCollinearSegments` asks them

`crates/connectivity/src/cleanup.rs:564-583` against `tracks_cleaner.cpp::testMergeCollinearSegments`. KiCad tests only `getConnectedItems( aSeg )`, the items connectivity has joined to the segment, and asks every non-track item with `citem->HitTest( point, ( aSeg1->GetWidth() + 1 ) / 2 )`. Ours differs in three ways:
- **Pads.** Ours uses `pg.pad.signed_distance(p) <= 0.0`: the joint must lie inside the copper. KiCad's `PAD::HitTest( aPosition, aAccuracy )` is `GetEffectivePolygon( l, ERROR_INSIDE )->Contains( aPosition, -1, aAccuracy )` over every padstack layer, or `GetEffectiveHoleShape()->Collide( aPosition, aAccuracy )`. A joint up to half the track width outside a pad that the track's copper touches is a node in KiCad and not in ours, so ours merges there.
- **Vias.** Ours uses `vias.iter().any(|v| v.at == p)`, with no layer or net check. A blind via that does not reach the track's layer is not connected in KiCad, so KiCad merges; ours refuses.
- **Zones.** Since 45dcab1a, ours takes any zone of the track's net on the layer whose outline edge or corner is near the joint. KiCad asks only a zone that `CONNECTIVITY_DATA::GetConnectedItems` returns, which is one whose **fill** touches the track (`checkZoneItemConnection`). Ours therefore also counts an unfilled zone, or one whose fill does not reach this track.
- **How to see it.** Segments of 0.25 mm meeting 0.1 mm outside an SMD pad's edge, collinear, nothing else there: KiCad keeps the two segments; ours merges them. Test: `merge_keeps_a_joint_within_half_width_of_a_pad`.
- **Severity.** Low: a merge made or missed. Connectivity is unchanged.
- **Already known?** No. A-5 / 45dcab1a cover only the zone's inside-versus-edge test.

---

### Checked and found to match (no need to review again)

- **45dcab1a, the zone part of `is_node`.**
  - `ZONE::HitTest`: `accuracy = std::max( aAccuracy, pcbIUScale.mmToIU( 0.1 ) )`, `HitTestForCorner( p, accuracy * 2 ) || HitTestForEdge( p, accuracy )`.
  - `aAccuracy = ( width + 1 ) / 2`, and holes are included (`m_Poly->CollideVertex` / `CollideEdge` walk every contour).
  - The rest of A-5 (overlap, fold-back, the 1 nm tolerance) is listed as left.
- **`CN_VISITOR::operator()`.**
  - The early-out for two items that cannot change net and differ in net. In KiCad, pads and zones are `CN_ITEM( .., false )` and tracks and arcs `true`; for vias see the note below.
  - Zone-to-item goes to `checkZoneItemConnection`; other items collide on each shared layer with `ConditionallyFlashed` giving `NEVER_FLASHED` (round 3/4).
- **`checkZoneItemConnection`.**
  - An anchor inside the fill first, then the item's whole shape.
  - A via or pad is taken `ALWAYS_FLASHED` (its full circle in ours, `zone_hits_point( center, radius )`).
  - The fills are KiCad's fractured `(filled_polygon ..)` rings, so the outer ring alone is right: `CN_ZONE_LAYER::m_outline` is `fillPoly->Outline( aSubpolyIndex )` too.
  - Not modelled and already known: `ZLO_FORCE_NO_ZONE_CONNECTION` / `ZLO_FORCE_FLASHED` (`flash.rs` module doc; GAPS 13's `zone_layer_connections`).
  - Note: KiCad makes a via `CN_ITEM( via, !via->GetIsFree() )`, so a free via is fixed-net. Ours makes every via `can_change_net: true`. This only matters for a free via overlapping a different-net pad or zone, which is a short anyway.
- **`RN_NET::AddCluster`:** one anchor per `CN_ZONE_LAYER` (`nAnchors = .. ? 1 : anchors.size()`), and the board edges chained from the cluster's first anchor.
- **`CONNECTIVITY_DATA::internalRecalculateRatsnest`'s island skip:** `c->IsOrphaned() && c->Size() == 1` with a `CN_ZONE_LAYER`. Ours checks a cluster that is one zone item; a cluster has no origin pad exactly when it holds no pad, so the two agree.
- **`RN_NET::compute` / `kruskalMST`:**
  - the `<= 2` nodes special case;
  - board edges of weight 0 merged into the triangulation's edges and sorted by weight;
  - an edge is output only if `unite` succeeds and its weight is greater than 0;
  - same-position chains weighted 1/0 by cluster.
  - Only ties between equal weights may order differently (KiCad's `std::sort` is unstable).
- **`AutoAssignZonePriorities`, apart from R5-Z5:**
  - eligibility (no rule area, no teardrop, on copper);
  - same-net pairs joined by union-find, and every member of a group taking the group's maximum after the graph;
  - `computeConstraint`'s counts: a pad or via counted once if it is on any shared layer, by net, inside the intersection; the 20 % `SIMILARITY_THRESHOLD`; the smaller-area rule with "equal areas give nothing";
  - `assignPrioritiesFromGraph`: edges sorted area-first then by `countDiff`, Kahn's queue sorted by current priority, cycle members appended last, `n - 1 - i`;
  - the "changed?" return.
- **Zone Properties' cross-field checks in `ops::edit_zone`:**
  - the thermal spoke against the minimum width, for copper pours only (`AcceptOptions`: "Thermal spoke width cannot be smaller than the minimum width.");
  - the hatch thickness and gap at least the minimum width (`m_gridStyleThickness.Validate( minThickness, INT_MAX )`);
  - "No layer selected." (`DIALOG_COPPER_ZONE::TransferDataFromWindow`).
  - The bounds (0.025 mm, 100 mm) and the wording are p13-p16's P13-7.
- **The fill itself:** kicad-cli's (`--refill-zones --save-board`). No ported filler is active, and none duplicates kicad-cli.


---

## Summary

| ID | What | Severity | Known before? |
|---|---|---|---|
| R5-1 | A route started on a pad/track of another layer than the active one starts with no net on the active layer (`getStartLayer`, `pickSingleItem`'s any-layer candidates) | high | no |
| R5-2 | The router's commit drops a replaced track's solder-mask opening and group; new tracks miss the entered group (`createBoardItem`'s source item, `m_replacementMap`) | high | no |
| R5-3 | The written `.kicad_pro` has no `track_widths` / `via_dimensions` (only the new pair list) | high | reader half: p22 F5 |
| R5-4 | A pair's start checks use the class sizes when custom dimensions are sent (applied after `start_diff_pair`) | medium | no |
| R5-5 | The new Differential Pairs list cannot be chosen: no entries in the pair menu, no W stepping during a pair | medium | no |
| R5-6 | Mid-route W steps a fixed 10-width ladder, not the board's list; first press does not keep the index | medium | partly (PARITY.md:240, stale reason) |
| R5-7 | The router's width/via/custom choices turn "Use Starting Track Width" off; KiCad only sets `m_TempOverrideTrackWidth` | medium | no |
| R5-8 | The router's message panel (`UpdateMessagePanel`: width/clearance/gap and their sources) is not ported | medium | no |
| R5-9 | The chosen width/via are kept by value, not re-read from the list after a Board Setup edit | low | no |
| R5-10 | Pre-defined Sizes: no sort on Apply, Add rows pre-filled, own message for track widths | low | no |
| R5-11 | `merge_project_predefined_sizes` skips raw row 0 before filtering malformed rows | low | no |
| R5-D1 | Rule engine footprints lack sheet path, component classes and fields (`memberOfSheet`, `getField`, `hasComponentClass` wrong for the router and Inspect) | high | partly (GAPS l.836, Inspect only) |
| R5-D2 | Router's copper barcodes/text boxes/tables typed "Graphic"; the implicit barcode clearance never applies | high | no |
| R5-D3 | `existsOnLayer('')` is a compile error in ours | low | no |
| R5-D4 | Missing-argument messages of `hasNetclass` / `hasExactNetclass` / `hasComponentClass` | low | no |
| R5-D5 | Clearance Resolution's stand-in pad for unnumbered pads, first vs last | low | no |
| R5-Z1 | Two same-net zone fills crossing without a vertex inside are connected in ours (kicad-cli proof) | high | no |
| R5-Z2 | Move/rotate/flip/mirror or Zone Properties OK empties a zone's fill; KiCad keeps it | high | no (GAPS 13 says it matches) |
| R5-Z3 | Zone Properties OK on an existing no-net copper zone gives it the first net | high | partly (P13-4, new zones only) |
| R5-Z4 | Cleanup merges collinear segments with a different-width track attached | medium | no |
| R5-Z5 | Auto-Assign Priorities ignores zone holes | medium | no |
| R5-Z6 | Cleanup's `is_node` pad tolerance, via layer/net, zone outline vs fill | low | no |

Counts: high 8, medium 7, low 7.
