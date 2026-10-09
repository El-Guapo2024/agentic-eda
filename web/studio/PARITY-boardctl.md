# Board control parity with KiCad pcbnew

One row per behaviour of the PCB board-control tools: `PCB_CONTROL` (`pcb_control.cpp`),
`BOARD_EDITOR_CONTROL` (`board_editor_control.cpp`), `BOARD_INSPECTION_TOOL`
(`board_inspection_tool.cpp`) and `ZONE_FILLER_TOOL` (`zone_filler_tool.cpp`), with its status and the
KiCad function it came from. Same status words as `PARITY-pcb.md`: **identical** (same logic and
constants, browser adaptations noted), **partial** (the core is ported, a real gap is named) and
**missing** (not wired; every such action has its reason in `tools/ui-parity-missing.json`, which
`tools/ui-parity-audit.mjs` checks, and `docs/parity/UI-ACTIONS.md` lists).

KiCad source: the read-only checkout at commit `8303b2ad` (`pcbnew/`, `common/`).

The logic lives in `src/kicad-port/boardControl.ts` (+ `boardControlState.ts`, `boardControlPick.ts`,
`polyTriangulate.ts`; unit tested, `npm run test:unit`), `crates/ops/src/board_control.rs` (zone merge,
zone priority, drill origin, Repair Board; `cargo test -p eda-ops`) and `crates/cli/src/board_output_api.rs`
/ `board_control_api.rs` (export routes). `src/actions/boardControlActions.ts` is the thin layer that
registers the actions.

