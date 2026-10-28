# agentic-eda vs KiCad: gaps and the plan for the next phase

Status on 2026-10-10, main at `89ff669`, KiCad source `8303b2ad` (10.99).

The first phase made the editors exist. The second (2026-10-07 to 10-10, 22 merges) made what exists behave like KiCad's: item editing and both
clipboards, Board Setup, DRC and ERC review, module sheets, the Properties panel, library choosers, footprints on the board as editable objects,
zone fill, the board outline, snap and grid, the Appearance panel, the router's shove and the 3D viewer. This file is the plan for the third: where
each gap stands now, the gaps the second phase found, a ranking by user impact, and 21 work packages (P1 to P21, the last section) with the files
each touches and the packages it collides with, so several agents can run at once. Verdicts are read from the code, not from the other parity docs;
where a doc and the code disagree, the code wins and the doc is named.

Item numbers are names: code comments and the PARITY docs cite "GAPS.md item 14" and "#8", so they never change and the rank is in the table below,
not in the numbers. Items 25 to 35 are new in this revision. The package names `WP1` to `WP6` of the first phase are kept at the end as history.

## Status

- **Actions wired: 586 of 766** (pcbnew 266/350, eeschema 170/233, common 150/183; `UI-ACTIONS.md`, 2026-10-10), up from 462 on 2026-10-07. The other 180 are
  recorded with a reason in `web/studio/tools/ui-parity-missing.json`; none is bare. **Every reason was re-checked against main on 2026-10-10
  (`STALE-REASONS.md`): 10 are stale (what they say is missing now exists), 18 are partly stale, 154 hold** (4 of those wait for a branch in
  flight; the library tables wired five reasons). The stale ones are Find by Properties, Migrate 3D Models, the microwave gap and stub, Append Board and its drop, the Footprint Editor's
  drop, Import Sheet, Draw Sheet from File and the schematic drop; the plan below carries them. (`task-files-in` wired Append Board, its drop and the Footprint Editor's drop, with Import Graphics for the board, the schematic and the Symbol Editor and a drop host: item 28.)
- **The original 30 gaps** (Appendix A; numbering kept because code comments cite "GAPS.md #8", "#20", "#6"): **13 closed, 9 partial, 0 open,
  8 out of scope** (was 10, 12, 0, 8 on 2026-10-07; #11 Property dialogs, #25 Align and distribute and, on `task-pcb-edit-details`, #14 Clipboard closed).
- **Items 1 to 24** are the ranked list of 2026-10-07 with their state brought forward; **items 25 to 35 are new**: the gaps the second phase
  found, from the `PARITY-*.md` "left" notes, the merge messages and reading the code.
- **In flight, not on main** (2026-10-11; read with `git diff main...<branch>`): `task-diff-pairs` (P5b), `task-files-in` (P8), `task-board-setup` (P9a), `task-sch-details` (P10).
  Do not start work that touches their files without reading them first.
- **Trapezoid, chamfered and custom pads are KiCad's (2026-10-10; `PARITY-pcb.md` section 27, `PARITY-fpedit.md` section 10).** A pad keeps its real shape in the IR (`Pad::ext`, `LibraryPad::custom`); the `.kicad_pcb` and `.kicad_mod`
  readers and writers are `pcb_io_kicad_sexpr`'s (kicad-cli's plot of the original and of our export is identical, for a board and for a library footprint); and the DRC, the routing gate, the router, connectivity and the zone filler
  measure that copper (`eda_drc::pad_shape`, `PadPiece`), so the gap to a trapezoid's slanted side, a chamfered corner or a custom pad's arm is within 2 um of kicad-cli's DRC. Pad Properties, on the board and in the footprint editor,
  takes KiCad's nine shape choices, a trapezoid's delta, a chamfer's ratio and corners and a custom pad's list of basic shapes (add, edit, delete: one undoable verb). Zone-fill parity (`crates/zone-filler/PARITY.md`, deleted 2026-10-13 with the filler; `git show 43f6c09:crates/zone-filler/PARITY.md`): `stonehenge`
  GND 11.33% to 0.15% of the area, `issue5093`'s `/right/GND` 776.56% to 1.49%; 198 of the 208 fills of the 17 QA boards are within 1% of kicad-cli's area and 189 within 0.5%. Left: a custom pad's proxy spokes, and the instant 3D scene
  (`eda_drc::pad_shape::pad_outline` and the state's `pads[].outline` are there for it); Explode and Recombine and padstacks came with P25 (below). Measured: 40 new Rust tests (five of them slow-tier, with kicad-cli) and 25 new
  `node --test` cases; `web/studio/e2e/pad-shapes.check.js` runs 6 scenarios through `window.__eda` on a served board.
- **Pad details: Edit Pad as Graphic Shapes, padstacks, Remove Unused Pads, Swap Pad Nets and Swap Gate Nets (2026-10-21, `task-pad-details`, not merged; `PARITY-pcb.md` sections 42 to 45).** (1) *Explode and Recombine* (`PAD_TOOL::EditPad`, the Footprint Editor's): a custom pad
  becomes its anchor plus free graphics on its layer, and Finish Pad Edit takes what touches the pad back (`Cmd::ExplodePad`, `Cmd::RecombinePad`, one undo step each). (2) *Padstacks* (`eda_model::padstack`): a pad has a shape of its own on the front, the inner and the back copper
  (`front_inner_back`, `custom`) and KiCad's `UNCONNECTED_LAYER_MODE`; the `(padstack ..)` of a `.kicad_pcb` and a `.kicad_mod` is read and written as `pcb_io_kicad_sexpr` does (a bottom-side footprint mirrored as `FlipLayers` does), the DRC gates, the router, connectivity,
  the canvas and the 3D scene read each layer's copper, and Pad Properties has Padstack mode, Edit layer and Copper layers; kicad-cli's plot of every copper layer of KiCad's own padstack board has the boxes the model has and our export plots as the original, and the re-export of the 12 QA boards
  is still 12 of 12. (3) *Remove Unused Pads* (`Cmd::RemoveUnusedPads`): KiCad's dialog and its two buttons set `remove_unused_layers` on the plated through-hole pads and the vias chosen; which layers keep their ring is the connectivity's (`eda_connectivity::flashed_layers`, `PAD::FlashLayer`)
  and reaches the state, the canvas and the routing gate. (4) *Swap Pad Nets and Swap Gate Nets* (`Cmd::SwapPadNets`, `Cmd::SwapGateNets`): the nets of the selected pads rotate in selection order with the tracks, vias and pads the copper reaches, KiCad's question about the pads
  nobody selected is asked (Ignore Unselected Pads, Swap All Connected Pads, Cancel), a footprint's `(units ..)` is read, written and derived from its symbol, and the context menu has Swap Gate Nets and "Swap with <unit>"; kicad-cli's `--schematic-parity` reports the swapped pads. Left: item 26 and item 31.
  Measured: 72 new Rust tests (five of them slow-tier, with kicad-cli) and 22 new `node --test` cases; the re-export of the 12 QA boards stays 12 of 12; `web/studio/e2e/pad-details.check.js` runs 10 scenarios through `window.__eda` on a served board (the mcu board, and a board of a 74HC00 for the gates).
- **A footprint carries its own graphics (2026-10-10, `task-fp-graphics`; `PARITY-pcb.md` section 28).** The silkscreen outline, the pin-1 mark, the fab body, the assembly drawings and the texts of a footprint (`fp_line`, `fp_arc`, `fp_circle`,
  `fp_rect`, `fp_poly`, `fp_curve`, `fp_text user`) are `Footprint::art` (`crates/model/src/fp_art.rs`), in the footprint's frame, so they move, turn and flip with it. The `.kicad_mod` and `.kicad_pcb` readers and writers are
  `pcb_io_kicad_sexpr`'s (a bottom-side footprint in the file's mirrored frame): kicad-cli's SVG plot of the silkscreen and fab layers of the original and of our export is the same, primitive for primitive, on six QA boards (24,090 primitives,
  `complex_hierarchy` among them), a synthetic board and a board made from `examples/opamp_filter.yaml`. The canvas draws them by layer under the Appearance rows, a click on a stroke picks the footprint, the 3D scene draws the silkscreen, the
  in-house board build reads them (a board made here and one imported are read the same way) and `drawings.footprint_extras` no longer holds a footprint's graphics or texts. A legacy library's through-hole pads on `F.SilkS` come back out too
  (`PadMaskInfo::art_layers`). The measure: `complex_hierarchy`'s `silk_over_copper` 104 -> 2 became 104 -> 104, `silk_overlap` 199 -> 5 became 199 -> 199, `silk_edge_clearance` 14 -> 2 became 14 -> 14; the re-export of a real board gives kicad-cli
  the original's counts on 7 of 12 boards (was 6). Left: hatch lines, the footprint editor's stroke types and vertical justification, the title block's variables in a footprint text, mask-only pads and a pad on the silkscreen of a library footprint.
  Measured: 33 new Rust tests (three of them slow-tier, with kicad-cli), 9 new `node --test` cases, and `web/studio/e2e/fp-graphics.check.js`, 6 scenarios through `window.__eda` on a served board.
- **A footprint's 3D models are an edit of their own (2026-10-13, `task-fp-3d`; `PARITY-pcb.md` section 31, `PARITY-fpedit.md` section 11).** `FootprintEdit::models3d` is the whole list on the board, with its verbs (`set_board_models`, `add_board_model`,
  `remove_board_model`, `move_board_model`, `edit_board_model`, each one undo step, a bad index or file name refused with its reason); the Footprint Properties dialogs of the board and of the Footprint Editor have KiCad's 3D Models tab (the list, the status of each file name,
  Add, the model chooser, Remove, Scale, Rotation, Offset and Opacity with their steps and the arithmetic of `evaluateTextCtrl`, the preview); the `.kicad_pcb` and `.kicad_mod` carry every `(model ...)` as `pcb_io_kicad_sexpr` writes it (kicad-cli's own rewrite of our board has the same
  entries character for character and exports the same VRML: slow-tier proof); the Footprint Chooser has the 3D view; Migrate 3D Models works (`pcbnew.GlobalEdit.migrate3DModels` leaves `ui-parity-missing.json`). Embedded files, `kicad-embed://` models and Collect and Embed 3D Models are done (2026-10-23, P26, `PARITY-pcb.md` section 47). Left: the Extruded 3D Body, the
  load-time auto-migration, "Show 3D viewer in own window" in the Footprint Chooser. Measured: 52 new Rust tests (one slow-tier) and 20 `node --test` cases; `web/studio/e2e/fp-models.check.js` runs 11 scenarios (153 checks) through `window.__eda`.
- **The microwave tools, and a schematic dropped on the sheet (2026-10-21, `task-microwave`, package P19's microwave half; `PARITY-pcb.md` section 41, `PARITY-sch.md` section 37).** Draw Microwave Gaps, Stubs, Arc Stubs, Polygonal Shapes and Lines are one verb family, `Cmd::Microwave` (`crates/ops/src/microwave.rs`,
  a port of `microwave_footprint.cpp`, `microwave_inductor.cpp` and `microwave_polygon.cpp`, the arithmetic KiCad's, in nanometres): each makes a footprint of its own on the board (`BoardPart`: no schematic part, no library, copper pads on `F.Cu` only, no courtyard, KiCad's default texts) as one undo step,
  after the prompts KiCad asks ("Gap Size:", "Stub Size:", "Arc Stub Radius Value:", "Angle in degrees:", the Complex Shape dialog with Normal, Symmetrical and Mirrored, its Size boxes and "Read Shape Description File...", "Length of track:" and "Component value:", and KiCad's four messages for a coil that cannot be made, said before the value is asked).
  kicad-cli loads each footprint, its `pcb upgrade` rewrite of the file has the same entries and its SVG plot has the pads, the pie slice's 19 points, the polygon, the coil's 12 mm and the flipped polygon where the formulas put them (slow tier). A `.kicad_sch` dropped on the sheet runs Import Sheet with it: the files of the drop are put beside each other on the server
  (`POST /api/sch/dropped_sheets`), the one that no other dropped file places is the file imported, and the sheets that were not dropped are named with the file that places them, in Choose Schematic and in a toast. Edit Footprint (Ctrl+E) on a footprint whose pads name no layer (these, and the model's own `0402`) no longer unmounts the page. Left: the footprint is put down at the first click
  (R and F act on it after, not on the cursor before the second click), the Size boxes count when typed in (KiCad's are overwritten with the file's scales), a second shape of one tool gets a number after the name (`muwave_gap_1`: the model finds a footprint by its name, KiCad's footprints are each their own), Specctra (the other half of P19; item 27's DSN boundary is untouched), one dropped schematic is imported at a time. Measured: 44 new Rust tests (one slow-tier), 11 new `node --test`
  cases and `web/studio/e2e/microwave.check.js`, 10 scenarios (121 checks) through `window.__eda`. The audit on the branch: pcbnew 314 of 350 handled (309 on main), eeschema 197 of 233 (196), common 163 of 183; the six rows are out of `ui-parity-missing.json` (`UI-ACTIONS.md` is left for the merge to regenerate).
- **Four findings change the picture.**
  1. *A wired action is not a working feature.* This was the finding of 2026-10-07 and it was fixed where it bit: the schematic edit tools act on
     every item kind (item 1, `2a8bf8f`) and so do the PCB ones (item 8, `88c8c0a`). Before closing any action, run it on every item kind.
  2. *The KiCad files we write drop design data, and kicad-cli judges those files.* The writer keeps zone settings, keepouts, dimensions, groups,
     locks, arcs, teardrops (`b145f0e`), the board outline as Edge.Cuts items (`a46fd29`) and the footprint's own graphics: part outline, pin-1 mark, fab body
     and texts, on the canvas, in the file and in the silkscreen kicad-cli plots (item 25, `task-fp-graphics`). **Since P23 (2026-10-19) the re-export of the 12 real QA boards is judged by kicad-cli as the originals are, finding by finding; what the files still drop is in item 2.**
  3. *One root sits under several gaps: the constraint model built from the intent has no edit verbs.* It now has an overlay for the rules
     (`drawings.rules`), for the nets (`design.nets`), for the libraries, for footprint fields, attributes and pad edits
     (`drawings.footprint_edits`), for the zone filler's pad and footprint settings (`drawings.zone_overrides`) and for footprints that exist only on
     the board (`drawings.board_parts`), and since 2026-10-12 for which footprint a part uses (`drawings.footprint_assignments`) and which reference a part
     has (`drawings.renamed_parts`): the schematic's Footprint field reaches the board, Change Footprint exists, and a reference rename carries the board's
     footprint, its pose and its routing (item 26).
  4. *Most earlier parity work was typechecked and unit-tested and never clicked.* Six areas now have a browser check driven through
     `window.__eda` (`web/studio/e2e/`: Appearance, DRC and ERC review, footprint edit and arrays, Properties panel, snap and grid, the 3D viewer);
     the schematic editors, Board Setup, the choosers, the router, the PCB edit tools, the zones and the hierarchy have none (item 33). **Since P20 (2026-10-23) 41 checks
     cover them, run together by `tools/run-all-checks.sh`; item 33 has the table by package and by PARITY section.**
- **Docs.** Action level: `UI-ACTIONS.md`, with `STALE-REASONS.md` beside it. Behaviour tables: `web/studio/PARITY-{pcb,sch,3d,fpedit,symedit,boardctl,common}.md`.
  Code level: `CODE-COMPARE-ui.md` (the first 266 handlers; about 310 added since have not been compared) and `CODE-COMPARE-router.md`,
  `crates/pns/PARITY.md`, `crates/zone-filler/PARITY.md` (deleted 2026-10-13: the zone fill is KiCad's; item 13). Measured round trip and connectivity: `REPORT.md` (the re-export part re-measured 2026-10-19, after P23: 12 of 12).
  Engines: `ARCHITECTURE.md`. The "Missing" lists below were re-read against the code where a merge touched them; a bullet no merge touched is
  carried forward from 2026-10-07 and says so.

## What moved since 2026-10-07

| item | what | state | merge |
|---|---|---|---|
| 1 | Schematic item editing: Move, Drag, Rotate, Mirror, Align, Properties for every kind | **Closed** | `2a8bf8f` (10-08) |
| 2 | KiCad files keep zone settings, keepouts, arcs, groups; the outline as Edge.Cuts items; the re-export of a real board is judged by kicad-cli as the original is (12 of 12 QA boards, 29 of 34 more) | Mostly done | `b145f0e` (10-08), `a46fd29` (10-09), `task-reexport` (10-19, P23) |
| 3 | Board Setup and the rules edit through an overlay | Mostly done | `b145f0e` (10-08) |
| 4 | Module sheets: editing on every sheet, nets traced across sheets | Partly (the rest in flight) | `08c70c8` (10-08) |
| 5 | Symbol Chooser and Footprint Chooser over the installed libraries; library tables, Manage Libraries, New and Add Library, Configure Paths | Mostly done (saving into a table library is left) | `80e972d` (10-08), `task-lib-tables` (10-10) |
| 6 | Schematic clipboard | **Closed** | `ee20bab` (10-08) |
| 7 | Router: shove near pads, the board edge, settings, via drag; segment and corner drags, routes from the middle of a track, pairs, length tuning; arcs, locks, mouse-trail posture, the time domain | Mostly done (the tuner's gaps are left) | `1e4d7ea`, `03b46de` (10-08), `task-router-drag` (10-10), `task-diff-pairs` (10-11), `task-length-tuning` (10-13), `task-router-rest` (10-20) |
| 8 | PCB edit tools and the clipboard for every item kind | Mostly done | `88c8c0a` (10-08) |
| 9 | Footprints on the board: fields, attributes, pads | Mostly done | `afcb91b` (10-09) |
| 10 | The Properties panel is KiCad's property grid | Mostly done | `bd712d3` (10-08) |
| 11 | DRC and ERC review workflow | Mostly closed | `02f383c` (10-08) |
| 12 | Schematic fields, labels, Autoplace Fields, De Morgan body styles | **Done** | `2d92851` (10-09) |
| 13 | Zone fill: spokes, hatch, smoothing, iterative refill; pad and footprint overrides | Partly | `eabaf91` (10-09) |
| 14 | Snap, grid and click versus drag | **Closed** (board, schematic) | `358606b` (10-09) |
| 16 | PCB context menu | Mostly done | `4c041a6` (10-09) |
| 17 | Groups, nested | Mostly done (board and sheet) | `4c041a6` (10-09), `task-sch-groups` (10-13, not merged) |
| 18 | Appearance panel | Mostly done | `6611ade` (10-09) |
| 21 | Find on the PCB | Mostly done | `4c041a6` (10-09) |
| 22 | Create Array | Mostly done | `afcb91b` (10-09) |
| 23 | The 3D viewer: KiCad's models, layer tree, stackup colours | **Done** | `89ff669` (10-10) |
| 25, 2 | Footprint graphics: silkscreen, fab and assembly drawings and texts are the footprint's own (IR, `.kicad_mod` and `.kicad_pcb`, canvas, 3D, in-house checks) | **Done** (limits in item 25) | `task-fp-graphics` (10-10, not merged) |
| 30 | The 3D models of a footprint: the 3D Models tab in both Footprint Properties dialogs, the list's verbs and file entries, the Footprint Chooser's 3D view, Migrate 3D Models | **Done** (limits in item 30) | `task-fp-3d` (10-13, not merged) |
| 32 | Design blocks: `.kicad_blocks` libraries and their table, the Design Blocks panes of the schematic and the board editor, Place (flat, as a sheet, linked), Save Sheet / Board / Selection, Update, Properties, Delete, a group's library link | **Done** (limits in item 32) | `task-design-blocks` (10-20, not merged) |
| 33 | A studio project survives KiCad's Update PCB from Schematic (F8): KiCad's net names everywhere (the user's decision of 10-23), one footprint name, the footprint's link to its symbol, the symbol's fields and the pads' pin types on the board, a flipped footprint in KiCad's frame (fix batch A; PARITY-pcb.md section 53) | Mostly done (a pin header keeps the built-in pads under the installed footprint's name; `_2` for two nets of one name, local power symbols, stacked pins, a non-plated pad with copper: section 53) | `task-fix-a-kicad-names` (10-24) |

Not moved: items 15, 19, 20, 24.

## Ranked list: open and partial items, by user impact

"Hit" is how often a KiCad user meets it; "Blocks" is whether a workflow stops. The package column is the plan at the end. The rank is the order of
the plan: a gap that stops a workflow every project meets is above one that costs a detail every session.

| rank | package | items | what the user meets | state | size |
|---|---|---|---|---|---|
| 1 | P1 Footprint graphics on the board | 25, 2 | no part outline, pin-1 mark or fab body on the canvas, in the file or in the silkscreen; an imported board loses them on export | done 2026-10-10 on `task-fp-graphics` (not merged); item 25 lists what is left | L |
| 2 | P2 Footprint assignment, exchange and reference rename | 26, 9 | the Footprint field of a symbol does nothing on the board; no Change Footprint; a rename leaves the footprint and its routing behind | done 2026-10-12 on `task-fp-assign` (not merged); item 26 lists what is left | L |
| 3 | P3 Library tables and paths | 5 | a symbol or footprint made in the editors cannot be saved into a table library's file; no Reset or Migrate Libraries; no 3D model search paths | mostly done | S |
| 4 | P4 Schematic hierarchy: the rest | 4 | Import Sheet, sheets in the clipboard, the last cross-sheet cases | in flight | M |
| 5 | P5 Router: drag, mid-segment, diff pairs, meanders | 7 | **P5a done on `task-router-drag` (2026-10-10)**: a segment slides, an illegal drag falls back, routes start and end mid-track, Route From Other End after the first fix. **P5b done on `task-diff-pairs` (2026-10-11)**: a pair is KiCad's, walked or shoved as one with its gap, with vias. **P5c done on `task-length-tuning` (2026-10-13)**: KiCad's meanders checked against its own code, its tuning tool for a track, a pair and a skew, tuning patterns as generator items written to the board file. **P5d done on `task-router-rest` (2026-10-20)**: arcs are router items (`ARC_T`: a route cuts one, `D` drags one, a shove keeps them), a locked track is neither dragged nor shoved, a head's via is shoved with it, the mouse trail sets the posture, Optimize entire track being dragged, Ctrl+E for a pair, the tuner's time domain. Left: the tuner's gaps (springback is done in all three placers: the line placer's with the rounded corners and virtual vias, `task-router-details`, 2026-10-23; the dragger's and the pair placer's, 2026-10-25) | P5a to P5d and the details done; item 7 lists what is left | S (what item 7 lists) |
| 6 | P6 PCB editing details | 8, 15, 17, 35 | Paste Special, rotation step, bounding boxes, a long tail of tool options | mostly done (merged 2026-10-10); items 8, 15, 17 and 35 list what is left | M, ongoing |
| 7 | P7 Custom-shape pads | 31, 13, 19 | custom pads import as their anchor; zone fill is wrong around them; trapezoid and chamfered pads drawn as rectangles | done 2026-10-10; item 31 lists what is left | L |
| 8 | P8 Files in: Append Board, DXF and SVG, drops | 28 | Import Netlist; a schematic or an image dropped on the sheet; the placement preview. (Another EDA's board and schematic come in since 2026-10-15: P22, `task-import-foreign`, not merged) | mostly done | S |
| 9 | P9 Board Setup completion and the rule editor | 3 | **P9a done on `task-board-setup` (2026-10-12)**: Text Variables, Board Editor Layers, Formatting, Zone Hatch Offsets, Length-tuning Patterns, Tuning Profiles, regex net patterns and composite classes, drawing tools from the Text & Graphics defaults. Board Data > Embedded Files done 2026-10-23 (P26). **P9b done on `task-rule-editor` (2026-10-23)**: the DRC Rule Editor, the rule-tree designer (`PARITY-pcb.md` section 48). Left: Component Classes; the rule editor's Show Matches and Syntax help | P9a and P9b done | S left |
| 10 | P10 Schematic details | 1, 12, 15, 17, 20 | a label's own fields (schematic groups, a user-defined field's place on the sheet and Pin Helpers done on `task-sch-groups`, 2026-10-13 to 15, not merged; done on `task-sch-details`, 2026-10-12, not merged: pins as items with Swap Pins, sheet pins, Auto-rotate, a text's justification, the net-collision overlay, the fields table's attribute columns, a power symbol's Properties, Copy as Text, the schematic's pan buttons and auto-pan) | mostly done (not merged) | S-M |
| 11 | P11 Search and inspection | 21, 29 | **done 2026-10-13 on `task-search-inspect` (not merged)**: Find by Properties (the From Selection tab and the Query tab on KiCad's expression language), Find following user fields, hidden fields and zone names, Clearance Resolution and Constraints Resolution (a rule engine in `crates/drc`: the implicit rules, the `.kicad_dru`, `EvalRules` with KiCad's report text), the Footprint Checker (kicad-cli's DRC on a scratch board). Items 21 and 29 list what is left | done (not merged) | M-L |
| 12 | P12 Board outline leftovers | 27 | placement and the DSN boundary ignore cutouts; NPTH holes missing from the 3D slab | open | M |
| 13 | P13 Zone leftovers | 13 | zone fill is KiCad's now (kicad-cli, `crates/zone-filler` deleted); non-copper and multi-layer zones, Auto-Assign Priorities, the Zone Manager preview, the zone lock | done 2026-10-13 on `task-zones` (not merged); item 13 lists what is left | L |
| 14 | P14 3D models of footprints | 30 | one model path in the editor; no model list, no preview, no Migrate 3D Models | done 2026-10-13 on `task-fp-3d` (not merged); item 30 lists what is left | M |
| 15 | P15 Review workflow leftovers | 11 | ERC exclusions never reach kicad-cli; no Delete Marker / Save report | done 2026-10-14 on `task-review` (not merged); item 11 lists what is left | S-M |
| 16 | P16 PCB item kinds the IR lacks | 24 | text box, tables (stackup table), barcode, point, reference image, hatch fill | tables done 2026-10-21 on `task-tables` (P16t, not merged); item 24 lists what is left of them; the other kinds deferred | L |
| 17 | P17 Library editor leftovers | 14, 19, 22 | snap to anchors in the editors, the pad array numbering, derived symbols, a fields grid in Footprint Properties, symbol fields on the canvas, alternate pin functions, the symbol text box, a shape properties dialog | **done** (merged); **reviewed 2026-10-23 on `task-p17-review`** (the round trip of KiCad's libraries, item 19); items 19 and 22 list what is left | L |
| 18 | P18 Design blocks | 32 | 23 actions; reusable blocks | **done 2026-10-20 on `task-design-blocks` (not merged)**; item 32 lists what is left | L |
| 19 | P19 Microwave tools and Specctra | 9, 27 | the five microwave tools (and a schematic dropped on the sheet); DSN export and SES import | **microwave half done 2026-10-21 on `task-microwave` (merged); Specctra (P19b) done 2026-10-23 on `task-specctra-wip` (merged)**: File > Export > Specctra DSN and File > Import > Specctra Session, `eda board export specctra` and `eda board import-ses` | S-M |
| 20 | P20 Browser checks for the older areas | 33 | most earlier work was never clicked | **2026-10-23 on `task-p20-browser-checks` (not merged)**: 41 checks run headless by `tools/run-all-checks.sh`, five new (the board and schematic editors' basics, the library editors' basics, the rest of Board Setup, plot and fab outputs), eight fixes found on the way; item 33 has the coverage table and what is left | standing | M each |
| 21 | P21 Simulator | 34 | `eeschema.Simulation.*`, `Simulator.*` | **done 2026-10-23 on `task-simulator` (not merged)**; item 34 lists what is left | XL |
| 22 | P24 The multichannel tool | 24, 32 | Generate Placement Rule Areas, Repeat Layout, Apply Design Block Layout; a placement rule area is a zone that names a sheet, a class or a group | **done 2026-10-21 on `task-multichannel` (not merged)**; items 24 and 32 list what is left | L |

## Items

"Port from" paths are under the KiCad source root. Each item says what is done (with the merge), what is still missing, and its package.

### 1. Schematic edit tools act on every item kind
**Closed 2026-10-08** (`2a8bf8f`; fields and labels `2d92851`); the details it left are done on `task-sch-details` (2026-10-12, not merged: P10). Hit: every schematic session.
- Round 2 (2026-10-27, `task-r2-schematic`, `PARITY-sch.md` section 44): a drawn wire is cut between a two-pin part's pins, a placed symbol cuts the wire it lands
  on, a dragged wire stays on its sheet pin, the clean-up keeps strokes apart, directive labels mirror across the axis, Align is rigid, Autoplace Fields skips
  hidden pins and treats each unit as a symbol. Since done (2026-10-28, `task-r2-leftovers`, `PARITY-sch.md` section 46): power symbols' Autoplace (C-M3),
  Align to Grid's kinds (C-L1), pivot boxes with fields (C-L2), a label named after its wire (C-L5), Paste's method and start (D-C3) and library precedence
  (D-C4), Duplicate's hover (D-C8), the Annotate dialog's enabling (D-A4); round 3: a deleted junction merges its wires, the commit-time `CleanUp` after
  Delete and placements, `J` breaks wires, `solid` and `default` strokes merge, the wire tool's terminal points (C3-H1, C3-M1 to M4). **Left**: C-L7 to C-L9,
  C3-L1 (Break and Repeat run `finishSegments`), C3-L3; a pasted sheet's contents use the default numbering.
- Round 4 (2026-10-28, `task-r4-schematic`, `PARITY-sch.md` section 47): a batch (Delete of a window over a T) cleans up once at its end (C4-H1); the label
  tool, Delete Sheet and Delete Sheet Pin clean up (C4-L1); a T's deleted dot merges the through wire (C4-L2); Align to Grid uses the selection's grid (C4-M1);
  a label on a pin has its net (C4-M2) and is a terminal point (C4-L3); a symbol on a shared screen is numbered per path by the project's method and page (D4-M1).
- Round 5 (2026-10-28, `task-r5-schematic`, `PARITY-sch.md` section 48): a batch deletes its junctions last (B5-H1), Delete Sheet / Sheet Pin merge through
  `DeleteJunction` (B5-L1); a wire finishes on any label or sheet pin anchor (B5-M1, r3 C3-M3 and r4 C4-L3 corrected); Find and Replace (selection only,
  Replace's target, escaped label text, label fields, body texts, reference chains, emptying a label, the end and wrap, KiCad's boxes: C5-M1, M2, M4, L1-L7);
  the Symbol Fields Table per sheet path, `R01`, empty edits, the preview's variables (C5-M3, L8, L9 in part); DNP / Exclude across sheets (D5-M2, server side)
  and Increment Annotations over all sheets (D5-M3). **Left**: the Fields Table's Description column and Save / Discard / Cancel close question (C5-L9), the
  Find dialog's Regular Expression box, the studio's DNP state from the sheet in view (D5-M2).
- Done (`crates/ops/src/{sch_move,sch_drag,sch_scene,sch_props}.rs`, `kicad-port/{schMove,schAlign,schProperties}.ts`, `components/{SchematicView,SchPropertiesDialogs}.tsx`):
  one verb family, `Cmd::SchMove`, moves, drags, turns, mirrors and aligns any set of symbols, power symbols, wires and bus wires, labels of every kind,
  free text, text boxes, shapes, rule areas, directive labels, junctions, no-connects, bus entries, graphic lines and sheets (with their pins), as one
  undo step on the sheet in view. Move leaves the wires where they are; Drag stretches the attached wires and adds the segments KiCad adds (right-angle
  bends, the stub at an unselected junction, label and sheet-pin special cases), then does KiCad's finishing (junctions, trimming, merging, dangling
  segments); Rotate and Mirror use KiCad's turn point; R, Shift+R, X and Y work while items are held, and the view shows what the server computes for the
  same command (`POST /api/sch/move_preview`). A plain click on a wire selects it, a click-drag works on any item, Align and Align to Grid move each item's
  wires with it. Properties (`E`, a double-click) opens the dialog of the kind (label, text, sheet, wires / buses / bus entries / graphic lines / junctions,
  shapes, rule areas, text boxes, directive labels); strokes and junction looks are drawn and written to the `.kicad_sch` as KiCad writes them, and read back.
- Measured: 49 Rust tests for the move family, 17 for Properties, 4 in `board.rs` (each verb is one undo step, the nets untouched), 19 `node --test` cases;
  clicked through on a copy of mcu30. Browser checks since P20: `sch-edit`, `sch-groups` (move, turn, mirror as a group), `sch-details` (item 33).
- Done 2026-10-12 (`task-sch-details`; `PARITY-sch.md` sections 21 to 26): a symbol's pins are items (hover, click, box, the Pins filter, the summary; the net highlight and Select Node start from one) with Swap Pins, Swap Pin Labels and Swap Unit Labels
  (`SchExtras::pin_swaps`, three `SchEdit` verbs, one undo step each); a sheet's pins are items one by one (moved along the border, Sheet Pin Properties, Delete, Alt+S swap; the hierarchy's nets follow); a global or hierarchical label with Auto rotates
  itself when it is placed, edited in Label Properties, moved and dragged (`AutoRotateItem`); a free text has a justification (`SchExtras::text_justify`) that is written, read back and flipped by Rotate and Mirror as `SCH_TEXT` flips it;
  the net-collision overlay of a Move or Drag (`SCH_DRAG_NET_COLLISION_MONITOR`: circles at the junctions the held items would make between two nets, marks where a connection is pulled apart, the warning cursor); the Symbol Fields Table's attribute
  columns (DNP, Exclude from BOM, board, simulation), Include 'DNP' / 'Exclude from BOM' symbols and KiCad's four built-in views; a power symbol's Symbol Properties (Reference, Value as the net, Show checks); Copy as Text.
- Done 2026-10-13 (`task-sch-groups`, P10b; `PARITY-sch.md` section 29): the sheet's groups (item 17): the selection tool picks the outermost group of a member, enters and leaves it, boxes take a group whole or by any member, and Move, Drag, Rotate, Mirror, Lock, Delete, Copy, Paste and Duplicate take the tree.
- Done 2026-10-13 (`task-sch-groups`, P10b; `PARITY-sch.md` section 30): a symbol's user-defined fields (`SchematicSection::user_fields`) are fields of the sheet after the four standard ones: hidden until shown, placed by Autoplace Fields with the others, picked, moved, turned, mirrored, edited in Field Properties, hidden with Delete, added, shown and removed in Symbol Properties, kept or dropped with their places by the Symbol Fields Table, written as `(property ..)` where the sheet keeps them and read back by `import_kicad_sch` (text, look, place and hide, for every turn and mirror).
- Done 2026-10-15 (`task-sch-groups`, P10b; `PARITY-sch.md` section 31): Pin Helpers (`PIN_TRICKS_MENU`): the submenu of a selection of pins and sheet pins with Wire (a stub one wire-grid step out of the pin, held by the Drag tool: a click drops it, Escape takes it away), No Connect and a Net, Hierarchical and Global Label named by the pin, reading away from it, in the shape of its type (`SchCmd::PinHelper`); and swapped pins
  through a file and the clipboard: the reader reads a symbol's `lib_name` copy of its library symbol as the places traded, Copy writes it, Paste and Duplicate keep the swap under the new reference (kicad-cli reads the clipboard with the wires on the pins they were on).
- Measured on `task-sch-details`: 76 new Rust tests (ops 62: pins 13, sheet pins 8, labels and text 26, net collision 9, fields table 6; kicad 5, one slow with kicad-cli; model 4; cli 5) and about 50 new `node --test` cases; the kicad-cli proof
  (`crates/kicad/tests/sch_details.rs`, `EDA_SLOW_TESTS=1`: a file with swapped pins, a sheet pin moved to the top edge and justified texts loads, its ERC reports the sheet pin at the moved place, and `kicad-cli sch upgrade` writes the justification,
  the angle, the pin and the cached copy of the symbol back as we wrote them); `web/studio/e2e/sch-details.check.js` (nine scenarios through `window.__eda`, 72 checks, every scenario undoes itself).
- Missing: a sheet's border and fill; a sheet pin's font face and colour have no place in the IR (its size, bold and italic do since 2026-10-15); a power symbol's Properties has no footprint, datasheet, exclusion flags or
  pin table; Autoplace Fields does not read the swapped pin places; the Symbol Fields Table has no variants and no scope selector; symbols this project drew (not imported) and turned or mirrored place their pins by
  `eda_engine::placed` (older note, not re-checked since the overlap fix, `f24fa31`, which made generated symbols draw their pins as KiCad's do).
- Port from: `eeschema/tools/sch_move_tool.cpp`, `sch_edit_tool.cpp`, `sch_selection_tool.cpp`, `sch_drag_net_collision.cpp`, `sch_align_tool.cpp`.

### 2. The KiCad files we write drop design data
**Mostly done (2026-10-08, 10-09, 10-10, 10-19).** Hit: any board with a pour, keepout or dimension; every export; every imported board. Blocks: no longer for keepouts. The footprint's own graphics (P1) were the biggest of what was left and are done. P23 (2026-10-19) closed the measured gap: the re-export of the 12 QA boards is judged by kicad-cli as the originals are (below); what the files still drop changes no DRC count of them.
- Done (round 6, `task-r6-board-io`, 2026-10-28; `PARITY-pcb.md` section 64): a legacy board's `(setup ..)` and `(setup (defaults ..))` are its design settings (the copper edge clearance, the Default class's sizes, the minimums, the Text & Graphics defaults, the pre-defined sizes, the Dimensions defaults) and the project's `board.design_settings` is then ignored, as `BOARD::SetProject` ignores it; a board's `(net_class ..)` blocks replace the project's classes; the project's Default class keeps its microvia and differential-pair sizes; a project's out-of-range rule value is KiCad's default; KiCad 5 `(module ..)` library files resolve a part's footprint. Left: the legacy `zone_clearance` and pad master, pcbnew's GUI-only inference of a legacy edge clearance, a Default class's via gap in a legacy block.
- Done (round 5, `task-r5-board-io`, 2026-10-28; `PARITY-pcb.md` section 61): a board or footprint file older than 20210606 has its `~X~` overbars read in the new `~{X}` notation (net names, net classes, pad nets, every text with `(effects ..)`: `crates/kicad/src/overbar.rs`), and an outline font's bold text keeps its thickness in a 20260826+ file (only the KiCad font's is widened back, `MigrateLegacyBoldStrokeWidth`).
- Done 2026-10-13 (`crates/kicad/src/fp_transform.rs`): boards in KiCad 10's format (file version 20260616 on, `FIRST_FP_AFFINE_TRANSFORM`) are read. A footprint's `(transform (translate x y) (rotate a) (scale sx sy))` stands for the `(at x y a)` older files have, and a footprint's own zones, stored in its library frame, are put back in the board frame (`TRANSFORM_TRS::Apply`). Pads and texts kept their meaning. Proof: four QA boards upgraded by kicad-cli 10.99.0-5334 read the same as the files they came from (`crates/kicad/tests/kicad10_transform.rs`, slow). A footprint scale other than 1:1 has no place in the model: it is read at 1:1 and named in `ImportNotes::footprints_scaled`. Bold text as a stroke-width multiplier (20260826) is read since P23 (below). Line ending shapes (20260818), grid items (20260728), via stitching and guarding (20260816) and drill charts, drill maps and the drill symbol profile (20260901) are read, written, drawn and edited since 2026-10-23 (branch `task-kicad10-items`, PARITY-pcb.md section 50), ported from KiCad 25ec2335, the commit of the box's kicad-cli: its QA corpus has boards with each but the grid item (a fixture kicad-cli saved stands in). Not read yet: microvia stack generators (20260830) and custom user properties (20260831); a generator of a kind nobody reads is kept as text (since 2026-10-27, `task-r2-misc-leftovers`, with its members by our ids, `DrawingsSection::raw_generators`: before, its `(members ..)` named uuids the written file no longer had and KiCad dropped it). Left of the four: grid items as routing and placement roles, the Drill Chart Properties and Drill Groups dialogs, a footprint's fields on copper as via-stitch obstacles (a via's backdrills and post-machining, the board's layer names and imperial holes are in the chart since round 6, `task-r6-board-io`, PARITY-pcb.md section 64). Reviewed against 25ec2335's code on `task-kicad10-review` (2026-10-23, PARITY-pcb.md section 50 "Review"): 23 divergences fixed, among them the drill chart's rebuild seeing the board before the command, staleness by the drill generation, the via-stitch grid re-anchoring on a new pitch or layout, Fill All not regenerating stitchings, and copper text not blocking vias.
- Done (`b145f0e`; `crates/kicad/src/{pcb,pcb_items,import_items,import}.rs`): a rule area is written as a keepout zone (before, it failed the whole export with
  `kicad.unknown_net`); every zone writes its own clearance, minimum width, priority, pad connection, thermal gap and spoke, hatch, island and fill settings;
  arc tracks are arcs; locks, dimensions, groups, zone names and teardrop flags are written and read back (round-trip tests in `pcb.rs`). **The board outline
  (`a46fd29`, 10-09)**: Edge.Cuts come back as the items they were (`gr_arc`, `gr_circle`, `gr_rect`, `gr_poly`, `gr_curve`, `gr_line`), a malformed outline stays
  malformed in the export, a footprint's own Edge.Cuts become board-level shapes, KiCad 6's `(gr_arc (start CENTRE) (end START) (angle A))` is read.
- Done (`task-fp-graphics`, 10-10; item 25): **a footprint's graphics and texts** (`fp_line`, `fp_circle`, `fp_text` ...: 512 `fp_line`s on `complex_hierarchy`) are the footprint's own and come back out, and so does a pad's
  silkscreen layer: `silk_over_copper` 104 -> 104 (was 2), `silk_overlap` 199 -> 199 (was 5), `silk_edge_clearance` 14 -> 14 (was 2).
- Done (P23, `task-reexport`, 2026-10-19; `PARITY-pcb.md` section 37): **the re-export of a real board is judged by kicad-cli as the original is, on 12 of 12 boards** (100%, was 7 of 12; `scores.json`'s floor is raised to it) --
  the same violations type by type and the same findings one by one (type, severity, items and places to a few micrometres: `REPORT.md` section 2), both for the boards as the corpus ships them and for the same boards as kicad-cli 10.99.0-5334
  writes them in its newest format (`pcb upgrade --force`). Found by diffing the two reports and the two files (`kicad-cli pcb diff` needs the same uuids, ours are not, so a small structural diff of both files after kicad-cli's own rewrite did the
  work), fixed one at a time in the reader or the writer: a text of several lines came back as one line full of backslashes (the reader did not take `DSNLEXER`'s escapes, the writer did not write `OUTPUTFORMATTER::Quotes`'): 5 `clearance`,
  2 `shorting_items`, 3 `solder_mask_bridge`, a `copper_edge_clearance` and an unconnected item on `complex_hierarchy`; a text's glyph width, vertical justification, bold, italic, knockout, line spacing, face and automatic stroke width were not kept
  (`DrawingsSection::text_extras`; `connection_width_rules`' labels sat half a line off their anchors); pads on one point with one number shared a uuid (8 `clearance`); a footprint with no library link was written as `eda:<name>` (72 + 2
  `lib_footprint_issues`: `FootprintExtra::unlinked_name`) and one with no courtyard got a made-up one (`courtyards_overlap`: `no_courtyard`); the outline's lines lost their width (`outline_width_um`); a pad's pin type was dropped (free pads);
  component classes were not in the IR (`BoardRules::component_classes`, `FootprintExtra::component_classes`, with the footprint's sheet) and neither were barcodes (`DrawingsSection::barcodes`); and the harness did not read the project's Board
  Setup pages as `eda import-kicad` does (the tuning profiles). KiCad 10's format: a bold text's thickness is the width before the bold factor 1.6 (read), and a net only copper names is a net of the board (it failed the export).
  **34 more boards** (17 of the templates KiCad installs, with its libraries in reach, and 17 more QA boards) were measured the same way to find what the 12 do not show: **29 of them are identical** too, and the work they asked for is in: a pad's paste,
  mask and edge connector type, pin function and other tokens, a via's type, tenting and padstack (`PadMaskInfo`, `ViaExtra`: the file had a tenting token on 10370 vias of the QA corpus and `api_kitchen_sink`'s untented vias came back tented), a hidden footprint text
  (a MountingHole's `${REFERENCE}`), a footprint's description, tags, paste margins, uuid and courtyard graphics as the file had them (a circle or four lines are not one polygon to `FOOTPRINT::FootprintNeedsUpdate`: every mounting hole was
  `lib_footprint_mismatch`), the nets a schematic's net class labels assign (`netclass_assignments`: `time_calculations`' skew rule judged nothing), the board's text boxes, images and tables as text (`DrawingsSection::raw_items`, named by
  `ImportNotes::unread_items`), a through-hole pad with no drill (the export panicked on 155 of them on one board). Every board of the QA corpus (223 that import) exports a file that imports again and that kicad-cli loads
  (`corpus_reexport_parses.rs`, slow tier). The five that still differ have a cause each (`REPORT.md` section 2): integer micrometres on two inch boards (`issue5830`, `issue9870`), the order of a net's tracks in the file (`time_calculations`: another
  segment of the same net is named), and two boards with no project file, whose original kicad-cli judges by its default severities (`issue4257`, `net_tie_drc`).
- Done (round 2, `task-r2-board-io`, 2026-10-27; `PARITY-pcb.md` section 54; `work/findings/round2/r2-board-io.md`): a masked track or copper shape keeps its layer and its mask opening, a through-hole or edge connector pad its paste and one-sided mask, a mask-only hole its margin and layers, a footprint its `allow_soldermask_bridges` and its want of `(attr ..)`, a legacy footprint's `ki_description` becomes its Description, a legacy `viasonmask` untents the vias, an empty Reference stays empty, a zone's filled flag is its own, and the file version follows what the board holds (20250228 for KiCad 10's via protection defaults, else 20241229; a reference image's scale across 20260623). Measured with `tools/board-roundtrip/run.sh` on 214 QA boards. Left of that review: F7/F14 (groups) and F11 (plot settings) on other branches; F15, part of F17, a `thieving` zone (unreadable in a 20241229 file).
- Missing: what no DRC count of these boards shows but a plot or a diff of the files does: a footprint's jumper pad groups and its Datasheet and Description fields are text and not fields (a legacy file's `ki_*` are made the fields KiCad makes of them), the link to its symbol (`path`: not written on purpose, the derived
  schematic has symbols of its own), a pad's teardrop settings, a property's `unlocked`, a shape's line style (dashes, dots: 3 of the corpus's 224 boards), a zone outline's hatch pitch, dimension details, reference images and text boxes (carried as text, drawn by
  kicad-cli's plots, not by the canvas, no editor: item 24, P16); the coordinates are held in micrometres, so an inch board's tenths of a micrometre are rounded (the findings agree to 5 micrometres, the descriptions differ in the fourth decimal, and a
  clearance a tenth of a micrometre short turns into a violation: `issue5830`); barcodes and component classes are carried but have no editor, no verb and no drawing (P16, P9). `scores.json` was last measured before the outline and footprint work (10-08)
  except for the re-export part.
