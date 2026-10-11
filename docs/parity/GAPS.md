# agentic-eda vs KiCad: gaps and the plan for the next phase

Status on 2026-10-10, main at `89ff669`, KiCad source `8303b2ad` (10.99).

The first phase made the editors exist. The second (2026-10-07 to 10-10, 22 merges) made what exists behave like KiCad's: item editing and both
clipboards, Board Setup, DRC and ERC review, module sheets, the Properties panel, library choosers, footprints on the board as editable objects,
zone fill, the board outline, snap and grid, the Appearance panel, the router's shove and the 3D viewer. This file is the plan for the third: where
each gap stands now, the gaps the second phase found, a ranking by user impact, and 21 work packages (P1 to P21, the last section) with the files
each touches and the packages it collides with, so several agents can run at once. Verdicts are read from the code, not from the other parity docs;
where a doc and the code disagree, the code wins and the doc is named.

Item numbers are names: code comments and the PARITY docs cite "GAPS.md item 14" and "#8", so they never change and the rank is in the table below,
not in the numbers. Items 25 to 35 are new in this revision. The package names `WP1` to `WP6` of the first phase are kept at the end for the six
library-table reasons in `ui-parity-missing.json` that cite "WP6"; that is P3 now.

## Status

- **Actions wired: 579 of 766** (pcbnew 266/350, eeschema 168/233, common 145/183; `UI-ACTIONS.md`), up from 462 on 2026-10-07. The other 187 are
  recorded with a reason in `web/studio/tools/ui-parity-missing.json`; none is bare. **Every reason was re-checked against main on 2026-10-10
  (`STALE-REASONS.md`): 10 are stale (what they say is missing now exists), 18 are partly stale, 159 hold** (10 of those wait for a branch in
  flight). The stale ones are Find by Properties, Migrate 3D Models, the microwave gap and stub, Append Board and its drop, the Footprint Editor's
  drop, Import Sheet, Draw Sheet from File and the schematic drop; the plan below carries them.
