// Canvas2D "GAL-like" schematic painter -- eeschema's look (KiCad Default
// theme colors from src/kicad/colors.json, which already covers eeschema's
// layers: LAYER_WIRE/LAYER_JUNCTION/LAYER_DEVICE/LAYER_PIN/etc., see
// layers.ts's layerColor(), which resolves a real KiCad key directly when
// it isn't one of this app's PCB "bucket" names) drawn from GET
// /api/schematic's structured data via layout.ts's ported geometry.
//
// v1, read-only: symbol boxes (generic IC rect, or a resistor/capacitor/
// inductor/diode glyph for a recognized 2-pin passive -- same heuristic
// eda-render uses), pin stubs + names, ref/value text, wire polylines,
// same-net T-junction dots, and a simplified net-label tag (colored by
// power/ground/plain, not the full ground-bar/power-flag/signal-tag glyph
// set crates/render/src/lib.rs draws -- that's a lot of glyph-specific
// path math for a first pass at a read-only view; this reads the net name
// at its anchor point instead, still in the right KiCad layer color).
import type { Schematic, SchematicWire } from "../../api/types";
import type { ViewTransform } from "../../state/store";
import { layerColor } from "../canvas/layers";
import { resolveSymbol, STUB, type ResolvedSymbol } from "./layout";

export interface SchematicPaintOptions {
  selection: Set<string>;
  netHighlight: string | null;
}

const REF_FONT = 1.6;
const VALUE_FONT = 1.4;
const PIN_FONT = 1.1;
const FIELD_FONT = 1.0; // footprint field: small, per the task's "footprint field in purple and small"

/** Net names that read as power/ground rails -- crates/engine/src/geometry.rs `is_power_or_ground_net_name`, ported. */
function isPowerOrGroundNetName(name: string): boolean {
  const upper = name.toUpperCase();
  const trimmed = upper.replace(/^\+/, "");
  const rails = ["GND", "AGND", "DGND", "VSS", "VCC", "VDD", "VDDA", "VBAT", "VBUS", "VSYS", "3V3", "5V", "1V8", "12V"];
  return rails.includes(trimmed) || trimmed.startsWith("GND") || trimmed.startsWith("AGND") || trimmed.startsWith("DGND") || trimmed.startsWith("VSS");
}

function labelColor(net: string): string {
  return isPowerOrGroundNetName(net) ? layerColor("LAYER_GLOBLABEL") : layerColor("LAYER_LOCLABEL");
}

function localPortPoint(port: ResolvedSymbol["ports"][number], width: number, height: number): [number, number] {
  switch (port.side) {
    case "top":
      return [port.offset, 0];
    case "bottom":
      return [port.offset, height];
    case "left":
      return [0, port.offset];
    case "right":
      return [width, port.offset];
  }
}

function stubTip(port: ResolvedSymbol["ports"][number], lx: number, ly: number): [number, number] {
  switch (port.side) {
    case "top":
      return [lx, ly - STUB];
    case "bottom":
      return [lx, ly + STUB];
    case "left":
      return [lx - STUB, ly];
    case "right":
      return [lx + STUB, ly];
  }
}

/** Same-net wire *endpoints* (not interior polyline vertices) that coincide 3+ times -- a reasonable T-junction approximation of geometry.rs `wire_junction_points` for this app's simplified wire model. */
function junctionPoints(wires: SchematicWire[]): Array<[number, number]> {
  const counts = new Map<string, { at: [number, number]; n: number }>();
  for (const w of wires) {
    if (w.pts.length === 0) continue;
    for (const p of [w.pts[0]!, w.pts[w.pts.length - 1]!]) {
      const key = `${w.net}|${p[0]},${p[1]}`;
      const e = counts.get(key);
      if (e) e.n++;
      else counts.set(key, { at: p, n: 1 });
    }
  }
  return [...counts.values()].filter((e) => e.n >= 3).map((e) => e.at);
}

