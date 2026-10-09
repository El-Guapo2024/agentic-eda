// The PCB editor's right-click menus: which entries KiCad offers for a selection, in which order, under which submenu.
//
// KiCad builds the menu of the selection tool out of what every tool adds to it in its `Init()`; `CONDITIONAL_MENU` (common/tool/conditional_menu.cpp) keeps
// each entry with the condition it is shown under and an order number, and `Evaluate( selection )` walks them. This port keeps that machinery, so the order of
// the entries (Select, Cancel, ..., Routing, Mirror / Rotate, Position, Locking, ..., Cut, Copy, ..., Zoom, Grid, Properties) is KiCad's own and not this port's
// guess, and builds the entries in the order the tools are initialised (pcb_edit_frame.cpp `setupTools`):
//
//   pcbnew/tools/pcb_selection_tool.cpp   PCB_SELECTION_TOOL::Init, SELECT_MENU                  Select, Cancel, Enter / Leave Group, Clear Net Highlighting
//   pcbnew/tools/edit_tool.cpp            EDIT_TOOL::Init and its submenu makers                 Move, Flip, Break Track, ..., Routing, Mirror / Rotate, Position,
//                                                                                                 Shape Modification, Cut ... Delete, Properties
//   pcbnew/tools/pad_tool.cpp             PAD_TOOL::Init                                         Copy / Paste / Push Pad Properties
//   pcbnew/tools/pcb_point_editor.cpp     PCB_POINT_EDITOR::Init                                 Cycle Arc Editing Mode
//   pcbnew/tools/board_editor_control.cpp BOARD_EDITOR_CONTROL::Init, LOCK_/ZONE_CONTEXT_MENU    Get and Move Footprint, Locking, Zones
//   pcbnew/tools/board_inspection_tool.cpp BOARD_INSPECTION_TOOL::Init, NET_CONTEXT_MENU          Net Inspection Tools
//   pcbnew/tools/align_distribute_tool.cpp ALIGN_DISTRIBUTE_TOOL::Init                           Align/Distribute
//   pcbnew/tools/convert_tool.cpp         CONVERT_TOOL::Init                                     Create from Selection
//   common/tool/group_tool.cpp            GROUP_TOOL::Init, GROUP_CONTEXT_MENU                   Grouping
//   common/eda_draw_frame.cpp             EDA_DRAW_FRAME::AddStandardSubMenus                    Zoom, Grid
//
// and the menus of the tools that take over the right click: pcbnew/router/router_tool.cpp (ROUTER_TOOL::Init, while routing) and
// pcbnew/tools/drawing_tool.cpp (DRAWING_TOOL::Init, while a graphic, zone, text, dimension or via is being drawn).
//
// Not built, because the IR has nothing for them: table cells (Select Column(s), Row(s), Table), gate swap, generators (Update All Tuning Patterns). An entry
// KiCad offers that this studio has not wired is still listed, disabled, as the schematic's menu does (`MenuNodeView` knows which actions have a handler).
//
// The entries of the Zoom and Grid submenus, the Track/Via Width list and the like are data, handed in by the caller (`PcbMenuContext`). Pure: no store, no
// DOM. Unit-tested in pcbContextMenu.test.ts.
import type { MenuNode } from "../kicad/types";

// ------------------------------------------------------------------------------------------------------------------------------------------------- the selection

export type ShapeKind = "segment" | "rect" | "circle" | "arc" | "polygon" | "bezier";

/** How many selected items there are of each kind the menu's conditions tell apart (`SELECTION_CONDITIONS::HasTypes / OnlyTypes / Count / MoreThan`). */
export interface PcbSelectionSummary {
  total: number;
  /** `PCB_FOOTPRINT_T`. */
  footprints: number;
  /** `PCB_PAD_T` (a pad selected on its own). */
  pads: number;
  /** `PCB_TRACE_T`. */
  tracks: number;
  /** `PCB_ARC_T`: a track that is an arc. */
  arcTracks: number;
  vias: number;
  /** `PCB_ZONE_T`, rule areas and teardrop areas included. */
  zones: number;
  /** Zones that are copper zones (neither a rule area nor a teardrop): what has a fill priority. */
  copperZones: number;
  /** `PCB_TEXT_T` (and fields, which this studio does not select on their own). */
  texts: number;
  dimensions: number;
  groups: number;
  /** `PCB_SHAPE_T`, by `SHAPE_T`. */
  shapes: Record<ShapeKind, number>;
  /** Of those shapes, how many sit on a copper layer (`PCB_SHAPE::IsOnCopperLayer`). */
  copperShapes: number;
  locked: number;
  unlocked: number;
  /** One selected zone or polygon whose corners can be edited (`selectionHasEditableCorners`: not a teardrop area). */
  editableCorners: boolean;
  /** `PCB_POINT_EDITOR::CanAddCorner( front )` / `CanChamferCorner( front )` for the one selected item. */
  canAddCorner: boolean;
  canChamferCorner: boolean;
  /** The zone could be raised / lowered among the zones it overlaps (`ZONE_PRIORITY_CONTEXT_MENU::update`). */
  zoneCanRaise: boolean;
  zoneCanLower: boolean;
  /** `GROUP_CONTEXT_MENU::update`: `selectionCount`, a group selected, one group selected, an ungrouped item selected, a member of a group selected. */
  group: { hasGroup: boolean; onlyOneGroup: boolean; hasUngroupedItems: boolean; hasMember: boolean };
  /** Something selected carries a net (`haveNetCond`: `BOARD_CONNECTED_ITEM` with a net code above 0). */
  hasNet: boolean;
  /** The selection is what `Convert` could make something of (`kicad-port/pcbConvert.ts` `convertAvailability`). */
  convert: { poly: boolean; zone: boolean; keepout: boolean; lines: boolean; outset: boolean; tracks: boolean; arc: boolean };
}

export function emptyShapes(): Record<ShapeKind, number> {
  return { segment: 0, rect: 0, circle: 0, arc: 0, polygon: 0, bezier: 0 };
}

export function emptyPcbSummary(): PcbSelectionSummary {
  return {
    total: 0,
    footprints: 0,
    pads: 0,
    tracks: 0,
    arcTracks: 0,
    vias: 0,
    zones: 0,
    copperZones: 0,
    texts: 0,
    dimensions: 0,
    groups: 0,
    shapes: emptyShapes(),
    copperShapes: 0,
    locked: 0,
    unlocked: 0,
    editableCorners: false,
    canAddCorner: false,
    canChamferCorner: false,
    zoneCanRaise: false,
    zoneCanLower: false,
    group: { hasGroup: false, onlyOneGroup: false, hasUngroupedItems: false, hasMember: false },
    hasNet: false,
    convert: { poly: false, zone: false, keepout: false, lines: false, outset: false, tracks: false, arc: false },
  };
}

