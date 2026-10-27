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
Right/Left/Up/Down <-> `angle_deg` (`kicad-port/pinOrientation.ts`,
confirmed against this app's own already-correct builtin symbols, tested).
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
| Ctrl+Shift+S Save Library As (`eeschema.SymbolLibraryControl.saveLibraryAs`) | partial: downloads the whole project library as one derived `.kicad_sym` (`GET /api/symbol_library/export`, `eda_kicad::export_kicad_sym_library`); no file chooser, no library-table entry. Read-only like the single-symbol export, so no verb and no undo entry. `kicad-cli` loads the file the same way as any `.kicad_sym` | `symbol_editor/symbol_editor.cpp::SYMBOL_EDIT_FRAME::saveLibrary( aLibrary, aNewFile )` via `symbol_editor_control.cpp::Save` |

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
| Import Symbol / Export Symbol | **done** | `ImportSymbol` + `DIALOG_IMPORT_SYMBOL_SELECT` (check the symbols to bring in, "Resolve Import Conflicts": Overwrite or Skip each, Skip All / Overwrite All, "Imported %d symbol(s), skipped %d."), `ExportSymbol` (`<name>.kicad_sym`). The reader is `crates/kicad/src/symbol_import.rs` (`POST /api/symbol_library/parse`, the inverse of `export_kicad_sym`, round-trip tested): a derived symbol (`extends`) stays derived, its parents imported first (section "Derived symbols" below), a pin's alternate functions and text boxes are read, bezier curves are dropped with a warning |
| Export View as PNG | **done** | `SYMBOL_EDITOR_CONTROL::ExportView`: the canvas as `<name>.png` |
| Add Symbol to Schematic | **done** | `AddSymbolToSchematic`: the open symbol, at the unit being edited, becomes the schematic's symbol being placed (a symbol nothing follows yet is published first, "Update Symbol on Board") |
| Bulk Edit Symbol Fields | partial | `DIALOG_LIB_FIELDS_TABLE`: every project symbol of the library as a row (or, as Bulk Edit Related Symbol Fields, a root symbol and what is derived from it), edited in place, OK writes the changes as one undo step. The columns are the ones the library model keeps: Symbol Name, Reference, Value, Footprint, Datasheet, Description, Keywords, the user fields, Exclude From BOM / Board, Power Symbol (no simulation fields) |
| Synchronized Pins Mode | **done** | `ToggleSyncedPinsMode` and what it drives: a pin placed on one unit gets its image on every other (`CreateImagePins`), moving or editing a pin carries the matching pins of the other units (`SYMBOL_EDITOR_MOVE_TOOL::Main`, `EditPinProperties`), deleting a pin deletes its twin in each other unit, the same place, body style, type and name (`DoDelete`, `kicad-port/symEditItems.ts::deletePlan`), Pin Properties shows the mode's info bar ("Synchronized Pins Mode.  New pin will be added to all units." ..., "Exit sync pins mode") and tooltips, and "This position is already occupied by another pin, in unit %d" asks first (`PlacePin`). On by default for a multi-unit symbol. `kicad-port/symPinSync.ts` |
| Push Pin Length / Name Size / Number Size | **done** | `SYMBOL_EDITOR_PIN_TOOL::PushPinProperties` from the one selected pin (tool entries and the right-click menu); length goes through `SCH_PIN::ChangeLength` (the connection point moves along the pin) and only to pins of the same body style |
| Draw Lines / Draw Polygons | **done** | one tool in this KiCad (both `SHAPE_T::POLY`, `doDrawShape`): the first click begins the outline, each later click continues it (no zero-length segment), a double click or Enter ends it, Escape drops the shape and keeps the tool. Unfilled, as `m_lastFillStyle` is. Ending on the start closes it (the file has no closed flag: the last point repeats the first). `kicad-port/symPolyDraw.ts`, ported from `EDA_SHAPE::{begin,continue,calc,end}Edit` |
| Draw Text | **done** | `TwoClickPlace`: the first click opens `DIALOG_TEXT_PROPERTIES` (text, size, horizontal / vertical; bold, italic and justification are not part of a library text here), the text follows the cursor, the second click places it; a text with nothing printable is dropped |
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
on the unit and body style being edited (`m_unit`; KiCad leaves the body style at "all"). A symbol has no curves, so a Bezier curve of the file is cut into segments at `ARC_LOW_DEF_MM` (20 micrometres), where KiCad keeps the curve; a polygon filled with a colour is filled with the outline's, the library fill having no colour. A text is anchored at the middle of its baseline here, so the file's horizontal justification
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
| `(property private ..)` lost its flag; a graphic's stroke style (`solid` 830 times, `dash` 23 times in the libraries) became `default`; a text's pen, bold, italic, justification and hidden flag were dropped; a `(bezier ..)` was left out (98 in Auxiliary_Items, 4 in Device:Thermal_Jumper) | kept (`LibrarySymbolField::private`, `LibrarySymbolKeep::stroke_types`, `text_styles`, `items`) and written back; a bezier is not drawn or edited, and Move Symbol Anchor does not move it | `saveField`, `STROKE_PARAMS::Format`, `EDA_TEXT::Format` |
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
| Rotate (R, Shift+R) | **identical for pins**: one item turns about its own position, several about `GetNearestHalfGridPosition( selection.GetCenter() )` (the merged `GetBoundingBox()` of the selection: each pin's line, name and number as `PIN_LAYOUT_CACHE::GetPinBoundingBox` gives them, with the pin's own text sizes, and each selected graphic's box, of whatever unit and body style -- an imported body is `Name_0_1` / `Name_1_1` -- texts left out (`if( !item->IsType( { SCH_TEXT_T, .. } ) )`) and a selection of texts only taking the mean of their positions; `BOX2I::GetCenter` in integer IU, snapped with `KiROUND` in KiCad's y-down frame; r3 E3-3, 2026-10-27; r4 E4-1, `rotatePivot`), in one commit; the other units' pins are not touched. Shapes do not turn yet | `SYMBOL_EDITOR_EDIT_TOOL::Rotate`, `SCH_PIN::Rotate`, `rotatePlan` |
| Repeat Pin (Insert) | **identical**: the last placed pin again, 100 mil down for a horizontal pin and across for a vertical one (`m_Repeat.pin_step`), name and number `IncrementString( .., 1 )`, with `PlacePin`'s question and images in sync mode, one undo step "Repeat Pin". The last pin is kept by its id (`g_lastPin` is a UUID), so it is found, and repeated as it is now, after it was moved, renumbered or renamed; the repeated pin leaves its orientation, type and shape in the pin template (`PlacePin`: `g_LastPinOrient`, `g_LastPinType`, `g_LastPinShape`) (r3 E3-4, E3-5); the new pin is left selected (`selectionClear`, then `selectItem` of the pin; r4 E4-2) | `SYMBOL_EDITOR_DRAWING_TOOLS::RepeatDrawItem`, `SYMBOL_EDITOR_PIN_TOOL::RepeatPin`, `repeatPin` |
| Duplicate (Ctrl+D, the context menu) | **differs**: the copies (each numbered pin `GetMaxPinNumber() + 1`, in `StrNumCmp` order of the old numbers) are added in place, one undo step; KiCad then starts a move with them | `SYMBOL_EDITOR_EDIT_TOOL::Duplicate`, `duplicateItems` |
| Cut / Copy / Paste (Ctrl+X/C/V, the context menu) | **differs**: the items go to the editor's own clipboard (not the system clipboard as a `.kicad_sym` text), and Paste adds them in place on the unit and body style being edited unless common (`SetUnit( GetUnit() ? m_frame->GetUnit() : 0 )`), refused on a derived symbol; KiCad then starts a move | `SYMBOL_EDITOR_EDIT_TOOL::Cut`, `Copy`, `Paste`, `pastedItems` |
| Pin Properties' OK | **identical**: a moved pin off the standard grid asks "This pin is not on a 50 mils grid which will make it difficult to connect to in the schematic.\nDo you wish to continue?" (Yes/No) -- KiCad tests `GetValue() % 50` in internal units, so ported as that (a step of 0.005 mm), not 50 mils; "Common to all units in symbol" is enabled for a multi-unit symbol only (unchecked otherwise, so OK puts the pin on the unit), "Common to all body styles" for a De Morgan symbol only, each with KiCad's tooltip | `DIALOG_PIN_PROPERTIES::TransferDataToWindow`, `TransferDataFromWindow`, `kicad-port/symPinDialog.ts` |
| A new symbol's Reference and Value | **identical**: at the origin (`LIB_SYMBOL::LIB_SYMBOL`'s `new SCH_FIELD( this, id )`), the Value the name | `Cmd::NewSymbol` |

