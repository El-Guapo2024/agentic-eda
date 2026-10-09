import { test } from "node:test";
import assert from "node:assert/strict";
import type { MenuNode } from "../kicad/types";
import { ConditionalMenu, defaultMenuContext, drawingMenu, emptyPcbSummary, emptyShapes, menuActions, pcbSelectionMenu, PCB_MENU_ACTIONS as A, pickerMenu, routerMenu, type PcbMenuContext, type PcbSelectionSummary, type RouterMenuContext } from "./pcbContextMenu";

const sel = (over: Omit<Partial<PcbSelectionSummary>, "shapes"> & { shapes?: Partial<PcbSelectionSummary["shapes"]> }): PcbSelectionSummary => {
  const s = { ...emptyPcbSummary(), ...over, shapes: { ...emptyShapes(), ...(over.shapes ?? {}) } };
  const kinds = s.footprints + s.pads + s.tracks + s.arcTracks + s.vias + s.zones + s.texts + s.dimensions + s.groups + Object.values(s.shapes).reduce((a, b) => a + b, 0);
  if (over.total === undefined) s.total = kinds;
  if (over.locked === undefined && over.unlocked === undefined) s.unlocked = s.total;
  return s;
};
const ctx = (over: Partial<PcbMenuContext> = {}): PcbMenuContext => ({ ...defaultMenuContext(), ...over });
const actions = (s: PcbSelectionSummary, c?: PcbMenuContext): string[] => menuActions(pcbSelectionMenu(s, c));
const submenu = (nodes: MenuNode[], label: string): MenuNode[] | null => {
  const n = nodes.find((x) => x.type === "submenu" && x.label === label);
  return n && n.type === "submenu" ? n.items : null;
};
/** The menu as a list of titles: a submenu is its label with `>`; a separator `-`; an action its name. */
const outline = (nodes: MenuNode[]): string[] => nodes.map((n) => (n.type === "separator" ? "-" : n.type === "submenu" ? `${n.label} >` : n.action));
const disabledIn = (nodes: MenuNode[]): string[] => menuActions(nodes.filter((n) => n.type === "item" && n.disabled));

// ------------------------------------------------------------------------------------------------------------------- order and structure

test("a single footprint: Select, then the edit tool's entries, the submenus, the clipboard block, Zoom and Grid, and Properties last -- KiCad's order", () => {
  const nodes = pcbSelectionMenu(sel({ footprints: 1 }));
  assert.deepEqual(outline(nodes), [
    "Select >",
    "-",
    A.selectAll,
    A.unselectAll,
    "-",
    A.move,
    A.drag45,
    A.flip,
    "-",
    A.editFpInFpEditor,
    A.updateFootprint,
    A.changeFootprint,
    "-",
    "Routing >",
    "Position >",
    "Locking >",
    "Create from Selection >",
    "Grouping >",
    "-",
    A.cut,
    A.copy,
    A.copyWithReference,
    A.paste,
    A.pasteSpecial,
    A.duplicate,
    A.doDelete,
    "-",
    "Zoom >",
    "Grid >",
    "-",
    A.properties,
  ]);
});

test("nothing selected: only what applies -- no Rotate, Flip, Delete, Align or Properties; Get and Move Footprint, Paste, Zoom and Grid", () => {
  const nodes = pcbSelectionMenu(sel({}));
  assert.deepEqual(outline(nodes), [A.selectAll, A.unselectAll, "-", A.getAndPlace, "-", A.paste, A.pasteSpecial, "-", "Zoom >", "Grid >"]);
  for (const gone of [A.rotateCcw, A.flip, A.doDelete, A.properties, A.cut, A.copy, A.alignLeft, A.group]) assert.ok(!menuActions(nodes).includes(gone), gone);
});

test("a board with nothing on it has no Select All / Unselect All", () => {
  assert.ok(!actions(sel({}), ctx({ boardHasItems: false })).includes(A.selectAll));
});

test("a menu never starts or ends with a separator, nor holds two in a row", () => {
  for (const s of [sel({}), sel({ footprints: 1 }), sel({ tracks: 2 }), sel({ groups: 1 }), sel({ zones: 1, copperZones: 1 })]) {
    for (const c of [ctx(), ctx({ toolActive: true }), ctx({ haveHighlight: true })]) {
      const o = outline(pcbSelectionMenu(s, c));
      assert.notEqual(o[0], "-");
      assert.notEqual(o[o.length - 1], "-");
      assert.ok(!o.some((x, i) => x === "-" && o[i + 1] === "-"), o.join(" "));
    }
  }
});

