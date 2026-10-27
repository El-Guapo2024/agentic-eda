# Round 5 findings: editors, 3D, design blocks, simulator, fab outputs and exports (r5-editors-exports)

**Reviewer:** r5-editors-exports. A findings-only review on 2026-10-27/28. No code was changed.

**Versions.**
- Our code: main's checkout at `8a8e47f1`, read in `/home/team_braneboard_com/work/agentic-eda`.
- KiCad: `kicad-src-8303b2ad` first. `kicad-src-25ec2335` was used for `kicad/cli/*`, `pcbnew/api/api_handler_pcb.cpp`, `pcbnew/pcbnew_jobs_handler.cpp` and the newer dialogs. The box's kicad-cli is `10.99.0-unknown-25ec2335f2`, so its IPC API and the generated protos in `crates/kicad-engine/src/api/gen` are the same version.

**What was run** (scratch dir `/tmp/findings-r5-editors-exports.701s/`):
- main at `8a8e47f1`, built with its own `CARGO_TARGET_DIR` (`target/debug/eda`) and used on scratch copies only. No studio server was started; ports 8848-8849 were not needed.
- `eda import-kicad` on copies of QA `pcbnew/padstacks_normal` and `pcbnew/issue8407` (NPTH, oval holes). Then `eda board export --kicad drill|pos|ipcd356` was run twice, once with the api-server lane and once with `EDA_KICAD_API=0` (per-run kicad-cli). Each kind also ran with extra arguments: `drill -- --excellon-oval-format route --excellon-separate-th --generate-map --map-format gerberx2`, and `pos -- --format csv --units mm --side front --exclude-dnp`, and `--format gerber --side front`. The outputs are in `cmp/`, and `cmp.sh` reruns the comparison.
- `kicad-cli pcb export png` with the round-4 argument order, on round 4's two-layer test board (`png/`).

**Scope.**
- (a) The round-4 merges, each checked against the C++:
  - judge PNG order (b89cdedd);
  - Schematic Plot page by number (4d7f8899);
  - plot settings with no undo step (9c9527f3);
  - IPC-2581 BOM columns (75f6c3bb);
  - Symbol Editor Rotate and Repeat Pin (a272cd30);
  - `${OP}` on lone labels (707351f3);
  - 3D wheel and stackup thicknesses (7efff80d);
  - Footprint Editor drawing, Text and pad tools (29387c25);
  - push-pad filter (34f6e9a8).
- (b) The api-server export jobs (4368f790: `crates/kicad-engine/src/api/{jobs,server}.rs`, `run_export` and `export_pcb_placement` in `crates/kicad-engine/src/lib.rs`). They were compared with per-run kicad-cli, both in the C++ and on the files.
- (c) Rows next to these that no round reviewed: the 3D model preview, the Footprint Editor's Text dialog, and active code that duplicates kicad-cli.

**Finding IDs:** X5-n fab outputs and exports; V5-n 3D; E5-n editors; D5-n duplication of kicad-cli.

---

## Part A. Exports and the api-server lane

