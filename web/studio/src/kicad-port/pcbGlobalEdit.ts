// Ports of the Global Edit tool's dialogs at 8303b2ad (pcbnew/tools/global_edit_tool.cpp):
//
//   Global Deletions     dialogs/dialog_global_deletion.cpp  DIALOG_GLOBAL_DELETION::DoGlobalDeletions
//   Cleanup Graphics     graphics_cleaner.cpp + dialogs/dialog_cleanup_graphics.cpp
//   Update / Change Footprints   dialogs/dialog_exchange_footprints.cpp's scope matching
//
// Pure functions over the studio's board state; the commands they produce are sent as one undo step by
// actions/pcbGlobalEditSweep.ts.
//
// Units: nanometres (the board's coordinates).

import type { BoardState, Cmd, CmdShape, Shape } from "../api/types";
import { arcCenter, toIU, type V } from "./pcbGeom";
import { healShapes } from "./pcbModify";
import { ancestors } from "./groupTree";
import { extraDeleteCmd } from "./pcbExtraItems";
import { zoneLayers } from "./zoneLayers";

// --------------------------------------------------------------------- global deletions

/** `DIALOG_GLOBAL_DELETION`'s controls. */
export interface GlobalDeletionOptions {
  zones: boolean;
  texts: boolean;
  boardEdges: boolean;
  drawings: boolean;
  footprints: boolean;
  tracks: boolean;
  teardrops: boolean;
  markers: boolean;
  /** "Clear board". */
  all: boolean;
  drawingFilterLocked: boolean;
  drawingFilterUnlocked: boolean;
  footprintFilterLocked: boolean;
  footprintFilterUnlocked: boolean;
  trackFilterLocked: boolean;
  trackFilterUnlocked: boolean;
  viaFilterLocked: boolean;
  viaFilterUnlocked: boolean;
  /** `m_rbLayersOption`: "Current layer (...) only" instead of "All layers". */
  currentLayerOnly: boolean;
}

/** The dialog opens benign: nothing ticked, only the "unlocked" filters on, all layers. */
export const DEFAULT_GLOBAL_DELETION: GlobalDeletionOptions = {
  zones: false,
  texts: false,
  boardEdges: false,
  drawings: false,
  footprints: false,
  tracks: false,
  teardrops: false,
  markers: false,
  all: false,
  drawingFilterLocked: false,
  drawingFilterUnlocked: true,
  footprintFilterLocked: false,
  footprintFilterUnlocked: true,
  trackFilterLocked: false,
  trackFilterUnlocked: true,
  viaFilterLocked: false,
  viaFilterUnlocked: true,
  currentLayerOnly: false,
};

export interface GlobalDeletionPlan {
  cmds: Cmd[];
  /** Every id the plan deletes or unplaces (footprints by reference). */
  removed: string[];
  /** `board->DeleteMARKERs()`: the DRC markers are cleared too. */
  clearMarkers: boolean;
}

const isCopper = (layer: string): boolean => layer.endsWith(".Cu");

/**
 * `DIALOG_GLOBAL_DELETION::DoGlobalDeletions` over the studio's items: zones (copper pours and teardrops apart), graphic shapes
 * (the non-copper layers except Edge.Cuts for "Graphics", Edge.Cuts for "Board outlines"), texts, footprints (taken off the board,
 * as Delete does), tracks and vias, each through its locked / unlocked filter and the layer filter. "Clear board" takes
 * everything, locked or not, on every layer.
 *
 * Differs from the C++ on one point: there Text is only deleted when no graphics option is ticked (an `else if` chain), which
 * looks like a slip; here Text always deletes the texts and the text boxes.
 */