test("ConditionalMenu: an entry with no order goes after the entries added so far; explicit orders sort, ties keep the order added", () => {
  const m = new ConditionalMenu();
  const yes = () => true;
  m.addItem("late", yes, 100);
  m.addItem("first", yes); // ANY_ORDER: order 1 (one entry so far)
  m.addItem("second", yes); // order 2
  m.addItem("early", yes, 0);
  m.addItem("tie", yes, 100);
  assert.deepEqual(menuActions(m.evaluate(emptyPcbSummary(), defaultMenuContext())), ["early", "first", "second", "late", "tie"]);
});

// ------------------------------------------------------------------------------------------------------------------------------ conditions

test("Rotate and Mirror are offered under Mirror / Rotate for what can be mirrored, not for footprints alone or pads alone", () => {
  assert.equal(submenu(pcbSelectionMenu(sel({ footprints: 2 })), "Mirror / Rotate"), null, "a footprint turns with R and flips with F; MirrorableItems has no footprint");
  assert.equal(submenu(pcbSelectionMenu(sel({ pads: 1 })), "Mirror / Rotate"), null, "pads alone cannot be mirrored in the board editor");
  for (const s of [sel({ tracks: 1 }), sel({ vias: 1 }), sel({ zones: 1 }), sel({ texts: 1 }), sel({ shapes: { rect: 1 } }), sel({ groups: 1 }), sel({ footprints: 1, tracks: 1 }), sel({ pads: 1, texts: 1 })]) {
    assert.deepEqual(menuActions(submenu(pcbSelectionMenu(s), "Mirror / Rotate")!), [A.rotateCcw, A.rotateCw, A.mirrorH, A.mirrorV]);
  }
});

test("Move Individually needs more than one item, and the Position submenu is gone while the selection is being moved", () => {
  assert.ok(!menuActions(submenu(pcbSelectionMenu(sel({ footprints: 1 })), "Position")!).includes(A.moveIndividually));
  assert.ok(menuActions(submenu(pcbSelectionMenu(sel({ footprints: 2 })), "Position")!).includes(A.moveIndividually));
  assert.equal(submenu(pcbSelectionMenu(sel({ footprints: 1 }), ctx({ moving: true })), "Position"), null);
  assert.ok(!actions(sel({ footprints: 1 }), ctx({ moving: true, toolActive: true })).includes(A.copyWithReference), "Copy with Reference needs a selection that is not moving");
  assert.ok(!actions(sel({ footprints: 1 }), ctx({ moving: true, toolActive: true })).includes(A.move), "and Move is not offered while moving");
  assert.ok(!actions(sel({ footprints: 1 }), ctx({ moving: true, toolActive: true })).includes(A.paste), "Paste only when no tool is active");
});

test("Routing needs a footprint, pad, track or via; Break Track and Fillet Tracks need tracks; Assign Netclass tracks, vias, pads or zones", () => {
  assert.notEqual(submenu(pcbSelectionMenu(sel({ tracks: 1 })), "Routing"), null);
  assert.notEqual(submenu(pcbSelectionMenu(sel({ pads: 1 })), "Routing"), null);
  assert.equal(submenu(pcbSelectionMenu(sel({ texts: 1 })), "Routing"), null);
  assert.equal(submenu(pcbSelectionMenu(sel({ shapes: { segment: 1 } })), "Routing"), null);
  const track = actions(sel({ tracks: 1 }));
  for (const a of [A.breakTrack, A.filletTracks, A.assignNetclass, A.drag45, A.dragFree]) assert.ok(track.includes(a), a);
  const two = actions(sel({ tracks: 2 }));
  assert.ok(!two.includes(A.breakTrack), "Break Track is for one track");
  assert.ok(two.includes(A.filletTracks) && two.includes(A.swap));
  const footprint = actions(sel({ footprints: 1 }));
  assert.ok(!footprint.includes(A.assignNetclass), "a footprint is none of the connected types");
  assert.ok(!footprint.includes(A.dragFree), "Drag Free Angle is not for a footprint");
  assert.ok(actions(sel({ zones: 1 })).includes(A.assignNetclass));
  assert.ok(!actions(sel({ tracks: 1, texts: 1 })).includes(A.assignNetclass));
});

