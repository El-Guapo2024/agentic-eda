# Round 3 findings: router, zones, rules and DRC (`r3-route-rules`, 2026-10-28)

Reviewer: `r3-route-rules`, read-only. Code read from main's checkout (`/home/team_braneboard_com/work/agentic-eda`, at 5cac9357). KiCad: `/home/team_braneboard_com/src/kicad-src-8303b2ad`.

**No runtime check was possible.** Main's `target/debug/eda` was built on 2026-10-23 11:08, before the round-2 router merge (556525c2, 2026-10-27). It reads schema-1 (micrometre) designs and has no rule resolver and no keepouts. I started it on 8846/8847 on two scratch boards (a keepout on F.Cu+B.Cu, and one on B.Cu only), saw that it predates the code under review, and stopped both servers. Every finding below therefore comes from reading the code on both sides. The tests are the ones I would write; none was run.

Scope, in priority order:
- (a) The round-2 merges: `crates/pns/src/rule_resolver.rs` (`PNS_PCBNEW_RULE_RESOLVER`), `Node::item_collision` (`collideSimple`), `from_ir::add_keepouts` (`syncZone`), `triangulation.rs` (`TesselatePolygon`), and PARITY Stage 15 (R-3 `SimplifyLine`).
- (b) Router behaviours next to them that are not ported and not listed: `SyncWorld`'s items, `ImportSizes`, `isStartingPointRoutable`, `IsFlashedOnLayer`.
- (c) Rows no round reviewed: `crates/drc/src/dru.rs` (`DRC_RULES_PARSER`) and the DRC Rule Editor's messages (`crates/drc/src/rule_editor.rs`).

---

## High

### R3-1. A keepout rule area on more than one copper layer keeps out only its first layer

- **What differs.**
  - Ours: `crates/pns/src/from_ir.rs:198` (`add_keepouts`) does `let Some(layer) = layers.index_of(&z.layer) else { continue };` and then gives every triangle `layers: LayerRange::single(layer)`.
  - `Zone::layer` is only the first layer of the set. `crates/model/src/ir.rs:1534-1540`: "`ZONE::GetLayerSet()` of a zone on more than one layer ... the whole set in the UI order ..., `layer` being its first". The importer fills `layers` for every `(layers "F.Cu" "B.Cu")` / `*.Cu` rule area (`crates/kicad/src/import.rs:2587`, `zone.set_layer_set(&layers)`).
  - So a rule area on F.Cu and B.Cu is no obstacle on B.Cu or on any inner layer. The model already has what a fix needs: `Zone::copper_layers()` (`ir.rs:1704`).
  - The tests check one layer only: `crates/pns/tests/rules_kicad.rs:193` (`rule_area`: `layer: "F.Cu"`, no `layers`). `CODE-COMPARE-router.md`'s syncZone row also says "on its layer".
  - KiCad, `pns_kicad_iface.cpp::PNS_KICAD_IFACE_BASE::syncZone`:
    ```cpp
    LSET layers = aZone->GetLayerSet();
    ...
    for( PCB_LAYER_ID layer : LAYER_RANGE( F_Cu, B_Cu, m_board->GetCopperLayerCount() ) )
    {
        if( !layers[ layer ] )
            continue;
        for( unsigned int polyId = 0; polyId < poly->TriangulatedPolyCount(); polyId++ )
        { ... solid->SetLayer( GetPNSLayerFromBoardLayer( layer ) ); ... aWorld->Add( std::move( solid ) ); }
    }
    ```
    That is one set of triangles on **each** copper layer of the area.
- **How to see it.**
  - Test `rules_kicad::a_keepout_on_both_copper_layers_blocks_a_route_on_the_back`. Use the `a_keepout_rule_area_blocks_tracks` board with two through-hole pads. Give the area `layer: "F.Cu"` and `layers: ["F.Cu", "B.Cu"]`. Route on B.Cu in Walk around mode.
  - Assert that no committed track crosses the outline. Today the route goes straight through.
  - Under `EDA_SLOW_TESTS=1`, kicad-cli should report no `items_not_allowed`. Today it reports one.
  - A unit test in `from_ir`: `build_node` of that board has keepout solids (`Solid::is_keepout`) on layer index 1 (B.Cu). Today it has none.
