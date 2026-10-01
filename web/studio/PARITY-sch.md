# Schematic editor parity with KiCad eeschema

One row per action. Status is **identical** (same logic/behavior, adapted
only where the browser platform genuinely requires it, noted inline),
**partial** (core behavior ported, a real gap remains, noted), or
**missing** (not started). Mirrors `PARITY-pcb.md`'s own format.

KiCad source snapshot read this session: the same `kicad-src` scratchpad
copy `PARITY-pcb.md` cites (commit `8303b2ad`, version 10.99).

Before the previous session: 0 of eeschema's 240 cataloged actions were
wired (read-only viewer, measured ~5% parity in `docs/parity/REPORT.md`).
That session ported selection/move/rotate/mirror/delete end to end
(frontend + backend), the ERC dialog (gap #4), and fixed the cross-tab
undo bug (gap #15). Wire/label/power-symbol drawing tools, a symbol
chooser, Annotate's dialog, and Properties were not yet wired.

This session ports `G` (drag, with wire rubber-banding -- section 1),
a plain click-and-drag directly on a symbol as part of the same change
(no hotkey needed first, defaulting to Drag's rubber-banding, matching
source's own default), `L`/Ctrl+`L`/`H`/`P`/`Q`/`T` (section 2/3 --
label/no-connect/power-symbol/text placement, plus a new minimal
`SchematicText` IR for `T`), and `A`'s symbol chooser (section 3 --
`SymbolChooserDialog.tsx`, a new `GET /api/symbol_library` endpoint, and
a new `LibSymbol::reference_prefix` field so a placed symbol gets a real
reference immediately instead of a "U?" placeholder), `E`/`U`/`V`/`F`
(section 1 -- `SymbolPropertiesDialog.tsx`, new `Cmd::RenameSymbol`/
`EditSymbolFields`), and Annotate's dialog (section 5 --
`AnnotateDialog.tsx`, new `Cmd::Annotate.order`/`.ids`). Still not wired:
`J` junction — see the bottom of each section.

**Found and fixed while wiring `E`/`U`/`V`/`F`'s hotkeys**: several
physical keys are double-booked by one `pcbnew.*` and one `eeschema.*`
action sharing the exact same extracted hotkey (R, M, G, X, E, U, V, F
all collide this way in the real table) -- `useGlobalHotkeys.ts`'s own
`hotkeyIndex.get(combo)?.find(isEnabled)` picked whichever name came
first in `actions.json`'s own order, regardless of which tab was
actually open, because `isEnabled` only ever checked "is a handler
registered at all" (always true for both, since every `pcbOnly`/
`schematicOnly` wrapper registers unconditionally and only its function
*body* checks the tab). Concretely: `pcbnew.InteractiveEdit.rotateCcw`
(R) and `pcbnew.InteractiveMove.move` (M) were **already silently dead
on the PCB tab** before this session touched anything, shadowed by the
previous session's own `eeschema.InteractiveEdit.rotateCCW`/
`InteractiveMove.move` entries -- and this session's own new `E`/`V`/`F`
registrations would have freshly broken `pcbnew.InteractiveEdit.
properties`, `pcbnew.Control.layerToggle` and `pcbnew.InteractiveEdit.
flip` (all three already implemented) the same way. Fixed at the root:
`isEnabled` (`kicad-port/actionTabGate.ts::isActionEnabledForTab`, unit
tested) now also gates on the action name's own module prefix, so the
tab-irrelevant half of a colliding pair is correctly skipped. **Needs a
real click-through to confirm** (see the list at the bottom) -- this was
found by reading the dispatch code while researching where to add new
bindings, not by exercising it in a browser.

## 0. Architecture: the "one netlist" rule