test("Flip, Move, Cut, Copy, Duplicate and Delete need a selection; Swap and Pack and Move need more than one; Clearance Resolution exactly two", () => {
  const one = actions(sel({ texts: 1 }));
  for (const a of [A.flip, A.move, A.cut, A.copy, A.duplicate, A.doDelete, A.copyWithReference]) assert.ok(one.includes(a), a);
  assert.ok(!one.includes(A.swap) && !one.includes(A.packAndMove) && !one.includes(A.inspectClearance));
  const two = actions(sel({ footprints: 2 }));
  assert.ok(two.includes(A.swap) && two.includes(A.packAndMove) && two.includes(A.inspectClearance));
  assert.ok(!actions(sel({ footprints: 3 })).includes(A.inspectClearance));
  assert.ok(!actions(sel({ texts: 2 })).includes(A.packAndMove), "Pack and Move needs a footprint among them");
  assert.ok(actions(sel({ pads: 2 })).includes(A.swapPadNets));
  assert.ok(!actions(sel({ pads: 1, footprints: 1 })).includes(A.swapPadNets));
});

test("the footprint block: Open, Update, Change for one footprint; the plural forms for several", () => {
  assert.deepEqual(
    actions(sel({ footprints: 1 })).filter((a) => /EditFpInFpEditor|GlobalEdit/.test(a)),
    [A.editFpInFpEditor, A.updateFootprint, A.changeFootprint]
  );
  assert.deepEqual(
    actions(sel({ footprints: 2 })).filter((a) => /EditFpInFpEditor|GlobalEdit/.test(a)),
    [A.updateFootprints, A.changeFootprints]
  );
  assert.deepEqual(actions(sel({ footprints: 1, tracks: 1 })).filter((a) => /EditFpInFpEditor|GlobalEdit/.test(a)), [], "a footprint among other things gets none of them");
});

test("Properties: one item of any kind, or several tracks and vias; never several footprints, and never nothing", () => {
  assert.ok(actions(sel({ footprints: 1 })).includes(A.properties));
  assert.ok(actions(sel({ groups: 1 })).includes(A.properties));
  assert.ok(actions(sel({ tracks: 2, vias: 1 })).includes(A.properties));
  assert.ok(actions(sel({ arcTracks: 1, tracks: 1 })).includes(A.properties));
  assert.ok(!actions(sel({ footprints: 2 })).includes(A.properties));
  assert.ok(!actions(sel({ tracks: 1, texts: 1 })).includes(A.properties));
  assert.ok(!actions(sel({})).includes(A.properties));
});

test("Copy as Text only for text and dimensions", () => {
  assert.ok(actions(sel({ texts: 1, dimensions: 1 })).includes(A.copyAsText));
  assert.ok(!actions(sel({ texts: 1, tracks: 1 })).includes(A.copyAsText));
  assert.ok(!actions(sel({ footprints: 1 })).includes(A.copyAsText));
});

test("Cancel tops the menu while a tool runs, and Paste goes", () => {
  const nodes = pcbSelectionMenu(sel({ footprints: 1 }), ctx({ toolActive: true }));
  assert.deepEqual(outline(nodes).slice(0, 3), ["Select >", A.cancel, "-"]);
  assert.ok(!menuActions(nodes).includes(A.paste) && !menuActions(nodes).includes(A.pasteSpecial));
  assert.ok(!actions(sel({ footprints: 1 })).includes(A.cancel));
});

test("Skip appears while Move Individually runs", () => {
  assert.ok(actions(sel({ footprints: 1 }), ctx({ movingIndividually: true, toolActive: true })).includes(A.skip));
  assert.ok(!actions(sel({ footprints: 2 })).includes(A.skip));
});

test("Clear Net Highlighting leads the menu when a net is highlighted, with a line after it", () => {
  const o = outline(pcbSelectionMenu(sel({ tracks: 1 }), ctx({ haveHighlight: true })));
  assert.deepEqual(o.slice(0, 3), ["Select >", A.clearHighlight, "-"]);
  assert.ok(!outline(pcbSelectionMenu(sel({ tracks: 1 }))).includes(A.clearHighlight), "and not otherwise (the Net Inspection Tools submenu has its own)");
});

