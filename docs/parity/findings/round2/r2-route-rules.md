# Round 2 findings: router, zones, rules and DRC (`r2-route-rules`, 2026-10-27)

Reviewer: `r2-route-rules`, read-only. Code read from main's checkout (`/home/team_braneboard_com/work/agentic-eda`, at 5cca5fff). KiCad: `/home/team_braneboard_com/src/kicad-src-8303b2ad`. I did not build, test or start a server. Every finding comes from reading the code on both sides. Where a test is proposed, it is the test I would write; none was run.

Scope, in priority order:
- (a) The code merged since round 1: Stage 14 springback in the dragger and the pair placer, its review fixes (`a050b233`, `38c11f42`, `f40fc27f`, `90fc0c95`), and the length type's conversion of pns, drc and connectivity (`c333fc24`, `6b03c610`, `aa7868fa`, `14e46f73`, `876e3bbc`, `3efc97b2`, `197eebbe`, `2b3ad36b`, `a376bb15`, `022788ec`, `7d474529`, `fa209b76`).
- (b) Rows round 1 left out on purpose: springback, virtual vias, and `LockJoint` in the pair shove.
- (c) Rows not reviewed in round 1: the router's rule resolver (`PNS_PCBNEW_RULE_RESOLVER`, `syncZone`), the `EvalRules` local-override and net-tie blocks of `crates/drc/src/constraints.rs`, Board Setup's Constraints validation, and `TestTrackEndpointDangling`.

---

## R-1. The router's clearance is the net class's only: a pad's or footprint's local clearance, a custom rule (`.kicad_dru`) and a net tie are ignored (HIGH)

- **What differs.**
  - Our side: `crates/pns/src/node.rs:896` `Node::clearance` calls `eda_drc::constraints::clearance(rules, a_net, b_net)`. That function (`crates/drc/src/constraints.rs:33`) returns `max(class(a), class(b), min_clearance)` and has nothing else to go on.
  - `Node::clearance_to` (`node.rs:905`) adds only the board-edge case. `crate::item::Solid` (`item.rs:67`) carries no clearance.
  - The shove reaches the same function through `shove.rs:552` (`Shove::clearance`), and so do the walkaround, the hulls and the pair placer.
  - KiCad: `pns_kicad_iface.cpp::PNS_PCBNEW_RULE_RESOLVER::Clearance` calls `QueryConstraint( CT_CLEARANCE, aA, aB, layer, &constraint )`, which asks `m_board->GetDesignSettings().m_DRCEngine` (`QueryConstraint`: `std::shared_ptr<DRC_ENGINE> drcEngine = m_board->GetDesignSettings().m_DRCEngine; ... case PNS::CONSTRAINT_TYPE::CT_CLEARANCE: hostType = CLEARANCE_CONSTRAINT;`).
  - So it gets the whole of `drc_engine.cpp::DRC_ENGINE::EvalRules`:
    - the net-tie block ("Net tie on %s; clearance: 0.");
    - "// Local overrides take precedence over everything *except* board min clearance", with `overrideA = ac->GetClearanceOverrides( nullptr )` and the larger of A's and B's;
    - every custom rule;
    - and then the implicit net-class rules.
  - A local override can make the clearance **smaller** than the net class's as well as larger.
- **Why the old justification no longer holds.** `docs/parity/CODE-COMPARE-router.md:149` (`NODE::GetClearance`, ADAPTED) says "no custom-rule resolver, keepouts, net ties", and row :160 says "(IR has none of these)". The IR has all of them now:
  - `ir.rs:2743` `PadZoneFacts::pad_clearance`, from `Pad::clearance` / `clearance_override` and the footprint's clearance;
  - the custom rules page (`set_custom_rules`, `drawings.rules`);
  - net ties (`RuleItem::net_tie`).
  - Our own `eda_drc::constraints::RuleEngine::eval_rules` already ports `EvalRules`, including these blocks (checked below, M-3). The router just never calls it.