- **The original 30 gaps** (Appendix A; numbering kept because code comments cite "GAPS.md #8", "#20", "#6"): **12 closed, 10 partial, 0 open,
  8 out of scope** (was 10, 12, 0, 8 on 2026-10-07; #11 Property dialogs and #25 Align and distribute closed).
- **Items 1 to 24** are the ranked list of 2026-10-07 with their state brought forward; **items 25 to 35 are new**: the gaps the second phase
  found, from the `PARITY-*.md` "left" notes, the merge messages and reading the code.
- **In flight, not on main** (read with `git diff main...<branch>`): `task-lib-tables` (item 5; the file format, path variables, global and project
  tables, and the index, choosers and lookups running over them: 2 commits, no dialog yet), `task-sch-hierarchy` (item 4; Annotate over the whole
  schematic with KiCad's scope and numbering options, Find and Replace across sheets, the SVG render and the schematic gates over every sheet:
  6 commits), `task-custom-pads` (item 31; branched, no commit yet). Do not start work that touches their files without reading them first.
- **Four findings change the picture.**
  1. *A wired action is not a working feature.* This was the finding of 2026-10-07 and it was fixed where it bit: the schematic edit tools act on
     every item kind (item 1, `2a8bf8f`) and so do the PCB ones (item 8, `88c8c0a`). Before closing any action, run it on every item kind.
  2. *The KiCad files we write drop design data, and kicad-cli judges those files.* The writer keeps zone settings, keepouts, dimensions, groups,
     locks, arcs, teardrops (`b145f0e`) and the board outline as Edge.Cuts items (`a46fd29`). **The biggest thing it still drops is the footprint's
     own graphics: no part outline, pin-1 mark or fab body is on the canvas, in the file or in the silkscreen kicad-cli plots (item 25).**
  3. *One root sits under several gaps: the constraint model built from the intent has no edit verbs.* It now has an overlay for the rules
     (`drawings.rules`), for the nets (`design.nets`), for the libraries, for footprint fields, attributes and pad edits
     (`drawings.footprint_edits`), for the zone filler's pad and footprint settings (`drawings.zone_overrides`) and for footprints that exist only on
     the board (`drawings.board_parts`). **It has none for which footprint a part uses: the schematic's Footprint field never reaches the board,
     Change Footprint does not exist, and a reference rename leaves the board's footprint, its pose and its routing behind (item 26).**
  4. *Most earlier parity work was typechecked and unit-tested and never clicked.* Six areas now have a browser check driven through
     `window.__eda` (`web/studio/e2e/`: Appearance, DRC and ERC review, footprint edit and arrays, Properties panel, snap and grid, the 3D viewer);
     the schematic editors, Board Setup, the choosers, the router, the PCB edit tools, the zones and the hierarchy have none (item 33).
- **Docs.** Action level: `UI-ACTIONS.md`, with `STALE-REASONS.md` beside it. Behaviour tables: `web/studio/PARITY-{pcb,sch,3d,fpedit,symedit,boardctl,common}.md`.
  Code level: `CODE-COMPARE-ui.md` (the first 266 handlers; about 310 added since have not been compared) and `CODE-COMPARE-router.md`,
  `crates/pns/PARITY.md`, `crates/zone-filler/PARITY.md`. Measured round trip and connectivity: `REPORT.md` (the re-export part measured 2026-10-08).
  Engines: `ARCHITECTURE.md`. The "Missing" lists below were re-read against the code where a merge touched them; a bullet no merge touched is
  carried forward from 2026-10-07 and says so.

## What moved since 2026-10-07

| item | what | state | merge |
|---|---|---|---|
| 1 | Schematic item editing: Move, Drag, Rotate, Mirror, Align, Properties for every kind | **Closed** | `2a8bf8f` (10-08) |
| 2 | KiCad files keep zone settings, keepouts, arcs, groups; the outline as Edge.Cuts items | Mostly done | `b145f0e` (10-08), `a46fd29` (10-09) |
| 3 | Board Setup and the rules edit through an overlay | Mostly done | `b145f0e` (10-08) |
| 4 | Module sheets: editing on every sheet, nets traced across sheets | Partly (the rest in flight) | `08c70c8` (10-08) |
| 5 | Symbol Chooser and Footprint Chooser over the installed libraries | Partly (tables in flight) | `80e972d` (10-08) |
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

Not moved: items 15, 19, 20, 24.

## Ranked list: open and partial items, by user impact

"Hit" is how often a KiCad user meets it; "Blocks" is whether a workflow stops. The package column is the plan at the end. The rank is the order of
the plan: a gap that stops a workflow every project meets is above one that costs a detail every session.

| rank | package | items | what the user meets | state | size |
|---|---|---|---|---|---|
| 1 | P1 Footprint graphics on the board | 25, 2 | no part outline, pin-1 mark or fab body on the canvas, in the file or in the silkscreen; an imported board loses them on export | open | L |
| 2 | P2 Footprint assignment, exchange and reference rename | 26, 9 | the Footprint field of a symbol does nothing on the board; no Change Footprint; a rename leaves the footprint and its routing behind | open | L |
| 3 | P3 Library tables and paths | 5 | libraries other than KiCad's installed ones cannot be added | engine in flight | M |
| 4 | P4 Schematic hierarchy: the rest | 4 | Import Sheet, sheets in the clipboard, the last cross-sheet cases | in flight | M |
| 5 | P5 Router: drag, mid-segment, diff pairs, meanders | 7 | dragging a segment moves its end; no start or end mid-segment; no coupled diff pair shove; no U meander | open | XL (3 parts) |
| 6 | P6 PCB editing details | 8, 15, 17, 35 | Paste Special, rotation step, bounding boxes, a long tail of tool options | open | M, ongoing |
| 7 | P7 Custom-shape pads | 31, 13, 19 | custom pads import as their anchor; zone fill is wrong around them; trapezoid and chamfered pads drawn as rectangles | in flight | L |
| 8 | P8 Files in: Append Board, DXF and SVG, drops | 28 | no way to bring an outline from a DXF or another board in | open | L |
| 9 | P9 Board Setup completion and the rule editor | 3 | Board Editor Layers, Text Variables, Formatting, Tuning and Component Classes pages; regex net patterns | open | L (rule editor XL) |
| 10 | P10 Schematic details | 1, 12, 15, 17, 20 | pins cannot be selected (Swap Pins), no net-collision overlay, no schematic groups, pan and auto-pan | open | M-L |
| 11 | P11 Search and inspection | 21, 29 | Find by Properties, Clearance Resolution, Footprint Checker; Find ignores user fields and zone names | open | M-L |
| 12 | P12 Board outline leftovers | 27 | placement and the DSN boundary ignore cutouts; NPTH holes missing from the 3D slab | open | M |
| 13 | P13 Zone leftovers | 13 | non-copper zones, multi-layer zones, `connect_nearby_polys`, mask-only NPTH knockouts | open | L |
| 14 | P14 3D models of footprints | 30 | one model path in the editor; no model list, no preview, no Migrate 3D Models | open | M |
| 15 | P15 Review workflow leftovers | 11 | ERC exclusions never reach kicad-cli; no Delete Marker / Save report | open | S-M |
| 16 | P16 PCB item kinds the IR lacks | 24 | text box, tables (stackup table), barcode, point, reference image, hatch fill | deferred | L |
| 17 | P17 Library editor leftovers | 19, 22 | no snap to anchors in the editors, no pad array numbering, no derived symbols, no alternate pin functions | open | L |
| 18 | P18 Design blocks | 32 | 23 actions; reusable blocks | after P3, P4 | L |
| 19 | P19 Microwave tools and Specctra | 9, 27 | the five microwave tools; DSN export and SES import | open | S-M |
| 20 | P20 Browser checks for the older areas | 33 | most earlier work was never clicked | standing | M each |
| 21 | P21 Simulator | 34 | `eeschema.Simulation.*`, `Simulator.*` | last | XL |

## Items

"Port from" paths are under the KiCad source root. Each item says what is done (with the merge), what is still missing, and its package.

### 1. Schematic edit tools act on every item kind
**Closed 2026-10-08** (`2a8bf8f`; fields and labels `2d92851`). Hit: every schematic session. What is left is package P10.
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
- Missing: the net-collision overlay of a drag (`sch_drag_net_collision.cpp`); `AutoRotateItem` after a label lands; a text has no justification to flip
  when it is mirrored; a sheet's border and fill; a user-defined field has no place on the sheet; sheet pins cannot be selected one by one; a power symbol has no
  Properties dialog (its Value is edited in Field Properties); symbols this project drew (not imported) and turned or mirrored place their pins by
  `eda_engine::placed` (older note, not re-checked since the overlap fix, `f24fa31`, which made generated symbols draw their pins as KiCad's do).
- Port from: `eeschema/tools/sch_move_tool.cpp`, `sch_edit_tool.cpp`, `sch_selection_tool.cpp`, `sch_drag_net_collision.cpp`, `sch_align_tool.cpp`.

### 2. The KiCad files we write drop design data
**Mostly done (2026-10-08, 10-09).** Hit: any board with a pour, keepout or dimension; every export; every imported board. Blocks: no longer for keepouts. Package P1 for the biggest of what is left.
- Done (`b145f0e`; `crates/kicad/src/{pcb,pcb_items,import_items,import}.rs`): a rule area is written as a keepout zone (before, it failed the whole export with
  `kicad.unknown_net`); every zone writes its own clearance, minimum width, priority, pad connection, thermal gap and spoke, hatch, island and fill settings;
  arc tracks are arcs; locks, dimensions, groups, zone names and teardrop flags are written and read back (round-trip tests in `pcb.rs`). **The board outline
  (`a46fd29`, 10-09)**: Edge.Cuts come back as the items they were (`gr_arc`, `gr_circle`, `gr_rect`, `gr_poly`, `gr_curve`, `gr_line`), a malformed outline stays
  malformed in the export, a footprint's own Edge.Cuts become board-level shapes, KiCad 6's `(gr_arc (start CENTRE) (end START) (angle A))` is read.
- Measured (`REPORT.md`, `scores.json`, 2026-10-08, kicad-cli 10.99 on `qa/data` of the KiCad sources): the re-export of a real board gives kicad-cli the same
  violation counts as the original on **6 of 12** boards (50.0%), up from 4 of 12; all 12 re-exports load.
- Missing: **footprint-local graphics and texts** (`fp_line`, `fp_circle`, `fp_text`) are imported into `drawings.footprint_extras`, which only the in-house DRC reads,
  and never written back (512 `fp_line`s on `complex_hierarchy`: `silk_over_copper` 104 -> 2, `silk_overlap` 199 -> 5) -- item 25; a footprint that had no library is
  written as `eda:<name>`, so a project that turns `lib_footprint_issues` on gets a violation for a library that does not exist; barcodes, component classes and tuning
  profiles are not read (the `api_kitchen_sink`, `component_classes_drc` and `drc_missing_tuning_profile` differences); 189 non-rectangular pad shapes of the QA corpus are
  approximated as rectangles (item 31). `scores.json` was last measured before the outline and footprint work (10-08): re-run `tools/parity_report.py` when P1 lands.
- Port from: `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` and `pcb_io_kicad_sexpr_parser.cpp`.

### 3. Board Setup and the rules
**Mostly done (2026-10-08).** Hit: the start of every project. Blocks: no; the pages that do not exist yet need a model first. Package P9.
- Done (`b145f0e`): `eda_model::rules::RulesOverlay` (`crates/model/src/rules.rs`) holds one optional page per Board Setup page in `design.json` (`drawings.rules`);
  `board::load` lays it over the intent's (or the imported project's) rules, as `design.nets` is, so the router, the gates, the studio and the derived `.kicad_pro`,
  `.kicad_pcb` and `.kicad_dru` that kicad-cli reads all follow an edit. Seven undoable verbs (`set_net_classes`, `set_constraints`, `set_mask_paste`,
  `set_text_graphics_defaults`, `set_stackup`, `set_rule_severities`, `set_custom_rules`; `crates/ops/src/board_setup.rs`) refuse a bad page with the setting's name.
  `components/BoardSetupDialog.tsx` is KiCad's page tree and all ten pages edit: Physical Stackup (a layer's `(color ...)` too, since 10-09), Solder Mask/Paste,
  Defaults, Dimensions, Constraints, Pre-defined Sizes, Teardrops, Net Classes (with the patterns and the nets each matches), Custom Rules (as text, "Check rule syntax"
  through kicad-cli: `POST /api/check_rules`) and Violation Severity (KiCad's 64 checks). Assign Netclass works from the board and the schematic.
- Missing: the Board Editor Layers, Zone Hatch Offsets, Formatting, Text Variables, Length-tuning Patterns, Tuning Profiles, Component Classes and Embedded Files pages
  (nothing in the model holds them); the rule-tree designer (`DRETool.drcRuleEditor`, 82 files in `pcbnew/drc/rule_editor`; the `.kicad_dru` is edited as text);
  KiCad's regular-expression net patterns, bus notation and composite net classes (a pattern here is `*` and `?`, first class wins); dielectric sub-layers and impedance
  control on the stackup; and this app's drawing tools do not start new items from the Text & Graphics defaults yet (they reach the derived `.kicad_pro` only).
- Port from: `pcbnew/dialogs/dialog_board_setup.cpp` and `panel_setup_*.cpp`, `common/dialogs/panel_setup_netclasses.cpp`, `panel_setup_severities.cpp`,
  `panel_text_variables.cpp`, `pcbnew/drc/drc_rule_parser.cpp`, `pcbnew/tools/drc_rule_editor_tool.cpp`.

### 4. Hierarchical sheets
**Editing and the nets across sheets are done (2026-10-08, `08c70c8`); the rest is in flight (`task-sch-hierarchy`).** Hit: every multi-sheet design. Package P4.
- Done: the schematic is derived as one sheet per functional module (`PARITY-sch.md` section 14). The sheet in view is part of every command the Schematic tab
  sends (`Cmd::OnSheet`, `Board.focus` in `crates/ops/src/lib.rs`, `kicad-port/schSheetCmd.ts`), so move, drag, wires, labels, delete, properties, the drawing tools,
  the sheet pin tools and Undo work on a nested sheet; nets are traced across sheets as `connection_graph.cpp` does (`eda_engine::nets::trace_nets_with`, drivers by
  priority, hierarchical labels and sheet pins, global labels and power symbols) and `reconcile_schematic` applies only the difference to `design.nets`
  (`nets::follow`); Reorganize into Module Sheets (`Cmd::ReorganizeSheets`); the Hierarchy panel and Back/Forward/Up; the Symbol Fields Table, BOM preview, symbol
  chooser and Annotate's numbering see every sheet. kicad-cli's ERC on mcu30 as module sheets: no findings.
- In flight (`task-sch-hierarchy`, not merged): Annotate with KiCad's dialog (scope, recursion, order, keep or reset, regroup units, numbering, the messages),
  Find and Replace across the hierarchy in page order, the SVG renderer and the schematic gates walking every sheet.
- Missing: Import Sheet, Draw Sheet from File and the schematic file drop (stale reasons, `STALE-REASONS.md` SR1; placing a design block is the same function);
  hierarchical sheets are not copied or pasted; a screen placed by two sheet symbols is traced once (KiCad makes a net per placement) and buses are traced as plain
  wires; Reorganize into Module Sheets lays the sheets out again and loses the stored places of fields, label spins and looks and body styles; Find on a nested sheet and
  Place Pins from Sheet were not click-tested; no browser check (item 33).
- Port from: `eeschema/sch_sheet_path.cpp`, `sch_screen.cpp`, `connection_graph.cpp`, `tools/sch_navigate_tool.cpp`, `tools/sch_drawing_tools.cpp` (`ImportSheet`,
  `DrawSheet`), `tools/sch_editor_control.cpp`, `dialogs/dialog_annotate.cpp`.

### 5. Access to the installed libraries
**Partly closed (2026-10-08, `80e972d`): the two choosers read KiCad's installed libraries; library tables are in flight (`task-lib-tables`).** Hit: every session that adds a part. Package P3.
- Done: **the Symbol Chooser is KiCad's** (`components/SymbolChooserDialog.tsx`, `components/chooser/`, `kicad-port/libChooser.ts`): a tree of "-- Recently Used --", "-- Already Placed --",
  the project's own libraries and all 222 installed ones, a search scored as `EDA_COMBINED_MATCHER` does, a row per unit, the symbol's drawing, its default footprint and the description pane.
  **The Footprint Chooser** (`components/FootprintChooserDialog.tsx`): the 155 installed footprint libraries with the same search, the drawing, the description, and, for a symbol, "Filter by
  pin count" and "Apply footprint filters". It serves **Place Footprint** (a mounting hole, a fiducial: `Cmd::PlaceFootprint` makes a board part), the Footprint field of Symbol Properties and
  Assign Footprints. **Placing an installed symbol** keeps its definition with the schematic in the same undoable command (`Cmd::EmbedLibSymbol`). Libraries are indexed on demand by one pass over
  their text and cached (`crates/kicad/src/{symbol_scan,footprint_scan}.rs`, `crates/cli/src/library_search.rs`); the Footprint and Symbol editors' trees list every installed library.
- In flight: a `LibSource` (a folder, or the resolved tables: KiCad's global table with the libraries it chains, then the board's own `sym-lib-table` and `fp-lib-table`, a nickname defined
  twice being the project's) behind `library_index`, `library_search`, `library_api`, `library_place` and `body_api`, with the file format, path variables and both table scopes
  (`crates/kicad/src/library_table.rs`, `crates/cli/src/library_tables.rs`). No dialog, no Add Library or New Library yet.
- Missing: the library table dialogs, Add Library, New Library, the library drop and Configure Paths (six rows of `ui-parity-missing.json` that go stale when the branch merges); KiCad's
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
**Partial.** Shove near pads, vias and tracks, the board edge, every setting and the via drag are done (2026-10-08, `1e4d7ea`, `03b46de`). Hit: dragging, a route that starts or ends mid-segment, differential pairs with obstacles. Blocks: partly. Package P5.
- Exists: `crates/pns` ports hulls, `LINE::Walkaround`, shove of tracks and vias (a stack of lines with ranks, `onCollidingSolid`, `pushOrShoveVia`, the optimizer over every shoved line),
  the optimizer with smart pads, loop removal, diff-pair routing, single-track and diff-pair length and skew tuning (dialog-driven), `D`/`G` drag of a corner or via; the clearance gate measures
  pads by their exact outline (`eda_drc::board::placed_pad_copper`); the board outline with its cutouts, arcs and a footprint's own Edge.Cuts are obstacles with the copper-to-edge clearance
  (`PNS_KICAD_IFACE_BASE::syncGraphicalItem`, `crates/pns/src/from_ir.rs`); every `RoutingSettings` field is read and Interactive Router Settings has `dialog_pns_settings.cpp`'s rows
  (`crates/pns/PARITY.md`; `CODE-COMPARE-router.md` D1-D6, D8, D10, D12, D15, D16 fixed).
- Missing (`CODE-COMPARE-router.md` D7, D18): a drag moves the nearer end instead of sliding the segment (`DM_SEGMENT`), at any angle (`dragCorner45`), with the `SHP_REVERSED` direction test and a
  last-valid-point restore; a route cannot start or end mid-segment (`SplitAdjacentSegments`) and Route From Other End works only before the first fix; a diff pair has no coupled shove or
  walkaround and no via; length tuning is a dialog on straight axis-aligned tracks that builds a 45-degree accordion, not KiCad's U meander (`meander.rs`); no arcs (`ARC_T`), mouse-trail posture
  (the dialog's "Use mouse path to set track posture", "Smooth dragged segments" and "Optimize entire track being dragged" rows are disabled), springback or `ShoveTimeLimit`; a head that ends in a
  via is not shoved with its via; `findRedundantSegment` and locked joints (`from_ir` never sets `Segment.locked`); the generators (`PCB_GENERATOR`, 9 rows) that keep a tuning pattern editable.
  No browser check (item 33).
- Port from: `pcbnew/router/` (`pns_dragger.cpp`, `pns_multi_dragger.cpp`, `pns_line_placer.cpp`, `pns_diff_pair_placer.cpp`, `pns_meander*.cpp`, `pns_shove.cpp`, `pns_walkaround.cpp`,
  `router_tool.cpp`), `pcbnew/generators/pcb_tuning_pattern.cpp`, `pcbnew/tools/generator_tool.cpp`.

### 8. PCB edit tools
**Mostly done (2026-10-08, `88c8c0a`).** Hit: every layout session. Blocks: no. Package P6 for what is left.
- Done (`PARITY-pcb.md` section 22): `move_items`, `rotate_items` and `flip_items` (`crates/ops/src/pcb_transform.rs`) move, turn and flip footprints, tracks (arcs stay arcs), vias, zones, graphics, text,
  dimensions and groups as KiCad's item classes do, one undo step each, and reach the `.kicad_pcb` kicad-cli reads. Move, drag, Move Individually, Move with Reference, Position Relative, Move Exactly,
  Rotate, Flip, Align and Distribute work for every kind with `FilterCollectorForLockedItems` and KiCad's rotation and flip points; `R` turns counter-clockwise. Duplicate, Copy, Cut and Paste take footprints
  (a copy is a `BoardPart`), dimensions and groups, in KiCad's clipboard format (`crates/kicad/src/clipboard.rs`, `Cmd::PasteClipboard`, `POST /api/clipboard/copy`). Pads are selectable (click, box, Selection
  Filter, highlight, the Properties pane); the pad edits are item 9's and are done.
- Missing: Paste Special on the PCB (`DIALOG_PASTE_SPECIAL`: annotation modes, clear nets; the action answers with a toast, and plain Paste renumbers a taken reference where KiCad's default keeps it);
  the rotation step and flip direction settings (`m_RotationAngle`, the flip direction; both planners and verbs take them); the anchor half of `BestSnapAnchor` for a Rotate or Flip reference point and a
  footprint's own bounding box (`FOOTPRINT::GetBoundingBox( false )`; the courtyard stands in); a footprint with copper routed to its pads still clears the routing when edited; Duplicate and Paste are a
  step of their own and the drop a second one; a copy keeps the original's nets, so `--strict` refuses moving it away.
- Port from: `pcbnew/tools/edit_tool.cpp`, `edit_tool_move_fct.cpp`, `align_distribute_tool.cpp`, `pcb_selection_tool.cpp`, `pcb_control.cpp` (`Paste`, `placeBoardItems`), `common/dialogs/dialog_paste_special.cpp`, `pcbnew/kicad_clipboard.cpp`.

### 9. Footprints on the board are editable objects
**Mostly done (2026-10-09, `afcb91b`).** Hit: every board (silkscreen cleanup, mounting holes). Blocks: no longer. Package P2 and P1 for what is left.
- Done (`PARITY-pcb.md` section 26; `crates/model/src/fp_edit.rs`, `crates/ops/src/fp_edit.rs`, `crates/kicad/src/fp_fields.rs`, `kicad-port/fpFields.ts`, `components/{FootprintPropertiesDialog,BoardPadPropertiesDialog}.tsx`): a
  footprint's Reference and Value are fields with a position, size, thickness, layer, visibility, rotation, justification, mirror, bold, italic, keep upright and knockout; user fields can be added, renamed
  and deleted; the attributes (SMD, through hole or unspecified, not in schematic, exclude from position files, exclude from BOM, do not populate, exempt from the courtyard requirement) are per instance,
  and do not populate and exclude from BOM stay in step with the schematic symbol; pads carry their own type, shape, size, offset, rotation, corner radius, hole and mask and paste margins. It is
  `DrawingsSection::footprint_edits` (additive), three undoable verbs (`edit_board_footprint`, `edit_board_field`, `edit_board_pad`), written as `property` / `attr` / pad extras that kicad-cli loads and
  whose position and BOM exports honour. A field is an item of the board (`REF:Name`): selected, drawn (following the Appearance rows), moved, dragged, turned, flipped. A pad's and a footprint's zone
  connection, relief gap, spoke width and angle and clearance are the zone filler's overlay (`drawings.zone_overrides`) and the same dialogs edit them. Board-only footprints are the "not in schematic" attribute.
- Measured: 69 new Rust tests and 39 `node --test` cases; `web/studio/e2e/footprint-edit.check.js` runs 19 scenarios through `window.__eda`; real kicad-cli honoured do-not-populate and exclude-from-position-files.
- Missing: Change Footprint(s), Update Footprints from Library that reads the library, geographical reannotate -- now item 26; per-footprint mask and paste margin overrides; trapezoid, chamfered and custom
  pads, padstacks (item 31), fabrication property, pad number and pin function edits, pad-to-die length; Datasheet and Description fields and text variables in field text; our own placement and courtyard
  checks do not read a pad's offset or margins (kicad-cli does); logos; the microwave tools (their reason, "no free footprint", is stale: `STALE-REASONS.md` SR3).
- Port from: `pcbnew/dialogs/dialog_footprint_properties.cpp`, `dialog_pad_properties.cpp`, `pcbnew/pcb_field.cpp`, `pcbnew/footprint.cpp`, `pcbnew/tools/board_editor_control.cpp` (`PlaceFootprint`).

### 10. The Properties panel
**Mostly done (2026-10-08, `bd712d3`; footprint fields, attributes and pads 10-09).** Hit: constantly. Blocks: no.
- Done (`kicad-port/{propertyManager,propertyGrid,pcbProperties,schItemProperties}.ts`, `components/panels/{PropertyGrid,PropertiesPanel}.tsx`; `PARITY-pcb.md` section 23, `PARITY-sch.md` section 17): the pane is KiCad's
  property grid. `PROPERTY_MANAGER` is ported (class registry, `InheritsAfter` / `Mask` / `ReplaceProperty` / `OverrideAvailability`, the ordering walk), so each class lists what KiCad's registration lists, in
  KiCad's order, on the board and on the sheet; a selection shows the rows every item has, the value they share or `<...>`, and an edit goes out as ONE `batch` (one undo step). Every edit is an existing verb
  except four added where none fit (`crates/ops/src/pcb_props.rs`: `edit_track`, `set_item_net`, `set_zone_name`, `replace_shape`). Footprint fields, attributes and pad rows edit since item 9.
- Measured: 87 `node --test` cases, 9 Rust tests for the four verbs; `web/studio/e2e/properties-panel.check.js` edits every editable row of every kind and undoes it.
- Missing: what the IR has no place for is not registered: a pad's trapezoid, chamfered and custom shapes, padstacks, via tenting and backdrill, board text's fonts, bold, italic, vertical justification and
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
**Done (2026-10-09, `2d92851`)**, limits below. Hit: every schematic cleanup. Blocks: no. Package P10 for the limits.
- Done (`PARITY-sch.md` sections 1, 15): the Reference, Value, Footprint and Datasheet of every symbol, the Value of a power symbol and the name and file of a sheet each have a position, orientation,
  justification and visibility of their own (`SchematicSection::field_layout`, in the item's own frame), placed by a port of Autoplace Fields (`eeschema/autoplace_fields.cpp`, `crates/engine/src/fields.rs`)
  and written to the `.kicad_sch`. Fields are items: picked, moved (`M`, a drag), turned, mirrored, hidden with Delete and edited in Field Properties; `O` runs Autoplace Fields; a turn re-autoplaces
  (`SchExtras::fields_autoplaced`). A label's spin, size, bold and italic are stored and edited. A placed symbol in the alternate ("De Morgan") body style is drawn, written and read back (`LibSymbol::alternate`,
  `SchExtras::body_styles`, Cycle Body Style). 44 Rust tests, 17 `node --test` cases.
- Missing: a field's font face and colour, a free text's justification, a label's own fields (netclass, intersheet references) and a user-defined field's place on the sheet have no place in the IR; the
  fields of a symbol read from a KiCad file are not items; `m_AutoplaceFields.enable` is always on; Reorganize into Module Sheets loses the stored places of fields, label spins and looks, and body
  styles; only the normal and the De Morgan body style are modelled; pins cannot be selected (Swap Pins, pin-level highlight).
- Port from: `eeschema/sch_field.cpp`, `sch_label.cpp`, `dialogs/dialog_field_properties.cpp`, `dialog_label_properties.cpp`, `dialog_symbol_properties.cpp`.

### 13. Zones: fill fidelity and settings
**Partial; fill fidelity improved 2026-10-08/09 (`eabaf91`, `a46fd29`).** Hit: most boards. Blocks: no. Packages P13, and P7 for custom pads. Of 208 fills on 17 KiCad QA boards, 190 are within 1% of kicad-cli's area (105 before), 182 within 0.5%; the notch row of `issue11814` went from 23.4% to 0.2% with the outline work.
- Exists: the filler (`crates/zone-filler`, `crates/drc/src/fill.rs`, `GET /api/fill`) follows `ZONE_FILLER::Fill`: zones fill from the highest priority down and are knocked out by each other's fills, islands go by
  connectivity, iterative refill, thermal spokes (`buildThermalSpokes`: turn with the pad, the pad's angle, width and gap, only the ones that reach copper stay), hatch fill with thermal rings, chamfer and
  fillet corner smoothing, the board-edge clearance from every Edge.Cuts item, and pad and footprint clearance overrides. A pad's or footprint's own connection, relief gap, spoke width, spoke angle and
  clearance beat the zone's (Footprint Properties and Pad Properties; read from and written to `.kicad_pcb` and `.kicad_mod`). The full settings dialog (`components/ZoneDialog.tsx`), keepout knockouts,
  Fill and Unfill Selected, Merge, Duplicate onto Layer, the Priority actions and the Zone Manager (`PARITY-boardctl.md`). Measured in `crates/zone-filler/PARITY.md`.
- Missing: custom pad shapes (the importer keeps the anchor: the 16 custom pads of `issue5093` leave 766% on one of its zones and the ring pads of `stonehenge` 11%; the largest gap left; item 31); the thermal
  rings of a hatched zone (0.6 to 0.9% XOR); `connect_nearby_polys`; mask-only NPTH holes (`issue14559`'s plugs), courtyards and net ties as knockouts; one layer per zone and no non-copper zones
  (`fillNonCopperZone`); no zone lock in Zone Properties (the name is edited in the Properties panel); no Auto-Assign Priorities; the Zone Manager has no preview; a hatched zone with a fine pitch over a whole
  board is slow (minutes in a debug build on `issue5093`). Appendix C has the function table.
- Port from: `pcbnew/zone_filler.cpp`, `zone.cpp`, `zone_manager/`, `dialogs/panel_zone_properties.cpp`, `dialogs/dialog_non_copper_zones_properties.cpp`, `pcbnew/teardrop/`.

### 14. Snap, grid and the click-versus-drag rule
**Closed 2026-10-09 for the board, the schematic and the click rule (`358606b`); the footprint and symbol editors snap to the grid only.** Hit: every drag. Blocks: no.
- Done: the click rule (`kicad-port/dragThreshold.ts`: a drag after more than 8 px along an axis, or on macOS a motion after 300 ms held, in all four editors); the board's `PCB_GRID_HELPER::BestSnapAnchor`
  with hysteresis, snap lines, construction geometry and intersections (`kicad-port/{snapGeom,snapScene,snapAnchors,constructionManager,gridHelperBase,pcbGridHelper}.ts`, 67 tests); grid overrides per category
  (`Toggle Grid Overrides`, `Ctrl+Shift+G`); the schematic's grid list, Next / Previous Grid, fast grids, Edit Grids and `EE_GRID_HELPER::BestSnapAnchor` (`kicad-port/schGridHelper.ts`); Magnetic Points in
  Preferences. `e2e/snap-grid.check.js` runs 43 checks. 1521 unit tests pass.
- Left: the footprint and symbol editors snap to the grid of the category, not to other items' anchors (`PcbGridHelper` / `SchGridHelper` are not wired there) and were not click-tested; with the default
  Magnetic Points ("In Track Tool") a move or a shape does not snap to pads or tracks until they are set to Always (KiCad's default too); the junction tool keeps its own 12 px snap; the schematic context menu
  has no Zoom and Grid submenus; the server still snaps a part's placement to 100 um (`meta.snap_um`) whatever grid is shown; the studio never moves the real pointer, so a Move computes the cursor
  KiCad's warp would give (`kicad-port/heldCursor.ts`).
- Port from: `pcbnew/tools/pcb_grid_helper.cpp`, `common/tool/grid_helper.cpp`, `eeschema/tools/ee_grid_helper.cpp`, `common/tool/tool_dispatcher.cpp`, `common/tool/construction_manager.cpp`.

### 15. Interaction details that differ
**Partial.** Hit: constantly, one detail at a time. Package P6 (PCB) and P10 (schematic).
- 42 of `CODE-COMPARE-ui.md`'s 266 rows were marked divergent (some since fixed), and about 310 handlers added since have not been compared. Fixed since 2026-10-07: High Contrast cycles in three states
  (`6611ade`) and `toggleLastNetHighlight` is wired. Carried forward from 2026-10-07 and not re-checked: Enter during a PCB route finishes it instead of clicking (`common.Control.cursorClick`); Route From Other End
  only before the first fix; `V` does nothing outside a route; Select Connection needs a selection (PCB and schematic); schematic net highlight and Select Node miss pins; Ctrl+U never returns to mils;
  Alt+` (`toggleNetHighlight`) is selection-driven; Mirror X then Y collapses to one flag; Add Corner inserts the cursor point instead of the nearest point on the edge.

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
- Missing: items drawn or pasted while a group is entered do not join it (`BOARD_COMMIT::Push`); Create Array skips groups (item 22); no groups in the schematic or the footprint editor.
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
**Partial.** Hit: library work. Blocks: no. Packages P14, P7 and P17.
- Exists: both editors with library trees, pad and pin tools, dialogs, tables, import and export (`PARITY-fpedit.md`, `PARITY-symedit.md`).
- Missing: the footprint editor's 3D model panel (it edits one model path while a footprint on a board keeps every `(model ...)`: item 30); custom-shape pads (item 31); the library editor's Footprint
  Properties has no fields grid (the board's has one since 10-09); derived symbols; alternate pin functions; symbol fields on the canvas; a shape properties dialog; snapping to other items' anchors in both
  editors (item 14). (The pad clearance and thermal overrides are read by the zone filler since 10-08; the note that no check reads them is gone.)
- Port from: `pcbnew/dialogs/panel_fp_properties_3d_model.cpp`, `dialog_pad_properties.cpp`, `eeschema/symbol_editor/`.

### 20. Schematic view controls
**Partial, unchanged.** Hit: constantly. Blocks: no. Package P10.
- The PCB canvas pans with the middle or right button and auto-pans at the edge (opt-in, as in KiCad; `Canvas.tsx`). The footprint and symbol canvases pan with either button but never auto-pan. The schematic
  canvas pans with the middle button only (`components/SchematicView.tsx` handles button 1) and does not auto-pan while drawing a wire. The native pinch gesture is not ported on any canvas
  (`PARITY-pcb.md` section 1).
- Port from: `common/view/wx_view_controls.cpp`.

### 21. Find on the PCB
**Mostly done (2026-10-08/09, `4c041a6`).** Hit: often on big boards. Blocks: no. Package P11.
- Done (`PARITY-pcb.md` section 7a): Find (Ctrl+F), Find Next (F3) and Find Previous (Shift+F3) on the board from `DIALOG_FIND`: history, Match case, Whole words only, Wildcards, Wrap, footprint references, values,
  other texts, DRC markers and net names, in KiCad's order, the hit selected and the view brought to it (`kicad-port/pcbFind.ts`, `components/PcbFindDialog.tsx`).
- Missing: Find by Properties (stale reason: the property registry exists; Properties tab M, Query tab M more; `STALE-REASONS.md` SR2); **Find does not follow the IR that landed after it**: user fields and hidden
  fields ("Include hidden fields" does nothing) and zone names (`Zone.name` exists; `pcbFind.ts` still says a zone has no name) are not searched; the dialog's "Show search panel" link
  (`common.Interactive.search` still jumps to the Activity tab). Find and Replace is the schematic's only, as in KiCad.
- Port from: `pcbnew/dialogs/dialog_find.cpp`, `dialog_find_by_properties.cpp`.

### 22. Arrays of footprints
**Mostly done (2026-10-09, `afcb91b`).** Hit: connector rows, LED grids. Blocks: no.
- Done (`PARITY-pcb.md` section 17; `crates/ops/src/array.rs`, `kicad-port/arrayOptions.ts`, `components/CreateArrayDialog.tsx`): Create Array makes grid and circular arrays of footprints, tracks, vias, zones,
  graphics, text, dimensions and groups, as copies or as an arrangement of the selected items, in one undo step; KiCad's tabs, boxes and wording, stagger, skew, centring, Full circle, "Rotate items", reference
  designators ("Assign unique" via `ReannotateDuplicates`, or keep). 23 Rust tests, 17 `node --test` cases, the browser check.
- Missing: the footprint editor's array of pads and its numbering (`ARRAY_AXIS`: numerals, hexadecimal, alphabets, 2D coordinates, `ARRAY_PAD_NUMBER_PROVIDER`; package P17); the original keeps the first point
  where KiCad's reverse loop leaves it on the last (same points, same references); the circle's centre starts at the selection, where KiCad's dialog ignores the origin it is given.
- Port from: `pcbnew/tools/array_tool.cpp`, `pcbnew/dialogs/dialog_create_array.cpp`, `pcbnew/array_pad_number_provider.cpp`.

### 23. The 3D viewer
**Done (2026-10-09/10, `89ff669`)**, limits below. Hit: every board review. Blocks: no.
- Done (`PARITY-3d.md` sections 1, 3, 5, 6, 7): each part is its own KiCad 3D model, fetched once per distinct model (`GET /api/3dmodel`, `crates/cli/src/model3d_api.rs`), read with three's `VRMLLoader` and placed by
  `render_3d_opengl.cpp`'s chain (`kicad-port/model3d.ts`); the route resolves a footprint's `(model ...)` path as `FILENAME_RESOLVER` does, serves it read-only and converts a STEP once through
  `kicad-cli pcb export vrml` into a cache. Time to the first real model on mcu30 (30 parts) and a 93-part QA board: 2.7 to 4.4 s from the request, from 10 to 50 s with the GLB export (now the optional "Exact
  export"). The Appearance manager is `appearance_controls_3D.cpp`'s layer tree (`kicad-port/appearance3d.ts`); stackup colours come from Board Setup (`StackupLayer.color`); hover highlight; animated view
  moves. The board slab and solder mask are extruded from the outline KiCad builds from Edge.Cuts. `e2e/viewer3d.check.js` (50 checks). Two stalls of the request loop were found by measuring and fixed.
- Missing: raytracing (out of scope); layer presets and viewports of the 3D viewer; the PCB editor's selection drawn green in the 3D view; the 3D Navigator; values, footprint text, off-board silkscreen, models not
  in the position file and DNP models; X3D and IGES models; per-layer ambient and specular colours of the board; the preferences that set the animation speed; NPTH holes are not cut out of the slab
  (`aIncludeNPTHAsOutlines`, item 27); the footprint editor's 3D model panel (item 30).

### 24. Item kinds the IR does not have
**Open, deferred.** Backlog; package P16 for the PCB items.
- Tables (PCB and schematic, 18 rows of `ui-parity-missing.json`), PCB text boxes, barcodes, points, reference images, hatch fills, schematic images, the symbol text box, derived symbols, alternate pin
  functions, design variants, generators, padstacks, component classes, tuning profiles, embedded files (blocked by the no-new-dependency rule: KiCad uses zstd), each with its reason in
  `ui-parity-missing.json`. Add one when a work package needs it. Custom-shape pads left this list: they are item 31.

### 25. Footprints on the board carry no graphics
**New, open.** Hit: every board. Blocks: yes, for the silkscreen and assembly output and for the look of the board. Package P1, size L.
- What is missing, read from the code: the engine `Footprint` (`crates/model/src/footprint.rs`) holds pads, courtyard and `models3d`. `LibraryFootprint::to_engine_footprint` (`crates/model/src/ir.rs`) says in its
  own comment that text, fields and attributes "stay editor-only (the engine has no use for silkscreen art...)", and the graphics of a library footprint are dropped with them. `write_footprint`
  (`crates/kicad/src/pcb.rs`) writes pads, the courtyard (`fp_rect` / `fp_poly` on `F.CrtYd`), the fields and the attributes -- no `fp_line`, `fp_arc`, `fp_circle`, `fp_poly` or `fp_text` on silk or fab. The
  canvas (`components/canvas/painter.ts`, `drawFootprint`) draws courtyard, pads and fields. So a board made here has no component outlines, no pin-1 marks and no F.Fab bodies on screen, in the file, in the
  Gerber silkscreen kicad-cli plots from it, or in the 3D viewer's silk; and our silk and mask checks have nothing of a part to judge.
- Imported boards: the importer keeps a footprint's silk and mask graphics and texts in `drawings.footprint_extras` (board space, `FootprintExtra.graphics`, `texts`), read only by the in-house silk and mask DRC
  and never written back: on `complex_hierarchy` (512 `fp_line`s) kicad-cli's `silk_over_copper` goes from 104 to 2 and `silk_overlap` from 199 to 5 after our export (`REPORT.md`, item 2).
- To do: graphics and texts as part of the placed footprint, in its frame (so they move, turn and flip with it), read from `.kicad_mod` and from boards (`fp_line`, `fp_arc`, `fp_circle`, `fp_rect`, `fp_poly`,
  `fp_curve`, `fp_text`), written by the board writer (a bottom-side footprint in KiCad's mirrored frame), drawn by the painter under the Appearance rows, hit-tested, drawn by the 3D scene, read by the
  silk and mask checks; `footprint_extras` retired; a kicad-cli silk plot equal before and after a round trip is the test (the pattern of `a46fd29`).
- Port from: `pcbnew/footprint.cpp`, `pcbnew/pcb_shape.cpp`, `pcbnew/pcb_text.cpp`, `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` (`format( FOOTPRINT )`), `pcb_io_kicad_sexpr_parser.cpp`
  (`parseFOOTPRINT_unchecked`), `pcbnew/pcb_painter.cpp` (`draw( FOOTPRINT )`).

### 26. Footprint assignment, exchange and reference rename
**New, open.** Hit: every project that changes a footprint or renumbers a part. Blocks: yes. Package P2, size L.
- What is missing, read from the code: a part's footprint comes from the intent (`Part.footprint`); `board::fold_unknown_symbols` copies a schematic symbol's Footprint field only for a symbol the model has
  not heard of. So `Cmd::EditSymbolFields { footprint }` (Symbol Properties, Assign Footprints, the Symbol Fields Table) edits the schematic field and nothing on the board, and Update PCB from Schematic (F8,
  `kicad-port/updatePcb.ts`) is a report that says the board is up to date. `Cmd::RenameSymbol`'s own doc says it does not retarget `design.placement`, so Edit Reference (`U`), Annotate with reset and Increment
  Annotations leave the old reference's footprint, pose, routing, fields, edits, zone overrides, locks and group membership where they were and the next reconcile makes a fresh unplaced part.
  "Update Footprints from Library" (`actions/pcbGlobalEditSweep.ts`) only publishes the project-library entry a footprint uses (`Cmd::UpdateFootprintOnBoard`); it never reads the installed library or a
  table. Insert footprint into PCB updates the parts that already name the footprint (`PARITY-fpedit.md`); `Cmd::PlaceFootprint` could add a new one.
- To do (in this order): (1) a rename that carries the board's part with it, as one undo step; (2) a per-instance footprint assignment overlay in `DrawingsSection` (beside `footprint_edits`, applied by `board::load`
  as `design.nets` and `rules` are), followed by the schematic's Footprint field (`replaceFootprint`); (3) Change Footprint(s) with the dialog's options (update or reset fields, text layers, 3D models; pads kept
  by number); (4) Update Footprints from Library that reads the library copy and lists what differs (`FootprintNeedsUpdate`); (5) Compare Footprint with Library; (6) Reannotate (geographical); (7) Insert
  footprint into PCB adds a part. Swap Pad Nets and Swap Gate Nets are decided here: a swap on the board makes the board disagree with the schematic drawing, as in KiCad (`--schematic-parity` reports it).
- Port from: `pcbnew/dialogs/dialog_exchange_footprints.cpp`, `pcbnew/pcb_edit_frame.cpp` (`ExchangeFootprint`), `pcbnew/netlist_reader/board_netlist_updater.cpp` (`replaceFootprint`,
  `updateFootprintParameters`, `updateComponentPadConnections`), `pcbnew/tools/global_edit_tool.cpp` (`ExchangeFootprints`), `pcbnew/tools/board_reannotate_tool.cpp`,
  `pcbnew/dialogs/dialog_board_reannotate.cpp`, `pcbnew/footprint.cpp` (`FootprintNeedsUpdate`), `pcbnew/tools/board_inspection_tool.cpp` (`DiffFootprint`), `eeschema/tools/assign_footprints.cpp`,
  `eeschema/tools/backannotate.cpp`, `pcbnew/tools/edit_tool_move_fct.cpp` (`SwapPadNets`).

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
**New, open.** Hit: sometimes; a DXF outline is common. Blocks: yes for DXF and SVG (no workaround but redrawing). Package P8, size L.
- Append Board and its drop, the Footprint Editor's drop, Import Sheet, Draw Sheet from File and the schematic drop have stale reasons: `Cmd::PasteClipboard`, `BoardPart`, `Cmd::AddSheet`, `Cmd::PasteSch` and
  `pickTextFile` exist (`STALE-REASONS.md` SR1, size M).
- Import Graphics (DXF, SVG) for the board, the schematic and the symbol editor, and its three drops, do not exist; the rule is no new dependency, but a DXF is text and an SVG path is a short grammar
  (KiCad uses libdxfrw and nanosvg). Import Netlist (`BoardPart` makes the parts and `pad_nets` the nets; low value next to item 26).
- Port from: `common/import_gfx/{dxf_import_plugin,svg_import_plugin,graphics_importer,graphics_importer_buffer}.cpp`, `pcbnew/import_gfx/{dialog_import_graphics,graphics_importer_pcbnew}.cpp`,
  `eeschema/import_gfx/`, `pcbnew/tools/pcb_control.cpp` (`AppendBoard`), `board_editor_control.cpp` (`ImportNetlist`), `eeschema/tools/sch_drawing_tools.cpp` (`ImportSheet`).

### 29. Search and inspection tools
**New, open.** Hit: occasionally; Clearance Resolution is how a DRC error is understood. Blocks: no. Package P11, size M-L.
- Find by Properties (the registry exists), Clearance Resolution and Constraints Resolution (`crates/drc/src/constraints.rs` is a partial `EvalRules` the router and the zone filler already use), the Footprint
  Checker (a scratch board with one footprint through `kicad-cli pcb drc`, filtered), Find following the new IR (item 21), the DRC Rule Editor (item 3). `STALE-REASONS.md` SR2.
- Port from: `pcbnew/dialogs/dialog_find_by_properties.cpp`, `pcbnew/pcbexpr_evaluator.cpp`, `pcbnew/tools/board_inspection_tool.cpp` (`InspectClearance`, `InspectConstraints`),
  `pcbnew/drc/drc_engine.cpp` (`EvalRules`), `pcbnew/dialogs/dialog_footprint_checker.cpp`.

### 30. The 3D models of a footprint
**New, open.** Hit: whoever makes a footprint or fits a different 3D model. Blocks: no. Package P14, size M. From `PARITY-3d.md` and `PARITY-fpedit.md`.
- A footprint on a board and in the installed libraries keeps every `(model ...)` with its offset, scale, rotation and opacity (`Footprint.models3d`, `89ff669`), and the 3D viewer, the importer and the writer
  use them. Nothing edits them: the library editor's Footprint Properties has one text field for a path (`LibraryFootprint.model`, a single `Option<String>`), `FootprintEdit` has no model list, and there is
  no 3D Models tab, no preview, no model chooser in either dialog. Migrate 3D Models has a stale reason (`STALE-REASONS.md` SR3) and needs the same verb. Collect and Embed 3D Models needs embedded files
  (zstd, item 24).
- Port from: `pcbnew/dialogs/panel_fp_properties_3d_model.cpp`, `dialog_footprint_properties.cpp`, `dialog_footprint_properties_fp_editor.cpp`, `dialog_migrate_3d_models.cpp`, `3d-viewer/3d_cache/`.

### 31. Custom-shape pads
**New (split out of items 9, 13 and 19), in flight: `task-custom-pads` has no commit yet.** Hit: footprints with custom pads, imported boards, the zone fill around them. Blocks: no. Package P7, size L.
- The footprint IR models six pad shapes. A custom pad imports as its anchor, a trapezoid or chamfered pad is drawn as a rectangle (189 non-rectangular pads of the QA corpus are approximated as rectangles,
  `REPORT.md`), so the knockout, clearance and spokes of the real shape are missing from the zone filler (766% on one zone of `issue5093`, 11% on `stonehenge`), the router and our checks. Four rows wait for it:
  Edit Pad as Graphic Shapes, Finish Pad Edit, the microwave arc stub and function shape.
- Port from: `pcbnew/pad.cpp`, `pcbnew/tools/pad_tool.cpp` (`EditPad`), `pcbnew/dialogs/dialog_pad_properties.cpp`, `pcbnew/zone_filler.cpp` (`addKnockout`, custom pad parts), `pcbnew/microwave/microwave_polygon.cpp`.

### 32. Design blocks
**New, open; after item 5 (tables) and Import Sheet (item 28).** Hit: reuse of sub-circuits. Blocks: no. Package P18, size L.
- 23 rows: the PCB and schematic panels, place (schematic: the same function as Import Sheet), place linked, save selection, sheet or board as a block, update, properties, delete, the block library table.
  No code names a design block. The schematic half's "no copy and paste" reason is stale.
- Port from: `eeschema/tools/sch_design_block_control.cpp`, `pcbnew/tools/pcb_design_block_control.cpp`, `common/tool/design_block_control.cpp`, `common/design_block{,_info,_io,_library_adapter,_tree_model_adapter}.cpp`,
  `common/widgets/{design_block_pane,panel_design_block_chooser}.cpp`, `common/dialogs/{dialog_design_block_properties,panel_design_block_lib_table}.cpp`, `eeschema/sch_design_block_utils.cpp`, `pcbnew/pcb_design_block_utils.cpp`.

### 33. Browser-check coverage
**New, standing.** Package P20.
- `web/studio/e2e/*.check.js` exist for the Appearance panel, the DRC and ERC review, footprint edit and arrays, the Properties panel, snap and grid, and the 3D viewer. Not covered: schematic editing and the clipboard
  (items 1, 6, 12), the hierarchy (4), Board Setup (3), the library choosers (5), the router (7), PCB move, rotate, flip and clipboard (8), zones (13), groups (17), Find (21), the context menu (16). The rule in
  force: a fix counts when the C++ function is named in the code, the logic has a `kicad-port/*.test.ts`, one user action is one undo step, and a browser check ran.

### 34. The simulator
**New (it was Appendix B), in scope and last.** Package P21, size XL.
- 29 rows: `eeschema.Simulation.*` (11), `eeschema.Simulator.*` (9), the OP voltages and currents and the simulator window (3), and the plot's zoom history and axes (6, `common.Control`). ngspice 45.2 is installed; the
  plan is `kicad-cli sch export netlist --format spice` and `ngspice -b`, the raw output read back. Port from: `eeschema/sim/`, `eeschema/tools/simulator_control.cpp`, `eeschema/dialogs/dialog_sim_*`.

### 35. The long tail of the PCB tools
**New, open.** Hit: rarely each. Package P6, with P13 for zones. From the "not ported" rows of `PARITY-pcb.md`.
- Cleanup Tracks & Vias: the net, net class, layer and "selected items only" filters, "Refill zones before and after cleanup" (section 12). Global edits of tracks and vias: filter by net class or exact width,
  through / micro / blind / buried via types, annular-ring and IPC-4761 presets; of text and graphics: footprint reference, value and other-field scope, dimension items, bold, italic, font, keep upright, "center
  text on footprint", "Set to layer default values" (section 13). Rule areas: multi-layer rule areas, `(disallow ...)` rules on non-keepout items, `DRCE_TEXT_ON_EDGECUTS` (section 14). Teardrops: curved edges,
  rectangular and non-circular pad anchors, track-to-track, two-segment borrowing, `m_TdOnPadsInZones`, incremental updates (section 15). Dimensions: a leader's text border, manual text position,
  Rotate/Flip of a dimension (section 18). The Draw Zone Fill Fracture Borders / Triangulation debug modes draw strokes only (the triangulation is an earcut port without KiCad's Z-order hash); Flip Board View mirrors text on
  non-side-specific layers with the board where KiCad keeps it readable; Automatically select track width does not turn off when a width is picked in the toolbar and the first width key while routing
  does not set `m_TempOverrideTrackWidth` (`PARITY-boardctl.md`).

## Appendix A. The original 30, one verdict each

12 closed, 10 partial, 0 open, 8 out of scope (2026-10-10). Changed since 2026-10-07: #11 and #25 closed; #6 and #26 re-worded.

| # | Gap | Verdict | Evidence, or where it went |
|---|---|---|---|
| 1 | Schematic editing | **Closed** | 168 eeschema handlers (`actions/useActionRunner.ts`, `schEditActions.ts`), `components/SchematicView.tsx`, verbs in `crates/ops/src/{lib.rs,sch_edit.rs,sch_move.rs}`, one-netlist rule in `crates/cli/src/board.rs::reconcile_schematic` (test `schematic_wire_connects_and_disconnects_pins_on_one_netlist`). Behaviour: items 1, 6, 12 are closed. |
| 2 | DRC engine hangs | Out of scope | kicad-cli is the only engine (`docs/ARCHITECTURE.md`); the Rust copy is deleted. |
| 3 | DRC precision | Out of scope | same |
| 4 | ERC has no UI | **Closed** | `components/ErcDialog.tsx`, `GET /api/erc` to `crates/kicad-engine/src/lib.rs::erc`, exclusions `Cmd::AddErcExclusion`, pin map `SchematicSetupDialog.tsx`. Review workflow: item 11. |
| 5 | Zone fill | Partial | `crates/zone-filler`, `crates/drc/src/fill.rs`, `ZoneDialog.tsx`, `ZoneManagerDialog.tsx`; 190 of 208 fills within 1% of kicad-cli's area; items 13 and 31. |
| 6 | Hierarchical sheets | Partial | editing on every sheet and nets traced across sheets (`08c70c8`: `Cmd::OnSheet`, `eda_engine::nets::trace_nets_with`), panel, navigation (`HierarchyPanel.tsx`); the rest is item 4 (in flight). |
| 7 | Routing | Partial | `crates/pns`; item 7. |
| 8 | Footprint editor and pad tool | Partial | editor, pad tools, dialogs, import and export (`components/footprint/`, `crates/ops/src/library_editors.rs`); items 19, 30, 31. |
| 9 | Board Setup | Partial | `BoardSetupDialog.tsx`: all 10 pages edit (rules overlay and 7 verbs, item 3); 8 of KiCad's pages have no model yet. |
| 10 | Net classes and rules | Partial | importer (`crates/kicad/src/import.rs::merge_project_net_classes`, `custom_rules.rs`) and editing (Net Classes, Custom Rules, Assign Netclass) done, item 3; open: regex patterns, the rule-tree designer. |
| 11 | Property dialogs | **Closed** | via, shape, zone, text, dimension and track width edit (`Cmd::EditVia`, `EditShape`, `EditZone`, `EditText`); the footprint (fields, attributes) and pad dialogs and the Properties panel edit every kind (`bd712d3`, `afcb91b`; items 9, 10). |
| 12 | Move excludes tracks and zones | **Closed** | `Cmd::MoveItems`, `RotateItems`, `FlipItems` (`crates/ops/src/pcb_transform.rs`), `kicad-port/pcbTransform.ts`, `pcbEditActions.ts::movableItem` for every kind; item 8. |
| 13 | Selection modifiers and box select | **Closed** | `kicad-port/selection.ts`, `components/canvas/selectionCandidates.ts::collectBoxSelection`, `Canvas.tsx`; `PARITY-pcb.md` section 3. |
| 14 | Clipboard | Partial | PCB: every item kind, footprints included, in KiCad's clipboard format (`crates/kicad/src/clipboard.rs`, `Cmd::PasteClipboard`, `Cmd::Duplicate`, `components/canvas/clipboard.ts`); schematic: done in KiCad's format (`Cmd::PasteSch`, item 6). Left: Paste Special on the PCB (item 8). |
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
  reference images and hatch fills, schematic images, the symbol text box (item 24), DXF and SVG import (4, item 28), derived symbols, alternate pin functions, design variants (3), padstacks (Remove Unused Pads),
  embedded files (4: zstd, and no new dependency), custom pads (4, item 31, in flight). The autoplacer (2) is declined: placement runs through `eda board` behind the placement gates. The Footprint and Symbol
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
| `addKnockout` | partial: no custom-pad convex-hull mode |
| `knockoutThermalReliefs`, `buildThermalSpokes` (`spokes.rs`) | ported: `DRC_ENGINE::EvalZoneConnection` (a pad's own connection over its footprint's over the zone's; "PTH only" thermal-relieves plated holes), the pad's relief gap and spoke width over the zone's (the width at least the zone's minimum and at most the pad's smaller side), the spoke angle (90 degrees for oval and rectangular pads, 45 otherwise, or the pad's own), spokes turned with the pad, circles built at 0 degrees and turned, the spoke ends, the test point and the mutual overlap, so only spokes that reach copper stay. Not ported: custom pads (their proxy spokes) |
| `buildCopperItemClearances` | partial: no courtyard knockouts, no net-tie exemptions; **the board edge** (`copper_edge_clearance` from every Edge.Cuts item as `knockoutGraphicClearance` takes it: an arc's curve, a circle's ring, a cutout's rim, a solid shape's inside; `FillInput::edge_cuts`); a pad's or footprint's own clearance override replaces the net class's and zone's for that pad, floored at the board minimum (`DRC_ENGINE::EvalRules`) |
| `ZONE::BuildSmoothedPoly` (`smoothed.rs`, `corner.rs`) | ported: same-net zones merged, the board outline (`GetBoardPolygonOutlines`: every outline with its cutouts, applied only when it is well-formed, `m_brdOutlinesValid`; `crates/drc/src/outline.rs`), the minimum-width apron, and the chamfer and fillet corner smoothing |
| `connect_nearby_polys` | missing |
| `fillNonCopperZone` | missing |
| `buildHatchZoneThermalRings`, `addHatchFillTypeOnZone` (`hatch.rs`) | ported: thickness, gap, orientation, smoothing level and amount, minimum hole area, and the thermal rings round pads and vias that are connected to a hatched zone |

## Phase one: the six packages (history)

Kept because six reasons in `ui-parity-missing.json` cite "GAPS.md item 5, WP6" (that is P3 now). WP1 schematic editing fidelity: done (`2a8bf8f`, `ee20bab`, `2d92851`, `358606b`); what is left is P10.
WP2 hierarchy and connectivity: module sheets (`08c70c8`), the rest is P4. WP3 PCB editing, selection and display: done (`88c8c0a`, `bd712d3`, `4c041a6`, `6611ade`, `358606b`); the rest is P6. WP4 router:
shove and follow-ups done (`1e4d7ea`, `03b46de`); the rest is P5. WP5 rules, zones and file fidelity: the writer and Board Setup (`b145f0e`), the review workflow (`02f383c`), zone fill (`eabaf91`), the outline
(`a46fd29`); the rest is P1, P9, P13 and P15. WP6 libraries, parts and footprints on the board: the choosers (`80e972d`), footprints as objects and arrays (`afcb91b`); library tables are P3, Change and Update
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

- **Running now**: P3 (`task-lib-tables`), P4 (`task-sch-hierarchy`), P7 (`task-custom-pads`).
- **Next free slot**: P1 -- P2, P14, P16, P17 and the Footprint Checker and Compare Footprint stand on a footprint that carries its graphics -- and P5 part 1, which shares no hub with anything.
- **After P1 has merged its IR**: P2 (its assignment overlay holds a definition with graphics). Step 1 of P2, the rename, can start sooner if P4 has merged its Annotate.
- **Any time a slot is free**: P12, P15, P11 (Find by Properties first), P6, P9a, P20 (test files only).
- **After P7**: P13, P17, and P19's arc stub and function shape.
- **After P3 and P4**: P18. **P8's PCB half** is independent; its sheet half is P4's.
- **Last**: P21.

### Conflict matrix

`x` the package edits the hub (more than a hunk); `(x)` a small hunk. A blank cell is no overlap.

| package | ir | ops | board | studio | kio | runner | store | canvas | schview | fpui | pns | zone | libs |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| P1 footprint graphics | x |  |  | x | x |  | (x) | x |  |  |  | (x) |  |
| P2 assignment, exchange, rename | x | x | x | (x) |  | (x) | (x) |  | (x) | x |  |  | (x) |
| P3 library tables (in flight) |  |  | (x) | x | (x) | (x) | (x) |  |  |  |  |  | x |
| P4 hierarchy (in flight) |  | x | x | x | (x) | x | x |  | x |  |  |  |  |
| P5 router |  |  |  |  |  | (x) | (x) | (x) |  |  | x |  |  |
| P6 PCB editing details |  | x |  |  | (x) | x | x | x |  |  |  |  |  |
| P7 custom pads (in flight) | x |  |  | (x) | x |  |  | x |  | x | (x) | x |  |
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

### P1. Footprint graphics on the board (item 25). Size L. Rank 1.
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

### P2. Footprint assignment, exchange and reference rename (item 26; SR4). Size L. Rank 2.
- Why: the Footprint field of a symbol does nothing on the board, Change Footprint does not exist, Update from Library never reads a library, and a reference rename strands the footprint.
- Files: `crates/model/src/ir.rs` (an assignment overlay in `DrawingsSection`, beside `footprint_edits`, applied by `board::load`), `crates/cli/src/board.rs` (`load`, `fold_unknown_symbols`, `reconcile_schematic`: the symbol's Footprint
  field is followed, a rename carries the part), `crates/ops/src/` new `fp_exchange.rs` plus the `Cmd` hunks in `lib.rs`, `rename_symbol` and `annotate` calling a `rename_part` that moves the pose, `footprint_edits`,
  `zone_overrides`, `locked_ids`, groups and the routing's pads; `fp_edit.rs`, `library_place.rs` (`Cmd::PlaceFootprint` for Insert footprint into PCB); studio `actions/pcbGlobalEditSweep.ts`, `components/PcbGlobalEditDialogs.tsx`
  (`ExchangeFootprintsDialog` exists for Update), `kicad-port/pcbGlobalEdit.ts`, `components/FootprintAssociationsDialog.tsx`, a new `ReannotateDialog.tsx`, `components/SymbolPropertiesDialog.tsx` and `actions/schSymbolActions.ts`.
- KiCad: `pcbnew/dialogs/dialog_exchange_footprints.cpp`, `pcb_edit_frame.cpp` (`ExchangeFootprint`), `netlist_reader/board_netlist_updater.cpp` (`replaceFootprint`, `updateFootprintParameters`, `updateComponentPadConnections`),
  `tools/global_edit_tool.cpp` (`ExchangeFootprints`), `tools/board_reannotate_tool.cpp`, `dialogs/dialog_board_reannotate.cpp`, `footprint.cpp` (`FootprintNeedsUpdate`), `tools/board_inspection_tool.cpp` (`DiffFootprint`),
  `eeschema/tools/assign_footprints.cpp`, `backannotate.cpp`, `tools/edit_tool_move_fct.cpp` (`SwapPadNets`).
- Order: the seven steps of item 26; Swap Pad Nets last and only if the board may disagree with the schematic.
- Conflicts: P4 (`rename_symbol` and `annotate` in `ops/lib.rs`, `reconcile_schematic`); P1 (the definition carries graphics); P14, P7 (`fp_edit.rs`); P3 (library lookup in `library_place.rs`, a hunk).

### P3. Library tables and paths (item 5). Size M for what is left. Rank 3. **In flight: `task-lib-tables`.**
- Why: only KiCad's installed libraries and the project library can be used; no user library can be added.
- Done in the branch: the table file format, path variables, global and project tables, the index, search, place and body lookups over a `LibSource` (`crates/kicad/src/library_table.rs`, `crates/cli/src/library_tables.rs`).
- Left: `GET/POST /api/library/tables`, `POST /api/library/new`, a new `components/LibraryTablesDialog.tsx` (Global and Project tabs; Nickname, URI, Options, Description; Add, Move, Delete, Browse) and
  `ConfigurePathsDialog.tsx`, Add Library / New Library / the library drop in `actions/commonLibraryActions.ts`, the library trees (`components/library/`) listing table libraries, the 3D path variables shared with
  `model3d_api.rs::resolve`; delete the six rows from `ui-parity-missing.json`.
- KiCad: `common/libraries/{library_manager,library_table,library_table_parser}.cpp`, `common/libraries/lib_table_grid_data_model.cpp`, `common/dialogs/{dialog_edit_library_tables,dialog_configure_paths}.cpp`,
  `eeschema/dialogs/panel_sym_lib_table.cpp`, `pcbnew/dialogs/panel_fp_lib_table.cpp`, `eeschema/tools/symbol_editor_control.cpp` and `pcbnew/tools/pcb_control.cpp` (`AddLibrary`, `NewLibrary`).
- Conflicts: P4 (`studio.rs`, `board.rs`: a few lines each); P17 and P18 build on it; P2 (`library_place.rs`).

### P4. Schematic hierarchy: the rest (item 4; SR1's sheet half). Size M for what is left. Rank 4. **In flight: `task-sch-hierarchy`.**
- Done in the branch: Annotate with KiCad's dialog and numbering (`crates/model/src/{annotate,sheet_list}.rs`, `crates/ops/src/sch_annotate.rs`), Find and Replace across sheets, the SVG render and the gates over every sheet.
- Left: Import Sheet, Draw Sheet from File and the schematic drop (`Cmd::ImportSheet` = `AddSheet` + the file's screen from `import_kicad_sch`, renumbered with `annotate_paste`; a route that reads the file); hierarchical sheets
  in the clipboard; a net per placement of a shared screen; buses across sheets; Reorganize keeping stored field places, label spins, looks and body styles; browser check for the hierarchy.
- Files: `crates/ops/src/{sheets,sch_clipboard}.rs` and a new `sheet_import.rs`, `crates/engine/src/{nets,hier}`, `crates/cli/src/{board,sch_hierarchy_api}.rs`, `crates/kicad/src/{sch_clipboard,sch_import}.rs`,
  `web/studio/src/components/{SchematicView,panels/HierarchyPanel}.tsx`, `kicad-port/{schSheet,schClipboard,navHistory}.ts`.
- KiCad: `eeschema/{sch_sheet_path,sch_screen,sch_sheet,connection_graph}.cpp`, `tools/{sch_drawing_tools (ImportSheet, DrawSheet),sch_navigate_tool,sch_editor_control (doCopy, Paste),sch_edit_tool (DdAppendFile)}.cpp`.
- Conflicts: P2 (`ops/lib.rs`, `board.rs`); P8 (the drop host); P10 (`SchematicView.tsx`); P18 (placing a block is Import Sheet).

### P5. Router: drag, mid-segment, diff pairs, meanders (item 7). Size XL, three parts. Rank 5. Independent of the rest.
- **P5a, M-L**: the dragger and the mid-segment route. `crates/pns/src/{dragger,line_placer,router,node}.rs` (D7: `DM_SEGMENT`, `dragCorner45`, `SHP_REVERSED`, last-valid restore; `SplitAdjacentSegments` so a route starts and ends
  mid-segment; Route From Other End after the first fix), `crates/cli/src/route_api.rs`, `web/studio/src/components/canvas/{routing,dragging}.ts`, `kicad-port/{routeTool,dragTool}.ts`, `actions/pcbRouterSweep.ts`.
- **P5b, L**: differential pairs with obstacles: coupled shove and walkaround, vias (`crates/pns/src/{diff_pair,dp_tune,shove,walkaround}.rs`, `diffPairRouting.ts`, `kicad-port/dpTool.ts`).
- **P5c, L**: the U meander and interactive tuning (`crates/pns/src/{meander,dp_tune}.rs`, `crates/cli/src/tune_api.rs`, `components/LengthTuningDialog.tsx`), arcs (`ARC_T`), mouse-trail posture, springback and `ShoveTimeLimit`,
  and, if wanted, tuning patterns as items (the nine generator rows).
- KiCad: `pcbnew/router/` (`pns_dragger`, `pns_multi_dragger`, `pns_line_placer`, `pns_diff_pair_placer`, `pns_meander*`, `pns_shove`, `pns_walkaround`, `pns_router`, `router_tool.cpp`), `pcbnew/generators/pcb_tuning_pattern.cpp`,
  `pcbnew/tools/generator_tool.cpp`.
- Conflicts: only among the three parts (`crates/pns`) and P6 in `Canvas.tsx` handlers. Each part ends with a replay of KiCad's own QA cases (`crates/pns/tests/qa_regressions.rs`) and a browser check.

### P6. PCB editing details (items 8, 15, 17, 35). Size M, ongoing. Rank 6.
- Paste Special on the PCB (`DIALOG_PASTE_SPECIAL`, `PASTE_MODE`, clear nets; a plain Paste then keeps references as KiCad's default does), the rotation step and flip direction settings, a footprint's bounding box and the anchor half of
  `BestSnapAnchor` for Rotate and Flip, the routing kept when a footprint with routed pads is edited, Duplicate and Paste as one step with the drop, items drawn while a group is entered joining it, the item 15 list, the item 35 tail.
- Files: `web/studio/src/components/canvas/{Canvas.tsx,clipboard.ts}`, `state/store.tsx`, `actions/{pcbEditSweep,pcbMenuActions}.ts`, `kicad-port/{pcbTransform,pcbCarry,pcbReference,alignDistribute,groupEdit}.ts`,
  `crates/ops/src/{pcb_paste,pcb_transform,pcb_groups}.rs`, `crates/kicad/src/clipboard.rs`.
- KiCad: `pcbnew/tools/{edit_tool,edit_tool_move_fct,pcb_control,pcb_selection_tool,pcb_group_tool,drawing_tool}.cpp`, `common/dialogs/dialog_paste_special.cpp`, `pcbnew/dialogs/{dialog_cleanup_tracks_and_vias,dialog_global_edit_*}.cpp`.
- Conflicts: P5 (`Canvas.tsx`), P16 (`Canvas.tsx`, `painter.ts`), P10 and P11 (`store`, `runner`: hunks).

### P7. Custom-shape pads (item 31; also 13, 19). Size L. Rank 7. **In flight: `task-custom-pads`, no commit yet.**
- Why: a custom pad imports as its anchor, a trapezoid or chamfered pad is a rectangle; zone fill is wrong around them (766% and 11% on two QA zones); four rows wait.
- Files: `crates/model/src/{footprint.rs,ir.rs}` (`Pad` and `LibraryPad`: anchor, primitives, trapezoid and chamfer), `crates/kicad/src/{footprint_lib,footprint_import,import,pcb}.rs` (`(primitives ..)` both ways),
  `crates/zone-filler` (`addKnockout` custom-pad mode, thermal proxy spokes), `crates/pns/src/from_ir.rs`, `crates/drc/src/board.rs` (`placed_pad_copper`), `components/BoardPadPropertiesDialog.tsx`,
  `components/footprint/PadPropertiesDialog.tsx`, `canvas/painter.ts`, `components/footprint/footprintPainter.ts`, `kicad-port/pad*.ts`, `actions/libraryEditorActions.ts` (explode and recombine).
- KiCad: `pcbnew/pad.cpp`, `tools/pad_tool.cpp` (`EditPad`), `dialogs/dialog_pad_properties.cpp`, `zone_filler.cpp` (`addKnockout`), `microwave/microwave_polygon.cpp`.
- Conflicts: P1 (footprint IR, writer, painter), P13 (`zone-filler`), P17 (pad dialogs), P19 (arc stub, function shape), P14 and P2 (`fp_edit.rs` pad edits).

### P8. Files in: Append Board, DXF and SVG, drops (item 28; SR1's PCB half). Size L. Rank 8.
- Parts: **P8a, M**: the drop host (`components/FileDropHost.tsx`), Append Board and its drop (`crates/cli/src/append_api.rs`: a route turning a `.kicad_pcb` into a paste, then `Cmd::PasteClipboard` with the carry), the Footprint Editor's drop.
  **P8b, M-L**: Import Graphics for the board (Edge.Cuts from a DXF is the case that matters), the schematic and the symbol editor: hand-written parsers in `crates/kicad/src/{dxf_import,svg_import}.rs` (LINE, ARC, CIRCLE, LWPOLYLINE,
  POLYLINE first; SVG paths) into a neutral shape list, a verb in `crates/ops/src/import_graphics.rs`, `components/ImportGraphicsDialog.tsx` (layer, line width, origin, scale, interactive placement).
  **P8c, M, low priority**: Import Netlist.
- KiCad: `common/import_gfx/{dxf_import_plugin,svg_import_plugin,graphics_importer,graphics_importer_buffer}.cpp`, `pcbnew/import_gfx/{dialog_import_graphics,graphics_importer_pcbnew}.cpp`, `eeschema/import_gfx/`,
  `pcbnew/tools/pcb_control.cpp` (`AppendBoard`, `placeBoardItems`, `DdAppendBoard`), `pcbnew/tools/board_editor_control.cpp` (`ImportNetlist`).
- Conflicts: P4 (the sheet drop), P3 (the library drop); `ops`, `runner`, `studio` hunks.

### P9. Board Setup completion and the rule editor (item 3). Size L; the rule editor XL on its own. Rank 9.
- **P9a, L**: the missing pages: Board Editor Layers, Text Variables (project variables and `${VAR}` expansion in texts and fields), Formatting, Zone Hatch Offsets, Length-tuning Patterns, Tuning Profiles (`missing_tuning_profile`),
  Component Classes (`component_classes_drc`), regular-expression net patterns and composite net classes, and drawing tools that start from the Text & Graphics defaults. Files: `crates/model/src/rules.rs`,
  `crates/ops/src/board_setup.rs`, `crates/kicad-engine/src/lib.rs` (project file), `crates/kicad/src/{pcb,import,custom_rules}.rs`, `web/studio/src/components/{BoardSetupDialog.tsx,boardSetup/*}`, `kicad-port/boardSetupRules.ts`.
- **P9b, XL**: the rule-tree designer (`DRETool.drcRuleEditor`).
- KiCad: `pcbnew/dialogs/panel_setup_{layers,formatting,zone_hatch_offsets,tuning_patterns,tuning_profiles,tuning_profile_info}.cpp`, `panel_assign_component_classes.cpp`, `pcbnew/component_classes/`,
  `common/dialogs/{panel_text_variables,panel_setup_netclasses}.cpp`, `common/project/component_class_settings.cpp`, `pcbnew/drc/rule_editor/` (82 files), `pcbnew/tools/drc_rule_editor_tool.cpp`.
- Conflicts: P15 (the project writer in `kicad-engine`: a hunk); `ops`, `runner` hunks.

### P10. Schematic details (items 1, 12, 15, 17, 20). Size M-L. Rank 10.
- Pin selection (`SCH_PIN_T`: Swap Pins, Swap Pin Labels, pin-level highlight), sheet pins selectable one by one, the net-collision overlay of a drag, `AutoRotateItem` after a label lands, a power symbol's Properties dialog,
  text justification flipping on a mirror, schematic groups, pan with the left or right button and auto-pan while drawing, the Symbol Fields Table's attribute columns and Exclude DNP filter (the flags exist:
  `SymbolInstance::{dnp, exclude_from_bom, exclude_from_sim}`), user-defined fields' places, Copy as Text, the item 15 schematic list.
- Files: `web/studio/src/components/SchematicView.tsx`, `components/schematic/{schItems,schHit,schSelectionSummary}.ts`, `kicad-port/{schMove,selection,schConnection}.ts`, `components/SymbolFieldsTableDialog.tsx`,
  `crates/ops/src/{sch_move,sch_edit,sch_props,sch_fields,fields_table}.rs`, `crates/model/src/sch_extras.rs`.
- KiCad: `eeschema/tools/{sch_edit_tool,sch_selection_tool,sch_drag_net_collision,sch_group_tool,sch_move_tool}.cpp`, `sch_label.cpp`, `sch_pin.cpp`, `dialogs/dialog_symbol_fields_table.cpp`, `fields_data_model.cpp`, `common/view/wx_view_controls.cpp`.
- Conflicts: P4 (`SchematicView.tsx`, `board.rs`), P6 (`store`, `runner` hunks), P16 (the schematic image).

### P11. Search and inspection (items 21, 29; SR2). Size M-L. Rank 11.
- Find by Properties (the Properties tab over `PCB_PROPERTIES`, then the Query tab), Find following the IR (user fields, hidden fields, zone names), Clearance and Constraints Resolution from `crates/drc/src/constraints.rs`
  (a function returning the winner and the rules considered), the Footprint Checker as a scratch-board DRC.
- Files: `web/studio/src/kicad-port/{pcbFind,propertyManager,pcbProperties}.ts` and a new `findByProperties.ts`, `actions/pcbFindActions.ts`, `components/PcbFindDialog.tsx` and new dialogs, `crates/drc/src/{constraints,pcbexpr}.rs`,
  a new `crates/cli/src/inspect_api.rs`, `crates/kicad-engine/src/lib.rs`.
- KiCad: `pcbnew/dialogs/dialog_find_by_properties.cpp`, `pcbexpr_evaluator.cpp`, `tools/board_inspection_tool.cpp` (`InspectClearance`, `InspectConstraints`), `drc/drc_engine.cpp` (`EvalRules`), `dialogs/dialog_footprint_checker.cpp`.
- Conflicts: none on the Rust hubs; `runner` and `store` for the dialog flags. Compare Footprint with Library belongs to P2.

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

### P14. The 3D models of a footprint (item 30; SR3). Size M. Rank 14.
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

### P17. Library editor leftovers (items 19, 22). Size L. Rank 17. After P7 and P3.
- Snapping to other items' anchors in both editors (wire `PcbGridHelper` and `SchGridHelper`), the footprint editor's pad array and numbering (`ARRAY_AXIS`, `ARRAY_PAD_NUMBER_PROVIDER`), a fields grid in the library editor's Footprint Properties, derived symbols
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
