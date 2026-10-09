# agentic-eda vs KiCad: gaps and the plan for the next phase

Status on 2026-10-07, main at `de88c02`, KiCad source `8303b2ad` (10.99).

The first phase made the editors exist. The next one makes what exists behave like KiCad's. This file is the
plan: where the original 30 gaps stand, the big gaps that list missed, a ranking by user impact, and six work
packages sized for one agent each. Verdicts are read from the code, not from the other parity docs; where a
doc and the code disagree, the code wins and the doc is named.

## Status

- **Actions wired: 462 of 766** (pcbnew 264/350, eeschema 124/233, common 74/183; `UI-ACTIONS.md`). 122 more are
  deliberately not wired, each with its reason in `web/studio/tools/ui-parity-missing.json`. The remaining 182
  (eeschema 77, common 105) are being wired now by the schematic-control and common-actions agents. Single
  actions are tracked in `UI-ACTIONS.md` and not repeated here.
- **The original 30 gaps** (Appendix A; numbering kept because code comments cite "GAPS.md #8", "#20", "#6"):
  **8 closed, 13 partial, 1 open, 8 out of scope** (kicad-cli covers DRC, ERC and the exports; the counts are those of Appendix A).
- **12 items are new**, found in `PARITY-*.md`, `CODE-COMPARE-*.md` and by reading the code. The ranked list marks them.
- **Three findings change the picture.**
  1. *A wired action is not a working feature.* `UI-ACTIONS.md` counted schematic Move, Drag, Rotate, Mirror,
     Properties and Align as wired, but they acted on symbols only. **Fixed on 2026-10-08 for the schematic (item 1):
     they act on every item kind, wires stretch, and Properties opens the dialog of the kind.** On the PCB, Move
     skips tracks and zones, Rotate and Flip take footprints and vias only, and Duplicate and Copy skip footprints
     (**the PCB half is fixed too, 2026-10-08, item 8**).
  2. *The KiCad files we write drop design data, and kicad-cli judges those files.* The PCB writer
     (`crates/kicad/src/pcb.rs`) gave every zone the board's default clearance and width instead of its own
     settings, wrote no keepout, no dimensions, no groups, no locks, and turned arc tracks into 32 segments; a
     rule area has no net, so the writer's net check rejected it and a board with a keepout could not be DRC'd or
     exported (confirmed by a test, then fixed). **Fixed on 2026-10-08 (item 2); what is left there is footprint
     graphics, library nicknames, component classes and tuning profiles.**
  3. *One root sits under several gaps:* the constraint model built from the intent has no edit verbs. Net
     classes, custom rules, adding or renaming a part, assigning a footprint and importing a netlist are all
     recorded as unwired for that reason. `design.json` already overrides the model in places (`design.nets`,
     the footprint and symbol libraries); extending that overlay unlocks items 3, 5 and 9. **The rules now have
     their overlay (`drawings.rules`, applied in `board::load`) and seven undoable verbs; every Board Setup page
     edits through them (item 3). Item 5 took the library half of it (2026-10-08: a placed installed symbol keeps its
     definition in the project symbol library, a library footprint becomes a board part), but a part's footprint
     still comes from the intent; item 9 (footprints on the board) still waits for the same kind of overlay.**
- **Docs.** Action level: `UI-ACTIONS.md`. Behaviour tables: `web/studio/PARITY-{pcb,sch,3d,fpedit,symedit,boardctl}.md`.
  Code level: `CODE-COMPARE-ui.md` (the first 266 handlers; about 200 added since have not been compared) and
  `CODE-COMPARE-router.md`, `crates/pns/PARITY.md`, `crates/zone-filler/PARITY.md`. Measured round trip and
  connectivity: `REPORT.md` (the real-board re-export part re-measured 2026-10-08; each part of it says its own date). Engines: `ARCHITECTURE.md`.

## Ranked list: open and partial items, by user impact

"Hit" is how often a KiCad user meets it; "Blocks" is whether a workflow stops. WP is the work package below.
"Port from" paths are under the KiCad source root.

### 1. Schematic edit tools act on symbols only
New (the residual of #1). **Mostly done (2026-10-08).** Hit: every schematic session. Blocks: no longer; Move, Drag, Rotate, Mirror, Properties and Align work on every item kind. WP1, size L (step 1 done).
- Done (`crates/ops/src/{sch_move,sch_drag,sch_scene,sch_props}.rs`, `kicad-port/{schMove,schAlign,schProperties}.ts`, `components/{SchematicView,SchPropertiesDialogs}.tsx`):
  one verb family, `Cmd::SchMove`, moves, drags, turns, mirrors and aligns any set of symbols, power symbols, wires and bus wires, labels of every kind,
  free text, text boxes, shapes, rule areas, directive labels, junctions, no-connects, bus entries, graphic lines and sheets (with their pins), as one undo
  step on the sheet in view. Move leaves the wires where they are; Drag stretches the attached wires and adds the segments KiCad adds (right-angle bends,
  the stub at an unselected junction, label and sheet-pin special cases), then does KiCad's finishing (junctions, trimming, merging, dangling segments);
  dragging a wire segment drags its neighbours. Rotate and Mirror use KiCad's turn point (own anchor, or the half-grid-snapped centre of the selection) and
  turn a label's spin (`SchExtras::label_spins`, drawn). R, Shift+R, X and Y work while items are held, and the view shows what the server computes for the
  same command (`POST /api/sch/move_preview`). A plain click on a wire selects it, a click-drag works on any item, and Align and Align to Grid move each
  item's wires with it so pins stay on the connection grid. Properties (`E`, a double-click) opens the dialog for the kind: label, text, sheet, wires /
  buses / bus entries / graphic lines / junctions (width, style, colour, junction size), shapes, rule areas, text boxes and directive labels; strokes and
  junction looks are drawn and written to the `.kicad_sch` as KiCad writes them, and read back.
- Measured: 49 Rust tests for the move family (every kind moved, dragged with stretch, turned, mirrored and undone; the net list unchanged when items move with
  their wires; every symbol of the user's board and of mcu30 as module sheets dragged keeps its wires on its pins), 17 for Properties, 4 in `board.rs`
  (each verb is one undo step; the nets are untouched), a stroke round trip through a `.kicad_sch`, and 19 `node --test` cases; clicked through on a copy of mcu30
  (wire segment drag with bends, held R during a move, rotate and mirror of five kinds together and Undo, Align Left, Align to Grid, each Properties dialog).
- Missing: the net-collision overlay of a drag (`sch_drag_net_collision.cpp`); `AutoRotateItem` after a label lands; a text has no justification to flip
  when it is mirrored; fonts and colours of text and labels (a label's size, bold and italic are in: item 12), a sheet's border and fill, extra fields and a
  label's fields have no place in the IR, so their dialog pages are absent; sheet pins cannot be selected one by one; a power symbol has no Properties dialog (its Value is edited in Field
  Properties); symbols that this project drew (not imported) and are turned or mirrored place
  their pins by `eda_engine::placed`, which disagrees with the drawn symbol for 90-degree and mirrored ones (older; the move tools use what is drawn).
- Port from: `eeschema/tools/sch_move_tool.cpp`, `sch_edit_tool.cpp`, `sch_selection_tool.cpp`, `sch_drag_net_collision.cpp`, `sch_align_tool.cpp`.

### 2. The KiCad files we write drop design data
New (the old "round trip" mention). **Mostly done (2026-10-08).** Hit: any board with a pour, keepout or dimension; every export. Blocks: no longer for keepouts; the rest changes what kicad-cli reports on some real boards. WP5, size L (step 1 done).
- Done (WP5 step 1; `crates/kicad/src/{pcb,pcb_items,import_items,import}.rs`): a rule area is written as a keepout zone, and one a footprint owns is
  written inside that footprint (as a board-level copy it reported its own footprint as a keepout violation); before, it failed the whole export with
  `kicad.unknown_net`, so DRC and every output on a board with a keepout failed (test `kicad_cli_drc_rule_area_reports_items_not_allowed_and_exports_gerbers`:
  the track inside is `items_not_allowed`, Gerbers plot). Every zone writes its own clearance, minimum width, priority, pad connection, thermal gap and
  spoke, hatch, island and fill settings. Arc tracks are arcs (`Track::arc_mid_offset`). Locks, dimensions, groups, zone names and teardrop flags are
  written and read back (round-trip tests in `pcb.rs`). The parity harness re-exports a real board with its own project beside it.
- Measured (`REPORT.md`, `scores.json`, 2026-10-08, kicad-cli 10.99 on `qa/data` of the KiCad sources): the re-export of a real board gives kicad-cli the same
  violation counts as the original on **6 of 12** boards (50.0%), up from 4 of 12 (33.3%) before the fixes; `bad_triangulation_case` (zones, isolated
  copper) and `fill_bad` now match. All 12 re-exports load.
- Missing: footprint-local graphics and texts (`fp_line`, `fp_circle`, `fp_text`) are imported into `drawings.footprint_extras`, which only the in-house DRC
  reads, and never written back (512 `fp_line`s on `complex_hierarchy`, hence its silkscreen and mask differences); a footprint that had no library is written
  as `eda:<name>`, so a project that turns `lib_footprint_issues` on gets a violation for a library that does not exist; barcodes, component classes and
  tuning profiles are not read (the `api_kitchen_sink`, `component_classes_drc` and `drc_missing_tuning_profile` differences).
- Port from: `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` and `pcb_io_kicad_sexpr_parser.cpp`.

### 3. Board Setup and the rules cannot be edited
Old #9 and #10. **Mostly done (2026-10-08).** Hit: the start of every project. Blocks: no; the pages that do not exist yet need a model first. WP5, size L (step 2 done).
- Done (WP5 step 2): `eda_model::rules::RulesOverlay` (`crates/model/src/rules.rs`) holds one optional page per Board Setup page in `design.json`
  (`drawings.rules`); `board::load` lays it over the intent's (or the imported project's) rules, as `design.nets` is, so the router, the gates, the studio and the
  derived `.kicad_pro`, `.kicad_pcb` and `.kicad_dru` that kicad-cli reads all follow an edit. Seven undoable verbs (`set_net_classes`, `set_constraints`,
  `set_mask_paste`, `set_text_graphics_defaults`, `set_stackup`, `set_rule_severities`, `set_custom_rules`; `crates/ops/src/board_setup.rs`) refuse a bad page
  with the setting's name, after the checks `ValidateDesignRules` and the panels make. `components/BoardSetupDialog.tsx` is KiCad's page tree and all ten pages
  edit: Physical Stackup, Solder Mask/Paste, Defaults, Dimensions, Constraints, Pre-defined Sizes, Teardrops, Net Classes (with the net-name patterns and the
  nets each matches), Custom Rules (as text, "Check rule syntax" through kicad-cli: `POST /api/check_rules`) and Violation Severity (KiCad's own list of 64
  checks, `tools/extract-drc-checks.js`). Assign Netclass... works from the board and the schematic. A changed clearance, a net class assigned by a pattern, a
  minimum track width, an ignored check and a custom rule each change what kicad-cli reports, and Undo restores it (`crates/cli/src/board.rs`).
- Missing: the Board Editor Layers, Zone Hatch Offsets, Formatting, Text Variables, Length-tuning Patterns, Tuning Profiles, Component Classes and Embedded Files
  pages (nothing in the model holds them); the rule-tree designer (`DRETool.drcRuleEditor`; the `.kicad_dru` is edited as text, without KiCad's highlighting,
  completion or compiler warnings); KiCad's regular-expression net patterns, bus notation and composite net classes (a pattern here is `*` and `?`, first class
  wins); dielectric sub-layers, colours and impedance control on the stackup; and this app's drawing tools do not start new items from the Text & Graphics defaults
  yet (they reach the derived `.kicad_pro` only).
