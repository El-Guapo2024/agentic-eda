# Shared (`common.*`) action parity with KiCad

One row per behaviour of the shared actions KiCad runs in every editor frame: `COMMON_TOOLS` (`common_tools.cpp`), `COMMON_CONTROL`
(`common_control.cpp`), `LIBRARY_EDITOR_CONTROL`, the selection, group and picker tools, the Checker's marker stepping and exclusion, and the dialogs they open.
Same status words as `PARITY-pcb.md`: **identical** (same logic and constants, browser adaptations noted), **partial** (the core is ported, a real
gap is named) and **missing** (not wired; every such action has its reason in `tools/ui-parity-missing.json`, which `tools/ui-parity-audit.mjs`
checks, and `docs/parity/UI-ACTIONS.md` lists).

KiCad source: the read-only checkout at commit `8303b2ad` (`common/`, `pcbnew/`, `eeschema/`).

Of KiCad's 183 `common.*` actions, 150 are wired and 33 carry a recorded reason (section 11); none is left unexplained. These tools run in the PCB
Editor, the Schematic Editor, the Footprint Editor and the Symbol Editor alike, so each handler works on whichever canvas is on screen through an
`EditorAdapter` (`src/actions/editorAdapter.ts`: view, cursor, grid, selection, item boxes, picking, delete), and is registered only on the tabs
where it does something -- an action an editor does not offer stays dimmed there. The code is in `src/actions/common*Actions.ts`, the pure rules in
`src/kicad-port/` (unit tested, `npm run test:unit`), the shared stores in `src/state/common*.ts`, `libraryTree.ts`, `gridOrigin.ts` and
`gridSettings.ts`, and the shared canvas layer (crosshair, lasso, bounding boxes, picker highlight) in `components/CommonOverlay.tsx` /
`CommonToolHost.tsx`.

## 1. Framing the view

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Zoom to Selection, Center on Selection, Center on Contents | identical | `COMMON_TOOLS::doZoomFit( ZOOM_FIT_SELECTION )` / `doCenter`: the selection's box (nothing selected does nothing), the document's own box or the default view box when it has no area. The library editors use the bigger fit margin (`LIBRARY_EDITOR_FIT_MARGIN`, "1.48") |
| Zoom to Selection Area (`zoomTool`) | identical | `ZOOM_TOOL::Main` / `selectRegion`: a drag draws the box, the left button zooms in so the box fills the screen, the right button zooms out by the same ratio, then the tool ends (Esc cancels; `zoomToAreaView`). The board and the schematic run it from `ZoomAreaOverlay.tsx`; the Footprint and Symbol Editors from `CommonToolHost.tsx` (the box is drawn by `CommonOverlay`) |
| Zoom presets (`zoomPreset`) | identical | `COMMON_TOOLS::ZoomPreset` / `doZoomToPreset`: entry `idx` of the editor's zoom list (`zoomList` per editor, `kicad-port/zoomFit.ts`) about the view centre, entry 0 is Zoom Auto (zoom to fit the page or the document) |

