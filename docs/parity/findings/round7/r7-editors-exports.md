# Round 7 findings: editors, 3D, design blocks, simulator, fab outputs and exports (r7-editors-exports)

**Reviewer:** r7-editors-exports. A findings-only review, done on 2026-10-28. No code was changed.

**Versions.**
- Our code: main's checkout at `f057b4ea`, read in `/home/team_braneboard_com/work/agentic-eda`.
- KiCad: `kicad-src-8303b2ad`. `kicad-src-25ec2335` was used for the drill writers, `pcbnew/api` and `api/proto`, and where a finding says so.

**What was run.** Nothing was built or run: no `eda`, no kicad-cli and no studio server, so ports 8848-8849 were not used. Scratch notes are in `/tmp/findings-r7-editors-exports.8jRo/`. Every finding comes from reading our code beside the C++.

**Scope.**
- (a) The round-6 merges, each checked against the C++:
  - `fd36a62b`: the simulator dialogs, `SIM_VALUE::ToDouble` / `Normalize` and the SI grammar;
  - `05e21200`: the 3D pan step and the preview's wheel;
  - `eba680ce`: the 3D defaults, the stackup colours, the default colours, the row labels;
  - `3f78d6ec` and `82171e81`: footprint fields in 3D, and paste from the pad's paste layer and margin;
  - `227b5bf9`, `fffafcd5`, `12e30265`, `a753ed4a`: the design-block write checks and dialogs;
  - `08558335`: the IPC-D-356 message;
  - `892d03f3`: the order of the served drill log;
  - `f5279e41`: the 3D hotkey modifiers.
- (b) The unported Simulation Model dialog (GAPS 34): how big a port is, and what a user loses without it.
- (c) Rows that no round had reviewed:
  - the Symbol Editor: Pin Properties, the Pin Table, library Symbol Properties, library-tree operations;
  - the Footprint Editor: its own Pad Properties dialog, the Pad Table, library Footprint Properties attributes, Default Pad Properties;
  - PARITY-boardctl: Repair Board, Export Footprints, zone priority and the ratsnest / highlight rows (Part F).
- (d) The convergence summary.

I re-read the high findings' key C++ lines myself. Two helper passes read the Symbol Editor, the Footprint Editor, the board-control rows and the Simulation Model scope. I spot-checked their findings in the code: E7-S1's file angle (`parseSymbolPin`) against `pinOrientation.ts`, E7-S5's `edit_symbol_properties`, E7-F2's type select, and R7-SM-2's `OwnerCtx::new` refusal.

**Finding IDs.**
- V7: 3D.
- R7-SM: Tune and the simulation fields.
- E7-S: Symbol Editor.
- E7-F: Footprint Editor.
- C7: board control.
- X7: exports.

---

## Part A. The round-6 merges

All 23 round-6 fixes are on main. 21 of them are right against the C++. One fix (V6-5) left an edge case (V7-1), and one (B6-1) differs from `access` in a way that does not matter (noted under "Checked and found to match").

### V7-1. 3D solder paste: a pad with no copper (a paste-only aperture pad) is grown by the footprint's or the board's paste margin; KiCad gives it none (low; the edge case of the V6-5 fix)
- **What differs.**
  - Ours: `web/studio/src/kicad-port/footprint3d.ts:44-52` (`padPaste3d`) always takes `pad.paste_margin ?? part.paste_margin ?? board.paste_margin_nm` and the ratio the same way.
  - KiCad (`pcbnew/pad.cpp::PAD::GetSolderPasteMargin`, 8303b2ad:1744-1748) returns zero first:
    ```cpp
    // Pads defined only on mask layers (and perhaps on other tech layers) use the shape
    // defined by the pad settings only.  ALL other pads, even those that don't actually have
    // any copper (such as NPTH pads with holes the same size as the pad) get paste expansion.
    if( ( m_padStack.LayerSet() & LSET::AllCuMask() ).none() )
        return VECTOR2I( 0, 0 );
    ```
  - `BOARD_ADAPTER::addPads` (`create_3Dgraphic_brd_items.cpp:554-557`) adds that margin for F_Paste / B_Paste. Since 82171e81 the state sends `pads[].layers`, so `padPaste3d` can see a pad that is on `F.Paste` only (`written_pad_layers` gives `smd_layers` as the file had them).
- **How to see it.** Many QFN / DFN library footprints split the exposed pad's paste into pads on `F.Paste` only. Set Board Setup's paste clearance to -0.05 mm. KiCad draws those windows at their own size, and ours draws each 0.1 mm smaller.
  - Test (`footprint3d.test.ts`): `"a pad with no copper layer gets no paste margin (GetSolderPasteMargin)"`. For `padPaste3d({layers:["F.Paste"], w:1e6, h:1e6, size:[1e6,1e6], ...}, {}, {paste_margin_nm:-50000})`, assert `{w:1e6, h:1e6}`.
