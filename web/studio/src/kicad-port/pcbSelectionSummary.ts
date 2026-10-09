// What a PCB selection is made of, in the terms the context menu's conditions use (kicad-port/pcbContextMenu.ts): how many items of each type, how many are
// locked, what the group tool and the point editor would say about it. Read off the board state; the selection ids are the ones `state.selection` holds.
//
//   common/tool/group_tool.cpp                GROUP_CONTEXT_MENU::update            the group facts
//   pcbnew/tools/pcb_point_editor.cpp         CanAddCorner, CanChamferCorner        the corner facts
//   pcbnew/tools/edit_tool.cpp                itemHasEditableCorners                editable corners
//   pcbnew/tools/board_editor_control.cpp     getOverlappingZones, ZONE_PRIORITY_CONTEXT_MENU::update   raise / lower
//
// Pure: no store, no DOM. Unit-tested in pcbSelectionSummary.test.ts.
import type { BoardState, Zone } from "../api/types";
import { convertAvailability } from "./pcbConvert";
import { emptyPcbSummary, type PcbSelectionSummary, type ShapeKind } from "./pcbContextMenu";
import { ancestors, parentGroup } from "./groupTree";
import { itemBounds, itemKind, padById } from "./pcbItems";

const isCopper = (layer: string): boolean => layer.endsWith(".Cu");

/** `BOARD_ITEM::IsLocked()`: the item's own lock, or that of a group above it. */
function lockedItem(board: BoardState, id: string): boolean {
  const locked = new Set(board.locked ?? []);
  if (locked.has(id)) return true;
  const groups = board.drawings?.groups ?? [];
  return ancestors(groups, id).some((g) => locked.has(g.id));
}

/** `getOverlappingZones`: the other copper zones on the same layer whose box meets this one's. (KiCad also tests the outlines against each other; the box is the first cut.) */
function overlappingZones(board: BoardState, zone: Zone): Zone[] {
  const box = itemBounds(board, zone.id);
  if (!box) return [];
  return (board.routing?.zones ?? []).filter((other) => {
    if (other.id === zone.id || other.is_rule_area || other.teardrop || other.layer !== zone.layer) return false;
    const b = itemBounds(board, other.id);
    return b != null && b[0] <= box[2] && b[2] >= box[0] && b[1] <= box[3] && b[3] >= box[1];
  });
}

/** Counts `ids` as the menu's conditions do. A pad id counts as a pad; an id the board does not have counts as nothing. */
export function summarizePcbSelection(board: BoardState, ids: readonly string[]): PcbSelectionSummary {
  const s = emptyPcbSummary();
  const groups = board.drawings?.groups ?? [];
  let hasGroup = false;
  let onlyOneGroup = false;
  let hasUngroupedItems = false;
  let hasMember = false;
  let onlyItem: string | null = null;

  for (const id of ids) {
    const kind = itemKind(board, id);
    if (kind === null) continue;
    onlyItem = id;
    s.total++;
    if (lockedItem(board, id)) s.locked++;
    else s.unlocked++;
    switch (kind) {
      case "part":
        s.footprints++;
        break;
      case "pad":
        s.pads++;
        if (padById(board, id)?.pad.net) s.hasNet = true;
        break;
      case "track": {
        const t = board.routing?.tracks.find((x) => x.id === id);
        if (t?.arc_mid) s.arcTracks++;
        else s.tracks++;
        if (t?.net) s.hasNet = true;
        break;
      }
      case "via": {
        s.vias++;
        if (board.routing?.vias.find((v) => v.id === id)?.net) s.hasNet = true;
        break;
      }
      case "zone": {
        const z = board.routing?.zones.find((x) => x.id === id);
        s.zones++;
        if (z && !z.is_rule_area && !z.teardrop) s.copperZones++;
        if (z && !z.is_rule_area && z.net) s.hasNet = true;
        break;
      }
      case "shape": {
        const shape = board.drawings?.shapes.find((x) => x.id === id);
        if (shape) {
          s.shapes[shape.kind as ShapeKind]++;
          if (isCopper(shape.layer)) s.copperShapes++;
        }
        break;
      }
      case "text":
        s.texts++;
        break;
      case "dimension":
        s.dimensions++;
        break;
      case "group":
        s.groups++;
        break;
    }
    // GROUP_CONTEXT_MENU::update
    if (kind === "group") {
      if (hasGroup) onlyOneGroup = false;
      else {
        onlyOneGroup = true;
        hasGroup = true;
      }
    } else if (!parentGroup(groups, kind === "pad" ? (padById(board, id)?.part.ref ?? id) : id)) hasUngroupedItems = true;
    if (parentGroup(groups, id)) hasMember = true;
  }
  s.group = { hasGroup, onlyOneGroup, hasUngroupedItems, hasMember };

  // The point editor's and the zone menu's facts are about the one selected item.
  if (s.total === 1 && onlyItem !== null) {
    const kind = itemKind(board, onlyItem);
    const zone = kind === "zone" ? board.routing?.zones.find((z) => z.id === onlyItem) : undefined;
    const shape = kind === "shape" ? board.drawings?.shapes.find((x) => x.id === onlyItem) : undefined;
    // CanAddCorner: a zone, or a segment, polygon or arc shape. CanChamferCorner: a zone or a polygon. itemHasEditableCorners: a polygon, or a zone that is not a teardrop area.
    s.canAddCorner = !!zone || (shape != null && (shape.kind === "segment" || shape.kind === "polygon" || shape.kind === "arc"));
    s.canChamferCorner = !!zone || shape?.kind === "polygon";
    s.editableCorners = (zone != null && !zone.teardrop) || shape?.kind === "polygon";
    if (zone && !zone.is_rule_area && !zone.teardrop) {
      for (const other of overlappingZones(board, zone)) {
        if (other.priority > zone.priority) s.zoneCanRaise = true;
        if (other.priority < zone.priority) s.zoneCanLower = true;
      }
    }
  }

  // CONVERT_TOOL::Init's conditions over the whole selection.
  const convert = convertAvailability(board, ids.filter((id) => itemKind(board, id) !== null));
  s.convert = { poly: convert.poly, zone: convert.zone, keepout: convert.keepout, lines: convert.lines, outset: convert.outset, tracks: convert.tracks, arc: convert.arc };
  return s;
}

/** Whether the board has anything on it (`!BOARD::IsEmpty()`): a footprint, a track, a via, a zone, a graphic, a text or a dimension. */
export function boardHasItems(board: BoardState): boolean {
  return (
    board.parts.some((p) => p.placed) ||
    (board.routing?.tracks.length ?? 0) > 0 ||
    (board.routing?.vias.length ?? 0) > 0 ||
    (board.routing?.zones.length ?? 0) > 0 ||
    (board.drawings?.shapes.length ?? 0) > 0 ||
    (board.drawings?.texts.length ?? 0) > 0 ||
    (board.drawings?.dimensions.length ?? 0) > 0
  );
}