// ---------------------------------------------------------------------------------------------------------------------------------- submenus

test("Select: always the same entries; Select Net, Items in Same Sheet, Select on Schematic and Unconnected follow their action's own enable condition", () => {
  const entries = (s: PcbSelectionSummary) => submenu(pcbSelectionMenu(s), "Select")!;
  assert.deepEqual(menuActions(entries(sel({ footprints: 1 }))), [A.filterSelection, A.selectConnection, A.selectNet, A.selectSameSheet, A.selectOnSchematic, A.selectUnconnected, A.grabUnconnected]);
  assert.deepEqual(disabledIn(entries(sel({ footprints: 1 }))), [A.selectNet]);
  assert.deepEqual(disabledIn(entries(sel({ tracks: 1 }))), [A.selectSameSheet, A.selectOnSchematic, A.selectUnconnected]);
  assert.deepEqual(disabledIn(entries(sel({ texts: 1 }))), [A.selectNet, A.selectSameSheet, A.selectOnSchematic, A.selectUnconnected]);
  assert.equal(submenu(pcbSelectionMenu(sel({})), "Select"), null, "nothing selected: no Select menu");
});

test("Locking: Lock when something is unlocked, Unlock when something is locked, Toggle always", () => {
  const lock = (s: PcbSelectionSummary) => menuActions(submenu(pcbSelectionMenu(s), "Locking")!);
  assert.deepEqual(lock(sel({ tracks: 2 })), [A.lock, A.toggleLock]);
  assert.deepEqual(lock(sel({ tracks: 2, locked: 2, unlocked: 0 })), [A.unlock, A.toggleLock]);
  assert.deepEqual(lock(sel({ tracks: 2, locked: 1, unlocked: 1 })), [A.lock, A.unlock, A.toggleLock]);
});

test("Grouping: Group needs two items, Ungroup a group, Add Items one group and an ungrouped item, Remove Items a member", () => {
  const grouping = (s: PcbSelectionSummary) => submenu(pcbSelectionMenu(s), "Grouping")!;
  const g = (over: Partial<PcbSelectionSummary["group"]>) => ({ hasGroup: false, onlyOneGroup: false, hasUngroupedItems: false, hasMember: false, ...over });
  assert.deepEqual(menuActions(grouping(sel({ footprints: 1 }))), [A.group, A.ungroup, A.addToGroup, A.removeFromGroup]);
  assert.deepEqual(disabledIn(grouping(sel({ footprints: 1, group: g({ hasUngroupedItems: true }) }))), [A.group, A.ungroup, A.addToGroup, A.removeFromGroup]);
  assert.deepEqual(disabledIn(grouping(sel({ footprints: 2, group: g({ hasUngroupedItems: true }) }))), [A.ungroup, A.addToGroup, A.removeFromGroup]);
  assert.deepEqual(disabledIn(grouping(sel({ groups: 1, footprints: 1, group: g({ hasGroup: true, onlyOneGroup: true, hasUngroupedItems: true }) }))), [A.removeFromGroup], "one group and a loose item: everything but Remove");
  assert.deepEqual(disabledIn(grouping(sel({ groups: 2, group: g({ hasGroup: true, onlyOneGroup: false }) }))), [A.addToGroup, A.removeFromGroup], "two groups cannot be added to each other with Add Items");
  assert.deepEqual(disabledIn(grouping(sel({ footprints: 2, group: g({ hasMember: true }) }))), [A.ungroup, A.addToGroup]);
});

test("Enter Group and the design block entries for one group; Leave Group while inside one", () => {
  const one = actions(sel({ groups: 1 }));
  for (const a of [A.groupEnter, A.applyDesignBlockLayout, A.placeLinkedDesignBlock, A.saveToLinkedDesignBlock]) assert.ok(one.includes(a), a);
  assert.ok(!actions(sel({ groups: 2 })).includes(A.groupEnter), "one group at a time");
  assert.ok(!actions(sel({ groups: 1, footprints: 1 })).includes(A.groupEnter), "a group among others is two items: Count( 1 ) fails");
  assert.ok(actions(sel({ footprints: 2 }), ctx({ inGroup: true })).includes(A.groupLeave));
  assert.ok(!actions(sel({ footprints: 2 })).includes(A.groupLeave));
  // Leave Group, like Cancel, stays among the first entries
  assert.ok(menuActions(pcbSelectionMenu(sel({ footprints: 2 }), ctx({ inGroup: true }))).indexOf(A.groupLeave) < menuActions(pcbSelectionMenu(sel({ footprints: 2 }), ctx({ inGroup: true }))).indexOf(A.selectAll));
});