- **Severity.** High. The router commits copper that kicad-cli's DRC rejects, and many keepouts are drawn on both sides or on `*.Cu` (KiCad's antenna and mounting-hole keepouts).
- **Already known?** No. R-2's fix and its tests use one layer. PARITY Stage 15 lists only the holes and the subdivision as "Differs".

### R3-2. Copper graphics are not router obstacles: board shapes on copper (with or without a net), a footprint's copper shapes, dimensions, barcodes, tables and text boxes on copper

- **What differs.**
  - Ours: `crates/pns/src/from_ir.rs:52-141` (`build_node_with`) adds only these to the world:
    - pads (:65);
    - tracks and arcs;
    - vias (:128);
    - the Edge.Cuts outline (:133);
    - copper texts (:134);
    - keepouts (:135).
  - It never reads:
    - `DrawingsSection::shapes` on a copper layer (`ir.rs:2454`). Their net is in `ShapeStyle::net`, and `eda_drc::board::DrcBoard::shapes` carries them.
    - A footprint's art shapes on copper (`fp_art.rs:430`).
    - `dimensions` (`ir.rs:2462`), `barcodes` (:2600) or `tables` (:2608).
  - KiCad, `pns_kicad_iface.cpp::PNS_KICAD_IFACE_BASE::SyncWorld`:
    - It runs over `m_board->Drawings()`: `case PCB_SHAPE_T: case PCB_TEXTBOX_T: syncGraphicalItem(..)`, `case PCB_TABLE_T: syncTextItem(..)`, `case PCB_BARCODE_T: syncBarcode(..)`, `case PCB_DIM_*: syncDimension(..)`.
    - For each footprint it does the same over `footprint->GraphicalItems()`.
  - `syncGraphicalItem` covers copper as well as Edge.Cuts:
    ```cpp
    if( aItem->GetLayer() == Edge_Cuts || aItem->GetLayer() == Margin || IsKicadCopperLayer( aItem->GetLayer() ) )
    { ... solid->SetLayer( GetPNSLayerFromBoardLayer( aItem->GetLayer() ) );
          solid->SetRoutable( aItem->Type() != PCB_TABLECELL_T ); ...
          solid->SetAnchorPoints( aItem->GetConnectionPoints() );
          solid->SetNet( aItem->GetNet() ); ... aWorld->Add( std::move( solid ) ); }
    ```
    A copper shape of another net (or of no net) is an obstacle at the clearance. A shape of the route's own net is routable, so a route can start or end on its connection points.
  - KiCad's QA boards have copper shapes: 20 of `qa/data/pcbnew/*.kicad_pcb` have a `gr_line` / `gr_poly` / `gr_rect` / `gr_circle` / `gr_arc` on F.Cu or B.Cu (for example `issue22102`, `graphics_load_save_v20240108`, `issue14549_2`, `api_kitchen_sink`). Net-tie footprints are built from a copper shape that bridges their pads.
- **How to see it.**
  - Test `from_ir::tests::a_copper_polygon_is_an_obstacle`. Put a filled `gr_poly` on F.Cu of no net between two pads of net A, and route between them in Walk around mode. Assert that the commit keeps the class clearance from the polygon. Today the route crosses it, and kicad-cli reports `clearance` (or `shorting_items` for a shape with a net).
  - A second case: a copper shape on net A. A route of net B must keep the clearance from it, and a route of net A may end on it.
- **Severity.** High: a short or a clearance violation, committed silently.
- **Already known?** No. GAPS item 7's "Left" lists only "a footprint's copper texts as obstacles". GAPS item 36's "copper graphics with a net" is about connectivity, not the router.

