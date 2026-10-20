// The boxes of the items of a sheet and of its groups -- the half of kicad-port/schGroups.ts that has to measure text (it takes the box of an id as a function; this is that function).
import type { Schematic } from "../../api/types";
import { groupBoundsOf, groupBox, groupsOf, type BoxOf } from "../../kicad-port/schGroups";
import { isGroup } from "../../kicad-port/groupTree";
import type { Box } from "../../kicad-port/schItemGeom";
import { allItems, itemBounds, type SchItemRef } from "./schItems";

/** The box of every item of the sheet by id, measured once; a group is the box of what is below it (`SCH_GROUP::GetBoundingBox`). */
export function sheetBoxOf(sch: Schematic): BoxOf {
  const refs = new Map<string, SchItemRef>(allItems(sch).map((r) => [r.id, r]));
  const cache = new Map<string, Box | null>();
  const of = (id: string): Box | null => {
    if (cache.has(id)) return cache.get(id)!;
    const ref = refs.get(id);
    const b = ref ? itemBounds(sch, ref) : null;
    cache.set(id, b);
    return b;
  };
  return (id) => (isGroup(groupsOf(sch), id) ? groupBox(groupsOf(sch), id, of) : of(id));
}

/** The box of an item or, for a group, of everything below it. */
export function boxOfId(sch: Schematic, id: string): Box | null {
  return sheetBoxOf(sch)(id);
}

/** The box a selected or entered group is drawn around: everything below it, grown by 10 mil. */
export function groupBounds(sch: Schematic, id: string, boxOf: BoxOf = sheetBoxOf(sch)): Box | null {
  return groupBoundsOf(groupsOf(sch), id, boxOf);
}
