// Customised toolbars (KiCad 8303b2ad): the toolbar model of include/tool/ui/toolbar_configuration.h (`TOOLBAR_ITEM`, `TOOLBAR_CONFIGURATION`, `TOOLBAR_LOC`), its JSON
// (common/tool/ui/toolbar_configuration.cpp: `to_json` / `from_json`, `TOOLBAR_SETTINGS`' "toolbars" param, the frame's `<frame>-toolbars.json`), which toolbar a frame
// shows (`TOOLBAR_SETTINGS::GetToolbarConfig( loc, m_CustomToolbars )`, common/eda_base_frame.cpp `configureToolbars`), and what Preferences > <editor> > Toolbars does
// (`PANEL_TOOLBAR_CUSTOMIZATION`, common/dialogs/panel_toolbar_customization.cpp: the shadow toolbars, the tree, Insert Separator / Spacer / Group, the Add button,
// Delete, Move Up / Move Down, the actions list and its filter, `ResetPanel`, `TransferDataFromWindow`).
//
//   * A toolbar is a list of items: a tool (an action), a separator, a spacer of some pixels, a control (a combo box such as the grid or zoom box,
//     `ACTION_TOOLBAR_CONTROL`, by its name "control.GridSelector") and a group (a named button over several tools, `TOOLBAR_GROUP_CONFIG`).
//   * A frame has up to four toolbars, `TOOLBAR_LOC` LEFT, RIGHT, TOP_MAIN and TOP_AUX (the studio's "options", "drawing", "main" and "auxiliary"); one the frame
//     has no default for (`DefaultToolbarConfig` returns nullopt) does not exist, whatever is stored for it.
//   * With "Customize toolbars" on (`APP_SETTINGS_BASE::m_CustomToolbars`, `appearance.custom_toolbars`, default off) a toolbar that has a stored configuration
//     shows it; otherwise the default.
//   * The page keeps a shadow copy of every toolbar, edits the shown one as a tree (a group's tools are its children) and writes them all back on OK, each control
//     on one toolbar only (`removeControlFromOtherToolbars` when one is added, the de-duplication of `TransferDataFromWindow`).
//
// Pure: no store, no DOM, no JSON import; the studio's default toolbars and its actions come in as arguments (state/toolbarSettings.ts reads the files).
// Unit-tested in toolbarCustomization.test.ts.
import type { ToolbarId, ToolbarItem } from "../kicad/types";

/** The frames that have a Toolbars page: the PCB, schematic, footprint and symbol editors and the 3D viewer (`FRAME_PCB_EDITOR`, `FRAME_SCH`, ...). */
export type ToolbarFrame = "pcb" | "schematic" | "footprint" | "symbol" | "3d";

/** `TOOLBAR_LOC`, in its declaration order (`magic_enum::enum_values`, which the page's toolbar choice follows). */
export type ToolbarLoc = "LEFT" | "RIGHT" | "TOP_MAIN" | "TOP_AUX";
export const TOOLBAR_LOCS: readonly ToolbarLoc[] = ["LEFT", "RIGHT", "TOP_MAIN", "TOP_AUX"];

/** `s_toolbarNameMap`: the toolbar choice's names. */
export const TOOLBAR_LOC_NAME: Record<ToolbarLoc, string> = { LEFT: "Left", RIGHT: "Right", TOP_MAIN: "Top main", TOP_AUX: "Top auxiliary" };

/** The studio's toolbar ids (src/kicad/*toolbars.json) for KiCad's locations: the options toolbar is LEFT, the drawing toolbar RIGHT. */
export const LOC_OF_TOOLBAR_ID: Record<ToolbarId, ToolbarLoc> = { main: "TOP_MAIN", options: "LEFT", drawing: "RIGHT", auxiliary: "TOP_AUX" };
export const TOOLBAR_ID_OF_LOC: Record<ToolbarLoc, ToolbarId> = { TOP_MAIN: "main", LEFT: "options", RIGHT: "drawing", TOP_AUX: "auxiliary" };

/** The frame's settings file names (`GetToolbarSettings<..>( "pcbnew-toolbars" )`, ...), which the person's store is keyed by. */
export const TOOLBAR_SETTINGS_FILE: Record<ToolbarFrame, string> = {
  pcb: "pcbnew-toolbars",
  schematic: "eeschema-toolbars",
  footprint: "fpedit-toolbars",
  symbol: "symbol_editor-toolbars",
  "3d": "3d_viewer-toolbars",
};