/** What the tools are doing and what the board looks like, as the menu's conditions ask for it. */
export interface PcbMenuContext {
  /** `!frame->ToolStackIsEmpty()`: a tool other than the selection tool is running. */
  toolActive: boolean;
  /** `aSelection.Front()->IsMoving()`: the selection is in the hand. */
  moving: boolean;
  /** `frame()->IsCurrentTool( PCB_ACTIONS::moveIndividually )`. */
  movingIndividually: boolean;
  /** `m_enteredGroup != nullptr`. */
  inGroup: boolean;
  /** `!GetBoard()->IsEmpty()`. */
  boardHasItems: boolean;
  /** `!cfg->GetHighlightNetCodes().empty()`. */
  haveHighlight: boolean;
  /** The point editor's handle under the pointer (`PCB_POINT_EDITOR::HasCorner() / HasMidpoint() / CanRemoveCorner()`). */
  handle: { corner: boolean; midpoint: boolean; canRemoveCorner: boolean };
  /** Menus that are data: `ZOOM_MENU::update` and `GRID_MENU::update`. */
  zoom: readonly { label: string; checked: boolean }[];
  grid: readonly { label: string; checked: boolean }[];
}

export function defaultMenuContext(): PcbMenuContext {
  return { toolActive: false, moving: false, movingIndividually: false, inGroup: false, boardHasItems: true, haveHighlight: false, handle: { corner: false, midpoint: false, canRemoveCorner: false }, zoom: [], grid: [] };
}

// ------------------------------------------------------------------------------------------------------------------------------------------ the type predicates

type Sel = PcbSelectionSummary;

/** The `KICAD_T` lists the conditions use, as the kinds of the summary they cover. */
type Kind = "footprint" | "pad" | "track" | "arc" | "via" | "zone" | "text" | "dimension" | "group" | "shape" | "segment" | "rect" | "circle" | "shapeArc" | "polygon" | "bezier";

const count = (s: Sel, kind: Kind): number => {
  switch (kind) {
    case "footprint":
      return s.footprints;
    case "pad":
      return s.pads;
    case "track":
      return s.tracks;
    case "arc":
      return s.arcTracks;
    case "via":
      return s.vias;
    case "zone":
      return s.zones;
    case "text":
      return s.texts;
    case "dimension":
      return s.dimensions;
    case "group":
      return s.groups;
    case "shape":
      return s.shapes.segment + s.shapes.rect + s.shapes.circle + s.shapes.arc + s.shapes.polygon + s.shapes.bezier;
    case "segment":
      return s.shapes.segment;
    case "rect":
      return s.shapes.rect;
    case "circle":
      return s.shapes.circle;
    case "shapeArc":
      return s.shapes.arc;
    case "polygon":
      return s.shapes.polygon;
    case "bezier":
      return s.shapes.bezier;
  }
};

/** `SELECTION_CONDITIONS::HasTypes`: some selected item is one of `kinds`. */
export const hasTypes = (s: Sel, kinds: readonly Kind[]): boolean => s.total > 0 && kinds.some((k) => count(s, k) > 0);

/** `SELECTION_CONDITIONS::OnlyTypes`: something is selected and every item is one of `kinds`. (The kinds must be disjoint: `shape` alone, or its subtypes.) */
export const onlyTypes = (s: Sel, kinds: readonly Kind[]): boolean => s.total > 0 && kinds.reduce((n, k) => n + count(s, k), 0) === s.total;

const TRACK_TYPES: readonly Kind[] = ["track", "arc", "via"];
const CONNECTED_TYPES: readonly Kind[] = ["track", "arc", "via", "pad", "zone"];
const ROUTABLE_TYPES: readonly Kind[] = ["track", "arc", "via", "pad", "footprint"];
/** `GENERAL_COLLECTOR::DraggableItems`. */
const DRAGGABLE_TYPES: readonly Kind[] = ["track", "via", "footprint", "arc"];
/** `EDIT_TOOL::MirrorableItems` (fields are texts here; generators and points do not exist). */
const MIRRORABLE_TYPES: readonly Kind[] = ["shape", "text", "zone", "pad", "track", "arc", "via", "group"];
const FILLET_CHAMFER_TYPES: readonly Kind[] = ["polygon", "rect", "segment"];
const HEAL_TYPES: readonly Kind[] = ["segment", "shapeArc", "bezier"];
const BOOLEAN_TYPES: readonly Kind[] = ["rect", "polygon", "circle"];
const POLYGON_SIMPLIFY_TYPES: readonly Kind[] = ["polygon", "zone"];
/** `canCopyAsText`'s list. */
const COPY_AS_TEXT_TYPES: readonly Kind[] = ["text", "dimension"];
const CROSS_PROBE_TYPES: readonly Kind[] = ["pad", "footprint", "group"];
const PAD_OWNER_TYPES: readonly Kind[] = ["footprint", "pad"];

// ---------------------------------------------------------------------------------------------------------------------------------------- the conditional menu

type Cond = (s: Sel, c: PcbMenuContext) => boolean;

const always: Cond = () => true;
const notEmpty: Cond = (s) => s.total > 0;
const countIs = (n: number): Cond => (s) => s.total === n;
const moreThan = (n: number): Cond => (s) => s.total > n;
const and =
  (...conds: Cond[]): Cond =>
  (s, c) =>
    conds.every((f) => f(s, c));
const not =
  (f: Cond): Cond =>
  (s, c) =>
    !f(s, c);
const whenHas = (...kinds: Kind[]): Cond => (s) => hasTypes(s, kinds);
const whenOnly = (...kinds: Kind[]): Cond => (s) => onlyTypes(s, kinds);

/** `ANY_ORDER`: the entry goes after everything added so far that is not above it, as `addEntry` places it (`m_entries.size()`). */
const ANY_ORDER = -1;

interface Entry {
  kind: "item" | "menu" | "separator";
  order: number;
  cond: Cond;
  /** An item: the action; a menu: the submenu's title. */
  action?: string;
  label?: string;
  /** A menu: its entries as nodes. */
  build?: (s: Sel, c: PcbMenuContext) => MenuNode[];
  /** An item: the event parameter the action runs with, whether it is drawn disabled, whether it is a check item and its state. */
  arg?: number | string;
  disabled?: Cond;
  checked?: Cond;
}

/** `CONDITIONAL_MENU`: entries with a condition and an order, shown by `Evaluate`. */
export class ConditionalMenu {
  private entries: Entry[] = [];

  /** `CONDITIONAL_MENU::addEntry`: an order below zero is the number of entries so far; the entry goes after every entry whose order is not above its own. */
  private addEntry(entry: Entry): void {
    if (entry.order < 0) entry.order = this.entries.length;
    let at = 0;
    while (at < this.entries.length && this.entries[at]!.order <= entry.order) at++;
    this.entries.splice(at, 0, entry);
  }