### X5-1. Through the api-server, a successful export's kicad-cli messages are dropped: IPC-D-356 shows "Done." alone, and warnings are lost (low)
- **What differs.**
  - Ours: `crates/kicad-engine/src/lib.rs:1665-1668` (`run_export`). With the lane, the reply's log is only the `message` of each `RunJobResponse`, and `ExecuteBoardJob` fills that only on failure.
  - The per-run path (`:1671-1674`) keeps kicad-cli's stdout and stderr as `log`, which `BoardExportDialog.tsx:413,465` shows under the result.
  - KiCad (`pcbnew/api/api_handler_pcb.cpp::ExecuteBoardJob`, 25ec2335):
    ```cpp
    int exitCode = aContext->GetKiway()->ProcessJob( KIWAY::FACE_PCB, &aJob, &reporter );
    ...
    if( exitCode == 0 )
    {
        response.set_status( types::JobStatus::JS_SUCCESS );
        return response;
    }
    ```
    The `WX_STRING_REPORTER`'s messages reach the response only in the error branch.
  - Measured on `issue8407`:

    | Path | Reply |
    |---|---|
    | per-run, ipcd356 | `"log": "Successfully created IPC-D-356 file"` |
    | api-server, ipcd356 | no `log` |
    | per-run, drill (CLI) | `"log": "Created file '…/intent.drl'\nDone."` |
    | api-server, drill (CLI) | no `log` |
    | per-run, pos | `"log": "Wrote front position data to '…'."` |
    | api-server, pos | no `log` |

  - So the same export reports differently depending on whether the server is up. A job's warnings are dropped too (any `RPT_SEVERITY_WARNING` the handler reports on a run that still succeeds).
  - Related wording: KiCad's `GenD356File` (`pcbnew/exporters/export_d356.cpp`, 8303b2ad) says `_( "IPC-D-356 netlist file created:\n'%s'." )`. Ours says "Done." (`BoardExportDialog.tsx:413`, `reply.message ?? "Done."`), followed by kicad-cli's line on the per-run path only.
- **How to see it.** Run `eda board export -C <b> --kicad ipcd356` with and without `EDA_KICAD_API=0`. Only the per-run reply has a `log`.
  - Test: `crates/kicad-engine/tests/api_jobs.rs::a_served_export_reports_what_kicad_says`. Run `export_pcb(.., "ipcd356", &[])` with the lane up, and assert that the reply's `message` (or `log`) is KiCad's "IPC-D-356 netlist file created:\n'<file>'.". Build that text ourselves, since the job does not return it.
- **Severity.** Low.
- **Already known?** No.

### X5-2. Export IPC-2581's kept BOM columns follow neither KiCad: 8303b2ad keeps nothing on Export, and 25ec2335 keeps every column including the Internal ID (low)
- **What differs.**
  - Ours (r4 X4-4, `web/studio/src/kicad-port/ipc2581Bom.ts:84-104`, `PARITY-boardctl.md:73`): the columns are kept for the next opening, but the Internal ID is not read back "as in KiCad (`TransferDataToWindow` sets it on `m_choiceMPN`)".
  - KiCad 8303b2ad: `dialog_export_2581.cpp::DIALOG_EXPORT_2581::onOKClick` sets a job before it transfers, so `TransferDataFromWindow` takes its `m_job` branch and never writes `prj.m_IP2581Bom`:
    ```cpp
    JOB_EXPORT_PCB_IPC2581 job;
    m_job = &job;

    TransferDataFromWindow();

    m_job = nullptr;
    ```
    `BOARD_EDITOR_CONTROL::GenIPC2581File` (`pcbnew/files.cpp`) only calls `dlg.ShowModal()`, and Close does not transfer. So in 8303b2ad the dialog keeps nothing, and the `m_choiceMPN` slip only matters for a project edited elsewhere.
  - KiCad 25ec2335: `onOKClick` calls `saveToProject()` first, which writes `id`, `mfg`, `MPN`, `distPN`, `dist` and `bomRev`, plus `mode`, `sections`, `netNames` and `refDes`. The BOM sub-dialog then reads the Internal ID back correctly (`dialog_export_2581_bom.cpp::DIALOG_EXPORT_2581_BOM::TransferDataToWindow`):
    ```cpp
    if( !m_oemRef->SetStringSelection( m_fields.m_internalId ) )
        m_oemRef->SetSelection( 0 );
    ```
  - So ours keeps the columns as 25ec2335 does, but drops the Internal ID as only 8303b2ad's (unreachable) read-back would.