- **How to see it.**
  - Test `router::tests::a_pad_local_clearance_holds_a_route_off`. Build two pads of different nets, with a Default class clearance of 0.2 mm and a 0.6 mm `clearance` on pad R2.1. In Walk around mode, route from R1.1 past R2.1 along a line 0.4 mm from R2.1's copper. Assert that the committed track keeps at least 0.6 mm from R2.1's outline. Today it keeps 0.2 mm, and `kicad-cli pcb drc` on the export reports `clearance`.
  - A second case: a custom rule `(rule hv (constraint clearance (min 1mm)) (condition "A.NetName == '/HV'"))`. The router keeps 0.2 mm from `/HV`, and kicad-cli flags it.
  - A third case: a net-tie footprint's pads of two nets. KiCad's router lets a track of one net touch the other pad (clearance 0); ours holds it off by the class clearance.
- **Severity.** High: the router commits copper that KiCad's DRC rejects, or refuses routes that KiCad allows.
- **Already known?** Only as the ADAPTED row of CODE-COMPARE-router.md (:149, :160), with a reason that is now false. It is not in GAPS item 7 or in `findings/p5-router.md`. Hole clearance (`CT_HOLE_CLEARANCE`) is also absent, but CODE-COMPARE :104 already lists it, so it is not counted here. It belongs to the same fix: route `Node::clearance_to` through `RuleEngine::eval_rules` as `PNS_PCBNEW_RULE_RESOLVER::Clearance` does, taking the max over `CT_HOLE_TO_HOLE` / `CT_HOLE_CLEARANCE` / `CT_CLEARANCE` / `CT_EDGE_CLEARANCE` / `CT_PHYSICAL_CLEARANCE`, then `rv - m_clearanceEpsilon`.

## R-2. A rule area that keeps out tracks or vias is not an obstacle to the router (HIGH)

- **What differs.**
  - Our side: `crates/pns/src/from_ir.rs:9-13`: "Zones are deliberately not inserted as `NODE` items ... the interactive router routes straight through/over a zone's fill area". Nothing in `from_ir.rs` looks at `Zone::is_rule_area` / `keepout_tracks` / `keepout_vias` / `keepout_pads` (`ir.rs:1613-1630`).
  - That is right for a copper zone. A rule area is different. `pns_kicad_iface.cpp::PNS_KICAD_IFACE_BASE::syncZone` starts with `if( !aZone->GetIsRuleArea() || !aZone->HasKeepoutParametersSet() ) return false;`. It then adds every triangle of the area's outline, on each of its copper layers, as a `SOLID` (`solid->SetNet( nullptr ); solid->SetParent( aZone ); ... solid->SetRoutable( false ); aWorld->Add( std::move( solid ) );`).
  - `pns_item.cpp::ITEM::collideSimple` then gives it an exact boundary: `else if( aNode->GetRuleResolver()->IsKeepout( this, aHead, &enforce ) || ... ) { if( enforce ) clearance = 0; // keepouts are exact boundary; no clearance else clearance = -1; }`.
  - `PNS_PCBNEW_RULE_RESOLVER::IsKeepout`'s `checkKeepout` enforces it per kind: `GetDoNotAllowTracks() && aOther->IsType( { PCB_ARC_T, PCB_TRACE_T } )`, `GetDoNotAllowVias() && aOther->Type() == PCB_VIA_T`, the same for pads, and the footprint case.
- **How to see it.** Test `from_ir::tests::a_keepout_rule_area_blocks_tracks`. On a board with two same-net pads, draw a rule area with `keepout_tracks: true` across the straight line between them. Route in Walk around mode. Assert that the committed track stays outside the area's outline. Today it goes straight through, and kicad-cli reports `items_not_allowed`. With `keepout_vias: true` and `keepout_tracks: false`, assert that a track may cross the area but `V` there is refused (shoved out in Shove mode).
- **Severity.** High: a wrong result that kicad-cli flags.
- **Already known?** Only as "keepouts" in CODE-COMPARE :149/:160 ("IR has none of these", now false: the IR has rule areas, `is_rule_area` and the `keepout_*` flags). It is not in GAPS item 7, item 13 or the round-1 findings.

## R-3. A differential pair's fixed legs are not merged with the collinear track they continue (`TOPOLOGY::SimplifyLine`) (MEDIUM)