  addItem(action: string, cond: Cond, order: number = ANY_ORDER, opts: { label?: string; arg?: number | string; disabled?: Cond; checked?: Cond } = {}): void {
    this.addEntry({ kind: "item", order, cond, action, ...opts });
  }

  addMenu(title: string, build: (s: Sel, c: PcbMenuContext) => MenuNode[], cond: Cond, order: number = ANY_ORDER): void {
    this.addEntry({ kind: "menu", order, cond, label: title, build });
  }

  addSeparator(cond: Cond = always, order: number = ANY_ORDER): void {
    this.addEntry({ kind: "separator", order, cond });
  }

  /**
   * `CONDITIONAL_MENU::Evaluate`: what the entries show for `s`. "We try to avoid adding useless separators": one is added only when an item or a submenu
   * came since the last. (A separator left at the very end is dropped: KiCad's native menu shows it as a line at the bottom, this one would be an empty strip.)
   */
  evaluate(s: Sel, c: PcbMenuContext): MenuNode[] {
    const out: MenuNode[] = [];
    let sinceSeparator = 0;
    for (const entry of this.entries) {
      if (!entry.cond(s, c)) continue;
      if (entry.kind === "separator") {
        if (sinceSeparator > 0) out.push({ type: "separator" });
        sinceSeparator = 0;
        continue;
      }
      sinceSeparator++;
      if (entry.kind === "item") {
        const node: MenuNode = { type: "item", action: entry.action! };
        if (entry.label !== undefined) node.label = entry.label;
        if (entry.arg !== undefined) node.arg = entry.arg;
        if (entry.disabled?.(s, c)) node.disabled = true;
        if (entry.checked) node.checked = entry.checked(s, c);
        out.push(node);
      } else {
        out.push({ type: "submenu", label: entry.label!, items: entry.build!(s, c) });
      }
    }
    while (out.length > 0 && out[out.length - 1]!.type === "separator") out.pop();
    return out;
  }
}

/** A submenu that is itself a `CONDITIONAL_MENU`: its title and what it shows. */
const submenu = (menu: ConditionalMenu): ((s: Sel, c: PcbMenuContext) => MenuNode[]) => (s, c) => menu.evaluate(s, c);

const item = (action: string, extra: Partial<Extract<MenuNode, { type: "item" }>> = {}): MenuNode => ({ type: "item", action, ...extra });
const sep: MenuNode = { type: "separator" };

// -------------------------------------------------------------------------------------------------------------------------------------------------- the actions