### R3-3. A route starts at the board's default width, not at the start net's class width (`ImportSizes`, `UseNetClassTrack()`)

- **What differs.**
  - Ours:
    - `web/studio/src/components/canvas/Canvas.tsx:743`: `const width = connected ?? state.currentTrackWidthNm ?? board.board_rules?.track_width ?? 250_000;`.
    - The same at `web/studio/src/actions/useActionRunner.ts:2101` (Route Selected / Autoroute), `pcbEditSweep.ts:229` and `pcbConvertSweep.ts:175`.
    - The backend falls back the same way: `crates/cli/src/route_api.rs:168` `num(&req, "width", model.board.track_width)`.
  - `currentTrackWidthNm` starts `null` (`state/store.tsx:945`) until W / Shift+W picks a preset. The fallback, `board_rules.track_width`, is the board's (Default class's) width, never the start item's class. (I did not check whether the W ladder has a "use net class" entry. If it has, choosing it gives the same fallback.)
  - The via tool does ask the class (`kicad-port/routerVia.ts:145-151`, `viaSizeFor`: `classValue(board, net, c => c.via_diameter)`). The track width has no such lookup.
  - KiCad, `pns_kicad_iface.cpp::PNS_KICAD_IFACE_BASE::ImportSizes`:
    ```cpp
    int  trackWidth = bds.m_TrackMinWidth;
    ...
    if( !found && bds.UseNetClassTrack() && aStartItem )
    {
        PNS::SEGMENT dummyTrack; ... dummyTrack.SetNet( static_cast<NETINFO_ITEM*>( aStartItem->Net() ) );
        if( m_ruleResolver->QueryConstraint( PNS::CONSTRAINT_TYPE::CT_WIDTH, &dummyTrack, nullptr, m_startLayer, &constraint ) )
        {
            trackWidth = std::max( trackWidth, constraint.m_Value.Opt() );
            found = true;
            if( trackWidth == constraint.m_Value.Opt() )
                aSizes.SetWidthSource( constraint.m_RuleName );
        }
    }
    if( !found ) { trackWidth = std::max( trackWidth, bds.GetCurrentTrackWidth() ); ... }
    ```
    `UseNetClassTrack()` is `m_trackWidthIndex <= 0 && !m_useCustomTrackVia` (`board_design_settings.h:321`), which is the default. `CT_WIDTH` is `TRACK_WIDTH_CONSTRAINT` through `EvalRules`, so it gives the start net's class width, a `.kicad_dru` `track_width (opt ..)` rule, and a diff-pair net's class diff-pair width (the implicit `A.inDiffPair('*')` rule).
- **How to see it.**
  - Take a board whose `Power` class has a 0.5 mm track width, a Default class of 0.25 mm, and a `VBUS` net in `Power`. Start a route on a VBUS pad with the width list on "use net class". KiCad routes at 0.5 mm; we route at 0.25 mm.
  - A test for the studio: `routing.test.ts` `a_route_takes_the_start_nets_class_width`. A test for the backend: a `Router::start` without an explicit width, on a `Power` pad, gives `preview().head.width == 0.5 mm`.
