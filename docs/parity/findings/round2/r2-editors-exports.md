# Round 2 findings: editors, 3D, design blocks, simulator, fab outputs and exports (r2-editors-exports)

**Reviewer:** r2-editors-exports, a findings-only review on 2026-10-27. It changes no code.

**Versions.** Our code was read on main's checkout at `5cca5fff`. KiCad's is `kicad-src-8303b2ad`, with `kicad-src-25ec2335` for `kicad/cli/*` and newer items.

**Practical checks.**
- Runs: `kicad-cli` 10.99.0 on scratch copies in `/tmp/findings-r2-editors-exports.GtQ7/`, and one `eda board serve` on port 8848 (stopped since).
- `target/debug/eda` is from 10-23, older than 4a3fc95c, c943dd26 and 4611df84. New code was judged by reading.

**Finding IDs, by part.**
- X: fab outputs and exports (Part A).
- E: Symbol and Footprint Editors (Part B).
- V, S, R: 3D viewer, simulator and model resolver (Part C).
- D: Specctra (Part D).

| Part | High | Medium | Low |
|---|---|---|---|
| A. Fab outputs and exports (X) | 3 | 7 | 1 |
| B. Symbol and Footprint Editors (E) | 1 | 6 | 5 |
| C. 3D viewer, design blocks, simulator (V, S, R) | 0 | 3 | 4 |
| D. Specctra (D) | 0 | 1 | 4 |
| **Total** | **4** | **17** | **14** |

The four highs:
- **X1:** drill slots are G85 where KiCad's dialog routes them.
- **X3:** Gerbers ignore the board's plot settings (Protel names), and an import drops `pcbplotparams`.
- **X4:** a STEP of an open outline is written silently, where KiCad refuses.
- **E1:** a new library footprint is exported with no Reference or Value.

---

## Part A. Fab outputs and exports through kicad-cli

Our side is read at main 5cca5fff. KiCad's side is 8303b2ad, plus 25ec2335 for `kicad/cli/*`, which the cleaned 8303b2ad tree does not have. The kicad-cli on the box is 10.99.0.

Each "How to see it" that says "run" was run with `/usr/local/bin/kicad-cli` on scratch copies in `/tmp/findings-r2-editors-exports.GtQ7/`. Main's `target/debug/eda` (built 10-23) is older than 4611df84, so it was not used to judge the new code.

### X1. Generate Drill Files writes oval holes as G85 slots; KiCad's dialog writes them as routed G00/M15 moves (high)
- **Fixed** on task-r2-editors-exports: 7b1553b1 (`--excellon-oval-format route` unless "Use alternate drill mode for oval holes"; `fab_api::tests::drill_defaults_are_the_dialogs`, slow `fab_dialogs::the_drill_files_are_two_with_routed_slots` on QA issue5093).
- **What differs.**
  - Ours: `crates/cli/src/fab_api.rs:45` `drill_args` passes only `--format excellon` (with `--excellon-separate-th` and `--drill-origin plot` when asked). kicad-cli's default `--excellon-oval-format` is `alternate` (`kicad/cli/command_pcb_export_drill.cpp`: `.default_value( std::string( "alternate" ) )`; `drillJob->m_excellonOvalDrillRoute = false`).
  - KiCad's dialog: "Use alternate drill mode for oval holes" (`m_altDrillMode`, `dialog_gendrill_base.cpp:76`) starts unchecked. `dialog_gendrill.cpp::DIALOG_GENDRILL::genDrillAndMapFiles` calls `excellonWriter.SetRouteModeForOvalHoles( !m_altDrillMode->GetValue() );`, so a slot is routed.
- **How to see it.** Run on QA `issue5093.kicad_pcb`:
  - `kicad-cli pcb export drill --format excellon` gives `X100.76Y-144.26G85X100.76Y-143.16`.
  - With `--excellon-oval-format route` it gives `G00X100.76Y-144.26` / `M15` / ..., which is what KiCad's dialog writes by default.
  - Test: `fab_api::tests::drill_defaults_are_the_dialogs`, asserting that `drill_args(..)` contains `--excellon-oval-format route` unless the request has `alt_drill_mode: true`.
- **Severity.** High. Every slot in the drill file the fab receives is in another Excellon dialect from KiCad's default.
- **Already known?** No.

