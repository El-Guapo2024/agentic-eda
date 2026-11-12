// The sheets Sync All Sheet Pins offers -- `SCH_DRAWING_TOOLS::SyncAllSheetsPins` (eeschema/tools/sch_drawing_tools.cpp at 8303b2ad): every sheet path of the
// whole hierarchy, not only the sheets of the page in view. Its `getSheetChildren` walks from each top-level sheet (`GetTopLevelSheets()`: the root, or with
// KiCad 10's virtual root the project's top-level sheets, which are not listed themselves -- "don't include virtual root in paths") down through every
// screen, listing each child sheet's path before going into it, and goes into a screen only once (`aVisited`): a file several sheets show lists its own
// sheets under the first path that reaches it only. An empty list is "No sub schematic found in the current project".
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).

/** One sheet of the tree as `GET /api/sch/hierarchy` lists it (api/schControlClient.ts `HierarchyEntry`), in its depth-first order, the root (`[]`) first. */
export interface SyncTreeEntry {
  path: readonly string[];
  file: string;
}

/** The sheet paths `SyncAllSheetsPins` hands `DIALOG_SYNC_SHEET_PINS`, in its order; `virtualRoot`: the root is KiCad 10's virtual root. */
export function syncSheetPaths(tree: readonly SyncTreeEntry[], virtualRoot: boolean): string[][] {
  const key = (p: readonly string[]) => p.join("/");
  const children = new Map<string, SyncTreeEntry[]>();
  for (const e of tree) {
    if (e.path.length === 0) continue;
    const parent = key(e.path.slice(0, -1));
    const list = children.get(parent) ?? [];
    list.push(e);
    children.set(parent, list);
  }
  const out: string[][] = [];
  const visited = new Set<string>();
  // `getSheetChildren( aPaths, aScene, aVisited, aCurPath )`
  const visit = (path: readonly string[], file: string) => {
    if (visited.has(file)) return;
    visited.add(file);
    for (const child of children.get(key(path)) ?? []) {
      out.push([...child.path]);
      visit(child.path, `f:${child.file}`);
    }
  };
  if (virtualRoot) {
    // the virtual root's screen is not walked: each top-level sheet is a start, unlisted (screens are keyed `f:<file>`, the root's "root")
    for (const top of children.get("") ?? []) visit(top.path, `f:${top.file}`);
  } else {
    visit([], "root");
  }
  return out;
}
