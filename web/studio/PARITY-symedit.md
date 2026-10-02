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
| `P` Place Pin (`eeschema.SymbolDrawing.placeSymbolPin`) | identical: arms the Pin tool (the hotkey again leaves it); each click places a pin whose number is the next free one after the pin just placed (`IncrementString`; a quick second click no longer re-reads a stale document and repeats the number -- `SymbolEditorCanvas.tsx` carries `lastPlacedPinRef` like the C++ tool's `m_lastPin`), the template (name, type, shape, length, orientation) carries from the last pin edited | `symbol_editor_pin_tool.cpp::PlacePin`, `SYMBOL_EDITOR_DRAWING_TOOLS` |
| Ctrl+N New Symbol (`eeschema.SymbolLibraryControl.newSymbol`) | partial: an empty symbol is created in the project library and opened, named `Untitled` (`Untitled_1`, ... until unused) with no name dialog -- the project library is the only destination this studio has, so `DIALOG_LIB_NEW_SYMBOL` has nothing to ask. A name that is already taken is refused (`new_symbol` verb, undoable). Registered on the Symbol tab only, so it does not shadow `common.Control.new` | `symbol_editor/symbol_editor.cpp::SYMBOL_EDIT_FRAME::CreateNewSymbol` (`kicad-port/symEditActions.ts`); `Cmd::NewSymbol` |
| Ctrl+Shift+S Save Library As (`eeschema.SymbolLibraryControl.saveLibraryAs`) | partial: downloads the whole project library as one derived `.kicad_sym` (`GET /api/symbol_library/export`, `eda_kicad::export_kicad_sym_library`); no file chooser, no library-table entry. Read-only like the single-symbol export, so no verb and no undo entry. `kicad-cli` loads the file the same way as any `.kicad_sym` | `symbol_editor/symbol_editor.cpp::SYMBOL_EDIT_FRAME::saveLibrary( aLibrary, aNewFile )` via `symbol_editor_control.cpp::Save` |

Fixed on the way: `GET /api/symbol?lib_id=...` did not percent-decode `lib_id`, so a symbol could never be shown (`Device%3AR` matched nothing) -- the studio's Symbol tab now loads symbols
(`crates/cli/src/studio.rs::query_value`).

## Known gaps (not fixed, scope-bounded)

- DeMorgan alternate body style is authorable and exports correctly to
  `.kicad_sym`, but `LibrarySymbol::to_engine_symbol` (the publish path)
  drops style-2 items -- `crate::symbol::SymbolGraphic`/`LibPin` have no
  `body_style` field at all, a pre-existing engine-level gap (confirmed:
  the *existing* `.kicad_sym` reader already discards the style suffix
  too). Authoring/export is real; a placed instance showing the alternate
  style is not.
- No "Alternate pin function definitions" (KiCad's per-pin extra name/
  type/shape the same physical pin can switch between).
- No dedicated Shape Properties dialog for symbol graphics (move/delete
  only) -- same gap the Footprint Editor's own plain graphics have.
- Pin name offset (`pin_name_offset_mm`) is stored/exported correctly but
  the canvas (and the schematic's own placed-instance renderer) always
  draws at the shared hardcoded 0.508mm offset -- pre-existing limitation,
  now also true here.
- "Push Pin Properties" has no UI entry point yet (Cmd + backend only).
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