export function planGlobalDeletions(board: BoardState, o: GlobalDeletionOptions, currentLayer: string | null): GlobalDeletionPlan {
  // `BOARD_ITEM::IsLocked`: the item's own lock, or a group's above it.
  const lockedIds = new Set(board.locked ?? []);
  const groups = board.drawings?.groups ?? [];
  const locked = { has: (id: string): boolean => lockedIds.has(id) || ancestors(groups, id).some((g) => lockedIds.has(g.id)) };
  const layerOk = (...layers: string[]): boolean => !o.currentLayerOnly || (currentLayer !== null && layers.includes(currentLayer));
  const cmds: Cmd[] = [];
  const removed: string[] = [];
  const trackIds: string[] = [];
  const viaIds: string[] = [];
  const del = (cmd: Cmd, id: string): void => {
    cmds.push(cmd);
    removed.push(id);
  };

  // `( item->GetLayerSet() & layers_mask ).any()`: a zone goes when any of its layers is the one asked.
  for (const z of board.routing?.zones ?? []) {
    const on = layerOk(...zoneLayers(z));
    if (o.all || (z.teardrop ? o.teardrops && on : o.zones && on)) del({ op: "delete_zone", id: z.id }, z.id);
  }

  for (const s of board.drawings?.shapes ?? []) {
    if (o.all) {
      del({ op: "delete_shape", id: s.id }, s.id);
      continue;
    }
    if (!(o.drawings || o.boardEdges)) continue;
    const matches = (o.drawings && !isCopper(s.layer) && s.layer !== "Edge.Cuts") || (o.boardEdges && s.layer === "Edge.Cuts");
    if (!matches || !layerOk(s.layer)) continue;
    if (locked.has(s.id) ? o.drawingFilterLocked : o.drawingFilterUnlocked) del({ op: "delete_shape", id: s.id }, s.id);
  }
  for (const t of board.drawings?.texts ?? []) {
    if (o.all || (o.texts && layerOk(t.layer))) del({ op: "delete_text", id: t.id }, t.id);
  }
  // `if( item->Type() == PCB_TEXT_T || item->Type() == PCB_TEXTBOX_T )`: Text takes the text boxes too.
  for (const t of board.drawings?.text_boxes ?? []) {
    if (!o.all && o.texts && layerOk(t.layer)) del(extraDeleteCmd({ kind: "textbox", id: t.id }), t.id);
  }
  if (o.all) {
    // "Clear board": `processItem( item, all_layers )` over every item of `board->Drawings()` -- dimensions, text boxes, tables, barcodes, reference
    // images, points, and KiCad 10's grid items and drill maps. A via stitching is a generator, not a drawing: its vias go with the tracks. A tuning
    // pattern left with no track goes with them (the backend drops it, as `pattern->GetBoardItems().empty()` does).
    for (const d of board.drawings?.dimensions ?? []) del({ op: "delete_dimension", id: d.id }, d.id);
    for (const t of board.drawings?.tables ?? []) del({ op: "table", verb: "delete", id: t.id }, t.id);
    const dr = board.drawings;
    const kinds = [
      ["textbox", dr?.text_boxes],
      ["image", dr?.images],
      ["point", dr?.points],
      ["barcode", dr?.barcodes],
      ["grid", dr?.grid_items],
      ["drill_map", dr?.drill_maps],
    ] as const;
    for (const [kind, items] of kinds) for (const item of items ?? []) del(extraDeleteCmd({ kind, id: item.id }), item.id);
  }

  if (o.all || o.footprints) {
    const top = board.layers[0] ?? "F.Cu";
    const bottom = board.layers[board.layers.length - 1] ?? "B.Cu";
    for (const p of board.parts) {
      if (!p.placed) continue;
      const layer = p.side === "bottom" ? bottom : top;
      if (o.all || (layerOk(layer) && (locked.has(p.ref) ? o.footprintFilterLocked : o.footprintFilterUnlocked))) del({ op: "rip", part: p.ref }, p.ref);
    }
  }

  if (o.all || o.tracks) {
    for (const t of board.routing?.tracks ?? []) {
      if (o.all || (layerOk(t.layer) && (locked.has(t.id) ? o.trackFilterLocked : o.trackFilterUnlocked))) {
        trackIds.push(t.id);
        removed.push(t.id);
      }
    }
    for (const v of board.routing?.vias ?? []) {
      const span = viaLayers(board.layers, v.from, v.to);
      if (o.all || (layerOk(...span) && (locked.has(v.id) ? o.viaFilterLocked : o.viaFilterUnlocked))) {
        viaIds.push(v.id);
        removed.push(v.id);
      }
    }
    if (trackIds.length > 0 || viaIds.length > 0) cmds.push({ op: "commit_route", remove_track_ids: trackIds, remove_via_ids: viaIds });
  }
  return { cmds, removed, clearMarkers: o.markers };
}