## 1. Display options (`PCB_CONTROL`)

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Sketch Graphic Items / Sketch Text Items (outline instead of fill) | identical | `pcb_control.cpp:GraphicDisplayMode` / `TextDisplayMode` (`m_DisplayGraphicsFill`, `m_DisplayTextFill`); `painter.ts` draws filled shapes as their outline and text with a one-pixel pen |
| Show Pad Numbers | identical, on by default | `pcb_viewer_tools.cpp:ShowPadNumbers` flips `m_ViewersDisplay.m_DisplayPadNumbers`, which is **true** in KiCad's own defaults, so the studio now shows pad numbers (above the net name, as `PCB_PAINTER::draw( PAD )` lays them out) until the switch is turned off in Appearance > Objects > Display Options (KiCad keeps it in Preferences > Display Options) |
| Draw Zone Fill Fracture Borders / Triangulation | partial | `PCB_PAINTER::draw( ZONE )` in the two debug modes: only the strokes are drawn. The fracture mode strokes the fill's polygons; the triangulation is an earcut port (`polyTriangulate.ts`, `POLYGON_TRIANGULATION`) without KiCad's Z-order hash and balanced splitting, so the triangles differ from KiCad's although they cover the same area. Needs a fill on screen (`/api/fill?polys=1` carries each island's holes) |
| Ratsnest Mode (3-state) | identical, Appearance panel for the radio buttons | `pcb_control.cpp:RatsnestModeCycle`: off, all layers, visible layers only. The cycle is the registered action (no menu entry in KiCad either); "Ratsnest display" in Appearance > Nets > Net Display Options (All, Visible layers, None) is the same switch, as `APPEARANCE_CONTROLS::onRatsnestMode` has it |
| Ratsnest drawing rules | identical | `RATSNEST_VIEW_ITEM::ViewDraw`: hidden nets draw nothing; a pad's local flag decides with its line's other end (both flags with the global ratsnest on, either with it off); in visible-layers mode both ends must be on a shown layer. The ratsnest route names each end's item and copper layers (`from_id`, `to_id`, `from_layers`, `to_layers`) |
| Local Ratsnest tool | identical | `board_inspection_tool.cpp:LocalRatsnestTool`: a click on a pad flips its flag, a click on a footprint sets every pad to the opposite of its first pad, a click on neither resets all, Esc leaves the tool and resets all (the finalize handler). Pads are picked by `padAt` (`boardControlPick.ts`) |
| Flip Board View | partial | `pcb_control.cpp:FlipPcbView` -> `view->SetMirror( m_FlipBoardView )`: the picture is mirrored about the middle of the canvas (pointer, wheel zoom, drag pan, auto-pan, zoom-to-area and the keyboard cursor are mapped through the mirror; the arrow keys use `cursorMove`'s `mirroredX`). Not ported: KiCad keeps text on non-side-specific layers (`PCB_PAINTER::draw( PCB_TEXT )`, text boxes, dimensions) readable in the flipped view; here it mirrors with the board. Also in the View menu and as "Flip board view" in Appearance > Layers > Layer Display Options |
| Repair Board | identical | `board_editor_control.cpp:RepairBoard`: duplicate item ids get new ones, a net an item uses that the netlist lacks is added back; KiCad's report lines. `Cmd::RepairBoard` (one undo step, refused with "No board problems found." when it would change nothing) |
| Zone Manager | partial | `DIALOG_ZONE_MANAGER` / `MODEL_ZONES_OVERVIEW`: every copper zone, highest priority first, filter by name/net text and layer, Top/Up/Down/Bottom swap priorities, OK writes consecutive ranks (top = n-1) as one undo step, "Edit zone..." opens Zone Properties. Not ported: the preview canvas, "Update Displayed Zones", "Refill zones" (fills here are live) and the Auto-Assign Priorities button (`AutoAssignZonePriorities`' overlap analysis) |

## 2. Zones (`BOARD_EDITOR_CONTROL`, `ZONE_FILLER_TOOL`)

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Draft Fill Selected Zone(s) / Unfill Selected Zone(s) | identical in effect | `ZONE_FILLER_TOOL::ZoneFill` / `ZoneUnfill`. Fills here are derived (`/api/fill`), so "which zones show a fill" is the `bcx.zoneFilled` set and the report is trimmed to it (`keepFilled`); Fill All / Unfill All reset it |
| Fill zones that have none (`zoneFillDirty`) | identical in effect | the system action behind the auto-refill: a filled zone refills with every edit already, so what is left is the zones never filled or just unfilled |
| Merge Zones | identical | `BOARD_EDITOR_CONTROL::ZoneMerge` / `BOARD::TestZoneIntersection`: same net, same rule-area-ness, same layer; a zone is merged when it touches a chosen one; "Zones have insufficient overlap for merging." when the union is not one outline; the highest priority is kept. A zone here is one ring, so a union with a hole is stored fractured (a slit ring), as Zone Cutout stores one. `Cmd::MergeZones` |
| Duplicate Zone onto Layer... | identical | `ZoneDuplicate`: Zone Properties opens on a copy of the zone's settings; the copy is moved 1 mm each way when it stays on the layer (`DUPLICATE_ZONE_OFFSET_UM`) |
| Zone Priority: Move to Top / Raise / Lower / Move to Bottom | identical, one difference | `getOverlappingZones`, `buildPriorityMap` and `findCascadeZones` as written (the cascade walks every copper zone, not only the overlapping ones, so a zone on another layer can be bumped too, as in KiCad). A move that would change nothing is an error here ("this zone is already above every zone it overlaps"), where KiCad does nothing, so no empty undo step is pushed; the four entries are always enabled where KiCad greys out the impossible ones. `Cmd::SetZonePriority` |
| Zones menu on the canvas | partial | `ZONE_CONTEXT_MENU`: offered when only zones are selected, as flat entries (the canvas menu has no submenus) |

## 3. Net inspection (`BOARD_INSPECTION_TOOL`)

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Highlight Net (selection) | identical | `HighlightNet( aUseSelection )`: every net of the selected connected items (a selected footprint stands for its pads), else the net under the cursor; more than one net can be highlighted (`bcx.netHighlightMore`) |
| Toggle Last Net Highlight | identical | `m_lastHighlighted`: the highlighted nets and the previous ones trade places |
| Hide / Show Net in Ratsnest | identical | `HideNetInRatsnest` / `ShowNetInRatsnest` (`doHideRatsnestNet`) on the selection's nets, `bcx.hiddenRatsnestNets` |
| Net Inspection Tools menu | partial | `NET_CONTEXT_MENU`: flat entries on the canvas menu, offered when the selection has a net |
| Show Footprint Associations | identical | `DIALOG_FOOTPRINT_ASSOCIATIONS`: the footprint's library link and its schematic symbol (`GET /api/footprint_associations`) |

## 4. Drill / place file origin

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Drill/Place File Origin, Reset Drill Origin | identical | `BOARD_EDITOR_CONTROL::DrillOrigin` / `DoSetDrillOrigin`: one click sets the origin (the tool ends), a red circle-and-cross (`ORIGIN_VIEWITEM`, 16 px) marks it unless it is (0, 0). `Cmd::SetAuxOrigin`, stored in `drawings.aux_origin`, written to the board as `(aux_axis_origin x y)` and read back |
| Used by the output dialogs | identical | the Plot dialog ("Use drill/place file origin"), the drill dialog (Origin: "Drill/place file origin") and the placement dialog (checked by default, as KiCad's) send `use_aux_origin`, which becomes `--use-drill-file-origin` (plot, pos) or `--drill-origin plot` (drill); the 3D formats' dialog (Output origin) and the GenCAD dialog ("Use drill/place file origin as origin") offer it too |
| Automatically select track width | partial | `BOARD_EDITOR_CONTROL::AutoTrackWidth` toggles `m_UseConnectedTrackWidth`: a route started at an existing track's end takes that track's width. Not ported: picking a width in the toolbar turns it off again (`Tracks_and_Vias_Size_Event`), and the first width key pressed while routing sets `m_TempOverrideTrackWidth` instead of cycling (`TrackWidthInc` / `TrackWidthDec`) |

## 5. Export and fabrication outputs

Every one of these runs `kicad-cli` (never a Rust or TypeScript exporter) through the studio's `offload`
and one-at-a-time lane, with the options of KiCad's own dialog and its defaults, into `export/kicad/<kind>/`.

| Output | Status | Note |
|---|---|---|
| STEP / GLB / BREP / XAO / PLY / STL / STPZ / U3D / 3D PDF | identical | `DIALOG_EXPORT_STEP` (which itself runs `kicad-cli pcb export <format>`) |
| VRML, GenCAD, IPC-D-356, IPC-2581, ODB++ | identical | `ExportVRML`, `ExportGenCAD`, `GenD356File`, `GenIPC2581File`, `GenerateODBPPFiles`; ODB++ output is a zip, tgz or folder as the compression option says |
| Bill of Materials | partial | `GenBOMFileFromBoard`'s fields and grouping (Id, Designator, Footprint, Quantity, Designation, Supplier and ref; grouped by value and footprint; `;`-delimited). The Footprint column is empty for parts that name only a package, because kicad-cli builds it from the exported schematic, which carries no footprint field for them |
| Footprint association file (.cmp) | identical in content | `RecreateCmpFile`: the one output written by the studio (`export/kicad/cmp/<name>.cmp`), kicad-cli has no command for it |
| Export Footprints... | partial | `ExportFootprintsToLibrary`: the studio has one project footprint library (`design.footprint_library`) rather than a library table, so each distinct footprint the board uses that is not an entry yet is added (`Cmd::OpenFootprintForEdit`, one undo step in the footprint editor's history). KiCad's "link the board footprints to the exported ones" has nothing to do here |
| kicad-cli time limit | new | a run that outlives its limit (120 s for DRC/ERC, 300 s for exports, `EDA_KICAD_TIMEOUT_SECS` overrides) is killed with its process group and reported as `kicad_cli_timeout`, so a stuck run never holds the lane (`crates/kicad-engine`, test with a fake `EDA_KICAD_CLI`) |

## 6. Not wired

These rows of the ten modules have no handler and each says why in `tools/ui-parity-missing.json`:
internal events that nothing posts here (`layerChanged`, `angleSnapModeChanged`, `trackViaSizeChanged`,
`hideDynamicRatsnest`, `updateLocalRatsnest`, `highlightItem`, `drillSetOrigin`); things that need a
subsystem the studio lacks (design blocks, the Footprint Viewer and Footprint Wizard windows, plugins,
Specctra DSN/SES, embedded 3D models, hatch fills, net colours, placement rule areas and the multichannel
tools, appending another board, importing a netlist, rescue of autosave files, project open/switch);
exports kicad-cli cannot do (IDF, Hyperlynx, the footprint report); and the ones that would need an edit verb
the intent-derived constraint model does not have (net classes, custom DRC rules, renaming a footprint).
Find by Properties and Compare Footprint with Library are recorded the same way: the first is built on
KiCad's property manager and the full expression language, the second is the DRC library-parity test.
