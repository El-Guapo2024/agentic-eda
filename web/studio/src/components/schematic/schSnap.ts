// The schematic editor's snapping, as the canvas uses it: the grid helper of the tool in force (kicad-port/schGridHelper.ts, a port of EE_GRID_HELPER) kept fed with the sheet,
// the view, the grid list and the grid overrides, answering "where does a click here go?" and, for a move, "where does the held selection start from and where does it go?".
//
// The sheet's items become `SchSnapItem`s once per sheet object (`itemsOf`): a symbol is its position and its pins, a wire each segment of its polyline (a `SCH_LINE` is one
// segment), a label its anchor, a junction, no-connect or bus entry its point(s), a sheet its corner and its pins, a field or a text its anchor. Shapes give no anchors.
import type { Schematic } from "../../api/types";
import { SchGridHelper, type SchSnapItem } from "../../kicad-port/schGridHelper";
import { computeVisibleGridSize } from "../../kicad-port/grid";
import { gridSizeFor, type GridCategory, type GridOverrides, type SchItemKind } from "../../kicad-port/gridOverrides";
import type { GridEnv, SnapOverlay } from "../../kicad-port/gridHelperBase";
import { box, type Pt } from "../../kicad-port/snapGeom";
import { fieldBox, fieldItems, isShownField } from "../../kicad-port/schFieldEdit";
import { allItems, itemBounds } from "./schItems";
import { resolveLibSymbol, symbolBounds } from "./libSymbol";
import { measureStrokeText } from "../text/strokeFont";
import { reportSnap } from "../../kicad-port/snapReport";

/** What the snapping needs to know about the editor; the canvas hands it over on every render. */
export interface SchSnapView {
  sch: Schematic | null;
  /** Screen pixels per um. */
  scale: number;
  gridUm: number;
  grids: readonly number[];
  overrides: GridOverrides | null;
}

export interface SchSnapMods {
  /** Ctrl (Cmd on a Mac): the grid off. */
  ctrl: boolean;
  /** Shift: anchors off. */
  shift: boolean;
}

const itemCache = new WeakMap<Schematic, SchSnapItem[]>();

const boxOf = (b: { minX: number; minY: number; maxX: number; maxY: number }) => box(b.minX, b.minY, b.maxX, b.maxY);