/** `TOOLBAR_ITEM`: one item of a toolbar. A group holds tools only (`ApplyConfiguration`: any other item in a group is "Unsupported group item type"). */
export type TbItem =
  | { type: "TOOL"; name: string }
  | { type: "SEPARATOR" }
  | { type: "SPACER"; size: number }
  | { type: "CONTROL"; name: string }
  | { type: "TB_GROUP"; groupName: string; items: string[] };

/** `TOOLBAR_CONFIGURATION::m_toolbarItems`. */
export type TbConfig = TbItem[];

/** The stored toolbars of a frame (`TOOLBAR_SETTINGS::m_toolbars`). */
export type StoredToolbars = Partial<Record<ToolbarLoc, TbConfig>>;

/** One frame's part of the person's settings: `m_CustomToolbars` and the stored toolbars. */
export interface FrameToolbarSettings {
  custom: boolean;
  toolbars: StoredToolbars;
}

export const EMPTY_FRAME_TOOLBAR_SETTINGS: FrameToolbarSettings = { custom: false, toolbars: {} };

// ------------------------------------------------------------------------------------------------ the controls

/** An `ACTION_TOOLBAR_CONTROL` (common/tool/action_toolbar.cpp `ACTION_TOOLBAR_CONTROLS`, pcbnew/toolbars_pcb_editor.cpp, eeschema/toolbars_sch_editor.cpp). */
export interface ToolbarControlDesc {
  /** `GetName()`, what the JSON stores. */
  name: string;
  /** The studio's id for it (the `control` of src/kicad/*toolbars.json). */
  id: string;
  /** `GetUiName()`: the tree's and the list's text. */
  uiName: string;
  /** `GetDescription()`: the list's tooltip, and what the filter searches. */
  description: string;
  /** `SupportedFor( aContext )` (of the frames that have a Toolbars page). */
  frames: readonly ToolbarFrame[];
}

export const TOOLBAR_CONTROLS: readonly ToolbarControlDesc[] = [
  { name: "control.GridSelector", id: "gridSelect", uiName: "Grid selector", description: "Grid Selection box", frames: ["schematic", "symbol", "pcb", "footprint"] },
  { name: "control.ZoomSelector", id: "zoomSelect", uiName: "Zoom selector", description: "Zoom Selection box", frames: ["schematic", "symbol", "pcb", "3d", "footprint"] },
  { name: "control.IPCPlugin", id: "ipcScripting", uiName: "IPC/Scripting plugins", description: "Region to hold the IPC/Scripting action buttons", frames: ["schematic", "pcb"] },
  { name: "control.LayerSelector", id: "layerSelector", uiName: "Layer selector", description: "Control to select the layer", frames: ["pcb", "footprint"] },
  { name: "control.UnitSelector", id: "unitSelector", uiName: "Symbol unit selector", description: "Displays the current unit", frames: ["symbol"] },
  { name: "control.BodyStyleSelector", id: "bodyStyleSelector", uiName: "Symbol body style selector", description: "Displays the current body style", frames: ["symbol"] },
  { name: "control.OverrideLocks", id: "overrideLocks", uiName: "Override locks", description: "Allow moving of locked items with the mouse", frames: ["pcb", "schematic"] },
  { name: "control.PCBTrackWidth", id: "trackWidth", uiName: "Track width selector", description: "Control to select the track width", frames: ["pcb"] },
  { name: "control.PCBViaDia", id: "viaDiameter", uiName: "Via diameter selector", description: "Control to select the via diameter", frames: ["pcb"] },
  // KiCad 25ec2335 `PCB_ACTION_TOOLBAR_CONTROLS::viaStack` (UI-LEFT-3 X3-1)
  { name: "control.PCBViaStack", id: "viaStack", uiName: "Microvia stack selector", description: "Control to select the microvia stack preset", frames: ["pcb"] },
  { name: "control.PCBCurrentVariant", id: "currentVariant", uiName: "Current variant", description: "Control to select the current variant", frames: ["pcb"] },
  { name: "control.currentVariant", id: "currentVariant", uiName: "Current variant", description: "Selects the current schematic variant", frames: ["schematic"] },
];

/** `ACTION_TOOLBAR::GetCustomControlList( aContext )`. */
export function controlsFor(frame: ToolbarFrame): ToolbarControlDesc[] {
  return TOOLBAR_CONTROLS.filter((c) => c.frames.includes(frame));
}

