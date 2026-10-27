# Round 3 findings: editors, 3D, design blocks, simulator, fab outputs and exports (r3-editors-exports)

**Reviewer:** r3-editors-exports. This was a findings-only review on 2026-10-27, and no code was changed.

**Versions.**
- Our code was read on main's checkout at `5cac9357`.
- KiCad's code is `kicad-src-8303b2ad`. `kicad-src-25ec2335` was used for `kicad/cli/*` and `JobExportBom`/`JobExportPos`.

**What was run.**
- `kicad-cli` 10.99.0, on scratch copies of KiCad QA boards in `/tmp/findings-r3-editors-exports.SbDN/`: `vm/`, `g/`, `bom/` and `drill/`.
- `target/debug/eda` dates from 10-23, older than every round-2 fix. So our side was judged by reading the code, and KiCad's side was confirmed with kicad-cli.

**Priority (a), the round-2 merges reviewed:**
- 7b1553b1, the plot params overlay plus the drill defaults;
- 2baa2200;
- 1b99cd05, the 3D outline check, VRML and the board centre;
- d018db9b, the placement dialog;
- baff7dc0, the board BOM;
- b4db06f7, the schematic Plot defaults;
- 1b77de3e, fea85246 and 056c2567, the Footprint and Symbol Editors;
- d743df5a, the 3D viewer, `${OP}` and the resolver.

**Priority (b):** PARITY-fpedit and PARITY-symedit rows that no round had reviewed (Set Anchor), and PARITY-boardctl section 5 (`.cmp`).

**Finding IDs:**
- X3-n: fab outputs and exports;
- E3-n: the editors;
- V3-n, S3-n, R3-n: the 3D viewer, the simulator and the resolver.

| Part | High | Medium | Low |
|---|---|---|---|
| A. Fab outputs and exports (X3) | 1 | 3 | 8 |
| B. Symbol and Footprint Editors (E3) | 2 | 2 | 2 |
| C. 3D viewer, simulator, resolver (V3, S3, R3) | 0 | 1 | 5 |
| **Total** | **3** | **6** | **15** |

---

## Part A. Fab outputs and exports

### X3-1. A legacy `(viasonmask ..)` is written back in `pcbplotparams`, so KiCad undoes the board's via tenting every time it reads a board we wrote (high)
**Fixed** on task-r3-editors-exports: 899dfdef
- **What differs.**
  - Ours:
    - `crates/kicad/src/import.rs:619` `import_plot_params` keeps every item of the block other than the two layer sets: `Some(_) => p.items.push(..)`. A legacy `(viasonmask yes)` is one of those items.
    - `import.rs:855-863` also turns that item into the board's tenting. That part is right, as round-2 F10 asked.
    - `crates/kicad/src/pcb.rs:150-159` writes `plot.items` back into `(pcbplotparams ..)`. It writes them after `(tenting (front ..) (back ..))` (`pcb.rs:120`), whatever the tenting is now.
  - KiCad:
    - `PCB_PLOT_PARAMS::Format` (`pcbnew/pcb_plot_params.cpp:145-215`) never writes `viasonmask`, so a board saved by KiCad loses it.
    - On every read, `pcb_io_kicad_sexpr_parser.cpp`, `case T_pcbplotparams`, applies it over the tenting it has just read:
      ```cpp
      if( plotParams.GetLegacyPlotViaOnMaskLayer().has_value() )
      {
          bool tent = !( *plotParams.GetLegacyPlotViaOnMaskLayer() );
          m_board->GetDesignSettings().m_TentViasFront = tent;
          m_board->GetDesignSettings().m_TentViasBack = tent;
      }
      ```
  - The result: a board imported from KiCad 7 or 8 with `(viasonmask yes)` comes in untented, which is correct. If the user then tents the vias in Board Setup, every `.kicad_pcb` we write still says `(viasonmask yes)`. kicad-cli then reads the vias untented, and so do its DRC, its Gerbers and KiCad itself. Our own re-import does the same, because it reads the setup in file order.
- **How to see it.**
  - Run: `vm/t.kicad_pcb` has `(tenting (front yes) (back yes))` and, in `pcbplotparams`, `(viasonmask yes)`. After `kicad-cli pcb upgrade --force` the file says `(tenting (front no) (back no))`, and `viasonmask` is gone.
  - QA boards with the token: `issue5320`, `issue3812`, `issue5978`.
  - Test: `crates/kicad/tests`, `a_legacy_viasonmask_is_not_written_back`.
    - Import a board whose `pcbplotparams` has `(viasonmask yes)`.
    - Set `tent_vias_front = tent_vias_back = true`.
    - Write the board. Assert that the text has no `viasonmask` and that re-importing it gives tented vias.
  - Fix direction: drop `viasonmask` from `items` on import, together with the other tokens `Format` no longer writes (`excludeedgelayer`, below; `plotinvisibletext`, `svguseinch`, `pscolor`, the HPGL ones).
- **Severity.** High. The board's via tenting is lost in every file KiCad and kicad-cli read.
- **Already known?** No. Round-2 F10 fixed the reading of the token, not the writing back.

