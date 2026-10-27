# Round 4 findings: router, zones, rules and DRC (`r4-route-rules`, 2026-10-28)

Reviewer: `r4-route-rules`, read-only. Code read from main's checkout (`/home/team_braneboard_com/work/agentic-eda`, at 01ffc984). KiCad: `/home/team_braneboard_com/src/kicad-src-8303b2ad` (Clipper2 2.0.1 from `kicad-src-25ec2335`).

Nothing was run: every finding comes from reading both sides. The tests named are the ones I would write.

Scope, in priority order:
- (a) The round-3 merges (14b75fef `task-r3-router`, 2d41e7af the oracle test fixes): `from_ir::add_keepouts` / `keepout_triangles`, `add_copper_graphics`, `add_margin`, `settings::import_sizes`, `Router::start_refusal`, `RuleResolver::is_flashed_on`, `crates/pns/tests/rules_kicad.rs`.
- (b) Router behaviours next to them that are not ported: `inheritTrackWidth` for a single route, the head's side of `IsFlashedOnLayer`.
- (c) Rows no round reviewed: the Clipper2 2.0.1 binding as `SHAPE_POLY_SET` uses it (`inflate2`, `inflateLine2`, `booleanOp`, `Simplify`), `loadImplicitRules`' net-class section, the ratsnest's `Triangulate` over the delaunator crate, Board Setup > Pre-defined Sizes.

---

## Medium

### R4-1. A route started on a pad or via does not take the width of the track already joined to it (`inheritTrackWidth`, `m_UseConnectedTrackWidth`)

**Fixed** on task-r4-router: 747bbd9a (server `inheritTrackWidth`, `auto_track_width` default on; studio sends it, on by default), 3f855aa1 (route_api test, browser check). Proof: `a_route_started_on_a_pad_takes_the_width_of_its_stub` (kicad-cli).

- **What differs.**
  - Ours, studio: `web/studio/src/components/canvas/Canvas.tsx:742-746` takes a connected width only through `connectedTrackWidth(findRouteAnchor(..)?.from ?? ..)`, and `web/studio/src/kicad-port/boardControl.ts:215` (`connectedTrackWidth`) returns `null` unless `from` starts with `"track "`. A start on a pad (`REF.PAD`) or a via (`via <id>`) gets the chosen width or, with none, the server's `ImportSizes`.
  - Ours, server: `crates/pns/src/settings.rs:213` (`import_sizes`) says "The connected track's width (`m_UseConnectedTrackWidth`, `inheritTrackWidth`) is the caller's", and `crates/cli/src/route_api.rs:174` calls it without that step. `Router::inherit_track_width` (`crates/pns/src/router.rs:1145`) exists, but only `start_diff_pair` calls it.
  - KiCad, `pns_kicad_iface.cpp::PNS_KICAD_IFACE_BASE::ImportSizes`:
    ```cpp
    if( bds.m_UseConnectedTrackWidth && !bds.m_TempOverrideTrackWidth && aStartItem != nullptr )
    {
        found = inheritTrackWidth( aStartItem, &trackWidth, startPosInt );

        if( found )
            aSizes.SetWidthSource( _( "existing track" ) );
    }
    ```
    `inheritTrackWidth` covers a pad or via too: `case PNS::ITEM::VIA_T: .. case PNS::ITEM::SOLID_T: ..`, `linkedSegs.ExcludeItem( aItem ).FilterKinds( PNS::ITEM::SEGMENT_T | PNS::ITEM::ARC_T )`, then "find the connected track whose far end is closest to the cursor". `m_UseConnectedTrackWidth` is the project's `board.auto_track_width`, default `true` (`project_local_settings.cpp:200`), and `ROUTER_TOOL::prepareInteractive` clears `m_TempOverrideTrackWidth` before each start.
- **How to see it.** A 0603 pad of `SIG` with a 0.15 mm stub already on it. `SIG` is in Default at 0.25 mm. The width list is on "use net class" and the Auto Track Width toggle is on, as by default. Press X on the pad: KiCad starts at 0.15 mm (source "existing track"); the studio and `POST /api/route/start` start at 0.25 mm. The same happens from a via with a stub.
  - Test (pns): `import_sizes_inherits_the_stub_of_a_start_pad`. Board: pad R1.1 on `SIG` with a 150 µm track ending on it, class width 250 µm. Call `Router::import_sizes(r1, "F.Cu")` with inheritance on. Assert `track_width == 150 µm` and `width_source == "existing track"`.
  - Test (studio, node): `connectedTrackWidth` for `from = "R1.1"` with a 150 µm track ending at the pad returns 150 000.
