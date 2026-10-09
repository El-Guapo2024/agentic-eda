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
applied to the ERC report (since 2026-10-03 kicad-cli's, see section 4) and
surfaced as a third `"excluded"` `ErcSeverity`), and item 8 -- wire box-select plus one real
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

**Status (2026-10-07):** of the items this paragraph lists as not wired, `J`, bus wires, bus entries and bus unfolding are wired
since (section 11: `J` and `C`; `Wire::bus` via `Cmd::AddWire`; `Cmd::AddBusEntry`). The overlap-trim and collinear-simplify
functions are still not ported.

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

- `crates/cli/src/board.rs::reconcile_schematic` runs after every
  `Domain::Schematic` command (`Cmd::domain`): it traces wire / label /
  power-symbol / sheet-pin / no-connect geometry of the whole sheet tree
  (`crates/engine/src/nets.rs`, a port of `connection_graph.cpp`, section
  14) and writes what the drawing says back into `design.schematic`'s own
  `Wire::net`/`pins`, `PowerSymbol::pin`, `NoConnect::pin` fields. The
  net list, `Design::nets`, is not re-read from the drawing: the drawing
  is traced before and after the command and only the difference is
  applied to the list that stands (`nets::follow`, section 14), so a net
  an edit did not touch keeps its name and pins even when the drawing
  shows it in pieces. (It used to retrace everything, which on an older
  drawing turned one drag into 74 loose pins called `NET_12`...)
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
| Box select: wires (crossing = whole wire if touched at all; enclosed = both endpoints, or one if it's dangling) | partial — a touched/enclosed wire's *whole* polyline is selected as one atomic unit (bounds test over every point, not just its own two ends — see `boxSelection.test.ts`'s multi-bend test); source's own finer per-`SCH_LINE`-segment granularity (a box can touch just one bend of a multi-segment wire and select only that piece) has no equivalent here since this app's `Wire.pts` is already one polyline per wire, not source's many atomic 2-point segments. A box-selected wire moves, drags, turns and mirrors with the rest (`Cmd::SchMove`, below) | `sch_selection_tool.cpp::SelectMultiple` lines ~2640-2666; `components/schematic/boxSelection.ts::collectBoxSelection` |
| Pins/junctions win ties over symbol body/wire at an exact hit | missing (no finer-than-symbol hit test yet) | `sch_selection_tool.cpp::GuessSelectionCandidates`/`narrowSelection` |
| `M`: move (every item kind moves rigidly; the wires on a moved pin are left where they are) | identical for every kind this IR stores: symbols, power symbols, wires, labels (local, global, hierarchical), free text, text boxes, shapes, rule areas, directive labels, junctions, no-connects, bus entries, graphic lines and sheets (a sheet takes its pins). One verb for all of them, `Cmd::SchMove` (`{ op: "sch_move", verb: "move" }`), one undo step on the sheet in view; the nets never change when the items move with the wires that join them (`sch_move_tests.rs`, `crates/cli/src/board.rs` `every_schematic_move_verb_is_one_undo_step_for_every_kind_of_item`). While an item is held the view shows the result the server computed for the same command (`POST /api/sch/move_preview`, `kicad-port/schMove.ts::applyPatch`), so what is drawn is what the drop commits. Not ported: the per-event incremental move (one move by the final offset is applied; the result is the same), `AutoRotateItem` after a label lands | `sch_move_tool.cpp::Main` / `performItemMove` / `moveItem` (`crates/ops/src/sch_move.rs`, `sch_drag.rs`, `sch_scene.rs`; `SchematicView.tsx`'s `moveMode` branch) |
| `G`: drag (attached wire endpoints rubber-band) | identical for every kind: the wires, labels, junctions, no-connects and bus entries attached to a dragged item stretch or follow with it, and the segments KiCad adds at a bend or a junction it does not move are added (right-angle bends with Ortho, the stub at an unselected 3-way junction, the special cases for a label or sheet pin sitting on a wire end). A wire is one polyline here and is split into KiCad's segments while it is dragged and joined again where they still form a chain; the finishing steps are KiCad's (junctions at the points that need one, overlapping wires trimmed and merged, dangling segments removed) | `sch_move_tool.cpp::getConnectedDragItems` / `getConnectedItems` / `orthoLineDrag` / `finalize`; `JUNCTION_HELPERS::AnalyzePoint`, `SCH_LINE::MergeOverlap`, `SCHEMATIC::CleanUp` (`crates/ops/src/sch_drag.rs`). Difference: KiCad's attachment cache also counts the dragged item at the end that moves, which can leave a short stub behind when the far end of a wire touches nothing; this port counts only what is attached at the end that stays, so such a wire moves as a whole |
| New stub wire at an unselected 3-way junction (source's own fallback when a junction's *other* wires are deliberately not moving) | identical | `ptHasUnselectedJunction` in `getConnectedDragItems` (`sch_drag.rs::get_connected_drag_items`); a selected wire can now be the thing dragged, so the case exists (`sch_move_tests.rs`) |
| Grid snap during move | identical (grid only) | `edit_tool_move_fct.cpp` (`kicad-port/gridSnap.ts::alignToGrid`, reused) |
| Anchor/pin snap during move | missing | `ee_grid_helper.cpp` — per-item-category grids + pin-anchor snap not ported; PCB side has the analogous gap too (`PARITY-pcb.md`) |
| Escape cancels an in-progress move without touching the prior selection | identical | shared `ESCAPE` reducer case (already generic across tabs) |
| `R`: rotate CCW | identical for every item kind | `sch_edit_tool.cpp::Rotate` (confirmed default is CCW, not CW): one verb for the selection (`SchMoveCmd::Rotate`). A single item turns about its own anchor (a symbol its origin, a sheet its corner, a text its position), a selection about the centre of its principal items snapped to the half grid (`GetNearestHalfGridPosition`; text and labels do not count toward the centre); a label's spin turns Right, Up, Left, Bottom (`SCH_LABEL_BASE::Rotate90`, kept in `SchExtras::label_spins` and drawn) |
| Shift+`R`: rotate CW | identical | same function, `rotateCW` action |
| R/Shift+R during an active move updates the live preview instead of committing separately | identical | `sch_move_tool.cpp::handleMoveToolActions` (`tryTransformDuringMove`, shared helper, now tab-aware) |
| Rotate pivot: own anchor (single item) / collective bbox center (multi-select) | identical (**closed 2026-10-08, `docs/parity/GAPS.md` item 1**): every selected item of every kind turns about the same point, as one undo step. With a wire or a sheet in the selection the pins and wires turn with it; a mirror is the same with the axis through that point | `sch_edit_tool.cpp::Rotate` / `Mirror` (`SELECTION::GetCenter`, `GetNearestHalfGridPosition`) |
| `X`: mirror horizontally (negate-X, KiCad's `SYM_MIRROR_Y`) | identical for every item kind: a symbol toggles `SymbolInstance::mirrored`, other items mirror their position and geometry about the selection's centre line; a label's spin mirrors (`MirrorSpinStyle`: Right/Left swap). A text keeps its angle: this IR has no justification to flip | `sch_edit_tool.cpp::Mirror` (`mirrorH`); `Cmd::SchMove` `Mirror`; the symbol orientation rules below are unchanged |
| `Y`: mirror vertically (`SYM_MIRROR_X`) | identical | `sch_edit_tool.cpp::Mirror` (`mirrorV`); new `SymbolInstance::mirror_y` + `Cmd::MirrorSymbolVertical`, mutually exclusive with `mirrored`/`MirrorSymbol` (toggling one clears the other, same as a real `SetOrientation` call replaces the whole orientation -- KiCad's own symbols never carry both at once). `transform_local_point` (reconcile's/import's pin-world resolution) extended to the second axis and cross-checked numerically against `transform.ts`'s own matrix table (already correct on the frontend, which was built two-axis-ready ahead of this field existing -- see its own "Found while wiring..." note two rows down) rather than re-derived from a description: `mirror_y` *cancels* the always-applied library-Y-up/sheet-Y-down flip instead of negating local X the way `mirrored` does, confirmed by a unit test matching all 4 rotations against the frontend's matrices, plus a real `.kicad_sch`-level round-trip test (`(mirror x)`/`(mirror y)` tags reading back to the correctly-opposite-named field -- `sch_io_kicad_sexpr_parser.cpp`'s own token names don't match this app's own field names 1:1, an easy, wrong-rendering mistake to make without checking source directly). `export_kicad_sch`'s own generic-box baking (`baked_local`, used only for a *synthesized*, non-library-resolved symbol) is **not** extended to the new axis -- a narrower, documented gap, since that function's local-box mirror math is a different convention than `transform_local_point`'s and real/library-resolved symbols (the common case) are unaffected |
| **Found while wiring this**: `api/types.ts`'s `SchematicSymbol.mirror: "x"\|"y"\|null` and the renderer that reads it (`transform.ts`, `libSymbol.ts`) were already fully two-axis, written *ahead of* this backend field by the session that merged the Eeschema-port itself -- the backend was only ever sending the single legacy `mirrored` boolean, so `X` mirroring only ever rendered at all because `api/client.ts`'s `fetchSchematic` already had a same-session compatibility shim (`sym.mirror ?? (legacy.mirrored ? "y" : null)`). `schematic_json` now sends a real `"mirror": "x"\|"y"\|null` field (plus `"mirrored"`, redundant but harmless, for that same shim), so the fallback is now dead code but was never a bug to fix -- no frontend rendering changes were needed for either axis | n/a | n/a |
| Mirror (X, Y) during an active move | identical: the turn is held with the items and committed with the drop, about the point they are held at (`SchMoveCmd::Move` / `Drag` carry `turns` and `about`) | `sch_edit_tool.cpp::Mirror`'s `IsMoving()` branch; `kicad-port/schMove.ts::heldCmd` |
| `Del`: delete symbol, wires left dangling (no cascade) | identical | `sch_edit_tool.cpp::DoDelete` (confirmed: wires are never auto-deleted); `Cmd::DeleteSymbol`, `common.Interactive.delete`'s schematic branch |
| `Del`: delete wire | identical (wire must already be selected -- a modified click, or now a box-select, item 8) | `Cmd::DeleteWire` |
| `E` Properties (and a double-click) | partial: the first selected item decides, as in source, and the dialog is the one for its kind. **Symbol**: Reference/Value/Footprint/Datasheet (no unit/DeMorgan/pin-table editing). **Label** (local, global, hierarchical): text, shape (global and hierarchical), orientation. **Text**: text, size (0.01 to 1000 mm), horizontal or vertical. **Sheet**: name, file (a file the project has is linked to; a new name moves the content or copies it when another sheet shows the old file; a sheet cannot show a file that shows it), page number. **Wires, buses, bus entries, graphic lines, junctions** (together only as KiCad allows): width, line style, colour, junction size; fields the items disagree on show "(mixed)" and are sent only when touched, and "Default" resets them. **Shapes and rule areas**: border width, style, colour, fill, the rule area's exclusion flags. **Text box** and **directive label**: the dialogs that drew them, filled in. An OK that changes nothing sends nothing; one that changes something is one undo step. Not in the dialogs because the IR has no place for them: fonts, bold/italic and colour of a text or label, a sheet's border and fill, extra fields, a label's fields (netclass, intersheet references), a power symbol's Value (no dialog for power symbols yet), sheet pins (not selectable one by one; `edit_sheet_pin` and the Sync Sheet Pins dialog edit them) | `sch_edit_tool.cpp::Properties` / `EditProperties`; `dialog_label_properties.cpp`, `dialog_text_properties.cpp`, `dialog_sheet_properties.cpp`, `dialog_wire_bus_properties.cpp`, `dialog_junction_props.cpp`, `dialog_shape_properties.cpp` (`kicad-port/schProperties.ts`, `components/SchPropertiesDialogs.tsx`; verbs `SchCmd::EditLabel`, `EditText`, `EditSheet`, `SetStroke`, `EditGraphic`, `crates/ops/src/sch_props.rs`). The width, style and colour of a wire, bus, bus entry or graphic line, and the size and colour of a junction, are drawn and written to the `.kicad_sch` as KiCad writes them (`(stroke ...)`, `(junction ... (diameter) (color))`) and read back on import |
| `U`/`V`/`F`: quick-edit Reference/Value/Footprint | partial — same dialog as `E`, just autofocused on the one field (source uses a separate, smaller single-field `DIALOG_FIELD_PROPERTIES` for these three; one shared component here, deliberately, since both ends run the same two Cmds either way) | `sch_edit_tool.cpp::EditField`/`sch_actions.cpp` (confirmed `U` = reference, not "unit" — a wrong guess here would have mis-bound a hotkey) |
| `U`'s own rename: cascades every `"REF.PIN"` string this sheet's wires/power-symbols/no-connects hold | partial — refuses a blank or already-used new id; a wire/power-symbol/no-connect whose pin happens to sit exactly on a real resolvable pin gets its reference re-derived for free by the next `reconcile_schematic` pass (same geometric mechanism that already runs after every schematic `Cmd`, confirmed by a dedicated test with real connected wire geometry) -- the explicit string-cascade in `rename_symbol` only matters for the degenerate case where a power-symbol/no-connect's own position doesn't resolve to a real pin at all, a safety net, not the primary mechanism. Deliberately does **not** retarget `design.placement`'s `FootprintInstance` (no rename concept on the PCB side at all) or intent.yaml (read-only to every verb in this file) -- the old reference's PCB footprint, if any, is left exactly where it was, now matching no schematic symbol; the next reconcile pass synthesizes a fresh, unplaced `Part` for the new reference, the same mechanism `AddSymbol` already uses for a part with no intent counterpart. A real "rename and keep the PCB placement" is future work | new `Cmd::RenameSymbol`, `crates/ops` |
| `E`'s Value/Footprint/Datasheet edits | identical for the Schematic tab's own display (the instance's own copy always wins when set, same precedent `SymbolInstance::value`'s doc already established); does not update `ConstraintModel::Part` (not persisted across requests anyway) | new `Cmd::EditSymbolFields`, `crates/ops` |
| A plain click-and-drag directly on a symbol (no `M`/`G` needed first) | identical in spirit -- defaults to Drag's rubber-banding, matching source's own default for a plain drag (`sch_selection_tool.cpp Main()`'s `IsDrag(BUT_LEFT)` handler: `SCH_ACTIONS::drag` unless the `drag_is_move` preference is set, which this app has no settings UI for); a click on a member of a larger selection keeps the whole group, same `RequestSelection`/`selectionContains` reasoning | `sch_selection_tool.cpp` Main() (`SchematicView.tsx`'s new `DragState` "move" kind, ported from Canvas.tsx's identical PCB-side gesture) |
| Click-vs-drag threshold (8px/300ms) | partial -- same approximation the PCB side already shipped (Canvas.tsx): "moved" is true once the *snapped* position first changes, not a literal pixel/time timer, so a release before the grid (or a nearby pin) actually moves is still a plain click. Good enough that a modifier-click is unaffected (that path never arms a drag at all) | `common/tool/tool_dispatcher.cpp` (`DragDistanceThreshold`/`DragTimeThreshold`) -- same documented simplification as PARITY-pcb.md's own row for this |
| A plain click on a wire selects it (net highlight is its own action) | identical: a click selects the wire, a click near one of its ends picks that end (`STARTPOINT` / `ENDPOINT`), anywhere else on a segment the two ends of that segment; `G` or a click-drag then stretches that end, or drags that segment with its neighbours following. Highlight Net stays the `Highlight Net` action (and Ctrl+click) | `sch_selection_tool.cpp::selectPoint` and `Main` (`kicad-port/schMove.ts::wirePick`, `SchematicView.tsx`) |
| Click-drag any item (not only a symbol) | identical: a label, text, power symbol, junction, no-connect, bus entry, graphic line, shape, text box, sheet or wire is picked up and dragged like a symbol; a click on a member of a larger selection keeps the whole selection | `sch_selection_tool.cpp::Main`'s `IsDrag` branch |
| Align Left / Right / Top / Bottom / Center and Align Items to Grid | identical: each item moves by its own offset (snapped to the connection grid) with its wires, so pins keep landing on the grid and keep connecting. An item whose connection points sit mostly off the grid is aligned by where most of them are | `sch_align_tool.cpp` (`selectTarget`, `AlignSchematicItemsToGrid`; `kicad-port/schAlign.ts`, `SchMoveCmd::Align` / `AlignToGrid`) |
| Net collision during a drag (the cursor and markers when a dragged wire would join another net) | missing: the drag shows the wires as they will be and the nets are re-read after the drop; the monitor's overlay is not ported | `sch_drag_net_collision.cpp` |

## 2. Wires, junctions, no-connects (`sch_line_wire_bus_tool.cpp`)

| Action | Status | KiCad file:function |
|---|---|---|
| `W`: draw wire, click to start/bend, snaps to a pin when close | partial — free-angle only, no 90°/45° auto-posture (every bend is wherever you click, not auto-right-angled); pin-snap only works for a symbol with real `lib_symbols` graphics resolved (see `pinSnapPoints`'s own doc for the generic-box-fallback gap) | `sch_line_wire_bus_tool.cpp::doDrawSegments`/`startSegments`/`finishSegments`; `SchematicView.tsx`'s wire-tool branch of `onPointerDown`, `Cmd::AddWire` |
| Shift+Space: cycle Free/90°/45° posture | missing. **Status (2026-10-07): closed** -- `eeschema.EditorControl.lineModeNext/lineModeFree/lineModeOrthonal/lineMode45` are registered and the wire tool follows the mode (`kicad-port/schLineMode.ts`, `components/SchematicView.tsx`); this also closes the "no 90°/45° auto-posture" part of the `W` row above | `SCH_ACTIONS::lineModeNext` |
| Double-click or Enter: finish the wire | partial — double-click wired (`onDoubleClick`); Enter is not (no generic "finish" hotkey action registered yet, unlike PCB's route/zone/shape tools, which also only wire double-click in practice) | `ACTIONS::finishInteractive` |
| Landing back on a pin auto-finishes the wire | identical, pins only (not also wires/junctions/sheet-pins, per `sch_screen.cpp::IsTerminalPoint`'s fuller list) | `sch_screen.cpp::IsTerminalPoint` |
| Backspace: undo last in-progress segment | identical | `SCH_ACTIONS::undoLastSegment`; `eeschema.InteractiveDrawingLineWireBus.undoLastSegment` |
| Escape discards the whole in-progress wire | identical | `doDrawSegments`'s `cleanup()` (shared `ESCAPE` reducer case, already generic) |
| Auto-junction at a T (3+ wire exit angles) | identical, computed live from geometry, never a stored item (the IR has no junction for these; only an explicit junction placed with `J` is stored, in `junctions` -- section 11). Connectivity was already correct before this session (`reconcile`'s own T-junction union-find pass, and `erc.rs`'s dangling checks, both already treated a wire endpoint landing on another wire's *interior* as a real connection) -- but the dot itself was not: the pre-existing `painter.ts::junctionPoints` only ever drew one for 3+ wire endpoints *exactly coincident* (a "star"), never for the classic T shape this row is actually named for (one wire ending partway along another's run), so a correctly-connected T drew no visual confirmation of it at all. Fixed this session: `components/schematic/junctions.ts` (extracted, unit-tested) ports the backend's own `point_on_segment_interior` test arithmetic-for-arithmetic, and also now covers a power-symbol or label anchor landing on a wire's interior (common in practice -- dropping a GND flag or a net label onto an existing wire rather than ending the wire exactly there), which had the identical dot-less symptom for the identical reason | `junction_helpers.cpp::AnalyzePoint`; `reconcile`'s own T-junction union-find pass |
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
| `L`/Ctrl+`L`/`H`: place local/global/hierarchical label | partial — see this section's intro for the click/dialog adaptation; no 90°/auto-rotate-on-placement (a new label reads its spin off the wire it ends, `inferSpin`; Rotate, Mirror and Label Properties then store an explicit spin in `SchExtras::label_spins`, which is drawn and kept through moves; the `.kicad_sch` writer still writes every label at angle 0) | `SCH_DRAWING_TOOLS::TwoClickPlace`/`createNewLabel`; `eeschema.InteractiveDrawing.place{Label,GlobalLabel,HierarchicalLabel}` |
| Auto-increment: a chained label's suggested text advances a trailing number (DATA0 → DATA1...), a name with none is left alone | identical | `common/increment.cpp::IncrementString` (`SCH_LABEL_BASE::IncrementLabel`'s own function) ported directly, not re-derived -- `kicad-port/incrementLabelText.ts`, unit-tested including the zero-pad and "no digits -> unchanged" cases; `state.lastLabelText` is the per-session memory `createNewLabel`'s caller would otherwise keep on the tool instance |
| `P`: place power symbol | partial — any `power:<net>` resolves to a real rail symbol (`eda_model::symbol::builtin`'s generic fallback), so typing a custom rail name works, but this app's own quick-pick (a `<datalist>` of the common presets) is not KiCad's real searchable `DIALOG_SYMBOL_CHOOSER`; orientation (0/90/180/270) is a dialog field instead of a live mid-placement spin | `SCH_DRAWING_TOOLS::PlaceSymbol` (power filter) → `DIALOG_SYMBOL_CHOOSER`; `eeschema.InteractiveDrawing.placePowerSymbol` |
| `T`: place text | identical in effect, adapted flow (see intro) — new `SchematicText` IR (`id`/`content`/`at`/`angle`/`size_um`; deliberately minimal next to real `SCH_TEXT`, no bold/italic/justify/color yet), `Cmd::AddSchText`/`DeleteSchText`, exposed over `GET /api/schematic` (`texts[]`) and rendered with the same Newstroke font every other schematic text uses (`painter.ts::drawSchText`, `LAYER_NOTES`). Also wired into `export_kicad_sch` (a `(text ...)` block, same shape as a label's own) and `import_kicad_sch` (reads `(text ...)` back), so free text now round-trips through a real `.kicad_sch` file like every other schematic item -- not independently round-trip-tested this session beyond the Rust unit/compile checks, since the existing label-import/export code it mirrors line-for-line was the thing actually verified against real files | `TwoClickPlace`/`createNewText`; `eeschema.InteractiveDrawing.placeSchematicText` |
| `A`: place symbol, via a chooser over the sheet's own `lib_symbols` | partial — `SymbolChooserDialog.tsx`: search + live preview (reuses `paintSchematic` on a synthetic one-symbol sheet) over `GET /api/symbol_library`'s catalog, then click to place. Real differences from source, all deliberate: the dialog runs *before* a position is chosen (same order source uses, unlike `L`/`P`/`T`'s own click-first adaptation -- there's no sensible position to capture before you've picked *what*), but a plain search list, not source's recently-used/already-placed tabbed `DIALOG_SYMBOL_CHOOSER`; a placed symbol gets a real, already-numbered reference immediately (`R7`, via `nextReference.ts`) rather than a `"U?"` placeholder left for Annotate -- `Cmd::AddSymbol` refuses an exact duplicate id, so reusing the bare placeholder for a second placement in the same session would be refused outright, and `annotate()`'s own numbering only recognizes an id ending *exactly* in `?` | `SCH_DRAWING_TOOLS::PlaceSymbol` → `DIALOG_SYMBOL_CHOOSER`; `eeschema.InteractiveDrawing.placeSymbol` |
| `GET /api/symbol_library`'s own catalog: "the libraries we already load" | partial — every library name this project's intent already resolved a part against, or that's already on the sheet, scanned for its *full* real-file contents (`eda_kicad::list_symbols_in_library`/`list_symbol_libraries`, both newly unit-tested) when a real `.kicad_sym` resolves (this session's own sandboxed environment has no real KiCad install, so every test board actually exercises the fallback path below, not this one); power symbols (own tool, `P`) and the parametric `Connector_Generic:Conn_01x<N>` family (40 variants, not worth hand-enumerating) are deliberately excluded | `DIALOG_SYMBOL_CHOOSER`'s own library-table-backed search, narrowed to what this app can search at all |
| ... falling back to `eda_model::symbol::builtin_catalog` | identical in spirit to every other "real library first, builtin fallback" resolution in this codebase | new function, mirrors `builtin`'s own existing precedence; required adding `LibSymbol::reference_prefix` (from the library's own `Reference` property, already parsed by `build_symbol` but previously discarded) -- another `~27`-call-site field addition, same cost as `SchematicText`'s own (section 3's intro) |
| Placing a symbol with no intent counterpart adds a synthesized `Part`, shows up unplaced on PCB | identical | `reconcile_schematic`, see section 0 |

## 4. ERC (gap #4) (`sch_inspection_tool.cpp`, `dialog_erc.cpp`)

**ERC is kicad-cli's** (2026-10-03, `docs/ARCHITECTURE.md`, "Engines"): the Rust ERC port is deleted, so there is no engine parity left to track here. `GET /api/erc` exports the current schematic to a derived `.kicad_sch`, runs `kicad-cli sch erc` on it (the project file carries the design's own pin map, section 9, and ignores the library-link checks, since every symbol is embedded) and points each reported item back at our own id. It takes a few seconds, so the dialog runs it on demand and shows a running state.

| Action | Status | KiCad file:function |
|---|---|---|
| Run ERC (menu item, no default hotkey) | identical, by running KiCad's own | `SCH_INSPECTION_TOOL::RunERC`/`ShowERCDialog`; `eeschema.InspectionTool.runERC` -> `ErcDialog.tsx`, `GET /api/erc` -> `kicad-cli sch erc`. Opening the dialog on a schematic kicad-cli has not judged yet runs it; "Run ERC" re-runs; a report older than the board says so |
| Results list: one row per finding, Errors/Warnings/Exclusions filter | identical in spirit, flatter structure | `dialog_erc.cpp` (`RC_TREE_MODEL`, flat list, not grouped by sheet -- matches); each row shows kicad-cli's own type name and description |
| Click a row: cross-probe (select + pan/zoom to it) | identical in spirit | `DIALOG_ERC::OnERCItemSelected`/`FocusOnItem`; a violation's `location` is our id for its first item (a symbol, a pin `REF.PIN`, a power symbol, a wire, a label, a no-connect, a text), which `ercMarkerPosition` resolves back to a point/refs; `jumpTo` selects/hots the refs and re-frames the sheet |
| Canvas markers independent of the dialog | identical in spirit | `SCH_MARKER` objects drawn via the normal VIEW; `painter.ts`'s `drawErcMarkers`, one circle per violation whose location resolves, color-coded by severity (`LAYER_ERC_ERR`/`WARN`/`EXCLUSION`), kept after the dialog closes until the next run |
| Exclusions (persisted "accepted" findings) | partial | `SCHEMATIC::RecordERCExclusions`/`ERC_SETTINGS::m_ErcExclusions` -- `Cmd::AddErcExclusion`/`DeleteErcExclusion` persist to `design.schematic.erc_exclusions` (matched by exact `(check, location)`), applied to the report by the backend as a third `ErcSeverity::Excluded` so an excluded finding stays visible (dimmed, its own filter) and is un-excludable; the report on screen is patched in place, no re-run. Real KiCad also offers this from a right-click on the canvas marker -- not ported |
| Our own readability checks (grid, wire length/overlap, label placement, sheet density) | not KiCad's, so not parity | `crates/lint` (`eda-lint`), `GET /api/lint`: a Lint tab of the dialog and blue diamond markers while the dialog is open, never mixed into kicad-cli's list |

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
| Grid of all symbols × fields (Reference, Qty, Value, Footprint, Datasheet, user fields) | partial — covers the design's symbols on every sheet (`fields_table::project_view`; an edit goes to the sheet that has its symbol, a new column is a column of every sheet), no scope selector; no attribute columns | `FIELDS_EDITOR_GRID_DATA_MODEL::RebuildRows`/`GetValue` ↔ `fields_table::build_table`, `POST /api/sch/fields_table` |
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
| Export tab: format presets CSV / TSV / Semicolons, field/string/reference/range delimiters, keep tabs, keep line breaks, live preview, write file | identical output format (`BOM_FMT_PRESET::CSV()` etc., doubled embedded string delimiters, `\n` after the last shown column, hidden columns omitted, children not exported, mixed values listed comma-separated) | `FIELDS_EDITOR_GRID_DATA_MODEL::Export`, `BOM_FMT_PRESET`, `OnExport` ↔ `fields_table::export_bom`, `BomFmt`, `POST /api/sch/bom_export` (separate from, and richer than, `POST /api/fab/bom`, which runs `kicad-cli sch export bom` with the fixed JLC-style columns) |
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
| The pin map is a per-design setting used by ERC | identical — stored additively as `schematic.erc_pin_map: Option<ErcPinMap>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`; 12×12 `PIN_ERROR` ints like KiCad's `pin_map` project entry); absent = KiCad's default table; a malformed grid is ignored | `ERC_SETTINGS::m_PinMap`/`m_defaultPinMap`, `pin_map` loader ↔ `eda_model::ir::ErcPinMap`/`DEFAULT_ERC_PIN_MAP`, `eda_kicad::custom_erc_pin_map` |
| 11×11 lower-triangle grid (NC excluded: "generates errors separately"), row/column labels | identical | `PANEL_SETUP_PINMAP::reBuildMatrixPanel`, `CommentERC_H/V` ↔ `SchematicSetupDialog.tsx`, `kicad-port/ercPinMap.ts` |
| Click a cell cycles OK → Warning → Error → OK, symmetric | identical (green ✓ / amber ! / red ×, with the source's tooltips) | `changeErrorLevel` (`( level + 1 ) % 3`, `SetPinMapValue( y, x )` and `( x, y )`) ↔ `Cmd::SetErcPinMapCell { a, b, level }` (undoable, schematic scope) |
| "Reset to Defaults" | identical (stored as an absent `erc_pin_map`; refused when already default; undoable) | `PANEL_SETUP_PINMAP::ResetPanel`/`ERC_SETTINGS::ResetPinMap` ↔ `Cmd::ResetErcPinMap` |
| ERC picks it up | identical — the derived project file carries the stored matrix as `erc.pin_map` (`eda_kicad::export_kicad_pro_for`), so `kicad-cli sch erc` judges pin conflicts by it: an Output↔Output cell set to OK removes the `pin_to_pin` finding (checked against kicad-cli), Warning downgrades it; run ERC again to see it | `ERC_TESTER::TestPinToPin` (kicad-cli's) |
| ERC severity per check (Ignore / Warning / Error) | **gap** — the design stores no per-check severity table; kicad-cli uses KiCad's defaults (the project file only ignores the two library-link checks). Needs a per-check severity map in the IR written into the project file's `erc.rule_severities` | `panel_setup_severities.cpp`, `ERC_SETTINGS::m_ERCSeverities` |
| Other Schematic Setup pages (general, formatting, annotation, field templates, net classes, text variables, bus aliases, ...) | missing — not shown as dead tabs | `dialog_schematic_setup.cpp` |
## 10. Plot and netlist export (`sch_plotter.cpp`, `netlist_exporter_*.cpp`)

**Plots and netlists are kicad-cli's** (2026-10-03): the Rust plotter and netlist exporter are deleted. File > Plot... on the Schematic tab (`common.Control.plot`, shared with the PCB tab's Gerber plot, which dispatches by tab) and File > Export > Netlist... (`eeschema.EditorControl.exportNetlist`) post to `/api/sch/plot` / `/api/sch/netlist` (`crates/cli/src/sch_output_api.rs`), which turn the dialog's options into `kicad-cli sch export svg|pdf|netlist` arguments and run it on the exported schematic, into `<board>/export/kicad/sch-<kind>/` (files named after the project). Both are read-only exports (nothing is written to `design.json`), so there is no `/api/cmd` verb or undo entry. A board with no `schematic` section exports the schematic the engine would derive from its intent, like `GET /api/schematic.svg`.

| Action | Status | KiCad file:function ↔ our code |
|---|---|---|
| Plot dialog: format (SVG / PDF), colour or black and white, drawing sheet, background colour, all pages / current page | identical, by running KiCad's own plotter | `DIALOG_PLOT_SCHEMATIC` / `SCH_PLOT_OPTS` ↔ `PlotSchematicDialog.tsx`, `kicad-port/schOutputs.ts` (`buildSchPlotRequest`) → `--black-and-white`, `--exclude-drawing-sheet`, `--no-background-color`, `--pages` |
| Page size Auto / A4 / A | **gap** — kicad-cli has no page-size override (it plots at the sheet's own size), so the dialog no longer offers the control | `SCH_PLOT_OPTS::m_pageSizeSelect` |
| SVG: one file per sheet; PDF: one multi-page file | identical | `SCH_PLOTTER::createSVGFiles`/`createPDFFile` (kicad-cli's) |
| Netlist: KiCad `.net` and generic `.xml` | identical | `kicad-cli sch export netlist --format kicadsexpr|kicadxml` |
| PostScript / DXF / PNG, a colour-theme chooser, PDF property popups | missing from the dialog (kicad-cli has them: `sch export ps|dxf|png`, `--theme`) | `DIALOG_PLOT_SCHEMATIC` |
| Spice / Cadstar / OrcadPCB2 / Allegro / PADS exporters | missing from the dialog (kicad-cli has them: `--format`) | `netlist_exporter_*.cpp` |
| Explicit junctions (`J`) and graphic lines (`I`) | identical -- both are in the derived `.kicad_sch` (`junction`, `polyline`), so kicad-cli plots them with everything else (the deleted Rust plotter did not) | `SCH_JUNCTION::Plot`, `SCH_LINE::Plot` (kicad-cli's) |

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

## 12. Edit, drawing, point-editor and selection tools (`sch_edit_tool.cpp`, `sch_drawing_tools.cpp`, `sch_point_editor.cpp`, `sch_selection_tool.cpp`)

Of the 75 `eeschema.InteractiveEdit` / `InteractiveDrawing` / `PointEditor` / `InteractiveSelection` actions, 60 are wired (36 of them in this pass) and the
other 15 are recorded with a reason in `tools/ui-parity-missing.json` (no handler stands in for them). Everything was ported from the KiCad source
(`eeschema/`, commit 8303b2ad), not from memory; each handler and each `kicad-port/sch*.ts` module cites the function it follows.

Data and verbs. Every edit is one `/api/cmd` verb with undo. The new family is `Cmd::SchEdit(SchCmd)` (`{"op":"sch_edit","verb":...}`,
`crates/ops/src/sch_edit.rs`): `set_locked`, `add_graphic` / `edit_graphic` / `delete_graphic`, `delete_sheet`, `add_sheet_pin` / `edit_sheet_pin` /
`delete_sheet_pin`, `change_symbol`, `update_library_symbols`. The new items live in `SchematicSection.extras` (`crates/model/src/sch_extras.rs`): `graphics`
(rectangle, circle, arc, bezier, polygon, text box, rule area, directive label) and `locked`; both are additive, defaulted and left out of `design.json`
when empty. The derived `.kicad_sch` carries them -- `rectangle`, `circle`, `arc`, `bezier`, `polyline`, `text_box`, `rule_area`, `netclass_flag` and `(locked yes)`
-- written the way `SCH_IO_KICAD_SEXPR` writes them (`crates/kicad/src/sch_extras_io.rs`) and read back by `import_kicad_sch`. None of them is part of a net, so
section 0's "one netlist" rule is untouched. The rules with no React in them are the pure `kicad-port/sch*.ts` modules (each with a `.test.ts`); the tool
state machines are `components/schematic/sch*Tool*.ts`, the handlers `actions/schEditActions.ts` (+ `schSheetPinActions.ts`, `schSymbolActions.ts`).

| Action(s) | Status | KiCad file:function |
|---|---|---|
| Selecting every item kind, the Selection Filter | identical for what exists: labels, texts, power symbols, no-connects, bus entries, sheets, drawn shapes, text boxes, rule areas and directive labels can be clicked, box-selected, Select All'ed and deleted (hit tests in `kicad-port/schItemGeom.ts`). The right dock's Selection Filter has KiCad's switches -- All items, Symbols, Text, Wires, Labels, Graphics, Rule Areas, Other items, Locked items (off by default: a locked item cannot be clicked, boxed or Select All'ed until it is on) -- except Pins and Images (no pin is selectable on its own and there are no bitmap items, so those switches would gate nothing). The selection survives a board refresh on the schematic tab (it is pruned against the sheet instead of being cleared) | `sch_selection_tool.cpp::itemPassesFilter`, `panel_sch_selection_filter.cpp` (`kicad-port/schSelectionFilter.ts`, `schSelectionPrune.ts`) |
| Right-click menu | identical conditions: the entries and their enabled state follow `SCH_CONDITIONS::Count/MoreThan/OnlyTypes/HasTypes` over the selection's item kinds (Lock/Unlock/Toggle Lock, Change To, Break/Slice, Create/Remove Corner, Change/Update Symbol, the sheet-pin entries, Edit Text & Graphics...). An entry KiCad would offer that is not wired shows as disabled; nested levels open on hover only | `sch_selection_tool.cpp::Init`, `sch_edit_tool.cpp::Init` (`kicad-port/schContextMenu.ts`, `components/schematic/SchContextMenu.tsx`) |
| Lock, Unlock, Toggle Lock | identical: one `set_locked` for every selected item (one undo step); Toggle unlocks when any selected item is locked, otherwise locks. A symbol locks by reference, so every unit of it locks together; sheet pins are skipped, as in source. A locked item is painted but cannot be selected (hence edited) without the filter's Locked items. Round-trips as `(locked yes)` | `sch_edit_tool.cpp::modifyLockSelected` (`kicad-port/schLock.ts`); `SchCmd::SetLocked` |
| Change To Label / Global Label / Hierarchical Label / Directive Label / Text / Text Box | identical in effect: every selected label, text, text box or directive label that is not already of the target type is replaced by a new one at the same place, carrying its text (a label's text unescaped, a new label's text made a valid net name), its shape and its text size, and the new items are selected. Delete + add go in as ONE batch, one undo step. A label's orientation is implicit here (read off its wire), so KiCad's spin bookkeeping becomes a text angle / a directive label's pole direction | `sch_edit_tool.cpp::ChangeTextType` (`kicad-port/schConvertText.ts`) |
| Draw Rectangle / Circle / Arc / Bezier / Text Box, Draw Rule Area, Place Class (Directive) Label | identical state machines: a rectangle, text box or circle takes two clicks, an arc two (start and end: the 90-degree arc between them, `calcEdit`'s state-1 rule; KiCad then adjusts it with the arc point editor, which the studio does not have), a Bezier four (start, end, two control points); a rule area is a polygon with the 45-degree leader and closing loop (a double-click or Close Outline finishes, Delete Last Point / Backspace drops the last corner, Esc cancels). The tools stay armed after each item and select what they draw. A text box and a directive label ask for their text / fields in a dialog. The drawing tools work on the sheet in view (every command is addressed to it, section 14: on a nested sheet the item lands there). Drawn with the default stroke and no fill; Properties (`E`, a double-click) opens Shape, Rule Area, Text Box and Directive Label Properties, and Edit Text & Graphics sets widths and fills in bulk. The Symbol Editor keeps its own Draw Rectangle / Circle / Arc handlers (the registry chains by tab) | `sch_drawing_tools.cpp::DrawShape`, `DrawRuleArea`, `TwoClickPlace`; `EDA_SHAPE::BeginEdit/ContinueEdit/CalcEdit/EndEdit`, `POLYGON_GEOM_MANAGER` (`kicad-port/schShapeEdit.ts`, `polygonGeom.ts`); `SchCmd::AddGraphic` |
| Create Corner, Remove Corner (point editor) | identical: with the pointer on a polygon's or rule area's outline Create Corner inserts a vertex on the nearest edge, with it on a corner Remove Corner deletes it (a rule area keeps at least 3 corners, a polygon 2, as `removeCorner`'s guards). The menu entries carry the same conditions. The arc point editor (`cycleArcEditMode`) is a separate row of `docs/parity/UI-ACTIONS.md` | `sch_point_editor.cpp::addCorner` / `removeCorner` and their conditions (`kicad-port/schPolyCorners.ts`); `SchCmd::EditGraphic` |
| Break, Slice | identical in effect: the line under the cursor (a wire, bus or graphic line; with several selected, each at its own midpoint) is cut and the new end follows the cursor with a live preview. Break ("divide into connected segments") bends the line at the cut, Slice ("unconnected segments") leaves two lines. One undo step | `sch_move_tool.cpp::preprocessBreakOrSliceSelection`, `sch_line_wire_bus_tool.cpp::BreakSegment` (`kicad-port/schBreak.ts`, `components/schematic/schBreakTool.ts`) |
| Place Next Symbol Unit | identical: with one multi-unit symbol selected, the lowest unit of it not yet on the sheet is armed next, as a copy of the selected symbol under the same reference (KiCad's info-bar messages when there is none: "only one unit", "all units already placed") | `sch_drawing_tools.cpp::PlaceNextSymbolUnit` (`kicad-port/schUnits.ts`) |
| Place Pins from Sheet, Autoplace All Sheet Pins, Cleanup Sheet Pins, Sync Selected / All Sheet Pins | partial: a sheet's pins are matched by name to the hierarchical labels of its own file. Place Pins from Sheet offers the labels that have no pin one at a time in natural order (a click puts the pin on the sheet's nearest border, `ConstrainOnEdge`; the tool ends when none is left); Autoplace lays the missing pins out row by row from the sheet's top-left corner (or after the last existing pin) and pushes each onto the nearest border, using an estimate of each pin's text width; Cleanup deletes the pins that have no label (a dialog lists them first); Sync is the pin side of `DIALOG_SYNC_SHEET_PINS` (add the missing pins, delete the unreferenced ones, take a label's shape). Pin edits work on the sheet in view (the pin verbs are addressed to it, section 14); the dialog's label side (adding labels to the sheet's file from its pins) is not ported | `sch_drawing_tools.cpp::TwoClickPlace` (sheet pins), `AutoPlaceAllSheetPins`, `SyncSheetsPins`, `SyncAllSheetsPins`; `sch_edit_tool.cpp::CleanupSheetPins`; `SCH_SHEET_PIN::ConstrainOnEdge` (`kicad-port/schSheetPins.ts`); `SchCmd::AddSheetPin/EditSheetPin/DeleteSheetPin` |
| Change Symbol(s), Update Symbol(s) | partial: `DIALOG_CHANGE_SYMBOLS` on the selection, all symbols, or those matching a reference / value / library id (`*` / `?` wildcards). A symbol that cannot take the new one is reported in the dialog's message list ("symbol not found", "new symbol has too few units") and left alone; the rest get the new library symbol, and the Reference / Value options reset the field text from it (the new prefix with the old number kept; "reset empty fields" as in source). Update publishes the project's edited library symbols. KiCad also resets field sizes, positions and visibilities; the studio keeps none per symbol, so only the field text options and the library link are ported | `sch_edit_tool.cpp::ChangeSymbols`, `dialog_change_symbols.cpp` (`kicad-port/schChangeSymbols.ts`); `SchCmd::ChangeSymbol`, `UpdateLibrarySymbols` |
| Swap Unit Labels | identical rule, not click-tested (no multi-unit symbol in the scratch design; unit-tested): with two or more units of one reference selected, each pin's single net label is swapped between the units in pin-position order, refused with KiCad's message unless every pin has exactly one label and nothing else on its net | `sch_edit_tool.cpp::SwapUnitLabels`, `findSingleNetLabelForPin` (`kicad-port/schSwapLabels.ts`) |
| Edit Text & Graphics Properties | partial: `DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS` over the selection or the whole sheet, for the kinds the IR stores (free texts, text boxes, drawn shapes, rule areas, graphic lines): text size, bold, italic and alignment of text boxes, text size of free texts, line width and style, fill; a property left "unchanged" is not touched. KiCad's fields, labels, junctions and wires (sizes, orientation, colours, visibility) are not covered: the studio keeps none of those per item | `sch_edit_tool.cpp::GlobalEdit`, `dialog_global_edit_text_and_graphics.cpp::processItem` (`kicad-port/schGlobalEdit.ts`) |
| Convert Stacked Pins, Explode Stacked Pin | Symbol Editor tab only (`actionTabGate`), identical rule: `[1-3]` notation is expanded (`ExpandStackedPinNotation`), the selected pins (or a pin and the others at its place) are folded into / exploded from it in one batch (one undo step); Explode leaves the smallest number shown and hidden copies of the rest. The pin's right-click menu offers them under KiCad's conditions (Convert: two or more pins at one place, or a pin that shares its place; Explode: one pin with valid stacked notation). Click-tested on a symbol with two pins at one place | `symbol_editor_edit_tool.cpp::ConvertStackedPins` / `ExplodeStackedPin`, `Init`'s `canConvertStackedPins` / `canExplodeStackedPin` (`kicad-port/stackedPins.ts`) |
| Autoplace Fields, Place Design Block | missing: no per-instance field positions / no design-block library | `tools/ui-parity-missing.json` (unchanged) |
| Place Linked Design Block, Save to Linked Design Block, Draw Sheet from Design Block | missing: no design-block library or block IR | `tools/ui-parity-missing.json` |
| Import Sheet, Draw Sheet from File | missing: need a file chooser and a sheet-file importer (the one reader, `import_kicad_sch`, builds a whole design rather than a sheet of this one). Draw Sheet itself works (section 11) | `tools/ui-parity-missing.json` |
| Draw Tables, Place Images | missing: the IR has no table or bitmap item | `tools/ui-parity-missing.json` |
| Assign Netclass | partial: `DIALOG_ASSIGN_NETCLASS` on the nets of the selected wires, labels and power symbols (`actions/assignNetclass.ts`, `components/AssignNetclassDialog.tsx`): the pattern KiCad proposes for them (`GetNetclassPatternForSet`), the class, the nets the pattern matches; OK stores the assignment with the board's net classes (`set_net_classes`, undone from the board editor). Not ported: the canvas preview that selects the matching nets while typing; regular-expression patterns | `common/dialogs/dialog_assign_netclass.cpp` |
| Find in Net Navigator | wired in section 13, with the Net Navigator panel it needed; Tab / Shift+Tab walk the net's items (section 11) | `sch_editor_control.cpp::FindNetInInspector` |
| Swap Pins, Swap Pin Labels | missing: need pin selection (the canvas never selects a single symbol pin); Swap Pins also needs per-instance pin geometry | `tools/ui-parity-missing.json` |
| Cycle Body Style (`toggleDeMorgan`) | missing: a placed symbol cannot show an alternate body style (`to_engine_symbol` drops it, see `PARITY-symedit.md`) | `tools/ui-parity-missing.json` |
| `SyncSelection` | missing by design: an internal action with no label, hotkey or menu entry that KiCad itself never runs as an action | `tools/ui-parity-missing.json` |

Found and fixed while verifying: the context menu opened every submenu at once, and a click inside it fell through to the canvas (Lock then hit the hovered
rule area instead of the selected symbol); a board refresh dropped every non-part id from the selection (Change To and Lock lost their selection); a locked item
could never be unlocked from the UI (hence the Selection Filter's Locked items); Backspace while drawing a rule area cleared the tool instead of its last corner;
a double-click finished a polygon at a stale cursor (it now uses the click's own point); Break / Slice did nothing when the pointer had not moved first; autoplaced
sheet pins sent fractional coordinates the backend refuses; Place Pins from Sheet could double-place a pin on a quick second click. In the shared canvas context menu
(`components/canvas/ContextMenu.tsx`, used by the PCB, footprint and symbol canvases) no entry ever ran: a press on an entry bubbled to the canvas's `onPointerDown`,
which closes the menu, so the menu was gone before the click (Rotate on the PCB canvas and Delete on the symbol canvas sent nothing); presses inside the menu now stay inside it. Update Symbol(s) flips `LibrarySymbol::published` in the symbol library, which a Schematic-scope undo did not restore (and no other
scope reached), so it could not be undone; `board.rs::restore_domain` now takes the published flags (only those) from the snapshot in that scope, with a test.

Verified in the browser pane against a scratch copy of `work/mcu30` (the real board was never touched): Lock / Unlock / Toggle Lock and the Locked items filter, Change
To, Delete and undo, all seven drawing tools including rule-area Backspace / Esc / Close Outline, Break and Slice, sheet pins (Sync dialog add and delete,
Autoplace, the Cleanup dialog, the Place Pins tool), Change Symbol including the "symbol not found" report, Create / Remove Corner, Edit Text & Graphics, and the Symbol tab's
Draw Rectangle / Circle / Arc after the merge with the library-editor work (they land in the open library symbol, not on the sheet), plus Convert / Explode Stacked Pins from
the Symbol tab's pin menu. Not click-tested: Swap Unit Labels (the scratch design has no multi-unit symbol; the rule is unit-tested). A scripted click needs a pointer move
first (the tools read the cursor from `pointermove`), and Cmd+Z, not Ctrl+Z, is the macOS undo key.

## 13. Schematic control: View toggles, hierarchy, attributes, library and export tools (`sch_editor_control.cpp`, `sch_navigate_tool.cpp`, `sch_inspection_tool.cpp`, `sch_edit_tool.cpp::SetAttribute`, `sch_tool_base.h::Increment`)

The `eeschema.EditorControl`, `NavigateTool`, `InspectionTool` and `Interactive.increment*` actions, `SymbolLibraryControl.exportSymbolAsSVG` and `InteractiveEdit.findNetInInspector`:
40 wired in `actions/schControlActions.ts`, 24 recorded with a reason in `tools/ui-parity-missing.json` and the 23 simulator rows reworded there as deferred (the simulator comes
after the UI parity sweep: ngspice 45.2 is installed, the plan is `kicad-cli sch export netlist --format spice` and `ngspice -b`). Everything was ported from the KiCad source
(`eeschema/`, commit 8303b2ad); each handler and `kicad-port/` module cites the function it follows.

Data, verbs and routes. A placed symbol carries `dnp`, `exclude_from_bom`, `exclude_from_board`, `exclude_from_sim` and a sheet placement a `page` (`crates/model/src/ir.rs`,
additive, left out of `design.json` when false / empty); the derived `.kicad_sch` writes them as `(dnp yes)`, `(in_bom no)`, `(on_board no)`, `(exclude_from_sim yes)` and the
`(instances ... (page ..))` of a sheet, which kicad-cli's BOM, netlist and ERC read, and `import_kicad_sch` reads them back. New verbs (`crates/ops/src/sch_control.rs`, each one undo step):
`set_symbol_attrs`, `set_sheet_page`, `set_sch_item_text`, `set_symbol_lib_ids`, `increment_annotations`. New routes (`crates/cli/src/sch_control_api.rs`, `sch_export_api.rs`,
`bom_plugins.rs`): `GET /api/sch/hierarchy`, `POST /api/sch/export_symbols` and `GET /api/sch/bom_plugins` (no process started), and the three that run kicad-cli,
`POST /api/sch/bom`, `POST /api/sch/bom_legacy` and `POST /api/sym/svg`, each through `offload` and the one-at-a-time lane (the lane's guard test knows the module). The rules with no
React in them are the pure `kicad-port/` modules, each with a `.test.ts`: `stringIncrement`, `sheetPages`, `schControl`, `symbolChecker`, `cmpFile`, `footprintFilter`, `symbolDiff`, `libLinks`.

| Action(s) | Status | KiCad file:function |
|---|---|---|
| Show Hidden Pins, Show Directive Labels, Show ERC Errors / Warnings / Exclusions, Mark items excluded from simulation (View) | identical: each is a flag with KiCad's default (hidden pins off, directive labels on, ERC errors and warnings on, exclusions off, simulation marks on), kept in the browser's local storage like KiCad keeps them in its settings file, with the check mark in the menu. A hidden pin is drawn in the hidden-items colour, a directive label is drawn only while selected when its toggle is off, ERC markers are filtered by severity, a symbol excluded from simulation gets its frame and circled "S" | `sch_editor_control.cpp::ToggleHiddenPins` ... `MarkSimExclusions`; `SCH_PAINTER::draw( SCH_SYMBOL )`, `draw( SCH_DIRECTIVE_LABEL )` (`components/schematic/displayOptions.ts`, `symbolMarkers.ts`) |
| Do not Populate, Exclude from Bill of Materials / Board / Simulation | identical: the whole selection (every unit of a multi-unit symbol) goes to one state, set when any of it lacks the attribute and cleared when all have it; the menu check mark shows it; a Do not Populate symbol is drawn with the red cross. Acts on the sheet in view (section 14), and the flags go with a symbol when Reorganize moves it to its module sheet | `sch_edit_tool.cpp::SetAttribute`, `SCH_PAINTER::draw( SCH_SYMBOL )` (`kicad-port/schControl.ts`); `Cmd::SetSymbolAttrs` |
| Next Sheet, Previous Sheet, Enter Sheet, Change Sheet | identical: the sheet one step along the page order (page numbers first in numeric order, then the other texts in natural order; a sheet with no page of its own takes its place in the walk; at either end nothing happens), Enter Sheet on the selected or hovered sheet, a double-click on a sheet; each cancels the tool in use and clears the selection | `sch_navigate_tool.cpp::Next/Previous/EnterSheet/changeSheet`, `SCH_SHEET::ComparePageNum`, `SortByPageNumbers` (`kicad-port/sheetPages.ts`); `GET /api/sch/hierarchy` |
| Edit Sheet Page Number... | identical: letters and digits only, empty puts the sheet back in the hierarchy order; the page of the sheet shown when none is selected. The root's page is not stored (it is page 1) | `sch_edit_tool.cpp::EditPageNumber`; `Cmd::SetSheetPage` |
| Net Navigator, Highlight Nets, Find in Net Navigator | the panel, docked in the schematic's left column above the hierarchy (`Left().Layer( 3 ).Position( 0 )`, `components/panels/SchematicDock.tsx`), is a tree of the sheet's nets, each with the items on it (symbol pins, labels, power symbols, no-connects), a wildcard filter, and with a net highlighted it shows that net alone; clicking a net highlights it, clicking an item selects it. Highlight Nets is the picker tool (a click highlights the net of the wire, label or power symbol under it, empty space clears, Esc leaves); Find in Net Navigator opens the panel on the net of the first selected item with one, else the highlighted net. Pins cannot be picked (no pin is selectable) | `sch_editor_control.cpp::ShowNetNavigator`, `HighlightNetCursor`, `FindNetInInspector`, `SCH_EDIT_FRAME::FindNetInInspector` |
| Increment Annotations From..., Increment / Increment Primary / Decrement Primary / Increment Secondary / Decrement Secondary | the dialog moves every reference with the start's letters from its number up by the step, all together (an `R5 -> R6, R6 -> R7` plan renames through temporary names), refused on a collision or a number below 0, one undo step; the increment actions step the selected labels or texts (or the one nearest the cursor) in their primary or secondary number with KiCad's `STRING_INCREMENTER`, a mixed selection does nothing. Not ported: incrementing a pin's number or name in the Symbol Editor (it needs the pin text boxes to see which one the pointer is on); the dialog's "All sheets" scope (it acts on the sheet in view) | `sch_editor_control.cpp::IncrementAnnotations`, `dialog_increment_annotations.cpp`; `sch_tool_base.h::Increment`; `string_utils.cpp::STRING_INCREMENTER` (`kicad-port/stringIncrement.ts`); `Cmd::IncrementAnnotations`, `SetSchItemText` |
| Bulk Edit Symbol Library Links..., Assign Footprints..., Import Footprint Assignments... | the links dialog lists each library id with the symbols that use it and a new id per row (Map Orphans fills in a symbol of the same name; "update symbol fields" resets Value, Footprint and Datasheet to the library's), checked with `LIB_ID::IsValid` and against the symbols the studio can load, the project library included; Assign Footprints is CvPcb's three lists (symbols, libraries, footprints narrowed by the symbol's footprint filters, the pin count and a search) applied as one undo step, without CvPcb's footprint preview and equivalence files; Import Footprint Assignments reads a `.cmp` file | `sch_editor_control.cpp::EditSymbolLibraryLinks`, `dialog_edit_symbols_libid.cpp`; `ShowCvpcb`, `footprint_filter.cpp`; `ImportFPAssignments`, `processCmpToFootprintLinkFile` (`kicad-port/libLinks.ts`, `footprintFilter.ts`, `cmpFile.ts`); `Cmd::SetSymbolLibIds` |
| Compare Symbol with Library, Symbol Checker, Show Bus Syntax Help, Switch to PCB Editor, Select on PCB, Next / Previous Symbol Unit | identical rules: the diff reports how the symbol the schematic draws differs from the library's (fields are not compared: they belong to the instance), the checker gives `CheckLibSymbol`'s warnings for the symbol in the Symbol Editor (an empty or digit-ended reference prefix, duplicate pins with stacked pins expanded, a power symbol that is not one unit with one power pin, a hidden power pin, a pin off the grid, a zero-size circle or rectangle), Select on PCB selects the symbols' footprints on the board tab, the unit steps wrap | `sch_inspection_tool.cpp::DiffSymbol`, `CheckSymbol`, `ShowBusSyntaxHelp`; `symbol_checker.cpp::CheckLibSymbol`; `sch_editor_control.cpp::ShowPcbNew`, `ExplicitCrossProbeToPcb`; `symbol_editor_control.cpp::ChangeUnit` (`kicad-port/symbolChecker.ts`, `symbolDiff.ts`) |
| Generate Bill of Materials... | the Symbol Fields Table opened on its Export tab. The preview is the table's own live port; the **file** is written by `kicad-cli sch export bom` from the saved design (edits not yet applied are applied first), with the table's columns, grouping, sort, filter and the format options handed over as KiCad's own BOM preset and format preset in the derived project file (`--preset`: the command line cannot say "sort descending", an explicit `--sort-asc false` crashes kicad-cli). The output file must be inside the board's `export/` folder. The two agree on the scratch board | `sch_editor_control.cpp::GenerateBOM`, `eeschema_jobs_handler.cpp::JobExportBom`; `POST /api/sch/bom` |
| Generate Legacy Bill of Materials... | the Bill of Materials dialog lists the generator scripts of the installed KiCad's plugins folder (`bom_csv_grouped_by_value.py` and the rest; the helper modules are left out), shows each one's header and command line, and Generate writes the XML netlist (`kicad-cli sch export python-bom`) and runs the script over it into `export/kicad/sch-python-bom/`; the output can be saved from the browser. Refused when a symbol is not annotated, as `ReadyToNetlist`. Only KiCad's own scripts run: adding a script of your own, editing its command line and the console flag are not ported (the browser could otherwise run any program on this machine) | `sch_editor_control.cpp::GenerateBOMLegacy`, `dialog_bom.cpp`, `bom_plugins.cpp`; `POST /api/sch/bom_legacy`, `GET /api/sch/bom_plugins` |
| Save Current Sheet Copy As..., Export Drawing to Clipboard | the sheet shown as a `.kicad_sch` download; the whole page (frame, title block, every item, nothing selected) drawn with the editor's own painters at 8 px/mm and put on the clipboard as a PNG (a browser that refuses the clipboard gets the PNG as a file). Click-tested up to the clipboard call, which the hidden browser pane refused ("Write permission denied") | `sch_editor_control.cpp::SaveCurrSheetCopyAs`, `DrawSheetOnClipboard` |
| Export Symbols... | every library symbol the schematic uses, once and in library-id order, as one `.kicad_sym` the browser saves (power symbols when asked; a symbol drawn as a plain box has no library symbol and is reported; two of one name keep the later). "Update schematic symbols to link to exported symbols" puts them into the project library under the new nickname (published) and moves the placed symbols' links in one undo step, like KiCad's "Update Library Identifiers" commit | `sch_editor_control.cpp::ExportSymbolsToLibrary`; `POST /api/sch/export_symbols`, `Cmd::SetSymbolLibIds` |
| Export Symbol as SVG... (Symbol Editor) | the symbol being edited, in its unit and body style, plotted by `kicad-cli sym export svg` (no plotter of ours) and saved as `<name>.svg`; kicad-cli plots with a 20 % margin where the editor uses 10 % | `symbol_editor_control.cpp::ExportSymbolAsSVG`; `POST /api/sym/svg` |
| Add / Remove Design Variant, Edit Variant Description | missing: no design variants (per-variant overrides of the attribute flags and fields) in the IR | `tools/ui-parity-missing.json` |
| Show Hidden Fields, Show Pin Alternate Icons, Annotate Automatically | missing: the schematic keeps no field positions or visibility, the library symbol IR no alternate pin functions, and a placed symbol is numbered at once (`nextReference`), so there is nothing to toggle | `tools/ui-parity-missing.json` |
| Import Non-KiCad Schematic, Remap Legacy Library Symbols, Rescue Symbols, Remote Symbols | missing: importers for other EDA formats, legacy `.lib` libraries, the cache a schematic keeps next to its libraries and online part libraries have no counterpart here | `tools/ui-parity-missing.json` |
| The three drag-and-drop handlers, `restartMove`, `updateNetHighlighting`, Generate Bill of Materials (External) | missing by design: internal actions with no label or menu entry, or (the external BOM) an action KiCad defines and never binds | `tools/ui-parity-missing.json` |
| The seven Design Block actions (panel, save sheet / selection, update from sheet / selection, properties, delete) | missing: a design-block library and the schematic copy and paste to place a block do not exist (the same reason as Place Design Block in section 12) | `tools/ui-parity-missing.json` |
| Simulator, OP voltages and currents, and every `Simulation` / `Simulator` action | deferred, not blocked: the simulator comes after the UI parity sweep | `tools/ui-parity-missing.json` |

Found and fixed while verifying: a symbol from a library (`Device:R`, ...) was drawn on the schematic with no body and no pins -- only its reference and value text -- because
`GET /api/schematic`'s `lib_symbols` carried the engine symbol's field names (`stroke_mm`, `filled`, `radius_mm`, `text`, no `body_style`, no pin `hidden`) while the painter
and the `LibSymbol` type read `stroke_width`, `fill`, `radius`, `content`, `body_style` and `hidden`, and keeps an item only when its `body_style` is 0 or the placed one's
(the scratch board `work/mcu30`'s symbols are all plain boxes, which is why it never showed). `lib_symbol_json` sends both spellings now (a filled rectangle is the body colour,
any other filled shape the outline colour: the engine symbol keeps only whether a shape is filled), with a test; Compare Symbol with Library depends on the same shape.
`export_kicad_sym_library` wrote a hidden property as `(effects ...) hide`, which kicad-cli answers with "Unable to load library", so every
`.kicad_sym` the studio exported (Export..., Save Library As..., and now Export Symbols and the SVG plot) was unreadable to KiCad whenever a symbol had a hidden property --
it is `(effects ... (hide yes))` now, with a test; `board::load` rebuilt the model from the intent file and lost the part `AddSymbol` synthesizes for a symbol the intent does
not have, so ERC, the BOM, the netlist and every plot refused a board with "symbol id has no matching part in the constraint model" as soon as one symbol had been placed -- `load`
folds the parts back in now (`fold_unknown_symbols`, with a test); `SetSymbolLibIds` could only link to a symbol the model's library files hold, not to one drawn in the Symbol Editor.

Verified in the browser pane against a scratch copy of `work/mcu30` (the real board was never touched): View toggles and check marks, the Net Navigator and Highlight Nets, Do not
Populate (red cross, undo), Increment Annotations (C10..C12 to C15..C17 and one undo), the Library Links, Assign Footprints and Bus Syntax Help dialogs, the legacy BOM generator
run (30 components) with Save a copy, Export Symbols with and without the link update (and its undo), the Fields Table's Export tab writing the BOM with kicad-cli (the file
matches the preview), Export Symbol as SVG from the Symbol tab and the Symbol Libraries tree showing the exported library. On a second scratch board made from
`examples/opamp_filter.yaml` (real `Device:R` and `Device:C` symbols): the symbols drawing again, the Do not Populate cross on a real resistor, Compare Symbol with Library (no
differences for R1), the Symbol Checker on `Device:R` (no issues) and the Library Links dialog. Not click-tested: Next / Previous Sheet and Enter Sheet
(the scratch board has no sheets; the page order is unit-tested), the clipboard write (refused by the hidden pane).

## 14. Module sheets: one hierarchical sheet per functional module (`crates/model/src/modules.rs`, `crates/engine/src/hier/`, `crates/engine/src/nets.rs`, `crates/ops/src/sheets.rs`)

KiCad has no command for this (a person draws the hierarchy by hand); it is the studio's own, built on KiCad's way of organizing a design: a root sheet with one sheet symbol
per module, each module on a hierarchical sheet of its own, power as power symbols, everything else crossing between sheets through sheet pins and hierarchical labels. The nets
across sheets are traced the way `connection_graph.cpp` does it (`SCH_SHEET_PATH`, `CONNECTION_SUBGRAPH`), ported from `eeschema/` at 8303b2ad. The one derived schematic of a design is
module sheets now; the old flat sheet is what an existing design has until Tools > Reorganize into Module Sheets.

| Item | Status | KiCad file:function / where |
|---|---|---|
| Module inference | each IC with the passives that only serve it (decoupling on its rails goes to the one IC on all of them, else to "misc"), repeated identical channels as one module (mcu30: the eight GPIO -> R -> LED -> GND), connectors each their own, the rest "misc"; declared modules first (a placement's recorded modules, else the floorplan's partition); readable names ("MCU (U1)", "LED channels (D1-D8)", "Header (J1)"); natural reference order, deterministic. mcu30 comes out as `MCU (U1)` (U1, C1..C12), `LED channels (D1-D8)` (R1..R8, D1..D8) and `Header (J1)` | `crates/model/src/modules.rs::infer_modules`, 9 unit tests |
| Hierarchical derivation | the root has a sheet symbol per module, in columns by role (connector, IC, channels, misc), with the sheet pins of every net that crosses, sorted by the partner they meet, wired between sheet symbols (straight when facing and aligned, else a routed Z with lanes, else a local label); a child sheet has hierarchical labels matching its pins by name. Inside a sheet the IC is centred with its passives beside its pins, a channel is a tidy chain in a row, short nets are wires and the rest labels, power is a power symbol hung off each pin on a two-cell wire (pins side by side on one rail -- U1's two supply pins, its two ground pins -- share one symbol on stubs three cells long, out beyond the part's own texts; one `PWR_FLAG` per rail that needs a source, standing on a pin's end; section 15). Page A4 unless the sheets need more; margins, grid (1.27 mm), the frame and the title block are KiCad's. The flat derivation fits its page too (its first row sat on the border). Tests: every symbol and field box inside the frame, no two symbol boxes overlapping, the sheets trace back to the intent's nets, deterministic, on every example | `crates/engine/src/hier/` (`kit.rs` measures text and pins the way the painter draws them), `crates/engine/src/placed.rs` (one place for a derived pin's position) |
| Reorganize into Module Sheets | Tools menu (`studio.Sheets.reorganize`, `kicad/menuExtras.ts`), one `reorganize_sheets` verb and one undo step: a symbol keeps its reference, library symbol, value, footprint, datasheet, user fields, DNP and exclusion flags and lock; ERC exclusions, the pin map and the title block stay. Refused when the schematic already has sheets, when the design is one module, or when a symbol has several units. The nets drawn are the live ones when the flat drawing agrees with the intent (>= 70 %), else a fresh trace when it agrees >= 90 %, else the intent's: an old drawing that reads as loose pins does not decide | `crates/ops/src/sheets.rs`; `Cmd::ReorganizeSheets` |
| Editing inside a sheet | the sheet in view is part of every command the Schematic tab sends (`Cmd::OnSheet`, `kicad-port/schSheetCmd.ts`): move / rotate / mirror, wires, labels, delete, properties, the drawing tools, the sheet pin tools, attributes, increments, Undo / Redo. All or nothing; a command of another editor ignores the path; a path that has gone stale (an undo took the sheets away) follows the sheet the server shows. A screen shared by two sheet symbols is one screen (KiCad's `SCH_SCREEN`) | `crates/ops/src/lib.rs` (`on_sheet`, `Board.focus`); `kicad-port/schSheetCmd.ts`; `state/store.tsx` (`runCmd`) |
| Navigation | the Hierarchy panel is the tree of every sheet (`GET /api/sch/hierarchy`, read again when the design changes) and a click runs `NavigateTool.changeSheet`, so Back / Forward / Up keep their history; a double-click on a sheet symbol enters it; the sheet in view is marked and the title block names it (`/MCU (U1)/`, its file) | `components/panels/HierarchyPanel.tsx`; `actions/schControlActions.ts` (not duplicated) |
| Nets across sheets | subgraphs by shared point, wire polyline and T-junction; drivers by priority (pin < sheet pin < hierarchical label < local label < power symbol < global label); a local or hierarchical label is a net on its sheet, a global label and a power symbol across all; a sheet pin and the hierarchical label of the same name inside are one net; the best driver names it (global first, then priority, then the sheet nearest the root, then alphabetically); a local name two nets share is told apart by its sheet path (`/b/X`). Not ported: buses are traced as plain wires, a screen placed by two sheet symbols is traced once (KiCad makes a net per placement) | `crates/engine/src/nets.rs::trace_nets_with` <- `connection_graph.cpp::buildItemSubGraphs / ResolveDrivers / compareDrivers / propagateToNeighbors` |
| The net list through an edit | the drawing is traced before and after a command and only the difference is applied to `design.nets` (`nets::follow`): an untouched net keeps its name and pins; a net the edit made is named by its driver, else by the net its pins come from (a wire that joins two nets joins them whole), else the way KiCad names it (`Net-(R1-Pad1)`, `Net-(U1-PA0)`, `unconnected-(R2-Pad1)`) -- never `NET_<n>`. A symbol that only moves or turns (M, G, R, X, Y) is layout and never rewrites the list (`Cmd::edits_connectivity`); the drawing is what ERC judges. Proven on a copy of the board that broke (`crates/cli/tests/fixtures/mcu30_legacy_flat.design.json`, `work/mcu30` before the drag that turned its nets into 74 loose pins): the exact drag leaves the list as it was, every one of the 30 symbols dragged / moved / turned / mirrored does the same, on that board and on mcu30 as module sheets, every routed net is still a net; a wire joining PA0 and PA2 joins them whole and nothing else moves | `crates/engine/src/nets.rs::follow`; `crates/cli/src/board.rs` (`reconcile_schematic` and 5 tests); `sch_pin.cpp::GetDefaultNetName`, `connection_graph.cpp::compareDrivers` |
| kicad-cli round trip | the exported `.kicad_sch` tree is the root and one file per sheet, each sheet symbol's instance path `/<root>/<sheet>` with its page (the one the user set, else its place in the hierarchy), each sheet pin's angle the side of the border it sits on (KiCad moves a pin to the edge its angle names: a pin on the left written as 0 jumped to the right and 34 ERC findings followed). `kicad-cli sch erc` on mcu30 as module sheets: no findings at all. On every example, module sheets: only each design's own `pin_to_pin` findings (l2, l3, l4), no hierarchy mismatch, no unconnected sheet pin, no dangling label | `crates/kicad/src/lib.rs` (`export_kicad_sch_tree`), `crates/kicad-engine/tests/hier_erc.rs` |

The tools that look across the design: a reference is one part for the whole design, so Add Symbol and Rename refuse a reference another sheet has, Annotate and Increment Annotations number after
the references of every sheet, and the next reference the `A` tool offers counts the design's parts, not only the sheet in view (`Board::symbols_elsewhere`, `kicad-port/nextReference.ts`); the Symbol Fields Table and the BOM
preview list the symbols of every sheet and Apply sends each edit to the sheet that has the symbol (`fields_table::project_view`, `set_symbol_fields`); the symbol chooser's library list reads every sheet; Find searches the sheet in view,
where Replace edits (`sheet` in `POST /api/sch/find`); a board with no stored schematic has the derived one in these dialogs too (`sch_api::loaded`). The file the BOM writes and the netlist and plots are kicad-cli's, over the whole tree.

Limits, recorded rather than hidden: a symbol placed with Add Symbol (`A`) puts its box corner at the click, not its origin, so the ghost the tool shows is off by the symbol's offset (the ops
tests pin those positions, so the placement verb is not changed); the layout never uses a quarter turn (rotation 0 and 180 only); Lint (`check_schematic`) and the SVG render (`render_schematic`) still read the root
sheet only, so on module sheets they see the sheet symbols and the wires between them, not what is inside a sheet; Find does not search the other sheets (KiCad's "all sheets" scope), and the attribute, library-link and
Update / Change Symbol tools act on the sheet in view; an old flat drawing keeps its labels off the pin tips (the engine drew them on the box edge then), so
the Properties panel shows "Nets -" for a symbol whose drawing connects nothing even though the board's net list is intact (Reorganize redraws it); a symbol moved without its wires stays on its net in the list while the
drawing shows it unwired, and ERC says so.

Verified in the browser pane against scratch copies of `work/mcu30` (the real board was never touched), the second one made from the board as it was before the drag that broke it: Tools > Reorganize into Module Sheets (the
flat sheet crammed top-left and over the border becomes a root with three sheet symbols, centred inside the frame, the pins wired between them); the Hierarchy panel listing Root and the three sheets and opening each (the
title block follows); the MCU sheet (U1 centred, one supply symbol over its two top pins and one ground symbol under its two bottom pins, the twelve capacitors in a row below with their power symbols), the LED channels sheet (eight
tidy columns, a hierarchical label over each resistor, a ground symbol at each LED) and the header sheet (J1 with its RESET and SWD labels; the painter's `Pin_1`... names are written over its supply and ground symbols, a limit
recorded above); a double-click on a sheet symbol; Navigate Up; a drag and a rotate on a nested sheet (the activity log says `on-sheet "<id>" schematic drag R1 ...`), the stored net list unchanged (20 nets, GND 23 pins, VDD 15, every
name the intent's) and the PCB tab still showing R1 on PA0 / LED1; Undo twice (the sheet as it was), and a third Undo taking the sheets away (the view follows to the root, which is the flat sheet again, the Hierarchy panel back to
Root alone); the Symbol Fields Table opened from the MCU sheet listing every symbol of the design (C1-C12, D1-D8, J1, R1-R8, U1 grouped) and an Apply of a new resistor value, made while the MCU sheet is in view, changing R1..R8 on the
LED channels sheet. Not click-tested: Find on a nested sheet (the endpoint is tested), Place Pins from Sheet on a nested sheet.

## 15. The clipboard: Cut, Copy, Paste, Paste Special, Duplicate (`sch_editor_control.cpp::doCopy / Cut / Copy / Paste / Duplicate`, `sch_io_kicad_sexpr.cpp::Format( SCH_SELECTION* )`, `sch_io_kicad_sexpr_parser.cpp::ParseSchematic( aIsCopyableOnly )`, `sch_reference_list.cpp`, `dialog_paste_special.cpp`)

The Schematic tab had no clipboard (`GAPS.md` item 6). It has KiCad's now, including its format: what Copy puts on the system clipboard is what `SCH_IO_KICAD_SEXPR::Format( SCH_SELECTION*, ..., aForClipboard )` writes
(`(lib_symbols ...)`, then the selected items, flat, no `(kicad_sch ...)` around them), so real KiCad pastes it, and a fragment real KiCad copied pastes here. Ported from `eeschema/` at 8303b2ad; each
function names the KiCad one it follows.

| Item | Status | KiCad file:function / where |
|---|---|---|
| Copy (Ctrl+C) | **identical** for symbols (every placed unit of a reference), power symbols, wires, buses, junctions, no-connects, bus entries, labels (local, global, hierarchical), texts, graphic lines and drawn shapes, text boxes, rule areas and directive labels: `RequestSelection` (the selection, else the item under the cursor), the text onto the system clipboard (`navigator.clipboard`; where the browser refuses a read, the last copy made here is the fallback). The library symbol of each copied symbol goes in `(lib_symbols ...)` under its full `lib_id`; a generated box is written as the library symbol it draws as. A derived sheet places a symbol by the corner of its box, KiCad by the library origin: the writer moves each symbol to KiCad's origin. A polyline is one `(wire ...)` per segment (KiCad has no other), plus the junction dots the wires imply. Fields are put beside the symbol and the symbol marked `(fields_autoplaced yes)` (the IR has no field positions). Not copied: hierarchical sheets (listed in the toast), tables, images, groups, a part's MPN / LCSC from the intent | `SCH_EDITOR_CONTROL::doCopy`, `SCH_IO_KICAD_SEXPR::Format( SCH_SELECTION* )`, `saveSymbol( aForClipboard )`; `crates/kicad/src/sch_clipboard.rs::write_clipboard`, `POST /api/sch/clipboard/copy`, `actions/schClipboardActions.ts` |
| Cut (Ctrl+X) | **identical**: Copy, and only when that worked the delete of the selection as one undo step; locked items are copied and stay (`DoDelete`) | `SCH_EDITOR_CONTROL::Cut`; `kicad-port/schDelete.ts` |
| Paste (Ctrl+V) | **identical** in what it does: the clipboard is read (KiCad's text from KiCad, or from this studio), what it would add is drawn following the cursor, selected, anchored by the leftmost then topmost connectable item (`GetTopLeftItem`), grid-snapped; a click drops it (one `paste_sch` command, one undo step) and the new items stay selected; Escape throws it away, so does leaving the sheet or arming another tool. **partial** in how: the studio asks the server what the paste would add (the paste is tried on a copy of the design) instead of putting the items on the sheet and hiding them; a right-click while carrying does nothing (KiCad opens the move tool's menu). Text that is not a schematic fragment is pasted as a text item at the origin, a long one (> 100 characters, `m_MaxPastedTextLength`) after a confirmation; a clipboard that holds a whole `.kicad_sch` file is text, as in KiCad (`Unexpected( T_kicad_sch )`) | `SCH_EDITOR_CONTROL::Paste`, `SCH_SELECTION::GetTopLeftItem`, `SCH_MOVE_TOOL::Main`; `crates/kicad/src/sch_clipboard.rs::parse_clipboard`, `POST /api/sch/clipboard/parse`, `kicad-port/schClipboard.ts`, `state/schPasteStore.ts`, `components/SchematicView.tsx` |
| What a paste reads | the top-level forms `ParseSchematic( aIsCopyableOnly )` takes and no others; the items through the `.kicad_sch` importer, the library symbols through the Symbol Editor's reader (hidden pins, fills, units kept); a symbol whose library symbol the clipboard lacks is left out and said so. Segments that meet at a plain bend (two wire ends, nothing else there) are one polyline again, so a copy of this studio's own wires pastes as the wires it copied. Locks are dropped (`SetLocked( false )`), every item gets a new id | `crates/kicad/src/sch_clipboard.rs` |
| Library symbols | the design's own library symbol wins and the fragment's comes in only for a name the design lacks (`currentScreen->GetLibSymbols()` before `tempScreen`'s): it is published into the project's symbol library, so the part it adds draws its pins from it; a generated box gets the name `clipboard:<ref>` (and boxes that draw alike share one) because the design has no part for the copy yet | `SCH_EDITOR_CONTROL::Paste` (the `libSymbol` block); `crates/ops/src/sch_clipboard.rs` |
| Reference designators | **identical** numbering for the three modes: Unique (the default, `annotation.automatic`) keeps a reference nothing else has and numbers a taken one with the first free number from its own, left to right within a prefix (`ReannotateDuplicates`, `FindFirstUnusedReference`); the units of one multi-unit part pasted together share a number, and a lone unit the part lacks joins it (`areUnitsAvailable`); power symbols are always numbered anew, `#PWR` with a leading zero (`formatRefStr`). Unique across every sheet: the numbering sees every sheet of the design. **partial**: a design here names a part by its reference, so Keep cannot make a duplicate (a taken reference is numbered as in Unique) and Clear numbers at once from 1 instead of leaving `R?` | `eda_model::sch_clipboard::annotate_paste` (11 tests); `SCH_REFERENCE_LIST::Annotate`, `REFDES_TRACKER` |
| Paste Special (Ctrl+Shift+V) | **identical**: `DIALOG_PASTE_SPECIAL`'s "Reference Designators" box with its three choices (the "Clear net assignments" box is the board editor's and the schematic's Paste does not read it, so it is not offered); the tab's Escape closes it | `components/SchPasteSpecialDialog.tsx`; `dialog_paste_special.cpp` |
| Duplicate (Ctrl+D) | **identical** in effect: a copy into a buffer of its own (the system clipboard is left alone), pasted at once, carried by the connection point nearest the cursor, or a symbol's origin, so it appears in place and follows the cursor (`m_duplicateClipboard`, the `eventPos` block of `Paste`) | `SCH_EDITOR_CONTROL::Duplicate`; `eda_model::sch_clipboard::closest_anchor` |
| Across sheets | a paste goes to the sheet in view (`Cmd::OnSheet`, the same as every schematic command), a copy can be pasted on any other sheet or design, and the numbers stay unique across all of them | `crates/ops/src/sch_clipboard.rs` (`used_references`) |
| One undo step | a paste is one command (`Cmd::PasteSch`): the library symbols it published stay in the project library after an undo (harmless: nothing draws from them) | `crates/ops/src/lib.rs` |
| Context menu | the clipboard group of `SCH_SELECTION_TOOL`'s menu: Cut, Copy (a selection), Paste, Paste Special, Delete, Duplicate (a selection); Copy as Text is not ported | `kicad-port/schContextMenu.ts` |

Tests: `crates/model/src/sch_clipboard.rs` (numbering, anchors, orientation: 11), `crates/kicad/src/sch_clipboard.rs` (a fragment as KiCad writes it, text that is not one, the bend join: 3),
`crates/kicad/tests/sch_clipboard.rs` (what a copy writes loads in real KiCad and gives the same nets, `EDA_SLOW_TESTS=1`; forms taken out of eight QA schematics read as the files do),
`crates/ops/src/sch_clipboard_tests.rs` (the verb: every item kind, the corner placement, unique across sheets, the three modes, units, library symbols: 11),
`crates/cli/src/sch_clipboard_api.rs` (copy, read back, paste with an offset: the same schematic, positions exact, one undo step; text; a second sheet: 2), `kicad-port/schClipboard.test.ts` and `schContextMenu.test.ts`.

Verified in the browser pane against a scratch copy of `work/mcu30` (the real board was never touched): select all, Ctrl+C, Ctrl+V (the ghost follows the cursor, drawn selected), a click (60 symbols, 60 unique references, the 30 new parts unplaced on the
PCB tab, the pasted items selected), Undo (back to 30 in one step), Redo, Undo; Tools > Reorganize into Module Sheets, copy the MCU sheet, paste it on the LED sheet (U1 -> U2, C1..C12 -> C13..C24, 43 symbols, all unique across the
sheets); Ctrl+D (the copy appears at the cursor, Escape throws it away); Ctrl+Shift+V with "Clear reference designators" (U3, C25..C36, the first free numbers); Ctrl+X then Ctrl+V (the symbols come back with the
references they had, since nothing holds them); the Cmd+C / Cmd+V keys; the right-click menu's clipboard group. The pane refuses the page reading the system clipboard ("Read permission denied"), so these ran on the fallback (the studio's own last copy);
what real KiCad writes and reads was checked with kicad-cli instead of the KiCad window.

Limits, recorded rather than hidden: hierarchical sheets are not copied or pasted; a field's position and size and a label's rotation, size and justification are not in the IR (`GAPS.md` item 12), so a copy carries none and
a paste from KiCad drops them; a wire's stroke and a junction's size are the defaults; a symbol drawn with an alternate body style shows its normal one (the engine's library symbol has one); a pasted symbol is a new part, unplaced on the PCB.

## 16. Derived schematics have no overlapping text or symbols (`crates/kicad/src/sch_overlap.rs`, `crates/engine/src/{fields,flat,obstacles}.rs`, `crates/engine/src/hier/`, `crates/model/src/{gensym,kicad_geom,kicad_font}.rs`)

What a derivation draws, module sheets and the flat sheet alike, overlaps nothing else as KiCad draws it. "As KiCad draws it" is measured, not estimated: the stroke font's advances
(`kicad_font.rs`, `EDA_TEXT::GetTextBox`: the sum of the advances less 0.2 of the size plus the pen, 17 % of height for the descenders), a label's box lifted by its text offset (`SCH_LABEL::GetBodyBoundingBox`),
a hierarchical label's flag (`SCH_HIERLABEL`), a pin's name and number where `PIN_LAYOUT_CACHE` puts them (8 mil clearance, the 6 mil pen), a field as `SCH_FIELD::GetBoundingBox` turns it through the symbol's transform
(`kicad_geom.rs`, 6 unit tests, `kicad_font.rs`). `eda_kicad::sch_overlap` reads a written `.kicad_sch` and checks every pair of drawn items (bodies, pin lines, pin names and numbers, fields, labels,
power symbols with their Value, sheet symbols and pins, wires, junctions, no-connect flags; and the frame and the title block); wires and pins that meet at a point, a pin's own texts inside its body, a power symbol's Value against its glyph and a flag on a
wire are intended contacts, boxes that go into one another by less than a pen (0.1 mm) touch, and the pins and pin texts of a symbol KiCad's library draws are its authors' (the AMS1117's names touch in KiCad's own drawing). The sweep test
(`crates/kicad-engine/tests/sch_overlap.rs`) runs it on every example, `examples/ladder/*` and mcu30 included, flat and as module sheets, and asserts none. `EDA_SCH_DUMP=<dir>` leaves the sheets to render with kicad-cli.

| Item | Status | KiCad file:function / where |
|---|---|---|
| Fields | every symbol, power symbol and sheet keeps where its fields are (`SchematicSection::field_layout`, `GAPS.md` item 12): Autoplace Fields' arithmetic in schematic units with C++'s truncating division (the preferred sides, the side with no pins or the fewest, the 25 x 15 mil padding, the 50 mil grid), then, where the fields would run over something drawn, the other sides and further out; a power symbol turned to run along a row of pins reads its Value along the row, past the glyph's tip. Written, sent and drawn where they are | `autoplace_fields.cpp::AUTOPLACER`, `sch_field.cpp`; `crates/engine/src/fields.rs`, `obstacles.rs` |
| Generated symbols | a part with no library symbol (and whose pins the generic Device and Connector_Generic symbols cannot take: a USB-C receptacle's A1..B12, a diode numbered A/K) gets `gen:<ref>`: pins 2.54 mm long on a 2.54 mm pitch, every connection point on the 1.27 mm grid, 1.27 mm text, names inside at 1.016 mm, a body wide enough for the longest names (measured with the stroke font), power on top, grounds below, inputs left, outputs right, no-connect last. Designs from before keep `eda:<ref>` and the box they were drawn with; a library symbol that lacks a pin the part has (an `nc` pin) is not drawn under its name with a box of ours | `crates/model/src/gensym.rs`, `ConstraintModel::lib_id_of` |
| Labels and power symbols | hung off the pin on a two-cell wire, outward, the glyph and the text running on away from the part (a label's spin is the way its wire arrives); pins of a net side by side on one part share one (stubs, a join, one symbol or a label on a tail); a flag stands on a pin's end and what shares the pin is three cells off; what hangs on a pin keeps clear of the parts, the wires and what hangs on the other pins: longer, else out and along the row of pins either way. Rails first, so a label moves, not a power symbol | `crates/engine/src/hier/items.rs` (`hang_route`, `rail_pins`, `label_run`) |
| The flat derivation | the same emitters hang its nets; a layout with no room for them is laid out again with more between its parts (0, 3, 6, 10, 14, 20, 28 cells), the first that places everything and still fits the largest paper; a routed wire that runs through the end of a pin that is not on its net, or is in the way of what hangs on a pin, is drawn with labels instead; the fields are placed after a first fit (Autoplace Fields rounds on the sheet's own coordinates, and C++'s truncating division is not the same either side of zero) and the paper is fitted again to hold them | `crates/engine/src/flat.rs`, `lib.rs::derive_schematic` |
| Pin texts in the studio | the server sends each library symbol's `pin_names_hidden`, `pin_numbers_hidden` and `pin_name_offset`; the painter writes names inside the body (offset above zero) or over the pin line (zero, then the number under it), numbers over the line, 1.27 mm text, a no-connect pin's included. The same cases are pinned in Rust (`kicad_geom`) and TypeScript (`pinText.test.ts`) | `pin_layout_cache.cpp::GetPinNameInfo / GetPinNumberInfo`; `components/schematic/pinText.ts` |
| kicad-cli | `kicad-cli sch erc` on every example, flat and as module sheets: no finding but each design's own `pin_to_pin` ones (l2, l3, l4), the same flat and as module sheets | `crates/kicad-engine/tests/hier_erc.rs` (`-- --ignored --nocapture`) |

Findings the sweep counted before this work, flat / module sheets: all_power_ground_net 46 / 49, dense_small_outline 36 / 30, l1_usb_mcu 2147 / 957, l2_sensor_hub 5298 / 1414, l3_motor_hub 15262 / 2238, l4_control_hub 37401 / 3617, ldo 142 / 140,
ldo_proximity_heavy 217 / 167, mcu_board_30plus 3100 / 1536, mixed_track_widths 142 / 140, nc_pins 67 / 68, opamp_filter 153 / 168, passive_divider_ladder 91 / 86, star_net 51 / 42, through_hole_headers 104 / 94, two_pin_nets 14 / 12,
unroutable_tiny_outline 56 / 56. Now none.

Limits, recorded rather than hidden: what the user moves is theirs (a symbol dragged on top of another overlaps it, as in KiCad); a stored design keeps what it was drawn with until Tools > Reorganize into Module Sheets
(its `eda:` symbols, its labels on the pin tips, its fields at the page corner until the first derivation stores them); the flat derivation is bigger than before where it needed room, and a design that needs more than the largest paper would leave something
overlapping (none of the examples does); the old SVG renderer (`eda-render`, the lint's text gates) places a field where the section keeps it but still draws its own boxes for the symbols; a field's place cannot be edited yet (`GAPS.md` item 12); the overlap
sweep reads the written file, so what only the studio's painter draws (selection halos, the grid) is not in it.

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
    shape in isolation, but only a real board proves `kicad-cli sch erc` actually
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
