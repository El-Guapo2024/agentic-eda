// The edit that deletes what the board's interactive delete tool picked. KiCad hands the one item it found to the editor's own
// `DeleteItems` (`EDIT_TOOL::DeleteItems( items, false )`); this is that deletion as `/api/cmd` verbs, in one batch so a click is
// one undo step. (The schematic's is kicad-port/schDelete.ts, `SCH_EDIT_TOOL::DoDelete`.)
//
//   pcbnew/tools/pcb_control.cpp    PCB_CONTROL::InteractiveDelete
//   pcbnew/tools/edit_tool.cpp      EDIT_TOOL::DeleteItems: a locked item is skipped; deleting a group deletes its members
import type { BoardState, Cmd } from "../api/types";
import { expandGroups } from "./groupTree";
import { lockedForTools } from "./pcbTransform";
import { extraDeleteCmd, extraItems } from "./pcbExtraItems";
import { cellById } from "./tables";
import { editFieldCmd, fieldById } from "./fpFields";
import { pointInRings } from "./pcbModify";
import { fpTextById, itemKind, padParent } from "./pcbItems";
import { padStaysPad } from "./freePads";

/** The tuning patterns (`PCB_GENERATOR`s) among `ids`: deleting one puts the track back as it was before it was tuned, which the tuner's API does (`/api/tune/remove`), not a verb. */
export function tuningPatternIds(board: BoardState | null | undefined, ids: readonly string[]): string[] {
  const patterns = board?.drawings?.tuning_patterns ?? [];
  return ids.filter((id) => patterns.some((p) => p.id === id));
}

/**
 * The verbs that delete `ids` from the board: tracks, vias, zones, graphics, text and dimensions by their verb, a placed footprint
 * by ripping it up (the board's footprints come from the schematic, so deleting one unplaces it), a group as all its members -- the groups it holds opened.
 * A locked item is skipped (`FilterCollectorForLockedItems`), and so is a group with a locked item anywhere below it or a locked group above it;
 * `skippedLocked` says how many of `ids` were. The backend takes a deleted item out of its groups, and a group left without members goes too.
 *
 * `case PCB_ZONE_T:` "cutouts can be deleted when the delete tool is called from inside a cutout when the zone is selected": with one item to
 * delete, not a Cut, and `cursor` inside one of that zone's holes (`ZONE::HitTestCutout`, the first hole that holds it), only that hole goes
 * (`ZONE::RemoveCutout`) and the zone stays selected (`keepSelection`). KiCad 8303b2ad pushes no commit for it (the removal does not count in
 * `itemsDeleted`, so no `commit.Push` runs and the change has no undo entry); here it is one undoable `set_zone_outline`, a deliberate difference.
 */
