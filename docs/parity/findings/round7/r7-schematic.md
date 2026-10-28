# Round 7 findings: the schematic editor and its files (`r7-schematic`, 2026-10-28)

**Scope.** `crates/engine`, `crates/kicad`'s `.kicad_sch` reader and writer, `crates/ops` `sch_*`, the studio's schematic code, `web/studio/PARITY-sch.md` §48-50. Main is at `f057b4ea`, read only. No code was changed anywhere.

**Priority.**
- (a) Round 6's merges (PARITY-sch §50): 9d05af7a (unit display names), ae5feeec (Replace All), 0b63e63d (alternates in the duplicate-name test), f1ccd18c (`~` by version), 327bb669 (DeleteJunction on deletes), ddc91f1a (cache user fields), d53c20b8 (Net Navigator), 7595d15c (Unfold from Bus), 8685a920 (Ctrl+E), 6c545baa / 02f258df (Compare Symbol), 8cc119ae (Hierarchy Navigator), 8ffb4585 (CleanHistory), c7cee1cd (Symbol Checker). Slice A.
- (b) The 130 QA roots through `eda import-kicad`, judged by kicad-cli. Slice B. Rerun as before (**124 of 130**), then twice more: with each root's `.kicad_pro` kept, and comparing the netlist's components and the files' items, not only nets. The extended runs found most of this round's highs.
- (c) Rows no round had reviewed: Pin Helpers (§31), the sheet's reference drawing, the file items no round compared (texts, text boxes, shape fills, the library cache's fields), and the project file of a schematic-only import. Slice C.