### X3-2. A legacy `(excludeedgelayer no)` is not plotted as KiCad plots it: Edge.Cuts on every layer (medium)
**Fixed** on task-r3-editors-exports: 899dfdef
- **What differs.**
  - KiCad, `PCB_PLOT_PARAMS_PARSER::Parse`:
    ```cpp
    case T_excludeedgelayer:
        if( !parseBool() )
            aPcbPlotParams->m_plotOnAllLayersSequence.push_back( Edge_Cuts );
    ```
    So the Plot dialog's "Plot on All Layers" list has Edge.Cuts, and every Gerber is plotted with the board outline on it.
  - Ours: the item stays a raw string in `items`. `fab_api.rs:52` `gerber_args` reads only `plot.plot_on_all_layers`, so no `--common-layers Edge.Cuts` is passed.
- **How to see it.**
  - Run: `vm/t.kicad_pcb` with `(excludeedgelayer no)` became `(plot_on_all_layers_selection 0x00000000_00000000_00000000_02000000)` after `kicad-cli pcb upgrade`. Bit 25 is Edge.Cuts.
  - QA: `issue12964/issue12964.kicad_pcb` has the token.
  - Test: `import_plot_params` of `(excludeedgelayer no)` gives `plot_on_all_layers == ["Edge.Cuts"]`, and `gerber_args` then contains `--common-layers Edge.Cuts`. Remove the token from `items` at the same time (X3-1).
- **Severity.** Medium. The Gerbers of a legacy board lack the outline KiCad's dialog plots on them.
- **Already known?** No.

### X3-3. The Plot and Drill dialogs never start from the board's plot settings in the studio: they are mounted before the board loads (medium)
**Fixed** on task-r3-editors-exports: 0e6b4f69 (the dialogs store their choices back as `set_plot_settings`, one undo step: 0e6b4f69; the check: 32c5338d)
- **What differs.**
  - Ours: `App.tsx:316,319` renders `<PlotDialog />` and `<GenerateDrillDialog />` from the first frame, when `state.board` is still `null` (`store.tsx:911`).
    - `PlotDialog.tsx:35` takes the checklist from a `useState` initializer: `state.board?.plot_params?.layers ?? defaultLayers(..)`. `PlotDialog.tsx:67` does the same for the origin, `useAuxOrigin`: `state.board?.plot_params?.use_aux_origin ?? false`.
    - `GenerateDrillDialog.tsx:20` sets its Origin the same way.
    - An initializer runs only once, so it always sees `board === null`. The two `useEffect`s after it only add or drop the copper and optional layers the board gains or loses. The board's own selection, for example a block without paste or with F.Fab, and its `useauxorigin` never reach either dialog.
    - `postFabGerbers` always sends `use_aux_origin` (`client.ts:346`). So the server's fallback (`fab_api.rs` `gerbers`: `if req.get("use_aux_origin").is_none() && plot.flag("useauxorigin")`) never applies to the dialog.
  - KiCad builds the dialog each time it opens:
    - `DIALOG_PLOT::init_Dialog` checks each layer from `m_plotOpts.GetLayerSelection()[layer]` and runs `m_useAuxOriginCheckBox->SetValue( m_plotOpts.GetUseAuxOrigin() )`;
    - `DIALOG_GENDRILL::TransferDataToWindow` runs `m_origin->SetSelection( m_plotOpts.GetUseAuxOrigin() ? 1 : 0 )`.
- **How to see it.** Import `issue22826.kicad_pcb`, which has `(useauxorigin yes)`. Open File > Fabrication Outputs > Gerbers and press Plot.
  - KiCad plots from the drill/place origin.
  - Ours sends `use_aux_origin: false` and plots at the absolute origin. The Drill dialog does the same, so our Gerbers and drill files agree with each other but not with KiCad's.
  - A board whose selection leaves out F.Paste gets a paste Gerber anyway.
  - Test: e2e in `plot-exports.check.js`, `the plot checklist and origin start from an imported board's pcbplotparams`. Open the dialog on an imported board with `(useauxorigin yes)` and a selection without F.Paste. Expect the box checked and F.Paste unchecked.
  - Fix direction: reset the state from `state.board.plot_params` each time `plotDialogOpen` becomes true. That is also what KiCad's per-open construction does.
- **Severity.** Medium. The 7b1553b1 fix is cut off by a wrong default in the studio. The CLI path (`fab_cmd.rs`) is not affected for the layers, but see X3-11.
- **Already known?** No. PARITY-boardctl section 5 says "the checklist and 'Use drill/place file origin' start from it".

