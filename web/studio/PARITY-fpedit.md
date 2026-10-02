# Footprint Editor parity with KiCad pcbnew's footprint editor

One row per footprint-editor action, its status, and the KiCad file it
came from. Follows the same status vocabulary `PARITY-pcb.md`/
`PARITY-sch.md` use: **identical** (same logic/constants, browser-platform
adaptations noted inline), **partial** (core behavior ported, a real gap
noted), **missing** (not started).

KiCad source snapshot: the read-only checkout at the path the task gave
this session (`pcbnew/` as of its own commit; this file does not re-derive
that commit hash -- see `PARITY-pcb.md`'s own header for how the project
usually records it, not repeated here since this session's snapshot path
was scratch, not the tracked mirror).

Ported logic lives in `web/studio/src/kicad-port/padNumbering.ts` (unit
tested, `npm run test:unit`) and `crates/model/src/ir.rs` (Rust unit
tests, `cargo test -p eda-model`) -- the two are deliberately the same
algorithm so a client-side preview can never disagree with the backend's
authoritative answer; see `padNumbering.ts`'s own header comment.

## 1. Opening a footprint (step 1)

| Behavior | Status | KiCad file:function |
|---|---|---|
| New "Footprint" tab next to PCB / Schematic / 3D | identical | `EditorTabs.tsx` -- real KiCad opens a separate top-level frame instead (`FOOTPRINT_EDIT_FRAME`); this app's whole UI is one window with tabs, same simplification every other editor here already makes |
| Ctrl+E ("Edit Footprint") on a selected board part | identical in effect, simplified underneath | `pcbnew/tools/footprint_editor_control.cpp` / `load_select_footprint.cpp:LoadFootprintFromBoard` (`actions/useFootprintEditHotkey.ts`). Source clones the board footprint with a `SetLink` back-reference so Save writes straight back to that one instance with no library touched; this app's model has no such live link -- editing and saving (`Cmd`s) always go to the named `design.footprint_library` entry, and reaching the board at all needs the separate, explicit `Update Footprint on Board` action (see step 6) |
| Open from "the libraries we load" | partial | `pcbnew/tools/footprint_editor_control.cpp:LoadFootprintFromLibrary`. This app's picker (`OpenFootprintPicker` in `FootprintEditorView.tsx`) lists `GET /api/footprint_library`'s names -- already-opened library entries plus whatever `ConstraintModel::footprints` resolved (real `.kicad_mod` files under `EDA_KICAD_FOOTPRINTS`, or intent-declared) -- not a full library-tree browser across every installed `.pretty` directory (GAPS.md's own "Footprint Chooser" honorable mention, out of scope) |
| New Footprint (blank) -- `Ctrl+N` / toolbar "New" | identical in effect; own verb since the hotkey sweep (section 7) | `footprint_editor_control.cpp:NewFootprint` -> `CreateNewFootprint`: an empty SMD footprint named "Untitled", made unique with `_1`, `_2`... `Cmd::NewFootprint { name }` refuses a name already taken; `Cmd::OpenFootprintForEdit` on a name that resolves to nothing still starts blank |
| Footprint drawn at its own anchor/origin | identical | `footprint_edit_frame.cpp`. `footprintPainter.ts`'s `drawAnchor` draws the small crosshair at local (0,0); `LibraryFootprint.anchor` always `{0,0}` in practice (`SetFootprintAnchor` normalizes by translating everything, see step 1's own Cmd doc) rather than keeping a persistent non-zero offset, a deliberate simplification of `MoveAnchorPosition`'s board-relative version (this editor has no board position for the anchor to stay fixed against) |
| Footprint editor's own layer set | partial | `footprint_edit_frame.cpp` edits a fresh standalone `BOARD()` with the board's full default layer table (no reduced fp-editor-specific set was found in source). This app's footprint canvas only ever shows F.Cu (pads), F.SilkS/F.Fab/F.CrtYd (graphics/text) -- no inner/adhesive/other-technical layers, since the graphics toolbar only offers those three (`GRAPHIC_LAYERS` in `FootprintEditorView.tsx`) |

## 2. Pad tool (step 2)

