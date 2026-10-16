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
  takes KiCad's nine shape choices, a trapezoid's delta, a chamfer's ratio and corners and a custom pad's list of basic shapes (add, edit, delete: one undoable verb). Zone-fill parity (`crates/zone-filler/PARITY.md`): `stonehenge`
  GND 11.33% to 0.15% of the area, `issue5093`'s `/right/GND` 776.56% to 1.49%; 198 of the 208 fills of the 17 QA boards are within 1% of kicad-cli's area and 189 within 0.5%. Left: Explode and Recombine, padstacks (a pad has one shape for all layers),
  a custom pad's proxy spokes, and the instant 3D scene (`eda_drc::pad_shape::pad_outline` and the state's `pads[].outline` are there for it). Measured: 40 new Rust tests (five of them slow-tier, with kicad-cli) and 25 new
  `node --test` cases; `web/studio/e2e/pad-shapes.check.js` runs 6 scenarios through `window.__eda` on a served board.
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
  entries character for character and exports the same VRML: slow-tier proof); the Footprint Chooser has the 3D view; Migrate 3D Models works (`pcbnew.GlobalEdit.migrate3DModels` leaves `ui-parity-missing.json`). Left: the Extruded 3D Body, embedded files and Collect and Embed 3D Models, the
  load-time auto-migration, "Show 3D viewer in own window" in the Footprint Chooser. Measured: 52 new Rust tests (one slow-tier) and 20 `node --test` cases; `web/studio/e2e/fp-models.check.js` runs 11 scenarios (153 checks) through `window.__eda`.
- **Four findings change the picture.**
  1. *A wired action is not a working feature.* This was the finding of 2026-10-07 and it was fixed where it bit: the schematic edit tools act on
     every item kind (item 1, `2a8bf8f`) and so do the PCB ones (item 8, `88c8c0a`). Before closing any action, run it on every item kind.
  2. *The KiCad files we write drop design data, and kicad-cli judges those files.* The writer keeps zone settings, keepouts, dimensions, groups,
     locks, arcs, teardrops (`b145f0e`), the board outline as Edge.Cuts items (`a46fd29`) and the footprint's own graphics: part outline, pin-1 mark, fab body
     and texts, on the canvas, in the file and in the silkscreen kicad-cli plots (item 25, `task-fp-graphics`). **What it still drops: the mask-only pads, barcodes, component classes and tuning profiles.**
  3. *One root sits under several gaps: the constraint model built from the intent has no edit verbs.* It now has an overlay for the rules
     (`drawings.rules`), for the nets (`design.nets`), for the libraries, for footprint fields, attributes and pad edits
     (`drawings.footprint_edits`), for the zone filler's pad and footprint settings (`drawings.zone_overrides`) and for footprints that exist only on
     the board (`drawings.board_parts`), and since 2026-10-12 for which footprint a part uses (`drawings.footprint_assignments`) and which reference a part
     has (`drawings.renamed_parts`): the schematic's Footprint field reaches the board, Change Footprint exists, and a reference rename carries the board's
     footprint, its pose and its routing (item 26).
  4. *Most earlier parity work was typechecked and unit-tested and never clicked.* Six areas now have a browser check driven through
     `window.__eda` (`web/studio/e2e/`: Appearance, DRC and ERC review, footprint edit and arrays, Properties panel, snap and grid, the 3D viewer);
     the schematic editors, Board Setup, the choosers, the router, the PCB edit tools, the zones and the hierarchy have none (item 33).
- **Docs.** Action level: `UI-ACTIONS.md`, with `STALE-REASONS.md` beside it. Behaviour tables: `web/studio/PARITY-{pcb,sch,3d,fpedit,symedit,boardctl,common}.md`.
  Code level: `CODE-COMPARE-ui.md` (the first 266 handlers; about 310 added since have not been compared) and `CODE-COMPARE-router.md`,
  `crates/pns/PARITY.md`, `crates/zone-filler/PARITY.md`. Measured round trip and connectivity: `REPORT.md` (the re-export part re-measured 2026-10-11, after P1).
  Engines: `ARCHITECTURE.md`. The "Missing" lists below were re-read against the code where a merge touched them; a bullet no merge touched is
  carried forward from 2026-10-07 and says so.

## What moved since 2026-10-07

| item | what | state | merge |
|---|---|---|---|
| 1 | Schematic item editing: Move, Drag, Rotate, Mirror, Align, Properties for every kind | **Closed** | `2a8bf8f` (10-08) |
| 2 | KiCad files keep zone settings, keepouts, arcs, groups; the outline as Edge.Cuts items | Mostly done | `b145f0e` (10-08), `a46fd29` (10-09) |
| 3 | Board Setup and the rules edit through an overlay | Mostly done | `b145f0e` (10-08) |
| 4 | Module sheets: editing on every sheet, nets traced across sheets | Partly (the rest in flight) | `08c70c8` (10-08) |
| 5 | Symbol Chooser and Footprint Chooser over the installed libraries; library tables, Manage Libraries, New and Add Library, Configure Paths | Mostly done (saving into a table library is left) | `80e972d` (10-08), `task-lib-tables` (10-10) |
| 6 | Schematic clipboard | **Closed** | `ee20bab` (10-08) |
| 7 | Router: shove near pads, the board edge, settings, via drag | Partly | `1e4d7ea`, `03b46de` (10-08) |
| 8 | PCB edit tools and the clipboard for every item kind | Mostly done | `88c8c0a` (10-08) |
| 9 | Footprints on the board: fields, attributes, pads | Mostly done | `afcb91b` (10-09) |
| 10 | The Properties panel is KiCad's property grid | Mostly done | `bd712d3` (10-08) |
| 11 | DRC and ERC review workflow | Mostly closed | `02f383c` (10-08) |
| 12 | Schematic fields, labels, Autoplace Fields, De Morgan body styles | **Done** | `2d92851` (10-09) |
| 13 | Zone fill: spokes, hatch, smoothing, iterative refill; pad and footprint overrides | Partly | `eabaf91` (10-09) |
| 14 | Snap, grid and click versus drag | **Closed** (board, schematic) | `358606b` (10-09) |
| 16 | PCB context menu | Mostly done | `4c041a6` (10-09) |
| 17 | Groups, nested | Mostly done (board) | `4c041a6` (10-09) |
| 18 | Appearance panel | Mostly done | `6611ade` (10-09) |
| 21 | Find on the PCB | Mostly done | `4c041a6` (10-09) |
| 22 | Create Array | Mostly done | `afcb91b` (10-09) |
| 23 | The 3D viewer: KiCad's models, layer tree, stackup colours | **Done** | `89ff669` (10-10) |
| 25, 2 | Footprint graphics: silkscreen, fab and assembly drawings and texts are the footprint's own (IR, `.kicad_mod` and `.kicad_pcb`, canvas, 3D, in-house checks) | **Done** (limits in item 25) | `task-fp-graphics` (10-10, not merged) |
| 30 | The 3D models of a footprint: the 3D Models tab in both Footprint Properties dialogs, the list's verbs and file entries, the Footprint Chooser's 3D view, Migrate 3D Models | **Done** (limits in item 30) | `task-fp-3d` (10-13, not merged) |

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
| 5 | P5 Router: drag, mid-segment, diff pairs, meanders | 7 | **P5a done on `task-router-drag` (2026-10-10)**: a segment slides, an illegal drag falls back, routes start and end mid-track, Route From Other End after the first fix. **P5b done on `task-diff-pairs` (2026-10-11)**: a pair is KiCad's, walked or shoved as one with its gap, with vias. **P5c done on `task-length-tuning` (2026-10-13)**: KiCad's meanders checked against its own code, its tuning tool for a track, a pair and a skew, tuning patterns as generator items written to the board file. Left: arcs as router items (`ARC_T`), mouse-trail posture, springback (`ShoveTimeLimit` is ported: a shove gives up after a second) | P5a, P5b and P5c done; the rest of item 7 open | M (what item 7 lists) |
| 6 | P6 PCB editing details | 8, 15, 17, 35 | Paste Special, rotation step, bounding boxes, a long tail of tool options | mostly done (merged 2026-10-10); items 8, 15, 17 and 35 list what is left | M, ongoing |
| 7 | P7 Custom-shape pads | 31, 13, 19 | custom pads import as their anchor; zone fill is wrong around them; trapezoid and chamfered pads drawn as rectangles | done 2026-10-10; item 31 lists what is left | L |
| 8 | P8 Files in: Append Board, DXF and SVG, drops | 28 | Import Netlist; a schematic or an image dropped on the sheet; the placement preview | mostly done | S |
| 9 | P9 Board Setup completion and the rule editor | 3 | **P9a done on `task-board-setup` (2026-10-12)**: Text Variables, Board Editor Layers, Formatting, Zone Hatch Offsets, Length-tuning Patterns, Tuning Profiles, regex net patterns and composite classes, drawing tools from the Text & Graphics defaults. Left: Component Classes, Embedded Files, the rule-tree designer (P9b) | P9a done, P9b open | L left (rule editor XL) |
| 10 | P10 Schematic details | 1, 12, 15, 17, 20 | no schematic groups, a user-defined field has no place on the sheet, no Pin Helpers menu (done on `task-sch-details`, 2026-10-12, not merged: pins as items with Swap Pins, sheet pins, Auto-rotate, a text's justification, the net-collision overlay, the fields table's attribute columns, a power symbol's Properties, Copy as Text, the schematic's pan buttons and auto-pan) | mostly done (not merged) | S-M |
| 11 | P11 Search and inspection | 21, 29 | **done 2026-10-13 on `task-search-inspect` (not merged)**: Find by Properties (the From Selection tab and the Query tab on KiCad's expression language), Find following user fields, hidden fields and zone names, Clearance Resolution and Constraints Resolution (a rule engine in `crates/drc`: the implicit rules, the `.kicad_dru`, `EvalRules` with KiCad's report text), the Footprint Checker (kicad-cli's DRC on a scratch board). Items 21 and 29 list what is left | done (not merged) | M-L |
| 12 | P12 Board outline leftovers | 27 | placement and the DSN boundary ignore cutouts; NPTH holes missing from the 3D slab | open | M |
| 13 | P13 Zone leftovers | 13 | non-copper zones, multi-layer zones, `connect_nearby_polys`, mask-only NPTH knockouts | open | L |
| 14 | P14 3D models of footprints | 30 | one model path in the editor; no model list, no preview, no Migrate 3D Models | done 2026-10-13 on `task-fp-3d` (not merged); item 30 lists what is left | M |
| 15 | P15 Review workflow leftovers | 11 | ERC exclusions never reach kicad-cli; no Delete Marker / Save report | open | S-M |
| 16 | P16 PCB item kinds the IR lacks | 24 | text box, tables (stackup table), barcode, point, reference image, hatch fill | deferred | L |
| 17 | P17 Library editor leftovers | 14, 19, 22 | snap to anchors in the editors, the pad array numbering, derived symbols, a fields grid in Footprint Properties, symbol fields on the canvas, alternate pin functions, the symbol text box, a shape properties dialog | **done 2026-10-16 on `task-lib-editors` (not merged)**; items 19 and 22 list what is left | L |
| 18 | P18 Design blocks | 32 | 23 actions; reusable blocks | after P3, P4 | L |
| 19 | P19 Microwave tools and Specctra | 9, 27 | the five microwave tools; DSN export and SES import | open | S-M |
| 20 | P20 Browser checks for the older areas | 33 | most earlier work was never clicked | standing | M each |
| 21 | P21 Simulator | 34 | `eeschema.Simulation.*`, `Simulator.*` | last | XL |

## Items

"Port from" paths are under the KiCad source root. Each item says what is done (with the merge), what is still missing, and its package.

### 1. Schematic edit tools act on every item kind
**Closed 2026-10-08** (`2a8bf8f`; fields and labels `2d92851`); the details it left are done on `task-sch-details` (2026-10-12, not merged: P10). Hit: every schematic session.
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
  clicked through on a copy of mcu30. No browser check script yet (item 33).
- Done 2026-10-12 (`task-sch-details`; `PARITY-sch.md` sections 21 to 26): a symbol's pins are items (hover, click, box, the Pins filter, the summary; the net highlight and Select Node start from one) with Swap Pins, Swap Pin Labels and Swap Unit Labels
  (`SchExtras::pin_swaps`, three `SchEdit` verbs, one undo step each); a sheet's pins are items one by one (moved along the border, Sheet Pin Properties, Delete, Alt+S swap; the hierarchy's nets follow); a global or hierarchical label with Auto rotates
  itself when it is placed, edited in Label Properties, moved and dragged (`AutoRotateItem`); a free text has a justification (`SchExtras::text_justify`) that is written, read back and flipped by Rotate and Mirror as `SCH_TEXT` flips it;
  the net-collision overlay of a Move or Drag (`SCH_DRAG_NET_COLLISION_MONITOR`: circles at the junctions the held items would make between two nets, marks where a connection is pulled apart, the warning cursor); the Symbol Fields Table's attribute
  columns (DNP, Exclude from BOM, board, simulation), Include 'DNP' / 'Exclude from BOM' symbols and KiCad's four built-in views; a power symbol's Symbol Properties (Reference, Value as the net, Show checks); Copy as Text.