- **Severity.** Medium: a wrong default width. It is the most common start, from a pad that already has a fan-out stub.
- **Already known?** No. p5 F4-5 fixed this for pairs only (`a_pair_started_on_a_pad_with_a_stub_takes_the_stubs_width`). Round 3's R3-3 left `m_UseConnectedTrackWidth` to "the caller", and the caller handles track ends only.

### R4-2. The head's side of `IsFlashedOnLayer` is missing: a dragged or shoved board via that removes its unconnected layers still has full copper on those layers

**Fixed** on task-r4-router: 747bbd9a (`QueryOpts::head_via`, `is_via_flashed_on`; a moved via keeps its id and extras through `CommitRoute`; a drag at a via takes the via, `pickSingleItem`). Proof: `an_unflashed_head_via_collides_by_its_hole_only` (kicad-cli).

- **What differs.**
  - Ours: `crates/pns/src/node.rs:1094`: `let flashed = self.rules.as_ref().is_none_or(|r| r.is_flashed_on(item, layers));`. Only the world's item is asked. `via_colliding` (`node.rs:1050`) probes with `Shape::Circle { r: via.diameter / 2 }` on all of `via.layers`, whatever the via is.
  - KiCad, `pns_item.cpp::ITEM::collideSimple` asks both sides:
    ```cpp
    else if( iface && !iface->IsFlashedOnLayer( this, aHead->Layers() ) )
    {
        clearance = -1;
    }
    else if( iface && !iface->IsFlashedOnLayer( aHead, Layers() ) )
    {
        clearance = -1;
    }
    ```
    `IsFlashedOnLayer( aHead, .. )` reads the head's parent `PCB_VIA` (`via->FlashLayer( .. )`). The head's hole is tested on its own afterwards ("a special case for the "head" via's hole").
- **How to see it.** A four-layer board. A via of `A` (0.6 / 0.3 mm) with `remove_unused_layers` and nothing on In1.Cu. A track of `B` on In1.Cu runs 0.30 mm from the via's centre, with a class clearance of 0.2 mm. In KiCad, dragging the via along the track (D, Walk around) is held only by its hole: 0.15 + 0.2 ≤ 0.30 is fine. Ours holds it at its pad: 0.30 + 0.2 > 0.30, so the via is pushed off, or the drag fails. Shoving such a via (`pushOrShoveVia`) has the same problem.
  - Test (pns): `an_unflashed_head_via_collides_by_its_hole_only`. Build that board and take the world's via `v` (with `source_via`). Assert `node.via_colliding(&v_moved, ..)` finds no obstacle on In1.Cu when the In1 track is 0.30 mm from the via centre, and does find one at 0.20 mm (the hole at the clearance).
- **Severity.** Medium: the router is wrong in the conservative direction. Nothing it commits is a violation, but it refuses or pushes where KiCad does not.
- **Already known?** No. Round 3's R3-7 and Stage 16 port the item's side only.

### R4-3. Board Setup > Pre-defined Sizes: no Differential Pairs list, and the via rows are checked with our own words and bounds

- **What differs.**
  - Ours:
    - `web/studio/src/components/BoardSetupDialog.tsx:306-360` has only Track Widths and Via Sizes.
    - The model has no diff-pair dimensions list. `grep` finds no `diff_pair_dimensions` anywhere in `crates/`, so the project's list is neither read nor written.
    - `crates/ops/src/lib.rs:3812-3819` (`set_via_presets`) refuses with "via drill and diameter must be positive" (when `<= 0`) and "drill must be smaller than diameter".
  - KiCad:
    - `panel_setup_tracks_and_vias.cpp` has three grids: track widths, via sizes and diff pairs (`m_diffPairsGrid`, `m_DiffPairDimensionsList`, validated through `ValidateDesignRules`' `diff_pair_dimensions_list[..]` errors).
    - `PANEL_SETUP_TRACKS_AND_VIAS::Validate` checks each via row with `pcb_track.cpp::PCB_VIA::ValidateViaParameters`:
      ```cpp
      if( aDiameter.has_value() && aDiameter.value() < GEOMETRY_MIN_SIZE )
          error.m_Message = _( "Via diameter is too small." );
      ..
      if( aPrimaryDrill.has_value() && aPrimaryDrill.value() < GEOMETRY_MIN_SIZE )
          error.m_Message = _( "Via drill is too small." );
      ..  _( "No via hole size defined." )  ..  _( "No via diameter defined." )
      ..  _( "Via hole size must be smaller than via diameter" )
      ```
      `GEOMETRY_MIN_SIZE` is `(int)( 0.001 * pcbIUScale.IU_PER_MM )`, 1 µm (`pcb_track.h:57`).
    - The router's pair sizes come from that list when the pair is not on "use net class" (`ImportSizes`' `else { diffPairWidth = bds.GetCurrentDiffPairWidth(); .. }`).