function drawSymbol(ctx: CanvasRenderingContext2D, view: ViewTransform, r: ResolvedSymbol, selected: boolean) {
  const { symbol, width, height, ports, pinPort, passive } = r;
  ctx.save();
  ctx.translate(symbol.at[0], symbol.at[1]);
  if (symbol.rot !== 0) ctx.rotate((symbol.rot * Math.PI) / 180);
  if (symbol.mirrored) ctx.scale(-1, 1);

  const hair = 1 / view.scale;
  ctx.lineWidth = Math.max(300, hair);

  if (passive) {
    ctx.strokeStyle = layerColor("LAYER_DEVICE");
    drawPassiveGlyph(ctx, passive, width, height);
  } else {
    ctx.fillStyle = layerColor("LAYER_DEVICE_BACKGROUND");
    ctx.strokeStyle = layerColor("LAYER_DEVICE");
    ctx.lineWidth = Math.max(400, hair);
    ctx.fillRect(0, 0, width, height);
    ctx.strokeRect(0, 0, width, height);
  }

  if (selected) {
    ctx.save();
    ctx.strokeStyle = layerColor("LAYER_SELECTION_SHADOWS");
    ctx.lineWidth = Math.max(800, hair * 3);
    ctx.strokeRect(-200, -200, width + 400, height + 400);
    ctx.restore();
  }

  // Pins.
  ctx.lineWidth = Math.max(150, hair);
  ctx.strokeStyle = layerColor("LAYER_PIN");
  ports.forEach((port, portIdx) => {
    const [lx, ly] = localPortPoint(port, width, height);
    const [sx, sy] = stubTip(port, lx, ly);
    const pinI = pinPort.indexOf(portIdx);
    const pin = pinI >= 0 ? symbol.pins[pinI] : undefined;
    ctx.beginPath();
    ctx.moveTo(lx, ly);
    ctx.lineTo(sx, sy);
    ctx.stroke();
    if (pin) {
      const pad = 200;
      // Pin name: just inside the body edge (KiCad shows this and the
      // number simultaneously by default -- there's no "hide names"
      // toggle in this app's model -- each in its own real KiCad color,
      // LAYER_PINNAM/LAYER_PINNUM, not one falling back to the other).
      if (pin.name) {
        ctx.save();
        ctx.font = `${PIN_FONT * 1000}px sans-serif`;
        ctx.fillStyle = layerColor("LAYER_PINNAM");
        if (port.side === "left") {
          ctx.textAlign = "start";
          ctx.textBaseline = "middle";
          ctx.fillText(pin.name, lx + pad, ly);
        } else if (port.side === "right") {
          ctx.textAlign = "end";
          ctx.textBaseline = "middle";
          ctx.fillText(pin.name, lx - pad, ly);
        } else {
          ctx.textAlign = "center";
          ctx.textBaseline = port.side === "top" ? "bottom" : "top";
          ctx.fillText(pin.name, lx, ly + (port.side === "top" ? -pad : pad));
        }
        ctx.restore();
      }
      // Pin number: along the stub itself, at its midpoint, offset to
      // sit just above the line (below it for a top-side stub, which
      // points the opposite way) rather than on top of it.
      if (pin.number) {
        ctx.save();
        ctx.font = `${PIN_FONT * 0.85 * 1000}px sans-serif`;
        ctx.fillStyle = layerColor("LAYER_PINNUM");
        const mx = (lx + sx) / 2;
        const my = (ly + sy) / 2;
        if (port.side === "left" || port.side === "right") {
          ctx.textAlign = "center";
          ctx.textBaseline = "bottom";
          ctx.fillText(pin.number, mx, my - pad * 0.5);
        } else {
          ctx.textAlign = port.side === "top" ? "end" : "start";
          ctx.textBaseline = "middle";
          ctx.fillText(pin.number, mx + (port.side === "top" ? -pad * 0.5 : pad * 0.5), my);
        }
        ctx.restore();
      }
      // No-connect: a small blue X centered on the stub's free end --
      // LAYER_NOCONNECT, KiCad's real color for this marker.
      if (pin.kind === "nc") {
        ctx.save();
        ctx.strokeStyle = layerColor("LAYER_NOCONNECT");
        ctx.lineWidth = Math.max(150, hair);
        const s = 300;
        ctx.beginPath();
        ctx.moveTo(sx - s, sy - s);
        ctx.lineTo(sx + s, sy + s);
        ctx.moveTo(sx - s, sy + s);
        ctx.lineTo(sx + s, sy - s);
        ctx.stroke();
        ctx.restore();
      }
    }
  });

  // Ref/value, above/below the box's left edge.
  ctx.save();
  ctx.textAlign = "start";
  ctx.textBaseline = "alphabetic";
  ctx.fillStyle = layerColor("LAYER_REFERENCEPART");
  ctx.font = `bold ${REF_FONT * 1000}px sans-serif`;
  ctx.fillText(symbol.id, 0, -400);
  if (symbol.value || symbol.mpn) {
    ctx.fillStyle = layerColor("LAYER_VALUEPART");
    ctx.font = `${VALUE_FONT * 1000}px sans-serif`;
    ctx.fillText(symbol.value ?? symbol.mpn ?? "", 0, height + 1800);
  }
  // Footprint field: small, LAYER_FIELDS purple -- a real field KiCad
  // draws alongside ref/value (this app has no separate "hide field"
  // flag per field, so it always shows when the symbol has a package).
  if (symbol.package) {
    ctx.fillStyle = layerColor("LAYER_FIELDS");
    ctx.font = `${FIELD_FONT * 1000}px sans-serif`;
    ctx.fillText(symbol.package, 0, height + 1800 + FIELD_FONT * 1150);
  }
  ctx.restore();

  ctx.restore();
}