- **What differs.**
  - KiCad: `pns_diff_pair_placer.cpp::DIFF_PAIR_PLACER::FixRoute` does `m_lastNode->Add( lineP ); m_lastNode->Add( lineN ); topo.SimplifyLine( &lineP ); topo.SimplifyLine( &lineN );`.
  - `pns_topology.cpp::TOPOLOGY::SimplifyLine` assembles the line through the node's existing same-net segments (`LINE l = m_world->AssembleLine( root, nullptr, false, false, false ); SHAPE_LINE_CHAIN simplified( l.CLine() ); simplified.Simplify(); if( simplified.PointCount() != l.PointCount() ) { m_world->Remove( l ); ... m_world->Add( lnew ); }`).
  - So a leg that goes straight on from the previous leg, or from the start pad's stub track, becomes one segment with it.
  - Our side: `crates/pns/src/dp_placer.rs:890` `fix_head` adds the legs with `node.add_line(..)` and nothing more. `router.rs:1138` `build_dp_commit` writes each leg's P and N as their own `Track`. `Router::simplify_new_lines` (`router.rs:793`, the single-track port of `simplifyNewLine`, findings p5 F2-6) is called at :395, :568 and :600, but never for a pair (`finish_diff_pair`, `router.rs:1129`).
- **How to see it.** Test `router::tests::a_pair_that_goes_straight_on_across_a_fix_is_one_segment_per_line`. Start a pair on a pad pair. Fix once at x+5 mm on a straight horizontal run (the last segment is not fixed, so the next leg starts at the second-last point), then finish at x+10 mm on the same line. Assert that the commit has, for each net, one track with 2 points from the start to the end, plus any start stub merged. Today it has two tracks per net, meeting at the fix point.
- **Severity.** Medium: extra vertices and tracks where KiCad has one segment. Connectivity is the same, but the written board differs and later edits behave differently (a drag slides one segment in KiCad and two here).
- **Already known?** No. p5 F2-6 and its fix cover the single-track `simplifyNewLine` only.

## R-4. Board Setup > Constraints: the arc approximation error's upper bound is 1 mm; KiCad's is 0.1 mm (MEDIUM)

- **What differs.**
  - Our side: `crates/model/src/rules.rs:221` `range("max_error", self.max_error_nm, Nm::new(1000), Nm::new(1_000_000))`, and the studio's copy at `web/studio/src/kicad-port/boardSetupRules.ts:240` (`{ key: "max_error_nm", setting: "max_error", min: 1_000, max: 1_000_000 }`). Its test asserts the wrong bound: `boardSetupRules.test.ts:237` `"Value must be between 0.001 and 1 mm."`.
  - KiCad: `board_design_settings.cpp::BOARD_DESIGN_SETTINGS::ValidateDesignRules`, which `PANEL_SETUP_CONSTRAINTS::TransferDataFromWindow` runs, has `addRangeError( wxS( "max_error" ), m_MaxError, pcbIUScale.mmToIU( MINIMUM_ERROR_SIZE_MM ), pcbIUScale.mmToIU( MAXIMUM_ERROR_SIZE_MM ) );`, with `board_design_settings.h`: `#define MINIMUM_ERROR_SIZE_MM 0.001` and `#define MAXIMUM_ERROR_SIZE_MM 0.1`.
  - The 1 mm comes from the `PARAM_SCALED<int>( "rules.max_error", .., pcbIUScale.mmToIU( 0.0001 ), pcbIUScale.mmToIU( 1.0 ) ..)` load range, which is not what the dialog checks.
- **How to see it.** `RuleConstraints { max_error_nm: mm(0.5), .. }.validate()` should return `[("max_error", "Value must be between 0.001 mm and 0.1 mm.")]` (see R-5 for the text). Today it returns nothing, and `set_constraints` accepts 0.5 mm, which KiCad's dialog refuses.
- **Severity.** Medium: a wrong range, so a value KiCad refuses is stored and written to `.kicad_pro`.
- **Already known?** No.
- **Fixed** on ds/r2-board-setup: dc584275 -- 0.1 mm (`MAXIMUM_ERROR_SIZE_MM`) in `crates/model/src/rules.rs` and `web/studio/src/kicad-port/boardSetupRules.ts`; the Rust and web tests assert the bound (0.1 mm accepted, one nanometre more refused) and the message.