- **How to see it.** Export with Internal ID = "Reference", then reopen the dialog. 25ec2335 shows "Reference"; ours shows "Generate unique". 8303b2ad shows nothing kept at all.
  - Test: `ipc2581Bom.test.ts` `the internal id is read back as 25ec2335's BOM dialog does`. `ipc2581DialogStart({id: "Reference"}, "", ["Reference", "Value"]).col_id === "Reference"`.
  - Or, if 8303b2ad is the target, drop the keeping and fix the PARITY text. Either way the PARITY row should name the KiCad it follows.
- **Severity.** Low.
- **Already known?** No. The row's "Differs" names only the `.kicad_pro` round trip.

### X5-3. A one-shot CLI export now pays for starting the api-server: drill, pos and ipcd356 run slower than per-run kicad-cli (low, performance)
- **What differs.**
  - Each `eda board export --kicad drill|pos|ipcd356` (and `eda fab drill|pos`) is a new process. `with_server` (`crates/kicad-engine/src/api/server.rs:327-371`) starts a `kicad-cli api-server`, waits for its `Ping`, opens the board, runs the job and shuts the server down at exit. Per-run kicad-cli only loads the board.
  - Measured with `/usr/bin/time`, debug `eda`:

    | Board | api-server | per-run |
    |---|---|---|
    | `padstacks_normal` | 1.02 s | 0.43 s |
    | `issue8407` | 1.80 s | 1.20 s |

    That is the same for each kind. The lane only pays off in the studio, where the server is kept between jobs.
- **How to see it.** `time eda board export -C <b> --kicad drill` against `time EDA_KICAD_API=0 eda board export ...`.
  - Fix direction: open the lane only when the caller is long-lived (the studio), or when a process runs several jobs.
- **Severity.** Low. The files are identical (see the checked list).
- **Already known?** No. The merge message says the server is "started on demand" without measuring a one-shot command.

### X5-4. `eda board export --kicad pos -- --format gerber` writes the Gerber X3 placement file as `<name>.pos` (low)
- **What differs.**
  - Ours: `export_pcb` (`crates/kicad-engine/src/lib.rs:1704-1716`) sends only ASCII and CSV to `export_pcb_placement`. A Gerber request goes to `run_export` with `pcb_ext_for("pos", ..)`, which gives `pos` (`:1576-1577`), so `-o` is `export/kicad/pos/<name>.pos`.
  - Run: `eda board export -C issue8407 --kicad pos -- --format gerber --side front` replied `"files": ["export/kicad/pos/intent.pos"]`, on both paths.
  - KiCad names it with `PLACEFILE_GERBER_WRITER::GetPlaceFileName`: `<name>-pnp_top.gbr` / `-pnp_bottom.gbr`. That is what our own `place_file_names` (`:1588-1594`) gives, and what the studio and `eda fab pos` use.
- **How to see it.** Run the command above.
  - Test: `export_pcb(.., "pos", ["--format","gerber","--side","front"])` names `export/kicad/pos/<name>-pnp_top.gbr`. Route every format through `export_pcb_placement`.
- **Severity.** Low (only the generic `eda board export` path).
- **Already known?** No.

### Checked and found to match (exports, api-server lane)
- **The jobs built from kicad-cli's arguments** (`api/jobs.rs`), compared with `CLI::PCB_EXPORT_DRILL_COMMAND` and `PCB_EXPORT_POS_COMMAND` (`doPerform` and the argument defaults) and with `API_HANDLER_PCB::handleRunBoardJobExportDrill`, `...Position` and `...IpcD356`:
  - every default: excellon, mm, decimal, alternate, absolute, map pdf only with `--generate-map`, precision 6, pos in/ascii/both;
  - `m_excellonCombinePTHNPTH = !separate`;
  - `--generate-report` and `--variant` left to the per-run path;
  - the "both" + gerber refusal left to kicad-cli's own words;
  - `m_singleFile` / `m_nakedFilename = true`;
  - `exclude_from_bom` false, the `JOB_EXPORT_PCB_POS` default kicad-cli never changes.
