// The Design Blocks actions of the schematic and the board editor (commit 8303b2ad):
//
//   common/tool/design_block_control.cpp      DESIGN_BLOCK_CONTROL::DeleteDesignBlock / EditDesignBlockProperties / HideLibraryTree
//   eeschema/tools/sch_design_block_control.cpp   SCH_DESIGN_BLOCK_CONTROL::SaveSheetAsDesignBlock / SaveSelectionAsDesignBlock / UpdateDesignBlockFromSheet / ...FromSelection
//   pcbnew/tools/pcb_design_block_control.cpp     PCB_DESIGN_BLOCK_CONTROL::SaveBoardAsDesignBlock / SaveSelectionAsDesignBlock / UpdateDesignBlockFromBoard / ...FromSelection
//   eeschema/sch_design_block_utils.cpp, pcbnew/pcb_design_block_utils.cpp   SCH_EDIT_FRAME / PCB_EDIT_FRAME::SaveSheetAsDesignBlock, SaveSelectionAsDesignBlock, Update...
//   common/widgets/design_block_pane.cpp      DESIGN_BLOCK_PANE::EditDesignBlockProperties / DeleteDesignBlockFromLibrary
//   eeschema/tools/sch_drawing_tools.cpp      SCH_DRAWING_TOOLS::ImportSheet (placingDesignBlock) / DrawSheet (drawSheetFromDesignBlock)
//   eeschema/tools/sch_editor_control.cpp     SCH_EDITOR_CONTROL::PlaceLinkedDesignBlock / SaveToLinkedDesignBlock
//   pcbnew/tools/pcb_control.cpp              PCB_CONTROL::AppendDesignBlock / PlaceLinkedDesignBlock / SaveToLinkedDesignBlock
//
// What the actions do is the server's (crates/cli/src/design_block_api.rs: the library, the files it writes, the checks KiCad's frames make before a dialog) and the ops
// commands' (`import_sheet` with a `block`, `place_design_block`, `link_design_block`: one undo step each); this file asks the questions KiCad's dialogs ask, in KiCad's order
// and words, and puts the answers to the server. The questions are `state/designBlockDialogs.ts`'s (components/designBlocks/DesignBlockDialogs.tsx shows them).
// `pcbnew.InteractiveDrawing.applyDesignBlockLayout` (PCB_CONTROL::ApplyDesignBlockLayout) is here too: the block's board layout is repeated on the footprints of the selected group with the
// multichannel tool (`RepeatLayout`, crates/ops/src/multichannel_copy.rs), as one command that takes the block's board file.
import type { Dispatch } from "react";
import { fetchSchematic, fetchState } from "../api/client";
import { deleteBlock, fetchBlockFile, prepareSave, saveBlock, saveBlockProperties, type Block, type BlockLibrary, type Refused, type SaveSource } from "../api/designBlockClient";
import { groupsOf } from "../kicad-port/schGroups";
import { findBlock, parseBlockId, placeProblem, schematicPlacement } from "../kicad-port/designBlocks";
import { askBlockConfirm, askBlockProperties } from "../state/designBlockDialogs";
import { ensureDesignBlocks, getDesignBlocksUi, refreshDesignBlocks, selectDesignBlock, setDesignBlockLibraryPinned, setDesignBlockPaneShown, type BlockEditor } from "../state/designBlocks";
import { setDockColumnCollapsed } from "../state/dockLayoutStore";
import { openLibraryFile } from "../state/libraryTableDialogs";
import type { Action, StudioApi } from "../state/store";
import type { ActionHandler } from "./commonActions";

export interface DesignBlockActionContext {
  tab: string;
  dispatch: Dispatch<Action>;
  api: StudioApi;
}

/** What the pane has selected, or the block an id names: its library and the listing's entry for it. */
interface Chosen {
  id: string;
  lib: string;
  name: string;
  library: BlockLibrary | undefined;
  block: Block | undefined;
}

function chosen(id: string | null): Chosen | null {
  const { lib, name } = parseBlockId(id);
  // `GetSelectedLibId().IsValid()`: a library row (`Lib:`) or nothing is no design block
  if (!lib || !name) return null;
  const library = (getDesignBlocksUi().reply?.libraries ?? []).find((l) => l.name === lib);
  return { id: `${lib}:${name}`, lib, name, library, block: findBlock(getDesignBlocksUi().reply?.libraries ?? [], `${lib}:${name}`) };
}