test("Align/Distribute: align for two or more, distribute for three or more", () => {
  const align = (n: number) => menuActions(submenu(pcbSelectionMenu(sel({ footprints: n })), "Align/Distribute") ?? []);
  assert.deepEqual(align(1), []);
  assert.equal(submenu(pcbSelectionMenu(sel({ footprints: 1 })), "Align/Distribute"), null);
  assert.deepEqual(align(2), [A.alignLeft, A.alignCenterX, A.alignRight, A.alignTop, A.alignCenterY, A.alignBottom]);
  assert.deepEqual(align(3).slice(6), [A.distributeHCenters, A.distributeHGaps, A.distributeVCenters, A.distributeVGaps]);
  // the lines between the groups of entries
  const two = submenu(pcbSelectionMenu(sel({ footprints: 2 })), "Align/Distribute")!;
  assert.deepEqual(outline(two), [A.alignLeft, A.alignCenterX, A.alignRight, "-", A.alignTop, A.alignCenterY, A.alignBottom]);
});

test("Zones: for a selection of zones only, with Merge for several, the single-zone entries for one, and Zone Priority by what overlaps", () => {
  const zones = (s: PcbSelectionSummary) => submenu(pcbSelectionMenu(s), "Zones");
  assert.equal(zones(sel({ zones: 1, tracks: 1 })), null);
  assert.equal(zones(sel({ footprints: 1 })), null);
  const one = zones(sel({ zones: 1, copperZones: 1, zoneCanRaise: true }))!;
  assert.deepEqual(disabledIn(one), [A.zoneMerge], "Merge needs two zones; the others take one");
  const priority = one.find((n) => n.type === "submenu")!;
  assert.ok(priority.type === "submenu");
  assert.deepEqual(menuActions(priority.items), [A.zonePriorityTop, A.zonePriorityRaise, A.zonePriorityLower, A.zonePriorityBottom]);
  assert.deepEqual(disabledIn(priority.items), [A.zonePriorityLower, A.zonePriorityBottom]);
  const two = zones(sel({ zones: 2 }))!;
  assert.deepEqual(disabledIn(two), [A.zoneDuplicate, A.zoneCutout, A.similarZone]);
  assert.deepEqual(menuActions(two).slice(0, 4), [A.zoneFill, A.zoneFillAll, A.zoneUnfill, A.zoneUnfillAll]);
});

test("Net Inspection Tools: only when every item is connectable (track, via, pad, zone, copper shape) -- not for a footprint, text or a plain graphic", () => {
  const net = (s: PcbSelectionSummary) => submenu(pcbSelectionMenu(s), "Net Inspection Tools");
  for (const s of [sel({ tracks: 1 }), sel({ vias: 2 }), sel({ pads: 1 }), sel({ zones: 1 }), sel({ arcTracks: 1, tracks: 1, vias: 1 }), sel({ shapes: { segment: 1 }, copperShapes: 1 })]) assert.notEqual(net(s), null);
  for (const s of [sel({ footprints: 1 }), sel({ tracks: 1, texts: 1 }), sel({ shapes: { segment: 1 } }), sel({ groups: 1 }), sel({})]) assert.equal(net(s), null);
  assert.deepEqual(disabledIn(net(sel({ tracks: 1, hasNet: true }))!), [], "a net on the track: Show and Hide are enabled");
  assert.deepEqual(disabledIn(net(sel({ tracks: 1 }))!), [A.showNet, A.hideNet], "no net: Show and Hide Net in Ratsnest are not");
});

test("Create from Selection: always has Create Array for a selection, and the others by what Convert could make", () => {
  const convert = (s: PcbSelectionSummary) => submenu(pcbSelectionMenu(s), "Create from Selection");
  assert.deepEqual(menuActions(convert(sel({ footprints: 1 }))!), [A.createArray]);
  const rect = sel({ shapes: { rect: 1 }, convert: { poly: true, zone: true, keepout: true, lines: true, outset: true, tracks: true, arc: false } });
  assert.deepEqual(outline(convert(rect)!), [A.convertToPoly, A.convertToZone, A.convertToKeepout, A.convertToLines, A.outsetItems, "-", A.convertToTracks, "-", A.createArray]);
  assert.equal(convert(sel({})), null);
});