/** The sheet as the grid helper sees it. */
export function itemsOf(sch: Schematic): SchSnapItem[] {
  const hit = itemCache.get(sch);
  if (hit) return hit;
  const out: SchSnapItem[] = [];

  for (const w of sch.wires) {
    const kind: SchItemKind = w.bus ? "bus" : "wire";
    for (let i = 0; i + 1 < w.pts.length; i++) {
      const a = w.pts[i]!;
      const b = w.pts[i + 1]!;
      out.push({
        id: `${w.id}#${i}`,
        owner: w.id,
        kind,
        connectable: true,
        position: a,
        connectionPoints: [a, b],
        bbox: box(Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[0], b[0]), Math.max(a[1], b[1])),
        line: { a, b, graphic: false },
      });
    }
  }
  for (const l of sch.lines ?? []) {
    for (let i = 0; i + 1 < l.pts.length; i++) {
      const a = l.pts[i]!;
      const b = l.pts[i + 1]!;
      out.push({
        id: `${l.id}#${i}`,
        owner: l.id,
        kind: "graphic_line",
        connectable: false,
        position: a,
        connectionPoints: [a, b],
        bbox: box(Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[0], b[0]), Math.max(a[1], b[1])),
        line: { a, b, graphic: true },
      });
    }
  }
  for (const j of sch.junctions ?? []) out.push({ id: j.id, owner: j.id, kind: "junction", connectable: true, position: j.at, connectionPoints: [j.at], bbox: box(j.at[0] - 460, j.at[1] - 460, j.at[0] + 460, j.at[1] + 460) });
  for (const n of sch.no_connects) out.push({ id: n.id, owner: n.id, kind: "no_connect", connectable: true, position: n.at, connectionPoints: [n.at], bbox: box(n.at[0] - 610, n.at[1] - 610, n.at[0] + 610, n.at[1] + 610) });
  for (const b of sch.bus_entries) {
    const end: Pt = [b.at[0] + b.size[0], b.at[1] + b.size[1]];
    out.push({ id: b.id, owner: b.id, kind: "bus_entry", connectable: true, position: b.at, connectionPoints: [b.at, end], bbox: box(Math.min(b.at[0], end[0]), Math.min(b.at[1], end[1]), Math.max(b.at[0], end[0]), Math.max(b.at[1], end[1])) });
  }
  for (const l of sch.labels) {
    const kind: SchItemKind = l.scope === "global" ? "global_label" : l.scope === "hierarchical" ? "hier_label" : "label";
    const b = itemBounds(sch, { id: l.id, kind: "label" });
    out.push({ id: l.id, owner: l.id, kind, connectable: true, position: l.at, connectionPoints: [l.at], bbox: b ? boxOf(b) : box(l.at[0], l.at[1], l.at[0], l.at[1]) });
  }
  for (const t of sch.texts) {
    const b = itemBounds(sch, { id: t.id, kind: "text" });
    out.push({ id: t.id, owner: t.id, kind: "text", connectable: false, position: t.at, connectionPoints: [], bbox: b ? boxOf(b) : box(t.at[0], t.at[1], t.at[0], t.at[1]) });
  }
  for (const s of sch.sheets) {
    out.push({ id: s.id, owner: s.id, kind: "sheet", connectable: true, position: s.at, connectionPoints: s.pins.map((p) => p.at as Pt), bbox: box(s.at[0], s.at[1], s.at[0] + s.size[0], s.at[1] + s.size[1]) });
  }
  for (const ps of sch.power_symbols) {
    const b = itemBounds(sch, { id: ps.id, kind: "power" });
    out.push({ id: ps.id, owner: ps.id, kind: "symbol", connectable: true, position: ps.at, connectionPoints: [ps.at], bbox: b ? boxOf(b) : box(ps.at[0] - 600, ps.at[1] - 600, ps.at[0] + 600, ps.at[1] + 600) });
  }
  // A symbol: its position and its pins' ends. A multi-unit symbol is one item per unit (each is a `SCH_SYMBOL` in KiCad).
  sch.symbols.forEach((s, i) => {
    const real = resolveLibSymbol(s, sch.lib_symbols);
    const b = symbolBounds(s, sch.lib_symbols);
    out.push({ id: sch.symbols.findIndex((q) => q.id === s.id) === i ? s.id : `${s.id}@${i}`, owner: s.id, kind: "symbol", connectable: true, position: s.at, connectionPoints: real ? real.pins.map((p) => p.tip as Pt) : [], bbox: boxOf(b) });
  });
  // The fields and (above) the texts: only their anchors, and only for a move of text alone (`aIncludeText`).
  for (const f of fieldItems(sch)) {
    if (!isShownField(f.field)) continue;
    const fb = fieldBox(f.field, measureStrokeText);
    out.push({ id: f.id, owner: f.owner, kind: "field", connectable: false, position: f.field.at as Pt, connectionPoints: [], bbox: fb ? boxOf(fb) : box(f.field.at[0], f.field.at[1], f.field.at[0], f.field.at[1]) });
  }
  itemCache.set(sch, out);
  return out;
}

/** The kind of an item the user selected (an id of `allItems`), for `GetItemGrid`. */
function kindOfSelected(sch: Schematic, id: string): SchItemKind {
  const ref = allItems(sch).find((r) => r.id === id);
  if (!ref) return "other";
  switch (ref.kind) {
    case "symbol":
    case "power":
      return "symbol";
    case "wire": {
      const w = sch.wires.find((x) => x.id === id);
      return w?.bus ? "bus" : "wire";
    }
    case "label": {
      const l = sch.labels.find((x) => x.id === id);
      return l?.scope === "global" ? "global_label" : l?.scope === "hierarchical" ? "hier_label" : "label";
    }
    case "text":
      return "text";
    case "no_connect":
      return "no_connect";
    case "bus_entry":
      return "bus_entry";
    case "junction":
      return "junction";
    case "line":
      return "graphic_line";
    case "sheet":
      return "sheet";
    case "graphic":
      return "shape";
    case "field":
      return "field";
  }
}

export class SchSnap {
  private view: SchSnapView | null = null;
  private helper: SchGridHelper | null = null;
  private tool: string | null = null;
  private cachedOverlay: SnapOverlay | null = null;
  private last: Pt | null = null;

  update(view: SchSnapView): void {
    this.view = view;
    if (this.helper && view.sch) this.helper.setItems(itemsOf(view.sch));
  }

  private env(): GridEnv {
    const v = this.view!;
    const scale = v.scale > 0 ? v.scale : 1;
    return { scale, gridUm: v.gridUm, visibleGridUm: computeVisibleGridSize(v.gridUm, scale), origin: [0, 0], gridSizeOf: (c) => gridSizeFor(c, v.gridUm, v.grids, v.overrides) };
  }

  private helperFor(tool: string): SchGridHelper {
    const v = this.view!;
    if (!this.helper || this.tool !== tool) {
      this.helper = new SchGridHelper(this.env(), v.sch ? itemsOf(v.sch) : []);
      this.tool = tool;
      this.cachedOverlay = null;
    }
    this.helper.setEnv(this.env());
    return this.helper;
  }