/** The one group the selection is, when it has a design block link (`groupWithDesignBlockLink`). */
function linkedGroup(ctx: DesignBlockActionContext): { id: string; link: string } | null {
  const st = ctx.api.getState();
  if (st.selection.size !== 1) return null;
  const only = [...st.selection][0]!;
  const groups = ctx.tab === "schematic" ? groupsOf(st.schematic) : (st.board?.drawings?.groups ?? []);
  const group = groups.find((g) => g.id === only);
  return group && (group.design_block ?? "") !== "" ? { id: group.id, link: group.design_block! } : null;
}

const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e));

export function registerDesignBlockActions(m: Map<string, ActionHandler>, ctx: DesignBlockActionContext): void {
  if (ctx.tab !== "schematic" && ctx.tab !== "pcb") return;
  const editor: BlockEditor = ctx.tab;
  const toast = (message: string, kind: "info" | "error" = "info") => ctx.dispatch({ type: "TOAST", message, kind });
  const say = (r: Refused) => toast(r.message, "error");

  /** The libraries as the server lists them (the pane lists them when it opens; a hotkey may come first). */
  const listing = async () => {
    await ensureDesignBlocks().catch(() => null);
  };

  // ------------------------------------------------------------------------------------------------------------------------------------- placing

  /**
   * `SCH_DRAWING_TOOLS::ImportSheet` with `placingDesignBlock`: the block's schematic is placed the way Import Sheet places a file -- as a sheet (two corners, the sheet's dialog)
   * or as its contents (one click), grouped and linked to the block when "Place as group" is on, the symbols numbered anew unless "Keep annotations". `forceSheet` is
   * `drawSheetFromDesignBlock`: a sheet, whatever "Place as sheet" says. The tool is armed (`state.schSheetImport`); the clicks are components/schematic/schImportTool.ts's.
   */
  const placeInSchematic = async (id: string | null, forceSheet = false) => {
    await listing();
    const c = chosen(id);
    if (!c) return;
    const problem = placeProblem(c.block, "schematic", c.id);
    if (problem || !c.block?.schematic_path) return toast(problem ?? "Design block has no schematic to place.", "error");
    const options = getDesignBlocksUi().options;
    ctx.dispatch({ type: "SET_SCH_SHEET_IMPORT", plan: { ...schematicPlacement(c.block.schematic_path, c.id, options), asSheet: forceSheet || options.asSheet, suffix: 1 } });
  };

  /**
   * `PCB_CONTROL::AppendDesignBlock` -> `AppendBoard( ..., designBlock )`: the block's board is put on the board with its origin at the cursor and carried by the Move tool until a
   * click drops it (Escape takes it back), as Append Board does; with "Place as group" what came in is grouped and linked to the block. With "Place repeated copies" the next copy starts
   * when this one is dropped (`do { AppendBoard } while( repeatPlacement && ret == 0 )`). One undo step per copy.
   */
  const placeOnBoard = async (id: string | null, repeat: boolean) => {
    await listing();
    const c = chosen(id);
    if (!c) return;
    const problem = placeProblem(c.block, "pcb", c.id);
    if (problem) return toast(problem, "error");
    let file: Awaited<ReturnType<typeof fetchBlockFile>>;
    try {
      file = await fetchBlockFile(c.lib, c.name, "pcb");
    } catch (e) {
      return toast(messageOf(e), "error");
    }
    if (!file.ok) return say(file);
    const text = file.text;
    const options = getDesignBlocksUi().options;
    await ctx.api.placeNewItems(
      (at) => ({ op: "place_design_block", text, at, block: c.id, as_group: options.asGroup, group_name: c.name }),
      true,
      repeat && options.repeated ? () => void placeOnBoard(id, true) : undefined
    );
  };

  /** `PlaceLinkedDesignBlock`: the block the selected group is linked to is placed again (the pane selects it first, `SelectLibId`). */
  const placeLinked = async () => {
    const g = linkedGroup(ctx);
    if (!g) return;
    await listing();
    const c = chosen(g.link);
    if (!c || !c.block) return toast(`Could not find design block ${g.link}.`, "error");
    if (editor === "schematic" ? !c.block.schematic : !c.block.board) return toast(`Design block ${g.link} does not have a ${editor === "schematic" ? "schematic" : "board"} file.`, "error");
    selectDesignBlock(c.id);
    if (editor === "schematic") await placeInSchematic(c.id);
    // the board's linked placement is `AppendBoard` once, with no repeat loop
    else await placeOnBoard(c.id, false);
  };

  /**
   * `PCB_CONTROL::ApplyDesignBlockLayout`: the group selected, linked to a design block, takes the block's layout. Nothing happens for a selection that is not one linked group
   * (the menu entry is there for every group); the block must be found and have a board file. The topology of the group's footprints and the block's must match, and none may be locked:
   * the server refuses in KiCad's words if not.
   */
  const applyLayout = async () => {
    const g = linkedGroup(ctx);
    if (!g) return toast("Select one group linked to a design block to apply its layout.", "error");
    await listing();
    const c = chosen(g.link);
    if (!c || !c.block) return toast(`Could not find design block ${g.link}.`, "error");
    if (!c.block.board) return toast(`Design block ${g.link} does not have a board file.`, "error");
    let file: Awaited<ReturnType<typeof fetchBlockFile>>;
    try {
      file = await fetchBlockFile(c.lib, c.name, "pcb");
    } catch (e) {
      return toast(messageOf(e), "error");
    }
    if (!file.ok) return say(file);
    const ok = await ctx.api.cmd({ op: "multichannel", verb: "apply_design_block_layout", group: g.id, block: c.id, text: file.text });
    if (ok) toast(`Applied the layout of design block ${c.id} to the group.`);
  };

  // ------------------------------------------------------------------------------------------------------------------------------------- saving

  /** The ids of the groups of the sheet in view or of the board. */
  const groupIds = (): Set<string> => {
    const st = ctx.api.getState();
    return new Set((ctx.tab === "schematic" ? groupsOf(st.schematic) : (st.board?.drawings?.groups ?? [])).map((g) => g.id));
  };

  const afterSaved = async (r: { lib: string; name: string; lib_id: string; linked?: boolean; link_error?: string | null }, groupsBefore: ReadonlySet<string>) => {
    await refreshDesignBlocks();
    // `m_designBlocksPane->RefreshLibs(); SelectLibId( blk.GetLibId() )`
    selectDesignBlock(r.lib_id);
    toast(`Design block '${r.name}' saved to library '${r.lib}'.`);
    if (r.link_error) toast(r.link_error, "error");
    if (!r.linked) return;
    // the group the save made (or linked) is a change of the design: read it now rather than at the next poll, and select the new group (`selectItem( newGroup )` after "Group Items")
    try {
      const groups = ctx.tab === "schematic" ? groupsOf(await fetchSchematic(ctx.api.getState().currentSheetPath)) : ((await fetchState()).drawings?.groups ?? []);
      await ctx.api.refresh();
      const made = groups.find((g) => !groupsBefore.has(g.id) && g.design_block === r.lib_id);
      if (made) ctx.dispatch({ type: "SET_SELECTION", refs: [made.id] });
    } catch {
      // the poll shows the group; only its selection is lost
    }
  };

  /**
   * Save Current Sheet / Board as Design Block..., Save Selection as Design Block..., Update Design Block from Current Sheet / Board and from Selection: `source` is what is
   * saved. A new block asks for its name and metadata (`DIALOG_DESIGN_BLOCK_PROPERTIES`), then whether to overwrite one of that name (`checkOverwriteDb`); an update asks whether to
   * overwrite the design the block has (`checkOverwriteDbSchematic` / `checkOverwriteDbLayout`) and, for a sheet, shows the dialog with the name locked.
   */
  const save = async (source: SaveSource, update: boolean) => {
    await listing();
    const ui = getDesignBlocksUi();
    const picked = parseBlockId(ui.selected);
    // the "Recently Used" group is no library to save into
    const here = (ui.reply?.libraries ?? []).some((l) => l.name === picked.lib) ? picked : { lib: "", name: "" };
    const selection = source.what === "selection" || source.what === "board_selection";
    // the frames check these in this order: some items selected, then a library (or a block, for an update)
    if (selection && source.ids.length === 0) return toast("Please select some items to save as a design block.", "error");
    if (!here.lib) return toast(update ? "Please select a design block to save the schematic to." : "Please select a library to save the design block to.", "error");
    if (update && !here.name) return toast(editor === "schematic" ? "Please select a design block to save the schematic to." : "Please select a library to save the design block to.", "error");

    let prepared;
    try {
      prepared = await prepareSave({ ...source, lib: here.lib, ...(update ? { update: { name: here.name } } : {}) });
    } catch (e) {
      return toast(messageOf(e), "error");
    }
    if (!prepared.ok) return say(prepared);

    let values: { name: string; description: string; keywords: string; fields: { name: string; value: string }[] } | null = null;
    if (update) {
      const board = editor === "pcb";
      if (prepared.has_design) {
        const yes = await askBlockConfirm({
          title: "Confirmation",
          message: `Design block '${here.name}' already has a ${board ? "layout" : "schematic"}.`,
          detail: board ? "Overwrite existing layout?" : "Overwrite existing schematic?",
          okLabel: "Overwrite",
        });
        if (!yes) return;
      }
      // only the sheet's update shows the dialog (name locked); the others keep the block's metadata
      if (source.what === "sheet") {
        values = await askBlockProperties({ lib: here.lib, initial: { name: prepared.name, description: prepared.description, keywords: prepared.keywords, fields: prepared.fields }, nameLocked: true, okLabel: "Save" });
        if (!values) return;
      }
    } else {
      values = await askBlockProperties({ lib: here.lib, initial: { name: prepared.name, description: prepared.description, keywords: prepared.keywords, fields: prepared.fields }, nameLocked: false, okLabel: "Save" });
      if (!values) return;
    }

    const name = update ? here.name : values!.name;
    const groupsBefore = groupIds();
    const body = { ...source, lib: here.lib, name, ...(values ? { description: values.description, keywords: values.keywords, fields: values.fields } : {}), ...(update ? { update: true } : {}) };
    let saved;
    try {
      saved = await saveBlock(body);
      if (!saved.ok && saved.exists) {
        const yes = await askBlockConfirm({ title: "Confirmation", message: saved.message, detail: "Overwrite existing design block?", okLabel: "Overwrite" });
        if (!yes) return;
        saved = await saveBlock({ ...body, overwrite: true });
      }
    } catch (e) {
      return toast(messageOf(e), "error");
    }
    if (!saved.ok) return say(saved);
    await afterSaved(saved, groupsBefore);
  };

  const sheetPath = () => ctx.api.getState().currentSheetPath.join("/");
  const selectedIds = () => [...ctx.api.getState().selection];

  /** `DESIGN_BLOCK_PANE::EditDesignBlockProperties`. */
  const properties = async () => {
    await listing();
    const c = chosen(getDesignBlocksUi().selected);
    if (!c) return;
    if (c.library && !c.library.writable) return toast(`Library '${c.lib}' is read only.`, "error");
    if (!c.block) return;
    const values = await askBlockProperties({ lib: c.lib, initial: { name: c.block.name, description: c.block.description, keywords: c.block.keywords, fields: c.block.fields.map((f) => ({ ...f })) }, nameLocked: false });
    if (!values) return;
    const body = { lib: c.lib, name: c.name, new_name: values.name !== c.name ? values.name : undefined, description: values.description, keywords: values.keywords, fields: values.fields };
    let r;
    try {
      r = await saveBlockProperties(body);
      if (!r.ok && r.exists) {
        const yes = await askBlockConfirm({ title: "Confirmation", message: r.message, detail: "Overwrite existing design block?", okLabel: "Overwrite" });
        if (!yes) return;
        r = await saveBlockProperties({ ...body, overwrite: true });
      }
    } catch (e) {
      return toast(messageOf(e), "error");
    }
    if (!r.ok) return say(r);
    await refreshDesignBlocks();
    selectDesignBlock(r.lib_id);
  };

  /** `DESIGN_BLOCK_PANE::DeleteDesignBlockFromLibrary( id, true )`. */
  const remove = async () => {
    await listing();
    const c = chosen(getDesignBlocksUi().selected);
    if (!c) return;
    if (c.library && !c.library.writable) return toast(`Library '${c.lib}' is read only.`, "error");
    const yes = await askBlockConfirm({ title: "Confirmation", message: `Delete design block '${c.name}' in library '${c.lib}' from disk?`, okLabel: "OK" });
    if (!yes) return;
    let r;
    try {
      r = await deleteBlock(c.lib, c.name);
    } catch (e) {
      return toast(messageOf(e), "error");
    }
    if (!r.ok) return say(r);
    toast(r.message);
    await refreshDesignBlocks();
  };

  /** `SaveToLinkedDesignBlock`: the group's block is the pane's selection, and `updateDesignBlockFromSelection` runs. */
  const saveToLinked = async () => {
    const g = linkedGroup(ctx);
    if (!g) return;
    await listing();
    const c = chosen(g.link);
    if (!c || !c.block) return toast(`Could not find design block ${g.link}.`, "error");
    selectDesignBlock(c.id);
    await save(editor === "schematic" ? { what: "selection", sheet: sheetPath(), ids: selectedIds() } : { what: "board_selection", ids: selectedIds() }, true);
  };

  // ------------------------------------------------------------------------------------------------------------------------------------- the actions

  // ACTIONS::pinLibrary / unpinLibrary / newLibrary / hideLibraryTree in the schematic and the board editor are the pane's (`DESIGN_BLOCK_CONTROL::setTransitions`): the library of
  // the node the pane has selected is pinned or let go, "New Library..." makes a design block library (`CreateNewDesignBlockLibrary`), and the pane is hidden.
  const pin = (pinned: boolean) => {
    const { lib } = parseBlockId(getDesignBlocksUi().selected);
    if (lib && (getDesignBlocksUi().reply?.libraries ?? []).some((l) => l.name === lib)) setDesignBlockLibraryPinned(lib, pinned);
  };
  m.set("common.Control.pinLibrary", () => pin(true));
  m.set("common.Control.unpinLibrary", () => pin(false));
  m.set("common.Control.newLibrary", () => openLibraryFile("design_block", true));
  m.set("common.Control.hideLibraryTree", () => setDesignBlockPaneShown(editor, false));

  if (editor === "schematic") {
    // SCH_ACTIONS::showDesignBlockPanel ("Design Blocks"): the pane shows or hides.
    m.set("eeschema.SchDesignBlockControl.showDesignBlockPanel", () => showPane("schematic"));
    // SCH_ACTIONS::placeDesignBlock (Shift+B) -- the block the pane has selected.
    m.set("eeschema.InteractiveDrawing.placeDesignBlock", () => void placeInSchematic(getDesignBlocksUi().selected));
    // SCH_ACTIONS::drawSheetFromDesignBlock -- posted by `ImportSheet` once "Place as sheet" is on; here it places the block as a sheet whatever the box says.
    m.set("eeschema.InteractiveDrawing.drawSheetFromDesignBlock", (arg) => void placeInSchematic(typeof arg === "string" ? arg : getDesignBlocksUi().selected, true));
    m.set("eeschema.InteractiveDrawing.placeLinkedDesignBlock", () => void placeLinked());
    m.set("eeschema.InteractiveDrawing.saveToLinkedDesignBlock", () => void saveToLinked());
    m.set("eeschema.SchDesignBlockControl.saveSheetAsDesignBlock", () => void save({ what: "sheet", sheet: sheetPath() }, false));
    m.set("eeschema.SchDesignBlockControl.saveSelectionAsDesignBlock", () => void save({ what: "selection", sheet: sheetPath(), ids: selectedIds() }, false));
    m.set("eeschema.SchDesignBlockControl.updateDesignBlockFromSheet", () => void save({ what: "sheet", sheet: sheetPath() }, true));
    m.set("eeschema.SchDesignBlockControl.updateDesignBlockFromSelection", () => void save({ what: "selection", sheet: sheetPath(), ids: selectedIds() }, true));
    m.set("eeschema.SchDesignBlockControl.editDesignBlockProperties", () => void properties());
    m.set("eeschema.SchDesignBlockControl.deleteDesignBlock", () => void remove());
  } else {
    m.set("pcbnew.PcbDesignBlockControl.showDesignBlockPanel", () => showPane("pcb"));
    m.set("pcbnew.InteractiveDrawing.placeDesignBlock", () => void placeOnBoard(getDesignBlocksUi().selected, true));
    m.set("pcbnew.InteractiveDrawing.placeLinkedDesignBlock", () => void placeLinked());
    m.set("pcbnew.InteractiveDrawing.applyDesignBlockLayout", () => void applyLayout());
    m.set("pcbnew.InteractiveDrawing.saveToLinkedDesignBlock", () => void saveToLinked());
    m.set("pcbnew.PcbDesignBlockControl.saveBoardAsDesignBlock", () => void save({ what: "board" }, false));
    m.set("pcbnew.PcbDesignBlockControl.saveSelectionAsDesignBlock", () => void save({ what: "board_selection", ids: selectedIds() }, false));
    m.set("pcbnew.PcbDesignBlockControl.updateDesignBlockFromBoard", () => void save({ what: "board" }, true));
    m.set("pcbnew.PcbDesignBlockControl.updateDesignBlockFromSelection", () => void save({ what: "board_selection", ids: selectedIds() }, true));
    m.set("pcbnew.PcbDesignBlockControl.editDesignBlockProperties", () => void properties());
    m.set("pcbnew.PcbDesignBlockControl.deleteDesignBlock", () => void remove());
  }
}

/** `showDesignBlockPanel`: `PANE_INFO.Show( !IsShown() )`. Showing it brings its column back when that was folded to its handle. */
function showPane(editor: BlockEditor): void {
  const shown = getDesignBlocksUi().shown[editor];
  setDesignBlockPaneShown(editor, !shown);
  if (!shown) setDockColumnCollapsed("right", false);
}