## R-5. Board Setup range messages: "between 0 and 25 mm." where KiCad says "between 0 mm and 25 mm." (MEDIUM)

- **What differs.**
  - Our side: `crates/model/src/rules.rs:201` and `:244` `format!("Value must be between {} and {} mm.", ..)`, and `boardSetupRules.ts:212`.
  - KiCad: `ValidateDesignRules`'s `addRangeError` formats each bound with the units: `wxString minValue = EDA_UNIT_UTILS::UI::StringFromValue( pcbIUScale, units, aMin, true ); wxString maxValue = ... ( pcbIUScale, units, aMax, true ); ... wxString::Format( _( "Value must be between %s and %s." ), minValue, maxValue )`.
  - `aAddUnitsText = true` appends `" mm"` to **each** bound (`eda_units.cpp::StringFromValue`: `text.Printf( "%.10f" ..); removeTrailingZeros( text ); ... if( aAddUnitsText ) text << EDA_UNIT_UTILS::GetText( aUnits, aType );`).
  - KiCad also uses the user's units (`m_Frame->GetUserUnits()`); ours is always mm.
  - Three more differences on the same page:
    - `boardSetupRules.ts:250` has "Value must be a whole number between 0 and 99." for the spoke count, where KiCad and `rules.rs:222` have "Value must be between 0 and 99.".
    - `boardSetupRules.ts:265` / `rules.rs:252` validate the Solder Mask/Paste page. KiCad's `PANEL_SETUP_MASK_AND_PASTE::TransferDataFromWindow` validates nothing (it stores the values and returns true). The ratio message has two forms of its own: "between -100 and 100 %." in TS and "-1.000 and 1.000" in Rust.
    - KiCad shows only `errors.front()` through `PAGED_DIALOG::SetError`.
- **How to see it.** In `rules.rs`'s tests: `RuleConstraints { min_clearance_nm: mm(30.0), .. }.validate()[0].1 == "Value must be between 0 mm and 25 mm."`.
- **Severity.** Medium (message text).
- **Already known?** No.
- **Fixed** on ds/r2-board-setup: dc584275 -- the range messages read "Value must be between 0 mm and 25 mm." in both crates (`addRangeError`'s units on each bound; the studio's `rangeMessage` too), the studio's spoke message is "Value must be between 0 and 99.", and the Solder Mask/Paste page's validation (`validate_mask_paste` in Rust and TS, `set_mask_paste`'s refusal, the studio page's red marks) is gone, as KiCad's page validates nothing.

## R-6. Pair placer, Shove mode: a move whose head cannot be fitted or walked shows the pushed tracks back in place (LOW)

- **What differs.**
  - Our side: `dp_placer.rs:707` `move_to` clears `shoved_lines` / `shoved_vias` (:714). `rh_shove_only` (:636) returns early at `if !ok { return false; }` and at the failed `try_walk_dp`, before it sets `shoved_lines` from the frame. The preview (`preview()`, :818) therefore draws no displacement for this move, although the springback stack (and `last_node = frame`) still has the pushes of the newest frame.
  - KiCad: `rhShoveOnly` sets `m_currentNode = m_shove->CurrentNode();` first. `Move` then does `m_lastNode = latestNode->Branch()` whatever `route()` returned, and the view is `CurrentNode()` (`m_lastNode`), so the pushed tracks stay pushed on screen.
  - No commit is affected: `FixRoute` needs `m_fitOk || AllowDRCViolations()`, and `AllowDRCViolations()` is false in Shove mode (`m_routingMode == RM_MarkObstacles && m_allowDRCViolations`).
- **How to see it.** Extend `a_via_the_pair_pushed_stays_pushed_until_the_leg_is_fixed`: after the push, move the cursor into a pad where `propagate_dp_head_forces` fails. Assert that `preview().displaced_vias` still holds the pushed GND via. Today it is empty, and it reappears at the next good move.
- **Severity.** Low (what is drawn).
- **Already known?** No.

## R-7. Drag, Walk around mode: a position that cannot be walked shows the last good position; KiCad shows the track where it started (LOW, and a wrong doc claim)