function drawPassiveGlyph(ctx: CanvasRenderingContext2D, kind: NonNullable<ResolvedSymbol["passive"]>, width: number, height: number) {
  const cy = height / 2;
  const lead = width * 0.32;
  const far = width - lead;
  ctx.beginPath();
  switch (kind) {
    case "resistor": {
      const bodyHalf = height * 0.18;
      ctx.moveTo(0, cy);
      ctx.lineTo(lead, cy);
      ctx.moveTo(far, cy);
      ctx.lineTo(width, cy);
      ctx.stroke();
      ctx.strokeRect(lead, cy - bodyHalf, far - lead, bodyHalf * 2);
      return;
    }
    case "capacitor": {
      const gap = width * 0.06;
      const plateHalf = height * 0.28;
      ctx.moveTo(0, cy);
      ctx.lineTo(width / 2 - gap, cy);
      ctx.moveTo(width / 2 - gap, cy - plateHalf);
      ctx.lineTo(width / 2 - gap, cy + plateHalf);
      ctx.moveTo(width / 2 + gap, cy - plateHalf);
      ctx.lineTo(width / 2 + gap, cy + plateHalf);
      ctx.moveTo(width / 2 + gap, cy);
      ctx.lineTo(width, cy);
      ctx.stroke();
      return;
    }
    case "inductor": {
      ctx.moveTo(0, cy);
      ctx.lineTo(lead, cy);
      const bumpR = (far - lead) / 6;
      for (let i = 0; i < 3; i++) {
        const c = lead + bumpR * (2 * i + 1);
        ctx.moveTo(c - bumpR, cy);
        ctx.arc(c, cy, bumpR, Math.PI, 0, false);
      }
      ctx.moveTo(far, cy);
      ctx.lineTo(width, cy);
      ctx.stroke();
      return;
    }
    case "diode": {
      const triHalf = height * 0.22;
      ctx.moveTo(0, cy);
      ctx.lineTo(lead, cy);
      ctx.moveTo(far, cy);
      ctx.lineTo(width, cy);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(lead, cy - triHalf);
      ctx.lineTo(lead, cy + triHalf);
      ctx.lineTo(far, cy);
      ctx.closePath();
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(far, cy - triHalf);
      ctx.lineTo(far, cy + triHalf);
      ctx.stroke();
      return;
    }
  }
}

// Background is filled once in screen-space by the caller, before the
// world transform is applied (see canvas/Canvas.tsx's PCB equivalent) --
// not here, which would mean computing an inverse-transformed rect on
// every repaint for no benefit.
export function paintSchematic(ctx: CanvasRenderingContext2D, view: ViewTransform, sch: Schematic, opts: SchematicPaintOptions): void {
  const hair = 1 / view.scale;

  // Wires.
  for (const w of sch.wires) {
    const on = opts.netHighlight === w.net;
    ctx.strokeStyle = on ? layerColor("LAYER_SELECTION_SHADOWS") : layerColor("LAYER_WIRE");
    ctx.lineWidth = Math.max(on ? 300 : 150, hair * (on ? 2.5 : 1));
    ctx.beginPath();
    w.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.stroke();
  }

  // Junctions.
  ctx.fillStyle = layerColor("LAYER_JUNCTION");
  for (const [x, y] of junctionPoints(sch.wires)) {
    ctx.beginPath();
    ctx.arc(x, y, Math.max(250, hair * 2), 0, Math.PI * 2);
    ctx.fill();
  }

  // Net labels: a small tag at the anchor, colored by rail-vs-signal.
  for (const l of sch.labels) {
    const on = opts.netHighlight === l.net;
    ctx.save();
    ctx.fillStyle = on ? layerColor("LAYER_SELECTION_SHADOWS") : labelColor(l.net);
    ctx.font = `italic ${1.3 * 1000}px sans-serif`;
    ctx.textAlign = "start";
    ctx.textBaseline = "middle";
    ctx.fillText(l.net, l.at[0] + 300, l.at[1]);
    ctx.beginPath();
    ctx.arc(l.at[0], l.at[1], Math.max(150, hair), 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  // Symbols (drawn last, like eda-render, so their fill sits on top of any wire stub reaching into the box).
  for (const s of sch.symbols) {
    const r = resolveSymbol(s);
    const selected = opts.selection.has(s.id);
    drawSymbol(ctx, view, r, selected);
  }
}