test("Shape Modification: fillet for polygons, rectangles and segments; Extend for two segments; booleans for several polygons; Heal for arcs and curves", () => {
  const shape = (s: PcbSelectionSummary, c?: PcbMenuContext) => submenu(pcbSelectionMenu(s, c), "Shape Modification");
  assert.equal(shape(sel({ footprints: 1 })), null);
  assert.equal(shape(sel({ tracks: 1 })), null);
  assert.deepEqual(menuActions(shape(sel({ shapes: { rect: 1 } }))!), [A.filletLines, A.chamferLines, A.dogbone]);
  assert.deepEqual(menuActions(shape(sel({ shapes: { segment: 2 } }))!), [A.healShapes, A.filletLines, A.chamferLines, A.dogbone, A.extendLines]);
  assert.deepEqual(menuActions(shape(sel({ shapes: { segment: 1 } }))!), [A.healShapes, A.filletLines, A.chamferLines, A.dogbone]);
  assert.deepEqual(menuActions(shape(sel({ shapes: { rect: 1, circle: 1 } }))!), [A.mergePolygons, A.subtractPolygons, A.intersectPolygons]);
  assert.deepEqual(menuActions(shape(sel({ shapes: { polygon: 2 } }))!), [A.simplifyPolygons, A.filletLines, A.chamferLines, A.dogbone, A.mergePolygons, A.subtractPolygons, A.intersectPolygons]);
});

test("Shape Modification: the corner entries follow the handle under the pointer and the one selected item", () => {
  const poly = sel({ shapes: { polygon: 1 }, editableCorners: true, canAddCorner: true, canChamferCorner: true });
  const base = menuActions(submenu(pcbSelectionMenu(poly), "Shape Modification")!);
  assert.ok(base.includes(A.addCorner) && base.includes(A.chamferCorner) && base.includes(A.editVertices));
  assert.ok(!base.includes(A.moveCorner) && !base.includes(A.removeCorner));
  const onCorner = menuActions(submenu(pcbSelectionMenu(poly, ctx({ handle: { corner: true, midpoint: false, canRemoveCorner: true } })), "Shape Modification")!);
  assert.ok(onCorner.includes(A.moveCorner) && onCorner.includes(A.removeCorner) && !onCorner.includes(A.moveMidpoint));
  const onMid = menuActions(submenu(pcbSelectionMenu(poly, ctx({ handle: { corner: false, midpoint: true, canRemoveCorner: false } })), "Shape Modification")!);
  assert.ok(onMid.includes(A.moveMidpoint) && !onMid.includes(A.removeCorner));
});

test("Heal Shapes for segments, arcs and curves; Simplify Polygons for polygons and zones", () => {
  const shape = (s: PcbSelectionSummary) => menuActions(submenu(pcbSelectionMenu(s), "Shape Modification") ?? []);
  assert.ok(shape(sel({ shapes: { arc: 1, segment: 1 } })).includes(A.healShapes));
  assert.ok(shape(sel({ shapes: { bezier: 1 } })).includes(A.healShapes));
  assert.ok(shape(sel({ zones: 1, editableCorners: true, canAddCorner: true })).includes(A.simplifyPolygons));
});

test("Cycle Arc Editing Mode for one arc; the pad settings for pads", () => {
  assert.ok(actions(sel({ shapes: { arc: 1 } })).includes(A.cycleArcEditMode));
  assert.ok(!actions(sel({ shapes: { arc: 2 } })).includes(A.cycleArcEditMode));
  assert.ok(!actions(sel({ shapes: { segment: 1 } })).includes(A.cycleArcEditMode));
  const one = actions(sel({ pads: 1 }));
  assert.ok(one.includes(A.copyPadSettings) && one.includes(A.applyPadSettings) && one.includes(A.pushPadSettings));
  const two = actions(sel({ pads: 2 }));
  assert.ok(!two.includes(A.copyPadSettings) && two.includes(A.applyPadSettings) && !two.includes(A.pushPadSettings));
  assert.ok(!actions(sel({ footprints: 1 })).includes(A.applyPadSettings));
});