const A = {
  cancel: "common.Interactive.cancel",
  groupEnter: "common.Interactive.groupEnter",
  groupLeave: "common.Interactive.groupLeave",
  applyDesignBlockLayout: "pcbnew.InteractiveDrawing.applyDesignBlockLayout",
  placeLinkedDesignBlock: "pcbnew.InteractiveDrawing.placeLinkedDesignBlock",
  saveToLinkedDesignBlock: "pcbnew.InteractiveDrawing.saveToLinkedDesignBlock",
  clearHighlight: "pcbnew.EditorControl.clearHighlight",
  selectColumns: "common.InteractiveSelection.SelectColumns",
  selectRows: "common.InteractiveSelection.Rows",
  selectTable: "common.InteractiveSelection.SelectTable",
  selectAll: "common.Interactive.selectAll",
  unselectAll: "common.Interactive.unselectAll",
  skip: "pcbnew.InteractiveEdit.skip",
  move: "pcbnew.InteractiveMove.move",
  drag45: "pcbnew.InteractiveRouter.Drag45Degree",
  dragFree: "pcbnew.InteractiveRouter.DragFreeAngle",
  flip: "pcbnew.InteractiveEdit.flip",
  swap: "pcbnew.InteractiveEdit.swap",
  swapPadNets: "pcbnew.InteractiveEdit.swapPadNets",
  breakTrack: "pcbnew.InteractiveRouter.BreakTrack",
  filletTracks: "pcbnew.InteractiveEdit.filletTracks",
  packAndMove: "pcbnew.InteractiveEdit.packAndMoveFootprints",
  assignNetclass: "pcbnew.EditorControl.assignNetclass",
  inspectClearance: "pcbnew.InspectionTool.InspectClearance",
  editFpInFpEditor: "pcbnew.EditorControl.EditFpInFpEditor",
  updateFootprint: "pcbnew.GlobalEdit.updateFootprint",
  updateFootprints: "pcbnew.GlobalEdit.updateFootprints",
  changeFootprint: "pcbnew.GlobalEdit.changeFootprint",
  changeFootprints: "pcbnew.GlobalEdit.changeFootprints",
  routeSelected: "pcbnew.InteractiveRouter.RouteSelected",
  routeSelectedFromEnd: "pcbnew.InteractiveRouter.RouteSelectedFromEnd",
  unrouteSelected: "pcbnew.InteractiveSelection.unrouteSelected",
  unrouteSegment: "pcbnew.InteractiveSelection.unrouteSegment",
  autoroute: "pcbnew.InteractiveRouter.Autoroute",
  rotateCcw: "pcbnew.InteractiveEdit.rotateCcw",
  rotateCw: "pcbnew.InteractiveEdit.rotateCw",
  mirrorH: "pcbnew.InteractiveEdit.mirrorHoriontally",
  mirrorV: "pcbnew.InteractiveEdit.mirrorVertically",
  moveExact: "pcbnew.InteractiveEdit.moveExact",
  moveWithReference: "pcbnew.InteractiveMove.moveWithReference",
  moveIndividually: "pcbnew.InteractiveMove.moveIndividually",
  positionRelative: "pcbnew.PositionRelative.positionRelative",
  interactiveOffset: "pcbnew.PositionRelative.interactiveOffsetTool",
  healShapes: "pcbnew.InteractiveEdit.healShapes",
  simplifyPolygons: "pcbnew.InteractiveEdit.simplifyPolygons",
  filletLines: "pcbnew.InteractiveEdit.filletLines",
  chamferLines: "pcbnew.InteractiveEdit.chamferLines",
  dogbone: "pcbnew.InteractiveEdit.dogboneCorners",
  extendLines: "pcbnew.InteractiveEdit.extendLines",
  moveCorner: "pcbnew.InteractiveEdit.moveCorner",
  moveMidpoint: "pcbnew.InteractiveEdit.moveMidpoint",
  addCorner: "pcbnew.PointEditor.addCorner",
  removeCorner: "pcbnew.PointEditor.removeCorner",
  chamferCorner: "pcbnew.PointEditor.chamferCorner",
  editVertices: "pcbnew.InteractiveEdit.editVertices",
  mergePolygons: "pcbnew.InteractiveEdit.mergePolygons",
  subtractPolygons: "pcbnew.InteractiveEdit.subtractPolygons",
  intersectPolygons: "pcbnew.InteractiveEdit.intersectPolygons",
  copyWithReference: "pcbnew.InteractiveMove.copyWithReference",
  cut: "common.Interactive.cut",
  copy: "common.Interactive.copy",
  copyAsText: "common.Interactive.copyAsText",
  paste: "common.Interactive.paste",
  pasteSpecial: "common.Interactive.pasteSpecial",
  duplicate: "common.Interactive.duplicate",
  doDelete: "common.Interactive.delete",
  properties: "pcbnew.InteractiveEdit.properties",
  copyPadSettings: "pcbnew.PadTool.CopyPadSettings",
  applyPadSettings: "pcbnew.PadTool.ApplyPadSettings",
  pushPadSettings: "pcbnew.PadTool.PushPadSettings",
  cycleArcEditMode: "common.Interactive.cycleArcEditMode",
  getAndPlace: "pcbnew.InteractiveEdit.FindMove",
  filterSelection: "pcbnew.InteractiveSelection.FilterSelection",
  selectConnection: "pcbnew.InteractiveSelection.SelectConnection",
  selectNet: "pcbnew.InteractiveSelection.SelectNet",
  selectSameSheet: "pcbnew.InteractiveSelection.SelectSameSheet",
  selectOnSchematic: "pcbnew.InteractiveSelection.SelectOnSchematic",
  selectUnconnected: "pcbnew.InteractiveSelection.SelectUnconnected",
  grabUnconnected: "pcbnew.InteractiveSelection.GrabUnconnected",
  lock: "pcbnew.EditorControl.lock",
  unlock: "pcbnew.EditorControl.unlock",
  toggleLock: "pcbnew.EditorControl.toggleLock",
  showNet: "pcbnew.EditorControl.showNet",
  hideNet: "pcbnew.EditorControl.hideNet",
  highlightNetSelection: "pcbnew.EditorControl.highlightNetSelection",
  alignLeft: "pcbnew.AlignAndDistribute.alignLeft",
  alignCenterX: "pcbnew.AlignAndDistribute.alignCenterX",
  alignRight: "pcbnew.AlignAndDistribute.alignRight",
  alignTop: "pcbnew.AlignAndDistribute.alignTop",
  alignCenterY: "pcbnew.AlignAndDistribute.alignCenterY",
  alignBottom: "pcbnew.AlignAndDistribute.alignBottom",
  distributeHCenters: "pcbnew.AlignAndDistribute.distributeHorizontallyCenters",
  distributeHGaps: "pcbnew.AlignAndDistribute.distributeHorizontallyGaps",
  distributeVCenters: "pcbnew.AlignAndDistribute.distributeVerticallyCenters",
  distributeVGaps: "pcbnew.AlignAndDistribute.distributeVerticallyGaps",
  convertToPoly: "pcbnew.Convert.convertToPoly",
  convertToZone: "pcbnew.Convert.convertToZone",
  convertToKeepout: "pcbnew.Convert.convertToKeepout",
  convertToLines: "pcbnew.Convert.convertToLines",
  outsetItems: "pcbnew.Convert.outsetItems",
  convertToTracks: "pcbnew.Convert.convertToTracks",
  convertToArc: "pcbnew.Convert.convertToArc",
  createArray: "pcbnew.Array.createArray",
  group: "common.Interactive.group",
  ungroup: "common.Interactive.ungroup",
  addToGroup: "common.Interactive.addToGroup",
  removeFromGroup: "common.Interactive.removeFromGroup",
  zoneFill: "pcbnew.ZoneFiller.zoneFill",
  zoneFillAll: "pcbnew.ZoneFiller.zoneFillAll",
  zoneUnfill: "pcbnew.ZoneFiller.zoneUnfill",
  zoneUnfillAll: "pcbnew.ZoneFiller.zoneUnfillAll",
  zoneMerge: "pcbnew.EditorControl.zoneMerge",
  zoneDuplicate: "pcbnew.EditorControl.zoneDuplicate",
  zoneCutout: "pcbnew.InteractiveDrawing.zoneCutout",
  similarZone: "pcbnew.InteractiveDrawing.similarZone",
  zonePriorityTop: "pcbnew.EditorControl.zonePriorityMoveToTop",
  zonePriorityRaise: "pcbnew.EditorControl.zonePriorityRaise",
  zonePriorityLower: "pcbnew.EditorControl.zonePriorityLower",
  zonePriorityBottom: "pcbnew.EditorControl.zonePriorityMoveToBottom",
  zonesManager: "pcbnew.Control.zonesManager",
  editGridOrigin: "common.Control.editGridOrigin",
  zoomPreset: "common.Control.zoomPreset",
  gridPreset: "common.Control.gridPreset",
} as const;

/** The actions of the selection tool's menu, by name, for the callers that need to tell one from another (and the tests). */
export const PCB_MENU_ACTIONS = A;

// ---------------------------------------------------------------------------------------------------------------------------------------- the conditions of EDIT_TOOL

/** `notMovingCondition`: nothing is selected, or what is selected is not in the hand. */
const notMoving: Cond = (_s, c) => !c.moving;
/** `isRoutable`: `NotEmpty && HasTypes( routableTypes ) && notMovingCondition && !inFootprintEditor`. */
const isRoutable: Cond = and(notEmpty, whenHas(...ROUTABLE_TYPES), notMoving);
/** `canMirror` (the board editor's): a selection of pads alone cannot, one holding a group can, else any mirrorable type. */
const canMirror: Cond = (s) => {
  if (onlyTypes(s, ["pad"])) return false;
  if (hasTypes(s, ["group"])) return true;
  return hasTypes(s, MIRRORABLE_TYPES);
};
const singleFootprint: Cond = and(whenOnly("footprint"), countIs(1));
const multipleFootprints: Cond = (s) => s.footprints > 1;
/** `propertiesCondition`: one item, or several tracks. (Nothing selected: Properties of the drawing sheet under the cursor, which this studio has no item for.) */
const propertiesCondition: Cond = (s) => {
  if (s.total === 0) return false;
  if (s.total === 1) return true;
  return onlyTypes(s, TRACK_TYPES);
};
/** `PCB_SELECTION_CONDITIONS::HasUnlockedItems / HasLockedItems`. */
const hasUnlocked: Cond = (s) => s.unlocked > 0;
const hasLocked: Cond = (s) => s.locked > 0;
/** `noItemsCondition`: the board has something on it. (Its name says the opposite.) */
const boardHasItems: Cond = (_s, c) => c.boardHasItems;

// -------------------------------------------------------------------------------------------------------------------------------------------------- submenus