- **What differs.**
  - Our side: `dragger.rs:574-584` (`Dragger::drag`, "Restore last solution") shows `state.last_solution`, the last good walked preview. For Walk around mode, `restore_onto_frame` is skipped (:577), and its doc and PARITY Stage 14 say "Walk around mode is unchanged: its parent is the session's world, which `last_solution` already shows".
  - KiCad: `DRAGGER::Drag`'s restore is `NODE* parent = m_lastNode->GetParent()->Branch(); ... m_lastNode->Add( m_lastDragSolution );`. `m_lastDragSolution` is set only in `startDragSegment` (`m_lastDragSolution = m_draggedLine;`, the line as assembled at Start) and in `dragShove` (`m_lastDragSolution = std::move( draggedPostShove );`). `dragWalkaround` never sets it.
  - So in Walk around mode the restored node is `m_world` plus the original line, which is redundant with the line already there, and `Traces()` is empty. The view shows the track back in its original place until a position works again. `FixRoute` then re-drags at `m_lastValidPoint`, as ours does.
  - A smaller related point: in Shove mode with "Optimize entire track being dragged" on, KiCad's `m_lastDragSolution` is `draggedPostShove` **before** the optimizer. `optimizeAndUpdateDraggedLine` writes the optimized line into `draggedPostOpt` (`optimizer.Optimize( &aDragged, &draggedPostOpt, &origLine )`) and leaves `aDragged` split but not optimized. Ours restores the optimized `pv.pts`.
- **How to see it.** Test `dragger::tests::walkaround_restore_shows_the_original_line`. In Walk around mode, drag a segment once to a free spot (ok), then to a spot boxed in by a pad on every side (walk fails). Assert `pv.restored` and `pv.pts == original.pts`. Today `pv.pts` is the first position's line.
- **Severity.** Low (what is drawn). The PARITY Stage 14 sentence and the `restore_onto_frame` doc comment should be corrected either way.
- **Already known?** No.

## R-8. Connectivity's dangling test: KiCad's integer accuracy and integer `getMinDist` (LOW)

- **What differs.**
  - Our side: `crates/connectivity/src/dangling.rs:96-105` tests both ends with the f64 `half_width` as the accuracy. `min_dist_to_track` (:71) compares f64 distances. The arc case measures against the tessellation's chords (:64).
  - KiCad: `connectivity_data.cpp::TestTrackEndpointDangling` uses `int accuracy = KiROUND( aTrack->GetWidth() / 2.0 );` and `shape->Collide( aTrack->GetStart(), accuracy )` on the true shape. `getMinDist` returns `int` (`return std::min( track->GetStart().Distance(aPoint ), track->GetEnd().Distance( aPoint ) );`, truncated), and the comparison is `getMinDist( item, start ) < getMinDist( item, end )`, so a tie after truncation counts the **end**.
  - The differences are at most 1 nm, plus the chord sag on arcs, which lies inside the arc and so can miss a hit by up to the tessellation error.
- **How to see it.** A 201 nm-wide track whose end is 100.6 nm beyond a pad edge: KiCad's accuracy is 101 and it connects; ours compares with 100.5 and calls it dangling. For the tie: a short track with both ends inside one other track, at distances 1000.4 and 1000.7 from its nearest end. KiCad counts the end; ours counts the start.
- **Severity.** Low (sub-nanometre edge cases; GAPS item 36 already lists the micrometre coordinate leftovers).
- **Already known?** Partly: item 36's leftovers are other causes.

---

## Checked and found to match (no need to review again)

**Stage 14, the dragger (`crates/pns/src/dragger.rs`) against `pns_dragger.cpp` and `pns_shove.cpp` at 8303b2ad.**
- **One SHOVE per gesture.** `DRAGGER::Start` creates `m_shove` only for `RM_Shove && !m_freeAngleMode`, and free angle goes to `dragMarkObstacles`. Ours matches (`solve`, `drag`). The mode is latched at Start (p5 F3-12, fixed).
- **`SHOVE::reduceSpringback`.**
  - `while( m_nodeStack.size() > 1 )`: the last frame is never popped, and neither is a locked one.
  - The head set is the line where the cursor puts it (`origHead`), or for a via the via as the current node has it (`FindViaByHandle( theVia )`).
  - After the pops, the via is restored from the top frame's `m_draggedVias`.
  - All of this matches `Springback::reduce_all`, `DragShove::reduce` / `via_pos` and `drag_via_shove` (:640-660).