- **Severity.** High: every route of a non-Default class is at the wrong width, unless the user changes it by hand (the ratsnest's power nets, impedance classes).
- **Already known?** No. r2-pcb-edit's finding about "Set to net class / custom rule values" is the same root in Edit Track & Via Properties. The router's start width is not listed anywhere.

---

## Medium

### R3-4. `ImportSizes` asks the net class, not the rule engine: a `.kicad_dru` rule's width, pair gap, via size and hole-to-hole never reach the router

- **What differs.**
  - Ours:
    - `crates/pns/src/dp_placer.rs:80` `DpSizes::resolve`: `rules.diff_pair_gap_of(net_p).max(rules.min_clearance_nm)`, `rules.diff_pair_width_of(net_p)`, `rules.via_diameter_of(net_p)`, `hole_to_hole: rules.hole_to_hole_min_nm`.
    - `crates/pns/src/settings.rs:173` `SizesSettings::for_net`: `rules.width_of(net)`, `rules.via_diameter_of(net)`.
    - The studio's `viaSizeFor` (`routerVia.ts:151`) reads class fields only.
  - Since R-1 the router has the engine (`RuleResolver`), but sizes never ask it.
  - KiCad's `ImportSizes` asks `QueryConstraint` (that is, `EvalRules`) for every size:
    - `CT_WIDTH` of the pair (`&dummyTrack, &coupledTrack`);
    - `CT_DIFF_PAIR_GAP` ("`diffPairGap = std::max( diffPairGap, constraint.m_Value.Opt() ); diffPairViaGap = std::max( diffPairViaGap, constraint.m_Value.Opt() );`");
    - `CT_VIA_DIAMETER` and `CT_VIA_HOLE` of a dummy via;
    - `CT_HOLE_TO_HOLE` of two dummy vias and of the via and its coupled one (`SetDiffPairHoleToHole( std::max( holeToHoleMin, aSizes.GetHoleToHole() ) )`).
  - So a custom rule such as `(rule usb (condition "A.inDiffPair('/USB*')") (constraint diff_pair_gap (min 0.12mm) (opt 0.15mm)) (constraint track_width (opt 0.2mm)))` sets the pair's geometry in KiCad.
- **How to see it.**
  - Test `dp_placer::tests::a_custom_rule_sets_the_pairs_gap_and_width`. Give the Default class a gap of 0.25 mm, and add the rule above in `custom_rules_text`. Start a pair on `/USB_P` / `/USB_N`.
  - Assert `sizes.gap == 0.15 mm` and `sizes.width == 0.2 mm`. Today it is 0.25 mm and the class width.
  - Under `EDA_SLOW_TESTS=1`, with `(min 0.3mm)` in the rule, kicad-cli reports `diff_pair_gap` for our pair and not for the rule's.
- **Severity.** Medium. These are wrong defaults. kicad-cli flags them only when the rule has a `min` above the class value.
- **Already known?** No. `CODE-COMPARE-router.md:216` (the `ImportSizes` row for a pair) is about the class's gap and via gap only.

### R3-5. A keepout's holes are not cut out: the router will not route inside a keepout's cutout

- **What differs.**
  - Ours: `from_ir.rs:202` tesselates `z.outline` only, and `z.holes` is never read (`ir.rs:1528`: "Nothing of the zone is inside a hole: ... a rule area keeps nothing out of it").
  - KiCad: `syncZone` triangulates `aZone->Outline()` (`poly->CacheTriangulation()`), the polygon with its holes, so the triangles leave the holes open.
- **How to see it.** Test `rules_kicad::a_route_may_cross_the_hole_of_a_keepout`. Make a 10 x 10 mm keepout (tracks) with a 4 x 4 mm hole, and two pads inside the hole. Route between them. Assert the route is straight. Today the start is refused ("The routing start point violates DRC.") or the route walks out of the area.
- **Severity.** Medium: a route that KiCad allows is refused. No bad copper is committed.
- **Already known?** Only as a "Differs" sentence in `crates/pns/PARITY.md` Stage 15 R-2 ("an area's holes are not cut out of it"). It is not in GAPS item 7's "Left". It is listed here so it reaches GAPS.

### R3-6. Margin-layer graphics are not board-edge obstacles

- **What differs.**
  - Ours: `from_ir.rs:164` `add_board_outline` takes `board.edge_cuts` only. `Side::is_edge` (`rule_resolver.rs:90`) is `Solid::edge`, so `CT_EDGE_CLEARANCE` is asked of Edge.Cuts only.
  - KiCad has three places that count Margin too:
    - `syncGraphicalItem`: `if( aItem->GetLayer() == Edge_Cuts || aItem->GetLayer() == Margin || ... ) { ... solid->SetLayers( PNS_LAYER_RANGE( 0, m_board->GetCopperLayerCount() - 1 ) ); solid->SetRoutable( false ); }`.
    - `isEdge`: `parent && ( parent->IsOnLayer( Edge_Cuts ) || parent->IsOnLayer( Margin ) )`.
    - KiCad's DRC tests both layers the same way: `drc_test_provider_edge_clearance.cpp:284` `forEachGeometryItem( { PCB_SHAPE_T }, LSET( { Edge_Cuts, Margin } ), ..)`.
- **How to see it.** Draw a line on Margin across the board, and route a track across it in Walk around mode. KiCad walks round it at the copper-to-edge clearance; we cross it, and kicad-cli reports `copper_edge_clearance`. Test `board_edge::a_margin_line_is_an_edge`.
- **Severity.** Medium: an edge case, but the copper it commits is flagged by kicad-cli.
- **Already known?** No.

### R3-7. Unflashed via and pad layers are still copper to the router (`IsFlashedOnLayer`), and the CODE-COMPARE reason for that is out of date

- **What differs.**
  - Ours: `Node::item_collision` (`node.rs:1060-1068`) tests `item.shape(..)`, the via's or pad's full copper, on every layer it spans. `Item::Via` has no flash information, and `from_ir` does not read `via_extras`' `unconnected_layers()` (`crates/model/src/via_extra.rs:73`) or a pad's `ext.stack.unconnected`.
  - `CODE-COMPARE-router.md:70` justifies this with "no unflashed layers in the IR (ADAPTED, justified)". That has been false since Remove Unused Pads (GAPS item 2's paragraph (3); `eda_connectivity::flashed_layers`, `crates/connectivity/src/flash.rs:67`, already computes the layers each pad and via keeps).
  - Row :160 lists "the unflashed-layer branches are not modelled".
  - KiCad, `pns_item.cpp::ITEM::collideSimple`: `else if( iface && !iface->IsFlashedOnLayer( this, aHead->Layers() ) ) { clearance = -1; }`. With `PNS_KICAD_IFACE_BASE::IsFlashedOnLayer`'s `case PCB_VIA_T: ... if( via->FlashLayer( GetBoardLayerFromPNSLayer( layer ) ) ) return true; ... return false;`, the copper of an unflashed via does not collide on that layer. Its hole does, at `max( HOLE_CLEARANCE, CLEARANCE )`: `holeI->collideSimple` runs first, and `isCopper( hole )` is true.
  - `VIA::Hull` likewise gives the hole's hull there (`if( m_hole && !ROUTER::GetInstance()->GetInterface()->IsFlashedOnLayer( this, aLayer ) ) return HullForHole(..)`).
- **How to see it.**
  - Take a 4-layer board with a via of 0.6/0.3 mm on net A, set to `remove_unused_layers`, nothing on In1.Cu, a 0.2 mm clearance and a 0.25 mm hole clearance. Route a net-B track on In1.Cu past it.
  - KiCad lets the track's edge come to 0.25 mm from the hole (0.40 mm from the centre). We hold it at 0.2 mm from the 0.6 mm ring (0.50 mm from the centre), and the walkaround goes further round.
  - Test `node::tests::an_unflashed_via_is_its_hole_on_that_layer`.
- **Severity.** Medium: we route more conservatively than KiCad on boards that use Remove Unused Pads. No violation results.
- **Already known?** Partly: CODE-COMPARE :70 and :160, with a reason that is no longer true. It is not in GAPS item 7.

### R3-8. The start point's refusals for a rule area and a text: "Rule area disallows tracks." and "Cannot start routing from a text item." are missing; a start inside a vias-only keepout is allowed

- **What differs.**
  - Ours: `crates/pns/src/router.rs:212-231` (`Router::start`) refuses an NPTH ("Cannot start routing from a non-plated hole.") and then runs the dot-of-track collision test (`:226`, "The routing start point violates DRC."). Nothing looks at what non-routable item is under the point.
  - KiCad, `pns_router.cpp::ROUTER::isStartingPointRoutable`:
    - It first walks `QueryHoverItems( aWhere )`: an item on the layer that `IsRoutable()` clears the reason and stops the walk.
    - Otherwise:
      ```cpp
      case PCB_ZONE_T:
          if( !zone->HasKeepoutParametersSet() ) break;
          if( !zone->GetZoneName().IsEmpty() )
              failureReason = wxString::Format( _( "Rule area '%s' disallows tracks." ), zone->GetZoneName() );
          else
              failureReason = _( "Rule area disallows tracks." );
          break;
      case PCB_FIELD_T: case PCB_TEXT_T: case PCB_TEXTBOX_T:
          failureReason = _( "Cannot start routing from a text item." );
      ```
    - This runs for every mode, the pair included, before the DRC dot test.
  - Three cases come out differently:
    1. A start inside a tracks keepout gives KiCad's rule-area message, where we say "The routing start point violates DRC.".
    2. A start on a copper text gives "Cannot start routing from a text item.", where we say "The routing start point violates DRC.".
    3. A start inside a keepout that disallows **vias only**: KiCad refuses it ("Rule area disallows tracks.", as its code says for any keepout). We start the route.
- **How to see it.** Tests `router::tests::a_start_inside_a_named_rule_area_says_so` (an area named `ANT` with tracks kept out, `start` at its centre; assert `Err("Rule area 'ANT' disallows tracks.")`) and `..._vias_only_rule_area_is_refused_too` (`Err("Rule area disallows tracks.")`).
- **Severity.** Medium: messages and an edge case.
- **Already known?** No.

---

## Low

### R3-9. An outline the ear cutting cannot finish becomes one solid of the whole outline; KiCad warns that the area is malformed and leaves it out

- Ours: `from_ir.rs:204` `None => vec![z.outline.clone()]`. KiCad `syncZone`: `if( !poly->IsTriangulationUpToDate() ) { msg.Printf( _( "%s is malformed." ), aZone->GetItemDescription( &unitsProvider, true ) ); KIDIALOG dlg( nullptr, msg, KIDIALOG::KD_WARNING ); dlg.ShowDetailedText( _( "This zone cannot be handled by the router.\nPlease verify it is not a self-intersecting polygon." ) ); ... return false; }`.
- KiCad's `CacheTriangulation` also subdivides and splits before it gives up (not ported, as PARITY says). So our fallback runs on outlines that KiCad does triangulate, which is fine. But for a really self-intersecting outline we keep a polygon obstacle where KiCad has none and tells the user.
- Test: `triangulation::tests::a_bow_tie_is_refused`, and `from_ir` adds no solid for it, plus a warning for the studio.
- **Severity.** Low. **Known?** Only the subdivision half, in PARITY Stage 15.

### R3-10. The dummy via's layer: KiCad's is the layer asked about; ours is always the first copper layer

- Ours: `rule_resolver.rs:546-560` `dummy_via`: `it.layers = copper(); it.layer = it.layers.first().cloned();`, with no `Layer` property.
- KiCad `getBoardItem`: `m_dummyVias[aIdx].SetLayer( aBoardLayer )`, and `PCB_VIA::SetLayer( aLayer ) { Padstack().Drill().start = aLayer; }`, so `GetLayer()` (what a rule's `A.Layer` reads) is the layer being evaluated.
- A custom rule `(condition "A.Type == 'Via' && A.Layer == 'B.Cu'")` matches the head's via on B.Cu in KiCad and never here.
- **Severity.** Low (a rare condition).

### R3-11. Net-tie pad groups: a backslash escape is not read, and a head of no net never matches a pad of no net

- Ours: `rule_resolver.rs:333` splits `net_tie_pad_groups` on `,` and trims. `:465` requires `p.net.is_some() && p.net == *net`.
- KiCad `FOOTPRINT::MapPadNumbersToNetTieGroups` keeps the character after a `\\` (`case '\\': esc = true;`). `DRC_ENGINE::IsNetTieExclusion` compares net codes (`aTrackNetCode == pad->GetNetCode()`), so code 0 matches code 0.
- **Severity.** Low.

---

## Checked and found to match (no need to review again)

**`PNS_PCBNEW_RULE_RESOLVER` (`rule_resolver.rs`) against `pns_kicad_iface.cpp` at 8303b2ad.**
- **`Clearance`.**
  - The layers: for an edge, the other side's; otherwise the intersection; nothing when they are disjoint (`LayerRange::default()` is `-1..-1`).
  - The per-layer order:
    - `CT_HOLE_TO_HOLE` when both are drilled holes, else `CT_HOLE_CLEARANCE` when either is a hole;
    - `CT_CLEARANCE` when both are copper ("No 'else'; plated holes get both");
    - `CT_EDGE_CLEARANCE` for an edge or a non-plated slot;
    - always `CT_PHYSICAL_CLEARANCE`.
  - The largest is taken, and then `rv - m_clearanceEpsilon` only when `rv > 0`. `DRC_EPSILON` is 500 nm (`ADVANCED_CFG::m_DRCEpsilon = 0.0005`, `GetDRCEpsilon` = `mmToIU`).
- **`isCopper`.** `!parent || parent->IsOnCopperLayer()`: a hole (whose `Parent()` is null) and a keepout on copper are copper; the edge is not. `isEdge` and `IsNonPlatedSlot` (NPTH and `DrillSizeX != DrillSizeY`) match, apart from Margin (R3-6).
- **`QueryConstraint`'s severity.** `RPT_SEVERITY_IGNORE`, except an implicit tuning-profile rule, gives a minimum of `-1`.
- **`getBoardItem`.** The dummy track's width is `PCB_TRACK`'s constructor default, `pcbIUScale.mmToIU( 0.2 )` (`pcb_track.cpp:68`), as the port says. `evaluateLineSegments` and an existing track asked about as a dummy are documented "Differs" in the module header.
- **`IsKeepout` / `checkKeepout`.**
  - Tracks for `PCB_ARC_T`/`PCB_TRACE_T`. Vias for `PCB_VIA_T`, which a via's hole (`HOLE_T` gives a dummy via) is too.
  - A `SOLID` gets `nullptr` from `getBoardItem`, so a pad is never kept out.
  - A keepout that does not enforce gives `clearance = -1`.
  - The filter is `GetIsRuleArea() && HasKeepoutParametersSet()`.
- **`IsNetTieExclusion`.** The groups' pads of the head's net, `GetEffectiveShape( aTrackLayer )->Collide( aCollisionPos, epsilon )`, and `IsInNetTie( this )` on the obstacle. Apart from R3-11.
- **`MEANDER_PLACER_BASE::Clearance`.** `QueryConstraint( CT_CLEARANCE, <first segment>, nullptr, CurrentLayer() )` is `track_clearance`.
- **The plain shortcut (`RuleResolver::clearance`).** It is consistent with what `EvalRules` returns on a board with only board-setup and plain net-class clearance rules:
  - track/via against track/via is `CT_CLEARANCE` only;
  - holes take hole clearance or hole to hole, plus `CT_CLEARANCE`;
  - the edge is `CT_EDGE_CLEARANCE` only;
  - anything with a local override, a text or a keepout goes to the engine.
  - I did not re-run `the_plain_answer_is_the_engines_on_kicads_boards`.

**`ITEM::collideSimple` (`node.rs:1060`).** The same net never collides, except hole to hole. The keepout test comes before the clearance (`enforce`: exact, otherwise no collision). The item's hole is tested against copper of another net. The head's hole is tested against the item's copper of another net and against its hole of any net, with `shouldWeConsiderHoleCollisions`' identical-via exception. The documented "Differs" (holes are not items, the `-1` of a touch, free pads, castellation) were not counted again.

**`NODE::NearestObstacle`.** The hull is `GetClearance( obstacle, aLine ) + Width()/2`; the via's is `+ Diameter/2`; the 90-degree box (`makeHull`); the first entry is taken by path length.

**`TOPOLOGY::SimplifyLine` (R-3).** `AssembleLine( root, nullptr, false, false, false )`, then `Simplify()`, then the line is replaced when the point count changes. Ours uses the same assembly options. Running it once at the commit instead of at each fix gives the same segments, as PARITY says.

**`TesselatePolygon`'s first pass (`triangulation.rs`).** Checked: `createList`'s winding and the 50 nm simplification, `isEar`, `earScore`, the two-vertex look-ahead, `isTooSmall` (1000) and the local self-intersection merge. Any valid triangulation covers the same area, so only the fallback (R3-9) and the holes (R3-5) matter.

**`DRC_RULES_PARSER` (`crates/drc/src/dru.rs`) against `drc_rule_parser.cpp`.**
- **`Parse`.** "Missing '('.", "Missing version statement." reported once, "Missing version number.", `m_tooRecent`, "Incomplete statement.", "rule or version", and "No errors found." when nothing was reported.
- **`parseDRC_RULE`.** "Missing rule name.", "Missing condition expression.", "Could not parse expression '%s'.", "'layer' keyword already present.", and "constraint, condition, or disallow".
- **`parseConstraint`.**
  - The three deprecations and their message ("The '%s' keyword has been deprecated.  Please use '%s' instead.").
  - "Missing constraint type.|  Expected ..." with KiCad's shorter list, and the `expected` list with `solder_mask_sliver` and `bridged_mask`.
  - "Rule already has a '%s' constraint." and the unitless set.
  - Every disallow token, and the zone connection's three tokens.
  - `min_resolved_spokes` read by `strtol`, and the assertion.
  - `within_diff_pairs` on skew only.
  - "Missing min/max/opt value.", "Time based units not allowed for constraint type.", "Mixed units for constraint values.", and the time and space options for length and skew.
- **`parseValueWithUnits`.** `KiROUND` of the result (`PCBEXPR_EVALUATOR::Evaluate`: `m_result = KiROUND( result->AsDouble() )`), and the reporter's `ERROR: <a href='%d:%d'>` form.

**DRC Rule Editor (`crates/drc/src/rule_editor.rs`).** All 25 distinct `result.AddError( _( "..." ) )` messages of `pcbnew/drc/rule_editor/*.h|cpp` appear word for word in the port. I did not compare the panel logic around them.

---

## Summary

| ID | What | Severity | Known before? |
|---|---|---|---|
| R3-1 | A keepout on several copper layers is an obstacle on its first layer only (`from_ir.rs:198`, `z.layer`) | high | no |
| R3-2 | Copper shapes (board and footprint), dimensions, barcodes, tables and text boxes on copper are not router obstacles (`SyncWorld`/`syncGraphicalItem`) | high | no (only a footprint's copper texts are listed) |
| R3-3 | A route starts at the board's default width, not the start net's class width (`ImportSizes`, `UseNetClassTrack`) | high | no |
| R3-4 | `ImportSizes` ignores `.kicad_dru` rules: the pair's width and gap, via size, hole to hole | medium | no |
| R3-5 | A keepout's holes are not cut out | medium | PARITY "Differs" only, not in GAPS |
| R3-6 | Margin-layer graphics are not edges | medium | no |
| R3-7 | Unflashed via and pad layers are still copper to the router; CODE-COMPARE :70's reason is stale | medium | partly (CODE-COMPARE) |
| R3-8 | Start refusals "Rule area disallows tracks." / "Cannot start routing from a text item." missing; a vias-only keepout start allowed | medium | no |
| R3-9 | A malformed keepout is one polygon solid; KiCad warns "%s is malformed." and leaves it out | low | partly |
| R3-10 | The dummy via's layer is the first copper layer, not the layer asked about | low | no |
| R3-11 | Net-tie groups ignore `\\` escapes; no-net heads never match a no-net tie pad | low | no |

Counts: high 3, medium 5, low 3.