/** The copper layers a via spans (`PCB_VIA::GetLayerSet`): `from` to `to` in stack order. */
function viaLayers(layers: readonly string[], from: string, to: string): string[] {
  const a = layers.indexOf(from);
  const b = layers.indexOf(to);
  if (a < 0 || b < 0) return [from, to];
  return layers.slice(Math.min(a, b), Math.max(a, b) + 1);
}

// ------------------------------------------------------------------- cleanup graphics

/** `DIALOG_CLEANUP_GRAPHICS`'s board-editor controls. */
export interface CleanupGraphicsOptions {
  /** "Merge lines into rectangles". */
  mergeRects: boolean;
  /** "Delete redundant graphics". */
  deleteRedundant: boolean;
  /** "Fix discontinuities in board outlines". */
  fixBoardOutlines: boolean;
  /** "Tolerance": the outline gap fixed (`s_defaultTolerance` = 2 mm). */
  toleranceNm: number;
}

export const DEFAULT_CLEANUP_GRAPHICS: CleanupGraphicsOptions = { mergeRects: false, deleteRedundant: false, fixBoardOutlines: false, toleranceNm: 2_000_000 };

/** `CLEANUP_ITEM` descriptions (cleanup_item.cpp). */
export const CLEANUP_LABELS = {
  null: "Remove zero-size graphic",
  duplicate: "Remove duplicated graphic",
  rect: "Convert lines to rectangle",
} as const;

export interface CleanupItem {
  kind: keyof typeof CLEANUP_LABELS;
  label: string;
  /** The shapes the item is about (`SetItems`). */
  ids: string[];
}

export interface CleanupGraphicsPlan {
  /** What the dialog lists under "Changes to be applied" (a dry run; outline fixes are not listed, as in the C++). */
  items: CleanupItem[];
  remove: string[];
  /** The board-outline lines `fixBoardOutlines` moved, edited in place (`m_commit.Modify( shape )`): their id, group and lock stay. */
  replace: { id: string; shape: CmdShape }[];
  add: CmdShape[];
}

/** `m_epsilon = GetDRCEpsilon()`: `ADVANCED_CFG::m_DRCEpsilon`, 0.5 um, in nanometres (round 5 B5-3: it was exact equality since the move to nanometres). */
export const CLEANUP_EPSILON_NM = 500;

/** `equivalent( a, b, epsilon )`: `abs( a.x - b.x ) < epsilon && abs( a.y - b.y ) < epsilon`. */
const same = (a: readonly [number, number], b: readonly [number, number]): boolean => Math.abs(a[0] - b[0]) < CLEANUP_EPSILON_NM && Math.abs(a[1] - b[1]) < CLEANUP_EPSILON_NM;

/** `GRAPHICS_CLEANER::isNullShape`. */
function isNullShape(s: Shape): boolean {
  switch (s.kind) {
    case "segment":
    case "rect":
    case "arc":
      return same(s.start, s.end);
    case "circle":
      return s.end[0] === s.center[0] && s.end[1] === s.center[1];
    case "polygon":
      return s.pts.length === 0;
    case "bezier":
      return same(s.start, s.end) && same(s.c1, s.start) && same(s.c2, s.start);
  }
}