### X3-4. Export STEP/GLB/...: no "Scaled models detected." warning (medium)
**Fixed** on task-r3-editors-exports: 0e6b4f69
- **What differs.**
  - KiCad's `DIALOG_EXPORT_STEP::DIALOG_EXPORT_STEP` lists every model whose scale is not 1 (up to 5) and shows this warning, which can be turned off:
    ```cpp
    wxString extendedMsg = _( "Non-unity scaled models:" ) + wxT( "\n" ) + bad_scales;
    KIDIALOG msgDlg( m_editFrame, _( "Scaled models detected.  Model scaling is not reliable for "
                                     "mechanical export." ),
                     _( "Model Scale Warning" ), wxOK | wxICON_WARNING );
    msgDlg.SetExtendedMessage( extendedMsg );
    msgDlg.DoNotShowCheckbox( __FILE__, __LINE__ );
    ```
  - Ours: `BoardExportDialog.tsx` and `board_output_api.rs:260` `three_d` have no such check. A grep for "Scaled models" finds nothing.
- **How to see it.** Give R1's 3D model a scale of 2,2,2 in Footprint Properties, then open Export > STEP. KiCad warns, with "Non-unity scaled models:" and the model's file name. Ours exports silently.
  - Test: a `BoardExportDialog` unit or e2e test. A board with one model scaled (2,1,1) shows "Scaled models detected.  Model scaling is not reliable for mechanical export." (two spaces after the full stop), with the file name in the details.
- **Severity.** Medium (a missing message).
- **Already known?** No.

### X3-5. `(gerberprecision 4)` (or lower) plots at 4.6 here; KiCad clamps it to 4.5 (low)
**Fixed** on task-r3-editors-exports: 899dfdef
- Ours: `fab_api.rs:71`: `if precision == 5 { "5" } else { "6" }`.
- KiCad: `case T_gerberprecision: m_gerberPrecision = parseInt( gbrDefaultPrecision - 1, gbrDefaultPrecision );`, where `parseInt` clamps to [5, 6], so 4 becomes 5.
- Run: `vm/t.kicad_pcb` with `(gerberprecision 4)` was upgraded to `(gerberprecision 5)`.
- Test: `gerber_args` with `(gerberprecision 4)` contains `--precision 5`.

### X3-6. The DNP fab options are passed as stored, not as the dialog reads them back (low)
**Fixed** on task-r3-editors-exports: 899dfdef
- KiCad's dialog normalizes the three flags.
  - `DIALOG_PLOT::init_Dialog`: `m_plotDNP` = hide || sketch || crossout. Then the "Hide" radio is chosen if hide is set, else "Cross-out".
  - `applyPlotSettings`:
    ```cpp
    tempOptions.SetHideDNPFPsOnFabLayers( m_plotDNP->GetValue() && m_hideDNP->GetValue() );
    tempOptions.SetSketchDNPFPsOnFabLayers( m_plotDNP->GetValue() && m_crossoutDNP->GetValue() );
    tempOptions.SetCrossoutDNPFPsOnFabLayers( m_plotDNP->GetValue() && m_crossoutDNP->GetValue() );
    ```
  - So `(hidednponfab yes) (sketchdnponfab yes) (crossoutdnponfab yes)` plots as hide only.
  - And `(sketchdnponfab yes) (crossoutdnponfab no)` plots as sketch and cross-out.
- Ours: `fab_api.rs:82-90` passes each flag as stored.
- Test: `gerber_args` of hide+sketch+crossout gives only `--hide-DNP-footprints-on-fab-layers`. Sketch alone gives both `--sketch-DNP..` and `--crossout-DNP..`. This only matters for F.Fab/B.Fab.

### X3-7. Generate Drill Files shows "Drill file written." instead of the writer's report (low)
**Fixed** on task-r3-editors-exports: 899dfdef
- Ours: `GenerateDrillDialog.tsx:32`.
- KiCad's `EXCELLON_WRITER::CreateDrillandMapFilesSet` reports `"Created file '%s'"` for each file (no full stop), then `ReportTail( _( "Done." ) )`.
- kicad-cli prints the same lines. Run: `drill/`: `Created file 'o/issue5093-PTH.drl'`, `Created file 'o/issue5093-NPTH.drl'`, `Done.`. They are not shown.

### X3-8. Gerber X3 placement report has no "Component count" lines (low)
**Fixed** on task-r3-editors-exports: 899dfdef
- Ours: `fab_api.rs:261-266` writes only the two file lines and "Done.".
- KiCad's `DIALOG_GEN_FOOTPRINT_POSITION::CreateGerberFiles` writes:
  - "Component count: %d." after each file;
  - then "Full component count: %d.";
  - then "Done.".
- The count is `CreatePlaceFile`'s return value. One way to get it here is to count the `%TO.C,` objects of each file.

### X3-9. Sketched pads on the fab layers use 0.1 mm, not the board's F.Fab line width (low; kicad-cli has no flag)
**Fixed** on task-r3-editors-exports: 32c5338d (recorded in GAPS.md item 37 as a kicad-cli limit; nothing to change in the code)
- KiCad's `DIALOG_PLOT::Plot` sets `m_plotOpts.SetSketchPadLineWidth( board->GetDesignSettings().GetLineThickness( F_Fab ) )`.
- kicad-cli never sets it, so the width stays `PCB_PLOT_PARAMS`' `m_sketchPadLineWidth = pcbIUScale.mmToIU( 0.1 )`.
- It differs only on a board whose fab line width is not 0.1 mm, with `(sketchpadsonfab yes)`. Record it in GAPS 37 as a kicad-cli limit.