- **The kept board** (`Client::open_board_kept`): the job loads its own board through `PCBNEW_JOBS_HANDLER::getBoard`. In a headless server with a project open, that is the cached `m_cliBoard` whatever path it is given, and `IFACE::closeBoard` clears it through `ClearCachedBoard`. Our key (canonical path + hash of the board and its `.kicad_pro`) reopens on any change, so a stale `m_cliBoard` cannot be used. Locale: every writer here has its own `LOCALE_IO` (`place_file_exporter.cpp`, `export_d356.cpp`, `gendrill_gerber_writer.cpp`, `gerber_placefile_writer.cpp`), so the per-run `LOCALE_IO` the server lacks makes no difference.
- **Files, server against per-run, byte-identical apart from the date lines:**
  - drill: `-PTH.drl`, `-NPTH.drl`, `.drl`, and the two Gerber X2 maps;
  - `-all.pos` and `-top-pos.csv`;
  - `.d356`;
  - on both QA boards (`cmp/<board>-on` against `cmp/<board>-off`).
- **Round-4 fixes verified:**
  - **X4-1:** kicad-cli with `--layers Edge.Cuts --common-layers B.Courtyard,B.SilkS,B.Cu,F.Cu,F.SilkS,F.Courtyard` paints F.Cu over B.Cu in the overlap (`png/out/t-Edge_Cuts.png`). The only side effect is that the outline is painted under the copper, which is cosmetic.
  - **X4-2:** `page_of` is the walk's `effective_page`. The root is "1", and the writer always writes the root as page "1" (`crates/kicad/src/lib.rs:771`), so the two agree.
  - **X4-3:** `outside_history` covers `SetPlotSettings` and `SetIpc2581Bom`. Undo and redo carry both over, and a `file` step restores them.
  - **X4-4, apart from X5-2:** `onDistPNChange`'s ten names in order; `onMfgPNChange`'s "manufacturer" / "mfg" (`FindString` ignores case); "N/A" and disabled with no column; the field set (`std::set<wxString>`, the four mandatory fields only when there is a footprint); the BOM revision falling back to the root title block's revision (`eeschema_config.cpp`: `m_IP2581Bom.schRevision = RootScreen()->GetTitleBlock().GetRevision()`).

---

## Part B. 3D viewer and 3D model preview

### V5-1. The 3D model preview's dummy board is 1.58 mm thick; KiCad's is 1.51 mm (low)
- **What differs.**
  - Ours: `web/studio/src/kicad-port/model3d.ts:201` `previewStack()` gives a body of `boardThicknessNm - 2 * 10 um` = 1.58 mm. It takes the dummy board to have no copper ("no copper is subtracted"), and `model3d.test.ts:257-260` pins 1.58.
  - KiCad: `pcbnew/dialogs/panel_preview_3d_model.cpp` (ctor):
    ```cpp
    dummy_bds.SetEnabledLayers( LSET::FrontMask() | LSET::BackMask() );
    ...
    dummy_board_stackup.BuildDefaultStackupList( &dummy_bds, 2 );
    ```
    - `common/lset.cpp::LSET::FrontMask` is `LSET( FrontTechMask() ).set( F_Cu )`, so the mask includes copper.
    - `BOARD_DESIGN_SETTINGS::SetEnabledLayers` gives `m_copperLayerCount = copperLayers.count()`, which is 2.
    - So `BOARD_STACKUP::BuildDefaultStackupList` takes 2 copper layers: the dielectric is 1.6 − 2 × 0.035 − 2 × 0.01 = 1.51 mm, and `InitSettings` sums only the dielectric into the body.
- **How to see it.** A model with no offset in the preview sits at 0.79 + 0.035 + 0.04 mm in ours and at 0.755 + 0.035 + 0.04 mm in KiCad. The slab is 0.07 mm thicker in ours.
  - Test: `model3d.test.ts` "the 3D model preview's dummy board" asserts `previewStack().boardThicknessMm === 1.51` and `footprintZ(s, false) === 0.755 + 0.035 + 0.04`. Fix direction: `previewStack` = `stackOfBoard({ copper_layers: 2, board_thickness_nm })`.