/** `GRAPHICS_CLEANER::areEquivalent` (a polygon is never equivalent: "TODO" in the source). */
function areEquivalent(a: Shape, b: Shape): boolean {
  if (a.kind !== b.kind || a.layer !== b.layer || a.stroke_width !== b.stroke_width) return false;
  switch (a.kind) {
    case "segment":
    case "rect": {
      const o = b as typeof a;
      return same(a.start, o.start) && same(a.end, o.end);
    }
    case "circle": {
      const o = b as typeof a;
      return same(a.center, o.center) && same(a.end, o.end);
    }
    case "arc": {
      const o = b as typeof a;
      const ca = arcCenter({ start: toIU(a.start), mid: toIU(a.mid), end: toIU(a.end) });
      const cb = arcCenter({ start: toIU(o.start), mid: toIU(o.mid), end: toIU(o.end) });
      return Math.abs(ca[0] - cb[0]) < CLEANUP_EPSILON_NM && Math.abs(ca[1] - cb[1]) < CLEANUP_EPSILON_NM && same(a.start, o.start) && same(a.end, o.end);
    }
    case "polygon":
      return false;
    case "bezier": {
      const o = b as typeof a;
      return same(a.start, o.start) && same(a.end, o.end) && same(a.c1, o.c1) && same(a.c2, o.c2);
    }
  }
}

interface Side {
  start: V;
  end: V;
  shape: Extract<Shape, { kind: "segment" }>;
}

/**
 * `GRAPHICS_CLEANER::CleanupBoard` over the board's drawings: duplicates and zero-size graphics out, board-outline gaps closed
 * (`ConnectBoardShapes`, which the dry run skips), and four lines that make a rectangle merged into one. Merging a footprint's
 * graphics into pads belongs to the footprint editor.
 */