### X3-10. The `.cmp` file's `Path` is `/<symbol uuid>`, not the footprint's `(path ..)` (low)
**Fixed** on task-r3-editors-exports: 9aedac85
- Ours: `board_control_api.rs:110` builds `format!("/{symbol}")` from the derived schematic's id map. A symbol on a sub-sheet therefore loses its sheet uuids, and the path differs from the `(path ..)` our own board writer gives the footprint (`pcb.rs:1181`, `link.path`).
- KiCad's `RecreateCmpFile` writes `fp->GetPath().AsString()`.
- KiCad's readers use only `Reference` and `IdModule`:
  - `CMP_READER::Load`;
  - `SCH_EDIT_FRAME::ProcessCmpToFootprintLinkFile` (`assign_footprints.cpp`).
  So the effect is cosmetic.
- PARITY-boardctl says "identical in content".

### X3-11. `eda fab gerbers` ignores the board's `useauxorigin` (low)
**Fixed** on task-r3-editors-exports: 899dfdef
- `fab_cmd.rs:32-42` never calls `with_aux_origin`, while `/api/fab/gerbers` applies the board's flag when the request does not say otherwise. The `fab_api` module doc promises that "a dialog and the CLI produce identical output for identical input".

### X3-12. Stale text in the Plot dialog (low, doc)
**Fixed** on task-r3-editors-exports: 0e6b4f69
- `PlotDialog.tsx:1-5`: "this keeps the two that actually change the output: X2 format and whether a board-edge check should block the plot". Neither of those is a control.
- `PlotDialog.tsx:120-121`: "(Gerber X2, with its .gbrjob job file)". That is false for a board with `(usegerberattributes no)`, which is now plotted with `--no-x2`.

### Checked and found to match (exports)
- **`eda_model::plot_params`:**
  - `parse_hex_bits` is `BASE_SET::ParseHex`: from the right, `_` skipped, stops at a non-hex character.
  - `encode_layer_selection` is `FmtHex`, with `_` every 8 nibbles.
  - The legacy map is `s_legacyLayerIdMap` (`LEGACY_PCB_LAYER_ID_COUNT` 64, User.1..9 at 50..58, Rescue dropped).
  - `< 20240819` is the gate.
  - A number is `LSET( { F_SilkS, B_SilkS } ) | AllCuMask()`.
  - The default selection is `0x00000000_00000000_55555555_5755f5ff`.
  - `parseBool` takes yes/no/true/false.
  - The written version is never below 20241229 (`pcb.rs:1666-1687`), so the new-numbered hex is never read as legacy.
- **`gerber_args` against `--board-plot-params`:**
  - Run on QA `issue22826`, which has Protel names, subtract mask, `useauxorigin yes`, `drillshape 0`, and a variant with `drillshape 1`.
  - Our flags and `--board-plot-params` gave byte-identical Gerbers for all 13 layers, apart from the date lines (`g/`).
  - kicad-cli restricts the plot to enabled layers (`layersToPlot = LSET( .. ) & brd->GetEnabledLayers()`), so a selected layer the board lacks plots nothing.
  - The layer names we pass are the file names, which `convertLayerArg` takes.
- **Drill:**
  - `--excellon-oval-format route --excellon-separate-th` gave `issue5093-PTH.drl` with 8 `M15` and no `G85`, plus `-NPTH.drl` (`drill/`).
  - The dialog's other defaults match kicad-cli's: Millimeters, Decimal, no mirror, no minimal header, map off.
- **The 3D outline refusal (X4):**
  - `board_outline_closes` follows `BuildBoardPolygonOutlines( .., maxError 0.05 mm, chainingEpsilon = tolerance, false )`, both passes (footprint holes taken out first).
  - A plain polygon outline counts as Edge.Cuts (`edge_cuts_shapes`).
  - The message is word for word, with `%.3f`.
  - The tolerance default is Tight, 0.001 mm (`dialog_export_step_base.cpp:283`).
- **VRML (X5, X6):**
  - The defaults are "User defined origin" checked at (0, 0) and units "meter" (`SetSelection( 1 )`).
  - Unchecked, kicad-cli computes `ComputeBoundingBox( true, true )` itself, as `ExportVRML` does.
  - The STEP "Board center origin" uses the same box (`board_edges_bounding_box`).
- **Board BOM (X7):**
  - kicad-cli with `pcb_bom_args()` honours `FP_EXCLUDE_FROM_BOM`. Run: `bom/issue14412`, whose only footprint has `exclude_from_bom`, gives a header-only file.
  - The grouping by value and footprint is `GenBOMFileFromBoard`'s.
  - The empty-board text is `ShowInfoBarError`'s, word for word.
- **Placement (X8):**
  - `CreateAsciiFiles`' candidate test and "No footprint for automated placement.".
  - The report lines and their order.
  - The defaults of `dialog_gen_footprint_position_file_base.cpp`.
  - Gerber X3 is always both files, named by `GetPlaceFileName` (`-pnp_top.gbr`/`-pnp_bottom.gbr`), with the DNP filter passed and no origin.
