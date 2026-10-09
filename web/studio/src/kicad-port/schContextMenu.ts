// The schematic's right-click menu: which actions `SCH_SELECTION_TOOL::Init` and `SCH_EDIT_TOOL::Init`
// (sch_selection_tool.cpp, sch_edit_tool.cpp at 8303b2ad) offer for a given selection. Each entry carries the
// condition KiCad puts on it -- `SCH_CONDITIONS::Count/MoreThan/OnlyTypes/HasTypes` over the selection's item types --
// evaluated here over a summary of the selection. The studio shows an action KiCad would offer but that is not wired
// as a disabled entry ("not ported yet"), the way its menus already do.
import type { MenuNode } from "../kicad/types";

/** How many selected items there are of each kind the menu's conditions distinguish. */
export interface SchSelectionSummary {
  total: number;
  symbols: number;
  powerSymbols: number;
  /** Wires (not buses). */
  wires: number;
  buses: number;
  /** Graphic lines on the notes layer (`SCH_LINE_T` on `LAYER_NOTES`). */
  lines: number;
  localLabels: number;
  globalLabels: number;
  hierLabels: number;
  directiveLabels: number;
  texts: number;
  textBoxes: number;
  junctions: number;
  busEntries: number;
  noConnects: number;
  sheets: number;
  /** Rectangles, circles, arcs, beziers and polygons. */
  shapes: number;
  /** Fields of a symbol, a power symbol or a sheet, picked on their own (`SCH_FIELD_T`). */
  fields: number;
  ruleAreas: number;
  locked: number;
  unlocked: number;
  /** The one selected sheet has pins with no hierarchical label of that name inside it (`SCH_SHEET::HasUndefinedPins`). */
  sheetHasUndefinedPins: boolean;
  /** The selection is symbol units of one multi-unit reference (`GetSameSymbolMultiUnitSelection`). */
  sameReferenceUnits: number;
  /** The one selected symbol has more than one body style (`SCH_CONDITIONS::SingleMultiBodyStyleSymbol`). */
  multiBodyStyle: boolean;
  /** Selected pins (pin selection is not part of this studio's schematic yet: always 0 there). */
  pins: number;
  /** The one selected polygon or rule area has its outline under the cursor (`SCH_POINT_EDITOR::addCornerCondition`). */
  canAddCorner: boolean;
  /** ... and a corner of it is under the cursor, leaving enough corners (`removeCornerCondition`). */
  canRemoveCorner: boolean;
}

export function emptySummary(): SchSelectionSummary {
  return {
    total: 0,
    symbols: 0,
    powerSymbols: 0,
    wires: 0,
    buses: 0,
    lines: 0,
    localLabels: 0,
    globalLabels: 0,
    hierLabels: 0,
    directiveLabels: 0,
    texts: 0,
    textBoxes: 0,
    junctions: 0,
    busEntries: 0,
    noConnects: 0,
    sheets: 0,
    shapes: 0,
    fields: 0,
    ruleAreas: 0,
    locked: 0,
    unlocked: 0,
    sheetHasUndefinedPins: false,
    sameReferenceUnits: 0,
    multiBodyStyle: false,
    pins: 0,
    canAddCorner: false,
    canRemoveCorner: false,
  };
}

const item = (action: string): MenuNode => ({ type: "item", action });
const sep: MenuNode = { type: "separator" };

/** Which `Change To` entries the selection gets (`makeConvertToMenu`'s conditions), in the menu's order. */
export function changeToEntries(s: SchSelectionSummary): string[] {
  const labels = s.localLabels + s.globalLabels + s.hierLabels;
  const textish = labels + s.directiveLabels + s.texts + s.textBoxes;
  if (s.total === 0 || textish !== s.total) return []; // `toChangeCondition`: OnlyTypes( allTextTypes )
  // One item: any text-ish type but the target's own. Several: any mix of them.
  const entry = (action: string, ownCount: number) => (s.total > 1 || ownCount === 0 ? [action] : []);
  return [
    ...entry("eeschema.InteractiveEdit.toLabel", s.localLabels),
    ...entry("eeschema.InteractiveEdit.toCLabel", s.directiveLabels),
    ...entry("eeschema.InteractiveEdit.toHLabel", s.hierLabels),
    ...entry("eeschema.InteractiveEdit.toGLabel", s.globalLabels),
    ...entry("eeschema.InteractiveEdit.toText", s.texts),
    ...entry("eeschema.InteractiveEdit.toTextBox", s.textBoxes),
  ];
}

