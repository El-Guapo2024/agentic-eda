# Schematic editor parity with KiCad eeschema

The Symbol Editor (eeschema's own, a separate sub-application from the
schematic editor this file otherwise covers) now has its own tab and its
own parity doc: see `PARITY-symedit.md`. `Ctrl+Shift+E` on a placed symbol
(section 1's `E`/`U`/`V`/`F` row) jumps there -- confirmed against real
source this session as the genuine `editLibSymbolWithLibEdit` hotkey,
*not* plain Ctrl+E (that's `pcbnew`'s unrelated Footprint Editor jump).

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
`EditSymbolFields`), Annotate's dialog (section 5 --
`AnnotateDialog.tsx`, new `Cmd::Annotate.order`/`.ids`), `Y` mirror
(section 1 -- new `SymbolInstance::mirror_y`, `Cmd::MirrorSymbolVertical`,
mutually exclusive with `X`/`mirrored`), and ERC canvas markers +
exclusions (section 4 -- new `components/schematic/ercMarkerPosition.ts`
resolving a `CheckResult::location` string to a canvas point/refs,
`painter.ts`'s `drawErcMarkers`, and `Cmd::AddErcExclusion`/
`DeleteErcExclusion` persisting to `design.schematic.erc_exclusions`,
applied by the pre-existing `check_erc_excluding` and surfaced as a third
`"excluded"` `ErcSeverity`), and item 8 -- wire box-select plus one real
`sch_line_wire_bus_tool.cpp`-adjacent correctness fix (section 1/2 --
`components/schematic/boxSelection.ts` extends box-select to wires, and
`components/schematic/junctions.ts` fixes a found-not-told junction-dot
gap: a wire ending partway along another wire, or a power-symbol/label
anchor dropped onto one, was already correctly connected model-side
-- `reconcile`'s own T-junction union-find predates this session -- but
drew no dot, since the pre-existing dot logic only ever matched 3+
exactly-coincident wire endpoints, never the interior-landing case the
name "T-junction" actually describes). No backend changes were needed
for item 8 -- both fixes are rendering/selection-side only, since the
connectivity model was already correct. Still not wired: `J` junction,
real KiCad's bus wires/bus entries/bus unfolding (no IR concept of a bus
at all -- a new subsystem, not an edge case of the existing wire tool,
deliberately out of scope this pass), and the finer per-segment box-
select/overlap-trim/collinear-simplify functions `sch_line_wire_bus_tool
.cpp` also has (`TrimOverLappingWires`/`simplifyWireList`) -- this app's
one-polyline-per-wire model doesn't produce the kind of duplicate/
overlapping-segment mess those exist to clean up after, so no concrete
bug motivated porting them — see the bottom of each section.

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
| Box select, left→right = fully enclosed, right→left = crossing | identical for symbols and wires | `common/tool/selection_tool.cpp::SelectRectArea` (`isCrossingSelection`, shared with the PCB port); `components/schematic/boxSelection.ts::collectBoxSelection` (extracted from `SchematicView.tsx` and unit-tested this session when wires were added) |
| Box select: wires (crossing = whole wire if touched at all; enclosed = both endpoints, or one if it's dangling) | partial — a touched/enclosed wire's *whole* polyline is selected as one atomic unit (bounds test over every point, not just its own two ends — see `boxSelection.test.ts`'s multi-bend test); source's own finer per-`SCH_LINE`-segment granularity (a box can touch just one bend of a multi-segment wire and select only that piece) has no equivalent here since this app's `Wire.pts` is already one polyline per wire, not source's many atomic 2-point segments (same architecture note `G`'s own row above makes). `Del` on a box-selected wire works (`common.Interactive.delete`'s existing `api.wireById` branch); `M`/`G` on a wire-only selection does not — there is no `move_wire`/`drag_wire` Cmd at all, so a selected wire can be deleted but not moved independent of a symbol | `sch_selection_tool.cpp::SelectMultiple` lines ~2640-2666 — researched a prior session, ported (to this app's own coarser-grained wire model) this one |
| Pins/junctions win ties over symbol body/wire at an exact hit | missing (no finer-than-symbol hit test yet) | `sch_selection_tool.cpp::GuessSelectionCandidates`/`narrowSelection` |
| `M`: move (breaks wire connections — a wire's endpoint is a bare coordinate, not a pin reference) | identical | `sch_move_tool.cpp` (`setupItemsForDrag` never adds connected wires in MOVE mode); `Cmd::MoveSymbol`, `SchematicView.tsx`'s `moveMode` branch |
| `G`: drag (attached wire endpoints rubber-band) | identical for this app's IR (see the gap noted below for what's left out) | `sch_move_tool.cpp::getConnectedDragItems` (`wireAttachment.ts::attachedWireEndpoints`, same arm-then-click-to-drop flow as `M`, `Cmd::DragSymbol`). A wire here is one polyline (every bend from one `W` session), not a separate `SCH_LINE` per segment, so only its own two true ends are tested for attachment -- matching `pinSnapPoints`'s existing "landing on a pin" convention. Every wire at a junction a dragged pin sits on attaches and moves together |
| New stub wire at an unselected 3-way junction (source's own fallback when a junction's *other* wires are deliberately not moving) | not applicable to this IR | `ptHasUnselectedJunction`'s branch only matters when a caller can select one wire at a junction independent of the symbol being dragged and then move it -- wire box-select exists now (item 8), but there is still no `move_wire`/`drag_wire` Cmd at all (see that row above), so a selected wire can never actually be the thing being dragged; every wire at a junction a dragged *symbol's* pin sits on is still always fully attached (previous row) and a stub is still never needed |
| Grid snap during move | identical (grid only) | `edit_tool_move_fct.cpp` (`kicad-port/gridSnap.ts::alignToGrid`, reused) |
| Anchor/pin snap during move | missing | `ee_grid_helper.cpp` — per-item-category grids + pin-anchor snap not ported; PCB side has the analogous gap too (`PARITY-pcb.md`) |
| Escape cancels an in-progress move without touching the prior selection | identical | shared `ESCAPE` reducer case (already generic across tabs) |
| `R`: rotate CCW | identical | `sch_edit_tool.cpp::Rotate` (confirmed default is CCW, not CW); `Cmd::RotateSymbol` |
| Shift+`R`: rotate CW | identical | same function, `rotateCW` action |
| R/Shift+R during an active move updates the live preview instead of committing separately | identical | `sch_move_tool.cpp::handleMoveToolActions` (`tryTransformDuringMove`, shared helper, now tab-aware) |
| Rotate pivot: own anchor (single item) / collective bbox center (multi-select) | partial — only single-selection rotate is wired (one symbol id) | `sch_edit_tool.cpp::Rotate` |
| `X`: mirror horizontally (negate-X, KiCad's `SYM_MIRROR_Y`) | identical | `sch_edit_tool.cpp::Mirror` (`mirrorH`); `Cmd::MirrorSymbol`, toggles `SymbolInstance::mirrored` |
| `Y`: mirror vertically (`SYM_MIRROR_X`) | identical | `sch_edit_tool.cpp::Mirror` (`mirrorV`); new `SymbolInstance::mirror_y` + `Cmd::MirrorSymbolVertical`, mutually exclusive with `mirrored`/`MirrorSymbol` (toggling one clears the other, same as a real `SetOrientation` call replaces the whole orientation -- KiCad's own symbols never carry both at once). `transform_local_point` (reconcile's/import's pin-world resolution) extended to the second axis and cross-checked numerically against `transform.ts`'s own matrix table (already correct on the frontend, which was built two-axis-ready ahead of this field existing -- see its own "Found while wiring..." note two rows down) rather than re-derived from a description: `mirror_y` *cancels* the always-applied library-Y-up/sheet-Y-down flip instead of negating local X the way `mirrored` does, confirmed by a unit test matching all 4 rotations against the frontend's matrices, plus a real `.kicad_sch`-level round-trip test (`(mirror x)`/`(mirror y)` tags reading back to the correctly-opposite-named field -- `sch_io_kicad_sexpr_parser.cpp`'s own token names don't match this app's own field names 1:1, an easy, wrong-rendering mistake to make without checking source directly). `export_kicad_sch`'s own generic-box baking (`baked_local`, used only for a *synthesized*, non-library-resolved symbol) is **not** extended to the new axis -- a narrower, documented gap, since that function's local-box mirror math is a different convention than `transform_local_point`'s and real/library-resolved symbols (the common case) are unaffected |
| **Found while wiring this**: `api/types.ts`'s `SchematicSymbol.mirror: "x"\|"y"\|null` and the renderer that reads it (`transform.ts`, `libSymbol.ts`) were already fully two-axis, written *ahead of* this backend field by the session that merged the Eeschema-port itself -- the backend was only ever sending the single legacy `mirrored` boolean, so `X` mirroring only ever rendered at all because `api/client.ts`'s `fetchSchematic` already had a same-session compatibility shim (`sym.mirror ?? (legacy.mirrored ? "y" : null)`). `schematic_json` now sends a real `"mirror": "x"\|"y"\|null` field (plus `"mirrored"`, redundant but harmless, for that same shim), so the fallback is now dead code but was never a bug to fix -- no frontend rendering changes were needed for either axis | n/a | n/a |
| Mirror during an active move | missing | `sch_edit_tool.cpp::Mirror`'s own `IsMoving()` branch (asymmetric from Rotate's in source itself — no `updateStoredPositions()` call) |
| `Del`: delete symbol, wires left dangling (no cascade) | identical | `sch_edit_tool.cpp::DoDelete` (confirmed: wires are never auto-deleted); `Cmd::DeleteSymbol`, `common.Interactive.delete`'s schematic branch |
| `Del`: delete wire | identical (wire must already be selected -- a modified click, or now a box-select, item 8) | `Cmd::DeleteWire` |
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
| Auto-junction at a T (3+ wire exit angles) | identical, computed live from geometry, never a stored item (matching this project's own IR -- no `junctions` field, see `Cmd::AddWire`'s own doc). Connectivity was already correct before this session (`reconcile`'s own T-junction union-find pass, and `erc.rs`'s dangling checks, both already treated a wire endpoint landing on another wire's *interior* as a real connection) -- but the dot itself was not: the pre-existing `painter.ts::junctionPoints` only ever drew one for 3+ wire endpoints *exactly coincident* (a "star"), never for the classic T shape this row is actually named for (one wire ending partway along another's run), so a correctly-connected T drew no visual confirmation of it at all. Fixed this session: `components/schematic/junctions.ts` (extracted, unit-tested) ports the backend's own `point_on_segment_interior` test arithmetic-for-arithmetic, and also now covers a power-symbol or label anchor landing on a wire's interior (common in practice -- dropping a GND flag or a net label onto an existing wire rather than ending the wire exactly there), which had the identical dot-less symptom for the identical reason | `junction_helpers.cpp::AnalyzePoint`; `reconcile`'s own T-junction union-find pass |
| `J`: explicit junction at a plain crossing | identical (new stored `junctions` list; see section 11 -- an explicit junction now joins two crossing wires, the computed T-dots above stay computed) | `SCH_DRAWING_TOOLS::SingleClickPlace`, `junction_helpers.cpp::AnalyzePoint` |
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
| Click a row: cross-probe (select + pan/zoom to it) | identical in spirit | `DIALOG_ERC::OnERCItemSelected`/`FocusOnItem`; `ercMarkerPosition(v.location, state.schematic)` resolves `location`'s several shapes (see types.ts's `ErcViolation.location` doc) back to a point/refs, then `jumpTo` selects/hots the refs and re-frames the schematic view exactly like `DrcDialog.tsx`'s own `jumpTo` (2mm padding around the resolved point, same `fitTransform` call). A location that resolves to `null` (a dangling ref/net, or a shape `ercMarkerPosition` doesn't recognize) still selects the row and switches tabs, just without re-framing — not expected in practice since every shape `check_erc` actually emits today resolves |
| Canvas markers independent of the dialog | identical in spirit, same gate DRC's own markers already have | `SCH_MARKER` objects drawn via the normal VIEW; `painter.ts`'s `drawErcMarkers` (ported from `canvas/painter.ts`'s `drawDrcMarkers`), one circle per violation whose `location` resolves to a point, color-coded by severity (`LAYER_ERC_ERR`/`WARN`/`EXCLUSION`), drawn whenever `state.erc` is populated -- same "only fetched while the dialog is open" gate `state.drc`/DRC markers already live with (store.tsx's poll loop), not a deeper ERC-specific gap |
| Exclusions (persisted "accepted" findings) | partial | `SCHEMATIC::RecordERCExclusions`/`ERC_SETTINGS::m_ErcExclusions` — `eda_kicad::Exclusions`/`check_erc_excluding` already existed engine-side; new `Cmd::AddErcExclusion`/`DeleteErcExclusion` persist to `design.schematic.erc_exclusions` (`crates/model::ErcExclusion`, matched by exact `(check, location)` string pair), surfaced as a third `ErcSeverity::Excluded` so an excluded finding stays visible (dimmed row/marker, its own "Exclusions" filter toggle) instead of disappearing, with an Exclude/Un-exclude button per dialog row. Real KiCad also offers this from a right-click on the canvas marker itself (`DIALOG_ERC`'s context menu) -- this clone only has the dialog-row button, no canvas context menu |

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

## 7. Symbol Fields Table (`dialog_symbol_fields_table.cpp`, `fields_data_model.cpp`)

`Tools > Bulk Edit Symbol Fields...` (`eeschema.EditorControl.editSymbolFields`,
already in `sch_menus.json`/`sch_toolbars.json`, previously disabled) opens
`SymbolFieldsTableDialog.tsx`. The grid model is ported to Rust
(`crates/ops/src/fields_table.rs`) and served by `POST /api/sch/fields_table`;
the staging of edits is a small pure TS module
(`kicad-port/fieldsTableStage.ts`, tested). KiCad's own source was read from
the `kicad-src` snapshot (commit `8303b2ad`).

| Action | Status | KiCad file:function ↔ our code |
|---|---|---|
| Grid of all symbols × fields (Reference, Qty, Value, Footprint, Datasheet, user fields) | partial — covers the design's symbols (the root sheet's `design.schematic`), no scope selector; no attribute columns | `FIELDS_EDITOR_GRID_DATA_MODEL::RebuildRows`/`GetValue` ↔ `fields_table::build_table`, `POST /api/sch/fields_table` |
| "Group symbols" + per-column "Group By" (default Value + Footprint = "Grouped By Value and Footprint") | identical | `groupMatch`/`unitMatch`/`BOM_PRESET::GroupedByValueFootprint` ↔ `Model::group_match`, `unit_match`, `TableSpec::default_editing` |
| Collapsed references `R1-R3, R5` (grid: `", "` / `"-"`; export: configurable) | identical, incl. KiCad's quirks: a run of exactly two is listed (`R1, R2`) not ranged; an empty range delimiter disables ranges; references sorted with `StrNumCmp`; other units of a multi-unit symbol de-duplicated | `SCH_REFERENCE_LIST::Shorthand` ↔ `fields_table::shorthand` (unit tests in `fields_table.rs`) |
| `${QUANTITY}` column, `${ITEM_NUMBER}`, "-- mixed values --" for a group whose members disagree | identical (Qty column shown by default; Item number is computed server-side but has no default column) | `GetValue(group, ...)`, `INDETERMINATE_STATE` |
| Expand/collapse a group into its per-symbol rows | identical in effect (child rows are read-edit rows of one symbol) | `ExpandRow`/`CollapseRow` ↔ `TableRow::children` |
| Show/hide columns, click header to sort, filter box | partial — sort uses a simplified `ValueStringCompare` (leading text, then number with SI suffix incl. `4k7`, then text); the filter is a case-insensitive substring (or `*`/`?` wildcard containment) rather than the full `EDA_COMBINED_MATCHER` | `Sort`/`cmp`/`ValueStringCompare`, `m_filter` ↔ `build_table`, `value_string_compare` |
| Add / rename / remove user field columns | identical; user fields live in new `SchematicSection::user_fields` (`#[serde(default)]`, reference → name → text, part-wide like Value/Footprint) rather than per placed unit; an added column creates the (empty) field on every symbol (`userAdded`); a removed column erases it everywhere; Reference/Value/Footprint/Datasheet/`${...}` cannot be renamed/removed | `AddColumn`/`RenameColumn`/`RemoveColumn`, `ApplyData` ↔ `fields_table::apply_field_changes` |
| Inline cell edit (not Reference, not generated columns); editing a group row sets every symbol in it | identical | `FIELDS_EDITOR_GRID_DATA_MODEL::SetValue` ↔ `SymbolFieldsTableDialog.tsx` `commitCell` → `stageEdit` per ref |
| Staged edits regroup the table before Apply; Apply commits; Close with unapplied edits asks | identical in effect — the server overlays the staged changes on the schematic *without saving* (`m_dataStore` equivalent) | `m_dataStore`, `OnClose`/`HandleUnsavedChanges` ↔ `staged_schematic` in `sch_api.rs`, `window.confirm` |
| Apply is one undoable step | identical | `ApplyData` + one `SCH_COMMIT` ↔ `Cmd::SetSymbolFields { edits, add_fields, rename_fields, remove_fields }` (applied remove → rename → add → edits; atomic — an invalid part refuses the whole batch). Undo = the existing snapshot undo, schematic scope |
| Rename / delete a symbol carries its user fields along | identical | `rename_symbol` / `delete_symbol` in `crates/ops` move/drop `user_fields[ref]` |
| Export tab: format presets CSV / TSV / Semicolons, field/string/reference/range delimiters, keep tabs, keep line breaks, live preview, write file | identical output format (`BOM_FMT_PRESET::CSV()` etc., doubled embedded string delimiters, `\n` after the last shown column, hidden columns omitted, children not exported, mixed values listed comma-separated) | `FIELDS_EDITOR_GRID_DATA_MODEL::Export`, `BOM_FMT_PRESET`, `OnExport` ↔ `fields_table::export_bom`, `BomFmt`, `POST /api/sch/bom_export` (separate from, and richer than, `POST /api/fab/bom`/`eda_fab::bom_csv`, which stays the JLC-style quick BOM) |
| Export target path | partial — relative to the board directory only (no `..`, no absolute path: a studio served over HTTP must not write anywhere); default `export/<intent>-bom.csv`; no `${VAR}` expansion, no file browser | `OnExport`, `EnsureFileDirectoryExists` ↔ `sch_api::safe_relative` |
| Attribute columns (`${DNP}`, `${EXCLUDE_FROM_BOM}`, ...), "Exclude DNP" / "Include excluded from BOM" filters, variants | missing — the IR has no DNP / BOM-exclusion attributes or variants | `BOM_PRESET::excludeDNP`, `isAttribute` |
| Saved view presets ("Grouped By Value" etc.), field name templates sidebar, scope (sheet / recursive), "Add field from library", 'Sidebar' | missing — the view (columns, grouping, sort, filter) resets to the default each time the dialog opens | `BOM_PRESET`, `m_scope` |

## 8. Find / Find and Replace (`sch_find_replace_tool.cpp`, `eda_item.cpp`, `dialog_sch_find.cpp`)

`Ctrl+F` (Find), `Ctrl+Alt+F` (Find and Replace), `F3` / `Shift+F3` (Find
Next / Previous) -- all four are the hotkeys already in `actions.json`
(KiCad's real defaults), registered on the Schematic tab only. Note
`Ctrl+H` is KiCad's *Hierarchy Navigator* (`eeschema.EditorTool.showHierarchy`),
not Replace, so it is deliberately not rebound. Matching/replacing is Rust
(`crates/ops/src/search.rs`, `POST /api/sch/find`); cycling is the pure,
tested `kicad-port/schFind.ts` + `components/schematic/findNavigation.ts`.

| Action | Status | KiCad file:function ↔ our code |
|---|---|---|
| Plain / Whole word / Wildcard (`*`, `?`) matching, Match case | identical (word chars = alphanumeric or `_`; wildcard = whole-string `wxString::Matches`; case-insensitive by default via upper-casing both sides) | `EDA_ITEM::Matches( aText, aSearchData )` ↔ `search::matches_text`, `wild_match` |
| Regex mode, permissive mode | missing — the workspace carries no regex engine (and the eeschema dialog exposes regex only as an optional checkbox) | `EDA_SEARCH_MATCH_MODE::REGEX`/`PERMISSIVE` |
| Search all fields (hidden fields too) | partial — the IR has no per-field visibility, so KiCad's template defaults apply: Reference/Value visible; Footprint, Datasheet and user fields hidden (found only with the option) | `SCH_FIELD::Matches` (`searchAllFields`) ↔ `search::field_visible` |
| Reference field matches when its *symbol* matches; skipped in replace mode unless "Replace in references" | identical | `SCH_FIELD::Matches` → `SCH_SYMBOL::Matches`, `replaceReferences` ↔ `search::find_items` |
| Labels match on their text; net names; free text | partial — a label's net name is its own text here, so "Search net names" only adds *pin* net-name matches (via the intent's nets); `CTX_NETNAME` escaping of find/replace strings is not applied | `SCH_LABEL_BASE::Matches`/`Replace`, `SCH_TEXT` ↔ `search::find_items` |
| Search pins (name/number) and pin net names; pins never replaceable | identical in effect (pins match only outside replace mode) | `SCH_PIN::Matches`/`Replace` |
| Sheet names/pins/fields, symbol metadata (library name/description/keywords), "Find Next Marker" | missing | `SCH_SHEET`/`searchMetadata`/`markersOnly` |
| Match order & cycling: ascending x then y, reverse for Previous; at the end "Reached end of schematic. Find again to wrap around to the start." and the next Find wraps | identical in order and wrap behavior; every field of a symbol sits at the symbol's own position (no per-field position in the IR), ties keep field order | `SCH_FIND_REPLACE_TOOL::nextMatch`/`FindNext` ↔ `find_items`, `pickMatch` |
| Each hit is selected, highlighted and the view re-centered on it (zoom kept) | identical in spirit (same select + `FocusOnLocation` pair; the "force visible" brightening is the app's existing hot/selection highlight) | `FindNext`: `AddItemToSel`, `BrightenItem`, `FocusOnLocation` ↔ `findNavigation.ts::visitMatch` (the ERC dialog's `jumpTo` pattern) |
| "Search only selected objects" | identical for find and replace-all (scope = selected owner ids) | `searchSelectedOnly` |
| "Current sheet only" | not applicable — one sheet's schematic is searched/edited | `searchCurrentSheetOnly` |
| Replace (current match, then Find Next) / Replace All, undoable | identical; `Cmd::ReplaceText { search, items }` is one verb (all matches → one undo step; one `items` key → "Replace"); a Reference replace renames the symbol with the same cascade as `RenameSymbol`; refused (`ops_nothing_to_replace`) when nothing matched; replacing a label to empty is refused | `ReplaceAndFindNext`, `ReplaceAll`, `EDA_ITEM::Replace` ↔ `Board::replace_text`, `search::replace_text` (case-insensitive search copies the *original* text around hits, as the source does) |

## 9. ERC pin-conflict matrix (`panel_setup_pinmap.cpp`, `erc_settings.cpp`)

`File > Schematic Setup...` (`eeschema.EditorControl.schematicSetup`,
already in the menu/toolbar, previously disabled) opens
`SchematicSetupDialog.tsx`, which currently holds the one page that has a
backing setting: Electrical Rules > Pin Conflicts Map.

| Action | Status | KiCad file:function ↔ our code |
|---|---|---|
| The pin map is a per-design setting used by ERC | identical — stored additively as `schematic.erc_pin_map: Option<ErcPinMap>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`; 12×12 `PIN_ERROR` ints like KiCad's `pin_map` project entry); absent = KiCad's default table; a malformed grid is ignored, an out-of-range cell keeps its default | `ERC_SETTINGS::m_PinMap`/`m_defaultPinMap`, `pin_map` loader ↔ `eda_model::ir::ErcPinMap`/`DEFAULT_ERC_PIN_MAP` (the table moved from `erc.rs`'s private `MATRIX` into the IR crate so there is one copy), `eda_kicad::resolve_pin_map`, `check_pin_to_pin` |
| 11×11 lower-triangle grid (NC excluded: "generates errors separately"), row/column labels | identical | `PANEL_SETUP_PINMAP::reBuildMatrixPanel`, `CommentERC_H/V` ↔ `SchematicSetupDialog.tsx`, `kicad-port/ercPinMap.ts` |
| Click a cell cycles OK → Warning → Error → OK, symmetric | identical (green ✓ / amber ! / red ×, with the source's tooltips) | `changeErrorLevel` (`( level + 1 ) % 3`, `SetPinMapValue( y, x )` and `( x, y )`) ↔ `Cmd::SetErcPinMapCell { a, b, level }` (undoable, schematic scope) |
| "Reset to Defaults" | identical (stored as an absent `erc_pin_map`; refused when already default; undoable) | `PANEL_SETUP_PINMAP::ResetPanel`/`ERC_SETTINGS::ResetPinMap` ↔ `Cmd::ResetErcPinMap` |
| ERC picks it up | identical — an Output↔Output cell set to OK removes the `pin_to_pin` finding, Warning downgrades it (tested in `erc.rs`); the open ERC dialog refreshes through the usual version poll | `ERC_TESTER::TestPinToPin` ↔ `eda_kicad::erc::check_pin_to_pin` |
| ERC severity per check (Ignore / Warning / Error) | **gap** — does not fall out naturally: `crates/kicad/src/erc.rs` and `erc_style.rs` hard-code each check's `Fail`/`Warn` status inside ~30 separate `check_*` functions with no severity table to override, and `Ignore` has no representation in `CheckStatus`. Needs a per-check severity map in the IR plus a post-pass like `check_erc_excluding`'s | `panel_setup_severities.cpp`, `ERC_SETTINGS::m_ERCSeverities` |
| Other Schematic Setup pages (general, formatting, annotation, field templates, net classes, text variables, bus aliases, ...) | missing — not shown as dead tabs | `dialog_schematic_setup.cpp` |
## 10. Plot and netlist export (`sch_plotter.cpp`, `netlist_exporter_*.cpp`)

File > Plot... on the Schematic tab (`common.Control.plot`, which is shared
with the PCB tab's Gerber plot and so dispatches by tab -- there is no
`eeschema.EditorControl.plot` in the real action table) and File > Export >
Netlist... (`eeschema.EditorControl.exportNetlist`) now work end to end:
`PlotSchematicDialog.tsx`/`ExportNetlistDialog.tsx` -> `POST /api/sch/plot`/
`/api/sch/netlist` (`crates/cli/src/sch_api.rs`) -> `eda_kicad::plot_schematic`/
`export_netlist`, writing into the board's `export/`. Both are read-only
exports (nothing is written to `design.json`), so there is no `/api/cmd`
verb or undo entry. The netlist reads `ConstraintModel::nets` -- the one
netlist of section 0, with `Design::nets` already applied -- and never
re-derives connectivity from geometry. A board with no `schematic` section
plots/exports the schematic the engine would derive from its intent, like
`GET /api/schematic.svg`.

The plotter is a port of KiCad's `PLOTTER` class hierarchy
(`crates/kicad/src/plotter/`): one abstract `Plotter` trait
(`MoveTo`/`LineTo`/`FinishTo`/`PenTo`, `Circle`, `Arc`, `Rect`, `PlotPoly`,
`Text`, `SetColor`/`SetDash`/`SetCurrentLineWidth`), an SVG back end that
reproduces `SVG_PLOTTER`'s lazy `<g style=...>` grouping, mm device units and
4-digit precision, and a hand-written PDF back end that reproduces
`PDF_PLOTTER`'s object table / deferred `/Length` / page tree / outline /
`xref`+`trailer` layout (uncompressed streams -- KiCad's own
`m_DebugPDFWriter` path; the workspace has no zlib). Coordinates are
eeschema IU (0.1 um).

| KiCad feature | Status | KiCad source <-> our code |
|---|---|---|
| Plot dialog: format, colour/B&W, drawing sheet, background colour, page size, all/current page | partial | `DIALOG_PLOT_SCHEMATIC` / `SCH_PLOT_OPTS` <-> `PlotSchematicDialog.tsx`, `kicad-port/schOutputs.ts` (`buildSchPlotRequest`, unit-tested), `sch_api::plot`, `eda_kicad::SchPlotOpts`. Not ported: PostScript/DXF/PNG formats (HPGL is gone upstream), colour-theme chooser, hop-over, PDF property popups / hierarchical links / metadata, `m_plotPages` in the dialog (the API takes `pages`), output-directory/file-name fields (always `export/`) |
| SVG: one file per sheet, named `<root>-<sheet>-...` | identical | `SCH_PLOTTER::createSVGFiles`, `plotOneSheetSVG`, `SCHEMATIC::GetUniqueFilenameForCurrentSheet` <-> `sch_plot::plot_schematic` (`PlotFormat::Svg`) |
| PDF: one multi-page file, one page per sheet, page bookmarks nested by hierarchy | identical | `SCH_PLOTTER::createPDFFile`, `plotOneSheetPDF`, `setupPlotPagePDF`, `PDF_PLOTTER::StartPage`/`ClosePage` outline tree <-> `sch_plot::plot_schematic` (`PlotFormat::Pdf`), `plotter/pdf.rs` |
| Page size Auto / A4 / A and the `min( scalex, scaley )` viewport scale | identical (the IR has one paper size, A4, so "Auto" = A4) | `plotOneSheetSVG`/`setupPlotPagePDF` <-> `sch_plot::plot_page` |
| Background rect, black-and-white colour rule (`PSLIKE_PLOTTER::SetColor`: black unless white) | identical | `plotOneSheetSVG` <-> `plot_one_sheet`, `SvgPlotter::set_color`/`PdfPlotter::set_color` |
| Drawing sheet (frame, zone grid, title block) | identical for the default sheet; no custom `.kicad_wks` | `PlotDrawingSheet` (`common_plot_functions.cpp`), `DS_DATA_ITEM::SyncDrawItems`, `defaultDrawingSheet` <-> `sch_plot::plot_drawing_sheet` -- the real default description is embedded verbatim and parsed with `sexpr.rs`; corners, `repeat`/`incrx`/`incry`, label increment, in-page clipping and `${TITLE}`/`${REVISION}`/`${ISSUE_DATE}`/`${COMMENTn}`/`${SHEETPATH}`/`${#}`/`${##}` follow source |
| Stroke-font text (justification, rotation, `~{overbar}`/`_{sub}`/`^{super}`, italic) | identical layout, vector output | `FONT::getLinePositions`, `drawMarkup`, `STROKE_FONT::GetTextAsGlyphs` <-> `plotter::stroke_text_segments` over `eda_drc::stroke_font::glyph_strokes`. SVG also writes the invisible `<text>` + `<g class="stroked-text"><desc>` pair as `SVG_PLOTTER::Text` does. PDF writes strokes, not `PDF_PLOTTER`'s Type3 font text, so PDF text is not selectable/searchable |
| Item plot order: background pass, then SHEET, SYMBOL, labels, LINE, bus entries, no-connects, text, junctions last | identical, except the "overlapping symbols re-plot their fields on top" pass | `SCH_SCREEN::Plot` <-> `sch_plot::plot_screen` |
| Wires / buses / bus entries / no-connect / junction dots | identical geometry, widths (6/12 mil), colours | `SCH_LINE::Plot`, `SCH_BUS_ENTRY_BASE::Plot`, `SCH_NO_CONNECT::Plot`, `SCH_JUNCTION::Plot` <-> `plot_wire`/`plot_bus_entry`/`plot_no_connect`/`plot_junction`. The IR stores no junctions, so the dots use the canvas' own rule (`components/schematic/junctions.ts` ported to `junction_points`) at KiCad's default 6x wire-width diameter |
| Symbol graphics (rectangle, polyline, circle, arc, text) | partial | `LIB_SYMBOL::Plot`, `SCH_SHAPE::Plot` <-> `plot_graphic`. `SymbolGraphic::filled` is one bool, so every fill is `FILLED_SHAPE` (outline colour); KiCad's `FILLED_WITH_BG_BODYCOLOR` yellow body fill is not representable in the IR |
| Pins: line, inverted/clock/low/non-logic decorations, NC cross, names inside / numbers above | identical shapes; one text size | `SCH_PIN::PlotPinType`/`PlotPinTexts` <-> `plot_pin_type`/`plot_pin_texts`. `LibPin` has no name/number size or hide flag (50 mil, power-symbol pins hidden) and stacked-pin number formatting is not ported |
| Reference / Value fields | partial | `SCH_FIELD::Plot` <-> `plot_symbol`. The IR carries no per-field position/size/visibility: Reference sits above the body, Value below, centred, 50 mil (what the canvas does); Footprint/Datasheet are never drawn |
| Power symbols | partial | `SCH_SYMBOL::Plot` of a power symbol <-> `plot_power_symbol`; the Value text goes on the side the body extends to |
| Labels (local / global / hierarchical) | partial | `SCH_LABEL_BASE::Plot` <-> `plot_label`. Text offset (`GetSchematicTextOffset`) and colours follow source; spin is inferred from the attached wire (the IR stores none); global/hierarchical flags are a simplified outline, not `CreateGraphicShape` |
| Free text, hierarchical sheet box + name/file fields + pin names | partial | `SCH_TEXT::Plot`, `SCH_SHEET::Plot` (background + border, quirk included) <-> `plot_free_text`, `plot_sheet`; sheet pins draw their name, not the shape glyph |
| DNP cross-out, local-power icon, text boxes / tables / images, rule areas, variants, hop-over, per-symbol bookmarks, hyperlinks | missing | `SCH_SYMBOL::PlotDNP`/`PlotLocalPowerIconShape`, `SCH_TEXTBOX`/`SCH_TABLE`/`SCH_BITMAP::Plot`, `PDF_PLOTTER::Bookmark`/`HyperlinkBox`/`HyperlinkMenu` |
| Netlist: KiCad `.net` s-expression (`export (version "E")`) | identical structure | `NETLIST_EXPORTER_KICAD::Format`, `XNODE::Format`, `KICAD_FORMAT::Prettify` <-> `netlist::export_netlist` (`NetlistFormat::Kicad`), `XNode::format`, `netlist::prettify` (tab-indented like the real file) |
| Netlist: generic `.xml` | identical structure | `NETLIST_EXPORTER_XML::WriteNetlist` (`wxXmlDocument::Save`) <-> `NetlistFormat::Xml`, `XNode::write_xml` |
| Sections: `design` (+ one `sheet`/`title_block` per hierarchy sheet), `components`, `groups`+`variants` (`.net` only), `libparts`, `libraries`, `nets` | identical order; groups/variants empty | `makeRoot`, `makeDesignHeader`, `makeSymbols`, `makeGroups`, `makeVariants`, `makeLibParts`, `makeLibraries`, `makeListOfNets` <-> `netlist.rs`. `libraries` lists a library only when the installed `.kicad_sym` is found (`GetFullURI`) |
| Component ordering, `#` references skipped, multi-unit reference emitted once, `units`/`tstamps`/`sheetpath`/`libsource`/`fields`/`property` | identical | `makeSymbols`, `addSymbolFields`, `NETLIST_EXPORTER_BASE::findNextSymbol` <-> `make_symbols`. The IR has no per-symbol UUID: `tstamps` use the same `duid("sym:<ref>")` the `.kicad_sch` export writes; `MPN`/`LCSC` are emitted as user fields; library `in_bom`/`on_board` give `exclude_from_bom`/`exclude_from_board` (and `.net` skips off-board symbols as `GNL_OPT_KICAD` does) |
| Net order and codes, node order, duplicate removal, `pinfunction`, `pintype`, `+no_connect`, net class | identical | `makeListOfNets` <-> `make_list_of_nets`: nets sorted by `StrNumCmp(name)`, `code = index + 1` over the sorted list (nets with only `#` pins are skipped and leave a gap, as in source), nodes by ref then pin number, duplicates dropped, `pinfunction = <name>_<number>` only for named pins, `class` = the board's net class (else `Default`) |
| Pins no net names | identical naming | `SCH_PIN::GetDefaultNetName` <-> `default_net_name`: `Net-(R1-Pad2)` / `unconnected-(U1-NC-Pad5)` single-node nets |
| Spice / Cadstar / OrcadPCB2 / Allegro / PADS exporters, netlist plugins/commands, component classes, text variables, variants | missing | `netlist_exporter_spice.cpp`, `_cadstar.cpp`, `_orcadpcb2.cpp`, `_allegro.cpp`, `_pads.cpp` |

## 11. Hotkeyed-and-missing sweep: eeschema (`docs/parity/UI-ACTIONS.md`)

Eleven of the 21 hotkeyed-but-unwired eeschema actions are now ported from the KiCad source (`eeschema/`, commit 8303b2ad); the other ten are recorded
with a reason in `tools/ui-parity-missing.json` (no handler stands in for them). Each ported action is one `/api/cmd` verb with undo.
IR additions are additive with serde defaults: `SchematicSection.junctions` (`Junction { id, at }`) and `.lines` (`SchLine { id, pts, width_um }`).
Verified in the browser pane on a scratch board (Playwright is not installed): J, I, S, C, Alt+S, F1, Tab/Shift+Tab and the three Symbol Editor actions;
`kicad-cli sch erc` loads the derived `.kicad_sch` with the explicit junction, polyline, bus entry and reports the two nets as one (`multiple_net_names`).

| Action | Status | KiCad file:function |
|---|---|---|
| `J` Place Junction | identical gate and effect: the click snaps (12 px) to a wire vertex, pin or crossing, `isExplicitJunctionAllowed` refuses a point where fewer than three directions meet (toast "Junction location contains no joinable wires and/or pins."), a click on an existing junction does nothing, the tool stays armed. `reconcile` seeds every junction as a union point, so two wires that merely cross now join into one net; `export_kicad_sch` writes `(junction (at) (diameter 0) (color 0 0 0 0))`, `import_kicad_sch` reads it back. A junction is selectable by a plain click and `Del`-able (`delete_junction`). The computed T-dots of section 2 are unchanged | `sch_drawing_tools.cpp::SingleClickPlace`, `junction_helpers.cpp::AnalyzePoint` (`kicad-port/schJunction.ts`); `Cmd::AddJunction`/`DeleteJunction` |
| `I` Draw Lines | identical in effect: a graphic polyline on the notes layer, the same click-to-add-point state machine as `W` with the line-mode/posture keys, but no pin snap and no auto-finish on a pin (`GRID_GRAPHICS`, not `GRID_WIRES`). A double-click finishes; zero-length segments are dropped (`finishSegments`), in the UI and again in `add_sch_line`. A line carries no net, is never part of ERC, is selectable by a plain click and `Del`-able, and round-trips through `(polyline)` in the `.kicad_sch`. Width is the default (0) only: no stroke dialog, no dashed/dotted types, no arrowheads | `sch_line_wire_bus_tool.cpp::DrawSegments(LAYER_NOTES)`; `Cmd::AddSchLine`/`DeleteSchLine` |
| `S` Draw Hierarchical Sheet | partial: first click = top-left corner, the cursor sizes it (`sizeSheet`: at least 500 x 150 mil, far corner grid-snapped, a rubber-band outline follows the cursor), the second click opens `SheetDialog` (name, file) -- "Untitled Sheet" / `untitled.kicad_sch` by default, refused for a name already in use or a file with a folder in it. A new file gets a new empty screen, an existing one is shared (`sheet_contents`); the Hierarchy panel lists it at once. Not ported: sheet pins, the sheet's border/fill colors, extra fields, "Import sheet pin" | `sch_drawing_tools.cpp::DrawSheet`/`sizeSheet`, `EditSheetProperties` (`kicad-port/schSheet.ts`); `Cmd::AddSheet` |
| `C` Unfold from Bus | partial (popup menu -> modal list): with the cursor on a bus, `BusUnfoldDialog` lists the bus's member nets (`D[0..3]` -> D0..D3, `BUS_UNFOLD_MENU`). The entry roots at the bus point nearest the grid-snapped cursor (`doUnfoldBus`, so an on-grid bus gets an on-grid entry), the wire tool starts at the entry's far end, and finishing the wire commits entry + wire + the member's label at the wire's end as ONE batch (one undo step, like the C++'s single commit). Escape before the wire is drawn leaves nothing behind. Not ported: the label's persisted spin style | `sch_line_wire_bus_tool.cpp::UnfoldBus`/`doUnfoldBus`/`getBusForUnfolding` (`kicad-port/schBusUnfold.ts`); batch of `Cmd::AddBusEntry`/`AddWire`/`AddLabel` |
| Alt+S Swap | identical for what the IR positions: symbols, power symbols, labels and free texts. The selection is walked in selection order and each neighbour pair swaps positions (`sorted[i]` with `sorted[i + 1]`, so three items rotate), and two symbols of the SAME library symbol also swap orientation. One batch = one undo step. Limits: only symbols are click-selectable (box-select and Select All reach the rest), so the common use is two symbols; sheets and bus entries are not swapped | `sch_edit_tool.cpp::Swap`; `Cmd::SwapSchItems` |
| F1 (Insert off macOS) Repeat Last Item | partial: the placements the studio runs (`add_label`, text, power symbol, no-connect, bus entry, wire, line, symbol -- also a whole batch such as an unfold) are remembered; each press places another copy shifted by the default repeat offset (0, 100 mil), a label's text incremented (`D0` -> `D1`, `IncrementString`), a symbol cloned AT THE CURSOR with the next free reference. The copies become the new source, so presses chain (N1, N2, N3). Not repeatable (as in source): junctions, deletions, edits. The extractor kept only the macOS key (F1, which is zoomIn elsewhere); Insert is added in `useGlobalHotkeys.ts` for the others | `sch_edit_tool.cpp::RepeatDrawItem`, `SCH_EDIT_FRAME::SaveCopyForRepeatItem`, `common/increment.cpp` (`kicad-port/schRepeat.ts`, `incrementLabelText.ts`) |
| Tab / Shift+Tab Next / Previous Net Item | identical on one sheet: with a net highlighted and one of its navigator items selected (a symbol's pin, a label, a power symbol, a no-connect), Tab selects the next item of the net and Shift+Tab the previous, wrapping. Items are ordered by their navigator text (`Label 'HX' at (50.8 mm, 5.08 mm)`, plain string order); wires, buses, junctions and bus entries are not in the navigator, so Tab with a wire selected does nothing (as in source). The sheet nodes of a hierarchy are not walked | `net_navigator.cpp::SelectNextPrevNetNavigatorItem`, `MakeNetNavigatorNode`, `GetNetNavigatorItemText` (`kicad-port/netNavigator.ts`) |
| Autoplace Fields (`autoplaceFields`) | missing: needs per-instance field positions | the IR stores no field placement (reference/value draw at fixed offsets), so there is nothing to autoplace; `tools/ui-parity-missing.json` |
| Import Graphics, Place Design Block | missing: need a DXF/SVG importer / a design-block library | none exists and no new dependencies are allowed; `tools/ui-parity-missing.json` |
| Simulation: new analysis tab, open/save workbook (+ as), probe, tune, run | missing: needs ngspice | kicad-cli has no simulator; `tools/ui-parity-missing.json` (7 actions) |

Found and fixed while verifying: a pre-existing 400 px (not µm) wire-hit radius made a click anywhere near a junction select the junction; the junction and
line hit tests now use a tight screen-sized radius. `GET /api/symbol?lib_id=Device%3AR` never matched (the server did not percent-decode the query), so the Symbol
Editor could not show any `Lib:Name` symbol; `studio.rs::query_value` decodes it now (test `a_lib_id_query_value_is_percent_decoded`).

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
19. Select a symbol with real `lib_symbols` graphics (not a generic box),
    press `X` -- confirm it flips left-right, as before. Press `X` again
    to undo it, then press `Y` -- confirm it flips top-bottom instead
    (not left-right). With `Y` still active, press `X` -- confirm it
    switches to the `X` flip cleanly (not both at once, not a no-op).
    After a `Y` flip, draw a wire (`W`) to one of the now-vertically-
    mirrored symbol's pins and confirm it snaps exactly onto the pin's
    *visually drawn* position, not where it would be pre-mirror -- this
    is the real correctness check (pin-snap and the renderer must agree
    on where a Y-mirrored pin actually is).
20. Open the ERC dialog on a board with at least one `pin_not_connected`
    finding (any symbol with an unwired pin). Confirm a red circle marker
    with a "!" is drawn on the schematic canvas right at that pin, even
    while the dialog covers part of the view. Click the finding's row --
    confirm the canvas re-frames tightly around that exact pin (not just
    "somewhere on screen"), and the marker's own circle briefly shows the
    highlighted color while that row is selected. Click "Exclude" on the
    row -- confirm the row dims, moves under the "Exclusions" count, and
    the canvas marker turns gray immediately (no page reload needed --
    this rides the same version-poll `refreshErc` every other live update
    does). Reopen the dialog later (or toggle the "Exclusions" filter
    off/on) -- the exclusion should still be there (it's in
    `design.schematic.erc_exclusions`, not component state). Click
    "Un-exclude" -- confirm it goes back to reporting as a real error/
    warning, marker back to red/yellow. Also try a `pin_to_pin` or
    `wire_dangling` finding if the test board has one, to exercise the
    "NET:REF.PIN"/bare-net-name location shapes (not just plain
    "REF.PIN") -- `ercMarkerPosition.ts`'s own unit tests cover every
    shape in isolation, but only a real board proves `check_erc` actually
    emits the shapes that module expects.
21. Draw two wires that form a plain "T" -- one wire, then a second
    starting from a point partway along the first (not at either of its
    ends) and heading off in another direction. Confirm a junction dot
    now appears right at that T point (it did not before this session,
    even though `GET /api/schematic`'s `nets` already correctly merged
    the two wires onto one net -- this was a draw-only bug). Then drag a
    box around several wires and a symbol together (left-to-right for
    enclosed, right-to-left for crossing, same as symbols already work):
    confirm the wires highlight (the selection color, same as a net
    highlight) along with the symbol, and confirm `Del` removes all of
    them in one press. Separately, place a power symbol (`P`, e.g. GND)
    and/or a label (`L`) by clicking directly on top of an existing wire
    rather than at either of its ends -- confirm a junction dot appears
    there too. Finally, confirm two wires that simply cross in their
    interiors with neither ending on the other (an X, not a T) still show
    no dot and are not merged onto one net -- this is correct,
    unchanged, real-KiCad-matching behavior, not something this session's
    fix should have altered.
22. Symbol Fields Table (Tools > Bulk Edit Symbol Fields...) on a board
    with several same-valued resistors (e.g. R1, R2, R3, R5 all `10k` with
    the same footprint) and a few other parts. Confirm the grid opens with
    "Group symbols" on and one row `R1-R3, R5` (Qty 4) -- and that a run of
    exactly two reads `R1, R2`. Click the `>` to expand the group into its
    four child rows. Type a new Value into the group row's Value cell and
    press Enter: the grid should regroup immediately (still unsaved -- the
    Apply button is now enabled and the schematic canvas behind has NOT
    changed). Uncheck "Group" on Value/Footprint to see one row per symbol.
    Type "MPN" into "New field name" and click Add Field, fill a cell, then
    Apply: confirm the toast, that the canvas/E-dialog shows the new Value,
    and that ONE Ctrl+Z (Schematic tab) reverts the whole batch (all cell
    edits and the added MPN column). Rename and Del a user column (Ren /
    Del buttons), Apply, and confirm the values moved/vanished. Try to edit
    a Reference or Qty cell (should be read-only). Close with staged edits
    and confirm the "Discard?" prompt.
23. Same dialog, Export tab: pick CSV, then TSV, then Semicolons and watch
    the preview change (CSV quotes every field with `"`, references joined
    with `,`; TSV has no quotes). Put `-` in "Ref range delimiter" and
    confirm `R1-R3,R5`. Turn off a column's Show checkbox on the Edit tab
    and confirm it disappears from the preview. Click Export with the
    default path and confirm `<board dir>/export/<intent>-bom.csv` exists
    and matches the preview; type `../x.csv` and confirm it is refused.
24. Press `Ctrl+F` on the Schematic tab, search `10k`: Find Next should
    select + re-center on each matching symbol in left-to-right order, and
    after the last one say "Reached end of schematic. Find again to wrap
    around to the start." (the next press wraps). `Shift+F3` goes backward
    and `F3` works with the dialog closed. Search a footprint name: no
    result until "Search all fields (incl. hidden)" is ticked. Check Whole
    word (`R1` must not hit `R10`) and Wildcards (`R*`, `?1`). Open
    `Ctrl+Alt+F`, search `10k` replace `47k`, click Replace (replaces the
    current match then jumps to the next), then Replace All; confirm the
    Values changed and that Ctrl+Z reverts the Replace All in one step.
    Replace `R1` -> `R9` with "Replace in reference designators" off (nothing
    happens) and on (symbol renamed, wires stay attached).
25. File > Schematic Setup...: confirm the 11-row triangle with labelled
    rows/columns and that Output/Output starts as a red `x`. Click it once
    (-> green check), run ERC on a net joining two output pins: the
    `pin_to_pin` finding should be gone; click again (amber `!`: warning),
    again (red: error). Confirm clicking Input/Output mirrors nothing odd
    (the grid shows only one cell per pair; ERC applies it both ways).
    "Reset to Defaults" restores the original and is disabled when already
    default; Ctrl+Z on the Schematic tab undoes a cell click. Reload the page:
    the customized map persists (`design.json` -> `schematic.erc_pin_map`).
26. Plot and netlist export (section 10). Open the Schematic tab on a board
    with a few symbols, wires and a power symbol. File > Plot...: confirm
    the dialog opens (and that on the PCB tab the same menu item still
    opens the Gerber Plot dialog). Choose SVG + Color + Plot drawing sheet
    and click Plot: the result box should list `export/<board>.svg`; open
    that file in a browser -- the A4 frame with zone letters/numbers and
    the title block (Title/Rev/Date/Sheet/File/Id) should be there, wires
    green, symbol bodies/pins dark red, pin names/numbers and
    Reference/Value text legible, junction dots at T points. Re-plot as
    Black and white and confirm everything is black, with no page-colour
    fill; untick "Plot drawing sheet" and confirm the frame is gone. Choose
    PDF: `export/<board>.pdf` should open in a PDF viewer with one page per
    sheet (check a hierarchical board: one page per sheet, and the
    viewer's bookmark panel shows the sheet tree). On a hierarchical board
    choose "Plot current page only" while viewing a child sheet and
    confirm only `<board>-<sheet>.svg` is written. File > Export >
    Netlist...: export KiCad and then XML and open the files -- the `.net`
    should start `(export` / `(version "E")`, list `components` sorted by
    reference and `nets` sorted by name numbered from 1; import it in
    KiCad's PCB editor (File > Import > Netlist) to confirm it is
    accepted. Confirm no undo entry is created by either export (Ctrl+Z
    after exporting reverts your previous *edit*, not the export), and
    that editing the schematic (e.g. a label that merges two nets) and
    re-exporting changes the netlist accordingly.
    Not run this session: the frontend was verified by `tsc --noEmit`
    (no errors in the touched files; react types are not installed so the
    rest is filtered noise) and `schOutputs.test.ts` only -- the dialogs
    themselves were never rendered in a browser.
