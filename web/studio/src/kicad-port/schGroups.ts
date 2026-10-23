// Groups on a schematic sheet: which items can be grouped, which group a click on an item selects, what a box around items selects, and when each entry of the
// Grouping menu is on.
//
//   eeschema/sch_group.cpp                        SCH_GROUP::TopLevelGroup / WithinScope / getClosestGroup: "items with either symbols or sheets as their parents (e.g.,
//                                                 fields, pins, sheet pins, etc.) don't get their own groups and instead follow the group of their parent object"
//   eeschema/sch_item.cpp                         SCH_ITEM::IsGroupableType
//   eeschema/tools/sch_group_tool.cpp             SCH_GROUP_TOOL::Group: the selection without what belongs to a symbol and what cannot be grouped, at least two
//   eeschema/tools/sch_selection_tool.cpp         filterCollectorForHierarchy, SelectMultiple (window and crossing), EnterGroup / ExitGroup
//   common/tool/group_tool.cpp                    GROUP_CONTEXT_MENU::update: when Group, Ungroup, Add Items and Remove Items are enabled
//
// The tree (`parentGroup`, `topLevelGroup`, `withinScope`, `groupLeaves`) is kicad-port/groupTree.ts, the board's too; this adds what only the sheet has: the
// owner of a pin, a field or a sheet pin, the kinds that may be members, and the boxes of the items below a group. Pure: no store, no DOM.
import type { Group, Schematic } from "../api/types";
import { boxEncloses, inflateBox, unionBox, type Box } from "./schItemGeom";
import { groupLeaves, isGroup, parentGroup, substituteSelection, topLevelGroup, withinScope } from "./groupTree";
import { parseFieldId } from "./schFieldEdit";

/** The box of an item of the sheet by id (components/schematic/schGroupBounds.ts has the one that measures the sheet), or null for an id that is none. */
export type BoxOf = (id: string) => Box | null;

/**
 * Every id of the sheet that `SCH_ITEM::IsGroupableType` allows, less what belongs to a symbol or a sheet (see [`ownerOf`]): a symbol, a power symbol, a wire or bus, a label, a text,
 * a no-connect, a bus entry, a junction, a graphic line, a sheet and a drawn shape, text box, rule area or directive label.
 */
export function groupableIds(sch: Schematic): Set<string> {
  const out = new Set<string>();
  const add = (items: ReadonlyArray<{ id?: string }> | undefined) => {
    for (const i of items ?? []) if (i.id) out.add(i.id);
  };
  add(sch.symbols);
  add(sch.power_symbols);
  add(sch.wires);
  add(sch.labels);
  add(sch.texts);
  add(sch.no_connects);
  add(sch.bus_entries);
  add(sch.junctions);
  add(sch.lines);
  add(sch.sheets);
  add(sch.graphics);
  return out;
}

/** The groups of the sheet in view. */
export function groupsOf(sch: Schematic | null | undefined): readonly Group[] {
  return sch?.groups ?? [];
}

/**
 * The id of the item whose group `id` follows: a pin or a field goes with its symbol (a power symbol, a sheet), a sheet pin with its sheet; everything else is
 * its own (`getClosestGroup`).
 */