The action audit (`tools/ui-parity-audit.mjs`) counts an action by name, not by tab: `common.Interactive.copy`/`paste`/`cut`/`duplicate` and
`eeschema.InteractiveEdit.repeatDrawItem` were counted as wired from the schematic while the Symbol tab had none; they are the Symbol tab's own
now (`actions/editorFrameActions.ts`), but the audit still cannot tell a tab that lacks one.

## Differs from KiCad

- **Where a pin is placed.** Our pin is placed with one click: the click opens Pin Properties, its OK adds the pin, and `PlacePin`'s
  "This position is already occupied by another pin, in unit %d." question is asked at that OK. KiCad's `TwoClickPlace` opens the dialog on
  the first click and places the pin on a second click after the dialog's OK.
- **The New Symbol name field.** It is prefilled with the free "Untitled"/"Untitled_N" (so Ctrl+N then OK makes a symbol at once), where
  KiCad's starts empty, and we trim it before `GetName`'s escaping, which KiCad does not.

## Known gaps (not fixed, scope-bounded)

- DeMorgan alternate body style: authorable, exported to `.kicad_sym`, and (since the schematic item 12 work) shown by a placed symbol.
  `LibrarySymbol::to_engine_symbol` (the publish path) keeps the style-2 items as `LibSymbol::alternate` (shared items, `body_style` 0, are in
  both bodies), the `.kicad_sym` reader splits `Name_<unit>_<style>` sub-blocks the same way, and a placed symbol in style 2 (`SchExtras::
  body_styles`, Cycle Body Style) draws, exports and connects by that body (`PARITY-sch.md` section 1). Left: only the normal and the De Morgan
  pair are modelled (no named body styles), and the symbol chooser's preview shows the normal style.