/** `makeMirrorRotateMenu`. */
function mirrorRotateMenu(): ConditionalMenu {
  const menu = new ConditionalMenu();
  menu.addItem(A.rotateCcw, notEmpty);
  menu.addItem(A.rotateCw, notEmpty);
  menu.addItem(A.mirrorH, canMirror);
  menu.addItem(A.mirrorV, canMirror);
  return menu;
}

/** `makeRoutingToolsMenu`. */
function routingMenu(): ConditionalMenu {
  const menu = new ConditionalMenu();
  menu.addItem(A.routeSelected, isRoutable);
  menu.addItem(A.routeSelectedFromEnd, isRoutable);
  menu.addItem(A.unrouteSelected, isRoutable);
  menu.addItem(A.unrouteSegment, isRoutable);
  menu.addItem(A.autoroute, isRoutable);
  return menu;
}

/** `makePositioningToolsMenu` ("Position"). */
function positioningMenu(): ConditionalMenu {
  const menu = new ConditionalMenu();
  const some = and(notEmpty, notMoving);
  menu.addItem(A.moveExact, some);
  menu.addItem(A.moveWithReference, some);
  menu.addItem(A.moveIndividually, and(moreThan(1), notMoving));
  menu.addItem(A.positionRelative, some);
  menu.addItem(A.interactiveOffset, some);
  return menu;
}

/** `makeShapeModificationMenu`. */
function shapeModificationMenu(): ConditionalMenu {
  const menu = new ConditionalMenu();
  const filletChamfer = whenOnly(...FILLET_CHAMFER_TYPES);
  const polygonBoolean = and(whenOnly(...BOOLEAN_TYPES), moreThan(1));
  // Shape cleanup
  menu.addItem(A.healShapes, whenHas(...HEAL_TYPES));
  menu.addItem(A.simplifyPolygons, whenHas(...POLYGON_SIMPLIFY_TYPES));
  menu.addSeparator(filletChamfer);
  // Shape corner modifications
  menu.addItem(A.filletLines, filletChamfer);
  menu.addItem(A.chamferLines, filletChamfer);
  menu.addItem(A.dogbone, filletChamfer);
  menu.addItem(A.extendLines, and(whenOnly("segment"), countIs(2)));
  menu.addSeparator(countIs(1));
  // Point editor corner operations
  menu.addItem(A.moveCorner, (_s, c) => c.handle.corner);
  menu.addItem(A.moveMidpoint, (_s, c) => c.handle.midpoint);
  menu.addItem(A.addCorner, and(countIs(1), (s) => s.canAddCorner));
  menu.addItem(A.removeCorner, and(countIs(1), (_s, c) => c.handle.canRemoveCorner));
  menu.addItem(A.chamferCorner, and(countIs(1), (s) => s.canChamferCorner));
  menu.addItem(A.editVertices, (s) => s.total === 1 && s.editableCorners);
  menu.addSeparator(polygonBoolean);
  // Polygon boolean operations
  menu.addItem(A.mergePolygons, polygonBoolean);
  menu.addItem(A.subtractPolygons, polygonBoolean);
  menu.addItem(A.intersectPolygons, polygonBoolean);
  return menu;
}

/** `LOCK_CONTEXT_MENU` ("Locking"). */
function lockMenu(): ConditionalMenu {
  const menu = new ConditionalMenu();
  menu.addItem(A.lock, hasUnlocked);
  menu.addItem(A.unlock, hasLocked);
  menu.addItem(A.toggleLock, always);
  return menu;
}

/** `SELECT_MENU` ("Select"): every entry is there; whether it can be used is the action's own enable condition (`pcb_edit_frame.cpp` `setupUIConditions`). */
function selectMenu(s: Sel): MenuNode[] {
  return [
    item(A.filterSelection),
    sep,
    item(A.selectConnection),
    item(A.selectNet, { disabled: !onlyTypes(s, TRACK_TYPES) }),
    item(A.selectSameSheet, { disabled: !onlyTypes(s, ["footprint"]) }),
    item(A.selectOnSchematic, { disabled: !hasTypes(s, CROSS_PROBE_TYPES) }),
    item(A.selectUnconnected, { disabled: !onlyTypes(s, PAD_OWNER_TYPES) }),
    item(A.grabUnconnected),
  ];
}

/** `GROUP_CONTEXT_MENU` ("Grouping"): the four entries, enabled by what the selection holds (`GROUP_CONTEXT_MENU::update`). */
function groupingMenu(s: Sel): MenuNode[] {
  return [
    item(A.group, { disabled: s.total < 2 }),
    item(A.ungroup, { disabled: !s.group.hasGroup }),
    item(A.addToGroup, { disabled: !(s.group.onlyOneGroup && s.group.hasUngroupedItems) }),
    item(A.removeFromGroup, { disabled: !s.group.hasMember }),
  ];
}

/** `NET_CONTEXT_MENU` ("Net Inspection Tools"). */
function netMenu(s: Sel): MenuNode[] {
  return [item(A.showNet, { disabled: !s.hasNet }), item(A.hideNet, { disabled: !s.hasNet }), sep, item(A.highlightNetSelection), item(A.clearHighlight)];
}

/** `ZONE_CONTEXT_MENU` ("Zones") with `ZONE_PRIORITY_CONTEXT_MENU` ("Zone Priority"). */
function zoneMenu(s: Sel): MenuNode[] {
  const single = s.total === 1 && s.zones === 1;
  return [
    item(A.zoneFill),
    item(A.zoneFillAll),
    item(A.zoneUnfill),
    item(A.zoneUnfillAll),
    sep,
    item(A.zoneMerge, { disabled: !(s.total > 1 && onlyTypes(s, ["zone"])) }),
    item(A.zoneDuplicate, { disabled: !single }),
    item(A.zoneCutout, { disabled: !single }),
    item(A.similarZone, { disabled: !single }),
    sep,
    {
      type: "submenu",
      label: "Zone Priority",
      items: [item(A.zonePriorityTop, { disabled: !s.zoneCanRaise }), item(A.zonePriorityRaise, { disabled: !s.zoneCanRaise }), item(A.zonePriorityLower, { disabled: !s.zoneCanLower }), item(A.zonePriorityBottom, { disabled: !s.zoneCanLower })],
    },
    sep,
    item(A.zonesManager),
  ];
}

/** `ALIGN_DISTRIBUTE_TOOL::Init`'s `m_placementMenu` ("Align/Distribute"). */
function placementMenu(): ConditionalMenu {
  const menu = new ConditionalMenu();
  const canAlign = moreThan(1);
  const canDistribute = moreThan(2);
  menu.addItem(A.alignLeft, canAlign);
  menu.addItem(A.alignCenterX, canAlign);
  menu.addItem(A.alignRight, canAlign);
  menu.addSeparator(canAlign);
  menu.addItem(A.alignTop, canAlign);
  menu.addItem(A.alignCenterY, canAlign);
  menu.addItem(A.alignBottom, canAlign);
  menu.addSeparator(canDistribute);
  menu.addItem(A.distributeHCenters, canDistribute);
  menu.addItem(A.distributeHGaps, canDistribute);
  menu.addItem(A.distributeVCenters, canDistribute);
  menu.addItem(A.distributeVGaps, canDistribute);
  return menu;
}

