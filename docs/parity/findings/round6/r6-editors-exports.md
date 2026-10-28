# Round 6 findings: editors, 3D, design blocks, simulator, fab outputs and exports (r6-editors-exports)

**Reviewer:** r6-editors-exports. A findings-only review on 2026-10-28. No code was changed.

**Versions.**
- Our code: main's checkout at `7d772e6e`, read in `/home/team_braneboard_com/work/agentic-eda`.
- KiCad: `kicad-src-8303b2ad` first; `kicad-src-25ec2335` for `pcbnew/pcbnew_jobs_handler.cpp`, `pcbnew/api/*`, the drill writers, `dialog_export_2581_bom.cpp` and the drill charts (`pcbnew/pcb_drill_chart.cpp`, `pcb_generated_table.cpp`, `board_tables/generated_table_refresh.cpp`).

**What was run** (scratch dir `/tmp/findings-r6-editors-exports.ShOC/`):
- `kicad-cli pcb upgrade` twice in a row (no `--force`) on a copy of 25ec2335's QA `pcbnew/api_kitchen_sink.kicad_pcb` (one drill chart, 480 KB): both runs said "Successfully saved board file using the latest format", 0.41 s and 0.44 s (`dc/`).
- No `eda` build, no studio server; ports 8848-8849 were not needed.

**Scope.**
- (a) The round-5 merge `42697b29` (`a6095d11`): X5-1 served exports' messages, X5-2 IPC-2581 BOM columns, X5-3 one-shot CLI per-run, X5-4 Gerber placement names, V5-1 preview thickness, V5-2 preview zoom modifier, V5-3 dielectric sublayers, E5-1 / E5-2 the Footprint Editor's Text dialog.
- (b) The held reuse item D5-1: `crates/ops/src/drill_chart.rs` against kicad-cli.
- (c) Rows no round reviewed: the simulator's dialogs, the design-block library dialogs, the 3D viewer's options.

**Finding IDs:** X6-n exports; V6-n 3D; D6-n reuse; S6-n simulator; B6-n design blocks.

---

## Part A. The round-5 merges

### V6-1. The 3D model preview's wheel: the V5-2 fix reads KiCad backwards. With a zoom modifier set, KiCad's preview still zooms on a plain wheel and pans on Ctrl/Shift+wheel; ours now pans on a plain wheel (low; the r5 fix is wrong)

**Fixed** on task-r6-editors-exports: 05e21200 (the preview's `handleWheel` takes `aPan` apart; model3d.test.ts's round-5 test replaced).

- **What differs.**
  - Ours: `web/studio/src/kicad-port/model3d.ts:215-217` `previewWheelScroll` returns `{ ...DEFAULT_WHEEL_SCROLL, scrollModifierZoom: prefs.scrollModifierZoom }`, and `camera3d.ts:441` derives both the pan mode and the zoom-modifier comparison from that one value:
    ```ts
    const aPan = scroll.scrollModifierZoom !== "none";
    ...
    if (aPan && modifier !== scroll.scrollModifierZoom) { /* pan */ }
    ```
    So with Ctrl as the zoom modifier, a plain wheel pans and Ctrl+wheel zooms. `model3d.test.ts:268-285` pins that ("a plain wheel must not zoom while a zoom modifier is set").
  - KiCad (`3d-viewer/dialogs/panel_preview_3d_model.cpp::PANEL_PREVIEW_3D_MODEL::loadSettings`, the same at 25ec2335:344) sets only the pan flag from the preference:
    ```cpp
    // TODO(JE) use all control options
    m_boardAdapter.m_MousewheelPanning = settings->m_Input.scroll_modifier_zoom != 0;
    ```
    The preview's canvas never gets `SetVcSettings` (only `eda_3d_viewer_frame.cpp:960,1034` call it, for the main viewer), so its `m_settings` is `VC_SETTINGS()` = `Reset()` (`common/view/view_controls.cpp`): `m_scrollModifierZoom = 0`, `m_scrollModifierPanH = WXK_CONTROL`, `m_scrollModifierPanV = WXK_SHIFT`. `common/gal/hidpi_gl_3D_canvas.cpp::HIDPI_GL_3D_CANVAS::OnMouseWheelCamera` then compares the pressed modifier with that 0:
    ```cpp
    if( aPan && modifiers != m_settings.m_scrollModifierZoom )   // 0 in the preview
    {
        if( event.GetWheelAxis() == wxMOUSE_WHEEL_HORIZONTAL || modifiers == m_settings.m_scrollModifierPanH )
            m_camera.Pan( SFVEC3F( -delta_move, 0.0f, 0.0f ) );
        else
            m_camera.Pan( SFVEC3F( 0.0f, -delta_move, 0.0f ) );
    }
    else if( modifiers == m_settings.m_scrollModifierPanV && !aPan ) ...
    else if( modifiers == m_settings.m_scrollModifierPanH && !aPan ) ...
    else
        mouseActivity = m_camera.Zoom( ( event.GetWheelRotation() * zoomSign ) > 0 ? 1.1f : 1 / 1.1f );
    ```
  - So in KiCad's preview, with any zoom modifier set (`aPan` true): a plain wheel has `modifiers == 0 == m_scrollModifierZoom`, skips the pan branch, fails both `!aPan` branches and **zooms**; Ctrl+wheel pans horizontally, Shift+wheel and Alt+wheel pan vertically. With no zoom modifier the preview behaves as the main viewer's default. Round 5's V5-2 read only the first line and missed that the canvas's own `m_scrollModifierZoom` stays 0.
- **How to see it.** Preferences > Mouse and Touchpad, zoom modifier Ctrl; scroll in the Footprint Properties 3D Models tab's preview. KiCad zooms; ours pans.
  - Test (replace `model3d.test.ts`'s V5-2 test): `"the preview pans only with a modifier when a zoom modifier is set (OnMouseWheelCamera with Reset's m_scrollModifierZoom = 0)"`. With `scrollModifierZoom: "ctrl"`: a plain `{deltaY:-100}` changes `cam.zoom` and leaves the look-at point; `{deltaY:-100, ctrlKey:true}` moves the look-at point along x and keeps the zoom; `{shiftKey:true}` moves it along y.
  - Fix direction: `handleWheel` takes `aPan` separately from the settings (as `OnMouseWheelCamera( event, aPan )` does), and the preview passes `aPan = prefs.scrollModifierZoom !== "none"` with `DEFAULT_WHEEL_SCROLL` unchanged (`scrollModifierZoom: "none"`). The main viewer keeps passing the preference in both places (`eda_3d_viewer_frame.cpp:1022` and `SetVcSettings`), which is right.