- **How to see it.**
  - Via rows: enter a via of diameter 0.0005 mm. KiCad says "Via diameter is too small." against the cell. Ours accepts it, because it is positive.
  - Messages: a drill of 0.6 mm in a 0.6 mm via gives our "drill must be smaller than diameter" where KiCad says "Via hole size must be smaller than via diameter".
  - Diff pairs: a project whose `.kicad_pro` has `"diff_pair_dimensions": [{gap 0, via_gap 0, width 0}, {gap 0.15, via_gap 0.15, width 0.2}]` loses its pair row when it comes in.
  - Test (ops): `set_via_presets_refuses_with_kicads_words`. Presets `[{diameter: 500, drill: 300}]` (nm) give "Via diameter is too small."; `[{600_000, 600_000}]` gives "Via hole size must be smaller than via diameter".
- **Severity.** Medium: wrong messages and bounds, and a missing page section. Losing the project's pair list is also a lost default.
- **Already known?** Partly. The importer dropping the project's via and track presets is p22 F5. The diff-pair list, the page's grid and the messages are not listed (GAPS item 3; PARITY-pcb.md:464 says the page is "editable").
- **Fixed** on ds/r4-board-setup: cca40225 -- `RoutingSection::diff_pair_presets` (width/gap/via gap, serde default and skip-empty) written and read as the project's `board.design_settings.diff_pair_dimensions` (millimetres, KiCad's index-0 dummy first; read back without the dummy, a row missing a key skipped), `Cmd::SetDiffPairPresets` in the Pre-defined Sizes batch (one undo step) and the studio page's third grid; `set_via_presets` and `validatePredefinedSizes` now run `PCB_VIA::ValidateViaParameters` with `GEOMETRY_MIN_SIZE` 1 um and KiCad's three messages in its order, and the pairs through `ValidateDesignRules`' "No differential pair gap defined.".

---

## Low

### R4-4. A pair started on arc tracks skips the start-gap check, and takes the class width instead of the arcs'

**Fixed** on task-r4-router: 747bbd9a. Test: `a_pair_started_on_arcs_checks_the_gap`.