/** `CONVERT_TOOL::Init`'s menu ("Create from Selection"); it is offered when any of its entries is (`canCreate`). */
function convertMenu(): { menu: ConditionalMenu; canCreate: Cond } {
  const menu = new ConditionalMenu();
  const poly: Cond = (s) => s.convert.poly;
  menu.addItem(A.convertToPoly, poly);
  menu.addItem(A.convertToZone, poly);
  menu.addItem(A.convertToKeepout, poly);
  menu.addItem(A.convertToLines, (s) => s.convert.lines);
  menu.addItem(A.outsetItems, (s) => s.convert.outset);
  menu.addSeparator();
  menu.addItem(A.convertToTracks, (s) => s.convert.tracks);
  menu.addItem(A.convertToArc, (s) => s.convert.arc);
  menu.addSeparator();
  menu.addItem(A.createArray, moreThan(0)); // `canCreateArray = S_C::MoreThan( 0 )`
  const canCreate: Cond = (s) => s.convert.poly || s.convert.lines || s.convert.tracks || s.convert.arc || s.total > 0 || s.convert.outset;
  return { menu, canCreate };
}

/** `ZOOM_MENU::update`: "Zoom: 1.00" ..., the one nearest the current zoom checked. The entry `i` is the preset `i + 1` (0 is Auto Zoom, which the menu has none of). */
function zoomMenu(_s: Sel, c: PcbMenuContext): MenuNode[] {
  return c.zoom.map((z, i) => item(A.zoomPreset, { label: z.label, arg: i + 1, checked: z.checked }));
}

/** `GRID_MENU::update`: Grid Origin..., a line, then each grid with the current one checked. */
function gridMenu(_s: Sel, c: PcbMenuContext): MenuNode[] {
  return [item(A.editGridOrigin), sep, ...c.grid.map((g, i) => item(A.gridPreset, { label: g.label, arg: i, checked: g.checked }))];
}

// ---------------------------------------------------------------------------------------------------------------------------------- the selection tool's menu

/**
 * `PCB_SELECTION_TOOL`'s context menu for the board editor, built the way the tools' `Init()`s build it, in the order `setupTools` initialises them. The numbers
 * are the `aOrder`s the C++ passes; where it passes none (`ANY_ORDER`) the entry takes the count of entries so far, as `addEntry` does.
 */
export function buildSelectionMenu(): ConditionalMenu {
  const menu = new ConditionalMenu();

  // ---- PCB_SELECTION_TOOL::Init
  const groupEnterCondition: Cond = and(countIs(1), whenHas("group"));
  const tableCellSelection: Cond = () => false; // no table cells in the IR
  menu.addMenu("Select", selectMenu, notEmpty);
  menu.addSeparator(always, 1000);
  // "Cancel" goes at the top of the context menu when a tool is active
  menu.addItem(A.cancel, (_s, c) => c.toolActive, 1);
  menu.addItem(A.groupEnter, groupEnterCondition, 1);
  menu.addItem(A.groupLeave, (_s, c) => c.inGroup, 1);
  menu.addItem(A.applyDesignBlockLayout, groupEnterCondition, 1);
  menu.addItem(A.placeLinkedDesignBlock, groupEnterCondition, 1);
  menu.addItem(A.saveToLinkedDesignBlock, groupEnterCondition, 1);
  menu.addItem(A.clearHighlight, (_s, c) => c.haveHighlight, 1);
  menu.addSeparator((_s, c) => c.haveHighlight, 1);
  menu.addItem(A.selectColumns, tableCellSelection, 2);
  menu.addItem(A.selectRows, tableCellSelection, 2);
  menu.addItem(A.selectTable, tableCellSelection, 2);
  menu.addSeparator(always, 1);
  // frame->AddStandardSubMenus
  menu.addSeparator(always, 1000);
  menu.addMenu("Zoom", zoomMenu, always, 1000);
  menu.addMenu("Grid", gridMenu, always, 1000);

  // ---- EDIT_TOOL::Init
  const routing = routingMenu();
  const positioning = positioningMenu();
  const mirrorRotate = mirrorRotateMenu();
  const shapeModification = shapeModificationMenu();
  const positioningCondition: Cond = (s, c) => positioning.evaluate(s, c).length > 0;
  const shapeModificationCondition: Cond = (s, c) => shapeModification.evaluate(s, c).length > 0;
  const isSkippable: Cond = (_s, c) => c.movingIndividually;
  menu.addItem(A.selectAll, boardHasItems);
  menu.addItem(A.unselectAll, boardHasItems);
  menu.addSeparator();
  menu.addItem(A.skip, isSkippable);
  menu.addItem(A.move, and(notEmpty, notMoving));
  menu.addItem(A.drag45, and(countIs(1), whenOnly(...DRAGGABLE_TYPES)));
  menu.addItem(A.dragFree, and(countIs(1), whenOnly(...DRAGGABLE_TYPES), not(whenOnly("footprint"))));
  menu.addItem(A.flip, notEmpty);
  menu.addItem(A.swap, moreThan(1));
  menu.addItem(A.swapPadNets, and(moreThan(1), whenOnly("pad")));
  // (Swap Gate Nets and its submenu need the footprint's units, which the IR does not have.)
  menu.addSeparator();
  menu.addItem(A.breakTrack, and(countIs(1), whenOnly(...TRACK_TYPES)));
  menu.addItem(A.filletTracks, whenOnly(...TRACK_TYPES));
  menu.addSeparator();
  menu.addItem(A.packAndMove, and(moreThan(1), whenHas("footprint")));
  menu.addItem(A.assignNetclass, whenOnly(...CONNECTED_TYPES));
  menu.addItem(A.inspectClearance, countIs(2));
  // Footprint actions
  menu.addSeparator();
  menu.addItem(A.editFpInFpEditor, singleFootprint);
  menu.addItem(A.updateFootprint, singleFootprint);
  menu.addItem(A.updateFootprints, multipleFootprints);
  menu.addItem(A.changeFootprint, singleFootprint);
  menu.addItem(A.changeFootprints, multipleFootprints);
  // Add the submenu for the special tools: modifiers and positioning tools
  menu.addSeparator(always, 100);
  menu.addMenu("Routing", submenu(routing), isRoutable, 100);
  menu.addMenu("Mirror / Rotate", submenu(mirrorRotate), canMirror, 100);
  menu.addMenu("Shape Modification", submenu(shapeModification), shapeModificationCondition, 100);
  menu.addMenu("Position", submenu(positioning), positioningCondition, 100);
  menu.addSeparator(always, 150);
  menu.addItem(A.cut, notEmpty, 150);
  menu.addItem(A.copy, notEmpty, 150);
  menu.addItem(A.copyWithReference, and(notEmpty, notMoving), 150);
  menu.addItem(A.copyAsText, and(notEmpty, whenOnly(...COPY_AS_TEXT_TYPES)), 150);
  // Selection tool handles the context menu for some other tools, such as the Picker. Don't add things like Paste when another tool is active.
  const noActiveTool: Cond = (_s, c) => !c.toolActive;
  menu.addItem(A.paste, noActiveTool, 150);
  menu.addItem(A.pasteSpecial, noActiveTool, 150);
  menu.addItem(A.duplicate, notEmpty, 150);
  menu.addItem(A.doDelete, notEmpty, 150);
  menu.addSeparator(always, 2000);
  menu.addItem(A.properties, propertiesCondition, 2000);

  // ---- PAD_TOOL::Init
  const padSelection: Cond = whenHas("pad");
  const singlePad: Cond = and(countIs(1), whenOnly("pad"));
  menu.addSeparator(always, 400);
  menu.addItem(A.copyPadSettings, singlePad, 400);
  menu.addItem(A.applyPadSettings, padSelection, 400);
  menu.addItem(A.pushPadSettings, singlePad, 400);

  // ---- PCB_POINT_EDITOR::Init
  menu.addItem(A.cycleArcEditMode, and(countIs(1), (s) => s.shapes.arc === 1 && s.total === 1));

  // ---- BOARD_EDITOR_CONTROL::Init
  const inactiveState: Cond = (s, c) => !c.toolActive && s.total === 0;
  menu.addItem(A.getAndPlace, inactiveState);
  menu.addSeparator();
  menu.addMenu("Locking", submenu(lockMenu()), notEmpty, 100);
  menu.addMenu("Zones", zoneMenu, whenOnly("zone"), 100);

  // ---- BOARD_INSPECTION_TOOL::Init: "Only show the net menu if all items in the selection are connectable"
  const showNetMenu: Cond = (s) => s.total > 0 && s.footprints === 0 && s.texts === 0 && s.dimensions === 0 && s.groups === 0 && s.shapes.segment + s.shapes.rect + s.shapes.circle + s.shapes.arc + s.shapes.polygon + s.shapes.bezier === s.copperShapes;
  menu.addMenu("Net Inspection Tools", netMenu, showNetMenu, 100);

  // ---- ALIGN_DISTRIBUTE_TOOL::Init
  menu.addMenu("Align/Distribute", submenu(placementMenu()), moreThan(1), 100);

  // ---- CONVERT_TOOL::Init
  const convert = convertMenu();
  menu.addMenu("Create from Selection", submenu(convert.menu), convert.canCreate, 100);

  // ---- GROUP_TOOL::Init
  menu.addMenu("Grouping", groupingMenu, notEmpty, 100);

  return menu;
}