- ~~No "Alternate pin function definitions"~~ -- done 2026-10-15 (section "Alternate pin functions").
- ~~No dedicated Shape Properties dialog for symbol graphics~~ -- done 2026-10-16 for a rectangle, circle, arc and polygon (section "The symbol text box and the Shape Properties dialog"). A plain
  text has no Properties dialog (a double click on it does nothing), and the Footprint Editor's graphics and texts have none either.
- ~~Pin name offset (`pin_name_offset_mm`) is stored and exported but the canvas draws at the shared 0.508 mm~~ -- the Symbol Editor's canvas follows the symbol's offset since 2026-10-15 (names inside
  the body from the offset on, over the pin line at 0); the schematic's placed-instance renderer reads it from the symbol too.
- "Push Pin Properties" has its entry points now (tool and right-click menu, section above).
- ~~A new graphic or text is always on the unit being edited~~ -- since the P17 review (section "P17 review" below) it is on unit 0, common to all units, unless the
  symbol's units are locked (`m_drawSpecificUnit = UnitsLocked()` when a symbol is loaded and after Symbol Properties) or Shape Properties last said "not common".
- No live ghost/preview while a pin/shape tool is armed, before the first
  click -- same documented adaptation PARITY-sch.md's labels/power-symbol
  section already accepts for this app's "small-dialog-last" tool shape.

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