- Port from: `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` and `pcb_io_kicad_sexpr_parser.cpp`.

- 2026-10-27 (`task-r2-misc-leftovers`, PARITY-pcb.md section 56): the round-3 board-file findings (r3-board-io F1-F10, F12, most of F13) and round 2's F15 and F17: a bare `(net 0)` is no net, copper thieving zones (the board then written as 20260513), placement areas' keepout defaults, a third via's uuid, legacy group locks, `via_stack` members, no made-up 3D model, zone outline arcs and per-layer hatch offsets, teardrop border styles, legacy fields where KiCad puts them, one `(sheetfile)`, a padless footprint's zone connection, the file's technical layers, a legacy board's layer names. Round trip of the 214 QA boards: 35922 -> 32362 lines that differ, 8 -> 15 identical, 67 better, 1 worse (`via_stacks`, member uuids now present), `plugins/fakeboard` still fails to import. Left: an empty footprint `(embedded_files)`, the stackup `addsublayer`, a circle pad's `rect_delta`, the Edge.Cuts loop's direction (same geometry), the plot settings' Rescue bit (r3 F11, task-r3-editors-exports).

### 3. Board Setup and the rules
**Mostly done (2026-10-12; P9a).** Hit: the start of every project. Blocks: no. Package P9.
- Done (`b145f0e`): `eda_model::rules::RulesOverlay` (`crates/model/src/rules.rs`) holds one optional page per Board Setup page in `design.json` (`drawings.rules`);
  `board::load` lays it over the intent's (or the imported project's) rules, as `design.nets` is, so the router, the gates, the studio and the derived `.kicad_pro`,
  `.kicad_pcb` and `.kicad_dru` that kicad-cli reads all follow an edit. Seven undoable verbs (`set_net_classes`, `set_constraints`, `set_mask_paste`,
  `set_text_graphics_defaults`, `set_stackup`, `set_rule_severities`, `set_custom_rules`; `crates/ops/src/board_setup.rs`) refuse a bad page with the setting's name.
  `components/BoardSetupDialog.tsx` is KiCad's page tree and all ten pages edit: Physical Stackup (a layer's `(color ...)` too, since 10-09), Solder Mask/Paste,
  Defaults, Dimensions, Constraints, Pre-defined Sizes, Teardrops, Net Classes (with the patterns and the nets each matches), Custom Rules (as text, "Check rule syntax"
  through kicad-cli: `POST /api/check_rules`) and Violation Severity (KiCad's 64 checks). Assign Netclass works from the board and the schematic.
- Done (P9a, `task-board-setup`, 2026-10-12; `web/studio/PARITY-pcb.md` section 9b has the detail and the proofs): six more verbs and pages -- **Text Variables** (`set_text_variables`; `${NAME}` is
  expanded by `eda_model::text_vars` in board texts, footprint fields, schematic texts, symbol fields and the title block, the state carries the shown form, the derived `.kicad_pro` carries the
  variables so kicad-cli's plots show the same: proved), **Board Editor Layers** (`set_board_layers`; the technical and user layers, their names and types; the board file's `(layers ..)` is
  KiCad's and the importer reads it; the layer pickers, the Appearance panel and `${LAYER}` follow; an item cannot be put on a layer that is off nor a layer with items turned off; proved with
  kicad-cli), **Formatting** (dash and gap ratios in the plot parameters, the footprint boxes in the project and applied when a footprint is put on the board), **Zone Hatch Offsets** (the filler
  moves the hatch pattern; written as KiCad writes it), **Length-tuning Patterns** (the length tuner starts from them) and **Tuning Profiles** (with the profile each net class names;
  kicad-cli reports `missing_tuning_profile`: proved); the drawing tools start from the Text & Graphics defaults of the layer's class (lines, shapes, text, dimensions; `incWidth`/`decWidth`; the CLI too);
  and **regular-expression net patterns and composite net classes** (`eda_model::net_pattern`, `BoardRules::effective_class`: a net is in every class that matches it, the class higher in the list
  winning where several set a size and Default giving the rest; a net in two classes makes the derived project carry the classes, and kicad-cli agrees: proved). Thirteen verbs now in
  `crates/ops/src/board_setup.rs`'s family, 79 browser checks in `web/studio/e2e/board-setup-pages.check.js`.
- Missing: the **Component Classes** page (nothing in the model holds them; the `component_classes_drc` difference of the QA corpus stays; **Embedded Files** is done, P26, `PARITY-pcb.md` section 47); in the DRC Rule Editor
  (`DRETool.drcRuleEditor`, the rule-tree designer, done on `task-rule-editor`, P9b, `PARITY-pcb.md` section 48): Show Matches (`GetItemsMatchingRule`), the Syntax help window, auto-completion and dragging in the tree; bus notation in net patterns (`D[0..7]`) and the net class labels of the schematic; the
  transmission-line calculator of the Tuning Profiles grid; the tuner's corner style and single-sided settings (kept and written, not used by this app's meander generator); the Net Classes grid's own
  Tuning Profile column (the class's profile is chosen on the Tuning Profiles page); zone-level hatch offsets (the board's per-layer offset is what the filler reads; KiCad itself loses the board's
  when it saves, see section 9b); Formatting's dimension and barcode boxes (library footprints here hold neither) and, for a footprint the model already knows, the art of its library (only the copy
  of a footprint the board keeps is restyled); in Text Variables: `@{}` math, `VCSHASH`, schematic labels, text boxes and power symbol fields, `PIN_NAME(n)`, library footprint texts and text pasted
  from the clipboard; dielectric sub-layers and impedance control on the stackup.
- Done (round 6, `task-r6-board-io`, 2026-10-28; `PARITY-pcb.md` section 64): the Dimensions page's defaults come from the project and go back to it (`defaults.dimension_units`, `dimension_precision`, `dimensions.*`, the schema-0 precision migrated); the length tuner reads every `(constraint ..)` of a `.kicad_dru` rule and `(layer outer)` / `(layer inner)`.
- Port from: `pcbnew/dialogs/dialog_board_setup.cpp` and `panel_setup_*.cpp`, `common/dialogs/panel_setup_netclasses.cpp`, `panel_setup_severities.cpp`,
  `panel_text_variables.cpp`, `pcbnew/drc/drc_rule_parser.cpp`, `pcbnew/tools/drc_rule_editor_tool.cpp`.

- Round 6 (`task-r6-router`, 2026-10-28, `web/studio/PARITY-pcb.md` section 63): `A.Orientation` and `A.Locked` on the rule board are KiCad's; Multichannel and Find by Properties' Query read the rule board's component classes and sheets; a class rule KiCad cannot compile leaves none; Violation Severity lists the checks of the box kicad-cli (25ec2335); Custom Rules' Check rule syntax is `OnCompile`'s reporter; the spoke count field is KiCad's 0 .. 10; a project's tuning-pattern groups are read as KiCad reads them.

### 4. Hierarchical sheets
**Mostly done (2026-10-10, `task-sch-hierarchy`)**, limits below. Hit: every multi-sheet design. Package P4 for what is left.
- Round 2 (2026-10-27, `task-r2-schematic`, `PARITY-sch.md` section 44): `eda import-kicad` of a shared screen keeps each sheet path's references; sheets in a
  subfolder are read, keyed and written as KiCad resolves them; a KiCad 6 root's `sheet_instances` pages; a sheet's user fields; the tracer is KiCad 25ec2335's
  connectivity engine (the box's kicad-cli). The QA trace test compares net names through the real import: 24 of 24 alike (11 before). **Left** (findings
  `docs/parity/findings/round2/r2-schematic.md`): a pasted sheet's references from the copied path (B-M5), a sheet's DNP / BOM / board flags and look (B-L1),
  symbol uuids (B-L3), the rest of the writer's cosmetics (B-L4: a corner sheet pin's side, the provenance title block, the root instance and embedded fonts in
  every file), an unannotated reference's uuid hash (A-N13). Done 2026-10-28 (`task-r2-leftovers`, section 46): the pages of a partly paged file (B-M2), a unit
  per path (B-M4), `in_pos_files` (B-L2), local power symbols (QA issue22938), a common pin per unit, a bus sheet pin's members by `drivesRecord`, a `#`
  reference's claim, which of two same-named nets keeps the bare name by claim (A3-H1, H2, M1 to M3), and an undriven net's KiCad name after an edit (A3-H3).
  Done 2026-10-28 (`task-r4-schematic`, section 47): local power from the library symbol, so a placed or pasted one stays local (A4-H1); jumper pin groups and
  duplicate pins as jumpers (A4-H2), two pins of one number (A4-H3), stacked numbers (A4-H4), no-connect pins (A4-M1), bus wire entries (A4-M2), all alike
  in kicad-cli; a symbol cached under its `lib_name` (B4-H1); a unit per path and Exclude from position files follow the symbol (A4-M3, B4-M2); a sheet above
  the root's folder saves as a design block and a copy (B4-M1). Left: D4-L1 (the root's page), D4-L2 (a power symbol's flags), D4-L5 (Paste's per-screen cache).
  Done 2026-10-28 (`task-r5-schematic`, section 48): a KiCad 6 / early-7 root's `symbol_instances` give every symbol, the root's included, its reference,
  value and footprint (D5-H1, 11 QA roots); a root symbol's own `/<root>` instance names it (D5-H2); a directive label's "Net Class" field (D5-H4); a placed
  pin's alternate function read, kept, written and traced by its type (D5-H3); one pin number in two units (A5-H1); two bus entries end to end (A5-M1); a De
  Morgan placement's jumper groups (A5-M2); Delete's junctions last (B5-H1, B5-L1); a stacked pin's net in the studio (B5-M2); a pasted shared sheet's units
  (B5-L2). KiCad's 130 QA roots through `eda import-kicad`: 108 alike before, 123 after. Left: D5-M1 (a differing `lib_name` copy: one library symbol
  per id, `legacy_power`, `same_local_global_power`), A5-L1 (a shared screen's twin pin net names), a library symbol's user fields in the cache (`issue22286`'s
  `${LOCATION}`), the per-path units of screens copied whole (`add_sheet_tree`).
- Done earlier: the IR (`SchematicSection::sheets`, `Design::sheet_contents` in `crates/model/src/ir.rs`); tree import and multi-file export
  (`crates/kicad/src/sch_import.rs::import_kicad_sch_tree`, `lib.rs`); one derived schematic as module sheets (`crates/engine/src/hier/`,
  Tools > Reorganize into Module Sheets); every schematic command acts on the sheet in view (`Cmd::OnSheet`); the nets traced across sheets
  (`crates/engine/src/nets.rs`, `reconcile_schematic`); the Hierarchy panel, Back/Forward/Up, Draw Sheet, sheet pins and their sync, page numbers
  (`PARITY-sch.md` section 14); ERC per sheet through kicad-cli.
- Done 2026-10-10 (`PARITY-sch.md` sections 5, 8, 14, 15, 19): **Find and Replace over every sheet** (the sheets in page order, the match's sheet shown,
  Replace All one undo step; "Search the current sheet only" off by default); **Annotate over the whole schematic** (`DIALOG_ANNOTATE` in full: scope,
  recursion, X or Y order, reset or keep, regroup units, first free / sheet x 100 / x 1000, multi-unit parts, the messages and `CheckAnnotation`'s findings; a port of
  `SCH_REFERENCE_LIST` and `REFDES_TRACKER`, `crates/model/src/annotate.rs`); **Import Sheet and Draw Sheet from File** (`LoadSheetFromFile` and its checks: loops,
  symbols with no library link, references the design holds, missing libraries; as a sheet with its own file or as contents on the sheet in view; the symbols numbered by the
  Annotate settings or kept); **Copy, Cut and Paste of whole sheets** (a copy of each screen under a file of its own, a name that counts up, library symbols merged, symbols numbered unique across
  every sheet); **Reorganize into Module Sheets** keeps the places and looks of fields, the looks of labels and the body styles and takes multi-unit symbols (item 12); **Lint and
  the SVG render walk every sheet** and name the sheet a finding is on. kicad-cli's ERC after each (`EDA_SLOW_TESTS=1`): no `unannotated`, `duplicate_reference` or `duplicate_sheet_names` finding after Import
  Sheet, a sheet paste and Annotate over the whole schematic; none at all after Reorganize, with and without a multi-unit part; kicad-cli reads a copy of sheets beside their screens.
- Done 2026-10-21 (`task-microwave`, package P19; `PARITY-sch.md` section 37): **a schematic dropped on the sheet** (`eeschema.EditorControl.ddAppendFile`, `SCH_EDIT_TOOL::DdAppendFile`). A browser gives the page each file's text and no folder, so every `.kicad_sch` of the drop is posted (`POST /api/sch/dropped_sheets`, `crates/cli/src/sheet_drop_api.rs`) and put in a scratch folder of the
  server the way the sheets' own placements say (a sheet is found by its `Sheetfile` name, in either case, relative to the file that places it); the file no other dropped file places is the one imported (whatever the order of the drop), Choose Schematic opens on it with `LoadSheetFromFile`'s checks and Import Sheet's remembered options, and a sheet that was placed and not dropped is named with the file
  that places it, in the chooser's checks (`eda_kicad::load_sheet_file` says "The sheet file 'X' that 'Y' places was not found: its sheet comes in empty.") and in a toast; Open arms the placement tool as for a file picked in the chooser. The drop takes every `.kicad_sch` (a drop of other kinds still takes the first).
- Done 2026-10-23 (`task-sch-details`, P10; `PARITY-sch.md` section 39): **a screen shown by two sheets**. A sheet pasted or duplicated while the design shows its file shares that screen
  (`SCH_EDITOR_CONTROL::Paste`: `SetScreen( existingScreen )`), its symbols numbered for the new placement (`updatePastedSymbol`, `ReannotateDuplicates`) and kept in `instance_overrides`;
  the writer gives each symbol a reference per path, the model a part per reference, the tracer a net per placement (`eda_engine::nets`, `trace_design`), the studio the placement's references.
  kicad-cli's netlist of the two placements is the studio's net list (`slow_a_pasted_sheet_shares_its_screen_and_kicad_cli_makes_a_net_per_placement`); a PWR_FLAG no longer joins nets by name.
- Done 2026-10-23 (`task-sch-details`, P10; `PARITY-sch.md` section 39): **buses across sheets**. A bus is a layer of its own (a wire ending on it is not on it); a bus label, hierarchical
  label or sheet pin on it names it; each member's nets on the bus's sheet (the nets of the member's name) are one net with the member, and a bus sheet pin and the hierarchical
  label of its sheet are one bus whose members are matched by index (a vector) or by name (a group): `processSubGraphs`, `propagateToNeighbors`, `matchBusMember`. kicad-cli's
  netlist of a bus across sheets, with the same and with different names on the two sides, is the studio's (`slow_a_bus_across_sheets_is_one_net_per_member_as_kicad_cli_makes_it`).
- Reviewed 2026-10-23 (`task-sch-details-review`; `PARITY-sch.md` section 39 "Review"): a shared screen's per-placement references now follow its symbols (moved, added, deleted),
  a deleted placement takes its references with it, an imported tree that shows a screen twice numbers each placement, KiCad 6's root `symbol_instances` and sheet field names
  are read, a PWR_FLAG of any library names no net, a group bus matches by local name, a bus label's class goes to its members and a shared screen's label class to each placement;
  18 of KiCad's QA hierarchies trace as kicad-cli reads them (`slow_kicad_qa_hierarchies_trace_as_kicad_cli_reads_them`).
- Reviewed 2026-10-23 (`task-p10-review`; `PARITY-sch.md` section 40): Import Sheet, sheets in the clipboard, sheet pins and Annotate against KiCad's code. Fixed: a file
  with a power symbol drawn otherwise than the design's hung Import Sheet and Paste; a sheet pasted inside itself is refused (`TestForRecursion`); a pasted sheet's pins keep
  their look; Duplicate carries a sheet by its pin; Sheet Pin Properties and Cleanup Sheet Pins as KiCad's; the library-link check; Annotate's final check of a selection.
  The practical check: KiCad's 130 QA schematic roots imported, written back and judged by kicad-cli, 87 alike (53 before). Left: a duplicate sheet name, "Untitled Sheet"
  and the "Link?" prompt, ~~first-free page numbers~~ (done 2026-10-27, section 43), the sheet's fill colour.
- Done 2026-10-24 (`task-fix-d-sch-connectivity`, fix batch D; `PARITY-sch.md` section 41): two buses on one sheet that share a member name are one net (`generateBusAliasMembers`);
  an invisible power input of an ordinary symbol is a global power pin; a power symbol's `${#}` is resolved per placement; bus aliases come from the `.kicad_pro` and with a
  sheet file; a library pin common to every unit stays unit 0; an ERC exclusion of one placement of a shared screen leaves the other's; a KiCad project's own ERC exclusions
  come in with `eda import-kicad`. 22 of KiCad's QA hierarchies trace as kicad-cli reads them (`bus_connection`, `issue17771`, `netlists/video`, `ERC_dynamic_power_symbol_test` added).
  Also fixed: the schematic `eda import-kicad x.kicad_sch` derives kept the file's frame, which the writer bakes otherwise (every pin off the wires in kicad-cli).
- Done 2026-10-26 (`task-nested-shared-sheets`; `PARITY-sch.md` section 42): a screen shown twice inside a screen shown twice has a reference per whole sheet path
  (`SymbolPathOverride::sheet_path`, the chain of placement ids from the root, as `SCH_SYMBOL_INSTANCE` keys by `SCH_SHEET_PATH::Path()`; an old `design.json`'s
  `parent_sheet_instance_id` is migrated on read to every path it stood for); the reader keeps the whole `(instances (path ..))`, the writer writes each path's
  reference and a sheet's `(instances ..)` per path of its screen; Paste shares a screen that has sheets of its own (`updatePastedSheet`). KiCad's QA `issue10926_1` and
  `netlists/hierarchy_aliases` now trace as kicad-cli reads them (24 QA hierarchies alike).
- Done 2026-10-27 (`task-hierarchy-pages`; `PARITY-sch.md` section 43): **page numbers as KiCad keeps them**: a schematic with none stored shows the load's
  numbers (`SetInitialPageNumbers`), a sheet drawn, imported or pasted (and the sheets inside it) gets the next free page, a page once shown stays when the
  walk moves, and a sheet on a screen shown twice has a page per sheet path (read, written, edited); the walk visits a screen's sheets by place
  (`SCH_SCREEN::GetSheets`); the Hierarchy Navigator sorts each sheet's children by page and shows every page.
- Done 2026-10-27 (`task-r3-sch-files`; `PARITY-sch.md` section 45, round-3 slice B): a sheet file above the root's folder (or absolute) is written inside the derived
  folder, never over the project's own; `Sheetfile` / `${SHEETFILE}` show the file's own text; a sheet's user fields resolve in the studio's text, travel with Paste,
  Duplicate and Import Sheet, and go with a deleted sheet; a sheet with a missing file keeps its page, a file that places itself is not followed; a sheet's pages are
  this project's only, without whitespace, an empty one `#`. Left: the Properties panel shows and edits a sheet's key, not its text.
- Left: a symbol's and a sheet's instance paths are written in walk order,
  not sorted by `KIID_PATH` as KiCad writes them (cosmetic); Annotate numbers the screen's own references only, not the other paths' (which count as taken,
  so it hands none out twice);
  Find does not match a sheet's name, file or pins; Clear Annotation (a design names a part by its reference, so two cannot be `R?`); "Place as group" and a ghost of the contents while they are carried;
  of the drop, one file is imported at a time (the others of the drop that the first does not place are said to be left out; KiCad runs Import Sheet for each in turn) and Choose Schematic opens on the file, where KiCad, given a file, has no chooser and has the sheet or its contents in the hand at once;
  the attribute, library-link and Update / Change Symbol tools act on the sheet in view; a footprint already placed keeps
  the reference it had when Annotate or Rename changes the schematic's.
- Port from: `eeschema/sch_sheet_path.cpp`, `sch_screen.cpp`, `connection_graph.cpp`, `tools/sch_navigate_tool.cpp`,
  `tools/sch_editor_control.cpp`, `dialogs/dialog_annotate.cpp`.

### 5. Access to the installed libraries
**Mostly closed (the choosers 2026-10-08, `80e972d`; library tables 2026-10-10, `task-lib-tables`): the choosers, the library trees and the lookups read the libraries KiCad's library tables name, the board has library tables of its own and the dialogs to edit them exist. Saving into a table library's file is open.** Hit: every session that adds a part. Package P3.
- Done: **the Symbol Chooser is KiCad's** (`components/SymbolChooserDialog.tsx`, `components/chooser/`, `kicad-port/libChooser.ts`): a tree of "-- Recently Used --", "-- Already Placed --",
  the project's own libraries and all 222 installed ones, a search scored as `EDA_COMBINED_MATCHER` does, a row per unit, the symbol's drawing, its default footprint and the description pane.
  **The Footprint Chooser** (`components/FootprintChooserDialog.tsx`): the 155 installed footprint libraries with the same search, the drawing, the description, and, for a symbol, "Filter by
  pin count" and "Apply footprint filters". It serves **Place Footprint** (a mounting hole, a fiducial: `Cmd::PlaceFootprint` makes a board part), the Footprint field of Symbol Properties and
  Assign Footprints. **Placing an installed symbol** keeps its definition with the schematic in the same undoable command (`Cmd::EmbedLibSymbol`). Libraries are indexed on demand by one pass over
  their text and cached (`crates/kicad/src/{symbol_scan,footprint_scan}.rs`, `crates/cli/src/library_search.rs`); the Footprint and Symbol editors' trees list every installed library.
- Done (2026-10-10): **library tables** (`PARITY-common.md` section 14). `crates/kicad/src/library_table.rs` ports KiCad's table format (a byte-for-byte round trip of KiCad's own global table),
  `LIBRARY_MANAGER`'s way of putting the tables together (the global table, the tables it chains -- KiCad's 222 symbol and 155 footprint libraries are one such row -- then the board's own, a
  nickname defined twice being the later) and `KIwxExpandEnvVars` (`${NAME}`, the project's variables, `KIPRJMOD`, the environment, `kicad_common.json`, KiCad's defaults). KiCad's global tables in
  `~/Library/Preferences/kicad/<version>/` are **read only** (every route that writes refuses that folder); the board's `sym-lib-table` and `fp-lib-table` are the studio's to create and edit
  (`crates/cli/src/library_tables.rs`, `GET|POST /api/library_tables`, `/check`, `/add`, `/normalize`, `POST /api/path_variables`, `GET /api/fs/list`). A `LibSource` (`library_index.rs`: a folder, or
  the resolved tables) sits behind the index, the choosers, the editor trees, placing (`Cmd::EmbedLibSymbol`, `PlaceFootprint`), a part naming `Lib:Footprint`, the Change Symbols catalog, the body
  lookup and the 3D export, so a library of the tables lists, searches, opens and places like an installed one; `GET /api/3dmodel` reads a model path with the board's variables and KiCad's
  (item 23). **Manage Symbol Libraries**, **Manage Footprint Libraries** (a Global tab, read only, with the tables it chains in tabs of their own, and a Project tab with `VerifyTable`'s checks),
  **Add Library**, **New Library** (an empty `.kicad_sym` or `.pretty` and a row) and **Configure Paths** (KiCad's variables read only, the board's own editable) are in `components/libraryTables/`.
  The derived KiCad project carries the tables and the variables (`derive_project`), so kicad-cli ERC (`lib_symbol_issues`), DRC (`lib_footprint_issues`) and the 3D export resolve the same libraries
  (slow-tier tests); `eda import-kicad` brings a project's own tables and variables along. Tests: Rust unit tests for the format, the variables, the resolution, the routes, the import and a
  place-from-table round trip; 13 web unit tests; `e2e/library-tables.check.js` (53 checks).
- Missing: saving the editors' symbols and footprints into a table library's file (they write the project library of `design.json`; a library made with New Library stays empty until a file is put
  into it), Add Library by drop (a browser drop has no path), Reset and Migrate Libraries, formats other than KiCad's (listed with a warning, kept in the table, not read), symbol folders of
  `.kicad_sym` files, Edit Options..., 3D model search paths (Configure Paths lists KiCad's variables; the `3D Models` paths are the footprint's own text); KiCad's
  regular-expression (`/re/`) and relational (`voltage>3.3`) search words; the Footprint selector's list and the 3D preview of the Footprint Chooser; the chooser places a symbol in its normal
  body style (Cycle Body Style changes it afterwards, item 12); Change Symbols lists only the libraries the design references; power symbols are placed by `P`, not offered here.
- Port from: `common/libraries/{library_manager,library_table,library_table_parser}.cpp`, `common/libraries/lib_table_grid_data_model.cpp`, `common/dialogs/{dialog_edit_library_tables,dialog_configure_paths}.cpp`,
  `eeschema/dialogs/panel_sym_lib_table.cpp`, `pcbnew/dialogs/panel_fp_lib_table.cpp`, `common/lib_tree_model_adapter.cpp`, `common/footprint_info.cpp`, `eeschema/symbol_chooser_frame.cpp`,
  `pcbnew/footprint_chooser_frame.cpp`.

### 6. The schematic clipboard
**Closed 2026-10-08** (`ee20bab`). Hit: every session.
- Done: Cut, Copy, Paste, Paste Special and Duplicate on the Schematic tab, in KiCad's own clipboard format (`crates/kicad/src/sch_clipboard.rs`, `POST /api/sch/clipboard/copy`,
  `Cmd::PasteSch` in `crates/ops/src/sch_clipboard.rs`, one undo step); pasted symbols are numbered unique across every sheet (`eda_model::sch_clipboard::annotate_paste`, a port of
  `ReannotateDuplicates`), Paste Special offers KiCad's three reference-designator options. Proven against real KiCad both ways (`crates/kicad/tests/sch_clipboard.rs`, `EDA_SLOW_TESTS=1`).
- Left: hierarchical sheets are not copied or pasted (KiCad keeps their screens in a side buffer); a copied symbol's fields are written `(fields_autoplaced yes)`, so KiCad lays them out again instead of the
  copy carrying their places (a label's spin, size, bold and italic and a symbol's body style are carried: `crates/ops/src/sch_clipboard.rs`; `PARITY-sch.md` section 15 still says they are not); Keep annotations
  cannot make a duplicate reference (a design here names a part by its reference); Clear annotations numbers at once instead of leaving `R?`; Copy as Text; tables, images and groups; a part's MPN and LCSC from the intent.
- Port from: `eeschema/tools/sch_editor_control.cpp` (`doCopy`, `Paste`), `common/clipboard.cpp`, `eeschema/sch_io/kicad_sexpr/`.