- **Schematic Plot (X9):** PDF, colour, drawing sheet on, background off. `DIALOG_PLOT_SCHEMATIC` takes no other app setting for these.

---

## Part B. Symbol and Footprint Editors

These were read on main against 8303b2ad (25ec2335 where noted). Nothing was run.

### E3-1. Footprint Editor Set Anchor leaves the Reference, Value, user fields and 3D model offsets where they were (high)
**Fixed** on task-r3-editors-exports: a10c9d08 (the kept zones/dimensions and the `courtyard` override do not move: noted in PARITY-fpedit)
- **What differs.**
  - Ours: `crates/ops/src/lib.rs:5551-5564` `set_footprint_anchor` moves only `fp.pads`, `fp.graphics` and `fp.texts`. It does not move:
    - `fp.reference` / `fp.value` (`Option<FieldLayout>`; `None` is the default layout at (0,-0.5) / (0,1));
    - `fp.fields[*]` layouts;
    - `fp.models3d[*]` offsets;
    - the courtyard override.
  - KiCad: `pcbnew/footprint.cpp::FOOTPRINT::MoveAnchorPosition`, called from `drawing_tool.cpp::SetAnchor` and pushed as `_( "Move Footprint Anchor" )`:
    ```cpp
    // Update field local coordinates
    for( PCB_FIELD* field : m_fields )
        field->Move( moveVector );
    for( PAD* pad : m_pads ) pad->Move( moveVector );
    for( BOARD_ITEM* item : GraphicalItems() ) item->Move( moveVector );
    for( ZONE* zone : Zones() ) zone->Move( moveVector );
    for( FP_3DMODEL& model : Models() )
    {
        model.m_Offset.x += pcbIUScale.IUTomm( moveVector.x );
        model.m_Offset.y -= pcbIUScale.IUTomm( moveVector.y );
    }
    ```
  - `PARITY-fpedit.md:101` says "**done** ... everything shifts so the clicked point becomes the origin".
- **How to see it.** On a new footprint (REF** at (0,-0.5)), add a pad at (2,0) and a 3D model with offset 0. Use Set Anchor (Ctrl+Shift+N) and click (2,0).
  - KiCad: the pad is at (0,0), REF** at (-2,-0.5), the Value at (-2,1), and the model offset x is -2 mm.
  - Ours: the pad moves, but the fields and the model stay. The exported `.kicad_mod` has its Reference 2 mm off and its model misaligned on every board.
  - Test: `crates/ops/src/tests.rs::set_footprint_anchor_moves_fields_and_models_as_move_anchor_position`.
    - Input: `reference: None`, a user field at (1,1), and model offset (0,0,0).
    - Action: `SetFootprintAnchor { at: (2mm, 0) }`.
    - Assert: the Reference is written `(at -2 -0.5 0)`, the field is at (-1,1), and the model offset is (-2,0,0) mm (y negated as in KiCad).
- **Severity.** High. After one click the saved footprint has its fields and model in the wrong place.
- **Already known?** No. The existing test (`tests.rs:1914`) covers only pads, graphics and texts.

### E3-2. Symbol Editor Move Symbol Anchor leaves the fields where they were (high)
**Fixed** on task-r3-editors-exports: a10c9d08; the side note (no undo step in KiCad) recorded as a deliberate difference in PARITY-symedit: 32c5338d
- **What differs.**
  - Ours: `crates/ops/src/library_editors.rs:382-394` `set_symbol_anchor` moves `sym.graphics` and `sym.pins` only. A mandatory field with no stored layout stays at `default_field`'s place.
  - KiCad: `symbol_editor_drawing_tools.cpp::PlaceAnchor` calls `symbol->Move( -cursorPos );`.
    - `LIB_SYMBOL::Move` is `for( SCH_ITEM& item : m_drawings ) item.Move( aOffset );`.
    - The fields are in `m_drawings[SCH_FIELD_T]` (`LIB_SYMBOL::LIB_SYMBOL`, `addField`), so they move with the body.
  - `PARITY-symedit.md:138` says "**done**".
- **How to see it.** Take a symbol with the Reference at (0,5.08). Move Symbol Anchor to (2.54,0).
  - KiCad: the Reference is at (-2.54,5.08).
  - Ours: it stays at (0,5.08), offset from the body in the `.kicad_sym` and on every placed instance.
  - Test: `library_editors.rs::set_symbol_anchor_moves_the_fields_too`. Assert the Reference is at (-2.54,5.08) and the Value at (-2.54,2.54).
- **Severity.** High.
- **Already known?** No.
- **Side note (low).** `PlaceAnchor` pushes no commit: it only calls `OnModify()`, so it has no undo step in either 8303b2ad or 25ec2335. Ours has one. That is better, but PARITY-symedit should record it as a deliberate difference.