export function planCleanupGraphics(shapes: readonly Shape[], o: CleanupGraphicsOptions, dryRun: boolean): CleanupGraphicsPlan {
  const items: CleanupItem[] = [];
  const remove: string[] = [];
  const add: CmdShape[] = [];
  const healed = new Map<string, CmdShape>(); // the outline lines `fixBoardOutlines` moved, by id
  const deleted = new Set<string>(); // IS_DELETED

  if (o.deleteRedundant) {
    for (let i = 0; i < shapes.length; i++) {
      const shape = shapes[i]!;
      if (deleted.has(shape.id)) continue;
      if (isNullShape(shape)) {
        items.push({ kind: "null", label: CLEANUP_LABELS.null, ids: [shape.id] });
        deleted.add(shape.id);
        remove.push(shape.id);
        continue;
      }
      for (let j = i + 1; j < shapes.length; j++) {
        const other = shapes[j]!;
        if (deleted.has(other.id)) continue;
        if (areEquivalent(shape, other)) {
          items.push({ kind: "duplicate", label: CLEANUP_LABELS.duplicate, ids: [other.id] });
          deleted.add(other.id);
          remove.push(other.id);
        }
      }
    }
  }

  // `fixBoardOutlines`: `shapeList.push_back( shape ); m_commit.Modify( shape ); ... ConnectBoardShapes( shapeList, m_outlinesTolerance )` -- the shapes are moved in
  // place, so a healed line keeps its id, group and lock, and `mergeRects()` below sees it where it now is (round 5 B5-4).
  if (o.fixBoardOutlines && !dryRun) {
    const edge = shapes.filter((s) => s.layer === "Edge.Cuts" && !deleted.has(s.id));
    const res = healShapes(edge, o.toleranceNm);
    const byId = new Map(res.replaced.map((r) => [r.id, r.addIndex]));
    const used = new Set<number>();
    for (const id of res.remove) {
      const idx = byId.get(id);
      const next = idx === undefined ? undefined : res.add[idx];
      if (idx !== undefined && next) {
        healed.set(id, next as CmdShape);
        used.add(idx);
        continue;
      }
      deleted.add(id);
      remove.push(id);
    }
    res.add.forEach((shape, i) => {
      if (!used.has(i)) add.push(shape as CmdShape);
    });
  }

  if (o.mergeRects) {
    const sides: Side[] = [];
    const byStart = new Map<string, Side[]>();
    const key = (p: V) => `${p[0]},${p[1]}`;
    for (const original of shapes) {
      // A healed outline line, where the fix moved it.
      const h = healed.get(original.id);
      const s: Shape = h && h.kind === "segment" && original.kind === "segment" ? { ...original, start: [h.start.x, h.start.y], end: [h.end.x, h.end.y] } : original;
      if (s.kind !== "segment" || isNullShape(s) || deleted.has(s.id)) continue;
      if (s.start[0] !== s.end[0] && s.start[1] !== s.end[1]) continue;
      let start: V = [s.start[0], s.start[1]];
      let end: V = [s.end[0], s.end[1]];
      if (start[0] > end[0] || start[1] > end[1]) [start, end] = [end, start];
      const side: Side = { start, end, shape: s };
      sides.push(side);
      const list = byStart.get(key(start)) ?? [];
      list.push(side);
      byStart.set(key(start), list);
    }
    const at = (p: V): Side[] => byStart.get(key(p)) ?? [];
    for (const side of sides) {
      if (deleted.has(side.shape.id)) continue;
      const viable = (c: Side): boolean => c.shape.layer === side.shape.layer && c.shape.stroke_width === side.shape.stroke_width && !deleted.has(c.shape.id);
      let left: Side | null = null;
      let top: Side | null = null;
      if (side.start[0] === side.end[0]) {
        // A possible left side: look for a top starting at the same corner.
        left = side;
        top = at(left.start).find((c) => c !== left && viable(c)) ?? null;
      } else if (side.start[1] === side.end[1]) {
        top = side;
        left = at(top.start).find((c) => c !== top && viable(c)) ?? null;
      }
      if (!top || !left) continue;
      const right = at(top.end).find((c) => c !== top && c !== left && viable(c)) ?? null;
      const bottom = at(left.end).find((c) => c !== top && c !== left && viable(c)) ?? null;
      if (right && bottom && right.end[0] === bottom.end[0] && right.end[1] === bottom.end[1]) {
        const four = [left, top, right, bottom];
        for (const c of four) deleted.add(c.shape.id);
        items.push({ kind: "rect", label: CLEANUP_LABELS.rect, ids: four.map((c) => c.shape.id) });
        for (const c of four) remove.push(c.shape.id);
        add.push({ kind: "rect", layer: top.shape.layer, stroke_width: top.shape.stroke_width, filled: false, start: { x: top.start[0], y: top.start[1] }, end: { x: bottom.end[0], y: bottom.end[1] } });
      }
    }
  }

  // A healed line that a rectangle took is gone; the others are edited in place.
  const replace = [...healed].filter(([id]) => !deleted.has(id)).map(([id, shape]) => ({ id, shape }));
  return { items, remove, replace, add };
}

/** The plan as one undo step's commands ("Cleanup Graphics"): the removals, the in-place edits of the healed outline, the new shapes. */
export function cleanupGraphicsCmds(plan: CleanupGraphicsPlan): Cmd[] {
  return [...plan.remove.map((id): Cmd => ({ op: "delete_shape", id })), ...plan.replace.map(({ id, shape }): Cmd => ({ op: "replace_shape", id, shape })), ...plan.add.map((shape): Cmd => ({ op: "add_shape", shape }))];
}

// ---------------------------------------------------------------------- wildcards

/** `WildCompareString`-style match: `*` any run, `?` one character, case-insensitive. */
export function wildMatch(pattern: string, text: string): boolean {
  const re = new RegExp(`^${pattern.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".")}$`, "i");
  return re.test(text);
}
