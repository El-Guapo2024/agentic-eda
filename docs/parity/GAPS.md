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
  **6 closed, 14 partial, 2 open, 8 out of scope** (kicad-cli covers DRC, ERC and the exports).
- **12 items are new**, found in `PARITY-*.md`, `CODE-COMPARE-*.md` and by reading the code. The ranked list marks them.
- **Three findings change the picture.**
  1. *A wired action is not a working feature.* `UI-ACTIONS.md` counts schematic Move, Drag, Rotate, Mirror,
     Properties and Align as wired, but they act on symbols only: a label, wire, text, power symbol or sheet
     cannot be moved or rotated. On the PCB, Move skips tracks and zones, Rotate and Flip take footprints and
     vias only, and Duplicate and Copy skip footprints.
  2. *The KiCad files we write drop design data, and kicad-cli judges those files.* The PCB writer
     (`crates/kicad/src/pcb.rs`) gives every zone the board's default clearance and width instead of its own
     settings, writes no keepout, no dimensions, no groups, no locks, and turns arc tracks into 32 segments.
     By reading the code (not run), a rule area has no net and the writer's net check rejects it, so a board
     with a keepout cannot be DRC'd or exported. Confirm that with a test first; it is the quickest win here.
  3. *One root sits under several gaps:* the constraint model built from the intent has no edit verbs. Net
     classes, custom rules, adding or renaming a part, assigning a footprint and importing a netlist are all
     recorded as unwired for that reason. `design.json` already overrides the model in places (`design.nets`,
     the footprint and symbol libraries); extending that overlay unlocks items 3, 5 and 9.
- **Docs.** Action level: `UI-ACTIONS.md`. Behaviour tables: `web/studio/PARITY-{pcb,sch,3d,fpedit,symedit,boardctl}.md`.
  Code level: `CODE-COMPARE-ui.md` (the first 266 handlers; about 200 added since have not been compared) and
  `CODE-COMPARE-router.md`, `crates/pns/PARITY.md`, `crates/zone-filler/PARITY.md`. Measured round trip and
  connectivity: `REPORT.md` (2026-10-04, needs a re-run). Engines: `ARCHITECTURE.md`.

## Ranked list: open and partial items, by user impact

"Hit" is how often a KiCad user meets it; "Blocks" is whether a workflow stops. WP is the work package below.
"Port from" paths are under the KiCad source root.