- **`pushSpringback`.** The new frame's area is the previous frame's area merged with this shove's (`prev_area->Merge( *aAffectedArea )`, else `prev_area`), as in `Springback::push`. `totalAffectedArea` reaches `runOptimizer` (`shove.rs:1570-1575`).
- **A failed `Run`.** `delete m_currentNode; m_currentNode = parent;` (the pops stay), and `theVia = prevVia`, so `m_draggedVia` is the reduced frame's via. The port keeps the pops and walks around from `from` (the reduced frame's via place). Matches.
- **`DM_VIA` fallback.** `dragViaWalkaround( m_draggedVia, m_lastNode, aP )` runs in the frame. `propagateViaForces` runs on the frame with the via removed. `m_world->CheckColliding( &draggedLine )` decides whether to walk. `DragCorner( viaTargetPos, Find( aHandle.pos ) )` simplifies inside `dragCorner45`. A walk failure fails the position. All match `drag_via_walkaround` (:719), including the world/frame split and the frame's displacement in the preview and the commit (review fix f40fc27f).
- **Snap thresholds.** `Width() / 4` for mark obstacles and walk around, `Width() / 2` for shove, both integer division. `Nm / i64` truncates as C++ does (`units.rs:290`).
- **Line shove.** `SHP_SHOVE | SHP_DONT_LOCK_ENDPOINTS`, `SHP_REVERSED` for `DM_CORNER` at index 0. `preShoveNode->Remove( draggedPreShove )` is the original segment ids removed from the frame (`shove_head`).
- **`Drag`.** `firstDrag` gives forced mark-obstacles. `m_lastValidPoint` is updated only on `ret`. `FixRoute`'s three branches (`m_dragStatus`, forced mark plus Ctrl, re-drag at the last valid point) match `finish`. The Shove-mode restore onto the reduced frame (review fix 90fc0c95) is right for Shove mode; see R-7 for Walk around.
- **`startDragSegment` and `checkVirtualVia`.** `w2 = Width()/2` integer, `distA < w2 || distB < w2`, `distB <= distA` picks the end, and the free-angle third condition `!IsPtOnArc( idx + 1 )`. `checkVirtualVia` uses `<= w2` with A first.

**Stage 14, the pair placer (`crates/pns/src/dp_placer.rs`) against `pns_diff_pair_placer.cpp`.**
- **`rhShoveOnly`.** It starts from `m_shove->CurrentNode()` (`frame_node()`). Fit and `tryWalkDp( .., true )` run in it. The two heads go into one `Run` with one head set (`reduce_all`). `m_currentTrace.SetShape` takes the shoved heads. `m_fitOk` is true only if neither line collides in the shoved node. "bring back previous state" on failure. All match.
- **`SHOVE::Run`'s `LockJoint`.** Locked at the head's first point, and at its last point unless it ends with a via. Matches :660-668.
- **`FixRoute`.** The guard is `!m_fitOk && !AllowDRCViolations()`. Both lines need at least 1 segment. `m_initialDiagonal` comes from `CSegment( -2 )`. "avoid fixing the last segment" applies with `SegmentCount() > 1` on both lines. The vias go into `m_lastNode`. Every fix in Shove mode makes a fresh `SHOVE` (`m_shove = std::make_unique<SHOVE>( m_world, Router() )`), so the stack restarts empty and no frame is locked. All match, except `SimplifyLine` (R-3).