### E3-3. Rotate of several pins: the pivot is the box of the pins' two ends, but KiCad uses each pin's full bounding box (medium)
**Fixed** on task-r3-editors-exports: 3cd18c9f
- **What differs.**
  - Ours:
    - `web/studio/src/kicad-port/symEditItems.ts:66` `rotatePlan` boxes `at` and `pinEnd(p)` (`:47`).
    - `symbolEditorStore.tsx:662` `rotateSelection` passes no `otherBoxes`, so selected graphics are left out of the centre.
    - `nearestHalfGrid` uses `Math.round` in the y-up frame (`:56`). KiCad's `KiROUND` rounds ties away from zero, in y-down.
  - KiCad: `SYMBOL_EDITOR_EDIT_TOOL::Rotate` uses `rotPoint = m_frame->GetNearestHalfGridPosition( selection.GetCenter() )`.
    - `SELECTION::GetCenter` merges each item's `GetBoundingBox()`.
    - For a pin that is `GetBoundingBox( false, true, m_flags & SHOW_ELEC_TYPE )`.
    - `PIN_LAYOUT_CACHE::GetPinBoundingBox` adds the pen, the decoration, the name box and the number box.
  - `PARITY-symedit.md:263` says "identical for pins ... (the box of the selected pins, root to tip)".
- **How to see it.** Take two right-pointing pins at (0,0) and (0,-2.54), length 2.54, named "IN"/"OUT", names inside, grid 1.27.
  - Ours: the pivot x is 1.27.
  - KiCad: the name boxes widen the box to the right, so the pivot snaps to 2.54. After R, the pins end up 1.27 mm from where KiCad puts them.
  - Test: `symEditItems.test.ts` `rotate's pivot counts the pins' name and number boxes`.
- **Severity.** Medium.
- **Already known?** No. This is in the fix for round-2 E5.

### E3-4. Repeat Pin (Insert) finds the last pin by its content; KiCad finds it by UUID, so after an edit ours does nothing (medium)
**Fixed** on task-r3-editors-exports: 0e6b4f69
- **What differs.**
  - Ours: `symbolEditorStore.tsx:681` searches for a pin with `last.pin`'s number, unit, body style and position as placed. Once that pin has been moved, renumbered or renamed, nothing matches.
  - KiCad: `symbol_editor_drawing_tools.cpp::RepeatDrawItem` matches `test->m_Uuid == g_lastPin`.
    - `g_lastPin` is set in `CreatePin` and after each Repeat.
    - `RepeatPin` then duplicates the pin as it is now.
- **How to see it.** Place pin 1 at (0,0), change its number to 5, and press Insert. KiCad adds pin 6 at (0,-2.54); ours adds nothing.
  - Test: e2e in `lib-edit.check.js`, `Insert repeats the last pin after it was edited`.
- **Severity.** Medium (an edge case).
- **Already known?** No.

### E3-5. Repeat Pin does not update the sticky pin template (low)
**Fixed** on task-r3-editors-exports: 0e6b4f69
- Ours: `repeatLastPin` (`symbolEditorStore.tsx:673`) dispatches no `SET_PIN_TEMPLATE`.
- KiCad: `SYMBOL_EDITOR_PIN_TOOL::PlacePin` updates the template for every `aPin->IsNew() && !aPin->HasFlag( IS_PASTED )`, and `RepeatPin`'s pin is `IS_NEW`:
  `g_LastPinOrient = aPin->GetOrientation(); g_LastPinType = aPin->GetType(); g_LastPinShape = aPin->GetShape();`
- Test: after `repeatLastPin`, `pinTemplate.electrical_type` is the repeated pin's type.

### Checked and found to match (editors)
- **E1, `CreateNewFootprint`:**
  - REF** on F.SilkS at (0,-0.5), 1 mm text with a 0.1 mm pen;
  - the Value is the name, on F.Fab at (0,1), 0.15 mm pen;
  - `${REFERENCE}` on F.Fab at (0,2.5);
  - the attributes come from `fpnames.back()`, falling back to `FP_SMD`, and our byte order matches `FootprintEnumerate`'s;
  - `write_library_fields` always writes Reference and Value.
- **E2.** `CreatePin` sets no number. The empty-number check was dropped (056c2567), and `assign_missing_ids` handles collisions.
- **E3:**
  - `RepeatPin`'s step direction, `pin_step` 100 mil and `label_delta` 1;
  - `IncrementString` line for line;
  - `Duplicate`'s `StrNumCmp` order and `GetMaxPinNumber()+1` counting the copies already added;
  - `Paste`'s unit and body style, refused on a derived symbol;
  - Cut is Copy plus `DoDelete`.
- **E4.** `DoDelete`'s sync twins (position, body style, type, name; one per unit) in one commit, and the Footprint Editor's delete as one batch.
- **E5.** The turn direction and orientation map. A single item turns about its own position, and twins are not touched.
- **E6.** 1 IU from `CreateSymbol`, and `offset == 0 ? 20 : offset` IU from Symbol Properties.
- **E7.** `% 50` on IU, only when the position changed. The message is word for word, and `IsOK` has "Confirmation" and Yes/No.
- **E8.** The enable and checked rules, the labels, the three tooltips, the info bar texts and "Exit sync pins mode".
- **E9.** The template is saved at OK, before `PlacePin`.
- **E12.** Reference and Value at (0,0), 1.27 mm text.

