# Shared (`common.*`) action parity with KiCad

One row per behaviour of the shared actions KiCad runs in every editor frame: `COMMON_TOOLS` (`common_tools.cpp`), `COMMON_CONTROL`
(`common_control.cpp`), `LIBRARY_EDITOR_CONTROL`, the selection, group and picker tools, the Checker's marker stepping, and the dialogs they open.
Same status words as `PARITY-pcb.md`: **identical** (same logic and constants, browser adaptations noted), **partial** (the core is ported, a real
gap is named) and **missing** (not wired; every such action has its reason in `tools/ui-parity-missing.json`, which `tools/ui-parity-audit.mjs`
checks, and `docs/parity/UI-ACTIONS.md` lists).

KiCad source: the read-only checkout at commit `8303b2ad` (`common/`, `pcbnew/`, `eeschema/`).

Of KiCad's 183 `common.*` actions, 143 are wired and 40 carry a recorded reason (section 11); none is left unexplained. These tools run in the PCB
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
| Group Properties | partial | `DIALOG_GROUP_PROPERTIES`: name and member list, the plus button hides the dialog and asks for a click on a new member (`PCB_GROUP_TOOL::PickNewMember`), the trash button drops a row, a click on a row shows the item. OK is one undo step (`Cmd::EditGroup`: rename, replace the members, an item leaves any other group, under two members dissolves it). Not ported: the Locked box and the design-block library link |
| Add Items to Group, Remove Items from Group | identical | `GROUP_TOOL::AddToGroup` / `RemoveFromGroup` under `GROUP_CONTEXT_MENU`'s conditions, listed in the board's context menu where they apply |

## 5. Checker markers, Find and Replace

| Behaviour | Status | KiCad file:function |
|---|---|---|
| Next Marker, Previous Marker | identical | `DRC_TOOL::NextMarker` / `PrevMarker`, `SCH_INSPECTION_TOOL`'s, over `RC_TREE_MODEL`'s order: step the open DRC / ERC list, select the items the marker names and frame them |
| Exclude Marker | identical (schematic) | `DIALOG_ERC::ExcludeMarker` -> `add_erc_exclusion`; the board has no DRC exclusions to store, so the entry stays off on the PCB tab |
| Replace, Replace All, Update Find, Find and Replace | identical | the schematic find dialog's own actions (`SCH_EDITOR_CONTROL::ReplaceAndFindNext` / `ReplaceAll`, `UpdateFind`); matches are brightened while the dialog is open |
| Activate Point Editor, Update Menu | identical in effect | satisfied by construction: the point editor is always on, the menus read state |

## 6. Suite actions: About, Help, saving

| Behaviour | Status | KiCad file:function |
|---|---|---|
| About | partial | `DIALOG_ABOUT`: title and version, the three pages (About, Version, License), Donate, Report Bug and Copy Version Info (`GetVersionInfoData`'s text). Not ported: the contributor pages (Developers, Doc Writers, Librarians, Artists, Translators, Packagers) -- the About page points to KiCad's list instead of copying it |
| Help, Getting Started, Get Involved, Donate, Report Bug | partial | `COMMON_CONTROL::ShowHelp` and the others: the frame's online manual (`go.kicad.org/docs/<major.minor>/<language>/<help_name>/`; there is no installed copy to prefer), the beginners' guide, the involvement and donation pages. Report Bug opens an issue on this program's repository with the version information, not KiCad's tracker. The **Help** menu (`EDA_BASE_FRAME::AddStandardHelpMenu`) was missing from the extracted menu data, so the menu bar appends it |
| Save a Copy (PCB), Save All | identical in effect | `BOARD_EDITOR_CONTROL::SaveCopy`: the derived `.kicad_pcb` handed to the browser's Save, the same as Save As (design.json is the only master). `SYMBOL_EDITOR_CONTROL::Save( saveAll )`: every edit is already written when it is made, so Save All only reports |
| Save As (Footprint Editor) | partial | `FOOTPRINT_EDITOR_CONTROL::SaveAs` -> `SaveFootprintAs` and `SAVE_AS_DIALOG` ("Save Footprint As": name, "Save in library:" list, New Library...): the footprint the tree selects (else the loaded one) is stored under the name and library chosen as one `put_library_footprint` (undoable); a name the library has asks "Footprint %s already exists in %s." with Overwrite; the editor moves to the copy when it was the loaded footprint. The board's and the schematic's Save As (the derived KiCad files, in `useActionRunner.ts`) are separate. Not ported: Save Library As (a selected library row; there is no library file to copy -- it says so), and the Value field following the new name (a library footprint has no Value of its own here). The Symbol Editor's Ctrl+Shift+S is `saveLibraryAs` |
| Update Schematic from PCB | identical in effect | `UpdateSchematicFromPCB` (back annotation): the PCB cannot change a footprint's reference, value or assignment on its own (they are read-only in Footprint Properties), so there is nothing to carry over; like F8 the action refetches the design and says so |
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
| Edit Grids... | partial | `COMMON_TOOLS::GridProperties` -> `PANEL_GRID_SETTINGS`: a Preferences - Grids dialog on the editor that asked (PCB, Footprint or Symbol editor, with the others one step away): the list with Add (before the selected row) / Edit / Remove / Move Up / Move Down, the size checked as `DIALOG_GRID_SETTINGS` does (0.001 to 1000 mm, "already exists"), Fast Grid 1 and 2 with their hotkeys, Reset to Defaults (`ResetPanel`, fast grids keep their size or fall back to the first / last entry), and OK makes the selected row the editor's current grid. Each editor has its own list (kept in local storage, `state/gridSettings.ts`; KiCad keeps it in each editor's settings file); defaults are `DefaultGridSizeList` (the PCB list for the board and footprint editors, 100 / 50 / 25 / 10 mil for the symbol editor). The grid boxes end with "---" and "Edit Grids..." (`OnSelectGrid`), and the Show Grid button's right-click menu offers Edit Grids... and Grid Origin... (`WithContextMenu`). Not ported: a grid's name, a different Y size (the studio snaps to one square grid) and the grid overrides (below). The schematic does not offer it: its grid is the fixed 50 mil |
| Next Grid, Previous Grid, grid presets, Fast Grid 1 / 2 / Cycle | identical (PCB, Footprint, Symbol) | `COMMON_TOOLS::GridNext` / `GridPrev` / `GridPreset` / `GridFast1/2/Cycle` over the editor's own list: wrap round, clamp, the cursor goes to the new grid, a hotkey shows which grid it is. The fast grids were the board's alone, and Next / Previous Grid on the library tabs changed the board's grid; the schematic's grid is the fixed 50 mil, so none of them is offered there |
| Toggle Grid Overrides | missing | needs per-item-type grids and category-aware snapping; see section 11 |