**KiCad sources.** Connectivity, naming and ERC against 25ec2335 (the box's kicad-cli); files and tools against 8303b2ad (`$KICAD_SRC_DIR`).

**Binary.** Main `f057b4ea` was built from a `git archive` with its own `CARGO_TARGET_DIR`, which was deleted after. The binary is `/tmp/findings-r7-schematic.x94C/bin/eda`.

**Scratch.** Everything is in `/tmp/findings-r7-schematic.x94C/`:
- `bin/qa1.sh`, `bin/runset.sh`: round 6's harness, without the project. Output: `sweep.txt`, `names.txt`, `bin/qa/<root>/`.
- `bin/qa1p.sh`, `bin/runsetp.sh`: the same harness with the `.kicad_pro` / `.kicad_prl` kept. Output: `sweepp.txt`, `namesp.txt`, `bin/qap/<root>/`.
- `py/comps.py`: kicad-cli netlist components, original against derived. Output: `comps.txt`.
- `py/items.py`, `py/blocks.py`, `py/props.py`: item counts and item text over each root's hierarchy. Output: `items.txt`.
- `py/pro.py`: the project file's keys, original against derived. Output: `pro.txt`.
- The cases: `subpart/` (H5), `textsim/` (H4), `textvar/` (H6).

**Already known?** Every finding was checked against `docs/parity/GAPS.md`, `docs/parity/findings/` (rounds 2-6, p11-p18, p12) and PARITY-sch §39-§50.

**Read first.**
- The connectivity is converged. Without the project, 124 of 130 roots match, and the 6 that differ have the known causes. Net names match too, apart from the same known causes.
- **What remains is mostly the files.** Run with its `.kicad_pro`, a schematic-only import matches on only **110 of 130**. These are lost:
  - the project's net classes (H1);
  - its top-level sheets (H2);
  - its unit naming (H5) and text variables (H6);
  - every other Schematic Setup setting (M1);
  - its simulation model files (M6).
- The writer drops:
  - every placed symbol's Description (H3; 45 roots, 302 symbols);
  - a text's or text box's "exclude from simulation", so a SPICE directive that was excluded runs again (H4, proved with kicad-cli's SPICE netlist).

## Slice A: round 6's merges

### A7-M1. Net Navigator: a pin's line has the reference without its unit, and the library pin's name instead of the alternate (medium)

**What differs.**
- KiCad 8303b2ad and 25ec2335, `net_navigator.cpp::GetNetNavigatorItemText`:
  ```cpp
  case SCH_PIN_T:
      retv.Printf( _( "Symbol '%s' pin '%s'" ),
                   symbol->GetRef( &aSheetPath, true ),
                   UnescapeString( pin->GetNumber() ) );
      if( wxString pinName = UnescapeString( pin->GetShownName() ); !pinName.IsEmpty() )
          retv += wxString::Format( " (%s)", pinName );
  ```
  - `GetRef( .., true )` adds the unit (`if( aIncludeUnit && GetUnitCount() > 1 ) ref += subRef;`, `sch_symbol.cpp::GetRef`).
  - `SCH_PIN::GetShownName` returns `m_alt` when the pin has one (`sch_pin.cpp:662`).
- Ours:
  - `web/studio/src/kicad-port/netNavigator.ts:85` builds ``Symbol '${p.ref}' pin '${p.number}'`` + ` (${p.name})`.
  - `p.ref` is `sym.id`, the bare reference. `p.name` is the library pin's name (`NavigatorPanel.tsx:18`, `p.pin.name`).

**How to see it.** A dual op-amp U1 with both units placed. Highlight the net of unit B's output.
- KiCad lists `Symbol 'U1B' pin '7' (~)`.
- Ours lists `Symbol 'U1' pin '7' (~)`.

A pin that picked an alternate is listed by its library name. The sort order follows the text, so it shifts too.

**Test.** `netNavigator.test.ts`: `a_pin_of_a_multi_unit_symbol_is_listed_with_its_unit_and_alternate`. Input: a symbol with unit_count 2 placed as unit 2, and a pin with alternate `SDA`. Assert the text is `Symbol 'U1B' pin '5' (SDA)`.

**Severity.** Medium: a message. The r6 slice C "item texts" row checked the format string, but not its arguments.

**Already known?** No.

### A7-M2. Edit with Symbol Editor opens unit 1 in body style 1, not the placed symbol's unit and body style (medium)

**What differs.**
- KiCad 8303b2ad:
  - `symbol_edit_frame.cpp::LoadSymbolFromSchematic`:
    ```cpp
    m_unit = std::max( 1, aSymbol->GetUnit() );
    m_bodyStyle = std::max( 1, aSymbol->GetBodyStyle() );
    ```
  - Ctrl+Shift+E (`editLibSymbolWithLibEdit`) does the same through `symbolEditor->LoadSymbol( symbol->GetLibId(), symbol->GetUnit(), symbol->GetBodyStyle() )` (`sch_editor_control.cpp::EditWithSymbolEditor`).
- Ours:
  - `useActionRunner.ts:1487` calls `symApi.openSymbol(target.libId)`, and `symbolEditorStore.tsx:560` `openSymbol(libId)` takes no unit or style.
  - The editor opens on its default, unit A.

**How to see it.** Select U1C of a quad op-amp and press Ctrl+E. KiCad shows unit C, ours shows unit A. The same happens with a De Morgan symbol placed in body style 2.

**Test.** `editWithSymbolEditor.test.ts`: `symbolEditorTarget` returns `unit` / `bodyStyle` from the placed symbol, and the action passes them on.

**Severity.** Medium: a wrong default.

**Already known?** No. C6-M3 covered selection and the broken link only.

### A7-L1. Compare Symbol with Library: the 2-unit tolerance is applied in nanometres, not schematic units, and a polyline sorts by its last point (low)

**What differs.**
- KiCad works in schematic internal units: `SCH_IU_PER_MM = 1e4`, so 1 IU is 100 nm (`base_units.h:74`). `EDA_SHAPE::Compare`'s `#define EPSILON 2` is therefore 200 nm, and `SCH_ITEM::compare` tests positions exactly at 100 nm.
- `symbolDiff.ts:41` converts to nanometres (`x * 1e6`), and `:137` keeps `EPSILON = 2`. Two shapes 100 nm apart are "Graphic item differs" in ours, alike in KiCad. A pin 50 nm away is "Pin %s differs" in ours only.
- A `SHAPE_T::POLY` has `m_start` / `m_end` left at (0,0) by the parser. KiCad's `TEST_PT( m_start ..); TEST_PT( m_end ..)` is a tie, and the order falls to the vertex count, then the vertices. Ours sets `start = pts[0]` and `end = pts[last]` (`shapeOfResolved`), so it sorts by the last vertex first. That pairs different polylines in the "Graphic item differs" lines.

**Test.** `symbolDiff.test.ts`:
- `shapes_150_nm_apart_are_the_same`: assert `[]`.
- `two_polylines_with_one_first_point_pair_by_vertex_count`.

**Severity.** Low: the report lines only. `iu()` should be `Math.round(x * 1e4)`.

**Already known?** No.

### A7-L2. Unfold from Bus: R / Shift+R does not turn the member's label while it follows the cursor, and the turn is not remembered (low)

**What differs.**
- KiCad 8303b2ad `sch_line_wire_bus_tool.cpp::doDrawSegments`:
  ```cpp
  else if( evt->IsAction( &SCH_ACTIONS::rotateCW ) || evt->IsAction( &SCH_ACTIONS::rotateCCW ) )
  {
      if( m_busUnfold.in_progress )
      {
          m_busUnfold.label->Rotate90( evt->IsAction( &SCH_ACTIONS::rotateCW ) );
          busUnfoldPersistentSettings.label_spin_style = m_busUnfold.label->GetSpinStyle();
  ```
  The next unfold starts with that spin (`doUnfoldBus`: `SetSpinStyle( busUnfoldPersistentSettings.label_spin_style )`).
- Ours: `SchematicView.tsx:531-536` commits `add_label` with no spin, and nothing handles R during the unfold. The label's ghost is not drawn either, which PARITY §50 already lists.

**Severity.** Low.

**Already known?** Partly: the ghost is listed, the rotation is not.

### A7-L3. Hierarchy Navigator: KiCad has a project row above the root sheet's row, and every sheet row reads "name (page N)" (low, cosmetic)

**What differs.**
- KiCad 8303b2ad `hierarchy_pane.cpp::UpdateHierarchyTree`:
  ```cpp
  wxTreeItemId projectRoot = m_tree->AddRoot( getRootString(), 0, 1 );
  // Don't set item data for the project root - it doesn't correspond to a real sheet
  ...
  wxString sheetName = formatPageString( sheetNameBase, m_list.GetPageNumber() );
  wxTreeItemId topLevelItem = m_tree->AppendItem( projectRoot, sheetName, 0, 1 );
  ```
  `formatPageString` returns `aName + " " + "(page %s)"`.
- Ours: `HierarchyPanel.tsx:95` makes the root sheet's own row show the project's name (`hierarchyRowName`). Each row shows the name in bold, with "file · page N" under it.

**Severity.** Low, cosmetic.

**Already known?** Partly: the module comment says "here the name, then the file and the page".

### A7-L4. The cache's library user fields are written hidden at (0, 0) with default effects (low, cosmetic)

**What differs.**
- `crates/kicad/src/lib.rs:1385-1387` writes `write_property(out, name, text, 0.0, 0.0, true)` for every user field.
- KiCad's `SaveSymbol` writes each field as the library has it: place, visibility, effects, `show_name`. Only "Compare Symbol with Library" against the cache, or a KiCad user looking at the cache, would see the difference.
- The same writer puts `Reference "U"` (`:1382`) in place of the library's prefix (`R`, `C`, ...), and no Footprint, Datasheet, Description, `ki_keywords` or `ki_fp_filters`. The Description half of this is H3.

**Severity.** Low, apart from H3's Description.

**Already known?** No.

### Checked and found to match (slice A)
- **9d05af7a.**
  - `LibSymbol::sub_reference` against 25ec2335 `SCH_SYMBOL::SubReference`. The display name wins over the letter.
  - The cache writes `(unit_name ..)` in each `_<unit>_<style>` block, as `SaveSymbol` does after `(symbol "%s_%d_%d"`.
  - The reader takes it from any sub-symbol (`T_unit_name`).
  - The tracer applies it only where `unit_count > 1`, as `GetRef`'s `GetUnitCount() > 1`.
  - The QA root `issue21980` now matches in names.
  - Only the project settings are missing (H5).
- **ae5feeec.**
  - The end state is checked once, and the renames are unchecked, which is the contrast with `SCH_FIELD::Replace`.
  - A chain over two screens works, because `moving` holds `(screen, old)`.
  - The refusal of two references made into one is a stated Differs.
- **0b63e63d.**
  - 25ec2335 `conn_facts.cpp` keys `numbersByName[pin->GetShownName()]` on the placed pin. Ours counts by `placed_shown`.
  - The `name.name` in the rendered net name stays the library pin's (`libPin->GetShownName()`). Ours keeps `facts.shown`.
  - An alternate counts only where `SetAlt` would keep it: the library pin has it, and it is not the base name.
  - A later path's reference comes from `reference_under`.
- **f1ccd18c.** `T_instances`: `if( m_requiredVersion < 20250318 && CurStr() == "~" )` for both the value and the footprint. The reference is taken raw.
- **327bb669** against `SCH_EDIT_TOOL::DoDelete`:
  - each connectable item's `GetConnectionPoints` is read before the removal;
  - then, for each point, a junction that is deleted or no longer explicit goes through `DeleteJunction`;
  - then the commit's `LOCAL_CLEANUP`.
  
  The batch path defers through `pending_junction_merges`. The wire's points, the label anchor, the no-connect point, and the symbol's, power symbol's and bus entry's scene connection points are all read before the delete.
- **ddc91f1a:** the user fields reach the cache, so `${LOCATION}` resolves (QA `issue22286` matches). The way they are written is A7-L4.
- **d53c20b8:**
  - a label is listed under `net_name`;
  - a pin under its traced `net`;
  - power symbols under their net;
  - the plain `<` sort.
  
  The text is A7-M1.
- **7595d15c** against `doDrawSegments`:
  - `flipX` / `flipY` from `cursor_delta` against the entry's origin;
  - `SetStartPoint( entry->GetEnd() )`;
  - the first click sets `label_placed`, the label goes where the click was, and the wire carries on from the entry's end through the click;
  - `IsTerminalPoint` finishes on the first click too;
  - `getBusForUnfolding` uses the selected bus wherever the cursor is.
- **8685a920** against `EditWithSymbolEditor`:
  - `RequestSelection( { SCH_SYMBOL_T } )`;
  - `Front()`;
  - a non-symbol selection is trimmed to nothing (no hover);
  - `IsMissingLibSymbol` refuses with the exact message.
  
  The unit is A7-M2. Not ported: KiCad hands the key to Edit Table when there is no symbol ("Giant hack").
- **6c545baa** against `LIB_SYMBOL::Compare( .., ERC, r )` and `SCH_INSPECTION_TOOL::DiffSymbol`:
  - the order of the checks;
  - "Missing pin" / "Missing field" are never reached (`aRhs.GetPin` on `aRhs`'s own pins);
  - a mandatory field is found by id and a user field by name;
  - the placed fields are moved by `-symbol->GetPosition()`;
  - a private field differs, and a copy is never private;
  - `SCH_SHAPE_T` only;
  - every message word for word.
  
  The units and the polyline sort are A7-L1.
- **8ffb4585:** `CleanHistory`: unknown sheets are dropped, consecutive repeats are dropped, and the index goes to `begin` when 1 or fewer entries are left, else to the last; `wxCHECK( !sheets.empty() )`.
- **c7cee1cd:** `CheckLibSymbol`'s `!aSymbol->IsGlobalPower()`.
- **8cc119ae:**
  - `getRootString` falls back to "Schematic";
  - a nameless sheet is named by `wxFileName( GetFileName() ).GetName()`;
  - Expand All / Collapse All are in the menu.
  
  The row layout is A7-L3.

## Slice B: the 130 QA roots, on main f057b4ea

### B.1 Round 6's harness (project left out): 124 of 130 alike

The 6 that differ have the known causes, as round 6 said after its fixes:
- `ampli_ht`, `issue16915`, `test_multiunit_reannotate_4`, `test_multiunit_reannotate_5`: unannotated or duplicate references (§40, §42).
- `legacy_power`, `same_local_global_power`: `lib_name` copies (D5-M1).

By net names, only those 6 and `complex_hierarchy` differ. `complex_hierarchy` is the uuid hash of an unannotated name (r2 A-N13). `issue21980` and `issue22286` now match.

### B.2 With each root's `.kicad_pro` kept (`sweepp.txt`): 110 of 130

The 14 new differences:

| Root(s) | What differs | Cause |
|---|---|---|
| `netlists/issue14494`, `netlists/multinetclasses` | `undefined_netclass` 4 in ours, or 7 against 13; netlist `class="net_01"` becomes `class="net_01,Default"` | **H1** |
| `issue23403` | nets `unconnected-(C1-Pad1)_1`, `unconnected-(C1-Pad2)_1` missing | **H2** |
| `spice_netlists/{cmos_not, fliege_filter, npn_ce_amp, opamp, rectifier}` | `simulation_model_issue` in ours: "Simulation model library not found at '.../out/.kicad/diode.lib'" | **M6** |
| `netlists/complex_hierarchy`, `spice_netlists/legacy_{laser_driver, pspice, rectifier, sallen_key, sources}` | `lib_symbol_mismatch` in the original only | not a defect of ours: the original's project resolves its libraries; the derived one is checked without them. Its writer side is A7-L4 |

### B.3 Beyond nets: components, items and the project file

- **kicad-cli's netlist components** (`comps.txt`):
  - `Description` is lost on 45 roots, 302 components in all (**H3**);
  - an empty Value is written as the reference (`fliege_filter` `V1`, **M2**);
  - otherwise only the `Sheetfile` renames (`board.kicad_sch`, expected) and the unannotated-reference roots.
- **Item text over each hierarchy** (`items.txt`, plus a block diff of `api_kitchen_sink`):
  - **H4**: a text's `exclude_from_sim`, `href`, `italic` / `bold` / colour;
  - **M5**: a text box's font colour, a label's or directive label's font colour, hatch fills;
  - **L1**: added junction dots;
  - **L2**: the graphics-only `groups_load_save` is refused.
  - Variants (`issue22938`, `variants`) are a known model limit (GAPS: design variants).
  - The sheets' flags and looks are r2 B-L1 (left).
- **The derived `.kicad_pro`** (`pro.txt`) has no `schematic` section at all. See H5, H6 and M1.

### H1. A schematic-only import drops the project's net classes and their assignments (high: a file KiCad reads differently)

**What differs.**
- Ours:
  - `crates/cli/src/import_kicad.rs:279`: with no board, `None => (empty_board(), eda_model::ConstraintModel::default(), None, None)`.
  - `merge_project_net_classes` and the other project merges run only in the board branch (`:225-248`).
  - Only the ERC settings and exclusions are kept (`:356-357`).
  - The derived `board.kicad_pro`'s `net_settings.classes` is `["Default"]`, with no `netclass_assignments`.
- KiCad: the classes live in the `.kicad_pro` (`PROJECT_FILE` → `NET_SETTINGS`), and kicad-cli reads them for any schematic of the project.

**How to see it (run).** QA `netlists/issue14494` with its project. Its classes are `Default`, `net_01`, `net_02`, `usb_diff`, with assignments `{'/Y': 'net_01', '/subsheet_01/C': 'net_01'}`.
- kicad-cli on the original: `<net name="/Y" class="net_01">`, and no `undefined_netclass`.
- On the derived file: `class="net_01,Default"`, and 4 × "Netclass net_01 is not defined".
- `multinetclasses`: `undefined_netclass` 7 against 13.

**Test.** `import_kicad` tests: `a_schematic_only_import_keeps_the_projects_net_classes`. Import `netlists/issue14494/issue14494.kicad_sch` with its `.kicad_pro`. Assert:
- the derived `.kicad_pro`'s `net_settings.classes` names `net_01`, `net_02`, `usb_diff`;
- `netclass_assignments["/Y"] == ["net_01"]`.

**Severity.** High: the nets lose their class, and with it the widths and clearances the board takes from the schematic.

**Already known?** No. GAPS #3 covers the board path, and PARITY §39 l.1199 says the derived project "carries `net_settings.classes`", which holds only for the design's own classes.

### H2. A KiCad 10 project's top-level sheets other than the one opened are dropped (high: lost circuitry and nets)

**What differs.**
- KiCad 8303b2ad `files-io.cpp::OpenProjectFiles` loads every sheet of the project file, not just the one opened:
  ```cpp
  const std::vector<TOP_LEVEL_SHEET_INFO>& topLevelSheets = projectFile.GetTopLevelSheets();
  if( !topLevelSheets.empty() )
  {
      for( const TOP_LEVEL_SHEET_INFO& sheetInfo : topLevelSheets )
      {
          wxFileName sheetFileName( Prj().GetProjectPath(), sheetInfo.filename );
          ...
          SCH_SHEET* sheet = pi->LoadSchematicFile( sheetPath, newSchematic.get() );
  ```
  The list is `schematic.top_level_sheets` (`project_file.cpp:58`).
- Ours: `eda import-kicad x.kicad_sch` reads that file's hierarchy only. No code reads `top_level_sheets`; the only `grep` hits are the Eagle path's test (`import_kicad.rs:711`).

**How to see it (run).** QA `issue23403`. Its project lists `issue23403.kicad_sch` and `top_level_sheet_1.kicad_sch`, and both place `shared1.kicad_sch`.
- kicad-cli, with the project: C1 on two paths, with nets `unconnected-(C1-Pad1)` and `unconnected-(C1-Pad1)_1`, and the same for Pad2.
- Ours: one C1 and two nets, so the second top-level sheet's C1 is gone.

**Test.** `import_kicad` tests: `every_top_level_sheet_of_the_project_is_imported`. Import `issue23403`. Assert two placements of C1, and 4 nets in the kicad-cli netlist of the derived file.

**Severity.** High: missing parts and nets.

**Already known?** In part. The studio's lack of a multi-root model is known (C6-L2's "New / Delete Top-Level Sheet"; PARITY §972 for Eagle). That a KiCad project's own top-level sheets are silently dropped on import is not. The Eagle path's workaround (placing the sheets on a root) is one way.

### H3. A placed symbol's Description is never written, and the library cache has no Description, keywords or footprint filters (high: data loss)

**What differs.**
- Ours:
  - the reader keeps a placed symbol's `(property "Description" ..)` neither as a field nor anywhere else (PARITY §30 l.915: every property "but Reference, Value, Footprint, Datasheet and Description" is a user field);
  - the placed-symbol writer (`crates/kicad/src/lib.rs:460-482`, `eda_engine::fields::symbol_fields`) writes Reference, Value, Footprint, Datasheet and user fields;
  - the cache writer (`:1382-1387`) writes `Reference "U"`, `Value <name>` and the user fields.
  - `design.json` has the library's `description`, but it is never written.
- KiCad 8303b2ad:
  - `sch_io_kicad_sexpr.cpp::saveSymbol` writes every field, the mandatory `FIELD_T::DESCRIPTION` included;
  - `sch_io_kicad_sexpr_lib_cache.cpp::SaveSymbol` writes `GetFields( orderedFields )`, every field, then `saveDcmInfoAsFields` (`ki_keywords`, `ki_fp_filters`).

**How to see it (run).** QA `erc_multiple_pin_to_pin`.
- kicad-cli on the original: 8 lines with "Generic connector, single row, 01x01, ...", as `<description>` and `<field name="Description">`.
- On the derived file: 0.
- Over the sweep: 45 roots and 302 components lose `F:Description`, and 104 lose `libsource.description` (`comps.txt`).

**Test.** `crates/kicad` writer: `a_placed_symbols_description_is_written`. Import a file with `(property "Description" "X" ..)` on a placed symbol. Assert the derived text has it, on the symbol and in its cache entry.

**Severity.** High: the BOM's and the netlist's Description column comes back empty in KiCad.

**Already known?** No. GAPS l.235 lists the footprint's Description field (board side) only.

### H4. A free text's `exclude_from_sim` is dropped (a text box is always written `no`), so an excluded SPICE directive runs; `href`, bold, italic and colour go too (high: wrong simulation)

**What differs.**
- Ours:
  - `crates/kicad/src/sch_import.rs:655-666` reads a `(text ..)`'s content, `at`, the font size and the justification only. `SchematicText` has no other field.
  - The writer (`lib.rs:736-744`) writes `(at ..)`, `text_effects_str(size, justify)` and the uuid.
  - A text box is written with a literal `(exclude_from_sim no)` (`sch_extras_io.rs:215`), and its font colour is not kept.
- KiCad 8303b2ad:
  - `sch_io_kicad_sexpr.cpp::saveText` writes `KICAD_FORMAT::FormatBool( m_out, "exclude_from_sim", aText->GetExcludedFromSim() )`, then the effects with `(href ..)`, bold, italic and colour;
  - the SPICE netlister skips texts with `GetExcludedFromSim()`.

**How to see it (run).** `/tmp/findings-r7-schematic.x94C/textsim/`: QA `spice_netlists/rlc`, plus `(text ".param EXCLUDED=1" (exclude_from_sim yes) ..)`.
- `kicad-cli sch export netlist --format spice` on the original: only `.tran 1u 10m`.
- On the derived file: `.tran 1u 10m` and `.param EXCLUDED=1`.
- QA `api_kitchen_sink`'s `(text "A link" (exclude_from_sim yes) .. (italic yes) .. (href "https://kicad.org"))` comes back as a plain text.

**Test.** `sch_import` tests: `a_texts_exclude_from_sim_href_and_style_are_kept`. Round-trip the kitchen-sink text and assert the three tokens. Slow variant: the kicad-cli SPICE netlist of the derived `rlc` copy has no `.param EXCLUDED`.

**Severity.** High: a different simulation, and a link lost.

**Already known?** No.

### H5. The project's "subpart_id_separator" / "subpart_first_id" are not read or written, so units are named `VD1A` where KiCad names them `VD1.1` (high: net names and references differ)

**What differs.**
- KiCad 25ec2335 `schematic_settings.cpp`:
  ```cpp
  m_params.emplace_back( new PARAM<int>( "subpart_id_separator", &m_SubpartIdSeparator, 0, 0, 126 ) );
  m_params.emplace_back( new PARAM<int>( "subpart_first_id", &m_SubpartFirstId, 'A', '1', 'z' ) );
  ...
  wxString SCHEMATIC_SETTINGS::SubReference( int aUnit, bool aAddSeparator ) const
  {
      if( m_SubpartIdSeparator != 0 && aAddSeparator ) subRef << wxChar( m_SubpartIdSeparator );
      if( m_SubpartFirstId >= '0' && m_SubpartFirstId <= '9' ) subRef << aUnit;
      else subRef << LIB_SYMBOL::LetterSubReference( aUnit, m_SubpartFirstId );
  ```
  `SCH_SYMBOL::SubReference` uses it, with the separator before a display name too. `GetRef( &aPath, true )` uses it in every `Net-(..)` / `unconnected-(..)` name (`RenderPinNetName`'s `referenceWithUnit`) and in the reference drawn on the sheet.
- Ours:
  - `eda_model::annotate::unit_letters` everywhere: `LibSymbol::sub_reference` (`crates/model/src/symbol.rs:298`), `board.rs:561`, `text_vars.rs`, `painter.ts:1094`;
  - no `grep` hit for either key;
  - the derived `.kicad_pro` has no `schematic` section.
  - PARITY §50 says "The separator is KiCad's default, none, as before". That is an assumption, and the project can say otherwise.

**How to see it (run).** `/tmp/findings-r7-schematic.x94C/subpart/`: QA `issue1768` with `"schematic": {"subpart_id_separator": 46, "subpart_first_id": 49}`.
- kicad-cli: `unconnected-(VD1.1-K-Pad1)`, `unconnected-(VD1.3-A-Pad2)`.
- Our `design.json`: `unconnected-(VD1A-K-Pad1)`.
- The derived file, read by kicad-cli: `unconnected-(VD1A-K-Pad1)`, because the setting is gone.

**Test.** `sch_hierarchy_tests` (slow): `the_projects_unit_naming_names_the_nets`. Import that case. Assert the design has `unconnected-(VD1.1-K-Pad1)`, and the derived `.kicad_pro` has the two keys.

**Severity.** High: net names, and a file KiCad reads differently.

**Already known?** Partly: the separator assumption is stated in PARITY §50. `subpart_first_id` and the project file are not mentioned, and it is not in GAPS.

### H6. A schematic-only import drops the project's text variables (high: a file KiCad reads differently)

**What differs.**
- Ours: `library_table::import_project_setup`, which carries the project's `text_variables`, is called only in the board branch (`import_kicad.rs:263`). Without a board, `setup` is `None` (`:279`).
- KiCad resolves `${VAR}` from `PROJECT::GetTextVars()` (`text_variables` in the `.kicad_pro`).

**How to see it (run).** `/tmp/findings-r7-schematic.x94C/textvar/`: QA `issue1768`, plus `text_variables: {Version: V1.2.0}` and a text `Rev ${Version}`.
- kicad-cli ERC of the original: no `unresolved_variable`.
- Of the derived file: `unresolved_variable 1`.
- QA `issue19646`'s project has `ProjectName`, `Version` and `ProjectDescription`, and its derived project has no `text_variables`.

**Test.** `import_kicad` tests: `a_schematic_only_import_keeps_the_projects_text_variables`.

**Severity.** High: a file KiCad reads differently.

**Already known?** No. The test at `import_kicad.rs:527` covers the board path.

### M1. The derived project has no `schematic` section: Schematic Setup's formatting, inter-sheet references, annotation start, BOM, netlist and simulator settings of an imported project are lost (medium)

**What differs.**
- `pro.txt` compares the original's keys against the derived project's. The derived `board.kicad_pro` has only `board`, `erc` and `net_settings`.
- The non-default settings lost include:
  - `drawing.label_size_ratio` / `text_offset_ratio` (0.3 / 0.3 in 20 roots: KiCad draws every global label's box and text offset by them);
  - `drawing.default_text_size` 60 (`issue14657`, `legacy_sallen_key`);
  - `pin_symbol_size`, `default_junction_size`;
  - `intersheets_ref_show` true (`issue18299`, `issue18606`, a §39 "done" item that the import loses);
  - `annotate_start_num` 10 (`test_multiunit_reannotate_5`);
  - `net_format_name`;
  - `spice_save_all_voltages` / `currents`, `ngspice.model_mode`, workbook;
  - `bom_settings` / `bom_presets`;
  - `used_designators`.
- KiCad: `SCHEMATIC_SETTINGS`' `m_params` (`schematic_settings.cpp`), under `"schematic"` in the `.kicad_pro`.

**Severity.** Medium. Mostly looks and defaults, but annotation numbering and the inter-sheet references change what a user sees.

**Already known?** In part. GAPS l.542 lists the Formatting pages as not ported. That an imported project's values are dropped, and the inter-sheet references with them, is not listed.

### M2. An empty Value is written as the symbol's reference (medium)

**What differs.**
- Ours: `crates/kicad/src/lib.rs:956`:
  ```rust
  let value = if !sym.value.is_empty() { sym.value.clone() } else { part.value.clone().unwrap_or_else(|| sym.id.clone()) };
  ```
- KiCad writes the field as it is: `saveField`, with the empty text.

**How to see it (run).** QA `spice_netlists/fliege_filter`'s `V1` (`(property "Value" "" ..)`, instance `(value "")`). kicad-cli gives value `~` for the original and `V1` for the derived file.

**Test.** Writer: `an_empty_value_stays_empty`.

**Severity.** Medium: the BOM's and the netlist's value change.

**Already known?** No.

### M3. The sheet draws the unit suffix only when two units of the reference are on that sheet; KiCad draws it whenever the symbol has more than one unit (medium)

**What differs.**
- Ours: `painter.ts:1089-1094` sets `unitSuffix = (unitCounts.get(s.id) ?? 1) > 1 ? ... : ""`, where `unitCounts` counts placements of the reference on this sheet.
- KiCad 8303b2ad `sch_symbol.cpp::GetRef`: `if( aIncludeUnit && GetUnitCount() > 1 ) ref += subRef;`, which the Reference field shows.

**How to see it.** LM358: unit A on the root and unit B on a sub-sheet, or a quad op-amp with only unit A placed.
- KiCad draws `U1A` and `U1B`.
- Ours draws `U1` and `U1`.

**Test.** A painter unit test: one placement of a 2-unit symbol is drawn with its suffix.

**Severity.** Medium: a reference shown wrong in daily use.

**Already known?** No. The tracer's naming uses `unit_count > 1` and is right.

### M4. Pin Helpers' labels ignore the pin's alternate: the library pin's name and type are used (medium)

**What differs.**
- KiCad 8303b2ad `sch_selection_tool.cpp` (`ID_POPUP_SCH_PIN_TRICKS_*`) uses the placed pin's own shown name and type. `SCH_PIN::GetShownName` returns `m_alt`, and `GetType` the alternate's `m_Type`:
  ```cpp
  labelText = pin->GetShownName();
  ...
  pinType = pin->GetType();
  ```
- Ours: `crates/ops/src/sch_pin_helpers.rs:138-145` takes `p.name` and `p.electrical_type` from the library pin, with no look at `SchExtras::pin_alternate`.

**How to see it.** An MCU pin `PA9` with alternate `USART1_TX` (output) chosen. Pin Helpers > Global Label.
- KiCad: `USART1_TX`, output shape.
- Ours: `PA9`, the base type's shape.

**Test.** `sch_pin_helpers_tests`: `a_pins_label_says_its_alternate`.

**Severity.** Medium.

**Already known?** No.

### M5. Directive labels, labels and text boxes lose their font colour; a label field's bold, thickness and `show_name` are dropped; schematic shapes' hatch fills become solid (medium: looks lost)

**What differs** (block diff of QA `api_kitchen_sink`, original against derived).

| Item | Original | Derived |
|---|---|---|
| `label "B0"` | `(font (size 1.651 1.651) (color 255 0 254 1))`, and the field `(show_name yes)`, `(thickness 0.254) (bold yes) (italic yes)` | colour, `show_name`, bold and thickness gone (italic kept) |
| `netclass_flag` | `(font .. (color 236 104 255 1))` | colour gone |
| `text_box` | `(font (size 1.27 1.27) (color 10 9 37 1))` | colour gone |
| `rectangle` | `(fill (type hatch) ..)` | `(fill (type color) ..)` |
| `circle` | `cross_hatch` / `reverse_hatch` | `color` |

KiCad writes each of these: `saveText` / `saveField` with `EDA_TEXT::Format`, and `EDA_SHAPE` fill types in `saveShape`. The writer's 20250114 format version may be why the hatch types are mapped to `color`. KiCad 8303b2ad reads hatch at that version, so this needs checking against `parseFill`.

**Severity.** Medium per the brief's "data loss". Each is cosmetic on its own.

**Already known?** Partly:
- PARITY §39 l.1195: label fields re-autoplaced, left;
- §39 l.1161: a label field keeps "name, text, shown, italic";
- GAPS l.201: a sheet's border and fill.

The label, flag and text box font colours and the shapes' hatch fills are not listed.

### M6. A symbol's `Sim.Library` file is not carried into the derived project, so kicad-cli ERC reports `simulation_model_issue` and KiCad cannot simulate it (medium)

**What differs.** QA `spice_netlists/rectifier`'s `D1` has `Sim.Library "diode.lib"`, relative to the project.
- The derived `.kicad/` has no `diode.lib`, and the property is written unchanged.
- kicad-cli on the derived file: "Simulation model library not found at '.../out/.kicad/diode.lib' ... could not find base model 'DIODE1' in library 'diode.lib'".
- The same happens on 5 roots in `sweepp.txt`. The original's project sets `simulation_model_issue` to error, and ours keeps that severity.

KiCad resolves the path against the project (`SIM_LIB_MGR::ResolveLibraryPath`, `${KIPRJMOD}`).

**Test.** Import `rectifier` and assert either `.kicad/diode.lib` exists or the written `Sim.Library` resolves to the source's file.

**Severity.** Medium.

**Already known?** No. The Simulation Model dialog's absence (GAPS #34) is another matter.

### L1. The writer adds a junction at every T, so the derived file has dots the original did not (low, cosmetic)

**What differs.**
- `crates/kicad/src/lib.rs:621-645` writes a junction at each `eda_engine::geometry::wire_junction_points` point (three wire ends, or an end on another wire's span), on top of the file's own junctions.
- KiCad writes the junctions the screen holds.

**How to see it (run).**
- QA `issue17870`: 12 junctions become 13, a new one at (40.64, 72.39), where a wire end meets the span of another and a `+1V0` pin.
- QA `issue17771`: new junctions at (45.72, 57.15) on the root and (59.69, 54.61) in `sub3`.

The nets are alike, so this is electrically harmless here. But KiCad now treats these as explicit junctions: deleting the T wire in KiCad leaves the dot.

**Severity.** Low. It is a design choice (`schematic_missing_junction`) that changes imported files.

**Already known?** No.

### L2. A schematic with only graphics is refused on import (low)

`crates/ops/src/import_foreign.rs:67` fails with "'groups_load_save' holds no schematic: no symbol, no wire, no label, no sheet". It is used for KiCad files too. QA `groups_load_save.kicad_sch` has two rectangles, a text and a group, and KiCad opens it. A notes-only root sheet cannot be imported.

**Severity.** Low.

**Already known?** No.

## Slice C: rows no round had reviewed

- **Pin Helpers (§31)** against `sch_selection_tool.cpp`'s `ID_POPUP_SCH_PIN_TRICKS_*`:
  - the No Connect: `ClearSelection` after;
  - the Wire: one `GRID_WIRES` step by `pinOrientation`, `ENDPOINT` flags, the drag, and `Revert` on cancel;
  - the labels: the spin by orientation, the shape table (bidi, input, output, tri-state, unspecified as they are, the rest input), the sheet pin shapes with passive as input, the new labels selected, and `"%s_%s"` with `GetRef( &sheetPath )` and the number.
  
  These match. The alternate is M4. A sheet pin's label text is `GetShownText( &sheetPath, false )` in KiCad, with variables resolved; ours uses the raw name (`sch_pin_helpers.rs:157`), low. A later path's reference comes from `sym.id`, not `GetRef( &sheetPath )`, low.
- **The sheet's reference text:** M3.
- **The file's items** (slice B.3): tables (`issue23840`) and images (`api_kitchen_sink`, `issue19646/f28p65x_osc_reset_debug`) round-trip unchanged by block diff. So do rule areas, bezier, arcs, bus entries, no-connects, global labels (the `Intersheetrefs` field's tokens are reordered only) and hierarchical labels.
- **The project file of a schematic-only import:** H1, H5, H6 and M1.

## Convergence: what still differs from KiCad that a user would notice, ranked

1. **Opening a KiCad project's schematic loses project-level data** (H1, H2, H5, H6, M1, M6). This is the biggest gap left, and round 6's sweep could not see it because it removed the `.kicad_pro`. A single fix covers most of it: read the `.kicad_pro` in the schematic-only branch as the board branch does, read `top_level_sheets`, and keep the `schematic` section in the derived project.
2. **The writer drops item data:** H3 (Description on every symbol), H4 (texts: `exclude_from_sim`, `href`, style), M2 (an empty Value), M5 (colours, hatch fills, label field looks), and the known sheet flags and looks (r2 B-L1) and variants (GAPS).
3. **One library symbol per `lib_id`** (D5-M1): `lib_name` copies trace as the id's (2 QA roots), and Ctrl+E edits every placement (C6-M3's Differs).
4. **The unannotated-reference model** (§40/§42): `R?` is renamed `R?1`, and a duplicate reference is not kept (4 QA roots). The connectivity is otherwise the same.
5. **References shown on the sheet:** the unit suffix (M3) and the project's unit naming (H5).
6. **Smaller UI details:**
   - the Net Navigator's line (A7-M1);
   - Pin Helpers with alternates (M4);
   - Ctrl+E's unit (A7-M2);
   - the unfold label's rotation (A7-L2);
   - Compare Symbol's tolerances (A7-L1).
7. **Cosmetic only:** the Hierarchy Navigator rows (A7-L3), the cache's field looks (A7-L4), the added junction dots (L1).

**What now matches.**
- **Connectivity and net naming are converged.** All 130 QA roots trace to KiCad's pin groups and names, apart from the two model limits in 3 and 4. Round 6's naming fixes hold.
- **Round 6's editor ports match KiCad's code**, apart from the noted details: Delete's junctions, Replace All, `~` values, CleanHistory, the Symbol Checker note, the bus unfold posture, and Compare Symbol's rules.

## Summary

| ID | Finding | Severity | Confirmed | Known? |
|---|---|---|---|---|
| H1 | Schematic-only import drops the project's net classes and assignments | high | run (`issue14494`, `multinetclasses`) | no |
| H2 | A KiCad 10 project's other top-level sheets are dropped | high | run (`issue23403`) | in part (no multi-root model) |
| H3 | A placed symbol's Description, and the cache's Description, keywords and filters, are never written | high | run (45 roots, 302 symbols) | no |
| H4 | A text's `exclude_from_sim` is lost (text box always `no`): an excluded SPICE directive runs; `href`, bold, italic and colour lost | high | run (kicad-cli SPICE netlist) | no |
| H5 | The project's `subpart_id_separator` / `subpart_first_id` are ignored and not written (`VD1A` against `VD1.1`) | high | run (kicad-cli netlist) | partly (PARITY §50 assumption) |
| H6 | Schematic-only import drops the project's text variables | high | run (`unresolved_variable`) | no |
| M1 | The derived project has no `schematic` section (formatting, inter-sheet references, annotation start, BOM, simulator) | medium | run (`pro.txt`) | in part (GAPS l.542) |
| M2 | An empty Value is written as the reference | medium | run (`fliege_filter`) | no |
| M3 | The unit suffix is drawn only when two units are on the sheet | medium | code | no |
| M4 | Pin Helpers' labels ignore the alternate's name and type | medium | code | no |
| M5 | Font colours, hatch fills and label field looks are lost | medium | run (`api_kitchen_sink`) | partly |
| M6 | The `Sim.Library` file is not carried, so the derived file has `simulation_model_issue` | medium | run (5 roots) | no |
| A7-M1 | Net Navigator pin text lacks the unit and the alternate | medium | code | no |
| A7-M2 | Ctrl+E opens unit 1, body style 1 | medium | code | no |
| A7-L1 | Compare Symbol: EPSILON in nm, not 100 nm IU; polyline sort | low | code | no |
| A7-L2 | Unfold from Bus: no R on the label, spin not remembered | low | code | partly |
| A7-L3 | Hierarchy Navigator rows (project row, "(page N)") | low | code | partly |
| A7-L4 | Cache user fields hidden at (0, 0); `Reference "U"`; no Footprint or Datasheet | low | code | no |
| L1 | The writer adds T junction dots the original lacks | low | run (`issue17870`, `issue17771`) | no |
| L2 | A graphics-only schematic is refused on import | low | run (`groups_load_save`) | no |

QA sweep on main `f057b4ea`: **124 of 130** alike without the project (6 known causes); **110 of 130** with each root's `.kicad_pro` (H1, H2, M6, plus the original-only `lib_symbol_mismatch` of 6 roots, which is not ours).

Counts: **high 6, medium 8, low 6**.