- **Severity.** Low.
- **Already known?** No. It comes from V4-2's fix.

### V5-2. The 3D model preview ignores the zoom-modifier preference for the wheel (low)
- **What differs.**
  - Ours: `web/studio/src/components/footprint/ModelPreview3D.tsx:211` calls `camera.handleWheel({...})` without the scroll settings, so pan mode (`aPan`) is always off.
  - KiCad: `panel_preview_3d_model.cpp::PANEL_PREVIEW_3D_MODEL::loadSettings`:
    ```cpp
    m_boardAdapter.m_MousewheelPanning = settings->m_Input.scroll_modifier_zoom != 0;
    ```
    The preview canvas is never given `SetVcSettings`, so its other modifiers stay at `VC_SETTINGS::Reset`'s defaults (pan H = Ctrl, pan V = Shift).
- **How to see it.** Set the zoom modifier to Ctrl in Preferences > Mouse and Touchpad, then scroll in the preview. KiCad pans; ours zooms. The main viewer is right: it passes the preferences (`Viewer3D.tsx:458`).
  - Test: with `scrollModifierZoom` set, a plain `{deltaY:-100}` moves the look-at point and does not zoom.
- **Severity.** Low.
- **Already known?** No.

### V5-3. A dielectric with sublayers counts only its first sublayer in the 3D body, though the comment says they are summed (low)
- **What differs.**
  - Ours: `crates/kicad/src/import.rs:947` reads a layer with `addsublayer` as its first sublayer. `model3d.ts:155` says "(every sublayer; ours holds them summed)", which is not what the import does.
  - KiCad: `board_adapter.cpp::BOARD_ADAPTER::InitSettings`:
    ```cpp
    case BS_ITEM_TYPE_DIELECTRIC:
        for( int sublayer = 0; sublayer < item->GetSublayersCount(); sublayer++ )
            body_thickness += item->GetThickness( sublayer );
    ```
- **How to see it.** A board whose core has two 0.5 mm sublayers. KiCad's body counts 1.0 mm for it and ours 0.5 mm, so the models sit too low.
- **Severity.** Low.
- **Already known?** In part: the sublayer loss on import is r2 F17 (open). Its effect on the 3D body, and the wrong comment, are not listed.

### V5-4 (cosmetic, not counted). Mask and silkscreen heights in the 3D scene
- `scene.ts:124-130` puts the mask plane at copper/2 + mask/2, below the copper's top, and the silkscreen at copper + mask + 0.01 mm.
- KiCad's `InitSettings` runs F.Mask from the copper top to the copper top + mask, and starts F.SilkS 1.1 × 0.025 mm above the copper top.
- The difference is under 0.05 mm.

---

## Part C. Footprint Editor (the Text tool's dialog)

### E5-1. The Footprint Editor's Text dialog has its own size message (low)
- **What differs.**
  - Ours: `web/studio/src/components/footprint/FpTextDialog.tsx:54` says "The text size must be between 0.001 mm and 250 mm."
  - KiCad: `dialog_text_properties.cpp::DIALOG_TEXT_PROPERTIES::TransferDataFromWindow` validates width and height through `common/widgets/unit_binder.cpp::UNIT_BINDER::Validate`: `_( "%s must be at least %s." )` and `_( "%s must be less than %s." )`. The `%s` is the label without its colon ("Width", "Height", `dialog_text_properties_base.cpp`).
- **How to see it.** A size of 0 should give "Width must be at least 0.001 mm.", and a size of 300 "Width must be less than 250 mm." Ours has one Size field (a documented difference), so the Width wording applies.
  - Test: `fpTextSizeMessage(0) === "Width must be at least 0.001 mm."`.