## 11. Not wired, with the reason (40)

All in `tools/ui-parity-missing.json`; each is a subsystem the studio does not have, and no fake handler stands in for it.

| Actions | Reason |
|---|---|
| New, Open, Quit, Revert, Show Project Manager | one project directory chosen at launch, no project manager; a web page has no application to quit; every edit is already in design.json, so there is nothing to revert |
| Add Library, New Library, Add Library by drop, Symbol / Footprint / Design Block Library Tables, Configure Paths | the studio has no library tables or design-block library (`docs/parity/GAPS.md` items 5 and 24) |
| Open Directory, Open with Text Editor | a browser cannot open the operating system's file manager or editor |
| Plugins Reload | no Python scripting or API plugins |
| Calculator Tools | a separate program in KiCad |
| Undo / Redo Zoom, Zoom In / Out Horizontally / Vertically | the simulator plot's zoom history and axes; there is no simulator (`GAPS.md`: Simulator, last) |
| Select Rows / Columns / Table, the Table Editor (add / delete rows and columns, merge, unmerge, export CSV) | the design IR has no table type |
| Embed File, Extract File, Remove File | the IR has no embedded-file list (KiCad stores them zstd-compressed, base64-encoded and checksummed) |
| Paste Special | nothing to special-case yet: the clipboard holds no footprints or symbols and the copper it holds must name a net |
| Toggle Grid Overrides | the five category grids (connected items, wires, vias, text, graphics) and the category-aware snapping that reads them |

## 12. Offered by KiCad in an editor, not by the studio there

The wired actions above are registered only on the tabs where they do something. These are the ones KiCad offers in an editor that the studio leaves dimmed
there (or, for Group / Ungroup on the schematic, leaves enabled and inert), with where the reason is kept.

| Action | Editor | Why |
|---|---|---|
| Find, Find Next / Previous, Find and Replace, Update Find, Find Next Marker | PCB | `GAPS.md` item 21: there is no Find on the board; the actions are the schematic find dialog's |
| Find, Find and Replace | Symbol Editor | `editor_toolbar_support.json`: Find searches the schematic, not an open library symbol |
| Exclude Marker | PCB | DRC exclusions are not modelled (ERC's are) |
| Left / Center / Right Justify | Schematic | `SchematicText` has no justification yet (`GAPS.md` item 12) |
| Group Properties, Add / Remove Items, New Group Member | Schematic | the schematic has no groups (`GAPS.md` item 17); Group / Ungroup are registered for the board only and do nothing there |
| Group, Ungroup | Footprint Editor | `editor_toolbar_support.json`: a library footprint has no groups |
| Edit Grids, grid presets, Next / Previous Grid, Fast Grids, Grid Origin | Schematic | its grid is the fixed 50 mil (`GAPS.md` item 14); a grid origin is the board's and the Footprint Editor's |
| Show Properties | Footprint and Symbol editors | `editor_toolbar_support.json`: no Properties pane yet (the Pad, Footprint, Symbol and Pin Properties dialogs are there) |
| Show Datasheet | Board | KiCad offers it in the schematic, the Symbol Editor and the Footprint Editor only |

## 13. The scripted test hook

`window.__eda` (`kicad-port/edaTestHook.ts` + `actions/useEdaTestHook.ts`) is there for agents and test scripts: `actions()` lists the actions the
runner handles on the current tab with whether each is enabled and why not, `await run( id, args )` runs one exactly as a menu click does and waits for
every round trip it started, `state()` is a small snapshot (tab, revision, tool, picker, selection with kinds, item counts, the editor's grid, the
dialogs on screen, the open dialog and panel flags) and `errors()` the errors since the page loaded. Nothing in it changes what the studio does.
