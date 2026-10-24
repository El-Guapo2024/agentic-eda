# Handoff: the KiCad port, 2026-10-24 (resumed)

For a fresh session, for example on the `ws-agentic-eda` cloud box (`~/work/agentic-eda`). Read this first, then `docs/ARCHITECTURE.md` and `docs/parity/GAPS.md`.

## Where it stands

- **Paused on 2026-10-23 at the user's request.** `main` (this commit) is green and pushed: `tools/check.sh fast` Rust 3627 passed, web 2449, and `tools/run-all-checks.sh` 42/42 browser checks (see "Gates").
- 738 of KiCad's 785 editor actions are wired: pcbnew 346/369, eeschema 220/233, common 172/183 (`docs/parity/UI-ACTIONS.md`). The other 47 each carry a reason in `web/studio/tools/ui-parity-missing.json`.
- Every task of the 2026-10-21 list is merged (below), and so are function-by-function reviews against KiCad's source of every port merged on 10-23, plus the library editors (P3/P17) and the schematic core (P4/P10).
- **Merged 2026-10-24:** the IR's lengths are KiCad's nanometres (`task-nanometres`; `docs/NANOMETRES.md`). Old design.json files migrate on read, one way.
- **Found, not fixed:** findings-only reviews of every other package, in `docs/parity/findings/` (`INDEX.md`: 39 high, 87 medium, 71 low, in six fix batches A–F).
- A real KiCad board brought through the studio gives kicad-cli's DRC the same findings as the original on all 12 QA boards (`docs/parity/REPORT.md` section 2); connectivity agrees with kicad-cli on 97.8% of counts over 186 boards.
- The rules that don't change:
  - `design.json` plus the intent is the only source of truth; KiCad files are derived from it.
  - kicad-cli is the only DRC, ERC, plot, export and zone-fill engine.
  - Every edit is one undoable `/api/cmd` verb.

## What landed (2026-10-14 to 10-24)

- 2026-10-24: **fix batches B and C** (`28ba30c4`): zones keep their holes, SMD pads on several copper layers, paste-only pads, curves, footprint Edge.Cuts (KiCad's footprint outline pass), rectangle corner radius (the router's edge as KiCad's effective shapes), Extruded 3D Body, importers' `.kicad_dru`, a project's DRC exclusions; refills keep teardrop copper via `(legacy_teardrops yes)` (trade-off: GAPS 13). Open: imported ERC exclusions (batch D's file), Repeat Layout target holes.
- 2026-10-24: **fix batch F, editing** (`963764f7`): items edited in place keep their group, the real courtyard box, R/F while carrying, Position Relative's angle, Recombine, Repeat Layout's nets and groups, design-block layers and padless nets (`board_nets`), KiCad's Net Inspector with per-layer lengths; one-member groups kept, the < 2 rule only on groups a verb touched.
- 2026-10-24: **fix batch E, the router** (`a6e4500a`): all 7 highs, 18/21 mediums; KiCad's 7 router QA sessions replay exactly; F is Attempt Finish, Esc commits fixed legs, empty-space routes on no net. Plus `ea752e1a`, the tuner's default board thickness (1.6 µm → 1.6 mm, a nanometre leftover).

Each package's leftovers are listed in its `GAPS.md` item and `PARITY-*.md` section.