### X2. Generate Drill Files merges PTH and NPTH into one file by default; KiCad's dialog writes two (medium)
- **Fixed** on task-r2-editors-exports: 7b1553b1 ("PTH and NPTH in single file", unchecked: `-PTH.drl`/`-NPTH.drl`).
- **What differs.**
  - Ours: `GenerateDrillDialog.tsx:14` `useState(false)` for "Generate independent files for NPTH and PTH holes", so one `<name>.drl` is written (kicad-cli's default, `m_excellonCombinePTHNPTH = !separate_th`).
  - KiCad: the checkbox is the other way round. It reads "PTH and NPTH in single file" (`m_Check_Merge_PTH_NPTH`, `dialog_gendrill_base.cpp:71`), has no SetValue, and starts unchecked. `excellonWriter.SetOptions( ..., m_Check_Merge_PTH_NPTH->IsChecked() )` writes `<name>-PTH.drl` and `<name>-NPTH.drl`.
  - Our label is not KiCad's either.
- **How to see it.** Open the dialog and press Generate. We write `export/kicad/drill/<name>.drl`; KiCad writes `-PTH.drl` and `-NPTH.drl`. (Run: kicad-cli without `--excellon-separate-th` wrote the single `issue5093.drl`.)
  - Test: e2e or unit: the default request has `separate_th: true`, and the checkbox is labelled "PTH and NPTH in single file", inverted.
- **Severity.** Medium (a wrong default; the file set differs).
- **Already known?** No. The dialog's header comment says the other options are not wired, but not that this default is reversed.

### X3. Gerbers: KiCad's Plot dialog plots with the board's plot settings; ours plots with kicad-cli's defaults, so the file names (Protel extensions) and options differ, and an imported board's plot settings are dropped (high)
- **Fixed** on task-r2-editors-exports: 7b1553b1 (`eda_model::plot_params` overlay, kept on import and written back; the plot passes the block's options as kicad-cli flags rather than `--board-plot-params`, which plots every copper layer whatever the checklist; `pcbplotparams_survive_import_and_write`, `the_plot_uses_the_boards_plot_settings`, slow `fab_dialogs::the_gerbers_are_named_and_made_as_the_boards_plot_settings_say`). Also r2-board-io F11.
- **What differs.**
  - Ours: `fab_api.rs:40` `gerber_args` is `--layers ...` only, without `--board-plot-params`. kicad-cli's defaults (`command_pcb_export_gerber.cpp::populateJob`) include `m_useProtelFileExtension = !m_argParser.get<bool>( ARG_NO_PROTEL_EXTENSION )`, which is **on**. So files are named `-F_Cu.gtl`, `-B_Cu.gbl`, `-Edge_Cuts.gm1`.
  - KiCad's dialog uses the board's `PCB_PLOT_PARAMS`, whose defaults (`pcb_plot_params.cpp::PCB_PLOT_PARAMS::PCB_PLOT_PARAMS`) are `m_useGerberProtelExtensions = false`, so `-F_Cu.gbr`. They also include `m_sketchDNPFPsOnFabLayers = true`, `m_crossoutDNPFPsOnFabLayers = true` (kicad-cli: both off unless `--sdnp`/`--cdnp`), `m_drillMarks = SMALL_DRILL_SHAPE`, `m_gerberPrecision`, `m_subtractMaskFromSilk` and the board's own `m_layerSelection`.
  - The derived board's `(pcbplotparams ...)` is a fixed block (`crates/kicad/src/pcb.rs:147-188`). Only the dash ratios are read back (`crates/kicad/src/import.rs:599-610`, `import_formatting_and_hatch_offsets`). An imported project's "Use Protel filename extensions", "Subtract soldermask from silkscreen", precision 5, "Plot reference designators" off, the layer selection and the output directory are lost on import and on every write of the board.
- **How to see it.** Run on QA `issue5093.kicad_pcb`:
  - `kicad-cli pcb export gerbers --layers F.Cu,B.Cu,Edge.Cuts` gave `issue5093-F_Cu.gtl, -B_Cu.gbl, -Edge_Cuts.gm1`.
  - `--board-plot-params` gave `issue5093-F_Cu.gbr, ...`, all `.gbr`. That board has `(usegerberextensions false)`.
  - Tests:
    - `crates/kicad/tests`: `pcbplotparams_survive_import_and_write`. Import a board with `(usegerberextensions yes) (subtractmaskfromsilk yes) (gerberprecision 5) (plotreference no)`, write it, and assert that the four are kept.
    - `fab_api`: `the_plot_uses_the_boards_plot_settings`, asserting that `--board-plot-params` is passed (or the equivalent flags, `--no-protel-ext`, ...).
- **Severity.** High. A board's own plot settings are lost (data loss in the file KiCad reads), and the Gerber names differ from KiCad's default for every board.
- **Already known?** No. GAPS has no Plot-dialog item. `PlotDialog.tsx:1-5` says the dialog's options "aren't ported", but nothing tracks that the board's stored settings are dropped.

### X4. STEP/GLB/... export of a board whose outline is not closed: KiCad refuses, ours writes the file and drops kicad-cli's warning (high)
- **Fixed** on task-r2-editors-exports: 1b99cd05 (refused in KiCad's words before kicad-cli runs; kicad-cli's output kept as the reply's `log` and shown; `a_3d_export_of_an_open_outline_is_refused_in_kicads_words`).
- **What differs.**
  - KiCad (`dialog_export_step.cpp::DIALOG_EXPORT_STEP::onExportButton`) runs `BuildBoardPolygonOutlines( m_editFrame->GetBoard(), outline, maxError, chainingEpsilon, false )` first, and on failure shows `"Board outline is missing or not closed using %.3f mm tolerance.\nRun DRC for a full analysis."` and does not run kicad-cli.
  - Ours: `board_output_api.rs:251` `three_d` runs kicad-cli directly. kicad-cli warns `Board outline is malformed. Run DRC for a full analysis.` and still writes a STEP. `kicad-engine/src/lib.rs:1648` `run_export` puts stdout/stderr in the reply only on failure. The reply is `ok` and the warning is lost.
- **How to see it.** Run: QA `issue5093.kicad_pcb` with one Edge.Cuts `gr_line` (line 13096) deleted. `kicad-cli pcb export step --no-components --drill-origin --min-distance=0.001mm -f -o open.step open.kicad_pcb` printed the warning, exit 0, and wrote an 88 kB STEP.
  - Test: `board_output_api` (slow): `a_3d_export_of_an_open_outline_is_refused_in_kicads_words`, an open-outline design, `three_d(dir, b"{}")["ok"] == false` and the message as quoted (with `0.001`).
- **Severity.** High (a wrong result is delivered as a success).
- **Already known?** No.

### X5. Export VRML's origin default: KiCad's "User defined origin" is checked at (0, 0); ours is the board centre (medium)
- **Fixed** on task-r2-editors-exports: 1b99cd05.
- **What differs.**
  - KiCad: `dialog_export_vrml_base.cpp:45` `m_cbUserDefinedOrigin->SetValue(true);` with X and Y "0". `dialog_export_vrml.cpp::BOARD_EDITOR_CONTROL::ExportVRML` uses `dlg.GetXRefMM()`/`GetYRefMM()` while it is checked.
  - Ours: `web/studio/src/api/boardControl.ts:109` `DEFAULT_VRML_OPTIONS.user_origin: false`, so `board_output_api.rs:117` `vrml_args` sends the board centre. The default VRML is shifted by the board centre relative to KiCad's.
  - The label "User defined origin (the board center otherwise)" is not KiCad's "User defined origin".
- **How to see it.** Open Export VRML and export with the defaults. Ours: `--user-origin=<cx>x<cy>mm`; KiCad: origin (0, 0).
  - Test: `boardControl.test.ts` `vrml defaults are the dialog's`: `DEFAULT_VRML_OPTIONS.user_origin === true`.
- **Severity.** Medium (a wrong default).
- **Already known?** No. `PARITY-boardctl.md:70` says "identical".

### X6. "Board center" for STEP and VRML is the centre of our largest outline ring, not KiCad's `ComputeBoundingBox( true, true )` (medium)
- **Fixed** on task-r2-editors-exports: 1b99cd05 (STEP: `board_edges_bounding_box`; VRML unchecked: no `--user-origin`, kicad-cli computes it; `the_board_centre_counts_every_edge_cuts_shape`).
- **What differs.**
  - Ours: `board_output_api.rs:50` `board_center_mm` and `:245` `outline_center` take the bounding box of `design.placement.outline`. That is `refresh_outline_summary`'s `main_ring()` (`crates/drc/src/outline.rs:1002`), the largest ring only.
  - KiCad (`board.cpp::BOARD::ComputeBoundingBox`, with `aBoardEdgesOnly`): it merges `item->GetBoundingBox()` of **every** Edge.Cuts `PCB_SHAPE` on the board and of every footprint's Edge.Cuts shapes (`for( const BOARD_ITEM* edge : footprint->GraphicalItems() ) if( edge->GetLayer() == Edge_Cuts && edge->Type() == PCB_SHAPE_T ) bbox.Merge( ... )`).
  - Both `DIALOG_EXPORT_STEP` ("Board center origin") and `ExportVRML` (the unchecked box) use it.
  - For VRML, kicad-cli computes exactly this itself when no `--user-origin` is given (`pcbnew_jobs_handler.cpp::JobExportStep`, `if( !aStepJob->m_hasUserOrigin ) { BOX2I bbox = brd->ComputeBoundingBox( true, true ); ... }`). Passing our centre is both unnecessary and wrong.
- **How to see it.** On a board with two separate outlines (a panel of two boards side by side, or a footprint's Edge.Cuts slot outside the main ring), export VRML with the box unchecked. Our origin is the centre of the larger board only.
  - Test: `board_output_api::tests::the_board_centre_counts_every_edge_cuts_shape`. Two rectangles, (0..10, 0..10) and (20..30, 0..10). Assert a centre of (15, 5), where ours gives (5, 5) or (25, 5).
- **Severity.** Medium.
- **Already known?** No.

### X7. Board > Fabrication Outputs > BOM is not `GenBOMFileFromBoard`'s file (medium)
- **Fixed in part** on task-r2-editors-exports: baff7dc0 (`kicad-cli pcb export bom` on the board: board-only footprints in; the empty-board message). Left: kicad-cli's writer quotes every value, keeps the library prefix and has no `;;;` (no kicad-cli option for them; GAPS 37).
- **What differs.**
  - Ours: `board_output_api.rs:224` `pcb_bom_args` runs `kicad-cli sch export bom` on the derived **schematic**.
  - KiCad (`pcbnew/build_BOM_from_board.cpp::BOARD_EDITOR_CONTROL::GenBOMFileFromBoard`) walks `board->Footprints()`, skipping only `FP_EXCLUDE_FROM_BOM`. It groups by `m_Val` and **`m_FPID`**, and writes:
    - the header `"Id";"Designator";"Footprint";"Quantity";"Designation";"Supplier and ref";` (trailing `;`);
    - rows `msg << id++ << wxT( ";\"" ) ... << From_UTF8( curEntry.m_FPID.GetLibItemName().c_str() ) << wxT( "\";" ); msg << curEntry.m_Count << wxT( ";\"" ); msg << curEntry.m_Val << wxT( "\";;;\n" );`.
  - So in KiCad's file the Id and the Quantity are unquoted, the Footprint is the **item name without the library**, and each row ends in `;;;`. Board-only footprints (no symbol: mounting holes, logos, test points placed on the board) are in it.
  - It refuses an empty board with `"Cannot export BOM: there are no footprints on the PCB."`.
- **How to see it.** Run: `kicad-cli sch export bom` with `pcb_bom_args()` on QA `component_classes.kicad_sch` gave `"1";"C1, C2";"Capacitor_SMD:C_0805_2012Metric";"2";"C";""`. KiCad's board BOM row is `1;"C1, C2";"C_0805_2012Metric";2;"C";;;`.
  - Test: `pcb_bom_matches_gen_bom_file_from_board`, on a board with a board-only footprint MH1. Assert that the header line ends with `;`, that row 1 is `1;"C1, C2";"C_0805_2012Metric";2;"C";;;`, and that MH1 is present.
- **Severity.** Medium (a different file; no connectivity at stake).
- **Already known?** In part. `PARITY-boardctl.md:71` "partial" notes only the empty Footprint column. The library prefix, the quoting, the trailing `;`, the board-only footprints and the empty-board message are not listed.

### X8. Generate Placement Files: missing options, an extra Side control, no "No footprint for automated placement." (medium)
- **Fixed** on task-r2-editors-exports: d018db9b (Gerber X3, negate X, board edge, no side choice, units order, the report and "No footprint for automated placement."). Left: Exclude from BOM (no kicad-cli flag, shown disabled), the variant. Note: KiCad's Gerber X3 names are `-pnp_top.gbr`/`-pnp_bottom.gbr` (`PLACEFILE_GERBER_WRITER::GetPlaceFileName`), not `-top-pos.gbr`.
- **What differs.** Ours: `web/studio/src/components/FootprintPositionDialog.tsx`, `fab_api.rs:129` `pos`. KiCad: `dialog_gen_footprint_position.cpp`, `dialog_gen_footprint_position_file_base.cpp`.
  - **Not offered:**
    - "Use negative X coordinates for footprints on bottom layer" (`m_negateXcb`; kicad-cli has `--bottom-negate-x`);
    - the "Gerber X3" format (`m_formatCtrl` choice 2, `CreateGerberFiles`, `-top-pos.gbr`/`-bottom-pos.gbr` via `PLACEFILE_GERBER_WRITER::GetPlaceFileName`; kicad-cli `--format gerber --side front|back`, `--gerber-board-edge`) with "Include board edge layer";
    - "Exclude all footprints with the Exclude from BOM flag set" (`m_excludeBOM`; kicad-cli 25ec2335 has no flag for it, `m_excludeBOM` stays `false`, so it cannot be passed through as things stand);
    - the design variant choice.
  - **Extra:** KiCad's dialog has no Side choice (`m_job->m_side = JOB_EXPORT_PCB_POS::SIDE::BOTH` and `CreateAsciiFiles` always does both sides). Our Front/Back/Both radios are the studio's own.
  - **Message:** `CreateAsciiFiles` first counts candidates and, with none, shows `wxMessageBox( _( "No footprint for automated placement." ) )` and writes nothing. Ours writes a header-only file and reports success. The report lines `"Placement file: '%s'."`, `"Front (top side) placement file: '%s'."`, `"Back (bottom side) placement file: '%s'."`, `"Component count: %d."`, `"Full component count: %d."`, `"Done."` are ours as "Position file written.".
  - The units choice is listed Inches, Millimeters in KiCad; ours is the other way round.
- **How to see it.** On a board where every footprint is through-hole, tick "Include only SMD footprints" and Generate. KiCad says "No footprint for automated placement."; ours writes `<name>-all-pos.csv` with only the header.
  - Test: `fab_api` `pos_with_no_candidate_is_refused_in_kicads_words`.
- **Severity.** Medium (a missing message; missing options). The Side radios and the order are low.
- **Already known?** No. The 4611df84 review of the names is right: `place_file_names` matches `DecorateFilename` and `CreateAsciiFiles`, see "Checked" below.

### X9. Schematic Plot dialog defaults: KiCad starts at PDF with "Plot background color" unchecked; ours starts at SVG with the background on (medium)
- **Fixed** on task-r2-editors-exports: b4db06f7.
- **What differs.**
  - KiCad: `eeschema/dialogs/dialog_plot_schematic_base.cpp:57` `m_plotFormatOpt->SetSelection( 1 )` is PDF, out of Postscript/PDF/SVG/DXF/PNG. `m_plotBackgroundColor` (`:107`, label "Plot background color") has no SetValue, so it is unchecked, and `plotOpts.m_useBackgroundColor = m_plotBackgroundColor->GetValue()`.
  - KiCad's `DIALOG_SHIM::LoadControlState` (`common/dialog_shim.cpp:672`) also restores the last choices.
  - Ours: `web/studio/src/kicad-port/schOutputs.ts:32-38` `DEFAULT_SCH_PLOT_FORM` = `format: "svg"`, `useBackgroundColor: true`, so no `--no-background-color` is passed and kicad-cli fills the background. The doc comment cites `SCH_PLOT_OPTS::SCH_PLOT_OPTS`, which is not where the dialog takes its initial state.
  - `PlotSchematicDialog.tsx:30` resets the form on every open.
  - The label is "Use background color", where KiCad's is "Plot background color".
- **How to see it.** Open File > Plot on the Schematic tab and press Plot. Ours plots an SVG with a background rectangle; KiCad plots a PDF without one.
  - Test: `schOutputs.test.ts` `the plot dialog starts as KiCad's`: `DEFAULT_SCH_PLOT_FORM.format === "pdf" && !DEFAULT_SCH_PLOT_FORM.useBackgroundColor`.
- **Severity.** Medium (a wrong default).
- **Already known?** No. `PARITY-sch.md:384` says "identical".

### X10. The PCB Plot dialog has only Gerber and three controls (medium, a tracking gap)
- **Tracked** on task-r2-editors-exports: d2b7f55a (GAPS item 37; the board's own settings now reach the plot, X3).
- **What differs.** KiCad's `DIALOG_PLOT` (`pcbnew/dialogs/dialog_plot.cpp`) also offers:
  - the formats PostScript, SVG, DXF, HPGL and PDF;
  - "Plot drawing sheet", "Plot footprint values" and "Plot reference designators";
  - "Sketch pads on fabrication layers", the DNP options and the drill marks;
  - "Use Protel filename extensions", "Generate Gerber job file", "Coordinate format" 4.5/4.6, "Use extended X2 format", "Include netlist attributes", "Disable aperture macros" and "Subtract soldermask from silkscreen";
  - the output directory, "Generate Drill Files..." and Run DRC.

  kicad-cli has a flag for each. Ours (`PlotDialog.tsx`) has layers, "Check zone fills before plotting" and "Use drill/place file origin".
- **How to see it.** Open File > Plot on the PCB tab.
- **Severity.** Medium (missing controls; see X3 for the effect on the file).
- **Already known?** Only in the component's header comment. It is in neither `GAPS.md` nor a PARITY row. It should get a GAPS line.

### X11. Export Symbol as SVG: "no symbol to export" (low)
- **Fixed** on task-r2-editors-exports: bb470595 (the message; the theme/margin difference is in GAPS 37).
- **What differs.** `crates/cli/src/sch_export_api.rs:239` says `"no symbol to export"`. KiCad (`symbol_editor_control.cpp::SYMBOL_EDITOR_CONTROL::ExportSymbolAsSVG`) says `wxMessageBox( _( "No symbol to export" ) )`. The editor's export plots in colour with the editor's colour theme and a 10% margin (`symbolBBox.Inflate( size * 0.1 )`); kicad-cli's `sym export svg` plots with its own theme and page.
- **Severity.** Low.
- **Already known?** No.

### Checked and found to match (exports)
- **Placement file names (4611df84):** `kicad-engine/src/lib.rs:1568` `place_file_names`.
  - It matches `PLACE_FILE_EXPORTER::DecorateFilename` (`-all`/`-top`/`-bottom`, `GetFrontSideName()` "top", `GetBackSideName()` "bottom") and `CreateAsciiFiles` (`FootprintPlaceFileExtension` "pos"; CSV `-pos` + `.csv`).
  - The two-file mode is two kicad-cli runs with `--side front` / `--side back`. That is what `CreateAsciiFiles` does (`DoGenFootprintsPositionFile` with `topSide=true,bottomSide=false`, then `false,true`). kicad-cli forces `m_singleFile = true; m_nakedFilename = true` (`command_pcb_export_pos.cpp`), so naming the file ourselves is right.
  - The dialog's defaults match `dialog_gen_footprint_position_file_base.cpp`: Plain text, Millimeters (`SetSelection( 1 )`), "Use drill/place file origin" checked, single file checked. kicad-cli's own `--units in` default is overridden because `--units` is always passed.
- **STEP and the other 3D formats:** `board_output_api.rs:66` `three_d_args` against `DIALOG_EXPORT_STEP::onExportButton`, flag by flag and in order:
  - `--no-unspecified`, `--no-dnp`, `--subst-models`, `--no-optimize-step`, `--no-board-body`, `--no-components`, the include flags, `--fuse-shapes`, `--cut-vias-in-body`, `--fill-all-vias`, `--net-filter`, `--component-filter` (Only selected and matching filter), the origin choice, `--min-distance=%.3fmm`, `-f`;
  - `c_formatCommand`'s nine formats;
  - the defaults (`dialog_export_step_base.cpp`): drill/place origin, Tight 0.001 mm, board body, components, substitute models, "Don't write P-curves", overwrite.
- **GenCAD:** `gencad_args` against `DIALOG_GENCAD_EXPORT_OPTIONS::createOptCheckboxes` (labels word for word, all unchecked) and `ExportGenCAD`.
- **IPC-2581:** `ipc2581_args` against `DIALOG_EXPORT_2581` (units, precision 4..10 default 6, version C, compress, the BOM columns). The BOM revision falls back to the project's in kicad-cli as in `GenerateFile`.
- **ODB++:** `odb_args`. Precision 2..16, default 6, and ZIP (`dialog_export_odbpp_base.cpp:64,78`).
- **IPC-D-356** (no options) and the file extensions of `pcb_ext`.
- **Drill origin:** `with_aux_origin`. `--drill-origin plot` for drill, `--use-drill-file-origin` for pos and gerbers, as in the CLI's arguments.
- **Plot "Check zone fills before plotting"** default on.
- **Gerber default layer list:** `default_gerber_layers` matches `PCB_PLOT_PARAMS`' `m_layerSelection` (AllCu + F/B SilkS, Mask, Paste, Edge.Cuts) for a new board.
- **Schematic plot arguments:** `sch_output_api.rs:62-76` (`--black-and-white`, `--exclude-drawing-sheet`, `--no-background-color` only in colour, `--pages`).

---

## Part B. Symbol and Footprint Editors


Code read on main's checkout (HEAD `5cca5fff`), KiCad `kicad-src-8303b2ad` (`25ec2335` checked where it says so). Priority (a) was 4a3fc95c and c943dd26 (the pin tool and New Symbol ask first, and the four review corrections). Then the most-used "identical"/"done" rows of `PARITY-symedit.md` and `PARITY-fpedit.md` that round 1 did not review. Round 1 covered only custom pads and the Footprint Checker in this area (p7-p8, p11-p18).

Practical check: the debug `eda` binary was built 2026-10-23, so it is older than the two commits. I served a scratch `ldo` board on port 8848 (stopped since) and ran `POST /api/cmd` `new_footprint` / `new_symbol` + `edit_symbol_properties`, then `GET /api/footprint/export` and `/api/symbol/export`, then `kicad-cli fp upgrade --force` on the exported `.kicad_mod`. Scratch: `/tmp/findings-r2-editors-exports.GtQ7/ed/`. The writer code paths I relied on (`footprint_lib.rs::write_library_fields`, `NewSymbolDialog.tsx`) are the same on main.

---

### Findings

#### E1. A new footprint (or any library footprint with no stored Reference/Value layout) is exported with no Reference and no Value, and has no `${REFERENCE}` fab text. KiCad then reads both fields as empty. **high**
- **Fixed** on task-r2-editors-exports: 1b77de3e (and the low: the attributes of the library's last footprint).
- **What differs.**
  - Ours: `crates/ops/src/lib.rs:5381-5393` (`new_footprint`) is `LibraryFootprint::new_empty(name)` plus `attributes.smd = true`, with no texts.
  - `crates/kicad/src/footprint_lib.rs:285-303` (`write_library_fields`) writes the Reference only when `fp.reference` is `Some`, or when the Reference is hidden. The Value follows the same rule. A visible, never-placed field is therefore not written at all. The unit test `export_kicad_mod_writes_the_fields_with_their_layouts_and_reads_them_back` (`footprint_lib.rs:791-792`) asserts exactly this: "a footprint with no fields writes none".
  - KiCad: `footprint_libraries_utils.cpp::PCB_BASE_FRAME::CreateNewFootprint` fills the fields from `m_DefaultFPTextItems`. The footprint editor's defaults are `{ "REF**", true, F_SilkS }, { "", true, F_Fab }, { "${REFERENCE}", true, F_Fab }` (`footprint_editor_settings.cpp:189-191`):
    ```cpp
    footprint->Reference().SetText( settings.m_DefaultFPTextItems[0].m_Text );
    ...
    default_pos.y -= settings.GetTextSize( txt_layer ).y / 2;
    footprint->Reference().SetPosition( default_pos );
    ...
    for( size_t i = 2; i < settings.m_DefaultFPTextItems.size(); ++i )
    {   PCB_TEXT* textItem = new PCB_TEXT( footprint ); ... footprint->GraphicalItems().push_back( textItem ); }
    if( footprint->GetValue().IsEmpty() )
        footprint->SetValue( aFootprintName );
    ```
    It also sets every text's size, thickness, italic and keep-upright from the layer defaults (1.0 mm; 0.1 mm on silk, 0.15 mm on fab). With 1 mm text the positions are REF** at (0, -0.5) on F.SilkS, Value = name at (0, 1.0) on F.Fab, and `${REFERENCE}` at (0, 2.5) on F.Fab.
- **How to see it.** I ran `new_footprint {"name":"Untitled"}`, then `GET /api/footprint/export?name=Untitled`. It wrote only `(layer "F.Cu") (attr smd)`. After `kicad-cli fp upgrade --force` the file has `(property "Reference" "" (at 0 0 0) (layer "F.SilkS") ... (size 1.27 1.27))` and `(property "Value" "" (at 0 0 0) (layer "F.Fab") ...)`: no REF**, an empty Value, and the `${REFERENCE}` text missing. The same happens to a builtin footprint materialized by `from_engine_footprint` (`ir.rs:3685`, reference `None`). It also happens to "Copy Footprint", whose clipboard holds the same text.
- **Test.** `crates/kicad/src/footprint_lib.rs` `a_new_footprint_writes_kicads_default_fields`: export `LibraryFootprint` as `new_footprint` makes it, then assert:
  - it contains `(property "Reference" "REF**" (at 0 -0.5 0) (layer "F.SilkS")`;
  - it contains `(property "Value" "Untitled" (at 0 1 0) (layer "F.Fab")`;
  - it has an `(fp_text user "${REFERENCE}" (at 0 2.5 0) (layer "F.Fab")` or KiCad 9's `(property ...)`-free `fp_text`.
  - A slow variant: kicad-cli's `fp upgrade` keeps REF**.
- **Already known?** No. The test that pins the current behaviour must change with the fix.
- Related, **low**: `CreateNewFootprint` takes the new footprint's attributes from the library's last footprint (`fp = adapter->LoadFootprint( aLibName, fpnames.back() ) ... footprintAttrs = fp->GetAttributes();`), falling back to `FP_SMD`. Ours is always SMD.

#### E2. The Pin tool gives a new pin a number. KiCad's `CreatePin` leaves it empty. **medium**
- **Fixed** on task-r2-editors-exports: fea85246.
- **What differs.**
  - Ours: `SymbolEditorCanvas.tsx:397-398` calls `nextPinNumber(...)` and passes it to `newPinFromClick`. PARITY-symedit row 102 calls this "identical" ("The number is the next free one after the pin just placed (`IncrementString` ... like the C++ tool's `m_lastPin`)"). The `useActionRunner.ts:2280` comment says the same.
  - KiCad: `symbol_editor_pin_tool.cpp::CreatePin` sets the position, length, orientation, type, shape, the two sizes, unit, body style and visibility, and **no number**. `SCH_PIN::SCH_PIN( LIB_SYMBOL* )` (`sch_pin.cpp:123`) has an empty number. `DIALOG_PIN_PROPERTIES::TransferDataToWindow` shows `m_pin->GetNumber()`, which is empty.
  - `g_lastPin` (`symbol_editor_drawing_tools.cpp:218-221`) is only read by `RepeatDrawItem`. `IncrementString` is only called in `RepeatPin`. The same holds in 25ec2335.
- **How to see it.** In KiCad, press P on an empty symbol and click: Pin Properties opens with an empty "Pin number". Ours shows "1", then "2", and so on.
- **Test.** `symPinTemplate.test.ts` `a clicked pin has no number, as CreatePin leaves it`: `newPinFromClick(DEFAULT_PIN_TEMPLATE, 1, 1, ..., at)` has `number === ""`. The canvas would then stop calling `nextPinNumber` (keep it for Repeat, E3).
- **Already known?** No. The doc claim is wrong.

#### E3. The Symbol Editor has no Repeat (Insert), no item Duplicate (Ctrl+D), and no item Copy/Paste. The action audit counts all three as wired. **medium**
- **Fixed** on task-r2-editors-exports: fea85246 (the audit counts by action name and cannot tell tabs apart; noted in PARITY-symedit).
- **What differs.**
  - Ours: `eeschema.InteractiveEdit.repeatDrawItem` is registered `schematicOnly` (`useActionRunner.ts:2271-2277`). `common.Interactive.duplicate` is handled for `pcb` and `footprint` only (`useActionRunner.ts:640-643`). `common.Interactive.copy`/`paste` are `pcbOnly` (`:644-645`), and the schematic sweep covers the schematic. The symbol canvas' context menu (`SymbolEditorCanvas.tsx:604-624`) has no Duplicate. So on the Symbol tab, Insert, Ctrl+D, Ctrl+C and Ctrl+V do nothing. Because the action ids are wired elsewhere, `ui-parity-missing.json` lists none of this.
  - KiCad:
    - `SYMBOL_EDITOR_DRAWING_TOOLS::RepeatDrawItem` calls `SYMBOL_EDITOR_PIN_TOOL::RepeatPin`. That duplicates `g_lastPin` and moves it by `m_Repeat.pin_step` (down for a right/left pin, across for up/down). It runs `IncrementString( nextName, cfg->m_Repeat.label_delta )` and `IncrementString( nextNumber, ...)`, then `PlacePin`, then `commit.Push( _( "Repeat Pin" ) )`.
    - `SYMBOL_EDITOR_EDIT_TOOL::Duplicate` gives each duplicated pin that has a number `wxString::Format( wxT( "%i" ), symbol->GetMaxPinNumber() + 1 )`. It does this in `StrNumCmp` order of the old numbers, then starts a move and pushes "Duplicate".
    - `SYMBOL_EDITOR_EDIT_TOOL::Paste` parses the clipboard as library symbols, puts items on the current unit and body style unless they are common, and hands them to the move tool ("Paste").
- **How to see it.** Place pin "1" named "A1", press Insert: KiCad adds pin "2" named "A2" 100 mil below it. Ours does nothing. Select a pin and press Ctrl+D: nothing.
- **Test.** An e2e scenario in `lib-edit.check.js`, `Insert repeats the last pin with the next number and name`: after one placed pin `1`/`A1` at (0,0) facing right, Insert gives a second pin `2`/`A2` at (0,-2.54). A unit test, `symRepeat.test.ts`, for the step per orientation and `IncrementString`.
- **Already known?** No. Neither GAPS item 19 nor PARITY-symedit names it.

#### E4. In Synchronized Pins Mode, deleting a pin leaves its twins in the other units. Delete of several items is one undo step per item. **medium**
- **Fixed** on task-r2-editors-exports: fea85246.
- **What differs.**
  - Ours: `editorFrameActions.ts:147-155` (symbol) and `editorAdapter.ts:339-346` loop over the selection and call `symApi.deletePin(id)` once per pin. That is one `delete_symbol_pin` each (`symbolEditorStore.tsx:624-629`; `ops/lib.rs:5898` removes the one id). There is no sync step and no batch.
  - KiCad: `symbol_editor_edit_tool.cpp::SYMBOL_EDITOR_EDIT_TOOL::DoDelete` does it in one commit:
    ```cpp
    // when pin editing is synchronized, pins in the same position, with the same name
    // in different units are also removed.  But only one pin per unit (matching)
    if( m_frame->SynchronizePins() )
    { ... if( pin->GetPosition() != pos ) continue; if( pin->GetBodyStyle() != curr_pin->GetBodyStyle() ) continue;
          if( pin->GetType() != curr_pin->GetType() ) continue; if( pin->GetName() != curr_pin->GetName() ) continue;
          toDelete.insert( pin ); got_unit[pin->GetUnit()] = true; }
    ```
    A selected visible field is hidden (`field->SetVisible( false )`), not deleted.
  - The Footprint Editor has the same one-step-per-item delete: `editorFrameActions.ts:105-114` says "each its own verb". KiCad's `EDIT_TOOL::Remove` is one commit.
- **How to see it.** Take a 2-unit symbol with interchangeable units. Place pin 1 on unit A, so its image "1-UB" appears on B. Delete pin 1 on A: KiCad removes both, ours leaves "1-UB". Then select three pins and Delete: Undo brings back one pin, not three.
- **Test.** `symbolEditorStore` or an e2e test, `delete in sync mode takes the twins, one undo step`: assert one `batch` command holding both `delete_symbol_pin`s, and that one undo restores both.
- **Already known?** No. The PARITY-symedit "Synchronized Pins Mode" row (134) lists placing, moving and editing, not deleting.

#### E5. Rotate of several pins turns each pin in place, and in sync mode it also rotates the twins. It is one undo step per pin. **medium**
- **Fixed** on task-r2-editors-exports: fea85246.
- **What differs.**
  - Ours: `editorFrameActions.ts:133-145` (`rotatePins`) changes `angle_deg` of each selected pin around its own position, through `symApi.editPin` once per pin. `editPin` (`symbolEditorStore.tsx:630-645`) adds `planSyncedEdit`'s updates in sync mode, so the twins in the other units turn too.
  - KiCad: `SYMBOL_EDITOR_EDIT_TOOL::Rotate` rotates the whole selection about one point in one commit `_( "Rotate" )`, and it never touches pins outside the selection:
    ```cpp
    if( selection.GetSize() == 1 ) rotPoint = item->GetPosition();
    else rotPoint = m_frame->GetNearestHalfGridPosition( selection.GetCenter() );
    for( ... ) item->Rotate( rotPoint, ccw );
    ```
- **How to see it.** Select pins at (0,0) and (0,-2.54) and press R. KiCad moves the second pin to (2.54,0)-ish about the snapped centre. Ours leaves both where they are and turns them. In sync mode, unit B's pins turn as well.
- **Test.** `libEditorEdit.test.ts` `rotating two pins turns them about the half-grid centre`: the positions after one CCW turn about `nearestHalfGrid(centre)`, and a single command.
- **Already known?** In part: the toolbar note says shape rotation is unported (PARITY-symedit:123). The pivot, the sync side effect and the undo steps are not listed.

#### E6. "Pin name inside" with an offset of 0 is written as 20 mil. KiCad writes 1 IU from New Symbol and 20 IU from Symbol Properties. **medium**
- **Fixed** on task-r2-editors-exports: fea85246.
- **What differs.**
  - Ours: `NewSymbolDialog.tsx:95-97` and `SymbolPropertiesDialog.tsx:108` both do `namesInside ? (offset === 0 ? 0.508 : offset) : 0`. The comment says this is "as Symbol Properties' 'Place pin names inside' already makes it" (c943dd26).
  - KiCad's `lib_symbol_library_manager.cpp::LIB_SYMBOL_LIBRARY_MANAGER::CreateSymbol`:
    ```cpp
    if( aProps.pinNameInside )
    {   new_symbol->SetPinNameOffset( aProps.pinTextPosition );
        if( new_symbol->GetPinNameOffset() == 0 )
            new_symbol->SetPinNameOffset( 1 ); }
    ```
    That is 1 schematic IU = 0.0001 mm.
  - KiCad's `dialog_lib_symbol_properties.cpp::TransferDataFromWindow` (`:694-697`): `int offset = KiROUND( (double) m_pinNameOffset.GetValue() ); ... SetPinNameOffset( offset == 0 ? 20 : offset );`. `UNIT_BINDER::GetValue` is in IU, so this is 0.002 mm, not 20 mil.
  - The writer (`sch_io_kicad_sexpr_lib_cache.cpp:430-439`) prints `(offset ..)` whenever the offset is not `MilsToIU( DEFAULT_PIN_NAME_OFFSET )`. So KiCad's file says `(pin_names (offset 0.0001))` or `(offset 0.002)`, where ours omits the offset (the default 0.508).
- **How to see it.** New Symbol with offset 0 and "Pin name inside" checked. KiCad draws the names right at the pin's inner end. Ours draws them 0.508 mm in, and the `.kicad_sym` differs. The model keeps 0.0001: `edit_symbol_properties` with `pin_name_offset_mm: 0.0001` exported `(pin_names (offset 0.0001) ...)`.
- **Test.** `symEditActions.test.ts` (or a pure helper both dialogs share) `an inside offset of 0 is 1 IU from New Symbol and 20 IU from Symbol Properties`: 0.0001 mm and 0.002 mm.
- **Already known?** No. c943dd26 introduced the New Symbol half and the doc claims parity.

#### E7. Pin Properties never gives KiCad's "not on a 50 mils grid" warning. **medium**
- **Fixed** on task-r2-editors-exports: fea85246. Note: KiCad tests `% 50` on the value in internal units (`m_posX.GetValue()`, 100 nm), not mils, so X = 1 mm does not ask; ported as the code does.
- **What differs.** `PinPropertiesDialog.tsx:70-98` (`submit`) only checks the alternates. KiCad's `dialog_pin_properties.cpp::DIALOG_PIN_PROPERTIES::TransferDataFromWindow` asks first:
  ```cpp
  if( ( m_origPos != newPos ) && ( ( m_posX.GetValue() % standard_grid ) || ( m_posY.GetValue() % standard_grid ) ) )
  {   wxString msg = wxString::Format( _( "This pin is not on a %d mils grid which will make it "
                                         "difficult to connect to in the schematic.\n"
                                         "Do you wish to continue?" ), standard_grid );
      if( !IsOK( this, msg ) ) return false; }
  ```
  `standard_grid = 50`.
- **How to see it.** Edit a pin's X to 1 mm and press OK. KiCad asks "This pin is not on a 50 mils grid which will make it difficult to connect to in the schematic. Do you wish to continue?"; ours applies it silently.
- **Test.** An e2e step in `lib-edit.check.js`: set X to 1.0 and press OK. Expect a confirm with that text; Cancel keeps the dialog open.
- **Already known?** No.

#### E8. Pin Properties' "Common to all units / body styles" boxes are always enabled. Their tooltips and the sync info bar are missing. **low**
- **Fixed** on task-r2-editors-exports: fea85246.
- KiCad's `DIALOG_PIN_PROPERTIES::TransferDataToWindow`:
  - `m_checkApplyToAllParts->Enable( IsMultiUnit() )` and `SetValue( IsMultiUnit() && GetUnit() == 0 )`;
  - `m_checkApplyToAllBodyStyles->Enable( IsMultiBodyStyle() )`;
  - an info bar "Synchronized Pins Mode." / "Synchronized Pins Mode.  New pin will be added to all units." / "Synchronized Pins Mode.  Matching pins in other units will be updated." with an "Exit sync pins mode" link;
  - tooltips "Synchronized pins mode is enabled.\nSimilar pins will be edited regardless of this option." / "If checked, this pin will exist in all units." / "This symbol only has one unit. This control has no effect.".
- On OK, `SetUnit( checked ? 0 : m_frame->GetUnit() )`, so a single-unit symbol's pin always gets unit 1.
- Ours (`PinPropertiesDialog.tsx:153-160`) lets a single-unit symbol's pin be unit 0. That 0 then sticks in the template (`pinTemplateOf`), where KiCad's `g_LastPinCommonUnit` would be false.
- Test: on a single-unit symbol the box is disabled and unchecked, and OK writes unit 1.

#### E9. When the occupied-spot question is declined, the pin template is not updated. KiCad has already saved it. **low**
- **Fixed** on task-r2-editors-exports: fea85246.
- KiCad: `CreatePin` → `EditPinProperties` saves `g_LastPin*` at its end, before `TwoClickPlace`'s second click reaches `PlacePin`. Answering Cancel there (`delete aPin; return false;`) keeps the new look.
- Ours (`PinPropertiesDialog.tsx:87-93`) returns before `SET_PIN_TEMPLATE` when the confirm is declined.
- Test: `pinPlacement`/e2e: change the type to Output, decline "Place Pin Anyway", and the next click's pin is Output.

#### E10. Docs: PARITY-symedit row 102 says "identical" for a one-click placement that the same file's "Differs from KiCad" lists as different. **low (doc)**
- **Fixed** on task-r2-editors-exports: fea85246.
- PARITY-symedit.md:259-262 already describes the one-click placement and the position taken from the dialog. In KiCad (`TwoClickPlace`), the pin follows the cursor after OK, and its place, including the occupied test, is the second click (`item->SetPosition( VECTOR2I( cursorPos.x, -cursorPos.y ) )`). Row 102 should say "differs (see below)", and the `m_lastPin`/`IncrementString` claim should go (E2).

#### E11. Footprint Editor Duplicate and Increment of several pads: we number them consecutively, KiCad gives them the same number. **low (decide: KiCad's looks like a bug)**
- **Recorded** as a deliberate difference on task-r2-editors-exports: fea85246 (PARITY-fpedit).
- KiCad's `edit_tool.cpp::EDIT_TOOL::Duplicate` (`:3370-3380`) calls `parentFootprint->DuplicateItem( true, &commit, orig_item )` with the default `addToFootprint = false` (`footprint.h:1171-1172`). So the copy is not in `m_pads` when the next pad runs `GetNextPadNumber( padTool->GetLastPadNumber() )`, which starts from the last number and returns it while it is free.
- With pads 1 and 2 selected and last number "2", KiCad gives "3" and "3". Ours (`fpEditActions.ts:48-63`, `used = [...used, copy]`) gives "3" and "4".
- Our numbering is the useful one. Record it as a deliberate difference, not a parity claim. PARITY-fpedit row 102 says "identical".

#### E12. A new symbol's Reference and Value are placed at (0, 5.08) and (0, 2.54). KiCad's are at the origin. **low**
- **Fixed** on task-r2-editors-exports: fea85246.
- `LIB_SYMBOL::LIB_SYMBOL( const wxString& aName, ...)` (`lib_symbol.cpp:226-260`) makes the five mandatory fields with `new SCH_FIELD( this, id )` at (0,0), and `CreateSymbol` sets only their texts.
- Ours: `/api/symbol/export` of a fresh `new_symbol` writes `(property "Reference" "U" (at 0 5.08 0))` and `(property "Value" "S0" (at 0 2.54 0))`.
- Cosmetic, but a file KiCad would write differently.

---

### Checked and found to match (do not review again)
- The sticky pin template: KiCad's `g_LastPin*` defaults are input / right / line / visible, the length and the name/number sizes come from `m_Defaults` (100 / 50 / 50 mil), and the name is not carried. It is saved only by `EditPinProperties`' OK, for a new pin or an edited one, and selecting a pin saves nothing. The template is kept across symbols (static). Unit/body style: `g_LastPinCommon* ? 0 : frame unit/style` (`symPinTemplate.ts`).
- Cancel in Pin Properties on a new pin places nothing (`CreatePin`: `delete pin`).
- `PlacePin`'s occupied test: same position, and a body style that is either 0 or the same. The message is "This position is already occupied by another pin, in unit %d." with the extended message "Disable the 'Synchronized Pins Mode' option to avoid this message.", the title "Confirmation", the OK label "Place Pin Anyway", and the test runs only in sync mode (`pinPlacement.ts`, `symPinSync.ts::pinOccupying`).
- `CreateImagePins`: none for a unit-0 pin, and one per other unit numbered `%s-U%c` (`imagePinsFor`). `SynchronizePins` = mode && multi-unit && !locked.
- `EditPinProperties`' sync for an edited (not new) pin goes through `planSyncedEdit`. The rule was read, but not every branch was compared.
- `PushPinProperties`: the length only for body style 0 or the shown style, the name and number sizes for every pin (`ops/lib.rs:5919-5937`).
- `DIALOG_LIB_NEW_SYMBOL`:
  - its defaults: reference "U"; units 1..64; "All units are interchangeable" unchecked; De Morgan unchecked; power unchecked; exclude BOM/board unchecked; pin name offset 20 mil; "Pin name inside", "Show pin number text" and "Show pin name text" all checked;
  - `onPowerCheckBox` checks and locks the two exclude boxes;
  - `GetName` is `EscapeString( CTX_LIBID )` with spaces made `_` (our extra trim is in the "Differs" list);
  - the validator's "Symbol must have a name." and "Symbol '%s' already exists in library '%s'." with `UnescapeString`; the missing Overwrite is documented;
  - `LockUnits( !unitsInterchangeable )` and `LockUnits(false)` for fewer than 2 units;
  - `SetShowPinNumbers`/`SetShowPinNames`;
  - one undo step.
- The pad tool: `PAD_PLACER::CreateItem` makes the pad from `m_Pad_Master` with `GetNextPadNumber( m_lastPadNumber )`, which starts at the last number and returns it if it is free, using `GetRefDesPrefix`/`GetTrailingInt` (`padNumbering.ts`). `PlaceItem` pushes the pad back into the master (`SET_DEFAULT_PAD`). `m_lastPadNumber` is "1" on load.
- `SetDefaultMasterPad`: 2.54 × 1.27 mm, round-rect radius 1.27 × 0.15, a 0.8 mm round drill, PTH on `*.Cu *.Mask` (`padSettings.ts` `DEFAULT_PAD_MASTER`).
- Duplicate and Increment of one pad: `GetNextPadNumber( last )`, and `SetLastPadNumber` (`duplicatePads`).
- New Footprint's name: "Untitled", then `Untitled_N` (`baseName + "_%d"`, idx from 1) until it is unused. The Value is the name (`footprint_lib.rs:302` `l.shown(&fp.name)`, when a Value is written at all, E1).

### Summary

| # | Finding | Severity | Known? |
|---|---|---|---|
| E1 | New/builtin library footprint exported without Reference (REF**), Value or `${REFERENCE}` text; KiCad reads empty fields | high | no |
| E2 | Pin tool pre-numbers a new pin; `CreatePin` leaves the number empty (doc claims identical) | medium | no |
| E3 | Symbol Editor: no Repeat Pin (Insert), no item Duplicate, no item Copy/Paste; hidden from the action audit | medium | no |
| E4 | Delete in sync mode leaves the twins; one undo step per deleted item (both editors) | medium | no |
| E5 | Rotate of several pins: no common pivot, rotates the sync twins, one step per pin | medium | partly |
| E6 | "Pin name inside" with offset 0 becomes 20 mil; KiCad 1 IU (New Symbol) / 20 IU (Symbol Properties) | medium | no |
| E7 | No "This pin is not on a 50 mils grid..." warning | medium | no |
| E8 | Common-to-all-units/body-styles boxes always enabled; no sync info bar or tooltips | low | no |
| E9 | Declined occupied-spot question does not save the template | low | no |
| E10 | PARITY-symedit row 102 "identical" contradicts its own Differs section | low | doc |
| E11 | Duplicate and Increment of several pads: KiCad repeats one number (KiCad bug?) | low | no |
| E12 | New symbol's Reference/Value at (0,5.08)/(0,2.54), KiCad (0,0) | low | no |

Counts: high 1, medium 6, low 5.

---

## Part C. 3D viewer, design blocks, simulator


Read on main (`5cca5fff`) against KiCad `8303b2ad`. Nothing was run. Round 1 covered P14 (3D model tab, placement chain) in `p13-p16.md` and P18 (design blocks) in `p11-p18.md`, and `PARITY-sch.md` 38.1 is a function-by-function review of the simulator. What those list is not repeated here.

Since 2026-10-24 in these paths: `8a567378` (P18-1 fix: Place Design Block grows the copper layers and keeps the nets) and `c40ebb4c` (the block's `(layers ..)` table, the 3D tab's real courtyard). Both were checked and match (see the end).

### Findings

#### V1. The 3D viewer shows User.1 .. User.45 by default; KiCad hides them (medium)
- **Fixed** on task-r2-editors-exports: d743df5a.
- **What differs.** `web/studio/src/kicad-port/appearance3d.ts:166` builds every user row with `row(..., "user", true)`, so `visible` defaults to true, and `isVisible` (`:204-208`) returns that default. `scene.ts` then draws every drawing on `User.n` (`shapeLayerOf` -> `{ kind: "user" }`).
  KiCad's defaults are in `3d-viewer/3d_viewer/eda_3d_viewer_settings.cpp::EDA_3D_VIEWER_SETTINGS`:
  ```
  for( int layer = 0; layer < 45; ++layer )
  {
      m_params.emplace_back( new PARAM<bool>( fmt::format( "render.show_user{}", layer + 1 ),
                                              &m_Render.show_user[layer], false ) );
  }
  ```
  `board_adapter.cpp::GetVisibleLayers` (the legacy or default preset) reads them: `ret.set( layer, m_Cfg->m_Render.show_user[ layer - LAYER_3D_USER_1 ] );`. `GetDefaultVisibleLayers` also sets all 45 to `false`. Only the four named layers (`render.show_comments`, `show_drawings`, `show_eco1`, `show_eco2`) default to `true`.
  `PARITY-3d.md` row 122 says "every row shown except Model Bounding Boxes". That is wrong for the 45 numbered user layers.
- **How to see it.** On a board with a `gr_line` on `User.1`, open the 3D viewer. The studio draws the line and the User.1 row's eye is open. In KiCad (fresh settings) the row is listed with its eye closed and nothing is drawn.
  Test: in `appearance3d.test.ts`, `user_defined_layers_are_hidden_by_default`. Assert `isVisible({}, "user_1") === false` and `isVisible({}, "user_45") === false`, while `isVisible({}, "user_drawings") === true` and `isVisible({}, "user_comments") === true`.
- **Already known?** No. GAPS 23 does not list it.

#### V2. A 3D model's type and DNP state ignore the board footprint's attributes, so DNP models show by default (medium)
- **Fixed** on task-r2-editors-exports: d743df5a (`part_json` from `Design::effective_attrs`; the "Models not in POS File" and "Models marked DNP" rows, P and D; `footprintShown` in `IsFootprintShown`'s order).
- **What differs.** `crates/cli/src/model3d_api.rs:619` `part_json` gives `kind3d` from `crates/kicad/src/model3d.rs:209-264` `part_models`. That kind is the installed library footprint's `(attr ..)`, or else `ModelKind::of_pads` (any PTH pad makes it TH, any SMD pad makes it SMD, otherwise virtual). It never reads the board footprint's own attributes. The IR has those attributes now: `FootprintAttrs { kind, dnp, exclude_from_pos_files, .. }` (`crates/model/src/fp_edit.rs:231`), set by Footprint Properties (`FootprintEdit::attrs`) and by the symbol's DNP flag (`Design::effective_attrs`, `fp_edit.rs:507`). `partModels.ts:90` then gates the model on that kind alone.
  KiCad's `board_adapter.cpp::BOARD_ADAPTER::IsFootprintShown` uses the board footprint's own flags:
  ```
  if( excludedFromPos ) { if( !m_Cfg->m_Render.show_footprints_not_in_posfile ) return false; }
  if( dnp )             { if( !m_Cfg->m_Render.show_footprints_dnp ) return false; }
  if( attributes & FP_SMD )          return m_Cfg->m_Render.show_footprints_insert;
  if( attributes & FP_THROUGH_HOLE ) return m_Cfg->m_Render.show_footprints_normal;
  return m_Cfg->m_Render.show_footprints_virtual;
  ```
  The default is `render.show_footprints_dnp` = `false` (`eda_3d_viewer_settings.cpp`). So in KiCad, a footprint marked "Do not populate" has no model in the 3D view by default. In the studio it always has one. Also, a footprint whose Component type is set to "Unspecified" in Footprint Properties is a virtual model in KiCad (the V key) and still an SMD or TH model here.
- **How to see it.** Mark R1 DNP (Footprint Properties, or the symbol's DNP). Open the 3D viewer: the studio shows R1's model, KiCad does not. Next, set U1's Component type to Unspecified and press V: KiCad hides U1's model, the studio does not.
  Test: `part_json_kind_follows_the_footprints_attributes` in `model3d_api.rs` tests. Give a part a `FootprintEdit { attrs: Some(FootprintAttrs { kind: Unspecified, dnp: true, .. }) }` and assert `kind3d == "virtual"` and `dnp == true` in `part_json`. In the viewer, the model is hidden unless the DNP row is on.
- **Already known?** In part. GAPS 23 lists "models not in the position file and DNP models" as missing. `appearance3d.ts:9` and `Viewer3D.tsx:479` give the reason as "no pick-and-place or DNP attribute" in the data model, which is no longer true. The kind ignoring the Component type is not listed. **Fix direction:** send the effective attributes in `part_json`, use them for `kind3d`, and add the two rows ("Models not in POS File" with default shown, "Models marked DNP" with default hidden, keys P and D).

#### V3. The copper swatches and the editor copper colours are not KiCad's single copper colour (low)
- **Fixed in part** on task-r2-editors-exports: d743df5a (a swatch sets both copper rows). Left: per-layer editor copper colours on inner layers (PARITY-3d, GAPS 23).
- **What differs.** `appearance3d.ts:resolveColors` (`:345-369`) applies a swatch to `copper_top` or to `copper_bottom` on its own, and with `useEditorCopperColors` it sets top from `F_Cu` and bottom from `B_Cu`. Inner-layer tracks then take the colour of `silkSide(layer)` (`scene.ts:961`), which is F or B.
  - KiCad, `appearance_controls_3D.cpp::onColorSwatchChanged`: "The internals of the 3D viewer only supports a single color for copper, which must be applied to all copper layers." A change to any of `LAYER_3D_COPPER_TOP`, `LAYER_3D_COPPER_BOTTOM` or `LAYER_3D_PLATED_BARRELS` sets the other two as well.
  - KiCad, `render_3d_opengl.cpp::setLayerMaterial`: with `GetUseBoardEditorCopperLayerColors()`, each copper layer takes its own 2D colour: `COLOR4D copper_color = m_boardAdapter.m_BoardEditorColors[aLayerID];`. That includes In1.Cu .. In30.Cu. The option is off for the footprint holder (`!m_board->IsFootprintHolder()`).
- **How to see it.** Set the B.Cu swatch to red. The studio has gold top copper and red bottom copper, while KiCad turns both swatches and the barrels red. Then turn on "Use board editor copper colors" on a 4-layer board: KiCad draws In1.Cu in its 2D colour, the studio draws it in F.Cu's.
  Test: `resolveColors(base({ overrides: { copper_bottom: "#ff0000" } }))` gives `copper_top` equal to `copper_bottom`, both #ff0000.
- **Already known?** No. PARITY-3d.md row 87 says the bottom copper is forced to the top's, which holds only until a swatch is set.

#### S1. `${OP}` text variables are never resolved (medium)
- **Fixed** on task-r2-editors-exports: d743df5a (`kicad-port/sim/opVars.ts`, KiCad's regexes; resolved where the sheet paints).
- **What differs.** KiCad resolves `${OP}` in a symbol's fields and in a label's text from the last operating-point run:
  - `sch_symbol.cpp::SCH_SYMBOL::ResolveTextVar` matches `"^OP(:[^.]*)?(.([0-9])?([a-zA-Z]*))?$"` (a pin, a precision, a range);
  - `sch_label.cpp::SCH_LABEL_BASE::ResolveTextVar` matches `"^OP(.([0-9])?([a-zA-Z]*))?$"`;
  - `GetContextualTextVars` offers `OP` in both;
  - `SCH_EDIT_FRAME::RefreshOperatingPointDisplay` starts with "// Update items which may have ${OP} text variables" and clears their render caches.

  A KiCad user can therefore write `${OP}` in a field (a resistor's current), `${OP:1.2V}`, or `${OP}` in a net label (the node voltage), and see the value on the sheet. In the studio the text stays `${OP}`. `crates/model/src/text_vars.rs:21` lists "simulation `OP` values" among the variables left as written, and the web side has no resolver for them either.
- **How to see it.** Put a net label `OUT ${OP}` on the RC board of `e2e/simulator-board.yaml`, run an OP and look at the label. KiCad shows "OUT 0.98946V"-style text (3 digits, auto range); the studio shows `OUT ${OP}`.
  Test: `kicad-port/sim/sim.test.ts`, `a_labels_OP_variable_is_the_nets_operating_point`. With `opPoints = {"/out": 0.98946}`, resolving the label text `${OP}` on net `/OUT` gives `"989mV"` (`SPICE_VALUE::ToString( { 3, "~V" } )`), and `${OP.2}` gives 2 digits.
- **Already known?** Only in `text_vars.rs`'s doc comment. GAPS item 34 and PARITY-sch.md 38's "Not done" row do not list it.

#### S2. The operating point overlay ignores the project's precision and range (low)
- **Fixed in part** on task-r2-editors-exports: d743df5a (`opLabels` takes the four formats). Left: the project does not carry `drawing.operating_point_overlay_*` yet (GAPS 34).
- **What differs.** `kicad-port/sim/opOverlay.ts:86, :111, :118` hard-code `operatingPoint(points, name, 3, "~V")` and `(…, 3, "~A")`. KiCad uses `settings.m_OPO_VPrecision`, `m_OPO_VRange`, `m_OPO_IPrecision` and `m_OPO_IRange` (`sch_edit_frame.cpp::RefreshOperatingPointDisplay`). These are project settings: `schematic_settings.cpp`'s `"drawing.operating_point_overlay_v_precision"` (3), `"drawing.operating_point_overlay_v_range"` ("~V"), and the `_i_` pair ("~A"). Schematic Setup > Formatting edits them (`panel_setup_formatting.cpp:112-163`).
  `grep operating_point_overlay` finds nothing in `crates/` or `web/studio/src`. So a KiCad project that sets "mV" or 5 digits shows 3 digits auto-ranged in the studio. From the reading, the setting is probably also lost from the derived `.kicad_pro` (not run).
- **How to see it.** In a project with `"operating_point_overlay_v_range": "mV"` and an OP run, a 1.2 V net shows "1.20V" in the studio and "1200mV" in KiCad.
  Test: `opLabels` takes the four settings, and with `{ vPrecision: 4, vRange: "mV" }` a net at 1.2 V is labelled "1200mV" (as `SpiceValue.of(1.2).toFormatted({precision: 4, range: "mV"})` gives).
- **Already known?** No.

#### S3. Two smaller overlay differences (low)
- **Fixed** on task-r2-editors-exports: d743df5a.
- **A net with no pin on the sheet gets no voltage.** `opOverlay.ts:69-71` finds a sheet net's SPICE name only through a pin on one of its wires or on the sheet. KiCad walks `m_connectionGraph->GetNetMap()` and asks `GetOperatingPoint( key.Name, ..)` by net name. So a wire that joins only labels (a hierarchical label to a global label, for example) is labelled in KiCad and not here.
- **Pins are grouped by reference, not by symbol.** `opOverlay.ts:95-99` groups every pin of a reference on the sheet. KiCad works per `SCH_SYMBOL` (`symbol->GetPins( &sheetPath )`, then `if( pins.size() == 2 )`). A two-unit part with both units on one sheet, each unit with two pins, is "4 pins" here, so it goes down the model-pin path. In KiCad each unit is a two-pin symbol and takes the item's current on its first pin.
- **Already known?** The "net drawn in pieces gets one label" note is in GAPS 34. These two are not.

#### R1. The 3D resolver's `${ALIAS}` fallback and variable-name scan (low)
- **Fixed** on task-r2-editors-exports: d743df5a (names scanned as `KIwxExpandEnvVars`, `Contains("KISYS3DMOD")`). Left: the legacy 3D search-path alias fallback (no such settings here). Note: with `MY` defined, `${MY LIB}/x` becomes `<MY> LIB}/x`, as KiCad does.
- **What differs.** `model3d_api.rs:80-124` `expand`:
  1. KiCad's `KIwxExpandEnvVars` (`common.cpp`) ends a variable name at the first character that is not `wxIsalnum`, `_` or `:` (`while( wxIsalnum( str_m ) || str_m == wxT( '_' ) || str_m == wxT( ':' ) )`). Ours takes everything up to the closing bracket.
  2. KiCad treats any name that *contains* `KISYS3DMOD` as the 3D model dir (`strVarName.Contains( "KISYS3DMOD" )`). Ours needs the name to be exactly that.
  3. `filename_resolver.cpp::ResolvePath` tries an unresolved `${NAME}/x` against the user's 3D search-path aliases: "Try matching the variable name against user-defined (non-env-var) aliases in m_paths". Ours returns NotFound. This only matters for libraries that still use KiCad 5's 3D alias list.
- **How to see it.** `${MY LIB}/x.wrl` (with a space) is a variable `MY LIB` here. In KiCad the name ends at the space, so `${MY` is left unexpanded.
- **Already known?** No (P14-6 is the footprint library folder, a different step).

### Checked and found to match (no need to review again)
- **Place Design Block's layer growth** (`8a567378`, `c40ebb4c`; `crates/ops/src/design_blocks.rs`) against `pcb_control.cpp::AppendBoard`:
  - the copper count comes from the block's `(layers ..)` table (`cu.size()`), not from a pad's `(layers ..)`, and it only grows (`if( copperLayerCount > initialCopperLayerCount )`);
  - the technical and user layers the file enables are added (`enabledLayers |= initialEnabledLayers`);
  - every net of the file is the board's by name;
  - the model is refolded when the stackup changes.
- **The simulator netlist.** `SPICE_CIRCUIT_MODEL::WriteDirectives` and `NETLIST_EXPORTER_SPICE::WriteDirectives`: `.save all`, `.probe alli`, `.probe p(..)` only for `GetPinCount() >= 2` and not `A`, the nine analyses `isSimCommand` filters, `OPTION_SIM_COMMAND` added when the command is empty, the command printed after the directives, and nothing written when there are no items. `writeInclude` with `OPTION_ADJUST_INCLUDE_PATHS` and "Could not find library file '%s'".
- **kicad-cli's options.** kicad-cli's SPICE job uses `OPTION_SIM_COMMAND` alone (`eeschema_jobs_handler.cpp:503`). `OPTION_ADJUST_PASSIVE_VALS`, which the simulator sets, is never read by the 8303b2ad exporter, so making the netlist from kicad-cli's loses nothing there.
- **`SPICE_CIRCUIT_MODEL::VectorToSignal`**: `#branch`, `:power`, `@dev[i..]` and voltages (`simSignals.ts:282`).
- **The tuner's command**: `alter @{}={:g}` (`SPICE_GENERATOR_IDEAL::TunerCommand`, `SPICE_GENERATOR_SOURCE::TunerCommand` for V/I) and its place before `run`. The potentiometer's `altermod @{}[position]` is not ported (known).
- **`SCHEMATIC::GetOperatingPoint`**: lower case, then `ConvertToSpiceMarkup`, `gnd`/`0` give "", "--" with no run, "?" for an unknown name. The overlay puts a two-pin symbol's current on its first pin and reads the other pins as `<item>:<model pin>`.
- **`SIMULATOR_FRAME::canCloseWindow`**: "Save changes to workbook?", Save / Discard Changes / Cancel, and the default name `<project>.wbk` or `noname.wbk`.
- **`BOARD_ADAPTER::GetLayerColors`** (`appearance3d.ts`):
  - every entry of `g_SilkColors`, `g_MaskColors`, `g_BoardColors` and `g_FinishColors`;
  - the dielectric mix (`COLOR4D::Mix` is `aColor*(1-f) + this*f`, so ours is the same formula) and the alpha step;
  - the finish-name rules, the bottom copper set to the top's before the overrides, and the theme values of `builtin_color_themes.h`.
- **`APPEARANCE_CONTROLS_3D::s_layerSettings`**: the labels, the tooltips and the order of every row ported. Values, Footprint Text, Off-board Silkscreen, 3D Navigator, Models not in POS File and Models marked DNP are known to be missing (GAPS 23).
- **`FILENAME_RESOLVER::ResolvePath`'s order**:
  - the expanded name as it is, then relative to `${KIPRJMOD}`, then relative to `${KICAD*_3DMODEL_DIR}`;
  - an unresolved leading `${VAR}` gives up without the project or library fallback;
  - unmatched `KISYS3DMOD` / `KICAD*_3DMODEL_DIR` go to the current model library;
  - recursive expansion (`ModelNames::get`).
- **The STEP-to-VRML conversion's units.** `exporter_vrml.cpp` writes each `--models-dir` model through `S3D::WriteVRML` as loaded, in VRML's 0.1 inch, so the studio's `VRML_UNIT_MM = 2.54` is right whatever `--units` says.

### Summary

| # | Finding | Severity | Known? |
|---|---|---|---|
| V1 | User.1..45 drawn in 3D by default (`render.show_user{n}` defaults false) | medium | no |
| V2 | Model kind and DNP ignore the board footprint's attributes; DNP models shown (KiCad hides them by default); the GAPS reason ("no DNP attribute") is stale | medium | in part (GAPS 23) |
| S1 | `${OP}` in fields and labels never resolved | medium | only in a doc comment |
| V3 | Copper swatches not synced top/bottom/barrels; editor copper colours not per layer | low | no |
| S2 | OP overlay ignores `drawing.operating_point_overlay_*` (precision, range) | low | no |
| S3 | OP overlay: net with no pin on the sheet unlabelled; pins grouped by reference, not symbol | low | no |
| R1 | 3D resolver: variable-name scan, `Contains("KISYS3DMOD")`, legacy alias fallback | low | no |

Counts: high 0, medium 3, low 4.

---

## Part D. Specctra


Reviewed against KiCad 8303b2ad, `pcbnew/specctra_import_export/specctra_import.cpp` (`ImportSpecctraSession`, `FromSESSION`, `makeTRACK`, `makeARC`, `makeVIA`, `scale`, `mapPt`), `specctra.cpp` (`doSESSION`, `doROUTE`, `doNET_OUT`, `doWIRE`, `doWIRE_VIA`, `doPLACE`, `doRESOLUTION`, `doUNIT`, `ELEM::GetUnits`), `specctra.h` (each `GetUnits` override), `specctra_export.cpp` (`ExportSpecctraFile`, `ExportBoardToSpecctraFile`, the placement's rotation, `FlipFOOTPRINTs`), `pcb_track.cpp` (`PCB_VIA::GetDrillValue`, `SetLayerPair`, `SanitizeLayers`), `footprint.cpp::FOOTPRINT::Flip`, `board.cpp` (`RemoveAll`, `DeleteMARKERs`). The kicad-cli job and the newer import are from 25ec2335 (`pcbnew_jobs_handler.cpp::JobExportSpecctraDsn`, `specctra_import.cpp`).

Our side: `crates/kicad/src/specctra/session.rs`, `parse.rs`, `model.rs`, `crates/ops/src/specctra.rs`, `crates/cli/src/specctra_api.rs`, `crates/kicad-engine/src/lib.rs::export_specctra`, `web/studio/src/kicad-port/specctra.ts`, `web/studio/src/actions/importActions.ts`.

`crates/kicad/src/specctra/export.rs` is the deprecated fallback port of `FromBOARD`. Its only caller is `crates/cli/src/specctra_api.rs:71` (`Engine::Eda`), which runs when the kicad-cli found has no `pcb export specctra` or when `EDA_KICAD_CLI_NO_SPECCTRA` is set. The fast tests make their round trips with it. I did not review it line by line: it is to be deleted, and the slow test `the_dsn_is_kicad_clis_down_to_the_wiring` holds it to kicad-cli.

### Findings

#### D1. A microvia whose drill the session leaves undefined takes the net class's via drill, not its microvia drill (medium)
- **Not a bug** (task-r2-editors-exports: ef1d0a0f): `makeVIA`'s tests never type a padstack of two or more shapes MICROVIA (`topLayerNdx` is the largest index, so `topLayerNdx == 0 && botLayerNdx == 1` cannot hold); KiCad 8303b2ad makes the F.Cu-In1.Cu via BURIED with the class's via drill, as we do. Test `a_two_layer_via_of_a_session_is_buried_with_the_via_drill_as_kicads_make_via_types_it`.
- **What differs.** `crates/kicad/src/specctra/session.rs:184`: `let drill = if drill_diam_iu > 0 { drill_diam_iu } else { drill_of_net };`. `drill_of_net` is `b.via_drill_of(net)` (`session.rs:266`, `:248`) whatever kind of via `make_via` has just worked out. KiCad's `makeVIA` calls `via->SetDrill( drill_diam_iu )` with `UNDEFINED_DRILL_DIAMETER` (-1) in two cases: the padstack name's drill equals the Default class's via drill, or the name has no `:`/`_`. The drill is then `PCB_VIA::GetDrillValue` (`pcb_track.cpp:694`):
  ```
  if( m_padStack.Drill().size.x > 0 ) // Use the specific value.
      return m_padStack.Drill().size.x;
  // Use the default value from the Netclass
  NETCLASS* netclass = GetEffectiveNetClass();
  if( GetViaType() == VIATYPE::MICROVIA )
      return netclass->GetuViaDrill();
  return netclass->GetViaDrill();
  ```
  So a microvia (shapes on layers 0 and 1, or on the last two layers) gets the class's `uvia_drill` (0.1 mm by default). We give it the via drill (0.3 or 0.4 mm), and the board and its drill file then hold a 0.3 mm drill in a microvia. The design has the value we need: `NetClass::microvia_drill` (`crates/model/src/lib.rs:1258`), written as `(uvia_drill ..)` with 0.1 mm as the default (`crates/kicad/src/pcb.rs:592`).
- **How to see it.** Take a 4-layer board whose Default class has a via drill of 0.3 mm. Import a session whose `library_out` holds `(padstack "Via[0-1]_250:300_um" (shape (circle F.Cu 2500)) (shape (circle In1.Cu 2500)) (attach off))` and whose network has `(via "Via[0-1]_250:300_um" 0 0)` on a net of the Default class. KiCad: a MICROVIA F.Cu-In1.Cu with a 0.1 mm drill. Ours: kind `Micro`, drill 0.3 mm. A name with no drill, such as `(padstack uv ...)` from another router, gives the same result.
- **Test.** `crates/kicad/src/specctra/tests.rs::a_microvia_with_an_undefined_drill_takes_the_classs_microvia_drill`. Call `from_session_db` on a 4-layer `BoardView` with that session, `via_drill_default = 300_000`, and a resolver that answers 300 000 for a via and 100 000 for a microvia. Assert `vias[0].kind == ViaKind::Micro` and `vias[0].via.drill == Nm::new(100_000)`. This needs `drill_of_net` to take the via kind, or a second resolver for `microvia_drill`, defaulting to 0.1 mm.
- **Already known?** No. PARITY-pcb.md §46's row "A via's size and type" says "the net class's" without the microvia case.

#### D2. The ids of the new routing come from its content, so an unchanged track or via comes back as "the same item": it stays in its group and tuning pattern and keeps its old via extras (low)
- **Fixed** on task-r2-editors-exports: ef1d0a0f.
- **What differs.** `crates/ops/src/specctra.rs:50-63`. The old unlocked tracks and vias are dropped, the session's are added, and `rt.assign_missing_ids()` gives each new one an id from its content (`next_item_id("trk", id_seed)`, the points; a via's net, position, drill, size and layers, `crates/model/src/ir.rs:1405`). A segment or via the router sends back unchanged therefore gets its old id. After that:
  - `Board::apply`'s `tidy_groups` and `tidy_tuning_patterns` (`crates/ops/src/lib.rs:2721-2723`) see no id gone, so the new track stays a member of the old group and tuning pattern;
  - `dr.via_extras.retain(|e| via_ids.contains(&e.id))` (`specctra.rs:63`) keeps the old via's extra (its tokens: tenting, `remove_unused_layers`, zone layer connections, capping and so on) on the new through via. The `!= Through` branch only rewrites non-through vias.

  In KiCad every track, arc and via of the session is a new object. `FromSESSION` deletes the old ones after `group->RemoveItem( track )` (8303b2ad; 25ec2335 does it through `aCommit.Remove`), and `makeVIA` builds a `new PCB_VIA` with default padstack settings. The new routing is in no group or generator and has none of the old via's settings.
- **How to see it.** Group a track and a footprint, give a via "tented" in Via Properties, export the DSN, and import the echo session (`echo_session`, the session a router that changes nothing would write). KiCad: the group holds only the footprint and the via is not tented. Ours: the group still lists the track and the via is still tented.
- **Test.** `crates/ops/src/specctra.rs` tests: `the_sessions_routing_is_new_items_in_no_group_and_with_no_old_via_settings`. Use a design with a group `{trk_x, R1}` and a via extra with tokens on `via_y`, and apply `Cmd::Specctra` with the echo session. Assert the group's members are `["R1"]` (or that the group is dissolved, as the group rules say) and that no via has the old tokens. A fix is to put the old ids in the "gone" set explicitly, or to give the session's items fresh ids.
- **Already known?** No.

#### D3. The import does not follow KiCad in deleting every DRC marker, the exclusions included (low; probably keep ours, but say so)
- **Documented** (kept on purpose) on task-r2-editors-exports: ef1d0a0f.
- **What differs.** `FromSESSION` calls `aBoard->DeleteMARKERs()` (`board.cpp:1737`), the overload that deletes every marker, the excluded ones included. On the next save `BOARD::RecordDRCExclusions` rewrites the project's exclusions from the markers left, so in KiCad a session import followed by a save loses the DRC exclusions. Ours keeps `drawings.drc_exclusions`. The module doc (`crates/ops/src/specctra.rs:17`) says "The DRC markers `DeleteMARKERs` deletes are kicad-cli's report here, which is never stored", which leaves out the exclusions.
- **How to see it.** Exclude a DRC finding, import a session, save. KiCad's `.kicad_pro` has no `drc_exclusions`; ours keeps it.
- **Test.** None needed if we keep ours. Write the difference into PARITY-pcb.md §46 ("Differs: the board's DRC exclusions are kept; KiCad's `DeleteMARKERs()` drops the excluded markers too, so its next save forgets them").
- **Already known?** No.

#### D4. The import's error is shown without KiCad's first line (low)
- **Fixed** on task-r2-editors-exports: ef1d0a0f ("Fix problem and try again" over the error; the "Board may be corrupted" sentence left out, the board is untouched).
- **What differs.** `web/studio/src/kicad-port/specctra.ts:48-50`: a refused import shows only the `IO_ERROR` text. KiCad's `PCB_EDIT_FRAME::ImportSpecctraSession` shows `DisplayErrorMessage( this, msg, extra )` with `msg = _( "Board may be corrupted, do not save it.\n Fix problem and try again" )` and the error as the extended text. Our board is untouched by a refused import, so the first sentence would be false. The second ("Fix problem and try again") is true and is dropped: it appears only when the reply has no message at all.
- **How to see it.** Import a session that names layer `In9.Cu` on a 2-layer board. Ours: "Session file uses invalid layer id 'In9.Cu'.". KiCad: "Board may be corrupted, do not save it. / Fix problem and try again" over that line.
- **Test.** `specctra.test.ts`: `describeImport({ ok: false, message: "Session file uses invalid layer id 'X'." }).message` contains "Fix problem and try again" and the error. Or write the difference in §46.
- **Already known?** No.

#### D5. Docs out of date (low)
- **Fixed** on task-r2-editors-exports: ef1d0a0f.
- PARITY-pcb.md:1945 (§46, the Units row): "rounded to the design's micrometres". The design is in nanometres now and `scale` gives nanometres (`session.rs` `scale`, `pt`).
- `session.rs`: the doc of `SessionImport` says "micrometres", and so do `make_via`'s and `from_session_db`'s docs for `drill_of_net`. All of these are nanometres now.
- GAPS.md:141 (item 19 row) says Specctra is "done 2026-10-23 on `task-specctra-wip` (not merged)". It is on main.
- Upstream moved: in 25ec2335, `FromSESSION` no longer refuses the whole session on a missing reference, an unknown layer or a missing padstack. It skips the item and warns "%d session item(s) were skipped due to unresolved reference, layer, or padstack.", and it refuses a rect or circle wire with "Unsupported wire shape: '%s' for net: '%s'". It also does the import through a `BOARD_COMMIT` ("Import Specctra Session"), so it is an undo step, as ours is. Our port follows 8303b2ad (refuse everything) and is right for that version. §46 could note that KiCad's next release skips instead of refusing.

### Checked and matches (no need to review again)
- `scale` (the factor per unit, `KiROUND( factor * distance / resValue )`) and `mapPt` (y negated): `session.rs` `scale`, `map_pt`.
- The units in force: a route's `(resolution ..)`, else `UNIT_RES::Default` (inch, 2540000); a placement's own `(unit|resolution ..)`, else the default. This matches `ROUTE::GetUnits`, `PLACEMENT::GetUnits` and `ELEM::GetUnits`, since a session has no units of its own. A `(unit X)` keeps the value 2540000, as `doUNIT` leaves it (`UnitRes::unit`). `doRESOLUTION` lower-cases the unit word and `doUNIT` takes keyword tokens: both ported.
- The placement: an unknown reference is refused ("Reference '%s' not found."), and a place with no vertex is skipped. The orientation is `rotation` on the front and `rotation + 180` on the back. Through the writer's frame (`pcb.rs:1067-1094`: the file angle is the negated design angle, and a back footprint in the library frame is turned half a turn) and `import_rot_millideg` (negation), I worked out that a no-op round trip gives back the design's rotation and side. KiCad's own round trip is the identity too: `FOOTPRINT::Flip` TOP_BOTTOM negates the orientation, the export writes `180 - O`, and the import does `+180`. The side change flips the fields (`flip_footprint_fields`). Locks on footprints are not looked at, as in KiCad. A second place of one reference wins.
- What is deleted: every track, arc and via not locked, netless ones included, which is `RemoveAll( { PCB_TRACE_T } )` (`board.cpp:1490`, which takes arcs and vias with it). Locked ones stay (`locked_ids`).
- `makeTRACK` (one track per pair of path points; width from `aperture_width` at the route resolution; layer by the board's user names, `buildLayerMaps`; `(type fix)` locked, `protect` not) and `makeARC` (`CalcArcMid` with `aMinArcAngle = true`, `trigo.cpp:209`, ported as written). Polygon wires and rect/circle wires are ignored, as in 8303b2ad. `polyline_path` is read as `path`. The net is the `net_out`'s (FindNet); the wire's own `(net ..)` is ignored, as in KiCad.
- `makeVIA`: the drill parsed from the padstack name (after `:`, before the last `_`, `fast_float` with leading white space, truncated with `static_cast<int>`, undefined when equal to the Default class's via drill); "Session via padstack has no shapes"; one shape or one per copper layer is a through via F.Cu-B.Cu; otherwise micro/buried/blind by the same index tests, with the first circle's diameter and the layer pair sorted as `SanitizeLayers` sorts it. Every message is word for word, the trailing full stop only in the one-shape case included. The via's net is the `net_out`'s. `(type fix)` is locked.
- `FromSESSION`'s section checks and messages ("Session file is missing the \"routes\" section" and the same for `"library_out"`), and "A wire_via refers to missing padstack '%s'.".
- The parser: `doSESSION` (a session id made of several tokens, `base_design`, `history`, `structure`, `placement`, `was_is`, `routes`, the duplicate refusals), `doROUTE` (a second `(parser ..)` replaces the first, as KiCad's Electra workaround does; `network_out` takes `net` or `Net`; `test_points` refused), `doNET_OUT`, `doPLACE` (it keeps KiCad's missing `NeedRIGHT` after `mirror`, `status`, `logical_part` and `lock_type`, so the same files parse the same way), `doWIRE_VIA`.
- The tests: `a_session_is_read_in_its_own_resolution`, `a_qarc_is_an_arc_through_its_middle`, `a_bad_session_is_refused_in_kicads_words` and `a_via_drill_equal_to_the_default_is_the_nets_own` assert what their names and docs claim. `a_session_brings_back_the_routing_and_the_placement_the_dsn_had` makes its session from the fallback exporter's DSN; the kicad-cli path is covered by the slow `the_imported_session_makes_no_new_drc_violation` in `crates/cli/tests/specctra_session.rs`.
- The undo: one step. 8303b2ad clears the undo list instead; 25ec2335 pushes a commit "Import Specctra Session", which is what ours does.
- The DSN export through kicad-cli (`kicad-engine/src/lib.rs::export_specctra`): run in the output folder with `-o <board>.dsn`. I checked on `qa/data/pcbnew/issue5093.kicad_pcb` that kicad-cli writes `(pcb x.dsn` into the folder it runs in and says "Successfully created Specctra DSN file", that its via padstacks are named `Via[0-1]_600:300_um` (the name `makeVIA` reads the drill from), and that `SPECCTRA_OK` filters exactly that line. The studio's "BOARD exported OK." and the "Unable to export, please fix and try again" prefix are `ExportSpecctraFile`'s. The locked tracks, vias and footprints of the derived board are written `(locked yes)` (`pcb.rs:246-397`), so they reach the DSN as `(type fix)`.

### Summary

| # | Finding | Severity | Known? |
|---|---|---|---|
| D1 | A microvia whose drill the session leaves undefined takes the class's via drill, not its uvia drill (`PCB_VIA::GetDrillValue`) | medium | no |
| D2 | Content-derived ids: unchanged session tracks and vias stay in their old group and tuning pattern and keep the old via's extras | low | no |
| D3 | DRC exclusions kept where KiCad's `DeleteMARKERs()` drops them; undocumented | low | no |
| D4 | A refused import shows the error without "Fix problem and try again" | low | no |
| D5 | Docs: micrometres in §46 and in session.rs; GAPS 19 says "not merged"; 25ec2335's skip-and-warn not noted | low | no |

---

## Summary, Part A (exports)

| # | Finding | Severity | Known? |
|---|---|---|---|
| X1 | Drill: oval holes are G85 (kicad-cli's `alternate`); KiCad's dialog routes them (`SetRouteModeForOvalHoles( !m_altDrillMode )`) | high | no |
| X2 | Drill: PTH and NPTH merged by default; KiCad's "PTH and NPTH in single file" starts unchecked | medium | no |
| X3 | Gerbers: no `--board-plot-params`, so Protel names; the board's `pcbplotparams` are dropped on import and write | high | no |
| X4 | 3D export of an open outline: KiCad refuses ("Board outline is missing or not closed using %.3f mm tolerance."); ours writes it and drops the warning | high | no |
| X5 | VRML: KiCad's "User defined origin" (0, 0) is checked by default; ours uses the board centre | medium | no (PARITY-boardctl says identical) |
| X6 | "Board center" = our largest ring, not `ComputeBoundingBox( true, true )` over every Edge.Cuts shape | medium | no |
| X7 | Board BOM is the schematic's, not `GenBOMFileFromBoard`'s (lib prefix, quoting, `;;;`, board-only footprints, empty-board message) | medium | in part |
| X8 | Placement dialog: no negate-X / Gerber X3 / Exclude-BOM / variant; extra Side radio; no "No footprint for automated placement." | medium | no |
| X9 | Schematic Plot defaults: PDF and no background in KiCad; SVG and a background in ours | medium | no (PARITY-sch says identical) |
| X10 | PCB Plot dialog: Gerber only, three controls; untracked in GAPS | medium | no |
| X11 | Export Symbol as SVG: "no symbol to export" vs "No symbol to export" | low | no |