Every schematic edit is a `POST /api/cmd` verb (new `Cmd` variants in
`crates/ops/src/lib.rs`, one per action below), with undo/redo, that
changes `design.json`'s `schematic` section — same contract the PCB side
already has. Two pieces make connectivity itself stay a single source of
truth (GAPS.md #1's hard rule):

- `crates/kicad/src/sch_import.rs::reconcile` (already existed, for
  reading a real `.kicad_sch` file) is now also `pub` and reused by
  `crates/cli/src/board.rs::reconcile_schematic`, which runs after every
  `Domain::Schematic` command (`Cmd::domain`): it retraces wire/label/
  power-symbol/no-connect geometry with the same union-find and writes
  the result back into `design.schematic`'s own `Wire::net`/`pins`,
  `PowerSymbol::pin`, `NoConnect::pin` fields, and into a new
  `Design::nets` field.
- `crates/cli/src/board.rs::load` applies `design.nets` as an override on
  top of the intent-derived `ConstraintModel::nets` *only when it is
  `Some`* — i.e. only once a board has actually been hand-edited in the
  studio. Every board in this project's own test/parity corpus has never
  executed a schematic `Cmd`, so `design.nets` stays `None` for all of
  them and `docs/parity/scores.json`'s ratchet is untouched.
- A symbol placed with no intent counterpart (`AddSymbol`) gets a
  synthesized `Part` the same way, so it shows up unplaced on the PCB tab
  — `reconcile_schematic`'s own doc comment has the full mechanism.
- Proven by two `crates/cli/src/board.rs` tests, covering all three cases
  the hard rule names ("wire connects pins, label merges nets, delete
  splits"): `schematic_wire_connects_and_disconnects_pins_on_one_netlist`
  (draws a wire between two previously-separate nets, asserts they merge
  in `model.nets`, deletes the wire, asserts they split back apart) and
  `schematic_label_merges_nets_with_no_wire_between_them` (two same-named
  labels merge two pins' nets with *no wire at all*, matching how a real
  spread-out schematic usually ties a rail together; deleting one label
  splits them back apart).

KiCad files stay derived: nothing above touches `.kicad_sch` export or
the intent YAML file.

## 1. Selection and move (`sch_selection_tool.cpp`, `sch_move_tool.cpp`, `sch_edit_tool.cpp`)

| Action | Status | KiCad file:function |
|---|---|---|
| Click a symbol: select, replacing the current selection | identical | `sch_selection_tool.cpp::selectPoint` (`SchematicView.tsx::hitSymbol` + `kicad-port/selection.ts::applySingleClickModifier`) |
| Shift+click: add to selection | identical | `common/tool/selection_tool.cpp` modifier table (`computeClickModifiers`, shared with the PCB port) |
| Ctrl/Cmd+click: toggle (exclusive-or) | identical | same (confirmed `ctrlClickHighlights()` is off by default, same as pcbnew) |
| Ctrl/Cmd+Shift+click: subtract | identical | same |
| Box select, left→right = fully enclosed, right→left = crossing | identical for symbols | `common/tool/selection_tool.cpp::SelectRectArea` (`isCrossingSelection`, shared with the PCB port); `SchematicView.tsx::collectBoxSelection` |
| Box select: wires (crossing = whole wire if touched at all; enclosed = both endpoints, or one if it's dangling) | missing (wires are not box-selectable yet) | `sch_selection_tool.cpp::SelectMultiple` lines ~2640-2666 — researched this session, not yet ported; see `collectBoxSelection`'s own doc |
| Pins/junctions win ties over symbol body/wire at an exact hit | missing (no finer-than-symbol hit test yet) | `sch_selection_tool.cpp::GuessSelectionCandidates`/`narrowSelection` |
| `M`: move (breaks wire connections — a wire's endpoint is a bare coordinate, not a pin reference) | identical | `sch_move_tool.cpp` (`setupItemsForDrag` never adds connected wires in MOVE mode); `Cmd::MoveSymbol`, `SchematicView.tsx`'s `moveMode` branch |
| `G`: drag (attached wire endpoints rubber-band) | identical for this app's IR (see the gap noted below for what's left out) | `sch_move_tool.cpp::getConnectedDragItems` (`wireAttachment.ts::attachedWireEndpoints`, same arm-then-click-to-drop flow as `M`, `Cmd::DragSymbol`). A wire here is one polyline (every bend from one `W` session), not a separate `SCH_LINE` per segment, so only its own two true ends are tested for attachment -- matching `pinSnapPoints`'s existing "landing on a pin" convention. Every wire at a junction a dragged pin sits on attaches and moves together |
| New stub wire at an unselected 3-way junction (source's own fallback when a junction's *other* wires are deliberately not moving) | not applicable to this IR | `ptHasUnselectedJunction`'s branch only matters when a caller can select one wire at a junction independent of the symbol being dragged -- this app's `G` has no such partial case yet (no wire box-select, see item 8 below), so every wire at a junction a dragged pin sits on is always fully attached (previous row) and a stub is never needed |
| Grid snap during move | identical (grid only) | `edit_tool_move_fct.cpp` (`kicad-port/gridSnap.ts::alignToGrid`, reused) |
| Anchor/pin snap during move | missing | `ee_grid_helper.cpp` — per-item-category grids + pin-anchor snap not ported; PCB side has the analogous gap too (`PARITY-pcb.md`) |
| Escape cancels an in-progress move without touching the prior selection | identical | shared `ESCAPE` reducer case (already generic across tabs) |
| `R`: rotate CCW | identical | `sch_edit_tool.cpp::Rotate` (confirmed default is CCW, not CW); `Cmd::RotateSymbol` |
| Shift+`R`: rotate CW | identical | same function, `rotateCW` action |
| R/Shift+R during an active move updates the live preview instead of committing separately | identical | `sch_move_tool.cpp::handleMoveToolActions` (`tryTransformDuringMove`, shared helper, now tab-aware) |
| Rotate pivot: own anchor (single item) / collective bbox center (multi-select) | partial — only single-selection rotate is wired (one symbol id) | `sch_edit_tool.cpp::Rotate` |
| `X`: mirror horizontally (negate-X, KiCad's `SYM_MIRROR_Y`) | identical | `sch_edit_tool.cpp::Mirror` (`mirrorH`); `Cmd::MirrorSymbol`, toggles `SymbolInstance::mirrored` |
| `Y`: mirror vertically (negate-Y, `SYM_MIRROR_X`) | missing — the IR models one mirror axis only (`SymbolInstance::mirrored`, confirmed against `transform_local_point` to be the X/negate-X one) | `sch_edit_tool.cpp::Mirror` (`mirrorV`). Needs a second bool on `SymbolInstance` plus an axis-aware `transform_local_point`/painter.ts change — deferred to avoid widening a function `sch_import.rs` also depends on, under this session's time budget |
| Mirror during an active move | missing | `sch_edit_tool.cpp::Mirror`'s own `IsMoving()` branch (asymmetric from Rotate's in source itself — no `updateStoredPositions()` call) |
| `Del`: delete symbol, wires left dangling (no cascade) | identical | `sch_edit_tool.cpp::DoDelete` (confirmed: wires are never auto-deleted); `Cmd::DeleteSymbol`, `common.Interactive.delete`'s schematic branch |
| `Del`: delete wire | identical (wire must already be selected via a modified click — see the box-select gap above) | `Cmd::DeleteWire` |
| `E` Properties | partial — `SymbolPropertiesDialog.tsx`: Reference/Value/Footprint/Datasheet, the fields this IR actually has (no unit/DeMorgan/pin-table editing source's full `DIALOG_SYMBOL_PROPERTIES` also offers) | `sch_edit_tool.cpp::Properties` — dispatches to one of 6 different dialogs by item type; this app only has symbols selectable, so always this one shape |
| `U`/`V`/`F`: quick-edit Reference/Value/Footprint | partial — same dialog as `E`, just autofocused on the one field (source uses a separate, smaller single-field `DIALOG_FIELD_PROPERTIES` for these three; one shared component here, deliberately, since both ends run the same two Cmds either way) | `sch_edit_tool.cpp::EditField`/`sch_actions.cpp` (confirmed `U` = reference, not "unit" — a wrong guess here would have mis-bound a hotkey) |
| `U`'s own rename: cascades every `"REF.PIN"` string this sheet's wires/power-symbols/no-connects hold | partial — refuses a blank or already-used new id; a wire/power-symbol/no-connect whose pin happens to sit exactly on a real resolvable pin gets its reference re-derived for free by the next `reconcile_schematic` pass (same geometric mechanism that already runs after every schematic `Cmd`, confirmed by a dedicated test with real connected wire geometry) -- the explicit string-cascade in `rename_symbol` only matters for the degenerate case where a power-symbol/no-connect's own position doesn't resolve to a real pin at all, a safety net, not the primary mechanism. Deliberately does **not** retarget `design.placement`'s `FootprintInstance` (no rename concept on the PCB side at all) or intent.yaml (read-only to every verb in this file) -- the old reference's PCB footprint, if any, is left exactly where it was, now matching no schematic symbol; the next reconcile pass synthesizes a fresh, unplaced `Part` for the new reference, the same mechanism `AddSymbol` already uses for a part with no intent counterpart. A real "rename and keep the PCB placement" is future work | new `Cmd::RenameSymbol`, `crates/ops` |
| `E`'s Value/Footprint/Datasheet edits | identical for the Schematic tab's own display (the instance's own copy always wins when set, same precedent `SymbolInstance::value`'s doc already established); does not update `ConstraintModel::Part` (not persisted across requests anyway) | new `Cmd::EditSymbolFields`, `crates/ops` |
| A plain click-and-drag directly on a symbol (no `M`/`G` needed first) | identical in spirit -- defaults to Drag's rubber-banding, matching source's own default for a plain drag (`sch_selection_tool.cpp Main()`'s `IsDrag(BUT_LEFT)` handler: `SCH_ACTIONS::drag` unless the `drag_is_move` preference is set, which this app has no settings UI for); a click on a member of a larger selection keeps the whole group, same `RequestSelection`/`selectionContains` reasoning | `sch_selection_tool.cpp` Main() (`SchematicView.tsx`'s new `DragState` "move" kind, ported from Canvas.tsx's identical PCB-side gesture) |
| Click-vs-drag threshold (8px/300ms) | partial -- same approximation the PCB side already shipped (Canvas.tsx): "moved" is true once the *snapped* position first changes, not a literal pixel/time timer, so a release before the grid (or a nearby pin) actually moves is still a plain click. Good enough that a modifier-click is unaffected (that path never arms a drag at all) | `common/tool/tool_dispatcher.cpp` (`DragDistanceThreshold`/`DragTimeThreshold`) -- same documented simplification as PARITY-pcb.md's own row for this |

## 2. Wires, junctions, no-connects (`sch_line_wire_bus_tool.cpp`)

| Action | Status | KiCad file:function |
|---|---|---|
| `W`: draw wire, click to start/bend, snaps to a pin when close | partial — free-angle only, no 90°/45° auto-posture (every bend is wherever you click, not auto-right-angled); pin-snap only works for a symbol with real `lib_symbols` graphics resolved (see `pinSnapPoints`'s own doc for the generic-box-fallback gap) | `sch_line_wire_bus_tool.cpp::doDrawSegments`/`startSegments`/`finishSegments`; `SchematicView.tsx`'s wire-tool branch of `onPointerDown`, `Cmd::AddWire` |
| Shift+Space: cycle Free/90°/45° posture | missing | `SCH_ACTIONS::lineModeNext` |
| Double-click or Enter: finish the wire | partial — double-click wired (`onDoubleClick`); Enter is not (no generic "finish" hotkey action registered yet, unlike PCB's route/zone/shape tools, which also only wire double-click in practice) | `ACTIONS::finishInteractive` |
| Landing back on a pin auto-finishes the wire | identical, pins only (not also wires/junctions/sheet-pins, per `sch_screen.cpp::IsTerminalPoint`'s fuller list) | `sch_screen.cpp::IsTerminalPoint` |
| Backspace: undo last in-progress segment | identical | `SCH_ACTIONS::undoLastSegment`; `eeschema.InteractiveDrawingLineWireBus.undoLastSegment` |
| Escape discards the whole in-progress wire | identical | `doDrawSegments`'s `cleanup()` (shared `ESCAPE` reducer case, already generic) |
| Auto-junction at a T (3+ wire exit angles) | identical, **for connectivity** — the dot itself is drawn wherever 3+ wire endpoints/segments meet, computed live from geometry (`painter.ts::junctionPoints`, pre-existing); never a stored item, matching this project's own IR (no `junctions` field — see `Cmd::AddWire`'s own doc) | `junction_helpers.cpp::AnalyzePoint`; `reconcile`'s own T-junction union-find pass already gives correct electrical connectivity with or without a visible dot |
| `J`: explicit junction at a plain crossing | missing (no stored concept to place one at — see above; connectivity is correct regardless, this is a cosmetic/explicit-marker gap only) | `SCH_DRAWING_TOOLS::SingleClickPlace` |
| `Q`: no-connect flag, click to place | partial — place only; source's own "click again on one to remove it" isn't ported, and neither is selecting/`Del`-ing an already-placed no-connect by any other means (`Cmd::DeleteNoConnect` exists; no frontend hit-test for this item kind yet, same documented gap as the labels/power-symbols/text this session also only ever *adds* — see this section's own intro) | `SCH_DRAWING_TOOLS::SingleClickPlace`, no-connect branch; `SchematicView.tsx`'s `sch_no_connect` tool branch pin-snaps the same way `W` does, then `Cmd::AddNoConnect` commits immediately -- stays armed for the next click, same as the wire tool |
| Wire merges two nets / delete splits them | identical | proven by this session's Rust test, see section 0 |

## 3. Labels, text, power symbols, symbol placement (`sch_drawing_tools.cpp`)

`L`/Ctrl+`L`/`H`/`P`/`T` are wired this session, all through the same
adapted flow: arm the tool (stays armed for chained placement, like `W`),
click to capture a position (pin-snapped for `L`/`P`, since a power
symbol's own `pin` field needs to land on a real pin to resolve at all --
see `Cmd::AddPowerSymbol`'s doc -- and a label commonly tags a wire/pin
too; plain grid-snap for `T`), then a small dialog
(`LabelDialog.tsx`/`PowerSymbolDialog.tsx`/`SchTextDialog.tsx`) confirms
the text/symbol/content before the real `Cmd` commits. This is a
deliberate adaptation of source's own order -- real eeschema pops
`DIALOG_LABEL_PROPERTIES`/a full `DIALOG_SYMBOL_CHOOSER`/
`DIALOG_TEXT_PROPERTIES` *immediately* on the hotkey, then the item
follows the cursor for a final placement click -- chosen to reuse this
app's own established "draw/click first, small dialog last" shape
(ZoneDialog/TextDialog already work this way for the PCB side) rather
than build a second, different interaction pattern. No live ghost/preview
follows the cursor before that first click (the status bar's tool message
-- now shown on the Schematic tab too, see StatusBar.tsx -- is the only
"what will clicking do" affordance); a real canvas preview is a further,
not-yet-done step.

Newly placed labels/power symbols/free text cannot yet be clicked to
select/move/`Del` afterward (no hit-test for those item kinds in
`SchematicView.tsx` yet, only for symbols/wires) -- this section only
covers *placing* them, matching this pass's scope.

| Action | Status | KiCad file:function |
|---|---|---|
| `L`/Ctrl+`L`/`H`: place local/global/hierarchical label | partial — see this section's intro for the click/dialog adaptation; no 90°/auto-rotate-on-placement (this IR's `NetLabel` has no orientation field at all -- `drawLabel` always infers the spin live from nearby wire geometry, `inferSpin`, so there is nothing to set during placement the way source's `SetSpinStyle`/`AutoRotateOnPlacement` do) | `SCH_DRAWING_TOOLS::TwoClickPlace`/`createNewLabel`; `eeschema.InteractiveDrawing.place{Label,GlobalLabel,HierarchicalLabel}` |
| Auto-increment: a chained label's suggested text advances a trailing number (DATA0 → DATA1...), a name with none is left alone | identical | `common/increment.cpp::IncrementString` (`SCH_LABEL_BASE::IncrementLabel`'s own function) ported directly, not re-derived -- `kicad-port/incrementLabelText.ts`, unit-tested including the zero-pad and "no digits -> unchanged" cases; `state.lastLabelText` is the per-session memory `createNewLabel`'s caller would otherwise keep on the tool instance |
| `P`: place power symbol | partial — any `power:<net>` resolves to a real rail symbol (`eda_model::symbol::builtin`'s generic fallback), so typing a custom rail name works, but this app's own quick-pick (a `<datalist>` of the common presets) is not KiCad's real searchable `DIALOG_SYMBOL_CHOOSER`; orientation (0/90/180/270) is a dialog field instead of a live mid-placement spin | `SCH_DRAWING_TOOLS::PlaceSymbol` (power filter) → `DIALOG_SYMBOL_CHOOSER`; `eeschema.InteractiveDrawing.placePowerSymbol` |
| `T`: place text | identical in effect, adapted flow (see intro) — new `SchematicText` IR (`id`/`content`/`at`/`angle`/`size_um`; deliberately minimal next to real `SCH_TEXT`, no bold/italic/justify/color yet), `Cmd::AddSchText`/`DeleteSchText`, exposed over `GET /api/schematic` (`texts[]`) and rendered with the same Newstroke font every other schematic text uses (`painter.ts::drawSchText`, `LAYER_NOTES`). Also wired into `export_kicad_sch` (a `(text ...)` block, same shape as a label's own) and `import_kicad_sch` (reads `(text ...)` back), so free text now round-trips through a real `.kicad_sch` file like every other schematic item -- not independently round-trip-tested this session beyond the Rust unit/compile checks, since the existing label-import/export code it mirrors line-for-line was the thing actually verified against real files | `TwoClickPlace`/`createNewText`; `eeschema.InteractiveDrawing.placeSchematicText` |
| `A`: place symbol, via a chooser over the sheet's own `lib_symbols` | partial — `SymbolChooserDialog.tsx`: search + live preview (reuses `paintSchematic` on a synthetic one-symbol sheet) over `GET /api/symbol_library`'s catalog, then click to place. Real differences from source, all deliberate: the dialog runs *before* a position is chosen (same order source uses, unlike `L`/`P`/`T`'s own click-first adaptation -- there's no sensible position to capture before you've picked *what*), but a plain search list, not source's recently-used/already-placed tabbed `DIALOG_SYMBOL_CHOOSER`; a placed symbol gets a real, already-numbered reference immediately (`R7`, via `nextReference.ts`) rather than a `"U?"` placeholder left for Annotate -- `Cmd::AddSymbol` refuses an exact duplicate id, so reusing the bare placeholder for a second placement in the same session would be refused outright, and `annotate()`'s own numbering only recognizes an id ending *exactly* in `?` | `SCH_DRAWING_TOOLS::PlaceSymbol` → `DIALOG_SYMBOL_CHOOSER`; `eeschema.InteractiveDrawing.placeSymbol` |
| `GET /api/symbol_library`'s own catalog: "the libraries we already load" | partial — every library name this project's intent already resolved a part against, or that's already on the sheet, scanned for its *full* real-file contents (`eda_kicad::list_symbols_in_library`/`list_symbol_libraries`, both newly unit-tested) when a real `.kicad_sym` resolves (this session's own sandboxed environment has no real KiCad install, so every test board actually exercises the fallback path below, not this one); power symbols (own tool, `P`) and the parametric `Connector_Generic:Conn_01x<N>` family (40 variants, not worth hand-enumerating) are deliberately excluded | `DIALOG_SYMBOL_CHOOSER`'s own library-table-backed search, narrowed to what this app can search at all |
| ... falling back to `eda_model::symbol::builtin_catalog` | identical in spirit to every other "real library first, builtin fallback" resolution in this codebase | new function, mirrors `builtin`'s own existing precedence; required adding `LibSymbol::reference_prefix` (from the library's own `Reference` property, already parsed by `build_symbol` but previously discarded) -- another `~27`-call-site field addition, same cost as `SchematicText`'s own (section 3's intro) |
| Placing a symbol with no intent counterpart adds a synthesized `Part`, shows up unplaced on PCB | identical | `reconcile_schematic`, see section 0 |

## 4. ERC (gap #4) (`sch_inspection_tool.cpp`, `dialog_erc.cpp`)

| Action | Status | KiCad file:function |
|---|---|---|
| Run ERC (menu item, no default hotkey) | identical | `SCH_INSPECTION_TOOL::RunERC`/`ShowERCDialog`; `eeschema.InspectionTool.runERC` → `ErcDialog.tsx`, `GET /api/erc` → `eda_kicad::check_erc` (unexposed before this session — GAPS.md #4) |
| Results list: one row per finding, Errors/Warnings filter | identical in spirit, flatter structure | `dialog_erc.cpp` (`RC_TREE_MODEL`, flat list, not grouped by sheet — matches); `check_erc` reports plain `CheckResult`s (no item/position breakdown the way `DrcViolation` has), so `ErcDialog.tsx` is flatter than `DrcDialog.tsx` |
| Click a row: cross-probe (select + pan/zoom to it) | partial — selects the named symbol and switches to the Schematic tab, but does not re-frame the view the way `DrcDialog.tsx`'s `jumpTo` zooms to a violation's exact point (no position data to zoom to — see above) | `DIALOG_ERC::OnERCItemSelected`/`FocusOnItem` |
| Canvas markers independent of the dialog | missing | `SCH_MARKER` objects drawn via the normal VIEW; no canvas-marker rendering added this session |
| Exclusions (persisted "accepted" findings) | missing | `SCHEMATIC::RecordERCExclusions`/`ERC_SETTINGS::m_ErcExclusions` — `eda_kicad::Exclusions` exists engine-side (`check_erc_excluding`) but nothing in studio.rs/the UI writes to it yet |

## 5. Annotate

`Ctrl+A` now opens `AnnotateDialog.tsx` instead of calling `Cmd::Annotate`
directly -- scope, order and reset are all real choices, confirmed by
three new `crates/cli` tests (default order vs. the opposite order,
explicit-selection scope leaving everything else untouched).

| Action | Status | KiCad file:function |
|---|---|---|
| Assign reference designators to unannotated (`"U?"`-style) symbols | identical for the one scope this IR actually has | `dialog_annotate.cpp` (`INCREMENTAL_BY_REF` -- this project's numbering starts at 1 per prefix, not KiCad's configurable start-at-0 default, unchanged from before this session) |
| Scope: whole sheet / current selection | partial — source's own three (Schematic/Sheet/Selection) collapse to two here: this IR has no sheet hierarchy, so "Schematic" and "Sheet" are the same "whole sheet" scope `Cmd::Annotate`'s new `ids: None` already was; "Selection" is new (`ids: Some(selected symbol ids)`) | `dialog_annotate.cpp`'s scope radio group; `new Cmd::Annotate.ids` |
| Order: sort by Y then X (default) / X then Y | identical | `dialog_annotate.cpp`'s order radio group (`SORT_BY_Y_POSITION` default); `new Cmd::Annotate.order`/`AnnotateOrder` |
| Numbering scheme: First Free / Sheet x100 / Sheet x1000 | not applicable — all three only differ for a multi-sheet hierarchy numbering each sheet into its own block; this IR has exactly one sheet, so there is nothing for this control to choose between (left out of the dialog entirely rather than shown as a dead control) | `dialog_annotate.cpp`'s numbering-scheme radio group |
| "Clear and re-annotate" (reset existing) vs "Keep existing" | identical, and now reachable from the UI | `Cmd::Annotate.reset_existing`, already implemented; `AnnotateDialog.tsx`'s checkbox |

## 6. Cross-tab undo/redo (gap #15)

| Action | Status | KiCad file:function |
|---|---|---|
| Ctrl+Z/Y on the Schematic tab only ever reverts/replays that tab's own edits (a clean no-op once its own stack is empty, never a silent PCB revert) | identical | Internal fix, no KiCad source counterpart (KiCad has genuinely separate editor processes/undo buffers; this app has one shared `design.json`). `crates/ops::Domain` + `Cmd::domain`, `crates/cli/src/board.rs`'s `push_snapshot`/`pop_snapshot`/`restore_domain` (tag each undo-stack entry by domain, splice only that domain's fields back on restore), `POST /api/undo`/`/api/redo`'s new `{"domain": ...}` body, `store.tsx`'s `api.undo`/`redo` (always pass `state.tab`). Proven by `board.rs`'s `undo_redo_are_scoped_to_the_tab_that_asked` test: one PCB edit + one schematic edit, every undo/redo combination checked. `eda board undo`/`redo` (CLI, no tab concept) keep the original unscoped behavior (`scope: None`) |

## Manual click-through needed

The in-app browser pane could not be used this session (hidden pane, per
the task brief) — everything above is verified by `cargo test`/`npm run
test:unit`/`typecheck`/`build` only. Before trusting this in real use,
click through:

1. Open the Schematic tab on a board with 2+ symbols. Click a symbol
   (selects, highlights); Shift/Ctrl-click a second (adds/toggles); drag a
   box around several (crossing vs. enclosed — drag direction matters).
2. Press `M`, move the mouse (ghost should follow, snapped to the 1.27mm
   grid), click to drop. Confirm `GET /api/schematic` now shows the new
   position and the PCB tab's part (same ref) did *not* move.
3. With a symbol selected, `R`/Shift+`R` a few times, then `X`. Confirm
   the symbol visibly rotates/mirrors and the backend accepts it (no
   toast error).
4. Select a symbol, `M`, then `R` *mid-move* before clicking to drop —
   confirm it rotates live and commits rotated-and-moved in one step.
5. `Del` a symbol; confirm any wire that was touching its pins is now
   drawn landing on nothing (dangling), not deleted itself.
6. Make a PCB edit (e.g. nudge a footprint), switch to the Schematic tab,
   move a symbol, then Ctrl+Z twice: first undo reverts the schematic
   move only; second undo is a no-op (check the status toast), and the
   PCB edit from step 1 is still there. Switch to the PCB tab and Ctrl+Z
   — *that* reverts the PCB edit. This is the gap #15 regression test.
7. Press `W`, click near one pin (should snap exactly onto it), click
   near another symbol's pin (should auto-finish the wire there);
   confirm `GET /api/schematic` shows one wire whose `net`/`pins` name
   both pins. Then switch to the PCB tab: since `ratsnest_json` and
   `erc_json` both go through the same `board::load` that applies
   `design.nets`, the two pins' footprints should now show a ratsnest
   airwire between them if they weren't already connected — this is the
   "PCB ratsnest follows schematic" hard rule, worth confirming with a
   real board since it was only exercised by the Rust unit test, not
   through the actual HTTP/ratsnest path, this session. Try Backspace
   mid-draw (should remove the last bend) and Escape (discard the wire).
8. Open the ERC dialog (menu — Inspect/wherever `MenuBar.tsx` surfaces
   `eeschema.InspectionTool.runERC`) on a board with known ERC issues;
   confirm the list is non-empty and matches roughly what `cargo run --
   board erc` / `kicad-cli sch erc` would report; click a row and confirm
   it selects a symbol and switches tabs.
9. Draw a wire (`W`) from one symbol's pin to another's so they're
   connected, select the first symbol, press `G`, move the mouse — the
   wire's endpoint at that symbol should visibly track it (rubber-band)
   while the wire's other end (still on the second symbol's pin) stays
   put; click to drop, then confirm `GET /api/schematic` shows the wire's
   point still landing exactly on the moved symbol's new pin position
   (still connected, not dangling). Compare against `M` on the same setup
   — the wire should visibly NOT follow, staying dangling where the
   symbol used to be.
10. Without pressing `M`/`G` first, click directly on a symbol and drag it
    (same click, hold, move gesture) — should behave like `G` above
    (rubber-band) by default by just moving the mouse past the first grid
    step; releasing without moving the mouse at all should behave like a
    plain click (select only, nothing committed — check no spurious
    `/api/cmd` POST in the network tab). With 2+ symbols selected, click
    and drag one member of the group — the whole group should move
    together, not just the one clicked.
11. Press `L`, click near a wire/pin (should snap exactly onto it) — the
    Label dialog should open pre-filled with an empty (first time) or
    auto-incremented (after a prior placement) net name; confirm, then
    click again elsewhere and confirm a second time with the suggested
    text incremented by one (e.g. DATA0 → DATA1 if you typed "DATA0" the
    first time). Confirm the tool stays armed (status bar still shows the
    Label tool message) until Escape or `L` again. Repeat for Ctrl+`L`
    (global) and `H` (hierarchical) — both should also show the Shape
    dropdown the local-scope dialog doesn't.
12. Press `P`, click near a pin — the Power Symbol dialog should open;
    type/pick "power:GND", confirm, and check the symbol renders (a GND
    tee/rail shape) at the clicked pin, and `GET /api/schematic` shows a
    new `power_symbols[]` entry whose `net` is "GND". Try a custom rail
    name (e.g. "power:+1V8") and confirm it still renders (the generic
    rail shape) rather than erroring.
13. Press `T`, click anywhere, type some text, confirm — check it renders
    in the schematic (plain stroke-font text, no net) and
    `GET /api/schematic` shows a new `texts[]` entry. Select it on the PCB
    tab's export (`cargo run -- board export-kicad-sch` or equivalent) and
    confirm a real KiCad instance (or at least `kicad-cli sch export`)
    doesn't choke on the new `(text ...)` block.
14. Press `Q`, click a pin that has nothing on it — confirm an X marker
    appears there and stays armed for a second click elsewhere. Note: a
    placed no-connect can't yet be selected/deleted from the canvas (see
    section 2's own row) — undo (Ctrl+Z) is the only way back right now.
15. Press `A` — the Place Symbol dialog should open immediately (before
    any click). Type "R" in the search box — the list should narrow to
    resistor-shaped entries (at minimum "Device:R" from the builtin
    fallback, since this environment has no real KiCad install at
    `EDA_KICAD_SYMBOLS`/the default path); clicking one should draw its
    real graphics in the preview pane, not a blank box. Confirm (double-
    click the row, or the Place button) — the dialog should close and the
    status bar should show the Place Symbol tool message. Click twice on
    the canvas; confirm two new resistors appear with sequential
    references (e.g. "R1" then "R2", or continuing from whatever `R`
    count is already on the sheet) and `GET /api/schematic` shows both
    with real pin/graphics data resolved. Press `A` again and place a
    "Device:C" — confirm it gets "C1", independent of the resistor
    numbering. Try a board with an intent that references a part from a
    real installed library (if one is configured) and confirm the
    chooser's list for that library name shows every symbol the real
    `.kicad_sym` file defines, not just the one part already used.
16. **The hotkey-dispatch fix, highest priority to confirm**: on the PCB
    tab, select a footprint and press `R` -- it must still rotate (this
    was the already-broken-on-main case the fix targeted). Press `M`,
    move it, click to drop -- must still work. Press `E` -- the PCB
    FootprintPropertiesDialog must still open (not silently do nothing).
    Select a track and press `V` (layer toggle) and `F` (flip a
    footprint) -- both must still fire. Then switch to the Schematic tab
    and confirm `R`/`M`/`E`/`V`/`F` all do the *schematic* thing instead
    (rotate/move a symbol, open Symbol Properties, edit value, edit
    footprint) -- not a silent no-op, and not the PCB action leaking
    through.
17. Select a symbol, press `E` -- Symbol Properties should open with
    Reference/Value/Footprint/Datasheet pre-filled from the real symbol.
    Change Value and Footprint, OK -- confirm `GET /api/schematic` shows
    both updated and nothing else changed. Reopen, change Reference to
    an id already used by another symbol -- confirm it's refused with a
    message, dialog stays open. Change it to a fresh id instead -- confirm
    the symbol renames and (if it had a wire/power-symbol/no-connect
    attached) the connection survives the rename. Press `U`/`V`/`F`
    directly (no `E` first) on a selected symbol -- confirm the same
    dialog opens with the right field pre-selected/highlighted for
    immediate typing.
18. Place a couple of unannotated symbols (e.g. via `A`'s chooser, or
    `E`'s Reference field renamed to end in `?`), then press Ctrl+`A` --
    Annotate Schematic should open with "Whole sheet" selected (nothing
    was selected) and "Sort by Y" the default. Confirm -- both get real
    references. Select one symbol first, reopen the dialog -- "Selection
    only (1 symbol)" should now be the default and the whole-sheet radio
    still pickable; rename that one symbol's reference to end in `?`
    again (via `E`), check "Selection only", confirm -- only that one
    symbol should renumber, everything else on the sheet untouched. Try
    "Clear and re-annotate" with "Sort by X" on the whole sheet and
    confirm the numbering order visibly follows X position instead of Y.
