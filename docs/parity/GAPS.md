# agentic-eda vs KiCad — Top 30 Gaps

Ranked by user-visible impact (does it block a whole workflow, or shave a corner off one you can already do). UI gaps are grounded in a full read of `web/studio/src`'s action registry, dialogs, and canvas interaction code against `pcbnew/tools/*`, `eeschema/tools/*`, and `common/tool|view/*`.

**DRC/ERC engine parity is closed (2026-10-03).** The Rust DRC and ERC ports this document used to measure (and the gaps #2, #3, #4 and #19-#24 below) are gone: DRC, ERC, plots, Gerbers, drill, position files, netlist, BOM and board statistics all run through kicad-cli on the exported design (`docs/ARCHITECTURE.md`, "Engines"), so there is one answer per question and no parity to measure. What stays measured is the live connectivity port (`docs/parity/REPORT.md`) and the exporter/importer round trip. Our own checks, the ones KiCad does not have, are `crates/lint`.

---

1. **Area**: eeschema — core schematic editing. **Symptom**: the schematic editor is a read-only viewer: no way to place a symbol, draw a wire, add a label, or move anything. Every `eeschema.*` action (240 cataloged) is unwired; `useActionRunner.ts` registers zero of them. Measured UI parity: **~5%**. **Port from**: `eeschema/tools/sch_drawing_tools.cpp` (`PlaceSymbol`), `sch_line_wire_bus_tool.cpp` (`doDrawSegments`/`startSegments`/`finishSegments`), `sch_edit_tool.cpp`, `sch_move_tool.cpp`. **Size**: XL (root gap — the items below decompose it for actual sequencing).

2. **Area**: DRC — the Rust engine hangs on anything but a tiny board. **Closed 2026-10-03.** kicad-cli is the only DRC/ERC engine (`docs/ARCHITECTURE.md`, "Engines"); the Rust port this gap measured is deleted, so there is no engine parity left to chase. (It also no longer matters that the engine's speed was a measurement artefact: kicad-cli runs in about 4 s on a 30-part board and the studio shows a running state.)

3. **Area**: DRC — precision on real boards is very low. **Closed 2026-10-03.** kicad-cli is the only DRC/ERC engine (`docs/ARCHITECTURE.md`, "Engines"); the Rust port this gap measured is deleted, so there is no engine parity left to chase. The fixes that came out of chasing it live on in the importer and exporter (back-side pad mirroring, real `F.CrtYd`/`B.CrtYd` courtyards, non-copper pads, `insideCourtyard()` rule conditions in `eda_drc::pcbexpr`).

4. **Area**: ERC — engine built, zero UI exposure. **Closed 2026-10-03:** the Inspect > Electrical Rules Checker dialog runs `kicad-cli sch erc` on the exported schematic (with the design's own ERC pin map in the project file) and marks the findings on the sheet; the Rust ERC port is deleted. Our own schematic readability checks are `crates/lint`, in the dialog's Lint tab.

5. **Area**: Zone fill — not ported, both engines and UI. **Symptom**: `export_kicad_pcb` writes a zone's outline only; no fill polygon is ever computed. Measured impact: across KiCad's full 185-board QA corpus, `import_kicad_pcb` skips **5,847 zones** outright (reported in its own `ImportNotes`); on `examples/ladder/l4_control_hub.yaml` specifically, 1 zone survives placement/routing but 0 survive the PCB round trip. `ZoneDialog.tsx` captures net+layer only (no clearance/min-width/priority/hatch/thermal-relief). The fill the exporter writes is what kicad-cli judges (the live zone filler in `crates/zone-filler`); see REPORT.md's zone-fill-impact section for the concrete before/after on two boards. **Port from**: `pcbnew/zone_filler.cpp` (the fill algorithm itself), `pcbnew/dialogs/panel_zone_properties.cpp` (the dialog), `pcbnew/tools/zone_filler_tool.cpp`. **Size**: L (fill engine) + M (dialog).

6. **Area**: eeschema — no hierarchical sheets in the data model. **Symptom**: a multi-sheet schematic can't even be displayed, let alone navigated; the Hierarchy panel is permanently stuck on one "Root" row by its own code comment. Measured corroboration: of the 33 real QA schematics imported, 9 have sheets this project's importer records as "not descended into". **Port from**: `crates/model/src/ir.rs`'s `SchematicSection` needs a `sheets` concept; then `eeschema/tools/sch_navigate_tool.cpp` (back/forward history stack) and `sch_drawing_tools.cpp::DrawSheet`. **Size**: L.

7. **Area**: pcbnew — routing is single-segment-per-click with no push-and-shove, diff pairs, or length tuning. **Update**: largely closed. `crates/pns` now ports push-and-shove routing (walkaround/shove/mark-obstacles, posture, via placement), `D`-drag of an existing segment/corner/via, loop removal, an `OPTIMIZER` (`MERGE_SEGMENTS`+`MERGE_OBTUSE`+`MERGE_COLINEAR`), differential-pair routing (`6`, net-name-suffix pair detection + net-class gap/width), and single-track length tuning (`7`, dialog-driven rather than a live session) -- see `crates/pns/PARITY.md` and `web/studio/PARITY-pcb.md` section 4 for exactly what's faithful vs. scoped down. **Remaining**: `MOUSE_TRAIL_TRACER`/springback, `SMART_PADS`/`FANOUT_CLEANUP`, coupled shove/walkaround for a diff pair, diff-pair length/skew tuning (`8`/`9`), and a router-driven footprint drag -- each tracked individually in `crates/pns/PARITY.md`'s "Known gaps" list. **Port from**: `pcbnew/router/router_tool.cpp` (`PNS::ROUTER`), `pcbnew/tools/pcb_actions.cpp` (`routeDiffPair`/`tuneSingleTrack`/`tuneDiffPair`/`tuneSkew`/`routerShoveMode`/`WalkaroundMode`). **Size**: now S-M for what's left (was L).

8. **Area**: pcbnew — no footprint editor or pad tool. **Symptom**: no way to create or edit a footprint, renumber/push/copy pad properties, or assign a per-footprint 3D model. **Port from**: `pcbnew/tools/footprint_editor_control.cpp`, `pad_tool.cpp`, `pcbnew/dialogs/panel_fp_properties_3d_model.cpp`. **Size**: L.

9. **Area**: pcbnew — no Board Setup dialog. **Symptom**: layer stackup, rule authoring, and zone/text/dimension/track defaults are hardcoded or missing outright; there's no settings surface for the board at all. **Port from**: `pcbnew/dialogs/dialog_board_setup.cpp` + its ~15 `panel_setup_*.cpp` children. **Size**: L.

10. **Area**: pcbnew — no net classes or custom DRC rules, and `import_kicad_pcb` doesn't reconstruct per-net overrides. **Symptom**: `assignNetClass` has no UI and there's no rule-authoring surface; an imported `.kicad_dru` is carried to kicad-cli, so DRC honours it, but nothing in the studio edits it. **Port from**: `pcbnew/dialogs/panel_setup_rules.cpp`, `dialog_copper_zones.cpp`'s net-class picker; importer side, whatever `crates/kicad/src/import.rs` currently does with `<net_class>`/`<rule>` sections in a real `.kicad_pcb`. **Size**: L.

11. **Area**: pcbnew — property dialogs are read-only for almost everything. **Symptom**: `FootprintPropertiesDialog.tsx` and the track/via/zone/shape branch of `ItemPropertiesDialog.tsx` are explicitly display-only except track width and text; can't edit a via's diameter/drill, a zone's outline, or any footprint field. Two-layer gap: the backend `Cmd` enum (`api/types.ts`) has no `edit_via`/`edit_zone`/`edit_shape`/`edit_footprint` ops to even call. **Port from**: `pcbnew/dialogs/dialog_footprint_properties.cpp`, `dialog_track_via_properties.cpp`, `dialog_pad_properties.cpp`. **Size**: L.

12. **Area**: pcbnew — move excludes tracks and zones entirely. **Symptom**: `move`/drag works for footprints, vias, shapes, and text, but there is no `Cmd` to move a track or a zone at all. **Port from**: `pcbnew/tools/edit_tool.cpp::Main`/`Drag`. **Size**: M.

13. **Area**: pcbnew — selection is missing modifiers and most item types. **Symptom**: no Ctrl-click XOR or Ctrl+Shift subtract (only plain replace and Shift-add); a drag-box only ever selects footprints — tracks, vias, zones, shapes, and text are never box-selectable even though the box-direction rule itself (left-right = enclosed, right-left = crossing) is correctly ported. **Port from**: `pcbnew/tools/pcb_selection_tool.cpp::SelectRectArea` (selects any `BOARD_ITEM`) and its `m_additive`/`m_subtractive`/`m_exclusive_or` handling. **Size**: M.

14. **Area**: Both editors — no clipboard (cut/copy/paste/duplicate). **Symptom**: nothing can be copied or duplicated anywhere in the app — a basic, constant-use operation on both PCB and schematic. **Port from**: `pcbnew/tools/edit_tool.cpp` (`copyToClipboard`/`cutToClipboard`/`Duplicate`), `eeschema/tools/sch_editor_control.cpp` (`doCopy`/`Paste`). **Size**: M.

15. **Area**: eeschema — cross-tab undo/redo bug. **Symptom**: pressing Ctrl+Z while viewing the Schematic tab silently undoes the last **PCB** edit instead of being a no-op, because `common.Interactive.undo`/`redo` are two of the only actions in `useActionRunner.ts` not wrapped in the `pcbOnly()` guard every sibling action uses. A genuine correctness bug, not just a missing feature. **Port from**: internal fix — `actions/useActionRunner.ts` (add the guard, or scope undo/redo per-tab). **Size**: S.

16. **Area**: pcbnew — hotkey-extraction bug on every platform-conditional default. **Symptom**: Ctrl+Y doesn't redo, Home doesn't zoom-to-fit, F1/F2 zoom in/out don't exist as hotkeys at all (the authors noticed and excluded both rather than fix the extraction), and `drawZone`'s extracted hotkey is a nonsense merge of the Mac and non-Mac bindings. Confirmed systemic across every `#ifdef __WXMAC__` action, not a one-off typo. **Port from**: fix is in `web/studio/tools/extract-actions.js`, reading the `#else` (non-Mac) branch as primary; reference bindings in `common/tool/actions.cpp` and `pcbnew/tools/pcb_actions.cpp`. **Size**: S.

17. **Area**: pcbnew — no click-vs-drag threshold. **Symptom**: a move-drag flags "moved" the instant a grid-snapped delta is non-zero; a sub-pixel jitter between mouse-down and mouse-up on a part can silently nudge it by one grid step. Concrete, reproducible, and cheap to fix. **Port from**: `common/tool/tool_dispatcher.cpp` (`DragDistanceThreshold`=8px, `DragTimeThreshold`=300ms, `include/tool/tool_dispatcher.h`). **Size**: S.

18. **Area**: Both editors — grid snapping is grid-only, missing object/anchor and construction-line snap. **Symptom**: outside the routing/via path (which has a fixed 500 µm anchor snap), moving or drawing anything snaps to the grid only — no snap to a nearby pad/track-end, no snap-to-extension-line at an angle; the schematic grid is additionally a hardcoded constant (`components/schematic/layout.ts`'s `GRID = 1270`) that the shared grid-cycle hotkeys don't even affect. **Port from**: `pcbnew/tools/pcb_grid_helper.cpp::BestSnapAnchor` (25 px + hysteresis), `common/tool/grid_helper.cpp::SnapToConstructionLines`, `eeschema/tools/ee_grid_helper.cpp` (per-item-category grids + live snap-point indicator). **Size**: M.

19. **Area**: DRC — footprint/schematic cross-check (`--schematic-parity`). **Closed 2026-10-03.** kicad-cli is the only DRC/ERC engine (`docs/ARCHITECTURE.md`, "Engines"); the Rust port this gap measured is deleted, so there is no engine parity left to chase. (kicad-cli has the flag; wiring the DRC dialog's "Test for parity between PCB and schematic" checkbox to it is UI work, not engine work.)

20. **Area**: ERC — bus and hierarchical-label checks. **Closed 2026-10-03.** kicad-cli is the only DRC/ERC engine (`docs/ARCHITECTURE.md`, "Engines"); the Rust port this gap measured is deleted, so there is no engine parity left to chase.

21. **Area**: ERC — multi-unit-symbol checks. **Closed 2026-10-03.** kicad-cli is the only DRC/ERC engine (`docs/ARCHITECTURE.md`, "Engines"); the Rust port this gap measured is deleted, so there is no engine parity left to chase.

22. **Area**: DRC — signal-integrity / diff-pair checks. **Closed 2026-10-03.** kicad-cli is the only DRC/ERC engine (`docs/ARCHITECTURE.md`, "Engines"); the Rust port this gap measured is deleted, so there is no engine parity left to chase.

23. **Area**: DRC — footprint/library-sync and padstack-validity checks. **Closed 2026-10-03.** kicad-cli is the only DRC/ERC engine (`docs/ARCHITECTURE.md`, "Engines"); the Rust port this gap measured is deleted, so there is no engine parity left to chase.

24. **Area**: DRC — remaining DFM/misc checks (courtyard, outline, sliver, connection width, isolated copper). **Closed 2026-10-03.** kicad-cli is the only DRC/ERC engine (`docs/ARCHITECTURE.md`, "Engines"); the Rust port this gap measured is deleted, so there is no engine parity left to chase.

25. **Area**: pcbnew — no align & distribute. **Symptom**: no align-top/bottom/left/right/center or distribute-with-even-gaps for a multi-selection — a basic layout-tidying operation used constantly in real KiCad work. **Port from**: `pcbnew/tools/align_distribute_tool.cpp` (`AlignTop/Bottom/Left/Right/CenterX/CenterY/DistributeItems`). **Size**: S-M.

26. **Area**: pcbnew — no array tool. **Symptom**: no way to stamp a grid or circular array of vias/footprints (common for connector rows, LED matrices, mounting holes). **Port from**: `pcbnew/tools/array_tool.cpp`, `pcbnew/dialogs/dialog_create_array.cpp`. **Size**: M.

**Update (follow-up session): closed for every arrayable kind but footprints.** Ctrl+T now ports both grid (spacing, oblique offset, centring, brick stagger) and circular (centre, count, angle, direction, in-place item rotation) geometry, in both "Duplicate" and "Arrange selection" modes, for tracks/vias/zones/shapes/text; "Arrange selection" also covers a placed part (a pure reposition). See `web/studio/PARITY-pcb.md` section 17 for the full behavior table. **Still open**: arraying *footprints* specifically (the symptom's own headline example) only works via "Arrange selection" on footprints already on the board -- creating new placed copies is not possible, because a `FootprintInstance`'s id is its schematic symbol's id and this model has no "conjure a new placed copy with no symbol behind it" operation (the same restriction `Cmd::Duplicate` already has). Footprint reannotation and the footprint-editor's own pad-numbering half of source's dialog (`ARRAY_AXIS` schemes, `ARRAY_PAD_NUMBER_PROVIDER`) are also not ported -- see that section for why. **Size**: the common grid/circular-of-copper-and-drawing-items case from the original symptom is done; a new-footprint-copies path, if wanted, is its own separate follow-up (L -- it is really "teach the ops layer to place a fresh instance of an existing library footprint with no symbol," a much bigger change than array geometry itself).

27. **Area**: Both editors — no grouping. **Symptom**: no way to group items so they select/move as one unit. **Port from**: `common/tool/group_tool.cpp` (`GroupProperties`/`Ungroup`/`AddToGroup`/`RemoveFromGroup`/`EnterGroup`/`LeaveGroup`); needs a group concept in the board/schematic state first. **Size**: L.

**Update (follow-up session): pcbnew half closed, eeschema half still fully open.** A real `Group` IR type (id/name/member\_ids) now backs Ctrl+G/Ctrl+Shift+G group/ungroup, whole-group selection (clicking any member selects the group instead, unless you're inside it), and enter/leave (double-click a single selected group to enter it, Escape to leave, slotted into the same tiered Escape handler source uses). See `web/studio/PARITY-pcb.md` section 16 for the full behavior table. Still open even for pcbnew: no context-menu entries for any of this (hotkey/double-click/Escape only), `AddToGroup`/`RemoveFromGroup` exist as `Cmd`s with no UI wired to them, no nested groups, no group-aware bulk move/rotate/flip/delete (today "the group" is just its member ids under the hood), and no entered-group overlay rendering. The schematic editor side (`eeschema.*`) is untouched by this update — it remains exactly the gap originally described here, blocked on the same root issue as gap #1 (core schematic editing isn't wired up at all yet). **Size**: now S for what's left on the pcbnew side (context menu entries, add/remove-from-group UI); the eeschema half is still its own L, unstarted.

28. **Area**: pcbnew — no dimensioning or ad-hoc measurement tools. **Symptom**: no aligned/center/radial/orthogonal dimension or leader (a basic fabrication-drawing need), and no on-demand ruler beyond incidental dx/dy/dist in the status bar while mid-move. **Port from**: `pcbnew/tools/drawing_tool.cpp::DrawDimension`, `pcbnew/dialogs/dialog_dimension_properties.cpp`; `ACTIONS::measureTool` (`common/tool/actions.cpp`) + handler in `common/tool/common_tools.cpp`. **Size**: M.

**Update (follow-up session): dimensioning closed.** All five kinds (Aligned, Orthogonal, Radial, Leader, Center) are ported -- geometry, value-text formatting (units/precision/prefix-suffix/override), Board Setup defaults, selection, move, and a two-click-then-properties-dialog placement flow reachable from the already-extracted "Dimension objects" toolbar dropdown. See `web/studio/PARITY-pcb.md` section 18 for the full behavior table and its own named simplifications (no live third-click height preview, no text border, no manual text dragging, an approximated text-width knockout). The ad-hoc ruler half of this gap's symptom (`measureTool`) was already done before this update -- see PARITY-pcb.md section 11's measure tool entry. **Size**: done; remaining items are named, bounded follow-ups in section 18 itself, not a reason to reopen this gap.

29. **Area**: pcbnew — pan is middle-drag only. **Symptom**: no right-drag pan option, no hold-a-key-to-pan, no auto-pan-at-edge while mid-route/draw (makes working across a large board tedious), no trackpad two-finger pan gesture. **Port from**: `common/view/wx_view_controls.cpp` (`handleAutoPanning`, the motion-pan modifier, `onPanGesture`). **Size**: M.

30. **Area**: pcbnew — right-click context menu is a hardcoded 5-item list. **Symptom**: always offers rotate/flip/delete/net-highlight/zoom-fit regardless of what's actually selected, instead of KiCad's fully dynamic per-selection-type menu. **Port from**: `pcbnew/tools/pcb_selection_tool.cpp`/`edit_tool.cpp`'s context-menu population (`Init()` wiring `ctxMenu.AddItem(...)` conditionally per action). **Size**: M.

---

## Honorable mentions (just outside the top 30)

- Round-trip: re-exporting a real QA board through this project's writer changes kicad-cli's DRC verdict on 8 of 12 sampled boards (always still *parses* cleanly — the exporter itself is solid — but the violation-type counts shift), a direct, expected consequence of #5/#10/arc-approximation rather than a new root cause. See REPORT.md section 4.
- pcbnew: no symbol/footprint library browsers or calculator tools (`showSymbolBrowser`/`showFootprintBrowser`/`showCalculatorTools`) — XL, lowest priority of the surface-area gaps since it's effectively a second mini-application, same as eeschema's symbol editor.
- eeschema: no Symbol Editor at all (new/edit/duplicate a library symbol, place pins) — XL, same reasoning.
- pcbnew: no keyboard layer switching (PgUp/PgDn/+/-, only a dropdown) — S, `pcb_actions.cpp` layer-jump actions.
- Both: no "select on PCB from schematic" / "select on schematic from PCB" cross-probe action, though the underlying mechanism (shared `state.selection`) already makes same-reference cross-highlighting work by accident.
- 3D viewer: placed-part geometry in the always-on procedural scene is flat boxes, not real component shapes (real models only via a separate, best-effort async GLB fetch) — L, confidence low (KiCad's `3d-viewer/` source wasn't present in the snapshot read for this audit).

## Function audit: `pcbnew/zone_filler.cpp` (3,927 lines) vs `crates/zone-filler` (876)

Function-by-function, against KiCad commit 8303b2ad. "Plumbing" = threading,
progress, cancel, undo/commit -- no effect on the fill geometry.

| KiCad function | lines | ours | status |
|---|---|---|---|
| `Fill` (orchestration) | ~1080 | `eda_drc::fill::fill_all_zones` | **partial**: no iterative refill (issue 21746), island removal is our own `apply_island_removal` not `FillIsolatedIslandsMap` via connectivity, no teardrop/zone priority ordering pass; rest is plumbing |
| `addKnockout` (pad / graphic) | ~110 | `add_knockout` | partial: no custom-pad convex-hull mode |
| `addHoleKnockout` | 5 | inline | ported |
| `knockoutThermalReliefs` | ~315 | inline in `fill_zone` | **partial**: no per-pad zone-connection overrides, no padstack per-layer shapes |
| `buildCopperItemClearances` | ~500 | inline in `fill_zone` | **partial**: copper text (as strokes) since 5770886; no courtyard clearance knockouts, no net-tie exemptions, no Edge.Cuts/Margin graphic knockouts by edge clearance (board outline only) |
| `buildDifferentNetZoneClearances` | ~65 | `other_zones` loop | ported |
| `subtractHigherPriorityZones` | ~35 | in `fill_zone` | ported |
| `connect_nearby_polys` | ~45 | -- | **missing** |
| `postKnockoutMinWidthPrune` | ~55 | `postknockout_min_width_prune_if_needed` | ported |
| `fillCopperZone` | ~355 | `fill_zone` | partial (see rows above) |
| `fillNonCopperZone` | ~110 | -- | **missing** (non-copper zones) |
| `fillSingleZone` | ~35 | `fill_zone` | partial |
| `buildThermalSpokes` | ~375 | `spokes::build_spokes` (~30) | **simplified**: bbox-based spokes; KiCad uses pad shape, spoke angle, circle/oval special cases, spoke-end-in-fill test with epsilon |
| `buildHatchZoneThermalRings` | ~115 | -- | **missing** |
| `addHatchFillTypeOnZone` | ~235 | -- | **missing** (hatch fill mode) |
| `refillZoneFromCache` | ~110 | -- | missing (iterative refill) |