  /** `BestSnapAnchor`: where a click or cursor at `world` goes for the tool `tool`. */
  point(tool: string, world: Pt, mods: SchSnapMods, category: GridCategory = "current", skip: ReadonlySet<string> = new Set()): Pt {
    if (!this.view) return world;
    const h = this.helperFor(tool);
    h.setUseGrid(!mods.ctrl);
    h.setSnap(!mods.shift);
    const p = h.bestSnapAnchor(world, category, skip);
    this.cachedOverlay = h.overlay();
    this.last = p;
    reportSnap({ tool, input: world, output: p, types: 0, anchored: this.cachedOverlay.snapPoint !== null });
    return p;
  }

  /** `GetGridSize( category )`: the category's grid under the overrides. */
  gridSize(category: GridCategory): number {
    const v = this.view;
    return v ? gridSizeFor(category, v.gridUm, v.grids, v.overrides) : 0;
  }

  /** `Align( point, category )` only: the grid of the category, no anchors (a rubber band follows the cursor with it). */
  align(world: Pt, mods: SchSnapMods, category: GridCategory = "current"): Pt {
    if (!this.view) return world;
    const h = this.helperFor("align");
    h.setUseGrid(!mods.ctrl);
    return h.align(world, category);
  }

  /** The point the last `point` call put the cursor at; null before one and after `reset`. */
  lastPoint(): Pt | null {
    return this.last;
  }

  /** `GetSelectionGrid` of the ids. */
  selectionCategory(ids: readonly string[]): GridCategory {
    const v = this.view;
    if (!v?.sch) return "current";
    return this.helperFor("selection").getSelectionGrid(ids.map((id) => ({ kind: kindOfSelected(v.sch!, id) })));
  }

  /** `BestDragOrigin`: the point of the held items a move starts from. */
  dragOrigin(mouse: Pt, ids: readonly string[]): Pt {
    const v = this.view;
    if (!v?.sch) return mouse;
    const wanted = new Set(ids);
    const items = itemsOf(v.sch).filter((i) => wanted.has(i.owner) || wanted.has(i.id));
    const h = this.helperFor("move");
    return h.bestDragOrigin(mouse, h.getSelectionGrid(items), items);
  }

  /** The cursor of a move of `ids`: `BestSnapAnchor( mouse, selectionGrid, selection )`. */
  moveCursor(mouse: Pt, mods: SchSnapMods, ids: readonly string[]): Pt {
    return this.point("move", mouse, mods, this.selectionCategory(ids), new Set(ids));
  }

  overlay(): SnapOverlay | null {
    return this.cachedOverlay;
  }

  reset(): void {
    this.helper = null;
    this.tool = null;
    this.cachedOverlay = null;
    this.last = null;
    reportSnap(null);
  }
}

/**
 * The grid category each placing tool snaps on, as its KiCad tool names it (`GRID_WIRES` for the wire and bus tools, a junction, a bus entry, a break; `GRID_GRAPHICS` for the line,
 * shape and text box tools; `GRID_TEXT` for text; `GRID_CONNECTABLE` for labels, power symbols, no-connects, sheets, sheet pins, symbols and rule areas).
 */
export function toolCategory(tool: string): GridCategory {
  switch (tool) {
    case "wire":
    case "bus":
    case "sch_junction":
    case "sch_bus_entry":
    case "sch_break":
      return "wires";
    case "sch_line":
    case "sch_rect":
    case "sch_circle":
    case "sch_arc":
    case "sch_bezier":
    case "sch_textbox":
      return "graphics";
    case "sch_text":
      return "text";
    case "sch_label_local":
    case "sch_label_global":
    case "sch_label_hier":
    case "sch_directive":
    case "sch_power":
    case "sch_no_connect":
    case "sch_sheet":
    case "sch_sheet_pin":
    case "sch_place_symbol":
    case "sch_rule_area":
      return "connectable";
    default:
      return "current";
  }
}

/** The tools whose clicks go through the grid helper: the cursor's marker and snap lines follow them on every move. */
export const SCH_PLACING_TOOLS: ReadonlySet<string> = new Set(["wire", "bus", "sch_junction", "sch_bus_entry", "sch_break", "sch_line", "sch_rect", "sch_circle", "sch_arc", "sch_bezier", "sch_textbox", "sch_text", "sch_label_local", "sch_label_global", "sch_label_hier", "sch_directive", "sch_power", "sch_no_connect", "sch_sheet", "sch_sheet_pin", "sch_place_symbol", "sch_rule_area"]);