- Port from: `pcbnew/dialogs/dialog_board_setup.cpp` and `panel_setup_*.cpp`, `common/dialogs/panel_setup_netclasses.cpp`,
  `panel_setup_severities.cpp`, `pcbnew/drc/drc_rule_parser.cpp`, `pcbnew/tools/drc_rule_editor_tool.cpp`.

### 4. Hierarchical sheets can be navigated but not edited
Old #6. **Partial.** Hit: every multi-sheet design. Blocks: yes. WP2, size L.
- Exists: the IR (`SchematicSection::sheets`, `Design::sheet_contents` in `crates/model/src/ir.rs`); tree import and
  multi-file export (`crates/kicad/src/sch_import.rs::import_kicad_sch_tree`, `lib.rs`); the Hierarchy panel and Back/Forward
  (`components/panels/HierarchyPanel.tsx`, `kicad-port/navHistory.ts`); Draw Sheet, sheet pins and their sync
  (`kicad-port/schSheet.ts`, `schSheetPins.ts`); ERC per sheet through kicad-cli.
- Missing: every schematic verb acts on the root section (`crates/ops/src/lib.rs::schematic_mut`), so a sub-sheet is
  read-only; the drawing and sheet-pin tools refuse on a nested sheet (`actions/schEditActions.ts`, `schSheetPinActions.ts`).
  `reconcile_schematic` (`crates/cli/src/board.rs`) retraces the root only and overwrites `design.nets` with root-only nets;
  nothing merges nets across sheets (hierarchical and global labels, sheet pins) in the live netlist. Also missing: Import
  Sheet, Draw Sheet from File, Next/Previous/Change Sheet, sheet page numbers, annotation across sheets, copy between sheets.
- Port from: `eeschema/sch_sheet_path.cpp`, `sch_screen.cpp`, `connection_graph.cpp`, `tools/sch_navigate_tool.cpp`,
  `tools/sch_editor_control.cpp`, `dialogs/dialog_annotate.cpp`.

