// The group tree of a board: which group holds an item, which items are below a group, which group a click on an item selects, and
// whether an item can be picked while a group is entered. Groups nest (a group's member may be another group), as `EDA_GROUP`'s do.
//
//   common/eda_group.cpp          EDA_GROUP::AddItem / RemoveItem: an item is in one group at a time (`GetParentGroup`)
//   pcbnew/pcb_group.cpp          getClosestGroup, getNestedGroup, PCB_GROUP::TopLevelGroup, PCB_GROUP::WithinScope
//   pcbnew/tools/pcb_selection_tool.cpp   FilterCollectorForHierarchy: "If any element is a member of a group, replace those elements with
//                                          the top containing group"; "If a group is entered, disallow selections of objects outside the group"
//
// Pure: no store, no DOM. The Rust twin of the tree reads is `crates/model/src/groups.rs`. Unit-tested in groupTree.test.ts.

export interface GroupLike {
  id: string;
  member_ids: readonly string[];
}

/** `EDA_ITEM::GetParentGroup()`: the group that lists `id` as a member, if any. */
export function parentGroup(groups: readonly GroupLike[], id: string): GroupLike | undefined {
  return groups.find((g) => g.member_ids.includes(id));
}

export function isGroup(groups: readonly GroupLike[], id: string): boolean {
  return groups.some((g) => g.id === id);
}

/**
 * The items below the group `id`, nested groups opened (each member that is not itself a group, in member order, once): what a move, a turn, a flip,
 * a delete or a copy of the group acts on. Empty for an id that is no group. A loop in a hand-made file ends the walk.
 */
export function groupLeaves(groups: readonly GroupLike[], id: string): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  const walk = (gid: string) => {
    const g = groups.find((x) => x.id === gid);
    if (!g || seen.has(gid)) return;
    seen.add(gid);
    for (const m of g.member_ids) {
      if (groups.some((x) => x.id === m)) walk(m);
      else if (!out.includes(m)) out.push(m);
    }
  };
  walk(id);
  return out;
}

/** `ids` with every group replaced by the items below it (see [`groupLeaves`]); an id that is no group stays; each item once, in order. */
export function expandGroups(groups: readonly GroupLike[], ids: readonly string[]): string[] {
  const out: string[] = [];
  for (const id of ids) {
    for (const leaf of isGroup(groups, id) ? groupLeaves(groups, id) : [id]) if (!out.includes(leaf)) out.push(leaf);
  }
  return out;
}

/** Every group at or below the group `id` (the group itself first). */
export function groupAndDescendants(groups: readonly GroupLike[], id: string): string[] {
  const out: string[] = [];
  const walk = (gid: string) => {
    const g = groups.find((x) => x.id === gid);
    if (!g || out.includes(gid)) return;
    out.push(gid);
    for (const m of g.member_ids) walk(m);
  };
  walk(id);
  return out;
}

/** Whether the group `inner` is the group `outer` or lies below it. */
export function groupHolds(groups: readonly GroupLike[], outer: string, inner: string): boolean {
  return groupAndDescendants(groups, outer).includes(inner);
}

/** Every group above `id`, the nearest first. */
export function ancestors(groups: readonly GroupLike[], id: string): GroupLike[] {
  const out: GroupLike[] = [];
  let at = parentGroup(groups, id);
  while (at && !out.includes(at)) {
    out.push(at);
    at = parentGroup(groups, at.id);
  }
  return out;
}

/**
 * `getNestedGroup( aItem, aScope )`: the group, among those that hold `id`, that sits directly inside the group `scope` (`null`: no group is
 * entered, so the outermost one). `null` when `id` is in no group, or sits directly in `scope`. An item outside `scope` gets the outermost group
 * that holds it.
 */
export function topLevelGroup(groups: readonly GroupLike[], id: string, scope: string | null = null): string | null {
  const chain = ancestors(groups, id);
  if (chain.length === 0) return null;
  if (chain[0]!.id === scope) return null;
  let group = chain[0]!;
  for (let i = 1; i < chain.length; i++) {
    const above = chain[i]!;
    if (above.id === scope) break;
    group = above;
  }
  return group.id;
}

/**
 * `PCB_GROUP::WithinScope( aItem, aScope )`: whether `id` can be picked while the group `scope` is entered -- it is one of the group's own members, or
 * lies in a group that is. Everything is within the scope of no entered group (`scope == null`).
 */
export function withinScope(groups: readonly GroupLike[], id: string, scope: string | null): boolean {
  if (scope === null) return true;
  const chain = ancestors(groups, id);
  if (chain[0]?.id === scope) return true;
  const nested = topLevelGroup(groups, id, scope);
  if (nested === null) return false;
  return parentGroup(groups, nested)?.id === scope;
}

/**
 * What a click on (or a box around) `ids` selects, with `entered` the group being worked in (`FilterCollectorForHierarchy`): an item in a group is
 * replaced by the outermost group that holds it inside `entered`. Anything outside `entered` is not selectable there; selecting one anyway leaves the
 * group (`PCB_SELECTION_TOOL::select`: `ExitGroup()`), reported in `exits`, and the ids are then taken as with nothing entered.
 */
export function substituteSelection(groups: readonly GroupLike[], ids: readonly string[], entered: string | null): { ids: string[]; exits: boolean } {
  if (groups.length === 0) return { ids: [...ids], exits: false };
  const exits = entered !== null && ids.some((id) => !withinScope(groups, id, entered));
  const scope = exits ? null : entered;
  const out: string[] = [];
  for (const id of ids) {
    const top = topLevelGroup(groups, id, scope);
    const picked = top ?? id;
    if (!out.includes(picked)) out.push(picked);
  }
  return { ids: out, exits };
}