/** The control of a frame by its KiCad name. */
export function controlByName(frame: ToolbarFrame, name: string): ToolbarControlDesc | undefined {
  return controlsFor(frame).find((c) => c.name === name);
}

/** The control of a frame by the studio's id (the two "Current variant" controls are one id, each frame's own). */
export function controlById(frame: ToolbarFrame, id: string): ToolbarControlDesc | undefined {
  return controlsFor(frame).find((c) => c.id === id);
}

// ------------------------------------------------------------------------------------------------ the actions

/**
 * The actions that are never offered for a toolbar (`TOOLBAR_STATE::HIDDEN`): the ones common/tool/actions.cpp and eda_3d_actions.cpp mark so. An action
 * without an icon is hidden too (`TOOL_ACTION::TOOL_ACTION`: "If there is no icon, then the action should be hidden from the toolbar").
 */
export const TOOLBAR_HIDDEN_ACTIONS: ReadonlySet<string> = new Set([
  "common.Interactive.finish",
  "common.Control.showContextMenu",
  "common.Interactive.updateMenu",
  "common.Control.activatePointEditor",
  "common.Control.updateFind",
  "common.Control.cursorUp",
  "common.Control.cursorDown",
  "common.Control.cursorLeft",
  "common.Control.cursorRight",
  "common.Control.cursorUpFast",
  "common.Control.cursorDownFast",
  "common.Control.cursorLeftFast",
  "common.Control.cursorRightFast",
  "common.Control.cursorClick",
  "common.Control.cursorDblClick",
  "common.Control.refreshPreview",
  "common.Control.libraryTreeSearch",
  "common.Control.updateUnits",
  "common.Control.updatePreferences",
  "common.Control.selectColumns",
  "common.InteractivePicker.pickerTool",
  "common.InteractivePicker.pickerSubTool",
  "3DViewer.Control",
]);

/** The zoom actions of the 3D viewer's toolbar (`isActionSupported`'s `FRAME_PCB_DISPLAY3D` case). */
const VIEWER3D_COMMON_ACTIONS: ReadonlySet<string> = new Set(["common.Control.zoomRedraw", "common.Control.zoomInCenter", "common.Control.zoomOutCenter", "common.Control.zoomFitScreen"]);

/**
 * `PANEL_TOOLBAR_CUSTOMIZATION::isActionSupported`: the 3D viewer takes its own actions and the four zoom actions; the others take `common.*` and their
 * kiface's own (`pcbnew.*` for the board and footprint editors, `eeschema.*` for the schematic and symbol editors).
 */
export function isActionSupported(frame: ToolbarFrame, name: string): boolean {
  if (frame === "3d") return name.startsWith("3DViewer.") || VIEWER3D_COMMON_ACTIONS.has(name);
  if (name.startsWith("common.")) return true;
  return name.startsWith(frame === "pcb" || frame === "footprint" ? "pcbnew." : "eeschema.");
}

/** An action as the page needs it: `GetName()`, `GetFriendlyName()`, `GetDescription()` (the tooltip) and the icon. */
export interface ToolbarAction {
  name: string;
  label: string;
  tooltip: string;
  icon: string | null;
}

/** An entry of the actions list (`ACTION_LIST_ENTRY`). */
export type ActionListEntry =
  | { kind: "tool"; name: string; label: string; tooltip: string; icon: string | null; searchText: string }
  | { kind: "control"; name: string; label: string; tooltip: string; icon: null; searchText: string };

/** `wxString::CmpNoCase`. */
function cmpNoCase(a: string, b: string): number {
  const x = a.toLowerCase();
  const y = b.toLowerCase();
  return x < y ? -1 : x > y ? 1 : 0;
}

/**
 * `populateActions`: every action the frame supports that is not hidden (has an icon, no `TOOLBAR_STATE::HIDDEN`), and the frame's controls, sorted by their
 * text without regard to case; each searched by its text and tooltip, upper-cased.
 */