- **Severity.** Low (navigation in the preview; the r5 fix made it differ in the opposite direction).
- **Already known?** No. It is the r5 V5-2 fix.

### X6-1. Generate IPC-D-356 still ends with "Done." and kicad-cli's line; KiCad's dialog says "IPC-D-356 netlist file created:\n'%s'." (low; the r5 X5-1 fix did the log only)

**Fixed** on task-r6-editors-exports: 08558335 (`board_output_api::ipcd356_message`, test `ipcd356_says_what_gend356file_says`).

- **What differs.**
  - Ours: `web/studio/src/components/BoardExportDialog.tsx:413` shows `reply.message ?? (reply.ok ? "Done." : "Failed.")` in bold, and under it the log, which since X5-1 is "Successfully created IPC-D-356 file" on both paths (`crates/kicad-engine/src/api/jobs.rs:226`, `success_log`). `crates/cli/src/board_output_api.rs:313-315` sets no message.
  - KiCad (`pcbnew/exporters/export_d356.cpp::BOARD_EDITOR_CONTROL::GenD356File`, 8303b2ad:461; 25ec2335:553):
    ```cpp
    DisplayInfoMessage( m_frame, wxString::Format( _( "IPC-D-356 netlist file created:\n'%s'." ),
                                                   fn.GetFullPath() ) );
    ```
    The kicad-cli line is `JobExportIpcD356`'s, which the GUI never shows. None of the other export commands of this dialog (VRML, GenCAD, IDF, IPC-2581, ODB++) shows a message on success in KiCad, so this is the only one.
  - r5 X5-1 named this wording under "Related wording"; the Fixed note covers only the log.
- **How to see it.** File > Fabrication Outputs > IPC-D-356 Netlist File..., Export. Ours: "Done." then "Successfully created IPC-D-356 file". KiCad: an info box "IPC-D-356 netlist file created:" and the quoted path.
  - Test: `crates/cli` `board_output_api::tests::ipcd356_says_what_gend356file_says`: with a fake kicad-cli, `ipcd356(dir)["message"] == "IPC-D-356 netlist file created:\n'<dir>/export/kicad/ipcd356/<name>.d356'."`.