/** The right-click menu of the selection tool for `s`: what KiCad shows when the selection tool (or a tool that lets it, like Move) takes the click. */
export function pcbSelectionMenu(s: PcbSelectionSummary, c: PcbMenuContext = defaultMenuContext()): MenuNode[] {
  return buildSelectionMenu().evaluate(s, c);
}

// ------------------------------------------------------------------------------------------------------------------------------------------ the router's menu

/** What the router's menu asks about the tool and the board (`ROUTER_TOOL::Init`'s lambdas). */
export interface RouterMenuContext {
  /** `m_router->RoutingInProgress()`. */
  routing: boolean;
  /** `m_inRouteSelected`: Route Selected is running. */
  inRouteSelected: boolean;
  /** `hasOtherEnd`: the net being routed has something left unconnected to finish to. */
  hasOtherEnd: boolean;
  /** A selection exists (`SELECTION_CONDITIONS::NotEmpty`, for Autoroute Selected). */
  hasSelection: boolean;
  haveHighlight: boolean;
  /** The pair tool is running (`m_router->Mode() == PNS_MODE_ROUTE_DIFF_PAIR`). */
  diffPair: boolean;
  /** `Settings().GetCornerMode()`: which of the four the router is in. */
  cornerMode: "45" | "arc45" | "90" | "arc90";
  /** The Track/Via Width and Differential Pair Dimensions menus (data: `TRACK_WIDTH_MENU::update`, `DIFF_PAIR_MENU::update`). */
  trackViaMenu: readonly MenuNode[];
  diffPairMenu: readonly MenuNode[];
  zoom: readonly { label: string; checked: boolean }[];
  grid: readonly { label: string; checked: boolean }[];
}

const R = {
  placeVia: "pcbnew.InteractiveRouter.PlaceVia",
  placeBlindVia: "pcbnew.InteractiveRouter.PlaceBlindVia",
  placeMicroVia: "pcbnew.InteractiveRouter.PlaceMicroVia",
  selLayerVia: "pcbnew.InteractiveRouter.SelLayerAndPlaceVia",
  selLayerBlindVia: "pcbnew.InteractiveRouter.SelLayerAndPlaceBlindVia",
  selLayerMicroVia: "pcbnew.InteractiveRouter.SelLayerAndPlaceMicroVia",
  switchPosture: "pcbnew.InteractiveRouter.SwitchPosture",
  cornerNext: "pcbnew.InteractiveRouter.SwitchRoundingToNext",
  corner45: "pcbnew.InteractiveRouter.SwitchRounding45",
  cornerArc45: "pcbnew.InteractiveRouter.SwitchRoundingArc45",
  corner90: "pcbnew.InteractiveRouter.SwitchRounding90",
  cornerArc90: "pcbnew.InteractiveRouter.SwitchRoundingArc90",
  singleTrack: "pcbnew.InteractiveRouter.SingleTrack",
  diffPairTool: "pcbnew.InteractiveRouter.DiffPair",
  finish: "common.Interactive.finish",
  undoLastSegment: "pcbnew.InteractiveRouter.UndoLastSegment",
  continueFromEnd: "pcbnew.InteractiveRouter.ContinueFromEnd",
  attemptFinish: "pcbnew.InteractiveRouter.AttemptFinish",
  cancelCurrentItem: "pcbnew.InteractiveRouter.CancelCurrentItem",
  settings: "pcbnew.InteractiveRouter.SettingsDialog",
} as const;