| Behavior | Status | KiCad file:function |
|---|---|---|
| Place a pad, auto-incremented number | identical algorithm | `pad_tool.cpp`'s `PAD_PLACER`/`FOOTPRINT::GetNextPadNumber` (`footprint.cpp:3357`): split the last number into prefix + trailing integer, increment past any collision. Ported twice, byte-for-byte the same algorithm: `crates/model/src/ir.rs`'s `next_pad_number_after`/`next_pad_number` (backend, authoritative) and `kicad-port/padNumbering.ts` (frontend preview). Source's own "last placed" session memory (`m_lastPadNumber`) is approximated by seeding from the footprint's own highest-numbered pad instead, since this editor has no interactive-session state to carry between separate `Cmd` round-trips |
| Cycle pad shape while placing | n/a | confirmed absent in source itself (no such hotkey/behavior exists in `pad_tool.cpp` or `pcb_actions.cpp`) -- nothing to port |
| Move / Rotate / Delete a pad | identical | generic `EDIT_TOOL`, not pad-specific in source either. `Cmd::MovePad`/`RotatePad`/`DeletePad`; rotation is a fixed quarter-turn (this app's own R/Shift+R convention everywhere) rather than source's configurable `PCBNEW_SETTINGS::m_RotationAngle` |
| Renumber Pads (Enumerate) | **partial, documented simplification** | `pad_tool.cpp`'s `EnumeratePads`: numbers pads in **click/drag order** via an interactive picker, with per-pad undo-on-reclick and a start/prefix/step dialog (`DIALOG_ENUM_PADS`). This app's `Cmd::RenumberPads` has the same start/prefix/step options but orders pads by **position** (top-to-bottom, then left-to-right -- reading order, same sort `Annotate` already uses for symbols) instead of a live click sequence, since building that exact interactive picker was out of this session's time budget. `renumber_pads_orders_by_position_not_by_current_number` (ops/tests.rs) locks in the documented behavior |
| Push Pad Properties | partial | `pad_tool.cpp`'s `doPushPadProperties`: source pushes to every *other footprint on the board* sharing the same library FPID, each of 4 filters (shape/orientation/layers/type) independently gating which of *their* pads get touched. This editor only ever has one footprint open at a time (no board-wide multi-footprint concept here), so `Cmd::PushPadProperties` pushes to every other pad *within the same open footprint* instead -- the per-pad filter logic itself (`push_pad_properties_only_touches_filtered_matches` test) is a direct, unmodified port of source's own per-pad matching rules |
| Copy / Paste Pad Properties | identical in effect, client-side | `pad_tool.cpp`'s `copyPadSettings`/`pastePadProperties` (via the global `m_Pad_Master`). This app has no persistent cross-session master pad; `copyPadProperties`/`pastePadProperties` (`footprintEditorStore.tsx`) hold the copied fields in local React state instead, applied to the current selection via `edit_pad` on Paste -- same field scope (shape/size/drill/layers/overrides, never number/position/rotation) |
| Explode/Recombine (custom pad shape editing) | missing | `pad_tool.cpp`'s `EditPad`. No custom (primitive-based) pad shape at all -- see `LibraryPadShape`'s own doc in `ir.rs`. A `.kicad_mod` with a custom-shape pad loads (the anchor pad shape/size survive; the primitives are dropped) but cannot be authored here |

## 3. Pad Properties dialog (step 3)

| Behavior | Status | KiCad file:function |
|---|---|---|
| Shape: circle/rect/oval/round rect/trapezoid/chamfered rect | identical set, minus source's two custom-anchor variants | `dialog_pad_properties.cpp`'s `CODE_CHOICE` enum. `LibraryPadShape` (`ir.rs`) -- the two `CHOICE_CUSTOM_*_ANCHOR` entries (a true primitive-based custom shape) are out of scope, same reasoning as Explode/Recombine above |
| Size, offset, rotation | identical | `PadPropertiesDialog.tsx`'s own form; `LibraryPad.offset` maps to `PAD::SetOffset` exactly (copper shape translated from the drill/anchor point, in the pad's own rotated frame -- see `footprintPainter.ts:drawPad`'s nested-transform comment) |
| Drill: round or slot | identical | `dialog_pad_properties.cpp`'s drill radio; `LibraryPad.drill`/`drill_slot`, mutually exclusive, same as the engine's own `Pad` |
| Layers: SMD / connector (no paste) / THT / NPTH presets | partial, by design | `pad.cpp`'s static masks (`PTHMask`/`SMDMask`/`ConnSMDMask`/`UnplatedHoleMask`) as four preset buttons (`LAYER_PRESETS` in `PadPropertiesDialog.tsx`), not a full per-layer checkbox grid -- this task's own scope says "layers (SMD/THT/NPTH/connector presets)", so the fully general per-layer picker (source's own Layers tab, every tech layer individually) was not built |
| Roundrect ratio / chamfer ratio+corners / trapezoid delta+axis | identical fields, approximate rendering for the latter two | `dialog_pad_properties.cpp`. Roundrect ratio is exact (same formula the engine's own `Footprint::courtyard_half`-adjacent code already uses elsewhere). Chamfer and trapezoid are drawn correctly on the canvas (`footprintPainter.ts`'s `chamferedRectPath`/`trapezoidPath`) but this session could not verify KiCad's *exact* corner-cut/delta formula against source closely enough to be certain the numbers are pixel-identical -- a reasonable, visually-correct approximation, not a verified byte-exact port |
| Clearance / thermal gap / thermal spoke width overrides | partial | `dialog_pad_properties.cpp`'s "Clearance Overrides and Settings" panel. Fields exist and round-trip through the JSON/Cmd layer (`clearance_override`/`thermal_gap_override`/`thermal_spoke_width_override`); **not yet consumed by any check** (no DRC/zone-fill provider in this codebase reads a *footprint-library* pad's override -- only the engine's board-level `Pad` would need to, and this editor's pads don't feed that path until `Update Footprint on Board`, which does not currently thread per-pad overrides through either). Tracked as a real gap, not silently dropped |
| Fabrication property (BGA/fiducial/testpoint/...) | missing | `dialog_pad_properties.cpp`'s `m_choiceFabProperty`. Cosmetic/DRC-hint metadata with no consumer anywhere in this app yet; not modeled |
| Padstack mode (front/inner/back differ) | n/a | this app's board has no inner-copper-layer concept for a pad to differ across; source's own mode is PTH-only and collapses to "Normal" otherwise |
| Validation (hole-leaves-no-copper, SMD-has-no-drill, etc.) | identical, reused not reimplemented | `pad.cpp`'s `CheckPad`/`doCheckPad`. `add_pad`/`edit_pad` (`crates/ops/src/lib.rs`) build a one-pad probe `Footprint` and call its existing, already-tested `validate()` rather than re-deriving the rules (`add_pad_reuses_footprint_validate_for_a_through_hole_with_no_drill` test) |

## 4. Graphics and text (step 4)

| Behavior | Status | KiCad file:function |
|---|---|---|
| Line / Arc / Rect / Circle on F.SilkS / F.Fab / F.CrtYd | identical | `pcbnew/tools/drawing_tool.cpp` family, reusing the exact `Shape`/`CmdShape` the PCB tab's own drawing tools already use (`Cmd::AddFootprintGraphic` mirrors `AddShape` field-for-field). The arc tool is `DrawArc`'s centre -> start -> end with `/` (arcPosture) flipping the direction (`kicad-port/arcGeom.ts`, shared with the board tab) |
| Bezier curve (`Ctrl+Shift+B`, toolbar) | identical | `DrawBezier`/`BEZIER_GEOM_MANAGER` (`kicad-port/bezierGeom.ts`, shared with the board tab); stored as `Shape::Bezier`, written as `fp_curve` in the derived `.kicad_mod` and read back from one |
| Text | identical | reuses `Text`/`CmdText`, same as above (`AddFootprintText` mirrors `AddText`) |
| Polygon | identical | click-to-add-points, Enter/double-click/the same `finishDraw` to close it -- same convention the PCB tab's own zone/polygon tools use. No fixed point count (`AUTO_FINISH` has no `polygon` entry, so it never auto-finishes) |
| Move / Edit / Delete a graphic or text item | identical | `Cmd::MoveFootprintGraphic`/`EditFootprintGraphic`/`DeleteFootprintGraphic` and the `*Text` equivalents, same shape as the PCB tab's |

## 5. Footprint Properties dialog (step 5)

| Behavior | Status | KiCad file:function |
|---|---|---|
| Name / description / keywords | partial (name is read-only here) | `dialog_footprint_properties_fp_editor.cpp`'s General tab. Renaming a footprint (and updating every board instance's `Part::footprint` string to match) has no `Cmd` yet -- `FootprintLibraryPropertiesDialog.tsx` shows the name but does not let it be edited |
| Attributes (SMD/THT/exclude from BOM/position files/board only/DNP/allow missing courtyard/allow solder mask bridges) | identical | `FOOTPRINT_ATTR_T` (`footprint.h:83-91`) plus the dialog's two related non-attribute-bit checkboxes -- all eight map onto `FootprintAttributes` one field each |
| 3D model path | partial | one path, a plain text field (`FootprintLibraryPropertiesDialog.tsx`), not the full `PANEL_FP_PROPERTIES_3D_MODEL` list (multiple models, each with its own offset/scale/rotation/show toggle) -- `LibraryFootprint.model` is a single `Option<String>`, same shape the engine's own `Footprint::model` already has |
| Reference / Value field visibility | partial | source positions and styles these as real text items on the footprint (`PCB_FIELD`s with their own at/layer/size/orientation); this app only models whether each is shown at all (`reference_visible`/`value_visible`), not where or how |
| Fields grid (custom fields beyond Reference/Value) | missing (modeled, no UI) | `dialog_footprint_properties_fp_editor.cpp`'s Fields grid (`PCB_FIELDS_GRID_TABLE`). `FootprintField` exists in the IR (`ir.rs`) and round-trips through JSON, but no dialog edits it yet |
| Layers tab (private/custom user layers) | missing | this app's board has a fixed layer set; no private-layer concept to assign from |
| Clearances tab | missing, duplicate of pad-level overrides | same fields as the Pad dialog's own overrides, at the footprint level instead -- not built; see step 3's own overrides row for the one place this app does have the concept |
| Net-tie / jumper pad groups | n/a | no net-tie concept in this model |

## 6. Save, export, update (step 6)

| Behavior | Status | KiCad file:function |
|---|---|---|
| Save to the project library | identical, and automatic | every `Cmd` here persists through the normal `board::step` -> `design.json` write, same as every other editor in this app -- there is no separate "Save" action needed or offered, consistent with the rest of the app's own convention (tracks/zones/etc. have no Save button either) |
| Update Footprint on Board | identical in effect, different mechanism | `pcbnew/tools/footprint_editor_control.cpp` (board-linked `SaveFootprint` -> `SaveFootprintToBoard`) re-clones the edited footprint onto the one linked board instance by UUID. This app's board instances have no stored per-instance pad copy at all (`FootprintInstance` is placement-only; pads are always resolved fresh from `ConstraintModel::footprint_of` at render/export/DRC time) -- `Cmd::UpdateFootprintOnBoard` instead flips `LibraryFootprint.published`, which `crate::board::load`'s overlay then merges into `model.footprints` for *every* instance naming that name (closer to KiCad's bulk "Update Footprints from Library" than its per-instance version -- this app has no per-instance choice). Explicit either way: editing pads alone never reaches the board (`update_footprint_on_board_only_flips_the_explicit_flag` test) |
| Export derived `.kicad_mod` | partial | `crates/kicad/src/footprint_lib.rs`'s `export_kicad_mod`, `GET /api/footprint/export?name=...`, a browser download via the toolbar's "Export .kicad_mod" button. Round-trips through this crate's own reader (`export_kicad_mod_round_trips_through_this_crates_own_reader` test) for circle/rect/oval/round_rect pads, graphics, text, courtyard and the 3D model path. **Known gaps, deliberately not guessed at**: a trapezoid/chamfered-rect pad exports as its `to_engine_pad` approximation (plain rect / roundrect) rather than KiCad's real trapezoid/chamfer pad fields (`export_kicad_mod_approximates_trapezoid_and_chamfered_pads_as_valid_kicad_shapes` locks this in as a *valid*, loadable file rather than a byte-exact one); per-pad thermal-gap/spoke-width overrides and the footprint's own `allow_soldermask_bridges` attribute are not written at all, since this session could not verify their exact on-disk token from the available source read. Never round-tripped against a real `kicad-cli`/KiCad install (none was available) -- verification is this crate's own reader only |
| Rename a footprint | missing | no `Cmd` renames `design.footprint_library`'s key (or updates `Part::footprint` strings pointing at the old name) |
| Delete a footprint definition | identical | `Cmd::DeleteLibraryFootprint`, exposed as a button in `FootprintLibraryPropertiesDialog.tsx` |

## 7. Hotkey sweep: New, Set Anchor, Duplicate and Increment

Four of the 14 pcbnew hotkeyed-and-missing actions (`PARITY-pcb.md` section 20)
live in this editor. All four are gated to the Footprint tab by name in
`kicad-port/actionTabGate.ts` (KiCad's footprint editor is its own frame but
its actions keep the `pcbnew.` prefix).

| Behavior | Status | KiCad file:function |
|---|---|---|
| `Ctrl+N` New Footprint | **done** | `footprint_libraries_utils.cpp:CreateNewFootprint`, `footprint_editor_control.cpp:NewFootprint`. "Untitled", then `Untitled_1`... until unused (`kicad-port/fpEditActions.ts:uniqueFootprintName`, checked against the library names), attribute SMD; the editor switches to it and fits the view. `Cmd::NewFootprint` (FootprintEditor undo domain). No "name your footprint" prompt: source asks for a name when the editor was opened from a library; the library here is the project's own, so the default name stands until renamed (and rename is still missing, section 6) |
| `Ctrl+Shift+N` Set Anchor | **done** | `drawing_tool.cpp:SetAnchor`: arms the anchor tool; one click calls `MoveAnchorPosition` (the pre-existing `Cmd::SetFootprintAnchor`: everything shifts so the clicked point becomes the origin) and the tool pops back to Select. Verified: pads shifted by exactly the click point |
| `Ctrl+Shift+D` Duplicate and Increment | **done**; `Ctrl+D` plain duplicate is the same code with `increment` false | `edit_tool.cpp:Duplicate( increment )`. The selected pads, graphics and texts are copied in place in one batch, the copies are selected and picked up by an armed Move (the next click drops them; Escape undoes the duplicate). With `increment`, a pad that can hold a number (not NPTH: `PAD::CanHaveNumber`) takes `GetNextPadNumber( lastPadNumber )`, seeded from the footprint's highest pad number (`padNumbering.ts:highestPadNumber`) and then from the last placed or duplicated pad (`state.lastPadNumber`). 9 unit tests (`fpEditActions.test.ts`) |

Arc and Bezier tools (`/`, `Ctrl+Shift+B`) are in section 4. **Bug fixed on the
way**: `fetchFootprint` crashed (`graphics is not iterable`) on a footprint with
no graphics, because the IR omits empty lists -- the client now defaults
`pads`, `graphics`, `texts` and `fields` to `[]`.

## Known gaps summary (ranked, highest first)

1. **No manual click-through testing was possible this session** -- the in-app browser pane would not navigate (confirmed, not assumed: a `navigate` call timed out at 300s). Every UI interaction (pad placement, drag-to-move, dialogs opening/submitting, the context menu) is implemented following this codebase's own established Canvas.tsx/ZoneDialog.tsx patterns and is typecheck-clean, but has only been exercised indirectly: via the backend API directly (curl, see the commit log) and via this app's own `node:test` unit tests for the pure-logic pieces (padNumbering.ts). **A real click-through is the single highest-priority remaining item.**
2. **Renumber Pads is click-order in source, position-order here** (see step 2) -- a deliberate scope cut, not an oversight, but the biggest *behavioral* (not just missing-feature) divergence from KiCad in this port.
3. **Pad clearance/thermal overrides are not consumed anywhere** (step 3) -- modeled and saved, but inert until some check reads them.
4. **No footprint rename**, and **no custom (primitive) pad shape** -- both explicitly out of scope per the task's own "if feasible" hedge on the latter.
5. **`.kicad_mod` export has three known, documented fidelity gaps** (trapezoid/chamfer exact shape, two override fields, one attribute) and was never checked against real KiCad -- see step 6.
