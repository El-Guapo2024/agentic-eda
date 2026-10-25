# DS lane log (`ds/handoff-1025`)

A DeepSeek V4.1 Flash lane continuing `docs/HANDOFF.md` while the Claude lane is out of usage; a Claude (Opus) lane reviews before merge.
One handoff item at a time, a failing test first, committed only on this branch, never pushed or merged.

## Item 4, leftover 1 -- springback in the dragger and the pair placer (done 2026-10-25)

**What I did.**

- Dragger (`crates/pns/src/dragger.rs`). A `DragShove` beside the dragger carries the gesture's `Springback` and, per frame, the place a
  via drag left its via in (`m_draggedVias`; a `World` is a diff and cannot carry it). Each move reduces the frames the new head no longer
  presses on, shoves the stretched line (or the via, at the top frame's place) from the reduced frame's world with that frame's area
  (`shove_dragged_line` and `shove_via` now take `prior_area`), pushes the world the shove left, and truncates the via places with the
  popped frames. A via the shove cannot move goes to `drag_via_walkaround`, in the frame. The dragged via may be virtual or unnamed, so
  `place_via` puts it back at the frame's place after `frame.apply(world)`. `preview()` reduces a copy of the stack; `drag` uses the live
  one. The commit at the last valid point is built from the preview, whose displacement is the newest frame's against the session's
  world, so it carries everything the gesture pushed, not only the last move's.
- Pair placer (`crates/pns/src/dp_placer.rs`). `frame_node()` is `m_shove->CurrentNode()`: the newest frame's world over the session's
  with the fixed legs in it. `rhShoveOnly` fits, walks and shoves in that frame, reduces both lines' heads together (`Springback::reduce_all`,
  new: `CheckColliding( aHeadSet )` asks about the one set) and shoves with the frame's area through `shove_heads_after`; the world the
  shove leaves is pushed as the newest frame. `fix_head` restarts the stack (`Springback::new()`), as `FixRoute` makes a fresh `SHOVE`
  over `m_world`; no frame is ever locked. `undo_last_segment` (the port's Backspace; KiCad's pair placer has no `UnfixRoute`) clears the
  stack with the leg. `last_node` follows `m_lastNode`: the frame at the start of the move, the shove's world on success, the reduced
  frame on failure (`SHOVE::Run`'s `m_currentNode = parent`).
- Docs: `crates/pns/PARITY.md` (new Stage 14; the places that said "no springback"), `docs/parity/GAPS.md` (item 7's table and section,
  P5, D18, the summary table), `docs/HANDOFF.md` (the router-leftovers row and item 4).

**Commits.**

- `a050b233` -- pns dragger: springback for the whole gesture (one SHOVE, a frame per move that shoved, the dragged via's place per
  frame, the commit built from the newest frame; shove_dragged_line and shove_via take the frame's area)
- `38c11f42` -- pns diff pair: springback (the move starts from the newest frame, both heads reduced together, a fresh SHOVE at every
  fix); PARITY Stage 14, GAPS and HANDOFF updated
- this log, in its own commit.

**Tests run.**

- Failing first, then passing. Both new placer tests were run against the pre-change code and failed there: the dragger's
  `a_pushed_via_stays_pushed_while_the_drag_still_presses_on_the_frame` (the via sprang back as soon as the cursor left it) and the
  pair's `a_via_the_pair_pushed_stays_pushed_until_the_leg_is_fixed` (re-confirmed on the old pair code: the second move's preview had
  no displaced via, `assert_eq!(pv.displaced_vias.len(), 1)` failing `left: 0, right: 1`). `springback.rs`'s
  `a_frame_stays_while_any_of_the_heads_presses_on_it` covers the two-head reduce.
- `cargo test -p eda-pns --lib`: 301 passed, 0 failed.
- `cargo test -p eda-pns` (fast tier): every suite green, `qa_regressions` and `dp_boards` included.
- `EDA_SLOW_TESTS=1 KICAD_QA_DATA=$HOME/src/kicad-src-8303b2ad/qa/data cargo test -p eda-pns --tests`: exit 0 -- 388 passed, 3
  ignored, 0 failed in all (unit 301; `arc_boards` 4, `arc_tracks` 22, `board_edge` 6, `direction45_oracle` 2, `dp_boards` 5,
  `head_via` 5, `locked_tracks` 7, `meander_oracle` 7, `mouse_trail_oracle` 2, `qa_regressions` 13, `real_board` 1, `shove_footprint` 2,
  `shove_scenarios` 11).
- `cargo check --workspace --tests --keep-going`: exit 0.
- `cargo clippy -p eda-pns --tests --no-deps`: red on pre-existing code -- two deny-by-default `clippy::approx_constant` errors at
  `crates/pns/src/arcs.rs:375` (test code, not touched by this item) and 35 warnings across the crate. None of the errors and none of
  the warnings falls on a line this item added or changed (checked against the diff's hunks; the only `dp_placer.rs` warnings are at
  lines 334 and 357, in `new()`, outside them). Workspace-wide clippy is red on the same lint in `crates/model/src/units.rs`.

**Unsure / for the reviewer.**

- The handoff names the pair-placer half "`DIFF_PAIR_PLACER::tryShove`", but KiCad 8.0's pair placer keeps the stack in
  `rhShoveOnly`/`Move`/`FixRoute` (`tryShove` does not appear in `pns_diff_pair_placer.cpp`). I ported the code that exists.
- Dragger failure path: a shove that cannot resolve keeps the pops (`m_currentNode = parent`) and the dragger falls back to marking
  obstacles; the preview then shows the reduced frame's displacement rather than the last successful solution's. I believe that is what
  KiCad shows (the shove's current node), but I did not run KiCad's dragger side by side to compare.
- A `World` is a diff against the session world and carries moved named vias along with the IR tracks. The via the dragger itself drags
  is handled by `place_via` even when it is virtual or unnamed; unnamed vias the *shove* pushes incidentally are not in the frame or the
  commit, which is how the rest of the crate already treats them (no source id to commit by).
- `frame_node()` clones the session world on every Shove-mode pair move (an `apply`); KiCad branches a node instead. Correct, but a
  per-move allocation whose cost I did not measure.
- The pair placer's failure path sets `last_node` to the reduced frame; that only matters for a fix under `AllowDRCViolations`.
- I did not run `tools/check.sh fast` workspace-wide or `tools/run-all-checks.sh` (web and e2e) for this item: the change is confined to
  `eda-pns`, its whole fast and slow tiers are green, and the workspace compiles with tests. Say if the lane gate needs those too.

## Item 4, leftover 2 -- a shared screen inside a shared screen (stopped: needs a decision and touches four crates)

**Not done; the question is below.** The handoff says it "needs references keyed by the whole path". I mapped what that key change touches and
stopped there, under the lane's rule for a design decision that spans many crates at once.

**What I found.**

- The model keys a screen's per-placement references by the *immediate* parent: `SchematicSection::instance_overrides` (`SymbolPathOverride`,
  `crates/model/src/ir.rs:281-289` and `:901-924`, key `parent_sheet_instance_id`), looked up by
  `eda_model::sheet_list::reference_under(screen, sym, placement: Option<&str>)` (`crates/model/src/sheet_list.rs:188-196`) and pruned by
  `prune_orphaned_overrides` (`:210-227`).
- Why that is wrong once a shared screen is nested: a screen's content is stored once (`Design::sheet_contents`), so the `SheetInstance` ids of
  a screen placed twice are the *same* ids under both placements. In KiCad's `qa/data/eeschema/issue10926_1`, the inner screen's symbols carry
  the paths `/30037e9f.../745d7512.../0203a5e0...` (U4) and `/30037e9f.../48e5c29d.../0203a5e0...` (U1) -- the same last component
  `0203a5e0...`, two references -- so a key on the last component can hold only one of them for both. `netlists/hierarchy_aliases` is the
  same shape. That is exactly the two projects the slow QA test still prints as differing (`QA_TRACED_OTHERWISE`,
  `crates/cli/src/sch_hierarchy_tests.rs:1291-1293`).
- The writer already builds full paths (`ScreenCtx`'s parallel `instance_paths`/`instance_parents`, `crates/kicad/src/lib.rs:138-151`;
  `export_kicad_sch_tree`'s walk makes `/<root-uuid>/<sheet-uuid>...`) but hands `reference_under` only the last id (`lib.rs:433-434`).
  `write_sheet_item` also writes the `(instances (path ... (page ...)))` of a shared screen's own sheets under only
  `instance_paths.first()` (`lib.rs:676`, `:754-757`), so a screen placed twice that has sheets of its own is under-described there too.
- The importer keeps only the last component of each instance path (`sch_import.rs:1052-1077`).
- Callers to change with the key: `crates/ops/src/{sch_sheets,sch_clipboard,multichannel_facts}.rs`, `crates/kicad/src/lib.rs` (two
  lookups), `crates/cli/src/{board,studio}.rs`, and tests in `crates/cli/src/sch_hierarchy_tests.rs` that compare keys directly. Four
  crates (model, ops, kicad, cli) and a change to what `design.json` means, not just to code.

**The question.**

1. *Key format and old designs.* Store the root-relative chain of `SheetInstance::id`s (`"745d7512.../0203a5e0..."`) as the key. Keep the
   field name `parent_sheet_instance_id` (a bare value is the depth-1 path it always was, so old `design.json`s read exactly as today --
   nested sharing included -- until saved again), or add a new field (`sheet_path: Vec<String>` or `path: String`, `#[serde(default)]`) and
   migrate on read? The struct is `deny_unknown_fields`, so a new field reads old files; the old one would then be retired or kept in sync.
2. *The root component.* KiCad's instance paths start with the root screen's uuid, which the model does not keep as a sheet id
   (`SheetEntry::path` starts at the first child). The import would drop the first component of every parsed path and join the rest; the
   writer would pass the model-relative chain to `reference_under` and go on writing the full KiCad path. Is "drop the first component" the
   convention to adopt, or should the root screen's uuid become part of the model?
3. *The shared screen's own page paths.* May `write_sheet_item` write one `(instances (path ... (page ...)))` per placement of the screen it
   draws (KiCad does), in the same change?
4. *Paste.* GAPS pairs this with "a screen that has sheets of its own is still pasted as a copy" (KiCad shares that screen too:
   `SCH_EDITOR_CONTROL::Paste`'s `SearchHierarchy`/`loadedScreens`); the same path keying is what it needs. Should Paste share nested
   screens in this item, or stay a copy for now?
5. *Acceptance.* Move `issue10926_1` and `netlists/hierarchy_aliases` from `QA_TRACED_OTHERWISE` to `QA_TRACED_ALIKE` in
   `slow_kicad_qa_hierarchies_trace_as_kicad_cli_reads_them`, plus a unit test that a screen shown twice inside a screen shown twice gets one
   reference per full path (the fixture is the `issue10926_1` shape above).
