# Round 4 findings: editors, 3D, design blocks, simulator, fab outputs and exports (r4-editors-exports)

**Reviewer:** r4-editors-exports. A findings-only review on 2026-10-27/28; no code was changed.

**Versions.**
- Our code: main's checkout at `01ffc984` (the merge of task-reuse-cpp), read in `/home/team_braneboard_com/work/agentic-eda`.
- KiCad: `kicad-src-8303b2ad`; `kicad-src-25ec2335` for `kicad/cli/*`, `pcbnew/pcb_plotter.cpp` and the vendored nanosvg (8303b2ad's `thirdparty` is cleaned out).

**What was run** (scratch dir `/tmp/findings-r4-editors-exports.t7TQ/`):
- main at `01ffc984` built with its own `CARGO_TARGET_DIR` (`target/debug/eda`), used on scratch copies only, and its studio server on port 8848;
- `kicad-cli` 10.99.0 on a hand-made two-layer board (`png/t.kicad_pcb`) and on a copy of QA `eeschema/issue12814*` (`pg/`).

**Scope.**
- (a) The round-3 merges: the plot params whitelist and `Cmd::SetPlotSettings` (899dfdef, 0e6b4f69), Set Anchor in both editors (a10c9d08), the Rotate pivot (3cd18c9f), Repeat Pin (0e6b4f69), `${OP}` and the 3D/model-path fixes (459f1ffe), the `.cmp` path (9aedac85), the STEP Model Scale Warning; the pictures from kicad-cli (cb0825a0, `crates/kicad-engine/src/pictures.rs`); the SVG import through KiCad's nanosvg (6592765d).
- (b) Rows marked done/identical that no round reviewed: the schematic Plot dialog's "current page", the board export dialogs (IPC-2581, ODB++), Push Pin properties, the 3D viewer's wheel and stackup, design blocks and the simulator (no new difference there), and the Footprint Editor rows listed in Part D.

**Finding IDs:** X4-n fab outputs, exports and pictures; E4-n editors; S4-n simulator / `${OP}`; V4-n 3D viewer; E4-Fn Footprint Editor (Part D).

---

## Part A. Fab outputs, exports and pictures

### X4-1. The judge's board PNG paints the back copper, the back silkscreen and the courtyards over the front copper (medium)
- **What differs.**
  - Ours: `crates/kicad-engine/src/pictures.rs:155` plots the PNG as `pcb export png --layers F.Cu --common-layers Edge.Cuts,B.Courtyard,B.SilkS,B.Cu,<inner, reversed>,F.SilkS,F.Courtyard ...`. The doc comment of `board_picture_layers` (`:91-94`) says the layers are "back to front ... the front last".
  - kicad-cli (`pcbnew/pcb_plotter.cpp::PCB_PLOTTER::getPlotSequence`, 25ec2335) always plots the `--layers` entry first and the common layers after it, in their order:
    ```cpp
    // Base layer always gets plotted first.
    plotSequence.push_back( aLayerToPlot );

    for( PCB_LAYER_ID layer : aPlotWithAllLayersSeq )
    {
        // Don't plot the same layer more than once;
        if( find( plotSequence.begin(), plotSequence.end(), layer ) != plotSequence.end() )
            continue;

        plotSequence.push_back( layer );
    }
    ```
    So F.Cu is painted first, at the very back, and B.Cu, every inner layer, B.SilkS and both courtyards are painted over it.
  - The SVG (`--mode-single --layers Edge.Cuts,...,F.Courtyard`) is in the right order (checked: B.Cu's colour comes before F.Cu's in the file). The PNG is what the judge sends to the model whenever kicad-cli has `pcb export png` (`crates/judge/src/lib.rs:140-142`), and 10.99 has it.
- **How to see it.**
  - Run: `png/t.kicad_pcb` has an F.Cu rectangle (100..120, 100..110) and a B.Cu rectangle (110..130, 100..110). The command line of `board_plot` (with `--dpi 150` and the empty drawing sheet) gives `png/out/t-F_Cu.png`, in which the overlap 110..120 is blue (B.Cu). The front copper is hidden under the back copper wherever both have copper: under every through-hole pad and via, and under any back-side pour.
  - Test: slow `crates/cli/tests/pictures.rs::the_board_png_has_the_front_copper_on_top`. Build a board with overlapping F.Cu and B.Cu shapes, call `board_plot(.., PictureFormat::Png, true)`, and assert that the pixel in the overlap has F.Cu's theme colour (`#C83434` area), not B.Cu's.
  - Fix direction: make the back-most layer the base, `--layers Edge.Cuts --common-layers B.Courtyard,B.SilkS,B.Cu,..,F.Cu,F.SilkS,F.Courtyard` (the output name changes to `-Edge_Cuts.png`, which `plotted()` already reads from kicad-cli's report).
- **Severity.** Medium: every routing picture the judge sees has the wrong copper on top, so a reviewer (or the model) judges the front side by the back.
- **Already known?** No. GAPS 38 lists only the rubric and the page size.

### X4-2. Schematic Plot "current page only" plots the sheet whose page number is the sheet's place in the stored list, so it can plot another sheet (medium)
- **What differs.**
  - Ours: `crates/cli/src/sch_output_api.rs:32-58` `page_of` counts the sheets depth first over `sch.sheets` in stored order (`:37`) and returns that count. `plot_args` then passes it as `--pages <n>` (`:76`).
  - kicad-cli: `--pages` is a list of page *numbers* (the sheet's `(page "..")`), which `SCH_PLOTTER` keeps by `SCH_SHEET_LIST::TrimToPageNumbers` (`eeschema/sch_sheet_path.cpp`):
    ```cpp
    return std::find( aPageInclusions.begin(), aPageInclusions.end(),
                      sheet.GetPageNumber() ) == aPageInclusions.end();
    ```
  - KiCad's dialog does not use page numbers for this at all: "Plot Current Page" plots `m_schematic->CurrentSheet()` (`SCH_PLOTTER::createSVGFiles` and the other formats: `sheetList.push_back( m_schematic->CurrentSheet() )`).
  - The two counts differ:
    - when a sheet was renumbered (Edit Sheet Page Number, or an imported file with its own numbers: `SheetInstance::page`, `SchExtras::sheet_pages`);
    - when the stored order is not `SCH_SCREEN::GetSheets`' order (by x, then y). The derived file's virtual page numbers follow that order (`eda_model::sheet_list::sheets_in_order`, used by the writer in `crates/kicad/src/lib.rs:903`), but `page_of` does not.
- **How to see it.**
  - Run: in a copy of QA `issue12814.kicad_sch` the two sheets' pages were swapped ("Drive" page 3, "Usage" page 2) and the copy was imported (`pg/b`). `POST /api/sch/plot {"format":"svg","plot_all":false,"sheet_path":"9a030759-73ea-44ba-bd5b-ba78c58ff624"}` asks for "Drive". The reply was `"files":["export/kicad/sch-svg/intent-Usage.svg"]`: the other sheet was plotted.
  - Test: `sch_output_api::tests::plot_current_page_passes_the_sheets_own_page_number`. Two root sheets: A at x=100 with page "5", and B at x=50 with no page. Assert `page_of(A) == "5"` and `page_of(B) == "2"` (B is first by x). Build the pages from `eda_model::sheet_list::walk(design)` and `SheetEntry::effective_page()`.
- **Severity.** Medium: the wrong page is plotted, or none.
- **Already known?** No. PARITY-sch.md:384 calls the dialog "identical".

### X4-3. Plot / Generate Drill Files store their choices as an undo step; KiCad's `applyPlotSettings` and `updateConfig` push none (low)
- **What differs.**
  - Ours: `PlotDialog.tsx:110` and `GenerateDrillDialog.tsx:43` send `set_plot_settings` through `api.cmd` (`crates/ops/src/board_control.rs:326`). That is one undo step, so Ctrl+Z after a plot takes back the dialog's layer checklist and origin, not the user's last edit.
  - KiCad: `dialog_plot.cpp::DIALOG_PLOT::applyPlotSettings` does `m_editFrame->SetPlotSettings( tempOptions ); m_editFrame->OnModify();`, and `dialog_gendrill.cpp::DIALOG_GENDRILL::updateConfig` does `m_board->SetPlotOptions( m_plotOpts ); m_pcbEditFrame->OnModify();`. Neither has a commit, so there is no undo entry.
  - PARITY-boardctl.md:70-71 says "one undo step" in the sentence that describes it as KiCad's behaviour, not as a difference. Round 3 recorded the same kind of extra undo step for Move Symbol Anchor as a deliberate difference (E3-2's side note).
- **How to see it.** Move a footprint, then open Plot, untick F.Paste and press Plot. Ctrl+Z ticks F.Paste back in ours. In KiCad it moves the footprint back.
- **Severity.** Low. Either keep it and record it in PARITY-boardctl as a deliberate difference, or apply the verb outside the undo history.
- **Already known?** No.

### X4-4. Export IPC-2581: the BOM columns are free text, are not kept in the project, and the distributor is not guessed; PARITY says "identical" (low)
- **What differs.**
  - KiCad: `pcbnew/dialogs/dialog_export_2581.cpp`.
    - The column controls are choices of the board's footprint field names (`m_oemRef->Append( items )`).
    - `TransferDataFromWindow` keeps them in the project (`prj.m_IP2581Bom.id/mfg/MPN/distPN/dist/bomRev`), and `TransferDataToWindow` reads them back. The BOM revision falls back to `prj.m_IP2581Bom.schRevision`.
    - `onDistPNChange` fills Distributor from the Distributor P/N column's name ("DIGIKEY" -> "Digi-Key", "MOUSER" -> "Mouser", ..., "LCSC" -> "LCSC"). Distributor is "N/A" and disabled while no Distributor P/N column is chosen.
  - Ours: `web/studio/src/components/BoardExportDialog.tsx:268-305` has six free text inputs that start empty on every opening. The project keeps nothing. A typed field name that does not exist is passed to kicad-cli as it is. With Distributor left empty, `--bom-col-dist` is not passed, where KiCad would send "LCSC" for an "LCSC" column.
  - PARITY-boardctl.md:73 lists IPC-2581 as "identical".
- **How to see it.** On a board whose parts have an `LCSC` field, choose Distributor P/N = LCSC and export. KiCad's file names the distributor "LCSC". Ours names none unless it is typed. Reopen the dialog: KiCad keeps the columns, ours has forgotten them.
  - Test: `board_output_api::tests::ipc2581_guesses_the_distributor_from_the_pn_column`. Input `{col_dist_pn: "LCSC"}`; assert `--bom-col-dist LCSC` is in the arguments.
- **Severity.** Low.
- **Already known?** No.

### Checked and found to match (exports, pictures, import)
- **Plot params whitelist (899dfdef):**
  - `FORMAT_TOKENS` is exactly the item list `PCB_PLOT_PARAMS::Format` writes (8303b2ad and 25ec2335 alike).
  - `read_item` drops `viasonmask`, `svguseinch`, `pscolor`, `plotinvisibletext`, the HPGL tokens and unknown ones (`default: skipCurrent()`).
  - `(excludeedgelayer no)` pushes Edge.Cuts.
  - `gerberprecision` is clamped `parseInt( 5, 6 )` and is not written at 6.
  - `normalized()` re-reads an older stored block the same way.
  - Cosmetic only: an unknown `(mode 2)` is kept as written, where KiCad ignores `mode` on read and writes its own.
- **`Cmd::SetPlotSettings` (0e6b4f69):**
  - The selection is the checked layers `| ( LSET::AllCuMask() & ~GetEnabledLayers() )` (`applyPlotSettings`), with layers the board has not enabled dropped (`m_layerList` is `GetEnabledLayers().UIOrder()`).
  - Only a change is stored (`!m_plotOpts.IsSameAs( tempOptions )`; `IsSameAs` compares `m_useAuxOrigin`).
  - The drill dialog stores its Origin only (`updateConfig`).
  - The dialogs start from the board at each opening (`init_Dialog`, `TransferDataToWindow`).
- **DNP options:** read back as `init_Dialog` reads them (hide, else sketch + cross-out).
- **`.cmp` path (9aedac85):** the footprint's own `(path ..)` from the written board.
- **STEP Model Scale Warning:** the scan, the 5-model stop, the caption, the message with its two spaces, and the extended message, all word for word (`DIALOG_EXPORT_STEP::DIALOG_EXPORT_STEP`).
- **ODB++ defaults:** mm, precision 6 (2..16), ZIP (`dialog_export_odbpp_base.cpp`).
- **IPC-2581 defaults:** mm, precision 6 (4..10), version C.
- **Pictures:**
  - `pcb export png` always draws the drawing sheet (`JOB_EXPORT_PCB_PLOT::m_plotDrawingSheet( true )`, and `--include-border-title` only sets it true), so the empty drawing sheet is needed.
  - The SVG layer order is right.
  - `plotted()` reads kicad-cli's "Plotted to '..'." lines.
- **SVG import through nanosvg (6592765d):**
  - The vendored `nanosvg.cpp`/`.h` are byte-identical to 25ec2335's `thirdparty/nanosvg`.
  - The shim calls `nsvgParse( buf, "mm", 96 )` on a NUL-terminated copy, as `SVG_IMPORT_PLUGIN::LoadFromMemory` does.
  - `NSVG_PAINT_UNDEF` cannot reach the importer (`nsvg__createGradients` turns it into NONE).
  - `import_shapes` follows `SVG_IMPORT_PLUGIN::Import` line for line: visibility, the no-paint skip, the `-1` width, the fill alpha, black as unspecified, the dash classes with `strokeWidth * 1.9`, the fill rule, and open filled paths drawn twice. The differences are the documented ones (text, the gradient's alpha, the messages).
- **"Check zone fills before plotting":** KiCad's filler through kicad-cli (`ensure_filled`), stored in the board as the dialog's checker does. It does not duplicate kicad-cli's `--check-zones`.

---

## Part B. Symbol Editor (and the Set Anchor of both editors)

### E4-1. Rotate of several items: the pivot leaves out every imported or unit-specific graphic, and counts selected texts that KiCad leaves out (medium)
**Fixed** on task-r4-editors-exports: a272cd30

- **What differs.**
  - Ours: `web/studio/src/state/symbolEditorStore.tsx:681` takes the selected graphics' boxes from `symbolItemBoxes(sym, 0, 0)`. That function keeps an item only if `(u === 0 || u === unit) && (b === 0 || b === bodyStyle)` (`kicad-port/itemBoxes.ts:208`). With unit 0 and body style 0 it keeps only graphics of unit 0 *and* body style 0, which is what the studio's own draw tool makes (`drawTarget`). A symbol read from a `.kicad_sym` has its body in `Name_0_1` (unit 0, body style 1) or `Name_1_1` (`crates/kicad/src/symbol_lib.rs:1153` shows the form). Those graphics are never in the box, so Select All + R turns the pins about the centre of the pins alone.
  - KiCad: `symbol_editor_edit_tool.cpp::SYMBOL_EDITOR_EDIT_TOOL::Rotate` uses `rotPoint = m_frame->GetNearestHalfGridPosition( selection.GetCenter() )`. `common/tool/selection.cpp::SELECTION::GetCenter` merges every selected item except texts:
    ```cpp
    for( EDA_ITEM* item : m_items )
    {
        if( !item->IsType( { SCH_TEXT_T, SCH_LABEL_LOCATE_ANY_T } ) )
            bbox.Merge( item->GetBoundingBox() );
    }
    ```
    A selection of texts only takes the mean of their positions. Our loop adds a selected `text` graphic's box (when its unit and style pass the filter above).
  - PARITY-symedit.md:263 says the pivot counts "each selected graphic's box". Shapes not turning is a known, separate gap.
- **How to see it.** Open an imported symbol whose rectangle is wider on one side of the pins (for example a connector with pins on the left only). Select All and press R. KiCad's pivot is the centre of the rectangle and pins together; ours is the centre of the pin column, so the pins land somewhere else relative to the body (which does not turn in either case here).
  - Test: `symbolEditorStore` (or a pure helper extracted from `rotateSelection`), `the rotate pivot counts a body_style 1 rectangle and skips a text`. A symbol with a rectangle `{unit:0, body_style:1}` from (0,0) to (10,10) mm, one pin at (0,5) pointing left, and a text at (20,20). Select all three. Assert that the pivot is the half-grid snap of the centre of the rectangle and pin boxes, and that the text is not in it.
  - Fix direction: box the selected graphics by id regardless of unit/style, and skip `kind === "text"`.
- **Severity.** Medium: it is the r3 E3-3 fix's own claim, and the pins end up in the wrong place after a common action.
- **Already known?** No. r3 E3-3 asked for the graphics' boxes, and they are filtered out.

### E4-2. Repeat Pin leaves nothing selected; KiCad selects the new pin (low)
**Fixed** on task-r4-editors-exports: a272cd30

- Ours: `symbolEditorStore.tsx:706` (`repeatLastPin`) dispatches `CLEAR_SELECTION` and never selects the pin it added.
- KiCad: `symbol_editor_drawing_tools.cpp::RepeatDrawItem`:
  ```cpp
  m_toolMgr->RunAction( ACTIONS::selectionClear );

  if( pin )
      m_toolMgr->RunAction<EDA_ITEM*>( ACTIONS::selectItem, pin );
  ```
  So after Insert, the new pin is selected and E, Delete or R act on it.
- PARITY-symedit.md:264 calls Repeat Pin "identical".
- Test: e2e in `lib-edit.check.js`, `Insert selects the repeated pin`. After Insert, the selection is the new pin's id.

### Checked and found to match (editors)
- **Footprint Set Anchor (a10c9d08):**
  - `FOOTPRINT::MoveAnchorPosition` with `moveVector = GetPosition() - cursorPos` (a library footprint sits at 0): fields, pads, graphics and texts move, and the model offset does `x += mm(dx)`, `y -= mm(dy)`.
  - The kept zones/dimensions and the courtyard override are documented as differences.
  - Cosmetic: an absent Datasheet/Description field (not written, KiCad gives it a default at the origin) does not move, where KiCad's hidden one would.
- **Move Symbol Anchor:**
  - `LIB_SYMBOL::Move( -cursorPos )` moves the fields with the body (`all_fields` / `set_fields` keep the texts).
  - It is refused on a derived symbol, as KiCad's `EDIT_TOOL` condition (`isEditableCond`: `!IsSymbolAlias()`) disables it.
  - The tool stays active after a click, as `PlaceAnchor` does.
- **Rotate pivot arithmetic (3cd18c9f):** integer IU box, `BOX2I::GetCenter`, `KiROUND` half-grid snap, and the pins' name and number boxes with their own sizes.
- **Repeat Pin (0e6b4f69):**
  - It is found by id (`g_lastPin` is a UUID), repeated as the pin is now, and the template takes the orientation, type and shape (`PlacePin`).
  - The occupied-spot question has the caption, OK label and extended text, all word for word.
- **Push Pin Length / Name Size / Number Size:** `PushPinProperties` (the length only to pins of body style 0 or the shown one) and `SCH_PIN::ChangeLength` per orientation, in our y-up frame.
- **New Symbol:** the dialog's defaults (`dialog_lib_new_symbol_base.cpp`: one unit, the three pin-text boxes checked) as round 2 found.

---

## Part C. Simulator, `${OP}`, 3D viewer, design blocks

### S4-1. A label with no wire and no pin under it shows `?` for `${OP}`; KiCad gives it the net it joins by name (low)
**Fixed** on task-r4-editors-exports: 707351f3

- Ours: `web/studio/src/kicad-port/sim/opVars.ts:92-102` `netAt` finds a label's net only from a wire, a symbol pin or a power symbol at its anchor. With none of them it returns `null`, and the label shows "?".
- KiCad: `sch_label.cpp::SCH_LABEL_BASE::ResolveTextVar` uses `Connection()`, which every label has once the connection graph is built. A label on its own forms a subgraph that is merged with the other subgraphs of the same name (a local label on the same sheet, a global label anywhere). So `VIN ${OP}`, dropped anywhere on a sheet that has a wired `VIN` label, shows VIN's voltage.
- Test: `opVars.test.ts` `a label on nothing takes the net its name joins`. No wires; a label "VIN" at (0,0) and another "VIN" on a wire whose net is "/VIN"; points `{"/vin": 5}`. Assert the first label shows "5V".
- Fix direction: fall back to the label's own driven net name (the server's connectivity for labels), not "?".

### V4-1. The 3D viewer's modifier+wheel is the `aPan = true` branch; KiCad's default is `aPan = false` (medium)
**Fixed** on task-r4-editors-exports: 7efff80d
- **What differs.**
  - Ours: `web/studio/src/kicad-port/camera3d.ts:422-438` `handleWheel` assumes `m_MousewheelPanning` is always true. Its comment (`:399-421`) says it is "hardcoded `true` in board_adapter.cpp's ctor and never exposed".
  - KiCad: that holds only for `BOARD_ADAPTER`'s constructor. `3d-viewer/3d_viewer/eda_3d_viewer_frame.cpp:1022` (`loadCommonSettings`, run when the frame is built) overrides it:
    ```cpp
    m_boardAdapter.m_MousewheelPanning = settings->m_Input.scroll_modifier_zoom != 0;
    ```
    The default is `input.scroll_modifier_zoom` = 0 (`common_settings.cpp:278`), so `EDA_3D_CANVAS::OnMouseWheel` calls `OnMouseWheelCamera( event, false )`.
  - With `aPan` false, `common/gal/hidpi_gl_3D_canvas.cpp::HIDPI_GL_3D_CANVAS::OnMouseWheelCamera` does this:
    ```cpp
    else if( modifiers == m_settings.m_scrollModifierPanV && !aPan )
        m_camera.Pan( SFVEC3F( 0.0f, -delta_move, 0.0f ) );
    else if( modifiers == m_settings.m_scrollModifierPanH && !aPan )
        m_camera.Pan( SFVEC3F( delta_move * horizontalSign, 0.0f, 0.0f ) );
    else
        mouseActivity = m_camera.Zoom( ... > 0 ? 1.1f : 1 / 1.1f );
    ```
    The default modifiers are PanV = Shift and PanH = Ctrl, and `delta_move = 0.7 * zoom` is negated for a negative rotation, not scaled by the wheel.
  - So in KiCad:
    - Ctrl+wheel pans the other way from ours;
    - Alt+wheel zooms, where ours pans;
    - Shift+wheel pans vertically whatever the wheel axis, where ours pans sideways on a horizontal swipe;
    - each step is a fixed `0.7 * zoom`, where ours scales with the wheel delta.
  - On macOS the defaults differ (`common_settings.cpp:537-539`: zoom = Ctrl, so `aPan` is true there). Ours has one mapping for every platform.
- **How to see it.** In the 3D viewer, Ctrl+scroll up moves the board one way in KiCad (Linux, Windows) and the other way here. Alt+scroll zooms in KiCad and pans here.
  - Test: `camera3d.test.ts` `the modifier wheel follows OnMouseWheelCamera with aPan false`. `{deltaY:-100, altKey}` zooms ×1.1; `{deltaY:-100, ctrlKey}` moves the look-at by +0.7·zoom along camera x; `{deltaX:-100, shiftKey}` pans along y.
- **Severity.** Medium (navigation goes the wrong way). PARITY-3d.md:43 says "identical".
- **Already known?** No.

### V4-2. The 3D viewer draws every board 1.6 mm thick with 35 µm copper, and mounts the models from those numbers, whatever the stackup says (medium)
**Fixed** on task-r4-editors-exports: 7efff80d
- **What differs.**
  - Ours:
    - `web/studio/src/components/viewer3d/scene.ts:81,84` have `BOARD_THICKNESS_MM = 1.6` and `COPPER_MM = 0.035`, used for every surface (`surfaceY`, `maskCenterY`, `silkY`).
    - `Viewer3D.tsx:678` calls `partModels.sync(board, {...})` with no `stack`, so `DEFAULT_STACK` (`kicad-port/model3d.ts:127`) gives `footprintZ`.
    - The board's thickness is in the payload but unused: `board_rules.board_thickness_nm` (`api/types.ts:1083`) and the stackup's `thickness_mm` (`types.ts:1295`).
  - KiCad: `3d-viewer/3d_canvas/board_adapter.cpp::BOARD_ADAPTER::InitSettings` takes the stackup when there is one (`if( bds.GetStackupDescriptor().GetCount() )`):
    - it sums the dielectric sublayers and the inner copper into `body_thickness`;
    - it takes the outer copper and mask thicknesses from their items;
    - it sets `m_boardBodyThickness3DU = body_thickness * m_biuTo3Dunits;`.
    - `GetFootprintZPos` uses these.
- **How to see it.** On a 0.8 mm board (Board Setup > Physical Stackup, or a 0.8 mm QA board), KiCad's slab is 0.8 mm and the top models sit at 0.4 + Cu. Ours draws a 1.6 mm slab and floats the models 0.4 mm off each side. kicad-cli's STEP/VRML are right.
  - Test: `model3d.test.ts` `the mounting height follows the stackup`. With 0.035 Cu / 0.71 core / 0.035 Cu, `footprintZ(stack, false) === 0.71 / 2 + 0.035`; and `Viewer3D` passes that stack to `partModels.sync`.
- **Severity.** Medium. PARITY-3d.md:90 and :153 describe the default only.
- **Already known?** No (GAPS 23 does not list it).

### Design blocks and the simulator
No new difference was found; the checked list is below. P18-3 (`design_block_api.rs::board_selection_made` drops every picked group, not only a lone one) is still in main's code; it is listed in `p11-p18.md` and not counted again. Cosmetic, not counted: `sch_clipboard.rs:685` `fragment_as_sheet_file` gives every block saved from a schematic selection the same root `(uuid)` (`duid_plain("design-block:selection-file")`), where KiCad's new `SCH_SCREEN` gets a fresh KIID.

### Checked and found to match (simulator, `${OP}`, 3D, design blocks)
- **`${OP}` (459f1ffe):**
  - a label on a pin or mid-wire, and sheet pins (`SCH_SHEET_PIN` is an `SCH_HIERLABEL`);
  - an escaped `\${OP}` is kept;
  - `${OP:<pin>}` before a run is "--";
  - the two-pin test counts hidden pins (`GetPins( &sheetPath )`).
- **3D and model paths:** the T/S/V/P/D hotkeys without Shift; a trailing `$`, `${` or `$(` is dropped (`KIwxExpandEnvVars`).
- **Design blocks:**
  - `DESIGN_BLOCK_IO::DesignBlockSave`: the refusal order and messages word for word, no copy onto itself, the metadata write, and other files left alone;
  - `SaveBoardAsDesignBlock` / `UpdateDesignBlockFromBoard`: "Design block '%s' does not exist." and `checkOverwriteDbLayout`'s "Design block '%s' already has a layout." / "Overwrite existing layout?";
  - `SaveSheetAsDesignBlock`: "Design blocks with nested sheets are not supported.";
  - `saveSelectionToDesignBlock`: footprint nets through their pads, and groups cloned.
- **Simulator:**
  - `CommandToSimType`, `ParseDCCommand`, `ParseNoiseCommand`, `ParsePZCommand`;
  - `DIALOG_SIM_COMMAND::TransferDataFromWindow` for every page: .tran's optional fields and max step, .sp's "1", .noise's `,GetItemName(ref)`, the messages "A frequency range must be specified." and "Source 1 and Source 2 must be different.", FFT's `linearize`/`fft`;
  - the compatibility modes 0-5 (LT_PSPICE default);
  - `NGSPICE::LoadNetlist`'s `unset ngbehavior` and the `esave none` lines;
  - `SaveWorkbook` / `loadJsonWorkbook`: version 7, the keys, `setw(2)` and sorted keys.
  - `crates/sim/src/netlist.rs` re-applies `WriteDirectives` over kicad-cli's SPICE netlist. kicad-cli honours only `OPTION_SIM_COMMAND`, so this is needed and not a duplicate.
- **3D:**
  - every default hotkey of `eda_3d_actions.cpp`;
  - rotation step 10°, animation on, speed ×3;
  - zoom limits 0.02 / 2.0, wheel zoom ×1.1, pan step factor 0.7.

---

## Part D. Footprint Editor

These are rows of `PARITY-fpedit.md` that no earlier round reviewed (p1-p2, p7-p8, r2 and r3 skipped). Every finding here comes from reading the code; E4-F1 and E4-F6 were checked a second time against the code.

### E4-F1. The drawing tools use one 0.15 mm line width on every layer, and a polygon is always filled (medium)
**Fixed** on task-r4-editors-exports: 29387c25 (the layer class's width, polygons unfilled on Edge.Cuts and courtyards; the layer box now lists every layer, it had F.SilkS/F.Fab/F.CrtYd only)
- **What differs.**
  - Ours:
    - `web/studio/src/components/footprint/FootprintCanvas.tsx:36` is `const DRAW_STROKE_WIDTH_NM = 150_000;`.
    - `:137-145` give `stroke_width: 150_000` to the segment, rect, circle, arc and polygon. The polygon is `filled: true` on any layer.
  - KiCad:
    - The tools take the layer class's width: `drawing_tool.cpp` does `m_stroke.SetWidth( bds.GetLineThickness( m_layer ) )` (lines 317, 2372, 2506, 2796, 2941), and `GetLineThickness` is `m_LineThickness[ GetLayerClass( aLayer ) ]`.
    - The Footprint Editor's defaults (`footprint_editor_settings.cpp:208-274`) are silk 0.1, copper 0.2, edges 0.05, courtyard 0.05, fab 0.1, others 0.1.
    - A graphic polygon (`zone_create_helper.cpp:244`) gets `poly->SetFilled( layer != Edge_Cuts && layer != F_CrtYd && layer != B_CrtYd );` with the layer's line thickness.
  - `PARITY-fpedit.md:62,65` say "identical".
- **How to see it.** Draw a rectangle and a polygon on F.CrtYd and export the `.kicad_mod`. Ours writes `(width 0.15)` and the polygon `(fill yes)`; KiCad writes `(width 0.05)` and `(fill no)`. A filled courtyard polygon is also read differently by the courtyard checks.
  - Test: `fpDrawDefaults.test.ts` `a drawn shape takes its layer class's width`. F.CrtYd 0.05, F.SilkS 0.1, F.Fab 0.1, F.Cu 0.2, Edge.Cuts 0.05, Dwgs.User 0.1; a polygon on F.CrtYd or Edge.Cuts is unfilled.
- **Already known?** No.

### E4-F2. The text tool places "TEXT" with a fixed size, pen and centring, opens no dialog, and leaves the tool (medium)
**Fixed** on task-r4-editors-exports: 29387c25 (Footprint Text Properties, carry, layer defaults, left, mirrored on back layers, tool stays; bottom justification not kept: the text model has no vertical justification)
- **What differs.**
  - Ours: `FootprintCanvas.tsx:501-503` calls `api.addText({ content: "TEXT", ..., size_nm: 1_000_000, stroke_width: 150_000, justify: "center", mirror: false })`, then `SET_ACTIVE_TOOL select`.
  - KiCad: `drawing_tool.cpp::DRAWING_TOOL::PlaceText`:
    ```cpp
    textAttrs.m_Size = bds.GetTextSize( layer );
    textAttrs.m_StrokeWidth = bds.GetTextThickness( layer );
    textAttrs.m_Italic = bds.GetTextItalic( layer );
    textAttrs.m_KeepUpright = bds.GetTextUpright( layer );
    textAttrs.m_Mirrored = m_board->IsBackLayer( layer );
    textAttrs.m_Halign = GR_TEXT_H_ALIGN_LEFT;
    textAttrs.m_Valign = GR_TEXT_V_ALIGN_BOTTOM;
    ```
    `DIALOG_TEXT_PROPERTIES` then asks for the text, and an empty text is dropped. The text is carried to a second click and committed as "Draw Text", and the tool stays armed.
  - What differs in the defaults:
    - the silk pen is 0.1 mm (ours 0.15);
    - copper text is 1.5 mm with a 0.3 mm pen;
    - KiCad's text is left/bottom justified, not centred;
    - text on a back layer is mirrored.
  - `PARITY-fpedit.md:64` says "Text | identical".
- **How to see it.** Arm Text on B.SilkS and click. Ours places "TEXT", centred, unmirrored, with a 0.15 mm pen. KiCad opens the dialog, then places left/bottom justified, mirrored text with a 0.1 mm pen, and stays in the tool.
  - Test: `fpTextTool.test.ts` `a placed text takes the layer's defaults`. Layer B.SilkS gives size 1 mm, pen 0.1 mm, left/bottom, mirrored.
- **Already known?** In part: GAPS 19 says the Footprint Editor's texts have no Properties dialog. The defaults, justification, mirroring and the tool exit are not listed.

### E4-F3. The pad tool places at once: no pad is carried, so R or F cannot turn or flip it first (low)
**Fixed** on task-r4-editors-exports: 29387c25 (carried pad, R / Shift+R / F before the click, the master takes the turn; a padstack pad with other copper shapes is not flipped)
- Ours: `FootprintCanvas.tsx:453-460` adds the master pad at the click. With the pad tool armed, R rotates the selection (`editorFrameActions.ts:90`).
- KiCad: `pad_tool.cpp::PAD_TOOL::PlacePad` runs `doInteractiveItemPlacement( aEvent, &placer, _( "Place pad" ), IPO_REPEAT | IPO_SINGLE_CLICK | IPO_ROTATE | IPO_FLIP )`. The pad follows the cursor and R or F turns or flips it. `PlaceItem` then does `m_Pad_Master->ImportSettingsFrom( *pad )`, so the next pad is turned too.
- Test (e2e): `pad tool: R before the click turns the placed pad and the master`.
- `PARITY-fpedit.md:133` says "done".

### E4-F4. The Footprint Editor's Footprint Properties uses the board dialog's text-size messages (low)
**Fixed** on task-r4-editors-exports: 29387c25
- Ours: `web/studio/src/kicad-port/fpFields.ts:163-173` `checkFieldRow` serves both dialogs (`fpLibFields.ts:68`, `FootprintPropertiesDialog.tsx:143`). Its messages are "Text width must be at least %s." / "... at most %s." and "Text thickness is too large for the text size.\nIt will be clamped at %s.", from `dialog_footprint_properties.cpp`, the board's dialog.
- KiCad's library dialog, `dialog_footprint_properties_fp_editor.cpp::DIALOG_FOOTPRINT_PROPERTIES_FP_EDITOR::Validate`, says:
  - `_( "The text width must be between %s and %s." )`
  - `_( "The text height must be between %s and %s." )`
  - `_( "The text thickness is too large for the text size.\n" "It will be clamped." )`
- Test: `fpLibFields.test.ts` `the library dialog's size message is the fp editor's`. A width of 0 gives "The text width must be between 0.001 mm and 1000 mm.".

### E4-F5. Push Pad Properties' "same type" filter does not tell an SMD pad from a connector or aperture pad (low)
- Ours: `crates/ops/src/lib.rs:5837` `if filter_type && p.kind != src.kind { continue; }`. An edge-connector pad is `PadKind::Smd` with `kicad.connector = true` (`footprint_import.rs:331,380`), so it passes as an SMD pad.
- KiCad: `pad_tool.cpp::doPushPadProperties`:
  ```cpp
  if( aPadTypeFilter && ( pad->GetAttribute() != aSrcPad.GetAttribute() ) )
      continue;
  // Special-case for aperture pads
  if( aPadTypeFilter && pad->GetAttribute() == PAD_ATTRIB::CONN )
  {
      if( pad->IsAperturePad() != aSrcPad.IsAperturePad() )
          continue;
  }
  ```
- Test: `crates/ops/src/tests.rs::push_pad_type_filter_keeps_connector_pads_apart`. Push from an SMD pad with every filter on; the `connect` pad of the same shape keeps its size and `kicad.connector`.

### E4-F6. Paste Footprint gives the copy's Value the new name; KiCad keeps the old Value (low)
**Fixed** on task-r4-editors-exports: 29387c25
- Ours: `web/studio/src/actions/footprintLibraryOps.ts:89-90` stores `{ ...copied, name }`. A Value with no stored text shows the footprint's name (`footprint_import.rs:174` stores a text only when it differs from the name), so "X_copy" shows Value "X_copy".
- KiCad: `footprint_editor_control.cpp::FOOTPRINT_EDITOR_CONTROL::PasteFootprint` only renames:
  ```cpp
  while( PROJECT_PCB::FootprintLibAdapter( &m_frame->Prj() )->FootprintExists( newLib, newName ) )
      newName += _( "_copy" );

  m_copiedFootprint->SetFPID( LIB_ID( newLib, newName ) );
  ```
  So the Value stays "X". (Duplicate does update the Value, `DuplicateFootprint`: `if( aFootprint->GetValue() == footprintName ) aFootprint->SetValue( newName );`, and ours matches that.)
- Test: paste X into its own library and export X_copy. Assert `(property "Value" "X")`.

### E4-F7. Renumber Pads: the dialog's labels, limits and the popup are not KiCad's (low)
**Fixed** on task-r4-editors-exports: 29387c25
- Ours:
  - `web/studio/src/components/footprint/FootprintEditorView.tsx:26-70` has the labels "Prefix", "Start number" and "Number step", no limit on the prefix, any number (a negative start is taken), and the hint "Then click the pads in the order they should be numbered.".
  - `kicad-port/padEnumeration.ts:53-55` shows the popup "Click on pad %s -- Esc cancels all, double-click finishes".
- KiCad:
  - `dialog_enum_pads_base.cpp` has `_("Pad names are restricted to 4 characters (including number).")`, `_("Pad name prefix:")`, `_("First pad number:")` and `_("Numbering step:")`, with `m_padPrefix->SetMaxLength( 4 )` and spin controls `( .., 0, 999, 1 )`.
  - `pad_tool.cpp::EnumeratePads` shows `_( "Click on pad %s\nPress <esc> to cancel all; double-click to finish" )`.
- Test: `enumeratePopupText(...)` of start 1 equals `"Click on pad 1\nPress <esc> to cancel all; double-click to finish"`.

### Checked and found to match (Footprint Editor)
- **Pad numbering:** `GetNextPadNumber`, `GetRefDesPrefix` and `GetTrailingInt`; the tool's number starts at "1".
- **The master pad:** `SetDefaultMasterPad` (2.54 × 1.27 round-rect at 0.15, 0.8 mm drill, PTH on `*.Cu` `*.Mask`); `PlaceItem` feeds the placed pad back into the master; `CanHaveNumber`.
- **New Footprint:** "Untitled" / `Untitled_%d`, with the attributes of the library's last footprint, else SMD.
- **Rename Footprint:** "Footprint must have a name.", "Footprint '%s' already exists in library '%s'." with Confirmation/Overwrite; the same name is a no-op; the Value follows only when it equalled the old name.
- **Save Footprint As:** "A library must be specified." before "Footprint must have a name."; "Footprint %s already exists in %s." with Overwrite; `updateValue`.
- **Duplicate Footprint:** the `_%d` suffixes and the Value. The name rules are `StringLibNameInvalidChars` `%$<>\t\n\r"\/:`.
- **Repair Footprint:** "%d potential problems repaired." / "%d duplicate IDs replaced." / "No footprint problems found.".
- **Pad Table:** the `PIN_NUMBERS` summary and duplicates ("none").
- **Push Pad Properties:** the shape, orientation and layer filters, all on by default, and `ImportSettingsFrom`'s fix-ups.
- **Create Array in the Footprint Editor:** `ARRAY_PAD_NUMBER_PROVIDER`, the order of points and numbers, and no duplicated fields.
- **Renumber Pads:** the state machine; the 0.1 mm sweep with `unique()`; "Renumber Pads" on a double click or a tool change, and Escape reverts; refused with no pads or while a pad is edited; `s_lastUsedParams`.
- **Footprint Checker:** it runs kicad-cli DRC on a scratch board (`crates/kicad-engine/src/footprint_check.rs`), so no active code duplicates kicad-cli. Its known gaps are round 1's P11-2 and P11-3.

---

## Summary

| ID | Area | What | Severity | Known? |
|---|---|---|---|---|
| X4-1 | pictures.rs | The judge's board PNG paints B.Cu, B.SilkS and the courtyards over F.Cu (`getPlotSequence` puts the `--layers` entry first); confirmed with kicad-cli | medium | no |
| X4-2 | Schematic Plot | "Current page" passes the sheet's place in the stored list as `--pages`; kicad-cli matches page numbers, so another sheet is plotted; confirmed on the studio | medium | no |
| X4-3 | Plot / Drill dialogs | `set_plot_settings` is an undo step; KiCad's `applyPlotSettings` / `updateConfig` only `OnModify()` | low | no |
| X4-4 | IPC-2581 | BOM columns are free text, not kept in the project, no distributor guess; PARITY says identical | low | no |
| E4-1 | Symbol Editor Rotate | The pivot drops imported and unit-specific graphics (`symbolItemBoxes(sym, 0, 0)`) and counts texts, which `SELECTION::GetCenter` skips | medium | no (r3 E3-3's fix) |
| E4-2 | Repeat Pin | The new pin is not selected (`RepeatDrawItem` selects it) | low | no |
| S4-1 | `${OP}` | A label on no wire or pin shows "?"; KiCad gives it the net its name joins | low | no |
| V4-1 | 3D viewer | The modifier+wheel is the `aPan = true` branch; KiCad's default is false (Ctrl pans the other way, Alt zooms, Shift pans along y) | medium | no |
| V4-2 | 3D viewer | The slab is always 1.6 mm with 35 µm copper, and the models are mounted from that, ignoring the stackup | medium | no |
| E4-F1 | Footprint Editor | Drawn shapes are 0.15 mm on every layer and a polygon is always filled (KiCad: the layer class's width, unfilled on Edge.Cuts and courtyards) | medium | no |
| E4-F2 | Footprint Editor | The text tool places "TEXT" centred with a fixed pen, no dialog, no mirroring on back layers, and leaves the tool | medium | in part (GAPS 19) |
| E4-F3 | Footprint Editor | The pad tool does not carry the pad (no R/F before the click) | low | no |
| E4-F4 | Footprint Editor | Footprint Properties uses the board dialog's text-size messages | low | no |
| E4-F5 | Footprint Editor | Push Pad Properties' type filter does not tell SMD from connector or aperture pads | low | no |
| E4-F6 | Footprint Editor | Paste Footprint renames the Value | low | no |
| E4-F7 | Footprint Editor | Renumber Pads labels, limits and popup text | low | no |

**Counts:** high 0, medium 7, low 9 (16 findings).

**Matches confirmed (not to review again):** the round-3 fixes X3-1..X3-12, E3-1, E3-2, E3-4, E3-5, S3-1..S3-4, V3-1 and R3-1. E3-3's arithmetic is right, but its input is not (E4-1). Also confirmed: the plot params whitelist, `Cmd::SetPlotSettings`' selection rule, the nanosvg switch, the STEP warning, the ODB++ and IPC-2581 defaults, the design-block save/update messages, the simulator command builders and workbook, and the Footprint Editor's library operations listed in Part D.