export function actionEntries(frame: ToolbarFrame, actions: readonly ToolbarAction[]): ActionListEntry[] {
  const out: ActionListEntry[] = [];
  const seen = new Set<string>();
  for (const a of actions) {
    if (seen.has(a.name)) continue;
    seen.add(a.name);
    if (!isActionSupported(frame, a.name) || !a.icon || TOOLBAR_HIDDEN_ACTIONS.has(a.name)) continue;
    out.push({ kind: "tool", name: a.name, label: a.label, tooltip: a.tooltip, icon: a.icon, searchText: `${a.label.toUpperCase()} ${a.tooltip.toUpperCase()}` });
  }
  for (const c of controlsFor(frame)) out.push({ kind: "control", name: c.name, label: c.uiName, tooltip: c.description, icon: null, searchText: `${c.uiName.toUpperCase()} ${c.description.toUpperCase()}` });
  // `std::sort` with `CmpNoCase`: entries of one text keep no promised order; here they keep the order above
  return out.map((e, i) => ({ e, i })).sort((a, b) => cmpNoCase(a.e.label, b.e.label) || a.i - b.i).map((x) => x.e);
}

/** `actionMatchesFilter`: an empty filter takes everything, else the upper-cased filter inside the entry's search text. */
export function filterActionEntries(entries: readonly ActionListEntry[], filter: string): ActionListEntry[] {
  if (filter === "") return [...entries];
  const f = filter.toUpperCase();
  return entries.filter((e) => e.searchText.includes(f));
}

// ------------------------------------------------------------------------------------------------ the studio's items and KiCad's

/** What a toolbar of the studio draws: the extracted items, and a spacer (`AddSpacer( size )`) a customised toolbar can have. */
export type RenderToolbarItem = ToolbarItem | { type: "spacer"; size: number };

/** A default toolbar of the studio (src/kicad/*toolbars.json) as KiCad's configuration: a group by its name, a control by its KiCad name. */
export function configOfStudioItems(frame: ToolbarFrame, items: readonly ToolbarItem[]): TbConfig {
  const out: TbConfig = [];
  for (const it of items) {
    if (it.type === "action") out.push({ type: "TOOL", name: it.action });
    else if (it.type === "separator") out.push({ type: "SEPARATOR" });
    else if (it.type === "group") out.push({ type: "TB_GROUP", groupName: it.label, items: [...it.items] });
    else {
      const c = controlById(frame, it.control);
      out.push({ type: "CONTROL", name: c?.name ?? it.control });
    }
  }
  return out;
}

/**
 * A configuration as the studio's toolbar draws it (`ACTION_TOOLBAR::ApplyConfiguration`): a tool the frame does not have is left out ("Unable to find toolbar tool
 * %s"), a group shows the icon of its first tool (the stand-in the extracted defaults use too) and is left out when none of its tools is known, a control the frame
 * has no factory for is left out ("Unable to find control factory for %s"). `controlLabel` gives a control the label the default toolbars give it, if any.
 */
export function studioItemsOfConfig(
  frame: ToolbarFrame,
  config: readonly TbItem[],
  ctx: { hasTool: (name: string) => boolean; iconOf: (name: string) => string | null; controlLabel?: (id: string) => string | undefined; hasControl?: (id: string) => boolean },
): RenderToolbarItem[] {
  const out: RenderToolbarItem[] = [];
  for (const it of config) {
    if (it.type === "SEPARATOR") out.push({ type: "separator" });
    else if (it.type === "SPACER") out.push({ type: "spacer", size: it.size });
    else if (it.type === "TOOL") {
      if (ctx.hasTool(it.name)) out.push({ type: "action", action: it.name });
    } else if (it.type === "TB_GROUP") {
      const tools = it.items.filter(ctx.hasTool);
      if (tools.length > 0) out.push({ type: "group", label: it.groupName, icon: ctx.iconOf(tools[0]!), items: tools });
    } else {
      const c = controlByName(frame, it.name);
      if (!c || (ctx.hasControl && !ctx.hasControl(c.id))) continue;
      out.push({ type: "control", control: c.id, label: ctx.controlLabel?.(c.id) ?? c.uiName });
    }
  }
  return out;
}

/**
 * `TOOLBAR_SETTINGS::GetToolbarConfig( aToolbar, aAllowCustom )`: nothing for a toolbar the frame has no default for (a stored one is ignored too); the stored
 * configuration when custom toolbars are allowed and one is stored; else the default.
 */
export function toolbarConfig(defaults: Partial<Record<ToolbarLoc, TbConfig>>, stored: StoredToolbars, loc: ToolbarLoc, allowCustom: boolean): TbConfig | null {
  const def = defaults[loc];
  if (!def) return null;
  if (allowCustom) {
    const s = stored[loc];
    if (s) return s;
  }
  return def;
}

// ------------------------------------------------------------------------------------------------ JSON (`<frame>-toolbars.json`)