export function schContextMenu(s: SchSelectionSummary): MenuNode[] {
  const out: MenuNode[] = [];
  const add = (...nodes: (MenuNode | false)[]) => out.push(...nodes.filter((n): n is MenuNode => n !== false));

  const labels = s.localLabels + s.globalLabels + s.hierLabels;
  const single = s.total === 1;
  const singleSymbolOrPower = single && s.symbols + s.powerSymbols === 1;
  const multipleSymbolsOrPower = s.total > 1 && s.symbols + s.powerSymbols === s.total;
  const connectedCount = s.powerSymbols + s.wires + s.buses + s.busEntries + labels + s.directiveLabels + s.junctions;
  const connected = connectedCount > 0 && connectedCount === s.total;
  const linesSelection = s.total > 0 && s.wires + s.buses + s.lines === s.total;
  const sheetSelection = single && s.sheets === 1;
  // `RotatableItems` holds the fields too (`SCH_FIELD_T`): a field turns its text, and a mirror flips its justification
  const orientable = s.symbols + s.powerSymbols + labels + s.directiveLabels + s.texts + s.textBoxes + s.shapes + s.ruleAreas + s.sheets + s.fields > 0;

  if (sheetSelection) add(item("eeschema.NavigateTool.enterSheet"), sep);
  // `makeBodyStyleMenu` at `SingleMultiBodyStyleSymbol` (KiCad lists each body style; the studio has the one command that cycles through them)
  add(single && s.symbols === 1 && s.multiBodyStyle && item("eeschema.InteractiveEdit.toggleDeMorgan"));
  if (orientable) {
    add({
      type: "submenu",
      label: "Transform Selection",
      items: [item("eeschema.InteractiveEdit.rotateCCW"), item("eeschema.InteractiveEdit.rotateCW"), item("eeschema.InteractiveEdit.mirrorV"), item("eeschema.InteractiveEdit.mirrorH")],
    });
  }
  // `swapSelectionCondition`: OnlyTypes( SwappableItems ) && MoreThan( 1 ).
  const swappable = s.symbols + s.powerSymbols + labels + s.directiveLabels + s.texts + s.textBoxes + s.shapes + s.ruleAreas + s.sheets + s.junctions + s.noConnects;
  add(s.total > 1 && swappable === s.total && item("eeschema.InteractiveEdit.swap"));
  // `propertiesCondition`: one item of a type that has a properties dialog.
  add(single && s.symbols + s.sheets + s.texts + s.textBoxes + labels + s.directiveLabels + s.shapes + s.ruleAreas + s.fields === 1 && item("eeschema.InteractiveEdit.properties"));
  // `autoplaceCondition`: any selected item is one that has fields (a symbol, a sheet); the command also takes the item of a selected field
  add(s.symbols + s.powerSymbols + s.sheets + s.fields > 0 && item("eeschema.InteractiveEdit.autoplaceFields"));
  if (single && s.symbols === 1) {
    add({ type: "submenu", label: "Edit Main Fields", items: [item("eeschema.InteractiveEdit.editReference"), item("eeschema.InteractiveEdit.editValue"), item("eeschema.InteractiveEdit.editFootprint")] });
  }
  add(singleSymbolOrPower && item("eeschema.InteractiveEdit.changeSymbol"), singleSymbolOrPower && item("eeschema.InteractiveEdit.updateSymbol"), multipleSymbolsOrPower && item("eeschema.InteractiveEdit.changeSymbols"), multipleSymbolsOrPower && item("eeschema.InteractiveEdit.updateSymbols"));

  const changeTo = changeToEntries(s);
  if (changeTo.length > 0) add({ type: "submenu", label: "Change To", items: changeTo.map(item) });

  // Wire/bus and line selections, sheets, units, connected items (SCH_SELECTION_TOOL::Init's priority-250 block).
  add(linesSelection && item("eeschema.InteractiveEdit.breakWire"), linesSelection && item("eeschema.InteractiveEdit.slice"));
  if (sheetSelection) {
    add(item("eeschema.InteractiveDrawing.placeSheetPin"), item("eeschema.InteractiveDrawing.autoplaceAllSheetPins"), item("eeschema.InteractiveDrawing.syncSheetPins"));
    add(s.sheetHasUndefinedPins && item("eeschema.InteractiveEdit.cleanupSheetPins"));
  }
  add(s.pins > 1 && item("eeschema.InteractiveEdit.swapPinLabels"), s.sameReferenceUnits > 1 && item("eeschema.InteractiveEdit.swapUnitLabels"), s.pins > 1 && item("eeschema.InteractiveEdit.swapPins"));
  add(connected && item("eeschema.InteractiveEdit.assignNetclass"), connected && item("eeschema.InteractiveEdit.findNetInInspector"));
  // `SCH_POINT_EDITOR::Init`: Count( 1 ) && addCornerCondition / removeCornerCondition.
  add(single && s.canAddCorner && item("eeschema.PointEditor.addCorner"), single && s.canRemoveCorner && item("eeschema.PointEditor.removeCorner"));

  // The Locking submenu (`LOCK_CONTEXT_MENU`): Lock when anything is unlocked, Unlock when anything is locked, Toggle always.
  if (s.total > 0) {
    add({ type: "submenu", label: "Locking", items: [...(s.unlocked > 0 ? [item("eeschema.InteractiveEdit.lock")] : []), ...(s.locked > 0 ? [item("eeschema.InteractiveEdit.unlock")] : []), item("eeschema.InteractiveEdit.toggleLock")] });
  }

  // The clipboard group (`selToolMenu.AddItem( ACTIONS::cut ... ACTIONS::duplicate, ..., 300 )`): Cut and Copy for a selection, Paste and Paste Special
  // always (`S_C::Idle`), then Delete and Duplicate for a selection. Copy as Text (`canCopyText`) is not ported.
  const some = s.total > 0;
  add(sep, some && item("common.Interactive.cut"), some && item("common.Interactive.copy"), item("common.Interactive.paste"), item("common.Interactive.pasteSpecial"), some && item("common.Interactive.delete"), some && item("common.Interactive.duplicate"), sep, item("common.Interactive.selectAll"));
  return out;
}