### 5. No access to the installed libraries
New (the old "library browsers" mention). **Partly closed on 2026-10-08: the two choosers read KiCad's installed libraries; library tables are still open.** Hit: every session that adds a part. Blocks: no longer, a part can come from any installed library. WP6, size L.
- Done: **the Symbol Chooser is KiCad's** (`components/SymbolChooserDialog.tsx`, `components/chooser/`, `kicad-port/libChooser.ts`): a tree of "-- Recently Used --"
  (the last 8), "-- Already Placed --", the project's own libraries and all 222 installed ones (opened one at a time), a search over names, descriptions,
  keywords, library names and default footprints scored as `EDA_COMBINED_MATCHER` does (an exact term 8 times its weight, a match at the start twice, anywhere once;
  a word may carry `*` and `?`), a row per unit under a multi-unit symbol, the symbol's drawing one unit at a time, its default footprint and drawing, and the description pane.
  **The Footprint Chooser** (`components/FootprintChooserDialog.tsx`): the 155 installed footprint libraries with the same search (name, description, tags),
  the drawing, the description, and, when it is opened for a symbol, "Filter by pin count" and "Apply footprint filters" (`ki_fp_filters`). It serves **Place Footprint**
  (a mounting hole, a fiducial: `Cmd::PlaceFootprint` makes a board part `H1`, `TP1`, ... with the library's pads), the Footprint field of Symbol Properties (Browse...)
  and Assign Footprints (From KiCad's libraries...). **Placing an installed symbol** keeps its definition with the schematic in the same undoable command
  (`Cmd::EmbedLibSymbol`, a published entry of the project symbol library; `crates/cli/src/library_place.rs` adds it to an `AddSymbol` on the server), with the library's
  Value, Footprint and Datasheet, so it draws, exports and passes kicad-cli ERC (a slow-tier test); one undo takes the instance and its definition back, redo restores both.
  **Speed**: a library is indexed on demand, once, by a single pass over its text that keeps a few fields and the byte span of each symbol (`crates/kicad/src/symbol_scan.rs`,
  `footprint_scan.rs`; 230 MB of symbols in 2.5 s in a debug build), cached against the file's modification time; a thread indexes the rest on the first search, which says
  `indexing` until it is done (`crates/cli/src/library_search.rs`, `GET /api/library/entries|search|details|project`). Only the best 400 matches leave the server, and one
  symbol's drawing is cut out of its file and sent when it is selected, never a library.
- Exists besides: library trees for the project library (`components/library/`); `.kicad_sym` and `.kicad_mod` reading from one root set by `EDA_KICAD_SYMBOLS` and
  `EDA_KICAD_FOOTPRINTS`; the Footprint and Symbol editors' trees list every installed KiCad library (`crates/cli/src/library_index.rs`,
  `GET /api/library/index|items|all`, `components/library/LibraryTree.tsx`) and opening an installed item copies it into the project library first.
- Missing: library tables (user and project libraries, Add and New Library, Configure Paths); KiCad's regular-expression (`/re/`) and relational (`voltage>3.3`) search words
  (a word is searched as text); the Footprint selector's list and the 3D preview of the Footprint Chooser; an alternate (DeMorgan) body style cannot be chosen when placing --
  a placed instance has no body style in the IR, so it shows the standard one while the kept definition holds both; Change Symbols still lists only the libraries the
  design references; power symbols are placed by `P`, not offered here.
- Port from: `common/libraries/library_manager.cpp`, `library_table.cpp`, `common/lib_tree_model_adapter.cpp`,
  `common/footprint_info.cpp`, `eeschema/libraries/symbol_library_adapter.cpp`, `eeschema/symbol_chooser_frame.cpp`,
  `pcbnew/footprint_chooser_frame.cpp`, `pcbnew/footprint_library_adapter.cpp`.

### 6. The schematic has no clipboard
New (the schematic half of old #14). **Closed on 2026-10-08.** Hit: every session. Blocks: no longer. WP1, size M.
- Done: Cut, Copy, Paste, Paste Special and Duplicate on the Schematic tab, in KiCad's own clipboard format (`(lib_symbols ...)` and the selected items, no
  `(kicad_sch ...)` around them). Copy puts it on the system clipboard (`crates/kicad/src/sch_clipboard.rs::write_clipboard`, `POST /api/sch/clipboard/copy`); Paste reads
  KiCad's (`parse_clipboard`) and shows what it would add following the cursor until a click places it (`kicad-port/schClipboard.ts`, `components/SchematicView.tsx`); one verb
  adds the batch to the sheet in view, one undo step (`Cmd::PasteSch`, `crates/ops/src/sch_clipboard.rs`). Pasted symbols are numbered unique across every sheet
  (`eda_model::sch_clipboard::annotate_paste`, a port of `ReannotateDuplicates`), Paste Special offers KiCad's three reference-designator options
  (`components/SchPasteSpecialDialog.tsx`), Duplicate copies into a buffer of its own and carries the copy by the connection point nearest the cursor. Proven against real KiCad both ways:
  kicad-cli reads what a copy writes and finds the same nets (`crates/kicad/tests/sch_clipboard.rs`, `EDA_SLOW_TESTS=1`), and forms taken out of KiCad's QA schematics read as the files do.
- Left: hierarchical sheets are not copied or pasted (KiCad keeps their screens in a side buffer); a field's position and a label's rotation are not in the IR, so a copy carries
  neither (the fields are marked `fields_autoplaced`, so KiCad lays them out when the pasted symbol moves); Keep annotations cannot make a duplicate reference (a design here names a part
  by its reference), so a taken one is numbered anew; Clear annotations numbers at once instead of leaving `R?`; Copy as Text; tables, images and groups; a part's MPN and LCSC from the intent.
- Port from: `eeschema/tools/sch_editor_control.cpp` (`doCopy`, `Paste`), `common/clipboard.cpp`, `eeschema/sch_io/kicad_sexpr/`.

### 7. The router: shove and drag are not KiCad's
Old #7. **Partial.** Hit: dragging, a route that starts or ends mid-segment, differential pairs with obstacles (Shove, the headline feature, now works near pads, vias and tracks; see the status below). Blocks: partly. WP4, size XL.
- Exists: `crates/pns` ports hulls, `LINE::Walkaround`, shove of tracks and vias, the optimizer with smart pads, loop removal,
  diff-pair routing, single-track and diff-pair length and skew tuning (dialog-driven), and `D`/`G` drag of a corner or via
  (`crates/pns/PARITY.md`; `CODE-COMPARE-router.md` D1-D4 and D8 are fixed).
- **Status (2026-10-08): shove near pads is ported** (`CODE-COMPARE-router.md` D5, D6, D10, D12 closed). `crates/pns/src/shove.rs` now
  follows `pns_shove.cpp`: a stack of lines with ranks, the nearest obstacle by path length (pads, then vias, then tracks),
  `onCollidingSolid` walking the current line around the cluster of a pad, `pushOrShoveVia` with the minimum translation vector and a
  45-degree re-shape of the via's tracks, `runOptimizer` over every shoved line, widths kept, and the head walked around pads first
  (`rhShoveOnly`). KiCad's own `simple-shove-1` and `issue22749` pushes are replayed (`crates/pns/tests/qa_regressions.rs`): the same
  13 and 5 tracks move, no new violation. Still open in Shove: a head that ends in a via is not shoved with its via, no springback or
  time limit.
- **Status (2026-10-08, follow-ups): the clearance gate, the board edge, the settings and the via drag are done.**
  `routing_clearance` measures every pad by its exact outline (`eda_drc::board::placed_pad_copper`, `Shape::gap_to`; a gap is a
  violation under `clearance - 0.5 um`, as kicad-cli counts it), so a valid shove is no longer refused at a round pad's corner; on
  `mcu30`, `pic_programmer` and `backspace1` every pair it fails is a pair kicad-cli reports (slow test), where the old rectangles
  failed six pairs of TO-92 pads kicad-cli calls clean. The board outline is a router obstacle with the copper-to-edge clearance
  (`PNS_KICAD_IFACE_BASE::syncGraphicalItem`): a shove toward the edge stops at it, and kicad-cli sees no `copper_edge_clearance`
  on eight shoves toward `mcu30`'s edge. Every `RoutingSettings` field is read (D16) and Interactive Router Settings has
  `dialog_pns_settings.cpp`'s rows. Dragging a via is a shove (D7), and a Walk around drag walks around. IR limits: a trapezoid or
  custom pad reaches the IR as its rectangle, so the gate and the router measure that; an inner Edge.Cuts cutout is not an obstacle.
- Missing (`CODE-COMPARE-router.md`): a drag moves the nearer end instead of sliding the segment, and at any angle (D7: the segment
  slide, `dragCorner45`, the optimize-after-drag); a route cannot start or end mid-segment
  and Route From Other End works only before the first fix; a diff pair has no coupled shove or walkaround and no via; length tuning
  is a dialog on straight axis-aligned tracks that builds a 45-degree accordion, not KiCad's U meander (`meander.rs`); no arcs
  (`ARC_T`), mouse-trail posture (so the dialog's "Use mouse path to set track posture", "Smooth dragged segments" and "Optimize entire
  track being dragged" rows are disabled) or springback.
- Port from: `pcbnew/router/` (`pns_shove.cpp`, `pns_walkaround.cpp`, `pns_dragger.cpp`, `pns_line_placer.cpp`,
  `pns_diff_pair_placer.cpp`, `pns_meander*.cpp`, `router_tool.cpp`), `pcbnew/generators/pcb_tuning_pattern.cpp`.

### 8. PCB edit tools skip item kinds
Old #12, and the PCB parts of #14 and #25. **Mostly done (2026-10-08).** Hit: every layout session. Blocks: no. WP3, size M-L (the edit tools done; the Properties panel is item 10, pad edits item 9).
- Done (`PARITY-pcb.md` section 22): three verbs, `move_items`, `rotate_items` and `flip_items` (`crates/ops/src/pcb_transform.rs`), move, turn and flip
  footprints, tracks (arcs stay arcs), vias, zones, graphics, text, dimensions and groups as KiCad's item classes do, one undo step each, and reach the
  `.kicad_pcb` kicad-cli reads (test `a_moved_track_is_where_kicad_cli_finds_it`). Move, drag, Move Individually, Move with Reference, Position Relative,
  Move Exactly, Rotate and Flip work for every kind, with `FilterCollectorForLockedItems`, `FilterCollectorForFreePads` and KiCad's rotation and flip points
  (`kicad-port/pcbTransform.ts`); `R` turns counter-clockwise now (it turned clockwise), and `R` and `F` during a move act on the carried selection about the
  point it was picked up at. Align and Distribute take every kind, a locked item is the Align target and never moves (`kicad-port/alignDistribute.ts`). Duplicate,
  Copy, Cut and Paste take footprints (a copy is a part of the board, `DrawingsSection::board_parts`), dimensions and groups, in KiCad's clipboard format
  (`crates/kicad/src/clipboard.rs`, `Cmd::PasteClipboard`, `POST /api/clipboard/copy`): kicad-cli loads our text as a board and what KiCad 10 writes reads
  back; Escape takes a carried duplicate or paste away again. Pads are selectable (click, box, Selection Filter, highlight, a Properties pane summary).
  A footprint edit clears the routing only when copper is routed to its pads, so a copy can be placed and deleted on a routed board.
- Missing: `BestSnapAnchor` beyond the grid and a footprint's own bounding box (the courtyard stands in); a footprint with copper on its pads still clears
  the whole routing when edited; Duplicate and Paste are a step of their own and the drop a second one; a copy keeps the original's nets, so `--strict` refuses
  moving it away; no rotation-step or flip-direction setting; Paste Special on the PCB; the pad edits (item 9).
- Port from: `pcbnew/tools/edit_tool.cpp`, `edit_tool_move_fct.cpp`, `align_distribute_tool.cpp`, `pcb_selection_tool.cpp`, `pcbnew/kicad_clipboard.cpp`.

### 9. Footprints on the board are not editable objects
New (the residual of old #11; blocks #26). **Open.** Hit: every board (silkscreen cleanup, mounting holes). Blocks: partly. WP6, size L.
- Exists: pose, side and a four-way reference text side (`FootprintInstance` in `crates/model/src/ir.rs`, `Cmd::SetLabelSide`,
  `components/FootprintPropertiesDialog.tsx`).
- Missing: reference and value text position, size, layer and visibility, user fields; per-instance attributes (DNP, exclude from
  BOM or position files); per-pad overrides (pads are selectable since 2026-10-08, item 8); board-only footprints beyond Place Footprint (mounting holes and fiducials from a library exist since
  2026-10-08, item 5; logos and the microwave tools are declined for the same reason as before); Change Footprint(s), Update Footprints from Library and geographical
  reannotate (recorded unwired: a part's footprint comes from the intent, which has no verb); new copies of a footprint by Array (Duplicate and Paste make them since 2026-10-08, item 8).
- Port from: `pcbnew/dialogs/dialog_footprint_properties.cpp`, `dialog_exchange_footprints.cpp`, `dialog_update_pcb.cpp`,
  `pcbnew/pcb_field.cpp`, `pcbnew/tools/board_editor_control.cpp` (`PlaceFootprint`).

### 10. The Properties panel is a read-only summary
New (old #11). **Mostly done (2026-10-08).** Hit: constantly. Blocks: no longer; the dialogs and the pane edit the same things. WP3, size M (done).
- Done (`kicad-port/{propertyManager,propertyGrid,pcbProperties,schItemProperties}.ts`, `components/panels/{PropertyGrid,PropertiesPanel}.tsx`; `PARITY-pcb.md` section 23, `PARITY-sch.md`
  section 17): the pane is KiCad's property grid. `PROPERTY_MANAGER` is ported (the class registry, `InheritsAfter` / `Mask` / `ReplaceProperty` / `OverrideAvailability`, the walk that orders
  a class's rows and groups), so each class lists what KiCad's registration lists, in KiCad's order: footprints, pads, tracks, arcs, vias, zones and rule areas, text, shapes of every
  kind, the five dimensions and groups on the board; symbols, power symbols, wires, buses, graphic lines, junctions, bus entries, labels of the four kinds, text, text boxes, shapes, rule
  areas, directive labels and sheets on the sheet. A selection shows the rows every item has (same name, available, same choices), the value they share or `<...>`, and a row is writeable
  when it is for all of them. An edit sets the property on every item and goes out as ONE `batch`, so a multi-selection is one undo step; a refused value shows its message above the
  grid (`valueChanging`), Enter commits and moves to the next row, Escape puts the value back. Every edit is an existing verb (`move_items`, `rotate_items`, `flip_items`,
  `set_locked`, `set_track_width`, `edit_tracks_and_vias`, `edit_via`, `edit_zone`, `edit_shape`, `edit_text`, `edit_dimension`, `edit_group`; `sch_move`, `rotate_symbol`,
  `mirror_symbol*`, `rename_symbol`, `edit_symbol_fields`, `set_symbol_attrs`, `sch_edit` `set_locked` / `edit_label` / `edit_text` / `edit_sheet` / `set_stroke` / `edit_graphic`) except four
  small ones added where none fit (`crates/ops/src/pcb_props.rs`): `edit_track` (a track's or arc's end points), `set_item_net` (tracks, vias, zones), `set_zone_name`, `replace_shape`
  (a shape's geometry, in place).
- Measured: 87 `node --test` cases (the registry's order and masks, the grid merge, validators, every class's list and every setter's commands), 9 Rust tests for the four verbs and one
  in `board.rs` (a panel edit of several items is one undo step); clicked through on a scratch board with one of every kind: each editable row of each kind was edited, its value read back and
  the edit undone to the exact item, three tracks edited at once were one undo step, and so were items of different kinds locked together (`web/studio/e2e/properties-panel.check.js` repeats it).
- Missing: what the IR has no place for is not registered: a pad's own shape, type, size and drill (read-only summary only), footprint attributes and overrides, a footprint's reference, value
  and library link (read-only: they come from the schematic and the intent, item 9), via tenting and backdrill, text fonts, bold, italic, vertical justification and colour, a shape's line style
  and colour, a sheet's border and fill, symbol pin names and numbers, extra fields; a wire's end points and length are read-only (they are dragged); the grid is not in the Footprint and
  Symbol editors; a footprint's position lands on the placement grid like every pose; a pad's position moves its footprint (and twice for two pads of one).
- Port from: `common/widgets/properties_panel.cpp`, `common/properties/property_mgr.cpp`, `pcbnew/widgets/pcb_properties_panel.cpp`, `eeschema/widgets/sch_properties_panel.cpp` and the
  `PROPERTY_MANAGER` registrations of each item class.

### 11. The DRC and ERC dialogs lack the review workflow
New (the UI half of old #19). **Mostly closed on 2026-10-08.** Hit: every review pass. Blocks: no. WP5, size M.
- Done: Run DRC and Run ERC through kicad-cli with a running state, out-of-date marking and lint tabs; the list is `RC_TREE_MODEL`'s (`Error: ` / `Excluded warning: `
  lines, the items and the exclusion comment under each, Show All / Errors / Warnings / Exclusions with the counts); DRC exclusions as an undoable verb
  (`Cmd::AddDrcExclusions` / `DeleteDrcExclusions`, `design.drawings.drc_exclusions`) written into the derived `.kicad_pro`; Next, Previous and Exclude Marker on both
  editors over the rows the Show boxes list (KiCad gives them no default hotkey); a marker menu in both dialogs and on both canvases (Exclude, Exclude with comment,
  Exclude all of the check, Change severity, Ignore, Edit severities, Show in the dialog); the Ignored Tests and Schematic Parity pages (`--schematic-parity`); a Violation
  Severity page in Schematic Setup (`Cmd::SetErcSeverities` -> `erc.rule_severities`). Details: `web/studio/PARITY-pcb.md` section 9a, `PARITY-sch.md` sections 4 and 9,
  `PARITY-common.md` section 5.
- Left: **a DRC exclusion reaches kicad-cli only where the report can say where the marker is.** KiCad matches an exclusion to a marker by its exact text, position
  included, and kicad-cli's report gives the items' positions and never the marker's, so the studio writes the positions worth trying (the items', a track's ends and middle,
  the middle of the first two) and the rest are waived in the studio's report alone (`kicad_matched: false` on the row). A KiCad patch adding the marker position to
  `RC_JSON::VIOLATION` would close it. ERC exclusions are the studio's alone (the derived project does not carry them) and have no comment. Not ported: selecting a marker
  with a left click, Delete Marker / Delete All Markers and Save report, "Exclude all violations of rule '...'" (per custom rule: the report does not name the rule), the
  Inspect / Fix menu entries, Edit connection grid spacing (Schematic Setup has no Formatting page).
- Port from: `pcbnew/dialogs/dialog_drc.cpp`, `eeschema/dialogs/dialog_erc.cpp`, `common/dialogs/panel_setup_severities.cpp`, `eeschema/dialogs/dialog_schematic_setup.cpp`, `common/rc_item.cpp`.

### 12. Schematic symbols, fields and labels carry no per-instance geometry
New. **Done (2026-10-09)**, limits below. Hit: every schematic cleanup. Blocks: no. WP1, size M.
- Done (2026-10-08, `PARITY-sch.md` section 15): the Reference, Value, Footprint and Datasheet of every symbol, the Value of a power
  symbol and the name and file of a sheet each have a position, an orientation, a justification and a visibility of their own
  (`SchematicSection::field_layout`, keyed by the item, in the item's own frame so a move, a turn or a mirror carries them), placed
  by a port of Autoplace Fields (`eeschema/autoplace_fields.cpp`, `crates/engine/src/fields.rs`) and moved to another side or further
  out where they would run over what is drawn; the `.kicad_sch` writer writes them, `GET /api/schematic` sends them (`fields`) and the
  painter draws them there. Generated symbols (`gen:<ref>`) draw their pins and texts the way KiCad's own do.
- Done (2026-10-09, `PARITY-sch.md` sections 1 and 15): fields are items of their own. Each is picked (a click, a box), moved (`M`, a drag),
  turned and mirrored, hidden with Delete and edited in Field Properties (text, position, size, orientation, bold, italic, justification,
  visible, shown name, allow autoplacement; one undo step), and moving one on its own takes its item out of the autoplaced ones
  (`SchExtras::fields_autoplaced`, `(fields_autoplaced yes)` in the file); Symbol Properties has a Show check for each of the four fields and
  Show Hidden Fields draws the hidden ones (neither is selectable, as in KiCad). `eeschema.InteractiveEdit.autoplaceFields` runs the manual
  Autoplace Fields (`AUTOPLACE_MANUAL`: the colliding-side and fit-between-wires search, the 10 mm drawable area) on the selection, and a
  turn of a symbol alone places its autoplaced fields again (`AUTOPLACE_AUTO`). A label's spin, size, bold and italic are stored
  (`SchExtras::label_spins`, `label_looks`), edited in Label Properties, written the way KiCad writes them (angle and justification) and
  read back from a `.kicad_sch`. A placed symbol in the alternate ("De Morgan") body style is drawn and written in it: the library symbol
  keeps both bodies (`LibSymbol::alternate`, `Name_<unit>_2` sub-blocks), `SchExtras::body_styles` says which style a symbol is in,
  Cycle Body Style (`eeschema.InteractiveEdit.toggleDeMorgan`, `SchCmd::SetBodyStyle`) changes it and kicad-cli reads the pins of that style
  where the file puts them (`crates/kicad/tests/sch_body_style.rs`). Measured: 44 Rust tests (26 for the fields, 5 for the label geometry, one
  of them through KiCad's own writer, 13 for the body styles, one of them through kicad-cli's netlist and one reading KiCad's own De Morgan QA file), 17 `node --test` cases, the overlap
  sweep unchanged, and the studio's fields, labels, Cycle Body Style and the Symbol Properties dialog clicked through on a copy of mcu30.
- Missing: a field's font face and colour, a free text's justification, a label's own fields (netclass, intersheet references) and a
  user-defined field's place on the sheet have no place in the IR; the fields of a symbol read from a KiCad file are not items (the
  importer keeps KiCad's origin, the fields keep theirs in the file); the fields' preference `m_AutoplaceFields.enable` is always on;
  Reorganize into Module Sheets lays the sheets out again and loses the stored places of fields, label spins and looks, and body styles;
  only the normal and the De Morgan body style are modelled (a symbol with named body styles draws its first two) and the symbol
  chooser always places in the normal style; pins cannot be selected (Swap Pins, pin-level highlight).
- Port from: `eeschema/sch_field.cpp`, `sch_label.cpp`, `dialogs/dialog_field_properties.cpp`, `dialog_label_properties.cpp`,
  `dialog_symbol_properties.cpp`.

### 13. Zones: fill fidelity and settings
Old #5. **Partial.** Hit: most boards. Blocks: no. WP5, size L. **Fill fidelity improved 2026-10-08: the filler follows `ZONE_FILLER::Fill`, and of 208 fills on 17 KiCad QA boards 190 are within 1% of kicad-cli's area (105 before), 182 within 0.5% (99 before); what is left is custom pads, Edge.Cuts arcs the importer turns into chords, and the hatch rings.**
- Exists: the filler (`crates/zone-filler`, `crates/drc/src/fill.rs`, `GET /api/fill`): zones fill from the highest priority down and are knocked out by each other's fills, islands go by
  connectivity and the zones below get the space back (iterative refill), thermal spokes are `buildThermalSpokes` (they turn with the pad, have its spoke angle, width and gap, and only the ones that
  reach copper stay), hatch fill (`addHatchFillTypeOnZone`) with thermal rings, chamfer and fillet corner smoothing, the board-edge clearance, and pad and footprint clearance overrides.
  Measured against kicad-cli, before and after, in `crates/zone-filler/PARITY.md`. A pad's or footprint's own connection (solid, thermal, PTH-only thermal, none), relief gap, spoke width, spoke angle and
  clearance beat the zone's: set in Footprint Properties for a footprint on the board (`Cmd::SetPadZoneOverrides`, `Cmd::SetFootprintZoneConnection`, one undo step each) and in the footprint
  editor's Pad Properties and Footprint Properties, read from and written to `.kicad_pcb` and `.kicad_mod`; Zone Properties edits the corner smoothing. Also: the full settings dialog
  (`components/ZoneDialog.tsx`); keepout knockouts; Fill and Unfill Selected, Merge, Duplicate onto Layer, the Priority actions and the Zone Manager (`PARITY-boardctl.md`); the importer reads zones (`import.rs::import_zones`).
- Missing: custom pad shapes (the importer keeps the anchor: the 16 custom pads of `issue5093` leave 766% on one of its zones and the ring pads of `stonehenge` 11%; the largest gap left); the thermal rings of a hatched zone (0.6 to 0.9% XOR); `connect_nearby_polys`;
  Edge.Cuts arcs (the importer keeps their chord: one `tessellate_arc` call in `import.rs::import_outline` would fix it, outside this item; the notch of `issue11814` costs its zones 2 to 23%), Edge.Cuts
  cutouts and open outlines, mask-only NPTH holes, courtyards and net ties as knockouts; one layer per zone, no non-copper zones, no zone lock or border style in Zone Properties (the name is edited in the Properties panel); no Auto-Assign
  Priorities; the Zone Manager has no preview; no footprint-level clearance, mask or paste fields in the footprint editor; a hatched zone with a fine pitch over a whole board is slow (minutes in a debug
  build on `issue5093`). Appendix C has the function table.
- Port from: `pcbnew/zone_filler.cpp`, `zone.cpp`, `zone_manager/`, `dialogs/panel_zone_properties.cpp`, `dialogs/dialog_non_copper_zones_properties.cpp`, `pcbnew/teardrop/`.

### 14. Snap, grid and the click-versus-drag rule
Old #17 (open) and #18 (partial). Hit: every drag. Blocks: no. WP3 for the PCB, WP1 for the schematic, size M.
- Missing: anchor snap works for Move and the picker only; draw, route, zone and shape clicks use the plain grid (`Canvas.tsx`
  `snapPoint`); no snap hysteresis or construction-line snap (header of `kicad-port/gridSnap.ts`); no per-category grids
  (`toggleGridOverrides` is recorded unwired); the schematic grid is a constant (`components/schematic/layout.ts` `GRID`) with no
  pin or anchor snap in Move. A press followed by a one-grid jitter counts as a drag, because `drag.moved` is set when the snapped
  delta is non-zero (`Canvas.tsx`); KiCad starts a drag after 8 px of travel (on macOS also after 300 ms held).
- Done since: the board, footprint and symbol editors have an editable grid list, fast grids and Edit Grids... (`PARITY-common.md`
  section 10) and a grid origin the server's placement snap follows. The schematic's constant grid means it offers none of the grid
  list, Next / Previous Grid, fast grids or Edit Grids; a grid choice there waits for the schematic's own snapping work.
- Port from: `pcbnew/tools/pcb_grid_helper.cpp`, `common/tool/grid_helper.cpp`, `eeschema/tools/ee_grid_helper.cpp`, `common/tool/tool_dispatcher.cpp`.

### 15. Interaction details that differ
New (from `CODE-COMPARE-ui.md`). **Partial.** Hit: constantly, one detail at a time. WP3, WP1 and WP4, size M and ongoing.
- 42 of that file's 266 rows were marked divergent (some since fixed), and about 200 handlers added since have not been compared.
  Re-verified open on `de88c02`: Enter during a PCB route finishes it instead of clicking (`common.Control.cursorClick`); Route From
  Other End only before the first fix; `V` does nothing outside a route; Select Connection needs a selection (PCB and schematic);
  schematic net highlight and Select Node miss pins; Ctrl+U never returns to mils; High Contrast has two states, not three;
  Alt+` (`toggleNetHighlight`) is still selection-driven while the faithful `toggleLastNetHighlight` has no hotkey; Mirror X then Y
  collapses to one flag; Add Corner inserts the cursor point instead of the nearest point on the edge.

### 16. The PCB context menu
Old #30. **Mostly done (2026-10-08).** Hit: constantly. Blocks: no. WP3, size S-M (done).
- Done (`PARITY-pcb.md` section 7): the menu is built the way KiCad builds it, from the entries every tool adds with its condition and order number
  (`kicad-port/pcbContextMenu.ts`: `ConditionalMenu` and `buildSelectionMenu`, in `PCB_EDIT_FRAME::setupTools` order, so the order is KiCad's), and lists only what
  applies: Select, Routing, Mirror / Rotate, Shape Modification, Position, Locking, Zones, Net Inspection Tools, Align/Distribute, Create from Selection and Grouping
  as submenus, then Cut, Copy, Paste, Duplicate and Delete, Zoom and Grid, and Properties last. The router's menu opens while a route is drawn, the drawing tools' while one runs, and the picker
  tools offer Cancel, Zoom and Grid. The entries run the existing actions (`components/canvas/pcbMenuBuilder.ts`, `actions/pcbMenuActions.ts`); `actions/pcbSweepMenu.ts`
  and the flat list in `Canvas.tsx` are gone. A right click keeps a selection and selects the item under the pointer only when there is none, as KiCad does.
- Missing: table cells, gate swap and generators have no IR, so no entries; the pad settings are listed dimmed (they belong to the Footprint Editor tab) and so are most
  of the router's via, posture and corner-mode actions (Place Through Via works while routing; Track Corner Mode always shows 45); Zone Priority raise and lower go by
  the zones' boxes overlapping, not their filled shapes.
- Port from: `pcbnew/tools/pcb_selection_tool.cpp`, `edit_tool.cpp`, `pcb_editor_conditions.cpp`, `board_inspection_tool.cpp`, `router/router_tool.cpp`.

### 17. Groups
Old #27. **Mostly done on the board (2026-10-08); the schematic and the footprint editor have none.** Hit: sometimes. Blocks: no. WP3, with WP1 for the schematic, size S-M.
- Done: Group, Ungroup, whole-group selection, enter and leave, Add Items, Remove Items and Group Properties (`PARITY-common.md` section 4: one undo step, `Cmd::EditGroup`);
  and, since 2026-10-08 (`PARITY-pcb.md` section 16): groups nest (`EDA_GROUP`: a group may hold groups; `crates/model/src/groups.rs`, `crates/ops/src/pcb_groups.rs`, a loop is
  refused, a group under two members dissolves up the tree); move, rotate, flip, Delete, Duplicate, Copy and Paste take the whole tree, with a locked leaf locking its group
  for the tools; a deleted member leaves its group; selecting a member picks the outermost group, a double click or Enter Group enters one (only its members can be picked),
  and Escape, a click outside it, selecting something outside it or Leave Group leave it; the entered group is drawn with its box and name and everything outside it dimmed, a selected
  group with its box and name; the `.kicad_pcb` file and the clipboard text hold the tree; the Grouping submenu of the right-click menu lists the four actions (item 16).
- Missing: items drawn or pasted while a group is entered do not join it (`BOARD_COMMIT::Push`); Create Array skips groups (item 22); no groups in the schematic or the
  footprint editor (their Group / Ungroup and the group dialogs are dimmed or do nothing there).
- Port from: `common/tool/group_tool.cpp`, `pcbnew/tools/pcb_group_tool.cpp`, `eeschema/tools/sch_group_tool.cpp` (the last one is open).

### 18. Appearance and display options
New. **Mostly done (2026-10-09).** Hit: every session. Blocks: no. WP3, size M (done).
- Done (`PARITY-pcb.md` section 24; `components/panels/AppearancePanel.tsx`, `components/panels/appearance/`, `kicad-port/appearance*.ts`, `layerPresets.ts`): KiCad's `APPEARANCE_CONTROLS`. The Objects tab lists 20 of
  its 22 rows (tracks, vias, pads, zones, filled shapes, footprints front and back, values, references, footprint text, ratsnest, DRC warnings, errors and exclusions, anchors, locked item shadow, colliding
  courtyards, board area shadow, drawing sheet, grid) with eyes and opacity sliders, and the painter and the pickers honour each (an object at opacity 0 is neither drawn nor picked); the Nets tab has per-net
  ratsnest eyes, colours (on copper in the "All" mode, on the ratsnest, or nowhere: `pcbnew.Control.netColorMode`), KiCad's right-click menu and Net Display Options; the Net Classes tab has eyes, colours and its menu;
  the Layers tab has the layer list's menu, "Inactive layers" Normal / Dim / Hide and the eight built-in layer presets plus saved ones; viewports save and recall. All of it is kept per project in `appearance.json`
  (the `.kicad_prl` and `.kicad_pro` split, never `design.json`) and written into the derived KiCad project (`.kicad_prl`, net colours, presets, viewports, and a coloured net class's definition).
- Missing: Images and Points rows (the board model has no reference images or snap points); Values shows nothing (no value text is drawn); the colour theme is not editable (swatches of layers and objects are read-only,
  so "Use Color from Schematic" stays dimmed); the quick switchers on Ctrl+Tab and Alt+Tab; the selection filter is not in the saved settings; the drawing sheet starts off (KiCad: on) and has no zone letters.
- Port from: `pcbnew/widgets/appearance_controls.cpp`.

### 19. Footprint and symbol editor leftovers
Old #8. **Partial.** Hit: library work. Blocks: no. WP6, size M.
- Exists: both editors with library trees, pad and pin tools, dialogs, tables, import and export (`PARITY-fpedit.md`, `PARITY-symedit.md`).
- Missing: more than one 3D model per footprint with offset, scale and rotation (one path today); custom-shape pads (unwired);
  the footprint fields grid (modelled, no UI); pad clearance and thermal overrides are stored but no check reads them; derived
  symbols; alternate pin functions; symbol fields on the canvas; a shape properties dialog.
- Port from: `pcbnew/dialogs/panel_fp_properties_3d_model.cpp`, `dialog_pad_properties.cpp`, `eeschema/symbol_editor/`.

### 20. Schematic view controls
Old #29. **Partial.** Hit: constantly. Blocks: no. WP1, size S.
- The PCB canvas pans with the middle or right button and auto-pans at the edge (opt-in, as in KiCad; `Canvas.tsx`). The footprint and
  symbol canvases pan with either button but never auto-pan. The schematic canvas pans with the middle button only
  (`components/SchematicView.tsx` handles button 1) and does not auto-pan while drawing a wire. The native pinch gesture is not
  ported on any canvas (`PARITY-pcb.md` section 1).
- Port from: `common/view/wx_view_controls.cpp`.

### 21. No Find on the PCB
New. **Mostly done (2026-10-08).** Hit: often on big boards. Blocks: no. WP3, size S-M (done).
- Done (`PARITY-pcb.md` section 7a): Find (Ctrl+F), Find Next (F3) and Find Previous (Shift+F3) on the board, from `DIALOG_FIND`: the search text with its history, Match case,
  Whole words only, Wildcards, Wrap, the footprint references, values, other texts, DRC markers and net names, Restart Search and the status line; the hit list in KiCad's
  order (footprints, texts, markers, nets); the hit selected (a net: its tracks and vias) and the view brought to it as `PCB_SELECTION_TOOL::FindItem` does; the dialog opens
  with the selected footprint's value or text (`kicad-port/pcbFind.ts`, `state/pcbFind.ts`, `actions/pcbFindActions.ts`, `components/PcbFindDialog.tsx`).
- Missing: Find by Properties (`pcbnew.EditorControl.findByProperties` stays unwired: it needs the property manager and a PCBEXPR search over every item, the reason is in
  `ui-parity-missing.json`); Include hidden fields does nothing (a footprint has no hidden field here) and a zone has no name, so none matches; the dialog's "Show search panel"
  link (`common.Interactive.search` still jumps to the Activity tab). Find and Replace is the schematic's only, as in KiCad.
- Port from: `pcbnew/dialogs/dialog_find.cpp`.

### 22. Arrays of footprints
Old #26. **Partial.** Hit: connector rows, LED grids. Blocks: partly. WP6 (after item 9), size M.
- Grid and circular arrays work for tracks, vias, zones, shapes and text, and Arrange Selection repositions placed footprints
  (`PARITY-pcb.md` section 17). New footprint copies and pad renumbering are impossible while a footprint's id is its schematic symbol's id.

### 23. The 3D viewer
Old honorable mention. **Partial.** Backlog, size L.
- Camera, trackball, view presets, lighting and materials follow `3d-viewer` (`PARITY-3d.md`); the toolbar is KiCad's own, with its icons, and
  the Appearance manager is a first version (view, show, render). The instant scene draws parts as boxes sized from the footprint's `F.Fab` body
  (courtyard when it has none); the real board with its models comes from kicad-cli's GLB export in the background and takes minutes
  (`components/viewer3d/Viewer3D.tsx`, `GET /api/board.glb`). A footprint without a `(model ...)` gets KiCad's model for its package from an
  explicit table (`crates/model/src/footprint.rs::kicad_footprint_for`: 0402/0603/0805 passives and LEDs by reference prefix, SOT-23 and SOT-223, SOIC, TSSOP, MSOP, pin headers), and the export draws
  copper on a net the netlist lost instead of refusing the board. Missing: the Appearance manager's layer tree and stackup colours, models for
  every other package, hover highlight, raytracing, camera animation, a zone toggle.

### 24. Item kinds the IR does not have
New. **Open, deferred.** Backlog.
- Text boxes, tables, reference images, barcodes, points, custom-shape pads, derived symbols, design blocks and generators, each with
  its reason in `ui-parity-missing.json`. Add one when a work package needs it.

## Work packages

Each is sized for one agent. Begin each with a `CODE-COMPARE` pass over the handlers in its area (the newer ones are uncompared) and
end each with click-through or e2e checks (`web/studio/e2e/`): most earlier parity work was typechecked and unit-tested but never clicked.
A fix counts when the C++ function is named in the code, the logic has a `kicad-port/*.test.ts`, one user action is one undo step,
and the item is struck here and in its `PARITY` doc.

Shared files to sequence: `actions/useActionRunner.ts` (WP1, WP3, WP4), `components/SchematicView.tsx` (WP1, WP2),
`crates/ops/src/lib.rs` (all), `state/store.tsx` (WP1-WP3), `crates/cli/src/board.rs` (WP2, WP5, WP6). Do WP2 step 1 (sheet
addressing) before WP1 adds verbs, and WP5 step 2 (the model overlay) before WP6 adds parts.

**WP1. Schematic editing fidelity** (items 1, 6, 12, 20; schematic parts of 14, 15, 17). Size XL.
- Files: `web/studio/src/components/SchematicView.tsx`, `components/schematic/`, `kicad-port/sch*.ts`, `actions/sch*Actions.ts`, the eeschema
  block of `actions/useActionRunner.ts`, schematic cases of `state/store.tsx`; `crates/ops/src/sch_edit.rs` and the schematic verbs in
  `crates/ops/src/lib.rs`, `crates/model/src/{ir.rs,sch_extras.rs}`, `crates/kicad/src/{sch_extras_io.rs,sch_import.rs}`.
- KiCad: `eeschema/tools/` (`sch_move_tool`, `sch_edit_tool`, `sch_selection_tool`, `sch_editor_control`, `sch_align_tool`,
  `sch_drag_net_collision`, `sch_group_tool`, `ee_grid_helper`), `eeschema/{sch_field,sch_label,sch_symbol,sch_line}.cpp`,
  `eeschema/autoplace_fields.cpp`, `eeschema/dialogs/dialog_{label,field,symbol,wire_bus}_properties.cpp`, `common/view/wx_view_controls.cpp`.
- Order: move, drag, rotate and mirror for every item kind with wire stretch (1); clipboard (6, done 2026-10-08); fields and labels (12); view controls and snap.

**WP2. Schematic hierarchy and connectivity** (item 4). Size L. Go first.
- Files: `crates/ops/src/lib.rs` (add a sheet path to the schematic verbs and `schematic_mut`), `crates/cli/src/board.rs` (`reconcile_schematic`,
  the schematic undo scope), `crates/kicad/src/{sch_import.rs,lib.rs}` and `eda_kicad::reconcile`, `web/studio/src/state/store.tsx` (`currentSheetPath`),
  `components/panels/HierarchyPanel.tsx`, `kicad-port/{navHistory,schSheet,schSheetPins}.ts`, `components/AnnotateDialog.tsx`.
- KiCad: `eeschema/{sch_sheet_path,sch_screen,sch_sheet,sch_sheet_pin,connection_graph,sch_reference_list,annotate}.cpp`,
  `eeschema/tools/{sch_navigate_tool,sch_editor_control}.cpp`, `eeschema/sync_sheet_pin/`, `eeschema/dialogs/dialog_annotate.cpp`.
- Order: sheet-addressed verbs; cross-sheet nets in `reconcile`; edit tools on nested sheets; Import Sheet and the navigation actions; annotation.

**WP3. PCB editing, selection and display** (items 8, 10, 14, 15, 16, 17, 18, 21; array of footprints with WP6). Size XL.
- Files: `web/studio/src/components/canvas/` (`Canvas.tsx`, `ContextMenu.tsx`, `clipboard.ts`, `selectionCandidates.ts`), `state/store.tsx`,
  the pcbnew blocks of `actions/useActionRunner.ts` and `actions/{pcb*Sweep,boardControlActions}.ts`, `kicad-port/{gridSnap,pcbEditActions,alignDistribute,selection,pcbSelectionOps,pcbReference}.ts`,
  `components/panels/{PropertiesPanel,AppearancePanel}.tsx`; Rust: move and rotate verbs for tracks, zones, text and shapes in `crates/ops/src/lib.rs`.
- KiCad: `pcbnew/tools/{edit_tool,edit_tool_move_fct,pcb_selection_tool,pcb_grid_helper,align_distribute_tool,pcb_group_tool,pcb_control,board_editor_control}.cpp`,
  `common/tool/{tool_dispatcher,grid_helper,selection_tool}.cpp`, `pcbnew/widgets/{appearance_controls,pcb_properties_panel}.cpp`, `common/widgets/properties_panel.cpp`,
  `pcbnew/kicad_clipboard.cpp`, `pcbnew/dialogs/dialog_find.cpp`.
- Order: item coverage for Move, Rotate, Flip, Copy (8); drag threshold and snap (14); context menu (16); Properties panel (10); Appearance, Find, groups.

**WP4. Router** (item 7). Size XL. Independent of the rest.
- Files: `crates/pns/src/*`, `crates/cli/src/{route_api,tune_api}.rs`, `web/studio/src/components/canvas/{routing,diffPairRouting,dragging}.ts`,
  `kicad-port/{routeTool,dpTool,dragTool}.ts`, `components/{RouterSettingsDialog,LengthTuningDialog}.tsx`, `actions/pcbRouterSweep.ts`.
- KiCad: `pcbnew/router/` (`pns_shove`, `pns_walkaround`, `pns_dragger`, `pns_line_placer`, `pns_diff_pair_placer`, `pns_meander*`, `pns_optimizer`,
  `pns_router`, `router_tool.cpp`), `pcbnew/generators/pcb_tuning_pattern.cpp`.
- Order: ~~D5 solids pre-pass and `onCollidingSolid`; optimizer over shoved lines (D6, D10, D12)~~ (done 2026-10-08); dragger segment slide and 45-degree corners (D7);
  start and end mid-segment, Route From Other End; diff-pair coupling and vias; interactive tuning; arcs; the dead settings (D16).

**WP5. Rules, zones and KiCad file fidelity** (items 2, 3, 11, 13). Size XL; split after step 2 if two agents are free.
- Files: `crates/kicad/src/{pcb,import,sch_import,lib,custom_rules}.rs`, `crates/kicad-engine/src/lib.rs` (project file, `--schematic-parity`),
  `crates/model/src/{lib,ir}.rs` (the `BoardRules` overlay), `crates/cli/src/board.rs::load`, new verbs in `crates/ops/src/`, `crates/zone-filler`, `crates/drc/src/{board,fill}.rs`;
  `web/studio/src/components/{BoardSetupDialog,SchematicSetupDialog,DrcDialog,ErcDialog,ZoneDialog,ZoneManagerDialog}.tsx`.
- KiCad: `pcbnew/pcb_io/kicad_sexpr/` (writer and parser), `eeschema/sch_io/kicad_sexpr/`, `pcbnew/dialogs/dialog_board_setup.cpp` and `panel_setup_*.cpp`,
  `common/dialogs/panel_setup_{netclasses,severities}.cpp`, `pcbnew/drc/drc_rule_parser.cpp`, `pcbnew/tools/drc_rule_editor_tool.cpp`, `pcbnew/dialogs/dialog_drc.cpp`,
  `eeschema/dialogs/dialog_{schematic_setup,erc}.cpp`, `pcbnew/zone_filler.cpp`, `pcbnew/zone.cpp`, `pcbnew/teardrop/`.
- Order: (1) writer: keepouts, per-zone settings, dimensions, groups, locks, arcs, teardrops, with a round-trip test and a fresh `scores.json` (**done 2026-10-08**);
  (2) the `design.json` overlay for `BoardRules` and the Board Setup pages (**done 2026-10-08**); (3) severities and the DRC and ERC review workflow (**done 2026-10-08**: Violation Severity in Board Setup and Schematic Setup, DRC exclusions, `--schematic-parity`, the
  Ignored Tests and Schematic Parity pages, marker menus, Next / Previous / Exclude Marker; item 11 says what is left); (4) zone filler fidelity (**done 2026-10-08**; item 13 lists what is left).

**WP6. Libraries, parts and footprints on the board** (items 5, 9, 19, 22). Size XL.
- Files: `crates/kicad/src/{symbol_lib,footprint_lib}.rs`, `crates/cli/src/{studio,library_api}.rs`, `crates/model/src/{ir,footprint,symbol}.rs` (`FootprintInstance`,
  instance fields), `crates/ops/src/{lib,library_editors}.rs`; `web/studio/src/components/{SymbolChooserDialog,FootprintPropertiesDialog,PcbParityDialogs}.tsx`,
  `components/{library,footprint,symbol}/`, `actions/{footprintLibraryOps,symbolLibraryOps,libraryEditorActions}.ts`.
- KiCad: `common/libraries/{library_manager,library_table}.cpp`, `common/{lib_tree_model_adapter,footprint_info}.cpp`, `eeschema/libraries/symbol_library_adapter.cpp`,
  `eeschema/{symbol_chooser_frame,symbol_library_manager}.cpp`, `pcbnew/{footprint_chooser_frame,footprint_library_adapter,pcb_field}.cpp`,
  `pcbnew/dialogs/{dialog_footprint_properties,dialog_exchange_footprints,dialog_update_pcb}.cpp`, `pcbnew/tools/{board_editor_control,footprint_editor_control,pad_tool}.cpp`.
- Order: library tables and the chooser (5; the choosers are done, 2026-10-08, the tables are not); board footprint fields, attributes and pad selection (9); Change and Update Footprints and board-only footprints once the overlay exists; editor leftovers (19); arrays (22).

## Appendix A. The original 30, one verdict each

| # | Gap | Verdict | Evidence, or where it went |
|---|---|---|---|
| 1 | Schematic editing | **Closed** | 124 eeschema handlers (`actions/useActionRunner.ts`, `schEditActions.ts`), `components/SchematicView.tsx`, verbs in `crates/ops/src/{lib.rs,sch_edit.rs}`, one-netlist rule in `crates/cli/src/board.rs::reconcile_schematic` (test `schematic_wire_connects_and_disconnects_pins_on_one_netlist`). Behaviour gaps: items 1, 6, 12. |
| 2 | DRC engine hangs | Out of scope | kicad-cli is the only engine (`docs/ARCHITECTURE.md`); the Rust copy is deleted. |
| 3 | DRC precision | Out of scope | same |
| 4 | ERC has no UI | **Closed** | `components/ErcDialog.tsx`, `GET /api/erc` to `crates/kicad-engine/src/lib.rs::erc`, exclusions `Cmd::AddErcExclusion`, pin map `SchematicSetupDialog.tsx`. Review workflow: item 11. |
| 5 | Zone fill | Partial | `crates/zone-filler`, `crates/drc/src/fill.rs`, `ZoneDialog.tsx`, `ZoneManagerDialog.tsx`; item 13 and item 2. |
| 6 | Hierarchical sheets | Partial | IR, import, export, panel, navigation exist (`ir.rs`, `sch_import.rs`, `HierarchyPanel.tsx`); item 4. |
| 7 | Routing | Partial | `crates/pns`; item 7. |
| 8 | Footprint editor and pad tool | Partial | editor, pad tools, dialogs, import and export (`components/footprint/`, `crates/ops/src/library_editors.rs`); item 19. |
| 9 | Board Setup | Partial | `BoardSetupDialog.tsx`: all 10 pages edit (rules overlay and 7 verbs, item 3); 8 of KiCad's pages have no model yet. |
| 10 | Net classes and rules | Partial | importer (`crates/kicad/src/import.rs::merge_project_net_classes`, `custom_rules.rs`) and editing (Net Classes, Custom Rules, Assign Netclass) done, item 3; open: regex patterns, the rule-tree designer. |
| 11 | Property dialogs | Partial | via, shape, zone, text, dimension and track width edit (`Cmd::EditVia`, `EditShape`, `EditZone`, `EditText`); footprint, pads, panel: items 9, 10. |
| 12 | Move excludes tracks and zones | **Closed** | `Cmd::MoveItems`, `RotateItems`, `FlipItems` (`crates/ops/src/pcb_transform.rs`), `kicad-port/pcbTransform.ts`, `pcbEditActions.ts::movableItem` for every kind; item 8. |
| 13 | Selection modifiers and box select | **Closed** | `kicad-port/selection.ts`, `components/canvas/selectionCandidates.ts::collectBoxSelection`, `Canvas.tsx`; `PARITY-pcb.md` section 3. |
| 14 | Clipboard | Partial | PCB: every item kind, footprints included, in KiCad's clipboard format (`crates/kicad/src/clipboard.rs`, `Cmd::PasteClipboard`, `Cmd::Duplicate`, `components/canvas/clipboard.ts`); schematic: done in KiCad's format (`Cmd::PasteSch`, item 6). Left: Paste Special on the PCB (item 8). |
| 15 | Cross-tab undo | **Closed** | `crates/ops` `Domain`, `crates/cli/src/board.rs::restore_domain` (test `undo_redo_are_scoped_to_the_tab_that_asked`); the Footprint and Symbol tabs undo in their own scopes. |
| 16 | Hotkey extraction | **Closed** | `web/studio/tools/lib/actionsParser.js::extractPlatformRaw` (with test), `src/kicad/actions.json` (`common.Interactive.redo` is Ctrl+Y), `actions/hotkeys.ts::effectiveHotkey`. |
| 17 | Click-versus-drag threshold | **Open** | `Canvas.tsx` sets `drag.moved` on a non-zero snapped delta; item 14. |
| 18 | Snapping | Partial | `kicad-port/gridSnap.ts`, `components/canvas/gridHelper.ts` (Move and picker); item 14. |
| 19 | DRC schematic parity | Out of scope | kicad-cli has `--schematic-parity`; the dialog passes it (2026-10-08, item 11): the Schematic Parity page lists what it reports. |
| 20-24 | ERC bus and hierarchy, multi-unit, SI, library-sync, DFM checks | Out of scope | kicad-cli runs these. |
| 25 | Align and distribute | Partial | PCB: every item kind, locks respected (`state/store.tsx`, `kicad-port/alignDistribute.ts`); schematic Align: item 1. |
| 26 | Array tool | Partial | `Cmd::CreateArray`, `CreateArrayDialog.tsx`; item 22. |
| 27 | Grouping | Partial | board: done, nested (`Cmd::Group` family in `crates/ops/src/pcb_groups.rs`, `kicad-port/groupTree.ts`, `state/store.tsx::withGroupSubstitution`, the entered-group overlay in `painter.ts`, the `.kicad_pcb` file); the schematic and the footprint editor have no groups; item 17. |
| 28 | Dimensions and measure | **Closed** | `crates/connectivity/src/dimension.rs`, `Cmd::AddDimension` family, `components/DimensionPropertiesDialog.tsx`, the measure tool. Left: the interactive height click, text border, manual text position, export (item 2). |
| 29 | Pan | Partial | `kicad-port/viewControls.ts`, `Canvas.tsx` (PCB); item 20. |
| 30 | Context menu | **Closed** | `kicad-port/pcbContextMenu.ts` (KiCad's `CONDITIONAL_MENU` and the entries of every tool's `Init()`), `pcbSelectionSummary.ts`, `components/canvas/pcbMenuBuilder.ts`, `Canvas.tsx`. Left: table cells, gate swap, generators, most of the router's via and posture entries (item 16, item 7). |

## Appendix B. Out of scope and deferred

- **kicad-cli's job** (`docs/ARCHITECTURE.md`): DRC, ERC, plots, Gerbers, drill and position files, netlist, BOM, statistics, STEP, GLB, VRML,
  IPC-2581, ODB++, D356 and GenCAD. Old #2, #3 and #19-#24. The UI around them is in scope (item 11). IDF, Hyperlynx, Specctra and the footprint report stay unwired: kicad-cli has none.
- **Python scripting and plugins**: `common.API.pluginsReload`, `pcbnew.ScriptingTool.*`, the footprint wizards (`pcbnew.FpWizard.*`, `ModuleEditor.createFootprint`).
- **Project management**: New, Open, Revert, Save All, Open Non-KiCad Board, Append Board, Rescue.
- **Simulator, last**: `eeschema.Simulation.*` and `eeschema.Simulator.*` need ngspice.
- **Deferred until the IR has the item** (reasons in `ui-parity-missing.json`): design blocks (8 actions), generators (8), tables (5), text boxes, barcodes, points,
  reference images, DXF and SVG import, custom pads, derived symbols; the autoplacer and the microwave tools are declined.

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
| `buildCopperItemClearances` | partial: no courtyard knockouts, no net-tie exemptions; **the board edge now** (`copper_edge_clearance` against the outer Edge.Cuts outline; no interior cutouts); a pad's or footprint's own clearance override replaces the net class's and zone's for that pad, floored at the board minimum (`DRC_ENGINE::EvalRules`) |
| `ZONE::BuildSmoothedPoly` (`smoothed.rs`, `corner.rs`) | ported: same-net zones merged, the board outline, the minimum-width apron, and the chamfer and fillet corner smoothing |
| `connect_nearby_polys` | missing |
| `fillNonCopperZone` | missing |
| `buildHatchZoneThermalRings`, `addHatchFillTypeOnZone` (`hatch.rs`) | ported: thickness, gap, orientation, smoothing level and amount, minimum hole area, and the thermal rings round pads and vias that are connected to a hatched zone |