## 2. Cursor, crosshair and display options

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Small, Full-window and 45-degree crosshair | identical | `COMMON_TOOLS::CursorSmallCrosshairs` / `CursorFullCrosshairs` / `Cursor45Crosshairs`: `CommonOverlay` draws the mode over all four canvases (the small cross is KiCad's 80 px); one setting for the four canvases (`state/commonOptions.ts`, kept in local storage; KiCad keeps one per window) |
| Always Show Crosshairs, Show Bounding Boxes | identical | `ToggleCursor` (`m_forceDisplayCursor`) / `ToggleBoundingBoxes` (`SetDrawBoundingBoxes`) |
| High Contrast Mode (Inactive Layer View Mode) | identical (PCB), partial (Footprint Editor) | `PCB_CONTROL::HighContrastMode`: what is not on the active layer is drawn dimmed (the same 0.25 on both canvases); the Footprint Editor's own switch dims graphics and text off the layer box's layer and, since this editor's layer box has only silkscreen, fab and courtyard, always the copper pads. Checked in the menus and drawn pressed on the toolbars. The Symbol Editor has no layers |
| Polar Coordinates | partial | `PCB_ACTIONS::togglePolarCoords`: the status line shows the cursor as r and theta; checked in the menus and drawn pressed on the toolbars. The board and the Footprint Editor have it (the Footprint Editor's status line now shows its zoom, cursor and grid, as the Symbol Editor's does); one flag serves both, where KiCad keeps one per frame. The relative dx / dy / dist readout stays the board's: the library editors have no local origin |
| Object Snapping (active / all layers) | identical (PCB) | `PCB_CONTROL::SnapMode`; the library editors have no object snapping (their clicks snap to the grid only), so they do not offer it |
| Update Units / Update Preferences / Refresh Preview / Show Context Menu | identical in effect | the running tool recomputes its assistant at the cursor; everything else reads the units and preferences from state |
| Check marks | identical | the crosshair mode, Always Show Crosshairs, bounding boxes, selection mode and Library Tree show whether they are on in the menus and toolbars of every editor (`commonChecked.ts`) |

## 3. Selection, delete tool, picker

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Selection tool events (Select Item(s), Unselect Item(s), Reselect, Clear, Selection Cursor, Selection Menu) | identical | `SELECTION_TOOL::AddItemToSel` / `AddItemsToSel` / `RemoveItem(s)FromSel` / `ReselectItem` / `SelectionMenu`, `PCB_SELECTION_TOOL::CursorSelection` / `ClearSelection`: the action carries the ids as its parameter (`run( name, ids )`) |
| Select by Rectangle / Select by Lasso | identical | `PCB_SELECTION_TOOL::SetSelectRect` / `SetSelectPoly`, and the schematic tool's: a drag on empty space draws the lasso on the board, the schematic and both library editors; clockwise selects what is inside, counterclockwise what it touches (`kicad-port/lasso.ts`, `KIGEOM::BoxHitTest`) |
| Select All / Unselect All in the library editors | identical | `SelectAll`: every item of the footprint or symbol that is open (the symbol's: those of the unit and body style on show); `ClearSelection` empties it (the board's and the schematic's are in `useActionRunner.ts`) |
| Measure Tool | partial | `PCB_VIEWER_TOOLS::MeasureTool`: two clicks on the grid draw a ruler, a dashed line with the distance and its dx / dy at the middle (`kicad-port/measureRuler.ts`); a click on a finished ruler starts the next, Esc clears it, the button is drawn pressed while the tool runs. The board draws it in `painter.ts`, the Footprint Editor in `CommonOverlay.tsx` (clicks in `CommonToolHost.tsx`). Not ported: `RULER_ITEM`'s arrowheads and angle readout. eeschema has no measure tool |
| Delete tool | identical | `PCB_CONTROL::InteractiveDelete` / `SCH_TOOL_BASE::InteractiveDelete`: highlights the item under the pointer, a click deletes it, Esc leaves; in all four editors, each delete one undo step through the editor's own verbs |
| Picker tool and sub-tool | identical | `PICKER_TOOL::Main`: the studio's `PickerHost` (`actions/pcbPicker.ts`) now also runs on the schematic and the library editors (`CommonToolHost` answers it there) |
| Selection tables (`selectRows`, `selectColumns`, `selectTable`) | missing | needs table items; see section 11 |

## 4. Groups

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Group Properties | partial | `DIALOG_GROUP_PROPERTIES`: name and member list, the plus button hides the dialog and asks for a click on a new member (`PCB_GROUP_TOOL::PickNewMember`), the trash button drops a row, a click on a row shows the item. OK is one undo step (`Cmd::EditGroup`: rename, replace the members, an item leaves any other group, under two members dissolves it; a member may be another group, but not this one or one that holds it). The "Library link:" box is the group's design block (`edit_group`'s `design_block`, refused as "Invalid library link: '...'" when it is no `Lib:Name`: `PARITY-sch.md` section 33). Not ported: the Locked box |
| Add Items to Group, Remove Items from Group | identical | `GROUP_TOOL::AddToGroup` / `RemoveFromGroup` under `GROUP_CONTEXT_MENU`'s conditions, listed in the board's context menu where they apply (it lists them since 2026-10-08: `PARITY-pcb.md` section 7) |
| Group, Ungroup | identical in effect, nested (2026-10-08) | `GROUP_TOOL::Group` / `Ungroup`, `PCB_GROUP_TOOL::Group`: grouping a selection that holds groups makes them members of the new one, ungrouping frees the members of the group above too, a group under two members dissolves up the tree (`PARITY-pcb.md` section 16) |
| Enter Group, Leave Group | identical (2026-10-08) | `PCB_SELECTION_TOOL::EnterGroup` / `ExitGroup`: Enter Group (the menu, or a double click) selects the group's members and only they can be picked; Leave Group (the menu, which selects the group), Escape, a click outside the group and selecting something outside it leave it. The rest of the board is drawn dimmed meanwhile (`PARITY-pcb.md` section 16) |

## 5. Checker markers, Find and Replace

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Next Marker, Previous Marker | identical (2026-10-08) | `DRC_TOOL::NextMarker` / `PrevMarker`, `SCH_INSPECTION_TOOL`'s, over `RC_TREE_MODEL`'s order: with the dialog up they step the page it shows (Violations, Unconnected Items or Schematic Parity on the board; Violations on the schematic) among the rows its Show boxes list, select the items the marker names and frame them; with no dialog the board's opens it and stops (`ShowDRCDialog`). Nothing to step on the Ignored Tests page. KiCad assigns no default hotkey to these three actions (`ACTIONS::nextMarker` / `prevMarker` / `excludeMarker`), so none is bound; they are in both Inspect menus and the dialogs' own buttons (`actions/commonCheckerActions.ts`, `checkerOps.ts`, `kicad-port/rcItems.ts` `stepListed`) |
| Exclude Marker | identical (2026-10-08), board and schematic | `DIALOG_DRC::ExcludeMarker` (the Violations page, a marker not excluded yet) -> `add_drc_exclusions`; `DIALOG_ERC::ExcludeMarker` -> `add_erc_exclusion`. The row stays dim when exclusions are shown; when they are hidden it leaves the list and the selection moves to the next one (`RC_TREE_MODEL::DeleteCurrentItem`). The report on screen is patched in place, a current report stays current |
| Replace, Replace All, Update Find, Find and Replace | identical | the schematic find dialog's own actions (`SCH_EDITOR_CONTROL::ReplaceAndFindNext` / `ReplaceAll`, `UpdateFind`); matches are brightened while the dialog is open |
| Activate Point Editor, Update Menu | identical in effect | satisfied by construction: the point editor is always on, the menus read state |

## 6. Suite actions: About, Help, saving

| Behaviour | Status | KiCad file:function |
|---|---|---|
| About | partial | `DIALOG_ABOUT`: title and version, the three pages (About, Version, License), Donate, Report Bug and Copy Version Info (`GetVersionInfoData`'s text). Not ported: the contributor pages (Developers, Doc Writers, Librarians, Artists, Translators, Packagers) -- the About page points to KiCad's list instead of copying it |
| Help, Getting Started, Get Involved, Donate, Report Bug | partial | `COMMON_CONTROL::ShowHelp` and the others: the frame's online manual (`go.kicad.org/docs/<major.minor>/<language>/<help_name>/`; there is no installed copy to prefer), the beginners' guide, the involvement and donation pages. Report Bug opens an issue on this program's repository with the version information, not KiCad's tracker. The **Help** menu (`EDA_BASE_FRAME::AddStandardHelpMenu`) was missing from the extracted menu data, so the menu bar appends it |
| Save a Copy (PCB), Save All | identical in effect | `BOARD_EDITOR_CONTROL::SaveCopy`: the derived `.kicad_pcb` handed to the browser's Save, the same as Save As (design.json is the only master). `SYMBOL_EDITOR_CONTROL::Save( saveAll )`: every edit is already written when it is made, so Save All only reports |
| Save As (Footprint Editor) | partial | `FOOTPRINT_EDITOR_CONTROL::SaveAs` -> `SaveFootprintAs` and `SAVE_AS_DIALOG` ("Save Footprint As": name, "Save in library:" list, New Library...): the footprint the tree selects (else the loaded one) is stored under the name and library chosen as one `put_library_footprint` (undoable); a name the library has asks "Footprint %s already exists in %s." with Overwrite; the editor moves to the copy when it was the loaded footprint. The board's and the schematic's Save As (the derived KiCad files, in `useActionRunner.ts`) are separate. Not ported: Save Library As (a selected library row; there is no library file to copy -- it says so), and the Value field following the new name (a library footprint has no Value of its own here). The Symbol Editor's Ctrl+Shift+S is `saveLibraryAs` |
| Update Schematic from PCB | identical in effect | `UpdateSchematicFromPCB` (back annotation): the board follows the schematic's reference, value and Footprint field in the same step, and a reannotation or an exchange on the board moves the symbols with it (`PARITY-pcb.md` section 30), so what is left to carry over is a footprint the board is on that the symbol does not name (a locked footprint, or one no library has): the dialog lists each "Change R1 footprint assignment from 'A' to 'B'." and "Update Schematic" sets the symbols' Footprint fields to the board's, as one undo step |
| Open Footprint Editor, Open Symbol Editor | identical in effect | `COMMON_CONTROL::ShowPlayer`: the tab is the window |

## 7. Library tree and browsers (Footprint Editor, Symbol Editor)

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Show / Hide Library Tree | identical | `LIBRARY_EDITOR_CONTROL::ToggleLibraryTree`: the dock layout's Libraries column (`dockLayoutStore.ts`) |
| Pin Library, Unpin Library | identical | `changeSelectedPinStatus`: the selected library rows are pinned or let go; a pinned library is listed first with the star glyph (`LIB_TREE_NODE::Compare`); the state is `state/libraryTree.ts`, so the tree and the actions agree |
| Expand All, Collapse All | identical | `ACTIONS::expandAll` / `collapseAll` over the tree (an explicit choice of a library wins over the mode until the next Expand / Collapse All) |
| Library Tree Search | identical | `LIBRARY_EDITOR_CONTROL::LibraryTreeSearch` (Ctrl+L): shows the tree when it is hidden, then focuses its search box |
| Show Datasheet (Footprint Editor) | identical | `FOOTPRINT_EDITOR_CONTROL::ShowDatasheet` / `GetFootprintDocumentationURL`: the footprint's Datasheet field, else the first web address in its description (read up to a character a URI cannot hold, a closing bracket without its opener, trailing punctuation dropped), opened in a new browser tab; none says "No datasheet found in the footprint." (`kicad-port/footprintDatasheet.ts`). The Symbol Editor's and the schematic's are `editorFrameActions.ts`'s and `useActionRunner.ts`'s |
| Footprint / Symbol Library Browsers | partial | `COMMON_CONTROL::ShowPlayer( FRAME_FOOTPRINT_VIEWER / FRAME_SCH_VIEWER )`: KiCad's read-only viewer window; here the browser is the editor with its library tree shown (the editors are the one place the libraries are listed) |

## 8. Text

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Left / Center / Right Justify (board, Footprint Editor) | identical | `EDIT_TOOL::JustifyText`: every selected text, or the one under the cursor, takes the horizontal justification as one undo step; locked text is skipped (`kicad-port/justifyText.ts`) |
| Left / Center / Right Justify (schematic) | missing | `SCH_EDIT_TOOL::JustifyText` waits for a justification on schematic text: `SchematicText` has none yet, so the schematic does not offer the actions |

## 9. Page Settings

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Page Settings (board, schematic) | partial | `DIALOG_PAGES_SETTINGS`: paper (standard formats, orientation, custom size within the editor's limits) and title block (date with Apply, revision, title, company, comments 1 to 9) with a page sketch. OK is one undo step (`Cmd::SetBoardPage` / `SetSchematicPage`; nothing is sent when nothing changed). The IR holds `PageSettings` and the title block (additive), the exporters write `(paper ...)` and `(title_block ...)`, the importers read them back. A schematic sheet keeps its paper's *name* in its title block (`TitleBlock::paper`, which the layout engine chooses per sheet and the dialog now edits) and, only for a paper the name cannot say -- portrait, a user size -- the full settings in `SchExtras::page`; `PageSettings::of_sheet` reads the two as one, and the state JSON's `paper` (size, for the drawing sheet) and `page` (the dialog's) both come from it. The schematic's drawing sheet is laid out on the viewed sheet's own paper and draws its size, company and comments. Not ported: a custom drawing-sheet file (`.kicad_wks`; the sheet is KiCad's default one) and the schematic dialog's "export to other sheets" boxes (the editor works on one sheet) |

## 10. Grids

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Grid Origin (picker), Reset Grid Origin | identical | `PCB_CONTROL::GridPlaceOrigin` / `GridResetOrigin` / `DoSetGridOrigin`: the board's origin is a verb (`Cmd::SetGridOrigin`, undoable, saved as `(grid_origin x y)` in the `.kicad_pcb` and read back) and the server's placement snap is anchored at it, so a position snapped to the grid the editor draws is the position a verb keeps. The editors snap to it, anchor the grid dots at it and draw KiCad's circle-and-X marker; the Footprint Editor's origin is the session's |
| Grid Origin... | identical | `COMMON_TOOLS::GridOrigin`: the X / Y entry dialog; OK runs Grid Origin with the point as its parameter |
| Edit Grids... | partial | `COMMON_TOOLS::GridProperties` -> `PANEL_GRID_SETTINGS`: a Preferences - Grids dialog on the editor that asked, with an Editor choice for the other three (PCB, Footprint, **Schematic**, Symbol): the list with Add (before the selected row) / Edit / Remove / Move Up / Move Down, the size checked as `DIALOG_GRID_SETTINGS` does (0.001 to 1000 mm, "already exists"), Fast Grid 1 and 2 with their hotkeys, **Grid Overrides** (below), Reset to Defaults (`ResetPanel`: the list goes back, the fast grids keep their size or fall back to the first / last entry, the overrides keep their grids by size), and OK makes the selected row the editor's current grid. Each editor has its own list (kept in local storage, `state/gridSettings.ts`; KiCad keeps it in each editor's settings file); defaults are `DefaultGridSizeList` (the PCB list for the board and footprint editors; 100 / 50 / 25 / 10 mil for the schematic and symbol editors, `eeschema_settings.cpp`'s). The grid boxes end with "---" and "Edit Grids..." (`OnSelectGrid`), and the Show Grid button's right-click menu offers Edit Grids... and Grid Origin... (`WithContextMenu`). Not ported: a grid's name, a different Y size (the studio snaps to one square grid), and the schematic context menu's own Grid submenu |
| Grid Overrides (the section of the Grids page): a checkbox and a grid from the list for each of Connected items, Wires, Vias (board editors only), Text and Graphics | identical | `PANEL_GRID_SETTINGS` (`override_connected`, `override_wires`, `override_vias`, `override_text`, `override_graphics` and their `_idx`), defaults from `APP_SETTINGS_BASE::addParamsForWindow`: the schematic and symbol editors start with connected items and wires on 50 mil, text on 10 mil, graphics off (25 mil chosen), the board editors with every one off. `kicad-port/gridOverrides.ts` (`defaultGridOverrides`; `rebuildOverrides` finds an override's grid again by size after the list is edited, else the first entry, as `RebuildGridSizes` keeps the selection), `state/gridOverrides.ts`, `GridsDialog.tsx` |
| Next Grid, Previous Grid, grid presets, Fast Grid 1 / 2 / Cycle | identical (PCB, Footprint, Schematic, Symbol) | `COMMON_TOOLS::GridNext` / `GridPrev` / `GridPreset` / `GridFast1/2/Cycle` over the editor's own list: wrap round, clamp, the cursor goes to the new grid, a hotkey shows which grid it is. The schematic took its grid from a constant (`GRID`, 50 mil) until 2026-10-09: it has the list now (`state.schGridUm`, `editorAdapter.ts`), the status bar shows it, and the painter's grid dots follow the choice (`computeVisibleGridSize`: a fine grid is drawn every few steps when zoomed out, as `GAL::GetVisibleGridSize` does) |
| Toggle Grid Overrides (`Ctrl+Shift+G`, the toolbar button) | identical | `COMMON_TOOLS::ToggleGridOverrides` (`SetGridOverrides( !IsGridOverridden() )`) -- `commonGridListActions.ts` registers it on all four tabs, `actionChecked.ts` shows the button pressed while the editor's overrides are on, and every snapping tool reads the grid of its item category from the overrides: the board and schematic editors through `PCB_GRID_HELPER::GetGridSize` / `EE_GRID_HELPER::GetGridSize` (`components/canvas/pcbSnap.ts`, `components/schematic/schSnap.ts`), the Footprint and Symbol editors for what they place and move (a pad or pin on the connected-items grid, a shape on the graphics grid, text on the text grid; `FootprintCanvas.tsx`, `SymbolEditorCanvas.tsx`). `Ctrl+Shift+G` was bound to Ungroup before (an earlier brief's binding); it is this action's key now, as KiCad's table has it, and Ungroup has none (the toolbar and the context menu keep it) |

## 11. Not wired, with the reason (33)

All in `tools/ui-parity-missing.json`; each is a subsystem the studio does not have, and no fake handler stands in for it.

| Actions | Reason |
|---|---|
| New, Open, Quit, Revert, Show Project Manager | one project directory chosen at launch, no project manager; a web page has no application to quit; every edit is already in design.json, so there is nothing to revert |
| Add Library by drop | a browser drop carries a file's contents, not its path, so there is no path for the library table (Add Library takes a typed path or one chosen in the server's folders: section 14) |
| Design Block Library Table | wired 2026-10-20 (`task-design-blocks`): Manage Design Block Libraries is section 14's dialog on the third table kind, and the Design Blocks panes use it (`PARITY-sch.md` section 33) |
| Open Directory, Open with Text Editor | a browser cannot open the operating system's file manager or editor |
| Plugins Reload | no Python scripting or API plugins |
| Calculator Tools | a separate program in KiCad |
| Undo / Redo Zoom, Zoom In / Out Horizontally / Vertically | the simulator plot's zoom history and axes; there is no simulator (`GAPS.md`: Simulator, last) |
| Select Rows / Columns / Table, the Table Editor (add / delete rows and columns, merge, unmerge, export CSV) | the design IR has no table type |
| Embed File, Extract File, Remove File | the IR has no embedded-file list (KiCad stores them zstd-compressed, base64-encoded and checksummed) |
| Paste Special | **Schematic tab: identical** (`DIALOG_PASTE_SPECIAL`'s reference-designator choices; `PARITY-sch.md` section 15). PCB tab: nothing to special-case yet (the copper it holds must name a net; the studio says so) |

## 12. Offered by KiCad in an editor, not by the studio there

The wired actions above are registered only on the tabs where they do something. These are the ones KiCad offers in an editor that the studio leaves dimmed
there (or, for Group / Ungroup on the schematic, leaves enabled and inert), with where the reason is kept.

| Action | Editor | Why |
|---|---|---|
| Find and Replace, Update Find, Find Next Marker | PCB | KiCad's board editor does not offer them either; they are the schematic find dialog's. Find, Find Next and Find Previous are live on the board since 2026-10-08 (`PARITY-pcb.md` section 7a, `GAPS.md` item 21) |
| Find, Find and Replace | Symbol Editor | `editor_toolbar_support.json`: Find searches the schematic, not an open library symbol |
| Left / Center / Right Justify | Schematic | `SchematicText` has no justification yet (`GAPS.md` item 12) |
| Group Properties, Add / Remove Items, New Group Member | Schematic | the schematic has no groups (`GAPS.md` item 17); Group / Ungroup are registered for the board only and do nothing there |
| Group, Ungroup | Footprint Editor | `editor_toolbar_support.json`: a library footprint has no groups |
| Grid Origin, Reset Grid Origin | Schematic, Symbol Editor | a grid origin is the board's and the Footprint Editor's; KiCad's schematic and symbol editors have none either |
| Show Properties | Footprint and Symbol editors | `editor_toolbar_support.json`: no Properties pane yet (the Pad, Footprint, Symbol and Pin Properties dialogs are there) |
| Show Datasheet | Board | KiCad offers it in the schematic, the Symbol Editor and the Footprint Editor only |

## 13. The scripted test hook

`window.__eda` (`kicad-port/edaTestHook.ts` + `actions/useEdaTestHook.ts`) is there for agents and test scripts: `actions()` lists the actions the
runner handles on the current tab with whether each is enabled and why not, `await run( id, args )` runs one exactly as a menu click does and waits for
every round trip it started, `state()` is a small snapshot (tab, revision, tool, picker, selection with kinds, item counts, the editor's grid, the
dialogs on screen, the open dialog and panel flags) and `errors()` the errors since the page loaded. Nothing in it changes what the studio does.

## 13a. File drops (`EDA_BASE_FRAME::OnDropFiles`, `DoWithAcceptedFiles`, each frame's `m_acceptedExts`)

`components/FileDropHost.tsx`, mounted once, listens on the window: it keeps the browser from opening a dropped file (`dragover` and `drop` are default-prevented), shows a dashed outline while a file is over the window, reads the file and runs the action the active editor's table names for its
extension with `{ name, text }` (KiCad's takes the path; a browser has none). The tables are `kicad-port/fileDrops.ts` (unit tested), taken from the frames: the board editor `.kicad_pcb` and `.brd` (`pcbnew.Control.DdAppendBoard`), `.dxf` and `.svg` (`pcbnew.InteractiveDrawing.ddImportGraphics`); the Footprint Editor `.kicad_mod`
(`pcbnew.Control.ddImportFootprint`); the schematic editor `.kicad_sch` (`eeschema.EditorControl.ddAppendFile`, not available: Import Sheet reads by path), `.png`, `.jpg`, `.jpeg` (`ddAddImage`, not available: no bitmap item), `.dxf` and `.svg` (`eeschema.EditorControl.ddImportGraphics`); the Symbol Editor and the 3D viewer none.
`kicad-port/actionTabGate.ts` keeps each of these actions to its own tab. A file the frame does not take is ignored by KiCad and said so here; one it takes but the studio cannot open is said so; several files open the first and say the rest were skipped (KiCad runs each in turn). While a dialog is open a drop is ignored, except on Import Graphics,
which takes the graphic into its File: field. `e2e/files-in.check.js` drives the drops with a `DataTransfer`. See PARITY-pcb.md section 29.

## 14. Library tables (Manage Symbol Libraries, Manage Footprint Libraries, Configure Paths, New Library, Add Library)

The libraries the studio lists, searches and places from are the ones KiCad's library tables name: KiCad's own **global** table (read only: the studio never writes
KiCad's configuration) and the board's own **project** table (`sym-lib-table` and `fp-lib-table` in the board's folder, which the studio creates and edits). The pure
half is `crates/kicad/src/library_table.rs` (the format, the variables, how the tables are put together; unit tested), the machine half `crates/cli/src/library_tables.rs`
(where KiCad's configuration is, the routes of the dialogs), the pages `components/libraryTables/*` over `kicad-port/libraryTable.ts` (unit tested) and
`actions/libraryTableActions.ts`. `e2e/library-tables.check.js` drives it all through `window.__eda`.

| Behaviour | Status | KiCad file:function |
|---|---|---|
| The tables: `(sym_lib_table (version 7) (lib (name ..) (type ..) (uri ..) (options ..) (descr ..) [(disabled)] [(hidden)]))`, read and written | identical | `library_table_parser.cpp` / `library_table_grammar.h`, `LIBRARY_TABLE::Format` and its prettifier: KiCad's own global table parses and writes back byte for byte; a syntax error says `Syntax error at line L, column C`. A value cannot hold a double quote or a line break (the reader has no escapes), so such a table is refused, not written wrongly |
| Which tables are read | identical | `LIBRARY_MANAGER::LoadGlobalTables` / `LoadProjectTables`, `PATHS::GetUserSettingsPath`: the newest version folder of KiCad's configuration (`~/Library/Preferences/kicad/10.99`; `KICAD_CONFIG_HOME`, or `EDA_KICAD_CONFIG_DIR` for a test), then the board's folder. Without a global table the studio reads the one KiCad installs (`SharedSupport/template`), which is what KiCad creates on its first run, and without that the library folder's files |
| A row of type `Table` chains another table | identical | `loadNestedTables`, `Rows`: a chained table's path is relative to the table that names it, a table already visited is an error ("A reference to this library table already exists"), a disabled chained table contributes nothing, its rows are hidden with it; the project's rows come after the global ones and a nickname defined twice is the later. KiCad's own table is one such row: its 222 symbol and 155 footprint libraries are listed through it |
| Path variables | identical | `KIwxExpandEnvVars`, `env_vars.cpp`, `InitializeEnvironment`: `${NAME}`, `$(NAME)`, `$NAME`; the project's variables, then `KIPRJMOD`, the environment, `kicad_common.json`'s `environment.vars`, KiCad's defaults (`KICAD10_SYMBOL_DIR` ...); `KICAD*_SYMBOL_DIR` falls back to whichever version is defined; a variable whose value names another is expanded again, a loop ends; an undefined one stays as written |
| Index, trees and choosers over the tables | identical | `SYMBOL_LIBRARY_ADAPTER` / `FOOTPRINT_LIBRARY_ADAPTER`: a library of either table is listed (Description from its row, the table it is in), searched, and opened like an installed one; a library switched off or whose file is not there is not a library; one with its Show box off is found but not listed (`!Hidden()`). The trees and the open choosers refresh when a table changes |
| Placing from a table library | identical | the definition goes in with the command (`Cmd::EmbedLibSymbol`, the footprint's pads with `PlaceFootprint`), so it draws, exports and passes kicad-cli ERC; a part that names `Lib:Footprint` finds it through the tables; the Change Symbols catalog reads them |
| The derived KiCad project | studio addition | the board's tables are written into the derived `.kicad` folder (`${KIPRJMOD}` named the real folder) and its path variables become the project file's text variables (`KIPRJMOD` included), so kicad-cli ERC and DRC resolve the same libraries and a footprint's 3D model written `${TEAM}/part.step` is found in the 3D export (slow-tier tests: ERC `lib_symbol_issues`, DRC `lib_footprint_issues`, the glb) |
| Importing a KiCad project | studio addition | `eda import-kicad board.kicad_pcb -o dir` brings the project's own `sym-lib-table` / `fp-lib-table` and the `text_variables` of its `.kicad_pro` (the names a path variable may have, never `KIPRJMOD`) into the board's folder, with `${KIPRJMOD}` written out as the project's folder, so the project's libraries still resolve from the new folder and stay where they are; a table or variables file the folder already has is kept (`library_table::import_project_setup`, unit tested; the command says what it brought) |
| Guards | studio addition | the studio never writes KiCad's configuration: a board folder that is, or lies inside, KiCad's configuration folder is refused by every route that writes (tables, New / Add Library, path variables; tested against a copy of the configuration); the nickname `eda` is the studio's own project library (`eda:Name`) and is refused for a table library, by the server and by the dialog's OK check |
| Manage Symbol Libraries / Manage Footprint Libraries | partial | `DIALOG_EDIT_LIBRARY_TABLES` + `PANEL_SYM_LIB_TABLE` / `PANEL_FP_LIB_TABLE`, `LIB_TABLE_GRID_DATA_MODEL`, `LIB_TABLE_GRID_TRICKS`: a Global and a Project Specific tab over the grid (status, Enable, Show for symbols, Nickname, Library Path, Library Format, Options, Description); the Global tab and the tables it chains are read only and say so; the buttons are add row, Add Existing (KiCad file or folder, Table), move up / down, delete; Activate / Deactivate Selected and Set / Unset Visible Flag in the row menu; a path gives the row the format it says and its status (KiCad's words: "Library '%s' not found.", "Nested table '%s' not found."); the Available path substitutions list; OK is `VerifyTable` (a row with no path is dropped, a nickname is needed and legal, no two alike) and then writes the board's table. Not ported: Reset Libraries and Migrate Libraries (the global table is KiCad's; the studio reads KiCad-format libraries only, other formats are listed with a warning and kept), Edit Options..., symbol folders of `.kicad_sym` files, pasting table text |
| Add Existing, New Library, Add Library | partial | `browseLibrariesHandler`, `SYMBOL_EDIT_FRAME::AddLibraryFile`, `PCB_BASE_EDIT_FRAME::CreateNewLibrary` / `AddLibrary`: the nickname is the file's name (`LIB_ID::FixIllegalChars`; a footprint folder that is not a `.pretty` keeps its whole name), the path is written with a variable or `${KIPRJMOD}` where one holds it (`NormalizePath`), a nickname a library already has is refused ("Library '%s' already exists."), New Library makes an empty `.kicad_sym` file or `.pretty` folder. The file dialog is a folder browser over the server's folders (a page cannot see the machine's), and there is no "global table" choice: the row goes into the board's table. A duplicate nickname in Add Existing asks Skip / Add Anyway |
| Configure Paths | partial | `DIALOG_CONFIGURE_PATHS`: KiCad's variables listed read only (grey where the real environment defines them, which takes precedence; a variable a table uses and nothing defines is listed with a Define button), and the board's own variables editable with `ENV_VAR_NAME_VALIDATOR`'s name rule (capitals, digits, `_`), "Environment variable name / path cannot be empty." and `KIPRJMOD` reserved, a Help text. The board's variables are KiCad's project text variables (`path-variables.json` beside the board), not KiCad's settings, which the studio does not write |
| Saving into a table library | **not done** | the Symbol and Footprint editors write the project library of `design.json`; Save, Save As and Export do not write a table library's file. A new library made here is empty until a file is put into it |