export function ownerOf(sch: Schematic, id: string): string {
  if (id.startsWith("pin:")) {
    const key = id.slice("pin:".length).split(":")[0] ?? id;
    return key.replace(/#\d+$/, "");
  }
  const field = parseFieldId(id);
  if (field) return field.owner.replace(/#\d+$/, "");
  const sheet = sch.sheets.find((s) => (s.pins ?? []).some((p) => p.id === id));
  return sheet ? sheet.id : id;
}

/**
 * What a click on (or a box around) `ids` selects, with `entered` the group being worked in: an item in a group is replaced by the outermost group that holds it
 * inside `entered` (`filterCollectorForHierarchy`). A pin, a field and a sheet pin take the group of their symbol or sheet. Anything outside `entered` is not
 * selectable there; selecting one anyway leaves the group (`SCH_SELECTION_TOOL::select`: `ExitGroup()`), reported in `exits`.
 */
export function substituteSchSelection(sch: Schematic | null | undefined, ids: readonly string[], entered: string | null): { ids: string[]; exits: boolean } {
  const groups = groupsOf(sch);
  if (!sch || groups.length === 0) return { ids: [...ids], exits: false };
  const exits = entered !== null && ids.some((id) => !withinScope(groups, ownerOf(sch, id), entered));
  const scope = exits ? null : entered;
  const out: string[] = [];
  for (const id of ids) {
    const top = topLevelGroup(groups, ownerOf(sch, id), scope);
    const picked = top ?? id;
    if (!out.includes(picked)) out.push(picked);
  }
  return { ids: out, exits };
}

/** Whether the item `id` can be picked while `entered` is the group worked in (`WithinScope`; everything can when none is). */
export function inEnteredScope(sch: Schematic, id: string, entered: string | null): boolean {
  return withinScope(groupsOf(sch), ownerOf(sch, id), entered);
}

/**
 * What a window or a crossing box around the sheet selects (`SCH_SELECTION_TOOL::SelectMultiple`): `hits` are the items the box found. A window (`crossing` false)
 * takes a group only when the group's whole box is inside it, with everything below it, and never a member on its own; a crossing box takes any member, and either way
 * a member becomes its outermost group inside `entered` (`filterCollectorForHierarchy`), and what is outside `entered` is left out. `pick` is the selection filter.
 */
export function boxSelectionWithGroups(sch: Schematic, hits: readonly string[], box: Box, crossing: boolean, entered: string | null, boxOf: BoxOf, pick: (id: string) => boolean = () => true): string[] {
  const groups = groupsOf(sch);
  if (groups.length === 0) return [...hits];
  // the pins a box found are not hierarchy: they are taken as pins, unless their group is selected too (`SelectMultiple`: "If the pin lives inside a group that is already being selected,
  // don't also select the pin")
  const pins = hits.filter((id) => id.startsWith("pin:"));
  let found = hits.filter((id) => !id.startsWith("pin:"));
  if (!crossing) {
    // `group_items`: the direct members of every group but the entered one -- "( !containedMode || !group_items.count( item ) )": a member counts only through its group
    const members = new Set<string>();
    for (const g of groups) if (g.id !== entered) for (const m of g.member_ids) members.add(m);
    found = found.filter((id) => !members.has(ownerOf(sch, id)));
    // a group whose box the window holds brings its members (`containedMode && boxContained( group->GetBoundingBox() )`); the group in it comes with them
    for (const g of groups) {
      if (g.id === entered || g.member_ids.length === 0) continue;
      const gb = groupBox(groups, g.id, boxOf);
      if (!gb || !boxEncloses(box, gb)) continue;
      for (const m of g.member_ids) if ((isGroup(groups, m) || pick(m)) && !found.includes(m)) found.push(m);
    }
  }
  found = found.filter((id) => withinScope(groups, ownerOf(sch, id), entered));
  const out: string[] = [];
  for (const id of found) {
    const top = topLevelGroup(groups, ownerOf(sch, id), entered);
    const picked = top ?? id;
    if (!out.includes(picked)) out.push(picked);
  }
  for (const pin of pins) {
    const top = topLevelGroup(groups, ownerOf(sch, pin), entered);
    if (!(top && out.includes(top)) && withinScope(groups, ownerOf(sch, pin), entered)) out.push(pin);
  }
  return out;
}

/** The ids `ids` stand for with every group opened to the items below it (what a move, a delete or a copy acts on); each once. */
export function leafIds(sch: Schematic | null | undefined, ids: readonly string[]): string[] {
  const groups = groupsOf(sch);
  if (groups.length === 0) return [...ids];
  const out: string[] = [];
  for (const id of ids) for (const leaf of isGroup(groups, id) ? groupLeaves(groups, id) : [id]) if (!out.includes(leaf)) out.push(leaf);
  return out;
}

/** `SCH_GROUP::GetBoundingBox` before `bbox.Inflate( 10 mil )`: the union of the boxes of the items below the group. */
export function groupBox(groups: readonly Group[], id: string, boxOf: BoxOf): Box | null {
  let box: Box | null = null;
  for (const leaf of groupLeaves(groups, id)) {
    const b = boxOf(leaf);
    if (b) box = unionBox(box, b);
  }
  return box;
}

/** What the group is drawn around: its members' box grown by 10 mil (`bbox.Inflate( schIUScale.MilsToIU( 10 ) )`), micrometres. */
export const GROUP_INFLATE_NM = 254_000;

/** The box drawn for a selected or entered group (`SCH_PAINTER::draw( SCH_GROUP )`), or null when nothing is left in it. */
export function groupBoundsOf(groups: readonly Group[], id: string, boxOf: BoxOf): Box | null {
  const b = groupBox(groups, id, boxOf);
  return b ? inflateBox(b, GROUP_INFLATE_NM) : null;
}

/** The ids of `selection` that `SCH_GROUP_TOOL::Group` takes: groups and groupable items, not what belongs to a symbol or a sheet. Each once. */
export function groupableOf(sch: Schematic, selection: readonly string[]): string[] {
  const groups = groupsOf(sch);
  const items = groupableIds(sch);
  const out: string[] = [];
  for (const id of selection) {
    if ((isGroup(groups, id) || items.has(id)) && !out.includes(id)) out.push(id);
  }
  return out;
}

/** `SCH_GROUP_TOOL::Group`: the items the new group is made over, or null when fewer than two are left ("if( selection.GetSize() < 2 ) return 0"). */
export function planGroup(sch: Schematic, selection: readonly string[]): string[] | null {
  const ids = groupableOf(sch, selection);
  return ids.length >= 2 ? ids : null;
}

/** `GROUP_CONTEXT_MENU::update`: what the selection holds, for the four entries of Grouping. */
export function groupMenuFlags(sch: Schematic, selection: readonly string[]): { count: number; hasGroup: boolean; onlyOneGroup: boolean; hasUngroupedItems: boolean; hasMember: boolean } {
  const groups = groupsOf(sch);
  let hasGroup = false;
  let onlyOneGroup = false;
  let hasUngroupedItems = false;
  let hasMember = false;
  for (const id of selection) {
    if (isGroup(groups, id)) {
      // "Only allow one group to be selected for adding to existing group"
      if (hasGroup) onlyOneGroup = false;
      else {
        onlyOneGroup = true;
        hasGroup = true;
      }
    } else if (!parentGroup(groups, ownerOf(sch, id))) hasUngroupedItems = true;
    if (parentGroup(groups, ownerOf(sch, id))) hasMember = true;
  }
  return { count: selection.length, hasGroup, onlyOneGroup, hasUngroupedItems, hasMember };
}

/** `SCH_GROUP::GetItemDescription`. */
export function describeGroup(g: Group): string {
  return g.name ? `Group '${g.name}', ${g.member_ids.length} members` : `Unnamed Group, ${g.member_ids.length} members`;
}

/** The selection after `substituteSelection` for the editor that has no `Schematic` to hand (a plain list of groups): the board's version, re-exported for the tests. */
export { substituteSelection };
