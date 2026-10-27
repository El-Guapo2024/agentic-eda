# Handoff: the KiCad port, 2026-10-25 (fix batches and the length type done)

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

- 2026-10-28: **round 3 of the KiCad review, fixed** (findings `docs/parity/findings/round3/`, 125: schematic 41, PCB editing 36, editors/exports 24, board files 13, router 11; each marked Fixed or Open) and **round 2's leftovers** (`integrate-r3`): the router's copper graphics and multi-layer keepouts and rule-engine start sizes; Cleanup Tracks & Vias per segment, table cells, zone-drawing undo; Set Anchor, plot params (KiCad's whitelist, the dialogs saving back, `Cmd::SetPlotSettings`); sheets outside the root folder kept inside `.kicad/`, the ERC table from 25ec2335; local power symbols, per-path units, KiCad's junction clean-up; thieving zones, `(net 0)`, via uuids, the `.kicad_pro` hole clearance 0.25. The router's kicad-cli DRC proofs fixed (every net needs a pad of its own in the cluster, or kicad-cli renames it; `tracks_crossing`, `diff_pair_gap_out_of_range`).
- 2026-10-28: **pictures from kicad-cli** (the user's reuse rule): `eda schematic`, the judge and `/api/schematic.svg`; `crates/render` and the judge's drawing kept INACTIVE behind `EDA_RUST_PICTURES=1` (the user: never delete a replaced port).
- 2026-10-27: **round 2 of the KiCad review, fixed** (findings `docs/parity/findings/round2/`, 160): router (rule-engine clearances, keepouts), PCB editing (34/36), editors and exports (35, Gerbers with the board's plot settings, drill defaults), board files (masked tracks, pad layers, the file version), schematic (KiCad's new connectivity engine, the user's 2026-10-27 choice; 24/24 QA projects alike in net names), Board Setup messages. The open items of each are marked in its findings file. Known: the `.kicad_pro` written for an intent board says min_hole_clearance 0.2 where the router uses 0.25 (fixed on `task-r2-misc-leftovers`).
- 2026-10-27: **Hierarchy Navigator and sheet pages**: the walk by place, children sorted by page, every path paged (virtual pages, `GetNextPageNumber`), pages per sheet path in the file (`SchExtras::sheet_pages`). Differs: the panel's label reads "name, file · page N" (KiCad: "name (page N)").
- 2026-10-26: **nested shared screens** (leftover 2): references keyed by the whole sheet path (`SymbolPathOverride::sheet_path`), old keys migrated on read with the same reference per placement; per-path sheet instances; Paste shares nested screens; Annotate counts other paths' references. Left (GAPS 4): a page number per sheet path (the reader keeps the first), per-path Annotate, sorted instance paths.
- 2026-10-26: **the DeepSeek lane's leftovers** (reviewed by Claude against KiCad, then fixed and gated): springback in the dragger and the pair placer, the Symbol Editor's pin and New Symbol dialogs, Both-sides placement files with KiCad's names, browser checks for Page Settings/origin, Board Statistics/Swap Layers and the board view (`docs/DS-LOG.md`). Differs (small): New Symbol's pin-name offset of 0 with "Pin name inside" becomes 20 mil (KiCad: 1 IU), as Symbol Properties already does; a pin is placed on OK, not on a second click.
- 2026-10-25: **the length type, wave 3**: the cross-crate items typed, the shims deleted, `docs/NANOMETRES.md` finished (the whole API, the integer kernels kept on purpose, every unit bug of the three waves); parity raw outputs refreshed. **The length type is done.** Follow-up (parity): KiCad's schematic graphics import approximates a spline with `aStroke.GetWidth() / 2` (`setupSplineOrLine`), ours always with `ARC_HIGH_DEF` (`eda_kicad::graphics_import::spline_or_line`).
- 2026-10-25: **the length type, wave 2** (`53d856b7` ops, `044a5b46` pns, `027cfd87` cli group, `3bb23b95` core crates): every shim removed; same_design 138/138. Bugs caught: import-kicad's empty board was 0.1 x 0.08 mm (now 100 x 80), render's mirrored-symbol bounds ~15 km off (mm/nm mix), lint's um/nm message, the mouse-trail oracle test at 1/1000 scale, the undo tests' sheet at 0.09 mm.
- 2026-10-25: **the length type, wave 1** (`7745bb56`): `Nm` is a newtype (see `docs/NANOMETRES.md` "The length type"); the model converted, other crates through `raw_shim` markers; no behaviour change (`tools/same_design.sh`: 138 cases identical). Caught three micrometre leftovers: the no-connect obstacle, the custom-pad max error, DRC's reference silk box.
- 2026-10-25: **fix batch D, schematic connectivity**: buses sharing a member name, hidden power pins, `${#}` per placement, bus aliases, unit-0 pins, ERC exclusions per shared-sheet placement and on import; an imported KiCad schematic is no longer left in KiCad's frame. 22 QA hierarchies trace as kicad-cli reads them. Open: F5 keyed on a second item, project `${VAR}` in power values.
- 2026-10-25: **fix batch A, KiCad's names** (`a8287704`): KiCad's net names everywhere incl. the studio (option 1), every net the intent names labelled in the derived schematic (the user, 2026-10-24), one footprint name, `(path)`/sheetname/sheetfile, fields and pinfunction/pintype, back footprints in KiCad's frame; kicad-cli's schematic-parity DRC: 0 differences on opamp_filter, ldo, mcu30 (were 22 and 76). R6 merged 2026-10-25: package parts carry the installed footprint whole (pin headers keep the built-in pads); the repair loop reorders LED/resistor chains and re-places for proximity, so every example places at seeds 0-2.
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
| Router leftovers (10-23) | Springback, the four corner modes, virtual vias and copper text as an obstacle, reviewed against `pcbnew/router/`. Left: the mark-obstacles hull snap, and keeping the previous head when the head cannot be resolved. (Springback in the dragger and the pair placer was on this list; done 2026-10-25, `crates/pns/PARITY.md` Stage 14.) |
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
| `task-r4-schematic` (`-embedded`, 2026-10-28) | The round-4 schematic findings (`findings/round4/r4-schematic.md`): 25ec2335's pin facts (jumpers, duplicate and stacked numbers, NC pins, bus wire entries), local power from the library symbol, `lib_name` resolution, one clean-up per batch, a path's unit and Exclude from position files through renames, sheets above the root's folder, per-path numbering, Align to Grid's grid, a label on a pin's net, ERC severities and markers; `PARITY-sch.md` section 47 | 6/6 high, 8/8 medium, 5/8 low | D4-L1, D4-L2, D4-L5 |
| `task-r4-pcb-edit` (`-router`, 2026-10-28) | Round 4's PCB editing findings (`r4-pcb-edit`, 1 high, 10 medium, 9 low): teardrops as kicad-cli makes them, Swap Layers and tuning patterns, layer defaults, Create Array's kinds and group centre, the drawing tools' loops (Line chain, first click, Delete, Redo, closing on the first corner, the line mode), Global Deletions, and 8 lows (PARITY-pcb section 58) | done | C-8, the rest of A-5, Create Array of tuning patterns (GAPS item 8) |
| Round 4 fixes (2026-10-28): `task-r4-schematic` (`-embedded`), `task-r4-pcb-edit` (`-router`), `task-r4-router` (`-rules`), `task-r4-editors-exports` (`-sim`), `ds/r4-board-setup` (`ds-agentic-eda`, then the board-files findings), `task-kicad-api-server` (`-exports`) | Round 4 findings: `docs/parity/findings/round4/` (copies; live in `~/work/findings/round4/`): 74 (schematic 22, PCB editing 20, editors/exports 16, router 8, board files 8; 7 high). Round 3 had 125 (19 high), round 2 160, round 1 about 200 | started | all |
| `task-reuse-cpp` (`-sim`, 2026-10-28) | The user's reuse rule: KiCad's own Clipper2 2.0.1 and nanosvg (C++, `third_party/`, built with `cc`), the `delaunator` crate, and one copy of the expression language (Find by Properties posts to `/api/pcb/query`); the ports kept INACTIVE (features `eda-clipper2/rust-port`, `eda-connectivity/rust-port`, `eda-kicad/rust-nanosvg`; the TS `pcbExpr.ts`/`libeval.ts` imported only by tests); ARCHITECTURE "Reuse before porting", GAPS item 39 | done | SVG text (needs Fontconfig/FreeType/HarfBuzz); the SPICE value parser stays in two languages; the Query tab's geometry functions |
| `task-kicad-api-server` | The kicad-cli api-server lane (`crates/kicad-engine/src/api`: one `kicad-cli api-server` kept up, protobuf types generated from KiCad's `api/proto`, nng's SP framing over the Unix socket); the Specctra session import through its `ImportSpecctraSession` (our `crates/kicad/src/specctra` INACTIVE, the fallback); `drill`, `pos`, `ipcd356` exports through its jobs (per-run fallback) | Done, gates run | Other export kinds through the lane; the `nng` crate was approved but not used (CMake), the lead to confirm |
| `task-r4-editors-exports` (`-sim`, 2026-10-28) | Round 4's editors/exports findings (`work/findings/round4/r4-editors-exports.md`): the judge's board PNG back to front (X4-1); Schematic Plot's current page by its page number (X4-2); the Plot/Drill dialogs' stored settings and Export IPC-2581's BOM columns no undo step, kept through undo/redo (X4-3, X4-4: field-name choices, distributor guess, kept in `design.json` by `set_ipc2581_bom`); the Symbol Editor's Rotate pivot and Repeat Pin's selection (E4-1, E4-2); `${OP}` on a label on nothing (S4-1); the 3D wheel and the stackup's thicknesses (V4-1, V4-2); the Footprint Editor's drawing widths, Text tool, carried pad, messages, Paste Footprint, Renumber Pads, Push Pad's type filter (E4-F1..F7) | done | text bottom justification (no vertical justify in the text model); the IPC-2581 columns not in the `.kicad_pro`; a hierarchical label on nothing in `${OP}` |

Gates for any of them:
1. `cargo check --workspace --tests --keep-going`, with no warnings.
2. `tools/check.sh fast -p <crates touched>`: debug Rust tests, the web typecheck and the unit tests.
3. The slow kicad-cli proofs of the change: `EDA_SLOW_TESTS=1 KICAD_SRC_DIR=<kicad source> cargo test -p <crate> --test <name>`.
4. If the readers or writers changed, the 12-board re-export with a private `TMPDIR`: `EDA_SLOW_TESTS=1 EDA_PARITY_PARTS=reexport python3 tools/parity_report.py`.
5. The browser check: `web/studio/e2e/<name>.check.js`, pasted into a served page and run through `window.__eda`.

## Next tasks, in order (the user's decisions of 2026-10-23)

The 2026-10-21 list is done: Specctra, embedded files and the pad-details fix, the simulator, the rule editor, the router details, the schematic details, the KiCad 10 items, P20's browser checks, and the connectivity measure (each row under "What landed").

1. ~~**Finish `task-nanometres`**~~ merged 2026-10-24: lengths are KiCad's nanometres, design.json schema 2, old files migrated on read (one way). Connectivity 98.9%. See `docs/NANOMETRES.md`.
2. ~~**Fix the findings, batch by batch**~~ all six batches merged 2026-10-24/25 (see "What landed"); what each left open is in its findings file and GAPS.md. Original plan: (`docs/parity/findings/INDEX.md`). Each batch's findings carry our `file:line`, KiCad's function, a repro and the failing test to write. A–D can run in parallel, then E and F.
   - **A. KiCad compatibility of what we derive (first).** Today a studio project opened in KiCad and updated from its schematic (F8) renames every net and duplicates every footprint. **The user chose (2026-10-23) KiCad's own net names everywhere, the studio's display included**: `/sheet/NAME`, `/NAME` on a flat sheet, `Net-(REF-PadN)` unlabelled; global and power names unchanged. Also: one footprint name everywhere (the installed KiCad footprint), the footprint-to-symbol `(path ...)` link, symbol fields on footprints, `pinfunction`/`pintype`, the flipped footprint's frame. Prove it: kicad-cli's `--schematic-parity` DRC on the examples finds 0 differences (it finds 76 on mcu30 today).
   - **B. Board reader and writer losses:** zones with holes read as two zones, SMD pads on `*.Cu`, paste-only pads, curves with a control point on an end, footprint Edge.Cuts, rectangle corner radius, Extruded 3D Body, importers' `.kicad_dru`, a project's DRC/ERC exclusions.
   - **C. Teardrops:** Fill All and every refill before DRC or a plot wipe every teardrop's fill.
   - **D. Schematic connectivity:** buses joined by a shared member name, hidden power pins, `${#}` per placement, bus aliases, unit-0 pins, the ERC exclusion key on shared sheets.
   - **E. Router:** the walk-around detour, lost end vias, Walk-around via violations, Esc discarding fixed segments, the diff-pair gap, and the replay tests' macOS default path.
   - **F. Editing:** point edit and Mirror dropping items from groups, footprint bounding boxes, the carry's rotate pivot, Position Relative's angle, Recombine, Repeat Layout's nets and groups, design-block copper, the Net Inspector.
   - Then the mediums and lows, and the wrong doc claims listed at the end of `INDEX.md`.
   - Order as run (2026-10-24, the user: "do all"): A, B+C, E and F in parallel (agents' extra brief: `~/work/briefs/fix/common-fix.md`); D after A merges (both touch `crates/engine` naming); then the length newtype below.
3. ~~**The length newtype**~~ done 2026-10-25 in three waves (see "What landed" and `docs/NANOMETRES.md`). Original plan: (the user, 2026-10-24: do it): make `Nm` a newtype so the compiler catches unit mixing, with `mm()`/`mil()` test helpers and conversions only at the boundaries, done in waves by crate (model, then kicad/drc/engine, then ops/pns, then cli/studio). Run it last, on a quiet main, since it touches every crate.
4. **Next: leftovers named in the merges** (each in GAPS.md): ~~springback in the dragger and the pair placer~~ (done 2026-10-25, `crates/pns/PARITY.md` Stage 14), ~~a shared screen inside a shared screen~~ (done 2026-10-26, `web/studio/PARITY-sch.md` §42), ~~the Symbol Editor's pin and New Symbol dialogs~~ (done 2026-10-25, `web/studio/PARITY-symedit.md`; the Pin tool's click opens Pin Properties first and Ctrl+N opens the New Symbol dialog), ~~Both sides placement files~~ (done 2026-10-26; KiCad's `-all-pos`/`-top-pos`/`-bottom-pos` names and the single-file box, `kicad-engine::place_file_names`), the checks P20 left uncovered.

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

- `tune_edit::tests::the_tuner_on_the_boards_of_kicads_own_qa_never_makes_what_the_board_would_refuse` (eda-cli slow) failed once under load before wave 3 fixed its target (it aimed every track at ~3 mm, a nanometre leftover); it now tunes 34 tracks properly. If it fails again, look at it fresh.

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