/** `TOOLBAR_ITEM_TYPE`'s names, as `magic_enum::enum_name` writes them. */
const ITEM_TYPES = ["TOOL", "TB_GROUP", "SPACER", "CONTROL", "SEPARATOR"] as const;

/** `to_json( TOOLBAR_ITEM )`. */
export function itemToJson(it: TbItem): Record<string, unknown> {
  switch (it.type) {
    case "SEPARATOR":
      return { type: "SEPARATOR" };
    case "SPACER":
      return { type: "SPACER", size: it.size };
    case "CONTROL":
      return { type: "CONTROL", name: it.name };
    case "TOOL":
      return { type: "TOOL", name: it.name };
    case "TB_GROUP":
      return { type: "TB_GROUP", group_name: it.groupName, group_items: it.items.map((name) => ({ type: "TOOL", name })) };
  }
}

const str = (v: unknown): string => (typeof v === "string" ? v : "");

/**
 * `from_json( TOOLBAR_ITEM )`: an empty object is a default item (a tool of no name, which no toolbar can show: dropped here); the type is read without regard to
 * case and an unknown one leaves the default, a tool; a group keeps its tools (`ApplyConfiguration` takes nothing else from a group).
 */
export function itemFromJson(j: unknown): TbItem | null {
  if (!j || typeof j !== "object" || Array.isArray(j)) return null;
  const o = j as Record<string, unknown>;
  const t = typeof o.type === "string" ? ITEM_TYPES.find((x) => x.toLowerCase() === (o.type as string).toLowerCase()) : undefined;
  const type = t ?? "TOOL";
  switch (type) {
    case "SEPARATOR":
      return { type: "SEPARATOR" };
    case "SPACER":
      return { type: "SPACER", size: typeof o.size === "number" && Number.isFinite(o.size) ? Math.trunc(o.size) : 0 };
    case "CONTROL":
      return str(o.name) ? { type: "CONTROL", name: str(o.name) } : null;
    case "TOOL":
      return str(o.name) ? { type: "TOOL", name: str(o.name) } : null;
    case "TB_GROUP": {
      const items: string[] = [];
      if (Array.isArray(o.group_items)) {
        for (const g of o.group_items) {
          const gi = itemFromJson(g);
          if (gi?.type === "TOOL") items.push(gi.name);
        }
      }
      return { type: "TB_GROUP", groupName: str(o.group_name), items };
    }
  }
}

/** `to_json( TOOLBAR_CONFIGURATION )`. */
export function configToJson(config: readonly TbItem[]): unknown[] {
  return config.map(itemToJson);
}

/** `from_json( TOOLBAR_CONFIGURATION )`: anything but an array is an empty toolbar. */
export function configFromJson(j: unknown): TbConfig {
  if (!Array.isArray(j)) return [];
  const out: TbConfig = [];
  for (const e of j) {
    const it = itemFromJson(e);
    if (it) out.push(it);
  }
  return out;
}

/** `TOOLBAR_SETTINGS`' "toolbars" param: `[ { name: "TOP_MAIN", contents: [..] }, .. ]` in location order. */
export function toolbarsToJson(stored: StoredToolbars): unknown[] {
  return TOOLBAR_LOCS.flatMap((loc) => (stored[loc] ? [{ name: loc, contents: configToJson(stored[loc]!) }] : []));
}

/** The param's reader: not an array is nothing stored; an entry that is not an object or names no location is skipped; the first of a location is kept (`emplace`). */
export function toolbarsFromJson(j: unknown): StoredToolbars {
  const out: StoredToolbars = {};
  if (!Array.isArray(j)) return out;
  for (const e of j) {
    if (!e || typeof e !== "object" || Array.isArray(e)) continue;
    const o = e as Record<string, unknown>;
    const loc = typeof o.name === "string" ? TOOLBAR_LOCS.find((l) => l.toLowerCase() === (o.name as string).toLowerCase()) : undefined;
    if (!loc || out[loc]) continue;
    out[loc] = configFromJson(o.contents);
  }
  return out;
}

/** The person's toolbar settings of every frame as the store keeps them: by the frame's settings file name, `custom_toolbars` and KiCad's "toolbars" array. */
export type StoredToolbarSettings = Partial<Record<ToolbarFrame, FrameToolbarSettings>>;