| Package | What it brought |
|---|---|
| P13 zones | KiCad's fill is the only fill (`crates/zone-filler` is deleted). Fill All runs `kicad-cli pcb drc --refill-zones --save-board` on a copy and caches the rings. |
| P15 review | ERC and DRC exclusions as KiCad 10.99's structured records, Delete Marker, Save report |
| P10b schematic | Groups, a user field's place, Pin Helpers, swapped pins |
| P17 library editors | Pad arrays, derived symbols, snapping, alternate pins, symbol text boxes |
| P22 non-KiCad import | `kicad-cli pcb import` and `sch import` (Eagle, Altium, CADSTAR, PADS, ...) |
| P5d router | Arcs as router items, locked tracks, a head's via, mouse-trail posture, pair Ctrl+E, time-domain tuning |
| P18 design blocks | Libraries, panes, place, save, update |
| P23 re-export fidelity | 12/12; component classes read |
| P16t tables | Board and sheet tables, stackup, board characteristics |
| P16i item kinds | The board's text box, reference and schematic images, points, hatch fills, barcodes (drawn from kicad-cli's plot) |
| P24 multichannel | Placement rule areas, Generate, Repeat Layout, Apply Design Block Layout |
| P19 microwave | Gap, stub, arc stub, polygon, inductor; a schematic dropped on the sheet |
| P25 pad details (10-22) | Explode and recombine, padstacks, Remove Unused Pads, Swap Pad Nets and Swap Gate Nets |
| P26 step 1 (10-22) | Embedded files: the IR and the four readers and writers. `zstd` is added, the one new dependency the user approved. |
| P19b steps 1-2 (10-22) | Specctra: the DSN model with KiCad's writer, lexer and parser, plus `FromBOARD`. kicad-cli has no DSN, so this is our own exporter; the user approved it on 2026-10-21. |
| P19b done (10-23) | Specctra: File > Export > Specctra DSN, File > Import > Specctra Session (`FromSESSION` as `Cmd::Specctra`, one undo step), `eda board export specctra` and `eda board import-ses`. The round trip leaves kicad-cli's DRC unchanged; `e2e/specctra.check.js` passes 5/5. PARITY-pcb.md section 46. No golden DSN exists in `qa/data`, and no outside router was run. |
| P26 done (10-23) | Embedded files: the Embedded Files panel (Board Setup, Schematic Setup, both Footprint Properties), `kicad-embed://` 3D models, Collect and Embed 3D Models; byte-for-byte round trips; STEP/GLB with an embedded model; `e2e/embedded-files.check.js` 9/9. PARITY-pcb.md section 47. Left: font embedding (no outline fonts), the Symbol Editor's tab. |
| P25 fix (10-23) | Remove Unused Pads uses `BOARD::LayerDepth` as KiCad computes it. |
| Reviews (10-23) | P19b and P26 reviewed function by function against KiCad 8303b2ad, with failing tests first: Specctra got 7 fixes (geometry in nm, arcs, custom pads, keepouts, messages), embedded files 12 (the board's font block, Collect's order, the panel's questions word for word). kicad-cli re-saves our `embedded_files` blocks character for character. |
| P9b (10-23) | The DRC Rule Editor (`pcbnew/drc/rule_editor/`), over the `.kicad_dru`; `Cmd::EditDrcRule`. PARITY-pcb.md section 48 lists where it keeps rules KiCad's editor would lose. Left: Show Matches, Syntax help, auto-completion, drag and drop, units. |
| P21 (10-23) | The SPICE simulator: kicad-cli's SPICE netlist, `ngspice -b` (`EDA_NGSPICE` or PATH), the Simulation window, Probe and Tune, the exports, the `.wbk` workbook. Reviewed against `eeschema/sim/`; the vectors equal ngspice run by hand on kicad-cli's netlist. Left: plotting while it runs (no libngspice), S-parameter port numbers, potentiometer tuning, IBIS currents. PARITY-sch.md section 38. |
| Specctra via kicad-cli (10-23) | DSN export through `kicad-cli pcb export specctra` when kicad-cli has it; our exporter is the DEPRECATED fallback (the user's decision). |
| IDF via kicad-cli (10-23) | File > Export > IDFv3 through `kicad-cli pcb export idf`; GenCAD (already wired) gated the same way. Both are disabled, with the reason, on a kicad-cli without the command. kicad-cli reads `--user-origin-x/-y` in 1/25400 mm with `--units mils`, and `idf_args` compensates; the slow test `idf_takes_the_dialogs_reference_point_in_mils` fails if a later nightly fixes that. |
| Router leftovers (10-23) | Springback, the four corner modes, virtual vias and copper text as an obstacle, reviewed against `pcbnew/router/`. Left: springback in the dragger (~200 lines) and the pair placer, the mark-obstacles hull snap, and keeping the previous head when the head cannot be resolved. |
| Schematic details (10-23) | A pasted sheet shares its screen, buses across sheets, a label's own fields; reviewed against eeschema's code; 18 of KiCad's QA hierarchies trace as kicad-cli reads them. Left: a shared screen inside a shared screen (needs references keyed by the whole path), two buses joined by a shared member name, `${#}` per placement, 20 of 371 nets of `netlists/video`. PARITY-sch.md section 39. |
| Router via actions (10-23) | Blind/buried and micro vias, Select Layer and Place, Custom Track/Via Size, Switch Posture, in the router's context. The nightly's DRC adds `microvia_crosses_core`, which KiCad 8303b2ad lacks: a microvia from an outer layer fails on KiCad's default 4-layer stackup. |
| KiCad 10 items (10-23) | Line endings, grid items, via stitching, drill charts and maps, ported from KiCad 25ec2335 (`~/src/kicad-src-25ec2335`, the box kicad-cli's commit; 8303b2ad predates them) and reviewed against it. Drill charts match kicad-cli's rebuild exactly. Left: imperial drills (needs drills in nm), a footprint's copper fields as stitch obstacles, undoing a chart placement keeps its profile marks. PARITY-pcb.md section 50. |
| Library editors review (10-23) | P3 and P17 against KiCad's code; KiCad's installed libraries round-trip through our readers and writers (164 footprints, 243 symbols, none differing beyond sub-µm rounding). `LibraryPad` has a new `kicad` field: older branches' literals need `Default::default()`. Left: the IR's µm (70 of 164 footprints round by up to 0.5 µm), a hidden fp_text, colours, `.kicad_mod` still written as 20240108. |
| Schematic core review (10-23) | P4, P10 and P10b against eeschema's code. Turned and mirrored symbols now go through KiCad's TRANSFORM in the engine, the tracer and the writer; before, their pins were not where they were drawn. 87 of KiCad's 130 QA schematics written back are judged alike by kicad-cli's ERC and netlist (a ratchet). Left: sheet naming (kept together), nested shared screens, `${#}`. PARITY-sch.md section 40. |
| Connectivity measure (10-23) | A fixed set of 186 boards (`examples/` plus KiCad's QA boards, never `work/`); 97.8% of counts equal kicad-cli's; the ratchet passes on the box. Left: sub-µm segments (6 boards, needs nm in the IR), pads sharing a number but not a net, copper graphics with a net, teardrops, a custom-rule board. The studio's Fill All may lose teardrop zones' fills (read from the code, not run: GAPS.md item 36). PARITY-pcb.md section 51. |
| P20 (10-23) | 42 browser checks; `tools/run-all-checks.sh [-j N]` builds once, serves each check a fresh copy of its board and prints one line per check (42/42, about 24 min at -j 3). Found and left: the schematic-parity DRC on the routed mcu30 finds 76 board-vs-schematic differences (footprint fields, nets), the Symbol Editor's pin tool and New Symbol skip their dialogs, Both sides writes one placement file. GAPS.md item 33 has the coverage table. |
| Also | The Generators Manager no longer swallows Escape app-wide, and `studio_import_foreign` reports why its test server died |

## Open branches (on GitHub, not merged)

| Branch | What it does | How far | What's left |
|---|---|---|---|
| `task-fix-a-kicad-names` (`~/work/agentic-eda-embedded`) | Batch A: KiCad's net names everywhere (the loader traces the schematic, `crates/cli/src/net_names.rs`; the derived schematic labels every net the intent names), one footprint name (`footprint_lib_id`), `(path)`/`sheetname`/`sheetfile`, fields, `pinfunction`/`pintype`, `unconnected-(..)`, the flip frame | review fixes R1-R6 in progress | gate, merge; then batch D |

Gates for any of them:
1. `cargo check --workspace --tests --keep-going`, with no warnings.
2. `tools/check.sh fast -p <crates touched>`: debug Rust tests, the web typecheck and the unit tests.
3. The slow kicad-cli proofs of the change: `EDA_SLOW_TESTS=1 KICAD_SRC_DIR=<kicad source> cargo test -p <crate> --test <name>`.
4. If the readers or writers changed, the 12-board re-export with a private `TMPDIR`: `EDA_SLOW_TESTS=1 EDA_PARITY_PARTS=reexport python3 tools/parity_report.py`.
5. The browser check: `web/studio/e2e/<name>.check.js`, pasted into a served page and run through `window.__eda`.

## Next tasks, in order (the user's decisions of 2026-10-23)

The 2026-10-21 list is done: Specctra, embedded files and the pad-details fix, the simulator, the rule editor, the router details, the schematic details, the KiCad 10 items, P20's browser checks, and the connectivity measure (each row under "What landed").

1. ~~**Finish `task-nanometres`**~~ merged 2026-10-24: lengths are KiCad's nanometres, design.json schema 2, old files migrated on read (one way). Connectivity 98.9%. See `docs/NANOMETRES.md`.
2. **Fix the findings, batch by batch, from the new main** (`docs/parity/findings/INDEX.md`). Each batch's findings carry our `file:line`, KiCad's function, a repro and the failing test to write. A–D can run in parallel, then E and F.
   - **A. KiCad compatibility of what we derive (first).** Today a studio project opened in KiCad and updated from its schematic (F8) renames every net and duplicates every footprint. **The user chose (2026-10-23) KiCad's own net names everywhere, the studio's display included**: `/sheet/NAME`, `/NAME` on a flat sheet, `Net-(REF-PadN)` unlabelled; global and power names unchanged. Also: one footprint name everywhere (the installed KiCad footprint), the footprint-to-symbol `(path ...)` link, symbol fields on footprints, `pinfunction`/`pintype`, the flipped footprint's frame. Prove it: kicad-cli's `--schematic-parity` DRC on the examples finds 0 differences (it finds 76 on mcu30 today).
   - **B. Board reader and writer losses:** zones with holes read as two zones, SMD pads on `*.Cu`, paste-only pads, curves with a control point on an end, footprint Edge.Cuts, rectangle corner radius, Extruded 3D Body, importers' `.kicad_dru`, a project's DRC/ERC exclusions.
   - **C. Teardrops:** Fill All and every refill before DRC or a plot wipe every teardrop's fill.
   - **D. Schematic connectivity:** buses joined by a shared member name, hidden power pins, `${#}` per placement, bus aliases, unit-0 pins, the ERC exclusion key on shared sheets.
   - **E. Router:** the walk-around detour, lost end vias, Walk-around via violations, Esc discarding fixed segments, the diff-pair gap, and the replay tests' macOS default path.
   - **F. Editing:** point edit and Mirror dropping items from groups, footprint bounding boxes, the carry's rotate pivot, Position Relative's angle, Recombine, Repeat Layout's nets and groups, design-block copper, the Net Inspector.
   - Then the mediums and lows, and the wrong doc claims listed at the end of `INDEX.md`.
   - Order as run (2026-10-24, the user: "do all"): A, B+C, E and F in parallel (agents' extra brief: `~/work/briefs/fix/common-fix.md`); D after A merges (both touch `crates/engine` naming); then the length newtype below.
3. **The length newtype** (the user, 2026-10-24: do it): make `Nm` a newtype so the compiler catches unit mixing, with `mm()`/`mil()` test helpers and conversions only at the boundaries, done in waves by crate (model, then kicad/drc/engine, then ops/pns, then cli/studio). Run it last, on a quiet main, since it touches every crate.
4. **Leftovers named in the merges** (each in GAPS.md): springback in the dragger and the pair placer, a shared screen inside a shared screen, the Symbol Editor's pin and New Symbol dialogs, Both sides placement files, the checks P20 left uncovered.

TODO (the user, 2026-10-23: not now): check the kicad-cli fallbacks on a kicad-cli without `pcb export specctra`, `idf` or `gencad` (the Mac's 10.99.0-5334); delete the deprecated Specctra exporter once every machine has the command (scope in GAPS.md P19).

Kept out by decision: Import Netlist (user, 2026-10-21); the browser shell, Python plugins, the autoplacer and the viewer windows (see `STALE-REASONS.md`); Hyperlynx and footprint-report export (no kicad-cli exporter, not even in the box's nightly; ask the user before porting them as was done for Specctra). IDFv3 and GenCAD are kicad-cli's (user, 2026-10-23: `task-cli-exports`), offered where the kicad-cli in use has the command and disabled with the reason where it has not.

## How the work was run

- **One task per git worktree**, each on its own `task-*` branch. On the box, use `~/work/agentic-eda-<task>`.
- **Merge `main` into the branch first, in its own worktree:**
  - Keep both sides of every conflict.
  - A new PARITY section is numbered after the last one on main; renumber on a clash, and fix that section's references in `GAPS.md` and `STALE-REASONS.md`.
- **Then gate it and merge it:**
  - Run gates 1 and 2 above.
  - `git merge --no-ff <branch>` into main.
  - Regenerate the action audit: `KICAD_SRC_DIR=<kicad source> node web/studio/tools/ui-parity-audit.mjs`, then commit `docs/parity/UI-ACTIONS.md`.
  - Push.
  - Run `tools/check.sh fast` (whole workspace) after each batch.
- **Briefs for port agents:**
  - Port from the KiCad source, not from memory, and cite the KiCad function.
  - IR additions are overlays with serde defaults.
  - No new crates.io or npm dependency without the user's OK.
  - Never edit `work/` or `examples/`; build scratch boards from `examples/*.yaml` with `eda board new`.
  - Keep hub-file edits small and additive (`crates/ops/src/lib.rs`, `ir.rs`, `studio.rs`, `board.rs`, `useActionRunner.ts`, `store.tsx`, `painter.ts`).

## Known pitfalls

- **kicad-cli versions:**
  - The Mac ran 10.99.0-5334; the box has nightly 20261021 (25ec2335f2).
  - 5334 writes footprints with `(transform ...)`, stores DRC and ERC exclusions as structured records, gives ERC positions in mm, and crashes plotting a designator-form cross-reference (we write `${/path/uuid:FIELD}`).
  - Rerun the slow suites on a new nightly before trusting them.
  - The Specctra DSN export takes `kicad-cli pcb export specctra` where kicad-cli has it (the box's nightly does; 5334 has not, so the Mac gets our exporter, `engine: "eda"`). Delete the fallback (`export_dsn`, marked DEPRECATED) once every machine runs a kicad-cli with the command; GAPS.md P19 lists which `crates/kicad/src/specctra/` modules go with it (`export.rs` and the writer tests) and which the SES import keeps (`lexer`, `parse`, `model`, `format`, `geom`, `board`, `session`).
- **Nanometres are one way** (`task-nanometres`, `docs/NANOMETRES.md`): `design.json` and its `.history/` snapshots hold KiCad's nm since `schema: 2`. An old binary (from before the merge) that reads a board a new one has saved reads every length 1000x too big, and nothing guards against it. Don't run an old `eda` on such a board. The other way works: a new binary reads a `schema: 1` (µm) board, its snapshots, `board.json`, `view.json`, `appearance.json` and the hand-written intents (`examples/*.yaml`, which have no `schema` and are µm), and scales them on read. A board that is only read, never edited, stays µm on disk. Lengths in tests and in the studio are nm: a literal `1000` is 1 µm, and `1_000_000` is 1 mm.
- **The serde gotcha:** starlark turns on serde_json's `arbitrary_precision`, which breaks the tagged-enum reader on numbers. Read such types field by field.
- **New required fields break other branches' literals.** Two examples: `Group` has `design_block`, and `LibraryPad` has `stack` (padstacks). A merge can leave a literal without one; `cargo check` finds it, and `Default::default()` fixes it.
- **`studio_import_foreign` failed once under load:** the server exited before printing its port. The test now prints the server's output and exit status, so the next failure explains itself.
- **Linux box vs the Mac** (fixed 10-23, `task-linux-gate`): KiCad's libraries are found under `$KICAD_PATH` or `/usr/share/kicad-nightly` (`eda_kicad::kicad_shared_dir`), not only the macOS bundle. Without them the studio falls back to built-in symbols and packages, a path the Mac never ran; `EDA_KICAD_SYMBOLS=/nonexistent` exercises it. Node 22's `node --test` takes files or globs, not a directory.
- **`._*` files in the box's KiCad source** (AppleDouble files from the Mac copy, e.g. `qa/data/pcbnew/._api_kitchen_sink.kicad_pcb`): the re-export harness takes the first 12 boards by name, which are these, and kicad-cli cannot load them (0/12 and a ratchet failure). Run Part 3 with `EDA_PARITY_REEXPORT_CAP=500 EDA_PARITY_REEXPORT_ONLY=<the 12 names of docs/parity/raw/roundtrip.json>` (a partial run, it writes nothing), or delete the `._*` files; `KICAD_QA_DATA` must point at `$KICAD_SRC_DIR/qa/data`.
- **Disk:** a debug `target/` is 20-40 GB per worktree, and `CARGO_INCREMENTAL=0` keeps it smaller.
- **Never commit `web/studio/node_modules`:** it can be a symlink to another checkout. Add files by explicit path.
- **Browser checks** need a browser on a served board (`target/debug/eda board serve -C <dir> --port <p> --ui web/studio/dist`, after `npm run build`). On the box, Playwright and Chromium are installed outside the repo (`~/tools/pw`, 2026-10-23): `PLAYWRIGHT_MODULE=~/tools/pw/node_modules/playwright/index.mjs node web/studio/e2e/run-check.mjs http://localhost:<p>/ web/studio/e2e/<name>.check.js [minutes]` runs one check and exits 0 when it passes (pad-details: 10/10). `tools/run-all-checks.sh [-j N] [name ...]` runs all of them (or some), each on a fresh copy of the board its header names (`// run-all-checks: board=...`), on ports 8810-8814. A board copied in by hand (`cp design.json`) is an undoable "changed outside a command" step: clear its `.history` first, or a check's undo-until-clean loop unplaces the board.

- **Agents sharing the box:** give each its own scratch directory and its own port range, and kill only processes it started. Never use `pkill -f` with a pattern that also matches your own shell's command line: it kills that shell. Check a port is free (`ss -ltn`) before serving: two agents once drove each other's studios.

## Not in git (what a fresh session may need)

- **The KiCad source:** the port reads it, and the slow tests take their QA boards from `qa/data`.
  - On the box: `~/src/kicad-src-8303b2ad` (`KICAD_SRC_DIR`; its macOS `._*` files were deleted, they broke `tools/parity_report.py`), and `~/src/kicad-src-25ec2335`, the box kicad-cli's commit, for KiCad 10 items newer than 8303b2ad (cited as "KiCad 25ec2335").
  - On the Mac: `~/ws/kicad-src-8303b2ad`.
  - `tools/parity_report.py` and the slow tests need `KICAD_QA_DATA=$KICAD_SRC_DIR/qa/data`.
- **Tools installed on the box, outside the repo:**
  - Playwright + Chromium in `~/tools/pw` (`PLAYWRIGHT_MODULE=~/tools/pw/node_modules/playwright/index.mjs`) for `web/studio/e2e/run-check.mjs` and `tools/run-all-checks.sh`.
  - ngspice 42 extracted to `~/tools/ngspice`, linked at `~/.local/bin/ngspice`. apt's ngspice conflicts with the nightly's `libngspice-kicad`, so don't apt-install it.
- **Agent briefs:** `docs/agent-briefs/common.md` (the rules every port agent got) and `findings.md` (the findings-only reviews). One worktree and one agent per task, each brief starting from `common.md`.
- **The user's boards in `work/`** are the user's own and not needed for development. The real ones are on the Mac; the box's `work/mcu30` is a near-empty scratch, backed up in `~/backups/` before the nanometre work. Don't undo or reorganize them without asking. After `task-nanometres` merges, they are migrated on first load, one way.

## Gates of the last main

- **`fd3f1b5`** (the box, 2026-10-23, every task of the list merged): `tools/check.sh fast` Rust 3627 passed, 0 failed, 25 ignored; web typecheck ok, 2449 unit tests. `tools/run-all-checks.sh -j 3`: 42 passed, 0 failed of 42 in 1382 s.

- **`tools/check.sh fast` on `ef84155`** (the box, ws-agentic-eda, 2026-10-23): Rust 3387 passed, 0 failed, 24 ignored; web typecheck ok, 2347 unit tests. `cargo check --workspace --tests` has no warnings.

- **`tools/check.sh fast` on `2b0cafb`** (everything above merged) passed: Rust 3387 passed, 0 failed, 24 ignored; web typecheck ok, 2347 unit tests passed. This commit only adds this file.
- **The slow tier** (`EDA_SLOW_TESTS=1`, kicad-cli) ran per package on the Mac's 10.99.0-5334. Rerun it on the box's nightly before relying on it.
