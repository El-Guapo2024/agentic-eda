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

## Item 4, leftover 3 -- the Symbol Editor's pin and New Symbol dialogs (done 2026-10-25)

**What I did.**

- The Pin tool (`SYMBOL_EDITOR_PIN_TOOL::CreatePin`). A click no longer places a pin at once: `SymbolEditorCanvas.tsx` builds the pin the tool
  has dropped (sticky template + active unit/body style + the next free number) and puts it in the store as `pendingPin`; `PinPropertiesDialog`
  opens on it. Cancel drops it and places nothing (the tool stays armed); OK asks `PlacePin`'s Synchronized Pins Mode question when the spot is
  taken ("This position is already occupied by another pin, in unit %d." + the extended line, OK "Place Pin Anyway"; a refusal drops the pin),
  adds the pin (image pins in every other unit as before), and only then saves the template (`g_LastPin*`) and the number (`g_lastPin`). An
  existing pin still opens the same dialog by id and is edited as before; "Delete Pin" is not offered on the pending one.
- The sticky template moved from a ref in the canvas into the store (`state.pinTemplate`), because the dialog's OK is now what saves it. It
  survives opening another symbol (`g_LastPin*` is static in KiCad) and `SET_PIN_TEMPLATE` returns the same state when the template is unchanged,
  so the ~700 ms poll does not re-render the editor.