- **Severity.** Low.
- **Already known?** No.

### E5-2. A text placed at an offset edited in the Text dialog is left unselected (low)
- **What differs.**
  - Ours: `FpTextDialog.tsx:68` does `void api.addText(done)` and selects nothing. The usual path, a second click on the carried text (`FootprintCanvas.tsx:504-511`), does select it.
  - KiCad: `drawing_tool.cpp::DRAWING_TOOL::PlaceText`. When the dialog moved the text (`text->GetTextPos() != cursorPos`), it commits at once and selects:
    ```cpp
    commit.Add( text ); commit.Push( _( "Draw Text" ) );
    m_toolMgr->RunAction<EDA_ITEM*>( ACTIONS::selectItem, text );
    ```
- **How to see it.** With the Text tool, click, change Offset X in the dialog and press OK. In KiCad the text is selected, so E or Delete act on it; in ours nothing is.
  - Test (e2e, `fp-tools.check.js`): after OK with an edited offset, the selection is the new text's id.
- **Severity.** Low.
- **Already known?** No.

---

## Part D. Active code that duplicates kicad-cli (round-5 rule)

### D5-1. Drill charts are built by a Rust port that its PARITY row proves "identical to kicad-cli's `pcb upgrade`" (low, reuse)
- `crates/ops/src/drill_chart.rs` (869 lines) ports KiCad 10's drill model, `DRILL_CHART_MODEL::Build`, `AssignDrillSymbols` and `PCB_GENERATED_TABLE::RebuildCells`. It rebuilds a chart on every edit (`Board::refresh_drill_charts`).
- `PARITY-pcb.md:2049` says the built chart is "identical to kicad-cli's `pcb upgrade`, which rebuilds a stale chart". So kicad-cli already does the same work. With the api-server lane now kept (4368f790), a chart's cells could come from KiCad itself (`RefreshGeneratedTables` on the open board) instead of the port.
- The canvas drawing of the marks (`drillChart.ts`) is display code and is not in question.
- **Severity.** Low (no wrong output found). Reported because of the round-5 reuse rule.
- **Not counted, documented choice:** the 3D viewer builds the board slab, copper, mask and silkscreen itself (`scene.ts`), while kicad-cli's GLB is the optional "Exact export". GAPS 23 explains this by speed (2.7-4.4 s against 10-50 s).

---

## Checked and found to match (editors, 3D, `${OP}`), not to review again
- **V4-1, 3D wheel** (`camera3d.ts:436`): every branch of `HIDPI_GL_3D_CANVAS::OnMouseWheelCamera`:
  - the modifier precedence;
  - the `0.7 * zoom` step, scaled by the wheel in pan mode and negated for a negative rotation otherwise;
  - the two pan branches and the ×1.1 zoom, each with its reversal;
  - pan mode only with a zoom modifier set (`eda_3d_viewer_frame.cpp:1022`).
  - Round 4's macOS remark (`common_settings.cpp:537-539`) is the 0→1 settings migration, not a platform default, so one mapping is right.
- **V4-2, stackup** (`stackOfBoard`, `footprintZ`):
  - the body is the dielectrics plus the enabled inner copper; the outer copper and the masks come from their items, at least 1 µm each;
  - the defaults are 1.6 / 0.035 / 0.025 mm and the paste is 0.04 mm;
  - `GetFootprintZPos` on both sides;
  - the no-stackup path is `BuildDefaultStackupList`'s integer division.