### 1. Schematic edit tools act on symbols only
New (the residual of #1). **Partial.** Hit: every schematic session. Blocks: yes, moving a label or wire means delete and redraw. WP1, size L.
- Exists: select, box-select, Select All, Delete, Lock, Change To and the context menu cover every item kind
  (`kicad-port/schItemGeom.ts`, `schDelete.ts`, `schContextMenu.ts`, `components/schematic/SchContextMenu.tsx`).
- Missing: Move and Drag (`M`, `G`, click-drag), Rotate, Mirror, Properties (`E`) and Align act on placed symbols
  only. The handlers in `actions/useActionRunner.ts` (`eeschema.InteractiveMove.move/drag`, `InteractiveEdit.rotateCCW`,
  `mirrorH`, `properties`) return unless the item is a symbol; `components/SchematicView.tsx` starts a drag only on
  a symbol; `crates/ops` has `MoveSymbol` and `DragSymbol` and no verb that moves a wire, label, power symbol, text,
  junction, no-connect, bus entry or sheet. A plain click on a wire toggles net highlight instead of selecting it.
  No drag of a wire segment (`G` on a wire stretches its neighbours). Align and Align to Grid move symbols without
  their wires, so pins leave the connection grid and stop connecting (`CODE-COMPARE-ui.md` item 9).
- Port from: `eeschema/tools/sch_move_tool.cpp`, `sch_edit_tool.cpp`, `sch_selection_tool.cpp`, `sch_drag_net_collision.cpp`, `sch_align_tool.cpp`.

### 2. The KiCad files we write drop design data
New (the old "round trip" mention). **Partial.** Hit: any board with a pour, keepout or dimension; every export. Blocks: yes for keepouts (by reading), silently wrong otherwise. WP5, size L.
- Exists: tracks, vias, pads, footprints, shapes, text, net classes, the drill origin, fills from `crates/zone-filler`;
  the schematic writer carries its extras and sub-sheets (`crates/kicad/src/lib.rs`, `sch_extras_io.rs`); the importer reads
  zones, net classes, project rules and `.kicad_dru` (`import.rs`, `custom_rules.rs`).
- Missing, read from `crates/kicad/src/pcb.rs` (`:285-291` is the zone block): each zone gets the board's default clearance and
  minimum width and none of its own priority, pad connection, thermal gap and spoke, hatch or island settings; a rule area is
  written as a pour and, having no net, fails the net check (`kicad.unknown_net`) that rejects the whole export, so DRC and
  every output on a board with a keepout fail (not run; `PARITY-pcb.md` section 14 contradicts itself on this); dimensions,
  groups, locks and teardrop flags are not written, and the importer reads neither dimensions nor groups; arc tracks
  (`Track::arc`) are written as 32 segments. `REPORT.md` (2026-10-04) measured re-export changing kicad-cli's verdict on 8
  of 12 real boards (`scores.json`, 33.3%); re-measure after the fixes.
- Port from: `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` and `pcb_io_kicad_sexpr_parser.cpp`.

### 3. Board Setup and the rules cannot be edited
Old #9 and #10. **Partial.** Hit: the start of every project. Blocks: yes, clearances, net classes and the stackup change only by editing the intent file. WP5, size L.
- Exists: `components/BoardSetupDialog.tsx` has 7 pages; 3 edit (Track Widths and Vias, Teardrops, Dimensions:
  `Cmd::SetTrackWidthPresets`, `SetViaPresets`, `SetTeardropSettings`, `SetDimensionSettings`) and 4 mirror read-only
  (Net Classes, Design Rules, Text and Graphics, Layer Stackup). The importer reads net classes, project rules, severities
  and `.kicad_dru`; the writer emits net classes.
- Missing: editing net classes and their net-name patterns (`assignNetclass`), constraints (minimum clearance, track,
  annular ring, hole, edge), solder mask and paste, zone defaults, text defaults, the physical stackup (layer count,
  thickness), tuning profiles, violation severities, the custom rule editor (`DRETool.drcRuleEditor`). The cause is that
  `BoardRules` comes from the intent and has no edit verb or undo (header of `BoardSetupDialog.tsx`;
  `ui-parity-missing.json`). Fix pattern: a `design.json` overlay applied in `crates/cli/src/board.rs::load`, as `design.nets` is.
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
New (the old "library browsers" mention). **Open.** Hit: every session that adds a part. Blocks: yes, a part must come from a library the design already uses, or the built-in catalog. WP6, size L.
- Exists: the Symbol Chooser with search and preview (`components/SymbolChooserDialog.tsx`); library trees for the
  project library (`components/library/`); `.kicad_sym` and `.kicad_mod` reading from one root set by
  `EDA_KICAD_SYMBOLS` and `EDA_KICAD_FOOTPRINTS`. The Footprint and Symbol editors' trees also list every installed
  KiCad library (155 footprint, 223 symbol) beside the project's own: names one library at a time, cached
  (`crates/cli/src/library_index.rs`, `GET /api/library/index|items|all`, `components/library/LibraryTree.tsx`); opening an
  installed item copies it into the project library first. The two choosers do not use that index yet: placing an installed
  symbol needs `add_symbol` to resolve its definition into the schematic's `lib_symbols`, search needs the symbols' descriptions
  (names only are indexed), and KiCad's chooser is a tree rather than a flat list.
- Missing: `GET /api/symbol_library` (`crates/cli/src/studio.rs::symbol_library_json`) lists only libraries the design
  references plus the built-in catalog; footprints resolve by name and are never enumerated
  (`crates/kicad/src/footprint_lib.rs`). No library tables (user and project libraries, Add and New Library, Configure
  Paths), no symbol or footprint browser, a plain search list instead of the chooser's recently-used and filter tabs
  (`PARITY-sch.md` section 3), and no footprint chooser: `placeFootprint` lists unplaced schematic parts only.
- Port from: `common/libraries/library_manager.cpp`, `library_table.cpp`, `common/lib_tree_model_adapter.cpp`,
  `common/footprint_info.cpp`, `eeschema/libraries/symbol_library_adapter.cpp`, `eeschema/symbol_chooser_frame.cpp`,
  `pcbnew/footprint_chooser_frame.cpp`, `pcbnew/footprint_library_adapter.cpp`.

### 6. The schematic has no clipboard
New (the schematic half of old #14). **Open.** Hit: every session. Blocks: partly; Repeat Last Item (`kicad-port/schRepeat.ts`) is the only workaround. WP1, size M.
- Missing: cut, copy, paste, duplicate and Paste Special do nothing on the Schematic tab (`common.Interactive.copy/paste` are
  `pcbOnly`; `duplicate` serves the PCB and footprint tabs, `actions/useActionRunner.ts`). Needed: selected items with their
  wires and labels, paste carried by the cursor, annotation of pasted symbols, paste across sheets, and KiCad's clipboard
  format so copy and paste work with real KiCad.
- Port from: `eeschema/tools/sch_editor_control.cpp` (`doCopy`, `Paste`), `common/clipboard.cpp`, `eeschema/sch_io/kicad_sexpr/`.

### 7. The router: shove and drag are not KiCad's
Old #7. **Partial.** Hit: every routing session in Shove mode (Walkaround, the default, works). Blocks: shove, the router's headline feature. WP4, size XL.
- Exists: `crates/pns` ports hulls, `LINE::Walkaround`, shove of tracks and vias, the optimizer with smart pads, loop removal,
  diff-pair routing, single-track and diff-pair length and skew tuning (dialog-driven), and `D`/`G` drag of a corner or via
  (`crates/pns/PARITY.md`; `CODE-COMPARE-router.md` D1-D4 and D8 are fixed).
- Missing (`CODE-COMPARE-router.md`): shove gives up at the first pad (`crates/pns/src/shove.rs`, `Item::Solid(_) => return None`)
  and has no solids-only pre-pass or `onCollidingSolid`, so near pads Shove degenerates to Walkaround (D5); via push over- and
  under-shoots (D6); a drag moves the nearer end instead of sliding the segment, at any angle, and is refused onto obstacles in
  the default mode (D7); shoved lines get no optimizer pass and widths are normalised (D10); a route cannot start or end mid-segment
  and Route From Other End works only before the first fix; a diff pair has no coupled shove or walkaround and no via; length tuning
  is a dialog on straight axis-aligned tracks that builds a 45-degree accordion, not KiCad's U meander (`meander.rs`); no arcs
  (`ARC_T`), mouse-trail posture or springback; six `RoutingSettings` fields are never read (D16).
- Port from: `pcbnew/router/` (`pns_shove.cpp`, `pns_walkaround.cpp`, `pns_dragger.cpp`, `pns_line_placer.cpp`,
  `pns_diff_pair_placer.cpp`, `pns_meander*.cpp`, `router_tool.cpp`), `pcbnew/generators/pcb_tuning_pattern.cpp`.

### 8. PCB edit tools skip item kinds
Old #12, and the PCB parts of #14 and #25. **Open** for #12, partial for the rest. Hit: every layout session. Blocks: partly. WP3, size M-L.
- Missing: Move and drag have no verb for tracks and zones (`kicad-port/pcbEditActions.ts::movableItem`: "tracks and zones have no
  move Cmd"; Move Individually, Move with Reference and Position Relative skip them too). Rotate and Flip take footprints and vias
  only (`state/store.tsx` `rotateSelection`, `flipSelection`), so `R` on text or a graphic does nothing. Duplicate, Copy and Paste skip
  footprints, dimensions and groups, and the clipboard is in-app, not KiCad's format (`components/canvas/clipboard.ts`). Align and
  Distribute take placed footprints only and ignore locks and the cursor target (`store.tsx` `alignSelection`,
  `kicad-port/alignDistribute.ts`). Pads are not selectable (`SelectableKind` in `components/canvas/selectionCandidates.ts`).
- Fix: client-side as one `commit_route` or `set_zone_outline` batch, or `MoveTrack` and `MoveZone` verbs.
- Port from: `pcbnew/tools/edit_tool.cpp`, `edit_tool_move_fct.cpp`, `align_distribute_tool.cpp`, `pcbnew/kicad_clipboard.cpp`.

### 9. Footprints on the board are not editable objects
New (the residual of old #11; blocks #26). **Open.** Hit: every board (silkscreen cleanup, mounting holes). Blocks: partly. WP6, size L.
- Exists: pose, side and a four-way reference text side (`FootprintInstance` in `crates/model/src/ir.rs`, `Cmd::SetLabelSide`,
  `components/FootprintPropertiesDialog.tsx`).
- Missing: reference and value text position, size, layer and visibility, user fields; per-instance attributes (DNP, exclude from
  BOM or position files); pad selection and per-pad overrides; board-only footprints (mounting holes, fiducials, logos: the
  microwave tools are declined for the same reason); Change Footprint(s), Update Footprints from Library and geographical
  reannotate (recorded unwired: a part's footprint comes from the intent, which has no verb); new copies of a footprint (Duplicate, Array).
- Port from: `pcbnew/dialogs/dialog_footprint_properties.cpp`, `dialog_exchange_footprints.cpp`, `dialog_update_pcb.cpp`,
  `pcbnew/pcb_field.cpp`, `pcbnew/tools/board_editor_control.cpp` (`PlaceFootprint`).

### 10. The Properties panel is a read-only summary
New (old #11). **Partial.** Hit: constantly. Blocks: no, dialogs edit most things. WP3, size M.
- Exists: a key/value summary of one footprint or symbol with Rotate and Delete buttons (`components/panels/PropertiesPanel.tsx`).
- Missing: an editable property grid for every item kind (position, angle, layer, width, net, size, text) and for a multi-selection.
- Port from: `common/widgets/properties_panel.cpp`, `pcbnew/widgets/pcb_properties_panel.cpp`, `eeschema/widgets/sch_properties_panel.cpp`.

### 11. The DRC and ERC dialogs lack the review workflow
New (the UI half of old #19). **Partial.** Hit: every review pass. Blocks: partly. WP5, size M.
- Exists: Run DRC and Run ERC through kicad-cli with a running state and out-of-date marking, lint tabs, ERC exclusions
  (`Cmd::AddErcExclusion`) and the ERC pin map (`components/DrcDialog.tsx`, `ErcDialog.tsx`, `SchematicSetupDialog.tsx`).
- Missing: DRC exclusions; the Ignored Tests and Schematic Parity tabs are empty (`DrcDialog.tsx` `STUB_TABS`) and
  `--schematic-parity` is never passed (`crates/kicad-engine/src/lib.rs`); per-check severities for the board and the schematic
  (Schematic Setup has the pin map page only); Next, Previous and Exclude Marker; a marker context menu.
- Port from: `pcbnew/dialogs/dialog_drc.cpp`, `eeschema/dialogs/dialog_erc.cpp`, `common/dialogs/panel_setup_severities.cpp`, `eeschema/dialogs/dialog_schematic_setup.cpp`.

### 12. Schematic symbols, fields and labels carry no per-instance geometry
New. **Open.** Hit: every schematic cleanup. Blocks: no. WP1, size L.
- Missing: the IR stores no position, size, visibility or orientation for a symbol's Reference, Value and other fields
  (`SymbolInstance` in `ir.rs`; Autoplace Fields is recorded unwired for this reason) and none for a label (`NetLabel` is net,
  point and kind; its spin is read off the wire), so no label rotation, size, justification or shape; DNP and exclusion flags
  (the schematic-control agent is adding some, so take its IR fields); an alternate body style cannot show on a placed symbol
  (`to_engine_symbol` drops style 2); pins cannot be selected (Swap Pins, pin-level highlight).
- Port from: `eeschema/sch_field.cpp`, `sch_label.cpp`, `autoplace_fields.cpp`, `dialogs/dialog_field_properties.cpp`,
  `dialog_label_properties.cpp`, `dialog_symbol_properties.cpp`.

### 13. Zones: fill fidelity and settings
Old #5. **Partial.** Hit: most boards. Blocks: no. WP5, size L.
- Exists: the filler (`crates/zone-filler`, `crates/drc/src/fill.rs`, `GET /api/fill`) measured against kicad-cli at 6 of 8 fills
  within 0.2% (`crates/zone-filler/PARITY.md`); the full settings dialog (`components/ZoneDialog.tsx`); keepout knockouts; island
  removal; Fill and Unfill Selected, Merge, Duplicate onto Layer, the Priority actions and the Zone Manager (`PARITY-boardctl.md`);
  the importer reads zones (`import.rs::import_zones`).
- Missing: hatch fill falls back to solid (`zone-filler/src/lib.rs`); thermal spokes are four axis-aligned bars (`spokes.rs`);
  no per-pad connection overrides, no `connect_nearby_polys`, no iterative refill; one layer per zone, no non-copper zones, no
  corner smoothing, no zone name, lock or border style; no Auto-Assign Priorities; the Zone Manager has no preview; the
  exporter ignores the settings (item 2). Appendix C has the function table.
- Port from: `pcbnew/zone_filler.cpp`, `zone.cpp`, `zone_manager/`, `dialogs/panel_zone_properties.cpp`, `dialogs/dialog_non_copper_zones_properties.cpp`, `pcbnew/teardrop/`.

### 14. Snap, grid and the click-versus-drag rule
Old #17 (open) and #18 (partial). Hit: every drag. Blocks: no. WP3 for the PCB, WP1 for the schematic, size M.
- Missing: anchor snap works for Move and the picker only; draw, route, zone and shape clicks use the plain grid (`Canvas.tsx`
  `snapPoint`); no snap hysteresis or construction-line snap (header of `kicad-port/gridSnap.ts`); no per-category grids
  (`toggleGridOverrides` is recorded unwired); the schematic grid is a constant (`components/schematic/layout.ts` `GRID`) with no
  pin or anchor snap in Move. A press followed by a one-grid jitter counts as a drag, because `drag.moved` is set when the snapped
  delta is non-zero (`Canvas.tsx`); KiCad starts a drag after 8 px of travel (on macOS also after 300 ms held).
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
Old #30. **Partial.** Hit: constantly. Blocks: no. WP3, size S-M.
- Exists: conditional entries for the edit tools (`actions/pcbSweepMenu.ts`) and zone and net-inspection groups (`Canvas.tsx`); the
  schematic menu already follows KiCad's conditions (`kicad-port/schContextMenu.ts`).
- Missing: the base block (Rotate, Flip, Move Exactly, Create Array, Copy, Cut, Duplicate, Delete, Align, Distribute) is always listed,
  greyed out when it does not apply, where KiCad lists only what applies; no Properties, Lock, Group, Select Connection or router
  entries; a flat list without submenus.
- Port from: `pcbnew/tools/pcb_selection_tool.cpp`, `edit_tool.cpp`, `pcb_editor_conditions.cpp`.

### 17. Groups
Old #27. **Partial.** Hit: sometimes. Blocks: no. WP3, with WP1 for the schematic, size S-M.
- Exists: Group, Ungroup, whole-group selection, enter and leave (`Cmd::Group`, `state/store.tsx` `withGroupSubstitution`).
- Missing: Add Items, Remove Items and Group Properties (unwired); group-aware move, rotate, flip and delete; nested groups; the
  entered-group overlay; export (item 2); no groups in the schematic.
- Port from: `common/tool/group_tool.cpp`, `pcbnew/tools/pcb_group_tool.cpp`, `eeschema/tools/sch_group_tool.cpp`.

### 18. Appearance and display options
New. **Partial.** Hit: every session. Blocks: no. WP3, size M.
- Exists: layer visibility and opacity, high contrast, flip board; Objects tab with ratsnest, ratsnest mode, grid and pad numbers;
  a net list with highlight (`components/panels/AppearancePanel.tsx`).
- Missing: per-object visibility (tracks, vias, pads, zones, text and so on), net visibility and colours (net colours are recorded
  unwired), a net classes tab, saved presets and viewports.
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
New. **Open.** Hit: often on big boards. Blocks: no. WP3, size S-M.
- Find, Find and Replace and Find Next are registered on the Schematic tab only; `common.Interactive.search` jumps to the Activity
  tab; Find by Properties is recorded unwired (`actions/useActionRunner.ts`, `ui-parity-missing.json`).
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
- Order: move, drag, rotate and mirror for every item kind with wire stretch (1); clipboard (6); fields and labels (12); view controls and snap.

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
- Order: D5 solids pre-pass and `onCollidingSolid`; optimizer over shoved lines (D6, D10, D12); dragger segment slide and 45-degree corners (D7);
  start and end mid-segment, Route From Other End; diff-pair coupling and vias; interactive tuning; arcs; the dead settings (D16).

**WP5. Rules, zones and KiCad file fidelity** (items 2, 3, 11, 13). Size XL; split after step 2 if two agents are free.
- Files: `crates/kicad/src/{pcb,import,sch_import,lib,custom_rules}.rs`, `crates/kicad-engine/src/lib.rs` (project file, `--schematic-parity`),
  `crates/model/src/{lib,ir}.rs` (the `BoardRules` overlay), `crates/cli/src/board.rs::load`, new verbs in `crates/ops/src/`, `crates/zone-filler`, `crates/drc/src/{board,fill}.rs`;
  `web/studio/src/components/{BoardSetupDialog,SchematicSetupDialog,DrcDialog,ErcDialog,ZoneDialog,ZoneManagerDialog}.tsx`.
- KiCad: `pcbnew/pcb_io/kicad_sexpr/` (writer and parser), `eeschema/sch_io/kicad_sexpr/`, `pcbnew/dialogs/dialog_board_setup.cpp` and `panel_setup_*.cpp`,
  `common/dialogs/panel_setup_{netclasses,severities}.cpp`, `pcbnew/drc/drc_rule_parser.cpp`, `pcbnew/tools/drc_rule_editor_tool.cpp`, `pcbnew/dialogs/dialog_drc.cpp`,
  `eeschema/dialogs/dialog_{schematic_setup,erc}.cpp`, `pcbnew/zone_filler.cpp`, `pcbnew/zone.cpp`, `pcbnew/teardrop/`.
- Order: (1) writer: keepouts, per-zone settings, dimensions, groups, locks, arcs, teardrops, with a round-trip test and a fresh `scores.json`;
  (2) the `design.json` overlay for `BoardRules` and the Board Setup pages; (3) severities and the DRC and ERC review workflow; (4) zone filler fidelity.

**WP6. Libraries, parts and footprints on the board** (items 5, 9, 19, 22). Size XL.
- Files: `crates/kicad/src/{symbol_lib,footprint_lib}.rs`, `crates/cli/src/{studio,library_api}.rs`, `crates/model/src/{ir,footprint,symbol}.rs` (`FootprintInstance`,
  instance fields), `crates/ops/src/{lib,library_editors}.rs`; `web/studio/src/components/{SymbolChooserDialog,FootprintPropertiesDialog,PcbParityDialogs}.tsx`,
  `components/{library,footprint,symbol}/`, `actions/{footprintLibraryOps,symbolLibraryOps,libraryEditorActions}.ts`.
- KiCad: `common/libraries/{library_manager,library_table}.cpp`, `common/{lib_tree_model_adapter,footprint_info}.cpp`, `eeschema/libraries/symbol_library_adapter.cpp`,
  `eeschema/{symbol_chooser_frame,symbol_library_manager}.cpp`, `pcbnew/{footprint_chooser_frame,footprint_library_adapter,pcb_field}.cpp`,
  `pcbnew/dialogs/{dialog_footprint_properties,dialog_exchange_footprints,dialog_update_pcb}.cpp`, `pcbnew/tools/{board_editor_control,footprint_editor_control,pad_tool}.cpp`.
- Order: library tables and the chooser (5); board footprint fields, attributes and pad selection (9); Change and Update Footprints and board-only footprints once the overlay exists; editor leftovers (19); arrays (22).

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
| 9 | Board Setup | Partial | `BoardSetupDialog.tsx`, 3 of 7 pages edit; item 3. |
| 10 | Net classes and rules | Partial | importer done (`crates/kicad/src/import.rs::merge_project_net_classes`, `custom_rules.rs`); editing open, item 3. |
| 11 | Property dialogs | Partial | via, shape, zone, text, dimension and track width edit (`Cmd::EditVia`, `EditShape`, `EditZone`, `EditText`); footprint, pads, panel: items 9, 10. |
| 12 | Move excludes tracks and zones | **Open** | `kicad-port/pcbEditActions.ts::movableItem`; no `MoveTrack` or `MoveZone` in `crates/ops`; item 8. |
| 13 | Selection modifiers and box select | **Closed** | `kicad-port/selection.ts`, `components/canvas/selectionCandidates.ts::collectBoxSelection`, `Canvas.tsx`; `PARITY-pcb.md` section 3. |
| 14 | Clipboard | Partial | PCB tracks, vias, zones, shapes, text (`components/canvas/clipboard.ts`, `Cmd::PasteItems`, `Cmd::Duplicate`); schematic: item 6; footprints: item 8. |
| 15 | Cross-tab undo | **Closed** | `crates/ops` `Domain`, `crates/cli/src/board.rs::restore_domain` (test `undo_redo_are_scoped_to_the_tab_that_asked`); the Footprint and Symbol tabs undo in their own scopes. |
| 16 | Hotkey extraction | **Closed** | `web/studio/tools/lib/actionsParser.js::extractPlatformRaw` (with test), `src/kicad/actions.json` (`common.Interactive.redo` is Ctrl+Y), `actions/hotkeys.ts::effectiveHotkey`. |
| 17 | Click-versus-drag threshold | **Open** | `Canvas.tsx` sets `drag.moved` on a non-zero snapped delta; item 14. |
| 18 | Snapping | Partial | `kicad-port/gridSnap.ts`, `components/canvas/gridHelper.ts` (Move and picker); item 14. |
| 19 | DRC schematic parity | Out of scope | kicad-cli has `--schematic-parity`; wiring the dialog is item 11. |
| 20-24 | ERC bus and hierarchy, multi-unit, SI, library-sync, DFM checks | Out of scope | kicad-cli runs these. |
| 25 | Align and distribute | Partial | PCB footprints only (`state/store.tsx`, `kicad-port/alignDistribute.ts`): item 8; schematic Align: item 1. |
| 26 | Array tool | Partial | `Cmd::CreateArray`, `CreateArrayDialog.tsx`; item 22. |
| 27 | Grouping | Partial | `Cmd::Group` family, `state/store.tsx::withGroupSubstitution`; item 17. |
| 28 | Dimensions and measure | **Closed** | `crates/connectivity/src/dimension.rs`, `Cmd::AddDimension` family, `components/DimensionPropertiesDialog.tsx`, the measure tool. Left: the interactive height click, text border, manual text position, export (item 2). |
| 29 | Pan | Partial | `kicad-port/viewControls.ts`, `Canvas.tsx` (PCB); item 20. |
| 30 | Context menu | Partial | `actions/pcbSweepMenu.ts`, `Canvas.tsx`; item 16. |

## Appendix B. Out of scope and deferred

- **kicad-cli's job** (`docs/ARCHITECTURE.md`): DRC, ERC, plots, Gerbers, drill and position files, netlist, BOM, statistics, STEP, GLB, VRML,
  IPC-2581, ODB++, D356 and GenCAD. Old #2, #3 and #19-#24. The UI around them is in scope (item 11). IDF, Hyperlynx, Specctra and the footprint report stay unwired: kicad-cli has none.
- **Python scripting and plugins**: `common.API.pluginsReload`, `pcbnew.ScriptingTool.*`, the footprint wizards (`pcbnew.FpWizard.*`, `ModuleEditor.createFootprint`).
- **Project management**: New, Open, Revert, Save All, Open Non-KiCad Board, Append Board, Rescue.
- **Simulator, last**: `eeschema.Simulation.*` and `eeschema.Simulator.*` need ngspice.
- **Deferred until the IR has the item** (reasons in `ui-parity-missing.json`): design blocks (8 actions), generators (8), tables (5), text boxes, barcodes, points,
  reference images, DXF and SVG import, custom pads, derived symbols; the autoplacer and the microwave tools are declined.

## Appendix C. Zone filler against `pcbnew/zone_filler.cpp`

Audited 2026-10-02 (`e45bf2a`) and re-checked 2026-10-07 for the rows marked *. The ported functions are not listed: `addHoleKnockout`,
`buildDifferentNetZoneClearances`, `subtractHigherPriorityZones`, `postKnockoutMinWidthPrune`, the rule-area knockouts (`FillKeepout`) and the `fillSingleZone` structure.

| KiCad function | `crates/zone-filler` |
|---|---|
| `Fill` | partial: no iterative refill; island removal is our own `apply_island_removal`, not `FillIsolatedIslandsMap` through connectivity |
| `addKnockout` | partial: no custom-pad convex-hull mode |
| `knockoutThermalReliefs`, `buildThermalSpokes` * | simplified: four axis-aligned spokes from the pad's bounding box (`spokes.rs`); no per-pad overrides, spoke angle or circle and oval cases |
| `buildCopperItemClearances` | partial: no courtyard knockouts, no net-tie exemptions, board outline only for edge clearance |
| `connect_nearby_polys` * | missing |
| `fillNonCopperZone` | missing |
| `buildHatchZoneThermalRings`, `addHatchFillTypeOnZone` * | missing; hatch falls back to solid (`lib.rs`) |
| `refillZoneFromCache` | missing |