export function serializeToolbarSettings(s: StoredToolbarSettings): string {
  const out: Record<string, unknown> = {};
  for (const f of Object.keys(TOOLBAR_SETTINGS_FILE) as ToolbarFrame[]) {
    const v = s[f];
    if (v) out[TOOLBAR_SETTINGS_FILE[f]] = { custom_toolbars: v.custom, meta: { version: 1 }, toolbars: toolbarsToJson(v.toolbars) };
  }
  return JSON.stringify(out);
}

/** The store's text read back; anything malformed is nothing customised (the defaults). */
export function parseToolbarSettings(raw: string | null): StoredToolbarSettings {
  if (!raw) return {};
  try {
    const j = JSON.parse(raw) as unknown;
    if (!j || typeof j !== "object" || Array.isArray(j)) return {};
    const out: StoredToolbarSettings = {};
    for (const f of Object.keys(TOOLBAR_SETTINGS_FILE) as ToolbarFrame[]) {
      const v = (j as Record<string, unknown>)[TOOLBAR_SETTINGS_FILE[f]];
      if (!v || typeof v !== "object" || Array.isArray(v)) continue;
      const o = v as Record<string, unknown>;
      out[f] = { custom: o.custom_toolbars === true, toolbars: toolbarsFromJson(o.toolbars) };
    }
    return out;
  } catch {
    return {};
  }
}

// ------------------------------------------------------------------------------------------------ the page's tree

/** A row of the page's tree (`TOOLBAR_TREE_ITEM_DATA`); a group's children are rows too. */
export type TreeNode =
  | { id: number; kind: "tool"; name: string }
  | { id: number; kind: "control"; name: string }
  | { id: number; kind: "separator" }
  | { id: number; kind: "spacer"; size: number }
  | { id: number; kind: "group"; name: string; children: TreeNode[] };

export interface ToolbarTree {
  nodes: TreeNode[];
  /** The selected row (`GetSelection()`), or null. */
  selected: number | null;
  nextId: number;
}

/** Where the rows come from: the actions the frame has (`m_availableTools`) and its controls (`m_availableControls`). */
export interface TreeContext {
  frame: ToolbarFrame;
  hasTool: (name: string) => boolean;
}

/**
 * `populateToolbarTree`: a row per item -- a tool the frame does not have, or does not support, and a control it does not have are left out; a group's tools the
 * same way, and a group left with none is deleted --; the first row selected.
 */
export function populateToolbarTree(config: readonly TbItem[], ctx: TreeContext): ToolbarTree {
  let id = 1;
  const nodes: TreeNode[] = [];
  const tool = (name: string) => ctx.hasTool(name) && isActionSupported(ctx.frame, name);
  for (const it of config) {
    if (it.type === "SEPARATOR") nodes.push({ id: id++, kind: "separator" });
    else if (it.type === "SPACER") nodes.push({ id: id++, kind: "spacer", size: it.size });
    else if (it.type === "CONTROL") {
      if (controlByName(ctx.frame, it.name)) nodes.push({ id: id++, kind: "control", name: it.name });
    } else if (it.type === "TOOL") {
      if (tool(it.name)) nodes.push({ id: id++, kind: "tool", name: it.name });
    } else {
      const gid = id++;
      const children: TreeNode[] = it.items.filter(tool).map((name) => ({ id: id++, kind: "tool" as const, name }));
      if (children.length > 0) nodes.push({ id: gid, kind: "group", name: it.groupName, children });
    }
  }
  return { nodes, selected: nodes[0]?.id ?? null, nextId: id };
}

/** `parseToolbarTree`: the rows back to a configuration; in a group only its tools ("Invalid entry in a group" for anything else). */
export function parseToolbarTree(tree: ToolbarTree): TbConfig {
  const out: TbConfig = [];
  for (const n of tree.nodes) {
    if (n.kind === "spacer") out.push({ type: "SPACER", size: n.size });
    else if (n.kind === "separator") out.push({ type: "SEPARATOR" });
    else if (n.kind === "control") out.push({ type: "CONTROL", name: n.name });
    else if (n.kind === "tool") out.push({ type: "TOOL", name: n.name });
    else out.push({ type: "TB_GROUP", groupName: n.name, items: n.children.flatMap((c) => (c.kind === "tool" ? [c.name] : [])) });
  }
  return out;
}

/** The row of an id and where it is: its parent group (null at the top) and its index among its siblings. */
export function findNode(tree: ToolbarTree, id: number | null): { node: TreeNode; parent: Extract<TreeNode, { kind: "group" }> | null; index: number } | null {
  if (id === null) return null;
  for (let i = 0; i < tree.nodes.length; i++) {
    const n = tree.nodes[i]!;
    if (n.id === id) return { node: n, parent: null, index: i };
    if (n.kind === "group") {
      const j = n.children.findIndex((c) => c.id === id);
      if (j >= 0) return { node: n.children[j]!, parent: n, index: j };
    }
  }
  return null;
}