**The length type (pns, drc, connectivity).** I read every changed rounding in the conversion diffs: `kiround` vs `as i64` vs `ceil`/`floor`. Each `(x).round() as i64` became `Nm::kiround(x)` (half away from zero, the same as `f64::round`). Each truncating `as i64` stayed a truncation (`Nm::new(x as i64)`): the octagon chamfers, `Simplify`'s lengths, `l += a.Length()` (commented as truncated), the hull's `2/(1+sqrt2)` term, and the delay conversions. `ceil` stayed `ceil` (the pair gateways' `reach`, `g_p` / `g_n`). The i128 segment kernels take `get()` at their boundary, and the Nm arithmetic is i64, so no int overflow appears that was not there before. `Nm::from_mm` clamps to `INT_LIMIT` as `parseBoardUnits` does. No mm/nm mix found in the changed lines. `3d2bdc79` (the reference silk box in micrometres) was such a mix, and it is fixed.

**`crates/drc/src/constraints.rs` against `drc_engine.cpp::EvalRules`.**
- **The footprint net-tie block.** The condition is `(!ac) ^ (!bc)` or the same existing footprint, and both items on copper. The order is reversed (`alt_items = {bc, ac}`). The message is "Net tie on %s; clearance: 0.". Matches (:1185-1206).
- **The local-override block.** A's override, then B's if greater. Clamped to the board minimum ("board minimum", "Board minimum clearance: %s.") or, for HOLE_CLEARANCE, to the board minimum hole ("board minimum hole", "Board minimum hole clearance: %s."). "Local override on %s; clearance: %s.". Matches (:1208-1250).
- **`applyConstraint`.** The `MAXIMUM_CLEARANCE` clamp for the clearance types. The disallow flags are copied (not ORed), as is the zone connection. Matches.
- **The final `CLEARANCE_CONSTRAINT` minimum clamp.** `rules.min_clearance` floors the net-class value in `constraints::clearance`.

**Board Setup > Constraints.** The other 16 ranges match `ValidateDesignRules` (min_clearance 0..25, min_connection 0..100, ..., min_silk_clearance -10..100, min_copper_edge_clearance -0.01..25), as do the `.kicad_pro` keys used as setting names and min_resolved_spokes 0..99. Exceptions: max_error (R-4) and the message text (R-5). The default copper-to-edge clearance of 0.5 mm matches `DEFAULT_COPPEREDGECLEARANCE`.

**`NODE::FixupVirtualVias` (`node.rs:222`).**
- Matches: multilayer joints skipped; `n_seg` never incremented (KiCad's dead counter); `prev_w` / width change / `max_w`; `is_locked` from the last segment; a virtual via only when `n_solid == 0 && n_vias == 0`; the second virtual via at the far end of a locked segment; the size `max_w + 2 * PNS_HULL_MARGIN`.
- Not compared: KiCad 8303b2ad also counts a solder-mask difference (`HasSolderMask()`, `GetLocalSolderMaskMargin()`) as a width change, but IR tracks carry no mask attributes (GAPS item 24), so there is nothing to compare.

**`TestTrackEndpointDangling`, the via branch.**
- Matches: no connections and no net is not an error; dangling when every connection is on the same layer (`CN_ITEM::Layer()` is the start of the range, our `layer_lo`).
- The track branch's structure matches too: hit both ends, the zone or pad/via early return under `aIgnoreTracksInPads`, the `getMinDist` tie-break, and the return when both counts are above 0. Except for R-8.

---

## Summary

| ID | What | Severity | Known before? |
|---|---|---|---|
| R-1 | The router's clearance ignores local clearances, custom rules and net ties (net class only) | high | only as a stale ADAPTED row in CODE-COMPARE |
| R-2 | Rule areas that keep out tracks or vias are not router obstacles | high | only as a stale ADAPTED row in CODE-COMPARE |
| R-3 | A pair's legs are not `SimplifyLine`d into the collinear track they continue | medium | no |
| R-4 | Board Setup max_error upper bound 1 mm, KiCad 0.1 mm (test asserts the wrong bound) | medium | no |
| R-5 | Range messages lack the unit on the lower bound; spoke and paste messages and validation differ | medium | no |
| R-6 | Pair Shove: a failed fit or walk shows pushed tracks back in place | low | no |
| R-7 | Walk around drag restore shows the last good position; KiCad shows the original (and the Stage 14 doc claim is wrong) | low | no |
| R-8 | Dangling test: f64 accuracy and distances where KiCad rounds or truncates to int; arcs by chords | low | partly |

Counts: high 2, medium 3, low 3.