- New Symbol (Ctrl+N, `SYMBOL_EDIT_FRAME::CreateNewSymbol` with no parent). A new `NewSymbolDialog.tsx` asks what `DIALOG_LIB_NEW_SYMBOL` asks and
  this model can hold: the name (`GetName`: trimmed, spaces to underscores, LIB_ID specials escaped; the validator's "Symbol must have a name.",
  and a name the project library has is refused in the dialog), the default reference (`U`), units per package (1..64) with "All units are
  interchangeable" (the opposite of `units_locked`, set only for several units), "Create symbol with alternate body style (De Morgan)", "Create
  symbol as power symbol" (checking it forces and locks "Exclude from bill of materials"/"Exclude from board", as `onPowerCheckBox` does; unchecking
  re-enables them and leaves them true), the pin name position offset (default 20 mil = 0.508 mm, shown in the user's unit) and "Show pin number
  text"/"Show pin name text". OK is one `batch` (`new_symbol` + `edit_symbol_properties`), one undo step; the symbol opens in the editor.
- The dialog starts on `uniqueSymbolLibId`'s free name ("Untitled", "Untitled_1", ...) until the person types one, so Ctrl+N + OK still makes a
  symbol the way it used to; the names are fetched each opening and again on OK (as the Derive dialog does). Ctrl+N now dispatches
  `SET_NEW_SYMBOL_OPEN` (`useActionRunner.ts`), still on the Symbol tab only.
- Pure modules with their unit tests (written first, failing): `kicad-port/symPinTemplate.ts` (the sticky defaults, `newPinFromClick`,
  `pinTemplateOf`, `samePinTemplate`), `kicad-port/pinPlacement.ts` (`pinPlacementOccupant`, `pinOccupiedMessage`), and
  `symEditActions.ts::newSymbolName`/`newSymbolNameCheck` (the name's form, the validator, the whole-lib_id membership test).
- `SymbolEditorApi.newSymbol(request)` now takes the dialog's answers and returns whether the verb accepted them; `addPin` returns the same (the
  dialog has to know before it closes and saves the template). Existing callers ignore the value.
- Docs: `web/studio/PARITY-symedit.md` (the two hotkey rows), `web/studio/PARITY-common.md` (the P20 finds table, and the "found, not fixed"
  line loses the sentence), `docs/parity/GAPS.md` (the P20 paragraph's entry moved to a fixed one), `docs/HANDOFF.md` (item 4).
- Browser checks: `lib-edit.check.js` (the New Symbol and pin-tool scenarios rewritten around the dialogs, plus a duplicate/Cancel scenario),
  `lib-editors.check.js` (its four `clickAt` pin placements now OK the dialog) and `files-in.check.js` (Ctrl+N is followed by the dialog).

**Commits.**

- `4a3fc95c` -- studio: the Symbol Editor's pin tool and New Symbol ask first, as KiCad does
- this log, in its own commit.

**Tests run.**

- Failing first: `npm --prefix web/studio run test:unit` on the old code gave EXIT 2 -- `TS2307: Cannot find module './pinPlacement'` and
  `'./symPinTemplate'`, `TS2305: Module '"./symEditActions"' has no exported member 'newSymbolNameCheck'` -- and
  `tools/run-all-checks.sh -j 1 lib-edit` gave EXIT 1 with seven failing scenarios (the three new ones -- New Symbol's dialog, the duplicate
  refusal, the pin tool's dialog -- and four cascades, since the symbol they depend on was never made).
- After: `npm --prefix web/studio run typecheck` EXIT 0; `npm --prefix web/studio run test:unit` 2477 tests, 2477 pass, 0 fail, EXIT 0;
  `tools/run-all-checks.sh -j 1 lib-edit` ok, 13 scenarios, 58s; `tools/run-all-checks.sh -j 1 lib-editors files-in` ok, 26 and 103
  scenarios, 203s and 115s, EXIT 0.
- No Rust changed: the two verbs (`new_symbol`, `edit_symbol_properties`) and `batch` were already there, so no cargo run this item.

**Unsure / for the reviewer.**

- A taken name is refused with the verb's words ("a symbol with this name already exists; pick another name") instead of KiCad's
  "Symbol '%s' already exists in library '%s'." + Overwrite. `new_symbol` cannot overwrite, and delete + create would be destructive; the
  deviation is written into `PARITY-symedit.md`. If a real Overwrite is wanted, it is a new verb in `crates/ops` (out of this item's scope).
- Two controls are not asked: "Place pin names inside" (no field in `SymbolPropertiesFields`/`LibrarySymbol`; adding it is a model + kicad writer
  change) and the parent/derive combo (`derive_symbol` is the Derive dialog's verb, and that dialog asks it).
- The template also follows a pin merely selected in the editor (the selection effect saves `pinTemplateOf(pin)`), which KiCad's `g_LastPin*` does
  not do -- only `CreatePin` writes it there. That behaviour predates this item (it was the canvas's `pinTemplateRef`); keeping it meant the
  template now includes the fields the dialog can change, not just the five the old ref kept. Say if it should follow only the Pin tool's dialog.
- `pinPlacementOccupant` tests the dialog's edited copy (so a number/position changed in the dialog is what the question uses) against the
  symbol's pins with `synchronizePins(state.syncPins, unit_count, units_locked)`. KiCad's `PlacePin` checks `m_frame->GetScreen()`'s pins;
  I believe the two agree, but the unit the message prints is the occupant's `unit`, which for a unit-0 pin is 0 ("in unit 0"), the same as KiCad.
- The new symbol's properties go through `edit_symbol_properties` with empty description/keywords/datasheet and no footprint filters, as a fresh
  `LibrarySymbol` has them; if `new_symbol` ever writes something else there, the batch would overwrite it.

## Item 4, leftover 4 -- Both sides placement files (done 2026-10-26)

**What I did.**

- The naming, taken from KiCad 8's source first: `PLACE_FILE_EXPORTER::DecorateFilename` suffixes the base name `-all` when both sides go in
  one file, `-top` for the front and `-bottom` for the back; the ASCII file is `<base>.pos` (`CreateAsciiFiles`, `FILEEXT::FootprintPlaceFileExtension`)
  and the CSV `<base>-pos.csv` (`CreateCSVFiles` puts `-pos` before the extension), so e.g. `board-all-pos.csv`, `board-top.pos`.
  `DIALOG_GEN_FOOTPRINT_POSITION`'s "Generate single file with both front and back positions" (`m_singleFile`) starts checked; with it off and
  "Both sides" asked, the dialog writes the top's and the bottom's files. `kicad-cli pcb export pos` writes exactly the name its `-o` has (the
  newer API calls it `m_nakedFilename`), so the two-file mode has to be two runs, each with its own `--side front` / `--side back`.
- `crates/kicad-engine/src/lib.rs`: `place_file_names(stem, side, format, single_file)` holds those rules (one side always wins over
  `single_file`); `run_export_command` extracted from `run_export` so both paths share the invocation; `export_pcb` sends text `pos` exports
  (`--format` absent/`ascii`/`csv`) to the new `export_pcb_placement(..., single_file)`, which returns `{ ok, engine, files }` like
  `export_pcb`. `export_pcb_placement` deletes each target before its run (an old file must not pass as this run's) and lists the targets it
  named rather than the files modified since a second before the run -- the mtime window `run_export` uses cannot tell the two sides apart on a
  coarse file system. Gerber's raw route is untouched.
- `crates/cli/src/kicad_engine.rs::export_pos(dir, args, single_file)`; `fab_api::pos` takes `"single_file"` (default true, the dialog's
  default, so older callers keep one file); `eda fab pos --separate-sides` asks for the two files.
- `web/studio`: `FabPosOptions.single_file?`; the dialog's checkbox "Generate single file with both front and back positions" (checked; disabled
  unless "Both sides" is chosen, since the choice does not apply otherwise) and the footer's note names the files; the header comment cites the
  dialog and `DecorateFilename`.
- `plot-exports.check.js`: the placement scenario asks for CSV, both sides, default checked -> one `-all-pos.csv` and `single_file === true` in
  the request; unchecks the box -> `-top-pos.csv` and `-bottom-pos.csv` and `single_file === false`; re-checks it; then ASCII -> one
  `-all.pos`.
- Docs: `docs/HANDOFF.md` (item 4), `docs/parity/GAPS.md` (the "Both sides" find becomes a fixed one, 2026-10-26),
  `web/studio/PARITY-common.md` (P20's finds table and the "found, not fixed" list).

**Commits.**

- `4611df84` -- placement files: KiCad's -all / -top / -bottom names, the single-file box, and two runs for two sides
- this log, in its own commit.

**Tests run.**

- Failing first: the naming test was written before the function and `cargo test -p eda-kicad-engine --lib placement_files` gave EXIT 101,
  `error[E0425]: cannot find function 'place_file_names' in this scope`. The fake-kicad-cli test (one run per file, each run's `--side`) was
  added once `place_file_names` and `export_pcb_placement` existed, so it is a regression check rather than red-first evidence.
- After: `cargo test -p eda-kicad-engine --lib` 41 passed, 0 failed; the placement filter (`cargo test -p eda-kicad-engine --lib
  placement`) 4 passed.
- `cargo test -p eda-cli fab` 8 passed, 0 failed. (`-p eda-cli --lib` does not exist: no library target.)
- `cargo check -p eda-cli --tests` finished clean; `cargo clippy -p eda-kicad-engine -p eda-cli --tests --no-deps` exit 0 with only the
  pre-existing warnings (21 in eda-cli, none at a line this item added).
- The two slow tests that use the real kicad-cli, run individually with `EDA_SLOW_TESTS=1` on the eda-cli unittests:
  `board::tests::kicad_cli_loads_the_edited_board_and_its_position_and_bom_exports_honour_the_attributes` ok and
  `board::edit_details_tests::kicad_cli_reads_the_references_a_paste_gave_and_the_board_after_paste_special_loads_for_its_drc` ok.
- `npm --prefix web/studio run typecheck` EXIT 0.
- `tools/run-all-checks.sh -j 1 plot-exports`: `ok plot-exports 11 scenarios 36s board=opamp-routed`, EXIT 0.
- By hand with the real kicad-cli on `/tmp/poscheck` (`eda board new examples/opamp_filter.yaml -o /tmp/poscheck`): `eda fab pos` wrote
  `export/kicad/pos/opamp_filter-all-pos.csv`; `--separate-sides` wrote `opamp_filter-top-pos.csv` and `opamp_filter-bottom-pos.csv`;
  `--format ascii` `opamp_filter-all.pos`; `--side front` with ASCII `opamp_filter-top.pos`.
- `cargo fmt --all -- --check` is red workspace-wide on pre-existing style; `cargo fmt -p eda-kicad-engine -p eda-cli -- --check` finds 282
  hunks in the six files it lists, 173 of them in `kicad-engine/src/lib.rs` (2,674 lines of long lines) and the rest in
  `footprint_check.rs`, `foreign.rs` and the three cli files; the new lines follow the surrounding long-line style, as the earlier entries'
  code does.

**Unsure / for the reviewer.**

- `export_pcb` now routes text `pos` exports to the new function with `single_file = true`; a caller of the generic route that used to get
  `<stem>.pos` now gets `<stem>-all.pos`/`<stem>-all-pos.csv`, which is KiCad's own default. I found no hard-coded `.pos` path elsewhere
  (grep), and the remaining tests read `reply["files"][0]` only; `eda fab`'s package path (`crates/cli/src/main.rs:1031`) runs kicad-cli itself
  with its own `-o` name and is untouched. Say if the generic route should keep the plain name instead.
- Two files cost two kicad-cli runs (each run is seconds on a real board) and leave any older `-all` file from a previous run beside them in
  `export/kicad/pos/` -- only the current run's targets are removed and listed, the same as the other export kinds. Is deleting the other
  naming variant on each run wanted?
- The checkbox is disabled unless "Both sides" is chosen. The server ignores `single_file` for one side anyway; I did not find an
  enable/disable in the dialog's source I read, so if KiCad leaves it enabled this is a small deviation.
- `--separate-sides` is the CLI flag name I chose (kicad-cli has no counterpart; the box lives in the dialog only). Rename it if the lane has a
  convention.
- The ASCII names (`-all.pos` etc.) are read off `CreateAsciiFiles` + `DecorateFilename`; the CSV `-pos` infix off `CreateCSVFiles`. I
  confirmed end-to-end only that kicad-cli writes whatever `-o` names, not KiCad's GUI output side by side.
- I did not run the whole workspace test suite or the other browser checks; the kicad-engine suite, the eda-cli fab filter, the two slow
  kicad-cli tests, the typecheck and `plot-exports` are green. Say if the lane gate wants `tools/check.sh fast` and the rest of
  `tools/run-all-checks.sh` too.

## Item 4, leftover 5 -- the checks P20 left uncovered: Page Settings and the drill/place origin, Board Statistics and Swap Layers, and the board view (done 2026-10-26)

**What I did.**

- Registered nothing new in `tools/run-all-checks.sh`: each check file's header already names a board kind `make_board` builds -- `board=mcu30`
  for `board-page-origin` (a fresh `eda board new`), `board=mcu30-zones` for `board-stats` (routed with pours, so both copper sides have
  something to swap) and `board=mcu30-routed` for `board-view` -- so the three names run as the runner reads them.
- Ran `bash tools/run-all-checks.sh -j 1 --ports 8830-8832 board-page-origin board-stats board-view`. The first run had two failing checks,
  both on assertions that were wrong about the studio rather than defects in it (below). Fixed the checks only; no studio or runner code was
  touched, and no assertion was dropped or weakened.

**The first run and what each failure was.**

- `board-page-origin`, scenario "Page Settings on the board: paper and title block in one undo step, Cancel and an unchanged OK sending
  nothing": `FAIL: the change is one set_board_page command (page --paper A3 --title "P9A page")`. The check tested `/set_board_page/` against
  `activity[0].cmd`, but the activity log holds no op name: `Activity` is `{ t, by, cmd, ok, message }` (`web/studio/src/api/types.ts`) and
  `cmd` is the CLI line `cmd_line` builds (`crates/cli/src/board.rs`, `Cmd::SetBoardPage`) -- `page --paper A3 --title "P9A page"`. The
  "one command, not a batch of two settings" assertion stands, now as the exact CLI line, `ok === true`, and exactly one new activity entry
  (`before.activity.length + 1`) -- stronger than the regex it replaced, not looser.
- `board-view`, scenario "N and Shift+N cycle the grid, Ctrl+U switches units, and the status bar's X/Y and dx/dy follow the pointer and
  Space", three notes:
  - `FAIL: the status bar shows the pointer at 15.0850, 16.3097 mm (Z 0.79 15.085, 16.310 mm ...)` -- `formatXY` prints the unit's own
    decimals (`DECIMALS` in `web/studio/src/state/units.ts`: 3 for mm, 1 for mil, 4 for in) and the check hard-coded 4. The readout is right;
    the expected string was wrong (now the unit's decimals, with `u0` still the unit used).
  - `FAIL: dx/dy are measured from the Space point (undefined; expected 1.9194, 0.9597 mm)` and `FAIL: and dist is their length (undefined
    undefined; expected 2.1460 mm)` -- the check's `status()` helper collapses every run of whitespace to one space (`replace(/\s+/g, " ")`)
    and the regex still asked for two spaces between the fields (`dx ... {2}dy ...`). The status bar's `dx <len> <unit>  dy ...  dist ...`
    layout (`StatusBar.tsx`) is right; the regex was wrong.
  - The rest of that scenario (N/Shift+N, Ctrl+U, Space -> `common.Control.resetLocalCoords` -> `SET_LOCAL_ORIGIN` at `state.cursorNm`) was
    green, so the Space behaviour itself was never in question.
- No studio defect was found: there is no finding to record against the studio beyond these two check fixes.

**Commits.**

- `a88caf31` -- browser checks: Page Settings and the drill/place origin, Board Statistics and Swap Layers, and the board view's zoom, pan,
  grid, units and context menu
- this log, in its own commit.

**Tests run.**

- `bash tools/run-all-checks.sh -j 1 --ports 8830-8832 board-page-origin board-stats board-view`, first run:
  `run-all-checks: 1 passed, 2 failed of 3 in 111s (logs: /tmp/run-all-checks.b6gj/runs)`.
- The same command after the fixes:

  ```
  run-all-checks: 3 passed, 0 failed of 3 in 89s (logs: /tmp/run-all-checks.XIny/runs)
  ```

- Its line items: `ok board-page-origin 4 scenarios 11s board=mcu30`, `ok board-stats 5 scenarios 66s board=mcu30-zones`,
  `ok board-view 6 scenarios 12s board=mcu30-routed`.

**Unsure / for the reviewer.**

- The activity log exposes no op name, so a check can only assert the CLI line a command is typed as, or that no new entry appeared. If
  later checks want the op, `Activity` and `activity.jsonl` would need it; the server was not changed here.
- `board-stats` runs its Swap Layers scenarios with the workspace's strict toggle off (as `board-edit.check.js` does) and puts the toggle
  back: swapping F.Cu with B.Cu moves the tracks and zones but not the SMD pads, so strict (the default) refuses the swap for the
  connectivity it breaks. Whether the dialog should behave differently under strict is a separate question about the studio, not about this
  check.
- `board-stats` needs kicad-cli for `POST /api/board_stats` and waits up to 90 s for the dialog to fill; the header's `minutes=10` covers the
  slow runs seen so far (66 s wall for the check), but a machine without kicad-cli will fail on the setup assertion, as intended.

## Symbol Editor review corrections F1-F4 on `4a3fc95c` (done 2026-10-26)

**What I did** (from the Claude reviewer, checked against KiCad 8303b2ad).

- **F1**: New Symbol's "All units are interchangeable" starts **unchecked** (`dialog_lib_new_symbol_base.cpp:78` never calls `SetValue`), so
  `CreateSymbol`'s `LockUnits( !aProps.unitsInterchangeable )` locks a new multi-unit symbol made with default answers. The store already turned
  that into `units_locked: unitCount > 1 && !unitsInterchangeable`, so no model change was needed.
- **F2**: added KiCad's "Pin name inside" checkbox, **checked** (`dialog_lib_new_symbol_base.cpp:147-148`). OK folds it into `pinNameOffsetMm` the
  way `SymbolPropertiesDialog.tsx`'s "Place pin names inside" already does: inside keeps the offset, an inside offset of 0 becomes 20 mils
  (0.508 mm), outside writes 0 (`CreateSymbol`, `lib_symbol_library_manager.cpp:55`). The offset field is disabled while the box is off. No model or
  writer change.
- **F3**: `PinTemplate` no longer carries `name` (`g_LastPin*`, `symbol_editor_pin_tool.cpp:38-48` and `~197-205`, holds none);
  `newPinFromClick` starts every pin with `name: ""`. Removed the canvas effect that saved `pinTemplateOf(pin)` whenever one pin was selected:
  only Pin Properties' OK updates the template now -- the pending pin's OK (as before) and an edited pin's OK (`EditPinProperties` saves the
  globals for that case too), which previously did not.
- **F4**: the taken-name refusal is KiCad's own wording, "Symbol '%s' already exists in library '%s'." (`symbolExistsMessage`, the name
  unescaped as the validator does), with no Overwrite, which `new_symbol` cannot do. The lib-edit regex now asks for those words.
- **Parity**: `web/studio/PARITY-symedit.md` -- the Pin tool and Ctrl+N rows corrected, and a new "Differs from KiCad" section: a pin is placed
  with one click here (OK places it; the occupied question is asked at OK) where KiCad's `TwoClickPlace` places on a second click; the name field
  is prefilled "Untitled"/"Untitled_N" where KiCad's starts empty; the name is trimmed.
- Tests written first: `symPinTemplate.test.ts` (no `name` in the template; a click's pin starts unnamed) and `symEditActions.test.ts`
  (`symbolExistsMessage`: words, unescaping, no Overwrite), plus two `lib-edit.check.js` scenarios -- New Symbol's defaults and the offset
  (two units + unchecked interchangeable is `units_locked: true`; inside with offset 0 gives 0.508 mm; outside gives 0) and the template (the
  next click starts from the last OK's look but with no name; selecting a pin does not reset it).

**Commits.**

- `c943dd26` -- studio: four Symbol Editor corrections from the 4a3fc95c review
- this log, in its own commit.

**Tests run.**

- Failing first: `npm --prefix web/studio run test:unit` EXIT 1 -- `TS2305: Module '"./symEditActions"' has no exported member 'symbolExistsMessage'`
  (the template tests were changed to the new rule in the same edit).
- After: `npm --prefix web/studio run test:unit` 2478 tests, 2478 pass, 0 fail, EXIT 0; `npm --prefix web/studio run typecheck` EXIT 0;
  `bash tools/run-all-checks.sh -j 1 --ports 8830-8832 lib-edit` -> `ok lib-edit 15 scenarios 70s board=mcu30`,
  `run-all-checks: 1 passed, 0 failed of 1 in 70s`, EXIT 0 (the check had 13 scenarios before; the two new ones are the two above).
- No Rust changed (no model, writer or verb), so no cargo run this round.
- `npm test` in `web/studio` has no such script (`Missing script: "test"`); the package's runner is `npm run test:unit`, which I ran.

**Unsure / for the reviewer.**

- F2's "inside with an offset of 0" becomes **20 mils** here, as `SymbolPropertiesDialog`'s "Place pin names inside" already makes it, where
  `CreateSymbol` itself calls `SetPinNameOffset( 1 )` (1 IU = 0.0001 mm). The reviewer said to copy the existing dialog, so the two dialogs agree;
  KiCad's exact 1 IU is not reproduced.
- Added the template save on an **edited** pin's OK as well. KiCad's `EditPinProperties` saves `g_LastPin*` for that path, and the reviewer's
  "only the pin dialog's OK" read allows it; before, only the pending pin's OK saved the template.
- `Pin name inside` changes only `pin_name_offset_mm` in the batch; "Show pin name text" still owns `pin_names_hidden`, as `CreateSymbol` keeps
  them separate.

## Import Sheet and the Hierarchy Navigator browser checks (done 2026-10-27)

**What I did** (the task: GAPS.md item 33, package P4's hierarchy row, "Not: Import Sheet, the Hierarchy Navigator").

- `web/studio/e2e/import-sheet.check.js` (board `mcu30-sheet`, 3 scenarios): Choose Schematic lists the project's `.kicad_sch` files and says
  what the file brings and what `LoadSheetFromFile` finds (a missing library is a warning), with the remembered Place as sheet/group and Keep
  annotations options; Open arms `sch_import_sheet`; two canvas clicks open Sheet Properties named after the file; Import brings the copy and
  its screen (one symbol numbered anew, the hierarchical labels IN and OUT, two nested sheets showing
  `issue10926_1_subsheet_1_1.kicad_sch` with pins IN_A/OUT_A), and one Undo takes the placement and both screens away, the hierarchy back to
  what it was. Every scenario undoes what it did; the end compares the design with the start.
- `web/studio/e2e/hierarchy-navigator.check.js` (board `mcu30-sheet`, 4 scenarios): the setup imports the QA file through `import_sheet` (the
  dialog is the other check's subject), then the panel lists all 7 sheets of `GET /api/sch/hierarchy` in walk order with the nested rows
  indented and the shared screen twice with pages 4 and 2; a click opens the row's sheet (LED channels), a nested row opens its own placement,
  the row is `aria-current`, the root row comes back, and `GET /api/schematic?sheet=<ids joined by />` gives the breadcrumb at each level.
- `tools/run-all-checks.sh`: board kind `mcu30-sheet` -- `eda board new examples/mcu_board_30plus.yaml` with KiCad's QA
  `issue10926_1_subsheet_1.kicad_sch` and `issue10926_1_subsheet_1_1.kicad_sch` (from `$KICAD_SRC_DIR/qa/data/eeschema`, default
  `~/src/kicad-src-8303b2ad`) under `fixtures/`. They are under `fixtures/` so the copy Import Sheet writes into the project folder does not
  overwrite the file it reads.

**Findings (the studio, not the check).** Both are in `hierarchy-navigator.check.js`; the assertion each would fail is left out with a comment
that points here.

- **hierarchy-navigator: a parent's children are not in page order.** **Fixed by `f8b4ef93`** (the panel sorts each sheet's children by page, `hierarchyTreeOrder`; the walk itself now visits a screen's sheets by place, `SCH_SCREEN::GetSheets`; `PARITY-sch.md` section 43; the assertion is back in the check, `63cd3f26`). Scenario "The hierarchy: every sheet once, ...". After Import Sheet of
  the QA file, its two nested placements hold pages 4 (`Subsheet_1_1`) and 2 (`Subsheet_1_2`) in `GET /api/sch/hierarchy`, and the panel lists
  them in walk order (4 then 2). KiCad's pane builds the same depth-first tree and then sorts each parent's children by page:
  `HIERARCHY_PANE::buildHierarchyTree` calls `m_tree->SortChildren( aParent )` (eeschema/widgets/hierarchy_pane.cpp:153) and
  `HIERARCHY_TREE::OnCompareItems` compares `SCH_SHEET_PATH::ComparePageNum` (:61-66), which compares the last sheet's page numerically
  (`SCH_SHEET::ComparePageNum`, eeschema/sch_sheet.cpp:1877; `SCH_SHEET_PATH::ComparePageNum`, eeschema/sch_sheet_path.cpp:219). Page 2 would
  be listed above page 4. The panel's order is the list's walk order (`HierarchyPanel.tsx:38-64`), not a page sort.
- **hierarchy-navigator: no page number for a sheet that was never given one.** **Fixed by `f8b4ef93`** (every sheet path shows and is written with a page: the load's numbers where none is stored, the next free page for a new sheet path, a page per path of a screen shown twice; `PARITY-sch.md` section 43; the assertion is back, `63cd3f26`. One correction to the check found on the way: KiCad does not keep the QA file's pages 4 and 2 for the imported sheet's nested sheets -- `AddNewSheetInstances` finds no instance of the new path and gives them the lowest free pages, 6 and 7 here). Same scenario. `GET /api/sch/hierarchy` returns `page: ""`
  for the three module sheets and for the imported sheet, and the panel prints the file alone (`HierarchyPanel.tsx:61`: `s.page ? ... : file`).
  KiCad's pane labels every sheet `formatPageString( name, GetPageNumber() )` ("name (page N)", eeschema/widgets/hierarchy_pane.cpp:145,
  :844-847); a schematic loaded with every page empty is numbered 1..N by `SCH_SHEET_LIST::SetInitialPageNumbers` (eeschema/files-io.cpp:456-457
  calls it when `AllSheetPageNumbersEmpty()`, eeschema/sch_sheet_path.cpp:1653), and a sheet drawn or imported gets the first free number
  (`SCH_SHEET_LIST::GetNextPageNumber`, eeschema/sch_sheet_path.cpp:1198, called by `SCH_DRAWING_TOOLS::DrawSheet`,
  eeschema/tools/sch_drawing_tools.cpp:3437). The studio has `effectivePage()` for Next/Previous (`web/studio/src/kicad-port/sheetPages.ts`);
  the panel does not use it. The imported sheet's part of this is the known "first-free page numbers" left item (GAPS.md item 4); the three
  modules' (a design `eda board new` makes has no stored pages) is the new part.

**Commits.**

- `b7f5700a` -- browser checks: Import Sheet and the Hierarchy Navigator (the checks, the board kind, GAPS.md item 33)
- this log, in its own commit.

**Tests run.**

- `bash tools/run-all-checks.sh -j 1 --ports 8830-8832 import-sheet hierarchy-navigator` (a fresh build of the studio and eda-cli):

  ```
  run-all-checks: 2 passed, 0 failed of 2 in 34s (logs: /tmp/run-all-checks.j0MY/runs)
  ```

  and its lines: `ok   import-sheet             3 scenarios    22s  board=mcu30-sheet`,
  `ok   hierarchy-navigator      4 scenarios    12s  board=mcu30-sheet`.
- An earlier `--no-build` run of the hierarchy check alone passed too (`ok   hierarchy-navigator      4 scenarios    13s  board=mcu30-sheet`),
  and the import-sheet check was first run against a served scratch board (`__importSheetCheck: done=true ok=true scenarios=3 failed=[]`).

**Unsure / for the reviewer.**

- The pages 4 and 2 are what the studio's import stored (the reader keeps each sheet's first instance page; GAPS.md item 4's known limit).
  The check asserts what the panel shows for the design it holds and what KiCad's pane would do with pages: it does not claim KiCad's Import
  Sheet would assign 4 and 2 (KiCad numbers new sheet instances itself, `SCH_SHEET_LIST::AddNewSheetInstances`).
- Finding 2 asks the panel to show a page for a sheet no one gave one; the studio may prefer to leave page assignment to the writer/import.
  The assertion is left out rather than changed, as asked.
