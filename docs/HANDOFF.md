# Handoff: the KiCad port, 2026-10-22

For a fresh session, for example on the `ws-agentic-eda` cloud box (`~/work/agentic-eda`). Read this first, then `docs/ARCHITECTURE.md` and `docs/parity/GAPS.md`.

## Where it stands

- `main` is green (see "Gates" for the last run) and pushed to GitHub.
- 682 of KiCad's 766 editor actions are wired: pcbnew 322/350, eeschema 197/233, common 163/183 (`docs/parity/UI-ACTIONS.md`).
- Of the 84 that are not, 44 are kept out on purpose, each with its reason in `web/studio/tools/ui-parity-missing.json`, and 40 are still to do (below).
- A real KiCad board brought through the studio gives kicad-cli's DRC the same findings as the original on all 12 QA boards, in both file formats (`docs/parity/REPORT.md` section 2).
- The rules that don't change:
  - `design.json` plus the intent is the only source of truth; KiCad files are derived from it.
  - kicad-cli is the only DRC, ERC, plot, export and zone-fill engine.
  - Every edit is one undoable `/api/cmd` verb.

## What landed (2026-10-14 to 10-22)

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
| Also | The Generators Manager no longer swallows Escape app-wide, and `studio_import_foreign` reports why its test server died |

## Open branches (on GitHub, not merged)

None after 2026-10-23's merges, apart from those started below.

Each was cut from an older main. Merge `main` into it first.

| Branch | What it does | How far | What's left |
|---|---|---|---|
| `task-p20-browser-checks` | P20: `tools/run-all-checks.sh` (every browser check headless, on the boards their headers name); five new checks (board-edit, sch-edit, lib-edit, board-setup-rules, plot-exports); eight fixes they found (item edits off the placement grid, the junction of a T, the library editors' E / R / Del actions, Pre-defined Sizes in one undo step, Escape on a popup menu, a CSV placement file's name, the parity DRC on a board with no stored schematic); stale checks brought up to date | done; gates: cargo check 0 warnings, check.sh fast Rust 3552/0, web 2414; run-all-checks 40/41 then the one left (embedded-files, a fixed sleep) 1/1 | what GAPS.md item 33 lists |

Gates for any of them:
1. `cargo check --workspace --tests --keep-going`, with no warnings.
2. `tools/check.sh fast -p <crates touched>`: debug Rust tests, the web typecheck and the unit tests.
3. The slow kicad-cli proofs of the change: `EDA_SLOW_TESTS=1 KICAD_SRC_DIR=<kicad source> cargo test -p <crate> --test <name>`.
4. If the readers or writers changed, the 12-board re-export with a private `TMPDIR`: `EDA_SLOW_TESTS=1 EDA_PARITY_PARTS=reexport python3 tools/parity_report.py`.
5. The browser check: `web/studio/e2e/<name>.check.js`, pasted into a served page and run through `window.__eda`.

## Next tasks, in order (the user's decisions of 2026-10-21)