- **Severity.** Low (message).
- **Already known?** In part (r5 X5-1's "Related wording", marked Fixed).

### X6-2. A served drill job with backdrills lists the backdrill pair file before its drill file (low, edge case)

**Fixed** on task-r6-editors-exports: 892d03f3 (`success_log` puts a `-backdrill.drl` pair file after the file that follows it).

- **What differs.**
  - Ours: `success_log` (`crates/kicad-engine/src/api/jobs.rs:219-245`) writes one "Created file" line per `RunJobResponse::output_path`, in the order of the paths.
  - KiCad 25ec2335 `pcbnew/exporters/gendrill_excellon_writer.cpp::EXCELLON_WRITER::CreateDrillandMapFilesSet` reports the drill file when it opens it (`msg.Printf( _( "Created file '%s'" ), fullFilename )`), then writes it, then `writeBackdrillLayerPairFile` reports and `AddCreatedFile`s the pair file (`:693`, `:715`), and only after that `AddCreatedFile( fullFilename )` for the drill file (`:170`). `JobExportDrill` turns `GetCreatedFiles()` into the outputs, so for a backdrill span the outputs are pair file, drill file, while the per-run log is drill file, pair file.
- **How to see it.** A board with a backdrilled via (KiCad 10 padstack secondary drill): the served log's two lines come in the other order from per-run kicad-cli's. `a_served_export_reports_what_kicad_says` (issue8407) has no backdrill.
- **Severity.** Low. The IR has no backdrills yet (`drill_chart.rs` header: "backdrills ... not ported"), so a studio board cannot reach it today; an imported board's backdrill is dropped. Recorded so the rebuilt log is not called identical when backdrills come.
- **Already known?** No.

### Checked and found to match (round-5 merges)
- **X5-1** `success_log`: every string against 25ec2335 -- Excellon drill files `Created file '%s'` (no full stop, `gendrill_excellon_writer.cpp:126`), maps `Created file '%s'.` (`gendrill_writer_base.cpp:345`), Gerber drill files and maps with the full stop (`gendrill_gerber_writer.cpp:96,142`), `ReportTail( _( "Done." ) )` for both writers, `Successfully created IPC-D-356 file` (`pcbnew_jobs_handler.cpp:3182`), `Wrote front/back position data to '%s'.` and `Wrote position data to '%s'.` with the two-file and Gerber cases as `JobExportPos` (`:2535-2634`). The outputs are `GetCreatedFiles()` / `AddOutput` in the order written (apart from X6-2), and `ExecuteBoardJob` fills `output_path` from `aJob.GetOutputs()` (`api_handler_pcb.cpp:2285`). `CLI_REPORTER::Report` prints every severity but debug, so the per-run log has the same lines.
- **X5-2** `ipc2581DialogStart` against `DIALOG_EXPORT_2581_BOM::TransferDataToWindow` (25ec2335): the Internal ID by `SetStringSelection` else item 0; Manufacturer only when the MPN column is found (else item 0 and disabled); Distributor the stored text when the Distributor P/N column is found, else "N/A" and disabled; the revision `bomRev.IsEmpty() ? schRevision : bomRev` (`dialog_export_2581.cpp:250`), and `saveToProject` storing the shown revision, as `ipc2581Chosen` does.
- **X5-3** `exports_use_server` / `long_lived_process`: only `studio::serve` calls it; `run_export` and `export_pcb_placement` both ask it; `EDA_KICAD_API=0` still closes the lane. (Cosmetic: `EDA_KICAD_API=` set to an empty string counts as asking for the lane.)
- **X5-4** `export_pcb` sends every `pos` to `export_pcb_placement`; `place_file_names` for Gerber gives `-pnp_top.gbr` / `-pnp_bottom.gbr` and both sides as two runs with `--side front|back` and the exact `-o` (kicad-cli's `m_nakedFilename` keeps it), which is `JobExportPos`'s naming.
- **V5-1** `previewStack = stackOfBoard({copper_layers: 2, ..})`: 1.6 - 2 x 0.035 - 2 x 0.01 = 1.51 mm, as `BuildDefaultStackupList( &dummy_bds, 2 )` with `FrontMask()` holding F.Cu.
- **V5-3** `import_stackup` sums the layer's own thickness and each `addsublayer`'s, as `BOARD_ADAPTER::InitSettings` sums `GetThickness( sublayer )`; this is also what `BOARD_STACKUP` layer distances sum. Side effect, not a new finding: the writer (`crates/kicad/src/pcb.rs:634-680`) now writes the sum as the one dielectric's thickness, with the first sublayer's epsilon and loss, where it wrote the first sublayer's thickness. The total thickness KiCad reads back is now right; the sublayers themselves are still lost (r2 F17, open).
- **E5-1** `fpTextSizeMessage` against `UNIT_BINDER::Validate` (`common/widgets/unit_binder.cpp:344-388`): `"%s must be at least %s."` / `"%s must be less than %s."`, `valueDescriptionFromLabel` strips the colon, `StringFromValue` with `%.10f` and `removeTrailingZeros` (`common/eda_units.cpp:32,323`) gives "0.001 mm" and "250 mm"; `<` min and `>` max as ours. The studio's dialog is millimetres only, so `m_units` cannot differ.
- **E5-2** `addText` answers with the new text's id read back from `fetchFootprint`; the dialog's edited-offset path and the canvas's second click both select it, as `PlaceText`'s `RunAction( ACTIONS::selectItem, text )`.

---

## Part B. The held reuse item: `crates/ops/src/drill_chart.rs` against kicad-cli

### D6-1. kicad-cli already rebuilds every drill chart on every run, so the port's cells only ever reach the studio's canvas and the saved file; `kicad-cli pcb upgrade` could replace it (low, reuse)

**Fixed** on task-r6-board-io: d72c0ae5 (the user's decision, 2026-10-28: keep the Rust refresh, port what it lacked): a via's backdrill spans and "Backdrill" rows with their stub lengths, post-machining in the key, the board's layer names in FROM / TO, the backdrill filter; imperial drills keyed from nanometres (`d304800`, printed 0.305 as KiCad prints it); `crates/ops/tests/drill_charts_kicad_cli.rs` holds a 10-column chart of all of them (with and without a stackup, and filtered) cell for cell to `kicad-cli pcb upgrade`'s.

- **What the port does.** `crates/ops/src/drill_chart.rs` (869 lines) ports `EnumerateDrillOperations`, `GroupKeyString`, `DRILL_CHART_MODEL::Build`, `AssignDrillSymbols`, `PCB_DRILL_CHART::generate`, `PCB_GENERATED_TABLE::RebuildCells`, `migrateRows` and `RefreshGeneratedTables`; `Board::refresh_drill_charts` runs it after every command, in that command's undo step (`PARITY-pcb.md:2049`, "identical to kicad-cli's `pcb upgrade`"; proof `crates/ops/tests/drill_charts_kicad_cli.rs`, which compares with `pcb upgrade --force`).
- **The kicad-cli command that does the same.** `kicad-cli pcb upgrade <board>` (`PCBNEW_JOBS_HANDLER::JobUpgrade`, 25ec2335 `pcbnew_jobs_handler.cpp:3300-3330`):
  ```cpp
  // A chart is derived data, so the upgrade brings every one up to date and saves if
  // that changed anything
  if( RefreshGeneratedTables( *brd ) > 0 )
      shouldSave = true;
  ```
  `--force` is not needed: a chart read from a file is always stale in KiCad, since `PCB_DRILL_CHART::IsStale` is `m_builtGeneration != aBoard.GetDrillModelGeneration()` (`pcb_drill_chart.cpp:124`), a loaded chart's `m_builtGeneration` is 0 (`:58`) and a board's generation starts at 1 (`board.cpp:127`) and is bumped by every hole added. Measured: two `pcb upgrade` runs in a row on `api_kitchen_sink` both saved, 0.41 s and 0.44 s.
- **Already done by kicad-cli wherever a chart is output.** `PCB_PLOTTER::Plot` (`pcb_plotter.cpp:96`, "The one path GUI plotting, PNG, PS and every CLI plot job share") and `JobExportGerbers` / the single-file Gerber job (`pcbnew_jobs_handler.cpp:1823`, `:2288`) call `RefreshGeneratedTables` first. With the always-stale rule above, every chart in every Gerber, PDF, SVG, PNG and DXF kicad-cli writes for us is KiCad's own rebuild, whatever cells the port wrote. So the port's output reaches only (1) the studio's canvas and Properties panel and (2) the cell texts in the `.kicad_pcb` we save (which KiCad itself rebuilds on its first `OnModify`, `pcb_edit_frame.cpp:2381`).
- **What replacing it with `pcb upgrade` would lose.**
  - Speed and the per-command undo step: the port refreshes in-process inside each command; kicad-cli would be one write of the whole board, a 0.4 s+ run and a re-import per command that touches a hole (vias, pads, footprints, drill maps), or a debounce that leaves the canvas showing a stale chart for that long and makes the refresh its own step.
  - The import round trip: the rebuilt cells come back through `import_kicad_pcb`, whose table geometry is micrometres ("a table cell's corners within 2 um", PARITY section 50); only the chart's table, `drill_chart` and the profile should be taken from the result, not the whole design.
  - The digest staleness (`DrillChart::built_generation`): a hand-edited cell would be overwritten by every kicad-cli run (every load is stale), as KiCad's own first `OnModify` after a load does. Today ours keeps it until the holes change; KiCad keeps it only within one session.
  - Work without kicad-cli: none in practice, the studio's exports and pictures already need it.
- **What it would gain.** The port's documented differences go away (PARITY-pcb section 50 "Charts differ"): backdrill rows, post-machining keys (`m0:0:0:0/0:0:0:0`), user layer names, imperial holes keyed at micrometres (0.3048 -> 0.305).
- **A cheaper middle.** Keep the port for the live canvas (display code), and treat the cells it writes as a preview: nothing KiCad outputs depends on them. Or rebuild through kicad-cli only on Save / export (`pcb upgrade` on the derived board before it is copied out), which costs one run where one is already made.
- **Not verified:** whether the kept api-server can do it without a save: a plot job (`RunBoardJobExportGerbers`) refreshes the charts of the server's `m_cliBoard`, but I found no handler that returns that board's items, so I do not count it.
- **Severity.** Low (reuse; no wrong output found).
- **Already known?** Yes as r5 D5-1 (held); this answers which command and what would be lost.

---
## Part C. The simulator's dialogs (no round reviewed them)

Read against 8303b2ad's `eeschema/dialogs/dialog_sim_command.cpp`, `dialog_sim_command_base.cpp`, `dialog_user_defined_signals.cpp`, `dialog_sim_format_value.cpp`, `eeschema/sim/*`. The Simulation Model dialog (`dialog_sim_model.cpp`) is not ported and PARITY-sch section 38 has no row for it: nothing in the studio writes `Sim.Device` / `Sim.Pins` (only Tune's Save writes `Sim.Params`, reviewed in 38.1). Not a finding, but it is not in GAPS 34's "Left" list either.

### S6-1. "Add full path for .include library directives" starts from the tab's option bit, not the project setting, and OK writes it back into the project (medium)

**Fixed** on task-r6-editors-exports: fd36a62b (`commandDialogOptions`).
- **Ours:** `web/studio/src/components/simulator/SimDialogs.tsx:73` `fixIncludePaths: !!(d.options & OPTION_ADJUST_INCLUDE_PATHS)`; `d.options` is `OPTION_DEFAULT_FLAGS` for a new tab and `tab.options` when editing (`state/simulator.ts:292,301`).
- **KiCad:** `DIALOG_SIM_COMMAND::TransferDataToWindow` (`dialog_sim_command.cpp:153`) runs after `SetSimOptions` and overrides the box:
  ```cpp
  m_fixIncludePaths->SetValue( m_settings->GetFixIncludePaths() );
  ```
  `ApplySettings` / `TransferDataFromWindow` then store it (`:496` `m_settings->SetFixIncludePaths( m_fixIncludePaths->GetValue() )`) and clear the tab's bit when unchecked (`:506` `if( !m_fixIncludePaths->GetValue() ) options &= ~OPTION_ADJUST_INCLUDE_PATHS;`). `NETLIST_EXPORTER_SPICE::writeInclude` acts on that bit (ours `crates/sim/src/netlist.rs:153`).
- **How to see it:** a project with `schematic.ngspice.fix_include_paths: false`; New Analysis Tab, OK. KiCad: box unchecked, tab without the bit, relative `.include`s. Ours: box checked, absolute paths in the netlist, and the project setting flipped to true.
- **Test:** `analysis dialog's include-path box is the project's setting`: open with `settings.fix_include_paths=false`, `options=OPTION_DEFAULT_FLAGS`; the box is unchecked and OK's options lack `OPTION_ADJUST_INCLUDE_PATHS`. Reverse: an edited tab without the bit and the setting true shows it checked.
- **Severity:** medium (a wrong default that changes the netlist and the project setting). **Known?** No.

### S6-2. Plot Setup: Min/Max are empty for an unlocked axis, shown as JS numbers for a locked one, and read back as SPICE values, not SI (medium)

**Fixed** on task-r6-editors-exports: fd36a62b (`plotSetupOf` / `applyPlotSetup`, `simValueToDouble` / `simValueNormalize` ports of `SIM_VALUE::ToDouble` / `Normalize`).
- **Ours:** `SimDialogs.tsx:82-91` `scaleText` gives values only when `s.locked`, shown by `toString()` ("0.0015"); OK (`:118-123`) parses with `SpiceValue.parse`, so "" and "abc" give 0 and "2M" gives 2e-3 (SPICE "m"/"M" = milli).
- **KiCad:** `DIALOG_SIM_COMMAND::SetPlotSettings` (`:202-210`):
  ```cpp
  double min = 0.0, max = 0.0;
  bool   locked = plotTab->GetY1Scale( &min, &max );
  m_lockY1->SetValue( locked );
  if( !std::isnan( min ) )
      m_y1Min->SetValue( SIM_VALUE::Normalize( min ) );
  ```
  `GetY1Scale` -> `mpScaleY::GetAxisMinMax` gives the tick range when unlocked (`else if( !m_tickValues.empty() ) { *minV = m_tickValues.front(); *maxV = m_tickValues.back(); }`), `Normalize` writes SI ("1.5m", "2M"); `ApplySettings` (`:532`) reads with `SIM_VALUE::ToDouble( m_y1Min->GetValue().ToStdString() )` (SI: M = 1e6; NaN for unreadable text).
- **How to see it:** after a TRAN run with voltage ticks 0..12, Edit Analysis Tab > Plot Setup, tick "Fixed Voltage scale" without typing: KiCad shows 0 and 12 and locks there; ours shows empty fields and locks to 0..0.
- **Test:** `plot setup prefills the tick range and reads SI values`: unlocked y1 with ticks [0..12] shows "0"/"12"; locked 0.0015 shows "1.5m"; Max "2M" applies 2e6.
- **Severity:** medium (wrong default and wrong value applied). **Known?** No.

### S6-3. User-defined Signals: Delete removes the last row, not the cursor's row (medium)

**Fixed** on task-r6-editors-exports: fd36a62b (`deleteGridRows`, `WX_GRID::OnDeleteRows`); browser check 776aef9f.
- **Ours:** `SimDialogs.tsx:455` `setRows((rs) => rs.slice(0, -1))`; rows cannot be selected; the read-only id column is hidden.
- **KiCad:** `DIALOG_USER_DEFINED_SIGNALS::onDeleteSignal`: `m_grid->OnDeleteRows( [&]( int row ){ m_grid->DeleteRows( row, 1 ); } )`; `WX_GRID::OnDeleteRows` deletes the selected rows, else `GetGridCursorRow()`.
- **How to see it:** user0 "V(a)*2", user1 "V(b)", user2 "V(c)"; cursor on row 1, Delete, OK. KiCad keeps ids {0, 2} and removes user1's trace; ours keeps {0, 1} and removes user2's.
- **Test:** `user signals Delete removes the cursor row`: the remaining ids are [0, 2].
- **Severity:** medium (the wrong signal deleted). **Known?** No.

### S6-4. DC sweep: a parsed source that is not one of the choice's entries is written as an empty name (medium, edge case)

**Fixed** on task-r6-editors-exports: fd36a62b (`selectDcSources`). Correction to case (1): KiCad does not write `.dc TEMP` for `.dc temp` -- `updateDCSources( src[0] )` compares the letter case-sensitively, so 't' lists nothing and OK says "A DC source must be specified."; `TEMP` (any text starting with 'T') selects TEMP. Ours now does the same; never an empty name.
- **Ours:** `web/studio/src/kicad-port/sim/simCommand.ts:314-318` keeps the parsed text (`f.dcSource1 = p.src1.source`); `dcControls` (`:378-386`) tests only `""` and `"TEMP"`, then `itemName(source)` returns "" for an unknown reference.
- **KiCad:** `parseCommand` does `m_dcSource1->SetStringSelection( src1.m_source )` on the list of `item.refName` (case-insensitive `FindString`; unknown text leaves no selection); `evaluateDCControls` reads `aDcSource->GetString( GetSelection() )`, so it either refuses with "A DC source must be specified." or writes the list's own text ("TEMP").
- **How to see it:** (1) `.dc temp -40 85 5` edited and OK'd: KiCad writes `.dc TEMP -40 85 5`, ours `.dc  -40 85 5`. (2) a source `BAT1` (item `VBAT1`) and a tab `.dc VBAT1 0 5 0.1` re-OK'd: KiCad says "A DC source must be specified.", ours writes `.dc  0 5 0.1`.
- **Test:** `buildCommand(parseCommand(".dc temp -40 85 5"), names) === ".dc TEMP -40 85 5"`; with items `[{ref:"BAT1", name:"VBAT1", device:"V"}]`, `.dc VBAT1 0 5 0.1` gives `{error: "A DC source must be specified."}`.
- **Severity:** medium (an invalid command reaches ngspice). **Known?** No.

### S6-5. The signal filter treats every text as a JavaScript regex; KiCad's `/regex/` form does not work (low)

**Fixed** on task-r6-editors-exports: fd36a62b (`signalMatches`: `EDA_PATTERN_MATCH_REGEX::SetPattern`'s explicit forms only).
- **Ours:** `kicad-port/sim/simSignals.ts:82-97` tries `new RegExp(f)` first.
- **KiCad:** `EDA_PATTERN_MATCH_REGEX::SetPattern` takes a regex only for `^...$` or `/.../` ("For now regular expressions must be explicit"); otherwise the wildcard and substring matchers.
- **How to see it:** filter `V(OUT)` with nets OUT and VOUT: KiCad lists V(OUT) only, ours also V(VOUT) and I(VOUT) (the regex is "VOUT"); filter `/^I/`: KiCad lists the currents, ours nothing. The FFT page's filter shares it.
- **Test:** `signalMatches("V(OUT)", "V(VOUT)") === false`; `signalMatches("/^I/", "I(R1)") === true`; `signalMatches("R1.", "I(R10)") === false`.
- **Severity:** low. **Known?** No.

### S6-6. Noise and pole-zero node choices show the first net while their value is empty (low)

**Fixed** on task-r6-editors-exports: fd36a62b (blank choices; a text not in the list is no selection; the points label fixed too).
- **Ours:** the `choice()` selects at `SimDialogs.tsx:157-168` (Measured node, Noise source, PZ Input/Ref/Output/Ref) have no blank entry and start at "": the browser shows the first net, OK writes `.noise v() ...` or `.pz    vol pz`.
- **KiCad:** the `wxChoice`s start with no selection, shown blank.
- **Severity:** low. **Known?** No.

### Cosmetic, not counted
- The points label switches to "Number of points per octave:" / "Number of points:" with the scale (`SimDialogs.tsx:170`); KiCad's is the fixed "Number of points per decade:" (`dialog_sim_command_base.cpp:60,415,485`).
- The format dialog's title is the caller's ("Format Value", or the cursor column label, `SimSidePanel.tsx:198,266`), and it adds a "3~V" hint; `DIALOG_SIM_FORMAT_VALUE` titles itself "Format Voltage" / "Current" / "Time" / "Frequency" / "Gain" / "Phase" / "Value".
- Point counts are `Number(v)` (`simCommand.ts:395`): "1e2", "10.0", " 10" pass and are written as typed; `wxIntegerValidator` refuses them.

### Checked and found to match (simulator)
- `TransferDataFromWindow` on every page: the command texts, TRAN's max-step / uic / initial rule, `.sp`'s "1", `evaluateDCControls`' order with GetItemName and TEMP, "Source 1 and Source 2 must be different.", NOISE (no validation, the ref through GetItemName, the source as its reference, "A frequency range must be specified."), PZ, FFT over the visible rows, Trim only when the command changed, `SPICE_VALIDATOR`'s "Please, fill required fields" and ',' to '.'.
- `parseCommand`: AC / SP / TRAN tokens (case-sensitive `uic`), NOISE `pts.ToSpiceString()` (1000 shows "1k"), PZ, DC's `-1` sentinel, the linearize box never set from the command, the Custom page from `loadDirectives`, the title "X  —  Y" and "New Simulation Tab" with KiCad's order of types.
- `ApplySettings` and the base dialog: the option bits and compatibility modes; every label and default of `dialog_sim_command_base.cpp`; Plot Setup's grid on, legend off, dotted on, margins 70/30/45/70; the PZ choice texts.
- Swap sources and `updateDCSources` (sorted unique references by device; TEMP disables the choice).
- `SPICE_VALUE` (`%lf%7s`, "meg", `Normalize`, `ToSpiceString` as `%g`, `ToString` `%.3f` and with a `SPICE_VALUE_FORMAT`, `StripZeros`, the prefix tables, the operators); `SPICE_VALUE_FORMAT` `FromString` / `ToString` / `UpdateUnits`; `DIALOG_SIM_FORMAT_VALUE`'s units, ranges, precision 1..9 and Auto as "~"+units.
- `rebuildSignalsList`, `rebuildSignalsGrid`, `updatePlotCursors` (Diff row "[2 - 1]", custom cursors, "--"), `onCursorsGridCellChanged`, `UpdateMeasurement` and the measurements grid's empty row, User-defined Signals' Add id and dropped empty rows, `SetUserDefinedSignals`, `CommandToSimType` (Rust and TS agree).

---
## Part D. Design-block library dialogs (no round reviewed them)

Skipped as known: GAPS 32's "Left" and `docs/parity/findings/p11-p18.md` P18-1..8.

### B6-1. The read-only check tests the permission bits, not whether this user can write (medium)

**Fixed** on task-r6-editors-exports: 227b5bf9 (`is_writable`: effective uid/gid/groups from /proc against the mode bits; ACLs and read-only mounts not seen, noted in PARITY-common 14).
- **Ours:** `crates/kicad/src/design_block.rs:261-263`:
  ```rust
  pub fn is_writable(library: &Path) -> bool {
      std::fs::metadata(library).map(|m| m.is_dir() && !m.permissions().readonly()).unwrap_or(false)
  }
  ```
  On Unix `readonly()` is false when any write bit (owner, group, other) is set. `design_block_api.rs:92` (`writable_library`) and the listing's `"writable"` (`:152`) use it.
- **KiCad:** `common/design_block_io.cpp::IsLibraryWritable`: `return path.IsOk() && path.IsDirWritable();` (`access( W_OK )` for this user); the pane's `EditDesignBlockProperties`, `DeleteDesignBlockFromLibrary` and the Save flows then refuse with `_( "Library '%s' is read only." )`.
- **How to see it:** a table row for a root-owned 0755 folder (an installed design-block library through the global table). Ours calls it writable; Properties, Delete or Save fail later with "Design block metadata file '...' could not be saved." / "Design block folder '...' could not be deleted."; KiCad refuses at once with "Library 'X' is read only.".
- **Test:** `a_folder_owned_by_another_user_is_read_only`: as a non-root user, `assert!(!is_writable(Path::new("/usr")))` (true today).
- **Severity:** medium (wrong edge-case refusal). **Known?** No.

### B6-2. Save Current Sheet as Design Block starts with the project's name for the root, and the raw name for a sheet with '.' or '/' (low)

**Fixed** on task-r6-editors-exports: fffafcd5.
- **Ours:** `crates/cli/src/design_block_api.rs:309` `default_name: if name.is_empty() { project } else { name }`.
- **KiCad:** `eeschema/sch_design_block_utils.cpp::SaveSheetAsDesignBlock`:
  ```cpp
  wxFileName fn = wxFileNameFromPath( aSheetPath.Last()->GetName() );
  blk.SetLibId( LIB_ID( aLibraryName, fn.GetName() ) );
  ```
  The root sheet's name is "Root" (`files-io.cpp`: `topSheet->SetName( _( "Root" ) );`), and `GetName()` drops what follows the last '.' and what precedes the last '/'.
- **How to see it:** Save Current Sheet on a root: KiCad offers "Root", ours the project name. A sheet "PSU v1.2": KiCad offers "PSU v1", ours "PSU v1.2", which our own OK then refuses for its dot.
- **Test:** `prepare_for_the_root_sheet_names_it_root`: POST `/prepare` `{what:"sheet", sheet:"", lib:L}` gives `name == "Root"`; a sheet "A.b" gives "A".
- **Severity:** low. **Known?** No.

### B6-3. The chooser hides library nodes without blocks; KiCad shows an empty "-- Recently Used --" and keeps a matching empty library (low)

**Fixed** on task-r6-editors-exports: 12e30265.
- **Ours:** `web/studio/src/kicad-port/designBlocks.ts:53` adds "-- Recently Used --" only with items; `:57` `if (searching && scored.length === 0) continue;`.
- **KiCad:** `panel_design_block_chooser.cpp::rebuildHistoryNode` always `DoAddLibrary( "-- Recently Used --", ... )`; `lib_tree_model.cpp::LIB_TREE_NODE_LIBRARY::UpdateScore` gives a childless library `m_Score = 1` plus its own terms (its name at weight 8), and `GetChildren` shows a node with `m_Score > 0`.
- **Test:** `composeBlockGroups({libraries:[{name:"Power", blocks:[]}], query:"pow", history:[], pinned:∅})` contains "Power"; with query "" and no history the first group is the Recently Used label.
- **Severity:** low. **Known?** No.

### B6-4. New Design Block Library on an existing folder, and a duplicate nickname (low)

**Fixed** on task-r6-editors-exports: a753ed4a (read only, or Overwrite; the duplicate-nickname refusal is kept and documented in PARITY-common 14).
- **Ours:** `crates/cli/src/library_tables.rs:755-756` always refuses "Library {path} already exists."; `:738-741` refuses a duplicate nickname "Library 'X' already exists.".
- **KiCad:** `design_block_pane.cpp::createNewDesignBlockLibrary`: not writable, `_( "Library %s is read only." )`; writable, a KIDIALOG `_( "Library %s already exists." )` with OK labelled "Overwrite", then `pi->DeleteLibrary( libPath ); pi->CreateLibrary( libPath );`. `AddDesignBlockLibrary` adds the row with no nickname check.
- Ours is safer, but undocumented in `PARITY-common.md` section 14 (row 183). **Test:** `new_design_block_library_on_a_read_only_existing_folder_says_read_only`.
- **Severity:** low. **Known?** No.

### B6-5. Messages and looks (low)

**Fixed** on task-r6-editors-exports: 12e30265.
- Delete Design Block's buttons are "OK"/"Cancel" (`designBlockActions.ts:282`); KiCad's `IsOK` sets `SetOKCancelLabels( _( "&Yes" ), _( "&No" ) )`.
- The tree shows "(n)" after a library name (`DesignBlocksPane.tsx:225`); `LIB_TREE_MODEL_ADAPTER::GetValue`'s name column has only the pin and the name.
- The details' description is a plain div (`DesignBlocksPane.tsx:249`); `DESIGN_BLOCK_TREE_MODEL_ADAPTER::GenerateInfo` does `esc_desc.Replace( "\n", "<br>" ); esc_desc = LinkifyHTML( esc_desc );`.
- The table dialog's title is "Manage Design Block Libraries" (`kicad-port/libraryTable.ts:48`); KiCad's is `_( "Design Block Libraries" )` (likewise "Symbol Libraries", "Footprint Libraries").
- `DesignBlockDialogs.tsx` says "A placed sheet gets these as hidden fields.", which GAPS 32 says it does not.

### Checked and found to match (design blocks)
- `DIALOG_DESIGN_BLOCK_PROPERTIES`: layout, "Untitled Field", the name checks' order and message, `TransferDataFromGrid`'s newline / double-space cleanup and "Duplicate fields are not allowed.", the name locked only for Update from Sheet.
- `DESIGN_BLOCK_IO`: `DesignBlockLoad` (optional files, "could not be read", `null` fields), `DesignBlockSave`, `DesignBlockDelete`, `CreateLibrary`, `DeleteLibrary`, hidden folders skipped.
- The pane: `EditDesignBlockProperties`' order (read-only check, dialog, `checkOverwrite` on rename, save, delete the old), `DeleteDesignBlockFromLibrary`'s message and status, "New Design Block Library" and its description prompt; history only on double click / Enter, 7 entries, deleted blocks dropped; Recently Used, pinned, then score (`LIB_TREE_NODE::Compare`); hidden rows; the first Escape clears the search; the check boxes' labels and tooltips (`SCH_DESIGN_BLOCK_PANE::setLabelsAndTooltips`); the table panel's types, browse nickname rule and `KICAD10_DESIGN_BLOCK_DIR`.

---

## Part E. The 3D viewer's options (no round reviewed them)

V6-2 and V6-3 were re-read by me in the C++ (`board_adapter.cpp:378-386`, `eda_3d_viewer_frame.cpp:571-585`, `camera3d.ts:20-40,108-116,366-380`); V6-4..V6-9 come from a delegated reading and were not re-checked line by line.

### V6-2. Arrow-key and wheel pan steps are 0.7 mm, not 0.7 of KiCad's 3D units (medium; the r4/r5 "checked" note on the step is wrong)

**Fixed** on task-r6-editors-exports: 05e21200.
- **Ours:** `web/studio/src/kicad-port/camera3d.ts:113` `PAN_STEP_FACTOR = 0.7`; `panArrow` (`:373`), `animatePan` (`:652-656`) and `handleWheel` (`:445`) add `0.7 * zoom` to `cameraPosLocal`, which is in millimetres (the header, `:20-40`: "board geometry is never rescaled (it stays in real millimetres)").
- **KiCad:** `eda_3d_canvas.cpp::SetView3D` `delta_move = m_delta_move_step_factor * m_camera.GetZoom(); m_camera.Pan_T1( SFVEC3F( -delta_move, 0, 0 ) )` in 3D units, and a board spans `RANGE_SCALE_3D * 1.6 = 12.8` of them (`board_adapter.cpp:381-385`: `m_biuTo3Dunits = RANGE_SCALE_3D / std::max( m_boardSize.x, m_boardSize.y ); ... m_biuTo3Dunits *= 1.6f;`). One step is 0.7 / 12.8 = 5.47 % of the board's longest side. The header derives this scale for the camera distance but not for the pan.
- **How to see it:** a 100 mm board moves about 5.47 mm per arrow in KiCad, 0.7 mm in ours (a 10 mm module: 0.55 mm against 0.7 mm). The wheel's pan branches scale the same way.
- **Test:** `camera3d.test.ts` "a pan step is 0.7*zoom in KiCad's 3D units": `setBoardGeometry(100, ..)`, `reset()`, `panArrow("right")`: the x offset is 100 / 12.8 x 0.7 = 5.469 mm. r4's V4-1 wheel test pins the mm step and needs the same scale.
- **Severity:** medium. **Known?** No (r5 lists "the `0.7 * zoom` step" as matching).

### V6-3. Default appearance: stackup colours on and every layer row visible; KiCad starts on "Follow PCB Plot Settings" without stackup colours (medium)

**Fixed** on task-r6-editors-exports: eba680ce (untouched rows follow the plot selection; "Plot on All Layers" and the plot-reference flag taken at their defaults, noted in PARITY-3d).
- **Ours:** `state/store.tsx:463` `useStackupColors: true`; `appearance3d.ts`'s rows show Adhesive and User.Drawings / Comments / Eco1 / Eco2; PARITY-3d section 5 says identical.
- **KiCad:** `eda_3d_viewer_settings.cpp:425` defaults `current_layer_preset` to `LEGACY_PRESET_FLAG`, and `EDA_3D_VIEWER_FRAME::LoadSettings` (`eda_3d_viewer_frame.cpp:571-585`) turns it into `cfg->m_UseStackupColors = false; ... cfg->m_CurrentPreset = FOLLOW_PLOT_SETTINGS;`. `BOARD_ADAPTER::GetVisibleLayers` then follows the plot selection (`ret.set( LAYER_3D_ADHESIVE, layers.test( F_Adhes ) )`, ...), whose default (`pcb_plot_params.cpp`) is copper, silkscreen, mask, paste and Edge.Cuts only.
- **Test:** with the default plot selection, `adhesive` and `user_drawings` are hidden and `useStackupColors` is false.
- **Severity:** medium (wrong default). **Known?** No.

### V6-4. The 3D Reference text is drawn from the courtyard, not from the Reference field (medium)

**Fixed** on task-r6-editors-exports: 3f78d6ec (`kicad-port/footprint3d.ts`).
- **Ours:** `components/viewer3d/scene.ts:847-880` `buildRefDesignatorMesh`: `part.ref` at the courtyard centre, 0.5-0.9 mm high, angle 0, always on the silkscreen; `part.fields` ignored.
- **KiCad:** `create_3Dgraphic_brd_items.cpp::addFootprintShapes`: `if( field->IsReference() && !aFlags.test( LAYER_FP_REFERENCES ) ) continue;` ... `if( field->GetLayer() == aLayerId && field->IsVisible() ) addText( field, aContainer, field );`.
- **How to see it:** a hidden Reference, or one on F.Fab: ours shows "R1" on the silkscreen, KiCad nothing.
- **Test:** a part whose Reference is `visible:false` gives no `part-ref` mesh; a visible one uses the field's position, size and angle.
- **Severity:** medium. **Known?** In part: GAPS 23 lists the Value and user fields as missing, not the Reference drawn from the wrong source.

### V6-5. Solder paste ignores the pad's paste layer and margin (medium)

**Fixed** on task-r6-editors-exports: 3f78d6ec (margin and ratio), 82171e81 (`/api/state` sends each pad's file layers). A pad margin from the board file (not an edit) is still not in the state.
- **Ours:** `scene.ts:1056-1058` `if (shown("solder_paste") && !pad.th) addPad(..., pad.w, pad.h ...)`; `paste_margin` / `paste_ratio` never read.
- **KiCad:** `BOARD_ADAPTER::addPads`: `if( !pad->IsOnLayer( aLayerId ) ) continue;` and for F_Paste / B_Paste `margin += pad->GetSolderPasteMargin( aLayerId )`.
- **How to see it:** a fiducial (no F.Paste) gets paste in ours; a -0.05 mm paste clearance shows full-size paste.
- **Test:** a pad with `paste_margin: -100000` gives a paste mesh 0.2 mm smaller than the pad; a pad without F.Paste gives none.
- **Severity:** medium. **Known?** No.

### V6-6. An unknown stackup colour name is transparent here, opaque black in KiCad (low; medium with stackup colours on, see V6-3)

**Fixed** on task-r6-editors-exports: eba680ce.
- **Ours:** `appearance3d.ts:127-131` `findColor` returns `{0,0,0,0}` on a miss.
- **KiCad:** `board_adapter.cpp::GetLayerColors`' `findColor` ends `return KIGFX::COLOR4D();` = (0,0,0,1).
- **Test:** `resolveColors({useStackupColors:true, stackup:{layers:[{kind:"core"}]}}).board` deep-equals `{0,0,0,1}`.

### V6-7. Reset to default goes to the theme colour, not `GetDefaultColors()` (low)

**Fixed** on task-r6-editors-exports: eba680ce.
- **Ours:** `Viewer3DAppearancePanel.tsx:62-70`. **KiCad:** `appearance_controls_3D.cpp::rebuildLayers` gives the swatch `defaultColors[layer]`, `dialog_color_picker.cpp::OnResetButton` resets to it (e.g. `g_DefaultBoardBody = COLOR4D( 0.43, 0.45, 0.30, 0.90 )`).
- **Test:** after resetting Board Body, its colour is (110, 115, 77, 0.9).

### V6-8. F, Space, the arrows and F1/F2/F5 fire with modifiers held (low)

**Fixed** on task-r6-editors-exports: f5279e41.
- **Ours:** `actions3d.ts:103-117` accepts Shift+F, Shift+arrows and any modifier with F1/F2/F5. **KiCad:** `eda_3d_actions.cpp` binds `'F'`, `' '`, `WXK_LEFT` ... with no modifier. (r3 V3-1 was the model-row hotkeys only.)
- **Test:** `resolve3DAction({key:"F", shiftKey:true}) === null`.

### V6-9. Row labels and tooltips (low)

**Fixed** on task-r6-editors-exports: eba680ce.
- Model Bounding Boxes' tooltip "Show a bounding box per model" (`appearance3d.ts:187`); KiCad's "Show 3D model bounding boxes in realtime renderer".
- KiCad labels board-layer rows with `GetBoard()->GetLayerName(...)` (a renamed user layer shows its name); ours fixed labels (`Viewer3DAppearancePanel.tsx:137`).
- KiCad always lists User.Drawings, Comments, Eco1, Eco2 (only User.1..45 filtered by `enabled`); ours hides them unless the board draws on them, and PARITY-3d's reason for it is wrong.

### Checked and found to match (3D options)
- `EDA_3D_VIEWER_SETTINGS` defaults: every `show_*`, `rotation_increment` 10, animation on at speed 3, perspective, grid none, material normal, `subtract_mask_from_silk` and `clip_silk_on_via_annulus` off.
- Colours: the theme's 3D colours, `g_SilkColors`, `g_MaskColors`, `g_BoardColors`, `g_FinishColors` and the finish names, the dielectric mix, bottom copper = top copper, the copper swatch setting both rows, the read-only stackup rows.
- Row order and the action tooltips (`s_layerSettings`); `IsFootprintShown`; the `RotateView` signs and 10° step, zoom 1.26, the FIT / TOP / BOTTOM speed clamps, the arrow pan's LINEAR interpolation at speed 8, commands refused while moving, every `DefaultHotkey` of `eda_3d_actions.cpp`, the hover text (reference, two spaces, value).
- Not reviewed: the Display Options and OpenGL Options preference pages, which the studio does not have (PARITY-3d section 1).

---

## Summary

| ID | Area | What | Severity | Known? |
|---|---|---|---|---|
| V6-1 | 3D model preview | V5-2's fix is backwards: KiCad's preview zooms on a plain wheel (its canvas keeps `m_scrollModifierZoom = 0`), ours pans | low | no (r5 fix wrong) |
| X6-1 | IPC-D-356 | "Done." + kicad-cli's line; KiCad "IPC-D-356 netlist file created:\n'%s'." | low | in part (r5 X5-1) |
| X6-2 | api-server drill log | a backdrill pair file's line before its drill file's | low | no |
| D6-1 | drill charts | `kicad-cli pcb upgrade` rebuilds every chart (always stale on load), and every kicad-cli plot already does; the port only feeds the canvas and saved cells | low (reuse) | r5 D5-1 |
| S6-1 | Simulation Analysis | the include-path box from the tab's bit, not `GetFixIncludePaths()`; OK writes it into the project | medium | no |
| S6-2 | Plot Setup | Min/Max empty when unlocked (KiCad: the tick range), not SI-formatted, read as SPICE ("2M" = 2e-3) | medium | no |
| S6-3 | User-defined Signals | Delete removes the last row, not the cursor's | medium | no |
| S6-4 | DC sweep | an unlisted source (e.g. `temp`) is written as an empty name | medium | no |
| S6-5 | signal filter | any text is a regex; KiCad only `^..$` / `/../` | low | no |
| S6-6 | Noise / PZ | node choices show the first net while empty | low | no |
| B6-1 | design blocks | writable = permission bits, not `IsDirWritable` | medium | no |
| B6-2 | Save Sheet as Design Block | root named after the project (KiCad "Root"); no `wxFileName::GetName` trim | low | no |
| B6-3 | chooser | empty Recently Used / empty matching library hidden | low | no |
| B6-4 | New Library | existing folder always refused (KiCad: read-only message or Overwrite) | low | no |
| B6-5 | design blocks | Yes/No labels, "(n)", description links, the table title, a stale note | low | no |
| V6-2 | 3D viewer | pan step 0.7 mm, KiCad 0.7 of 12.8 board-span units | medium | no |
| V6-3 | 3D viewer | defaults: stackup colours on, all rows visible (KiCad: follow plot settings, no stackup colours) | medium | no |
| V6-4 | 3D viewer | the Reference drawn from the courtyard, not the field (hidden or Fab ref shown) | medium | in part (GAPS 23) |
| V6-5 | 3D viewer | paste ignores the paste layer and margin | medium | no |
| V6-6 | 3D viewer | unknown stackup colour transparent, KiCad opaque black | low | no |
| V6-7 | 3D viewer | colour reset to the theme, not `GetDefaultColors()` | low | no |
| V6-8 | 3D viewer | hotkeys fire with modifiers | low | no |
| V6-9 | 3D viewer | row labels and tooltips | low | no |

**Counts:** high 0, medium 10, low 13 (plus the simulator's 3 cosmetic notes, not counted).

**Round-5 merges:** 8 of the 9 fixes are right against the C++; V5-2's is backwards (V6-1), and X5-1 left the IPC-D-356 dialog message (X6-1).