---

## Part C. 3D viewer, simulator, model resolver

This part reviews d743df5a, read against 8303b2ad. Nothing was run. Design blocks have not changed since round 2's checked list (8a567378, c40ebb4c), so they were not reviewed again.

### S3-1. A label's `${OP}` shows "?" when the label sits on a pin with no wire; sheet pins are not resolved (medium)
**Fixed** on task-r3-editors-exports: 459f1ffe
- **What differs.**
  - Ours: `web/studio/src/kicad-port/sim/opVars.ts` `withOpVars` finds a label's net only from a wire vertex: `sch.wires.find((w) => !w.bus && w.pts.some(at))?.net ?? null`. With no wire there, it shows "?". Only `sch.labels` are visited.
  - KiCad: `sch_label.cpp::SCH_LABEL_BASE::ResolveTextVar`:
    ```cpp
    const SCH_CONNECTION* connection = Connection();
    *token = wxS( "?" );
    if( connection )
        *token = schematic->GetOperatingPoint( connection->Name( false ), precision, range );
    ```
    - A label on a pin end has that pin's connection (`connection_graph.cpp::updateGenericItemConnectivity`).
    - `SCH_SHEET_PIN` derives from `SCH_HIERLABEL` (`sch_sheet_pin.h:65`), so a sheet pin's `${OP}` resolves too.
- **How to see it.** On `e2e/simulator-board.yaml`'s RC sheet, put a local label `OUT ${OP}` straight on R1 pin 2 and run an OP. KiCad shows "OUT 989mV"; ours shows "OUT ?".
  - Test: `opVars.test.ts` `a label on a pin takes the pin's net`.
    - Input: no wires; a symbol pin "2" at (0,0) on net "/OUT"; a label at (0,0) with `${OP}`; points `{"/out": 0.98946}`.
    - Assert: the label's text is "989mV".
- **Severity.** Medium.
- **Already known?** No.

### S3-2. An escaped `\${OP}` is resolved anyway (low)
**Fixed** on task-r3-editors-exports: 459f1ffe
- Ours: `crates/model/src/text_vars.rs::shown_text` undoes the escape (`text.replace(ESC_DOLLAR, "${")`) before the studio runs `withOpVars` on `shown`. The literal `${OP}` therefore becomes the value.
- KiCad resolves `${OP}` inside `ResolveTextVars`, before the `<<<ESC_DOLLAR:` markers are undone (`common.cpp`).
- `opVars.test.ts:15` tests only the raw `\${OP}`, which is not what reaches the resolver.
- Test: a field `{ text: "\\${OP}", shown: "${OP}" }` keeps `${OP}`.

### S3-3. `${OP:<pin>}` shows "?" before any run (or for a symbol not in the circuit); KiCad shows "--" (low)
**Fixed** on task-r3-editors-exports: 459f1ffe
- Ours: `opVars.ts` `symbolOpToken` walks the pins only `if (item)`.
- KiCad: `SCH_SYMBOL::ResolveTextVar` always builds the model (`simLibMgr.CreateModel`) and calls `GetOperatingPoint`, which returns `wxS( "--" )` while `m_operatingPoints.empty()`.
- Test: `symbolOpToken("OP:1", { points: new Map(), ref: "R1", item: undefined, pins: [{ number: "1", name: "~" }] }) === "--"`.

### S3-4. The OP current overlay counts only visible pins in its two-pin test (low)
**Fixed** on task-r3-editors-exports: 459f1ffe
- Ours: `opPainter.ts` gets its pins from `schPins.ts::pinsOfSymbol`, which skips hidden pins. `opOverlay.ts` then tests `ps.length === 2`.
- KiCad: `sch_edit_frame.cpp::RefreshOperatingPointDisplay` uses `symbol->GetPins( &sheetPath )`, hidden pins included, then `if( pins.size() == 2 )`.
- Test: a unit with 2 visible pins and 1 hidden pin takes the model-pin path.

### V3-1. 3D model-row hotkeys also fire with Shift held (low)
**Fixed** on task-r3-editors-exports: 459f1ffe
- Ours: `Viewer3D.tsx` `onKeyDown` matches `e.key.toLowerCase()` for t/s/v/p/d and never checks Shift.
- KiCad binds the plain letters only: `eda_3d_actions.cpp` `showTHT` 'T', `showSMD` 'S', `showVirtual` 'V', `showNotInPosFile` 'P', `showDNP` 'D'.
- Test: Shift+D leaves `dnp_models` unchanged.