1. ~~**Finish Specctra**~~ merged 2026-10-23 (`9cd36d7`).
2. ~~**Finish embedded files**, then merge `task-pad-details-wip`~~ both merged 2026-10-23.
3. ~~**The simulator**~~ merged 2026-10-23. (P21; 32 rows of `ui-parity-missing.json`): `kicad-cli sch export netlist --format spice`, then ngspice (`ngspice -b`, or libngspice), parsed into a Simulation panel. ngspice is a separate program, not a crate. KiCad bundles libngspice; on the box, `sudo apt-get install ngspice`. Port the panels from `eeschema/sim/`.
4. ~~**The graphical rule editor**~~ merged 2026-10-23. (P9b, `pcbnew.DRETool.drcRuleEditor`), after the simulator. Custom Rules is edited as text today.
5. **Details** listed in `GAPS.md`:
   - the router: ~~springback, rounded corners, virtual vias~~ on `task-router-details` (springback for the line placer only);
   - the schematic: copying and pasting whole sheets, buses across sheets, a label's own fields;
   - KiCad 10 items the reader skips: line endings, grid items, via stitching, drill charts;
   - browser checks for the older areas (P20);
   - re-running `tools/parity_report.py` (connectivity is at 84%). On the box (2026-10-23) it needs `KICAD_QA_DATA=$KICAD_SRC_DIR/qa/data`, and its connectivity part evaluates 56 boards, against a floor of 58 set on the Mac, which fails the ratchet. The two boards are the user's `work/` boards that exist only on the Mac (`work/l1-order`; a worktree has no `work/mcu30` either), so the floor counts boards the box does not have. That is a machine difference, not a regression. A floor that counts only `qa/data` and `examples/` would fix it.

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
- **The serde gotcha:** starlark turns on serde_json's `arbitrary_precision`, which breaks the tagged-enum reader on numbers. Read such types field by field.
- **New required fields break other branches' literals.** Two examples: `Group` has `design_block`, and `LibraryPad` has `stack` (padstacks). A merge can leave a literal without one; `cargo check` finds it, and `Default::default()` fixes it.
- **`studio_import_foreign` failed once under load:** the server exited before printing its port. The test now prints the server's output and exit status, so the next failure explains itself.
- **Linux box vs the Mac** (fixed 10-23, `task-linux-gate`): KiCad's libraries are found under `$KICAD_PATH` or `/usr/share/kicad-nightly` (`eda_kicad::kicad_shared_dir`), not only the macOS bundle. Without them the studio falls back to built-in symbols and packages, a path the Mac never ran; `EDA_KICAD_SYMBOLS=/nonexistent` exercises it. Node 22's `node --test` takes files or globs, not a directory.
- **`._*` files in the box's KiCad source** (AppleDouble files from the Mac copy, e.g. `qa/data/pcbnew/._api_kitchen_sink.kicad_pcb`): the re-export harness takes the first 12 boards by name, which are these, and kicad-cli cannot load them (0/12 and a ratchet failure). Run Part 3 with `EDA_PARITY_REEXPORT_CAP=500 EDA_PARITY_REEXPORT_ONLY=<the 12 names of docs/parity/raw/roundtrip.json>` (a partial run, it writes nothing), or delete the `._*` files; `KICAD_QA_DATA` must point at `$KICAD_SRC_DIR/qa/data`.
- **Disk:** a debug `target/` is 20-40 GB per worktree, and `CARGO_INCREMENTAL=0` keeps it smaller.
- **Never commit `web/studio/node_modules`:** it can be a symlink to another checkout. Add files by explicit path.
- **Browser checks** need a browser on a served board (`target/debug/eda board serve -C <dir> --port <p> --ui web/studio/dist`, after `npm run build`). On the box, Playwright and Chromium are installed outside the repo (`~/tools/pw`, 2026-10-23): `PLAYWRIGHT_MODULE=~/tools/pw/node_modules/playwright/index.mjs node web/studio/e2e/run-check.mjs http://localhost:<p>/ web/studio/e2e/<name>.check.js [minutes]` runs one check and exits 0 when it passes (pad-details: 10/10). `tools/run-all-checks.sh [-j N] [name ...]` runs all of them (or some), each on a fresh copy of the board its header names (`// run-all-checks: board=...`), on ports 8810-8814. A board copied in by hand (`cp design.json`) is an undoable "changed outside a command" step: clear its `.history` first, or a check's undo-until-clean loop unplaces the board.

## Not in git (what a fresh session may need)

- **The KiCad source:** the port reads it, and the slow tests take their QA boards from `qa/data`.
  - The Mac has commit `8303b2ad` at `~/ws/kicad-src-8303b2ad` (544 MB). The box has none, so copy it or clone KiCad at `8303b2ad`.
  - Set `KICAD_SRC_DIR` to it.
- **The agent briefs:** the brief template, the task briefs and a stall watch are on the Mac in `.claude/briefs/`. Their task texts are summarised above; the template's rules are under "How the work was run".
- **The user's boards in `work/`** (for example `work/mcu30`, which the studio on 8766 serves) are the user's own and not needed for development. Its J1 drag from 10-07 left broken nets; don't undo or reorganize it without asking the user.

## Gates of the last main

- **`tools/check.sh fast` on `ef84155`** (the box, ws-agentic-eda, 2026-10-23): Rust 3387 passed, 0 failed, 24 ignored; web typecheck ok, 2347 unit tests. `cargo check --workspace --tests` has no warnings.

- **`tools/check.sh fast` on `2b0cafb`** (everything above merged) passed: Rust 3387 passed, 0 failed, 24 ignored; web typecheck ok, 2347 unit tests passed. This commit only adds this file.
- **The slow tier** (`EDA_SLOW_TESTS=1`, kicad-cli) ran per package on the Mac's 10.99.0-5334. Rerun it on the box's nightly before relying on it.