/** The labels of the router's own actions, which `actions.json` does not carry (they are `static const TOOL_ACTION`s inside router_tool.cpp). */
export const ROUTER_ACTION_LABELS: Readonly<Record<string, string>> = {
  [R.placeVia]: "Place Through Via",
  [R.placeBlindVia]: "Place Blind/Buried Via",
  [R.placeMicroVia]: "Place Microvia",
  [R.selLayerVia]: "Select Layer and Place Through Via...",
  [R.selLayerBlindVia]: "Select Layer and Place Blind/Buried Via...",
  [R.selLayerMicroVia]: "Select Layer and Place Micro Via...",
  [R.switchPosture]: "Switch Track Posture",
  [R.cornerNext]: "Track Corner Mode Switch",
  [R.corner45]: "Track Corner Mode 45",
  [R.cornerArc45]: "Track Corner Mode Arc 45",
  [R.corner90]: "Track Corner Mode 90",
  [R.cornerArc90]: "Track Corner Mode Arc 90",
};

/**
 * `ROUTER_TOOL::Init`'s menu, the one the right click opens while the router is running (a route in progress, or the tool armed): Cancel, the route's own
 * commands, the via and posture actions, the corner mode, the width menus, the settings and the standard Zoom and Grid.
 */
export function routerMenu(r: RouterMenuContext): MenuNode[] {
  const s = emptyPcbSummary();
  const c: PcbMenuContext = { ...defaultMenuContext(), haveHighlight: r.haveHighlight, zoom: r.zoom, grid: r.grid };
  const menu = new ConditionalMenu();
  const notRouting: Cond = () => !r.routing;
  const withLabel = (action: string) => ({ label: ROUTER_ACTION_LABELS[action] });
  menu.addItem(A.cancel, always, 1);
  menu.addItem(R.cancelCurrentItem, () => r.inRouteSelected, 1);
  menu.addSeparator(always, 1);
  menu.addItem(A.clearHighlight, () => r.haveHighlight, 2);
  menu.addSeparator(() => r.haveHighlight, 2);
  menu.addItem(R.singleTrack, notRouting);
  menu.addItem(R.diffPairTool, notRouting);
  menu.addItem(R.finish, always);
  menu.addItem(R.undoLastSegment, always);
  menu.addItem(R.continueFromEnd, () => r.hasOtherEnd);
  menu.addItem(R.attemptFinish, () => r.hasOtherEnd);
  menu.addItem(A.autoroute, and(notRouting, () => r.hasSelection));
  menu.addItem(A.breakTrack, notRouting);
  menu.addItem(A.drag45, notRouting);
  menu.addItem(A.dragFree, notRouting);
  for (const via of [R.placeVia, R.placeBlindVia, R.placeMicroVia, R.selLayerVia, R.selLayerBlindVia, R.selLayerMicroVia, R.switchPosture]) menu.addItem(via, always, ANY_ORDER, withLabel(via));
  // "Track Corner Mode": Switch, a line, then one check item per mode.
  const corner = new ConditionalMenu();
  corner.addItem(R.cornerNext, always, ANY_ORDER, withLabel(R.cornerNext));
  corner.addSeparator(always, 1);
  const mode = (action: string, which: RouterMenuContext["cornerMode"]) => corner.addItem(action, always, ANY_ORDER, { ...withLabel(action), checked: () => r.cornerMode === which });
  mode(R.corner45, "45");
  mode(R.cornerArc45, "arc45");
  mode(R.corner90, "90");
  mode(R.cornerArc90, "arc90");
  menu.addMenu("Track Corner Mode", submenu(corner), always);
  menu.addSeparator();
  menu.addMenu("Select Track/Via Width", () => [...r.trackViaMenu], always);
  menu.addMenu("Select Differential Pair Dimensions", () => [...r.diffPairMenu], () => r.diffPair);
  menu.addItem(R.settings, always);
  menu.addSeparator();
  // frame->AddStandardSubMenus
  menu.addSeparator(always, 1000);
  menu.addMenu("Zoom", zoomMenu, always, 1000);
  menu.addMenu("Grid", gridMenu, always, 1000);
  return menu.evaluate(s, c);
}

// ----------------------------------------------------------------------------------------------------------------------------------- the drawing tools' menu

/** `DRAWING_TOOL::MODE`, as far as the menu tells them apart. */
export type DrawingMode = "line" | "circle" | "arc" | "rectangle" | "bezier" | "polygon" | "text" | "dimension" | "keepout" | "zone" | "via";

/** `DRAWING_TOOL::Init`'s menu: Cancel, what the running tool offers (Close Outline, Delete Last Point, Switch Arc Posture, Switch Dimension Arrows), Zoom and Grid. */
export function drawingMenu(mode: DrawingMode, c: PcbMenuContext, zoneMenuFor?: PcbSelectionSummary): MenuNode[] {
  const menu = new ConditionalMenu();
  const canUndoPoint = ["arc", "zone", "keepout", "polygon", "bezier", "line"].includes(mode);
  const canCloseOutline = ["zone", "keepout", "polygon"].includes(mode);
  menu.addItem(A.cancel, always, 1);
  menu.addSeparator(always, 1);
  menu.addItem(A.clearHighlight, (_s, ctx) => ctx.haveHighlight, 2);
  menu.addSeparator((_s, ctx) => ctx.haveHighlight, 2);
  menu.addItem("pcbnew.InteractiveDrawing.closeOutline", () => canCloseOutline, 200);
  menu.addItem("pcbnew.InteractiveDrawing.deleteLastPoint", () => canUndoPoint, 200);
  menu.addItem("pcbnew.InteractiveDrawing.arcPosture", () => mode === "arc", 200);
  menu.addItem("pcbnew.InteractiveDrawing.changeDimensionArrows", () => mode === "dimension", 200);
  menu.addSeparator(always, 500);
  menu.addSeparator(always, 500);
  // The zone menu the PCB control tool adds for the zone tool (`menu.AddMenu( zoneMenu, toolActiveFunctor( MODE::ZONE ), 300 )`).
  menu.addMenu("Zones", zoneMenu, () => mode === "zone", 300);
  menu.addSeparator(always, 1000);
  menu.addMenu("Zoom", zoomMenu, always, 1000);
  menu.addMenu("Grid", gridMenu, always, 1000);
  return menu.evaluate(zoneMenuFor ?? emptyPcbSummary(), c);
}

/** The menu of a tool that only offers Cancel and the standard submenus (the Picker: Delete tool, Local Ratsnest, Set Origin). */
export function pickerMenu(c: PcbMenuContext): MenuNode[] {
  const menu = new ConditionalMenu();
  menu.addItem(A.cancel, always, 1);
  menu.addSeparator(always, 1);
  menu.addSeparator(always, 1000);
  menu.addMenu("Zoom", zoomMenu, always, 1000);
  menu.addMenu("Grid", gridMenu, always, 1000);
  return menu.evaluate(emptyPcbSummary(), c);
}

/** The actions a menu names, flattened (for the tests and for the callers that need to know which actions a menu would run). */
export function menuActions(nodes: readonly MenuNode[]): string[] {
  return nodes.flatMap((n) => (n.type === "item" ? [n.action] : n.type === "submenu" ? menuActions(n.items) : []));
}