test("the pad settings sit after the clipboard block and before Zoom and Grid", () => {
  const o = outline(pcbSelectionMenu(sel({ pads: 1 })));
  const at = (x: string) => o.indexOf(x);
  assert.ok(at(A.doDelete) < at(A.copyPadSettings) && at(A.pushPadSettings) < at("Zoom >"));
});

// -------------------------------------------------------------------------------------------------------------------------------- zoom and grid

test("Zoom lists the presets (entry i is preset i + 1) and Grid Origin, a line and the grids (entry i is grid i), the current ones checked", () => {
  const c = ctx({ zoom: [{ label: "Zoom: 0.50", checked: false }, { label: "Zoom: 1.00", checked: true }], grid: [{ label: "1.000 mm (39.370 mils)", checked: true }, { label: "0.500 mm (19.685 mils)", checked: false }] });
  const nodes = pcbSelectionMenu(sel({}), c);
  const zoom = submenu(nodes, "Zoom")!;
  assert.deepEqual(zoom, [
    { type: "item", action: A.zoomPreset, label: "Zoom: 0.50", arg: 1, checked: false },
    { type: "item", action: A.zoomPreset, label: "Zoom: 1.00", arg: 2, checked: true },
  ]);
  const grid = submenu(nodes, "Grid")!;
  assert.deepEqual(outline(grid), [A.editGridOrigin, "-", A.gridPreset, A.gridPreset]);
  assert.deepEqual(grid.slice(2), [
    { type: "item", action: A.gridPreset, label: "1.000 mm (39.370 mils)", arg: 0, checked: true },
    { type: "item", action: A.gridPreset, label: "0.500 mm (19.685 mils)", arg: 1, checked: false },
  ]);
});

// ------------------------------------------------------------------------------------------------------------------------------ the router's menu

const router = (over: Partial<RouterMenuContext> = {}): RouterMenuContext => ({
  routing: false,
  inRouteSelected: false,
  hasOtherEnd: false,
  hasSelection: false,
  haveHighlight: false,
  diffPair: false,
  cornerMode: "45",
  trackViaMenu: [{ type: "item", action: "studio.Router.useNetclassSizes", label: "Use Net Class Values", checked: true }],
  diffPairMenu: [],
  zoom: [],
  grid: [],
  ...over,
});

test("the router's menu: Cancel first, the tool and route commands, vias and posture, Track Corner Mode, the width menu and the settings, then Zoom and Grid", () => {
  const o = outline(routerMenu(router()));
  assert.deepEqual(o, [
    A.cancel,
    "-",
    "pcbnew.InteractiveRouter.SingleTrack",
    "pcbnew.InteractiveRouter.DiffPair",
    "common.Interactive.finish",
    "pcbnew.InteractiveRouter.UndoLastSegment",
    A.breakTrack,
    A.drag45,
    A.dragFree,
    "pcbnew.InteractiveRouter.PlaceVia",
    "pcbnew.InteractiveRouter.PlaceBlindVia",
    "pcbnew.InteractiveRouter.PlaceMicroVia",
    "pcbnew.InteractiveRouter.SelLayerAndPlaceVia",
    "pcbnew.InteractiveRouter.SelLayerAndPlaceBlindVia",
    "pcbnew.InteractiveRouter.SelLayerAndPlaceMicroVia",
    "pcbnew.InteractiveRouter.SwitchPosture",
    "Track Corner Mode >",
    "-",
    "Select Track/Via Width >",
    "pcbnew.InteractiveRouter.SettingsDialog",
    "-",
    "Zoom >",
    "Grid >",
  ]);
});

test("while routing: no new tool, no Break Track or Drag; Route From Other End and Attempt Finish need something left to connect to", () => {
  const routing = menuActions(routerMenu(router({ routing: true })));
  for (const gone of ["pcbnew.InteractiveRouter.SingleTrack", "pcbnew.InteractiveRouter.DiffPair", A.breakTrack, A.drag45, A.dragFree, A.autoroute]) assert.ok(!routing.includes(gone), gone);
  assert.ok(routing.includes("common.Interactive.finish") && routing.includes("pcbnew.InteractiveRouter.UndoLastSegment"));
  assert.ok(!routing.includes("pcbnew.InteractiveRouter.ContinueFromEnd") && !routing.includes("pcbnew.InteractiveRouter.AttemptFinish"));
  const more = menuActions(routerMenu(router({ routing: true, hasOtherEnd: true })));
  assert.ok(more.includes("pcbnew.InteractiveRouter.ContinueFromEnd") && more.includes("pcbnew.InteractiveRouter.AttemptFinish"));
});