/** The siblings `node` sits among, replaced by `f( siblings )`. */
function withSiblings(tree: ToolbarTree, parent: Extract<TreeNode, { kind: "group" }> | null, f: (s: TreeNode[]) => TreeNode[]): TreeNode[] {
  if (!parent) return f([...tree.nodes]);
  return tree.nodes.map((n) => (n.id === parent.id && n.kind === "group" ? { ...n, children: f([...n.children]) } : n));
}

/**
 * `onSeparatorPress`, `onSpacerPress` (a spacer of 5) and `onGroupPress` ("Group"): the new row goes after the selected one, or at the end when nothing is
 * selected, and is selected; none of them goes into a group ("Can't insert a separator in a group yet", "Can't add a group onto a group"): with a group's tool
 * selected nothing happens.
 */
export function insertItem(tree: ToolbarTree, kind: "separator" | "spacer" | "group", groupName = "Group"): ToolbarTree {
  const node: TreeNode = kind === "separator" ? { id: tree.nextId, kind } : kind === "spacer" ? { id: tree.nextId, kind, size: 5 } : { id: tree.nextId, kind, name: groupName, children: [] };
  const sel = findNode(tree, tree.selected);
  if (sel?.parent) return tree;
  const nodes = [...tree.nodes];
  if (sel) nodes.splice(sel.index + 1, 0, node);
  else nodes.push(node);
  return { nodes, selected: node.id, nextId: tree.nextId + 1 };
}

/** `removeControlFromCurrentTree`: the top rows that are the control. */
export function removeControlFromTree(tree: ToolbarTree, controlName: string): ToolbarTree {
  const nodes = tree.nodes.filter((n) => !(n.kind === "control" && n.name === controlName));
  if (nodes.length === tree.nodes.length) return tree;
  // a row deleted while selected leaves nothing selected
  const selected = findNode({ ...tree, nodes }, tree.selected) ? tree.selected : null;
  return { ...tree, nodes, selected };
}

/** `removeControlFromOtherToolbars`: the control taken off every shadow toolbar but the one shown. */
export function removeControlFromOtherToolbars(shadow: StoredToolbars, current: ToolbarLoc, controlName: string): StoredToolbars {
  const out: StoredToolbars = {};
  for (const loc of TOOLBAR_LOCS) {
    const cfg = shadow[loc];
    if (!cfg) continue;
    out[loc] = loc === current ? cfg : cfg.filter((it) => !(it.type === "CONTROL" && it.name === controlName));
  }
  return out;
}

/**
 * `onBtnAddAction`: the list's entry as a row -- into a selected group at its end, else after the selected row among its siblings (a group's tool selected: into
 * that group), else at the end -- and selected. A control is first taken off the other toolbars and this one (it can be on one toolbar only). Returns the tree and
 * the shadow toolbars.
 */
export function addEntry(tree: ToolbarTree, shadow: StoredToolbars, current: ToolbarLoc, entry: { kind: "tool" | "control"; name: string }): { tree: ToolbarTree; shadow: StoredToolbars } {
  let t = tree;
  let s = shadow;
  if (entry.kind === "control") {
    s = removeControlFromOtherToolbars(shadow, current, entry.name);
    t = removeControlFromTree(t, entry.name);
  }
  const node: TreeNode = entry.kind === "tool" ? { id: t.nextId, kind: "tool", name: entry.name } : { id: t.nextId, kind: "control", name: entry.name };
  const sel = findNode(t, t.selected);
  let nodes: TreeNode[];
  if (sel && sel.node.kind === "group") {
    const gid = sel.node.id;
    nodes = t.nodes.map((n) => (n.id === gid && n.kind === "group" ? { ...n, children: [...n.children, node] } : n));
  } else if (sel) {
    nodes = withSiblings(t, sel.parent, (sib) => (sib.splice(sel.index + 1, 0, node), sib));
  } else {
    nodes = [...t.nodes, node];
  }
  return { tree: { nodes, selected: node.id, nextId: t.nextId + 1 }, shadow: s };
}

/**
 * `onToolDelete`: the selected row (a group with its tools) goes; the row before it is selected (KiCad: "The tree control defaults to nothing selected if you
 * delete the item at the last place"); the first of a group's tools deleted selects the group, the first top row nothing.
 */