- **What differs.**
  - Ours: `crates/pns/src/dp_placer.rs:353-365` inherits the width and runs the "does not match the configured gap" test only for `(Item::Segment(sp), Item::Segment(_))`.
  - KiCad, `pns_router.cpp::ROUTER::isStartingPointRoutable`: `if( aStartItem->OfKind( ITEM::SEGMENT_T | ITEM::ARC_T ) ) { int actualGap = ..; int configuredGap = m_sizes.DiffPairGap() + m_sizes.DiffPairWidth(); .. }`. The width is `inheritTrackWidth`'s, which takes `ARC_T` too (`tryGetTrackWidth`). The message is "The differential pair gap at the start point does not match the configured gap. This can occur in neckdown areas where tracks have narrower width and spacing. Adjust the differential pair settings or start from a location with the correct gap."
  - Ours also takes P's width (`sp.width`) where KiCad takes the start item's width (`inheritTrackWidth( aStartItem, .. )`, which may be N's).
- **How to see it.** Two arc tracks of `/USB_P` and `/USB_N`, 0.2 mm wide, at a 0.5 mm pitch; the class is 0.2 / 0.15. KiCad refuses the start (|0.5 − 0.35| > 0.035). Ours starts.
  - Test: `a_pair_started_on_arcs_checks_the_gap`: assert `start_diff_pair` returns the message above.
- **Severity.** Low.
- **Already known?** p5 F4-2 lists "arcs in the pair code" in general. This start check is not named.

### R4-5. `ImportSizes` details: hole to hole with no start item, the pair's dummy track at anchor 0, and the dummy via's position

**Fixed** on task-r4-router: 747bbd9a. Test: `hole_to_hole_is_asked_with_no_start_item_too` (the anchor-0 dummies and the (0, 0) dummy via changed with it, untested: no rule reading position on hand).

- **What differs.** `crates/pns/src/settings.rs:262-312` against `pns_kicad_iface.cpp::ImportSizes`:
  - KiCad queries `CT_HOLE_TO_HOLE` of `&dummyVia, &dummyVia, UNDEFINED_LAYER` (and of the coupled via) **outside** `if( aStartItem )`. With no start item, a custom rule that matches a no-net via still sets `SetHoleToHole`. Ours only queries with an anchor, and otherwise keeps `rules.hole_to_hole_min_nm` (`settings.rs:262-312`).
  - KiCad builds the pair's `dummyTrack` / `coupledTrack` at `aStartItem->Anchor( 0 )`, not at `startAnchor`. Ours uses the start anchor `p` (the anchor nearer the cursor).
  - KiCad's `PNS::VIA dummyVia` is default-built, so its position is (0, 0). Ours puts it at `p`.
  - The last two matter only for a rule whose condition reads the position (`A.intersectsArea(..)`, `A.insideCourtyard(..)`).
- **How to see it.** A rule `(rule h2h (condition "A.Type == 'Via' && A.NetName == ''") (constraint hole_to_hole (min 0.5mm)))`. A route started in empty space gets `hole_to_hole` 0.5 mm in KiCad, and the board's minimum in ours.
- **Severity.** Low.
- **Already known?** No.

### R4-6. Copper-graphics details of `SyncWorld` (the R3-2 port)

**Fixed** on task-r4-router: 919021d7 (text box stroke with border off; arcs/circle rings get ARC_HIGH_DEF more stroke). Hidden board texts and hidden dimension text: **not a router bug** (the importer drops them, so the derived board kicad-cli checks has none; footprint hidden texts are hidden fields, dropped by KiCad too) -- an importer gap, noted in GAPS 7.

`crates/pns/src/from_ir.rs` against `pns_kicad_iface.cpp`:
- **A text box with its border off still has its stroke width in KiCad.**
  - Ours: `from_ir.rs:213`: `stroke_width: if tb.border { tb.stroke_width_nm } else { Nm::ZERO }`.
  - KiCad: `syncGraphicalItem` calls `EDA_SHAPE::makeEffectiveShapes` with `width = GetEffectiveWidth()`, which is `GetWidth()`, the stroke width. `m_borderEnabled` is not read there; it is read only in `PCB_TEXTBOX`'s own drawing and polygon code (`pcb_textbox.cpp:733,751`).
  - Example: a 0.1 mm stroke with `(border no)` is a 0.1 mm wide ring to KiCad's router, and a zero-width one to ours.
- **Hidden texts.**
  - KiCad's `syncTextItem` drops only hidden **fields**: `if( aItem->Type() == PCB_FIELD_T && !static_cast<PCB_FIELD*>( aItem )->IsVisible() ) return false;`. A hidden `PCB_TEXT` on copper, on the board or in a footprint, is still an obstacle.
  - Ours drops every hidden one: `fp_art.rs:304` `!t.layout.visible`, and the importer skips hidden board copper texts (`add_copper_texts`' doc).
- **Dimensions.**
  - KiCad adds a dimension's text only `if( aDimension->IsVisible() && !aDimension->GetText().IsEmpty() )`.
  - Ours tests `!g.text.is_empty()` only (`from_ir.rs:221`).
- **Arcs and circles on copper.**
  - Ours: chords at `ARC_HIGH_DEF`, a `Shape::Strokes` (`eda_drc::board::effective_shapes`). The chords lie up to 5 µm inside the arc, on its outer side.
  - KiCad: one `SHAPE_ARC` (`makeEffectiveShapes`: `new SHAPE_ARC( m_arcCenter, m_start, GetArcAngle(), width )`, and a 360° `SHAPE_ARC` for an unfilled circle).
  - The Edge.Cuts solids get an error band for this (`add_edge_solids`), and copper shapes do not.
- **Severity.** Low.
- **Already known?** Partly. Stage 16's "Left of it" names the dimension's and barcode's glyph boxes and the rounded rectangle, not these.

### R4-7. `inflate2` / `inflateLine2` with `aSimplify`: ours simplifies the offset; KiCad's call discards the result (latent)

**Fixed** on task-r4-router: a2c130eb. Test: `inflate_with_simplify_keeps_the_offsets_vertices`.

- **What differs.**
  - Ours: `crates/shape-poly-set/src/lib.rs:321` `let paths = cl::simplify_paths(&paths, (amount as f64).abs() * coeff, true);`, and `:353` the same with `false`. Both feed the result to the union.
  - KiCad, `shape_poly_set.cpp::SHAPE_POLY_SET::inflate2` (8303b2ad and 25ec2335 alike): `Clipper2Lib::SimplifyPaths( paths, std::abs( aAmount ) * coeff, true );`. That is a statement whose return value is dropped. `SimplifyPaths` takes `const Paths<T>&` and returns a new `Paths<T>` (`third_party/clipper2/include/clipper2/clipper.h:706`), so KiCad unions the **unsimplified** offset. `inflateLine2` (`:1271`) is the same.
- **How to see it.** `ShapePolySet::inflate(amount, .., simplify: true)` on a 64-segment circle. KiCad keeps every vertex of the offset; ours drops those within `|amount| * (1 − cos(π/n))`.
  - Test: `inflate_with_simplify_keeps_the_offsets_vertices`: compare the vertex count against `simplify: false` followed by a self-union.
- **Severity.** Low and latent. Every active caller passes `simplify: false` today (`ops/convert.rs`, `drc/hatch.rs`, `drc/via_stitch.rs`, `kicad/specctra/export.rs`). The first caller to pass `true` would differ from KiCad.
- **Already known?** No.

### R4-8. A preset equal to the Default class's width is taken for "use net class"

**Not done on task-r4-router:** studio-only (the width list kept by value in `routerVia.ts::chosenTrackWidth` / `useActionRunner.ts`); no server half is needed -- the server already takes an explicit `width`.

- **What differs.**
  - Ours: `web/studio/src/kicad-port/routerVia.ts:160-163` (`chosenTrackWidth`): `if (current == null || current === netClassEntry) return undefined;`. `useActionRunner.ts:1014-1017` builds the list as `[base, ...track_width_presets]` and steps it with `list.indexOf(cur)`.
  - KiCad: `BOARD_DESIGN_SETTINGS::UseNetClassTrack()` is `m_trackWidthIndex == 0 && !m_useCustomTrackVia`. It goes by the **index**: a pre-defined width that happens to equal the Default class's width (index > 0) is "user choice" (`ImportSizes`: `trackWidth = std::max( trackWidth, bds.GetCurrentTrackWidth() )`, source "user choice").
- **How to see it.** Default is 0.25 mm, Power is 0.5 mm, and the pre-defined widths are [0.25, 0.4]. Pick 0.25 mm from the list and start on a Power pad: KiCad routes at 0.25 mm and ours at 0.5 mm. With that duplicate, W from 0.25 also steps from index 0 (`indexOf`), so it never reaches the preset entry.
- **Severity.** Low.
- **Already known?** No.
- **Fixed** on ds/r4-board-setup: cca40225 -- the studio chooses by the list index: `chosenTrackWidth`/`chosenViaSize` (`web/studio/src/kicad-port/routerVia.ts`) return "use net class" only when the index is absent, 0, or past the list (`GetCurrentTrackWidth`/`GetCurrentViaSize`'s range check), so a preset equal to the Default class's width at index > 0 is a user choice; the W/via cycles (`useActionRunner.ts`), the menu's checks (`pcbMenuBuilder.ts`) and `currentTrackWidthIndex`/`currentViaIndex` (`store.tsx`) all carry the index. The web tests cover the duplicate-width preset at index 1 and the past-the-end fallback.

---

## Checked and found to match (no need to review again)

**`PNS_KICAD_IFACE_BASE::SyncWorld` and the round-3 port (`from_ir.rs`).**
- The kinds it syncs: `PCB_SHAPE_T` / `PCB_TEXTBOX_T` (`syncGraphicalItem`), `PCB_TEXT_T` and `PCB_TABLE_T` (`syncTextItem`), `PCB_BARCODE_T`, the five dimension kinds; reference images and targets are ignored.
- Footprints: their reference, value and fields; their graphical items; their zones. A footprint's rule areas reach `add_keepouts` because the importer puts them in `routing.zones` with `parent_footprint`.
- **`syncZone`.**
  - The filter is `GetIsRuleArea() && HasKeepoutParametersSet()`: tracks, vias, pads, footprints or zone fills.
  - The `LAYER_RANGE( F_Cu, B_Cu, .. )` loop: one solid per triangle per copper layer of the set, with net `nullptr` (ours `not_a_net`), not routable.
  - The malformed outline's dialog: "%s is malformed." plus "This zone cannot be handled by the router.\nPlease verify it is not a self-intersecting polygon.", and the zone is left out.
- **`syncGraphicalItem`.**
  - Edge.Cuts and Margin are on every copper layer and not routable. Copper shapes are on their own layer and routable.
  - `SetWidth( 0 )` applies to Edge.Cuts only. The net is `aItem->GetNet()`: a no-net board shape is the orphaned net, which is our `None`. The anchors are `GetConnectionPoints()`.
  - `makeEffectiveShapes`: a filled shape is its polygon plus its stroke when `width > 0 || !solidFill`; a Bézier is one segment per flattened piece.
- **`syncBarcode` / `syncDimension` / `syncTextItem`.** No net, not routable, on the item's layer. A board text's hull is `buildBoundingHull` (the glyph box turned back by the draw rotation, the four corners turned forward).
- `SOLID::Hull` of a compound: the hull of each primitive, `hullSet.Simplify()`, `Outline( 0 )`.

**`ROUTER::isStartingPointRoutable` (`Router::start_refusal`, `start_diff_pair`, `DiffPairPlacer::start`).**
- `AllowDRCViolations()` returns true first, and the pair's "Diff pair gap is less than board minimum clearance." comes before the hover items.
- The Edge.Cuts skip, the break at a routable item, and the last non-routable item's reason. Each message matches word for word: "Cannot start routing from a non-plated hole.", "Rule area '%s' disallows tracks." / "Rule area disallows tracks.", "Cannot start routing from a text item.", "Cannot start a differential pair in the middle of nowhere.", "The routing start point violates DRC.".
- The dot of track at the route's width and then at `BoardMinTrackWidth()`.
- A text box is routable (`SetRoutable( aItem->Type() != PCB_TABLECELL_T )`), so its `PCB_TEXTBOX_T` refusal never fires in KiCad, and ours (a graphic, routable) agrees.

**`ImportSizes` (`settings::import_sizes`), apart from R4-1 and R4-5.**
- The clearance: the board minimum, replaced only when `CT_CLEARANCE`'s min is at least that.
- The width: `CT_WIDTH`'s opt over `m_TrackMinWidth`, with the sources "board minimum track width", the rule's name and "netclass 'Default'". The start anchor nearer the cursor for a segment.
- The via: `CT_VIA_DIAMETER` / `CT_VIA_HOLE` opt over `m_ViasMinSize` / `m_MinThroughDrill`.
- The pair: `CT_WIDTH` of track and coupled track, and `CT_DIFF_PAIR_GAP` setting both gap and via gap.
- `SetDiffPairHoleToHole( max( .., GetHoleToHole() ) )`.

**`ROUTER_TOOL::handleLayerSwitch`'s via size (`Router::net_class_via_size`, `RuleResolver::via_size`).**
- `UseNetClassVia() || viaType == MICROVIA`, starting from `m_ViasMinSize` / `m_MinThroughDrill`.
- `EvalRules( VIA_DIAMETER_CONSTRAINT / HOLE_SIZE_CONSTRAINT, &dummyVia, nullptr, currentLayer )`, applied only when not null.
- The `Via Type` enum names "Through", "Blind", "Buried", "Micro" (`pcb_track.cpp:2878`).

**`IsFlashedOnLayer`, the item's side (`RuleResolver::is_flashed_on`).**
- The range form ("any layer of the intersection flashed").
- `VIA::Hull` of an unflashed via is its hole. The hole is walked at the hole's clearance, which is what KiCad's separate `HOLE` item gives.
- **Not a bug:** an NPTH pad whose hole fills its pad (`PAD::FlashLayer` false) has copper in our world at `CT_CLEARANCE`. KiCad has no copper there, but its parentless `HOLE` is "copper" to `isCopper` and takes `max( CT_HOLE_CLEARANCE, CT_CLEARANCE )`, so the distance held is the same.

**`DRC_ENGINE::loadImplicitRules`, section 3 (`constraints.rs:760`).**
- Every net-class rule's name ("netclass '%s'", " (diff pair)", " (uvia)"), condition, min and opt.
- The micro-via diameter rule has a min only, because KiCad's second `SetMin` overwrites the first.
- The "narrower diffpair gap" clearance rule.
- The order: Default, then the classes by name, then the composite classes; the clearance and width rules sorted by their first minimum; the item rules after them.
- Section 4 (tuning-profile rules) is missing, but that is known (GAPS line 813). Note for whoever does it: through `ImportSizes`' `CT_WIDTH`, a class with a tuning profile makes KiCad's router start at the profile's width for the layer, and kicad-cli flags any other width (min = max).

**Clipper2 2.0.1 as KiCad calls it (`crates/shape-poly-set`, `crates/pns/src/hull.rs`), apart from R4-7.**
- `inflate2` / `inflateLine2`:
  - the join type and miter limit per `CORNER_STRATEGY` (10 for `ALLOW_ACUTE_CORNERS`, 2 otherwise);
  - `aCircleSegCount < 6` raised to 6;
  - the arc tolerance `|amount| * (1 − cos(π/n))`;
  - `EndType::Polygon` / `EndType::Butt`;
  - the second pass is `Union` under `FillRule::Positive` with `PreserveCollinear( false )` and `ReverseSolution( false )`.
- `booleanOp` is `FillRule::NonZero`. `Simplify` is a self-union; its `splitCollinearOutlines` pre-pass is documented as left out, and it does not matter for the router's hull unions.

**`RN_NET::TRIANGULATOR_STATE::Triangulate` on the delaunator crate (`ratsnest.rs`).** Coincident anchors are chained with weight 1 or 0 by cluster; all-collinear anchors are chained in sorted order; the edges are each triangle's three plus each halfedge's pair. The crate replaces KiCad's vendored delaunator-cpp (same algorithm and author). Only cocircular ties may pick another diagonal, and that never changes the MST's length.

**The round-3 oracle tests (`crates/pns/tests/rules_kicad.rs`, 2d41e7af).**
- `kicad_violations` matches the kind exactly (`kinds.contains(&v.kind.as_str())`, `!v.excluded`).
- Every board with a deliberate violation has a pad of the offending net in that copper's cluster: R3 anchors the keepout-body track, G1 the GND polygon, R1 the polygon-clearance track, and the vias-only via sits on the routed SIG track.
- The kinds named are KiCad's (`tracks_crossing`, `diff_pair_gap_out_of_range`, `items_not_allowed`, `copper_edge_clearance`, `hole_clearance`).

---

## Summary

| ID | What | Severity | Known before? |
|---|---|---|---|
| R4-1 | A route started on a pad or via does not inherit the joined track's width (`inheritTrackWidth`, `auto_track_width` on by default); the studio handles track ends only, and the server never does | medium | no (pairs only, p5 F4-5) |
| R4-2 | `collideSimple`'s `IsFlashedOnLayer( aHead, Layers() )` is missing: a dragged or shoved via with removed layers has full copper there | medium | no |
| R4-3 | Pre-defined Sizes: no Differential Pairs list (model, page, project); the via rows' bounds (> 0 vs 1 µm) and messages are not `ValidateViaParameters`' | medium | partly (p22 F5: the importer's presets) |
| R4-4 | A pair started on arc tracks skips the start-gap check and the arcs' width | low | partly (p5 F4-2) |
| R4-5 | `ImportSizes`: hole to hole with no start item, the pair's dummies at anchor 0, the dummy via at (0, 0) | low | no |
| R4-6 | Copper graphics: a text box with its border off keeps its stroke, hidden non-field texts stay obstacles, a hidden dimension's text, arcs as chords | low | partly |
| R4-7 | `inflate2` / `inflateLine2` simplify the offset; KiCad discards `SimplifyPaths`' result (latent: no caller passes `simplify`) | low | no |
| R4-8 | A pre-defined width equal to the Default class's is taken for "use net class" | low | no |

Counts: high 0, medium 3, low 5.