- Measured on `task-sch-details`: 76 new Rust tests (ops 62: pins 13, sheet pins 8, labels and text 26, net collision 9, fields table 6; kicad 5, one slow with kicad-cli; model 4; cli 5) and about 50 new `node --test` cases; the kicad-cli proof
  (`crates/kicad/tests/sch_details.rs`, `EDA_SLOW_TESTS=1`: a file with swapped pins, a sheet pin moved to the top edge and justified texts loads, its ERC reports the sheet pin at the moved place, and `kicad-cli sch upgrade` writes the justification,
  the angle, the pin and the cached copy of the symbol back as we wrote them); `web/studio/e2e/sch-details.check.js` (nine scenarios through `window.__eda`, 72 checks, every scenario undoes itself).
- Missing: Pin Helpers (`PIN_TRICKS_MENU`: Wire, No Connect, Net Label, Hierarchical Label and Global Label on a selected pin or sheet pin); a sheet's border and fill; a user-defined field has no place on the sheet (`FieldPlacement::name` can name one in `SchematicSection::field_layout`, but the editor
  lists, draws and autoplaces only the standard fields and `user_fields` holds values only: package P10 step 8, not reached); a sheet pin's font, size, bold, italic and colour have no place in the IR; a power symbol's Properties has no footprint, datasheet, exclusion flags or
  pin table; Autoplace Fields does not read the swapped pin places and the reader does not turn a `lib_name` copy back into a swap (so a copy, paste or duplicate of a symbol with swapped pins has the library's pins); the Symbol Fields Table has no variants and no scope selector; symbols this project drew (not imported) and turned or mirrored place their pins by
  `eda_engine::placed` (older note, not re-checked since the overlap fix, `f24fa31`, which made generated symbols draw their pins as KiCad's do).
- Port from: `eeschema/tools/sch_move_tool.cpp`, `sch_edit_tool.cpp`, `sch_selection_tool.cpp`, `sch_drag_net_collision.cpp`, `sch_align_tool.cpp`.

### 2. The KiCad files we write drop design data
**Mostly done (2026-10-08, 10-09, 10-10).** Hit: any board with a pour, keepout or dimension; every export; every imported board. Blocks: no longer for keepouts. The footprint's own graphics (P1) were the biggest of what was left and are done.
- Done 2026-10-13 (`crates/kicad/src/fp_transform.rs`): boards in KiCad 10's format (file version 20260616 on, `FIRST_FP_AFFINE_TRANSFORM`) are read. A footprint's `(transform (translate x y) (rotate a) (scale sx sy))` stands for the `(at x y a)` older files have, and a footprint's own zones, stored in its library frame, are put back in the board frame (`TRANSFORM_TRS::Apply`). Pads and texts kept their meaning. Proof: four QA boards upgraded by kicad-cli 10.99.0-5334 read the same as the files they came from (`crates/kicad/tests/kicad10_transform.rs`, slow). A footprint scale other than 1:1 has no place in the model: it is read at 1:1 and named in `ImportNotes::footprints_scaled`. Not read yet from the same format: bold text as a stroke-width multiplier (20260826), line ending shapes (20260818), grid items (20260728), via stitching and guarding (20260816), microvia stack generators (20260830), custom user properties (20260831), drill charts and maps (20260901).
- Done (`b145f0e`; `crates/kicad/src/{pcb,pcb_items,import_items,import}.rs`): a rule area is written as a keepout zone (before, it failed the whole export with
  `kicad.unknown_net`); every zone writes its own clearance, minimum width, priority, pad connection, thermal gap and spoke, hatch, island and fill settings;
  arc tracks are arcs; locks, dimensions, groups, zone names and teardrop flags are written and read back (round-trip tests in `pcb.rs`). **The board outline
  (`a46fd29`, 10-09)**: Edge.Cuts come back as the items they were (`gr_arc`, `gr_circle`, `gr_rect`, `gr_poly`, `gr_curve`, `gr_line`), a malformed outline stays
  malformed in the export, a footprint's own Edge.Cuts become board-level shapes, KiCad 6's `(gr_arc (start CENTRE) (end START) (angle A))` is read.
- Done (`task-fp-graphics`, 10-10; item 25): **a footprint's graphics and texts** (`fp_line`, `fp_circle`, `fp_text` ...: 512 `fp_line`s on `complex_hierarchy`) are the footprint's own and come back out, and so does a pad's
  silkscreen layer: `silk_over_copper` 104 -> 104 (was 2), `silk_overlap` 199 -> 199 (was 5), `silk_edge_clearance` 14 -> 14 (was 2).
- Measured (`REPORT.md`, `scores.json`, the re-export part on 2026-10-11, kicad-cli 10.99 on `qa/data` of the KiCad sources): the re-export of a real board gives kicad-cli the same
  violation counts as the original on **7 of 12** boards (58.3%), up from 6 (10-08) and 4; all 12 re-exports load.
- Missing: a footprint that had no library is written as `eda:<name>`, so a project that turns `lib_footprint_issues` on gets a violation for a library that does not exist (72 on `complex_hierarchy`); barcodes,
  component classes and tuning profiles are not read (the `api_kitchen_sink`, `component_classes_drc` and `drc_missing_tuning_profile` differences); 189 non-rectangular pad shapes of the QA corpus are
  approximated as rectangles (item 31); `complex_hierarchy` still differs in `clearance` (112 -> 117), `copper_edge_clearance` (14 -> 15), `solder_mask_bridge` (112 -> 115) and `shorting_items` (0 -> 2), not analysed.
  `scores.json` was last measured before the outline and footprint work (10-08) except for the re-export part.
- Port from: `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` and `pcb_io_kicad_sexpr_parser.cpp`.

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
- Missing: the **Component Classes** and **Embedded Files** pages (nothing in the model holds them; the `component_classes_drc` difference of the QA corpus stays); the rule-tree designer
  (`DRETool.drcRuleEditor`, 82 files in `pcbnew/drc/rule_editor`; the `.kicad_dru` is edited as text -- P9b); bus notation in net patterns (`D[0..7]`) and the net class labels of the schematic; the
  transmission-line calculator of the Tuning Profiles grid; the tuner's corner style and single-sided settings (kept and written, not used by this app's meander generator); the Net Classes grid's own
  Tuning Profile column (the class's profile is chosen on the Tuning Profiles page); zone-level hatch offsets (the board's per-layer offset is what the filler reads; KiCad itself loses the board's
  when it saves, see section 9b); Formatting's dimension and barcode boxes (library footprints here hold neither) and, for a footprint the model already knows, the art of its library (only the copy
  of a footprint the board keeps is restyled); in Text Variables: `@{}` math, `VCSHASH`, schematic labels, text boxes and power symbol fields, `PIN_NAME(n)`, library footprint texts and text pasted
  from the clipboard; dielectric sub-layers and impedance control on the stackup.
- Port from: `pcbnew/dialogs/dialog_board_setup.cpp` and `panel_setup_*.cpp`, `common/dialogs/panel_setup_netclasses.cpp`, `panel_setup_severities.cpp`,
  `panel_text_variables.cpp`, `pcbnew/drc/drc_rule_parser.cpp`, `pcbnew/tools/drc_rule_editor_tool.cpp`.