export function boardDeleteCmds(
  board: BoardState,
  given: readonly string[],
  opts: { cursor?: { x: number; y: number } | null; isCut?: boolean } = {}
): { cmds: Cmd[]; skippedLocked: number; message?: string; keepSelection?: string[] } {
  const groups = board.drawings?.groups ?? [];
  let skippedLocked = 0;
  const free: string[] = [];
  // KiCad 10's microvia stack: `PCB_VIA_STACK::Remove` takes its vias and traces with it (one verb, UI-LEFT-3 X3-1)
  const viaStacks = new Set((board.drawings?.via_stacks ?? []).map((s) => s.id));
  const stackCmds: Cmd[] = [];
  for (const id of given) {
    if (!viaStacks.has(id)) continue;
    if (lockedForTools(board, id)) skippedLocked++;
    else stackCmds.push({ op: "pcb_item", verb: "delete_via_stack", id });
  }
  const ids = given.filter((id) => !viaStacks.has(id));
  const patterns = new Set(tuningPatternIds(board, ids));
  for (const id of ids) {
    // a tuning pattern is not deleted with its tracks: it is removed (`PCB_TUNING_PATTERN::Remove`, the track put back) -- see [`tuningPatternIds`]
    if (patterns.has(id)) continue;
    if (lockedForTools(board, id)) skippedLocked++;
    else free.push(id);
  }
  if (free.length === 1 && patterns.size === 0 && !opts.isCut && opts.cursor) {
    const zone = (board.routing?.zones ?? []).find((z) => z.id === free[0]);
    const holes = zone?.holes ?? [];
    const c = opts.cursor;
    const hit = holes.findIndex((h) => pointInRings([h], [c.x, c.y]));
    if (zone && hit >= 0) {
      const xy = (r: readonly (readonly [number, number])[]) => r.map(([x, y]) => ({ x, y }));
      return {
        cmds: [{ op: "set_zone_outline", id: zone.id, outline: xy(zone.outline), holes: holes.filter((_, i) => i !== hit).map(xy) }],
        skippedLocked,
        keepSelection: [zone.id],
      };
    }
  }
  const wanted = new Set<string>(expandGroups(groups, free));
  const tracks = new Set((board.routing?.tracks ?? []).map((t) => t.id));
  const vias = new Set((board.routing?.vias ?? []).map((v) => v.id));
  const zones = new Set((board.routing?.zones ?? []).map((z) => z.id));
  const shapes = new Set((board.drawings?.shapes ?? []).map((s) => s.id));
  const texts = new Set((board.drawings?.texts ?? []).map((t) => t.id));
  const dimensions = new Set((board.drawings?.dimensions ?? []).map((d) => d.id));
  const placed = new Set(board.parts.filter((p) => p.placed).map((p) => p.ref));
  const extras = new Map(extraItems(board).map((i) => [i.id, i]));
  const tables = new Set((board.drawings?.tables ?? []).map((t) => t.id));
  const cmds: Cmd[] = [];
  // `EDIT_TOOL::DeleteItems`: a table is removed, but a table cell only has its text cleared ("Clear contents of table cell"); a locked table keeps both.
  const cleared = new Map<string, [number, number][]>();
  for (const id of wanted) {
    const cell = tables.has(id) ? undefined : cellById(board, id);
    if (!cell) continue;
    if (wanted.has(cell.table.id)) continue;
    if (lockedForTools(board, cell.table.id)) {
      skippedLocked++;
      continue;
    }
    cleared.set(cell.table.id, [...(cleared.get(cell.table.id) ?? []), [cell.cell.row, cell.cell.col]]);
  }
  for (const [tid, cells] of cleared) cmds.push({ op: "table", verb: "edit_cells", id: tid, cells, edit: { text: "" } });
  // `case PCB_FIELD_T:` a field is not deleted: a shown one is hidden ("Hide Field" / "Hide Fields"); for one already hidden, when nothing else was
  // deleted, "Use the Footprint Properties dialog to remove fields." (A field of a footprint that is itself deleted goes with it.)
  let alreadyHidden = 0;
  let deleted = 0;
  for (const id of wanted) {
    const f = fieldById(board, id);
    if (!f || wanted.has(f.part.ref)) continue;
    if (f.field.visible) cmds.push(editFieldCmd(f.part, f.field, { visible: false }));
    else alreadyHidden++;
  }
  // `case PCB_TEXT_T:` a footprint's own text is removed from its footprint (`commit.Remove( board_item )`; UI-LEFT P15), in the instance's own copy -- the highest
  // index first, as each removal renumbers the texts after it. (One of a footprint that is itself deleted goes with it.)
  const fpTexts = [...wanted]
    .map((id) => ({ id, at: id.lastIndexOf("~t") }))
    .filter(({ id, at }) => at > 0 && fpTextById(board, id) && !wanted.has(id.slice(0, at)))
    .sort((a, b) => a.id.slice(0, a.at).localeCompare(b.id.slice(0, b.at)) || Number(b.id.slice(b.at + 2)) - Number(a.id.slice(a.at + 2)));
  for (const { id } of fpTexts) {
    cmds.push({ op: "edit_board_footprint_text", id, delete: true });
    deleted++;
  }
  // `case PCB_PAD_T: if( IsFootprintEditor() || m_AllowFreePads ) commit.Remove( board_item )`: a pad leaves its footprint (one of a footprint that is itself deleted goes
  // with it) -- the later pads of its number first, as each removal renames the ones after it (`REF.NUM#k`). Without "Allow free pads" the caller has promoted it.
  const pads = [...wanted]
    .filter((id) => padStaysPad() && itemKind(board, id) === "pad" && !wanted.has(padParent(board, id) ?? ""))
    .map((id) => {
      // `REF.NUM` or `REF.NUM#k`: the footprint's reference holds no `.`
      const hash = /#(\d+)$/.exec(id);
      return { id, base: hash ? id.slice(0, hash.index) : id, nth: hash ? Number(hash[1]) : 1 };
    })
    .sort((a, b) => a.base.localeCompare(b.base) || b.nth - a.nth);
  for (const { id } of pads) {
    cmds.push({ op: "remove_board_pad", pad: id });
    deleted++;
  }
  for (const id of wanted) {
    const known = tables.has(id) || tracks.has(id) || vias.has(id) || zones.has(id) || shapes.has(id) || texts.has(id) || dimensions.has(id) || extras.has(id) || placed.has(id);
    if (!known) continue;
    if (tables.has(id)) cmds.push({ op: "table", verb: "delete", id });
    else if (tracks.has(id)) cmds.push({ op: "delete_track", id });
    else if (vias.has(id)) cmds.push({ op: "delete_via", id });
    else if (zones.has(id)) cmds.push({ op: "delete_zone", id });
    else if (shapes.has(id)) cmds.push({ op: "delete_shape", id });
    else if (texts.has(id)) cmds.push({ op: "delete_text", id });
    else if (dimensions.has(id)) cmds.push({ op: "delete_dimension", id });
    else if (extras.has(id)) cmds.push(extraDeleteCmd(extras.get(id)!));
    else cmds.push({ op: "rip", part: id });
    deleted++;
  }
  const message = deleted === 0 && cleared.size === 0 && cmds.length === 0 && stackCmds.length === 0 && alreadyHidden > 0 ? "Use the Footprint Properties dialog to remove fields." : undefined;
  return { cmds: [...stackCmds, ...cmds], skippedLocked, ...(message ? { message } : {}) };
}