### R3-1. Model-path expansion keeps a trailing `$` or `${`; KiCad drops it (low)
**Fixed** on task-r3-editors-exports: 459f1ffe
- KiCad: `common.cpp::KIwxExpandEnvVars`, case `'$'`: `size_t m = n + 1; if( m >= strlen ) break;`. Nothing is appended, so `"a$"` becomes `"a"` and `"a${"` becomes `"a"`.
- Ours: `crates/cli/src/model3d_api.rs` `expand` does `out.extend(&s[n..]); break;`.
- The new test `variables_are_scanned_as_kiwxexpandenvvars_scans_them` asserts `expand("a$") == "a$"`, which is the wrong result.

### Note on round-2 S2 (known, under-stated)
`crates/kicad/src/pcb.rs:904` `export_kicad_pro_for` rebuilds `schematic.drawing` keeping only the `intersheets_ref_*` keys. So an imported project's `drawing.operating_point_overlay_*` settings are lost from the derived `.kicad_pro` on every write. GAPS 34 says only "not carried".

### Checked and found to match (3D, simulator)
- **V1.** `render.show_user{n}` defaults to false, and comments, drawings, eco1 and eco2 default to true.
- **V2:**
  - `footprintShown` in `IsFootprintShown`'s order;
  - `show_footprints_not_in_posfile` true and `show_footprints_dnp` false;
  - the row labels and tooltips word for word;
  - `kind3d` from `effective_attrs`.
- **V3, fixed part.** A copper swatch sets both rows (`copperIndices`).
- **S1:**
  - the symbol regex and groups;
  - the precision digit;
  - "~A" by default, and "~W" for `:power` with no range;
  - a pin matched by name or number, lower case;
  - the 2-pin item current, else `<ref>:<pin>`;
  - the label regex and "~V".
- **`SCHEMATIC::GetOperatingPoint`.** "" for gnd/0, "--" with no points, "?" otherwise.
- **S3.** The two-pin branch uses the raw reference, the overlay goes per unit, and nets are asked by name.
- **`SPICE_VALUE::ToString( SPICE_VALUE_FORMAT )`:**
  - the "°" branch;
  - the "~" Normalize;
  - the fixed-prefix rescale;
  - KiROUND, `%g`;
  - the SI and SPICE prefixes, `StripZeros`.
- **R1:**
  - the name scan (alnum, `_`, `:`);
  - `Contains("KISYS3DMOD")`;
  - unexpanded variables written back;
  - project text variables through `model_names`.
- **3D hotkeys (`actions3d.ts` against `eda_3d_actions.cpp`).** Space, R/Shift+R, the arrows, Home, F, and Y/X/Z with their Shift forms.

---

## Summary

| # | Finding | Severity | Known? |
|---|---|---|---|
| X3-1 | Legacy `(viasonmask ..)` written back in `pcbplotparams`: KiCad re-untents the vias on every read | high | no (F10 fixed only the read) |
| E3-1 | Footprint Editor Set Anchor leaves fields, 3D model offsets and courtyard override behind | high | no (PARITY-fpedit says done) |
| E3-2 | Symbol Editor Move Symbol Anchor leaves the fields behind | high | no (PARITY-symedit says done) |
| X3-2 | Legacy `(excludeedgelayer no)` not plotted as Edge.Cuts on all layers | medium | no |
| X3-3 | Plot/Drill dialogs mounted before the board loads: checklist and origin never from the board's plot settings | medium | no (PARITY-boardctl says they are) |
| X3-4 | No "Scaled models detected." warning in Export STEP | medium | no |
| E3-3 | Multi-pin Rotate pivot ignores name/number boxes and selected graphics | medium | no |
| E3-4 | Repeat Pin finds the last pin by content, not UUID | medium | no |
| S3-1 | Label `${OP}` on a pin with no wire is "?"; sheet pins not resolved | medium | no |
| X3-5 | `(gerberprecision 4)` plotted at 6, KiCad clamps to 5 | low | no |
| X3-6 | DNP fab options not normalized as the dialog does | low | no |
| X3-7 | Drill dialog: "Drill file written." instead of "Created file '%s'" / "Done." | low | no |
| X3-8 | Gerber X3 placement report lacks the component counts | low | no |
| X3-9 | Sketched pad width 0.1 mm, not the F.Fab line width (no kicad-cli flag) | low | no |
| X3-10 | `.cmp` `Path` is `/<symbol uuid>`, not the footprint's path | low | no |
| X3-11 | `eda fab gerbers` ignores the board's `useauxorigin` | low | no |
| X3-12 | Stale Plot dialog header and "Gerber X2" note | low | doc |
| E3-5 | Repeat Pin does not update the pin template | low | no |
| E3-2 note | KiCad's PlaceAnchor has no undo step; record ours as deliberate | low | doc |
| S3-2 | Escaped `\${OP}` resolved anyway | low | no |
| S3-3 | `${OP:<pin>}` with no run is "?", KiCad "--" | low | no |
| S3-4 | OP two-pin test ignores hidden pins | low | no |
| V3-1 | Shift+T/S/V/P/D also toggle the 3D model rows | low | no |
| R3-1 | Trailing `$`/`${` kept in model paths; the new test asserts it | low | no |

Counts: high 3, medium 6, low 15 (the E3-2 note included; 14 without it).