### 4. Hierarchical sheets
**Mostly done (2026-10-10, `task-sch-hierarchy`)**, limits below. Hit: every multi-sheet design. Package P4 for what is left.
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
- Left: a screen two sheet symbols show is traced once (KiCad makes a net per placement) and a pasted sheet is a copy, never a shared screen; buses across sheets are traced as plain wires;
  Find does not match a sheet's name, file or pins; Clear Annotation (a design names a part by its reference, so two cannot be `R?`); "Place as group" and a ghost of the contents while they are carried;
  a file dropped on the sheet (`DdAppendFile`, a browser has no path for it); the attribute, library-link and Update / Change Symbol tools act on the sheet in view; a footprint already placed keeps
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
**Partial.** Shove near pads, vias and tracks, the board edge, every setting and the via drag are done (2026-10-08, `1e4d7ea`, `03b46de`). **Dragging a segment, a route that starts or ends mid-segment and Route From Other End after the first fix are done too (P5a, 2026-10-10, branch `task-router-drag`: `68eb737`, `c1b409c`, `bae172b`, `0a9290e`, `9dbcd8e`, `77e526e`, `bc573db`). A differential pair is KiCad's too: its gateways, Highlight collisions / Walk around / Shove with the two lines held at the pair's gap, `V` for a pair of vias, the pair optimizer (P5b, 2026-10-11, branch `task-diff-pairs`: `aa48f28`, `8df6fba`, `04c753d`, `cdc464b`, `3de42d5`, `4666af7`, `880ad08`, `708d29c`, `d9c5fe7`).** **Length tuning is KiCad's too: its meanders, the tuning pattern tool for a track, a pair and a skew, and the pattern as a generator item (P5c, 2026-10-13, branch `task-length-tuning`: `0c3036f`, `efd5d28`, `dd35b60`, `d31692f`, `8d9d4b8`, `c7ab8a1`, `4ee2939`, `fce168d`, `65129b4`, `8bbd7b2`, `db9963a`, `543fa0d`).** Hit: arcs are chords in the router's world, and mouse-trail posture and springback are not ported (`ShoveTimeLimit` is: a shove gives up after a second, as KiCad's, 2026-10-13). Blocks: no. Package P5.
- Exists: `crates/pns` ports hulls, `LINE::Walkaround`, shove of tracks and vias (a stack of lines with ranks, `onCollidingSolid`, `pushOrShoveVia`, the optimizer over every shoved line),
  the optimizer with smart pads, loop removal, **the differential pair as `DIFF_PAIR_PLACER` (`diff_pair.rs`, `dp_placer.rs`: the gateways of `pns_diff_pair.cpp`, the pair started from a pad pair, a via pair or a pair of track ends and meeting the coupled
  pair under the cursor, walked round what is in its way as one unit and shoved with its lines held at the pair's gap, `V` for a pair of vias with the route going on on the other layer, `/` posture, Backspace, one undo step, the pair optimizer `mergeDpSegments`;
  kicad-cli's DRC finds no clearance, track width or diff-pair gap violation a placed pair adds)**, **length and skew tuning as KiCad's tool (`7`/`8`/`9`: `meander.rs`, `meander_placer.rs`, `dp_meander_placer.rs`, `tuning_path.rs`, `tuning_commit.rs`, checked against KiCad's own meander code on 100 000 random cases; the status box, `1`-`4`, Ctrl+L, one undo step; a tuning pattern is a generator item, `(generated ..)` in the file, selected, tuned again, removed, dragged by its edit points, listed in the Generators Manager; kicad-cli finds the tuned nets within the length and skew rules they were tuned to)**, `D`/`G` drag of a segment (`DM_SEGMENT`: it slides, its neighbours follow at 45 degrees),
  a corner or a via, with the last valid position restored when the cursor is where the drag cannot be resolved; routes that start and end in the middle of a track (`SplitAdjacentSegments`) and Route From Other End
  before and after the first fix (`ContinueFromEnd`); the clearance gate measures
  pads by their exact outline (`eda_drc::board::placed_pad_copper`); the board outline with its cutouts, arcs and a footprint's own Edge.Cuts are obstacles with the copper-to-edge clearance
  (`PNS_KICAD_IFACE_BASE::syncGraphicalItem`, `crates/pns/src/from_ir.rs`); every `RoutingSettings` field is read and Interactive Router Settings has `dialog_pns_settings.cpp`'s rows
  (`crates/pns/PARITY.md`; `CODE-COMPARE-router.md` D1-D6, D8, D10, D12, D15, D16 fixed).
- Missing (`CODE-COMPARE-router.md` D18): arcs as router items (`ARC_T`): an arc track is its 32 chords in the world (`from_ir.rs`, since P5c, right to 0.01 % in length and as an obstacle), so a route is not split inside an
  arc (`SplitAdjacentArcs`), `DM_ARC` is not a drag, a shove or a drag turns an arc into a polyline, a track that is an arc cannot be picked to start a tuning pattern on and a pattern across an arc is tuned along its chords;
  mouse-trail posture (`pns_mouse_trail_tracer.cpp`: the dialog's "Use mouse path to set track posture" and "Optimize entire track being dragged" rows are disabled; the second needs the optimizer's
  `KEEP_TOPOLOGY` and `PRESERVE_VERTEX`) or springback; a single track's head that ends in a via is not shoved with its via (a pair's heads are: `Shove::run_heads`); `findRedundantSegment` and locked joints (`from_ir` never sets `Segment.locked`: a locked
  track is dragged and shoved all the same). **Left of the tuner (P5c): the time domain (a target in picoseconds, the delays of a tuning profile) and the pad-to-die length (the model has neither), the message panel's lines of a pattern (the Generators Manager has its row), the merged
  outline of a pair's pattern (two capsules are drawn), a pattern moved, turned or flipped like KiCad lets one (here a pattern and its tracks are as good as locked for those: they are changed through its properties and edit points), the router's world cloned once at the start of a tuning where
  KiCad branches at every move, and a line that stops where the track width changes where KiCad rewrites the whole of it.**
  Browser check of P5a done (2026-10-10, `window.__eda` on the 30-part board: segment drag, 30-degree segment, corner dropped on a pad, route started and ended mid-track, Route From Other End after a fix: item 33 for the rest).
  **Left of the pair (P5b): Route From Other End (`ROUTER::ContinueFromEnd` restarts the placer of the same mode, so KiCad does it for a pair; here only a single track), springback (every move of a pair resolves from the world the last fix left), a pair that fails to walk shows
  the pair as drawn and flags it (KiCad's `tryWalkDp` empties it: `bestScore > 0.0` is always true), two walk attempts where KiCad makes four (it repeats two), Backspace is an extension (`DIFF_PAIR_PLACER` has no `UnfixRoute`); a drag of a pair needs nothing (KiCad's `DRAGGER`
  has no pair mode: `m_draggedLine` is one `LINE`).** Browser check of P5b done (2026-10-11, `web/studio/e2e/diff-pairs.check.js` on a scratch board built from `examples/through_hole_headers.yaml`: starting from either pad, the three modes, a pair routed, finished on the other header and undone in one step,
  the pair of vias, `/`, Backspace, Escape). `docs/parity/CODE-COMPARE-router.md` section I and D18, `crates/pns/PARITY.md` Stage 7.
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
- Missing: per-footprint mask and paste margin overrides; (Change Footprint(s), Update Footprints from Library that reads the library and geographical reannotate are done, item 26;) padstacks (item 31; trapezoid,
  chamfered and custom pads are done, 2026-10-10), fabrication property, pad number and pin function edits, pad-to-die length; Datasheet and Description fields and text variables in field text; our own placement and courtyard
  checks do not read a pad's offset or margins (kicad-cli does); logos; the microwave tools (their reason, "no free footprint", is stale: `STALE-REASONS.md` SR3).
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
**Mostly closed (2026-10-08, `02f383c`).** Hit: every review pass. Blocks: no. Package P15.
- Done: Run DRC and Run ERC through kicad-cli with a running state, out-of-date marking and lint tabs; the list is `RC_TREE_MODEL`'s; DRC exclusions as an undoable verb
  (`Cmd::AddDrcExclusions` / `DeleteDrcExclusions`, `design.drawings.drc_exclusions`) written into the derived `.kicad_pro`; Next, Previous and Exclude Marker; a marker menu in both dialogs and on both
  canvases; the Ignored Tests and Schematic Parity pages (`--schematic-parity`); a Violation Severity page in Schematic Setup. `web/studio/e2e/drc-review.check.js` drives it. Details:
  `PARITY-pcb.md` section 9a, `PARITY-sch.md` sections 4 and 9, `PARITY-common.md` section 5.
- Left: **a DRC exclusion reaches kicad-cli only where the report can say where the marker is.** KiCad matches an exclusion to a marker by its exact text, position included, and kicad-cli's report gives
  the items' positions and never the marker's, so the studio writes the positions worth trying (`eda_kicad::drc_exclusions`) and waives the rest in the studio's report alone (`kicad_matched: false`). A
  KiCad patch adding the marker position to `RC_JSON::VIOLATION` would close it; that is upstream work and not planned. **ERC exclusions are the studio's alone**: the derived project has no
  `erc.erc_exclusions`, though their serialization (`SCH_MARKER::SerializeToString`: key, position, the two item uuids, sheet paths) is within reach -- the exporter mints the uuids, so the DRC method
  of guessing the position applies -- and they have no comment. Not ported: selecting a marker with a left click, Delete Marker / Delete All Markers and Save report, "Exclude all violations of rule
  '...'", the Inspect / Fix menu entries, Edit connection grid spacing (Schematic Setup has no Formatting page).
- Port from: `pcbnew/dialogs/dialog_drc.cpp`, `eeschema/dialogs/dialog_erc.cpp`, `eeschema/sch_marker.cpp`, `eeschema/schematic.cpp` (`ResolveERCExclusions`), `eeschema/erc/erc_settings.cpp`,
  `common/rc_item.cpp`, `common/dialogs/panel_setup_severities.cpp`.

### 12. Schematic symbols, fields and labels carry per-instance geometry
**Done (2026-10-09, `2d92851`)**, limits below; a free text's justification and pin selection added 2026-10-12 (`task-sch-details`). Hit: every schematic cleanup. Blocks: no. Package P10 for the limits.
- Done (`PARITY-sch.md` sections 1, 15): the Reference, Value, Footprint and Datasheet of every symbol, the Value of a power symbol and the name and file of a sheet each have a position, orientation,
  justification and visibility of their own (`SchematicSection::field_layout`, in the item's own frame), placed by a port of Autoplace Fields (`eeschema/autoplace_fields.cpp`, `crates/engine/src/fields.rs`)
  and written to the `.kicad_sch`. Fields are items: picked, moved (`M`, a drag), turned, mirrored, hidden with Delete and edited in Field Properties; `O` runs Autoplace Fields; a turn re-autoplaces
  (`SchExtras::fields_autoplaced`). A label's spin, size, bold and italic are stored and edited. A placed symbol in the alternate ("De Morgan") body style is drawn, written and read back (`LibSymbol::alternate`,
  `SchExtras::body_styles`, Cycle Body Style). 44 Rust tests, 17 `node --test` cases.
- Done 2026-10-12 (`PARITY-sch.md` sections 21 and 23): a free text's justification (`SchExtras::text_justify`: stored when it is not left and bottom, written as `(justify ..)`, read back, flipped by Rotate and Mirror, carried by copy and paste); a label's Auto
  (`LabelLook::auto_rotate`); a symbol's pins are items, and Swap Pins keeps each pin's place in `SchExtras::pin_swaps`.
- Missing: a field's font face and colour, a label's own fields (netclass, intersheet references) and a user-defined field's place on the sheet have no place in the IR; the
  fields of a symbol read from a KiCad file are not items; `m_AutoplaceFields.enable` is always on and Autoplace Fields ignores swapped pins; Reorganize into Module Sheets (2026-10-10) keeps the fields' places and looks, the labels' looks and the body styles, but not a
  label's spin; only the normal and the De Morgan body style are modelled.
- Port from: `eeschema/sch_field.cpp`, `sch_label.cpp`, `dialogs/dialog_field_properties.cpp`, `dialog_label_properties.cpp`, `dialog_symbol_properties.cpp`.

### 13. Zones: fill fidelity and settings
**Partial; fill fidelity improved 2026-10-08/09 (`eabaf91`, `a46fd29`).** Hit: most boards. Blocks: no. Package P13 (P7, the custom pads, merged 2026-10-10). Of 208 fills on 17 KiCad QA boards, 198 are within 1% of kicad-cli's area (105 before 2026-10-08, 190 before the custom pads), 189 within 0.5%; the notch row of `issue11814` went from 23.4% to 0.2% with the outline work.
- Exists: the filler (`crates/zone-filler`, `crates/drc/src/fill.rs`, `GET /api/fill`) follows `ZONE_FILLER::Fill`: zones fill from the highest priority down and are knocked out by each other's fills, islands go by
  connectivity, iterative refill, thermal spokes (`buildThermalSpokes`: turn with the pad, the pad's angle, width and gap, only the ones that reach copper stay), hatch fill with thermal rings, chamfer and
  fillet corner smoothing, the board-edge clearance from every Edge.Cuts item, and pad and footprint clearance overrides. A pad's or footprint's own connection, relief gap, spoke width, spoke angle and
  clearance beat the zone's (Footprint Properties and Pad Properties; read from and written to `.kicad_pcb` and `.kicad_mod`). The full settings dialog (`components/ZoneDialog.tsx`), keepout knockouts,
  Fill and Unfill Selected, Merge, Duplicate onto Layer, the Priority actions and the Zone Manager (`PARITY-boardctl.md`). Measured in `crates/zone-filler/PARITY.md`.
- Missing: a custom pad's proxy spokes (`gr_vector`; its spokes start from the box of its copper) and padstacks whose inner and back shapes differ (item 31; custom pads themselves were done on 2026-10-10: `stonehenge` GND 11.33% to 0.15%, `issue5093` `/right/GND` 776.56% to 1.49%); the thermal
  rings of a hatched zone (0.6 to 0.9% XOR); `connect_nearby_polys`; mask-only NPTH holes (`issue14559`'s plugs), courtyards and net ties as knockouts; one layer per zone and no non-copper zones
  (`fillNonCopperZone`); no zone lock in Zone Properties (the name is edited in the Properties panel); no Auto-Assign Priorities; the Zone Manager has no preview; a hatched zone with a fine pitch over a whole
  board is slow (minutes in a debug build on `issue5093`). Appendix C has the function table.
- Port from: `pcbnew/zone_filler.cpp`, `zone.cpp`, `zone_manager/`, `dialogs/panel_zone_properties.cpp`, `dialogs/dialog_non_copper_zones_properties.cpp`, `pcbnew/teardrop/`.

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
**Mostly done on the board (2026-10-08/09, `4c041a6`); the schematic and the footprint editor have none.** Hit: sometimes. Blocks: no. Package P6 (board) and P10 (schematic).
- Done: Group, Ungroup, whole-group selection, enter and leave, Add Items, Remove Items and Group Properties (`Cmd::EditGroup`); groups nest (`EDA_GROUP`; `crates/model/src/groups.rs`,
  `crates/ops/src/pcb_groups.rs`); move, rotate, flip, Delete, Duplicate, Copy and Paste take the whole tree; the entered group is drawn with its box and name; the `.kicad_pcb` file and the clipboard hold the tree.
- Done 2026-10-10 (`task-pcb-edit-details`): items drawn, pasted, duplicated or arrayed while a group is entered join it (`BOARD_COMMIT::Push`; `Cmd::InGroup`, `crates/ops/src/pcb_groups.rs`; `PARITY-pcb.md` section 16).
- Missing: what the router commits is not made in the entered group (package P5); Create Array skips groups (item 22); no groups in the schematic (P10's last step, `sch_group_tool.cpp`: not reached on `task-sch-details`) or the footprint editor.
- Port from: `common/tool/group_tool.cpp`, `pcbnew/tools/pcb_group_tool.cpp`, `eeschema/tools/sch_group_tool.cpp` (the last one is open).

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
- Missing: Explode and Recombine of a custom pad (item 31); a placed symbol gets a library text box as its border and fill only, not its text (the engine's `SymbolGraphic` has no text box), and has no alternate pin functions (`LibPin` has none); the border's colour and
  style, a custom fill colour and Private to Symbol Editor of a library shape and text box; a plain text of a symbol and the Footprint Editor's graphics and texts have no Properties dialog; the fields table has no simulation fields; no named body styles. (The pad clearance and thermal overrides are read by the zone filler since 10-08; the note that no check reads them is gone.)
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
- Missing: raytracing (out of scope); layer presets and viewports of the 3D viewer; the PCB editor's selection drawn green in the 3D view; the 3D Navigator; the Value and user fields (a footprint's own texts and the Reference are drawn since P1), off-board silkscreen, models not
  in the position file and DNP models; X3D and IGES models; per-layer ambient and specular colours of the board; the preferences that set the animation speed; NPTH holes are not cut out of the slab
  (`aIncludeNPTHAsOutlines`, item 27); the Extruded 3D Body of a footprint (item 30).

### 24. Item kinds the IR does not have
**Open, deferred.** Backlog; package P16 for the PCB items.
- Tables (PCB and schematic, 18 rows of `ui-parity-missing.json`), PCB text boxes, barcodes, points, reference images, hatch fills, schematic images, the symbol text box, derived symbols, alternate pin
  functions, design variants, generators, padstacks, component classes, tuning profiles, embedded files (blocked by the no-new-dependency rule: KiCad uses zstd), each with its reason in
  `ui-parity-missing.json`. Add one when a work package needs it. Custom-shape pads left this list: they are item 31.

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
**Done (2026-10-12, `task-fp-assign`, not merged yet)**, limits below. Hit: every project that changes a footprint or renumbers a part. Blocks: no longer. Package P2, size L.
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
- Left: Swap Pad Nets (`EDIT_TOOL::SwapPadNets`) was not reached. In this IR a pad's net is its pin's place in `design.nets`, so it is a verb that rotates the nets of the selected pads by moving their pins between nets, with the tracks that end on the pads (`Track::pins`) taking the new net; it makes the board disagree with the schematic, as in KiCad (kicad-cli's `--schematic-parity` reports it), until Update Schematic from PCB carries a pin swap back, and the next schematic edit that changes connectivity (`Cmd::edits_connectivity`) reads `design.nets` from the drawing again, so a swap kept only there would be undone by it. Swap Gate Nets also needs the footprint's unit info (`FOOTPRINT::GetUnitInfo`: the `(units ..)` of a multi-gate footprint), which the IR does not hold. Pad nets of an exchange are kept by pad number only (KiCad also matches pads that share no number by position). `FootprintNeedsUpdate` compares what the model keeps of a library footprint,
  so the file's attribute flags, net-tie groups, rule areas and stackup layers are not reported. Insert footprint into PCB places the footprint at the centre of the board instead of carrying it on the cursor. Import
  Netlist (a part from a netlist has no schematic symbol).
- Port from: `pcbnew/dialogs/dialog_exchange_footprints.cpp`, `pcbnew/pcb_edit_frame.cpp` (`ExchangeFootprint`), `pcbnew/netlist_reader/board_netlist_updater.cpp` (`replaceFootprint`,
  `updateFootprintParameters`, `updateComponentPadConnections`), `pcbnew/tools/global_edit_tool.cpp` (`ExchangeFootprints`), `pcbnew/tools/board_reannotate_tool.cpp`,
  `pcbnew/dialogs/dialog_board_reannotate.cpp`, `pcbnew/footprint.cpp` and `pcbnew/drc/drc_test_provider_library_parity.cpp` (`FootprintNeedsUpdate`), `pcbnew/tools/board_inspection_tool.cpp` (`DiffFootprint`),
  `eeschema/tools/assign_footprints.cpp`, `eeschema/tools/backannotate.cpp`, `pcbnew/tools/edit_tool_move_fct.cpp` (`SwapPadNets`).

### 27. Board outline leftovers
**New, open.** Hit: boards with cutouts, slots or rounded corners. Blocks: no. Package P12, size M. From `PARITY-pcb.md` section 25 ("Not done") and `crates/zone-filler/PARITY.md`.
- The autoplacer and the placement verbs (`crates/place`, `Cmd::PlaceAt` and the rest) still work from the outline's bounding box: a part can be placed inside a cutout's box.
- The Freerouting DSN boundary (`eda_freeroute::design::to_dsn`) is the summary polygon, with no cutouts, and the autoroute works inside it.
- NPTH holes are not cut out of the 3D slab (`aIncludeNPTHAsOutlines`); mask-only NPTH holes do not knock copper out of a zone (item 13).
- A rectangle's corner radius and the arcs of a `gr_poly` are not in the IR; there is no outline-specific editor (the generic shape tools draw and edit Edge.Cuts); "Fix discontinuities in board outlines" in
  Update PCB from Schematic has nothing behind it; the footprint pass of `BuildBoardPolygonOutlines` (`isCopperOutside`) is the ordinary nesting test here, and a footprint's Edge.Cuts do not follow it when it
  moves.
- Port from: `pcbnew/convert_shape_list_to_polygon.cpp`, `pcbnew/board.cpp` (`GetBoardPolygonOutlines`, its `aIncludeNPTHAsOutlines` argument), `pcbnew/specctra_import_export/specctra_export.cpp`,
  `pcbnew/drc/drc_test_provider_misc.cpp`.

### 28. Bringing files in
**Mostly done (2026-10-11, `task-files-in`)**, limits below. Hit: sometimes; a DXF outline is common. Package P8 for what is left (Import Netlist).
- Done (`PARITY-pcb.md` section 29, `PARITY-sch.md` section 20, `PARITY-symedit.md`, `PARITY-common.md` section 13a): **Import Graphics** for the board, the schematic and the Symbol Editor, from a DXF (LINE, ARC, CIRCLE, POINT, LWPOLYLINE and POLYLINE with bulges, SPLINE, ELLIPSE,
  TEXT and MTEXT, BLOCK and INSERT, `$INSUNITS`, lineweights) or an SVG (paths with lines, cubic and quadratic curves and arcs, rect, circle, ellipse, line, polyline, polygon, transforms, fills with holes). The readers are hand-written, no new dependency
  (`crates/kicad/src/{graphics_import,dxf_import,svg_import}.rs`), the verbs are `Cmd::ImportGraphics` (the layer, the scale, the origin with KiCad's own arithmetic, the default line width and unit, Group imported items, Fix discontinuities with `ConnectBoardShapes`, and a studio option that makes
  an imported Edge.Cuts the board's outline), `Cmd::ImportSchGraphics` and `Cmd::ImportSymbolGraphics` (`crates/ops/src/{import_graphics,import_graphics_sch,fix_shapes}.rs`), each one undo step; the dialog is `components/ImportGraphicsDialog.tsx`.
  kicad-cli's DRC finds a DXF-imported Edge.Cuts outline closed, and reports `invalid_outline` with a side missing (`EDA_SLOW_TESTS=1`). **Append Board** (`crates/cli/src/append_api.rs`: the file read as a paste, the point it is held by, what the paste will change; then `Cmd::PasteClipboard` at the cursor, carried by the Move tool, one undo step).
  **The drops** (`components/FileDropHost.tsx`, `kicad-port/fileDrops.ts`: each editor's `m_acceptedExts`): a `.kicad_pcb` on the board, a `.dxf` or `.svg` on the board and the schematic, a `.kicad_mod` on the Footprint Editor.
  Browser check `e2e/files-in.check.js`: 102 checks through `window.__eda`.
- Left: **Import Netlist** (P8c; the parts and nets come from the intent); `DdAppendFile` (a schematic dropped on the sheet: Import Sheet reads a file by its path on the server) and `DdAddImage` (no bitmap item); Import Graphics in the Footprint Editor (`placeImportedGraphics` there);
  the placement preview (the picture is on the board while the Move tool carries it, in a step held open: one undo step with its drop, Escape leaves nothing; KiCad previews it first; the Symbol Editor's items are added at the cursor and selected); an Append Board that adds the file's layers and keeps every net as KiCad does (the paste's rules apply: copper on a layer this board lacks is left out, an item on a net nobody brings has none; a repeated reference is shown as the file has it, as KiCad leaves it, but filed under a free one);
  several files in one drop (the first is opened, the rest said to be skipped); the DXF's text keeps the file's height only, a symbol has no curves (a curve is cut into segments), a shape with no stroke is a hairline, an SVG pattern and gradient are not drawn, SVG `text`, `image` and `use` are reported and not imported.
- Port from: `common/import_gfx/{dxf_import_plugin,svg_import_plugin,graphics_importer,graphics_importer_buffer}.cpp`, `pcbnew/import_gfx/{dialog_import_graphics,graphics_importer_pcbnew}.cpp`,
  `eeschema/import_gfx/`, `pcbnew/tools/pcb_control.cpp` (`AppendBoard`), `board_editor_control.cpp` (`ImportNetlist`), `eeschema/tools/sch_drawing_tools.cpp` (`ImportSheet`).

### 29. Search and inspection tools
**Done 2026-10-13 on `task-search-inspect` (not merged); what is left below.** Hit: occasionally; Clearance Resolution is how a DRC error is understood. Blocks: no. Package P11, size M-L.
- Done (`PARITY-pcb.md` sections 32a to 32c, `PARITY-fpedit.md` section 12): Find by Properties and Find following the IR (item 21); **Inspect > Clearance Resolution and Constraints Resolution**: `crates/drc` now holds KiCad's PCB expression language
  (`libeval.rs`, `pcbexpr.rs`: the lexer and grammar, the code generator, the stack machine and the built-in functions), the `.kicad_dru` parser (`dru.rs`, `DRC_RULES_PARSER`) and `DRC_ENGINE::EvalRules` (`constraints.rs`, `RuleEngine::eval_rules`: the implicit rules
  of the board setup, the net classes and the composite ones, the diff pairs and the keepout areas, then the rules file in order, the local overrides of a pad and a footprint and a zone's own clearance, net ties, the board minimum and the assertions) in one function that returns
  the winning constraint and every rule it considered with KiCad's report text; `rule_board.rs` makes the items and the rules of a design, `inspect.rs` the report pages (`reportClearance`, `InspectConstraints`), `crates/cli/src/inspect_api.rs` the route
  (`POST /api/inspect/{clearance,constraints}`), `components/InspectReportDialog.tsx` the two dialogs and `actions/inspectActions.ts` the commands with KiCad's selection and picker rules. This inspects the rules; kicad-cli stays the only DRC engine.
  Proof against kicad-cli (`EDA_SLOW_TESTS=1`): for a net class, an explicit rule and a loosening rule the clearance the report resolves is the one kicad-cli's DRC enforces (a violation 10 um under it, none 10 um over); the rule sets of
  `qa/data/pcbnew` (38 `.kicad_dru` files) parse and four of them resolve to the winner their rules say. **The Footprint Checker** (`pcbnew.ModuleEditor.checkFootprint`): `crates/kicad-engine/src/footprint_check.rs` puts the footprint being edited on a scratch board
  (its `.kicad_mod`, a net per pad number, the Footprint Editor's severities), runs `kicad-cli pcb drc` on it through the kicad lane and returns the violations of the footprint-level checks (courtyard, attributes, pads, shorting pads, holes, net ties,
  clipped silk) as markers with the editor's own item ids; the dialog (Show boxes and badges, the marker list, Delete Marker, Delete All Markers, Run Checks) and the markers on the footprint canvas.
- Missing: the implicit rules of the tuning profiles (`addTuningSingleRule`; the profiles are in the IR since P9a); component classes and sheet paths in a condition (`hasComponentClass`, `memberOfSheet` say false); `fromTo`; a footprint's user fields in `getField`
  (only Reference, Value and Footprint); properties KiCad has and the IR does not (a rule that names one compiles and reads it as undefined); micro vias (the IR has no via type: a via is through, blind or buried by its layers), a footprint's own paste margin and the net-tie
  cache of a footprint's copper graphics; text variables and cross references in the rules text are not expanded (`${VAR}` is an unresolved variable here); a footprint field (`REF:Name`) cannot be inspected; the courtyard report notice (`MALFORMED_COURTYARDS`) is never set;
  the geometry functions treat touching as intersecting (KiCad deflates the area by an epsilon); the DRC Rule Editor (item 3, P9b).
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
- Left: the Extruded 3D Body box of the tab (`EXTRUDED_3D_BODY`: a body from the courtyard, fab or silkscreen outline or the pins' box, its height, standoff, material and colour, its STEP export) and the embedded files (`PANEL_EMBEDDED_FILES`, `kicad-embed://`, Collect and Embed 3D Models), which need the zstd of item 24;
  Migrate 3D Models' load-time half (`AutoMigrateByFilename` and the info bar "N WRL 3D models could not be matched", which `pcb_edit_frame.cpp` runs when a board opens) and a folder picker over the whole disk (the studio lists the folders it may read: KiCad's library, the board's, the path variables'); "Show 3D viewer in own window" of the Footprint Chooser; IGES and X3D models in the
  viewer and the chooser (a candidate may be an IGES file); for a bottom-side footprint the tab shows the numbers in the footprint's top-side frame where KiCad shows the file's (a half turn about z apart; the file written is KiCad's); a built-in package (`0603`) opened in the Footprint Editor has no model list (the generic model is added only when a footprint is placed).
- Port from: `pcbnew/dialogs/panel_fp_properties_3d_model.cpp`, `dialog_footprint_properties.cpp`, `dialog_footprint_properties_fp_editor.cpp`, `dialog_migrate_3d_models.cpp`, `3d-viewer/dialogs/{panel_preview_3d_model,dialog_select_3d_model}.cpp`, `3d-viewer/3d_cache/`, `pcbnew/footprint_chooser_frame.cpp`.

### 31. Custom-shape pads
**Done (2026-10-10, `task-custom-pads`)**, limits below. Hit: footprints with custom pads, imported boards, the zone fill around them. Blocks: no. Package P7.
- Done (`PARITY-pcb.md` section 27, `PARITY-fpedit.md` section 10): A pad keeps its real shape in the IR (`Pad::ext`, `LibraryPad::custom`); the `.kicad_pcb` and `.kicad_mod`
  readers and writers are `pcb_io_kicad_sexpr`'s (kicad-cli's plot of the original and of our export is identical, for a board and for a library footprint); and the DRC, the routing gate, the router, connectivity and the zone filler
  measure that copper (`eda_drc::pad_shape`, `PadPiece`), so the gap to a trapezoid's slanted side, a chamfered corner or a custom pad's arm is within 2 um of kicad-cli's DRC. Pad Properties, on the board and in the footprint editor,
  takes KiCad's nine shape choices, a trapezoid's delta, a chamfer's ratio and corners and a custom pad's list of basic shapes (add, edit, delete: one undoable verb). Zone-fill parity (`crates/zone-filler/PARITY.md`): `stonehenge`
  GND 11.33% to 0.15% of the area, `issue5093`'s `/right/GND` 776.56% to 1.49%; 198 of the 208 fills of the 17 QA boards are within 1% of kicad-cli's area and 189 within 0.5%.
- Left: Explode and Recombine (Edit Pad as Graphic Shapes, Finish Pad Edit), padstacks (a pad has one shape for all layers; no `remove_unused_layers`),
  a custom pad's proxy spokes, `gr_rect` corner radius and arcs inside `gr_poly`, the microwave arc stub and function shape, and the instant 3D scene (`eda_drc::pad_shape::pad_outline` and the state's `pads[].outline` are there for it).
- Measured: 40 new Rust tests (five of them slow-tier, with kicad-cli) and 25 new
  `node --test` cases; `web/studio/e2e/pad-shapes.check.js` runs 6 scenarios through `window.__eda` on a served board.
- Port from: `pcbnew/pad.cpp`, `pcbnew/tools/pad_tool.cpp` (`EditPad`), `pcbnew/dialogs/dialog_pad_properties.cpp`, `pcbnew/zone_filler.cpp` (`addKnockout`, custom pad parts), `pcbnew/microwave/microwave_polygon.cpp`.

### 32. Design blocks
**New, open; after item 5 (tables) and Import Sheet (item 28).** Hit: reuse of sub-circuits. Blocks: no. Package P18, size L.
- 23 rows: the PCB and schematic panels, place (schematic: the same function as Import Sheet), place linked, save selection, sheet or board as a block, update, properties, delete, the block library table.
  No code names a design block. The schematic half's "no copy and paste" reason is stale.
- Port from: `eeschema/tools/sch_design_block_control.cpp`, `pcbnew/tools/pcb_design_block_control.cpp`, `common/tool/design_block_control.cpp`, `common/design_block{,_info,_io,_library_adapter,_tree_model_adapter}.cpp`,
  `common/widgets/{design_block_pane,panel_design_block_chooser}.cpp`, `common/dialogs/{dialog_design_block_properties,panel_design_block_lib_table}.cpp`, `eeschema/sch_design_block_utils.cpp`, `pcbnew/pcb_design_block_utils.cpp`.

### 33. Browser-check coverage
**New, standing.** Package P20.
- `web/studio/e2e/*.check.js` exist for the Appearance panel, the DRC and ERC review, footprint edit and arrays, the Properties panel, snap and grid, and the 3D viewer. `sch-details.check.js` (P10, 2026-10-12) covers the schematic's pins and Swap Pins, sheet pins, auto-rotating labels and
  justified text, the net-collision overlay, the fields table's attribute columns, a power symbol's Properties, Copy as Text and the pan buttons and auto-pan. Not covered: the rest of schematic editing and the clipboard
  (items 1, 6, 12), the hierarchy (4), Board Setup (3), the library choosers (5), the router (7), PCB move, rotate, flip and clipboard (8), zones (13), groups (17), Find (21), the context menu (16). The rule in
  force: a fix counts when the C++ function is named in the code, the logic has a `kicad-port/*.test.ts`, one user action is one undo step, and a browser check ran.

### 34. The simulator
**New (it was Appendix B), in scope and last.** Package P21, size XL.
- 29 rows: `eeschema.Simulation.*` (11), `eeschema.Simulator.*` (9), the OP voltages and currents and the simulator window (3), and the plot's zoom history and axes (6, `common.Control`). ngspice 45.2 is installed; the
  plan is `kicad-cli sch export netlist --format spice` and `ngspice -b`, the raw output read back. Port from: `eeschema/sim/`, `eeschema/tools/simulator_control.cpp`, `eeschema/dialogs/dialog_sim_*`.

### 35. The long tail of the PCB tools
**New, open.** Hit: rarely each. Package P6, with P13 for zones. From the "not ported" rows of `PARITY-pcb.md`.
- Done 2026-10-10 (`task-pcb-edit-details`): Cleanup Tracks & Vias takes the net, net class, layer and "selected items only" filters and "Refill zones before and after cleanup", and leaves locked tracks and vias alone
  (section 12); Edit Track & Via Properties filters by net class, exact track width and via diameter and picks through / blind / buried vias, and its layer filter reaches vias by the top layer of their span; Edit Text &
  Graphics Properties reaches dimensions, lists every board layer and has "Set to layer (and dimension) default values" (section 13). Still open there: microvias and the annular-ring and IPC-4761 presets (a via has no
  padstack here); of text and graphics the footprint reference, value, other-field, text, graphic and dimension scopes, the two "By parent ..." filters, bold, italic, font, keep upright, visible and "center text on footprint"
  (a footprint's own text is not an item of the board until package P1). Rule areas: multi-layer rule areas, `(disallow ...)` rules on non-keepout items, `DRCE_TEXT_ON_EDGECUTS` (section 14). Teardrops: curved edges,
  rectangular and non-circular pad anchors, track-to-track, two-segment borrowing, `m_TdOnPadsInZones`, incremental updates (section 15). Dimensions: a leader's text border, manual text position,
  Rotate/Flip of a dimension (section 18). The Draw Zone Fill Fracture Borders / Triangulation debug modes draw strokes only (the triangulation is an earcut port without KiCad's Z-order hash); Flip Board View mirrors text on
  non-side-specific layers with the board where KiCad keeps it readable; Automatically select track width does not turn off when a width is picked in the toolbar and the first width key while routing
  does not set `m_TempOverrideTrackWidth` (`PARITY-boardctl.md`).

## Appendix A. The original 30, one verdict each

13 closed, 9 partial, 0 open, 8 out of scope (2026-10-10). Changed since 2026-10-07: #11 and #25 closed, #14 closed on `task-pcb-edit-details` (not merged); #6 and #26 re-worded.

| # | Gap | Verdict | Evidence, or where it went |
|---|---|---|---|
| 1 | Schematic editing | **Closed** | 168 eeschema handlers (`actions/useActionRunner.ts`, `schEditActions.ts`), `components/SchematicView.tsx`, verbs in `crates/ops/src/{lib.rs,sch_edit.rs,sch_move.rs}`, one-netlist rule in `crates/cli/src/board.rs::reconcile_schematic` (test `schematic_wire_connects_and_disconnects_pins_on_one_netlist`). Behaviour: items 1, 6, 12 are closed. |
| 2 | DRC engine hangs | Out of scope | kicad-cli is the only engine (`docs/ARCHITECTURE.md`); the Rust copy is deleted. |
| 3 | DRC precision | Out of scope | same |
| 4 | ERC has no UI | **Closed** | `components/ErcDialog.tsx`, `GET /api/erc` to `crates/kicad-engine/src/lib.rs::erc`, exclusions `Cmd::AddErcExclusion`, pin map `SchematicSetupDialog.tsx`. Review workflow: item 11. |
| 5 | Zone fill | Partial | `crates/zone-filler`, `crates/drc/src/fill.rs`, `ZoneDialog.tsx`, `ZoneManagerDialog.tsx`; 190 of 208 fills within 1% of kicad-cli's area; items 13 and 31. |
| 6 | Hierarchical sheets | Partial | IR, import, export, panel, navigation, editing on every sheet, nets across sheets, Find, Annotate, Import Sheet, sheet copy and paste, lint and render over the hierarchy (`ir.rs`, `sch_import.rs`, `HierarchyPanel.tsx`, `sch_annotate.rs`, `sch_sheets.rs`); item 4 lists what is left. |
| 7 | Routing | Partial | `crates/pns`; item 7. |
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
| 27 | Grouping | Partial | board: done, nested (`Cmd::Group` family in `crates/ops/src/pcb_groups.rs`, `kicad-port/groupTree.ts`, `state/store.tsx::withGroupSubstitution`, the entered-group overlay in `painter.ts`, the `.kicad_pcb` file); the schematic and the footprint editor have no groups; item 17. |
| 28 | Dimensions and measure | **Closed** | `crates/connectivity/src/dimension.rs`, `Cmd::AddDimension` family, `components/DimensionPropertiesDialog.tsx`, the measure tool. Left: the interactive height click, text border, manual text position, export (item 2). |
| 29 | Pan | Partial | `kicad-port/viewControls.ts`, `Canvas.tsx` (PCB); item 20. |
| 30 | Context menu | **Closed** | `kicad-port/pcbContextMenu.ts` (KiCad's `CONDITIONAL_MENU` and the entries of every tool's `Init()`), `pcbSelectionSummary.ts`, `components/canvas/pcbMenuBuilder.ts`, `Canvas.tsx`. Left: table cells, gate swap, generators, most of the router's via and posture entries (item 16, item 7). |

## Appendix B. Out of scope and deferred

- **kicad-cli's job** (`docs/ARCHITECTURE.md`): DRC, ERC, plots, Gerbers, drill and position files, netlist, BOM, statistics, STEP, GLB, VRML,
  IPC-2581, ODB++, D356 and GenCAD. Old #2, #3 and #19-#24. The UI around them is in scope (item 11). IDF, Hyperlynx and the footprint report stay unwired: kicad-cli has none (3 rows).
- **Python scripting and plugins**: `common.API.pluginsReload`, `pcbnew.ScriptingTool.*`, the footprint wizards (`pcbnew.FpWizard.*`, `ModuleEditor.createFootprint`) (6 rows).
- **Project management and the application shell**: New, Open, Revert, Quit, the project manager, Open Non-KiCad Board, Rescue, Open Directory, Open with Text Editor, the calculators (10 rows). A web page has none of these.
- **Simulator, in scope and last**: 29 rows, now item 34 and package P21.
- **Deferred until the IR has the item** (reasons in `ui-parity-missing.json`, re-checked in `STALE-REASONS.md`): design blocks (23 rows, item 32), generators (9), tables (18), PCB text boxes, barcodes, points,
  reference images and hatch fills, schematic images, the symbol text box (item 24), derived symbols, alternate pin functions, design variants (3), padstacks (Remove Unused Pads),
  embedded files (4: zstd, and no new dependency), custom pads (item 31: Explode, Recombine and the microwave shapes are left). The autoplacer (2) is declined: placement runs through `eda board` behind the placement gates. The Footprint and Symbol
  Viewer windows (5) are not planned; the editors' library trees browse.
- **Internal events** (13 rows) have nothing to wire.

## Appendix C. Zone filler against `pcbnew/zone_filler.cpp`

Audited 2026-10-02 (`e45bf2a`), re-audited 2026-10-08 after the fidelity work (item 13). The functions that were already ported are not listed: `addHoleKnockout`,
`buildDifferentNetZoneClearances`, `subtractHigherPriorityZones`, `postKnockoutMinWidthPrune`, the rule-area knockouts (`FillKeepout`) and the `fillSingleZone` structure.
Each row below is measured against kicad-cli in `crates/zone-filler/PARITY.md` (before and after).

| KiCad function | `crates/zone-filler` |
|---|---|
| `Fill` | ported (`orchestrate.rs::fill_board`): zones fill from the highest priority down (`ZONE::HigherPriority`: teardrop, priority, then the id where KiCad uses the UUID) and each is knocked out by the filled copper of the zones above it, not their outlines; the iterative refill. Not ported: the dependency waves run in parallel (one sorted pass here) and a zone on several layers (one layer per zone) |
| `FillIsolatedIslandsMap` (`islands.rs`) | ported: an island is a fragment whose same-net copper cluster, through pads, tracks, vias and the zones of other layers, holds no pad; `ISLAND_REMOVAL_MODE` always / never / below area; islands mostly outside the board are dropped last |
| `refillZoneFromCache` | ported (`lib.rs::refill_zone_from_cache`): the pre-knockout fill is cached, and a zone that a higher-priority zone gave space back to is refilled from the cache against the fills as they now are |
| `addKnockout` | ported (2026-10-10): a pad is knocked out as its shape (`PAD::GetEffectiveShape`: a trapezoid, a chamfered rectangle, a custom pad's anchor and primitives) and a custom pad with `(clearance convexhull)` as the hull of its shapes (`CUSTOM_SHAPE_ZONE_MODE::CONVEXHULL`) |
| `knockoutThermalReliefs`, `buildThermalSpokes` (`spokes.rs`) | ported: `DRC_ENGINE::EvalZoneConnection` (a pad's own connection over its footprint's over the zone's; "PTH only" thermal-relieves plated holes), the pad's relief gap and spoke width over the zone's (the width at least the zone's minimum and at most the pad's smaller side), the spoke angle (90 degrees for oval and rectangular pads, 45 otherwise, or the pad's own), spokes turned with the pad, circles built at 0 degrees and turned, the spoke ends, the test point and the mutual overlap, so only spokes that reach copper stay; they start from the box of the pad's copper (`dummy_pad.GetBoundingBox`), a custom pad's too. Not ported: a custom pad's proxy spokes (`gr_vector`) |
| `buildCopperItemClearances` | partial: no courtyard knockouts, no net-tie exemptions; **the board edge** (`copper_edge_clearance` from every Edge.Cuts item as `knockoutGraphicClearance` takes it: an arc's curve, a circle's ring, a cutout's rim, a solid shape's inside; `FillInput::edge_cuts`); a pad's or footprint's own clearance override replaces the net class's and zone's for that pad, floored at the board minimum (`DRC_ENGINE::EvalRules`) |
| `ZONE::BuildSmoothedPoly` (`smoothed.rs`, `corner.rs`) | ported: same-net zones merged, the board outline (`GetBoardPolygonOutlines`: every outline with its cutouts, applied only when it is well-formed, `m_brdOutlinesValid`; `crates/drc/src/outline.rs`), the minimum-width apron, and the chamfer and fillet corner smoothing |
| `connect_nearby_polys` | missing |
| `fillNonCopperZone` | missing |
| `buildHatchZoneThermalRings`, `addHatchFillTypeOnZone` (`hatch.rs`) | ported: thickness, gap, orientation, smoothing level and amount, minimum hole area, and the thermal rings round pads and vias that are connected to a hatched zone |

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
  `zone` = `crates/zone-filler/` and `crates/drc/`; `libs` = `crates/cli/src/library_*.rs`, `body_api.rs` and the choosers; `docs` = `ui-parity-missing.json`, `UI-ACTIONS.md`, `GAPS.md`, `PARITY-*.md`.

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
- **P14** merged 2026-10-13 (the Extruded 3D Body, embedded files and the load-time migration are left: item 30). **P11** merged 2026-10-13 (Find by Properties, Clearance and Constraints Resolution, the Footprint Checker; what is left: items 21 and 29).
- **P2** merged 2026-10-12 (Compare Footprint with Library included); Swap Pad Nets and Swap Gate Nets are left (item 26).
- **Any time a slot is free**: P12, P15, P6, P9a, P20 (test files only). **P11** is done on `task-search-inspect` (2026-10-13, not merged).
- **After P7**: P13, P17, and P19's arc stub and function shape.
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
- **P13 after P7**: both are `crates/zone-filler` (custom-pad knockouts and spokes, then the rest).
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
- Left: Import Sheet, Draw Sheet from File and the schematic drop (`Cmd::ImportSheet` = `AddSheet` + the file's screen from `import_kicad_sch`, renumbered with `annotate_paste`; a route that reads the file); hierarchical sheets
  in the clipboard; a net per placement of a shared screen; buses across sheets; Reorganize keeping stored field places, label spins, looks and body styles; browser check for the hierarchy.
- Files: `crates/ops/src/{sheets,sch_clipboard}.rs` and a new `sheet_import.rs`, `crates/engine/src/{nets,hier}`, `crates/cli/src/{board,sch_hierarchy_api}.rs`, `crates/kicad/src/{sch_clipboard,sch_import}.rs`,
  `web/studio/src/components/{SchematicView,panels/HierarchyPanel}.tsx`, `kicad-port/{schSheet,schClipboard,navHistory}.ts`.
- KiCad: `eeschema/{sch_sheet_path,sch_screen,sch_sheet,connection_graph}.cpp`, `tools/{sch_drawing_tools (ImportSheet, DrawSheet),sch_navigate_tool,sch_editor_control (doCopy, Paste),sch_edit_tool (DdAppendFile)}.cpp`.
- Conflicts: P2 (`ops/lib.rs`, `board.rs`); P8 (the drop host); P10 (`SchematicView.tsx`); P18 (placing a block is Import Sheet).

### P5. Router: drag, mid-segment, diff pairs, meanders (item 7). Size XL, three parts. Rank 5. Independent of the rest. **P5a, P5b and P5c done; what item 7 lists as missing is left.**
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
  `web/studio/e2e/length-tuning.check.js` (93 checks through the hotkeys, the canvas and the dialogs on a scratch board built from `examples/mcu_board_30plus.yaml`). After the docs, on KiCad's own boards (these need `KICAD_SRC_DIR`; the sweep is the slow tier): the four patterns of `issue22536` tuned again from their baselines to 76.2 mm exactly over its arcs (`65129b4`), the tuner's length of 8 nets of `length_calculations` within 3 um of kicad-cli's through pads, vias and the stackup (`8bbd7b2`), and 142 tunings of random tracks on eleven QA boards, every one accepted by the board, 41 single ones measured as the placer said, removed to the length they had and tuned again (`db9963a`). From (4): arcs are chords in the world (`from_ir.rs`), and `ShoveTimeLimit` is KiCad's one second (`543fa0d`: `time_limit.rs`, checked after every iteration of the shove, `shove_time_limit` in the router settings). **Left, item 7 lists it: real `ARC_T` items, mouse-trail posture, springback, and the tuner's own gaps (time domain, pad-to-die, a merged outline for a pair, moving a pattern).**
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
  `crates/zone-filler` (`addKnockout` custom-pad mode, thermal proxy spokes), `crates/pns/src/from_ir.rs`, `crates/drc/src/board.rs` (`placed_pad_copper`), `components/BoardPadPropertiesDialog.tsx`,
  `components/footprint/PadPropertiesDialog.tsx`, `canvas/painter.ts`, `components/footprint/footprintPainter.ts`, `kicad-port/pad*.ts`, `actions/libraryEditorActions.ts` (explode and recombine).
- KiCad: `pcbnew/pad.cpp`, `tools/pad_tool.cpp` (`EditPad`), `dialogs/dialog_pad_properties.cpp`, `zone_filler.cpp` (`addKnockout`), `microwave/microwave_polygon.cpp`.
- Conflicts: P1 (footprint IR, writer, painter), P13 (`zone-filler`), P17 (pad dialogs), P19 (arc stub, function shape), P14 and P2 (`fp_edit.rs` pad edits).

### P8. Files in: Append Board, DXF and SVG, drops (item 28; SR1's PCB half). Size S for what is left. Rank 8. **Mostly done: `task-files-in` (2026-10-11); item 28 lists what is left.**
- Done in the branch: **P8a** the drop host (`components/FileDropHost.tsx`, `kicad-port/fileDrops.ts`), Append Board and its drop (`crates/cli/src/append_api.rs`, `kicad-port/appendBoard.ts`, then `Cmd::PasteClipboard` with the carry) and the Footprint Editor's drop; **P8b** Import Graphics for the board, the schematic and the symbol editor:
  the readers in `crates/kicad/src/{graphics_import,dxf_import,svg_import}.rs`, the verbs in `crates/ops/src/{import_graphics,import_graphics_sch,fix_shapes}.rs`, `crates/cli/src/graphics_api.rs`, `components/ImportGraphicsDialog.tsx`.
  Tests: 48 kicad unit tests and 11 integration tests on fixtures, 35 ops tests, 3 slow kicad-cli proofs, 15 cli tests, 23 web unit tests, and `e2e/files-in.check.js` (102 checks).
- Left: **P8c, M, low priority**: Import Netlist; the rest is in item 28's "Left".
- KiCad: `common/import_gfx/{dxf_import_plugin,svg_import_plugin,graphics_importer,graphics_importer_buffer}.cpp`, `pcbnew/import_gfx/{dialog_import_graphics,graphics_importer_pcbnew}.cpp`, `eeschema/import_gfx/`,
  `pcbnew/tools/pcb_control.cpp` (`AppendBoard`, `placeBoardItems`, `DdAppendBoard`), `pcbnew/tools/board_editor_control.cpp` (`ImportNetlist`).
- Conflicts: P4 (the sheet drop), P3 (the library drop), P6 (`store.tsx` `placeNewItems` hands a placement to the Move tool beside `pasteClipboard`); `ops`, `runner`, `studio` hunks.

### P9. Board Setup completion and the rule editor (item 3). Size L; the rule editor XL on its own. Rank 9.
- **Status (2026-10-12): P9a done on `task-board-setup`** except Component Classes (`component_classes_drc`) and Embedded Files; P9b open. Item 3 lists what each page does and what is left.
- **P9a, L**: the missing pages: Board Editor Layers, Text Variables (project variables and `${VAR}` expansion in texts and fields), Formatting, Zone Hatch Offsets, Length-tuning Patterns, Tuning Profiles (`missing_tuning_profile`),
  Component Classes (`component_classes_drc`), regular-expression net patterns and composite net classes, and drawing tools that start from the Text & Graphics defaults. Files: `crates/model/src/rules.rs`,
  `crates/ops/src/board_setup.rs`, `crates/kicad-engine/src/lib.rs` (project file), `crates/kicad/src/{pcb,import,custom_rules}.rs`, `web/studio/src/components/{BoardSetupDialog.tsx,boardSetup/*}`, `kicad-port/boardSetupRules.ts`.
- **P9b, XL**: the rule-tree designer (`DRETool.drcRuleEditor`).
- KiCad: `pcbnew/dialogs/panel_setup_{layers,formatting,zone_hatch_offsets,tuning_patterns,tuning_profiles,tuning_profile_info}.cpp`, `panel_assign_component_classes.cpp`, `pcbnew/component_classes/`,
  `common/dialogs/{panel_text_variables,panel_setup_netclasses}.cpp`, `common/project/component_class_settings.cpp`, `pcbnew/drc/rule_editor/` (82 files), `pcbnew/tools/drc_rule_editor_tool.cpp`.
- Conflicts: P15 (the project writer in `kicad-engine`: a hunk); `ops`, `runner` hunks.

### P10. Schematic details (items 1, 12, 15, 17, 20). Size M-L. Rank 10. **Mostly done on `task-sch-details` (2026-10-12, 15 commits, not merged); schematic groups, a user-defined field's place and Pin Helpers are left.**
- Done there: pin selection (`SCH_PIN_T`: Swap Pins, Swap Pin Labels, Swap Unit Labels, the pin-level highlight and Select Node), sheet pins one by one, the net-collision overlay of a drag, `AutoRotateItem` after a label lands, a power symbol's Properties dialog,
  text justification flipping on a mirror, pan with the middle and right button and auto-pan while drawing and moving, the Symbol Fields Table's attribute columns and Exclude DNP filter, Copy as Text, part of the item 15 schematic list. `PARITY-sch.md` sections 21 to 27;
  `web/studio/e2e/sch-details.check.js`; the kicad-cli proof is `crates/kicad/tests/sch_details.rs`.
- Left: schematic groups (`sch_group_tool.cpp`), a user-defined field's place on the sheet (`SchematicSection::field_layout`), Pin Helpers, a sheet pin's text look, a label's own fields.
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
- Conflicts: P19 (the DSN boundary), P1 and P7 (`ir`: a hunk each), P13 (mask-only NPTH knockout).

### P13. Zone leftovers (item 13). Size L. Rank 13. After P7.
- `connect_nearby_polys`, non-copper zones (`fillNonCopperZone`, `dialog_non_copper_zones_properties`), multi-layer zones, mask-only NPTH knockouts, courtyards and net ties as knockouts, the hatched zone's thermal rings, Auto-Assign
  Priorities, a Zone Manager preview, a zone lock in Zone Properties, the speed of a fine hatch over a board.
- Files: `crates/zone-filler/src/*`, `crates/drc/src/fill.rs`, `crates/model/src/ir.rs` (`Zone` layers), `crates/kicad/src/{pcb,import}.rs` (zone layers), `components/{ZoneDialog,ZoneManagerDialog}.tsx`.
- KiCad: `pcbnew/zone_filler.cpp`, `zone.cpp`, `zone_manager/`, `dialogs/panel_zone_properties.cpp`, `dialogs/dialog_non_copper_zones_properties.cpp`.
- Conflicts: P7 (`zone-filler`), P12 (NPTH), `kio` hunk.

### P14. The 3D models of a footprint (item 30; SR3). Size M. Rank 14. **Done 2026-10-13 on `task-fp-3d`, waiting for its merge; item 30 lists what is left.**
- A model list on `FootprintEdit` (`models3d`) with its verb, a 3D Models tab in both Footprint Properties dialogs (`PANEL_FP_PROPERTIES_3D_MODEL`: list, offset, scale, rotation, opacity, show; path chooser through `model3d_api`; a preview that
  reuses `viewer3d/modelCache.ts`), the library editor's `model` becoming a list, Migrate 3D Models, the 3D preview of the Footprint Chooser.
- Files: `crates/model/src/{fp_edit,ir}.rs`, `crates/ops/src/{fp_edit,library_editors}.rs`, `crates/kicad/src/{footprint_lib,footprint_import,pcb}.rs`, `components/{FootprintPropertiesDialog.tsx,footprint/FootprintPropertiesDialog.tsx}`,
  `components/FootprintChooserDialog.tsx`.
- KiCad: `pcbnew/dialogs/panel_fp_properties_3d_model.cpp`, `dialog_footprint_properties.cpp`, `dialog_footprint_properties_fp_editor.cpp`, `dialog_migrate_3d_models.cpp`, `3d-viewer/3d_cache/`.
- Conflicts: P2 and P7 (`fp_edit.rs`), P1 (the footprint), P17 (the editor dialog).

### P15. Review workflow leftovers (item 11). Size S-M. Rank 15.
- ERC exclusions into the derived project (`erc.erc_exclusions`, `SCH_MARKER::SerializeToString`, positions guessed as `eda_kicad::drc_exclusions` does) with their comments; Delete Marker, Delete All Markers and Save report; "Exclude all violations of
  rule"; a marker selected by a left click; the Inspect / Fix entries; Edit connection grid spacing. The kicad-cli marker position stays upstream and unplanned.
- Files: `crates/kicad-engine/src/lib.rs`, `crates/kicad/src/drc_exclusions.rs` and a new `erc_exclusions.rs`, `crates/ops` (`AddErcExclusion` gets a comment), `components/{ErcDialog,DrcDialog,SchematicSetupDialog}.tsx`.
- KiCad: `eeschema/sch_marker.cpp`, `eeschema/schematic.cpp` (`ResolveERCExclusions`), `eeschema/erc/erc_settings.cpp`, `dialogs/dialog_erc.cpp`, `pcbnew/dialogs/dialog_drc.cpp`, `common/rc_item.cpp`.
- Conflicts: P9 (the project writer: a hunk).

### P16. PCB item kinds the IR lacks (item 24). Size L. Rank 16. Deferred.
- Text box first (`PCB_TEXTBOX`), tables (`PCB_TABLE`, Place Stackup, Place Board Characteristics: the fab drawing tables) with the schematic's, barcodes, points, reference images, hatch fills, the schematic image. Each is an IR item, a reader and a writer, a painter,
  a hit test, Properties rows and verbs.
- Files: `crates/model/src/ir.rs`, `crates/kicad/src/{pcb,pcb_items,import_items}.rs`, new files in `crates/ops/src/`, `canvas/painter.ts`, `kicad-port/{pcbItems,pcbProperties}.ts`.
- KiCad: `pcbnew/pcb_textbox.cpp`, `pcb_table.cpp`, `pcb_tablecell.cpp`, `pcb_barcode.cpp`, `pcb_point.cpp`, `pcb_reference_image.cpp`, `dialogs/dialog_{table,tablecell,barcode,reference_image}_properties.cpp`, `tools/pcb_edit_table_tool.cpp`,
  `tools/drawing_tool.cpp`, `eeschema/sch_{table,bitmap}.cpp`, `eeschema/tools/sch_edit_table_tool.cpp`.
- Conflicts: P1 (`painter.ts`, `ir`, `kio`), P6 (`Canvas.tsx`), P7 (`ir`).

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

### P18. Design blocks (item 32; SR5). Size L. Rank 18. After P3 and P4.
- A block library (the table of P3, `.kicad_blocks` folders with a `.kicad_sch`, a `.kicad_pcb` and metadata), the panels, place (schematic: Import Sheet; board: Append Board), place linked, save selection, sheet or board as a block, update, properties, delete.
- Files: new `crates/kicad/src/design_block.rs`, `crates/ops` (new file), `crates/cli` (a new `design_block_api.rs`), `components/panels/DesignBlockPanel.tsx`, dialogs.
- KiCad: `eeschema/tools/sch_design_block_control.cpp`, `pcbnew/tools/pcb_design_block_control.cpp`, `common/tool/design_block_control.cpp`, `common/design_block{,_info,_io,_library_adapter}.cpp`,
  `common/widgets/{design_block_pane,panel_design_block_chooser}.cpp`, `common/dialogs/dialog_design_block_properties.cpp`.
- Conflicts: P3, P4 (Import Sheet), P8 (Append Board); `ops`, `studio`, `runner` hunks.

### P19. Microwave tools and Specctra (items 9, 27; SR3, SR6). Size S-M each. Rank 19.
- Microwave: the gap and the stub now (a dialog for the size, a two-pad footprint builder in `kicad-port/microwave.ts`, `Cmd::PlaceFootprint` with a `definition`); the arc stub and function shape after P7; the line needs the inductor generator
  (`microwave_inductor.cpp`). Specctra: `eda_freeroute::design::to_dsn` exposed as a download, the boundary with cutouts (P12), then an SES reader and a verb that adds the tracks and vias.
- KiCad: `pcbnew/microwave/{microwave_tool,microwave_footprint,microwave_inductor,microwave_polygon}.cpp`, `pcbnew/specctra_import_export/{specctra,specctra_export,specctra_import}.cpp`.
- Conflicts: P7 (custom pads for two rows), P12 (the boundary), `ops` and `runner` hunks.

### P20. Browser checks for the older areas (item 33). Size M each, standing. Rank 20.
- One `web/studio/e2e/<area>.check.js` per area in the pattern of the six that exist (a copy of a board, `window.__eda`, every edit undone and the board compared): schematic editing and clipboard, hierarchy, Board Setup, the choosers, the router,
  PCB move, rotate, flip and clipboard, zones, groups, Find, the context menu. Test files only, plus additions to `kicad-port/edaTestHook.ts`.
- Conflicts: none beyond `edaTestHook.ts` (a hunk). Can run beside anything.

### P21. The simulator (item 34). Size XL. Rank 21. Last.
- `kicad-cli sch export netlist --format spice` and `ngspice -b` through the studio's kicad-cli lane, the raw output read back; analysis tabs, probes (`Probe Schematic`), the plot with its zoom history and axes, OP voltages and currents on the schematic,
  workbook files, exports (CSV, PNG, clipboard, plot to schematic).
- Files: a new `crates/sim/` (netlist through kicad-cli, the ngspice runner, the raw reader), `web/studio/src/components/simulator/*`, `actions/simActions.ts`.
- KiCad: `eeschema/sim/`, `eeschema/tools/simulator_control.cpp`, `eeschema/dialogs/dialog_sim_*`.
- Conflicts: `runner` and `schview` hunks (probes), P10.