- **E4-1:** `rotatePivot` is `SELECTION::GetCenter`. A selection of texts only takes the truncated mean of their positions; otherwise the box of the non-text items, of every unit and body style. Then `GetNearestHalfGridPosition`.
- **E4-2:** `RepeatDrawItem` clears the selection and selects the new pin, also after the occupied-spot question is refused.
- **S4-1:** `labelNet` falls back to a same-scope same-named label of the sheet, then a global power symbol, then its own driver name. It shows "--" before a run.
- **E4-F1:**
  - the layer-class order and the Footprint Editor's widths, 0.1 / 0.2 / 0.05 / 0.05 / 0.1 / 0.1 mm;
  - the text defaults, 1 / 0.1, 1.5 / 0.3 and 1 / 0.15 mm;
  - polygons unfilled on Edge.Cuts and the courtyards, at the layer's width.
- **E4-F2, apart from E5-1 and E5-2:**
  - `PlaceText`'s size, pen and left alignment;
  - mirroring on `IsBackLayer`'s 7 layers;
  - "Footprint Text Properties" with "Offset X:" / "Offset Y:";
  - an empty text dropped, and the clamp message;
  - the tool staying armed, with the carried text selected when it is placed.
- **E4-F3:** `PAD::Flip` (the offset, trapezoid delta, orientation, chamfer corners, layers, and a custom pad's shapes with arcs' ends swapped), and the 90° step.
- **E4-F4:** the messages of `DIALOG_FOOTPRINT_PROPERTIES_FP_EDITOR::Validate`, word for word, with limits 0.001 / 250 mm.
- **E4-F5:** `doPushPadProperties`' attribute test and the aperture special case (an aperture pad is one on no copper layer, `*.Cu` counted as copper).
- **E4-F6, E4-F7:** Paste keeps the Value. Renumber Pads matches `dialog_enum_pads_base.cpp` and `EnumeratePads`: title, note, labels, 4-character prefix, 0..999 spins and the popup.

---

## Summary

| ID | Area | What | Severity | Known? |
|---|---|---|---|---|
| X5-1 | api-server exports | A served job's kicad-cli messages are dropped on success (`ExecuteBoardJob` fills `message` only on error); IPC-D-356 shows "Done." where KiCad says "IPC-D-356 netlist file created:\n'%s'." | low | no |
| X5-2 | Export IPC-2581 | Kept columns follow neither KiCad: 8303b2ad keeps nothing on Export (`onOKClick` transfers into a job), 25ec2335 keeps all and reads the Internal ID back | low | no |
| X5-3 | api-server lane | One-shot CLI exports start a server each time: 1.02 s against 0.43 s, 1.80 s against 1.20 s | low | no |
| X5-4 | `eda board export` | `pos --format gerber` writes `<name>.pos`; KiCad's `-pnp_top.gbr` | low | no |
| V5-1 | 3D model preview | Dummy board body 1.58 mm; KiCad 1.51 (FrontMask has F.Cu, so 2 copper layers) | low | no (V4-2's fix) |
| V5-2 | 3D model preview | The wheel ignores the zoom-modifier preference (`loadSettings`) | low | no |
| V5-3 | 3D viewer | Only a dielectric's first sublayer counts; the comment says they are summed | low | in part (r2 F17) |
| E5-1 | Footprint Editor Text dialog | Size message is not `UNIT_BINDER::Validate`'s "Width must be at least %s." | low | no |
| E5-2 | Footprint Editor Text tool | A text placed at an edited offset is not selected | low | no |
| D5-1 | Drill charts | `drill_chart.rs` re-does what kicad-cli's `pcb upgrade` / `RefreshGeneratedTables` does | low (reuse) | no |
| V5-4 | 3D scene | Mask and silkscreen heights a few µm off | cosmetic, not counted | no |

**Counts:** high 0, medium 0, low 10 (plus 1 cosmetic, not counted).

**Round-4 merges:** all 16 fixes (X4-1..4, E4-1, E4-2, S4-1, V4-1, V4-2, E4-F1..F7) are correct against KiCad's code. The only new differences come from V4-2's preview path (V5-1) and E4-F2's dialog (E5-1, E5-2). The api-server's drill, pos and IPC-D-356 files are byte-identical to per-run kicad-cli on two QA boards, apart from the date lines.
