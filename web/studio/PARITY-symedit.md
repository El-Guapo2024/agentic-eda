# Symbol Editor parity with KiCad eeschema's Symbol Editor

Status: backend complete and tested (`cargo test`/`clippy` clean on
`eda-model`/`eda-ops`/`eda-kicad`/`eda-cli`). Frontend implemented and
passes `npm run typecheck && npm run test:unit && npm run build`, but
**not click-through verified** -- the in-app browser was unavailable this
session (task brief). Treat the UI as "should work, unverified" until
someone clicks through the list at the bottom.

## Architecture

Mirrors the Footprint Editor tab (GAPS.md #8) field-for-field:

- `design.symbol_library: Option<SymbolLibrarySection>` (new, additive
  `Design` field -- `crates/model/src/ir.rs`), holding `LibrarySymbol`
  entries keyed by `lib_id`. Distinct from the existing, read-only
  `eda_model::LibSymbol`/`SymbolGraphic`/`LibPin` (used by ~dozens of call
  sites: schematic rendering, ERC, the `.kicad_sch` exporter) for the same
  reason `LibraryFootprint`/`LibraryPad` are distinct from
  `crate::footprint::Footprint`/`Pad` -- see that type's own doc. New
  `LibrarySymbolGraphic`/`LibrarySymbolPin` add `id` (addressing) and
  `body_style` (DeMorgan) the read-only types don't have.
- `Domain::SymbolEditor`, its own undo/redo scope (`crates/ops`,
  `crates/cli/src/board.rs`'s `restore_domain`/`domain_tag`/`pop_snapshot`),
  proven by `board::tests::symbol_editor_undo_is_scoped_independently_of_pcb_and_schematic`.
  `board::load`'s overlay: a *published* symbol-library entry reaches
  `ConstraintModel::symbols` (`loading_overlays_only_published_symbols_onto_the_model`),
  so "Update Symbol on Board" actually updates a placed instance's
  rendering/ERC through the existing `model.symbol_of` resolution path --
  no separate propagation code needed.
- New `Cmd`s: `OpenSymbolForEdit`, `DeleteLibrarySymbol`,
  `EditSymbolProperties`, `UpdateSymbolOnBoard`, `AddSymbolPin`/`Move`/
  `Delete`/`EditSymbolPin`, `PushPinProperty` (Length/NameSize/NumberSize,
  mirroring `symbol_editor_pin_tool.cpp`'s three "Push Pin ..." menu
  items), `AddSymbolGraphic`/`Move`/`Delete`/`EditSymbolGraphic`,
  `EditSymbolText`. All validated (`crates/ops/src/tests.rs`, 16 new
  tests): empty pin number/type refused, unknown lib_id/pin/shape refused,
  `unit_count` must be >=1, editing a symbol never auto-publishes it.
- `crates/kicad/src/symbol_lib.rs::export_kicad_sym`: derived `.kicad_sym`
  writer, round-trips through this crate's own `parse_symbol_library`
  (3 tests) including a declared-but-empty unit (KiCad's own file format
  has no "unit count" field -- it's inferred from which `_<N>_1`
  sub-blocks exist, so an empty unit still needs its own sub-block) and
  an alternate body style (`_<N>_2` sub-blocks, only emitted when
  `has_alternate_body_style`).
- `GET /api/symbol?lib_id=`, `GET /api/symbol_editor/names`,
  `GET /api/symbol/export?lib_id=` (`crates/cli/src/studio.rs`) -- same
  three-route shape as the Footprint Editor's own.

**Not fixed, flagged separately**: `studio.rs::lib_symbol_json` (the
*existing*, read-only `/api/schematic` + `/api/symbol_library` endpoint)
emits `"stroke_mm"` where `api/types.ts`'s `LibGraphic.stroke_width` and
`body_style` expect different/absent keys -- a pre-existing wire-format
mismatch this session found but did not touch (out of this task's scope;
my own new `/api/symbol` endpoint serializes the *editable* type directly
via serde and does not share this bug).

## Frontend

`state/symbolEditorStore.tsx` (mirrors `footprintEditorStore.tsx`).
Internal canvas space is this app's usual µm/+y-down (not the wire
format's mm/+y-up) so `kicad-port/view.ts`/`gridHelper.ts` and
`components/schematic/transform.ts`'s `resolvePin`/`resolveLibPoint` are
reused unchanged; conversion back to mm happens once, at the `Cmd`
boundary. `components/symbol/symbolPainter.ts` reuses the schematic's own
placed-instance rendering code outright (`drawRealGraphic`/
`drawPinDecoration`/`drawPinText`, newly exported from
`components/schematic/painter.ts`) resolved with an identity transform --
the Symbol Editor's preview and a placed instance's own rendering share
one code path by construction, not just by intent.

`SymbolEditorCanvas.tsx`: select/move/pin/draw-shape/text tools, pin
auto-increment (`kicad-port/pinNumbering.ts`, ported+tested against
`crates/model/src/ir.rs`'s identical algorithm), orientation
Right/Left/Up/Down <-> `angle_deg` (`kicad-port/pinOrientation.ts`): the
file's angle as `parseSymbolPin` reads it, 0 Right, 90 Up, 180 Left, 270
Down (round 7, E7-S1: the earlier "confirmed against builtin symbols"
mapping was turned 180 degrees, so every label was the opposite one).
Only the active unit/body-style's own items (plus unit/style `0` =
shared) are shown/hit-testable, matching real eeschema. `R`/Shift+`R`
rotates a selected pin 90 deg CCW/CW. `PinPropertiesDialog.tsx` (full
field set per `dialog_pin_properties.cpp`, electrical-type/shape/
orientation combo orders confirmed against source), `PinTableDialog.tsx`
(one row per physical pin -- source's own row-merges-identical-pins
grouping is not ported), `SymbolPropertiesDialog.tsx`'s
`LibrarySymbolPropertiesDialog` (named to avoid colliding with the
existing placed-instance `SymbolPropertiesDialog`, same split
`FootprintPropertiesDialog.tsx` already has).

`useSymbolEditHotkey.ts`: **Ctrl+Shift+E** from a selected schematic
symbol opens it in the Symbol Editor -- confirmed against
`eeschema/tools/sch_actions.cpp`/`sch_editor_control.cpp`'s real
`editLibSymbolWithLibEdit` this session (not plain Ctrl+E, which is
`pcbnew`'s unrelated "Edit Footprint"; this task's own brief said Ctrl+E,
corrected here against source).

## Hotkeyed actions wired in the eeschema sweep (`docs/parity/UI-ACTIONS.md`)

Three of the Symbol Editor's hotkeyed actions were unwired; all three are now
registered in `useActionRunner.ts` (Symbol tab only) and checked in the browser pane:

| Action | Status | KiCad file:function |
|---|---|---|
| `P` Place Pin (`eeschema.SymbolDrawing.placeSymbolPin`) | differs (see "Differs from KiCad": one click, not `TwoClickPlace`'s two): arms the Pin tool (the hotkey again leaves it); each click opens Pin Properties on the new pin first (`SYMBOL_EDITOR_PIN_TOOL::CreatePin` -> `EditPinProperties`), Cancel places nothing, OK places it -- after `PlacePin`'s Synchronized Pins Mode question when the spot is taken (`pinPlacement.ts`, OK label "Place Pin Anyway"; a refusal drops the pin). The new pin has no number and no name, as `CreatePin` leaves them (`SCH_PIN::SCH_PIN( LIB_SYMBOL* )`; only Repeat Pin increments, below), the pin placed is `g_lastPin` (`state.lastPlacedPin`, for Insert), and the sticky template (`state.pinTemplate`, the C++ `g_LastPin*`: type, shape, orientation, length, name/number sizes, common-unit and common-body-style, visible -- no name, as KiCad's globals hold none) is saved by the dialog's OK (`EditPinProperties`, an edited pin's too, and a new pin's before `PlacePin`'s question, so a declined "Place Pin Anyway" keeps it) and carried to the next click; merely selecting a pin updates nothing (`kicad-port/symPinTemplate.ts`) | `symbol_editor_pin_tool.cpp::CreatePin`/`PlacePin`, `SYMBOL_EDITOR_DRAWING_TOOLS` |
| Ctrl+N New Symbol (`eeschema.SymbolLibraryControl.newSymbol`) | identical, except no "Overwrite" and no parent combo: `DIALOG_LIB_NEW_SYMBOL`'s name (empty -> "Symbol must have a name."; escaped the way `GetName` does with `saveAsSymbolName`) and its properties -- default reference `U`, units per package 1..64 with "All units are interchangeable" (unchecked, as the base dialog leaves it, so `CreateSymbol`'s `LockUnits( !aProps.unitsInterchangeable )` locks a multi-unit symbol unless it is checked), alternate body style (De Morgan), "Create symbol as power symbol" (excludes it from the BOM and the board and locks those two), pin name position offset (default 20 mil = 0.508 mm) with "Pin name inside" (checked: the offset, or 1 internal unit = 0.0001 mm for one of 0, `CreateSymbol`'s `SetPinNameOffset( 1 )`; unchecked: 0), "Show pin number text" and "Show pin name text". A name the project library already has is refused in the dialog with KiCad's own words, "Symbol '%s' already exists in library '%s'." (`symbolExistsMessage`), without the Overwrite the `new_symbol` verb cannot do; a parent symbol is the Derive from Existing Symbol dialog's question. OK is one undo step (`batch`: `new_symbol` + `edit_symbol_properties`). Registered on the Symbol tab only, so it does not shadow `common.Control.new` | `dialogs/dialog_lib_new_symbol.cpp`, `symbol_editor/symbol_editor.cpp::SYMBOL_EDIT_FRAME::CreateNewSymbol` (`kicad-port/symEditActions.ts::newSymbolNameCheck`); `Cmd::NewSymbol` |
| Ctrl+Shift+S Save Library As (`eeschema.SymbolLibraryControl.saveLibraryAs`) | partial (2026-11-05, section "Saving into the libraries of the tables"): the target library (the project library too) written to a new `.kicad_sym` with a row of the project's table, or the library's row pointed at it. The download of the project library (`GET /api/symbol_library/export`) is no longer behind this action | `symbol_editor/symbol_editor.cpp::SYMBOL_EDIT_FRAME::saveLibrary( aLibrary, aNewFile )` via `symbol_editor_control.cpp::Save` |

Fixed on the way: `GET /api/symbol?lib_id=...` did not percent-decode `lib_id`, so a symbol could never be shown (`Device%3AR` matched nothing) -- the studio's Symbol tab now loads symbols
(`crates/cli/src/studio.rs::query_value`).

## Library tree, drawing tools and view options (`eeschema.SymbolLibraryControl.*`, `eeschema.SymbolDrawing.*`, `eeschema.PinEditing.*`)

The Symbol tab has the library tree (`SYMBOL_TREE_PANE`, several symbols selectable with Ctrl/Shift) with the context menu
`SYMBOL_EDITOR_CONTROL::Init` builds, and its own menu bar: `eeschema/symbol_editor/menubar_symbol_editor.cpp` extracted into
`src/kicad/sym_menus.json` by `tools/extract-lib-editor-menus.js` (the tab used to show the board editor's menu). The studio's library is the
project library (`design.symbol_library`, entries keyed by `Lib:Name`): a symbol that only the intent, a library file or the built-in table
defines is read-only until it is opened (which makes an editable copy). Copy, Export, Duplicate and Save Copy As read any symbol
(`GET /api/library/symbol`); Delete and Rename need a project entry. The editor's view starts as KiCad's settings do
(`SYMBOL_EDITOR_SETTINGS`): electrical types and hidden pins shown, pin numbers not forced.

The tree lists the project library and every installed KiCad `.kicad_sym` library (223), one library at a time on expand and cached
(`crates/cli/src/library_index.rs`, `GET /api/library/index|items|all`: the top-level `(symbol "Name"` entries of the file, without parsing it);
opening an installed symbol copies it into the project library first (`put_library_symbol`), so the installed original is never touched. The frame is
KiCad's (`symbol_edit_frame.cpp`): the three icon toolbars are the symbol editor's `toolbars_symbol_editor.cpp` extracted into
`src/kicad/sym_toolbars.json`, each button with its KiCad icon; one whose action needs an unported subsystem (rotate, mirror, ...) is dimmed with its
reason (`src/kicad/editor_toolbar_support.json`). With nothing open the canvas is empty, with its grid and a hint.

| Action | Status | KiCad file:function |
|---|---|---|
| Edit / Cut / Copy / Paste / Duplicate / Delete / Rename Symbol | **done** | `symbol_editor_control.cpp`: `EditSymbol`, `CutCopyDelete`, `DuplicateSymbol` (Paste is `DuplicateSymbol( true )`: each clipboard symbol lands in the target library under a free name), `DeleteSymbolFromLibrary`, `RenameSymbol` (an existing name asks "Overwrite"). The clipboard holds the bare `(symbol ...)` forms (`kicad-port/symClipboard.ts`), as KiCad's own does; the editor also keeps its copy, so Paste works where the browser denies the clipboard. Verbs `put_library_symbol`, `rename_library_symbol`, `delete_library_symbol` |
| Save As / Save Copy As | **done** | `symbol_editor.cpp::saveSymbolCopyAs`: `SAVE_SYMBOL_AS_DIALOG` (name trimmed, spaces to `_`, `EscapeString( CTX_LIBID )`; "Save in library", New Library...), "Symbol '%s' already exists in library '%s'. Do you want to overwrite it?"; Save As opens the copy |
| Import Symbol / Export Symbol | **done** | `ImportSymbol` + `DIALOG_IMPORT_SYMBOL_SELECT` (check the symbols to bring in, "Resolve Import Conflicts": Overwrite or Skip each, Skip All / Overwrite All, "Imported %d symbol(s), skipped %d."), `ExportSymbol` (`<name>.kicad_sym`). The reader is `crates/kicad/src/symbol_import.rs` (`POST /api/symbol_library/parse`, the inverse of `export_kicad_sym`, round-trip tested): a derived symbol (`extends`) stays derived, its parents imported first (section "Derived symbols" below), a pin's alternate functions and text boxes are read, a bezier curve is read as a graphic (since 2026-11-05: section "Rotate, Mirror, beziers, the Properties pane and the carried items") |
| Export View as PNG | **done** | `SYMBOL_EDITOR_CONTROL::ExportView`: the canvas as `<name>.png` |
| Add Symbol to Schematic | **done** | `AddSymbolToSchematic`: the open symbol, at the unit being edited, becomes the schematic's symbol being placed (a symbol nothing follows yet is published first, "Update Symbol on Board") |
| Bulk Edit Symbol Fields | partial | `DIALOG_LIB_FIELDS_TABLE`: every project symbol of the library as a row (or, as Bulk Edit Related Symbol Fields, a root symbol and what is derived from it), edited in place, OK writes the changes as one undo step. The columns are the ones the library model keeps: Symbol Name, Reference, Value, Footprint, Datasheet, Description, Keywords, the user fields, Exclude From BOM / Board, Power Symbol (no simulation fields) |
| Synchronized Pins Mode | **done** | `ToggleSyncedPinsMode` and what it drives: a pin placed on one unit gets its image on every other (`CreateImagePins`), moving or editing a pin carries the matching pins of the other units (`SYMBOL_EDITOR_MOVE_TOOL::Main`, `EditPinProperties`), deleting a pin deletes its twin in each other unit, the same place, body style, type and name (`DoDelete`, `kicad-port/symEditItems.ts::deletePlan`), Pin Properties shows the mode's info bar ("Synchronized Pins Mode.  New pin will be added to all units." ..., "Exit sync pins mode") and tooltips, and "This position is already occupied by another pin, in unit %d" asks first (`PlacePin`). On by default for a multi-unit symbol. `kicad-port/symPinSync.ts` |
| Push Pin Length / Name Size / Number Size | **done** | `SYMBOL_EDITOR_PIN_TOOL::PushPinProperties` from the one selected pin (tool entries and the right-click menu); length goes through `SCH_PIN::ChangeLength` (the connection point moves along the pin) and only to pins of the same body style |
| Draw Lines / Draw Polygons | **done** | one tool in this KiCad (both `SHAPE_T::POLY`, `doDrawShape`): the first click begins the outline, each later click continues it (no zero-length segment), a double click or Enter ends it, Escape drops the shape and keeps the tool. Unfilled, as `m_lastFillStyle` is. Ending on the start closes it (the file has no closed flag: the last point repeats the first). `kicad-port/symPolyDraw.ts`, ported from `EDA_SHAPE::{begin,continue,calc,end}Edit` |
| Draw Text | **done** | `TwoClickPlace`: the first click opens `DIALOG_TEXT_PROPERTIES` (text, size, bold, italic, both justifications, horizontal / vertical, common to all units / body styles: since 2026-11-05 a library text keeps them), the text follows the cursor, the second click places it; a text with nothing printable is dropped |
| Move Symbol Anchor | **done** (fields fixed 2026-10-27, r3 E3-2) | `PlaceAnchor`: a click makes that point the origin (`symbol->Move( -cursorPos )`, `Cmd::SetSymbolAnchor`) and the view is re-centred by the same amount so the symbol does not move on the screen. `LIB_SYMBOL::Move` moves every item of `m_drawings`, the fields among them (`addField`), so the five mandatory fields (at their default places when the symbol stores none) and the user fields move with the body. Deliberate difference: KiCad's `PlaceAnchor` pushes no commit (it only calls `OnModify()`, in 8303b2ad and 25ec2335), so it cannot be undone; here it is one undo step, as every edit is one undoable `/api/cmd` verb |
| Show Pin Electrical Types / Show Hidden Pins / Show Pin Numbers | **done** | the left toolbar's view toggles: `PIN_LAYOUT_CACHE::GetPinElectricalTypeInfo` (the type's name beyond the connection point, `kicad-port/symPinText.ts`), `SCH_PAINTER::draw( SCH_PIN )` (a hidden pin is drawn in the hidden colour or not at all, and cannot be picked when not drawn). `m_ShowPinNumbers` is read only by the Symbol Viewer in this KiCad, so here it forces the numbers on where the symbol hides them (`pin_numbers_hidden`, which the canvas now honours, with `pin_names_hidden`) |
| Place > Rectangle / Circle / Arc, Edit > Pin Table, File > Symbol Properties | done | the same actions the schematic registers, driving this editor's tools and dialogs on this tab |
| Derive from Existing Symbol, Flatten Symbol, Update Symbol Fields, Bulk Edit Related Symbol Fields | **done** (2026-10-14) | section "Derived symbols" below |
| Show Hidden Fields, Show Pin Alternate Icons | **done** (2026-10-15) | sections "Symbol fields on the canvas" and "Alternate pin functions" below |
| Draw Text Boxes | **done** (2026-10-16) | section "The symbol text box and the Shape Properties dialog" below |
| Export Symbol as SVG, Display next / previous symbol | missing (reason recorded) | plots are kicad-cli's, the last two are the Symbol Viewer's |

Bugs found and fixed on the way: **a library symbol with a graphic made `design.json` unreadable** (opening `Device:R` for editing was enough: `starlark` turns
`serde_json`'s `arbitrary_precision` on for the whole build and serde's tagged-enum reader then reads every buffered fraction as a map --
`LibrarySymbolGraphic` is now read field by field, `crates/model/tests/library_symbol_graphic_wire.rs`); the server omits a pin's unit and body
style when they are 1 (and empty text fields), so no pin ever matched the unit being edited -- nothing was drawn or pickable
(`kicad-port/libraryDefaults.ts`); a symbol a few millimetres across was blown up to the whole canvas; the editors' toolbars overlapped.

## Grid overrides and click versus drag (GAPS.md item 14, 2026-10-09)

| Behaviour | Status | KiCad file:function |
|---|---|---|
| A press becomes a drag after more than 8 px along an axis (on macOS also a motion after 300 ms held); a click is at the press | identical | `tool_dispatcher.cpp:handleMouseButton` -- `kicad-port/dragThreshold.ts`, `SymbolEditorCanvas.tsx` (`gestureRef`, `justPannedRef`); a move drag that ends where it started commits nothing |
| `Toggle Grid Overrides` (`Ctrl+Shift+G`, the toolbar button): a pin snaps to the connected-items grid, a shape and the anchor to the graphics grid, text to the text grid; what is picked up snaps on the coarsest of its items' grids. The symbol editor starts with connected items on 50 mil, text on 10 mil, graphics off, as the schematic does | identical for the grid; the pointer is not warped | `ee_grid_helper.cpp:GetItemGrid` / `GetSelectionGrid`, `symbol_editor_drawing_tools.cpp` (`GRID_GRAPHICS` for the anchor) -- `SymbolEditorCanvas.tsx` (`toolGrid`, `heldGrid`), `kicad-port/gridOverrides.ts` |
| Snapping to a pin's end or another item's anchor (`EE_GRID_HELPER::BestSnapAnchor`) | **done** (2026-10-14, section "Snapping to the pins of the symbol" below) | `SchGridHelper` (`kicad-port/schGridHelper.ts`) is wired into this editor too |

`window.__eda.state().view` is the library editors' view now (it was null on this tab), and the snapped coordinates are checked by real pointer events in `e2e/lib-editors.check.js` (the section below).

## Import Graphics (`symbol_editor_drawing_tools.cpp::ImportGraphics`, `graphics_importer_lib_symbol.cpp`; GAPS.md item 28)

File > Import > Graphics... (`eeschema.EditorControl.importGraphics`, the schematic's action: `SCH_ACTIONS::importGraphics` runs in this frame too) imports a DXF or SVG into the open symbol, through the same dialog as the schematic's (PARITY-sch.md section 20) and one command,
`Cmd::ImportSymbolGraphics` (`crates/ops/src/import_graphics_sch.rs`): one undo step in the Symbol Editor's own history. A line is a two-point polyline, a polygon a closed polyline (filled with the outline's colour when filled), a circle and an arc as they are, text as text, in millimetres with the library's Y axis up (the drawing comes out the way up it was drawn)
on the unit and body style being edited (`m_unit`; KiCad leaves the body style at "all"). A Bezier curve of the file is cut into segments at `ARC_LOW_DEF_MM` (20 micrometres), where KiCad keeps the curve (a symbol has beziers since 2026-11-05, but the importer does not make them yet); a polygon filled with a colour is filled with the outline's, the library fill having no colour. A text is anchored at the middle of its baseline here, so the file's horizontal justification
is applied to its position. The items are added at the cursor's grid point (interactive placement) or at "At" X, Y, and selected; they are not carried by the Move tool, which this editor has no such flow for -- drag them (KiCad previews them under the cursor first). Tests: `crates/ops/src/import_graphics_sch_tests.rs` (the symbol's: an outline in mm with Y up, unit and body style, scale and offset, text, an SVG curve, the refusals), the browser check `e2e/files-in.check.js` (a DXF at 10, 5 mm giving 4 polylines and 4 arcs spanning x 10 to 50, y -5 to 25, selected, one undo; an SVG outline as a closed polyline of its 0.1 mm stroke; interactive placement on the 1.27 mm grid).
The Symbol Editor accepts no dropped file (KiCad's `SYMBOL_EDIT_FRAME` has no `m_acceptedExts`): a drop is said not to be a file this editor opens.

## Derived symbols (`extends`; GAPS.md item 19, package P17, 2026-10-14)

A library symbol can name a parent (`LibrarySymbol::extends`, a `lib_id` of the same library, the link by name as KiCad's `(extends "Parent")` is) and keeps its own properties and fields (`LibrarySymbol::fields`:
`crates/model/src/sym_fields.rs`, each field with its place, size, justification, bold, italic, `show_name`, `do_not_autoplace` and hidden flag); it has no drawing of its own. The flat symbol --
what a derived symbol stands for -- is `SymbolLibrarySection::flatten` (`LIB_SYMBOL::Flatten`: the root's drawing and pins, each field and property of the nearest symbol of the chain that sets it), `GET /api/symbol?flatten=1`.

| Behavior | Status | KiCad file:function |
|---|---|---|
| Derive from Existing Symbol: the library tree's menu offers it on one selected symbol (its conditions), the dialog is `DIALOG_LIB_NEW_SYMBOL` with a parent -- name `<parent>_1` made unique, the four "keep" boxes (footprint, datasheet, user fields transferred, their content kept) -- and OK makes the symbol (`derive_symbol`), opens it flat; its row in the tree says "derived from <parent>" (KiCad nests it under the parent) | **done** | `symbol_editor_control.cpp::AddSymbol` (`CreateNewSymbol( parent )`), `LIB_SYMBOL::SetParent` |
| Flatten Symbol: the derived symbol becomes the flat symbol (`flatten_symbol`, one undo step); offered for a derived symbol only | **done** | `SYMBOL_EDITOR_CONTROL::FlattenSymbol`, `LIB_SYMBOL::Flatten` |
| Update Symbol Fields: `DIALOG_UPDATE_SYMBOL_FIELDS`'s list of the fields (the parent's and the symbol's own, checked), "Remove fields if not in parent symbol", "Reset fields if empty in parent symbol", "Update/reset field text", "... visibilities", "... sizes and styles", "... positions" (`update_symbol_fields`; the last four on to begin with); a symbol that is not derived is told "Symbol is not derived from another symbol." (KiCad's info bar, a toast here) | **done**; the action is enabled for any editable symbol, as KiCad's condition is | `SYMBOL_EDITOR_EDIT_TOOL::UpdateSymbolFields`, `DIALOG_UPDATE_SYMBOL_FIELDS` |
| Symbol Properties: the fields grid of `FIELDS_GRID_TABLE` -- Name, Value, Show, Show Name, H Align, V Align, Italic, Bold and behind "More columns" the text size, orientation, position and autoplacement; the five mandatory rows ("The first 5 fields are mandatory."), add, delete, up and down; a derived symbol lists the parent's fields it inherits in italics and grey (`This field is inherited from '<parent>'.`), and deleting one resets it to the parent's value; "Derive from symbol" and its parent combo; "Derived symbol must have a parent selected"; the name used twice, a field with text and no name and a root symbol whose reference has no letter are refused | **done** | `dialog_lib_symbol_properties.cpp` (`DIALOG_LIB_SYMBOL_PROPERTIES`), `fields_grid_table` |
| Bulk Edit Related Symbol Fields: the fields table (`DIALOG_LIB_FIELDS_TABLE`) with the scope "Related Symbols" -- the root and everything derived from it -- as well as the whole library, the Value, Footprint and user-field columns | **done** | `SYMBOL_EDITOR_CONTROL::ShowLibraryTable`, `DIALOG_LIB_FIELDS_TABLE::SCOPE` |
| A derived symbol opens flat, with the info bar KiCad shows ("...derived symbol... the root symbol", a link to the root) and refuses drawing and pin edits (`ops_derived_symbol`); deleting a base symbol asks first ("Deleting a base symbol will delete all symbols derived from it...") and takes what is derived with it; Rename on a parent keeps the children linked; Copy and Paste move the flat symbol (as KiCad's clipboard does), Duplicate keeps it derived; Save As into its own library keeps it derived with the parents it needs, Flatten and remove symbol inheritance saves the flat symbol; Import and Paste order parents first | **done** | `symbol_editor_control.cpp` (`DeleteSymbolFromLibrary`, `CopySymbolToClipboard`, `DuplicateSymbol`), `SYMBOL_SAVE_AS_HANDLER::DoSave`, `CheckSavingIntoOwnInheritance`, `CheckForParentalChainConflicts` |
| A published derived symbol is placed as the flat symbol (the schematic resolves the chain: the root's drawing, the datasheet of the nearest symbol that has one); an installed derived symbol opens flat; the `.kicad_sym` reader keeps `extends` and every `(property ...)` (place, size, justify, `hide`, `show_name`), the writer writes a derived symbol as `SaveSymbol` does | **done** | `SCH_IO_KICAD_SEXPR_LIB_CACHE::SaveSymbol`, `SCH_IO_KICAD_SEXPR_PARSER::parseSymbol` |

Tests: `crates/model/src/sym_fields.rs` 11 and `crates/ops/src/sym_derive_tests.rs` 10 (the chain, flatten, derive's four options, update's options, rename, delete, the refusals), `crates/kicad/src/symbol_import.rs` (a derived symbol read, written and read back), `crates/cli/src/lib_editors_tests.rs` (placed as the flat symbol, exported flat on its own and derived in the library, the names route, one undo step per verb); `kicad-port/{symFields,symFieldsGrid,symImportPlan,symSaveAsPlan}.test.ts` 28 cases;
11 scenarios of `e2e/lib-editors.check.js` (the tree menu, the dialogs, the banner, delete, Save As, copy and paste, rename). The proof against kicad-cli (`EDA_SLOW_TESTS=1`): a library of three symbols (`Base`, `Mid` derived from it, `Leaf` from that) written by this studio loads, `kicad-cli sym upgrade` keeps every symbol the same (parent, fields with their text, place and look, the footprint filters and keywords once flattened), and kicad-cli's SVG plot of the derived `Leaf` is its plot of the flat `Leaf` this studio's Flatten gives, hidden fields included.

**Approximations, and what is left.** The fields table has no simulation fields. A symbol's inherited footprint filters are written out in full by KiCad's own save (`saveDcmInfoAsFields` asks `GetFPFilters()`), ours only the symbol's own; the two agree once flattened.

## Snapping to the pins of the symbol (`EE_GRID_HELPER::BestSnapAnchor`; GAPS.md item 14, 2026-10-14)

The Symbol Editor's tools call the schematic's `SchGridHelper` (`kicad-port/schGridHelper.ts`) through `components/symbol/symbolSnap.ts`, fed with the pins of the unit and body style being edited (`kicad-port/symSnapItems.ts`: a pin is `SNAPPABLE | ORIGIN`;
a text and a field only offer their anchor as the point a drag holds, as `EE_GRID_HELPER::computeAnchors` does for items that are not connectable). A click of the Pin tool with the grid off goes to the end of another pin (the marker and the snap lines follow
the snapped cursor), with the grid on to the grid of its category (`GRID_CONNECTABLE`; overrides as before); Ctrl (Cmd on a Mac) turns the grid off, Shift turns the anchors off; a move starts from `BestDragOrigin` and follows the held cursor (`kicad-port/heldCursor.ts`; the pointer is not warped).
Tests: `kicad-port/symSnapItems.test.ts` 4; `e2e/lib-editors.check.js` ("Symbol Editor snapping": real pointer events on the canvas -- the pin tool goes to the end of another pin with the grid off and to the grid otherwise, with Shift it is on the grid, with Ctrl+Shift where the pointer is).

## Symbol fields on the canvas, Show Hidden Fields (`SCH_PAINTER::draw( SCH_FIELD )`; GAPS.md item 19, 2026-10-15)

The fields of the open symbol are drawn where `(property "Name" "text" (at x y angle) ...)` puts them: size, justification, bold, italic, a text at 90 degrees running up, `Name: text` when the name is shown, the unit's letter after the Reference of a multi-unit symbol (`SCH_FIELD::GetFullText`),
in the reference, value and field colours; a field that is not shown is drawn in the hidden-items colour only while View > Show Hidden Fields is on (`eeschema.SymbolLibraryControl.showHiddenFields`, with its check mark; on to begin with, as `SYMBOL_EDITOR_SETTINGS::m_ShowHiddenFields` is). A field is picked by a click (its box, `SCH_FIELD::HitTest`), moved by a drag on the text grid
with the rest of the selection (one `edit_library_symbol_fields`; a field a derived symbol inherits becomes its own), and a double click opens Symbol Properties on its row. Tests: `kicad-port/symFieldLayout.test.ts` 6; the scenario "Symbol Editor fields" of `e2e/lib-editors.check.js` (the ink of a shown and a hidden field, the toggle, a click, a drag with one undo step, the double click on the Value's row).

## Alternate pin functions (`togglePinAltIcons`, `DIALOG_PIN_PROPERTIES`; GAPS.md item 19, 2026-10-15)

| Behavior | Status | KiCad file:function |
|---|---|---|
| A pin keeps its alternates: `(alternate "name" type shape)` inside `(pin ...)`, read by name (a name given twice keeps its last definition, a nameless one is not read), written in the order of the names, nameless ones left out (`LibrarySymbolPin::alternates`, additive, empty by default) | **done** | `SCH_PIN::GetAlternates` (a `std::map`), `SCH_IO_KICAD_SEXPR_LIB_CACHE::savePin`, `parsePin` |
| Pin Properties' "Alternate pin function definitions": a turndown that starts open when the pin has alternates or was left open the last time, with the grid "Alternate Pin Name / Electrical Type / Graphic Style", + (a new row has no name and the pin's own type and style) and a delete for the selected row; OK refuses a row without a name ("Alternate pin definitions must have a name.") and the pane is disabled, with KiCad's tooltip, for a symbol with several body styles | **done**; the type and style are plain selects where KiCad's are icon popups | `dialog_pin_properties.cpp` (`ALT_PIN_DATA_MODEL`, `OnAddAlternate`, `TransferDataFromWindow`) |
| `add_symbol_pin` / `edit_symbol_pin` keep each name once (the last row stays), refuse an empty name, an unknown type or style, and any alternate on a symbol with several body styles ("Alternate pin assignments are not available for symbols with multiple body styles.") | **done** | the same |
| View > Show Pin Alternate Icons (`eeschema.EditorControl.togglePinAltIcons`, checked, on to begin with): the icon of two arrows and the quarter arc beside the name of a pin that has alternates -- `min( name size, 1.5 mm )` square, three quarters of its size past the name inside the body or before it over the pin, turned a quarter for a pin that runs up or down, in the pin name's colour | **done**; the icon is not part of the pin's selection box | `PIN_LAYOUT_CACHE::getUntransformedAltIconBox`, `drawAltPinModesIcon` (`kicad-port/symPinAltIcon.ts`) |

The Symbol Editor's pin names now follow the symbol's pin name offset (inside the body from the offset on, over the pin line at 0), which the icon's place depends on. Tests: `crates/ops/src/pin_alternates_tests.rs` 3, `crates/kicad/src/symbol_import.rs` (alternates read, written by name, a file as KiCad writes it), `kicad-port/{symPinAlternates,symPinAltIcon}.test.ts` 10; the scenario "Symbol Editor pin alternates" of `e2e/lib-editors.check.js` (the grid, the refusal, one undo step, the file, the icon's pixels and place, two body styles).
The proof against kicad-cli (`EDA_SLOW_TESTS=1`): a symbol with four alternates on a pin loads, `kicad-cli sym upgrade` keeps every alternate (name, type, style; in the order of the names) and the plot is the plot of the same symbol without them.
**Left:** the schematic's embedded copy of a placed symbol (`lib_symbols`) has no alternate functions, because the engine's resolved symbol (`LibPin`) has none: a placed pin is read from the engine's table.

## The symbol text box and the Shape Properties dialog (`SYMBOL_EDITOR_DRAWING_TOOLS::doDrawShape`, `DIALOG_TEXT_PROPERTIES`, `DIALOG_SHAPE_PROPERTIES`; GAPS.md item 19, 2026-10-16)

| Behavior | Status | KiCad file:function |
|---|---|---|
| The text box in the library symbol: `LibrarySymbolGraphic::TextBox` (two corners in the symbol's Y-up mm, the text, its angle 0 or 90, size, bold, italic, alignment, margin, border width, fill; additive); `.kicad_sym` `(text_box "text" (at x y angle) (size w h) (margins m m m m) (stroke ...) (fill ...) (effects (font (size s s) bold italic) (justify left top)))` read and written as KiCad does -- the box by its start and its size (the height negative when the box runs down), a line break as `\n` (a raw one made kicad-cli refuse the library: `sexpr_str` writes it escaped now and the reader decodes it), an `(effects ...)` without `(justify ...)` is centred and none at all left and top, a border of no width is the width -0.0001 | **done**; the four margins are one number (the greatest of a file's), the border's colour and style, a custom fill colour and `private` are not kept | `SCH_IO_KICAD_SEXPR_LIB_CACHE::saveTextBox`, `parseSymbolTextBox`, `parseEDA_TEXT`, `STROKE_PARAMS::Format` |
| Draw Text Box (`eeschema.SymbolDrawing.drawSymbolTextBox`, the toolbar button and the Place menu): the first click begins the box, the second opens Text Box Properties on it, OK adds it (one undo step) and Cancel drops it; the next box starts as the last one was (bold, italic, direction, left alignment, fill, border) | **done** | `doDrawShape` (`isTextBox`), `m_lastText*`, `m_lastStroke` |
| Text Box Properties: the text, the text size (0.01 to 1000 mm), Bold and Italic, the horizontal and vertical alignment, Horizontal or Vertical, Border with its width, Filled, and Common to all units / body styles (enabled for a symbol with several); OK makes a box as high as its wrapped text needs; a double click, `E` and Properties in the right-click menu open it on a box (`replace_symbol_graphic`, one undo step) | **done**; the font, the text and border colours, the border style, the hyperlink and Private are not part of a library text box here | `DIALOG_TEXT_PROPERTIES` (`TransferDataFromWindow`), `SCH_TEXTBOX::GetMinSize` |
| The box is drawn as `SCH_PAINTER::draw( SCH_TEXTBOX )` does on the symbol layer: the fill (the body's background or outline colour), the text wrapped to the box's width less the margins, the border when it has a width (0 is the default line width); a click anywhere in the box picks it; a drag moves both corners; Delete removes it | **done**; the wrapping is the sheet text box's (`kicad-port/schTextBox.ts`) | `SCH_TEXTBOX::GetDrawPos`, `HitTest`, `GetLegacyTextMargin` (four fifths of the text size on `LAYER_DEVICE`), `kicad-port/symTextBox.ts` |
| Shape Properties of a rectangle, circle, arc or polygon (a double click, `E`, Properties in the menu): "<Shape> Properties", Border and its width (0 is the default line width; unchecked is the width -0.0001), the Fill Style (do not fill, fill with the body's outline colour, fill with its background colour), Common to all units in symbol and Common to all body styles; a derived symbol's OK says "Read Only" (`edit_symbol_graphic` with the unit and body style, one undo step) | **done**; the border's colour and style, "Fill with:" a colour and Private to Symbol Editor are not modelled by the library symbol | `DIALOG_SHAPE_PROPERTIES` (`TransferDataFromWindow`), `kicad-port/symShapeProps.ts` |

Tests: `crates/ops/src/sym_graphic_tests.rs` 4 (the scope boxes, a text box added, moved and replaced with its id kept, every refusal, a derived symbol), `crates/model/tests/library_symbol_graphic_wire.rs` (the wire form and its defaults), `crates/kicad/src/symbol_import.rs` (a text box as KiCad writes it -- a real one from the standard library `Analog_ADC` -- read, written and read back), `kicad-port/{symTextBox,symShapeProps}.test.ts` 10, the scenarios "Shape Properties" and "Symbol Editor text box" of `e2e/lib-editors.check.js` (the tool, Cancel, OK, the file, the ink of the text and the border, a click inside, the tool's memory, the edit with its id kept, the refused size, a drag, three undo steps).
The proof against kicad-cli (`EDA_SLOW_TESTS=1`): a text box with no text plots exactly as the rectangle of its two corners (the start, the size and the Y-up mm are the file's own), three boxes of different looks (bold with a line break, vertical and italic with no border and a background fill, right-aligned with a margin of its own) load, `kicad-cli sym upgrade` keeps every one (text, corners, angle, size, bold, italic, alignment, margin, border, fill) and the plot of the file this studio wrote is the plot of KiCad's rewrite of it.
**Left:** a placed symbol gets a text box as what the engine's symbol type can hold -- its border and fill as a rectangle -- but not its text (`SymbolGraphic` has no text box, and the `.kicad_sch` writer bakes each graphic through the instance's transform); a text box of a library file read for a placed symbol (`symbol_lib::parse_symbol_library`) is not read; the wrapping of the text differs from KiCad's by the stroke font's metrics in places.

## P17 review: the Symbol Editor against KiCad's code and KiCad's own libraries (2026-10-23, `task-p17-review`)

An adversarial review of the sections above against KiCad 8303b2ad, with a practical check: symbols of the installed libraries (`/usr/share/kicad-nightly/symbols`; each with the
symbols it extends, as one library) read by the editor's reader (`parse_library_symbols`), written back by its writer (`export_kicad_sym_library_with`), and both re-saved by
`kicad-cli sym upgrade --force`; KiCad's two writes are compared node by node (`crates/kicad/tests/library_roundtrip.rs`, slow tier). Before the fixes, of 92 symbols (37 derived,
12 of several units) 78 were identical and 14 differed; after them, of 243 symbols (93 derived, 23 of several units, and the ones of known shapes: beziers, unit names, a private
field, a power symbol, a symbol excluded from the position files) all 243 are identical.

| Divergence found | Now | KiCad file:function |
|---|---|---|
| `ki_locked` (the units are not interchangeable) was read and dropped: 10 of the 92 symbols came back with interchangeable units | `LibrarySymbol::units_locked`, written after the fields; Symbol Properties' "All units are interchangeable" (on for several units only, `LockUnits( GetUnitCount() > 1 && !interchangeable )`); Synchronized Pins Mode starts off and stays off for such a symbol (`SynchronizePins`, `m_SyncPinEdit = IsMultiUnit() && !UnitsLocked()`); a new graphic is on the unit being edited only for it | `SaveSymbol`, `DIALOG_LIB_SYMBOL_PROPERTIES::TransferDataFromWindow`, `SYMBOL_EDIT_FRAME::SynchronizePins`, `SetDrawSpecificUnit` |
| An item common to every body style (`Name_1_0`) was written into `Name_1_1` (and copied into `Name_1_2`) | one sub-symbol per unit and body style that has items, as `GetUnitDrawItems` sorts them | `SaveSymbol` |
| `(power local)` was read as a power symbol and written `(power)`, a global one | `power_local`, written `(power local)` / `(power global)`; "Define as local power symbol" | `parseSymbol` (`T_power`), `SaveSymbol`, `SetLocalPower` |
| `(exclude_from_sim ..)`, `(in_pos_files ..)`, `(duplicate_pin_numbers_are_jumpers ..)`, `(jumper_pin_groups ..)`, a unit's `(unit_name ..)` were dropped | kept and written; "Exclude from simulation" and "Exclude from position files" in Symbol Properties (checked and disabled for a power symbol, `onPowerCheckBox`); the jumpers and unit names are kept but not edited | `SaveSymbol`, `onPowerCheckBox` |
| `(property private ..)` lost its flag; a graphic's stroke style (`solid` 830 times, `dash` 23 times in the libraries) became `default`; a text's pen, bold, italic, justification and hidden flag were dropped; a `(bezier ..)` was left out (98 in Auxiliary_Items, 4 in Device:Thermal_Jumper) | kept (`LibrarySymbolField::private`, `LibrarySymbolKeep::stroke_types`, `text_styles`, `items`) and written back; since 2026-11-05 a bezier is a graphic (drawn, edited, moved by Move Symbol Anchor) and a text's bold, italic and justification are the text's own (section "Rotate, Mirror, beziers, ...") | `saveField`, `STROKE_PARAMS::Format`, `EDA_TEXT::Format` |
| The writer's `(version 20231120)` reads a `~` as an empty text (before 20250318), so a datasheet `~` became empty; the reader took a pin name `~` for an empty one in any file | the writer's version is 20250324 and an empty pin name is `""`; the reader reads `~` as empty only in a file before 20250318 (a property's value, a pin's name and number) | `SCH_IO_KICAD_SEXPR_PARSER::parseProperty`, `parseSymbol` |
| Symbol Properties' lower panel had its own words ("In BOM", "On board", "Show pin numbers", "Pin name offset (mm)", "Has alternate body style (DeMorgan)") and no "Place pin names inside" | KiCad's groups and words: General, Attributes (the four "Exclude from" boxes), Pin Text Options ("Show pin number", "Show pin name", "Place pin names inside" -- an offset of 0 is outside, and inside with 0 is 20 internal units, 0.002 mm, `offset == 0 ? 20 : offset` -- "Position offset:"), Symbol Units ("Number of units:", "All units are interchangeable"), Body Styles (single, De Morgan) | `DIALOG_LIB_SYMBOL_PROPERTIES_BASE`, `TransferDataToWindow`, `TransferDataFromWindow` |
| Flatten took the excluded-from-BOM and -board flags of the nearest parent but not the simulation and position-file ones | all four | `LIB_SYMBOL::Flatten` |

Left: the unit display names grid, the Pin Connections tab (jumpers) and custom body style names are not in the dialog (their values are kept in the file); a graphic's `private`
flag and a stroke or fill colour are not kept; the order of a flattened symbol's fields is the parent's with the derived symbol's in place (KiCad removes the parent's field and appends
the derived one, ordered by ordinal). The schematic's own files are `(version 20250114)`, where a `~` is still an empty text.

Tests: `crates/kicad/tests/library_keep.rs` (the symbol cases), `crates/kicad/src/symbol_import.rs` `a_tilde_is_an_empty_text_in_a_file_before_20250318_and_itself_after`,
`crates/ops/src/lib_keep_tests.rs` (`LockUnits`, the local power, `Flatten`'s flags), `kicad-port/symPinSync.test.ts`, the slow `crates/kicad/tests/library_roundtrip.rs`, and the scenario
"Symbol Properties: All units are interchangeable ..." of `e2e/lib-editors.check.js`.

## Item edits on the Symbol tab (round 2, r2-editors-exports E2-E9, E12)

| Item | Status | KiCad source |
|---|---|---|
| Delete (Del, Backspace, the context menu) | **identical**: the selected pins and graphics in one commit, one undo step; in Synchronized Pins Mode the twin of each pin in every other unit too ("But only one pin per unit (matching)"). The Footprint Editor's Delete is one commit as well (`EDIT_TOOL::Remove`). A selected field is hidden in KiCad; the selection here holds no fields | `SYMBOL_EDITOR_EDIT_TOOL::DoDelete`, `kicad-port/symEditItems.ts::deletePlan` |
| Rotate (R, Shift+R) | **identical** for every kind since 2026-11-05 (section "Rotate, Mirror, beziers, the Properties pane and the carried items"); before, for pins only: one item turns about its own position, several about `GetNearestHalfGridPosition( selection.GetCenter() )` (the merged `GetBoundingBox()` of the selection: each pin's line, name and number as `PIN_LAYOUT_CACHE::GetPinBoundingBox` gives them, with the pin's own text sizes, and each selected graphic's box, of whatever unit and body style -- an imported body is `Name_0_1` / `Name_1_1` -- texts left out (`if( !item->IsType( { SCH_TEXT_T, .. } ) )`) and a selection of texts only taking the mean of their positions; `BOX2I::GetCenter` in integer IU, snapped with `KiROUND` in KiCad's y-down frame; r3 E3-3, 2026-10-27; r4 E4-1, `rotatePivot`), in one commit; the other units' pins are not touched. Shapes do not turn yet | `SYMBOL_EDITOR_EDIT_TOOL::Rotate`, `SCH_PIN::Rotate`, `rotatePlan` |
| Repeat Pin (Insert) | **identical**: the last placed pin again, 100 mil down for a horizontal pin and across for a vertical one (`m_Repeat.pin_step`), name and number `IncrementString( .., 1 )`, with `PlacePin`'s question and images in sync mode, one undo step "Repeat Pin". The last pin is kept by its id (`g_lastPin` is a UUID), so it is found, and repeated as it is now, after it was moved, renumbered or renamed; the repeated pin leaves its orientation, type and shape in the pin template (`PlacePin`: `g_LastPinOrient`, `g_LastPinType`, `g_LastPinShape`) (r3 E3-4, E3-5); the new pin is left selected (`selectionClear`, then `selectItem` of the pin; r4 E4-2) | `SYMBOL_EDITOR_DRAWING_TOOLS::RepeatDrawItem`, `SYMBOL_EDITOR_PIN_TOOL::RepeatPin`, `repeatPin` |
| Duplicate (Ctrl+D, the context menu) | **identical** (2026-11-05): the copies (each numbered pin `GetMaxPinNumber() + 1`, in `StrNumCmp` order of the old numbers) follow the cursor from where it was until a click places them, one undo step; Escape adds nothing | `SYMBOL_EDITOR_EDIT_TOOL::Duplicate`, `duplicateItems` |
| Cut / Copy / Paste (Ctrl+X/C/V, the context menu) | **identical** (2026-11-05): Copy puts the symbol with only the selected pins and graphics on the system clipboard as `.kicad_sym` text (the editor keeps the items too, for a browser that will not share the clipboard); Paste reads KiCad's text or ours -- a text that is not a symbol is pasted as a text -- on the unit and body style being edited unless common (`SetUnit( GetUnit() ? m_frame->GetUnit() : 0 )`), refused on a derived symbol, and the items follow the cursor until a click | `SYMBOL_EDITOR_EDIT_TOOL::Cut`, `Copy`, `Paste`, `pastedItems` |
| Pin Properties' OK | **identical**: a moved pin off the standard grid asks "This pin is not on a 50 mils grid which will make it difficult to connect to in the schematic.\nDo you wish to continue?" (Yes/No) -- KiCad tests `GetValue() % 50` in internal units, so ported as that (a step of 0.005 mm), not 50 mils; "Common to all units in symbol" is enabled for a multi-unit symbol only (unchecked otherwise, so OK puts the pin on the unit), "Common to all body styles" for a De Morgan symbol only, each with KiCad's tooltip | `DIALOG_PIN_PROPERTIES::TransferDataToWindow`, `TransferDataFromWindow`, `kicad-port/symPinDialog.ts` |
| A new symbol's Reference and Value | **identical**: at the origin (`LIB_SYMBOL::LIB_SYMBOL`'s `new SCH_FIELD( this, id )`), the Value the name | `Cmd::NewSymbol` |

The action audit (`tools/ui-parity-audit.mjs`) counts an action by name, not by tab: `common.Interactive.copy`/`paste`/`cut`/`duplicate` and
`eeschema.InteractiveEdit.repeatDrawItem` were counted as wired from the schematic while the Symbol tab had none; they are the Symbol tab's own
now (`actions/editorFrameActions.ts`), but the audit still cannot tell a tab that lacks one.

## Round 7: pin orientation, units and body styles, the Pin Table, the library names (`r7-editors-exports` E7-S1..S23; 2026-10-28, `task-r7-sym-editor`)

| Row | Status | KiCad file:function |
|---|---|---|
| Pin orientation (Pin Properties, the Pin Table, the pin tool's default) | **identical** (E7-S1): the label is the file angle's (0 Right, 90 Up, 180 Left, 270 Down); a new pin is Right, written at 0 (`g_LastPinOrient = PIN_RIGHT`). The stored angles did not change, so every file keeps its pins pointing the way they did; only the labels and the default were wrong | `sch_io_kicad_sexpr_parser.cpp::parseSymbolPin`, `pin_type.cpp::InitTables`, `symbol_editor_pin_tool.cpp` |
| A pin's length (Pin Properties, the Pin Table) | **identical** (E7-S2): `ChangeLength` -- the body end stays and the connection point moves by the change along the pin; Pin Properties takes the typed place, then the length (`SetPosition( newPos ); ChangeLength( .. )`) | `DIALOG_PIN_PROPERTIES::TransferDataFromWindow`, `PIN_INFO_FORMATTER::UpdatePin`, `SCH_PIN::ChangeLength` |
| A pin's Y (Pin Properties, the Pin Table) | **identical** (E7-S3): KiCad's internal Y, the file's negated (`parseXY( true )`; eeschema's `ORIGIN_TRANSFORMS` is the identity), so a pin at `(at 0 2.54 ..)` shows -2.54 | `DIALOG_PIN_PROPERTIES::TransferDataToWindow`, `PIN_INFO_FORMATTER::Format` |
| Symbol Properties' "Number of units" | **identical** (E7-S5, S6): fewer units ask "Delete extra units from symbol?" (Yes / No; No keeps the dialog) and delete every pin and graphic of the dropped units (and their display names); more units copy unit 1's items into each new unit; the canvas goes back to unit A when its unit is gone | `DIALOG_LIB_SYMBOL_PROPERTIES::Validate`, `LIB_SYMBOL::SetUnitCount( n, true )`, `RebuildSymbolUnitAndBodyStyleLists`; `crates/ops/src/library_editors.rs::set_unit_count` |
| Symbol Properties' body styles | **25ec2335's** (E7-S7): De Morgan on copies the body-style-1 pins into style 2 (`SetBodyStyleCount( 2, false, true )`); off asks "Delete extra body styles from symbol?" and deletes style 2. 8303b2ad asks but copies and deletes nothing (it calls `SetHasDeMorganBodyStyles` first, so `SetBodyStyleCount` returns at once), which 25ec2335 fixed | `Validate`, `TransferDataFromWindow` (25ec2335), `LIB_SYMBOL::SetBodyStyleCount`; `set_alternate_body_style` |
| Symbol Properties' "Symbol name:" | **identical** (E7-S8): shown unescaped, written `EscapeString( .., CTX_LIBID )`; "Symbol must have a name.", "Symbol name '%s' already in use in library '%s'." (any case); the rename is in the same undo step (`LIB_RENAME`), and a power symbol's Value follows the name as it is typed (`OnSymbolNameText`) | `TransferDataFromWindow`, `OnSymbolNameText` |
| The Pin Table | **identical** (E7-S10..S14): it edits copies of the pins; OK writes them as one undo step ("Edit Pins"), closing with changes asks "Save changes?" (Save / Discard Changes / Cancel), a derived symbol's OK is "Read Only"; the rows act on their own pins (no Synchronized Pins Mode); Unit is "ALL" and the units ("Unit A" or the display name), Body Style "ALL", "Standard", "Alternate", each only on a symbol that has several (else "ALL", not editable), and the verbs refuse a unit or body style the symbol has not (the writer would make a `Name_5_1` or drop the pin); Add Pin is `OnAddRow` (no number or name, unspecified, Right, unit and body style 0, or the last row's orientation, type, shape, unit and body style one `pin_step` from it) | `SYMBOL_EDITOR_EDIT_TOOL::PinTable`, `DIALOG_LIB_EDIT_PIN_TABLE` (`OnAddRow`, `OnClose`, `TransferDataFromWindow`), `PIN_INFO_FORMATTER`; `kicad-port/pinTable.ts` |
| Rename Symbol | **identical** (E7-S17): the name is trimmed (`RENAME_DIALOG`) and escaped (`A:B` is stored `A{colon}B`) | `SYMBOL_EDITOR_CONTROL::RenameSymbol` |
| A symbol name in use | **identical** (E7-S18): in any case, in the same library (`SymbolNameInUse`'s `CmpNoCase`): Paste and Duplicate name `R` `R_1` beside `r`, Rename and Save As ask about it, and the verbs refuse it (an overwrite replaces the other spelling) | `SYMBOL_LIBRARY_MANAGER::SymbolNameInUse`, `ensureUniqueName` |

Left, low (GAPS.md item 19): Pin Properties' Delete Pin button, its silent clamps and labels (E7-S4); the "Exclude from" boxes disabled on opening a power symbol (E7-S9); the Pin Table's summary, Count and text-size
columns, type names and sort (E7-S15); the Symbol Checker's unit letters (E7-S16); the library tree's E7-S21..S23 (E7-S19 and S20 are fixed: section "Saving into the libraries of the tables").

Tests: `crates/ops/src/sym_units_tests.rs` (5), `library_editors.rs` `a_symbol_name_is_taken_in_any_case`; `kicad-port/pinOrientation.test.ts`, `symPinTemplate.test.ts`, `symUnits.test.ts`, `pinTable.test.ts`,
`libraryNames.test.ts`; the scenario "r7: Symbol Properties' fewer units ..." of `e2e/lib-editors.check.js`.

## Saving into the libraries of the tables (UI-LEFT L1, Y7; 2026-11-05, `task-u1-libsave`)

The symbol the editor has open is the design's copy of a library's symbol (KiCad's library buffer); the library's own file is written by Save. The whole of it --
the writers, the routes, what differs -- is `PARITY-common.md` section 16; what the Symbol Editor shows of it:

| Row | Status | KiCad file:function |
|---|---|---|
| Save (Ctrl+S), Save All | **done** | `SYMBOL_EDIT_FRAME::Save` / `saveCurrentSymbol` / `saveAllLibraries`: the tree's library, else the open symbol's, into its `.kicad_sym` file or `.kicad_symdir` folder; "Symbol library '%s' is not writable." / "You must save to a different location." leads to Save Library As. The project library (`eda`) is the design's, saved as it is edited |
| The tree's ` *` | **done** | `SYMBOL_TREE_SYNCHRONIZING_ADAPTER::GetValue`: a symbol, and its library, not saved to the file yet |
| Save As / Save Copy As into a table library | **done** (E7-S19, E7-S20) | `saveSymbolCopyAs`: every library of the tables listed with its description; a read-only one refused ("Library '%s' is read-only. Choose a different library to save the symbol '%s' to."); the name tested against the library's file in any case, the symbol's own name too; New Library... makes a real library (`AddLibraryFile( true )`) and the dialog comes back on it. The copy is in the library's buffer until Save |
| New Symbol | **done** | `CreateNewSymbol` in `getTargetLib()` (the selected library row or symbol's library), its name tested against that library's file; Paste and Import take the same library |
| Revert | **done** | `SYMBOL_EDIT_FRAME::Revert`: the target symbol or library back to its file, one undo step; a library drops the symbols its file never had |
| Save Library As | **partial** | `saveLibrary( lib, true )`: see the table above and `PARITY-common.md` section 16 (no "Do not update library tables", no global table entry) |
| Left | -- | Delete and Rename of a library's symbol stay in the design (the file keeps the old one); E7-S21 (Duplicate opens the first copy at unit 1) |

## Rotate, Mirror, beziers, the Properties pane and the carried items (UI-LEFT Y1-Y5; 2026-11-05, `task-u1-symedit`)

Ported from KiCad 8303b2ad's `SYMBOL_EDITOR_EDIT_TOOL`, `SYMBOL_EDITOR_DRAWING_TOOLS`, `SYMBOL_EDITOR_PIN_TOOL`, `SCH_TOOL_BASE::Increment`, the items' own `Rotate` /
`Mirror*` and `SCH_PROPERTIES_PANEL`.

| Item | Status | KiCad file:function |
|---|---|---|
| Rotate (R, Shift+R) of every kind: pins, rectangles, polylines, circles, arcs, beziers, texts, text boxes and fields, one undo step. The pivot is the lone item's `GetPosition()` (a pin's position, a shape's start, a polygon's first vertex, a circle's centre, an arc's centre, a text's anchor, a text box's start, a field's anchor) or `GetNearestHalfGridPosition( selection.GetCenter() )` of several (the merged boxes of the pins, graphics and fields, texts left out). A text turns its position and toggles 0/90 whichever way it turns, a vertical one flipping its horizontal justification (`Rotate90( false )`); a text box toggles its text's angle; a field swaps left and right as `SCH_FIELD::Rotate` does. With something carried (a pin or text being placed, Paste's or Duplicate's items) it turns that | **identical** | `SYMBOL_EDITOR_EDIT_TOOL::Rotate`, `SCH_PIN::RotatePin`, `EDA_SHAPE::rotate`, `SCH_TEXT::Rotate`, `SCH_TEXTBOX::Rotate`, `SCH_FIELD::Rotate`; `kicad-port/symTransform.ts` |
| Mirror Horizontally / Vertically (X, Y, the toolbar, the context menu) of every kind about the same pivot: a pin's Right/Left (Up/Down) swap, an arc's ends swap (`EDA_SHAPE::flip`), a symbol's text mirrors its position and flips the justification across the mirror (`NormalizeJustification` before and after cancels), a text box flips its horizontal justification when its text runs across the mirror, a field mirrors its position; a field selected alone flips its justification only (`GetFlippedAlignment`). One undo step | **identical** | `SYMBOL_EDITOR_EDIT_TOOL::Mirror`, `SCH_PIN::MirrorHorizontallyPin` / `MirrorVerticallyPin`, `SCH_SHAPE::Mirror*`, `SCH_TEXT::Mirror*` (`LAYER_DEVICE`), `SCH_TEXTBOX::Mirror*`, `SCH_FIELD::Mirror*` |
| Beziers: `LibrarySymbolGraphic::Bezier` (start, c1, c2, end, stroke, fill), read as `parseSymbolBezier` and written as `formatBezier`; drawn with the curve (`DrawCurve`), hit and boxed by the curve's points at `ARC_LOW_DEF` (`RebuildBezierToSegmentsPointsList`), moved by Move Symbol Anchor, given a width and fill by Shape Properties ("Curve Properties"); a placed symbol draws its polyline. Draw Bezier Curve: `EDA_SHAPE`'s edit states (start; end with control 2; control 1; control 2), Enter or a double click ends it early; the point editor on a selected curve: four points and two indicator lines, a drag moves one point | **identical**; the point editor is ported for curves only (the other shapes are edited by their dialogs and drags) | `doDrawShape`, `EDA_SHAPE::beginEdit` / `calcEdit` / `continueEdit`, `EDA_BEZIER_POINT_EDIT_BEHAVIOR`; `kicad-port/symBezier.ts` |
| Text Properties of a plain text (double click, `E`, Properties in the context menu; a text is picked by its box, `SCH_TEXT::HitTest`, as a justified text's anchor is a corner of it): the text, "Text size:", bold, italic, both justifications, horizontal or vertical, common to all units / body styles; Shift+Enter is OK (`SCINTILLA_TRICKS` of a multi-line text). One undo step. The Text tool's first click opens the same dialog; the text then follows the cursor. A library text now keeps bold, italic and its justification (`LibrarySymbolGraphic::Text`, drawn as KiCad does: justified against its anchor, a fifth of the size for a bold pen) | **identical**; the font, the colour, the hyperlink and Private are not kept by a library text | `DIALOG_TEXT_PROPERTIES` (`m_isSymbolEditor`), `SYMBOL_EDITOR_EDIT_TOOL::editTextProperties`; `kicad-port/symText.ts` |
| The Properties pane, below the Libraries tree (Show Properties toggles it, shown to begin with): KiCad's rows of `SCH_PIN`, `SCH_SHAPE`, `SCH_TEXT`, `SCH_TEXTBOX`, `SCH_FIELD` and -- with nothing selected -- `LIB_SYMBOL` (Fields, Symbol Definition, Pin Display, Attributes, Units and Body Styles), in KiCad's groups and order as a library editor lists them (Unit, Body Style and Private shown; Locked not). A pin's Position Y is KiCad's internal Y. Edits are the editor's verbs in one batch, on its undo stack; the fields a row changes are one `edit_library_symbol_fields` | **identical** rows; read-only with the reason as the tooltip: a text's Font, Auto Thickness, Color and Hyperlink, a shape's Line Style, Line Color, Corner Radius and Fill Color, an arc's Start/End, a field's Unit and Body Style, Body Styles of the symbol, a hatched Fill; a text box's four margins are one value here | `SCH_PROPERTIES_PANEL`, `PROPERTIES_PANEL::rebuildProperties` (`isLibraryEditor`), the `*_DESC` registrations; `kicad-port/symItemProperties.ts` |
| The Pin tool (`TwoClickPlace` with `SCH_PIN_T`): the click opens Pin Properties first (`CreatePin`), the pin then follows the cursor on the connectable grid, the next click places it (`PlacePin`: the occupied-spot question in Synchronized Pins Mode, the images in the other units); Escape drops it, the tool stays | **identical** | `SYMBOL_EDITOR_DRAWING_TOOLS::TwoClickPlace`, `SYMBOL_EDITOR_PIN_TOOL::CreatePin`, `PlacePin` |
| Duplicate and Paste carry their new items from the cursor (`selection.SetReferencePoint( cursor )` then `SCH_ACTIONS::move`), a click places them as one commit and leaves them selected, Escape reverts (nothing added); a lone pin dropped asks `PlacePin`'s question | **identical** | `SYMBOL_EDITOR_EDIT_TOOL::Duplicate`, `Paste`, `SYMBOL_EDITOR_MOVE_TOOL::doMoveSelection` |
| Copy / Cut: the symbol with only the selected pins and graphics as `.kicad_sym` text on the system clipboard (`POST /api/symbol/clipboard/copy`), so a paste in KiCad takes it; Paste reads KiCad's own text (`ParseLibSymbols`), else pastes the clipboard as a text (at most 5000 characters) | **identical**; the clipboard's SVG and PNG images are not made | `SYMBOL_EDITOR_EDIT_TOOL::Copy`, `Paste` |
| Increment, Increment / Decrement Primary and Secondary, and Shift+Alt / Ctrl+Alt with the wheel: a selected pin's number when the cursor is on the number, else its name when it is on the name; a text's text; `STRING_INCREMENTER` with I, O, S, Q, X and Z skipped. Repeat Pin's step and label increment are Preferences' "Pitch of repeated pins" (a multiple of 25 mils) and "Label increment" | **identical**; Increment of the pin being placed (`TwoClickPlace`'s `ACTIONS::increment`) is not wired | `SCH_TOOL_BASE::Increment`, `SCH_SELECTION_TOOL::Main` (the wheel), `PANEL_SYM_EDITING_OPTIONS`; `kicad-port/symIncrement.ts` |

Opening a symbol of an installed library copies it into the project library first (`ensureSymbolInProject`); since 2026-11-06 a failed copy opens nothing and keeps the symbol
being edited, with `LoadSymbol`'s "Error loading symbol %s from library '%s'." and the reason (a read error used to be taken for "unknown" and a refused put ignored, so the
symbol opened blank; only the server's "no symbol with this lib_id" starts a blank one; `kicad-port/libraryEnsure.ts`, the sibling of the Footprint Editor's 23f4a206). The
read, the project's names and `open_symbol_for_edit` are requests of their own, and a command between them (another client deleting the design's copy) left the open verb, which
read no library, to start a blank symbol under the library's id that then stood in for the file. `OpenSymbolForEdit` of a `Lib:Name` the design has no copy of now puts the
library's symbol in the same step, as `SYMBOL_EDIT_FRAME::LoadSymbol` reads the library when it opens (`library_place::with_opened_symbol`, the sibling of e383c684's
`with_opened_footprint`; one undo step). A library or a symbol's text that is there and cannot be read (a truncated `.kicad_sym`, a folder library's file that does not parse)
is "Error loading symbol ..." with why (`library_search::read_installed_symbol`, `ops_bad_symbol`), from the read and from the open verb, and nothing is opened. Tests:
`library_save_tests.rs` (the interleaving forced: 0 pins without the fix, the file's pin and description with it; the unreadable file and folder).

A design written before keeps a text's bold, italic and justification in the kept text style (`LibrarySymbolKeep::text_styles`): `LibrarySymbol::absorb_kept_text_styles` moves
them into the text before an edit, a write and a `GET`, so nothing is lost and the edit's are the ones written. A bezier kept as text by the older reader stays kept (written back,
not drawn) until the symbol is read again from its file.

Tests: `kicad-port/{symTransform,symCarry,symBezier,symIncrement,symText}.test.ts`, `symPropertiesKicad.test.ts` (KiCad's registrations replayed, and the rows' behaviour),
`crates/model/tests/library_symbol_graphic_wire.rs`, `crates/kicad/tests/library_keep.rs` (the bezier, the text's looks, an older kept style), `crates/ops/src/sym_graphic_tests.rs`, the
copy route's test in `crates/cli/src/library_api.rs`; the slow `crates/kicad/tests/library_roundtrip.rs` samples every installed symbol with a bezier (9: 106 of 106 symbols identical
after `kicad-cli sym upgrade`); the browser check `e2e/symedit-edit.check.js` (9 scenarios: the mixed rotate and mirrors, the bezier and its handle, the pin's two clicks, Text
Properties, a pin row of the pane, Copy and Paste with KiCad's text, Save), whose saved library `kicad-cli sym export svg` plots (the curve as a cubic path). `e2e/lib-editors.check.js` and
`e2e/lib-edit.check.js` place their pins with the second click now (27/27 and 15/15).

Left: ~~the point editor of the other shapes~~ (done: section "The point editor of every shape", UI-LEFT-2 YY1); Increment of the pin being placed; the clipboard's images; Import Graphics' curves (still cut into segments); the Properties pane's rows the library symbol has no place for (above).

## The Pin Table, Pin Properties, Find and Replace, the tree, Export SVG (UI-LEFT Y6-Y8, X11; 2026-11-06, `task-u5-dialogs`)

KiCad 8303b2ad `dialog_lib_edit_pin_table.cpp`, `dialog_pin_properties_base.cpp`, `dialog_lib_symbol_properties.cpp::onPowerCheckBox`, `symbol_editor.cpp::DuplicateSymbol`, `sch_find_replace_tool.cpp`, `dialog_sch_find.cpp`, `ShowFindReplaceDialog`, `symbol_tree_synchronizing_adapter.cpp::GetAttr`, `SVGPlotSymbol`; KiCad 25ec2335 `JobSymExportSvg`, `GetSymbolPlotBBox`.

| Item | Status | KiCad file:function |
|---|---|---|
| Pin Table: the summary over every pin ("Pin numbers:", "Pin count:", "Duplicate pins:"); 14 columns, 8 shown to begin with (Count, Number, Name, Electrical Type, Graphic Style, Orientation, X Position, Y Position) and the header's menu for the rest; KiCad's type, style and orientation names; lengths in the frame's units; sort by Count then Number, a header click and its reverse; Group by name with a grouped row's renumbering ("1, 3" retyped adds, removes or renumbers pins); "-- mixed values --"; Refresh; the unit, body style and selection filters (KiCad's quirk kept: the unit filter's "ALL" keeps only pins common to all units); Export all or the shown pins to a `.csv` or the clipboard | **identical**. Differs: a typed length is a number with an optional unit (no `NUMERIC_EVALUATOR` arithmetic); no cell icons; a per-row Delete button | `PIN_INFO_FORMATTER::Format` / `UpdatePin`, `PIN_TABLE_DATA_MODEL::GetValue` / `SetValue` / `compare` / `RebuildRows` / `SortRows`, `OnColSort`, `OnAddRow`, `OnFilterCheckBox`, `updateSummary`, `PIN_TABLE_EXPORT::ExportData`, `CSV_WRITER`, `GRID_TRICKS::onGridLabelRightClick` |
| Pin Properties: KiCad's rows and words with colons, no Delete Pin, values taken as typed (r7 E7-S4) | **identical** | `DIALOG_PIN_PROPERTIES_BASE`, `TransferDataFromWindow` |
| A power symbol's Exclude boxes enabled on open; ticking "Define as power symbol" checks and disables them (E7-S9) | **identical** | `TransferDataToWindow`, `onPowerCheckBox` |
| Duplicate and Paste open the last new symbol at the unit and body style being edited; the tree selects the first (E7-S21) | **identical** | `LoadOneLibrarySymbolAux( .., GetUnit(), GetBodyStyle() )` |
| Find / Find and Replace (Ctrl+F, Ctrl+Alt+F, the toolbar, F3, Shift+F3) over every unit's fields, pins, texts and text boxes, sorted x then y; a hidden field matches only with Include hidden fields, the Reference never; a pin on its name or number; "Reached end of symbol. Find again to wrap around to the start." and a 4 s wrap; Replace changes a pin's name and number, one undo step, then Find Next; the matching is the schematic's on the server (`POST /api/find/text`: plain, whole word, wxRegEx) | **identical**. Differs: Replace All replaces (KiCad's `doReplace` stops at `wxCHECK_RET( aSheet )` in the Symbol Editor, which has no sheet path, in 8303b2ad and 25ec2335). Not ported: the find histories, brightening every match | `nextMatch`, `FindNext`, `ReplaceAndFindNext`, `getCurrentMatch` / `HasMatch`, `SCH_FIELD::Matches`, `SCH_PIN::Matches` / `Replace`, `EDA_TEXT::Replace`, `DIALOG_SCH_FIND` (constructor, `updateFlags`), `ShowFindReplaceDialog` |
| The tree: a derived symbol drawn in italics at its library's level (KiCad does not nest it under its parent) | **identical** | `GetAttr` (`SetItalic( !m_IsRoot )`) |
| The Symbol Checker's toolbar button | **live** | `CheckSymbol` |
| Export SVG with `--theme _builtin_default` (the editor's theme: the studio has no schematic theme setting) and the hidden pins and fields (`--include-hidden-pins`, `--include-hidden-fields`) | **identical** theme and hidden items. Differs: kicad-cli's plot box includes hidden fields; kicad-cli's pen is 6 mils | `SVGPlotSymbol`, `JobSymExportSvg`, `GetSymbolPlotBBox` |

Tests: `pinTableGrid.test.ts` (9), `symFind.test.ts` (5), `symImportPlan` (+1), `find_text_matches_and_replaces_with_the_schematics_matcher`, `the_symbol_svg_takes_a_builtin_theme_and_the_hidden_items`, slow `the_symbol_svg_route_plots_in_the_theme_it_is_given_with_the_hidden_pins`; browser checks `symedit-u5` 8/8, `lib-editors` 27/27, `lib-edit` 15/15, `symedit` 9/9.

Left: the Pin Table's Group Selected Pins, Import and the cross-probe highlight; Duplicate and Paste into a read-only library are not refused (KiCad: "Symbol library '%s' is not writable."); no highlight of every match.

## Symbol Properties' units, body styles and pin connections; a text and text box's colours, font and Private flag (UI-LEFT Y4; 2026-11-06, `task-u6-last`)

KiCad 25ec2335 `eeschema/dialogs/dialog_lib_symbol_properties.cpp` (`TransferDataToWindow`, `Validate`, `TransferDataFromWindow`, `OnBodyStyle`, `syncBodyStyleControls`, `updateUnitCount`, `OnAddBodyStyle` / `OnDeleteBodyStyle` / `OnBodyStyleMoveUp` / `Down`, `OnAddJumperGroup` / `OnRemoveJumperGroup`), `eeschema/lib_symbol.cpp` (`SetBodyStyleCount`, `PruneBodyStyleDrawItems`, `GetBodyStyleDescription`, `GetUnitDisplayName`), `common/jumper_group.cpp::JUMPER_GROUP::Make`, `eeschema/symbol_editor/symbol_edit_frame.cpp::RebuildSymbolUnitAndBodyStyleLists`, `eeschema/symbol_checker.cpp`, `eeschema/eeschema_jobs_handler.cpp::doSymExportSvg`; KiCad 8303b2ad `eeschema/dialogs/dialog_text_properties.cpp` (`TransferDataToWindow`, `TransferDataFromWindow`), `eeschema/sch_painter.cpp` (`getRenderColor`, `draw( SCH_TEXT / SCH_TEXTBOX / SCH_SHAPE )`), `eeschema/sch_io/kicad_sexpr/sch_io_kicad_sexpr_lib_cache.cpp` (`SaveSymbol`, `saveText`, `saveTextBox`), `sch_io_kicad_sexpr_parser.cpp` (`parseBodyStyles`, `parseSymbolText`, `parseSymbolTextBox`), `sch_io_kicad_sexpr_common.cpp::formatFill`, `common/stroke_params.cpp::STROKE_PARAMS::Format`, `common/eda_text.cpp::EDA_TEXT::Format`, `eeschema/tools/symbol_editor_drawing_tools.cpp` (`m_lastStroke`, `m_lastFillColor`).

The dialog's Units & Body Styles and Pin Connections parts follow 25ec2335 where it differs from 8303b2ad: 8303b2ad's `SetBodyStyleCount` copies the standard items into body style 2 only (three custom styles got nothing in the third; two to three copied into style 2 again) and erases everything above style 1 whenever the count drops, and its dialog called it after changing the flags; 25ec2335 fixed both and added the unit name and two-entry checks. The verbs already followed 25ec2335 for De Morgan.

| Item | Status | KiCad file:function |
|---|---|---|
| "Unit display names (optional):": a row per unit with its letters and its name; the rows follow "Number of units:"; OK keeps the non-empty names (`GetUnitDisplayNames().clear()`, then each row); a name in use by another row, case aside -- an empty row being its letters -- is refused ("The unit name '%s' is already in use.") | **identical** | `updateUnitCount`, `Validate`, `TransferDataFromWindow` |
| The toolbar's unit box names each unit by its display name, else "Unit A"; the Pin Table's and the Properties pane's Unit choices and the Symbol Checker's messages likewise (r7 E7-S16 fixed) | **identical** | `RebuildSymbolUnitAndBodyStyleLists`, `GetUnitDisplayName( i, true / false )`, `CheckDuplicatePins` |
| Body Styles: "Single body style", "'Standard' and 'Alternate' De Morgan body styles", "Custom body styles:" with a grid of names (Add, Move up, Move down, Delete; enabled for custom only); custom styles need two names ("Custom body styles must have at least 2 entries"); fewer styles than the symbol has asks "Delete extra body styles from symbol?"; De Morgan or custom on a symbol whose pins have alternate functions is refused ("Multiple body styles are not supported on symbols with alternate pin functions." with "(Pins %s.)") and the choice goes back to single | **identical**; the refusal is shown in the dialog's error line, not a message box | `OnBodyStyle`, `syncBodyStyleControls`, `Validate` |
| OK: `SetBodyStyleCount( 1 / 2 / names, .. )` from the symbol's own count -- each new style from the standard one (a De Morgan style its pins, custom styles every item), every item above the count erased, the items kept as text too -- then the flag and the names (`edit_symbol_units_and_connections`, after `edit_symbol_properties` in the dialog's one undo step); a symbol of custom styles is not changed by `edit_symbol_properties`' De Morgan flag being false | **identical** (25ec2335) | `TransferDataFromWindow`, `SetBodyStyleCount`, `PruneBodyStyleDrawItems` |
| `(body_styles demorgan)` / `(body_styles "Front" "Side" ..)` read and written; the toolbar's body style box (an empty entry, Standard and Alternate, or the names), the Pin Table's Body Style cell, the Properties pane's Body Style rows, "Common to all body styles" (`IsMultiBodyStyle`) and the Symbol Checker's messages (`GetBodyStyleDescription( .., true ).Lower()`) follow the custom styles; Export SVG takes kicad-cli's `_<name>` file of a custom style | **identical**. Differs: the file's version stays 20250324, where 8303b2ad's reader takes De Morgan from the `_N_2` sub-symbols whatever `(body_styles ..)` says (its custom names then show as Standard / Alternate); 25ec2335's reader, the box's kicad-cli, keeps the names (`!symbol->IsMultiBodyStyle()`). A placed symbol draws its second style as the alternate and has no third | `SaveSymbol`, `parseBodyStyles`, `parseLibSymbol`, `GetBodyStyleDescription`, `doSymExportSvg` |
| Pin Connections: "All pins with duplicate numbers are jumpers" and "Explicit jumper pin groups:" (Add, Remove); a row is cut at commas, spaces, tabs and line ends, each number a pin of the symbol ("Pin '%s' in jumper pin group %d does not exist in this symbol."), a group sorted with no repeats, an empty row no group; written `(duplicate_pin_numbers_are_jumpers yes)` and `(jumper_pin_groups ("1" "3") ..)` | **identical** (25ec2335). Differs: a pin's stacked numbers (`[1-4]`) are not spread (the writer's file version is before the notation) | `TransferDataFromWindow`, `JUMPER_GROUP::Make`, `HasPinNumber` |
| Text Box Properties: Font, text Color, Border with its width and Color, "Background fill" (`FILLED_WITH_COLOR`, else `NO_FILL`: a box filled with the body's background is one of the default colour after OK, drawn the same) and "Fill color:", "Private to symbol editor"; the next box takes the last box's border and fill colours | **identical**. Not here: the border Style (kept by the box's id as the file says it), the hyperlink (the schematic's only) | `DIALOG_TEXT_PROPERTIES::TransferDataToWindow` / `FromWindow`, `doDrawShape` |
| Text Properties of a symbol's text: Font, Color, "Private to symbol editor" | **identical** | same |
| `(text private "..")`, `(text_box private "..")`, `(stroke .. (color r g b a))`, `(fill (type color) (color r g b a))` (`COLOR4D::UNSPECIFIED` as `0 0 0 0`), `(font (face "..") .. (color ..))` read and written; a text that says "private" is no flag (the reader looked at the word as the text before, and the engine's reader drew it) | **identical**. Differs: an alpha is kept to 1/255 (0.5 is written 0.502) | `saveText`, `saveTextBox`, `formatFill`, `STROKE_PARAMS::Format`, `EDA_TEXT::Format`, `parseSymbolText`, `parseSymbolTextBox` |
| The canvas: a text box's fill colour (`LAYER_DEVICE_BACKGROUND`'s without one), border colour and text colour, a text's colour; a private text or text box is not drawn on a sheet (`LibrarySymbolGraphic::to_engine_graphics`, and the engine's reader of a library skips private shapes, texts and text boxes) | **identical**; an outline font's text is drawn in its face since 2026-11-06 (section "Texts in their faces" below; it was drawn with the stroke font) | `getRenderColor`, `draw( SCH_TEXTBOX )`, `IsPrivate() && !m_IsSymbolEditor` |

Tests: `crates/ops/src/sym_units_tests.rs` (4 new: the custom styles' populate and prune, the grids' rows, the JSON keys, a derived symbol refused), `crates/kicad/src/symbol_import.rs` (2 new: the text box's colours, font and Private and a private text; `(body_styles ..)`), `sch_export_api` (the custom style's SVG name); `kicad-port/symBodyStyles.test.ts` (7), `symText.test.ts` (+1); browser checks `symedit-props-u6` 5/5, `symedit-u5` 8/8, `symedit` 9/9, `lib-editors` (its text box scenario now expects the colour fill); the library round trip of KiCad's installed symbols and footprints (`EDA_SLOW_TESTS=1 cargo test -p eda-kicad --test library_roundtrip`) 2/2.

Left: a shape's Private flag and its border and fill colours (Shape Properties; the reader keeps a colour fill as the body's background); the text box's border style in the dialog; the schematic's Symbol Properties and Cycle Body Style for a third custom style (the engine's placed symbol has one alternate); embedded files and 25ec2335's Pin Map tab (named pin-to-pad maps of file version 20260629).

## The point editor of every shape (UI-LEFT-2 YY1; 2026-11-07, `task-v1-handles`)

KiCad's Symbol Editor runs the schematic's `SCH_POINT_EDITOR` (eeschema/tools/sch_point_editor.cpp at 8303b2ad, `m_isSymbolEditor`) on its `SCH_SHAPE` and `SCH_TEXTBOX` items;
the studio's port is the same module (`kicad-port/eePointEdit.ts`, PARITY-sch section 70), with the symbol's mm and +y up converted (`kicad-port/symPointEdit.ts`).

| Item | Status | KiCad file:function |
|---|---|---|
| Handles on one selected rectangle (the corners, the centre, the sides' middles), circle (centre, end), arc (start, middle, end, centre), polyline (each vertex and side) and text box -- the bezier's as before; a drag is one `replace_symbol_graphic`, one undo step | **ported** (only the bezier had them) | `makePointsAndBehavior`, the behaviours' `MakePoints` / `UpdateItem` |
| A rectangle's side takes the pins whose root is on it (not at its ends) along, of the rectangle's unit or every unit for a common one, each pin alone (no twin in Synchronized Pins Mode), with Preferences > Symbol Editor > "Keep pins attached when dragging edges" (on by default); a corner's drag takes none; the drag and the pins one batch, one undo step | **ported** | `RECTANGLE_POINT_EDIT_BEHAVIOR::dragPinsOnEdge`, `SYMBOL_EDITOR_SETTINGS::m_dragPinsAlongWithEdges`, `PANEL_SYM_EDITING_OPTIONS` |
| The arc's drag in the Symbol Editor's own arc edit mode (`m_ArcEditMode`, `editing.arc_edit_mode` of its settings); Ctrl+Space and the three `pointEditorArcKeep*` actions set it on the Symbol tab, and the context menu of one arc has Cycle Arc Editing Mode | **ported** | `SCH_POINT_EDITOR::changeArcEditMode` (`symCfg`), `Init`'s `arcIsEdited` |
| Create Corner / Remove Corner in the context menu of one polyline (the cursor on its outline; the point under the cursor on a vertex, more than two of them), one undo step; the polyline is an open chain, so its closing side is no candidate | **ported** | `addCornerCondition`, `addCorner`, `removeCornerCondition`, `removeCorner` |
| A derived symbol (`IsSymbolAlias`) has no point editor | **identical** | `SCH_POINT_EDITOR::Main` |

**Differs**: a symbol's rectangle has no corner radius here (`LibrarySymbolGraphic`'s rectangle holds none), so it has no radius point; a circle's end handle is on the radius to the right once a drag lands (the file keeps a radius).

**Left**: the Symbol Editor's Draw Arc keeps 8303b2ad's way (KiCad 25ec2335's five modes are the schematic's only here); a pin's own point editor does not exist in KiCad either.

Tests: `kicad-port/symPointEdit.test.ts` (the conversion both ways, `dragPinsOnEdge` with and without the setting, a corner's drag, the circle, polyline and arc, the Symbol Editor's own arc mode), `eePointEdit.test.ts`; browser: `e2e/handles-v1.check.js` (the rectangle's side taking the pin along and its corner not, a circle's radius, a polyline's vertex and Create Corner from the context menu, an arc's end, one undo step each).

## Texts in their faces, where KiCad draws them (UI-LEFT-2 PP1, CC1; 2026-11-06, `task-v1-text`)

The Symbol Editor's texts, fields and text boxes are laid out by the board's and the sheet's one layout (`kicad-port/kicadText.ts`, `schTextSpecs.ts`: `deviceTextDrawSpec`, `schFieldDrawSpec`,
`schTextBoxDrawSpec`; PARITY-pcb section 94, PARITY-sch section 72) and drawn in their face -- the stroke font, or the outline face the server's fontconfig gives (`SCH_PAINTER::draw( SCH_TEXT )`
on `LAYER_DEVICE`: the text at the centre of its box, at its left or right edge for a left or right justification; `draw( SCH_FIELD )`: a field centred on its box; the pen
`getTextThickness`'s). `window.__edaText.sym[id]` says what was drawn. The Properties pane's Font row is writeable for a text and a text box (`replace_symbol_graphic` with the font,
"Default Font" taking it off); a field's stays read-only (`FieldRow` keeps no font).

## The colour theme (UI-LEFT-2 SS9; 2026-11-07, `task-v2-prefs`)

The Symbol Editor draws in the schematic editor's theme while Preferences > Symbol Editor > Colors says "Use schematic editor color theme" (the default, `SYMBOL_EDITOR_SETTINGS::m_UseEeschemaColorSettings`), else in the theme chosen under "Use theme:" (`m_ColorTheme`), as `SYMBOL_EDIT_FRAME::GetColorSettings` picks it (KiCad 8303b2ad `eeschema/dialogs/panel_sym_color_settings.cpp`); its swatches are edited on Schematic Editor > Colors (PARITY-sch section 73), whose "Axes (symbol editor only)" row is the Symbol Editor's. Every item of the symbol is drawn in it, its own colours too unless the theme overrides them. **Differs**: the canvas keeps the studio's dark `#1a1a1a` background (KiCad: the theme's `LAYER_SCHEMATIC_BACKGROUND`), and the grid, the anchor and the snap guides keep the board's colours. Browser: `e2e/sch-color-theme.check.js` (the outlines in the schematic's theme, gone with "Use theme:" KiCad Default, back with the schematic's).

## Differs from KiCad

- ~~**Where a pin is placed.**~~ Since 2026-11-05 the Pin tool is KiCad's `TwoClickPlace`: the click opens Pin Properties, the pin then
  follows the cursor, and the second click places it, with `PlacePin`'s question at that click.
- **The New Symbol name field.** It is prefilled with the free "Untitled"/"Untitled_N" (so Ctrl+N then OK makes a symbol at once), where
  KiCad's starts empty, and we trim it before `GetName`'s escaping, which KiCad does not.

## Known gaps (not fixed, scope-bounded)

- DeMorgan alternate body style: authorable, exported to `.kicad_sym`, and (since the schematic item 12 work) shown by a placed symbol.
  `LibrarySymbol::to_engine_symbol` (the publish path) keeps the style-2 items as `LibSymbol::alternate` (shared items, `body_style` 0, are in
  both bodies), the `.kicad_sym` reader splits `Name_<unit>_<style>` sub-blocks the same way, and a placed symbol in style 2 (`SchExtras::
  body_styles`, Cycle Body Style) draws, exports and connects by that body (`PARITY-sch.md` section 1). Left: only the normal and the De Morgan
  pair are modelled for a placed symbol (a symbol of custom body styles, UI-LEFT Y4, is edited and written with every style, and a placed one draws its second as the
  alternate), and the symbol chooser's preview shows the normal style.
- ~~No "Alternate pin function definitions"~~ -- done 2026-10-15 (section "Alternate pin functions").
- ~~No dedicated Shape Properties dialog for symbol graphics~~ -- done 2026-10-16 for a rectangle, circle, arc and polygon (section "The symbol text box and the Shape Properties dialog"),
  and a plain text has its Text Properties since 2026-11-05. The Footprint Editor's graphics and texts have none.
- ~~Pin name offset (`pin_name_offset_mm`) is stored and exported but the canvas draws at the shared 0.508 mm~~ -- the Symbol Editor's canvas follows the symbol's offset since 2026-10-15 (names inside
  the body from the offset on, over the pin line at 0); the schematic's placed-instance renderer reads it from the symbol too.
- "Push Pin Properties" has its entry points now (tool and right-click menu, section above).
- ~~A new graphic or text is always on the unit being edited~~ -- since the P17 review (section "P17 review" below) it is on unit 0, common to all units, unless the
  symbol's units are locked (`m_drawSpecificUnit = UnitsLocked()` when a symbol is loaded and after Symbol Properties) or Shape Properties last said "not common".
- ~~No live ghost while a pin tool is armed~~ -- since 2026-11-05 the pin and the text follow the cursor after their dialogs, as in KiCad (which shows
  nothing before the first click either).

## Click-through needed (browser unavailable this session)

1. Open the Symbol tab; "Open from Library" should list builtin names
   (Device:R/C/L/D/LED, power:GND/PWR_FLAG) even with no real KiCad
   install. Open "Device:R" -- two pins should render with real
   decoration/text, rectangle body.
2. Select a placed resistor on the Schematic tab, press Ctrl+Shift+E --
   should switch to the Symbol tab and open its real lib_id.
3. Pin tool: click to place a few pins; numbers should auto-increment.
   Double-click one to open Pin Properties; change electrical type/
   orientation/length; OK; confirm the canvas updates.
4. Pin Table: open, inline-edit a cell, confirm it round-trips.
5. Draw a rectangle/circle/polyline/text on the body; move/delete one.
6. Symbol Properties: set Unit count to 2, confirm the toolbar's Unit
   selector grows; switch units and confirm pins/graphics on unit 1 are
   hidden while editing unit 2 (and a pin added with "common to all
   units" checked shows on both).
7. "Update Symbol on Board" after editing a symbol a placed instance
   uses; confirm the schematic canvas picks up the new pins/graphics.
8. "Export .kicad_sym"; confirm the download opens in a text editor as
   valid s-expression (or in real KiCad, if available).
9. Undo/redo on the Symbol tab must never touch the PCB/Schematic tabs'
   own history, and vice versa (same check GAPS.md #15/#8 already
   established for the other tabs).

## Round 2, task-v7-symedit (YY2, YY4-YY6)

UI-LEFT-2 YY2, YY4, YY5 and YY6, ported from KiCad 8303b2ad (`eeschema/sch_io/kicad_sexpr/sch_io_kicad_sexpr_common.cpp`, `sch_io_kicad_sexpr_lib_cache.cpp`, `sch_io_kicad_sexpr_parser.cpp`,
`common/eda_shape.cpp`, `common/eda_text.cpp`, `eeschema/sch_item.cpp`, `sch_shape.cpp`, `lib_symbol.cpp`, `sch_painter.cpp`, `dialogs/dialog_shape_properties.cpp`,
`dialogs/dialog_text_properties.cpp`, `dialogs/dialog_sch_find.cpp`, `tools/sch_find_replace_tool.cpp`, `tools/symbol_editor_drawing_tools.cpp`, `tools/symbol_editor_edit_tool.cpp`,
`tools/symbol_editor_control.cpp`, `tools/sch_tool_base.h`, `import_gfx/graphics_importer_lib_symbol.cpp`, `lib_fields_data_model.cpp`, `dialogs/dialog_lib_fields_table.cpp`,
`common/confirm.cpp`) and KiCad 25ec2335 where it is newer (`dialogs/dialog_lib_edit_pin_table.cpp`, `dialogs/dialog_lib_symbol_properties.cpp`).

**The looks beside an item.** What a library graphic or field has beyond its kind's members is kept beside it in the symbol's overlay (`eda_model::lib_keep::LibrarySymbolKeep::
graphic_looks`, `field_looks`; a text box's pen in `text_styles`; the line style in `stroke_types` as before): an overlay, so no literal of `LibrarySymbolGraphic` changes. Two verbs set
them as given, `set_symbol_graphic_looks` (an empty id is the graphic an `add_symbol_graphic` before it in the batch added, so Duplicate and Paste carry their originals' looks)
and `set_symbol_field_looks`; deleting a graphic takes its entries with it (an id is a hash of the content). Studio side: `kicad-port/symLooks.ts`.

| Item | Status | KiCad file:function |
|---|---|---|
| A shape's `private`, its stroke's `(color ..)`, its fill's colour and `hatch` / `reverse_hatch` / `cross_hatch` (a hatch is a colour fill with its hatch beside it), a rectangle's `(radius ..)`, a text's and a text box's `(href ..)`, a text box's `(thickness ..)`, a field's `(face ..)`, `(thickness ..)` and `(color ..)` read and written; a colour fill of a shape is its own colour now (it was read as the body background) | **identical** | `formatRect` / `formatArc` / `formatCircle` / `formatBezier` / `formatPoly`, `formatFill`, `STROKE_PARAMS::Format`, `EDA_TEXT::Format`, `saveField`, `parseFill`, `parseEDA_TEXT` |
| The Properties pane: Font, Color and Auto Thickness of a text, a text box and a field; Hyperlink of a text and a text box (a field's is masked); Line Style, Line Color, Corner Radius (at most half the shorter side, `VALIDATION_ERROR_TOO_LARGE`, clamped), Fill with the three hatches, Fill Color (writable for a colour fill only, `isFillColorEditable`), Fill Mode's "Fill color"; Private of a shape, a text and a text box; an arc's Start / End (the end moves about the kept centre, the middle recomputed as `GetArcMid`); Body Styles writable with KiCad's no-op setter. Each edit one undo step | **identical** | `EDA_TEXT_DESC`, `EDA_SHAPE_DESC` (`SetFillModeProp`, `SetCornerRadius`), `SCH_SHAPE_DESC`, `SCH_ITEM_DESC`, `EDA_SHAPE::SetStartX` .. `GetArcMid`, `EDA_TEXT::SetAutoThickness` / `GetEffectiveTextPenWidth`, `LIB_SYMBOL::SetBodyStyleProp` |
| Read-only, with the reason: a pin's Private and a field's Unit and Body Style | **differs**: KiCad lets them be set, and its file keeps none of them (`saveSymbolPin`, `saveField`), so KiCad loses them when the symbol is saved; the studio's design.json holds only what the file can say | `SCH_ITEM_DESC` |
| Shape Properties: the border's Color and Style (`lineTypeNames`; the default style opens as Solid, `DEFAULT_LINE_STYLE_LABEL`, and OK writes the one chosen), "Fill with:" and its swatch (`FILLED_WITH_COLOR`; picking a colour chooses the radio), "Private to Symbol Editor"; a hatch is none of the four radios and OK takes it away. Text Box Properties: the border's Style. One undo step with the dialog's verb | **identical**. Differs: the swatch of the outline and background fills does not show the layer's colour (OK keeps the shape's own fill colour then, which no file writes) | `DIALOG_SHAPE_PROPERTIES::TransferDataToWindow` / `FromWindow`, `onCustomColorSwatch`, `DIALOG_TEXT_PROPERTIES` (`m_borderStyleCombo`) |
| The canvas: a shape in its stroke colour and line style (`STROKE_PARAMS::Stroke`'s dashes), its hatch lines or its own fill colour (`LAYER_DEVICE`'s when unspecified), a rounded rectangle; a text box's border style; a field in its face and colour; a private shape is drawn in the editor and not on a sheet (`LibrarySymbol::to_engine_symbol`) | **identical** | `SCH_PAINTER::draw( SCH_SHAPE )`, `getRenderColor` |
| Pin Table: Group Selected Pins; the options menu's "Highlight on Cross-probe" and the cross-probed pin brightened (`LAYER_BRIGHTENED`), its unit and body style shown, the view moved when it is off it; Import From File... (the browser's chooser) and From Clipboard, Replace all / Append, KiCad's messages; Length, Number Text Size, Name Text Size, X and Y take arithmetic (`kicad-port/numericEvaluator.ts`); no per-row Delete (the trash under the grid deletes the selected rows); KiCad's icons in the type, style and orientation cells; the body style filter keeps the pins common to every body style | **identical** (25ec2335). Differs: the choice lists are the browser's, without icons; a pin behind a dialog is not counted as off the view | `OnGroupSelected`, `OnMenu`, `OnCellSelected`, `SYMBOL_EDIT_FRAME::FocusOnItem`, `EDA_DRAW_FRAME::FocusOnLocation`, `OnImportButtonClick`, `PIN_TABLE_IMPORT::ImportData`, `AutoDecodeCSV`, `PIN_TABLE_DATA_MODEL::SetValue` (`m_eval`), `OnDeleteRow`, `GRID_CELL_ICON_TEXT_RENDERER`, `RebuildRows` |
| Increment (`eeschema.Interactive.increment*`) of the pin or text the Pin and Text tools carry: it is the selection (`AddItemToSel( item )`), so a text's text, or a pin's number or name under the cursor, changes, and it goes on following the cursor; nothing is committed | **identical** | `TwoClickPlace`'s `ACTIONS::increment`, `SCH_TOOL_BASE::Increment` |
| Copy puts an SVG and a PNG of the selection beside the `.kicad_sym` text (one clipboard item; the SVG left out where the browser cannot hold one): the PNG drawn at the view's scale over the selection's box grown by 2% a side, 4096 px at most, on a transparent background; the SVG is kicad-cli's plot of the copied symbol (`POST /api/sym/svg` with `text`) | **identical** PNG. Differs: the SVG's page is kicad-cli's plot box of the copied symbol, not the selection's box (`plotSymbolToSvg` plots onto a page of the box); KiCad renders twice to work out the alpha, the browser draws on a transparent canvas | `SYMBOL_EDITOR_EDIT_TOOL::Copy`, `plotSymbolToSvg`, `renderSymbolToImageWithAlpha`, `clipboardBboxInflation`, `clipboardMaxBitmapSize` |
| Import Graphics: a curve of the file is a bezier of the symbol (four points), a degenerate one a line | **identical** | `GRAPHICS_IMPORTER_LIB_SYMBOL::AddSpline` |
| Paste Symbol and Duplicate Symbol into a library the tables lack ("Symbol library '%s' not found.") or one that cannot be written ("Symbol library '%s' is not writable."), and Duplicate with nothing selected ("No symbol selected") are refused before anything is read | **identical** words; shown in the studio's message line (KiCad's info bar) | `SYMBOL_EDITOR_CONTROL::DuplicateSymbol` |
| Find / Find and Replace: the combo boxes list what was searched and what replaced with (the latest first, ten, case counting; the dialog opens on the latest), kept while the editor is; every match brightened in `LAYER_BRIGHTENED` while the dialog is open, as the search changes or the symbol is edited, and nothing once it is closed | **identical** | `DIALOG_SCH_FIND::OnFind` / `OnReplace` / `SetFindEntries` / `SetReplaceEntries`, `SCH_BASE_FRAME::ShowFindReplaceDialog` / `OnFindDialogClose`, `SCH_FIND_REPLACE_TOOL::UpdateFind` |
| Symbol Properties' refusals in KiCad's "Error" box with OK (the fields, a derived symbol's parent, a unit name in use, two custom body styles, the body styles with alternate pin functions and "(Pins %s.)" under it, a name in use, the mandatory fields; "Symbol must have a name." as a plain message box) | **identical** | `DisplayError`, `DisplayErrorMessage`, `wxMessageBox`; `DIALOG_LIB_SYMBOL_PROPERTIES::Validate` (`m_delayedErrorMessage`), `OnBodyStyle`, `TransferDataFromWindow`, `OnDeleteField` |
| Bulk Edit Symbol Fields: Exclude From Simulation and Local Power Symbol, in `UpdateFieldList`'s order (Keywords, Exclude From BOM, Exclude From Simulation, Exclude From Board, Power Symbol, Local Power Symbol, then the user fields); Power off is `SetNormal`, Local Power off `SetGlobalPower` | **identical**. Differs: Reference and Description are shown (KiCad starts them hidden; the studio's table has no view controls) | `DIALOG_LIB_FIELDS_TABLE::UpdateFieldList`, `LIB_FIELDS_EDITOR_GRID_DATA_MODEL::setAttributeValue` |

Tests: `crates/kicad/tests/library_keep.rs` (+1: every look read, written in KiCad's place and read back), `crates/ops/src/sym_graphic_tests.rs` (+1: the verb, an empty id, the
defaults, delete), `crates/ops/src/import_graphics_sch_tests.rs` (the SVG curve is a bezier); `kicad-port/symPropertiesKicad.test.ts` (+3), `symShapeProps.test.ts` (+1),
`symFind.test.ts` (+1), `symClipboard.test.ts` (+1), `pinTableGrid.test.ts` (+6), `pinTableImport.test.ts` (5); browser `e2e/symedit-v7.check.js` 7/7, `e2e/pintable-v7.check.js` 5/5,
and `symedit-edit` 9/9, `symedit-u5` 8/8, `symedit-props-u6` 5/5 (its text box now writes `(type solid)`, as KiCad's OK does), `lib-editors` and `files-in` (refusals read from the message box).

**Left**: ~~a placed symbol does not draw a shape's colours, style, hatch or radius, nor a field's face (the engine's `SymbolGraphic` has none of them), and the schematic's
`lib_symbols` cache does not write them; a private shape is left out of that cache (KiCad writes it with `private`)~~ (done, `task-v8-sch`, PARITY-sch section 87:
`LibConnectivity::shape_looks` and `field_looks` on the engine symbol, drawn by the schematic canvas and written in the cache); a field's looks are kept by its name, so a renamed field
leaves them behind; the clipboard's images need the browser's clipboard permission (headless checks get the text only); the Text Box tool's next box does not take the last
one's line style.
