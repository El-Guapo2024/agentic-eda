# P5 router (`crates/pns`) vs KiCad `pcbnew/router` -- findings (p5-router, 2026-10-23)

Reviewer: `p5-router`. Findings only: no code was changed anywhere. KiCad side: `$KICAD_SRC_DIR` = `/home/team_braneboard_com/src/kicad-src-8303b2ad` (newer: `~/src/kicad-src-25ec2335`, used only for the newer recorded sessions). Our side: main's checkout `/home/team_braneboard_com/work/agentic-eda` at `be562f2e`.

Scope: the line placer (walkaround, shove, mark obstacles, fix/unfix, commit), the optimizer, the walkaround, the shove and the node, the dragger (segment, corner, via, arc), routes from or to the middle of a track and Route From Other End, diff pairs, meanders and length tuning, the router settings and their two dialogs. Left out, as reviewed today: springback, corner modes, virtual vias, `syncTextItem`, the via/posture actions (Stage 10/11 of `crates/pns/PARITY.md`).

Method. Four areas were read function by function against KiCad's C++ (sections F1 to F4 below; each quotes the C++ it judges), and KiCad's recorded router sessions were replayed on this router (section P, the practical check). The old divergence list of `docs/parity/CODE-COMPARE-router.md` was re-checked against the current code:
- **fixed:** D1, D2, D3, D4, D5, D6 (with two new gaps, F1-2 and F1-3), D7, D8, D11 (`mergeFull`/`mergeStep`), D12, D15, D16;
- **deliberate, documented:** D10 (our shove stops at a width change; KiCad's assembles through it);
- **still open:** D9 (F1-5), D13 (F1-7), D14 (F2-10), D17 (F1-10, F2-15), and from D18 the mark-obstacles hull snap, the walkaround cluster timeout, `optimizeTailHeadTransition`'s FANOUT_CLEANUP (F2-15);
- **doc errors found:** CODE-COMPARE section G calls `simplifyNewLine` `head.simplify()` (it is not ported, F2-6); section H calls the mark-obstacles via drag "free angle" without flagging it (F3-1); section I and `dp_placer.rs`'s header describe `tryWalkDp` wrongly (F4-7); CODE-COMPARE's (5,1) `IsSegment45Degree` example does not hold on a µm grid (F1-7); `optimizer.rs:498-501` says rounded-rect pads have no breakouts in KiCad (F2-10).

Duplicates were merged: the start-point check is F2-8 (F3-4 and P-2 are the same defect seen from the dragger review and the replay); `Simplify2` is F1-4 (repeated in F2-15).

## P. Practical check: KiCad's recorded router sessions replayed on this router

**How.** A scratch crate outside the repo (`/tmp/findings-p5-router.bIxL/replay`, path dependencies on main's `eda-pns`, `eda-kicad`, `eda-model`, `eda-drc`; built into its own target dir, nothing in the checkout touched) replays each `pns.log` the way KiCad's `qa/tools/pns/pns_log_player.cpp::PNS_LOG_PLAYER::ReplayLog` does: `EVT_START_ROUTE` = `Router::start` (or `DiffPairPlacer::start` for log mode 2), `EVT_START_DRAG` = `Dragger::start_checked`, `EVT_MOVE` = `preview` / `drag` / `move_to`, `EVT_FIX` = `fix`, `EVT_UNFIX` = `undo_last_segment`. At the end it compares what the router holds (shoved lines, fixed runs, the dragged line, the head) with the log's `addedItems` / `headItems`, segment by segment, at 0, 1, 3 and 20 µm. KiCad's own test (`qa_pns_regressions_main.cpp`, `PNS_LOG_FILE::COMMIT_STATE::Compare` → `comparePnsItems`) demands exact equality (`sa->Seg() != sb->Seg()` fails), in nm.

The 8303b2ad corpus has 7 sessions; 25ec2335 has 15 (the same 7, byte-identical except `walk-with-teardrops`'s log, plus 8 newer ones). The newer ones record newer KiCad code (`pns_dragger.cpp` now always runs the optimizer on a dragged line and has `bestAnchorForPoint`; `pns_router.cpp` no longer checks the pair gap at the start; `pns_diff_pair.cpp` differs by ~1000 lines; tuning profiles set pair widths), so a mismatch there is not by itself a divergence from 8303b2ad. Output: `/tmp/findings-p5-router.bIxL/replay-out.txt`, `replay-dp.txt`, `replay-ss1.txt`, `replay-wwt-old.txt`.

| Session (KiCad version) | What | Result here | Why, function by function |
|---|---|---|---|
| backspace1 (8303) | Shove route, 10 fixes, 9 Backspaces | **exact**: 2/2 segments at 0 µm | -- |
| issue23449-shove-lone-via-drag-crash (8303) | via drag, Shove | no crash, nothing moved, as recorded | -- |
| walk_drag_seg_against_board_edge (8303) | segment drag into the edge, Shove | 3/3 at 1 µm, 0/3 at 0 µm | µm rounding of the nm coordinates only (unit-sensitive) |
| issue22749-shove-weird-drag-track-end (8303) | route from a track end, Shove, 5 tracks pushed | 11/11 at 1 µm, 4/11 at 0 µm; same 5 tracks | µm rounding only |
| simple-shove-1 (8303) | free-hand route started in empty space, Shove, 13 tracks pushed | `Router::start` refuses ("no pad, via, or track end there to route from", P-2). Replayed through `LinePlacer::start( .., None, None, .. )`: same 13 tracks, 24/28 at 3 µm, 5/28 at 0 µm, 28/28 at 20 µm | the 4 misses are a 45-degree pushed line 2-5 µm further out than KiCad's (`Net-(R12-Pad1)` corner at x=168.434 vs 168.429): the per-line +1 µm of F1-6, plus rounding |
| simple-drag-shove-singlelayer (8303) | 14 mm segment slid 0.9 mm, Shove, 139 tracks replaced | 134/134 segments within 20 µm, 116 at 3 µm, 107 at 0 µm | **the 18 misses are one stack of parallel pushed tracks** (`/BE-2`, `/BE-1`, `/BE-0`, `/SELECT-`, `/ADR2`, `/ADR3`): KiCad's pitch is exactly 400 µm (200 width + 200 clearance), ours 401 µm, so the error grows by 1 µm per line of the stack, +4 µm .. +9 µm (`/ADR3` at y=85.905 vs 85.896). Cause: `shove.rs:984` adds `HULL_ROUNDING_GUARD` (1 µm) to every shove hull (F1-6, raised here to medium by this evidence) |
| walk-with-teardrops (8303 log) | segment drag, Walk around | **3/19**; ours 54 segments | **P-1** below: our walk accepts a 212 mm detour round the inside of the whole board outline where KiCad's recorded drag leaves the segment where it was |
| issue24132-shove-same-net-via (25ec) | Shove route that meets a via of its own net | head 2/2 at 1 µm, but KiCad also shoved the same-net via (1 removed, re-added); ours moves nothing | KiCad 25ec2335 added `SHOVE::SetCollisionFilter`; not judged against 8303b2ad |
| drag-acute-fallback, drag-walk-optimize-a, drag-walk-optimize-fix-corners (25ec) | segment drags, Walk around | 5/11, 4/9, 1/9 | 25ec2335's `DRAGGER::optimizeAndUpdateDraggedLine` always optimizes (8303b2ad only when `GetOptimizeEntireDraggedTrack()`), with `REQUIRE_OBTUSE_ANGLES` and `bestAnchorForPoint`: newer behaviour, not a divergence |
| issue21343-dp-incorrect-gap, issue25096-broken-low-aspect-connection (25ec) | pair from two track ends, Walk around | start refused: "The differential pair gap at the start point does not match the configured gap. ..." (8303b2ad's `isStartingPointRoutable` refuses too; 25ec2335 removed the check). Forced past it: 0/6 and 2/5 head segments | 21343: KiCad leaves the anchors with a 45-degree step, ours goes straight (25ec's `pns_diff_pair.cpp` is rewritten; not judged). 25096: the diagonal 33 µm off |
| issue23820-start-from-neckdown (25ec, `TCT_KNOWN_BUG`) | pair start in a neckdown, Shove | refused; forced: **gap 500 µm** where the event's sizes say 147 µm | **F4-1 (b)**, see the note under F4-1: `DpSizes::resolve` raises the gap to the P-N clearance (the `DIFF_PAIR_GAP`-less class `DIFF_IO` has clearance 0.5 mm) |
| issue25266-incorrect-width (25ec) | pair from two vias, Walk around | width 200 where KiCad's is 400 | KiCad 10's tuning profile (`MyTuningProfile`) sets the width; not in 8303b2ad |

**What the repo's own replay tests accept.** `crates/pns/tests/qa_regressions.rs` passes (12/12, run from main's prebuilt test binary with `KICAD_QA_DATA` pointed at the 8303b2ad corpus), but its gates are loose against KiCad's exact compare: `simple_shove_1_...` needs only 24 of 28 within 3 µm, `simple_drag_shove_singlelayer_reproduces_...` 80 % within 3 µm (and is slow-tier only), and `walk-with-teardrops` has no test at all. Its default `qa_root()` is a macOS path (`/Users/juanantonioluera/ws/...`), so on this machine every replay test **silently passes without running** unless `KICAD_QA_DATA` is set.

### P-1. A walk-around drag accepts a 3.5x detour round the inside of the board outline; KiCad's recorded drag does not move (`walk-with-teardrops`) (HIGH)

> **Fixed** on task-fix-e-router: e3a6b90a. On the nanometre grid the 212 mm detour no longer happened; what was left was the walk's 1 nm kinks (59 segments for KiCad's 19), which `Simplify2` (ported there) removes: 19/19 exact, test `walk_with_teardrops_drag_does_not_detour_round_the_board`. **Correction:** KiCad's recorded commit is *not* the original line -- it slides the grabbed segment to x = 59.27 (a 184 um stub between two 45-degree legs) and drops the jog below it.
- **What differs.** Session `qa/data/pcbnew/pns_regressions/walk-with-teardrops` (8303b2ad log `pns-no-hug-2.log`, settings `mode: 2` = Walk around, `optimize_dragged_track: false`): the B.Cu segment (59.673, 59.893)-(59.673, 42.586) of `/Progger/SWCLK` is grabbed in the middle and pulled left in 17 moves, 0.02-0.07 mm apart. KiCad's recorded result is the original line (all 19 segments re-added unchanged at the old x = 59.673). Ours follows the drag until the cursor reaches (59.60, 57.22), where the slid segment first comes within clearance of the parallel `/Progger/RESET` track at x = 59.273 (`drag_segment45` candidate at x = 59.600; 327 µm centre to centre, 400 needed). From there `dragger.rs::try_walkaround` (`DRAGGER::tryWalkaround`: WP_SHORTEST, length limit 30x) returns `Done` with a 55-point line 212.8 mm long (the original is 61.5 mm) that leaves at (48.163, 61.899), runs along all four sides of the board 0.501 mm inside `Edge.Cuts` (32.5..67.5 x 17.5..62.5, `min_copper_edge_clearance` 0.5) and comes back; `pv.colliding` is false and `restored` is false, so release commits it. The cluster the walk first meets is `/Progger/RESET`'s 17 items (none of them board edge); the edge cluster comes in at a later iteration. KiCad's walk of the same candidate ends `ST_STUCK` or not `ST_DONE` (the drag keeps the original line). KiCad's own result line does not collide with anything in our world (checked: each of its 19 segments against `Node::all_colliding`), so the difference is in the walk, not in the obstacles. Not localized further without running KiCad's player; candidates, in order: `walkaround.rs::Walker::process_cluster` / `line_walk::walkaround` on the thin hulls of the four `edge:N` solids (`from_ir.rs:160`, `Shape::Stadium { r: 0 }`, which KiCad builds as `SOLID`s with `SetRoutable( false )`, `pns_kicad_iface.cpp::syncGraphicalItem`), and the check-back / `shortest_alt` choice in `Walker::single_step`.
- **How to see it.** Test `walk_with_teardrops_drag_does_not_detour_round_the_board` in `qa_regressions.rs`: replay the 8303b2ad session (`route_events(&root, "walk-with-teardrops", ..)` with the board `walk-with-teardrops/pns-no-hug-2.dump`; the drag start is the first event, layer from `headItems` or the event); after every `drag`, assert the dragged line's length stays under 2x the original; at the end assert every recorded `addedItems` segment is matched within 3 µm (today 3 of 19).
- **Severity.** High: a drag commits a track three and a half times longer than the one it replaces, round the whole board, where KiCad leaves it alone.
- **Unit-sensitive.** No.

### P-2. (Counted under F2-8.) A route cannot start in empty space; KiCad starts a net-less track there (`simple-shove-1`)

> **Fixed** on task-fix-e-router: aa025e81 (see F2-8); `simple_shove_1_...` now starts through `Router::start` in empty space, 28/28 exact.
- **What differs.** `router.rs:205-210` `Router::start`: `let Some(pick) = self.pick_start(at, layer) else { return Err("no pad, via, or track end there to route from".into()) }` and `"that item has no net to route"`. KiCad `pns_router.cpp::ROUTER::isStartingPointRoutable` with no item under the cursor goes on to the dummy-line test (`dummyStartLine.SetNet( aStartItem ? aStartItem->Net() : 0 )`) and `StartRouting` places a net-less line; the recorded `simple-shove-1` session is exactly that (its head's `net` is `""`) and pushes 13 tracks. The doc comment of `Router::start` says this is deliberate ("starting mid-air has no obvious net to resolve to in this project's model and isn't a mode the task asked for"), so it is documented, but it is a behaviour KiCad has and its own QA session uses.
- **How to see it.** `Router::start(pt(&events[0]["position"]), "F.Cu", width)` on `simple.kicad_pcb` with the session's first event returns `Err` today; expected `Ok`, and a finished route commits tracks with `net == ""`.
- **Severity.** Medium, counted once under F2-8. **Unit-sensitive.** No.


## F2. Line placer, walkaround, optimizer, router session

Read against `$KICAD_SRC_DIR` = kicad-src-8303b2ad, `pcbnew/router/`. Our side is main's checkout (`/home/team_braneboard_com/work/agentic-eda`). Springback, corner modes, virtual vias, `syncTextItem` and the via/posture actions are left out, because they were reviewed today. No code was built or run for this section: every finding comes from reading the code. The proposed tests are tests nobody has written yet.


### F2-1. A via that ends the route is dropped from the commit. HIGH

> **Fixed** on task-fix-e-router: 66c9029d (`LinePlacer::fixed_vias`; test `head_via.rs::a_via_that_ends_the_route_is_committed`).
**What differs.** `Router::build_commit` (`crates/pns/src/router.rs:629-660`) emits a via only between two runs: `for w in runs.windows(2) { if w[0].layer != w[1].layer && w[0].last() == w[1].first() { ... } }` (:641). A via with no run after it is never emitted:
- the person presses V, clicks (`Router::fix` -> `LinePlacer::switch_layer`, `line_placer.rs:583`), then finishes at the via (double-click or End). `LinePlacer::finish` pushes no run, because the head is a single point (`line_placer.rs:700`: `if preview.head.point_count() >= 2`), so the via is lost;
- a via placed at the very start of a route (`switch_layer` with `head.point_count() < 2` -> `lock_fixed(node, None, Some(via))`, :604). There is no earlier run, so no window holds it;
- End or finish while a via is armed (`pending_via_layer`). `Router::finish` (:466) ignores the armed via.

KiCad adds the via to the node when it fixes it, and the commit is that node's diff. `pns_line_placer.cpp::FixRoute`:
```
    if( pl.EndsWithVia() )
    {
        auto newVia = Clone( pl.Via() );
        newVia->ResetUid();
        m_lastNode->Add( std::move( newVia ) );
    }
```
For a head with no segments: `// Nothing to commit if we have an empty line` / `if( !pl.EndsWithVia() ) return false;` ... `m_lastNode->Add( std::move( newVia ) ); m_shove->AddLockedSpringbackNode( m_lastNode );`. `ROUTER::CommitRouting( NODE* )` then writes every added item.

**How to see it.** Test `router::tests::a_route_that_ends_in_a_via_commits_the_via`: on `two_pad_board`, `start((-825,0),"F.Cu",200)`, `toggle_via(true,600,300,Some(B.Cu))`, `fix((2000,0))`, `finish((2000,0))`. Assert `commit.vias.len() == 1 && commit.vias[0].at == (2000,0)`. Today the count is 0. A second case: arm the via before the first move, fix at the start pad, route on B.Cu, finish, and assert one via. This is the usual SMD fan-out: route out of the pad, V, click, finish. The via the person placed is silently missing from the board.

**Unit-sensitive.** No.

### F2-2. Walk around mode with a via: the via is pushed off the obstacle, but the head and the commit do not follow it. HIGH

> **Fixed** on task-fix-e-router: 66c9029d (`rhWalkBase`'s via rounds; test `head_via.rs::a_walkaround_via_is_committed_where_it_was_pushed_and_the_route_goes_on_from_it`).
**What differs.** In Walk around mode (the default) and in Shove mode's walkaround fallback, our head is built and walked to the cursor. Only afterwards is the via pushed off, from the head's end (`LinePlacer::preview`, `line_placer.rs:440-452`: `let placed = self.place_via(node, rules, settings, via_pos, to_layer)`). The preview draws the via at `placed`, while the head still ends at `via_pos`.

`switch_layer` does the same (`:595`: `None => self.place_via(..., head.last()...)`) and pushes a head that ends at the cursor. The next run starts at `fixed_start()`, which is `runs.last().last()`, i.e. the head's end and not `via_pos` (`self.origin = via_pos` at :609 is only read when `runs` is empty). `build_commit` then emits the via at `w[0].last()`, which is the cursor position the pushout moved the via away from.

KiCad pushes the via out first and builds the head to the pushed position. `pns_line_placer.cpp::buildInitialLine`:
```
    const int collMask = ( aMode == RM_Walkaround ) ? ITEM::ANY_T : ITEM::SOLID_T;
    ...
        if( v.PushoutForce( m_currentNode, lead, force, collMask, iterLimit ) )
        {
            SHAPE_LINE_CHAIN line = guessedDir.BuildInitialTrace( m_p_start, aP + force, false, cornerMode );
            aHead = LINE( aHead, line );
            v.SetPos( v.Pos() + force );
            aHead.AppendVia( v );
```
KiCad also makes two attempts, the second with `lead = aP - m_last_p_end.value()`; ours makes one. `rhWalkBase` then runs a second round (`while( round < 2 && m_placingVia )`) that walks to the walked end, and `rhWalkOnly` appends the via at `aNewHead.CLastPoint()`.

**How to see it.** Extend `tests/head_via.rs::the_same_via_in_walkaround_mode_moves_the_via_not_the_track`: after the preview, `router.fix(p(-3450,0))` and `router.preview(p(-3450, 3000))`, then `finish`. Assert that `commit.vias[0].at == pv.via.0` and that `violations(&committed(&design,&commit), &model)` is empty. Today the via is committed at (-3450,0), 150 um from the B.Cu GND track: a clearance violation. Also assert `pv.head.last() == Some(pv.via.0)` on the preview.

**Unit-sensitive.** No.

### F2-3. Esc, and a finish that fails, throw away the segments already fixed. HIGH

> **Fixed** on task-fix-e-router: 9386cf68 (`Router::commit_fixed`; tests `route_api::cancel_after_two_fixes_keeps_the_fixed_runs`, `a_refused_finish_still_commits_the_fixed_runs`).
**What differs.** `Router::cancel` (`router.rs:456`) is `self.session = None`. The studio's Esc calls it (`web/studio/src/components/canvas/routing.ts:102` `cancelInteractiveRoute` -> `POST /api/route/cancel`, `crates/cli/src/route_api.rs:334`). When the last head collides, `route_api::finish` (:256) has already taken the router out of the cell (`.take()`) and returns "final segment still collides; route left uncommitted", so every earlier click is lost there too.

In KiCad, every way out of the routing loop commits what was fixed. `router_tool.cpp::performRouting`: the `IsCancelInteractive()` branch only does `break;`, and after the loop:
```
    m_router->CommitRouting();
```
`LINE_PLACER::CommitPlacement` commits `m_lastNode`, which holds every fixed leg and the shoves locked by `AddLockedSpringbackNode`. The finish path (`finishInteractive` / double-click) is `m_router->FixRoute( m_endSnapPoint, m_endItem, forceFinish, forceCommit ); break;`. It commits the fixed part even when that last FixRoute refuses.

**How to see it.** In `route_api` tests, `cancel_after_two_fixes_keeps_the_fixed_runs`: start, `fix` twice, `cancel`, `board::load`, and assert that the two fixed runs are tracks of the board (KiCad behaviour). Add a second test that finishes with the head inside a pad of another net and asserts the fixed runs are still committed.

**Unit-sensitive.** No.

### F2-4. When the walk fails, the head shows the raw colliding line, where KiCad keeps the last good head. MEDIUM

> **Fixed** on task-fix-e-router: 46da9732 (test `a_failed_route_head_keeps_the_last_good_head`). Note: a walk rarely fails outright -- `cursorDistMinimum`'s fall-back clips to a prefix -- so the case is mostly a start that collides.
**What differs.** `LinePlacer::build_head` (`line_placer.rs:345`): `None => HeadResult::plain_line(raw, true, node, current), // ST_STUCK: show the direct line, flagged violating`. A click on it is then `Blocked`.

When `routeHead` fails, KiCad restores the previous head and tail. `pns_line_placer.cpp::routeStep`:
```
        if( !routeHead( aP, newHead, newTail ) )
        {
            m_tail = std::move( prevTail );
            m_head = std::move( prevHead );
```
So in Walk around and Shove modes the trace is never shown colliding: it stays at the last position that worked, and `FixRoute` fixes that one. The FixRoute comment confirms this: "Note that collisions can occur even in walk/shove modes if the beginning of the trace collides".

**How to see it.** `line_placer::tests::a_stuck_walk_keeps_the_last_good_head`: a pad the walk cannot leave, for example the cursor inside a pad of another net with `walkaround_iteration_limit` = 1. First `preview` at a free point A, then at a stuck point B. Assert `!pv.colliding` and `pv.head.last() == Some(A)`. Then assert `fix(B) == Fixed` and that the fixed run ends at A.

**Unit-sensitive.** No.

### F2-5. Shove mode: when the solids-only walk fails, we fall back to a full walkaround, where KiCad gives up. MEDIUM

> **Fixed** on task-fix-e-router: 46da9732.
**What differs.** `line_placer.rs:282`: `if let Some(walked) = walkaround::walk_base_line(.., kind_mask::SOLID, ..) { ...shove... }`. When it is `None`, control falls through to the `Mode::Walkaround | Mode::Shove` arm, which walks around everything, tracks included. The comment at :277-280 says this is "exactly like upstream", which is wrong. KiCad `pns_line_placer.cpp::rhShoveOnly`:
```
    if( ! rhWalkBase( aP, walkSolids, ITEM::SOLID_T, RM_Shove, viaOk ) )
        return false;
```
`routeStep` then keeps the previous head (F2-4). KiCad falls back to `rhWalkOnly` only when the shove itself fails: `else { return rhWalkOnly( aP, aNewHead, aNewTail ); }`.

`rhWalkBase( SOLID_T )` fails, for example, on a long detour around a pad whose hug slice (`clipAndCheckCollisions` -> `m_currentNode->CheckColliding( &ctest )`, any kind) crosses a track. In that case ours walks the head around the track instead of shoving it or freezing.

**How to see it.** `shove_mode_does_not_walk_around_tracks_when_the_pad_walk_fails`. Set up a pad of another net that forces a detour longer than 2 x 1.5 times the direct length, with a GND track across the hug slice, in Shove mode. Assert that the head is the previous head, or flagged stuck, and never a path that hugs the track's hull. Today it is the walkaround path.

**Unit-sensitive.** No.

### F2-6. `simplifyNewLine` is not ported: a fixed leg is not merged with the collinear track it continues. MEDIUM

> **Fixed** on task-fix-e-router: 4259eaf5 (`Router::simplify_new_lines`; test `a_route_continuing_a_track_straight_is_merged_with_it`). An old track is taken in only when all of it is in the line; a commit with an arc track is left alone.
**What differs.** `CODE-COMPARE-router.md` section G says "`simplifyNewLine` is `head.simplify()`". It is not. In `pns_line_placer.cpp::simplifyNewLine`, KiCad first removes collinear overlapping dangling neighbours (`processJoint`). It then assembles the line in the node through the existing same-net segments (`aNode->AssembleLine( aLatest, nullptr, false, false, false )`), runs `OPTIMIZER::Optimize( &l, OPTIMIZER::MERGE_COLINEAR, aNode )` plus `Simplify()`, and replaces the line:
```
    if( optimized || simplified.PointCount() != l.PointCount() )
    {
        aNode->Remove( l_orig );
        l.SetShape( simplified );
        aNode->Add( l );
```
This runs on every FixRoute. So a route that starts at the end of a track and continues in its direction ends up as one segment, and so does a click in the middle of a straight run. Ours commits each run as its own track (`build_commit` -> `line_tracks` per run) and leaves the old track alone, which leaves a collinear joint.

**How to see it.** `router::tests::a_route_continuing_a_track_straight_is_merged_with_it`: a track (0,0)-(1000,0). `start((1000,0))`, `finish((3000,0))` on a free board. Assert the commit removes the old track and adds a single segment (0,0)-(3000,0). A second case: `start`, `fix((2000,0))`, `finish((4000,0))`, and assert one segment. The file holds a different segment count, but the connectivity is the same.

**Unit-sensitive.** No.

### F2-7. Remove loops looks only at the route's own two end points. MEDIUM

> **Fixed** on task-fix-e-router: df8b24e2 (test `remove_loops_follows_the_track_the_route_continues`). A track goes only when all of it is redundant.
**What differs.** `LinePlacer::remove_loops` (`line_placer.rs:738`) calls `find_lines_between_joints(start, end)` on the route's first and last points, and only at `finish`. The doc comment calls this "Deliberately coarser". KiCad `pns_line_placer.cpp::removeLoops` (called from `Move` whenever `reachesEnd` and an end item):
```
    for( int s = 0; s < aLatest.LinkCount(); s++ )
    {
        LINKED_ITEM* seg = aLatest.GetLink(s);
        LINE ourLine = aNode->AssembleLine( seg );
        ...
        aNode->FindLineEnds( ourLine, a, b );
        if( a == b )
            aNode->FindLineEnds( aLatest, a, b );
        aNode->FindLinesBetweenJoints( a, b, lines );
```
Here `ourLine` is the new route assembled through the existing tracks it continues. A route that starts at the free end of a track from pad A and reaches pad B, while an old A-B path exists, has its loop ends at A and B in KiCad, so KiCad removes the old path. Ours compares the track end with B and removes nothing.

**How to see it.** `remove_loops_follows_the_track_the_route_continues`: pad A, a track A->X, an old track A->(detour)->B, and a route from X to B. Assert that the old A-B track is in `remove_track_ids`. Today it is not.

**Unit-sensitive.** No.

### F2-8. No start-point check (`isStartingPointRoutable`), and no start in empty space. MEDIUM

> **Fixed** on task-fix-e-router: aa025e81 (the DRC start check and the empty-space start on the orphaned net; tests `a_start_point_that_violates_drc_is_refused`, `start_in_empty_space_routes_an_unconnected_track`). Still differs: a pad with no net is refused (no non-plated-hole flag), and the rule-area / text messages are not ported.
**What differs.** `Router::start` (`router.rs:205`) refuses a point with no pad, via or track end: "no pad, via, or track end there to route from", or "that item has no net to route". Otherwise it starts anywhere, even where the start already collides.

`pns_router.cpp::isStartingPointRoutable` and `StartRouting` behave differently. KiCad starts with no item on an orphaned net: `m_currentNet = aStartItem ? aStartItem->Net() : Router()->GetInterface()->GetOrphanedNetHandle();` in `LINE_PLACER::Start`. It refuses a start that collides even at the board's minimum width: `SetFailureReason( _( "The routing start point violates DRC." ) );`, unless `AllowDRCViolations()`. It also gives these messages: `_( "Cannot start routing from a non-plated hole." )`, `wxString::Format( _( "Rule area '%s' disallows tracks." ), zone->GetZoneName() )` / `_( "Rule area disallows tracks." )`, `_( "Cannot start routing from a text item." )`. The pair placer has its half (`dp_placer.rs:351`). The single track has none.

**How to see it.** `start_on_a_point_that_violates_drc_is_refused`: a start point 50 um from a pad of another net, with a 200 um width and 200 um clearance. Assert `Err("The routing start point violates DRC.")`. A second test, `start_in_empty_space_routes_an_unconnected_track`, asserts `Ok` and a commit with net "".

**Unit-sensitive.** No.

### F2-9. AttemptFinish ("F") finishes at the cursor instead of routing to the nearest unconnected anchor. MEDIUM

> **Fixed** on task-fix-e-router: 8ccd2e17 (`Router::attempt_finish`, `POST /api/route/attempt_finish`; tests `attempt_finish_routes_to_the_ratsnest_anchor`, `attempt_finish_commits_at_the_nearest_unconnected_anchor`).
**What differs.** The studio maps `pcbnew.InteractiveRouter.AttemptFinish` to `finishInteractiveRoute(cursor)` (`web/studio/src/actions/useActionRunner.ts:361`), which is `Router::finish(at)`: the route is committed wherever the cursor is. KiCad `pns_router.cpp::ROUTER::Finish`:
```
    if( !GetNearestRatnestAnchor( otherEnd, otherEndLayers, otherEndItem ) )
        return false;
    int      triesLeft = 5;
    do { moveResultPoint = placer->CurrentEnd(); Move( otherEnd, otherEndItem ); triesLeft--; }
    while( placer->CurrentEnd() != moveResultPoint && triesLeft );
    if( moveResultPoint == otherEnd && otherEndLayers.Overlaps( GetCurrentLayer() ) )
        ... return FixRoute( otherEnd, otherEndItem, forceFinish, allowViolations );
    return false;
```
When it fails, `router_tool.cpp` warps the cursor to `CurrentEnd()` and goes on routing. Ours has no `Router::attempt_finish`. `GetNearestRatnestAnchor` exists in `continue_from_end` (`router.rs:359`) and could be reused.

**How to see it.** `attempt_finish_routes_to_the_ratsnest_anchor`: on the two-pad board, start at pad 1 with the cursor in free space. Attempt finish, and assert the commit ends on pad 2. A blocked case asserts that the session is still open.

**Unit-sensitive.** No.

### F2-10. Smart Pads never reshape the exit from a rounded-rect pad (D14 still open). MEDIUM

> **Fixed** on task-fix-e-router: e78cd47f (test `smart_pads_breaks_out_of_a_roundrect_pad`; the exit is the diagonal breakout, not the `Rect` case's).
**What differs.** `optimizer::breakouts` (`optimizer.rs:502-561`) has `_ => Vec::new()` (:560) for `Shape::RoundRect`, and its doc comment (:498-501) says "A rounded rectangle reaches KiCad's router as a compound shape, which has no breakouts". `pns_kicad_iface.cpp::syncPad` gives a multi-shape pad a polygon:
```
                // For anything that's not a single shape we use a polygon.  Multiple shapes have a tendency
                // to confuse the hull generator. https://gitlab.com/kicad/code/kicad/-/issues/15553
                else
                {
                    const std::shared_ptr<SHAPE_POLY_SET>& poly = aPad->GetEffectivePolygon( aLayer, ERROR_OUTSIDE );
                    if( poly->OutlineCount() )
                        solid->SetShape( new SHAPE_SIMPLE( poly->Outline( 0 ) ) );
```
The polygon then goes through `customBreakouts` (`case SH_SIMPLE`). Rounded-rect pads are the default since KiCad 7, so smart pads do nothing on most SMD pads.

**How to see it.** `smart_pads_breaks_out_of_a_roundrect_pad`: the same input as `smart_pads_leaves_a_wide_pad_along_its_long_axis` (optimizer.rs:1001) with the pad as `Shape::RoundRect`. Assert the same exit as the `Rect` case. Today the line is unchanged. Related, low: `smartPadsSingle` skips pads with an offset ("don't do optimized connections for offset pads", `solid->Offset() != VECTOR2I( 0, 0 )`). Ours has no such test.

**Unit-sensitive.** No.

### F2-11. Snap radius for the route's end and start: 500 um to any same-net anchor. MEDIUM

> **Deferred** on task-fix-e-router, and partly **not a bug**: `pickSingleItem` queries the hover items at slop 0 and then at `maxSlopRadius = std::max( m_gridHelper->GetGrid().x, .y )` -- KiCad's grab radius is the grid size. The router does not know the studio's grid; the 500 um stands in for it. Fix: the studio sends its grid with the route's settings.
**What differs.** `LinePlacer::snap_end` (`line_placer.rs:469-495`) ends at an exact anchor, then at a magnetic track, then at `node.nearest_anchor(p, layers, SNAP_UM, Some(&self.net))` with `SNAP_UM = 500` (:37). `Router::pick_start` does the same with `START_SNAP_UM = 500` (router.rs). KiCad's end item is `TOOL_BASE::updateEndItem` / `pickSingleItem`: the item under the cursor, with the snap governed by the magnetic settings. No 0.5 mm grab radius exists in `pns_line_placer.cpp`. With the magnet off, or for a cursor the studio did not snap, ours ends the route (`real_end`) on a same-net pad or track end up to 0.5 mm away, and a click finishes the route there.

**How to see it.** `a_click_beside_a_same_net_pad_does_not_finish_the_route`: a pad r=400 at (3000,0), click at (3000,800). That is 400 um outside the pad, but within 500 um of the anchor. Assert `Fixed { real_end: false }`. Today it is `real_end: true`.

**Unit-sensitive.** Yes: `SNAP_UM`, `EXACT_UM` and `START_SNAP_UM` are in um and must be scaled by 1000.

### F2-12. The start posture continues the segment the route starts on; KiCad's never does. LOW

> **Fixed** on task-fix-e-router: 85dd866f (test `a_route_from_a_diagonal_track_end_starts_straight`); `start_diagonal` is not a setting (KiCad's default, off).
**What differs.** `LinePlacer::start` (`line_placer.rs:217-228`) sets `direction = Direction45::from_seg(..)` from the start segment and seeds the tracer with it (`trail.set_default_directions(direction, ..)`). In `pns_line_placer.cpp::Start`, the computed `lastSegDir` and `initialDir` are locals that are never used. The tracer gets `m_mouseTrailTracer.SetDefaultDirections( m_initial_direction, DIRECTION_45::UNDEFINED );`, where `m_initial_direction = Settings().InitialDirection()`, which is `N`, or `NE` with `start_diagonal`. Starting from the end of a diagonal track, ours builds the head diagonal first, and KiCad straight first. This matters until the mouse trail decides, and always with "Use mouse path" off. `start_diagonal` is also absent from `RoutingSettings`.

**How to see it.** `a_route_from_a_diagonal_track_end_starts_straight`: a track (0,0)-(1000,1000), start at (1000,1000), `auto_posture` false, preview at (3000,2000). Assert that the first segment is horizontal.

**Unit-sensitive.** No.

### F2-13. Smart Pads stay off for the whole route after one `/`. LOW

> **Fixed** on task-fix-e-router: 85dd866f.
**What differs.** `head_effort(settings, self.manually_forced)` (`line_placer.rs:272`). `self.manually_forced` is set by `flip_posture` (:666) and cleared only by `undo_last_segment` (:648). KiCad reads `!m_mouseTrailTracer.IsManuallyForced()`, and `MOUSE_TRAIL_TRACER::Clear()` resets it (`m_manuallyForced = false;`) on every FixRoute (`m_mouseTrailTracer.Clear()`). Ours also clears the tracer on a fix (`restart_trail`), but keeps its own flag.

**How to see it.** Flip, fix, then preview the next leg into a pad. Assert that the smart-pads exit is applied, which means `head_effort(..)` includes `SMART_PADS`.

**Unit-sensitive.** No.

### F2-14. `merge_obtuse` checks the whole candidate line instead of the 3-point bypass. LOW

> **Fixed** on task-fix-e-router: 812e3426 (test `merge_obtuse_checks_only_its_bypass`).
**What differs.** `optimizer.rs:202`: `if collides(node, &candidate, ...)`. KiCad `pns_optimizer.cpp::mergeObtuse` checks `opt_path` (`s1opt.A`, `ip`, `s2opt.B`) only: `LINE opt_track( *aLine, opt_path ); if( !checkColliding( &opt_track ) )`. A violation elsewhere on a shoved line blocks every obtuse merge in ours. The same rule is honoured in `merge_step` (test `a_violation_elsewhere_on_the_line_does_not_block_a_merge`). `intersect_lines` also truncates where `SEG::IntersectLines` rounds. MERGE_OBTUSE is used only by the shove at `OE_LOW` (`pns_shove.cpp:2037`).

**How to see it.** A copy of `a_violation_elsewhere_on_the_line_does_not_block_a_merge` with `effort::MERGE_OBTUSE` on an obtuse staircase.

**Unit-sensitive.** No.

### F2-15. Still open from CODE-COMPARE, confirmed in the current code. LOW unless noted

> **Partly fixed** on task-fix-e-router: `Simplify2` (e3a6b90a). Open: the `line_walk.rs` tolerances, the dead `optimizer::optimize()`, FANOUT_CLEANUP on the head, the mark-obstacles hull snap, the cluster timeout.
- `Line::simplify` (`line.rs:499-522`) is not `SHAPE_LINE_CHAIN::Simplify2`. KiCad drops every point within `LineDistance <= 1` of the infinite line `p0`-`p(n+2)`, so a back-tracking spike such as (0,0),(10,0),(5,0),(5,5) loses (10,0). On a 3-point chain it only dedups (`if( m_points[0] == m_points[1] ) Remove( 1 ); return *this;`). Ours drops only collinear points that lie between their neighbours, and does simplify 3-point chains. The optimizer, the walker (`process_cluster`) and `merge_step` all call it. Test: `simplify_drops_a_backtrack_spike` and `simplify_keeps_a_collinear_middle_of_three_points`.
- `line_walk::seg_contains` (`line_walk.rs:23`) uses `<= 1` where `SEG::Contains` uses squared distance `<= 3`. `collinear` is exact where KiCad allows `|det| <= 1`. `find` (:130) uses a squared norm where KiCad uses a rounded norm. These are rows B/D17. **Unit-sensitive: yes.** On a nanometre grid, KiCad's own tolerances apply verbatim; in um they are 1000 times looser than KiCad's.
- `optimizer::optimize()` (`optimizer.rs:254`) still runs MERGE_SEGMENTS together with MERGE_OBTUSE, which KiCad never combines. It has no caller outside tests, so it is dead.
- FANOUT_CLEANUP on the head (`LINE_PLACER::optimizeTailHeadTransition`: `if( !m_mouseTrailTracer.IsManuallyForced() && OPTIMIZER::Optimize( &linetmp, OPTIMIZER::FANOUT_CLEANUP, m_currentNode ) )`) is not run. A pad-to-via fan-out shorter than 10 x width keeps the posture's elbow where KiCad straightens it. MEDIUM for fan-outs. It is in the "Left" list.
- `rhMarkObstacles`' hull snap ("snap to the hull of the first obstacle", `if( ( nearest - aP ).EuclideanNorm() < m_head.Width() / 2 ) buildInitialLine( nearest, ... )`) is absent (`line_placer.rs:331`). Known.
- The `m_PNSProcessClusterTimeout` wall-clock bound is absent from `Walker::process_cluster`. Known.

### F2: checked and matches
- `rhWalkBase` with no tail (`walkaround::walk_base_line`): policies `{CCW, CW}`, MERGE_SEGMENTS of each DONE walk against the world, `len_ccw < len_cw` (CW wins ties), `hugThresholdLengthComplete = 2 x initial x threshold`, and the `cursorDistMinimum` fallback. The fallback covers global-minimum selection (the local minimum is discarded, as in KiCad), the `clipAndCheckCollisions` threshold and collision with any kind, and the choice `distCw < distCcw && validCw` else `validCcw`. `optimizer.SetCollisionMask` is not ported, and correctly so: KiCad's `OPTIMIZER::checkColliding( ITEM* )` ignores the mask (`return static_cast<bool>( m_world->CheckColliding( aItem ) );`).
- `WALKAROUND::Route` / `singleStep` / `processCluster`: `Simplify2` before each hull, statuses, the length cut-off (10x), the end-point checks (STUCK if the start moved, ALMOST_DONE if the end moved), the `WP_SHORTEST` check-back against processed items, and the bounding-box hull in 90-degree modes.
- `OPTIMIZER::mergeFull` / `mergeStep`: the pre-`Simplify2`, the step schedule, the bypass via `BuildInitialTrace( s1.A, s2.B, i, cornerMode )` in the router's corner mode, collision check of the bypass only, a per-candidate `Simplify2`, the tie rule `cost[0] < cost_orig && cost[0] < cost[1]` else `cost[1] < cost_orig`, skipping arc segments, and the `hasArcs` gating. D11 is fixed.
- `COST_ESTIMATOR::CornerCost` table (10/5/50/30/60/100).
- `smartPadsSingle` / `runSmartPads`: `p_end`, the inside-pad skip, the breakout x diag loop with `diag==0` = start diagonal, the forbidden-angle test, the length rejection, the variant cost and length tie-break, the end-vertex arithmetic, and no optimisation for vias. `rectBreakouts`, `circleBreakouts` and `customBreakouts` match in order and geometry (`+5` nm becomes `+1` um).
- `fanoutCleanup` structure, apart from the corner mode: ours is always MITERED_45, while KiCad's uses the router corner mode. Low.
- Effort gating for the head: `OE_LOW` -> 0, MEDIUM/FULL -> MERGE_SEGMENTS, plus SMART_PADS in 45-degree modes only. The shove-mode post-shove optimization runs against the shoved world.
- `FixRoute` with `GetFixAllSegments()` off: `lastV`, `p_pre_last`, `CSegment( -2 )` direction, arcs forcing fix-all, and real end / placing via fixing everything.
- `AllowDRCViolations()` = MarkObstacles && `can_violate_drc` (D8 fixed). `free_angle` only in MarkObstacles.
- `Move`: a route ending collinear on a track ends where it first touches the track (`targetSeg.NearestPoint( lastSeg.A )`).
- D1 is fixed: no via at a same-layer joint. D15 is fixed: one cluster per iteration.
- `ROUTER_TOOL` Ctrl+E / `ContinueFromEnd`: covered in `router.rs:359`, not re-reviewed beyond the commit path.

### F2 summary

| ID | Finding | Severity | Unit-sensitive |
|---|---|---|---|
| F2-1 | A via that ends the route (or is armed at finish) is dropped from the commit | high | no |
| F2-2 | Walk around mode: the via is pushed out, but the head and the commit stay at the cursor (DRC violation) | high | no |
| F2-3 | Esc, or a refused finish, discards the fixed segments (KiCad commits them) | high | no |
| F2-4 | A stuck walk shows the colliding raw line instead of keeping the last good head | medium | no |
| F2-5 | Shove mode falls back to a full walkaround when the solids-only walk fails | medium | no |
| F2-6 | `simplifyNewLine` is not ported (no merge with the collinear track continued, or across clicks) | medium | no |
| F2-7 | Remove loops uses only the route's own end points | medium | no |
| F2-8 | No `isStartingPointRoutable` (start-DRC check, messages), no start in empty space | medium | no |
| F2-9 | AttemptFinish finishes at the cursor instead of `ROUTER::Finish` | medium | no |
| F2-10 | Smart pads skip rounded-rect pads (D14) | medium | no |
| F2-11 | 500 um end and start snap radius to same-net anchors | medium | yes |
| F2-12 | The start posture continues the start segment (KiCad: `InitialDirection()`); no `start_diagonal` | low | no |
| F2-13 | `manually_forced` is not reset on a fix (smart pads stay off) | low | no |
| F2-14 | `merge_obtuse` checks the whole line | low | no |
| F2-15 | Simplify2 semantics, line_walk tolerances, FANOUT_CLEANUP, mark-obstacles snap, cluster timeout (still open) | low to medium | partly |


## F1. Shove and node

Compared against `$KICAD_SRC_DIR` (8303b2ad): `pcbnew/router/pns_shove.cpp`, `pns_node.cpp`, `pns_joint.h`, `pns_via.cpp`, `pns_item.cpp`, `pns_line.cpp`, `pns_utils.cpp`, `pns_topology.cpp`, `pns_kicad_iface.cpp`, `libs/kimath/src/geometry/{shape_collisions,shape_line_chain,seg}.cpp`. Left out: springback, corner modes, virtual vias, `syncTextItem`, the via/posture actions.


### F1-1. A head that ends in a via shoves a track on another layer around the head's own segments as well as the via (HIGH)

> **Fixed** on task-fix-e-router: e3a6b90a (the `LayersOverlap` test and `collideSimple`'s layer check), test 19156fb6. The mis-shove itself did not reproduce on main: with the finding's set-up the shove already went round the via only.
**What differs.** `SHOVE::ShoveObstacleLine` (pns_shove.cpp:574) uses the lone-via path whenever the pusher's via is the only part of it on the obstacle's layer:
```cpp
if( viaOnEnd && ( !aCurLine.LayersOverlap( &obstacleLine ) || aCurLine.SegmentCount() == 0 ) )
{
    // Shove obstacleLine to the hull of aCurLine's via.
    return shoveLineFromLoneVia( aCurLine, obstacleLine, aResultLine );
}
```
Ours, `shove.rs:964`, checks only `if via_on_end && cur.seg_count() == 0`. It has no layer test. So when an F.Cu head that ends in a via hits a B.Cu track (a track found only through the via: `Node::nearest_obstacle` queries the via on all its layers, node.rs:977-982), ours builds one hull per **F.Cu** head segment plus the via hull (shove.rs:974-990) and walks the B.Cu track around all of them.

A second gap makes it worse. KiCad's final collision test, `l.Collide( &aCurLine, .. )`, returns early when the layers differ (`ITEM::collideSimple`, pns_item.cpp: `// check if we are not on completely different layers first  if( !m_layers.Overlaps( aHead->m_layers ) ) return false;`). Our `lines_collide` (shove.rs:799-808) runs the track-against-track test with no layer check. The same applies in `on_colliding_solid` (shove.rs:1121) and `shove_line_from_lone_via` (shove.rs:875).

**How to see it.** Test `a_head_via_shoves_a_track_on_the_other_layer_around_the_via_only`:
- head: F.Cu, net A, (0,0)-(5000,0), width 200, ending in a through via of diameter 600 at (5000,0);
- obstacle: B.Cu, net B, track (4700,-3000)-(4700,3000), width 200;
- clearance 200;
- call `shove_heads_after(node, vec![ShoveHead{ line, via: Some(v) }], ..)`.

KiCad bends the B.Cu track round the via's hull only. Every point of the shoved line stays at x <= 4700 and |y| <= ~1200. Assert that no point of the displaced line has x > 5000 or x < 0. Today the B.Cu track is wrapped round the hull of the F.Cu segment it crosses (around the head's start, or past the via's end), or the shove fails.

**Fix pointer.** Add `|| !layers_overlap(cur, obstacle)` to the lone-via test, and a layer-overlap test to the track part of `lines_collide`.

**Unit-sensitive:** no.

### F1-2. `pushOrShoveVia` ignores a locked joint: the via a route starts from can be shoved (MEDIUM)

> **Fixed** on task-fix-e-router: 3c76a35e (test `a_via_on_a_locked_joint_is_not_shoved`).
**What differs.** KiCad, pns_shove.cpp:1063:
```cpp
if( Settings().ShoveVias() == false || aVia->IsLocked() )
    return SH_TRY_WALK;

if( jt->IsLocked() )
    return SH_INCOMPLETE;
```
`SHOVE::Run` locks the head's start joint (and its end joint, unless the head ends in a via): `m_currentNode->LockJoint( head.CPoint( 0 ), &head, true )`. Ours locks those joints in `line_placer.rs:294-296`. But `push_or_shove_via` (shove.rs:1191-1205) never reads `joint.locked`. Only `assemble` honours the lock (`stop_at_locked_joints`).

**How to see it.** A route in Shove mode starts at a via V of net A. The head pushes a net-B track, and the pushed track then runs into V. KiCad stops this shove (`SH_INCOMPLETE`). Ours moves V, drags the head's first segment with it (V's joint links the head segments, shove.rs:1213-1229) and commits the route's start somewhere else.

Test `a_shoved_line_does_not_move_the_via_the_route_starts_from`: assert that `outcome.displaced_vias` does not contain V, and that `outcome.head[0]` is V's position.

**Unit-sensitive:** no.

### F1-3. `VIA::PushoutForce` clamps each step to a quarter of the diameter; KiCad does not (MEDIUM)

> **Fixed** on task-fix-e-router: 3c76a35e. **Correction:** with `max_iterations = 1` KiCad returns false as well (the loop ends at the limit after the one step); two iterations free the via (test `a_via_placed_on_a_pad_is_pushed_off_within_the_via_force_iteration_limit`).
**What differs.** In KiCad, pns_via.cpp:205-208, the clamp is a no-op. `VECTOR2::Resize` is `const` and returns a new vector (vector2d.h:190), and the result is thrown away:
```cpp
// Limit the force magnitude to, say, 25% of the via diameter ...
if( forceMag > threshold )
{
    force.Resize( threshold );
}
totalForce += force;
```
The same code is in 25ec2335 (pns_via.cpp:223). KiCad therefore moves the full MTV in one step. Ours, `shove.rs:395-401` (`via_pushout_force`), really clamps to `diameter/4`.

Two effects:
- a large push takes several of the 40 iterations (`via_force_prop_iteration_limit`). After `max_iterations/2` (the `iter > max/2 && magnitude > threshold` branch), ours switches to the `aDirection` lead earlier and more often than KiCad;
- the end position is different when the via runs into more than one obstacle on its way.

Callers: `DRAGGER::dragViaWalkaround`, the line placer's via pushout (`pns_line_placer.cpp:2103`) and the pair placer.

**How to see it.** Test `via_pushout_moves_the_full_mtv_in_one_step`: a via of diameter 600 whose centre sits 100 µm inside a 2 mm pad's clearance (MTV about 500, more than 150 = d/4). With `max_iterations = 1`, KiCad returns `Some(mtv)`. Ours returns `None` (`iter == max_iterations`), because it needs four steps.

**Unit-sensitive:** no.

### F1-4. `Line::simplify` stands in for `SHAPE_LINE_CHAIN::Simplify2` in every shove step; `SelfIntersecting` misses a reversal (MEDIUM)

> **Fixed** on task-fix-e-router: e3a6b90a (`Line::simplify2`), 3c76a35e (`SelfIntersecting`); tests `simplify2_*`, `self_intersecting_flags_a_fold_back`.
**What differs.** KiCad's shove calls `Simplify2()`:
- after every hull walk (`path.Simplify2()`, pns_shove.cpp:416);
- on the shoved line (`shovedLine.Line().Simplify2()`, :666);
- in `preShoveCleanup` (:2367), `onCollidingSolid` (:844) and `pushOrShoveVia` (:1104).

`Simplify2` (shape_line_chain.cpp) drops any point that lies on the *infinite* line through its neighbours:
```cpp
while( n < np - 2
        && ( SEG( p0, pts_unique[n + 2] ).LineDistance( pts_unique[n + 1] ) <= 1
          || SEG( p0, pts_unique[n + 2] ).Collinear( SEG( p0, pts_unique[n + 1] ) ) ) )
    n++;
```
It also leaves a 3-point chain alone apart from a duplicate first point.

Ours, `line.rs:499-522`, drops a point only when it is exactly collinear **and between** its neighbours (`on_segment(a, c, b)`). It also drops the middle point of a 3-point chain. So a doubled-back spike such as (0,0),(10,0),(5,0),(5,5) survives here and is removed in KiCad.

The safety net misses it too. KiCad's `SelfIntersecting` flags adjacent segments that fold back (`seg1.Contains( b2 )` for `s2 == s1 + 1`, with `Contains` = squared distance <= 3), and would reject such a path. Ours (`shove.rs:156-169`) tests only `j >= i + 2`, with exact intersections. A shoved line with a zero-area spike can therefore be accepted and committed.

This is CODE-COMPARE rows B "Simplify2" and B "SelfIntersecting" and D17. They are **still open**.

**How to see it.**
- Unit test `simplify_drops_a_backtracking_point_like_simplify2`: `Line::from_points(.., [(0,0),(10,0),(5,0),(5,5)]).simplify()` gives `[(0,0),(5,0),(5,5)]`.
- Unit test `self_intersecting_flags_a_fold_back`: `self_intersecting(&[(0,0),(10,0),(5,0)])` is `true`.

**Unit-sensitive:** yes, partly. KiCad's `<= 1` tolerance is in nm, so exact collinearity in µm is close to it today. After the nm change, exact collinearity is stricter than KiCad's. The between/beyond difference and the 3-point rule are not unit-related.

### F1-5. The clearance epsilon is 1 µm; KiCad's is the DRC epsilon, 0.5 µm (D9, still open) (MEDIUM)

> **Fixed by the nanometre merge** (`CLEARANCE_EPSILON` = 500 nm, KiCad's default DRC epsilon). Not read from the board's own epsilon.
**What differs.** `node.rs:35`: `pub const CLEARANCE_EPSILON: Um = 1`. `colliding_with` and `nearest_obstacle` subtract it (node.rs:947, :985). KiCad, `PNS_PCBNEW_RULE_RESOLVER::Clearance` (pns_kicad_iface.cpp:824-825):
```cpp
if( aUseClearanceEpsilon && rv > 0 )
    rv = std::max( 0, rv - m_clearanceEpsilon );
```
with `m_clearanceEpsilon = aBoard->GetDesignSettings().GetDRCEpsilon()` (:235), i.e. 500 nm by default.

On the µm grid, a gap of `clearance - 1 µm` passes our collision test ("not colliding"). KiCad (c - 0.5) rejects it.

**How to see it.** Test `a_gap_one_micron_short_is_a_collision`: a segment and a pad whose gap is `clearance - 1` µm. Assert that `node.all_colliding(..)` is non-empty. Today it is empty.

**Unit-sensitive:** yes. In nm the constant should become 500, or better, be read from the board's DRC epsilon.

### F1-6. Shove hulls carry a 1 µm `HULL_ROUNDING_GUARD`, and the arc padding does not accumulate (MEDIUM; was LOW before the replay)

> **Fixed** on task-fix-e-router: e3a6b90a (guard removed, arc padding accumulates; with `collideSimple`'s `- 1` the replays are exact: simple-drag-shove-singlelayer 134/134).
**What differs.** `shove.rs:984`: `segment_hull(a, b, w, clearance + extra + pad + HULL_ROUNDING_GUARD, ..)`. KiCad: `seg.Hull( clearance + extraHullExpansion, obstacleLineWidth, .. )`, with no guard.

KiCad also grows `clearance` in place, so the arc padding accumulates over segments and attempts (pns_shove.cpp:586-592):
```cpp
if( aCurLine.CLine().IsArcSegment( i ) )
    clearance += KiROUND( SHAPE_ARC::DefaultAccuracyForPCB() );
```
That is +5 µm per arc chord. Every later hull is bigger, non-arc segments included, and the growth carries into the next attempt. Ours adds a constant 5 µm to the arc chords only. Our own comment at shove.rs:977-979 says it "piles up", but the code does not do that.

Result: every shoved track sits 1 µm further out than KiCad's. This is the "within 3 µm" tolerance of `qa_regressions.rs`. Behind an arc, ours pushes less than KiCad does.

**Replay evidence.** `simple-drag-shove-singlelayer`: a stack of six parallel tracks pushed one by one comes out at a 401 µm pitch where KiCad's is exactly 400 µm, so the error grows with every line of the stack (+4 µm .. +9 µm); these are 18 of the 134 recorded segments that miss at 3 µm. A file KiCad would have written differently, hence medium.

**How to see it.**
- Test `shoved_track_sits_at_exact_clearance`: a straight head pushing a parallel track. The gap equals the clearance (today clearance + 1).
- Test `arc_pusher_hulls_accumulate_padding`: a head made of an arc of N chords and then a straight segment. The straight segment's hull is inflated by 5·N µm.

**Unit-sensitive:** yes. The guard exists for µm rounding and can go in nm.

### F1-7. `SegmentHull` kink handling (D13, still open; reassessed) (LOW)

> Open (low).
**What differs.** In `hull.rs:285-351`:
- (a) `len` is truncated (`sqrt() as Um`); KiCad's `SEG::Length()` rounds;
- (b) ours computes `d`, `dr`, `xr2` **after** `cl += 1/2`. In KiCad they are computed before, so the bump only reaches `OctagonalHull` in the `a == b` case:
```cpp
int cl = aClearance + aWalkaroundThickness / 2;
double d = (double)aSeg.GetWidth() / 2.0 + cl;
...
if( std::abs(w) <= 1 ) { w = 0; cl ++; }
```
Result: a kink stub's hull is 1-2 µm too big.
- (c) `is_45` is exact where `IsSegment45Degree` allows ±1. In µm this is the right translation of KiCad's ±1 nm. The (5,1) example in CODE-COMPARE does not hold on a µm grid. It becomes wrong after the nm change.

**How to see it.** Test: `segment_hull((0,0),(1,7), 200, 200, 0)`. Its vertices match the re-typed KiCad hull (`d` taken before the bump).

**Unit-sensitive:** yes for (a) and (c); (b) no.

### F1-8. One joint per (position, net) whatever the layer (LOW)

> Open (low).
**What differs.** `node.rs:297-317` (`touch_joint`) and `joint.rs` key a joint by `(pos, net)` and merge layer ranges. KiCad keeps one joint per *overlapping* layer range (`NODE::touchJoint`). `FindJoint( p, aItem )` looks one up by the item's layers.

Our comment says disjoint ranges are "not a real configuration for the two-outer-layer boards". On a two-layer board they occur: an F.Cu SMD pad and a B.Cu track end at the same point and net, with no via. Ours puts the pad in the B.Cu track's joint, so `next_segment` returns `None` there (`Some(Item::Solid(_)) => return None`): the B.Cu line ends at a pad it is not connected to. A locked joint also locks both layers. The QA boards (4- and 6-layer) can have blind or buried vias.

**How to see it.** Test `a_pad_on_another_layer_does_not_end_a_line`: B.Cu segments (0,0)-(1000,0)-(2000,0) of net N, plus an F.Cu SMD pad of net N centred at (1000,0). `assemble_line(first)` has 3 points.

**Unit-sensitive:** no.

### F1-9. `onCollidingSolid`: cluster and walk exclusions differ from KiCad (LOW)

> Open (low).
**What differs.**
- (a) Ours skips `head_marked` and the current line's segments when it builds the cluster (`shove.rs:1077-1082`, `assemble_cluster(.., &|id| skip.contains(&id))`). In KiCad `MK_HEAD` is never set: `//head.Mark( MK_HEAD );` at pns_shove.cpp:2491, and every other use is inside `#if 0`. `TOPOLOGY::AssembleCluster`'s `!( obs.m_item->Marker() & MK_HEAD )` therefore never excludes anything, and a head segment that touches the pad joins the cluster. A shoved line of another net that walks round a pad the head ends on also walks round the head's touching segment in KiCad. Ours walks round the pad only, then hits the head in the next iteration.
- (b) The `edge_walk_bound` (shove.rs:1083) is our own addition. KiCad has none; it is documented in the code.
- (c) `if let Some(lv) = &cur.via { let Some(via_id) = self.joint_via(..) else { return Status::Incomplete } ..}` (shove.rs:1069-1071). KiCad (pns_shove.cpp:780-799) returns `SH_INCOMPLETE` only when the *joint* is missing; with a joint but no via in it, it falls through to the walk.
- (d) `trackOnTrack` is `SEGMENT_T` only in KiCad; ours `is_track()` includes arcs (node.rs:1069).

**How to see it.** For (c): a line whose detached `via` copy is not in the node, hitting a pad. KiCad walks, ours fails.

**Unit-sensitive:** no.

### F1-10. Smaller helper tolerances (D17, still open) (LOW)

> Open (low).
- `changed_area` (shove.rs:203): `sq_distance_to_point(p2) > 1`, where `LINE::ChangedArea` uses `!s.Contains( p2 )`, i.e. squared distance <= 3.
- `line_walk::find` (line_walk.rs:130): squared distance <= thr², where KiCad's `Find` uses the rounded norm.
- `point_in_polygon` on the boundary (`checkShoveDirection`'s `POINT_INSIDE_TRACKER` says "not inside" on the edge).
- `solid_via_mtv` for a via whose centre lies exactly on a rectangle's edge: KiCad's `inside` is inclusive (`c.x >= p0.x && c.x <= ..`); ours needs `best > 0`. A polygon pad (`SH_SIMPLE` in KiCad, so `Collide( CIRCLE, LINE_CHAIN_BASE )` with the `closest_mtv_seg` push) is approximated with the rectangle formula.

**How to see it.** Unit tests per helper, against the quoted KiCad rule.

**Unit-sensitive:** yes; all are 1-unit tolerances.

### F1: checked and matching (no need to review again)
- `shoveIteration` (shove.rs:1407-1500): search order SOLID, VIA, TRACK; `fixupViaCollisions`; `unwindLineStack( ni )`; the higher-rank branches (via: `patchTadpoleVia`, then via-via `onCollidingVia( .., rank+1 )` or `onReverseCollidingVia`; segment: pop, unwind, tadpole, `FindViaByHandle`, `onCollidingLine( revLine, current, rank+1 )`, push; arc: rank-1); the lower-rank branches with the `SH_TRY_WALK` fall-backs to `onCollidingSolid`. Arcs are searched with segments: a documented deviation (KiCad's mask leaves `ARC_T` out).
- `shoveMainLoop`: the iteration and time limits checked after every iteration, the off-by-one included.
- `shoveLineToHullSet`: 4 attempts, `invertTraversal`, `clockwise = attempt % 2`, `minDistP` with `PointInside`, then 0, append/insert order, per-hull walk, then simplify (see F1-4), endpoint equality, direction check, self-intersection (see F1-4), collision. The missing `vFirst/vLast + CompareGeometry` gate is redundant with the endpoint check. `c_ENDPOINT_ON_HULL_THRESHOLD` 1000 nm becomes `< 1.0` µm: on the µm grid that means exactly on the hull, which is unit-sensitive.
- `ShoveObstacleLine`: `voeStart/voeEnd`, the obstacle's via removed and re-appended, 3 attempts with +1 µm (1000 nm), `permitMoving*` from attempt 2, the via hull. Exceptions: F1-1, F1-6, and the via's hole clearance, which is not modelled ("holes are not modelled").
- `shoveLineFromLoneVia`: cw/ccw walk, direction pick, endpoint and collision tests. Hole clearance not modelled; `lines_collide` see F1-1.
- `onCollidingSegment` / `onCollidingLine` / `onCollidingArc`: ranks, `Simplify2` (see F1-4), unwind, replace, push. `onCollidingArc` returns `Incomplete` on a failed shove where KiCad returns `SH_OK` (documented).
- `onCollidingVia`: the MTV sources (line chain at `clearance + width/2` with the fan-out-enlarged diameter; via-via, which takes priority; solid) and their signs, checked through `collideSingleShapes`' `CollCaseReversed`; `pushoutForce` (5 corrections, `isqrt` distance); circle-circle `+3`; rectangle-circle inside/outside formulas. The degenerate centreline push is a documented deviation.
- `pushOrShoveVia`: zero force means `SH_OK`, `ShoveVias`/locked means `SH_TRY_WALK`, the "do not land on a joint" loop (`Resize( 2 )`), fan-out assembly, reversal at `segIndex == 0`, `DragCorner` 45 then `Simplify2`, the stitching-via `tmpLine`, `replaceLine` / `LinkVia` / rank / `rootEntry->newLine`. Exception: F1-2.
- `onReverseCollidingVia`: via-via test, then the fan-out tracks (on the current layer) shove the current line, the lone-via fallback, `AppendVia`, unwind / replace / push / rank.
- `unwindLineStack` (the tadpole keeps its via), `pushLineStack` (the "not linked" refusal), `pruneLineFromOptimizerQueue`, `popLineStack`, `patchTadpoleVia`, `fixupViaCollisions` (`maxw >= diameter` gives `maxw + 1`; a segment end via narrower than the track becomes the obstacle).
- `checkShoveDirection`: `cp`, the lone via, `SHP_REVERSED` only for the head's root, the tracker over obstacle + reversed shoved.
- `runOptimizer`: effort mapping (LOW: OBTUSE x1; MEDIUM/FULL: SEGMENTS x2), queue reversed each pass, heads skipped, SMART_PADS only in 45-degree modes, area = `totalAffectedArea()` inflated by the widest line. `LIMIT_CORNER_COUNT` is a no-op in KiCad (`CORNER_COUNT_LIMIT_CONSTRAINT::Check` always returns true). Not ported, and justified: `Intersect( VisibleViewArea() )`, since the backend has no view.
- `SHOVE::Run`: head rank 100000, the head via added and linked, "empty head" refused, `removeHeads`, the `LockJoint` calls (made by the line placer, line_placer.rs:294; but see F1-2).
- `NODE::NearestObstacle` (node.rs:965-1015): per-segment and via queries, hulls at `GetClearance( .., eps ) + width/2` with thickness 0, the via hull at `+ diameter/2`, `HullIntersection` validity (corner side test, `index_our` wrap), `PathLength` (rounded norms, the last-index rule), the first `dist == 0` wins, the `INT_MAX` fallback to `obstacles[0]`, and `simplifyHull` box hulls in 90-degree modes. KiCad orders its set by pointer, so our id order is as good.
- `NODE::AssembleLine` / `followLine` (node.rs:435-550): the start-anchor guard and no backward walk after it, `MaxVerts` 16384, `aStopAtLockedJoints` (the corner is recorded, then the walk breaks), `JOINT::NextSegment` (branch counting, locked neighbours skipped, a solid or via is a hard stop, the virtual-via exception) (D2 fixed). Deliberate deviation, documented: with `allow_width_mismatch: false` ours **stops** at a width change. KiCad's shove assembles with the default `aAllowSegmentSizeMismatch = true` and so changes the narrow part's width (D10, "Widths survive").
- `TOPOLOGY::AssembleCluster`: overlap query (clearance 0, any net), the `trackOnTrack` skip, the excluded net, a track's bbox grown by its whole line, the area-ratio break. Exceptions: F1-9.
- `LINE::ChangedArea`: matches apart from the tolerance in F1-10.
- D3: `split` tolerance <= 3 confirmed at line_walk.rs:151, so FIXED; I did not re-read `Seg::intersect`'s rescale.
- D4: `convex_hull_octagon` uses the minimum `LineDistance` over the vertices, so FIXED.
- D5, D12: FIXED.
- D6: FIXED, with F1-2 and F1-3 as new gaps.
- Rounded-rectangle pads: KiCad makes them a `SHAPE_SIMPLE` polygon (pns_kicad_iface.cpp:1560-1575, since the compound has 5 sub-shapes) and hulls it with `ConvexHull`. Ours unions the rectangle hull and 4 segment hulls (hull.rs:445-455). Both are octagons whose diagonals sit at `r + cl` from the corner centres, so the outlines are the same up to collinear vertices; not a finding.


## F3. Dragger, mid-track routes, settings and dialogs

KiCad side: `$KICAD_SRC_DIR` = kicad-src-8303b2ad. Ours: main's checkout `/home/team_braneboard_com/work/agentic-eda`.
Not covered here, since they were reviewed today: springback, corner modes, virtual vias, syncTextItem, via/posture actions.


### F3-1. A via dragged in Highlight collisions mode, or in a drag forced into marking obstacles, drags its tracks at any angle, not 45 degrees. HIGH

> **Fixed** on task-fix-e-router: 0ec7ad7b (test `mark_obstacles_via_drag_keeps_its_tracks_at_45_degrees`).
- **What differs.** `crates/pns/src/dragger.rs:285-296` (`via_candidate`, used by `solve_mark_obstacles` at :338-349) sets the end vertex of each attached line straight to the cursor. KiCad's `pns_dragger.cpp::DRAGGER::dragViaMarkObstacles` re-solves each line with the drag's angle mode:
  ```cpp
  draggedLine.DragCorner( aP, origLine.CLine().Find( aHandle.pos ), m_freeAngleMode );
  ```
  `m_freeAngleMode` is false for `D` (`DM_ANY`), so this is `dragCorner45`. A free-angle leg is right only for `G`. `CODE-COMPARE-router.md` section H describes this as "moves the end vertex straight to the cursor (free angle, fanout)" but does not flag it as a divergence.
- **Why it matters.** In Highlight collisions mode, a non-colliding release commits `preview.fanout` (`commit_of`, :631-638). The board gets tracks at arbitrary angles where KiCad writes 45-degree elbows. The same thing happens in Walk around or Shove mode once the first position has failed (`m_forceMarkObstaclesMode`): Ctrl+click then commits off-angle tracks.
- **How to see it.** Test `dragger::tests::mark_obstacles_via_drag_keeps_its_tracks_at_45_degrees`: a via at (0,0) with one track (0,0)-(5000,0), `Mode::MarkObstacles`, `Dragger::start(.., via)`, `drag(.., (2000,1300))`. Assert `on_45(&pv.fanout[0].pts)` and that `pv.fanout[0]` has 3 points (KiCad's elbow). Today it is the 2-point line (2000,1300)-(5000,0). Repeat with `free_angle=true` and assert the straight line.
- **Unit-sensitive:** no.

### F3-2. A refused click ends the drag; in KiCad the drag goes on. MEDIUM

> **Fixed** on task-fix-e-router: 107d73e8.
- **What differs.** In `router_tool.cpp::ROUTER_TOOL::performDragging` a click only leaves the loop when the fix succeeds:
  ```cpp
  if( m_router->FixRoute( m_endSnapPoint, m_endItem, forceFinish, forceCommit ) )
      break;
  ```
  `DRAGGER::FixRoute` returns false in a forced-mark-obstacles drag without Ctrl ("if( aForceCommit ) ... return false;"). It also returns false when `Drag( m_lastValidPoint )` fails. Either way the drag stays live and the hint stays up. Ours: `crates/cli/src/route_api.rs:457-475` `drag_finish` `take()`s the router out of the cell before calling `drag_finish_with`. On `None` it replies `ok:false`. `web/studio/src/components/canvas/dragging.ts:121-127` `finishInlineDrag` then clears the draw state and returns to Select whatever the reply says. The drag is lost: the person must press `D` again, and the forced-mark state starts over.
- **How to see it.** Browser check, or a `route_api` unit test: start a drag whose first position collides (forced mark). Call `drag_finish` without `force_commit`; it replies `ok:false`. Then call `drag_move` and assert `ok:true`. Today the reply is "not dragging".
- **Unit-sensitive:** no.

### F3-3. `D` on a locked track from the selection tool asks "Drag Anyway"; KiCad refuses with an info-bar warning. MEDIUM

> **Fixed** on task-fix-e-router: 3891f733 (the studio has no Override locks, so the warning is the short 'Selection contains locked items.' the other tools give).
- **What differs.** `D` from the select tool is `edit_tool.cpp::EDIT_TOOL::Drag`. Its selection filter calls `FilterCollectorForLockedItems`, which drops locked items unless Override Locks is on. When the selection comes out empty:
  ```cpp
  if( selection.Empty() ) { m_selectionTool->ReportFilteredLockedItems(); return 0; }
  ```
  That shows `ShowInfoBarWarning( _( "Selection contains locked items. Enable 'Override locks' to operate on them." ) )`. The question `KIDIALOG( frame(), _( "The selected item is locked." ), _( "Confirmation" ) ... SetOKLabel( _( "Drag Anyway" ) )` is only in `ROUTER_TOOL::performDragging`, which runs when `D` is pressed while the router tool is active. Ours: `web/studio/src/actions/useActionRunner.ts:461-470` (`startDragAtCursor`) plus `dragging.ts:72-76` ask "Drag Anyway" in every tool. The dialog also lacks "Do not show again" (`dlg.DoNotShowCheckbox`), which is already listed as a known gap.
- **How to see it.** In the select tool, lock a track and press `D` over it. KiCad shows the info-bar warning and does not drag. Ours opens the Confirmation dialog.
- **Unit-sensitive:** no.

### F3-4. A single-track route that starts in violation is not refused ("The routing start point violates DRC."). MEDIUM -- same defect as F2-8, counted there

> **Fixed** on task-fix-e-router: aa025e81 (see F2-8).
- **What differs.** `pns_router.cpp::ROUTER::isStartingPointRoutable` (`PNS_MODE_ROUTE_SINGLE`) builds a zero-length line of `m_sizes.TrackWidth()` at the start point. If it collides, it retries at `BoardMinTrackWidth()`. If that still collides, it marks the violations and fails:
  ```cpp
  SetFailureReason( _( "The routing start point violates DRC." ) );
  return false;
  ```
  The check is skipped when `AllowDRCViolations()`. It also refuses non-routable items with "Cannot start routing from a non-plated hole.", "Rule area '%s' disallows tracks." / "Rule area disallows tracks." and "Cannot start routing from a text item.". Ours: `crates/pns/src/router.rs:205-238` `Router::start` has no such check. The only copy of the message is in `dp_placer.rs:351`, for a pair. A route starts on a pad squeezed between two others and shows a colliding head from its first move.
- **How to see it.** Test `router::tests::a_start_point_that_violates_drc_is_refused`: two pads of different nets 300 um apart, clearance 200, track width 250 and board minimum 250. `start` on one pad returns `Err("The routing start point violates DRC.")`. With `mode = MarkObstacles` and `can_violate_drc = true` it returns `Ok`.
- **Unit-sensitive:** no.

### F3-5. Router settings are not saved; KiCad keeps them in `pcbnew.json` (`tools.pns`). MEDIUM

> **Fixed** on task-fix-e-router: 6d8b5506 (kept in the browser; the limits KiCad keeps only in the file are still not settable from the studio).
- **What differs.** `pns_tool_base.cpp::TOOL_BASE::Reset` does `settings->m_PnsSettings = std::make_unique<ROUTING_SETTINGS>( settings, "tools.pns" )`. `ROUTING_SETTINGS` is a `NESTED_SETTINGS` with `PARAM`s for every field (`mode`, `effort`, `remove_loops`, ..., `optimize_dragged_track`, `fix_all_segments`, `corner_mode`, the limits), so the dialog's choices survive a restart. Ours: `web/studio/src/state/store.tsx:1037` `routerSettings: DEFAULT_ROUTER_SETTINGS`, and `SET_ROUTER_SETTINGS` (:1658) only changes React state. Nothing reads or writes storage, so a reload goes back to Walk around with the defaults. The limits KiCad keeps only in the file (`shove_iteration_limit`, `shove_time_limit`, `walkaround_iteration_limit`, `via_force_prop_iteration_limit`, `walkaround_hug_length_threshold`) cannot be set from the studio at all; `route_api.rs::settings_over` does accept them.
- **How to see it.** Open Interactive Router Settings, choose Shove, press OK and reload the page. KiCad still has Shove; ours is back to Walk around. A `routerSettings` round-trip test (save, then load into the store's initial state) fails today because there is nothing to save.
- **Unit-sensitive:** no.

### F3-6. "Differential Pair Dimensions..." turns custom pair sizes on for every later pair; KiCad's dialog alone does not. MEDIUM

> **Fixed** on task-fix-e-router: b3f153c1.
- **What differs.** `router_tool.cpp::ROUTER_TOOL::DpDimensionsDialog` applies the result to the running router (`m_router->UpdateSizes( sizes )`) and stores it as the board's custom values (`bds.SetCustomDiffPairWidth/Gap/ViaGap`). It does **not** call `UseCustomDiffPairDimensions( true )`. The next pair's sizes come from `PNS_KICAD_IFACE_BASE::ImportSizes`, which takes the net class while `bds.UseNetClassDiffPair()` holds (`m_diffPairIndex == 0 && !m_useCustomDiffPair`). Only the menu item does `bds.UseCustomDiffPairDimensions( true ); toolManager->RunAction( PCB_ACTIONS::routerDiffPairDialog );` (`DIFF_PAIR_MENU::eventHandler`, `ID_POPUP_PCB_SELECT_CUSTOM_DIFFPAIR`), and that flag stays set even on Cancel. Ours: `web/studio/src/actions/pcbRouterSweep.ts:82-98` dispatches `customDiffPair: v` on OK from either entry, and `Canvas.tsx:759` starts every later pair with it. Cancel from "Use Custom Values..." leaves custom off, which also differs from KiCad.
- **How to see it.** On a board whose net class pair is 0.2/0.25, run the Route menu's Differential Pair Dimensions... with 0.3/0.3 and press OK, then start a new pair. KiCad routes 0.2/0.25 and the menu still checks "Use Net Class Values". Ours routes 0.3/0.3 and checks "Use Custom Values...".
- **Unit-sensitive:** no.

### F3-7. Fallback pair size 125/180 um where KiCad's Default class is 200/250/250 um. MEDIUM

> **Fixed** on task-fix-e-router: 92e696f7.
- **What differs.** `crates/model/src/lib.rs:849-858` (`diff_pair_width_of` 125, `diff_pair_gap_of` 180) and `pcbRouterSweep.ts:27-31` (`netClassDiffPair`: 125/180, and `viaGapSameAsTrackGap` checked when the class has no via gap) use the `SIZES_SETTINGS` constructor values. Those only apply before a board exists. A KiCad board's Default net class always has pair sizes (`netclass.cpp::NETCLASS::NETCLASS`, `aInitWithDefaults`):
  ```cpp
  const int DEFAULT_DIFF_PAIR_WIDTH  = pcbIUScale.mmToIU( 0.2 );
  const int DEFAULT_DIFF_PAIR_GAP    = pcbIUScale.mmToIU( 0.25 );
  const int DEFAULT_DIFF_PAIR_VIAGAP = pcbIUScale.mmToIU( 0.25 );
  ```
  A board made with `eda board new` (no pair fields) routes and tunes pairs at 125/180 where KiCad uses 200/250. `ImportSizes` also always does `SetDiffPairViaGapSameAsTraceGap( false )`, so KiCad's dialog opens with the box unchecked.
- **How to see it.** `BoardRules` with no class pair values: `diff_pair_width_of("x")` gives 125 today and should give 200; `diff_pair_gap_of` gives 180 and should give 250.
- **Unit-sensitive:** touched (the literals are µm and would become nm), not fixed by the change.
- (This belongs as much to the pair review, F4.)

### F3-8. The Diff Pair Dimensions dialog's validation and via gap handling. LOW

> **Fixed** on task-fix-e-router: b3f153c1 (the error stays inline with OK disabled, not a modal).
- **What differs.** `PcbRouterDialogs.tsx:136-141`. KiCad (`dialog_pns_diff_pair_dimensions.cpp::TransferDataFromWindow`) checks the gap only, on OK, with a modal `DisplayErrorMessage( this, _( "Track gap must be greater than 0." ) )` and focus on the gap field. Ours disables OK and shows inline text. It adds two messages KiCad does not have, "Width must be greater than 0." and "Via gap must be a length.". KiCad saves the via gap field's own value whatever the checkbox says (`m_sizes.SetDiffPairViaGap( m_viaGap.GetValue() )`, with `DiffPairViaGap()` returning the gap while the flag is set). Ours overwrites `viaGapUm` with the gap when the box is ticked, so unticking later shows the gap rather than the old via gap.
- **How to see it.** Open the dialog with via gap 0.3 and gap 0.2. Tick the box, OK, reopen and untick: KiCad shows 0.3, ours shows 0.2.
- **Unit-sensitive:** no.

### F3-9. Text and title details. LOW

> **Partly fixed** on task-fix-e-router: 94b98df1 (title, 179.0°), 107d73e8 ('(Ctrl+Click to commit anyway.)'). Open: the hint on every motion, our own messages for Route From Other End and for nothing to drag.
- `RouterSettingsDialog.tsx:78` title "Interactive Router Settings..." vs KiCad's `DIALOG_PNS_SETTINGS_BASE` `_("Interactive Router Settings")`.
- Drag violation hint: `dragging.ts:109` and `route_api.rs:471` say "Ctrl+click". KiCad's is `_( "(%s to commit anyway.)" )` with `KeyNameFromKeyCode( MD_CTRL + PSEUDO_WXK_CLICK )` = "Ctrl+Click", so "Track violates DRC." + "(Ctrl+Click to commit anyway.)". `route_api.rs:471` also drops the parentheses. KiCad shows it on every motion while `GetForceMarkObstaclesMode( &dragStatus )` holds and `!dragStatus`. Ours toasts once at the start.
- `arc_drag.rs:182` "Unable to resize arc tracks of 179° or greater." vs KiCad's `MessageTextFromValue( ANGLE_180 - maxTangentDeviation )` = "%.1f°", so "179.0°". KiCad shows it in the info bar (`ShowInfoBarError`).
- `dragger.rs:222` "The arc has no corner to round: its tangents do not meet." is ours only: `DragArcTrack` returns 0 silently when `!tanIntersect`.
- `useActionRunner.ts:467` "Nothing to drag there -- hover a track or via first." KiCad does nothing (an empty `RequestSelection`).
- `router.rs` Route From Other End messages ("Route From Other End: nothing is left to connect to.", "...does not start on a connection.") are ours. KiCad shows `ShowInfoBarError( m_router->FailureReason(), true )`, which is empty or stale on that path, and greys out the menu item (`hasOtherEnd`).
- **Unit-sensitive:** no.

### F3-10. The arc-size refusal is unsigned here and signed in KiCad. LOW

> Left as is: KiCad's signed test reads as a KiCad bug; to decide deliberately.
- **What differs.** `arc_drag.rs:193` `arc.central_angle().abs() + MAX_TANGENT_ANGLE_DEVIATION >= 180.0`. KiCad: `if( theArc->GetAngle() + maxTangentDeviation >= ANGLE_180 )`. `PCB_ARC::GetAngle()` is signed (`angle1.Normalize180() + angle2.Normalize180()`), so KiCad only refuses arcs of one winding. A 179.5-degree arc of the other winding is dragged in KiCad and refused here. This mirrors a KiCad bug; decide deliberately.
- **How to see it.** An arc start (1000,0), mid (0,-1000), end (-1000,10) and the same arc reversed: KiCad refuses one and drags the other, ours refuses both.
- **Unit-sensitive:** no.

### F3-11. Free-angle grab next to an arc. LOW

> **Fixed** on task-fix-e-router: ad4bbfbe (test `a_free_angle_grab_next_to_an_arc_takes_the_segments_start`).
- **What differs.** `dragger.rs:252-256`. KiCad's `startDragSegment` (free angle) adds a third condition: `( !m_draggedLine.CLine().IsPtOnArc( static_cast<size_t>(m_draggedSegmentIndex) + 1 ) )`. Ours does not check it, so `G` near the far end of a segment that runs into an arc takes the arc's end vertex. `dragCornerFree` then inserts a vertex and the result differs.
- **How to see it.** A line segment then a tangent arc. `G` grab at 70% along the segment: KiCad's `grabbed_index` is the segment's start, ours is the arc's start.
- **Unit-sensitive:** no.

### F3-12. The drag's mode is latched at Start in KiCad. LOW

> **Fixed** on task-fix-e-router: 2b740a03 (test `a_drag_keeps_the_mode_it_started_in`).
- **What differs.** `DRAGGER::Start`: `m_currentMode = Settings().Mode();`, and `m_shove` is only created if that mode is Shove. Ours: `Dragger::drag` / `solve` read `settings.mode` on every call (`dragger.rs:433-439`), and `RouterSettingsDialog.tsx` sends a new mode to a running drag (`routeSetSettings`). Changing the mode mid-drag switches algorithms here and does nothing in KiCad. The other settings, such as smooth or optimize-entire, are read live in KiCad too.
- **Unit-sensitive:** no.

### F3-13. Lead vector of `dragViaWalkaround`. LOW (already documented)

> Open (low).
- `dragger.rs:557` `lead = via_pos - to`. KiCad: `-m_mouseTrailTracer.GetTrailLeadVector()` = `-(m_trail.CLastPoint() - m_trail.CPoint(0))`, with the trail starting at the grab point (`AddTrailPoint( aP )` in `Start`), not at the via centre. They differ by the grab offset, and wherever the tracer's simplification has dropped the first point. `mouse_trail.rs` now exists and could be used.
- **Unit-sensitive:** no.

### F3: checked and matches (do not re-review)
- `DRAGGER::startDragSegment`: corner within `w2` (`distA < w2 || distB < w2`, double norm), `distB <= distA` takes `index+1`, DM_SEGMENT otherwise, and the free-angle `PointCount() - 2` rule (apart from F3-11). `DRAGGER::Start`: segment/via/arc dispatch and `checkVirtualVia` order. The locked start item is unmarked on Drag Anyway.
- `assemble` = `AssembleLine( aSeg, &idx )` defaults (no locked-segment follow, width mismatch allowed).
- `dragMarkObstacles` corner/segment: threshold `SmoothDraggedSegments() ? w/4 : 0`, `DragCorner( .., m_freeAngleMode )`, status `AllowDRCViolations() || !CheckColliding`.
- `dragWalkaround`: threshold `w/4`, `DragCorner` 45 degrees, collision check then `tryWalkaround` (WP_SHORTEST, iteration limit, length limit 30), `PointCount() < 2` gives not ok, `optimizeAndUpdateDraggedLine`.
- `dragShove` corner/segment: threshold `w/2`, the dragged line taken out of the world, `SHP_REVERSED` for DM_CORNER index 0, the optimizer on the shove's node. Via: SHOVE head; `ShoveVias() == false || IsLocked()` gives `SH_TRY_WALK`, then walkaround.
- `dragViaWalkaround`: PushoutForce with `ViaForcePropIterationLimit`, `DragCorner( viaTargetPos, Find( pos ), false )`, walkaround only for colliding lines, optimizer only on walked lines.
- `optimizeAndUpdateDraggedLine`: flags, `ChangedArea` or `BOX2I( aP )`, the anchor `Find`/`NearestPoint`, `Split( anchor )`, PRESERVE_VERTEX.
- `DRAGGER::Drag`: firstDrag fallback to forced mark, `m_lastValidPoint` only when ok, restore-last-solution (with the documented Walk around difference). `DRAGGER::FixRoute` branches (ours also calls `drag(to)` first, which is harmless because the last Move is at the same point).
- `LINE::DragCorner` / `dragCornerFree`, including the arc-end insert. The comment in `line.rs` says it differs, but the resulting points are the same as KiCad's. `DragSegment` asserts free angle in KiCad and ours never calls it that way.
- `TOOL_BASE::snapToItem` for segments and arcs: end within `w/2` (squared compare, `distA_sq < distB_sq ? A : B`), else on the segment or arc (`router.rs::pick_start`).
- `LINE_PLACER::SplitAdjacentSegments`: no split where a joint with at least one link exists; two clones replace the segment. The end split only when the head reaches the end item on the same net (`Move`). The Collinear+Overlaps end clip (`targetSeg.NearestPoint( lastSeg.A )`).
- `ROUTER::ContinueFromEnd` / `GetNearestRatnestAnchor` / `TOPOLOGY::NearestUnconnectedAnchorPoint` (`LinkCount >= 2`, or `>= 3` with a via), `NearestUnconnectedItem` (nearest anchor), the next layer `Overlaps( currentLayer ) ? currentLayer : Start()`, commit then restart, `Move( currentEnd )`, and APPEND_UNDO when something was placed.
- `ROUTING_SETTINGS` constructor defaults: every field ours has matches. `snap_to_tracks` defaults to false in KiCad, but `checkSnap` overwrites it from the magnetic setting, so ours is equivalent.
- The `DIALOG_PNS_SETTINGS` rows, their order, labels and tooltips word for word, `onModeChange` enabling, "Suggest track finish" hidden. The extra "Optimizer effort" row is a documented addition.
- `DIALOG_PNS_DIFF_PAIR_DIMENSIONS` labels ("Width:", "Track gap:", "Via gap:", "Via gap same as track gap"), the title, and the via-gap field disabled while the box is ticked.
- Known and documented, not re-reported: no MULTI_DRAGGER or COMPONENT_DRAGGER (`ROUTER::StartDragging` with more than one segment or with solids); DM_ARC judged by the PNS (an adaptation); routes must start on a net (KiCad allows net-less); "Do not show again" in Drag Anyway; dragger springback.

### F3 summary
| # | Finding | Severity | Unit-sensitive |
|---|---|---|---|
| F3-1 | Via drag in Highlight collisions or forced mark: tracks at any angle, not dragCorner45 | high | no |
| F3-2 | A refused drag click ends the drag (KiCad keeps dragging) | medium | no |
| F3-3 | `D` on a locked item from Select asks Drag Anyway (KiCad: info-bar warning, no drag) | medium | no |
| F3-4 | = F2-8 (counted there) | -- | no |
| F3-5 | Router settings not saved (KiCad `tools.pns` in pcbnew.json) | medium | no |
| F3-6 | DP Dimensions dialog switches on custom sizes for later pairs | medium | no |
| F3-7 | Pair fallback 125/180 µm vs Default class 200/250/250 µm | medium | touched |
| F3-8 | DP dialog validation text and via gap kept | low | no |
| F3-9 | Titles and messages: dialog title, "Ctrl+Click", "179.0°", invented messages | low | no |
| F3-10 | Arc refusal unsigned vs KiCad signed angle | low | no |
| F3-11 | Free-angle grab ignores `IsPtOnArc( idx+1 )` | low | no |
| F3-12 | Drag mode read live (KiCad latches at Start) | low | no |
| F3-13 | dragViaWalkaround lead vector from via centre, not mouse trail | low | no |


## F4. Diff pairs and length tuning

Scope: `crates/pns/src/{diff_pair,dp_placer,dp_meander_placer,meander_placer,tuning_path,tuning_commit}.rs`, the pair optimizer (`optimizer.rs` `optimize_pair`), the pair start in `router.rs`, the tuner session `crates/cli/src/{tune_api,tune_edit}.rs`. KiCad: `$KICAD_SRC_DIR` = kicad-src-8303b2ad, `pcbnew/router/pns_diff_pair*.cpp`, `pns_router.cpp`, `pns_kicad_iface.cpp`, `pns_topology.cpp`, `pns_meander*.cpp`, `pns_dp_meander_placer.cpp`, `generators/pcb_tuning_pattern.cpp`. Read only. Not covered here: springback, corner modes, virtual vias, via/posture actions; the meander geometry the oracle (`tests/meander_oracle.rs`) already checks.

---

### F4-1. A pair's gap is raised to the P-N clearance and its via gap comes from the net class's `diff_pair_via_gap`; KiCad uses the DIFF_PAIR_GAP constraint, floored at the board minimum clearance (HIGH; was medium before the replay)

> **Fixed** on task-fix-e-router: 92e696f7 (test `dp_sizes_take_the_gap_and_the_via_gap_from_the_pair_gap`). (c), the order of the minimum-clearance check, is left: it cannot fire from the rules in either.

**What differs.** `dp_placer.rs:76-82` `DpSizes::resolve`:
```rust
gap: rules.diff_pair_gap_of(net_p).max(clearance),
via_gap: rules.diff_pair_via_gap_of(net_p).max(clearance),
```
with `clearance = Node::clearance(rules, P, N)`, the net-to-net resolved clearance.

KiCad `pns_kicad_iface.cpp::PNS_KICAD_IFACE_BASE::ImportSizes` (net-class mode, the default) never reads the class's via gap. Both the gap and the via gap come from the `DIFF_PAIR_GAP` constraint, and the floor is the board minimum clearance:
```cpp
int diffPairGap = bds.m_MinClearance;
int diffPairViaGap = bds.m_MinClearance;
...
if( m_ruleResolver->QueryConstraint( PNS::CONSTRAINT_TYPE::CT_DIFF_PAIR_GAP, &dummyTrack,
                                     &coupledTrack, m_startLayer, &constraint ) )
{
    diffPairGap = std::max( diffPairGap, constraint.m_Value.Opt() );
    diffPairViaGap = std::max( diffPairViaGap, constraint.m_Value.Opt() );
```
`router_tool.cpp::ROUTER_TOOL::prepareSizes` repeats the floor: `sizes.SetDiffPairGap( std::max( bds.m_MinClearance, constraint.m_Value.Opt() ) )`.
- (a) The `V` vias of a pair sit `viaGap()` = `max( DiffPairViaGap, holeToHole + drill - diameter )` apart. Ours uses the class's `diff_pair_via_gap`, which can be any value. KiCad uses the class's diff pair gap.
- (b) The gap is raised to the P-to-N clearance, where KiCad raises it only to `m_MinClearance`. The module header documents this as deliberate. It is still a behaviour change: a class with clearance 0.25 mm and a pair gap of 0.2 mm routes at 0.25 mm here and at 0.2 mm in KiCad.
- (c) Since (b) has already raised the gap, the `"Diff pair gap is less than board minimum clearance."` check in `DiffPairPlacer::start` can never fire. KiCad's `ROUTER::isStartingPointRoutable` also makes that check *before* `FindDpPrimitivePair`; ours makes it after.

**How to see it.** Test `dp_sizes_take_the_via_gap_from_the_pair_gap`: rules with a class `usb` setting `diff_pair_gap: 200` and `diff_pair_via_gap: 500`, clearance 200, board minimum clearance 200. `DpSizes::resolve(&rules, "USB_P", "USB_N").via_gap` should be 200; it is 500 today. For (b): a class clearance of 250 with `diff_pair_gap: 200` gives `gap == 200` in KiCad and 250 here.

**Replay evidence.** `issue23820-start-from-neckdown` (25ec2335 corpus; this part of `ImportSizes` is the same in 8303b2ad): net class `DIFF_IO` has `clearance` 0.5 mm and no pair gap; KiCad's recorded sizes are `diffPairGap: 147000` nm, ours `DpSizes::resolve` gives 500 µm, so the pair is routed 3.4 times further apart than KiCad routes it. A board with the common "class clearance above the pair gap" set-up (e.g. 0.2 mm clearance, 0.15 mm 90-ohm gap) gets every pair at the wrong gap.

**Severity.** High (was medium): the gap (b) is a wrong result, the routed pair's geometry differs from KiCad's on every such board; (a) and (c) alone would be medium. **Unit-sensitive.** No.

### F4-2. Arc tracks are not handled by the pair router, the pair tuner or the skew tuner. (medium)

> **Deferred** on task-fix-e-router: arcs in the pair router, the pair tuner and the skew tuner are a port of their own (`DP_PRIMITIVE_PAIR`, `BuildFromPrimitivePair`, `AssembleDiffPair` for arcs).

**What differs.** KiCad treats `SEGMENT_T | ARC_T` alike everywhere in the pair code. Ours handles `Item::Segment` only:
- `diff_pair.rs:443` `DpPrimitivePair::directional` is `matches!(self.prim_p, Some(Item::Segment(_)))`, and `anchor_direction` (:449) handles only segments. KiCad `pns_diff_pair.cpp::DP_PRIMITIVE_PAIR::Directional`: `return m_primP->OfKind( ITEM::SEGMENT_T | ITEM::ARC_T );`, and `anchorDirection` the same.
- `diff_pair.rs:~769` `build_from_primitive_pair`: `let track = |i: &Item| matches!(i, Item::Segment(_));`, otherwise `return` with no gateways. KiCad `DP_GATEWAYS::BuildFromPrimitivePair`: `else if( aPair.PrimP()->OfKind( ITEM::SEGMENT_T | ITEM::ARC_T ) && aPair.PrimN()->OfKind( ITEM::SEGMENT_T | ITEM::ARC_T ) ) { buildDpContinuation( aPair, aPreferDiagonal ); return; }`. `find_dp_primitive_pair` *does* accept two arc ends (`dangling_anchor` handles `Item::Arc`). A pair started on two arc ends therefore has no entry gateways: `fit_gateways` finds nothing, `route_head` returns false and the pair never gets a head.
- `dp_placer.rs` `start`: the start-gap check (`"The differential pair gap at the start point does not match the configured gap. ..."`) and the width inheritance run only for `(Item::Segment, Item::Segment)`. KiCad `pns_router.cpp::ROUTER::isStartingPointRoutable`: `if( aStartItem->OfKind( ITEM::SEGMENT_T | ITEM::ARC_T ) )`.
- `dp_meander_placer.rs:60` `assemble_diff_pair` (`let Some(Item::Segment(start_seg)) = world.get(start) else { return None }`; `n_items` are segments only), `:306` `skew_setup` and `DpMeanderPlacer::start` all refuse an arc. KiCad `pns_topology.cpp::TOPOLOGY::AssembleDiffPair` pairs arcs by centre and radius:
  ```cpp
  else if( p_item->Kind() == ITEM::ARC_T ) { ... if( centerDist_sq > SEG::Square( DP_PARALLELITY_THRESHOLD ) ) continue;
      dist_sq = SEG::Square( p_arc->CArc().GetRadius() - n_arc->CArc().GetRadius() ); }
  ```
  It also computes the gap from the radii. `DP_MEANDER_PLACER::Start` and `MEANDER_SKEW_PLACER::Start` accept `ITEM::SEGMENT_T | ITEM::ARC_T`. So clicking an arc of a pair with the pair tuner gives `NO_PAIR` ("Unable to find complementary differential pair net for length tuning. ..."), and the skew tuner says "Please select a differential pair track you want to tune.". KiCad tunes both.
- `dp_meander_placer.rs` `do_move` `add_corners_until` ("a chain without arcs") adds an arc's chord points as corners. KiCad's `addCornersUntilIndex` calls `m_result.AddArc( *p_item.arc, *n_item.arc )` for arcs. A pair stretch that contains an arc (when the click is on a straight) comes out as the arc's 1 µm polygonisation instead of an arc.

**How to see it.** `tests/dp_boards.rs`: two parallel arc tracks on `USB_P` and `USB_N` (same centre, radii r and r + pitch), each with a dangling end. `DiffPairPlacer::start` on one end succeeds, but `move_to` 3 mm beyond returns false and `preview().head_a.pts` is empty. Expected: a head of at least 2 points. For the tuner: `DpMeanderPlacer::start(world, env, arc_id, ...)` should be `Ok`; today it is `Err(NO_PAIR)`.

**Severity.** Medium: the operation is refused or broken on boards with arcs. No data loss. **Unit-sensitive.** No.

### F4-3. The skew tuner's status box shows the partner's length as its min and max once the cursor moves. (medium)

> **Fixed** on task-fix-e-router: 909853d5.

**What differs.** `crates/cli/src/tune_edit.rs:705` (`Placer::Single` arm of `Tuner::mv`) builds the readout's `target` from `p.target()`. For a skew placer that is `meander_placer.rs` `target()`: `(s.coupled_length + target_skew.min(), …opt, …max)`, an absolute length. But `readout.current` is `tuning_length_result()` = `last_length - coupled_length`, the skew. When the move did nothing, `judge(length, target)` (`:745`) also compares the skew against `coupled + range`, so the status comes out "too_short".

KiCad `pcb_tuning_pattern.cpp::PCB_TUNING_PATTERN::GetPreviewItems`:
```cpp
if( m_tuningMode == DIFF_PAIR_SKEW ) { ...
    statusItem->SetMinMax( m_settings.m_targetSkew.Min(), m_settings.m_targetSkew.Max() );
...
    statusItem->SetCurrent( static_cast<double>( placer->TuningLengthResult() ), _( "current skew" ) );
```
The min and max are the skew's, around 0.

**How to see it.** Extend `tune_api.rs::the_skew_tuner_lengthens_the_clicked_line_until_it_matches_its_partner`. After `let m = mv(...)`, assert `(m["readout"]["min"].as_f64(), m["readout"]["max"].as_f64()) == (Some(-0.1), Some(0.1))`. Today it is about (21.3, 21.5), N's length ± 0.1 mm, and the box flags the current value red. The hover readout is right; the test already checks it.

**Severity.** Medium: wrong message. **Unit-sensitive.** No.

### F4-4. "Unable to tune segments inside other tuning patterns." is missing: a pattern can be started on another pattern's tracks. (medium)

> **Fixed** on task-fix-e-router: 909853d5 (the dangling-members question was not checked).

**What differs.** `tune_api.rs:157` `start` → `tune_edit::pick` takes any track under the cursor. The tuner's world is `from_ir::build_node_for_tuner` → `build_node_with(design, model, true)` (`from_ir.rs:41`), which unlocks the members of *every* pattern. KiCad `pcb_tuning_pattern.cpp::DRAWING_TOOL::PlaceTuningPattern`, on the first click:
```cpp
if( dynamic_cast<PCB_TUNING_PATTERN*>( m_pickerItem->GetParentGroup() ) )
{
    m_frame->ShowInfoBarWarning( _( "Unable to tune segments inside other "
                                    "tuning patterns." ) );
}
```
The comment at `from_ir.rs:37-38` itself says that KiCad marks only the pattern being edited `IN_EDIT`, and that every other pattern's members stay locked.

**How to see it.** Test in `tune_api.rs`: tune a track and fix it. Then `start` with `x, y` on one of the new pattern's member tracks. Expect `ok == false` and `message == "Unable to tune segments inside other tuning patterns."`. Today a second session starts, and fixing it replaces tracks that the first pattern lists in `member_ids`. Whether that leaves the first pattern's members dangling in the written `(generated ...)` was not checked; if it does, the severity is high.

**Severity.** Medium (could be high, see above). **Unit-sensitive.** No.

### F4-5. Pair width inheritance covers only two segment ends. (low/medium)

> Open (low).

**What differs.** `dp_placer.rs` `start`: `if let (Item::Segment(sp), Item::Segment(_)) = (&prim_p, &prim_n) { sizes.width = sp.width; }`. KiCad `ImportSizes`: `if( bds.m_UseConnectedTrackWidth && aStartItem ) found = inheritTrackWidth( aStartItem, &diffPairWidth, startPosInt );`. `m_UseConnectedTrackWidth` comes from `board.auto_track_width`, default `true` (`project_local_settings.cpp:200`). `inheritTrackWidth` also takes the width of the track joined to a start *pad or via* (`linkedSegs ... FilterKinds( SEGMENT_T | ARC_T )`, the one whose far end is nearest the cursor), and of an arc. Ours inherits the width unconditionally, but only from two segment ends: a pair started from pads that already have stubs takes the class width, where KiCad takes the stub's.

**How to see it.** Two pads of `USB_P` and `USB_N`, each with a 150 µm stub; the class width is 200. `DiffPairPlacer::start` on a pad should give `sizes.width == 150`; it gives 200.

**Severity.** Low (wrong default in an uncommon start). **Unit-sensitive.** No.

### F4-6. The pair start messages differ from KiCad's. (low)

> **Fixed** on task-fix-e-router: 85199c97.

**What differs.** `router.rs:828` `start_diff_pair`: `"no pad, via, or track end there to route from"`, which comes from `nearest_anchor` within `START_SNAP_UM` = 500 µm. KiCad `ROUTER::isStartingPointRoutable`: `_( "Cannot start a differential pair in the middle of nowhere." )` when there is no start item. A click in the middle of a pair's track has a start item in KiCad (the segment), so FindDpPrimitivePair answers `"Can't find a suitable starting point.  If starting from an existing differential pair make sure you are at the end."`. Ours finds no anchor within 500 µm and gives its own text.

**How to see it.** `Router::start_diff_pair` 3 mm from either end of a `USB_P` track → `Err` should be KiCad's "Can't find a suitable starting point. ..." text. Far from anything, it should be "Cannot start a differential pair in the middle of nowhere.".

**Severity.** Low (message). **Unit-sensitive.** No.

### F4-7. Stale doc claims about `tryWalkDp`. (low, doc)

> **Fixed** on task-fix-e-router: 92e696f7 (`dp_placer.rs` header and `try_walk_dp` doc), 2ab996c9 (CODE-COMPARE row I).

**What differs.** The `dp_placer.rs` header (lines ~20-23) says: "A walk that fails shows the pair as it was drawn ... Here a failed walk leaves the pair where route_head put it and reports that it does not fit". But `try_walk_dp` (`dp_placer.rs` ~ :395) does what KiCad does: `best.unwrap_or_default()` → empty pair → `true`. KiCad: `if( bestScore > 0.0 ) { ...aPair.SetShape( best ); optimizer.Optimize( &aPair ); return true; }`. The code is right and the header is stale. Also, `CODE-COMPARE-router.md` row I (`rhWalkOnly / attemptWalk / tryWalkDp`) and the `try_walk_dp` doc say the walk keeps "the longest coupled length, then least skew". KiCad (`double score = cl + fabs( skew ) * 3.0; if( score < bestScore )`) and ours both keep the *lowest* `1 + coupled + 3|skew|`, i.e. the *shortest* coupled length.

**Severity.** Low (doc). **Unit-sensitive.** No.

### F4-8. nm tolerances restated as µm are 10 to 1000 times looser. (low)

> **Fixed by the nanometre merge** (`GAP_CHECK_SLACK` 100 nm, `ApproxParallel` thresholds in nm, `EPSILON` 5 nm); `HULL_ROUNDING_GUARD` in `attemptWalk` replaced by `2 * PNS_HULL_MARGIN` on task-fix-e-router (e3a6b90a).

**What differs.**
- `diff_pair.rs` `GAP_CHECK_SLACK = 1` µm. KiCad `pns_diff_pair.cpp::checkGap`: `SEG::Square( gap - 100 )`, i.e. 100 nm.
- `approx_parallel(…, 2)` in `coupled_segment_pairs`, `approx_parallel(…, 1)` in `coupled_length_of_segs` and `cursor_orientation`: µm here. KiCad's `ApproxParallel( sn, 2 )` and the default `aDistanceThreshold = 1` are nm (`seg.h:298`).
- `dp_placer.rs` `attempt_walk`: `cur.gap - HULL_ROUNDING_GUARD` (1 µm). KiCad: `shove.ForceClearance( true, cur.Gap() - 2 * PNS_HULL_MARGIN )`, 20 nm.
- `buildDpContinuation` `EPSILON = 5` nm becomes "exact" here.

`dp_meander_placer.rs` is already in nm (`DP_PARALLELITY_THRESHOLD = 5`, `GAP_TOLERANCE = 10_000`) and is fine.

**Severity.** Low: edge cases at micrometre scale. **Unit-sensitive.** Yes: the nm change should restore KiCad's constants.

### F4-9. The pair's end item is picked by anchor distance, not by shape. (low)

> Open (low).

**What differs.** `dp_placer.rs` `pick_end_item`: pads and vias, then tracks, of either net whose *anchor* is within `SNAP_UM` (500 µm), nearest first. KiCad `pns_tool_base.cpp::TOOL_BASE::updateEndItem`: `for( NET_HANDLE net : nets ) { endItem = pickSingleItem( mousePos, net, layer, false, { m_startItem } ); if( endItem ) break; }`. That is P's net first, then N's, hit by shape (a large pad counts anywhere on it), gated by `checkSnap`. Snapping onto the target pads differs near big pads, and when both nets have candidates.

**Severity.** Low. **Unit-sensitive.** No.

---

### F4: checked and matches (no need to review again)
- `FindDpPrimitivePair`: kind match, `shapeMatches` layers rule, nearest dangling anchor, P/N order by `refNet != netP`, and the three messages word for word. `getDanglingAnchor` for via, solid, segment and arc.
- `DIFF_PAIR_PLACER::viaGap`, `gap()`, `routeHead` (lead-distance rule `> (gap + width) / 2`, `BuildForCursor( fpProj )` + `FilterByOrientation( ANG_STRAIGHT | ANG_HALF_FULL, dirV )`, `SetFitVias`, set gap → fit → set width/gap, `AppendVias` / `RemoveVias`).
- `propagateDpHeadForces`: virtual via diameters, mode masks, 40 iterations, the `handled` set, `aNewP = aP + force` (the largest single force, not the total, as in KiCad).
- `attemptWalk` loop, including the `continue` path, `iter == 3` failure and the `SetShape( .., !currentIsP )` swap. `tryWalkDp`: 4 attempts collapse to 2, which is equivalent since `wind_cw` is unused. `rhMarkObstacles`, `rhShoveOnly` (pads-only walk, two heads, the shoved shape written back).
- `FixRoute`: the `m_initialDiagonal` computed from `CSegment( -2 )` before the last segment is dropped, the `fixAllSegments` / `snapOnTarget` / `EndsWithVias` / `aForceFinish` conditions, `EndingPrimitives`.
- `DP_PRIMITIVE_PAIR::CursorOrientation` (segment branch with `Anchor(1)`, `ApproxParallel`, perpendicular fallback with sign by dot product).
- `DP_GATEWAYS`: `BuildForCursor`, `FilterByOrientation` (erases matches), `checkDiagonalAlignment`, `makeGapVector`, `buildEntries`, `buildDpContinuation` (priorities 100/20/5, sin 22.5 / 23.5), `BuildGeneric` (including KiCad's `st_p[i].Collinear( st_p[j] )` quirk, kept), `BuildFromPrimitivePair` for circle, rect, segment (oval) and other shapes via the bounding box, `FitGateways` (`score >= bestScore`, -3 for the non-preferred posture).
- `DIFF_PAIR::BuildInitial` (entry and target connection angles, `checkGap`, self-intersection and crossing on the inner traces only), `CheckConnectionAngle`, `Skew`, `TotalLength`, `CoupledLength*`, `commonParallelProjection`.
- Pair optimizer: `mergeDpSegments` (step schedule), `mergeDpStep` (budget `clenPre/10`, obtuse test, `BuildInitialTrace( s1.A, s2.B, dir1.IsDiagonal() )`, coupled vs uni branch, Simplify2), `coupledBypass` (max coupled length among clear candidates, first on tie: equivalent to KiCad's `coupledLength > bestLength && verify`), `verifyDpBypass`.
- `MEANDER_SETTINGS` defaults (0.2/1.0 mm amplitude, 0.6 mm spacing, 0.05 mm step, 80 % radius, LEFT side, unconstrained length and delay, skew 0 ± 0.1 mm), `SetTargetLength`, `SetTargetSkew`, `SetTargetLengthDelay` tolerances.
- `MEANDER_PLACER::doMove`: the `m_currentStart == aP` early return, Split, the side rule, the TOO_LONG test against `m_settings.m_targetLength.Max()` (also for skew, as KiCad), `tuneLineLength( aTarget - lineLen )`, the final status against `aTargetMin` / `aTargetMax`, both `keepEndpoints` branches. `TuningLengthResult` / `TuningDelayResult` (`if( m_lastLength )`). `CheckFit` (`w + spacing`). `AmplitudeStep`, `SpacingStep` (`max( s, width + Clearance() )`). `calculateTimeDomainTargets` (single, pair, skew; KiCad ignores `IsDiffPairCoupled`, so dropping it matches).
- `MEANDER_SKEW_PLACER`: target `m_coupledLength + targetSkew`, `TuningLengthResult = last - coupled`, the start messages for a non-track start and for no complementary net.
- `DP_MEANDER_PLACER::Move`: the early bails (#22041, no coupled segments), offset sign from `pairOrientation`, `baselineSegment`, `std::max` of the P and N lengths, `updateStatus` against `m_settings.m_targetLength`, `CheckFit` (`w + w*3`). `AssembleDiffPair` for segments (width, parallelity 5 nm, `commonParallelProjection`, `distTarget` tie-break, the linked-items fallback, polarity swap, gap formula).
- Tuning status strings `too_long` / `too_short` / `tuned` (file) and "too long" / "too short" / "tuned" (UI), the "%s (%s)" tuning info, the labels "current length" / "current delay" / "current skew", and "Select a track to tune first.".

## Summary

Counts (duplicates counted once): **high 7, medium 21, low 19** (47).

**Status 2026-10-24 (`task-fix-e-router`):** high 7/7 fixed; medium 18 fixed, 2 by the nanometre merge or deferred-with-reason (F1-5 fixed by nm; F2-11 deferred: KiCad's grab radius is the grid size), 1 deferred (F4-2, arcs in the pair code); low 11 fixed or partly, 1 left by decision (F3-10), the rest open. Each finding carries its status under its heading. The test infrastructure: `qa_regressions.rs` reads `$KICAD_QA_DATA` or `$KICAD_SRC_DIR/qa/data`, fails under `EDA_SLOW_TESTS=1` without the corpus, replays `walk-with-teardrops` too, and asserts KiCad's exact compare on all seven 8303b2ad sessions, which pass.

| ID | Finding | Severity | Unit-sensitive |
|---|---|---|---|
| P-1 | Walk-around drag accepts a 3.5x detour round the board outline; KiCad's recorded drag stays put (`walk-with-teardrops`, 3/19 reproduced) | high | no |
| F2-1 | A via that ends the route (or is armed at finish) is dropped from the commit | high | no |
| F2-2 | Walk around + via: via pushed out, head and commit stay at the cursor (DRC violation committed) | high | no |
| F2-3 | Esc, or a refused finish, discards the fixed segments (KiCad commits them) | high | no |
| F1-1 | A head ending in a via shoves a track on another layer round the head's own segments | high | no |
| F3-1 | Via drag in Highlight collisions / forced mark: tracks at any angle, not `dragCorner45` | high | no |
| F4-1 | Pair gap raised to the P-N clearance, via gap from the class's `diff_pair_via_gap` (issue23820: 500 µm vs KiCad's 147 µm) | high | no |
| F2-4 | A stuck walk shows the colliding raw line instead of keeping the last good head | medium | no |
| F2-5 | Shove mode falls back to a full walkaround when the solids-only walk fails | medium | no |
| F2-6 | `simplifyNewLine` not ported (no merge with the collinear track continued) | medium | no |
| F2-7 | Remove loops looks only at the route's own end points | medium | no |
| F2-8 | No `isStartingPointRoutable` for a single track ("The routing start point violates DRC.", the other messages), no start in empty space (= F3-4, P-2) | medium | no |
| F2-9 | AttemptFinish finishes at the cursor instead of `ROUTER::Finish` | medium | no |
| F2-10 | Smart pads skip rounded-rect pads (D14) | medium | no |
| F2-11 | 500 µm end and start snap radius to same-net anchors | medium | yes |
| F1-2 | `pushOrShoveVia` ignores the locked joint: the route's start via can be shoved | medium | no |
| F1-3 | Via push-out clamped to d/4 per step (KiCad's `Resize` result is discarded) | medium | no |
| F1-4 | `Line::simplify` is not `Simplify2`; `SelfIntersecting` misses a fold-back (also F2-15) | medium | partly |
| F1-5 | Clearance epsilon 1 µm vs KiCad's DRC epsilon 0.5 µm (D9) | medium | yes |
| F1-6 | +1 µm `HULL_ROUNDING_GUARD` on every shove hull, accumulating down a stack (replay: 401 vs 400 µm pitch); arc padding does not accumulate | medium | yes |
| F3-2 | A refused drag click ends the drag (KiCad keeps dragging) | medium | no |
| F3-3 | `D` on a locked item from Select asks Drag Anyway (KiCad: info-bar warning, no drag) | medium | no |
| F3-5 | Router settings not saved (KiCad `tools.pns` in pcbnew.json) | medium | no |
| F3-6 | DP Dimensions dialog switches on custom sizes for later pairs | medium | no |
| F3-7 | Pair fallback 125/180 µm vs Default class 200/250/250 µm | medium | touched |
| F4-2 | Arc tracks not handled by the pair router, pair tuner, skew tuner | medium | no |
| F4-3 | Skew tuner status box min/max are the partner's length, not the skew range | medium | no |
| F4-4 | "Unable to tune segments inside other tuning patterns." missing | medium (high if members dangle) | no |
| F2-12 | Start posture continues the start segment; no `start_diagonal` | low | no |
| F2-13 | `manually_forced` not reset on a fix (smart pads stay off) | low | no |
| F2-14 | `merge_obtuse` checks the whole line | low | no |
| F2-15 | line_walk tolerances, dead `optimize()`, FANOUT_CLEANUP, mark-obstacles snap, cluster timeout (still open) | low | partly |
| F1-7 | `SegmentHull` kink handling (D13) | low | partly |
| F1-8 | One joint per (position, net) whatever the layer | low | no |
| F1-9 | `onCollidingSolid` cluster/walk exclusions | low | no |
| F1-10 | Helper tolerances (D17) | low | yes |
| F3-8 | DP dialog validation text, via gap kept | low | no |
| F3-9 | Titles and messages ("Interactive Router Settings", "Ctrl+Click", "179.0°", invented messages) | low | no |
| F3-10 | Arc refusal unsigned vs KiCad's signed angle | low | no |
| F3-11 | Free-angle grab ignores `IsPtOnArc( idx+1 )` | low | no |
| F3-12 | Drag mode read live (KiCad latches it at Start) | low | no |
| F3-13 | `dragViaWalkaround` lead vector | low | no |
| F4-5 | Pair width inheritance only from two segment ends | low | no |
| F4-6 | Pair start messages | low | no |
| F4-7 | Stale `tryWalkDp` doc claims | low | no |
| F4-8 | nm tolerances restated in µm | low | yes |
| F4-9 | Pair end item picked by anchor distance, not shape | low | no |

Replay scratch (to re-run): `/tmp/findings-p5-router.bIxL/replay` (`CARGO_TARGET_DIR=/tmp/findings-p5-router.bIxL/target cargo build --offline --release`, then `p5replay <case dir> <board>`); outputs `replay-*.txt` beside it.