### 7. The router
**Mostly done.** Shove near pads, vias and tracks, the board edge, every setting and the via drag are done (2026-10-08, `1e4d7ea`, `03b46de`). **Dragging a segment, a route that starts or ends mid-segment and Route From Other End after the first fix are done too (P5a, 2026-10-10, branch `task-router-drag`: `68eb737`, `c1b409c`, `bae172b`, `0a9290e`, `9dbcd8e`, `77e526e`, `bc573db`). A differential pair is KiCad's too: its gateways, Highlight collisions / Walk around / Shove with the two lines held at the pair's gap, `V` for a pair of vias, the pair optimizer (P5b, 2026-10-11, branch `task-diff-pairs`: `aa48f28`, `8df6fba`, `04c753d`, `cdc464b`, `3de42d5`, `4666af7`, `880ad08`, `708d29c`, `d9c5fe7`).** **Length tuning is KiCad's too: its meanders, the tuning pattern tool for a track, a pair and a skew, and the pattern as a generator item (P5c, 2026-10-13, branch `task-length-tuning`: `0c3036f`, `efd5d28`, `dd35b60`, `d31692f`, `8d9d4b8`, `c7ab8a1`, `4ee2939`, `fce168d`, `65129b4`, `8bbd7b2`, `db9963a`, `543fa0d`).** **The leftovers are KiCad's too: arcs as router items, locked tracks, a track's head that ends in a via, mouse-trail posture, Optimize entire track being dragged, Route From Other End for a pair and the tuner's time domain (P5d, 2026-10-20, branch `task-router-rest`: `d58c9b83`, `9f993489`, `2e5e7d84`, `4688f7bc`, `88313a95`, `aaa736d2`, `71e75174`, `321a8109`, `0a9aee67`, `cbe3afff`, `e8eae12f`, and the docs).** **Springback (for the line placer), the rounded corners and virtual vias are KiCad's (2026-10-23, branch `task-router-details`, `crates/pns/PARITY.md` Stage 10), and so is the springback of the dragger and the pair placer (2026-10-25, branch `ds/handoff-1025`, `crates/pns/PARITY.md` Stage 14).** Hit: nothing of the router is left but the tuner's gaps (`ShoveTimeLimit` is ported: a shove gives up after a second, as KiCad's, 2026-10-13). Blocks: no. Package P5.
- Exists: `crates/pns` ports hulls, `LINE::Walkaround`, shove of tracks and vias (a stack of lines with ranks, `onCollidingSolid`, `pushOrShoveVia`, the optimizer over every shoved line),
  the optimizer with smart pads, loop removal, **the differential pair as `DIFF_PAIR_PLACER` (`diff_pair.rs`, `dp_placer.rs`: the gateways of `pns_diff_pair.cpp`, the pair started from a pad pair, a via pair or a pair of track ends and meeting the coupled
  pair under the cursor, walked round what is in its way as one unit and shoved with its lines held at the pair's gap, `V` for a pair of vias with the route going on on the other layer, `/` posture, Backspace, one undo step, the pair optimizer `mergeDpSegments`;
  kicad-cli's DRC finds no clearance, track width or diff-pair gap violation a placed pair adds)**, **length and skew tuning as KiCad's tool (`7`/`8`/`9`: `meander.rs`, `meander_placer.rs`, `dp_meander_placer.rs`, `tuning_path.rs`, `tuning_commit.rs`, checked against KiCad's own meander code on 100 000 random cases; the status box, `1`-`4`, Ctrl+L, one undo step; a tuning pattern is a generator item, `(generated ..)` in the file, selected, tuned again, removed, dragged by its edit points, listed in the Generators Manager; kicad-cli finds the tuned nets within the length and skew rules they were tuned to)**, `D`/`G` drag of a segment (`DM_SEGMENT`: it slides, its neighbours follow at 45 degrees),
  a corner or a via, with the last valid position restored when the cursor is where the drag cannot be resolved; routes that start and end in the middle of a track (`SplitAdjacentSegments`) and Route From Other End
  before and after the first fix (`ContinueFromEnd`); the clearance gate measures
  pads by their exact outline (`eda_drc::board::placed_pad_copper`); the board outline with its cutouts, arcs and a footprint's own Edge.Cuts are obstacles with the copper-to-edge clearance
  (`PNS_KICAD_IFACE_BASE::syncGraphicalItem`, `crates/pns/src/from_ir.rs`); every `RoutingSettings` field is read and Interactive Router Settings has `dialog_pns_settings.cpp`'s rows
  (`crates/pns/PARITY.md`; `CODE-COMPARE-router.md` D1-D6, D8, D10, D12, D15, D16 fixed).
- **P5d (2026-10-20): arcs are `ARC_T` items** (`arcs.rs`, `arc_drag.rs`: `from_ir` keeps an arc track as one `Arc` with its hull and collisions; a route starts and ends in the middle of one and cuts it in two, `SplitAdjacentArcs`; `D` on an arc is `DM_ARC`, KiCad's `EDIT_TOOL::DragArcTrack` with the three modes judging it, which KiCad's PNS does not; a shove and a drag keep the arcs they do not cut into and the commit writes them as `(arc ..)`; a tuning pattern starts on an arc track and is tuned along the arc), **locked tracks, arcs and vias are neither dragged nor shoved** (`from_ir` reads the locks and a pattern's members; D asks "The selected item is locked." and Drag Anyway drags it; `findRedundantSegment` / `findRedundantArc`), **a single track's head that ends in a via is shoved together with it** (`rhShoveOnly`), **mouse-trail posture** (`mouse_trail.rs` is `MOUSE_TRAIL_TRACER`, checked against KiCad's own code compiled over `libs/kimath`: 100 000 random walks of the cursor agree) and **Optimize entire track being dragged** (the optimizer's `KEEP_TOPOLOGY`, `PRESERVE_VERTEX`; both rows of Interactive Router Settings are live), **Route From Other End for a pair** (Ctrl+E: `ROUTER::ContinueFromEnd` restarts the placer of the same mode) and a pair that cannot be walked has no head (`tryWalkDp`), and **the tuner's time domain** (a target in picoseconds through the tuning profile of the net's class: `delay.rs`, `calculateTimeDomainTargets`, the Delay radio button, `is_time_domain` and `target_delay` in the board file). Proof: 81 new Rust tests in `crates/pns` (75) and `crates/cli` (6) (`tests/{arc_tracks,arc_boards,locked_tracks,head_via,mouse_trail_oracle}.rs`, KiCad's `issue12609` and `issue8407` for the arcs), the slow kicad-cli tests `routes_across_and_onto_an_arc_track_and_a_dragged_arc_add_no_violation_to_kicad_clis_drc` (a route across an arc, a route onto one, an arc dragged: no clearance, shorting, crossing, hole, edge or track-width violation added, every arc an `(arc ..)` of the file) and `kicad_cli_measures_the_delay_of_a_track_tuned_in_picoseconds_through_its_tuning_profile` (150 ps to 0.1 ps), and `web/studio/e2e/router-leftovers.check.js` (see below).
- Done 2026-10-23 (`task-router-details`, `crates/pns/PARITY.md` Stage 10): springback for the line placer (`reduceSpringback`, `pushSpringback`, `FixRoute`'s locked frame, `UnfixRoute`'s rewind: a pushed track stays pushed, is pushed on and springs back as the head draws back; KiCad's recorded `simple-shove-1` is now 24 of 28 segments, `issue22749` 11 of 11).
- Done 2026-10-25 (`ds/handoff-1025`, `crates/pns/PARITY.md` Stage 14): springback for the dragger (`DRAGGER`'s one `SHOVE` per gesture, the via's place per frame for `m_draggedVias`) and the pair placer (`DIFF_PAIR_PLACER::rhShoveOnly`'s frame per move, both heads reduced together, a fresh `SHOVE` at every fix and no locked frames).
- Done 2026-10-23: the rounded corners (`CORNER_MODE`'s four modes and the Track Corner Mode actions; a rounded corner is an `(arc ..)` track; `BuildInitialTrace` agrees with KiCad's compiled code on 100 000 cases).
- Done 2026-10-23: virtual vias (`FixupVirtualVias`; `D` at the end of a segment where the width changes or a locked segment ends drags that point as a via, every track there following; nothing of it is drawn or committed; a T of one width has none, as in KiCad, whose `n_seg` is never counted).
- Reviewed 2026-10-23 (`task-router-review`, `crates/pns/PARITY.md` "Stage 10 review"): the route's own tail is pushed by a later leg's shove (`LockJoint` ported), Backspace keeps the leg before, the 90-degree modes walk round the box of each hull, board text on copper is an obstacle (`syncTextItem`), the static `TOOL_ACTION`s of router_tool.cpp are in the action catalog.
- Done 2026-10-23 (`task-router-actions`, `crates/pns/PARITY.md` Stage 11): the router tool's Place Blind/Buried Via, Place Microvia, the three Select Layer and Place ... Via (the layer picker), Switch Track Posture and Custom Track/Via Size (`DIALOG_TRACK_VIA_SIZE`) are its actions; a blind or micro via commits with its type, layers and size (`(via blind ..)`, `(via micro ..)`), a shoved one keeps its type. Left of them: the via commands in the pair tool, and V's nearest-ratsnest-anchor layer.
- Done 2026-10-24 (`task-fix-e-router`, parity batch E, `crates/pns/PARITY.md` Stage 12): the router findings of `docs/parity/findings/p5-router.md` -- the seven highs (a route's end via committed, Walk around's via placed before the walk, Esc keeping the fixed part, the via drag at 45 degrees, the head via's other-layer shove, the pair gap, the walk-with-teardrops drag), 18 mediums and 13 lows (some in part); KiCad's seven recorded router sessions replay to the nanometre (`qa_regressions.rs`, KiCad's exact compare). Left: F2-11 (the grab radius is KiCad's grid size), F4-2 (arcs in the pair code) and the lows the findings file lists.
- Done 2026-10-27 (`task-r2-router`, `crates/pns/PARITY.md` Stage 15): the round-2 router findings (`findings/round2/r2-route-rules.md` R-1, R-2, R-3, R-6, R-7, R-8). The router's clearance is the DRC engine's (`PNS_PCBNEW_RULE_RESOLVER::Clearance` over `RuleEngine::eval_rules`: local pad and footprint clearances, net ties, the `.kicad_dru`, hole clearance and hole to hole), keepout rule areas are obstacles (`syncZone`'s triangles, `IsKeepout`), a pair's legs are simplified with the tracks they continue (`SimplifyLine`), the pair's Shove move that cannot be routed keeps showing its pushes, a Walk around drag that cannot be walked shows the track where it started, and the dangling test uses KiCad's integer accuracy and true arcs. Proof: `crates/pns/tests/rules_kicad.rs` (eight cases, each judged by kicad-cli under `EDA_SLOW_TESTS=1`).
- Done 2026-10-27 (`task-r3-router`, `crates/pns/PARITY.md` Stage 16): the round-3 router findings (`findings/round3/r3-route-rules.md` R3-1 .. R3-11). The router's world is `SyncWorld`'s: a keepout is an obstacle on every copper layer of its set, with its holes open, and a malformed one is left out with KiCad's warning; copper graphics are obstacles (board and footprint shapes with their net and connection points, a net tie's bridge in its tie, text boxes, dimensions, barcodes, tables, a footprint's copper texts) and Margin graphics are board edges; a pad's or via's removed layers are its hole only (`IsFlashedOnLayer`). A route starts at the start net's width through the rules (`ImportSizes`' `CT_WIDTH`: its class, a `.kicad_dru` rule, a pair's class width; the studio sends no width on "use net class"), and a pair's width, gap, via and hole to hole and an armed via's size are the rules' too. A start in a rule area or on a text says KiCad's words, a vias-only area included; net-tie groups read their escapes. Proofs: `tests/rules_kicad.rs` (kicad-cli under `EDA_SLOW_TESTS=1`), KiCad's 20 QA boards with copper graphics, the router replays still 13/13 exact. **Left of it:** a footprint's own Margin graphics; a dimension's and a barcode's text as the box of its glyphs, a barcode's printed text, a rounded rectangle's corners on copper; kicad-cli 10.99 reports no track against a copper graphic, and no `diff_pair_gap`, on the small test boards (it does on KiCad's `simple.kicad_pcb`), so those two are held by the router-side tests.
- Done 2026-10-28 (`task-r4-router`, `crates/pns/PARITY.md` Stage 17): the round-4 router findings (`findings/round4/r4-route-rules.md` R4-1, R4-2, R4-4 .. R4-7). A route or pair started on a pad or via takes the width of the track joined to it, as on a track (`inheritTrackWidth`, "Use Starting Track Width" on by default, over a chosen width too); a dragged or shoved via that removes its unconnected layers collides there by its hole only (`IsFlashedOnLayer( aHead, .. )`), keeps its id and extras when committed, and a drag at a via takes the via; a pair started on arcs checks its start gap; `ImportSizes` asks hole to hole with no start item; a borderless text box keeps its stroke and a copper arc its flattening error; `inflate2`'s `aSimplify` unions the offset as it is. **Left of it:** R4-8 (a pre-defined width equal to the Default class's is taken for "use net class": the studio keeps a width, not KiCad's index), a pair's custom dimensions applied after the start-gap check, and the importer leaving out a hidden board copper text and a dimension's `hide` (the router matches the derived board, KiCad's original has them).
- Done 2026-10-28 (`task-r5-router`, `crates/pns/PARITY.md` Stage 18): the round-5 router and studio findings (`findings/round5/r5-route-rules.md` R5-1 .. R5-9, R5-11, R5-10 in part). A route or pair started on an item of another layer than the active one starts on the item's layer with its net, and that layer is the active one from the start (`pickSingleItem` on every layer, `getStartLayer`; the active layer did not follow the route through a via, nor come back after a start on the active layer, until round 6: R6-R6); the router's commit gives a shoved or dragged track's replacement its solder-mask opening and group, and new items join the entered group (`createBoardItem`, `m_replacementMap`); the project carries and reads `track_widths` and `via_dimensions`; a pair's user dimensions are judged at its start; the pair menu lists Pre-defined Sizes' pairs and W steps them, W steps the board's widths (the first press keeping the inherited width's override), a chosen width no longer turns "Use Starting Track Width" off, the chosen sizes are read from the lists at route time; the message panel shows the route's width, clearance and gap with their sources. Left: "DP Max Uncoupled-length" in the panel; Cleanup's and Break Track's replacements carry no mask or group; Pre-defined Sizes' Add rows are pre-filled and a 0 width refused (R5-10).
- Done 2026-10-28 (`task-r6-router`, `crates/pns/PARITY.md` Stage 20): the round-6 router findings (`findings/round6/r6-route-rules.md` R6-R1 .. R6-R7). V while routing sizes its via as every via command (the net class's or a `.kicad_dru` rule's through the server, else the chosen Pre-defined Sizes entry or custom size); a chosen width below the board's minimum routes at the minimum, "(from board minimum track width)", at the start and when chosen while routing; the layer keys while a route is drawn are the router's (`LINE_PLACER::SetLayer`: the head goes to the layer, or a via to it is armed); Autoroute goes through `ROUTER::Finish` with KiCad's undo groups; V with the route tool armed starts the next route with a via; the active layer follows the route and comes back after (`syncRouterAndFrameLayer`, `m_originalActiveLayer`). Proofs: kicad-cli (`rules_kicad.rs`) for the via size, the minimum width and `A.Orientation`; router-leftovers' r6 scenario. **Left of it:** R6-R8 (a shoved line of several tracks takes the mask and group of the track whose id sorts first, KiCad's of its seed segment), the clearance not re-imported after a layer switch, and V's (and the start-with-via's) target the pair's other layer, not the nearest ratsnest anchor's.
- Left (`CODE-COMPARE-router.md` D18; `crates/pns/PARITY.md` Known gaps): the mark-obstacles hull snap (`rhMarkObstacles`), the walkaround cluster timeout, the line placer's `reduceTail` / `handlePullback` / `handleSelfIntersections` / `mergeHead` / `optimizeTailHeadTransition`, and "Do not show again" in the Drag Anyway question (KiCad's `KIDIALOG` has it). (Springback in the dragger and the pair placer was on this list; done 2026-10-25.) **Left of the pair (P5b):** two walk attempts where KiCad makes four (it repeats two), Backspace is an extension (`DIFF_PAIR_PLACER` has no `UnfixRoute`); a drag of a pair needs nothing (KiCad's `DRAGGER` has no pair mode: `m_draggedLine` is one `LINE`). **Left of the tuner (P5c, P5d):** the pad-to-die length and delay (the model has neither), a time-domain rule of a `.kicad_dru` file as the target (the length and skew rules are read), the message panel's lines of a pattern (the Generators Manager has its row), the merged outline of a pair's pattern (two capsules are drawn), a pattern moved, turned or flipped like KiCad lets one (here a pattern and its tracks are as good as locked for those: they are changed through its properties and edit points), the router's world cloned once at the start of a tuning where KiCad branches at every move, and a line that stops where the track width changes where KiCad rewrites the whole of it.
  Browser check of P5a done (2026-10-10, `window.__eda` on the 30-part board: segment drag, 30-degree segment, corner dropped on a pad, route started and ended mid-track, Route From Other End after a fix: item 33 for the rest).
  Browser check of P5b done (2026-10-11, `web/studio/e2e/diff-pairs.check.js` on a scratch board built from `examples/through_hole_headers.yaml`: starting from either pad, the three modes, a pair routed, finished on the other header and undone in one step,
  the pair of vias, `/`, Backspace, Escape). `docs/parity/CODE-COMPARE-router.md` section I and D18, `crates/pns/PARITY.md` Stage 7.
  **Browser check of P5d done (2026-10-20, `web/studio/e2e/router-leftovers.check.js` on a scratch board made with `eda board new examples/mcu_board_30plus.yaml`: 95 checks in 10 scenarios through `window.__eda`: the two router-setting rows, the mouse path setting the posture (and `/` flipping it) and not setting it with the row off, `D` on an arc dragging an arc, a shoved line keeping its arc, a route ending and starting in the middle of an arc, a locked track refused by `D` and dragged with Drag Anyway and walked round by a route, `V` shoving the track under a head's via, a tuning pattern on an arc track, and a target in picoseconds; Ctrl+E for a pair in `diff-pairs.check.js`: 12 checks, 79 in all). It found two bugs that the unit tests did not: Drag Anyway had no dialog host on the PCB tab (the question was never shown), and the clean-up after a walk dropped the vertex a tangent arc starts at, which cut the arc.** `docs/parity/CODE-COMPARE-router.md` sections C, E, G, H, I, J, K and D18, `crates/pns/PARITY.md` Stage 9, `web/studio/PARITY-pcb.md` section 35.
- Port from: `pcbnew/router/` (`pns_dragger.cpp`, `pns_multi_dragger.cpp`, `pns_line_placer.cpp`, `pns_diff_pair_placer.cpp`, `pns_meander*.cpp`, `pns_shove.cpp`, `pns_walkaround.cpp`,
  `router_tool.cpp`), `pcbnew/generators/pcb_tuning_pattern.cpp`, `pcbnew/tools/generator_tool.cpp`.

### 8. PCB edit tools
**Done (2026-10-08, `88c8c0a`; the details 2026-10-10, `task-pcb-edit-details`, not merged).** Hit: every layout session. Blocks: no. Package P6.
- Done (`PARITY-pcb.md` section 22): `move_items`, `rotate_items` and `flip_items` (`crates/ops/src/pcb_transform.rs`) move, turn and flip footprints, tracks (arcs stay arcs), vias, zones, graphics, text,
  dimensions and groups as KiCad's item classes do, one undo step each, and reach the `.kicad_pcb` kicad-cli reads. Move, drag, Move Individually, Move with Reference, Position Relative, Move Exactly,
  Rotate, Flip, Align and Distribute work for every kind with `FilterCollectorForLockedItems` and KiCad's rotation and flip points; `R` turns counter-clockwise. Duplicate, Copy, Cut and Paste take footprints
  (a copy is a `BoardPart`), dimensions and groups, in KiCad's clipboard format (`crates/kicad/src/clipboard.rs`, `Cmd::PasteClipboard`, `POST /api/clipboard/copy`). Pads are selectable (click, box, Selection
  Filter, highlight, the Properties pane); the pad edits are item 9's and are done.
- Done 2026-10-10 (`PARITY-pcb.md` sections 5 and 22): Paste Special on the PCB (`DIALOG_PASTE_SPECIAL`: the three annotation modes and Clear Nets, `Cmd::PasteSpecial`; a plain Paste keeps the reference the clipboard has, as
  KiCad's default does); the rotation step and the flip direction are settings (Preferences > PCB Editor > Editing Options) the planners and the verbs take; the reference point of Rotate and Flip is the best snap anchor of the
  centre of the selection's box, and a footprint's box is `FOOTPRINT::GetBoundingBox` (the origin square, pads, courtyard and fab body, with text for Rotate and Flip, without it for Align); editing a footprint whose pads
  are routed keeps the routing; Duplicate and Paste are one undo step with the drop, and Escape leaves nothing, not even a redo entry (`crates/cli/src/carry.rs`).
- Missing: a copy keeps the original's nets, so `--strict` refuses moving it away (paste it, or untick Strict); under `--strict` an edit that breaks a net that was complete is still refused (`routing_connectivity`) where KiCad
  shows a ratsnest line; a footprint's box lacks the silkscreen and fab graphics beyond the courtyard and the fab body until they are on the board (item 25).
- Done 2026-10-27 (`task-r2-pcb-edit`, `PARITY-pcb.md` section 55, the round-2 findings `r2-pcb-edit`): Delete inside a lone zone's cutout removes the cutout; Copy and Cut take every item kind (a field as a free
  text, a pad in a footprint of its own, barcodes, points, text boxes, images, tables) and Cut deletes only what the copy put on the clipboard; Paste, Duplicate and Create Array land text boxes and images (Create Array only since round 4 C-2, section 58); a rounded
  rectangle turned by a non-quarter angle keeps its rounded corners; Flip follows front/back user layer pairs; Mirror takes fields and points; Escape or Undo with a shape started keeps the drawing tool; a plain-text
  Paste is KiCad's text; Track & Via Properties works on the selection; Position Relative remembers; R right after M turns about the drag origin.
- Missing (2026-10-27): Mirror of a text box, an image and a tuning pattern; a footprint's own drawings on a user layer keep it when the footprint flips; Create Array and Repeat Layout turn a rounded rectangle as
- Done 2026-10-27 (`task-r2-misc-leftovers`, PARITY-pcb.md section 56): Mirror of text boxes and tuning patterns (an image is not mirrorable in KiCad), and the Mirror action now takes fields, points and text boxes (its filter had left them out); Create Array and Repeat Layout turn a rounded rectangle into its rounded polygon; a track Width of 0 in the Properties panel; the footprint rows are `FOOTPRINT_DESC`'s; Track & Via Properties has the via type, layer pair and annular rings. Still missing: a footprint's own drawings on a user layer flip to the pair (a model change: the art is per definition, side-agnostic); a pad, text or shape size of 0 from the panel (their verbs are also the dialogs', which refuse 0); the via's protection, backdrill and post-machining rows (KiCad 10 tokens).
  four corners; the Properties panel refuses a zero width or size KiCad takes (A7) and its footprint rows are not `FOOTPRINT_DESC`'s (A6); Track & Via Properties lacks the via's layer pair, solder mask and backdrill rows.
- Done 2026-10-27 (`task-r3-pcb-edit`, `PARITY-pcb.md` section 57, the round-3 findings `r3-pcb-edit`): Cleanup Tracks & Vias works on segments (only the dangling or shorting segment of a
  polyline goes; the cleaner's `aIgnoreTracksInPads`; widths in pads; redundant vias anywhere on a through-hole pad); Cut of table cells empties them and keeps the table; Delete and Undo while
  drawing a zone or polygon remove its last corner; Track & Via Properties asks about shorts and the pads a net change takes along; Select Connection walks as `selectAllConnectedTracks`;
  microwave pad numbers; Break Track on a locked track asks; Edit Text & Graphics reaches text boxes, cells and barcodes; Simplify keeps the polygon; Pad Properties' warnings; Add Field's Fab defaults.
- Missing (round 3): a turned rounded rectangle has no true arcs (no arcs in the IR's polygon); a dimension's mirror on a user layer typed back; the dimension text angle to `{:.10g}` (A-5);
  B-7 (a pad pasted while its footprint is gone brings the footprint back), B-8 (pasting an image), B-9 (a footprint's nested groups), B-10 (a group's half-made library link); a merged or cut track is new
  (KiCad keeps the first segment's uuid); a zone's holes under Simplify; the do-not-show boxes of the questions.
- Done 2026-10-28 (`task-r4-pcb-edit`, `PARITY-pcb.md` section 58, the round-4 findings `r4-pcb-edit`): teardrops measured from the anchor's edge (kicad-cli's own outlines); Swap Layers moves a
  tuning pattern with its tracks; a lone pattern turns about its origin; layer defaults take the class's width, height and italic; Create Array takes text boxes, barcodes, points, images and
  tables and a group by its box's centre; the Line tool chains and takes back; a shape's first click clears the selection, Delete drops it, Redo and fills are refused while drawing; a click on
  the first corner closes a zone or polygon and the line mode bends its legs; Clear board takes every drawing; Select Connection from a pattern; custom-pad checks; a locked group's members written
  locked; Cleanup's merge inside a pour; Merge Zones' holes and log lines.
- Missing (round 4): tuning patterns in Create Array; the circular array's angle in millidegrees (C-8); Cleanup's merge of overlapping or folded-back segments and its 1 nm tolerance; Track & Via
  Properties' short question on an OK with nothing edited; the cursor warp of a removed zone corner; the bell.
- Done 2026-10-28 (`task-r5-pcb-edit`, `PARITY-pcb.md` section 60, the round-5 findings `r5-pcb-edit`): teardrops on arc tracks (kicad-cli's own outlines, both ways round); the studio starts in the free
  line mode, a rectangle is free and Ctrl squares it, a 45-degree line keeps the cursor's x or y; a zone or polygon is finished through `OnComplete` (the loop's bend, `Simplify`); the first corner clears
  the selection and an emptied polygon stays started; Draw Via snaps, takes the track's, pad's or pour's net (Select Net:), is refused by the clearances ("Via location violates DRC.") and splits the
  track; the Text tool carries its text to a second click and stays armed, with the class's width and italic; Cleanup Graphics' 500 nm epsilon and the outline healed in place; Create from Selection keeps
  a rounded rectangle's corners, chains within 100 nm, makes non-copper zones, is one undo step, and takes KiCad's default widths.
- Missing (round 5): a free via's `(free yes)` (no flag in the IR's via); a Draw Via on an arc track does not cut it; the DRC check reads no constraint severity or pad clearance override; the canvas
  draws a text centred on its anchor whatever its vertical justification; Cleanup Graphics leaves text boxes alone (B5-6: KiCad's cleaner takes a `PCB_TEXTBOX` as a `PCB_SHAPE`) and its change list
  is not clickable; Create Tracks has its own dialog, delete flag and net (B5-16: KiCad's has none, uses the active copper layer and the shared `m_DeleteOriginals`, and leaves the tracks with no net,
  which `commit_route` here refuses). (`CalcArcCenter`'s round-number snap in the teardrop's arc: done in round 6, D6-1.)
- Done 2026-10-28 (`task-r6-pcb-edit`, `PARITY-pcb.md` section 62, the round-6 findings `r6-pcb-edit`): **no studio edit clears the routing any more** (E6-1, high: Swap, Move Exactly, the Pack and
  Move drop and placing a part from the unplaced list deleted every track, via and zone); Swap takes vias, graphics, texts and zones, swaps the layers of two items of a kind and leaves locked items out;
  Select / Grab Unconnected start from a pad; Select All Tracks in Net takes copper shapes; a group's lock is seen by Toggle Lock and the sweep filters; a stepped line width survives re-arming a tool;
  a board graphic polygon is unfilled on Edge.Cuts and the courtyards at the layer's width; a new copper text is bold (`InferBold`); teardrops follow 25ec2335 only (the chord filter, Max width 0, the
  arc's polyline and centre: 14 kicad-cli cases point for point); Draw Via ignores hidden layers, takes the board's via list entry and judges copper text by its own shape (and a knockout's hull);
  Create Zone / Rule Area from Selection is one dialog; the line mode is kept between sessions; `eda board via place`; messages KiCad does not have are gone.
- Missing (round 6): Swap of arc tracks, dimensions, groups, tables, text boxes, barcodes and points, and of two footprints' orientation and side inside a mixed selection, and of two pads as pads
  (`PAD::SwapShapePositions`); the Select Net menu's order by net code; the via ghost at `SnapItem`'s point (C6-7); "Via location violates DRC." with Allow DRC violations on (C6-6); a PTH pad's
  unflashed layer and text boxes, tables, dimensions and barcodes on copper in the via's DRC check (C6-4, C6-5); R / F / E on a carried text (A6-6); a rounded rectangle's corners through
  `TransformRoundChamferedRectToPolygon` (D6-3); the 100 nm chaining's direction (D6-4) and the outline fix's view of a removed duplicate (D6-5), both KiCad quirks; teardrops fitted to other nets'
  clearance (25ec2335's `computeFittedTeardropPolygon`) and the solder-mask teardrops; the text dialog's Bold control.
- Port from: `pcbnew/tools/edit_tool.cpp`, `edit_tool_move_fct.cpp`, `align_distribute_tool.cpp`, `pcb_selection_tool.cpp`, `pcb_control.cpp` (`Paste`, `placeBoardItems`), `common/dialogs/dialog_paste_special.cpp`, `pcbnew/kicad_clipboard.cpp`.

### 9. Footprints on the board are editable objects
**Mostly done (2026-10-09, `afcb91b`).** Hit: every board (silkscreen cleanup, mounting holes). Blocks: no longer. Package P2 for what is left (P1, the footprint's graphics, is done).
- Done (`PARITY-pcb.md` section 26; `crates/model/src/fp_edit.rs`, `crates/ops/src/fp_edit.rs`, `crates/kicad/src/fp_fields.rs`, `kicad-port/fpFields.ts`, `components/{FootprintPropertiesDialog,BoardPadPropertiesDialog}.tsx`): a
  footprint's Reference and Value are fields with a position, size, thickness, layer, visibility, rotation, justification, mirror, bold, italic, keep upright and knockout; user fields can be added, renamed
  and deleted; the attributes (SMD, through hole or unspecified, not in schematic, exclude from position files, exclude from BOM, do not populate, exempt from the courtyard requirement) are per instance,
  and do not populate and exclude from BOM stay in step with the schematic symbol; pads carry their own type, shape, size, offset, rotation, corner radius, hole and mask and paste margins. It is
  `DrawingsSection::footprint_edits` (additive), three undoable verbs (`edit_board_footprint`, `edit_board_field`, `edit_board_pad`), written as `property` / `attr` / pad extras that kicad-cli loads and
  whose position and BOM exports honour. A field is an item of the board (`REF:Name`): selected, drawn (following the Appearance rows), moved, dragged, turned, flipped. A pad's and a footprint's zone
  connection, relief gap, spoke width and angle and clearance are the zone filler's overlay (`drawings.zone_overrides`) and the same dialogs edit them. Board-only footprints are the "not in schematic" attribute.
- Measured: 69 new Rust tests and 39 `node --test` cases; `web/studio/e2e/footprint-edit.check.js` runs 19 scenarios through `window.__eda`; real kicad-cli honoured do-not-populate and exclude-from-position-files.
- Done 2026-10-21 (`task-microwave`, package P19; `PARITY-pcb.md` section 41): **the microwave tools**, the footprints a tool makes and no schematic owns. `Cmd::Microwave` (`crates/ops/src/microwave.rs`) has five verbs, `gap`, `stub`, `arc_stub`, `function_shape` and `inductor`; each puts a `BoardPart` with its `definition` on the board, the facts a footprint read from a
  board file has (`FootprintExtra`: copper on `F.Cu` only, no courtyard, no library link), KiCad's texts and attributes (`createBaseFootprint`: `REF**` on `F.SilkS`, an empty Value and `${REFERENCE}` on `F.Fab`, excluded from the position files and the BOM) and the pose, as one undo step. Draw Microwave Gaps and Stubs ask "Gap Size:" and "Stub Size:" (the box starts at the current track width),
  Arc Stubs "Arc Stub Radius Value:" and "Angle in degrees:" (clamped to a half turn), Polygonal Shapes the Complex Shape dialog (the file's `$COORD` points, `Unit`, `XScale` and `YScale` read as `ReadDataShapeDescr` reads them, strtok's quirks included; Normal, Symmetrical or Mirrored), Lines two clicks, "Length of track:" and
  "Component value:" (no character a footprint name cannot have). The S-shaped coil is `BuildCornersList_S_Shape` with `gen_arc`, and a length it cannot have is refused with KiCad's four messages. They are footprints like any other: selected, moved, turned, flipped, duplicated, copied, deleted, held to Strict, and opened in the Footprint Editor
  (the editor read a pad's missing `layers` as a list: fixed in `fetchFootprint`). Measured: 25 ops tests, 12 in `eda-cli` (one slow-tier: kicad-cli loads six footprints and plots them as the formulas say) and `e2e/microwave.check.js` (10 scenarios, 121 checks).
- Missing: per-footprint mask and paste margin overrides; (Change Footprint(s), Update Footprints from Library that reads the library and geographical reannotate are done, item 26;) padstacks (item 31; trapezoid,
  chamfered and custom pads are done, 2026-10-10), fabrication property, pad number and pin function edits, pad-to-die length; Datasheet and Description fields and text variables in field text; our own placement and courtyard
  checks do not read a pad's offset or margins (kicad-cli does); logos. Of the microwave tools: the footprint is put down at the first click and not carried on the cursor to a second (`doInteractiveItemPlacement`'s R, F and Enter before it; here Rotate and Flip act on the part afterwards),
  the Size boxes of the shape dialog count when they are edited (KiCad copies the file's scales over them), a second shape of a tool is named with a number after it (`muwave_gap_1`) because the model finds a footprint by its name, the footprints carry an explicit one-micrometre courtyard in the model (so Strict's derived courtyard
  of the pads does not refuse them; the file has none) and `placement_refdes_clear` still holds the reference text of one near another footprint's origin.
- Port from: `pcbnew/dialogs/dialog_footprint_properties.cpp`, `dialog_pad_properties.cpp`, `pcbnew/pcb_field.cpp`, `pcbnew/footprint.cpp`, `pcbnew/tools/board_editor_control.cpp` (`PlaceFootprint`).

### 10. The Properties panel
**Mostly done (2026-10-08, `bd712d3`; footprint fields, attributes and pads 10-09).** Hit: constantly. Blocks: no.
- Done (`kicad-port/{propertyManager,propertyGrid,pcbProperties,schItemProperties}.ts`, `components/panels/{PropertyGrid,PropertiesPanel}.tsx`; `PARITY-pcb.md` section 23, `PARITY-sch.md` section 17): the pane is KiCad's
  property grid. `PROPERTY_MANAGER` is ported (class registry, `InheritsAfter` / `Mask` / `ReplaceProperty` / `OverrideAvailability`, the ordering walk), so each class lists what KiCad's registration lists, in
  KiCad's order, on the board and on the sheet; a selection shows the rows every item has, the value they share or `<...>`, and an edit goes out as ONE `batch` (one undo step). Every edit is an existing verb
  except four added where none fit (`crates/ops/src/pcb_props.rs`: `edit_track`, `set_item_net`, `set_zone_name`, `replace_shape`). Footprint fields, attributes and pad rows edit since item 9.
- Measured: 87 `node --test` cases, 9 Rust tests for the four verbs; `web/studio/e2e/properties-panel.check.js` edits every editable row of every kind and undoes it.
- Missing: what the IR has no place for is not registered: a pad's trapezoid, chamfered and custom shapes (in the IR and Pad Properties since 2026-10-10, not checked in this panel), padstacks, via tenting and backdrill, board text's fonts, bold, italic, vertical justification and
  colour, a shape's line style and colour, a sheet's border and fill, symbol pin names and numbers, extra fields; a footprint's reference, value and library link text are read-only (they come from the
  schematic and the intent: item 26); a wire's end points and length are read-only; the grid is not in the Footprint and Symbol editors; a footprint's position lands on the placement grid like every pose.
- Port from: `common/widgets/properties_panel.cpp`, `common/properties/property_mgr.cpp`, `pcbnew/widgets/pcb_properties_panel.cpp`, `eeschema/widgets/sch_properties_panel.cpp`.

### 11. The DRC and ERC dialogs
**Mostly closed (2026-10-08, `02f383c`; the leftovers 2026-10-14, package P15 on `task-review`).** Hit: every review pass. Blocks: no.
- Done: Run DRC and Run ERC through kicad-cli with a running state, out-of-date marking and lint tabs; the list is `RC_TREE_MODEL`'s; DRC exclusions as an undoable verb
  (`Cmd::AddDrcExclusions` / `DeleteDrcExclusions`, `design.drawings.drc_exclusions`) written into the derived `.kicad_pro`; Next, Previous and Exclude Marker; a marker menu in both dialogs and on both
  canvases; the Ignored Tests and Schematic Parity pages (`--schematic-parity`); a Violation Severity page in Schematic Setup. `web/studio/e2e/drc-review.check.js` drives it. Details:
  `PARITY-pcb.md` section 9a, `PARITY-sch.md` sections 4 and 9, `PARITY-common.md` section 5.
- Done by P15 (`PARITY-pcb.md` section 33, `PARITY-sch.md` section 28): **ERC exclusions reach kicad-cli, with their comments** (`erc.erc_exclusions` of the derived project, from the marker kicad-cli reported:
  `eda_kicad::erc_exclusions`, `ErcViolation.marker`, `Cmd::AddErcExclusion { comment, marker }`); the exclusions of both domains are written as the records kicad-cli 10.99.0-5334 keeps in a project
  (`kiapi.board.DrcExclusion`, `kiapi.schematic.ErcExclusion`, protobuf JSON, positions in nanometres) with the section's schema said (`board.design_settings.meta.version` 3, `erc.meta.version` 1) -- not the
  pipe-delimited texts of `SerializeToString`, which that nightly migrates only out of a section at an older schema; **Delete Marker and Delete All Markers** in both dialogs (an excluded marker that is deleted takes its
  exclusion with it, as `RecordDRCExclusions` / `RecordERCExclusions` keep only the markers that are left: `delete_*_exclusion`, `Cmd::ClearDrcExclusions` / `ClearErcExclusions`, one undo step each); **a marker
  selected by a left click** on the board and on the sheet, a double click bringing the dialog up on it; **Exclude with comment / Edit exclusion comment** in the ERC menu; **Exclude all violations of rule '...'**
  (the rule is read from the violation's text); the **Inspect and Fix entries** of both menus (`InspectDRCErrorMenuText`, `FixDRCErrorMenuText`, `InspectERCErrorMenuText`, `FixERCErrorMenuText`);
  **Save...** from both dialogs (`kicad-cli ... --format report|json`, the Show boxes as the severities); **Edit connection grid spacing...** and Schematic Setup > General > Formatting (`Cmd::SetConnectionGrid`,
  `schematic.connection_grid_size`). `web/studio/e2e/review.check.js` drives it; the kicad-cli proofs (slow tier) are in `crates/cli/src/review_tests.rs` and `board.rs` (`drc_exclusions`).
- Left: **a DRC exclusion reaches kicad-cli only where the report can say where the marker is.** KiCad matches an exclusion to a marker by its kind, position and items, and kicad-cli's report gives
  the items' positions and never the marker's, so the studio writes the positions worth trying (`eda_kicad::drc_exclusions`) and waives the rest in the studio's report alone (`kicad_matched: false`). A
  KiCad patch adding the marker position to `RC_JSON::VIOLATION` would close it; that is upstream work and not planned. **An ERC exclusion reaches it only for the checks whose marker is known to the letter**
  (`eda_kicad::erc_exclusions::rule`): not `endpoint_off_grid` and `unconnected_wire_endpoint` (a marker at each end of one wire, and the report names the wire once), `generic-*`, `unresolved_variable`,
  `field_name_whitespace`, `label_multiple_wires`, `four_way_junction`, `no_connect_connected`, `hier_label_mismatch`, `similar_*`, `same_local_global_*`, `bus_definition_conflict`, `duplicate_pins`; a DRC check whose marker
  carries a layer (`copper_sliver`, `starved_thermal`, `generic-*`, `unresolved_variable`) is waived in the studio alone too. **The records are what kicad-cli 10.99.0-5334 reads**: an older kicad-cli
  (up to the pinned source, `8303b2ad`) reads the pipe-delimited texts only and would ignore them. Not ported: the ERC Fix entry "Edit Netclasses..." (Schematic Setup has no Net Classes page), Report all errors
  for each track (`--all-track-errors`), the cross-probe options, the other pages of Schematic Setup > Formatting (text sizes, line widths, junction and hop-over sizes; the inter-sheet references are done, item 12), and a file dialog
  for Save... (the page downloads the report).
- Done 2026-10-27 (`task-r3-sch-files`; `PARITY-sch.md` section 45, round-3 slice D): the ERC check table is KiCad 25ec2335's (the box's kicad-cli: 55 checks, nine of them new,
  so a project's severities for them come in and Setup takes them); a project's pin map keeps its good cells; the library, footprint, netclass, sheet-name and unit-footprint
  markers carry 25ec2335's sheet paths. Left: a pin-to-pin error ignored from the menu comes back on the next run (KiCad keeps it ignored until the project is reopened).
- Done 2026-10-28 (`task-r4-schematic`, `PARITY-sch.md` section 47): a project severity other than `warning` / `ignore` is an error (`SeverityFromString`); the three new
  checks' markers; the connectivity marker rules compared with 25ec2335's `ERC_MARKER_PLACER` (all match).
- Port from: `pcbnew/dialogs/dialog_drc.cpp`, `eeschema/dialogs/dialog_erc.cpp`, `eeschema/sch_marker.cpp`, `eeschema/schematic.cpp` (`ResolveERCExclusions`), `eeschema/erc/erc_settings.cpp`,
  `common/rc_item.cpp`, `common/dialogs/panel_setup_severities.cpp`.

- Round 6 (`task-r6-router`, 2026-10-28): "Refill all zones before performing DRC" starts checked; the All box is its own (`OnSeverity`). Noticed, not changed: KiCad's Show boxes start with Exclusions and All off.

### 12. Schematic symbols, fields and labels carry per-instance geometry
**Done (2026-10-09, `2d92851`)**, limits below; a free text's justification and pin selection added 2026-10-12 (`task-sch-details`). Hit: every schematic cleanup. Blocks: no. Package P10 for the limits.
- Done (`PARITY-sch.md` sections 1, 15): the Reference, Value, Footprint and Datasheet of every symbol, the Value of a power symbol and the name and file of a sheet each have a position, orientation,
  justification and visibility of their own (`SchematicSection::field_layout`, in the item's own frame), placed by a port of Autoplace Fields (`eeschema/autoplace_fields.cpp`, `crates/engine/src/fields.rs`)
  and written to the `.kicad_sch`. Fields are items: picked, moved (`M`, a drag), turned, mirrored, hidden with Delete and edited in Field Properties; `O` runs Autoplace Fields; a turn re-autoplaces
  (`SchExtras::fields_autoplaced`). A label's spin, size, bold and italic are stored and edited. A placed symbol in the alternate ("De Morgan") body style is drawn, written and read back (`LibSymbol::alternate`,
  `SchExtras::body_styles`, Cycle Body Style). 44 Rust tests, 17 `node --test` cases.
- Done 2026-10-12 (`PARITY-sch.md` sections 21 and 23): a free text's justification (`SchExtras::text_justify`: stored when it is not left and bottom, written as `(justify ..)`, read back, flipped by Rotate and Mirror, carried by copy and paste); a label's Auto
  (`LabelLook::auto_rotate`); a symbol's pins are items, and Swap Pins keeps each pin's place in `SchExtras::pin_swaps`.
- Done 2026-10-13 (`task-sch-groups`, `PARITY-sch.md` section 30): a user-defined field's place on the sheet (`FieldPlacement::name` names it in `field_layout`; `symbol_specs_in`, the writer, the reader, the editor); a sheet read from a KiCad file keeps the places of its user fields and no others.
- Done 2026-10-23 (`task-sch-details`, P10; `PARITY-sch.md` section 39): **a label's own fields** (`SchExtras::label_fields`): its net class (`Netclass`) and any other field, edited in
  Label Properties' grid (`DIALOG_LABEL_PROPERTIES`: Add Field gives a net class first, OK's rules for empty and unnamed rows, a name used twice refused), placed by
  `SCH_LABEL_BASE::AutoplaceFields`, drawn, written and read back, carried by the clipboard; the net is in the class (the model's net class, the project kicad-cli reads:
  its ERC's `undefined_netclass` and its netlist's `class` follow the field). **A global label's Sheet References** (`Intersheetrefs`, `${INTERSHEET_REFS}`) with
  Schematic Setup > Formatting > Inter-sheet References (`Cmd::SetIntersheetRefs`, `schematic.drawing.intersheets_ref_*`): the studio shows the pages as kicad-cli plots them.
- Missing: a field's font face and colour have no place in the IR; a label's field moved by hand is placed by `AutoplaceFields` again; the net class of a label on a bus is not
  given to the bus's members; the
  fields of a symbol read from a KiCad file are not items; `m_AutoplaceFields.enable` is always on and Autoplace Fields ignores swapped pins; Reorganize into Module Sheets (2026-10-10) keeps the fields' places and looks, the labels' looks and the body styles, but not a
  label's spin; only the normal and the De Morgan body style are modelled.
- Port from: `eeschema/sch_field.cpp`, `sch_label.cpp`, `dialogs/dialog_field_properties.cpp`, `dialog_label_properties.cpp`, `dialog_symbol_properties.cpp`.

### 13. Zones: the fill is KiCad's, and the zone settings
**Done 2026-10-13 on `task-zones` (not merged).** Hit: most boards. Blocks: no. Package P13.
- **Decided 2026-10-13: the zone fill is KiCad's, made by kicad-cli, and nobody else's.** `crates/zone-filler` (a port of `ZONE_FILLER` that was within 1% of kicad-cli's area on 198 of 208 fills of 17 QA boards, and
  `PARITY.md`, its measurements; `git show 43f6c09:crates/zone-filler/PARITY.md`) is deleted, with `eda_drc::fill` and the data only it read (the zone, keepout, knockout, copper-graphic and spoke-box fields of
  `DrcBoard`). P13's own plan for it (fill fidelity: courtyards and net ties as knockouts, the hatched zone's thermal rings, `connect_nearby_polys`, mask-only NPTH knockouts; the speed of a fine hatch) is
  **superseded**: nothing fills here, so nothing is left to port, and a hatch over a whole board is as fast as kicad-cli is (a few seconds).
- **Fill All Zones** (B, `POST /api/fill/kicad`, `eda board fill`): `kicad-cli pcb drc --refill-zones --save-board` on a COPY of the derived board (`.kicad/refill/`: the project's rules, none of the board's library
  tables, every DRC check ignored, started through `eda_kicad_engine::cli_command` like every other kicad-cli run, so with its empty settings folder), then each zone's `(filled_polygon ..)` read back
  (`eda_kicad::zone_fills`: nanometres, `(island yes)`; a footprint's own zones are taken out of the footprint's frame that kicad-cli 10.99.0-5334 stores them in, with the importer's `fp_transform` map). The
  fills are derived data beside `design.json` (`.kicad/zone_fills.json`), per zone with a fingerprint of the zone filled and the design revision. **Time**: a 30-part board (`examples/mcu_board_30plus.yaml`,
  placed and routed, a GND pour on both layers): 8.1 to 9.0 s with the user's footprint libraries (a DRC job does not start before the global libraries are enumerated), **1.9 to 3.3 s** with the empty settings
  folder, the same fill; KiCad's QA boards 1.7 to 2.9 s (`issue6284`, four zones and 90 rings: 8.4 s). kicad-cli fills the board's zones only (`ZONE_FILLER_TOOL::FillAllZones` walks `board()->Zones()`), so a
  footprint's own zone is no pour to wait for (`zone_fills::fillable`: not "not filled", not listed by `GET /api/fill` unless a file stored a fill for it).
- **Teardrops keep their fill** (2026-10-24, findings p13-p16 P13-1, `PARITY-pcb.md` section 52): the derived board says `(legacy_teardrops yes)` while the design has teardrops, so kicad-cli's refill keeps them
  (`UpdateTeardrops` returns at once) instead of deleting them all and rebuilding only those of pads with enabled teardrop parameters; a teardrop written back without a uuid is found by its layer and outline.
  Left: KiCad's own teardrop rebuild (the pads' and vias' teardrop parameters from `teardrop_settings`) is not what the studio asks for; its teardrops are the design's zones, regenerated by Add All Teardrops.
  Round 4 C-1 (2026-10-28, `task-r4-pcb-edit`, PARITY-pcb.md section 58): our generator's outline for a round anchor is now KiCad's point for point (the length from the anchor's edge, `computeAnchorPoints`' hull), checked against kicad-cli's own rebuild on 8 vias (`crates/kicad/tests/teardrops_kicad_cli.rs`); still round anchors and one segment only.
- **Follow-ups of the board reader fixes (2026-10-24, non-blocking):** a paste-only or mask-only pad (`FootprintExtra::aux_pads`) and an Extruded 3D Body (a footprint's kept tokens) are written only while
  the part is on the footprint and side they were read on, so a flipped footprint or a copy onto another footprint loses them, and a body's `(layer ..)` is not flipped; a DRC exclusion whose marker carries a
  layer (`copper_sliver`, `starved_thermal`, `generic_warning`, `generic_error`) is kept in the studio but never reaches kicad-cli (`drc_exclusions::markers` leaves it out); the `.kicad_dru` an Eagle or PADS
  import finds beside the input file may be a stale one from an earlier run (`foreign::imported_rules` takes the output's and the project's first); `isCopperOutside` tests each pad's box, not its
  effective polygon; a project's ERC exclusions are not read on import.
- **For a KiCad user of a board the studio exported** (the trade-off of the teardrop fix): a board with teardrops is written `(legacy_teardrops yes)`. Opened in KiCad, its teardrops are
  not kept current as tracks and pads are edited (KiCad rebuilds teardrops of a legacy board only when asked), Teardrop properties are greyed with KiCad's legacy-teardrop warning, and the
  teardrops are zones there that can be selected and edited as zones. In the studio they are the design's zones, regenerated by Add All Teardrops.
- **A zone keeps its holes** (2026-10-24, findings p22 F1): `Zone::holes`, the second and later `(polygon ..)` of a zone (`ZONE::AddPolygon`); Zone Cutout, Merge, Mirror (`set_zone_outline { holes }`),
  moves, paste and the clipboard keep them. Left (non-blocking): the point editor edits the outline only; Convert to shapes ignores the holes (`crates/ops/src/convert.rs`, `kicad-port/pcbConvert.ts`),
  as do Simplify (`actions/pcbEditSweep.ts`), the zone's shape in the rule evaluator's clearance tests (`crates/drc/src/rule_board.rs::zone_item`: its `shapes`; `insideArea` has the holes),
  the canvas hit test (`kicad-port/selectionCandidates.ts`), the Zone Manager's preview, and Repeat Layout's target area (the reference's outline without its holes).
- **Between fills the board shows the last fill**, as KiCad does with auto-refill off: an edit elsewhere leaves it; a zone moved, turned, mirrored or flipped keeps its fill moved with it (`ZONE::Move` /
  `Rotate` / `Mirror` / `Flip` transform `m_FilledPolysList`), and one edited in Zone Properties (a setting, its net) keeps it as it was (`Edit_Zone_Params` does not `UnFill()`), both counted out of date
  until the next fill (round 5 R5-Z2, 2026-10-28: before, any edit to a zone emptied it, which was not KiCad's); a zone reshaped (the point editor's `UnFill()`) or put on other layers (`SetLayerSet`) is emptied;
  a zone never filled is an outline (no copper for live connectivity, the 3D stand-in and the status bar's "N zones not filled (B)"). Read by: the canvas and the 3D stand-in (`GET /api/fill`), the `.kicad_pcb` writer (so the DRC, the
  render, the plots and File > Save As carry kicad-cli's rings), live connectivity and Cleanup Tracks (`eda_drc::zone_copper::ZoneCopper`). A board imported from a `.kicad_pcb` starts with the fills it was saved with.
  "Refill all zones before performing DRC", the Plot dialog's "Check zone fills before plotting" and the Zone Manager's "Refill zones" run the filler first, on a copy; the DRC itself runs without `--refill-zones`,
  so it judges the fill the studio keeps (a run that refills does not skip the courtyard checks, as was written here: kicad-cli 25ec2335 reports `courtyards_overlap` on `fill_bad` either way). Fill Selected, Unfill Selected and Unfill All Zones go to the server.
- **Zone kinds**: non-copper zones (silkscreen, mask, paste and user layers: `dialog_non_copper_zones_properties`; no net) and zones on several layers (`Zone::layers`, `(layers "F.Cu" "B.Cu")`, `*.Mask`, `F&B.Cu`),
  read and written as KiCad does, edited in Zone Properties (one box per layer, the dialog chosen by the layer drawn on). A zone keeps the uuid it had in an imported file (`Zone::uuid`): KiCad breaks ties between
  zones of one priority by uuid, and `issue5750`'s B.Cu fill came out at about half the area in kicad-cli's refill of our copy until it did (the slow proof below). A mask-only NPTH hole (`FootprintExtra::holes`) and a net
  tie's pad groups reach the derived board, so the fill kicad-cli makes of it keeps clear of the hole and exempts the tie.
- Round 5 (2026-10-28, `task-r5-rules-zones`, `PARITY-pcb.md` section 59): Auto-Assign Priorities takes the zones' holes (a pour in another's cutout does not overlap it; the areas less the holes, R5-Z5);
  Zone Properties offers `<no net>` and its OK keeps a copper zone on no net, with KiCad's warning (R5-Z3; a new zone still starts on the first net, P13-4); a zone keeps its fill through a move (R5-Z2, above).
  Left: a zone on both outer layers that is flipped keeps its layer set, and is taken as mirrored (each fill stays on its layer: the before and after alone cannot tell the two apart); a fill made before
  2026-10-28 holds only while its zone is unchanged (the cache did not keep the zone's shape).
- **Zone UI**: Auto-Assign Priorities in the Zone Manager (`AutoAssignZonePriorities` ported to `eda_ops::zone_priority`: the pads and vias of each net inside an overlap decide, the smaller zone where they do not,
  Kahn's order, one priority per same-net group; `GET /api/zone_priorities/auto`, staged and applied on OK), the Zone Manager's preview (a tab per layer of the zone: the board's edges, the fill or the outline;
  `components/ZonePreview.tsx`), a "Refill zones" box, and the Locked box of Zone Properties. The full settings dialog, keepouts, Merge, Duplicate onto Layer and the Priority actions are as before (`PARITY-boardctl.md`).
- **Proof**: in the fast tier, `crates/kicad-engine` tests with a stand-in kicad-cli (the refill runs `pcb drc --refill-zones --save-board` on the copy through `cli_command`'s settings folder and reads the
  saved board's rings; the DRC after a refill runs without `--refill-zones` on the board that carries them; a kicad-cli that cannot refill is told apart) and `eda_kicad::zone_fills` (the scanner, a footprint's
  frame, the fingerprint, the cache file). In the slow tier, `crates/kicad-engine/tests/refill_zones.rs` (`EDA_SLOW_TESTS=1`): kicad-cli's fill of the derived board equals its fill of the original, board by board and layer by layer, to 0.00 to 0.002% of the area
  on `issue5990` (mask-only NPTH holes), `issue14008` (a net tie), `issue6260` (a non-copper zone) and `issue5750` (the uuid tie), and on `issue6284`, `issue12609`, `issue2568`, `hatch_thermal_connectivity`,
  `zone_filler`, `issue7086`; the DRC runs on the board written with those fills. `web/studio/e2e/zones.check.js`: 8 scenarios through `window.__eda` on a served board, kicad-cli running for real.
- Left: `issue22826`'s inner layers differ by 0.5 to 0.8% because its through-hole pads are `(remove_unused_layers yes)` and `(zone_layer_connections)`, which the IR does not keep (item 31, padstacks); the Zone
  Manager's "Update Displayed Zones" (a fill of the dialog's staged copies; the preview shows the fills the board holds, and the staged priorities take effect with OK and a fill); no auto-refill after an edit
  (KiCad's own setting is off by default; `ZoneFillDirty` fills the zones that have none); a fill is derived data, not an undo step: Undo of a zone edit gives the fill back only because the cache still holds the old
  zone's entry, and a second fill replaces it.
- Port from: `pcbnew/zone.cpp`, `zone_utils.cpp` (`AutoAssignZonePriorities`), `zone_manager/`, `dialogs/panel_zone_properties.cpp`, `dialogs/dialog_non_copper_zones_properties.cpp`, `jobs` of `pcbnew_jobs_handler.cpp`
  (`JobExportDrc`: `--refill-zones`, `--save-board`), `pcb_io/kicad_sexpr/` (`format( const ZONE* )`: the layer set and the filled polygons), `pcbnew/teardrop/`.

- Round 6 (`task-r6-router`, 2026-10-28, `web/studio/PARITY-pcb.md` section 63): a duplicated, arrayed or pasted zone carries its fill (Copy writes it, the paste reads it back); a flipped one-sided zone keeps its fill but is written not filled; Auto-Assign Priorities counts net-0 pads and vias for a no-net zone; Cleanup's merge leaves out a different-width track the filter skips.

### 14. Snap, grid and the click-versus-drag rule
**Closed 2026-10-09 for the board, the schematic and the click rule (`358606b`); the footprint and symbol editors snap to other items' anchors since 2026-10-14 (P17, `task-lib-editors`, not merged).** Hit: every drag. Blocks: no.
- Done: the click rule (`kicad-port/dragThreshold.ts`: a drag after more than 8 px along an axis, or on macOS a motion after 300 ms held, in all four editors); the board's `PCB_GRID_HELPER::BestSnapAnchor`
  with hysteresis, snap lines, construction geometry and intersections (`kicad-port/{snapGeom,snapScene,snapAnchors,constructionManager,gridHelperBase,pcbGridHelper}.ts`, 67 tests); grid overrides per category
  (`Toggle Grid Overrides`, `Ctrl+Shift+G`); the schematic's grid list, Next / Previous Grid, fast grids, Edit Grids and `EE_GRID_HELPER::BestSnapAnchor` (`kicad-port/schGridHelper.ts`); Magnetic Points in
  Preferences. `e2e/snap-grid.check.js` runs 43 checks. 1521 unit tests pass.
- Done 2026-10-14 (P17, `PARITY-fpedit.md` section 14, `PARITY-symedit.md` "Snapping to the pins of the symbol"): the Footprint Editor's tools call `PcbGridHelper` on the open footprint's pads, graphics and texts (`kicad-port/fpSnapScene.ts`: the editor's own magnetic defaults, the
  active layer for shapes and text, every layer for pads and the anchor; the footprint itself is no item) and the Symbol Editor's `SchGridHelper` on the pins of the unit being edited (`kicad-port/symSnapItems.ts`); real pointer events in `e2e/lib-editors.check.js`.
- Left: Preferences > Magnetic Points is the board's (the footprint editor keeps KiCad's own defaults); with the default
  Magnetic Points ("In Track Tool") a move or a shape does not snap to pads or tracks until they are set to Always (KiCad's default too); the junction tool keeps its own 12 px snap; the schematic context menu
  has no Zoom and Grid submenus; the server still snaps a part's placement to 100 um (`meta.snap_um`) whatever grid is shown; the studio never moves the real pointer, so a Move computes the cursor
  KiCad's warp would give (`kicad-port/heldCursor.ts`).
- Port from: `pcbnew/tools/pcb_grid_helper.cpp`, `common/tool/grid_helper.cpp`, `eeschema/tools/ee_grid_helper.cpp`, `common/tool/tool_dispatcher.cpp`, `common/tool/construction_manager.cpp`.

### 15. Interaction details that differ
**Partial.** Hit: constantly, one detail at a time. Package P6 (PCB) and P10 (schematic).
- 42 of `CODE-COMPARE-ui.md`'s 266 rows were marked divergent (some since fixed), and about 310 handlers added since have not been compared. Fixed since 2026-10-07: High Contrast cycles in three states
  (`6611ade`) and `toggleLastNetHighlight` is wired. Fixed 2026-10-10 on the board (`task-pcb-edit-details`, `PARITY-pcb.md` section 22): Select Connection with nothing selected takes the connected item under the cursor
  (a pad starts a connection; a chain of graphic lines is not followed), Ctrl+U returns to the imperial unit last used, Alt+` (`toggleNetHighlight`) turns the highlight off and on from the net it remembers, and Add Corner
  puts the corner at the nearest point of the edge (zone, polygon, line, arc). Carried forward from 2026-10-07 and not re-checked: Enter during a PCB route finishes it instead of clicking (`common.Control.cursorClick`),
  Route From Other End only before the first fix and `V` does nothing outside a route (all three belong to the router, package P5); Select Connection needs a selection on the schematic; Mirror X then Y collapses to one flag.
  Fixed 2026-10-12 on the schematic (`task-sch-details`, `PARITY-sch.md` sections 21 to 27): the net highlight and Select Node start from a pin; a label's Auto-rotate; Copy as Text; Repeat Last Item repeats a label and a text with their properties;
  Global Edit's text size edits the text in place instead of replacing it (which dropped its id and justification); the schematic pans with the middle and right buttons as the preferences say and auto-pans while drawing and moving.

### 16. The PCB context menu
**Mostly done (2026-10-08/09, `4c041a6`).** Hit: constantly. Blocks: no.
- Done (`PARITY-pcb.md` section 7): the menu is built the way KiCad builds it, from the entries every tool adds with its condition and order number (`kicad-port/pcbContextMenu.ts`:
  `ConditionalMenu`, `buildSelectionMenu`), and lists only what applies, with the submenus (Select, Routing, Mirror / Rotate, Shape Modification, Position, Locking, Zones, Net Inspection Tools, Align/Distribute,
  Create from Selection, Grouping), Zoom and Grid, and Properties last. The router's menu opens while a route is drawn. A right click keeps a selection.
- Missing: table cells, gate swap and generators have no IR, so no entries; the pad settings are listed dimmed; most of the router's via, posture and corner-mode actions are dimmed (Place Through Via
  works while routing; Track Corner Mode always shows 45); Zone Priority raise and lower go by the zones' boxes overlapping, not their filled shapes.
- Port from: `pcbnew/tools/pcb_selection_tool.cpp`, `edit_tool.cpp`, `pcb_editor_conditions.cpp`, `board_inspection_tool.cpp`, `router/router_tool.cpp`.

### 17. Groups
**Mostly done on the board (2026-10-08/09, `4c041a6`) and on the schematic sheet (2026-10-13, `task-sch-groups`, not merged); the footprint editor has none.** Hit: sometimes. Blocks: no. Package P6 (board) and P10 (schematic).
- Done: Group, Ungroup, whole-group selection, enter and leave, Add Items, Remove Items and Group Properties (`Cmd::EditGroup`); groups nest (`EDA_GROUP`; `crates/model/src/groups.rs`,
  `crates/ops/src/pcb_groups.rs`); move, rotate, flip, Delete, Duplicate, Copy and Paste take the whole tree; the entered group is drawn with its box and name; the `.kicad_pcb` file and the clipboard hold the tree.
- Done 2026-10-10 (`task-pcb-edit-details`): items drawn, pasted, duplicated or arrayed while a group is entered join it (`BOARD_COMMIT::Push`; `Cmd::InGroup`, `crates/ops/src/pcb_groups.rs`; `PARITY-pcb.md` section 16).
- Done 2026-10-13 (`task-sch-groups`, P10b; `PARITY-sch.md` section 29): the sheet has the same groups. `eda_model::groups::{GroupTree, GroupHost}` is the one tree and the one set of list edits the board's drawings and the sheet's `SchExtras::groups` both run; Group, Ungroup, Add Items, Remove Items and
  Group Properties are `SchCmd` verbs (one undo step each) with the studio's whole-group selection, Enter and Leave Group, window and crossing boxes, the Grouping and Enter Group entries of the context menu, the group's box and name, Properties on a group, and Move, Drag, Rotate, Mirror (a lone group turns on itself), Lock,
  Delete, Copy, Cut, Paste and Duplicate taking the whole tree; what is made while a group is entered joins it; `(group "name" (uuid ..) (members ..))` is written as `saveGroup` writes it and read back (the writer names every item's uuid for it). 7 model, 23 ops, 8 kicad (one slow: kicad-cli loads nested groups, runs ERC, `sch upgrade` keeps them), 1 cli and 12 `node --test` cases;
  `web/studio/e2e/sch-groups.check.js` (thirteen scenarios through `window.__eda`).
- Done 2026-10-27 (`task-r2-pcb-edit`): a group of one is kept on import, Paste, Duplicate and Create Array; a group's lock is read back; a tuning pattern is a member both ways; a footprint's own groups are kept
  (`FootprintExtra::groups`); Group Items locks the group of a locked item and leaves pads and fields out; Group Properties has Locked and takes any library link (`LIB_ID::Parse( .., true )` never fails); text boxes,
  barcodes, images and generators are groupable.
- Missing: what the router commits is not made in the entered group (package P5); no groups in the footprint editor; the `group` verb used by itself nests a group made of items that share a group into it (KiCad does that
  only with the group entered, which the studio sends as `in_group`). (Create Array copies groups: `array.rs` uses `collect_copies`, PARITY-pcb.md section 17.) On the sheet: Reorganize into Module Sheets drops the groups of the sheets it takes items out of; a lone group's turn point counts a text or a label by its anchor; Group Properties does not take the view to a row's item; the Align and Distribute verbs are not group-aware.
- Port from: `common/tool/group_tool.cpp`, `pcbnew/tools/pcb_group_tool.cpp`, `eeschema/tools/sch_group_tool.cpp`.

### 18. Appearance and display options
**Mostly done (2026-10-09, `6611ade`).** Hit: every session. Blocks: no.
- Done (`PARITY-pcb.md` section 24; `components/panels/AppearancePanel.tsx`, `kicad-port/appearance*.ts`, `layerPresets.ts`): KiCad's `APPEARANCE_CONTROLS`. The Objects tab lists 20 of its 22 rows with eyes and opacity
  sliders that the painter and the pickers honour; the Nets and Net Classes tabs have per-net eyes, colours and KiCad's menus; the Layers tab has "Inactive layers" Normal / Dim / Hide and the eight built-in
  layer presets plus saved ones; viewports. Kept per project in `appearance.json` and written into the derived `.kicad_prl` / `.kicad_pro`. `e2e/appearance-panel.check.js`.
- Missing: Images and Points rows (no such items); the colour theme is not editable (swatches are read-only, so "Use Color from Schematic" stays dimmed); the quick switchers on Ctrl+Tab and Alt+Tab; the
  selection filter is not in the saved settings; the drawing sheet starts off (KiCad: on) and has no zone letters. (The Values row now acts: the Value is a footprint field since 10-09.)
- Port from: `pcbnew/widgets/appearance_controls.cpp`.

### 19. Footprint and symbol editor leftovers
**Mostly done (P17 2026-10-16 on `task-lib-editors`, not merged).** Hit: library work. Blocks: no. Packages P7 and P17 (P14, the 3D model tab, is done: item 30).
- Exists: both editors with library trees, pad and pin tools, dialogs, tables, import and export (`PARITY-fpedit.md`, `PARITY-symedit.md`).
- Done 2026-10-14 to 16 (P17; `PARITY-fpedit.md` sections 13 to 15, `PARITY-symedit.md` the five new sections; `e2e/lib-editors.check.js`, 24 scenarios; the slow kicad-cli proofs in `crates/cli/src/lib_editors_tests.rs`): derived symbols (`extends`, Derive from Existing Symbol, Flatten, Update Symbol Fields,
  Bulk Edit Related Symbol Fields, the fields grid of Symbol Properties; a library with derived symbols written by this studio loads in kicad-cli, `sym upgrade` keeps it and kicad-cli plots the derived symbol as it plots the flat one), a fields grid in the library editor's Footprint Properties, symbol fields on the canvas with
  Show Hidden Fields, alternate pin functions (`(alternate ...)`, the Pin Properties grid, Show Pin Alternate Icons), the symbol text box (Draw Text Box, Text Box Properties, `(text_box ...)`), a Shape Properties dialog for library shapes, snapping to other items' anchors in both editors (item 14).
- Reviewed 2026-10-23 (`task-p17-review`; `PARITY-fpedit.md` section 16, `PARITY-symedit.md` "P17 review"): the editors' readers and writers against KiCad's own libraries -- 164 installed footprints and
  243 symbols (93 derived, 23 of several units) read, written back and re-saved by kicad-cli as KiCad writes them (`crates/kicad/tests/library_roundtrip.rs`). Before: of 87 footprints 49 differed, of
  92 symbols 14. Fixed: units that are not interchangeable (`ki_locked`, lost on 10 of 92 symbols; "All units are interchangeable", Synchronized Pins, the unit a new graphic goes on), `(power local)`
  written as a global power symbol, items of body style 0 moved into body style 1, a `~` read as empty by the writer's old file version, the exclusions from simulation and position files, jumper
  pins, unit names, private fields, stroke styles, text styles, beziers; on the footprint side `allow_soldermask_bridges`, edge connector pads, a pad's fabrication property and margins, the
  footprint's margins, clearance, net ties, zones, points, groups (with the items' uuids), line styles, text looks, the Datasheet and Description fields (Footprint Properties' four mandatory rows),
  the precision of corner ratios, and the pad tool's next number (`GetNextPadNumber` from the tool's last number, "1" on load). Symbol Properties has KiCad's groups and words.
  Left: the micrometre grid of the IR (70 of the 164 footprints have a length finer than a micrometre, rounded by up to 0.5 um); see the two PARITY sections.
- Round 4 (2026-10-28, `task-r4-editors-exports`; `PARITY-fpedit.md` section 17): the Footprint Editor's drawing tools take the layer class's width (a polygon unfilled on Edge.Cuts and the courtyards) and the layer box
  lists every layer; the Text tool is KiCad's (Footprint Text Properties on the first click, the text carried, the layer's size and pen, left, mirrored on a back layer, the tool stays); the pad is carried and R / F
  turn or flip it before the click; the library Footprint Properties' size messages, Paste Footprint's Value and the Renumber Pads dialog and popup are KiCad's.
- Missing: Explode and Recombine of a custom pad (item 31); a placed symbol gets a library text box as its border and fill only, not its text (the engine's `SymbolGraphic` has no text box), keeps the alternate pin function a file picked (round 5, PARITY-sch section 48) but the studio cannot choose one or draw its name; the border's colour and
  a custom fill colour (the style is kept since the review, not edited) and Private to Symbol Editor of a library shape and text box; a plain text of a symbol and the Footprint Editor's graphics and texts have no Properties dialog (a new footprint text has its Footprint Text Properties since round 4, an existing one not yet); a footprint text has no vertical justification, italic or keep-upright; the fields table has no simulation fields; no named body styles;
  Symbol Properties has no unit display names grid and no Pin Connections tab (both kept in the file); a pad's fabrication property and margins are kept, not edited, in the library Pad Properties; the board's Footprint Properties still has two mandatory rows. (The pad clearance and thermal overrides are read by the zone filler since 10-08; the note that no check reads them is gone.)
- Port from: `pcbnew/dialogs/panel_fp_properties_3d_model.cpp`, `dialog_pad_properties.cpp`, `eeschema/symbol_editor/`.

### 20. Schematic view controls
**Mostly done on the schematic (2026-10-12, `task-sch-details`, not merged).** Hit: constantly. Blocks: no. Package P10.
- Done 2026-10-12 (`PARITY-sch.md` section 27): the schematic canvas pans, zooms or does nothing with the middle and the right button as Preferences > Mouse and Touchpad say (`input.drag_middle`, `input.drag_right`; a right press that is no drag still
  opens the context menu), and with Automatically pan while moving object on it auto-pans at the edge while a wire, bus, line, shape or sheet is drawn, items are held by M, G or a drag, a paste is carried or a box is dragged
  (`kicad-port/viewControls.ts`, `SchematicView.tsx`).
- Missing: the PCB canvas pans with the middle or right button and auto-pans at the edge (opt-in, as in KiCad; `Canvas.tsx`) but does not read the drag settings, and the footprint and symbol canvases pan with either button, ignore the settings and never auto-pan;
  the left button's drag setting (`input.drag_left`) is not offered. The native pinch gesture is not ported on any canvas (`PARITY-pcb.md` section 1).
- Port from: `common/view/wx_view_controls.cpp`.

### 21. Find on the PCB
**Done (Find 2026-10-08/09, `4c041a6`; Find by Properties and Find on the IR 2026-10-13 on `task-search-inspect`, not merged).** Hit: often on big boards. Blocks: no. Package P11.
- Done (`PARITY-pcb.md` section 7a): Find (Ctrl+F), Find Next (F3) and Find Previous (Shift+F3) on the board from `DIALOG_FIND`: history, Match case, Whole words only, Wildcards, Wrap, footprint references, values,
  other texts, DRC markers and net names, in KiCad's order, the hit selected and the view brought to it (`kicad-port/pcbFind.ts`, `components/PcbFindDialog.tsx`).
- Done (section 32a): Find by Properties, Edit > Find by Properties...: the From Selection tab (the properties the selected items share, a Match mode each, Select Matching with Deselect non-matching items and Zoom to fit) and the Query tab
  on KiCad's expression language (`kicad-port/{libeval,pcbExpr,findByProperties}.ts`, a port of `libeval_compiler`: lexer, grammar with its precedences, code generator and stack machine), Recently Used, Check Syntax and Create from Selection Tab
  (`components/FindByPropertiesDialog.tsx`). 57 `node --test` cases and `e2e/search-inspect.check.js`.
- Done (section 32b): Find follows the IR: a footprint is found by any of its fields (a user field's text) and its own texts, a hidden field only with Include hidden fields, a zone by its name; the dialog opens with a selected field's first line.
- Missing: the dialog's "Show search panel" link (the studio has no Search panel; `common.Interactive.search` still jumps to the Activity tab); on the Query tab the geometry functions (`intersectsCourtyard`, `intersectsArea`, `enclosedByArea`),
  `fromTo`, `memberOfSheet` and `hasComponentClass` say false (the rule engine of Clearance Resolution, `crates/drc/src/pcbexpr.rs`, has the geometry ones), `A.Net` is the net's name (KiCad's is the net code) and Recently Used is kept in the browser, not the project file;
  a footprint's graphics and texts other than its fields are not items of the selection, so a query cannot select them; the nets of the hit list are in name order (KiCad's is by net code). Find and Replace is the schematic's only, as in KiCad.
- Port from: `pcbnew/dialogs/dialog_find.cpp`, `dialog_find_by_properties.cpp`, `pcbnew/pcbexpr_evaluator.cpp`, `common/libeval_compiler/`.

### 22. Arrays of footprints
**Mostly done (2026-10-09, `afcb91b`).** Hit: connector rows, LED grids. Blocks: no.
- Done (`PARITY-pcb.md` section 17; `crates/ops/src/array.rs`, `kicad-port/arrayOptions.ts`, `components/CreateArrayDialog.tsx`): Create Array makes grid and circular arrays of footprints, tracks, vias, zones,
  graphics, text, dimensions and groups, as copies or as an arrangement of the selected items, in one undo step; KiCad's tabs, boxes and wording, stagger, skew, centring, Full circle, "Rotate items", reference
  designators ("Assign unique" via `ReannotateDuplicates`, or keep). 23 Rust tests, 17 `node --test` cases, the browser check.
- Done 2026-10-13 (P17, `task-lib-editors`, not merged; `PARITY-fpedit.md` section 13): the footprint editor's array of pads, graphics and texts with the numbering (`ARRAY_AXIS`: numerals, hexadecimal, the two alphabets, 2D coordinates, `ARRAY_PAD_NUMBER_PROVIDER`:
  Use first free number, serpentine, direction, the Coordinate scheme), `create_footprint_array`, one undo step; a footprint with a 3 x 4 BGA array written by this studio loads in kicad-cli and `fp upgrade` keeps its twelve pads.
- Missing: the original keeps the first point
  where KiCad's reverse loop leaves it on the last (same points, same references); the circle's centre starts at the selection, where KiCad's dialog ignores the origin it is given; the circle's "Pad numbering start" follows its own initial-number radio.
- Port from: `pcbnew/tools/array_tool.cpp`, `pcbnew/dialogs/dialog_create_array.cpp`, `pcbnew/array_pad_number_provider.cpp`.

### 23. The 3D viewer
**Done (2026-10-09/10, `89ff669`)**, limits below. Hit: every board review. Blocks: no.
- Done (`PARITY-3d.md` sections 1, 3, 5, 6, 7): each part is its own KiCad 3D model, fetched once per distinct model (`GET /api/3dmodel`, `crates/cli/src/model3d_api.rs`), read with three's `VRMLLoader` and placed by
  `render_3d_opengl.cpp`'s chain (`kicad-port/model3d.ts`); the route resolves a footprint's `(model ...)` path as `FILENAME_RESOLVER` does (a `${NAME}` is asked of the board's path variables and KiCad's before the environment, and the folders those name are readable, `library_tables::ModelNames`), serves it read-only and converts a STEP once through
  `kicad-cli pcb export vrml` into a cache. Time to the first real model on mcu30 (30 parts) and a 93-part QA board: 2.7 to 4.4 s from the request, from 10 to 50 s with the GLB export (now the optional "Exact
  export"). The Appearance manager is `appearance_controls_3D.cpp`'s layer tree (`kicad-port/appearance3d.ts`); stackup colours come from Board Setup (`StackupLayer.color`); hover highlight; animated view
  moves. The board slab and solder mask are extruded from the outline KiCad builds from Edge.Cuts. `e2e/viewer3d.check.js` (50 checks). Two stalls of the request loop were found by measuring and fixed.
- Missing: raytracing (out of scope); layer presets and viewports of the 3D viewer; the PCB editor's selection drawn green in the 3D view; the 3D Navigator; the Value and user fields (a footprint's own texts and the Reference are drawn since P1), off-board silkscreen;
  the PCB editor's per-layer copper colours on inner layers; X3D and IGES models; per-layer ambient and specular colours of the board; the preferences that set the animation speed; NPTH holes are not cut out of the slab
  (`aIncludeNPTHAsOutlines`, item 27); the Extruded 3D Body of a footprint (item 30).
- Round 4 (`task-r4-editors-exports`, V4-1, V4-2): the wheel is `OnMouseWheelCamera` with KiCad's default `aPan = false` (Shift pans along y, Ctrl along x, Alt and no modifier zoom; a fixed `0.7 * zoom` step), and the slab, copper and mask
  thicknesses and the models' mounting height come from the stackup (`BOARD_ADAPTER::InitSettings`, `GetFootprintZPos`; KiCad's default stackup when the board has none). Left: both masks are taken as enabled; the silkscreen keeps its own offset.

### 24. Item kinds the IR does not have
**Done in part (2026-10-21, `task-pcb-items`, package P16i, not merged yet): the board's text box, reference image, points, hatch fills and barcodes and the schematic's image are real items (tables are P16t's, on main, below); what is left is listed after.**
- Done (`PARITY-pcb.md` section 39, `PARITY-sch.md` section 36; `crates/model/src/{pcb_items,image_info,barcode}.rs`, `crates/kicad/src/{pcb_items_io,pcb_textbox,pcb_image,pcb_point}.rs`, `crates/ops/src/pcb_items.rs`, `crates/drc/src/hatch.rs`, `crates/shape-poly-set/src/{hatch,basic_shapes}.rs`, `crates/cli/src/{pcb_items_json,image_api,barcode_api}.rs`,
  `kicad-port/{pcbTextBox,pcbExtraItems,pcbItemProperties,refImage,shapeFill,barcodeShapes}.ts`, `components/{canvas/pcbItemsPaint.ts,canvas/barcodeCache.ts,BarcodeDialog.tsx}`):
  one additive field of `DrawingsSection`, `board_items`, holds the board's new kinds, with one `Cmd` variant for their verbs (`{"op": "pcb_item", "verb": "add_text_box" | "add_ref_image" | "set_shape_style" | "add_barcode" | ...}`).
  Step 1, the text box (`gr_text_box`): read and written as KiCad does (margins, border and stroke type, knockout, angle, a polygon when turned off the quarter turns), drawn on its layer and in the 3D silkscreen, Draw Text Box and Text Box Properties,
  the Properties rows, every verb that takes any item. Step 2, the images: `(image ..)` of a board and of a sheet are read and written with their pixels byte for byte (kicad-cli's rewrite keeps them), the size read from the header without a decoder;
  Place Reference Image, Place Image and the drop of an image file (`ddAddImage`) take a file from the browser, Reference Image Properties and Image Properties set the position, layer, lock, scale and size and Convert to Greyscale (the
  browser converts), the board draws an image under everything at its layer's alpha times the Images row of the Appearance panel (a new row), the sheet draws it opaque; a reference image is in no plot and kicad-cli's SVG plot of a sheet has the
  image where this studio puts it (slow tier). Step 3a, the point (`(point ..)`): read and written as KiCad does (no lock, not groupable), Place Point (repeating, snapping to the points there), drawn as an X and a ring, a snap anchor of its own
  for every placing tool and for drags, the Points row of the Appearance panel, kicad-cli keeps it and plots nothing of it (slow tier).
  Step 3b, the hatch fill: `(fill hatch)`, `(fill reverse_hatch)` and `(fill cross_hatch)` of a rectangle, circle or polygon are the board's `ShapeStyle` of the shape (the same enum a footprint's graphic has), read and written; `eda_drc::hatch` is `EDA_SHAPE::UpdateHatching` with `getHatchingKnockouts`
  (`SHAPE_POLY_SET::GenerateHatchLines` ported, the other shapes, texts, text boxes and the footprints' fields, graphics and courtyards cut the lines), worked out from the board each time it is described, which is what `rehatchShapes` does after every commit; the canvas draws the lines, a click on the outline or a line picks the shape,
  the 3D view draws them, the Fill row of the Properties panel and the Fill choice of Shape Properties have the five fills (`set_shape_style`), and Duplicate, Paste, Create Array, Move, Rotate, Flip and a handle drag keep the hatch. kicad-cli plots the same hatch polygons, to 2 micrometres, and a lattice of the same area for a cross-hatch (slow tier).
  Step 4, the barcode (`(barcode ..)`, a data item since P23): Add Barcode and Barcode Properties (with a preview), `add_barcode`, `edit_barcode` and `delete_barcode`, Move, Rotate, Flip, Lock and Delete, the Properties rows of `PCB_BARCODE_DESC`, drawn and picked; its shape is what kicad-cli plots of it (KiCad uses zint, which this studio has not and may not add): a board of the barcode alone is plotted and the polygons kept, by the barcode's parameters,
  and they are the board's own plot to 2 micrometres for every kind and for a turned, a mirrored and a knocked-out one (slow tier). Every step has Rust tests, `node --test` cases and scenarios of `web/studio/e2e/pcb-items.check.js` through `window.__eda`.
- Missing: Duplicate, Paste and Create Array of a text box, an image, a point or a barcode (a hatched shape has them); an image's pixels turned or mirrored by Rotate and Flip (a decoder is needed; the position
  and the layer follow); a text box, an image, a point or a barcode of a footprint (`fp_text_box`: part of the footprint, kept as its tokens); outline fonts and `render_cache` of a text box; a footprint's hatched graphic drawn with its lines (it is read and written hatched and drawn as the outline); the 3D view of a barcode; the shape of a barcode without kicad-cli (the box of the symbol, crossed).
- Still open from the original list (tables are done, below): the symbol text box (done in P17), derived symbols (done in P17), alternate pin
  functions (done in P17), design variants, generators, padstacks, component classes, tuning profiles, embedded files (done 2026-10-23, P26: `zstd` is the one dependency the user approved; `PARITY-pcb.md` section 47), each with its reason in
  `ui-parity-missing.json`. Add one when a work package needs it. Custom-shape pads left this list: they are item 31.
- **Tables are done (2026-10-21, `task-tables`, package P16t, not merged yet)**, limits below: they are no longer open in the list above, and the 18 rows of `ui-parity-missing.json` for them are gone (`PARITY-pcb.md` section 38, `PARITY-sch.md` section 35).
  A table is an IR item of the board (`DrawingsSection::tables`, `eda_model::table`) and of the sheet (`SchExtras::tables`): the grid, the border and separator strokes, each cell's text, margins, justification and merge (and a sheet cell's fill), read and written as the `(table ...)` of
  `.kicad_pcb` and `.kicad_sch` (`crates/kicad/src/{pcb_table,sch_table_io}.rs`; the re-export of KiCad's own boards stays at 12 of 12 in both formats). A cell's shown text is KiCad's (`${ROW}`, `${COL}`, `${ADDR}`, `${CELL(..)}`, `${LAYER}`, then the board's or the sheet's variables;
  `table_text.rs`, `text_vars.rs`) and the server places every line of it as `GetDrawPos` and `GetLinePositions` do (`crates/cli/src/table_json.rs`), so the canvas draws what kicad-cli plots. Every edit is one undoable `/api/cmd` verb the two editors share (`Cmd::Table`, `Cmd::SchTable`: `TableCmd`
  Add, Draw, PlaceStackup, PlaceCharacteristics, Delete, Set, EditCells, AddRow, AddColumn, DeleteRows, DeleteColumns, MergeCells, UnmergeCells; `crates/ops/src/pcb_tables.rs`), and a table is moved, turned (on a sheet between its two attitudes, `SCH_TABLE::Rotate`), flipped, locked, grouped and deleted
  with the generic verbs, a selected cell standing for its table in Move and Rotate (`aPromoteCellSelections`). In both editors: Draw Table (two clicks, then Table Properties), Table Properties, Table Cell Properties, Edit Table, Add Row Above/Below, Add Column Before/After, Delete Row(s)/Column(s), Merge and Unmerge
  Cells, Select Column(s)/Row(s)/Table, Export Table to CSV, the context menu's table entries, the Properties panel's rows (`PCB_TABLE_DESC`, `PCB_TABLECELL_DESC`, `SCH_TABLE_DESC`, `SCH_TABLECELL_DESC`), a click that picks a cell and a window that picks a table; on the board Place > Add Stackup Table and Add Board
  Characteristics Table (`Build_Board_Stackup_Table`, `Build_Board_Characteristics_Table`: the cells KiCad builds, `PCB_TABLE::Autosize` from the ink of the text, the numbers kicad-cli's `pcb export stats` measures). Tests: 74 Rust cases (`crates/{model,kicad,ops,cli,kicad-engine}`), 41 `node --test` cases, `web/studio/e2e/tables.check.js` (5 scenarios, 59 checks through `window.__eda`);
  the kicad-cli proofs (`EDA_SLOW_TESTS=1`): the two tables Place builds are plotted whole with their borders where `DrawBorders` puts them (`crates/cli/src/table_tests.rs`), a sheet table of ours is plotted as KiCad's own within 4 um, standing and lying, and the table of KiCad's QA sheet `BusAndVectors` comes back through our reader and writer plotted as it was
  (`crates/kicad-engine/tests/tables_sch.rs`).
- Left of the tables: Copy, Cut, Paste and Duplicate of a table or of cells (both editors; Copy says "nothing in the selection can be copied"; the table tools' `pasteCellsIntoSelection` is not ported); a drag across cells that selects the block under it
  (`SelectCellsBetween`: cells are added one at a time with a modifier click, or by Select Row(s) and Select Column(s)); on a sheet the `@{expression}` of a cell is shown as typed, a `${REF:FIELD}` that names another symbol is not turned into the symbol's id (`ConvertRefsToKIIDs`) and so does not follow a renamed reference, and the Table Cell Properties dialog has
  no fill and no text colour (the Properties panel's Background Fill and Background Fill Color edit the fill; the colour, the hyperlink and a font face are kept by the reader and the writer and edited nowhere); a flipped board table is the mirror image of the table (`PCB_TABLE::Flip` ends with a translation that is not reproduced); the stackup table has no sublayers (the studio's
  stackup has none) and prints the layer names as the studio holds them; a table inside a footprint is carried as text, and the Footprint Editor has no Draw Table.
- **The multichannel tool is done (2026-10-21, `task-multichannel`, package P24, not merged yet)**, limits below: the placement rule area, Generate Placement Rule Areas, Repeat Layout and Apply Design Block Layout; their three rows are out of `ui-parity-missing.json` (`PARITY-pcb.md` section 40, `STALE-REASONS.md`).
  A placement rule area is a zone that names a sheet, a component class or a group (`Zone::placement`, `eda_model::multichannel`; `(placement (enabled ..) (sheetname|component_class|group ".."))` read and written as KiCad does, kicad-cli keeps it), with the Placement section of Rule Area Properties and the Placement group of the Properties panel.
  Generate Placement Rule Areas (the dialog's three tabs; Replace existing; Group footprints) makes the area KiCad makes -- the rectified hull of the boxes of the footprints and 0.1 mm, a staircase, the corners of KiCad's own area to the micron -- Repeat Layout (the reference from the selection, a group or the picker; the table of targets with each match and KiCad's words for a mismatch; the anchor footprint; the six options, kept) copies
  the placement, the routing, the zones and the other items of the area onto the other channels, matched by topology (`eda_model::topo_match`, `TMATCH` with its step limit and its reasons), on the nets of the target's pads, and Apply Design Block Layout does the same from a design block's layout to a group linked to it. Each is one undoable `Cmd::Multichannel` verb (`crates/ops/src/multichannel*.rs`) with three read routes for the dialogs (`crates/cli/src/multichannel_api.rs`).
  Where a footprint's sheet comes from: the file it was imported from, the schematic (a sheet placed more than once has each placement's symbols) or the module sheets the studio derives from the intent. Tests: 16 model, 28 ops and 3 cli Rust cases, 7 `node --test` cases, `web/studio/e2e/multichannel.check.js` (17 scenarios, 157 checks through `window.__eda`, on `e2e/multichannel-board.yaml`); the kicad-cli proofs (`EDA_SLOW_TESTS=1`): the DRC of three channels, one routed and the others repeated from it, has no violation the first does not have and its open nets closed, kicad-cli's rewrite of the rule areas keeps their placement and corners (`crates/cli/src/multichannel_board_tests.rs`), and KiCad's QA multichannel board is read, its area made again to the micron and written back (`crates/ops/tests/multichannel_kicad_qa.rs`, `KICAD_SRC_DIR`).
- Left of the multichannel tool: a board made here, written to a `.kicad_pcb`, has no `(sheetname)` or `(sheetfile)` on its footprints (an imported footprint keeps its own), so KiCad's own Repeat Layout on such a file finds the areas' sheets empty until the netlist is updated; a class given by a `Custom` assignment condition (a DRC expression) is given to no footprint; Repeat Layout and Apply Design Block Layout do not copy tuning patterns, barcodes, reference images or tables
  (tracks, arcs, vias, zones, shapes with their hatch, texts, text boxes, dimensions and points are copied); a text or a dimension is judged inside an area by the corners of its box and a track by its centre line and width, where KiCad subtracts the polygons; the anchor of Apply Design Block Layout is the first pair in the block file's order (KiCad's is the first of a set of pointers); the options of the dialogs are kept for the session of the page, not in a settings file; a board made from an intent has one module sheet for each module, a sheet placed several times is known from an imported or stored schematic.

### 25. Footprints on the board carry no graphics
**Done (2026-10-10, `task-fp-graphics`, not merged yet)**, limits below. Hit: every board. Blocks: no longer. Package P1, size L.
- Done (`PARITY-pcb.md` section 28; `crates/model/src/fp_art.rs`, `crates/kicad/src/fp_art_io.rs`, `crates/cli/src/fp_art_json.rs`, `crates/drc/src/fp_art.rs`, `components/canvas/fpArtPaint.ts`, `kicad-port/fpArt.ts`, `viewer3d/scene.ts`):
  a footprint's graphics and texts (`fp_line`, `fp_arc`, `fp_circle`, `fp_rect`, `fp_poly`, `fp_curve`, `fp_text user`: the silkscreen outline, the pin-1 mark, the fab body, the assembly drawings) are `Footprint::art`, in the
  frame of the pads, so a move, a turn or a flip carries them. Read from `.kicad_mod` and `.kicad_pcb` and written by `write_footprint` as `format( FOOTPRINT )` does (a bottom-side footprint mirrored, on the other
  side's layers); built-in packages (`0603`, `SOIC-8`) get the art of the installed KiCad footprint they stand for. kicad-cli's SVG plot of `F.SilkS`, `B.SilkS`, `F.Fab` and `B.Fab` is the same before and after a round trip
  on six QA boards (24,090 primitives, `complex_hierarchy` among them), a synthetic board and a board made from an example (slow tier). The canvas draws them by layer under the Appearance rows, a click on a stroke picks
  the footprint, the 3D scene draws the silkscreen and the drawings on the other flat layers, the in-house board build (`eda_drc::DrcBoard`) reads them, and the importer no longer keeps a footprint's graphics and
  texts in `drawings.footprint_extras`. A pad on the silkscreen (a legacy library's through-hole pads) is written back with its layer (`PadMaskInfo::art_layers`). Measured: `complex_hierarchy`'s `silk_over_copper`,
  `silk_overlap` and `silk_edge_clearance` are the original's after our export (104, 199, 14; they were 2, 5, 2), and `component_classes` and `api_kitchen_sink`'s silkscreen counts too.
- Missing: the hatch lines of a hatched fill (`EDA_SHAPE::UpdateHatching`; drawn as the outline); the footprint editor's model has no field for a stroke type, a hatch, a vertical justification, bold, italic or keep upright, so a
  footprint saved from the editor loses them; no verb edits a footprint's graphics on the board (the footprint editor does); the title block's variables in a footprint text; the mask-only pads (`FootprintExtra::graphics`
  with a pad number) are not written back; a library footprint's pads on the silkscreen are not read and Pad Properties has no field for a pad's technical layers; the Value and user fields are not in the 3D view.
- Port from: `pcbnew/footprint.cpp`, `pcbnew/pcb_shape.cpp`, `pcbnew/pcb_text.cpp`, `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` (`format( FOOTPRINT )`), `pcb_io_kicad_sexpr_parser.cpp`
  (`parseFOOTPRINT_unchecked`), `pcbnew/pcb_painter.cpp` (`draw( FOOTPRINT )`).

### 26. Footprint assignment, exchange and reference rename
**Done (2026-10-12, `task-fp-assign`, not merged yet; Swap Pad Nets and Swap Gate Nets 2026-10-21, `task-pad-details`, P25)**, limits below. Hit: every project that changes a footprint or renumbers a part. Blocks: no longer. Package P2, size L.
- Done (`PARITY-pcb.md` section 30; `crates/model/src/{fp_assign,fp_exchange,fp_compare,reannotate}.rs`, `crates/ops/src/{part_rename,fp_exchange,reannotate}.rs`, `crates/cli/src/{library_place,fp_assign_api}.rs`,
  `components/{PcbGlobalEditDialogs,FootprintDiffDialog,ReannotateDialog,UpdatePcbDialog}.tsx`, `kicad-port/{pcbExchange,updatePcb,footprintDiff,reannotate,fpInsert}.ts`), the seven steps, each its own commit (`7af8ee7`, `a81dfd2`, `4d01fe8`, `9de882f`, `a8f4b22`, `65f2fa1`, `4a3f088`):
  (1) a rename carries the board's part (`Board::rename_part`: pose, footprint, the pads the routing ends on, net pins, fields and edits, zone overrides, lock, groups; the intent's part in `drawings.renamed_parts`) from Edit Reference,
  Annotate with reset and Increment Annotations, in one undo step on both editors (the history's `both` tag, `Cmd::spans_both_editors`); (2) `DrawingsSection::footprint_assignments`: the schematic symbol's Footprint field
  is the board's footprint (`replaceFootprint`: Symbol Properties, Assign Footprints, the Symbol Fields Table), Update PCB from Schematic (F8) lists and applies what the board could not take (a locked footprint, a
  footprint no library has) and Update Schematic from PCB carries the board's back; (3) Change Footprint(s) with the dialog's options (`Cmd::ExchangeFootprints`, `ExchangeFootprint` ported with its pad alignment and its
  text matching), four actions and the Footprint Properties buttons; (4) Update Footprints from Library reads the library copy through the tables and lists what differs (`FootprintNeedsUpdate`, `eda_model::fp_compare`);
  (5) Compare Footprint with Library (the Summary and Visual pages, the slider, the update it opens); (6) Geographical Reannotate (`DIALOG_BOARD_REANNOTATE` ported, `Cmd::ReannotateBoard`, the symbols follow); (7) Insert
  footprint into PCB adds a part of its own where no part names the footprint. Every edit is an undoable `/api/cmd` verb; the IR additions are additive overlays with serde defaults.
- Measured: 76 new Rust tests and 28 `node --test` cases; kicad-cli's `--schematic-parity` (slow tier) says the same after a rename, after a Change Footprint and after a reannotation as it said before, and the derived
  `.kicad_pcb` and `.kicad_sch` name the new references; `web/studio/e2e/fp-assign.check.js` runs 8 scenarios (69 checks) through `window.__eda` (port 8824, a board made from `examples/mcu_board_30plus.yaml`).
- Done on `task-pad-details` (P25, `PARITY-pcb.md` section 45; `crates/ops/src/pad_nets.rs`, `crates/connectivity/src/pad_cluster.rs`, `crates/model/src/fp_units.rs`, `crates/cli/src/pad_nets_api.rs`, `components/SwapNetsDialog.tsx`, `kicad-port/gateSwap.ts`): Swap Pad Nets and Swap Gate Nets. The verbs rotate the nets of the
  selected pads by moving their pins between the nets of `design.nets`, with the tracks, vias and pads of the cluster each pad is in (`CONNECTIVITY_DATA::GetConnectedItems`) taking the new net; KiCad's question about the pads nobody selected is asked (`POST /api/swap_nets/preview`, `connected_pads`); a footprint's `(units ..)`
  is read, written and derived from its symbol (`LIB_SYMBOL::GetUnitPinInfo`); the context menu has both and "Swap with <unit>". It makes the board disagree with the schematic, as in KiCad: kicad-cli's `--schematic-parity` reports the swapped pads (slow tier), and the next schematic edit that changes
  connectivity (`Cmd::edits_connectivity`) follows the drawing's difference only (`eda_engine::nets::follow`), so a swap survives it unless that edit touches the same nets. Left: Update Schematic from PCB does not carry a pin swap back into the drawing, the pads only a zone fill joins to the swapped ones keep their net (the fills are kicad-cli's), a unit's custom display name.
  Measured: 18 new Rust tests (one slow-tier: kicad-cli's `--schematic-parity` reports one `net_conflict` for each swapped pad -- "Pad net (VCC) doesn't match net given by schematic (GND)" and the reverse -- and nothing after the undo), 7 `node --test` cases, and `web/studio/e2e/pad-details.check.js` on a 74HC00's board: the swap of gate A with gate B (`Swap with B`) exchanges the nets of pins 2/1/3 and 5/4/6 position by position, in one undo step.
  Left from P2: Pad nets of an exchange are kept by pad number only (KiCad also matches pads that share no number by position). `FootprintNeedsUpdate` compares what the model keeps of a library footprint,
  so the file's attribute flags, net-tie groups, rule areas and stackup layers are not reported. Insert footprint into PCB places the footprint at the centre of the board instead of carrying it on the cursor. Import
  Netlist (a part from a netlist has no schematic symbol).
  Round 5 (`task-r5-board-io`): Compare Footprint with Library also compares each pad's layer set ("Pad N layers differ.") and, always, its die length and fabrication property, as `padNeedsUpdate`. **Left:** the model does not carry a library pad's layer set, die length or fabrication property onto a footprint placed here or updated from the library (the writer writes its kind's default layers, `F.Cu F.Paste F.Mask` or `*.Cu *.Mask`, and no die length or property), so a library pad without paste, with a die length or with a property is reported as differing after the update too, which is what kicad-cli's library parity says of the file we write. The sets are compared without the board's enabled layers (`getBoardNormalizedLayerSet`) and as the default footprint stackup (`EXPAND_INNER_LAYERS`).
- Port from: `pcbnew/dialogs/dialog_exchange_footprints.cpp`, `pcbnew/pcb_edit_frame.cpp` (`ExchangeFootprint`), `pcbnew/netlist_reader/board_netlist_updater.cpp` (`replaceFootprint`,
  `updateFootprintParameters`, `updateComponentPadConnections`), `pcbnew/tools/global_edit_tool.cpp` (`ExchangeFootprints`), `pcbnew/tools/board_reannotate_tool.cpp`,
  `pcbnew/dialogs/dialog_board_reannotate.cpp`, `pcbnew/footprint.cpp` and `pcbnew/drc/drc_test_provider_library_parity.cpp` (`FootprintNeedsUpdate`), `pcbnew/tools/board_inspection_tool.cpp` (`DiffFootprint`),
  `eeschema/tools/assign_footprints.cpp`, `eeschema/tools/backannotate.cpp`, `pcbnew/tools/edit_tool_move_fct.cpp` (`SwapPadNets`).

### 27. Board outline leftovers
**New, open.** Hit: boards with cutouts, slots or rounded corners. Blocks: no. Package P12, size M. From `PARITY-pcb.md` section 25 ("Not done") and what `crates/zone-filler/PARITY.md` (deleted 2026-10-13) said of the outline.
- The placer, the DRC lint and the judge still measure a footprint by the symmetric courtyard box (`placed_courtyard`); the studio, Align, Distribute and the 3D tab take the real one (`placed_courtyard_box`, fix batch F). Follow-up: move those to the real box.
- The autoplacer (`crates/place`) still works from the outline's bounding box: a part can be placed inside a cutout's box. The placement verbs (`Cmd::PlaceAt`, `Place`, `PlaceRegion`, `PlaceEdge`) test the courtyard against the board's real outlines with their cutouts (`GetBoardPolygonOutlines`, fix batch F 2026-10-24); their copper-to-edge band is still measured to the box.
- The Freerouting DSN boundary (`eda_freeroute::design::to_dsn`) is the summary polygon, with no cutouts, and the autoroute works inside it. (File > Export > Specctra DSN, P19b, is another writer, `FromBOARD`'s: its boundary is the Edge.Cuts outline with its cutouts.)
- NPTH holes are not cut out of the 3D slab (`aIncludeNPTHAsOutlines`); a mask-only NPTH hole is kept (`FootprintExtra::holes`) and written, so kicad-cli's fill keeps clear of it (item 13).
- The arcs of a `gr_poly` are not in the IR (a rectangle's corner radius is, 2026-10-24: `ShapeStyle::corner_radius`, `PARITY-pcb.md` section 25); there is no outline-specific editor (the generic shape tools draw and edit Edge.Cuts); "Fix discontinuities in board outlines" in
  Update PCB from Schematic has nothing behind it; the footprint pass of `BuildBoardPolygonOutlines` (`isCopperOutside`) is the ordinary nesting test here, and a footprint's Edge.Cuts do not follow it when it
  moves.
- Port from: `pcbnew/convert_shape_list_to_polygon.cpp`, `pcbnew/board.cpp` (`GetBoardPolygonOutlines`, its `aIncludeNPTHAsOutlines` argument), `pcbnew/specctra_import_export/specctra_export.cpp`,
  `pcbnew/drc/drc_test_provider_misc.cpp`.

### 28. Bringing files in
**Mostly done (2026-10-11, `task-files-in`; another EDA's files 2026-10-15, `task-import-foreign`, package P22)**, limits below. Hit: sometimes; a DXF outline is common, and a project from Eagle or Altium comes in through another EDA's files. Package P8 for what is left (Import Netlist).
- Done (`PARITY-pcb.md` section 29, `PARITY-sch.md` section 20, `PARITY-symedit.md`, `PARITY-common.md` section 13a): **Import Graphics** for the board, the schematic and the Symbol Editor, from a DXF (LINE, ARC, CIRCLE, POINT, LWPOLYLINE and POLYLINE with bulges, SPLINE, ELLIPSE,
  TEXT and MTEXT, BLOCK and INSERT, `$INSUNITS`, lineweights) or an SVG (paths with lines, cubic and quadratic curves and arcs, rect, circle, ellipse, line, polyline, polygon, transforms, fills with holes). The readers are hand-written, no new dependency
  (`crates/kicad/src/{graphics_import,dxf_import,svg_import}.rs`), the verbs are `Cmd::ImportGraphics` (the layer, the scale, the origin with KiCad's own arithmetic, the default line width and unit, Group imported items, Fix discontinuities with `ConnectBoardShapes`, and a studio option that makes
  an imported Edge.Cuts the board's outline), `Cmd::ImportSchGraphics` and `Cmd::ImportSymbolGraphics` (`crates/ops/src/{import_graphics,import_graphics_sch,fix_shapes}.rs`), each one undo step; the dialog is `components/ImportGraphicsDialog.tsx`.
  kicad-cli's DRC finds a DXF-imported Edge.Cuts outline closed, and reports `invalid_outline` with a side missing (`EDA_SLOW_TESTS=1`). **Append Board** (`crates/cli/src/append_api.rs`: the file read as a paste, the point it is held by, what the paste will change; then `Cmd::PasteClipboard` at the cursor, carried by the Move tool, one undo step).
  **The drops** (`components/FileDropHost.tsx`, `kicad-port/fileDrops.ts`: each editor's `m_acceptedExts`): a `.kicad_pcb` on the board, a `.dxf` or `.svg` on the board and the schematic, a `.kicad_mod` on the Footprint Editor.
  Browser check `e2e/files-in.check.js`: 102 checks through `window.__eda`.
- Done 2026-10-15 (`task-import-foreign`, package P22; `PARITY-pcb.md` section 34, `PARITY-sch.md` section 33): **other EDA formats** (Eagle, Altium, CADSTAR, PADS, P-CAD, Fabmaster, EasyEDA, ...). kicad-cli 10.99 has `pcb import` and `sch import`, which run KiCad's own `PCB_IO` / `SCH_IO` plugins behind a command line, so no importer is written here and no dependency is added:
  `eda_kicad_engine::foreign` (`import_foreign_pcb`, `import_foreign_sch`) runs them through `cli_command` in a folder of the caller's and returns the KiCad file, kicad-cli's JSON report (the format it recognised, what it converted, where every source layer went) and the importers' own error and warning lines; our readers turn the file into the design.
  **File > Import > Non-KiCad Board File...** and **Non-KiCad Schematic File...** (`POST /api/import_foreign`: the file is an upload, never a path on the server) **replace** the open board or schematic as `OpenProjectFiles` and `OnImportProject` do, as `Cmd::ReplaceBoard` / `Cmd::ReplaceSchematic`: one undo step that brings the old one back with its model (the imported board's parts, nets, footprints and rules are a file under `imports/` that `DrawingsSection::intent_file` names).
  The layer dialog (`DIALOG_MAP_LAYERS`, answered through kicad-cli's `--layer-map`), Keep KiCad layer names and the Import Issues report are ported; the design rules and net classes an importer set come from the `.kicad_pro` that `kicad-cli import` writes. `eda import-kicad <file> -o <dir>` takes the same files (and pairs an Eagle or CADSTAR board with its schematic by name, an Altium `.PrjPcb` with its documents; kicad-cli pairs nothing).
  Held to kicad-cli's own output on KiCad's QA files (`EDA_SLOW_TESTS=1`): the footprints, nets, tracks and vias of 52 boards and the symbols of 5 schematics that our readers make equal the converted file's (this found two gaps in the reader, fixed: a KiCad 10 net named `"0"`, and a net only copper uses). Browser check `e2e/import-foreign.check.js` (53 checks through `window.__eda`).
- Left: **Import Netlist** (P8c; the parts and nets come from the intent); (`DdAppendFile`, a schematic dropped on the sheet, is done since 2026-10-21, `task-microwave`: item 4; `DdAddImage` since P16i: item 24;) Import Graphics in the Footprint Editor (`placeImportedGraphics` there);
  the placement preview (the picture is on the board while the Move tool carries it, in a step held open: one undo step with its drop, Escape leaves nothing; KiCad previews it first; the Symbol Editor's items are added at the cursor and selected); an Append Board that adds the file's layers and keeps every net as KiCad does (the paste's rules apply: copper on a layer this board lacks is left out, an item on a net nobody brings has none; a repeated reference is shown as the file has it, as KiCad leaves it, but filed under a free one);
  several files in one drop (the first is opened, the rest said to be skipped; a drop of schematics takes them all, item 4); the DXF's text keeps the file's height only, a symbol has no curves (a curve is cut into segments), a shape with no stroke is a hairline, an SVG pattern and gradient are not drawn, SVG `text`, `image` and `use` are reported and not imported.
- Left, other formats (P22): the layers a source layer may go to (`PermittedLayers`) are not in kicad-cli's report, so the layer dialog offers every layer for every source layer; a layer map can say where a layer goes and not leave it out, so an unmatched layer that has a guess keeps it (the dialog says which); Cancel abandons the import, where KiCad's dialog has no way out but OK.
  The studio takes one file at a time (an Eagle `.brd` and its `.sch` are two imports; the CLI imports the pair, and an Altium `.PrjPcb` with its sheets); an Altium project's variants are not read (the IR has none); an EasyEDA Pro project of several boards stops kicad-cli (`bad_function_call`: KiCad asks the person which board), and IPC-2581 and ODB++ boards have no importer in this kicad-cli.
  KiCad asks "Save changes?" before replacing the board and cannot take the import back; here nothing is lost and the toast says that Undo brings the old board back. The library kicad-cli makes of the imported footprints (`*-import-fps.pretty`) and symbols is kept by the CLI and not by the studio's route.
- Left, KiCad projects (round 2 F12, judged 2026-10-27 on `task-r2-board-io`): `eda import-kicad x.kicad_pcb` reads the board alone and not the `x.kicad_sch` beside it, and the export links every footprint to the derived schematic. The links are by design (the KiCad files are derived, `docs/ARCHITECTURE.md`; the derived schematic is the project's). Pairing the schematic as Eagle and CADSTAR pair would mean Replace Schematic on import, a netlist update that KiCad's Open Project does not make, so it is not done; a KiCad schematic is imported by itself (`eda import-kicad x.kicad_sch`).
- Port from: `common/import_gfx/{dxf_import_plugin,svg_import_plugin,graphics_importer,graphics_importer_buffer}.cpp`, `pcbnew/import_gfx/{dialog_import_graphics,graphics_importer_pcbnew}.cpp`,
  `eeschema/import_gfx/`, `pcbnew/tools/pcb_control.cpp` (`AppendBoard`), `board_editor_control.cpp` (`ImportNetlist`), `eeschema/tools/sch_drawing_tools.cpp` (`ImportSheet`),
  `pcbnew/files.cpp` (`OpenProjectFiles`), `pcbnew/dialogs/dialog_map_layers.cpp`, `eeschema/files-io.cpp` (`importFile`, `OnImportProject`).

### 29. Search and inspection tools
**Done 2026-10-13 on `task-search-inspect` (not merged); what is left below.** Hit: occasionally; Clearance Resolution is how a DRC error is understood. Blocks: no. Package P11, size M-L.
- **The Net Inspector** (fix batch F, 2026-10-24, `PARITY-pcb.md` section 32d): KiCad's columns and `CalculateLengthDetails`' numbers (`crates/cli/src/net_inspector_api.rs`), the filter, the sort and the report. Left: a dialog, not a docked panel; no Group by Netclass or custom groups, no time-domain columns, no Rename / Delete / Add Net.
- Done (`PARITY-pcb.md` sections 32a to 32c, `PARITY-fpedit.md` section 12): Find by Properties and Find following the IR (item 21); **Inspect > Clearance Resolution and Constraints Resolution**: `crates/drc` now holds KiCad's PCB expression language
  (`libeval.rs`, `pcbexpr.rs`: the lexer and grammar, the code generator, the stack machine and the built-in functions), the `.kicad_dru` parser (`dru.rs`, `DRC_RULES_PARSER`) and `DRC_ENGINE::EvalRules` (`constraints.rs`, `RuleEngine::eval_rules`: the implicit rules
  of the board setup, the net classes and the composite ones, the diff pairs and the keepout areas, then the rules file in order, the local overrides of a pad and a footprint and a zone's own clearance, net ties, the board minimum and the assertions) in one function that returns
  the winning constraint and every rule it considered with KiCad's report text; `rule_board.rs` makes the items and the rules of a design, `inspect.rs` the report pages (`reportClearance`, `InspectConstraints`), `crates/cli/src/inspect_api.rs` the route
  (`POST /api/inspect/{clearance,constraints}`), `components/InspectReportDialog.tsx` the two dialogs and `actions/inspectActions.ts` the commands with KiCad's selection and picker rules. This inspects the rules; kicad-cli stays the only DRC engine.
  Proof against kicad-cli (`EDA_SLOW_TESTS=1`): for a net class, an explicit rule and a loosening rule the clearance the report resolves is the one kicad-cli's DRC enforces (a violation 10 um under it, none 10 um over); the rule sets of
  `qa/data/pcbnew` (38 `.kicad_dru` files) parse and four of them resolve to the winner their rules say. **The Footprint Checker** (`pcbnew.ModuleEditor.checkFootprint`): `crates/kicad-engine/src/footprint_check.rs` puts the footprint being edited on a scratch board
  (its `.kicad_mod`, a net per pad number, the Footprint Editor's severities), runs `kicad-cli pcb drc` on it through the kicad lane and returns the violations of the footprint-level checks (courtyard, attributes, pads, shorting pads, holes, net ties,
  clipped silk) as markers with the editor's own item ids; the dialog (Show boxes and badges, the marker list, Delete Marker, Delete All Markers, Run Checks) and the markers on the footprint canvas.
- Done 2026-10-28 (`task-r5-rules-zones`, round 5 R5-D1..D5, `PARITY-pcb.md` section 59): the rule engine's footprints have their sheet (`GetSheetname`: the file's, else the symbol link's), their fields
  (`GetField`: Reference, Value, Datasheet, Description, the user's; no "Footprint" field) and their component class (the file's classes, the project's assignment rules and sheet classes compiled from
  `GetAssignmentInDRCLanguage` and evaluated on each footprint; `A.ComponentClass` compares by constituent), so `memberOfSheet`, `getField` and `hasComponentClass` hold in the router, Clearance Resolution and the via
  stitching as in kicad-cli (proof: `crates/pns/tests/rules_kicad.rs`); the router's copper barcodes, text boxes, tables and leaders are "Barcode", "Text Box", "Table" and "Leader" to the rules, so the barcodes'
  implicit 1 mm physical clearance holds; `existsOnLayer('')` compiles silently; the net class and component class functions' missing-argument words; the stand-in pad of a footprint is its last logical pad.
- Missing: the implicit rules of the tuning profiles (`addTuningSingleRule`; the profiles are in the IR since P9a); `fromTo`; Find by Properties' Query still sends no sheet path or class (`crates/drc/src/query.rs`,
  item 21); a custom assignment condition that itself asks `hasComponentClass` sees the file's classes only; properties KiCad has and the IR does not (a rule that names one compiles and reads it as undefined); micro vias (the IR has no via type: a via is through, blind or buried by its layers), a footprint's own paste margin and the net-tie
  cache of a footprint's copper graphics; text variables and cross references in the rules text are not expanded (`${VAR}` is an unresolved variable here); a footprint field (`REF:Name`) cannot be inspected; the courtyard report notice (`MALFORMED_COURTYARDS`) is never set;
  the geometry functions treat touching as intersecting (KiCad deflates the area by an epsilon). (The DRC Rule Editor: done, item 3, P9b, `PARITY-pcb.md` section 48.)
  The Footprint Checker's markers are where the first item of the violation is (kicad-cli's report has no marker position; KiCad puts a short at the contact point); a footprint's Reference and Value are not on the scratch board (a hidden `REF**` stands in).
- Port from: `pcbnew/dialogs/dialog_find_by_properties.cpp`, `pcbnew/pcbexpr_evaluator.cpp`, `pcbexpr_functions.cpp`, `common/libeval_compiler/`, `pcbnew/tools/board_inspection_tool.cpp` (`InspectClearance`, `InspectConstraints`),
  `pcbnew/drc/{drc_engine,drc_rule_parser,drc_rule_condition}.cpp` (`EvalRules`, `loadImplicitRules`), `common/dsnlexer.cpp`, `pcbnew/dialogs/dialog_footprint_checker.cpp`, `pcbnew/footprint.cpp` (`CheckPads`, `CheckShortingPads`, `CheckNetTies`).

### 30. The 3D models of a footprint
**Done (2026-10-13, `task-fp-3d`, not merged yet)**, limits below. Hit: whoever makes a footprint or fits a different 3D model. Blocks: no longer. Package P14, size M. From `PARITY-3d.md` and `PARITY-fpedit.md`.
- Done (`PARITY-pcb.md` section 31, `PARITY-fpedit.md` section 11; `crates/model/src/{fp_models,model_subst,fp_edit,ir}.rs`, `crates/ops/src/fp_models.rs`, `crates/kicad/src/{footprint_lib,footprint_import,pcb,model3d}.rs`, `crates/cli/src/model3d_tab_api.rs`,
  `components/{footprint/FootprintModelsPanel,footprint/ModelPreview3D,footprint/Model3dChooserDialog,Migrate3dModelsDialog,FootprintPropertiesDialog,FootprintChooserDialog}.tsx`, `kicad-port/{fpModels,migrate3d}.ts`), the five steps, each its own commit (`233a2af`, `58ee472`, `0ebde56`, `4bb96b8`, `7a327b6` and `3bcf0bc`):
  (1) `FootprintEdit::models3d`, the whole list on the board (`None` is the footprint's own, `Some` what was set, an emptied list included: `ConstraintModel::edited_models`), laid over the footprint by `edited_footprint` and its verbs (`set_board_models`, `add_board_model`, `remove_board_model`,
  `move_board_model`, `edit_board_model`, the list in `edit_footprint_properties`), one undo step each, a refusal with its reason (`ops_bad_model`, `ops_bad_model_index`, `ops_nothing_to_change`: no empty step), the file name checked as `FILENAME_RESOLVER::ValidateFileName` checks it and the numbers as
  the preview panel's fields take them; the `.kicad_pcb` entries as `format( FOOTPRINT )` writes them (hide, opacity, `FormatDouble2Str`) and read back, a bottom-side footprint's placement in KiCad's frame; the 3D viewer and the exports draw the list; (2) the 3D Models tab of the board's Footprint Properties (the grid with the status of each
  file name, Add, Browse through the model chooser and the library path variables, Remove, Configure Paths..., Scale, Rotation and Offset with their spin buttons, wheel and expressions, the Opacity slider, the preview that reuses `viewer3d/modelCache.ts`) and of the Footprint Editor's; (3) the library footprint keeps a list (`LibraryFootprint::models3d`, `.kicad_mod`
  import and export, the editor's dialog); (4) the Footprint Chooser's 3D view and a footprint's models reaching the board with it; (5) Migrate 3D Models (`eda_model::model_subst`, `GET /api/3dmodel/migrate`, `migrate_board_models`, the dialog: missing references, ranked STEP candidates, Add Search Directory..., Open External File..., preview, Replace in one step).
- Measured: 52 new Rust tests (one slow-tier: kicad-cli's `pcb upgrade` writes a board with two models -- offsets, scales, rotations, an opacity, one hidden -- with the same `(model ...)` entries character for character as it reads in ours, and `pcb export vrml` of both is the same) and 20 `node --test` cases;
  `web/studio/e2e/fp-models.check.js` runs 11 scenarios (153 checks) through `window.__eda` (port 8826, a board made from `examples/mcu_board_30plus.yaml`).
- Left: the Extruded 3D Body box of the tab (a file's body is kept on import as the file wrote it, 2026-10-24, findings p13-p16 P14-1; `EXTRUDED_3D_BODY`: a body from the courtyard, fab or silkscreen outline or the pins' box, its height, standoff, material and colour, its STEP export) (the embedded files -- `PANEL_EMBEDDED_FILES`, `kicad-embed://` models resolved from the board's and the footprints' files, Collect and Embed 3D Models -- are done 2026-10-23, P26, `PARITY-pcb.md` section 47);
  Migrate 3D Models' load-time half (`AutoMigrateByFilename` and the info bar "N WRL 3D models could not be matched", which `pcb_edit_frame.cpp` runs when a board opens) and a folder picker over the whole disk (the studio lists the folders it may read: KiCad's library, the board's, the path variables'); "Show 3D viewer in own window" of the Footprint Chooser; IGES and X3D models in the
  viewer and the chooser (a candidate may be an IGES file); for a bottom-side footprint the tab shows the numbers in the footprint's top-side frame where KiCad shows the file's (a half turn about z apart; the file written is KiCad's); a built-in package (`0603`) opened in the Footprint Editor has no model list (the generic model is added only when a footprint is placed).
- Port from: `pcbnew/dialogs/panel_fp_properties_3d_model.cpp`, `dialog_footprint_properties.cpp`, `dialog_footprint_properties_fp_editor.cpp`, `dialog_migrate_3d_models.cpp`, `3d-viewer/dialogs/{panel_preview_3d_model,dialog_select_3d_model}.cpp`, `3d-viewer/3d_cache/`, `pcbnew/footprint_chooser_frame.cpp`.

### 31. Custom-shape pads
**Done (2026-10-10, `task-custom-pads`; Explode and Recombine, padstacks and Remove Unused Pads 2026-10-21, `task-pad-details`, P25)**, limits below. Hit: footprints with custom pads, imported boards, the zone fill around them. Blocks: no. Package P7.
- Done (`PARITY-pcb.md` section 27, `PARITY-fpedit.md` section 10): A pad keeps its real shape in the IR (`Pad::ext`, `LibraryPad::custom`); the `.kicad_pcb` and `.kicad_mod`
  readers and writers are `pcb_io_kicad_sexpr`'s (kicad-cli's plot of the original and of our export is identical, for a board and for a library footprint); and the DRC, the routing gate, the router, connectivity and the zone filler
  measure that copper (`eda_drc::pad_shape`, `PadPiece`), so the gap to a trapezoid's slanted side, a chamfered corner or a custom pad's arm is within 2 um of kicad-cli's DRC. Pad Properties, on the board and in the footprint editor,
  takes KiCad's nine shape choices, a trapezoid's delta, a chamfer's ratio and corners and a custom pad's list of basic shapes (add, edit, delete: one undoable verb). Zone-fill parity (`crates/zone-filler/PARITY.md`, deleted 2026-10-13 with the filler; `git show 43f6c09:crates/zone-filler/PARITY.md`): `stonehenge`
  GND 11.33% to 0.15% of the area, `issue5093`'s `/right/GND` 776.56% to 1.49%; 198 of the 208 fills of the 17 QA boards are within 1% of kicad-cli's area and 189 within 0.5%.
- Done on `task-pad-details` (P25; `PARITY-pcb.md` sections 42 to 44): *Edit Pad as Graphic Shapes and Finish Pad Edit* in the Footprint Editor (`crates/ops/src/pad_edit.rs`: a custom pad is exploded into free graphics of its footprint and recombined, `Cmd::ExplodePad`, `Cmd::RecombinePad`, one undo step each);
  *padstacks* (`crates/model/src/padstack.rs`, `crates/kicad/src/padstack_io.rs`: a pad's shape on the front, inner and back copper, `front_inner_back` and `custom`, read and written as `pcb_io_kicad_sexpr` does, mirrored on the bottom as `PADSTACK::FlipLayers`, and read per layer by the DRC gates, the router, connectivity,
  the canvas and the 3D scene; Pad Properties has Padstack mode, Edit layer and Copper layers; kicad-cli plots each layer of KiCad's own `padstacks_complex` board as the model has it and our export as the original); *Remove Unused Pads* (`crates/ops/src/unused_pads.rs`, `eda_connectivity::flashed_layers`: `remove_unused_layers` for
  the pads and vias chosen, and the layers that keep their ring).
- Left: the inner copper of a padstack in the 3D scene (it draws the front and the back shapes), a custom stack's per-layer thermal and clearance settings (kept as tokens), a via's own padstack and the layer a zone is told to flash a pad or via on (`zone_layer_connections`), a custom pad's proxy spokes as zone-fill templates (the explode keeps them in the mode, not as graphics), `gr_rect` corner radius and arcs inside `gr_poly`, and the instant 3D scene (`eda_drc::pad_shape::pad_outline` and the state's `pads[].outline` are there for it).
- Measured: 40 new Rust tests (five of them slow-tier, with kicad-cli) and 25 new
  `node --test` cases; `web/studio/e2e/pad-shapes.check.js` runs 6 scenarios through `window.__eda` on a served board. P25: 72 new Rust tests (five slow-tier: kicad-cli's plot of every copper layer of `padstacks_complex`, of a bottom-side footprint and of a library footprint, the rings a pad keeps on a four-layer board, and the schematic parity of a swap), 22 `node --test` cases, `web/studio/e2e/pad-details.check.js` (10 scenarios); the re-export of the 12 QA boards is 12 of 12, as before.
- Port from: `pcbnew/pad.cpp`, `pcbnew/tools/pad_tool.cpp` (`EditPad`, `explodePad`), `pcbnew/padstack.cpp`, `pcbnew/dialogs/dialog_unused_pad_layers.cpp`, `pcbnew/tools/global_edit_tool.cpp` (`RemoveUnusedPads`), `pcbnew/pcb_io/kicad_sexpr`, `pcbnew/dialogs/dialog_pad_properties.cpp`, `pcbnew/zone_filler.cpp` (`addKnockout`, custom pad parts), `pcbnew/microwave/microwave_polygon.cpp`.

### 32. Design blocks
**Done 2026-10-20 on `task-design-blocks` (not merged); 22 of the 23 rows are wired and what is left is below; the 23rd, Apply Design Block Layout, is wired on `task-multichannel` (2026-10-21, package P24, not merged yet).** Package P18, size L. Hit: reuse of sub-circuits.
- What works: a design block is a `.kicad_blocks` library folder (a folder per block with its `.kicad_sch`, an optional `.kicad_pcb` and a `.json` of description, keywords and default fields) listed by `design-block-lib-table`, the third table beside the symbol and footprint ones (Manage
  Design Block Libraries; New Library). The Design Blocks pane of the schematic and of the board editor (View > Design Blocks) has the search box and the library tree (Recently Used, pinned libraries), kicad-cli's preview of the selected block and its details, and KiCad's four check boxes.
  Place Design Block (Shift+B, or a double click) is Import Sheet's tool with the block (contents or a sheet, grouped and linked to the block, renumbered) and, on the board, Append Board carried by the Move tool (grouped and linked, one undo step per copy, repeated copies until Escape);
  Place Linked and Save to Linked Design Block from a linked group; Save Current Sheet / Board and Save Selection as Design Block, Update from Sheet / Board / Selection, Properties and Delete, with KiCad's questions and refusals. A group keeps its block (`Group.design_block`,
  `(lib_id "Lib:Name")` in the `.kicad_sch` and `.kicad_pcb`) and Group Properties has the "Library link:" box. The studio writes only the library the user names.
- Done 2026-10-21 (`task-multichannel`, package P24; item 24's multichannel bullet, `PARITY-pcb.md` section 40): **Apply Design Block Layout** (`pcbnew.InteractiveDrawing.applyDesignBlockLayout`, its row is out of `ui-parity-missing.json`): a group linked to a block takes the block's board layout, the footprints matched by topology (`topo_match.cpp`), the block's tracks, vias, zones and other items copied on to the group's nets and into the group, in one undo step; a group with a locked footprint or one that does not match the block is refused in KiCad's words.
- Left: the footprint of the group that anchors the layout is the first pair in the block file's order, where KiCad's is the first of an unordered set (a pointer order, so there is nothing to copy); a placed sheet does not get the block's default fields as hidden fields, and a saved sheet's default fields start empty (the IR has no sheet user fields); a block name with a '.' is refused (KiCad lists a block by the part of its folder's name before the first dot, so it would be hidden from KiCad);
  KiCad's Append Board groups what it appends by default (`place_as_group`, named after the file) and the studio's still does not (the design block placement does); Import Sheet's chooser and the pane keep check boxes of their own (KiCad shares them); the pane's pinned libraries, recents and
  check boxes are the browser's (localStorage), not the project's or a settings file; the pane is a docked column, not a floating window; the preview is kicad-cli's drawing and not the canvas'; New Library writes the board's project table (KiCad's global table is read only here, and the template
  table the user's global one chains is not installed on this machine: the pane lists it as a note).
- Port from: `eeschema/tools/sch_design_block_control.cpp`, `pcbnew/tools/pcb_design_block_control.cpp`, `common/tool/design_block_control.cpp`, `common/design_block{,_info,_io,_library_adapter,_tree_model_adapter}.cpp`,
  `common/widgets/{design_block_pane,panel_design_block_chooser}.cpp`, `common/dialogs/{dialog_design_block_properties,panel_design_block_lib_table}.cpp`, `eeschema/sch_design_block_utils.cpp`, `pcbnew/pcb_design_block_utils.cpp`.

### 33. Browser-check coverage
**Standing.** Package P20. **2026-10-23 (`task-p20-browser-checks`): 42 checks, all run headless by `tools/run-all-checks.sh` (42 of 42 pass after the KiCad 10 items merge).**
- The runner builds once, makes each board a check needs once (the header line `// run-all-checks: board=<kind> [minutes=<m>] [ui=<ui>]` names it: `mcu30`,
  `mcu30-few`, `mcu30-placed`, `mcu30-routed`, `mcu30-zones`, `opamp-routed`, `kicad10-stitch`, `usb-pair`, `tune-board`, `multichannel`, `simulator`), serves a fresh copy with no undo history per
  check on ports 8810-8814 (`-j N` at once), runs `web/studio/e2e/run-check.mjs` (which now also runs the older one-expression checks and the ones whose `wait()` returns
  a summary) and prints one line per check; it exits non-zero on any failure. `tools/run-all-checks.sh zones sch-edit` runs some.
- New in P20: `board-edit.check.js` (select with the pointer, a window, move by drag, M, Escape, Move Exactly, R / Shift+R, F, Del, undo and redo chains, the Properties
  panel's Position and Orientation), `sch-edit.check.js` (the Symbol Chooser and a click, W, the junction of a T and J, L and the label dialog, Annotate on a selection,
  the ERC through kicad-cli on the symbol placed), `lib-edit.check.js` (New Footprint, the pad tool, Pad Properties, a pad dragged, turned, deleted, the line tool,
  Delete Footprint; New Symbol, the pin tool, Pin Properties, a rectangle, a pin dragged, turned, deleted, Delete Symbol), `board-setup-rules.check.js` (Physical Stackup,
  Solder Mask/Paste, Defaults, Dimensions, Constraints, Pre-defined Sizes, Teardrops, Custom Rules, Violation Severity) and `plot-exports.check.js` (Gerbers, drill,
  placement, STEP, VRML, IPC-D-356, IPC-2581, ODB++, the board BOM, the schematic's PDF and SVG). Zones (Fill All through kicad-cli) and the 3D viewer were covered
  already (`zones.check.js`, `viewer3d.check.js`).
- Found and fixed on the way (each with a unit test first): a footprint moved, turned or flipped by an item edit snapped to the placer's 0.1 mm grid (Move Exactly
  by 1.25 mm moved 1.3); a wire drawn to the middle of another made no junction (`finishSegments`); in the Footprint and Symbol Editors Properties (E), Rotate and
  Delete as actions did nothing or were not offered (only the canvas's keys worked); Pre-defined Sizes' Apply was two undo steps; Escape on a popup menu also closed
  the dialog under it (the DRC window's marker menu); a CSV placement file was named `.pos`; on a board with no stored schematic (every board `eda board new` makes)
  the DRC's "Test for parity between PCB and schematic" failed with "design has no placement section" (`load_with_schematic` replaced the design by the derived one).
- Fixed 2026-10-25 (the DS lane, `ds/handoff-1025`): the Symbol Editor's pin tool and New Symbol placed at once where KiCad asks first -- the pin tool's click now
  opens Pin Properties (`SYMBOL_EDITOR_PIN_TOOL::CreatePin`; Cancel places nothing, OK places it after `PlacePin`'s Synchronized Pins Mode question) and Ctrl+N opens
  a `DIALOG_LIB_NEW_SYMBOL`-shaped dialog (name validator, the default reference, units, body style, power, BOM/board, pin name offset and pin texts; one undo step).
- Fixed 2026-10-26 (the DS lane, `ds/handoff-1025`): Generate Placement Files now names its files as KiCad does (`PLACE_FILE_EXPORTER::DecorateFilename`):
  `<name>-all-pos.csv` for both sides in one file, `<name>-top-pos.csv` and `<name>-bottom-pos.csv` when the dialog's "Generate single file with both front and
  back positions" (checked by default, as `m_singleFile` is) is off -- two kicad-cli runs for the two sides -- and the ASCII names are `<name>-all.pos` etc.
  `eda fab pos --separate-sides` asks for the two files too (`kicad-engine::place_file_names`).
- Found, not fixed (listed for their packages): a symbol placed from the library under a lib_id the design already has keeps the design's definition (library_place.rs,
  by design), so `Device:R` placed from the chooser on mcu30 draws the built-in R, not KiCad's; the export routes list the files modified since a second before the run, so a
  second run within that second lists the first one's files too.
- Fixed since P20: the parity DRC of the mcu30 example, placed and routed, was 76 differences between the board and the schematic derived for it (30 footprints the
  symbols' empty Footprint fields did not name, 8 field mismatches, 38 net conflicts; items 2 and 26). Fix batch A (2026-10-25, `a8287704`) made it 0: kicad-cli's
  schematic-parity DRC finds none on mcu30, opamp_filter or ldo (`docs/HANDOFF.md`, "fix batch A").
- Coverage by package (the rows of the plan's table):

| package | checks |
|---|---|
| P1 footprint graphics | `fp-graphics` |
| P2 assignment, exchange, rename | `fp-assign` |
| P3 library tables | `library-tables`; the choosers: `sch-edit` (Symbol Chooser), `fp-models` (Footprint Chooser's 3D view) |
| P4 hierarchy | `sch-details` (sheet pins), `sch-details-2` (a screen shown twice, buses across sheets), `design-blocks` (place as a sheet), `import-sheet` (the chooser's checks and options, a sheet imported as one undo step, its symbols, labels and nested sheet pins), `hierarchy-navigator` (every sheet once, children in page order, every page shown and written, a screen shown twice twice, a click and the breadcrumb) |
| P5 router | `diff-pairs`, `length-tuning`, `router-leftovers` |
| P6 PCB editing details | `pcb-edit-details`, `board-edit` (select, move, rotate, flip, delete, undo) |
| P7 custom pads | `pad-shapes`, `pad-details` |
| P8 files in | `files-in`, `import-foreign` (P22) |
| P9 Board Setup, rule editor | `board-setup-pages`, `board-setup-rules`, `rule-editor`, `embedded-files` (P26) |
| P10 schematic details | `sch-details`, `sch-details-2`, `sch-groups`, `sch-edit` |
| P11 search, inspection | `search-inspect` |
| P12 board outline | none |
| P13 zones | `zones` |
| P14 3D models | `fp-models`, `viewer3d`, `embedded-files` |
| P15 review | `review`, `drc-review` |
| P16 item kinds | `pcb-items`, `tables` |
| P17 library editors | `lib-editors`, `lib-edit` |
| P18 design blocks | `design-blocks` |
| (P19, P21, P24) | `microwave`, `specctra`, `cli-exports`; `simulator`; `multichannel` |

- Coverage by PARITY section (a section not named has no check; "n/a" is a list of what is not wired):
  - `PARITY-pcb.md`: 1 view controls none; 2 `snap-grid`; 3, 4 `board-edit`, `pcb-edit-details`; 5 `pcb-edit-details`, `footprint-edit`; 6 `board-edit`, `footprint-edit`;
    7 context menus and hotkeys none on the board (the DRC window's, the tables' and the schematic's menus are checked); 7a `search-inspect`; 8 `zones`;
    9 `board-setup-pages`, `board-setup-rules`; 9a `drc-review`; 10 `footprint-edit`, `pcb-items`, `properties-panel`; 11 none; 11a `fp-models`, `library-tables` (a footprint placed);
    12, 13 `pcb-edit-details`; 14 `properties-panel`, `multichannel`; 15 `board-setup-rules` (the settings; Add All Teardrops none); 16 `pcb-edit-details`; 17 `footprint-edit`;
    18 `board-setup-rules` (the defaults), `properties-panel`; 19 Board Statistics, Swap Layers none; 20, 21 none; 22 `board-edit`, `pcb-items`, `properties-panel`;
    23 `properties-panel`, `board-edit`; 24 `appearance-panel`; 25 the outline none; 26 `footprint-edit`; 27 `pad-shapes`; 28 `fp-graphics`; 29 `files-in`; 30 `fp-assign`;
    31 `fp-models`; 32 `search-inspect`; 33 `review`; 34 `import-foreign`; 35 `router-leftovers`; 36 `design-blocks`; 37 the Rust re-export suite, not a browser check;
    38 `tables`; 39 `pcb-items`; 40 `multichannel`; 41 `microwave`; 42 Edit Pad as Graphic Shapes none; 43-45 `pad-details`; 46 `specctra`; 47 `embedded-files`;
    48 `rule-editor`; 49 `cli-exports`. Plot and fab outputs (`PARITY-boardctl.md` 5): `plot-exports`.
  - `PARITY-sch.md`: 0 n/a; 1 `sch-groups` (move), `sch-details`; 2 `sch-edit`; 3 `sch-edit`, `sch-details` (power symbols); 3a `sch-edit`; 4 `sch-edit`, `drc-review`, `review`;
    5 `sch-edit`; 6 cross-tab undo none; 7 `sch-details`, `lib-editors`; 8 Find on the sheet none; 9 the pin map none; 10 `plot-exports` (PDF, SVG; the netlist through `simulator`);
    11 none; 12 `sch-details`, `sch-groups`; 13 none; 14 `sch-details`, `sch-details-2`, `hierarchy-navigator`; 15 `sch-groups` (copy, paste, duplicate); 16 Rust tests; 17 the schematic's
    Properties panel none; 18 `snap-grid`; 19 `import-sheet`; 19b `board-setup-pages`; 20 `files-in`; 21 `sch-details`; 22 `sch-details`; 23 `sch-details`; 24 `sch-details`;
    25 `sch-details`; 26 `sch-details`; 27 `sch-details`; 28 `review`; 29 `sch-groups`; 30 `sch-groups`; 31 `sch-details`; 32 none; 33 `import-foreign`; 34 `design-blocks`;
    35 `tables`; 36 `pcb-items`; 37 `microwave`; 38 `simulator`; 39 `sch-details-2`.
  - `PARITY-fpedit.md`: 1-4 `lib-edit`; 5 `lib-editors`, `fp-models`; 6 `lib-editors` (the export routes); 7 none; 8 `lib-edit`, `library-tables`; 9 none; 10 `pad-shapes`
    (on the board); 11 `fp-models`; 12 `search-inspect`; 13-15 `lib-editors`.
  - `PARITY-symedit.md`: the frontend's tools `lib-edit`; derived symbols, snapping, fields on the canvas, alternate pins, the text box and Shape Properties `lib-editors`;
    Import Graphics `files-in`; grid overrides none.
  - `PARITY-3d.md`: 1, 2, 3, 5-8 `viewer3d` (and `fp-models`, `embedded-files` for the models); 4 lighting and background none.
  - `PARITY-boardctl.md`: 1 `appearance-panel`; 2 `zones`; 3 `search-inspect`; 4 the drill/place origin none; 5 `plot-exports`, `cli-exports`, `specctra`.
  - `PARITY-common.md`: 1, 2 none; 3 `board-edit` (selection; the delete tool none); 4 `pcb-edit-details`, `sch-groups`; 5 `drc-review`, `review`, `search-inspect`; 6 none;
    7 `lib-edit`, `library-tables`; 8 `pcb-items`; 9 Page Settings none; 10 `snap-grid`; 13 every check; 13a `files-in`; 14 `library-tables`.
- Left without a check: the board's view controls and context menu, the schematic's Find and Properties panel, the board
  outline (P12), Board Statistics and Swap Layers, Page Settings, the drill/place origin, Edit Pad as Graphic Shapes, the 3D viewer's lighting. The rule in force: a fix
  counts when the C++ function is named in the code, the logic has a `kicad-port/*.test.ts` (or a Rust test), one user action is one undo step, and a browser check ran.

### 34. The simulator
**Done 2026-10-23 on `task-simulator` (package P21).** PARITY-sch.md section 38 has the ports and the tests.
- Every row is wired: Inspect > Simulator, the 12 `eeschema.Simulation.*` (Probe and Tune on the sheet among them), the 9 `eeschema.Simulator.*` (the exports, the View toggles), Show OP Voltages and Currents, and the plot's zoom history and axes (6 `common.Control`).
- The architecture is the user's: kicad-cli's SPICE netlist (`kicad-cli sch export netlist --format spice`, and `--format kicadxml` for the fields and pins), ngspice as a separate program in batch mode (`ngspice -b`, `EDA_NGSPICE` or `PATH`), its raw files read back; the plot is a canvas painted as mathplot paints. The workbook (`.wbk`) is read and written byte for byte and kept in the design (`Cmd::SetSimulator`), the project's `schematic.ngspice` with it.
- Left, by that architecture: libngspice's background run and its live plot (a batch run shows its result when it is over; Stop kills ngspice and what it computed); a measurement or a Fourier analysis added after a run is a new run, not a command to the loaded simulator.
- Left: the user-defined signals' auto-completion, the probe's cursors and its net highlight, the SP analysis's S-parameter signals, the multi-run cursor's run labels. A net drawn in pieces gets one OP label per sheet. The project's OP overlay formats (`drawing.operating_point_overlay_*`) are not carried: the defaults are used. Tuning a potentiometer.
- Left, not ported: the Simulation Model dialog (`DIALOG_SIM_MODEL`, `eeschema/dialogs/dialog_sim_model.cpp`, Symbol Properties' "Simulation Model..."). KiCad's dialog picks the model source (a SPICE model from a file, `*.lib` / `*.sub` / `*.ibs`, with its model list, filter and IBIS pin / pin model / differential / waveform choices; or a built-in SPICE model by Device and Device type), shows the model's parameters in a property grid (Parameter, Value, Unit, Default, Type, with the instance / model / waveform categories), the model's Code page, "Save parameter in Value field", and the Pin Assignments grid (symbol pin to model pin or "Not Connected"); OK writes the `Sim.Device`, `Sim.Type`, `Sim.Library`, `Sim.Name`, `Sim.Pins`, `Sim.Params` and `Sim.Ibis.*` fields (`SIM_MODEL::WriteFields`). Here nothing writes those fields but Tune's Save (`Sim.Params`); a model is set by editing the fields by hand. A port would be one undoable verb on the symbol's fields, but it needs `SIM_MODEL`'s type registry, the ngspice parameter tables and the library readers (`sim_model*.cpp`, `sim_lib_mgr.cpp`, `spice_library_parser.cpp`, `ibis/*`, about 14,700 lines), none of which is ported (kicad-cli writes the netlist from the fields). PARITY-sch.md section 49.
- Round 6 (`task-r6-editors-exports`, S6-1..S6-6, PARITY-sch.md section 49): the include-path box is the project's setting, Plot Setup shows the tick range and reads SI values, User-defined Signals' Delete takes the cursor's row, an edited DC sweep keeps only a source its list holds, the signal filter takes a regular expression only as `^..$` or `/../`, the node choices start blank.
- Found on the way: the derived schematic now gives a placed symbol its library symbol's `Sim.*` fields (it had dropped them, so no `Simulation_SPICE` source simulated); two symbols of one library symbol placed at different angles still share one transform in the derived schematic (`write_regular_lib_symbol`), which can leave a pin unconnected for kicad-cli.

### 35. The long tail of the PCB tools
**New, open.** Hit: rarely each. Package P6, with P13 for zones. From the "not ported" rows of `PARITY-pcb.md`.
- Done 2026-10-10 (`task-pcb-edit-details`): Cleanup Tracks & Vias takes the net, net class, layer and "selected items only" filters and "Refill zones before and after cleanup", and leaves locked tracks and vias alone
  (section 12); Edit Track & Via Properties filters by net class, exact track width and via diameter and picks through / blind / buried vias, and its layer filter reaches vias by the top layer of their span; Edit Text &
  Graphics Properties reaches dimensions, lists every board layer and has "Set to layer (and dimension) default values" (section 13). Still open there: microvias and the annular-ring and IPC-4761 presets (a via has no
  padstack here); of text and graphics the footprint reference, value, other-field, text, graphic and dimension scopes, the two "By parent ..." filters, bold, italic, font, keep upright, visible and "center text on footprint"
  (a footprint's own text is not an item of the board until package P1). Rule areas: multi-layer rule areas, `(disallow ...)` rules on non-keepout items, `DRCE_TEXT_ON_EDGECUTS` (section 14). Teardrops: curved edges,
  rectangular and non-circular pad anchors, track-to-track, two-segment borrowing, `m_TdOnPadsInZones`, incremental updates (section 15; arc tracks done in round 5, section 60). Dimensions: a leader's text border, manual text position
  (section 18; Rotate/Flip of a dimension is done). The Draw Zone Fill Fracture Borders / Triangulation debug modes draw strokes only (the triangulation is an earcut port without KiCad's Z-order hash); Flip Board View mirrors text on
  non-side-specific layers with the board where KiCad keeps it readable; Automatically select track width does not turn off when a width is picked in the toolbar and the first width key while routing
  does not set `m_TempOverrideTrackWidth` (`PARITY-boardctl.md`).

### 36. Connectivity against kicad-cli
**Measured, mostly closed.** `tools/parity_report.py` section 1 (`crates/connectivity/tests/parity_connectivity.rs`), `PARITY-pcb.md` section 51.
- Done 2026-10-23 (`task-connectivity-parity`): the measured set is fixed (the examples and every `qa/data/pcbnew` board, never `work/`), kicad-cli runs on a copy with the ignored checks turned back on, and our counts are capped
  at its error limits. Fixed in `crates/connectivity`: an arc is one item, a pad turned off a multiple of 90 degrees is measured by its copper, and a pad or via that removes its unused copper is its hole on the layers it is
  conditionally flashed on.
- Done 2026-10-28 (`task-r5-rules-zones`, R5-Z1): two same-net fills join only by a vertex of one inside the other (`CN_VISITOR::checkZoneZoneConnection`), not where their edges cross; proof on
  kicad-cli's DRC of a "+" of two GND bars (`kicad_cli_ratsnest.rs`). Cleanup's merge (R5-Z4, R5-Z6): no merge from a segment with a track of another width attached, and the joint's pads, vias and zones
  asked as `testMergeCollinearSegments` asks them.
- Left (each with its boards in section 51): coordinates in micrometres (sub-micrometre segments of tuned tracks), pads that share a number but not a net, copper graphics with a net, the teardrops kicad-cli's Fill All
  rebuilds, and a `.kicad_dru` rule that ignores `via_dangling`.

### 37. The fab output dialogs (Plot, drill, placement, BOM)
**Partial.** `PARITY-boardctl.md` section 5; round 2's r2-editors-exports X1-X11 (fixed 2026-10-27 on `task-r2-editors-exports`: the board's plot settings kept and plotted with, the drill and placement dialogs' defaults and controls, the 3D export's outline check, the VRML origin, the board BOM on the board).
- Left: the PCB Plot dialog offers Gerber only, with the layer checklist, "Check zone fills before plotting" and "Use drill/place file origin"; KiCad's `DIALOG_PLOT` also has PostScript, SVG, DXF, HPGL and PDF, "Plot drawing sheet", "Plot footprint values" and "Plot reference designators", "Sketch pads on fabrication layers", the DNP options, the drill marks, "Use Protel filename extensions", "Generate Gerber job file", "Coordinate format", "Use extended X2 format", "Include netlist attributes", "Disable aperture macros", "Subtract soldermask from silkscreen", the output directory and Run DRC (kicad-cli has a flag for each; the board's own settings already reach the plot). The checklist and the origin are stored back in the board (2026-10-27, r3 X3-3, `set_plot_settings`); the other controls, once ported, would go through the same verb. A kicad-cli limit (r3 X3-9): "Sketch pads on fabrication layers" draws with 0.1 mm (`PCB_PLOT_PARAMS`' `m_sketchPadLineWidth`), where KiCad's dialog sets the board's F.Fab line width (`DIALOG_PLOT::Plot`: `SetSketchPadLineWidth( GetLineThickness( F_Fab ) )`); kicad-cli has no flag for it, so a board whose fab line width is not 0.1 mm plots its sketched pads thinner or thicker than KiCad's dialog does. Round 3 (r3-editors-exports X3-1..X3-12, fixed 2026-10-27 on `task-r3-editors-exports`): legacy plot tokens read and dropped as KiCad does (`viasonmask` no longer undoes the vias' tenting), the DNP flags and precision as the dialog reads them, the drill and Gerber X3 reports, the STEP Model Scale Warning, the `.cmp` path. The drill dialog lacks Gerber X2 drill files, map files, units, zeros, mirror and minimal header; the placement dialog lacks "Exclude from BOM" (no kicad-cli flag) and the design variant. The board BOM is kicad-cli's writer, not `GenBOMFileFromBoard`'s (quoting, the library prefix, no `;;;`). Export Symbol as SVG plots with kicad-cli's theme and page, not the editor's theme with a 10 % margin.

### 38. Pictures of the design by kicad-cli (the port-vs-reuse audit's row 2)
**Done 2026-10-28 on `task-pictures-kicad-cli`.** `eda schematic`'s `schematic.svg`, `GET /api/schematic.svg` and the judge's pictures are
`kicad-cli sch|pcb export svg|png` (`docs/ARCHITECTURE.md`, "Engines"; `crates/kicad-engine/src/pictures.rs`; proofs in `crates/cli/tests/pictures.rs`).
- Kept, inactive (the user's call of 2026-10-28: no code deleted): `crates/render` and `eda_judge::render_board_svg`, behind `EDA_RUST_PICTURES=1`.
- Left: the judge's rubric is `judge-rubric-2` and has not been re-baselined on the bench boards' verdicts (the audit's golden check: no
  `ANTHROPIC_API_KEY` on the box); a schematic picture is the whole page KiCad plots, so a small sheet is small in it (the old picture was
  cropped to the content); lint's `schematic_content_in_bounds` now measures against the paper (the page kicad-cli plots), not a cropped viewBox.
- Round 4 (`task-r4-editors-exports`, X4-1): the board PNG is painted back to front. kicad-cli plots the `--layers` entry first (`PCB_PLOTTER::getPlotSequence`), so the base is Edge.Cuts and F.Cu comes after
  B.Cu in `--common-layers` (it was F.Cu, painted under the back copper, the back silkscreen and the courtyards); proved by `pictures.rs::the_board_png_has_the_front_copper_on_top`.

## Appendix A. The original 30, one verdict each

13 closed, 9 partial, 0 open, 8 out of scope (2026-10-10). Changed since 2026-10-07: #11 and #25 closed, #14 closed on `task-pcb-edit-details` (not merged); #6 and #26 re-worded.

| # | Gap | Verdict | Evidence, or where it went |
|---|---|---|---|
| 1 | Schematic editing | **Closed** | 168 eeschema handlers (`actions/useActionRunner.ts`, `schEditActions.ts`), `components/SchematicView.tsx`, verbs in `crates/ops/src/{lib.rs,sch_edit.rs,sch_move.rs}`, one-netlist rule in `crates/cli/src/board.rs::reconcile_schematic` (test `schematic_wire_connects_and_disconnects_pins_on_one_netlist`). Behaviour: items 1, 6, 12 are closed. |
| 2 | DRC engine hangs | Out of scope | kicad-cli is the only engine (`docs/ARCHITECTURE.md`); the Rust copy is deleted. |
| 3 | DRC precision | Out of scope | same |
| 4 | ERC has no UI | **Closed** | `components/ErcDialog.tsx`, `GET /api/erc` to `crates/kicad-engine/src/lib.rs::erc`, exclusions `Cmd::AddErcExclusion`, pin map `SchematicSetupDialog.tsx`. Review workflow: item 11. |
| 5 | Zone fill | KiCad's own | `eda_kicad::zone_fills`, `eda_kicad_engine::refill_zones`, `crates/cli/src/fill_api.rs`, `ZoneDialog.tsx`, `ZoneManagerDialog.tsx`; the fill is kicad-cli's (no filler here); items 13 and 31. |
| 6 | Hierarchical sheets | Partial | IR, import, export, panel, navigation, editing on every sheet, nets across sheets, Find, Annotate, Import Sheet, sheet copy and paste, lint and render over the hierarchy (`ir.rs`, `sch_import.rs`, `HierarchyPanel.tsx`, `sch_annotate.rs`, `sch_sheets.rs`); item 4 lists what is left. |
| 7 | Routing | Partial | `crates/pns`; item 7 (P5a to P5d done 2026-10-10 to 10-20; springback in all three placers, rounded corners and virtual vias 2026-10-23 and 10-25: the tuner's gaps are what is left). |
| 8 | Footprint editor and pad tool | Partial | editor, pad tools, dialogs, import and export (`components/footprint/`, `crates/ops/src/library_editors.rs`); items 19, 30, 31. |
| 9 | Board Setup | Partial | `BoardSetupDialog.tsx`: all 10 pages edit (rules overlay and 7 verbs, item 3); 8 of KiCad's pages have no model yet. |
| 10 | Net classes and rules | Partial | importer (`crates/kicad/src/import.rs::merge_project_net_classes`, `custom_rules.rs`) and editing (Net Classes, Custom Rules, Assign Netclass) done, item 3; open: regex patterns, the rule-tree designer. |
| 11 | Property dialogs | **Closed** | via, shape, zone, text, dimension and track width edit (`Cmd::EditVia`, `EditShape`, `EditZone`, `EditText`); the footprint (fields, attributes) and pad dialogs and the Properties panel edit every kind (`bd712d3`, `afcb91b`; items 9, 10). |
| 12 | Move excludes tracks and zones | **Closed** | `Cmd::MoveItems`, `RotateItems`, `FlipItems` (`crates/ops/src/pcb_transform.rs`), `kicad-port/pcbTransform.ts`, `pcbEditActions.ts::movableItem` for every kind; item 8. |
| 13 | Selection modifiers and box select | **Closed** | `kicad-port/selection.ts`, `components/canvas/selectionCandidates.ts::collectBoxSelection`, `Canvas.tsx`; `PARITY-pcb.md` section 3. |
| 14 | Clipboard | **Closed** (on `task-pcb-edit-details`, not merged) | PCB: every item kind, footprints included, in KiCad's clipboard format (`crates/kicad/src/clipboard.rs`, `Cmd::PasteClipboard`, `Cmd::PasteSpecial`, `Cmd::Duplicate`, `components/canvas/clipboard.ts`, `components/PcbPasteSpecialDialog.tsx`), Paste Special with KiCad's annotation modes and Clear Nets (2026-10-10, item 8); schematic: done in KiCad's format (`Cmd::PasteSch`, item 6). |
| 15 | Cross-tab undo | **Closed** | `crates/ops` `Domain`, `crates/cli/src/board.rs::restore_domain` (test `undo_redo_are_scoped_to_the_tab_that_asked`); the Footprint and Symbol tabs undo in their own scopes. |
| 16 | Hotkey extraction | **Closed** | `web/studio/tools/lib/actionsParser.js::extractPlatformRaw` (with test), `src/kicad/actions.json` (`common.Interactive.redo` is Ctrl+Y), `actions/hotkeys.ts::effectiveHotkey`. |
| 17 | Click-versus-drag threshold | **Closed** | `kicad-port/dragThreshold.ts` in all four editors (2026-10-09); item 14. |
| 18 | Snapping | **Closed** (board and schematic) | `kicad-port/{pcbGridHelper,schGridHelper,constructionManager}.ts`, `components/canvas/pcbSnap.ts`, `components/schematic/schSnap.ts` (2026-10-09); the library editors snap to the grid only; item 14. |
| 19 | DRC schematic parity | Out of scope | kicad-cli has `--schematic-parity`; the dialog passes it (2026-10-08, item 11): the Schematic Parity page lists what it reports. |
| 20-24 | ERC bus and hierarchy, multi-unit, SI, library-sync, DFM checks | Out of scope | kicad-cli runs these. |
| 25 | Align and distribute | **Closed** | PCB: every item kind, locks respected (`state/store.tsx`, `kicad-port/alignDistribute.ts`, `88c8c0a`); schematic Align and Align to Grid move each item's wires with it (`kicad-port/schAlign.ts`, `2a8bf8f`). |
| 26 | Array tool | Partial | `Cmd::CreateArray` (`crates/ops/src/array.rs`), `CreateArrayDialog.tsx`: footprints and every other kind, since 2026-10-09 (`afcb91b`); the footprint editor's pad numbering is left (item 22). |
| 27 | Grouping | Partial | board: done, nested (`Cmd::Group` family in `crates/ops/src/pcb_groups.rs`, `kicad-port/groupTree.ts`, `state/store.tsx::withGroupSubstitution`, the entered-group overlay in `painter.ts`, the `.kicad_pcb` file); the schematic sheet's groups are done on `task-sch-groups` (`SchCmd::Group` family in `crates/ops/src/sch_groups.rs`, `kicad-port/schGroups.ts`, `PARITY-sch.md` section 29); the footprint editor has no groups; item 17. |
| 28 | Dimensions and measure | **Closed** | `crates/connectivity/src/dimension.rs`, `Cmd::AddDimension` family, `components/DimensionPropertiesDialog.tsx`, the measure tool. Left: the interactive height click, text border, manual text position, export (item 2). |
| 29 | Pan | Partial | `kicad-port/viewControls.ts`, `Canvas.tsx` (PCB); item 20. |
| 30 | Context menu | **Closed** | `kicad-port/pcbContextMenu.ts` (KiCad's `CONDITIONAL_MENU` and the entries of every tool's `Init()`), `pcbSelectionSummary.ts`, `components/canvas/pcbMenuBuilder.ts`, `Canvas.tsx`. Left: table cells, gate swap, generators, most of the router's via and posture entries (item 16, item 7). |

## Appendix B. Out of scope and deferred

- **kicad-cli's job** (`docs/ARCHITECTURE.md`): DRC, ERC, plots, Gerbers, drill and position files, netlist, BOM, statistics, STEP, GLB, VRML,
  IPC-2581, ODB++, D356, GenCAD and IDF (IDF since 2026-10-23, `PARITY-pcb.md` section 49: `kicad-cli pcb export idf`, offered where the kicad-cli has it). Old #2, #3 and #19-#24. The UI around them is in scope (item 11). Hyperlynx and the footprint report stay unwired: kicad-cli has none, not even the nightly (2 rows).
- **Python scripting and plugins**: `common.API.pluginsReload`, `pcbnew.ScriptingTool.*`, the footprint wizards (`pcbnew.FpWizard.*`, `ModuleEditor.createFootprint`) (6 rows).
- **Project management and the application shell**: New, Open, Revert, Quit, the project manager, Rescue, Open Directory, Open with Text Editor, the calculators (9 rows). A web page has none of these. (Open Non-KiCad Board was one: it replaces the board that is open and needs no project of its own, so it is wired, with Import Non-KiCad Schematic File, since 2026-10-15: item 28.)
- **Simulator**: 29 rows, item 34, package P21: done 2026-10-23 on `task-simulator`.
- **Deferred until the IR has the item** (reasons in `ui-parity-missing.json`, re-checked in `STALE-REASONS.md`): design blocks (none left: item 32), generators (9), tables (18), PCB text boxes, barcodes, points,
  reference images and hatch fills, schematic images, the symbol text box (item 24), derived symbols, alternate pin functions, design variants (3), padstacks (Remove Unused Pads),
  embedded files (4: done 2026-10-23, P26, with `zstd`), custom pads (item 31: Explode, Recombine and the microwave shapes are left). The autoplacer (2) is declined: placement runs through `eda board` behind the placement gates. The Footprint and Symbol
  Viewer windows (5) are not planned; the editors' library trees browse.
- **Internal events** (13 rows) have nothing to wire.

## Appendix C. Zone filler: not ported, KiCad's is used

From 2026-10-02 to 2026-10-13 this workspace held a port of `pcbnew/zone_filler.cpp` (`crates/zone-filler`, measured against kicad-cli in its `PARITY.md`). It is deleted: the zone fill is KiCad's, made by
`kicad-cli pcb drc --refill-zones --save-board` on a copy of the derived board and kept beside `design.json` (item 13). Every function below is therefore KiCad's own run, and none is ours to keep in step; the
table says what each did, and what the port had not reached when it went (all of it superseded).

| KiCad function | where it is now |
|---|---|
| `Fill`, `fillSingleZone`, `FillIsolatedIslandsMap`, `refillZoneFromCache` | kicad-cli (`JobExportDrc` with `--refill-zones`); the port had them |
| `addKnockout`, `addHoleKnockout`, `knockoutThermalReliefs`, `buildThermalSpokes`, `buildCopperItemClearances` | kicad-cli; the port lacked courtyard knockouts, net-tie exemptions and mask-only NPTH knockouts |
| `ZONE::BuildSmoothedPoly`, `postKnockoutMinWidthPrune`, `subtractHigherPriorityZones`, `buildDifferentNetZoneClearances` | kicad-cli |
| `connect_nearby_polys` | kicad-cli (the port lacked it) |
| `fillNonCopperZone` | kicad-cli (non-copper zones are written to the derived board and read back like the rest) |
| `buildHatchZoneThermalRings`, `addHatchFillTypeOnZone` | kicad-cli (the port was 0.6 to 0.9% XOR off on the rings) |
| `ZONE::HigherPriority` (priority, then the uuid) | kicad-cli, on the zone's uuid: kept from an imported file (`Zone::uuid`) |
| `AutoAssignZonePriorities` (`zone_utils.cpp`) | ported (`eda_ops::zone_priority`): it is not a fill, it works out priorities from the overlaps |

## Phase one: the six packages (history)

Kept as history. WP1 schematic editing fidelity: done (`2a8bf8f`, `ee20bab`, `2d92851`, `358606b`); what is left is P10.
WP2 hierarchy and connectivity: module sheets (`08c70c8`), the rest is P4. WP3 PCB editing, selection and display: done (`88c8c0a`, `bd712d3`, `4c041a6`, `6611ade`, `358606b`); the rest is P6. WP4 router:
shove and follow-ups done (`1e4d7ea`, `03b46de`); the rest is P5. WP5 rules, zones and file fidelity: the writer and Board Setup (`b145f0e`), the review workflow (`02f383c`), zone fill (`eabaf91`), the outline
(`a46fd29`); the rest is P1, P9, P13 and P15. WP6 libraries, parts and footprints on the board: the choosers (`80e972d`), footprints as objects and arrays (`afcb91b`); library tables (`task-lib-tables`, P3) are mostly done, Change and Update
Footprint are P2.

## Next phase: work packages

21 packages in the order of the ranked list above (user impact first). Each says why, the files it touches, the KiCad files to port from, its size and the packages it collides with. Sizes: **S** is part of
one agent's sitting, **M** one sitting, **L** two or three, **XL** is split into parts that are each L or less. The three packages in flight are marked; read their branches before touching their files.
The "SR" names are the packages of `STALE-REASONS.md` (the stale reasons), which are folded in here: SR1 into P8 and P4, SR2 into P11, SR3 into P14 and P19, SR4 into P2, SR5 into P18, SR6 into P19.

### How to run them in parallel

Hub files are the ones many packages edit. A collision in a hub is a merge, not a design problem, when each package keeps its hunk small:

- `ir` = `crates/model/src/{ir,footprint,fp_edit,symbol}.rs`; `ops` = `crates/ops/src/lib.rs` (the `Cmd` enum, `domain`, `subjects`, `edits_connectivity`, the dispatch); `board` = `crates/cli/src/board.rs` (`load`,
  the `fold_*` functions, `reconcile_schematic`, `step`); `studio` = `crates/cli/src/studio.rs` (routes, `/api/state`); `kio` = `crates/kicad/src/{pcb,import,footprint_lib,lib}.rs` (writer, importer);
  `runner` = `web/studio/src/actions/useActionRunner.ts`; `store` = `state/store.tsx` and `api/{types,client}.ts`; `canvas` = `components/canvas/{Canvas.tsx,painter.ts}`; `schview` = `components/SchematicView.tsx`;
  `fpui` = `components/{FootprintPropertiesDialog,BoardPadPropertiesDialog}.tsx` and `components/footprint/`; `pns` = `crates/pns/` and `components/canvas/{routing,dragging,diffPairRouting}.ts`;
  `zone` = `crates/kicad/src/zone_fills.rs`, `crates/kicad-engine/` and `crates/drc/` (`crates/zone-filler/` was deleted 2026-10-13); `libs` = `crates/cli/src/library_*.rs`, `body_api.rs` and the choosers; `docs` = `ui-parity-missing.json`, `UI-ACTIONS.md`, `GAPS.md`, `PARITY-*.md`.

Rules that keep them apart (each is how an earlier package did it):

1. **A verb's logic is a file of its own** (`pcb_props.rs`, `pcb_paste.rs`, `fp_edit.rs`, `sch_clipboard.rs`); `ops/lib.rs` gets the variant and three match arms. Both sides of a conflict there are kept.
2. **IR changes are additive overlays** in `DrawingsSection` next to `rules`, `footprint_edits`, `zone_overrides`, `board_parts` (`#[serde(default, skip_serializing_if ..)]`); never a required field of `Design`, which is
   built in some fifty places. New data lives in a new file where it can (`fp_art.rs`), with one hook in `ir.rs`.
3. **Actions register from a sweep file** (`actions/<area>Actions.ts`, one line in `useActionRunner.ts`; `registerPcbGlobalEditSweep` is the pattern); dialogs are new files; a dialog's open flag goes in the
   `state.pcbx` patch slot before a new reducer case.
4. **`ui-parity-missing.json` and `UI-ACTIONS.md`**: a package deletes its rows from the first (the audit warns about a reason for a wired action) and never touches the second; whoever merges runs
   `node web/studio/tools/ui-parity-audit.mjs` once.
5. **Docs**: a package edits its own item here and its own `PARITY` section, nothing else; `STALE-REASONS.md` loses the rows it wires.
6. **Proof**: each ends with `web/studio/e2e/<name>.check.js` in the pattern of the six that exist, the kicad-cli proof where kicad-cli judges the file (a slow test behind `EDA_SLOW_TESTS=1`), and
   `tools/check.sh fast` on the crates it touched.
7. **Budget**: three or four agents at a time, never more (the standing note on the token budget).

Waves:

- **Running now** (2026-10-12): P9a (`task-board-setup`), P10 (`task-sch-details`). P1, P2, P3, P4, P5a, P5b, P6, P7 and P8 are merged; their sections list what is left.
- **Next free slot**: P5c (P5a and P5b are merged), which shares no hub with anything; P16 and P17, which stand on a footprint that carries its graphics (P1, merged 2026-10-11).
- **P14** merged 2026-10-13 (the Extruded 3D Body and the load-time migration are left: item 30; the embedded files are done, P26, 2026-10-23). **P11** merged 2026-10-13 (Find by Properties, Clearance and Constraints Resolution, the Footprint Checker; what is left: items 21 and 29).
- **P2** merged 2026-10-12 (Compare Footprint with Library included); Swap Pad Nets and Swap Gate Nets are left (item 26).
- **Any time a slot is free**: P12, P15, P6, P9a, P20 (test files only). **P11** is done on `task-search-inspect` (2026-10-13, not merged).
- **P13** is done on `task-zones` (2026-10-13, not merged): the zone fill is KiCad's through kicad-cli and `crates/zone-filler` is deleted (item 13). **After P7**: P17, and P19's arc stub and function shape.
- **After P3 and P4**: P18. **P8's PCB half** is independent; its sheet half is P4's.
- **Last**: P21.

### Conflict matrix

`x` the package edits the hub (more than a hunk); `(x)` a small hunk. A blank cell is no overlap.

| package | ir | ops | board | studio | kio | runner | store | canvas | schview | fpui | pns | zone | libs |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| P1 footprint graphics | x |  |  | x | x |  | (x) | x |  |  |  | (x) |  |
| P2 assignment, exchange, rename | x | x | x | (x) |  | (x) | (x) |  | (x) | x |  |  | (x) |
| P3 library tables (done) |  |  | (x) | x | (x) | (x) | (x) |  |  |  |  |  | x |
| P4 hierarchy (in flight) |  | x | x | x | (x) | x | x |  | x |  |  |  |  |
| P5 router |  |  |  |  |  | (x) | (x) | (x) |  |  | x |  |  |
| P6 PCB editing details |  | x |  |  | (x) | x | x | x |  |  |  |  |  |
| P7 custom pads (merged) | x |  |  | (x) | x |  |  | x |  | x | (x) | x |  |
| P8 files in |  | x |  | x | x | x | (x) |  |  |  |  |  | (x) |
| P9 Board Setup, rule editor | x | x |  |  | x | (x) | (x) |  |  |  |  |  |  |
| P10 schematic details | (x) | x |  |  |  | x | x |  | x |  |  |  |  |
| P11 search, inspection |  |  |  | (x) |  | (x) | (x) |  |  |  |  | (x) |  |
| P12 board outline | (x) | x |  | (x) |  |  |  |  |  |  |  |  |  |
| P13 zone leftovers | x |  |  |  | x |  |  |  |  |  |  | x |  |
| P14 3D models of footprints | x | x |  |  | x |  |  |  |  | x |  |  | (x) |
| P15 review workflow |  | (x) |  |  | (x) | (x) |  |  |  |  |  |  |  |
| P16 PCB item kinds | x | x |  |  | x | x | x | x |  |  |  |  |  |
| P17 library editors | x | x |  |  | (x) |  |  |  |  | x |  |  | x |
| P18 design blocks |  | x |  | x |  | x |  |  | (x) |  |  |  | x |
| P19 microwave, Specctra |  | (x) |  |  | (x) | (x) |  | (x) |  |  |  |  |  |
| P20 browser checks |  |  |  |  |  |  | (x) |  |  |  |  |  |  |
| P21 simulator |  |  |  | (x) |  | (x) |  |  | (x) |  |  |  |  |

Pairs that must be sequenced, not just rebased (the rest conflict in a hunk):

- **P1 with P7**: both change the footprint's data, `write_footprint` and the footprint painter. P1 keeps its data in new files and hooks each hub once; whoever merges second rebases onto the other's IR.
- **P1 before P2, P14, P16, P17**: P2's stored definition, P14's model list on `FootprintEdit`, P16's painter hooks and P17's editor fields all want the footprint to have graphics first.
- **P2 with P4**: `rename_symbol` and `annotate` in `ops/lib.rs` and `reconcile_schematic` in `board.rs` are both packages' ground. P2's rename step starts after P4's Annotate has merged.
- **P2 with P14 and P7**: `crates/model/src/fp_edit.rs` and `crates/ops/src/fp_edit.rs` (P2 adds the assignment, P14 `models3d`, P7 pad shapes in the pad edit).
- **P13 after P7**: both were `crates/zone-filler` (custom-pad knockouts and spokes, then the rest); P13 deleted it (the zone fill is KiCad's).
- **P8 and P18 after P4** for Import Sheet: `crates/ops/src/sheets.rs`, `board.rs`.
- **P5 with P6** only in `Canvas.tsx` handlers and `routing.ts`' entry points.

### P1. Footprint graphics on the board (item 25). Size L. Rank 1. **Done 2026-10-10 on `task-fp-graphics`, waiting for its merge; item 25 lists what is left.**
- Status (2026-10-10): the five steps below, each its own commit (`6251ee0` IR, reader and writer; `2d76355` canvas; `75228c8` 3D; `85ae2c0` the in-house checks; `8e026f0` `footprint_extras` retired), then a pad's silkscreen layer
  (`e1f2e05`, which the `complex_hierarchy` measure needed: 185 of its pads are on `F.SilkS`) and a `#2` definition written under its library (`4cecb61`). kicad-cli: the silkscreen and fab plots of six QA boards, a synthetic
  board and an example board are equal after the round trip (slow tier); `complex_hierarchy` `silk_over_copper`, `silk_overlap`, `silk_edge_clearance` equal the original's. Browser: `e2e/fp-graphics.check.js`, 6 scenarios.
  Hubs touched, each by a hunk: `ir.rs` (`PadMaskInfo::art_layers`, docs), `footprint.rs` (`Footprint::art`), `model/lib.rs` (`installed_art`), `studio.rs`, `import.rs`, `pcb.rs`, `painter.ts`, `types.ts`. P2, P14, P16 and P17 can start from the
  merge: `FpArt`, `FpShape::placed` and `FpText::placed` are what they reuse.
- Why: a board made here has no part outlines, pin-1 marks or fab bodies on screen, in the `.kicad_pcb`, in the Gerber silkscreen kicad-cli plots from it or in the 3D silk; an imported board loses its footprint-local
  graphics on export (`REPORT.md`: `complex_hierarchy`, 512 `fp_line`s).
- Files: `crates/model/src/footprint.rs` and a new `fp_art.rs` (graphics and texts in the footprint's own frame; one hook in `Footprint`), `crates/model/src/ir.rs` (`LibraryFootprint::to_engine_footprint` stops dropping
  them; `FootprintExtra.graphics` and `texts` become the imported footprint's definition), `crates/kicad/src/{footprint_lib,footprint_scan,import,pcb}.rs` (read `fp_line`, `fp_arc`, `fp_circle`, `fp_rect`, `fp_poly`,
  `fp_curve`, `fp_text` from `.kicad_mod` and boards; write them in `write_footprint`, a bottom-side footprint in KiCad's mirrored frame), `crates/cli/src/studio.rs` (`parts[].graphics`),
  `web/studio/src/components/canvas/painter.ts` (`drawFootprint`: by layer under the Appearance rows) and `kicad-port/pcbItems.ts` (hit test), `components/viewer3d/scene.ts` (silk, fab body), `crates/drc/src/board.rs` and
  `crates/lint` (the in-house silk and mask checks read the same data), the 3D box stand-in.
- KiCad: `pcbnew/footprint.cpp`, `pcb_shape.cpp`, `pcb_text.cpp`, `pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` (`format( FOOTPRINT )`), `pcb_io_kicad_sexpr_parser.cpp` (`parseFOOTPRINT_unchecked`), `pcb_painter.cpp` (`draw( FOOTPRINT )`).
- Order: IR, reader and writer with a round-trip test whose proof is kicad-cli's silk plot equal before and after (the pattern of `a46fd29`); then the canvas; then 3D; then the checks; then retire `footprint_extras`;
  re-run `tools/parity_report.py` (the `complex_hierarchy` row is the measure).
- Conflicts: P7 (footprint IR, `write_footprint`, the footprint painter: sequence, see above); P2, P14, P16, P17 start after P1's IR lands; P12 none.

### P2. Footprint assignment, exchange and reference rename (item 26; SR4). Size L. Rank 2. **Done 2026-10-12 on `task-fp-assign`, waiting for its merge; item 26 lists what is left.**
- Status (2026-10-12): the seven steps below, each its own commit (`7af8ee7` the rename, `a81dfd2` the assignment and the Footprint field, `4d01fe8` Change Footprint(s), `9de882f` Update from Library, `a8f4b22` Compare Footprint with Library,
  `65f2fa1` Geographical Reannotate, `4a3f088` Insert footprint into PCB). kicad-cli: the parity test after a rename, a Change Footprint and a reannotation (slow tier). Browser: `e2e/fp-assign.check.js`, 8 scenarios. Swap Pad Nets and Swap Gate Nets are not done (item 26 says why).
  Hubs touched, each by a hunk: `ir.rs` (`renamed_parts`, `footprint_assignments`), `model/lib.rs`, `ops/lib.rs` (`Cmd::ExchangeFootprints`, `Cmd::ReannotateBoard`, `rename_symbol`, `spans_both_editors`), `sch_annotate.rs`, `sch_control.rs`, `board.rs` (`build_model`, the history tag, `carry.rs`), `studio.rs` (three routes), `types.ts`, `useActionRunner.ts` (F8), `commonSuiteActions.ts`; `store.tsx`, `Canvas.tsx`, `painter.ts` and `SchematicView.tsx` untouched.
- Why: the Footprint field of a symbol does nothing on the board, Change Footprint does not exist, Update from Library never reads a library, and a reference rename strands the footprint.
- Files: `crates/model/src/ir.rs` (an assignment overlay in `DrawingsSection`, beside `footprint_edits`, applied by `board::load`), `crates/cli/src/board.rs` (`load`, `fold_unknown_symbols`, `reconcile_schematic`: the symbol's Footprint
  field is followed, a rename carries the part), `crates/ops/src/` new `fp_exchange.rs` plus the `Cmd` hunks in `lib.rs`, `rename_symbol` and `annotate` calling a `rename_part` that moves the pose, `footprint_edits`,
  `zone_overrides`, `locked_ids`, groups and the routing's pads; `fp_edit.rs`, `library_place.rs` (`Cmd::PlaceFootprint` for Insert footprint into PCB); studio `actions/pcbGlobalEditSweep.ts`, `components/PcbGlobalEditDialogs.tsx`
  (`ExchangeFootprintsDialog` exists for Update), `kicad-port/pcbGlobalEdit.ts`, `components/FootprintAssociationsDialog.tsx`, a new `ReannotateDialog.tsx`, `components/SymbolPropertiesDialog.tsx` and `actions/schSymbolActions.ts`.
- KiCad: `pcbnew/dialogs/dialog_exchange_footprints.cpp`, `pcb_edit_frame.cpp` (`ExchangeFootprint`), `netlist_reader/board_netlist_updater.cpp` (`replaceFootprint`, `updateFootprintParameters`, `updateComponentPadConnections`),
  `tools/global_edit_tool.cpp` (`ExchangeFootprints`), `tools/board_reannotate_tool.cpp`, `dialogs/dialog_board_reannotate.cpp`, `footprint.cpp` (`FootprintNeedsUpdate`), `tools/board_inspection_tool.cpp` (`DiffFootprint`),
  `eeschema/tools/assign_footprints.cpp`, `backannotate.cpp`, `tools/edit_tool_move_fct.cpp` (`SwapPadNets`).
- Order: the seven steps of item 26; Swap Pad Nets last and only if the board may disagree with the schematic (it was not reached).
- Conflicts: P4 (`rename_symbol` and `annotate` in `ops/lib.rs`, `reconcile_schematic`); P1 (the definition carries graphics); P14, P7 (`fp_edit.rs`); P3 (merged: `library_place.rs` reads the libraries through a `LibSource`).

### P3. Library tables and paths (item 5). Size S for what is left. Rank 3. **Mostly done: `task-lib-tables`.**
- Why: only KiCad's installed libraries and the project library could be used; no user library could be added.
- Done: the table format, path variables, the global (read only) and project tables, and the index, search, place, body and 3D lookups over a `LibSource` (`crates/kicad/src/library_table.rs`,
  `crates/cli/src/library_tables.rs`); Manage Symbol Libraries, Manage Footprint Libraries, Configure Paths, New Library and Add Library (`components/libraryTables/`, `actions/libraryTableActions.ts`);
  the derived KiCad project and `eda import-kicad` carry the tables and the variables; `GET /api/3dmodel` reads a model path with the board's variables (`library_tables::ModelNames`).
- Left: saving the Symbol and Footprint editors' work into a table library's file (the editors write the project library; Save, Save As and Export would write the library the row names), the library
  drop (a browser drop carries no path), Reset and Migrate Libraries, Edit Options..., symbol folders of `.kicad_sym` files, formats other than KiCad's, the 3D model search paths and Migrate 3D Models (P14).
- KiCad: `common/libraries/{library_manager,library_table,library_table_parser}.cpp`, `common/libraries/lib_table_grid_data_model.cpp`, `common/dialogs/{dialog_edit_library_tables,dialog_configure_paths}.cpp`,
  `eeschema/dialogs/panel_sym_lib_table.cpp`, `pcbnew/dialogs/panel_fp_lib_table.cpp`, `eeschema/tools/symbol_editor_control.cpp` and `pcbnew/tools/pcb_control.cpp` (`AddLibrary`, `NewLibrary`).
- Conflicts: P17 and P18 build on it; P2 (`library_place.rs`).

### P4. Schematic hierarchy: the rest (item 4; SR1's sheet half). Size M for what is left. Rank 4. **Merged 2026-10-10 (`task-sch-hierarchy`); item 4 lists what is left.**
- Done in the branch: Annotate with KiCad's dialog and numbering (`crates/model/src/{annotate,sheet_list}.rs`, `crates/ops/src/sch_annotate.rs`), Find and Replace across sheets, the SVG render and the gates over every sheet.
- Left: Import Sheet, Draw Sheet from File and the schematic drop (`Cmd::ImportSheet` = `AddSheet` + the file's screen from `import_kicad_sch`, renumbered with `annotate_paste`; a route that reads the file; the drop is done, 2026-10-21, `task-microwave`: item 4); hierarchical sheets
  in the clipboard; a net per placement of a shared screen and buses across sheets (done 2026-10-23, item 4); Reorganize keeping stored field places, label spins, looks and body styles; browser check for the hierarchy.
- Files: `crates/ops/src/{sheets,sch_clipboard}.rs` and a new `sheet_import.rs`, `crates/engine/src/{nets,hier}`, `crates/cli/src/{board,sch_hierarchy_api}.rs`, `crates/kicad/src/{sch_clipboard,sch_import}.rs`,
  `web/studio/src/components/{SchematicView,panels/HierarchyPanel}.tsx`, `kicad-port/{schSheet,schClipboard,navHistory}.ts`.
- KiCad: `eeschema/{sch_sheet_path,sch_screen,sch_sheet,connection_graph}.cpp`, `tools/{sch_drawing_tools (ImportSheet, DrawSheet),sch_navigate_tool,sch_editor_control (doCopy, Paste),sch_edit_tool (DdAppendFile)}.cpp`.
- Conflicts: P2 (`ops/lib.rs`, `board.rs`); P8 (the drop host); P10 (`SchematicView.tsx`); P18 (placing a block is Import Sheet).

### P5. Router: drag, mid-segment, diff pairs, meanders (item 7). Size XL, three parts. Rank 5. Independent of the rest. **P5a, P5b, P5c and P5d done; item 7 lists what is left.**
- **P5a, M-L. DONE 2026-10-10 on `task-router-drag` (7 commits, `68eb737` to `bc573db`; D7 and its rows in `CODE-COMPARE-router.md` closed).** What is there: `Line::drag_segment45`, `snapToNeighbourSegments` and `snapDraggedCorner` ported and checked against
  KiCad's own functions (80 000 random lines, point for point); `Dragger` with `DM_SEGMENT`/`DM_CORNER`, `dragCorner45`, `SHP_REVERSED`, the last-valid-point restore and the forced Highlight-collisions mode with Ctrl+click;
  `split_segment` and the commit that gives a track a vertex where a route starts or ends in it; `Router::continue_from_end` and `POST /api/route/continue_from_end`; replays of KiCad's `walk_drag_seg_against_board_edge`
  (same three segments) and `simple-drag-shove-singlelayer` (116 of 134, the same 139 tracks); a browser check. Left of the original text below: nothing, except what the item 7 paragraph lists as missing.
  The package as it was planned: `crates/pns/src/{dragger,line_placer,router,node}.rs` (D7: `DM_SEGMENT`, `dragCorner45`, `SHP_REVERSED`, last-valid restore; `SplitAdjacentSegments` so a route starts and ends
  mid-segment; Route From Other End after the first fix), `crates/cli/src/route_api.rs`, `web/studio/src/components/canvas/{routing,dragging}.ts`, `kicad-port/{routeTool,dragTool}.ts`, `actions/pcbRouterSweep.ts`.
- **P5b, L. DONE 2026-10-11 on `task-diff-pairs` (9 commits, `aa48f28` to `d9c5fe7`; the rows of section I of `CODE-COMPARE-router.md` are FAITHFUL or ADAPTED now, D18's "diff-pair shove/walkaround/via" is closed).** What is there: `diff_pair.rs` is `pns_diff_pair.cpp` (`DP_PRIMITIVE_PAIR`, `DP_GATEWAYS` with their
  priorities and the 22.5-degree continuation, `DIFF_PAIR::BuildInitial`, `CheckConnectionAngle`, the coupled length and skew) and `dp_placer.rs` is `pns_diff_pair_placer.cpp` (`Start` / `FindDpPrimitivePair`, `routeHead`, `rhMarkObstacles`, `rhWalkOnly` / `attemptWalk` / `tryWalkDp`, `rhShoveOnly`, `propagateDpHeadForces`,
  `ToggleVia` / `SetLayer`, `FixRoute`, `FlipPosture`); `shove.rs` runs several heads and takes a forced clearance (`ShoveObstacleLine`, `ForceClearance`) and `optimizer.rs` is `mergeDpSegments` with a memo of the world's answers; `POST /api/route/dp_{start,dims,move,via,fix,undo_segment,finish}`;
  the studio's pair tool runs in the router's mode, shows what the shove pushes, takes `V`. Proof: unit tests, `crates/pns/tests/dp_boards.rs` (KiCad's `dp_test.kicad_pcb`: no QA corpus session routes a pair), three HTTP tests, the slow kicad-cli gate
  `a_placed_pair_passes_kicad_clis_clearance_and_diff_pair_gap_checks`, a slow random-board sweep (`whatever_the_cursor_does_an_accepted_pair_leaves_the_board_clean_and_keeps_its_gap`) and the browser check. Left, item 7 lists it: Route From Other End for a pair; nothing for a pair drag (KiCad has none).
  The package as it was planned: `crates/pns/src/{diff_pair,shove,walkaround}.rs`, `diffPairRouting.ts`, `kicad-port/dpTool.ts`.
- **P5c, L. DONE 2026-10-13 on `task-length-tuning` (`0c3036f` to `543fa0d`: 8 commits before the docs, 4 after them; section J of `CODE-COMPARE-router.md` rewritten: 7 FAITHFUL, 10 ADAPTED, none DIVERGENT or MISSING).** What is there, in the order of the brief: (1) **a single track**: `kigeom.rs` and `chain.rs` (`SEG`, `SHAPE_ARC`, `SHAPE_LINE_CHAIN` with arcs), `meander.rs` (`MEANDER_SETTINGS`, `MEANDER_SHAPE`, `MEANDERED_LINE`, `tuneLineLength`),
  `tuning_path.rs` (`AssembleTuningPath`, the length with `LENGTH_DELAY_CALCULATION`'s pad and via rules), `meander_placer.rs` (`MEANDER_PLACER`), all checked against KiCad's own `pns_meander.cpp` and `pns_meander_placer_base.cpp` compiled over the real `libs/kimath` (`crates/pns/oracle/`, `tests/meander_oracle.rs`: 100 000 random cases agreed,
  118 baked into the always-on test); (2) **a pair and a skew**: `dp_meander_placer.rs` (`DP_MEANDER_PLACER`, `AssembleDiffPair`, `CoupledSegmentPairs`) and `MEANDER_SKEW_PLACER`; (3) **a pattern is an item**: `TuningPattern` (`crates/model/src/tuning.rs`, an overlay of `DrawingsSection`), `Cmd::CommitTuning` (one undo step for a new pattern, a pattern tuned again or removed),
  `crates/kicad/src/pcb_generated.rs` (`(generated ..)` written and read as KiCad does; KiCad's own `tuning_generators_load_save.kicad_pcb` and `issue22536.kicad_pcb` load with their members and write again), `crates/cli/src/{tune_api,tune_edit}.rs` and `crates/pns/src/tuning_commit.rs` (what a tuned line becomes in the IR),
  the studio's tool (`7`/`8`/`9`, the status box, `1`-`4`, Ctrl+L and E in the Tuning Pattern dialog, selection as a generator, Delete, the four edit points, the Generators Manager, `Generator.*`; `kicad-port/tuneTool.ts`, `components/canvas/tune*.ts`, `TuningPatternDialog.tsx`, `GeneratorsManagerDialog.tsx`),
  defaults from Board Setup's Length-tuning Patterns and a target from a `.kicad_dru` length or skew rule. The nine `pcbnew.Generator.*` rows are wired and out of `ui-parity-missing.json`. Proof: unit tests in each module, the oracle, 9 HTTP tests in `tune_api.rs` (hover, a track, a pair, a skew, a line of several tracks, tuned again and removed, the edit points, the refusals, a stale session), the slow kicad-cli test
  (`kicad_cli_finds_the_tuned_nets_within_the_length_and_skew_rules_they_were_tuned_to`: a track tuned to 22 mm within 10 um of its length rule, a pair within 25.9..26.1 mm per net and 0.3 mm of skew, a 1.4 mm skew under a 0.15 mm rule after the skew tuner; the same rules flag the nets before they were tuned) and
  `web/studio/e2e/length-tuning.check.js` (93 checks through the hotkeys, the canvas and the dialogs on a scratch board built from `examples/mcu_board_30plus.yaml`). After the docs, on KiCad's own boards (these need `KICAD_SRC_DIR`; the sweep is the slow tier): the four patterns of `issue22536` tuned again from their baselines to 76.2 mm exactly over its arcs (`65129b4`), the tuner's length of 8 nets of `length_calculations` within 3 um of kicad-cli's through pads, vias and the stackup (`8bbd7b2`), and 142 tunings of random tracks on eleven QA boards, every one accepted by the board, 41 single ones measured as the placer said, removed to the length they had and tuned again (`db9963a`). From (4): arcs were chords in the world (`from_ir.rs`; `ARC_T` items since P5d), and `ShoveTimeLimit` is KiCad's one second (`543fa0d`: `time_limit.rs`, checked after every iteration of the shove, `shove_time_limit` in the router settings). **Left, item 7 lists it: springback (done 2026-10-23 for the line placer and 2026-10-25 for the dragger and the pair placer), and the tuner's own gaps (pad-to-die, a merged outline for a pair, moving a pattern); P5d did the rest.**
- **P5d, M-L. DONE 2026-10-20 on `task-router-rest` (`d58c9b83`, `9f993489`, `2e5e7d84`, `4688f7bc`, `88313a95`, `aaa736d2`, `71e75174`, `321a8109`, `0a9aee67`, `cbe3afff`, `e8eae12f`, then the docs; `CODE-COMPARE-router.md` D18 and the rows of sections C, E, G, H, I, J and K, `crates/pns/PARITY.md` Stage 9).** The six steps of its brief, in order, one commit each: (1) **arcs** -- `arcs.rs` (`ArcPts`, `ArcHull`, `SplitAdjacentArcs`), `Item::Arc`, `Line::arcs` (found again by their points: whatever cuts into an arc turns it into segments, whatever leaves it alone keeps it), `arc_drag.rs` (`DM_ARC`: `EDIT_TOOL::DragArcTrack` as a drag of the router, refused from 179 degrees), a route starts and ends in the middle of an arc, the tuner starts on one; (2) **locked tracks** -- `from_ir` sets `locked` from `locked_ids` and the members of a pattern that is not in edit, a locked item is a solid for the shove, the dragger refuses it ("The selected item is locked.") and Drag Anyway drags it alone, the halves of a locked track a route cuts stay locked (`RouteCommit::lock_tracks`), `findRedundantSegment` / `findRedundantArc`; (3) **a head that ends in a via** is shoved with it (`rhShoveOnly`: `AppendVia( makeVia( CLastPoint() ) )`, `AddHeads`); (4) **the mouse trail** (`mouse_trail.rs`, checked against KiCad's own `MOUSE_TRAIL_TRACER` and `DIRECTION_45` compiled over `libs/kimath`: `crates/pns/oracle/build.sh`, 100 000 random walks agree) and **Optimize entire track being dragged** (the optimizer's `OPT_CONSTRAINT`s: `PRESERVE_VERTEX`, `KEEP_TOPOLOGY`, `RESTRICT_AREA`), both rows of Interactive Router Settings enabled; (5) **a pair**: Route From Other End (`ROUTER::ContinueFromEnd`, Ctrl+E, `POST /api/route/dp_continue_from_end`, the two commits one undo step) and `tryWalkDp`'s failure (the pair is emptied, as KiCad's); (6) **the time domain** -- `delay.rs` (`TUNING_PROFILE`: the delay of a layer's track and of a via, `CalculateDelay`), the placers' `calculateTimeDomainTargets`, `is_time_domain` / `target_delay` in picoseconds in the dialog, the status box and the board file. Two bugs found late by the proofs and fixed with them: a shove's commit wrote the arcs it had not reached back as segments (found by the kicad-cli proof on `issue8407`), and the clean-up after a walk dropped the vertex a tangent arc starts at (found by the browser check on a fillet), and a third by the browser check, in step 2: Drag Anyway had no dialog host on the PCB tab. Proof: 75 new Rust tests in `crates/pns` (`tests/{arc_tracks,arc_boards,locked_tracks,head_via,mouse_trail_oracle}.rs` and the units of each module), six in `crates/cli`, the slow kicad-cli proofs of item 7, and `web/studio/e2e/router-leftovers.check.js` (95 checks) with Ctrl+E in `diff-pairs.check.js`. Left, item 7 lists it: springback, `LockJoint`, virtual vias, rounded corners, and the tuner's gaps (the first four are done since: `LockJoint`, virtual vias and the rounded corners 2026-10-23, springback 10-23 for the line placer and 10-25 for the dragger and the pair placer).
- KiCad: `pcbnew/router/` (`pns_dragger`, `pns_multi_dragger`, `pns_line_placer`, `pns_diff_pair_placer`, `pns_meander*`, `pns_shove`, `pns_walkaround`, `pns_router`, `router_tool.cpp`), `pcbnew/generators/pcb_tuning_pattern.cpp`,
  `pcbnew/tools/generator_tool.cpp`.
- Conflicts: only among the three parts (`crates/pns`) and P6 in `Canvas.tsx` handlers. Each part ends with a replay of KiCad's own QA cases (`crates/pns/tests/qa_regressions.rs`) and a browser check.

### P6. PCB editing details (items 8, 15, 17, 35). Size M, ongoing. Rank 6. **Mostly done; merged 2026-10-10 (`task-pcb-edit-details`).**
- Status: everything in the first bullet below is done except what items 8, 15, 17 and 35 list as open: the router's rows of item 15 (Enter during a route, Route From Other End, `V`) and the router's commits in an entered
  group wait for P5; Select Connection does not follow a chain of graphic lines; microvias, annular rings, IPC-4761 and the footprint-owned scopes of the global text edit wait for a via padstack and for P1; the rest of item 35
  (rule areas, teardrops, dimensions, the zone debug views, board flip, track width) is untouched and belongs to P13, P16 and the router.
- Paste Special on the PCB (`DIALOG_PASTE_SPECIAL`, `PASTE_MODE`, clear nets; a plain Paste then keeps references as KiCad's default does), the rotation step and flip direction settings, a footprint's bounding box and the anchor half of
  `BestSnapAnchor` for Rotate and Flip, the routing kept when a footprint with routed pads is edited, Duplicate and Paste as one step with the drop, items drawn while a group is entered joining it, the item 15 list, the item 35 tail.
- Files: `web/studio/src/components/canvas/{Canvas.tsx,clipboard.ts}`, `state/store.tsx`, `actions/{pcbEditSweep,pcbMenuActions}.ts`, `kicad-port/{pcbTransform,pcbCarry,pcbReference,alignDistribute,groupEdit}.ts`,
  `crates/ops/src/{pcb_paste,pcb_transform,pcb_groups}.rs`, `crates/kicad/src/clipboard.rs`.
- KiCad: `pcbnew/tools/{edit_tool,edit_tool_move_fct,pcb_control,pcb_selection_tool,pcb_group_tool,drawing_tool}.cpp`, `common/dialogs/dialog_paste_special.cpp`, `pcbnew/dialogs/{dialog_cleanup_tracks_and_vias,dialog_global_edit_*}.cpp`.
- Conflicts: P5 (`Canvas.tsx`), P16 (`Canvas.tsx`, `painter.ts`), P10 and P11 (`store`, `runner`: hunks).

### P7. Custom-shape pads (item 31; also 13, 19). Size L. Rank 7. **Merged 2026-10-10 (`task-custom-pads`); item 31 lists what is left.**
- Why: a custom pad imports as its anchor, a trapezoid or chamfered pad is a rectangle; zone fill is wrong around them (766% and 11% on two QA zones); four rows wait.
- Files: `crates/model/src/{footprint.rs,ir.rs}` (`Pad` and `LibraryPad`: anchor, primitives, trapezoid and chamfer), `crates/kicad/src/{footprint_lib,footprint_import,import,pcb}.rs` (`(primitives ..)` both ways),
  `crates/zone-filler` (`addKnockout` custom-pad mode, thermal proxy spokes; deleted 2026-10-13), `crates/pns/src/from_ir.rs`, `crates/drc/src/board.rs` (`placed_pad_copper`), `components/BoardPadPropertiesDialog.tsx`,
  `components/footprint/PadPropertiesDialog.tsx`, `canvas/painter.ts`, `components/footprint/footprintPainter.ts`, `kicad-port/pad*.ts`, `actions/libraryEditorActions.ts` (explode and recombine).
- KiCad: `pcbnew/pad.cpp`, `tools/pad_tool.cpp` (`EditPad`), `dialogs/dialog_pad_properties.cpp`, `zone_filler.cpp` (`addKnockout`), `microwave/microwave_polygon.cpp`.
- Conflicts: P1 (footprint IR, writer, painter), P17 (pad dialogs), P19 (arc stub, function shape), P14 and P2 (`fp_edit.rs` pad edits).

### P8. Files in: Append Board, DXF and SVG, drops (item 28; SR1's PCB half). Size S for what is left. Rank 8. **Mostly done: `task-files-in` (2026-10-11); item 28 lists what is left. Other EDA formats: P22, below.**
- Done in the branch: **P8a** the drop host (`components/FileDropHost.tsx`, `kicad-port/fileDrops.ts`), Append Board and its drop (`crates/cli/src/append_api.rs`, `kicad-port/appendBoard.ts`, then `Cmd::PasteClipboard` with the carry) and the Footprint Editor's drop; **P8b** Import Graphics for the board, the schematic and the symbol editor:
  the readers in `crates/kicad/src/{graphics_import,dxf_import,svg_import}.rs`, the verbs in `crates/ops/src/{import_graphics,import_graphics_sch,fix_shapes}.rs`, `crates/cli/src/graphics_api.rs`, `components/ImportGraphicsDialog.tsx`.
  Tests: 48 kicad unit tests and 11 integration tests on fixtures, 35 ops tests, 3 slow kicad-cli proofs, 15 cli tests, 23 web unit tests, and `e2e/files-in.check.js` (102 checks).
- Left: **P8c, M, low priority**: Import Netlist; the rest is in item 28's "Left".
- KiCad: `common/import_gfx/{dxf_import_plugin,svg_import_plugin,graphics_importer,graphics_importer_buffer}.cpp`, `pcbnew/import_gfx/{dialog_import_graphics,graphics_importer_pcbnew}.cpp`, `eeschema/import_gfx/`,
  `pcbnew/tools/pcb_control.cpp` (`AppendBoard`, `placeBoardItems`, `DdAppendBoard`), `pcbnew/tools/board_editor_control.cpp` (`ImportNetlist`).
- Conflicts: P4 (the sheet drop), P3 (the library drop), P6 (`store.tsx` `placeNewItems` hands a placement to the Move tool beside `pasteClipboard`); `ops`, `runner`, `studio` hunks.

### P9. Board Setup completion and the rule editor (item 3). Size L; the rule editor XL on its own. Rank 9.
- **Status (2026-10-12): P9a done on `task-board-setup`** except Component Classes (`component_classes_drc`). **P9b done on `task-rule-editor` (2026-10-23)**: the DRC Rule Editor (`PARITY-pcb.md` section 48; left: Show Matches, the Syntax help window). (Embedded Files: done 2026-10-23, P26.) Item 3 lists what each page does and what is left. (P23, 2026-10-19: the component classes of an imported project are in the IR and written to the derived project, so `component_classes_drc` is judged as the original is; the page that edits them is what is left.)
- **P9a, L**: the missing pages: Board Editor Layers, Text Variables (project variables and `${VAR}` expansion in texts and fields), Formatting, Zone Hatch Offsets, Length-tuning Patterns, Tuning Profiles (`missing_tuning_profile`),
  Component Classes (`component_classes_drc`), regular-expression net patterns and composite net classes, and drawing tools that start from the Text & Graphics defaults. Files: `crates/model/src/rules.rs`,
  `crates/ops/src/board_setup.rs`, `crates/kicad-engine/src/lib.rs` (project file), `crates/kicad/src/{pcb,import,custom_rules}.rs`, `web/studio/src/components/{BoardSetupDialog.tsx,boardSetup/*}`, `kicad-port/boardSetupRules.ts`.
- **P9b, XL, done 2026-10-23 (`task-rule-editor`)**: the rule-tree designer (`DRETool.drcRuleEditor`): `eda_drc::rule_editor` (loader, matcher, saver, the panels' data and checks), `Cmd::EditDrcRule`, `GET /api/rule_editor`, `components/RuleEditorDialog.tsx`.
- KiCad: `pcbnew/dialogs/panel_setup_{layers,formatting,zone_hatch_offsets,tuning_patterns,tuning_profiles,tuning_profile_info}.cpp`, `panel_assign_component_classes.cpp`, `pcbnew/component_classes/`,
  `common/dialogs/{panel_text_variables,panel_setup_netclasses}.cpp`, `common/project/component_class_settings.cpp`, `pcbnew/drc/rule_editor/` (82 files), `pcbnew/tools/drc_rule_editor_tool.cpp`.
- Conflicts: P15 (the project writer in `kicad-engine`: a hunk); `ops`, `runner` hunks.

### P10. Schematic details (items 1, 12, 15, 17, 20). Size M-L. Rank 10. **Mostly done on `task-sch-details` (2026-10-12, 15 commits, not merged) and, for the schematic's groups, on `task-sch-groups` (2026-10-13 to 15, P10b, not merged); a label's own fields, shared screens and buses across sheets done on `task-sch-details` 2026-10-23.**
- Done there: pin selection (`SCH_PIN_T`: Swap Pins, Swap Pin Labels, Swap Unit Labels, the pin-level highlight and Select Node), sheet pins one by one, the net-collision overlay of a drag, `AutoRotateItem` after a label lands, a power symbol's Properties dialog,
  text justification flipping on a mirror, pan with the middle and right button and auto-pan while drawing and moving, the Symbol Fields Table's attribute columns and Exclude DNP filter, Copy as Text, part of the item 15 schematic list. `PARITY-sch.md` sections 21 to 27;
  `web/studio/e2e/sch-details.check.js`; the kicad-cli proof is `crates/kicad/tests/sch_details.rs`.
- Done on `task-sch-groups` (P10b, `PARITY-sch.md` section 29): schematic groups (`sch_group_tool.cpp`, `group_tool.cpp`; item 17): Group, Ungroup, Add and Remove Items, Group Properties, whole-group selection, enter and leave, the tree under move, turn, delete and the clipboard, `(group ...)` in the file; `web/studio/e2e/sch-groups.check.js`; the kicad-cli proof is `crates/kicad/tests/sch_groups.rs`.
- Done on `task-sch-groups` (P10b, `PARITY-sch.md` section 30): a user-defined field's place on the sheet (item 12): the symbol's user fields listed, drawn, placed, moved, turned, edited, added, removed, written and read back; `crates/kicad/tests/sch_user_fields.rs` and the slow proof in `sch_groups.rs` (kicad-cli draws the field where we put it).
- Done on `task-sch-groups` (P10b, `PARITY-sch.md` section 31): Pin Helpers and swapped pins through the file and the clipboard (items 1, 12); `crates/kicad/tests/sch_pin_swaps.rs`, `crates/ops/src/sch_pin_helpers_tests.rs`.
- Done on `task-sch-groups` (P10b, `PARITY-sch.md` section 32): a sheet pin's text look (size, bold, italic: Sheet Pin Properties, the painter, the pin's box, `(effects (font ..))` written and read; `SchExtras::sheet_pin_looks`).
- Done on `task-sch-details` (2026-10-23, `PARITY-sch.md` section 39): a label's own fields (its net class), a global label's Sheet References and Schematic Setup's Inter-sheet
  References; a screen shown by two sheets (a net per placement, Paste sharing it); buses across sheets. `web/studio/e2e/sch-details-2.check.js`.
- Reviewed 2026-10-23 (`task-p10-review`, `PARITY-sch.md` section 40): a turned or mirrored symbol on a sheet this project drew is traced, written and its fields placed
  by KiCad's `TRANSFORM`, as it is drawn (the engine turned it the other way, mirrored it about its box and ignored a top-bottom mirror: kicad-cli and the studio's nets had
  its pins elsewhere); two references of one symbol turned differently each get their cache copy; a label keeps a spin of its own (RIGHT by default); Mirror of a held
  selection is about its centre; auto-rotate by bounding box; an empty field takes no row in Autoplace; the field dialogs' names and messages; a graphic line written as
  two-point polylines.
- Left: a sheet pin's font face and colour; a label's field moved by hand; see items 4 and 12.
- Files: `web/studio/src/components/SchematicView.tsx`, `components/schematic/{schItems,schHit,schSelectionSummary}.ts`, `kicad-port/{schMove,selection,schConnection}.ts`, `components/SymbolFieldsTableDialog.tsx`,
  `crates/ops/src/{sch_move,sch_edit,sch_props,sch_fields,fields_table}.rs`, `crates/model/src/sch_extras.rs`.
- KiCad: `eeschema/tools/{sch_edit_tool,sch_selection_tool,sch_drag_net_collision,sch_group_tool,sch_move_tool}.cpp`, `sch_label.cpp`, `sch_pin.cpp`, `dialogs/dialog_symbol_fields_table.cpp`, `fields_data_model.cpp`, `common/view/wx_view_controls.cpp`.
- Conflicts: P4 (`SchematicView.tsx`, `board.rs`), P6 (`store`, `runner` hunks), P16 (the schematic image).

### P11. Search and inspection (items 21, 29; SR2). Size M-L. Rank 11. **Done 2026-10-13 on `task-search-inspect` (7 commits, not merged); items 21 and 29 list what is left.**
- Done there: Find by Properties (the From Selection tab over `PCB_PROPERTIES` and the Query tab on a port of `libeval_compiler`), Find following the IR (user fields, hidden fields, zone names), Clearance and Constraints Resolution (a rule engine in `crates/drc`:
  the expression language, the `.kicad_dru` parser and `EvalRules` returning the winner and every rule considered, with KiCad's report text, behind `crates/cli/src/inspect_api.rs`) and the Footprint Checker (kicad-cli's DRC on a scratch board of
  `crates/kicad-engine/src/footprint_check.rs`). `PARITY-pcb.md` section 32, `PARITY-fpedit.md` section 12; `web/studio/e2e/search-inspect.check.js` (11 scenarios, 131 checks); the kicad-cli proofs are `inspect_api::tests::the_resolved_clearance_is_what_kicad_cli_enforces`
  and `footprint_check::tests::kicad_cli_finds_what_the_footprint_checker_finds` (both behind `EDA_SLOW_TESTS=1`). The four rows are out of `ui-parity-missing.json`.
- Left: the tuning-profile implicit rules, component classes and sheet paths, a footprint's user fields and paste margin, text variables in the rules text (item 29); the Search panel link and the geometry functions of the Query tab (item 21).
- Files: `web/studio/src/kicad-port/{libeval,pcbExpr,findByProperties,inspectReport,inspectPick,footprintChecker}.ts`, `actions/{findByPropertiesActions,inspectActions,footprintCheckerActions,pcbFindActions}.ts`, `components/{FindByPropertiesDialog,InspectReportDialog}.tsx`,
  `components/footprint/FootprintCheckerDialog.tsx`, `crates/drc/src/{libeval,pcbexpr,dru,constraints,rule_item,rule_board,inspect,known_props}.rs`, `crates/cli/src/{inspect_api,kicad_engine}.rs`, `crates/kicad-engine/src/footprint_check.rs`.
- KiCad: `pcbnew/dialogs/dialog_find_by_properties.cpp`, `pcbexpr_evaluator.cpp`, `tools/board_inspection_tool.cpp` (`InspectClearance`, `InspectConstraints`), `drc/drc_engine.cpp` (`EvalRules`), `dialogs/dialog_footprint_checker.cpp`.
- Conflicts: none on the Rust hubs; `runner` and `store` for the dialog flags (none needed: the dialogs keep their own module state). Compare Footprint with Library belongs to P2.

### P12. Board outline leftovers (item 27). Size M. Rank 12.
- Files: `crates/place/src/lib.rs` (the region from the outline's bounding box: use the outline with its cutouts, `Region` of `crates/gates/src/pcb.rs`), `crates/ops/src/{lib.rs,outline.rs}` (`Cmd::PlaceAt` and the snap), `crates/freeroute/src/design.rs`
  (`to_dsn` boundary and cutouts as keep-outs), `web/studio/src/components/viewer3d/scene.ts` and `kicad-port/pcbOutline.ts` (NPTH holes through the slab), `crates/model/src/{ir,outline}.rs` (a rectangle's corner radius, `gr_poly` arcs),
  an outline editor on the generic shape tools, "Fix discontinuities in board outlines".
- KiCad: `pcbnew/convert_shape_list_to_polygon.cpp`, `pcbnew/board.cpp` (`GetBoardPolygonOutlines`, `aIncludeNPTHAsOutlines`), `specctra_import_export/specctra_export.cpp`, `drc/drc_test_provider_misc.cpp`.
- Conflicts: P19 (the DSN boundary), P1 and P7 (`ir`: a hunk each), P13 (mask-only NPTH holes are kept and written, done).

### P13. Zone leftovers (item 13). Size L. Rank 13. **Done 2026-10-13 on `task-zones`, waiting for its merge; item 13 lists what is left.**
- Non-copper and multi-layer zones (the IR's layer set, the reader, the writer, Zone Properties), then, by decision of 2026-10-13, **the zone fill is KiCad's through kicad-cli and the Rust filler is deleted**: Fill All Zones
  on a copy of the derived board, the fills kept beside `design.json`, shown between fills as KiCad shows them, read by the writer, live connectivity and the 3D stand-in; Auto-Assign Priorities, the Zone Manager's preview,
  a zone lock in Zone Properties. The planned fill-fidelity and speed steps for our own filler are superseded.
- Files: `crates/kicad/src/{zone_fills,pcb,pcb_items,import}.rs`, `crates/kicad-engine/src/lib.rs` (`refill_zones`), `crates/drc/src/{zone_copper,shape_poly}.rs`, `crates/connectivity/src/*` (the zone copper),
  `crates/ops/src/zone_priority.rs`, `crates/cli/src/{fill_api,studio,fab_api,board}.rs`, `crates/model/src/ir.rs` (`Zone::layers`, `Zone::uuid`, `FootprintExtra::holes`),
  `components/{ZoneDialog,ZoneManagerDialog,ZonePreview,PlotDialog,StatusBar}.tsx`, `state/store.tsx`, `kicad-port/{zoneLayers,checkRevision}.ts`, `e2e/zones.check.js`.
- KiCad: `pcbnew/zone.cpp`, `zone_utils.cpp`, `zone_manager/`, `dialogs/panel_zone_properties.cpp`, `dialogs/dialog_non_copper_zones_properties.cpp`, `pcbnew_jobs_handler.cpp` (`JobExportDrc`).
- Conflicts: none left (`zone-filler` is gone); `kio` hunk.

### P14. The 3D models of a footprint (item 30; SR3). Size M. Rank 14. **Done 2026-10-13 on `task-fp-3d`, waiting for its merge; item 30 lists what is left.**
- A model list on `FootprintEdit` (`models3d`) with its verb, a 3D Models tab in both Footprint Properties dialogs (`PANEL_FP_PROPERTIES_3D_MODEL`: list, offset, scale, rotation, opacity, show; path chooser through `model3d_api`; a preview that
  reuses `viewer3d/modelCache.ts`), the library editor's `model` becoming a list, Migrate 3D Models, the 3D preview of the Footprint Chooser.
- Files: `crates/model/src/{fp_edit,ir}.rs`, `crates/ops/src/{fp_edit,library_editors}.rs`, `crates/kicad/src/{footprint_lib,footprint_import,pcb}.rs`, `components/{FootprintPropertiesDialog.tsx,footprint/FootprintPropertiesDialog.tsx}`,
  `components/FootprintChooserDialog.tsx`.
- KiCad: `pcbnew/dialogs/panel_fp_properties_3d_model.cpp`, `dialog_footprint_properties.cpp`, `dialog_footprint_properties_fp_editor.cpp`, `dialog_migrate_3d_models.cpp`, `3d-viewer/3d_cache/`.
- Conflicts: P2 and P7 (`fp_edit.rs`), P1 (the footprint), P17 (the editor dialog).

### P15. Review workflow leftovers (item 11). Size S-M. Rank 15. **Done 2026-10-14 on `task-review`, waiting for its merge; item 11 lists what is left.**
- ERC exclusions into the derived project (`erc.erc_exclusions`, `SCH_MARKER::SerializeToString`, positions guessed as `eda_kicad::drc_exclusions` does) with their comments; Delete Marker, Delete All Markers and Save report; "Exclude all violations of
  rule"; a marker selected by a left click; the Inspect / Fix entries; Edit connection grid spacing. The kicad-cli marker position stays upstream and unplanned.
- Files: `crates/kicad-engine/src/lib.rs`, `crates/kicad/src/drc_exclusions.rs` and a new `erc_exclusions.rs`, `crates/ops` (`AddErcExclusion` gets a comment), `components/{ErcDialog,DrcDialog,SchematicSetupDialog}.tsx`.
- KiCad: `eeschema/sch_marker.cpp`, `eeschema/schematic.cpp` (`ResolveERCExclusions`), `eeschema/erc/erc_settings.cpp`, `dialogs/dialog_erc.cpp`, `pcbnew/dialogs/dialog_drc.cpp`, `common/rc_item.cpp`.
- Conflicts: P9 (the project writer: a hunk).

### P16. PCB item kinds the IR lacks (item 24). Size L. Rank 16. **Done in part 2026-10-21 on `task-pcb-items` (P16i: the board's text box, reference images, points, hatch fills and barcodes and the schematic's images), waiting for its merge, main (51eb6ae, with the tables of P16t) merged in; item 24 lists what is left.**
- Done there, one commit per step: the text box (step 1), the reference image and the schematic's image (step 2), the point (step 3a), hatch fills with Rehatch Shapes (step 3b) and barcodes (step 4), each with a reader, a writer, a painter, a hit test, Properties rows and undoable verbs, Rust tests, `node --test` cases, a kicad-cli proof (slow tier) and scenarios of
  `web/studio/e2e/pcb-items.check.js`; the 12 QA boards still re-export as the originals do. `PARITY-pcb.md` section 39, `PARITY-sch.md` section 36.
- The plan was:
- Text box first (`PCB_TEXTBOX`), tables (`PCB_TABLE`, Place Stackup, Place Board Characteristics: the fab drawing tables) with the schematic's, barcodes, points, reference images, hatch fills, the schematic image. Each is an IR item, a reader and a writer, a painter,
  a hit test, Properties rows and verbs.
- Files: `crates/model/src/ir.rs`, `crates/kicad/src/{pcb,pcb_items,import_items}.rs`, new files in `crates/ops/src/`, `canvas/painter.ts`, `kicad-port/{pcbItems,pcbProperties}.ts`.
- KiCad: `pcbnew/pcb_textbox.cpp`, `pcb_table.cpp`, `pcb_tablecell.cpp`, `pcb_barcode.cpp`, `pcb_point.cpp`, `pcb_reference_image.cpp`, `dialogs/dialog_{table,tablecell,barcode,reference_image}_properties.cpp`, `tools/pcb_edit_table_tool.cpp`,
  `tools/drawing_tool.cpp`, `eeschema/sch_{table,bitmap}.cpp`, `eeschema/tools/sch_edit_table_tool.cpp`.
- Conflicts: P1 (`painter.ts`, `ir`, `kio`), P6 (`Canvas.tsx`), P7 (`ir`).
- **P16t, the tables, done 2026-10-21 on `task-tables`, waiting for its merge; item 24 lists what is left.** One commit per step: the board's table as an IR item with its reader, writer, painter and hit test (1a, 1b); the table verbs, Draw Table, Edit Table, the dialogs, the structure edits, Select Column/Row/Table and Export Table to CSV (2a to 2d); Place Stackup Table and
  Place Board Characteristics Table (3); the sheet's table: the IR and the file, the kicad-cli proofs, the verbs on the sheet and the move scene, the painter, Draw Table, the dialogs, the context menu, the Properties rows (4a to 4d). New files: `crates/model/src/{table,table_text,table_edit}.rs`, `crates/kicad/src/{pcb_table,sch_table_io}.rs`,
  `crates/ops/src/{pcb_tables,pcb_table_builders}.rs`, `crates/cli/src/table_json.rs`, `web/studio/src/kicad-port/{tables,pcbTableProperties,schTableProperties}.ts`, `components/{TableDialogs.tsx,schematic/schTablePaint.ts}`, `actions/{tableActions,pcbTableActions,schTableActions}.ts`.

### P17. Library editor leftovers (items 14, 19, 22). Size L. Rank 17. **Done 2026-10-16 on `task-lib-editors`, waiting for its merge; items 19 and 22 list what is left.**
- Done there, one commit per step: the footprint editor's pad array and numbering, derived symbols (backend and editor), snapping in both editors, a fields grid in the library editor's Footprint Properties, symbol fields on the canvas, alternate pin functions, the symbol text box and
  a Shape Properties dialog. Every step has Rust tests, `node --test` cases and a scenario of `web/studio/e2e/lib-editors.check.js` (24 scenarios through `window.__eda`, run in a headless Chromium); the kicad-cli proofs (behind `EDA_SLOW_TESTS=1`, in `crates/cli/src/lib_editors_tests.rs`): a BGA pad array,
  a footprint with fields, a library with derived symbols (and a flat symbol plotted as kicad-cli plots the derived one), alternate pin functions and text boxes each load and `fp upgrade` / `sym upgrade` keep them. `PARITY-fpedit.md` sections 13 to 15, `PARITY-symedit.md`.
  Shared effects: a raw line feed inside a quoted string made kicad-cli refuse the library, so `sexpr_str` writes `\n` and `\r` as `OUTPUTFORMATTER::Quotes` does and `sexpr::parse_quoted` reads them back (this also fixes a multi-line text box of the schematic's sheet).
- The plan was: snapping to other items' anchors in both editors (wire `PcbGridHelper` and `SchGridHelper`), the footprint editor's pad array and numbering (`ARRAY_AXIS`, `ARRAY_PAD_NUMBER_PROVIDER`), a fields grid in the library editor's Footprint Properties, derived symbols
  (`extends`, Flatten, Update Symbol Fields, the related-fields table), symbol fields on the canvas (Show Hidden Fields), alternate pin functions (`togglePinAltIcons`), the symbol text box, a shape properties dialog.
- Files: `crates/model/src/symbol.rs`, `crates/ops/src/library_editors.rs`, `crates/kicad/src/{symbol_lib,symbol_import,footprint_lib}.rs`, `components/{footprint,symbol}/*`, `kicad-port/{padNumbering,pinNumbering,pcbGridHelper,schGridHelper}.ts`.
- KiCad: `eeschema/symbol_editor/*`, `eeschema/lib_symbol.cpp`, `pcbnew/tools/array_tool.cpp`, `pcbnew/array_pad_number_provider.cpp`, `pcbnew/dialogs/dialog_create_array.cpp`.
- Conflicts: P7 (pad dialogs), P3 and P14 (`libs`, `fpui`), P1.

### P18. Design blocks (item 32; SR5). Size L. Rank 18. **Done 2026-10-20 on `task-design-blocks`, waiting for its merge; item 32 lists what is left.**
- Done there, in the order of the plan: the library (`crates/kicad/src/design_block.rs`: read, write, enumerate, delete; the metadata as nlohmann dumps it) and its table (`design-block-lib-table` as the third `TableKind`, Manage Design Block Libraries, New Library); the schematic's pane (tree, search, preview, place through Import Sheet, flat or as a sheet, grouped and linked);
  saving and editing (Save Sheet / Selection, Update, Properties, Delete, the group's library link); the board's half (the pane, Place Design Block through Append Board's carry, Save Board / Selection). Every edit is one undoable verb (`import_sheet` with a block, `place_design_block`, `link_design_block`, `edit_group`'s link). Rust tests in `crates/{kicad,ops,cli}` and `node --test` cases (`kicad-port/designBlocks.test.ts`, `svgPreview.test.ts`, `schImportSheet.test.ts`); the kicad-cli proofs
  (`EDA_SLOW_TESTS=1`, `crates/cli/src/design_block_tests.rs`): a schematic block and a board block this studio writes load in kicad-cli (`sch export`, `pcb drc`), and a block made from one of KiCad's own templates places with its parts and nets; `web/studio/e2e/design-blocks.check.js` (10 scenarios, 106 steps through `window.__eda`). `PARITY-sch.md` section 34, `PARITY-pcb.md` section 36.
- The plan was:
- A block library (the table of P3, `.kicad_blocks` folders with a `.kicad_sch`, a `.kicad_pcb` and metadata), the panels, place (schematic: Import Sheet; board: Append Board), place linked, save selection, sheet or board as a block, update, properties, delete.
- Files: new `crates/kicad/src/design_block.rs`, `crates/ops` (new file), `crates/cli` (a new `design_block_api.rs`), `components/panels/DesignBlockPanel.tsx`, dialogs.
- KiCad: `eeschema/tools/sch_design_block_control.cpp`, `pcbnew/tools/pcb_design_block_control.cpp`, `common/tool/design_block_control.cpp`, `common/design_block{,_info,_io,_library_adapter}.cpp`,
  `common/widgets/{design_block_pane,panel_design_block_chooser}.cpp`, `common/dialogs/dialog_design_block_properties.cpp`.
- Conflicts: P3, P4 (Import Sheet), P8 (Append Board); `ops`, `studio`, `runner` hunks.

### P19. Microwave tools and Specctra (items 9, 27; SR3, SR6). Size S-M each. Rank 19. **The microwave half is done 2026-10-21 on `task-microwave`; item 9 lists what is left of it. Specctra (P19b) is done 2026-10-23 on `task-specctra-wip` (merged); `PARITY-pcb.md` section 46.**
- **Specctra session import through kicad-cli api-server** (2026-10-28, `task-kicad-api-server`): `Cmd::Specctra` asks KiCad's `ImportSpecctraSession` on the api-server lane (`eda_kicad_engine::api::specctra`; the importer `main` sets, `eda_ops::set_session_importer`) and reads the new tracks, arcs, vias and moved footprints back by `GetItems`; `crates/kicad/src/specctra` is INACTIVE, the fallback where the lane is closed (`EDA_KICAD_API=0`, a kicad-cli without `api-server`). Proof: `crates/cli/tests/specctra_api_server.rs` (slow), 10 of the 13 core examples routed by `crates/freeroute` (3 do not route at seed 0) imported both ways: the same tracks and vias to the nanometre, within 50 nm of the routed board (the session's 0.1 um), and a part moved, turned and flipped the same both ways. Left: a part the board has not got is skipped by KiCad 25ec2335 and refused by the port (8303b2ad), by design; the import through the server takes 1.3-2.2 s from a cold `eda` process (server start and two board loads) against 0.2-0.8 s for the port, and runs on the studio's request loop as `/api/cmd` does; the design's net names after an import are the board's (`/FB`) where the pipeline's design has the intent's (`FB`), both ways (the net names decision, not this branch). The api-server lane's export jobs cover `drill`, `pos` and `ipcd356`; the other kinds (Gerbers, plots, STEP, IPC-2581, ODB++, GenCAD, IDF, statistics, the schematic's jobs) are per-run until their messages are mapped from kicad-cli's arguments and proved byte for byte (`crates/kicad-engine/tests/api_jobs.rs` is the pattern).
- **Specctra DSN through kicad-cli** (2026-10-23, `task-specctra-cli`): the export is `kicad-cli pcb export specctra` where the kicad-cli found has it (asked once per process); our `export_dsn` is the fallback, marked DEPRECATED. Delete it once every machine's kicad-cli has the command: then `crates/kicad/src/specctra/export.rs`, `export_dsn`, `DsnExport` and `shape_in_nm` in `mod.rs`, the fallback branch of `crates/cli/src/specctra_api.rs`, the writer and export tests (`tests.rs`; the session tests that build their DSN with `export_dsn` need another source, such as a DSN kicad-cli wrote), and `the_dsn_is_kicad_clis_down_to_the_wiring` go; the SES import keeps `lexer.rs`, `parse.rs`, `model.rs` (with `format.rs`, which its `Format` functions and `echo_session` write through), `geom.rs`, `board.rs` (the board `FromSESSION` reads) and `session.rs`.
- **Specctra, P19b** (2026-10-23, `task-specctra-wip`): a port of `SPECCTRA_DB` of our own, an approved exception to "kicad-cli is the only exporter" (kicad-cli has no DSN or SES command): the element tree with KiCad's writer, lexer and parser
  (`crates/kicad/src/specctra/{model,format,lexer,parse}.rs`), `FromBOARD` on the board KiCad would read (`export.rs`, `board.rs`: padstacks, images, net classes, the outline with its cutouts from Edge.Cuts as `BuiltBoardOutlines` makes it, the wiring),
  and `FromSESSION` (`session.rs`) applied by `Cmd::Specctra` (`crates/ops/src/specctra.rs`, one undo step where KiCad clears the undo list): the unlocked tracks and vias go, locked ones stay, a `(type fix)` wire comes in locked, the placement moves, flips and
  turns the footprints it names, a blind/buried/micro via keeps its type, and the errors are KiCad's `IO_ERROR` words. The studio: File > Export > Specctra DSN... (`POST /api/specctra/export`, a download named `<board>.dsn`) and File > Import > Specctra Session...
  (the chosen file's text in the command); the CLI: `eda board export specctra -o FILE.dsn`, `eda board import-ses FILE.ses`. Both rows are out of `ui-parity-missing.json`. Left: `DeleteMARKERs` has nothing to delete (kicad-cli's report is never stored), the
  session's `structure_out`, `was_is` and `supply_pin` are read and dropped as KiCad drops them, a microvia whose padstack name holds no drill takes the net class's via drill (the model has no per-class microvia drill), no golden DSN (KiCad's `qa/data`
  has none; the writer is held to a DSN KiCad 10.99 wrote, `crates/freeroute/parity/fixtures`), and no router is run: the round trip uses the session a router that keeps the routing would send (`echo_session`). Measured: 11 new kicad tests (5 of the writer, 6 of
  `FromSESSION`), 2 ops tests, 2 cli tests in `crates/cli/tests/specctra_session.rs` (one slow: kicad-cli's DRC of the board the session made has no violation the routed board did not have), 3 `node --test` cases, and `web/studio/e2e/specctra.check.js`, 5 scenarios,
  passed in headless Chromium on the routed opamp filter.
- Done there, one commit per step: Gaps, Stubs and Arc Stubs with the length dialogs (step 1), Lines, the inductor with KiCad's four messages (step 2), Polygonal Shapes with the Complex Shape dialog (step 3), and a schematic dropped on the sheet (step 4: `eeschema.EditorControl.ddAppendFile`, item 4); then the Footprint Editor's pad `layers` fix and
  `web/studio/e2e/microwave.check.js`. The six rows (`pcbnew.MicrowaveTool.createGap`, `createStub`, `createStubArc`, `createFunctionShape`, `createLine` and `eeschema.EditorControl.ddAppendFile`) are out of `ui-parity-missing.json`. New files: `crates/ops/src/{microwave,microwave_tests}.rs`, `crates/cli/src/{microwave_api,microwave_tests,sheet_drop_api}.rs`,
  `web/studio/src/{actions/microwaveActions.ts,components/MicrowaveDialogs.tsx,state/microwaveDialogs.ts,kicad-port/{microwave,sheetDrop}.ts,api/{microwaveClient,microwaveTypes,sheetDropClient}.ts}`. `PARITY-pcb.md` section 41, `PARITY-sch.md` section 37, `PARITY-fpedit.md` section 8's Microwave tools row.
- Plan (what was before): Microwave: the gap and the stub now (a dialog for the size, a two-pad footprint builder in `kicad-port/microwave.ts`, `Cmd::PlaceFootprint` with a `definition`); the arc stub and function shape after P7; the line needs the inductor generator
  (`microwave_inductor.cpp`). Specctra (still the plan): `eda_freeroute::design::to_dsn` exposed as a download, the boundary with cutouts (P12), then an SES reader and a verb that adds the tracks and vias.
- KiCad: `pcbnew/microwave/{microwave_tool,microwave_footprint,microwave_inductor,microwave_polygon}.cpp`, `pcbnew/specctra_import_export/{specctra,specctra_export,specctra_import}.cpp`.
- Conflicts: P7 (custom pads for two rows), P12 (the boundary), `ops` and `runner` hunks.

### P20. Browser checks for the older areas (item 33). Size M each, standing. Rank 20. **Done 2026-10-23 on `task-p20-browser-checks` (not merged); item 33 lists what is left.**
- One `web/studio/e2e/<area>.check.js` per area in the pattern of the ones that exist (a copy of a board, `window.__eda`, every edit undone and the board compared). Done: the board
  editor's basics (`board-edit`), the schematic editor's (`sch-edit`), the library editors' (`lib-edit`), the rest of Board Setup (`board-setup-rules`), plot and fab outputs
  (`plot-exports`), and `tools/run-all-checks.sh`, which runs all of them on the boards their headers name. Left: the board's view controls and context menu, the schematic's
  Find and Properties panel, Import Sheet and the Hierarchy Navigator, the board outline, Board Statistics, Swap Layers, Page Settings, the drill/place origin.
- Conflicts: none beyond `edaTestHook.ts` (a hunk). Can run beside anything.

### P21. The simulator (item 34). Size XL. Rank 21. Last.
- `kicad-cli sch export netlist --format spice` and `ngspice -b` through the studio's kicad-cli lane, the raw output read back; analysis tabs, probes (`Probe Schematic`), the plot with its zoom history and axes, OP voltages and currents on the schematic,
  workbook files, exports (CSV, PNG, clipboard, plot to schematic).
- Files: a new `crates/sim/` (netlist through kicad-cli, the ngspice runner, the raw reader), `web/studio/src/components/simulator/*`, `actions/simActions.ts`.
- KiCad: `eeschema/sim/`, `eeschema/tools/simulator_control.cpp`, `eeschema/dialogs/dialog_sim_*`.
- Conflicts: `runner` and `schview` hunks (probes), P10.

### P22. Other EDA formats in (item 28; the two rows `STALE-REASONS.md` called out of scope). Size M. **Done 2026-10-15 on `task-import-foreign` (not merged); item 28 lists what is left.**
- Done: Import Non-KiCad Board File and Non-KiCad Schematic File through `kicad-cli pcb import` and `sch import` (`crates/kicad-engine/src/foreign.rs`, `crates/cli/src/{foreign_api,import_foreign,import_kicad}.rs`, `Cmd::ReplaceBoard` / `Cmd::ReplaceSchematic` in `crates/ops/src/import_foreign.rs`,
  `components/ForeignImportDialogs.tsx`, `actions/importActions.ts`, `kicad-port/foreignImport.ts`), the layer dialog, Import Issues, the project's design rules and net classes, `eda import-kicad` for the files of other formats. The rows are out of `ui-parity-missing.json`.
- Left: item 28's "Left, other formats". Import Netlist (P8c) is not part of it.
- KiCad: `pcbnew/files.cpp`, `pcbnew/dialogs/dialog_map_layers.cpp`, `pcbnew/pcb_io/pcb_io_mgr.cpp`, `eeschema/files-io.cpp`.
- Conflicts: `crates/ops/src/lib.rs` (two `Cmd` variants), `crates/cli/src/{board,studio}.rs` (the model file and a route), `web/studio/src/App.tsx`.

### P24. The multichannel tool (items 24 and 32). Size L. **Done 2026-10-21 on `task-multichannel` (not merged); items 24 and 32 list what is left.**
- Done, in the order of the plan, one commit each: (1) the placement rule area (`Zone::placement`, the reader and the writer, Rule Area Properties' Placement section, the painter's label, the Properties panel's group); (2) Generate Placement Rule Areas (the rows from the sheets, classes and groups, the dialog, the area made as KiCad makes it); (3) Repeat Layout (the match by topology, the table of targets
  with KiCad's reasons, the dialog, the copy of the placement, the routing, the zones and the other items); (4) Apply Design Block Layout (the block's layout on a linked group). The three rows are out of `ui-parity-missing.json`; the audit is pcbnew 312 of 350 handled. `PARITY-pcb.md` section 40 has the behaviour table and the tests; the browser check is `web/studio/e2e/multichannel.check.js`.
- Left: item 24's "Left of the multichannel tool".
- KiCad: `pcbnew/tools/multichannel_tool.{h,cpp}`, `pcbnew/connectivity/topo_match.{h,cpp}`, `pcbnew/dialogs/dialog_multichannel_{generate_rule_areas,repeat_layout}.cpp`, `pcbnew/dialogs/dialog_rule_area_properties.cpp`, `pcbnew/zone.cpp`, `pcbnew/tools/pcb_control.cpp` (`ApplyDesignBlockLayout`), `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp`.
- Conflicts: `crates/ops/src/lib.rs` (a `Cmd` variant and `EditZone`'s new field), `crates/model/src/ir.rs` (`Zone::placement`), `crates/cli/src/{studio,board,main}.rs` (three routes, a module, a test module), `web/studio/src/{App.tsx,actions/pcbEditSweep.ts,actions/designBlockActions.ts,components/ZoneDialog.tsx,components/canvas/painter.ts,kicad-port/pcbProperties.ts}`.

### 39. Public C/C++ libraries called instead of ported (the port-vs-reuse audit's rows 3, 5, 6, 7)
**Done 2026-10-28 on `task-reuse-cpp`.** `docs/ARCHITECTURE.md`, "Reuse before porting" and "INACTIVE, not deleted".
- **Clipper2**: KiCad's vendored 2.0.1 C++ behind `eda_clipper2`'s API (`crates/clipper2/src/cpp.rs`). The port (`src/port/`, feature `rust-port`)
  follows an older Clipper2 than 2.0.1: on KiCad's QA board polygons its boolean operations are identical to 2.0.1's, but `ClipperOffset` differs
  (1.x hole list, a zero-area path offset to nothing, `DoBevel` without z) in about 9% of cases and `RectClip64` in 3%
  (`crates/clipper2/tests/differential.rs` prints the table). The audit said the port lacked the Z callback: it had it; no caller sets one.
- **delaunator** crate for the ratsnest; same triangles as the port except at co-circular ties (same MST length); the port's `hull` was wrong in
  places (nothing read it).
- **nanosvg**: KiCad's (25ec2335) for the SVG import. Left: `text`/`tspan` draw nothing (KiCad draws them through Fontconfig, FreeType and
  HarfBuzz, which this build stubs out, `crates/kicad/cpp/nofont`); only KiCad's basic colour keywords are colours (KiCad builds it without
  `NANOSVG_ALL_COLOR_KEYWORDS`: "lime" is grey there too); coordinates are `float` (a few nm off, as in KiCad). Gained: `<style>` sheets of class
  selectors, KiCad's default stroke width and aspect rules.
- **Expression language**: one copy, Rust. Find by Properties' Query posts to `/api/pcb/query`; `pcbExpr.ts`/`libeval.ts` INACTIVE.
  `memberOfSheet('/')` now answers as KiCad (the studio sends no sheet path: every footprint is on the root sheet). Left: the geometry functions
  (`intersectsCourtyard`, `intersectsArea`, `enclosedByArea`) still say false in the Query tab (the request carries no shapes; the rule engine's
  own items have them).
- Not done: the SPICE value parser (`crates/sim/src/value.rs` and `web/studio/src/kicad-port/sim/spiceValue.ts`): the TS copy formats every plot
  tick, cursor readout and typed value in the simulator as you type and pan; a round trip per label is not the same simple change.
