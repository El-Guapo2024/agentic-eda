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
import { drawStrokeText } from "../text/strokeFont";

/**
 * Canvas2D's own `textBaseline: "middle"` centers on the *font's* actual
 * ascent/descent metrics; stroke text has no such metric to ask for
 * (every glyph's y=0 already sits exactly on the baseline, per KiCad's
 * own FONT_OFFSET convention -- see strokeFont.ts), so vertical
 * centering here is an approximation: half of a typical cap-height
 * (measured off the real font data -- 'A' spans baseline to about -0.95
 * * size), nudged down a bit to not over-shoot for mixed-/lower-case
 * strings. Applied by shifting the anchor y before calling
 * drawStrokeText, since the function itself only knows baseline-left.
 */
const MIDDLE_OFFSET_FACTOR = 0.35;
/** A full-height glyph's baseline-to-top span, measured off the real font data (decodeGlyph('A') tops out at y ~= -0.95 * size) -- used to approximate Canvas2D's old textBaseline:"top"/"bottom" behavior now that stroke text only ever anchors at its own baseline. */
const CAP_HEIGHT = 0.95;
/** Stroke text has no filled outline to embolden -- a thicker stroke is KiCad's own way of drawing "bold" stroke-font text. */
const BOLD_THICKNESS_FACTOR = 1 / 5;

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
        const sizeUm = PIN_FONT * 1000;
        const color = layerColor("LAYER_PINNAM");
        if (port.side === "left") {
          drawStrokeText(ctx, pin.name, lx + pad, ly + sizeUm * MIDDLE_OFFSET_FACTOR, { sizeUm, justify: "left", color });
        } else if (port.side === "right") {
          drawStrokeText(ctx, pin.name, lx - pad, ly + sizeUm * MIDDLE_OFFSET_FACTOR, { sizeUm, justify: "right", color });
        } else {
          // top/bottom: text sits fully above (top-side pin) or fully
          // below (bottom-side pin) the stub point. Baseline sits close
          // to a glyph's visual bottom (pin names rarely have
          // descenders), so anchoring the baseline directly at the
          // pad point approximates "text bottom here, growing up" for a
          // top-side pin; for a bottom-side pin the baseline is pushed
          // down by one cap-height (CAP_HEIGHT) so the text's *top*
          // lands at the pad point instead.
          const y = port.side === "top" ? ly - pad : ly + pad + sizeUm * CAP_HEIGHT;
          drawStrokeText(ctx, pin.name, lx, y, { sizeUm, justify: "center", color });
        }
      }
      // Pin number: along the stub itself, at its midpoint, offset to
      // sit just above the line (below it for a top-side stub, which
      // points the opposite way) rather than on top of it.
      if (pin.number) {
        const sizeUm = PIN_FONT * 0.85 * 1000;
        const color = layerColor("LAYER_PINNUM");
        const mx = (lx + sx) / 2;
        const my = (ly + sy) / 2;
        if (port.side === "left" || port.side === "right") {
          // Sits just above the stub line (bottom-anchored -- see CAP_HEIGHT's comment).
          drawStrokeText(ctx, pin.number, mx, my - pad * 0.5, { sizeUm, justify: "center", color });
        } else {
          const justify = port.side === "top" ? "right" : "left";
          drawStrokeText(ctx, pin.number, mx + (port.side === "top" ? -pad * 0.5 : pad * 0.5), my + sizeUm * MIDDLE_OFFSET_FACTOR, { sizeUm, justify, color });
        }
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

  // Ref/value, above/below the box's left edge. Stroke text has no
  // "bold" variant (there's no filled outline to embolden) -- KiCad's
  // own way of making stroke text heavier is a thicker stroke, so the
  // reference designator (the one field this app already drew bold)
  // gets BOLD_THICKNESS_FACTOR instead of the default.
  const refSizeUm = REF_FONT * 1000;
  drawStrokeText(ctx, symbol.id, 0, -400, { sizeUm: refSizeUm, thicknessUm: refSizeUm * BOLD_THICKNESS_FACTOR, color: layerColor("LAYER_REFERENCEPART") });
  if (symbol.value || symbol.mpn) {
    const sizeUm = VALUE_FONT * 1000;
    drawStrokeText(ctx, symbol.value ?? symbol.mpn ?? "", 0, height + 1800, { sizeUm, color: layerColor("LAYER_VALUEPART") });
  }
  // Footprint field: small, LAYER_FIELDS purple -- a real field KiCad
  // draws alongside ref/value (this app has no separate "hide field"
  // flag per field, so it always shows when the symbol has a package).
  if (symbol.package) {
    const sizeUm = FIELD_FONT * 1000;
    drawStrokeText(ctx, symbol.package, 0, height + 1800 + FIELD_FONT * 1150, { sizeUm, color: layerColor("LAYER_FIELDS") });
  }

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
    const color = on ? layerColor("LAYER_SELECTION_SHADOWS") : labelColor(l.net);
    const sizeUm = 1.3 * 1000;
    drawStrokeText(ctx, l.net, l.at[0] + 300, l.at[1] + sizeUm * MIDDLE_OFFSET_FACTOR, { sizeUm, italic: true, color });
    ctx.save();
    ctx.fillStyle = color;
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