export function deleteSelected(tree: ToolbarTree): ToolbarTree {
  const sel = findNode(tree, tree.selected);
  if (!sel) return tree;
  const prev = sel.parent ? sel.parent.children[sel.index - 1] : tree.nodes[sel.index - 1];
  const nodes = withSiblings(tree, sel.parent, (sib) => (sib.splice(sel.index, 1), sib));
  return { ...tree, nodes, selected: prev?.id ?? sel.parent?.id ?? null };
}

/** `UP_DOWN_TREE::MoveItemUp` / `MoveItemDown`: the selected row one place up or down among its siblings (a group's tools stay in it); at the end nothing moves. */
export function moveSelected(tree: ToolbarTree, dir: -1 | 1): ToolbarTree {
  const sel = findNode(tree, tree.selected);
  if (!sel) return tree;
  const count = sel.parent ? sel.parent.children.length : tree.nodes.length;
  const to = sel.index + dir;
  if (to < 0 || to >= count) return tree;
  const nodes = withSiblings(tree, sel.parent, (sib) => {
    const [n] = sib.splice(sel.index, 1);
    sib.splice(to, 0, n!);
    return sib;
  });
  return { ...tree, nodes };
}

/**
 * A group renamed in the tree (`onTreeBeginLabelEdit`: only a group's or a spacer's label can be edited). 8303b2ad's `onTreeEndLabelEdit` keeps nothing; KiCad
 * 25ec2335 keeps the group's new name and refuses an empty one (`Strip( wxString::both ).IsEmpty()` vetoes the edit), which is what is done here. Returns null
 * for a refused edit.
 */
export function renameGroup(tree: ToolbarTree, id: number, name: string): ToolbarTree | null {
  if (name.trim() === "") return null;
  const f = findNode(tree, id);
  if (!f || f.node.kind !== "group") return null;
  return { ...tree, nodes: tree.nodes.map((n) => (n.id === id && n.kind === "group" ? { ...n, name } : n)) };
}

/** The tree's text of a row (`populateToolbarTree`): "Separator", "Spacer: %i", a control's UI name, a tool's friendly name, a group's name. */
export function treeLabel(n: TreeNode, frame: ToolbarFrame, labelOf: (name: string) => string): string {
  switch (n.kind) {
    case "separator":
      return "Separator";
    case "spacer":
      return `Spacer: ${n.size}`;
    case "control":
      return controlByName(frame, n.name)?.uiName ?? n.name;
    case "tool":
      return labelOf(n.name);
    case "group":
      return n.name;
  }
}

/**
 * `TransferDataFromWindow`'s clean-up before the shadow toolbars are stored: a control is kept on the first toolbar it is on (in location order) and taken off the
 * others.
 */
export function dedupeControls(shadow: StoredToolbars): StoredToolbars {
  const seen = new Set<string>();
  const out: StoredToolbars = {};
  for (const loc of TOOLBAR_LOCS) {
    const cfg = shadow[loc];
    if (!cfg) continue;
    out[loc] = cfg.filter((it) => {
      if (it.type !== "CONTROL") return true;
      if (seen.has(it.name)) return false;
      seen.add(it.name);
      return true;
    });
  }
  return out;
}

/** The toolbars the page offers (`m_toolbarChoices`): the frame's toolbars that have a default, in location order. */
export function toolbarChoices(defaults: Partial<Record<ToolbarLoc, TbConfig>>): ToolbarLoc[] {
  return TOOLBAR_LOCS.filter((l) => defaults[l] !== undefined);
}

/** `TransferDataToWindow`'s shadow toolbars: each toolbar the frame has, as it would show it with customisation allowed (stored, else default). */
export function shadowToolbars(defaults: Partial<Record<ToolbarLoc, TbConfig>>, stored: StoredToolbars): StoredToolbars {
  const out: StoredToolbars = {};
  for (const loc of toolbarChoices(defaults)) out[loc] = toolbarConfig(defaults, stored, loc, true)!;
  return out;
}

/** `ResetPanel`'s shadow toolbars: the defaults. */
export function defaultShadowToolbars(defaults: Partial<Record<ToolbarLoc, TbConfig>>): StoredToolbars {
  const out: StoredToolbars = {};
  for (const loc of toolbarChoices(defaults)) out[loc] = [...defaults[loc]!];
  return out;
}