test("the router's menu: Cancel Current Item only inside Route Selected, Autoroute Selected needs a selection, the pair menu only for a pair", () => {
  assert.ok(!menuActions(routerMenu(router())).includes("pcbnew.InteractiveRouter.CancelCurrentItem"));
  assert.equal(menuActions(routerMenu(router({ inRouteSelected: true })))[1], "pcbnew.InteractiveRouter.CancelCurrentItem");
  assert.ok(!menuActions(routerMenu(router())).includes(A.autoroute));
  assert.ok(menuActions(routerMenu(router({ hasSelection: true }))).includes(A.autoroute));
  assert.equal(submenu(routerMenu(router()), "Select Differential Pair Dimensions"), null);
  assert.notEqual(submenu(routerMenu(router({ diffPair: true })), "Select Differential Pair Dimensions"), null);
  assert.deepEqual(outline(routerMenu(router({ haveHighlight: true }))).slice(0, 4), [A.cancel, "-", A.clearHighlight, "-"]);
});

test("Track Corner Mode: Switch, a line, then the four modes with the router's current one checked", () => {
  const corner = submenu(routerMenu(router({ cornerMode: "arc90" })), "Track Corner Mode")!;
  assert.deepEqual(outline(corner), ["pcbnew.InteractiveRouter.SwitchRoundingToNext", "-", "pcbnew.InteractiveRouter.SwitchRounding45", "pcbnew.InteractiveRouter.SwitchRoundingArc45", "pcbnew.InteractiveRouter.SwitchRounding90", "pcbnew.InteractiveRouter.SwitchRoundingArc90"]);
  assert.deepEqual(
    corner.filter((n) => n.type === "item" && n.checked).map((n) => (n.type === "item" ? n.label : "")),
    ["Track Corner Mode Arc 90"]
  );
  assert.equal(corner.find((n) => n.type === "item" && n.checked === undefined && n.action.endsWith("ToNext"))?.type, "item");
});

test("the router's own actions, which actions.json does not have, carry the label KiCad gives them", () => {
  const via = routerMenu(router()).find((n) => n.type === "item" && n.action === "pcbnew.InteractiveRouter.PlaceVia");
  assert.equal(via && via.type === "item" ? via.label : null, "Place Through Via");
});

// ------------------------------------------------------------------------------------------------------------------- the drawing tools' menu

test("a drawing tool's menu: Cancel, what the tool offers (Close Outline for a polygon or zone, Delete Last Point where points are placed, arc posture, arrows), Zoom and Grid", () => {
  const c = ctx({ toolActive: true });
  assert.deepEqual(outline(drawingMenu("line", c)), [A.cancel, "-", "pcbnew.InteractiveDrawing.deleteLastPoint", "-", "Zoom >", "Grid >"]);
  assert.deepEqual(menuActions(drawingMenu("polygon", c)).slice(0, 3), [A.cancel, "pcbnew.InteractiveDrawing.closeOutline", "pcbnew.InteractiveDrawing.deleteLastPoint"]);
  assert.ok(menuActions(drawingMenu("arc", c)).includes("pcbnew.InteractiveDrawing.arcPosture"));
  assert.ok(!menuActions(drawingMenu("rectangle", c)).includes("pcbnew.InteractiveDrawing.deleteLastPoint"));
  assert.ok(menuActions(drawingMenu("dimension", c)).includes("pcbnew.InteractiveDrawing.changeDimensionArrows"));
  assert.notEqual(submenu(drawingMenu("zone", c), "Zones"), null, "the zone tool carries the zone menu");
  assert.equal(submenu(drawingMenu("line", c), "Zones"), null);
  assert.deepEqual(menuActions(drawingMenu("zone", ctx({ haveHighlight: true }))).slice(0, 2), [A.cancel, A.clearHighlight]);
});

test("a picking tool's menu is Cancel, Zoom and Grid", () => {
  assert.deepEqual(outline(pickerMenu(ctx())), [A.cancel, "-", "Zoom >", "Grid >"]);
});