- **Severity.** Low (the 3D picture only; the Gerbers are kicad-cli's).
- **Already known?** No. The V6-5 note lists the board-file pad margin and custom rules, not this.
- **Left (low)** on task-r7-sym-editor: GAPS.md item 23, PARITY-3d.md's paste row.

### Checked and found to match (round-6 merges)
- **S6-2 / SIM_VALUE.**
  - `simValueToDouble`'s regex against `SIM_VALUE_GRAMMAR::numberGrammar<TYPE_FLOAT, SI>` (`sim_value.h:100-170`): the six significand forms in PEG order, `exponent : seq<opt<sign>, opt<digits>>`, the 13 SI prefixes, `must<..., eof>`. The regex backtracks and PEG does not, but I found no input where that changes the result: "1E" and "1e-" are NaN on both sides (`std::stoi("")` throws `invalid_argument`, caught in `Parse`); "-.5" fails on both; "." and "-" are 0.
  - `UnitPrefixToExponent` / `ExponentToUnitPrefix` (SI) table for table.
  - `Normalize`, with `log10(0) = -inf` giving "0".
  - `ToDouble`'s `aDefault = NaN`.
  - `SetPlotSettings`: only a named axis is shown; `!std::isnan` leaves the field empty.
  - `GetAxisMinMax` (`mathplot.h:692-706`); `ApplySettings`' order (scales, `LockY` of the OR, grid, legend, dotted, margins through `TO_INT`, `AdjustLimitedView`).
  - Cosmetic, not counted: `stoll` overflow of an int or fraction part of 19 digits or more makes KiCad's parse fail (NaN); ours reads the number.
- **S6-1.** `TransferDataToWindow` (`dialog_sim_command.cpp:153`) runs after `SetSimOptions`; `ApplySettings` (`:506`) clears the bit when unchecked.
- **S6-3.** `WX_GRID::OnDeleteRows` (`wx_grid.cpp:753-815`): the selected rows, else the cursor's row; `nextSelRow = selectedRows.back() - 1`; deleted in reverse order. `OnAddRow` moves the cursor to the new row.
- **S6-4.** `parseCommand` (`:693-726`): `IsSameAs( "TEMP", false )` for the type, `updateDCSources( src.GetChar( 0 ) )` compared case-sensitively, then `SetStringSelection` (case-insensitive `FindString`; a miss keeps `prevSelection`, "TEMP" under 'T'). `evaluateDCControls`' "A DC source must be specified.". `ParseDCCommand` does not change case.
- **S6-5.** `EDA_PATTERN_MATCH_REGEX`'s explicit forms, the wildcard, then the substring.
- **S6-6.** The node choices start blank. The removed `ensureThirdYAxis` call had no counterpart in `ApplySettings`.
- **V6-1 / V6-2.**
  - `aPan` is passed separately (`OnMouseWheelCamera( event, aPan )`).
  - `mmPer3DUnit = initialDistanceMm / 16` equals `boardSpan / 12.8` (`RANGE_SCALE_3D * 1.6`, `board_adapter.cpp:381-385`), the board's longest side being x/y only.
  - Cosmetic, not counted: one DOM wheel notch is a rotation of 100, where wx's is 120, so the `aPan` step is 5/6 of KiCad's (the comment says so).
- **V6-3.** `EDA_3D_VIEWER_FRAME::LoadSettings` (`:571-585`) and `GetVisibleLayers`' `FOLLOW_PLOT_SETTINGS` branch (`board_adapter.cpp:906-932`), row for row. `PCB_PLOT_PARAMS`' default layer selection (`pcb_plot_params.cpp:102`). `GetLayerColors` with no preset found takes `DEFAULT_THEME`'s colours, which is what ours does.
  - Left, documented in PARITY-3d: "Plot on All Layers" and `plotreference` are not sent to the 3D viewer. Both are already in `PlotParams` (`plot_on_all_layers`, `flag("plotreference")`) beside `plot_params_json` (`crates/cli/src/studio.rs:1570`), so this is cheap to close.
- **V6-6, V6-7, V6-9.**
  - `findColor`'s `KIGFX::COLOR4D()`.
  - `GetDefaultColors` against `g_Default*` (`board_adapter.cpp:209-219`).
  - `rebuildLayers`: User.1..45 only when enabled; `Map3DLayerToPCBLayer` (`layer_id.cpp:271`) for the labels, with paste and adhesive unmapped as ours; the `showBBoxes` tooltip.
- **V6-4.** `addFootprintShapes`' field loop (`create_3Dgraphic_brd_items.cpp:209-224`): `LAYER_FP_TEXT`, Reference/Value gating, `GetLayer() == aLayerId && IsVisible()`. Footprint texts are not tested for visibility, and ours does not test them either.
- **V6-8.** `eda_3d_actions.cpp` and `actions.cpp:704-750`.
  - Cosmetic, not counted: on macOS KiCad binds only Cmd+'+', Cmd+'-', Cmd+0 and Cmd+R, and ours also takes bare F1/F2/Home/F5 there.
- **B6-1.** `IsLibraryWritable` / `IsDirWritable`.
  - Cosmetic, not counted: `access()` tests the *real* uid/gid, and `Credentials` reads the effective ones (the second number in `/proc/self/status`). They differ only in a setuid process.
- **B6-2.** `SaveSheetAsDesignBlock` (`sch_design_block_utils.cpp:97-99`) and `topSheet->SetName( _( "Root" ) )` (`files-io.cpp:361`). In 8303b2ad this name is given only to a single-root (legacy) file; a multi-root file's top sheets keep their own names, which the studio never writes.
- **B6-3.** `LIB_TREE_NODE_LIBRARY` (`lib_tree_model.cpp:262-313`): the name term at weight 8; childless `m_Score = 1` plus each term, 0 if any term scores 0. A history with no hits is hidden.
- **B6-4.** `createNewDesignBlockLibrary` (`design_block_pane.cpp:150-220`): the read-only and already-exists messages, Confirmation / Overwrite, `DeleteLibrary` then `CreateLibrary`.
  - Cosmetic, not counted: KiCad shows the read-only refusal in the info bar and its KIDIALOG has "Do not show again"; ours shows it in the dialog and has no checkbox.
- **B6-5.** `DIALOG_EDIT_LIBRARY_TABLES` titles, Yes/No, `GenerateInfo`.
- **X6-1.** `GenD356File`'s text; the path is made absolute.
- **X6-2.** 25ec2335 `gendrill_excellon_writer.cpp:95-175, 670-718`:
  - "Created file '%s'" (no full stop) for the drill file when it is opened;
  - then `writeBackdrillLayerPairFile`'s own report and `AddCreatedFile`;
  - then `AddCreatedFile( fullFilename )`.
  - The swap rule, by the `-backdrill` stem, cannot hit a board whose own name ends in "-backdrill", because every drill file name has a suffix after it.

---

## Part B. The Simulation Model dialog (GAPS 34): scope and what is lost

### Size of a faithful port (wc -l, 8303b2ad)
| Tier | Files | Lines |
|---|---|---|
| Dialog | `dialog_sim_model.cpp/.h`, `_base.cpp/.h` | 2,515 |
| T1 core: the type registry, fields read and written | `sim_model.*`, `sim_model_serializer.*`, `sim_property.*`, `sim_value.*` | 3,978 |
| T1 device models | ideal, r_pot, l_mutual, behavioral, source (1,413), switch, tline, ngspice, xspice, spice, raw_spice, spice_fallback | 3,750 |
| T1 ngspice parameter tables | `sim_model_ngspice_data_*.cpp` (17 files) | 8,504 |
| T2 library models | `sim_lib_mgr.*`, `sim_library*`, `spice_library_parser.*`, `spice_model_parser.*`, `spice_grammar.h`, `sim_model_subckt.*`, `sim_xspice_parser.h` | 2,156 |
| T3 IBIS | `sim/kibis/*` (6,307), `sim_library_ibis.*`, `sim_model_ibis.*` | 7,045 |
| **Total** | | **about 27,950** |

- **Doc correction.** GAPS 34 and PARITY-sch 49 say "about 14,700 lines". The measured figure is about 28k, or 25.4k without the dialog. The parameter tables alone are 8.5k lines, and kibis is 6.3k.
- **What the dialog itself splits into.** About 500 of its 1,802 lines are for library and IBIS models only (`loadLibrary` :836-925, the path / filter / model-name handlers :1238-1450, the IBIS handlers :1450-1516). About 1,300 are the built-in UI: the Device / Type choice, the parameter grid, the Code tab and the pin grid.
- **What a port would not need.** `SIM_MODEL::MigrateSimModel` (about 560 lines) and `InferSimModel` (about 330). kicad-cli does both when it writes the netlist (below).
- **Neither kicad-cli nor the API can do it.**
  - 25ec2335's `api/proto` has no sim-model message, only `exclude_from_sim`, `ERCET_SIMULATION_MODEL` and the SPICE netlist formats.
  - `kicad/cli` has only `sch export netlist --format spice|spicemodel`.
  - kicad-cli can still serve as the dialog's validator and its Code-tab preview: the spice netlist runs `SIM_LIB_MGR::CreateModel`, and `crates/sim/src/netlist.rs` already parses an item's line from it.

### What we already have to build on
- `kicad-port/sim/spiceValue.ts`: `SIM_VALUE`, SI notation.
- `crates/sim/src/value.rs`: `SPICE_VALUE`.
- `crates/sim/src/netlist.rs:250`: `parse_sim_pins`.
- `ConstraintModel::symbol_sim_fields`: the library's `Sim.*` fields, `extends` followed.
- `schFieldEdit.ts`: the undoable field verbs; OK would be one batch of them.

Nothing ports the type registry, the parameter tables, the serializer grammar or a library parser.

### Recommended order
1. **"T2-lite", about 1-1.5k lines, the most value.**
   - Read a `.lib` / `.sub` and list its `.subckt` and `.model` names (`spice_library_parser.cpp`, the head-line part of `spice_grammar.h`).
   - Show the subckt's port names (`sim_model_subckt.cpp`).
   - A pin grid.
   - Write `Sim.Library`, `Sim.Name` and `Sim.Pins`: `GeneratePins` (`sim_model_serializer.cpp` ~118-152) writes `"<symbolPin>=<modelPin>"` pairs separated by spaces and sorted with `StrNumCmp`.
   - `Sim.Device` is not needed when `Sim.Library` is set (`SIM_LIB_MGR::CreateModel` :284-294).
   - **Done** on task-r7-sym-editor: f3ea2d71 (the library reader and the routes), 03a9ab27 (the dialog), db35c853; PARITY-sch.md section 51, GAPS.md item 34 (what is still missing).
2. **The built-in sources' waveform parameters** (`SIM_MODEL_SOURCE` plus the core).
3. **The semiconductor models.** Generate the parameter tables as JSON with a script rather than porting them by hand.
4. **IBIS.**

### What a user loses today, ranked
1. **Vendor `.lib` / `.sub` models (op-amps, regulators, MOSFET subcircuits): high.**
   - `Sim.Library`, `Sim.Name` and `Sim.Pins` must be typed by hand, with no model list, no port names and no check before ngspice runs.
   - On a studio-drawn sheet they can be typed with Add Field (`SymbolPropertiesDialog.tsx:261-273`).
   - On a sheet read from a KiCad file it cannot be done at all. Add Field is hidden there, and `edit_field` is refused (`crates/ops/src/sch_fields.rs:241-243`).
   - KiCad's Symbol Properties has the "Simulation Model..." button (`dialog_symbol_properties.cpp:586`, `OnEditSpiceModel`, hidden for power symbols); ours has no such button.
2. **Built-in D / BJT / MOS / switch models with parameters: medium.**
   - `Sim.Device`, `Sim.Type`, `Sim.Params` and `Sim.Pins` must be typed with no list of parameter names.
   - A typing mistake shows only at netlist time: "Unknown simulation model parameter" (`sim_model.cpp:822-828`).
3. **Editing a library source's waveform (VSIN, VPULSE): medium; known (GAPS: "the fields table has no simulation fields").** The whole `Sim.Params` string has to be added as an override.
4. **Passive R/L/C and DC V/I sources: nothing lost.** kicad-cli's netlist runs `SIM_MODEL::InferSimModel` (`sim_lib_mgr.cpp:252-266`) from the reference prefix and the Value ("4k7", "DC 5", Meg, behavioural `=`), and `Simulation_SPICE:VDC` goes the same way.
5. **KiCad 6 `Spice_*` fields: low.** kicad-cli migrates them on load (`EESCHEMA_HELPERS::LoadSchematic` → `SCH_SCREEN::MigrateSimModels` → `SIM_MODEL::MigrateSimModel`), so old files still simulate. The only loss is that the file on disk is never migrated.
6. **Smaller:** "Save parameter in Value field", the Code tab, Not Connected pins, IBIS.

### R7-SM-1. Tune's Save on an inferred passive or DC source writes only Value; KiCad's `WriteFields` also adds hidden `Sim.Device`, `Sim.Type` and `Sim.Pins` (low)
- **What differs.**
  - Ours: `web/studio/src/state/simulator.ts:641-648` (`saveTunerValue`) sends one `edit_field` on `fld:<ref>:Value`.
  - KiCad: `SIMULATOR_FRAME_UI::UpdateTunerValue` (`simulator_frame_ui.cpp:1695-1740`) does `mgr.CreateModel(...)` (inferred, `SetIsStoredInValue( true )`), then `model.SetParamValue(...)`, then `model.WriteFields( symbol->GetFields() )`.
  - `WriteFields` (`sim_model.cpp:449-482`) sets `Sim.Device` (`GenerateDevice`), `Sim.Type`, `Sim.Pins` (`GeneratePins`), `Sim.Params`, and Value when the model is stored in Value. `SCH_FIELD`'s `SetFieldValue` adds a missing field hidden and removes one left empty.
  - For R1 "10k" KiCad writes hidden `Sim.Device "R"` and `Sim.Pins "1=+ 2=-"`; a DC source also gets `Sim.Type "DC"`.
- **How to see it.** Tune R1, Save, and diff the `.kicad_sch`: KiCad adds two hidden properties, ours none. The netlist is the same.
  - Test: `saveTunerValue({param:"r", text:"15k", ref:"R1"}, {})` on a studio sheet leaves Value "15k" and hidden `Sim.Device=R` and `Sim.Pins="1=+ 2=-"`, in one undo step.
- **Also, cosmetic.** When the value is held in `Sim.Params`, ours edits that one token in place. KiCad regenerates the whole string in the model's parameter order (`GenerateParams`), so the netlist is the same but the text can differ.
- **Severity.** Low (the file differs from KiCad's; the simulation is the same).
- **Already known?** No.
- **Left (low)** on task-r7-sym-editor: GAPS.md item 34.

### R7-SM-2. Tune's Save always fails on a schematic read from a KiCad file (medium)
- **What differs.**
  - Ours: `fieldsOf` (`components/simulator/SimSidePanel.tsx:296-300`) is null for an imported sheet, so Save goes to the Value branch. `edit_field` is then refused, because `OwnerCtx::new` returns `None` when `imported_from_kicad` (`crates/engine/src/fields_edit.rs:65-67`). The error is "no field with this id on the sheet (a schematic read from a KiCad file has no places for its fields yet)" (`crates/ops/src/sch_fields.rs:241-243`), shown as an error toast.
  - KiCad writes the value through `UpdateTunerValue`.
  - Once imported fields become editable, there is a second problem: with `fieldsOf` null, a value held in `Sim.Params` would still be written to Value, which `ParseParams` then ignores (`sim_model.cpp:442-445`).
- **How to see it.** Open a KiCad project, run a transient analysis, tune R1, press Save: an error toast, and the file is unchanged.
  - Test: an imported fixture, then Save changes R1's Value (or its `Sim.Params`).
  - Until it is fixed, PARITY-sch 38.1 should say that Save is refused on an imported sheet. Today it says "wired" with no caveat.
- **Severity.** Medium (an action that does nothing on the main use case: tuning an existing KiCad project).
- **Already known?** In part. GAPS item 12 says that the fields of a symbol read from a KiCad file are not items. It does not say that Tune's Save fails because of it.
- **Fixed** on task-r7-sym-editor: 03a9ab27 (Save through `set_symbol_fields`, the fields read from `/api/sim/model/symbol`), db35c853 (the derived schematic read); PARITY-sch.md 38 and 51.

### R7-SM-3. On a studio sheet, Save does not see a tuned parameter that is only in the library's `Sim.Params` (low)
- **What differs.** `fieldsOf` sees only `user_fields`. For a custom library symbol with `Sim.Device R` and `Sim.Params "r=10k"`, Save writes Value, which the netlist ignores. KiCad copies the library's fields onto the instance and edits `Sim.Params` there.
- **How to see it.** A custom library symbol with `Sim.Params "r=10k"`: tune it, save, and the netlist still has 10k. KiCad's stock tunable symbols do not hit this; only the POT has `Sim.Params`, and we do not tune POTs.
- **Severity.** Low.
- **Already known?** The cause is known (GAPS: no simulation fields in the table).
- **Fixed** on task-r7-sym-editor: 03a9ab27, with R7-SM-2 (the library symbol's `Sim.Params` is among the fields Save reads, and is written to the instance).

---

## Part C. Symbol Editor (rows no round reviewed)

Rows reviewed: Pin Properties, the Pin Table, library Symbol Properties (units, body styles, name, power), the library tree (Save As, Duplicate, Paste, Cut/Copy, Rename, Delete), and the Symbol Checker beyond r6's list. The C++ is `dialog_pin_properties.cpp`, `dialog_lib_edit_pin_table.cpp`, `dialog_lib_symbol_properties.cpp`, `lib_symbol.cpp`, `sch_pin.cpp`, `symbol_editor_control.cpp`, `symbol_library_manager.cpp` and `sch_io_kicad_sexpr_parser.cpp`.

### E7-S1. Every pin orientation is labelled the opposite way, and a new pin's default is written turned 180° (high)
- **What differs.**
  - Ours: `web/studio/src/kicad-port/pinOrientation.ts:14-27` maps `right: 180, left: 0, up: 270, down: 90`. Its header says '"Left" maps to 0'.
  - Ours: `symPinTemplate.ts:28` sets `DEFAULT_PIN_TEMPLATE.angle_deg = orientationToAngleDeg("right")` = 180, and `PinTableDialog.tsx:106` (Add Pin) hard-codes 180.
  - KiCad: `sch_io_kicad_sexpr_parser.cpp::parseSymbolPin` (`:1685-1688`):
    ```cpp
    case 0:   pin->SetOrientation( PIN_ORIENTATION::PIN_RIGHT ); break;
    case 90:  pin->SetOrientation( PIN_ORIENTATION::PIN_UP );    break;
    case 180: pin->SetOrientation( PIN_ORIENTATION::PIN_LEFT );  break;
    case 270: pin->SetOrientation( PIN_ORIENTATION::PIN_DOWN );  break;
    ```
  - KiCad's combo label for `PIN_RIGHT` is `_( "Right" )` (`pin_type.cpp::InitTables`). The default `g_LastPinOrient = PIN_ORIENTATION::PIN_RIGHT` (`symbol_editor_pin_tool.cpp:39`) is file angle 0.
- **How to see it.**
  - `Device:R` pin 1 is at file angle 270: KiCad's Pin Properties says "Down", ours "Up".
  - On a left-side pin `(at -5.08 0 0)`, KiCad says "Right", ours "Left".
  - P and a click with the defaults: KiCad writes `(at x y 0)`, ours `(at x y 180)`, so the connection point is on the other end.
  - Choosing "Right" in our dialog writes 180, which KiCad reads as `PIN_LEFT`.
  - Tests:
    - `pinOrientation.test.ts`: `angleDegToOrientation(0) === "right"` and `orientationToAngleDeg("up") === 90`.
    - `symPinTemplate.test.ts`: `DEFAULT_PIN_TEMPLATE.angle_deg === 0`.
- **Severity.** High: every pin the user places or reorients by its label is written facing the other way.
- **Already known?** No. r2's "checked" compared the template's label, not its angle. PARITY-symedit's "confirmed against builtin symbols" reached the wrong conclusion.
- **Fixed** on task-r7-sym-editor: 448607f2.

### E7-S2. A pin length edit keeps the connection point; KiCad's `ChangeLength` keeps the body end and moves the connection point (medium)
- **What differs.**
  - Ours: `PinPropertiesDialog.tsx:155` and `PinTableDialog.tsx:78` set `length_mm` only. `crates/ops/src/lib.rs:6587-6596` (`edit_symbol_pin`) stores the pin as given.
  - KiCad: `DIALOG_PIN_PROPERTIES::TransferDataFromWindow` does `m_pin->SetPosition( newPos ); m_pin->ChangeLength( ... )`, and the Pin Table's `UpdatePin` (`COL_LENGTH`) also calls `ChangeLength`. `SCH_PIN::ChangeLength`:
    ```cpp
    int lengthChange = GetLength() - aLength;
    // PIN_RIGHT: offsetX = lengthChange; PIN_LEFT: offsetX = -1 * lengthChange;
    // PIN_UP: offsetY = -1 * lengthChange; PIN_DOWN: offsetY = lengthChange;
    m_position += VECTOR2I( offsetX, offsetY ); m_length = aLength;
    ```
  - Sync mode's twins copy the edited pin's position, so they carry the same error.
  - The right helper already exists: `library_editors.rs::change_pin_length`, which only Push Pin Length uses.
- **How to see it.** A pin `(at -5.08 0 0)` of length 2.54, set to length 5.08: KiCad gives `(at -7.62 0 0)`, ours keeps `(at -5.08 0 0)`.
  - Test: `a pin length edit keeps the body end`.
- **Severity.** Medium.
- **Already known?** No.
- **Fixed** on task-r7-sym-editor: 91ced284.

### E7-S3. Pin Properties and the Pin Table show and take the file's +y-up Y; KiCad shows its internal Y, which is the negated file Y (medium)
- **What differs.**
  - Ours: `PinPropertiesDialog.tsx:163` and `PinTableDialog.tsx:75` use `LibrarySymbolPin.at` as it is.
  - KiCad: `parseSymbolPin` does `parseXY( true )` (negates Y). `TransferDataToWindow` does `m_posY.SetValue( m_origPos.y )`. eeschema's `ORIGIN_TRANSFORMS::ToDisplay` is the identity (`common/origin_transforms.cpp:35-44`).
  - Our own status bar is y-down, so it disagrees with our dialog for the same point.
- **How to see it.** For a pin at `(at 0 2.54 0)`, KiCad's "Y position:" says -2.54 and ours 2.54. Typing 5 writes `(at 0 -5 ..)` in KiCad and `(at 0 5 ..)` here.
- **Severity.** Medium.
- **Already known?** No.
- **Fixed** on task-r7-sym-editor: 91ced284.

### E7-S4. Pin Properties: an extra Delete Pin button that leaves the sync twins, silent clamps, and the labels (low)
- `PinPropertiesDialog.tsx:188-197` has a Delete Pin button that KiCad's dialog does not have. It is one `delete_symbol_pin`, so in Synchronized Pins Mode the twins that `DoDelete` would remove stay.
- The length is clamped to ≥ 0 and the text sizes to ≥ 0.1 mm without a message (`:155-159`). KiCad's `UNIT_BINDER`s take the value as typed.
- Labels: ours have no colons and one "Position" row. KiCad's are "Pin name:", "Pin number:", "X position:", "Y position:", "Pin length:", and so on.
- **Test.** Delete Pin in sync mode also removes "1-UB", or the button goes.
- **Severity.** Low.
- **Already known?** No.
- **Left (low)** on task-r7-sym-editor: GAPS.md item 19.

### E7-S5. Library Symbol Properties: lowering "Number of units" deletes nothing and asks nothing, so the file still holds the dropped units (high)
- **What differs.**
  - Ours: `SymbolPropertiesDialog.tsx:212` and `crates/ops/src/lib.rs:6496` set `sym.unit_count = unit_count` only. The writer (`crates/kicad/src/symbol_lib.rs:650-672`) still makes a `Name_<unit>_<style>` sub-symbol for every unit that has items.
  - KiCad: `DIALOG_LIB_SYMBOL_PROPERTIES::Validate`:
    ```cpp
    if( m_unitSpinCtrl->GetValue() < m_libEntry->GetUnitCount() )
        if( !IsOK( this, _( "Delete extra units from symbol?" ) ) ) return false;
    ```
    Then `SetUnitCount( n, true )` erases every item with `m_unit > aCount`. The parser grows the unit count back from the sub-symbol names.
  - Ours also does not clamp `activeUnit`, so the canvas can stay on a unit that is gone.
- **How to see it.** On a 3-unit symbol, set units to 2. The export still has `Name_3_1` with its pins, so KiCad and kicad-cli read a 3-unit symbol.
  - Test (ops): `lowering the unit count removes the items of the dropped units`.
- **Severity.** High (a file KiCad reads differently).
- **Already known?** No.
- **Fixed** on task-r7-sym-editor: 8c5ce5a7.

### E7-S6. Raising "Number of units" leaves the new units empty; KiCad copies unit 1's items into each new unit (medium)
- **What differs.** `LIB_SYMBOL::SetUnitCount( aCount, true )` duplicates each `m_unit == 1` item into units `prevCount+1 .. aCount`. Ours only sets the count.
- **How to see it.** On a 1-unit symbol with pins 1..3, set units to 2: KiCad's unit B has the same body and pins, ours an empty unit B.
  - Test (ops): `raising the unit count copies unit 1`.
- **Severity.** Medium.
- **Already known?** No.
- **Fixed** on task-r7-sym-editor: 8c5ce5a7.

### E7-S7. De Morgan on copies no pins; De Morgan off neither asks nor deletes (medium)
- **What differs.**
  - Ours: `has_alternate_body_style` is a plain flag (`ops/lib.rs:6497`). Style-2 items stay in the model, the writer hides them, and they come back when De Morgan is switched on again.
  - KiCad: `Validate` asks `_( "Delete extra body styles from symbol?" )`.
  - 25ec2335's `TransferDataFromWindow` does `SetBodyStyleCount( 2, false, true )`, which copies the style-1 pins, or `SetBodyStyleCount( 1, ... )`, which erases style 2.
  - 8303b2ad calls `SetHasDeMorganBodyStyles` first, so `SetBodyStyleCount` returns early. That looks like a KiCad bug, fixed in 25ec2335. The question is asked in both versions.
- **How to see it.** On a symbol with pins 1 and 2 on body style 1, check De Morgan: KiCad 25ec2335 has them on style 2 too. Uncheck it: KiCad asks and deletes.
  - Test (ops): `De Morgan on copies the style-1 pins; off removes style 2`.
- **Severity.** Medium.
- **Already known?** No.
- **Fixed** on task-r7-sym-editor: 8c5ce5a7 (as 25ec2335).

### E7-S8. Library Symbol Properties has no "Symbol name:" field (medium)
- **What differs.**
  - KiCad's `dialog_lib_symbol_properties_base.cpp:130-134` has the field. `TransferDataFromWindow` escapes the name (`CTX_LIBID`) and refuses with "Symbol must have a name." or "Symbol name '%s' already in use in library '%s'.". The undo step is `LIB_RENAME`.
  - For a power symbol, `OnSymbolNameText` keeps the Value in step with the name.
  - Ours shows the lib_id in the header and has no name control.
- **Test (e2e).** Rename in Symbol Properties, and the tree shows the new name.
- **Severity.** Medium.
- **Already known?** No.
- **Fixed** on task-r7-sym-editor: 8c5ce5a7, db35c853 (Undo after the rename).

### E7-S9. The four "Exclude from" boxes are disabled as soon as the dialog opens on a power symbol; KiCad disables them only when the power box is ticked (low)
- **What differs.** `SymbolPropertiesDialog.tsx:180` has `disabled={form.power}`. KiCad calls `Enable( false )` only in `onPowerCheckBox`.
- **Severity.** Low.
- **Already known?** No.
- **Left (low)** on task-r7-sym-editor: GAPS.md item 19.

### E7-S10. Pin Table: the Unit and Body Style cells take any integer, so the written file has extra units or loses the pin (high)
- **What differs.**
  - Ours: `PinTableDialog.tsx:81,84` are free number inputs. `validate_pin` (`ops/lib.rs:6517`) checks neither value.
  - The writer makes `Name_<unit>_1` for any unit and drops a pin with `body_style > max_style` (`symbol_lib.rs:613,650-672`).
  - KiCad: `GRID_CELL_COMBOBOX` with "All" and the units' display names, or "All" / Standard / Alternate. `PIN_INFO_FORMATTER::UpdatePin` ignores a value that matches nothing.
- **How to see it.** Unit 5 on a 2-unit symbol: the export has `Name_5_1`, and KiCad reads 5 units. Body style 2 without De Morgan: the pin is missing from the file.
  - Test (ops): `a pin's unit beyond the unit count is refused`, plus the body-style case.
- **Severity.** High (data loss; a file KiCad reads differently).
- **Already known?** No.
- **Fixed** on task-r7-sym-editor: 8c5ce5a7 (the verbs refuse it), 91ced284 (the combo boxes).

### E7-S11. Pin Table: each cell edit is its own undo step, with no OK, Cancel or "Save changes?" (medium)
- **What differs.**
  - Ours: `PinTableDialog.tsx:31-34,90,104-107` applies each edit at once.
  - KiCad: `SYMBOL_EDITOR_EDIT_TOOL::PinTable` works on copies, does `commit.Push( _( "Edit Pins" ) )` on OK, `OnClose` asks `_( "Save changes?" )`, and on a derived symbol OK is "Read Only".
- **Test (e2e).** Rename two pins, OK: one Ctrl+Z restores both.
- **Severity.** Medium.
- **Already known?** No.
- **Fixed** on task-r7-sym-editor: 91ced284.

### E7-S12. Pin Table edits go to the sync twins, and a unit of 0 deletes them; KiCad's table never synchronizes (medium)
- **What differs.**
  - Ours: the table calls `api.editPin`, which runs `planSyncedEdit` in sync mode (`symbolEditorStore.tsx:759-773`).
  - KiCad: `formatter.UpdatePin` acts on the row's own pins only, with no `SynchronizePins()`.
- **Test.** Rename pin 1 in the table on a 2-unit interchangeable symbol: "1-UB" keeps its name.
- **Severity.** Medium.
- **Already known?** No.
- **Fixed** on task-r7-sym-editor: 91ced284.

### E7-S13. Pin Table: Add Pin's defaults are not KiCad's (medium)
- **What differs.**
  - Ours: number max+1, Input, Line, 180, (0,0), 2.54, on the active unit.
  - KiCad's `OnAddRow`: an empty number and name, `PT_UNSPECIFIED`, `PIN_RIGHT`, unit 0 and body style 0. With pins in the table, it copies the last pin's orientation, type, shape, unit and body style, places it at the last pin's position minus `m_Repeat.pin_step` along y (along x for an up or down pin), and focuses Number.
- **Test.** `pinTable.test.ts`: `addRow copies the last pin and steps 100 mil`.
- **Severity.** Medium.
- **Already known?** No.
- **Fixed** on task-r7-sym-editor: 91ced284.

### E7-S14. Pin Table: each row keeps a stale copy of its pin and writes the whole copy back (medium)
- **What differs.** `PinTableDialog.tsx:22` does `useState(pin)` once per row and sends all of `local` on every commit. A server-side change is overwritten by the row's next edit: a sync update (E7-S12), an undo, a drag or Push Pin.
- **How to see it.** In sync mode, rename pin 1 in unit A; unit B's row still shows the old name. Edit unit B's type, and the old name comes back on both units.
- **Severity.** Medium.
- **Already known?** No.
- **Fixed** on task-r7-sym-editor: 91ced284 (the table edits its own copies until OK, as KiCad's).

### E7-S15. Pin Table: no summary or Count / text-size columns, raw type tokens, and a different sort order (low)
- **What differs.**
  - KiCad has the "Pin numbers:", "Pin count:" and "Duplicate pins:" summary (`PIN_NUMBERS`); `pinNumbersSummary.ts` exists here but the table does not use it.
  - KiCad's columns are Count, Number, Name, Electrical Type, Graphic Style, Orientation, Number Text Size, Name Text Size, Length, X/Y Position, Visible, Unit, Body Style.
  - KiCad shows `PinTypeNames()` and "Unit A"; ours shows `power_in` and "X (mm)".
  - KiCad sorts with `PIN_NUMBERS::Compare`; ours sorts by unit, then `localeCompare`.
- **Severity.** Low.
- **Already known?** In part (grouping, sorting and CSV are listed as not ported).
- **Left (low)** on task-r7-sym-editor: GAPS.md item 19.

### E7-S16. Symbol Checker names units by letter and ignores the unit's display name (low)
- **What differs.** `kicad-port/symbolChecker.ts:45-48` uses `unitLetter`. KiCad uses `GetUnitDisplayName( unit, false )` (`symbol_checker.cpp:149-150,182-183`), and `LibraryKeep.unit_names` already holds the names.
- **Severity.** Low.
- **Already known?** No.
- **Left (low)** on task-r7-sym-editor: GAPS.md item 19.

### E7-S17. Rename Symbol neither trims nor escapes the name (medium)
- **What differs.**
  - Ours: `actions/symbolLibraryOps.ts:196-204` and `check_symbol_lib_id` (`library_editors.rs:206-218`) refuse `A:B` and keep surrounding spaces.
  - KiCad: `RENAME_DIALOG` trims, and `RenameSymbol` does `EscapeString( aNewName, CTX_LIBID )`, refuses an empty name with "Symbol must have a name.", and returns when the name is unchanged.
  - Save As already does this right.
- **How to see it.** " A:B " becomes `A{colon}B` in KiCad and is refused here. " R2 " becomes `R2` in KiCad and is stored as " R2 " here.
- **Severity.** Medium.
- **Already known?** No.
- **Fixed** on task-r7-sym-editor: 8c5ce5a7.

### E7-S18. Name collisions are tested case-sensitively; KiCad's `SymbolNameInUse` ignores case (medium)
- **What differs.**
  - KiCad: `symbol_library_manager.cpp:657-659` `if( candidate.CmpNoCase( unescapedName ) == 0 ) return true;`. It is used by `ensureUniqueName`, Rename's Overwrite question, Save As and the inheritance checks.
  - Ours compares exactly everywhere: `libraryNames.ts::ensureUniqueName`, `symImportPlan.ts:65`, `symSaveAsPlan.ts`, and the server's checks.
- **How to see it.** With `r` in the library, Paste `R`: KiCad makes `R_1`, ours `R`. Two names that differ only in case are one file on a case-insensitive file system.
  - Test: `ensureUniqueName("R", ["r"]) === "R_1"`.
- **Severity.** Medium.
- **Already known?** No.
- **Fixed** on task-r7-sym-editor: 8c5ce5a7.

### E7-S19 .. E7-S23. Smaller library-tree differences (low)
- **S19.** Save As under the symbol's own name in its own library skips "Symbol '%s' already exists in library '%s'. Do you want to overwrite it?" (`SaveSymbolAsDialog.tsx:103`).
- **S20.** Duplicate and Save As never refuse a read-only (installed) library. KiCad says "Symbol library '%s' is not writable." or "Library '%s' is read-only. Choose a different library to save the symbol '%s' to.".
- **S21.** After Duplicate or Paste, KiCad shows the last new symbol at the current unit and body style; ours opens the first at unit 1.
- **S22.** Save As with Rename All opens the renamed copy; KiCad loads the typed name, which is the existing symbol. That is probably a KiCad bug, so keep ours and record it as a deliberate difference.
- **S23.** Message words:
  - "No symbol selected" has no full stop in KiCad.
  - Cut or Delete on a symbol that cannot be edited: KiCad says "Symbol library '%s' not found." or "... is not writable.".
  - Rename on a symbol that cannot be edited: KiCad returns silently.
- **Left (low)** on task-r7-sym-editor: GAPS.md item 19, one line each (S22 kept as a deliberate difference).

### Checked and found to match (Symbol Editor)
- **Pin Properties.**
  - The type, style and orientation combo orders and names.
  - The alternates grid and "Alternate pin definitions must have a name.".
  - `m_checkApplyToAllBodyStyles`; OK's unit and body style; the template saved at OK.
  - `EditPinProperties`' sync branch line by line against `planSyncedEdit`: one undo step, "Edit Pin Properties".
- **Symbol Properties.**
  - "References must start with a letter.", "Fields must have a name.", "Derived symbol must have a parent selected".
  - Empty fields dropped; `LockUnits`; the power Value; `SetLocalPower`; `onPowerCheckBox`; units 1-64; one undo step.
- **Pin Table.** Every pin of every unit and style, Visible, the sizes, a derived symbol refused, the combo orders.
- **Symbol Checker.** The entry point, "Symbol Warnings", "No symbol issues found.".
- **Save As.** The texts, the name rule, the validator order, Overwrite All / Rename All, Flatten, `DoSave`'s three cases.
- **Duplicate / Paste.** `%s_%d` from 1 (apart from case, E7-S18); Copy flattened; Paste's `~`.
- **Rename.** "Change Symbol Name" / "New name:", the Overwrite question; Value follows the name.
- **Delete.** The base-symbol warning, recursive removal, Cut = Copy + Delete.
- **Revert** is documented as missing (`UI-ACTIONS.md:73`).

---

## Part D. Footprint Editor (rows no round reviewed)

Rows reviewed: the Footprint Editor's own Pad Properties dialog (`components/footprint/PadPropertiesDialog.tsx`, a different component from the board's), Default Pad Properties and Add Pad, the Pad Table, and library Footprint Properties' attributes. The C++ is `dialog_pad_properties.cpp`, `dialog_fp_edit_pad_table.cpp`, `dialog_footprint_properties_fp_editor.cpp`, `pad.cpp` and `tools/pad_tool.cpp`.

### E7-F1. The Footprint Editor's Pad Properties does not run `padValuesOK`: it refuses a plated hole that reaches the pad's edge, its messages are not KiCad's, and it gives no warnings (medium)

**Fixed** on task-r7-fp-editor: c8a5ca2e (padValuesOK in the library dialog; `Footprint::validate_pad_values` for library pads).
- **What differs.**
  - Ours: `PadPropertiesDialog.tsx:88-106` sends `edit_pad`. The server's `validate_pad` (`crates/ops/src/lib.rs:6146`) calls `Footprint::validate` (`crates/model/src/footprint.rs:520-585`).
  - `Footprint::validate` refuses a plated pad with `drill >= min(size)` (`:572`, `:577`) with "drill {d} nm is not smaller than the {w} x {h} nm pad, so there is no annular ring". The result is a toast.
  - KiCad: `DIALOG_PAD_PROPERTIES::padValuesOK` blocks only on `DRCE_PADSTACK_INVALID` and `DRCE_PAD_TH_WITH_NO_HOLE`, and lists `DRCE_PADSTACK` as warnings:
    ```cpp
    if( errorCode == DRCE_PADSTACK_INVALID ) error_msgs.Add( _( "Error: " ) + msg );
    else if( errorCode == DRCE_PADSTACK ) warning_msgs.Add( _( "Warning: " ) + msg );
    else if( errorCode == DRCE_PAD_TH_WITH_NO_HOLE ) error_msgs.Add( _( "Error: Through hole pad has no hole." ) );
    ```
    `PAD::doCheckPad` (`pad.cpp:3001/3011`) gives "(PTH pad hole leaves no copper)" and "(PTH pad hole not fully inside copper)", both warnings.
  - The board dialog already uses the port of this (`kicad-port/fpFields.ts::padCheck`, r2 C5 / r3 C-4); the library dialog does not.
  - The same refusal hits Add Pad, Paste Pad Properties and the Pad Table, so a KiCad footprint with such a pad cannot be renumbered.
  - PARITY-fpedit section 3's "Validation: identical, reused" is wrong.
- **How to see it.** A TH pad, a 2 x 1 mm rectangle with a 1.2 mm drill, OK. KiCad warns and applies; ours refuses.
  - Test: `edit_pad_accepts_a_plated_hole_that_reaches_the_pad_edge`.
- **Severity.** Medium.
- **Already known?** No (fixed on the board dialog only).

### E7-F2. Changing "Pad type" changes only the type: the layers, the hole and the number stay; there is no Edge Connector or SMD Aperture type, and the Connector preset writes `smd` (high)

**Fixed** on task-r7-fp-editor: c8a5ca2e (`kicad-port/padType.ts::padTypeSelected`; Edge Connector and SMD Aperture; the Connector preset sets `kicad.connector`).
- **What differs.**
  - Ours:
    - `PadPropertiesDialog.tsx:135` does `set("kind", ...)` only. `KIND_OPTIONS` (`:19-23`) has three entries.
    - Number stays enabled for NPTH. The drill box shows `form.drill ?? 800_000` for a pad that has none (`:201`).
    - The "Connector (no paste)" preset (`:28`) sets `smd` and F.Cu/F.Mask but never `kicad.connector`, so it is written `smd`.
  - KiCad:
    - `DIALOG_PAD_PROPERTIES::PadTypeSelected` calls `updatePadLayersList( std::nullopt, ... )` ("the old pad's layer set is no longer meaningful"). It takes `PTHMask`, `SMDMask`, `ConnSMDMask`, `UnplatedHoleMask` or `ApertureMask`.
    - It zeroes the hole of a type that has none.
    - For a type with no connection: `m_padNumCtrl->ChangeValue( wxEmptyString ); m_padNetSelector->SetSelectedNetcode( 0 )`. `OnUpdateUI` does `m_padNumCtrl->Enable( hasConnection )`.
    - `transferDataToPad` (NPTH): `SetNumber( wxEmptyString )`.
    - The choices (`dialog_pad_properties_base.cpp:39`): "Through-hole", "SMD", "Edge Connector", "NPTH, Mechanical", "SMD Aperture".
- **How to see it.**
  1. A TH pad (`*.Cu F.Mask B.Mask`) set to SMD is written `(pad "1" smd ... (layers "*.Cu" "F.Mask" "B.Mask"))`, an SMD pad on every copper layer.
  2. Pad "1" set to NPTH is written `(pad "1" np_thru_hole ...)`. On a board, `BOARD_NETLIST_UPDATER` gives that hole pin 1's net.
  - Test: `padTypeChange.test.ts`. SMD gives `["F.Cu","F.Paste","F.Mask"]` and no drill; NPTH gives number "" with the box disabled; Edge Connector gives `kicad.connector` and F.Cu F.Mask.
- **Severity.** High (the pad written is not the pad the user chose; a mechanical hole can join a net).
- **Already known?** No. PARITY section 3 calls the presets "partial by design", but not this.

### E7-F3. OK in Pad Properties on a real pad does not set the Add Pad master or the last pad number, and Default Pad Properties has no number field (medium)

**Fixed** on task-r7-fp-editor: c8a5ca2e.
- **What differs.**
  - Ours: `PadPropertiesDialog.tsx:99-105` stores `defaultPad` in default mode only. The Number row is hidden in default mode (`:128`).
  - KiCad's `TransferDataFromWindow` writes the master for every pad:
    ```cpp
    if( !transferDataToPad( m_masterPad ) ) return false;
    ...
    padTool->SetLastPadNumber( m_masterPad->GetNumber() );
    m_masterPad->SetNetCode( NETINFO_LIST::UNCONNECTED );
    if( !m_currentPad ) return true;
    ```
    The default dialog shows `GetLastPadNumber()` and is titled "Default Pad Properties for Add Pad Tool".
- **How to see it.** Edit pad 2 to a 1 x 2 oval, then Add Pad. KiCad places a 1 x 2 oval numbered 5; ours places the old master's shape.
- **Severity.** Medium.
- **Already known?** No.

### E7-F4. Pad Table: a Type change does not apply `PAD::SetAttribute`'s layer and number rules (medium)

**Fixed** on task-r7-fp-editor: c8a5ca2e (`padType.ts::setPadAttribute`; a hole-less pad made plated takes 0.8 mm where KiCad leaves 0, as the IR refuses no hole).
- **What differs.**
  - Ours: `components/footprint/PadTableDialog.tsx:137-145` changes the kind and the drill (`?? 800_000`).
  - KiCad's `PAD::SetAttribute` (`pad.cpp:1370-1411`):
    - PTH: `layerMask |= LSET::AllCuMask()`.
    - SMD/CONN: trims to one copper layer (B_Cu if it is set) and sets the drill to 0.
    - NPTH: `m_number = wxEmptyString`, unconnected.
- **How to see it.** SMD to Through-hole: KiCad gives `*.Cu F.Paste F.Mask`, ours F.Cu only. Pad "3" to NPTH: KiCad clears the number.
- **Severity.** Medium.
- **Already known?** Only the missing Connector and Aperture types (the file header).

### E7-F5. Library Footprint Properties: "SMD" and "Through hole" are two check boxes, not KiCad's "Component type" choice, and the labels are not KiCad's (medium)

**Fixed** on task-r7-fp-editor: c8a5ca2e.
- **What differs.**
  - Ours: `components/footprint/FootprintPropertiesDialog.tsx:138-153` lets both boxes be ticked, and the writer then writes `(attr smd through_hole)` (`footprint_lib.rs:370-376`).
  - KiCad: `m_componentTypeChoices` is { "Through hole", "SMD", "Unspecified" }, a single choice. Its labels are "Not in schematic", "Exclude from position files", "Exclude from bill of materials", "Do not populate", "Exempt from courtyard requirement", "Allow bridged solder mask apertures between pads", and the zone choice "Use zone setting".
  - The board's own Footprint Properties already has KiCad's choice.
- **Test.** Choosing SMD clears `through_hole`.
- **Severity.** Medium (a footprint KiCad's dialog cannot make).
- **Already known?** No. PARITY section 5 says "identical".

### E7-F6. Switching the hole between Round and Slot replaces the size with fixed values (low)

**Left (low)**: GAPS.md item 19.
- **What differs.** `PadPropertiesDialog.tsx:192,195` substitutes 0.8 mm or 0.6 x 1.0 mm. KiCad's `OnDrillShapeSelected` keeps `m_holeX` / `m_holeY`, so a 1.0 mm round hole becomes a 1.0 x 1.0 oblong. KiCad's labels are "Round" / "Oblong".
- **Severity.** Low.
- **Already known?** No.

### E7-F7. Smaller, in the same dialog (low)

**Left (low)**: GAPS.md item 19 (the PARITY-fpedit "PTH-only thermal" rows corrected, 258abd35).
- The THT preset `["*.Cu","F.Mask","B.Mask"]` and the master's `*.Cu *.Mask` compare as different strings, so a pad placed by Add Pad shows no preset as current. KiCad reads both forms the same way.
- Our type label is "Through hole"; KiCad's is "Through-hole".
- PARITY section 3 lists a "PTH-only thermal" pad connection, which does not exist. The code's four choices are right; the document is stale.

### Checked and found to match (Footprint Editor)
- `PAD::PTHMask`, `SMDMask`, `ConnSMDMask`, `UnplatedHoleMask`, `ApertureMask` against the presets.
- `DEFAULT_PAD_DRILL_DIAMETER_MM` = 0.8.
- The nine pad shapes; the padstack controls for PTH only.
- An SMD pad's drill is never written.
- `PAD_PLACER::CreateItem` numbering.
- The `(attr ..)` word order.
- `checkFootprintName`'s messages.
- The Pad Table's title, columns, shape words and "Edit Pads" as one undo step.

---

## Part E. Exports and board control

### X7-1. Export Footprints adds only the names the project library lacks; KiCad saves every board footprint, as edited on the board, over the library's (medium)

**Fixed** on task-r7-fp-editor: 902380f0 (`Cmd::ExportFootprintsToLibrary`; the instance's fields and attributes are not written, PARITY-boardctl section 5).
- **What differs.**
  - Ours: `web/studio/src/actions/boardControlActions.ts:245-266` filters to the names not yet in the project footprint library and runs `open_footprint_for_edit` for each. A footprint already there is skipped, and the board's per-instance edits (GAPS 9, `DrawingsSection::footprint_edits`: pad type, size, hole, margins, fields) are never exported. The toast is our own: "%d footprint(s) added to the project footprint library, %d already there."
  - KiCad: `PCB_EDIT_FRAME::ExportFootprintsToLibrary` (`footprint_libraries_utils.cpp:647-697`) loops over every placed footprint:
    ```cpp
    FOOTPRINT* fpCopy = static_cast<FOOTPRINT*>( footprint->Duplicate( IGNORE_PARENT_GROUP ) );
    fpCopy->SetReference( "REF**" );
    ...
    adapter->SaveFootprint( nickname, fpCopy, true );   // overwrite
    ```
    So the library gets the board's version of each footprint, with the last instance of a name winning. KiCad also asks for the library ("Export footprints to library:") and offers "Update board footprints to link to exported footprints".
- **How to see it.** Edit R1's pad 1 on the board (Pad Properties, 1.2 x 1.4 mm), then File > Export > Footprints. KiCad's library entry gets the edited pad; ours says "already there" and changes nothing.
  - Test (store or e2e): `export footprints writes the board's edited footprint over the library entry`.
- **Severity.** Medium (the export the user asked for does not happen).
- **Already known?** In part. PARITY-boardctl section 5 describes the "add the missing names" design but not the lost edits or the overwrite.

### C7-12. Repair Board: a narrower duplicate-id scan, orphan nets taken from zones rather than pads, and an undo step that KiCad does not make (low)

**Left (low)**: GAPS.md item 35.
- **What differs.**
  - Ours: `crates/ops/src/board_control.rs:414-482` (`repair_board`).
    - It scans tracks, vias, zones, drawings, texts, groups and dimensions in that order.
    - Footprints, pads and footprint fields and graphics are not scanned. The import already makes a duplicated footprint uuid unique, silently: "the first keeps it", `ir.rs:3115-3118`.
    - Orphaned nets are collected from tracks, vias and zones.
    - It is `Cmd::RepairBoard`, one undo step.
  - KiCad:
    - `BOARD::RepairDuplicateItemUuids` (`board.cpp:2063-2131`) claims ids in this order: footprints, pads, tracks, then every footprint's fields, graphics, zones and groups, then the rest of the board.
    - `BOARD_EDITOR_CONTROL::RepairBoard` (`board_editor_control.cpp:760-838`) re-parents orphaned nets of pads and tracks/vias only (not zones), with "Orphaned net %s re-parented.\n".
    - It then calls `m_frame->OnModify()` with no commit, so there is no undo step.
    - Which of two clashing items keeps its id therefore differs (a track and a zone: the same in both; a track and a footprint field: KiCad keeps the track's).
- **How to see it.** A board whose zone names a net that is not in the netlist: KiCad reports "No board problems found."; ours reports "1 potential problems repaired." and adds the net.
- **Severity.** Low (rare; an extra undo step is harmless).
- **Already known?** No.

---

## Part F. Board control: zone priority, net highlight and hide, local ratsnest, ratsnest drawing

These rows were read against `board_editor_control.cpp`, `board_inspection_tool.cpp`, `pcb_control.cpp` and `ratsnest_view_item.cpp`. A footprint standing for its pads in Highlight Net (selection) and in Hide Net is already P11-11 (`p11-p18.md`), so it is not repeated here.

### C7-1. Zone Priority: a zone that lies inside another zone's hole is counted as overlapping it (medium)

**Fixed** on task-r7-fp-editor: a8ef86f4.
- **What differs.**
  - Ours: `crates/ops/src/board_control.rs:216-231` (`overlapping_zones`) uses `ring_contains(&c.outline, zone.outline[0])` and `edges_touch`, on the outer ring only. It ignores `Zone::holes`, although `zones_intersect` (`:89-100`) does honour them.
  - KiCad: `getOverlappingZones` tests `aZone->Outline()->Collide( candidate->Outline() )`, which goes through the triangulation and so has no triangles in the holes, and `Outline()->Contains( CVertex( 0 ) )`. `containsSingle` says: "If the point is inside a hole it is outside of the polygon".
- **How to see it.** B, priority 0, is a 20 mm square with a 10 mm hole. A, priority 1, is a 4 mm square in the middle of the hole, on the same layer. Lower A: KiCad does nothing (and greys the entry out); ours swaps the two priorities.
  - Test: `a_zone_inside_anothers_hole_does_not_overlap_it`.
- **Severity.** Medium.
- **Already known?** No.

### C7-2. The Zone Priority menu's enable test is a bounding box on the first layer only; the PARITY row is stale (medium)

**Fixed** on task-r7-fp-editor: a8ef86f4 (`kicad-port/zoneOverlap.ts`; the PARITY-boardctl row corrected, 258abd35).
- **What differs.**
  - Ours: `web/studio/src/kicad-port/pcbSelectionSummary.ts:29-38` (`overlappingZones`) decides `zoneCanRaise` / `zoneCanLower` from the bounding boxes, with `other.layer !== zone.layer` (the first layer only) and no outline test.
  - KiCad: `ZONE_PRIORITY_CONTEXT_MENU::update` calls the real `getOverlappingZones`, which intersects the layer sets and then tests the outlines.
- **How to see it.**
  - A zone on F.Cu + B.Cu with a higher-priority B.Cu zone over it: ours greys Raise out, KiCad enables it.
  - Two L-shaped zones whose boxes meet but whose outlines do not: ours enables the entries, and the command then refuses with "this zone is already above every zone it overlaps".
  - `PARITY-boardctl.md:41` still says "the four entries are always enabled", which is no longer true.
  - Test: `pcbSelectionSummary.test.ts`, "a multi-layer zone can rise over a zone on its second layer".
- **Severity.** Medium.
- **Already known?** No.

### C7-3. Ratsnest Mode (All to Visible) and the Appearance radio buttons keep the Local Ratsnest tool's pad flags; KiCad resets them on every change (medium)

**Fixed** on task-r7-fp-editor: a8ef86f4.
- **What differs.**
  - Ours: `actions/boardControlActions.ts:58-65` and `withViewSlice` (`state/store.tsx:1269-1282`) clear `bcx.localRatsnestPads` only when `showRatsnest` itself changes.
  - KiCad: `PCB_CONTROL::RatsnestModeCycle` and `APPEARANCE_CONTROLS::onRatsnestMode` always call `SetElementVisibility( LAYER_RATSNEST, m_ShowGlobalRatsnest )`. `BOARD::SetElementVisibility` then sets `SetLocalRatsnestVisible( isEnabled )` on every pad.
- **How to see it.** Use the Local Ratsnest tool on U1.1, then cycle the mode from All to Visible. KiCad's lines for U1.1 come back; ours stay hidden.
  - Test: `ratsnestModeCycle all->visible forgets the local ratsnest pads`.
- **Severity.** Medium.
- **Already known?** No.

### C7-4. Highlight Net (selection) with several nets overwrites the last-highlight memory; KiCad leaves it alone (medium)

**Fixed** on task-r7-fp-editor: a8ef86f4.
- **What differs.**
  - Ours: `withHighlight` (`kicad-port/boardControlState.ts:70-73`) sets `lastHighlight` on every change.
  - KiCad: `highlightNet`'s multi-net branch does `m_currentlyHighlighted = netcodes; return true;` before it reaches `m_lastHighlighted = netcodes`.
- **How to see it.** Highlight A, then highlight a selection on nets B and C, then Toggle Last Net Highlight: ours shows A, KiCad shows its older memory.
  - Test: `withHighlight(..., "A", ["B","C"]).lastHighlight` stays `[]`.
- **Severity.** Medium.
- **Already known?** No.

### C7-5. Highlight Net (selection) drops the unconnected net (code 0) (low)

**Left (low)**: GAPS.md item 35.
- **What differs.**
  - Ours: `netsOfSelection` (`kicad-port/boardControl.ts:116-135`) skips an empty net.
  - KiCad: it inserts `GetNetCode()` whatever it is. A selected pad with no net therefore highlights the unconnected items, and with a GND pad it gives a two-net highlight.
  - Ours falls back to the net under the cursor instead.
- **Severity.** Low.
- **Already known?** No.

### C7-6. Hide / Show Net in Ratsnest acts on the item under the cursor when nothing is selected, and shows a toast of our own (medium)

**Fixed** on task-r7-fp-editor: a8ef86f4 (the cursor fallback and the toast; the footprint part stays P11-11's).
- **What differs.**
  - Ours: `boardControlActions.ts:115-125` uses `requestSelection()` (the selection, else the item under the cursor). With no net it toasts "Select an item on the net to hide in the ratsnest.".
  - KiCad: `doHideRatsnestNet` reads `GetSelection()` only and keeps nets with `GetNetCode() > 0`. The action is disabled by `haveNetCond` (`pcb_edit_frame.cpp`), so there is no message.
- **How to see it.** With nothing selected and the cursor over a GND track, run `pcbnew.EditorControl.hideNet` from the palette: ours hides GND, KiCad does nothing.
- **Severity.** Medium.
- **Already known?** The footprint part is P11-11; the cursor fallback and the toast are new.

### C7-7. A ratsnest line whose two ends coincide is invisible; KiCad draws a 0.4 mm cross (medium)

**Fixed** on task-r7-fp-editor: a8ef86f4.
- **What differs.**
  - Ours: `components/canvas/painter.ts:1049-1077` always does `moveTo(a); lineTo(b)`.
  - KiCad: `RATSNEST_VIEW_ITEM::ViewDraw` has `if ( source == target )` and draws two lines of `CROSS_SIZE = 200000`. The dynamic ratsnest has the same case.
- **How to see it.** A via centred on an SMD pad of the same net on the other side, not connected: ours shows no ratsnest at all, so the open connection cannot be seen.
- **Severity.** Medium.
- **Already known?** No.

### C7-8. The curved ratsnest is a quadratic curve with 0.03·L bulge; KiCad's is `DrawCurve( source, center, center, target )` with `center` offset 0.1·L, a 0.075·L bulge (low)

**Left (low)**: GAPS.md item 35 (the PARITY-boardctl row no longer says identical, 258abd35).
- **What differs.** `painter.ts:1060-1071`. `ctx.bezierCurveTo(cx, cy, cx, cy, bx, by)` with KiCad's `center` would be exact.
- **Severity.** Low (cosmetic).
- **Already known?** In a code comment only; the PARITY row says "identical".

### C7-9. Ratsnest colours: the highlighted net is drawn in the selection-shadow colour at 2.5 px, and the other lines are not dimmed (low)

**Left (low)**: GAPS.md item 35.
- **What differs.**
  - KiCad keeps one width and brightens a highlighted net's own colour: `adjustColor( color, 0.8, color.a + 0.4 )`.
  - With a highlight or a local ratsnest active, KiCad sets `dimStatic`, which halves the alpha of every other line.
- **Severity.** Low (cosmetic).
- **Already known?** No.

### C7-10. Local Ratsnest picks a pad by its box, not `PAD::HitTest`, and leaves the selection alone (low)

**Left (low)**: GAPS.md item 35.
- **What differs.**
  - Ours: `boardControlPick.ts:21-34` (`padAt`) hits the pad's box.
  - KiCad: `selectionClear`, then `selectionCursor` with `PadFilter`, which hits the real shape. A click in the corner of a round pad's box therefore falls through to the footprint, and KiCad leaves the clicked item selected.
- **Severity.** Low.
- **Already known?** No.

### C7-11. The net under the cursor (when nothing is selected) is the nearest pad *centre* or a track within 10 px, not KiCad's collector (low)

**Left (low)**: GAPS.md item 35.
- **What differs.**
  - Ours: `components/canvas/netAtCursor.ts:11-37`.
  - KiCad: `highlightNet` uses the items' `HitTest`, `SetPreferredLayer( activeLayer )`, `SetIgnoreNoNets( true )` and a high-contrast filter, then takes `collector[0]`.
- **How to see it.** On a 3 mm pad, more than 10 px from its centre: ours finds nothing and clears the highlight, KiCad highlights the pad's net.
- **Severity.** Low.
- **Already known?** No.

### Checked and found to match (board control)
- **`buildPriorityMap`, `findCascadeZones`.** Rule areas and teardrops left out; the other layers included.
- **Move to Top / Bottom.** `maxOverlapping` from the zone's own priority; options A and B; `useDown = downViable && (!upViable || down.size() <= up.size())`; the mirror with `useUp` on ties.
- **Raise / Lower.** The nearest overlapping priority, `target+1`, the swap at `UINT_MAX` / 0, equal priorities never a target. One selected copper zone; one undo step.
- **Toggle Last Net Highlight.** The swap. A single-net `highlightNet` with `aUseSelection`: the selection first, then the cursor; nothing found turns the highlight off.
- **`doHideRatsnestNet`'s set operations.** The context menu's `hasNet` is `haveNetCond`.
- **`LocalRatsnestTool`.** Pad flip; footprint to `!Pads()[0]->GetLocalRatsnestVisible()`; empty click resets; Esc resets, `END_ACTIVATE` does not; the tool stays armed.
- **`RatsnestModeCycle`'s states.** The Appearance radios.
- **`ViewDraw`'s visibility rules.** Hidden nets; the local flags with the global ratsnest on or off; visible-layers mode; high contrast HIDDEN.
- **Documented already, not counted.** `SetZonePriority` refuses a no-op with a message where KiCad returns 0 (PARITY-boardctl:41).

---

## Convergence: what still differs in daily work, by area, ranked

**Footprint and Symbol Editors: they do not match KiCad yet.** This is where most of this round's findings are, because rounds 2-6 never read these dialogs.
1. **Symbol Editor pin orientation is reversed (E7-S1).** Every pin the user orients by its label, and every pin placed with the defaults, faces the wrong way in the file.
2. **Footprint Editor Pad type change (E7-F2).** The layers and the number stay, so an SMD pad ends up on every copper layer and an NPTH hole keeps a number and gets a net.
3. **Symbol units and De Morgan (E7-S5, S6, S7).** Lowering the unit count leaves the units in the file; raising it gives empty units.
4. **Pin Table (E7-S10..S14).** Unbounded unit / body style (data loss), per-cell undo, sync where KiCad has none, stale rows.
5. **Pin length and Y sign (E7-S2, S3).** The connection point stays and the body end moves; Y is shown with the opposite sign.
6. **Pad validation (E7-F1).** Refusals KiCad does not make; no warnings.
7. **The Add Pad master after Pad Properties (E7-F3).** Library Footprint Properties' SMD / TH check boxes (E7-F5).
8. **Library-tree name rules (E7-S17, S18).** No trim or escape; case-sensitive collisions.

**Simulator: the dialogs match KiCad as of round 6.** What a user still meets:
1. The missing Simulation Model dialog, which matters for vendor models. Recommended first step: the ~1-1.5k-line "T2-lite" in Part B.
2. Tune's Save is refused on every imported KiCad project (R7-SM-2).
3. Smaller differences in the saved fields (R7-SM-1, R7-SM-3).
- Passive and DC-source simulation loses nothing, because kicad-cli infers the models.

**3D viewer: it matches KiCad apart from the documented gaps** (GAPS 23: raytracing, presets, the navigator). Left from this round:
- V7-1: paste-only pads get a margin. Low; it shows only in the picture.
- Cheap to close: "Plot on All Layers" and `plotreference` for the plot-following rows.

**Design blocks: they match KiCad for what is wired.** What is left is GAPS 32's list. Cosmetic: the info bar and the "Do not show again" box.

**Fab outputs and exports: they match KiCad for every kicad-cli export.** The open items:
1. Export Footprints (X7-1) does not write the board's edited footprints over the library's.
2. GAPS 37: the Plot dialog's missing formats and options. This is still the largest gap a user notices in fab work.
3. The served log, the IPC-D-356 message and the drill order now match kicad-cli word for word.

**Board control: mostly matching.** Left:
1. A coinciding ratsnest end is invisible (C7-7).
2. Zone priority's holes and its menu test (C7-1, C7-2).
3. The reset of the local ratsnest flags (C7-3).
4. The highlight memory (C7-4).
5. Hide Net on the cursor item (C7-6).
- The curve, the colours and the picking are cosmetic or close to it (C7-8..C7-11).

**Reuse rule.** In this area I found no active code that duplicates kicad-cli or a linked C++ library. The drill-chart port stays by the user's decision (D6-1), and the IPC-D-356 and drill logs are rebuilt from the job's outputs, not re-implemented.

---

## Summary

| ID | Area | What | Severity | Known? |
|---|---|---|---|---|
| V7-1 | 3D paste | a pad with no copper still gets the paste margin (`GetSolderPasteMargin` returns 0) | low | no (V6-5 edge) |
| R7-SM-1 | Tune Save | only Value written; KiCad's `WriteFields` adds hidden Sim.Device/Type/Pins | low | no |
| R7-SM-2 | Tune Save | refused on every schematic read from a KiCad file | medium | in part (GAPS 12) |
| R7-SM-3 | Tune Save | a library-only `Sim.Params` is missed | low | cause known |
| E7-S1 | Pin orientation | labels reversed; default pin written at 180 (KiCad 0) | **high** | no |
| E7-S2 | Pin length | `ChangeLength` keeps the body end, ours the connection point | medium | no |
| E7-S3 | Pin Y | file +y-up shown; KiCad shows the negated Y | medium | no |
| E7-S4 | Pin Properties | Delete Pin skips the twins; clamps; labels | low | no |
| E7-S5 | Symbol units | fewer units deletes nothing, no question; the file keeps them | **high** | no |
| E7-S6 | Symbol units | more units not filled from unit 1 | medium | no |
| E7-S7 | De Morgan | no pin copy on; no question or delete off | medium | no |
| E7-S8 | Symbol Properties | no Symbol name field | medium | no |
| E7-S9 | Symbol Properties | Exclude boxes disabled on open for power | low | no |
| E7-S10 | Pin Table | unit / body style unbounded: extra units, a pin lost | **high** | no |
| E7-S11 | Pin Table | per-cell undo, no OK/Cancel | medium | no |
| E7-S12 | Pin Table | edits synced to twins | medium | no |
| E7-S13 | Pin Table | Add Pin defaults | medium | no |
| E7-S14 | Pin Table | stale row written back | medium | no |
| E7-S15 | Pin Table | summary, columns, labels, sort | low | in part |
| E7-S16 | Symbol Checker | unit letters, not unit names | low | no |
| E7-S17 | Rename Symbol | no trim or escape | medium | no |
| E7-S18 | Library names | case-sensitive collisions | medium | no |
| E7-S19..S23 | Library tree | Save As own name, read-only libraries, which symbol opens, Rename All, messages | low (5) | no |
| E7-F1 | Pad Properties (FP editor) | no `padValuesOK`; own refusals, no warnings | medium | no |
| E7-F2 | Pad type | type change keeps layers, hole and number; no Connector / Aperture | **high** | no |
| E7-F3 | Add Pad master | Pad Properties OK does not set the master or the last number | medium | no |
| E7-F4 | Pad Table (FP) | no `SetAttribute` layer and number rules | medium | in part |
| E7-F5 | FP Properties | SMD and TH as two boxes; labels | medium | no |
| E7-F6 | Pad drill | Round/Slot switch resets the size | low | no |
| E7-F7 | Pad Properties | preset strings, labels, stale doc | low | no |
| X7-1 | Export Footprints | existing entries and board edits not exported | medium | in part |
| C7-1 | Zone priority | a zone in a hole counts as overlapping | medium | no |
| C7-2 | Zone priority menu | box and first-layer test; stale PARITY row | medium | no |
| C7-3 | Ratsnest mode | local flags kept on All to Visible | medium | no |
| C7-4 | Highlight | multi-net overwrites the last highlight | medium | no |
| C7-5 | Highlight | the unconnected net dropped | low | no |
| C7-6 | Hide Net | the cursor item used with no selection; toast | medium | in part (P11-11) |
| C7-7 | Ratsnest | a coinciding pair is invisible (KiCad: a cross) | medium | no |
| C7-8 | Ratsnest | curve shape | low | code comment |
| C7-9 | Ratsnest | highlight colour, no dimming | low | no |
| C7-10 | Local Ratsnest | box pick, selection | low | no |
| C7-11 | Highlight | cursor net lookup | low | no |
| C7-12 | Repair Board | scan order and scope; orphan nets from zones; an undo step | low | no |

**Counts:** high 4, medium 23, low 20 (47 in all; E7-S19..S23 are counted as five lows). Cosmetic notes are not counted.

**Round-6 merges:** 21 of the 23 fixes are right against the C++. V6-5 left the paste-only-pad edge (V7-1). B6-1 uses the effective uid/gid where `access` uses the real ones, which does not matter outside a setuid process.

**Doc corrections for a fixer** (task-r7-sym-editor did the GAPS 34 / PARITY-sch 49 size, PARITY-sch 38.1's Save on imported sheets, and PARITY-symedit's orientation note):
- GAPS 34 / PARITY-sch 49: the Simulation Model port is about 28k lines, not 14.7k.
- PARITY-fpedit section 3: the "Validation" and "PTH-only thermal" rows. **Fixed** on task-r7-fp-editor: 258abd35.
- PARITY-fpedit section 5: "Attributes identical". **Fixed** on task-r7-fp-editor: 258abd35.
- PARITY-boardctl:41: the zone priority entries are no longer always enabled. **Fixed** on task-r7-fp-editor: 258abd35.
- PARITY-boardctl's curved ratsnest row. **Fixed** on task-r7-fp-editor: 258abd35 (the "Ratsnest drawing rules" row).
- PARITY-sch 38.1: Tune's Save fails on imported sheets.
- PARITY-symedit: the "confirmed against builtin symbols" note for pin orientation.
