// The operating point on the schematic (View > Show OP Voltages / Show OP Currents; commit 8303b2ad):
//
//   eeschema/sch_edit_frame.cpp  SCH_EDIT_FRAME::RefreshOperatingPointDisplay: a net's voltage on its longest wire, a two-pin symbol's current on its
//                                first pin, every other simulated symbol's currents on the pins its model names (`<item>:<model pin>`)
//   eeschema/schematic.cpp       SCHEMATIC::GetOperatingPoint: the name lower-cased and made SPICE's, ground none, `SPICE_VALUE( v ).ToString( precision, range )`
//                                with the project's OP formats (`OpFormats`; 3 digits, "~V" and "~A" by default)
//   eeschema/sch_painter.cpp     SCH_PAINTER::draw( SCH_LINE ) / draw( SCH_PIN ) on LAYER_OP_VOLTAGES / LAYER_OP_CURRENTS, getOperatingPointTextSize
//
// The values are those of the last operating-point run (state/simulator.ts `opPoints`, keyed as `SCHEMATIC::SetOperatingPoint` keys them); a net's
// SPICE name comes from the circuit (`pin_nets`: kicad-cli's XML netlist) through a pin on it, else from its own name. A net drawn on several wires of a
// sheet in pieces gets one label, on its longest wire (KiCad labels each connected piece, `CONNECTION_SUBGRAPH`). Currents go per symbol (unit).
import { SpiceValue } from "./spiceValue";
import { spiceNetName } from "./simNames";
import type { Circuit } from "./simSignals";

export interface OpLabel {
  text: string;
  x: number;
  y: number;
  /** The text runs up the sheet (a horizontal pin's current). */
  vertical: boolean;
  h: "left" | "center";
  v: "center" | "bottom";
  kind: "voltage" | "current";
}

export interface OpWire {
  net: string;
  pins: string[];
  pts: [number, number][];
  bus?: boolean;
}

export interface OpPin {
  reference: string;
  /** The symbol (one unit of a placed part) the pin is on: `RefreshOperatingPointDisplay` works per `SCH_SYMBOL`. Absent: the reference. */
  symbol?: string;
  number: string;
  tip: [number, number];
  root: [number, number];
  net: string | null;
}

/** The project's OP formats (`SCHEMATIC_SETTINGS`: `drawing.operating_point_overlay_v_precision` / `_v_range` / `_i_precision` / `_i_range`, Schematic Setup >
 * Formatting), at their defaults. The studio's project does not carry them yet, so the overlay uses these. */
export interface OpFormats {
  vPrecision: number;
  vRange: string;
  iPrecision: number;
  iRange: string;
}

export const DEFAULT_OP_FORMATS: OpFormats = { vPrecision: 3, vRange: "~V", iPrecision: 3, iRange: "~A" };

/** `SCHEMATIC::GetOperatingPoint`: "" for ground, "--" without a run, "?" for a name the run has not. */
export function operatingPoint(points: Map<string, number>, name: string, precision: number, range: string): string {
  const key = spiceNetName(name.toLowerCase());
  if (key === "gnd" || key === "0") return "";
  const v = points.get(key);
  if (v !== undefined) return SpiceValue.of(v).toFormatted({ precision, range });
  return points.size === 0 ? "--" : "?";
}

/** `SCH_PAINTER::getOperatingPointTextSize`: two thirds 7 pixels, one third 50 mils, in micrometres at `scale` pixels per micrometre. */
export function opTextSize(scale: number): number {
  const doc = 50 * 25.4;
  const screen = Math.abs(7 / (scale || 1));
  return Math.round((doc + screen * 2) / 3.0);
}

const usable = (op: string) => op !== "" && op !== "--" && op !== "?";

/** The labels of the operating point on a sheet. */
export function opLabels(args: { wires: OpWire[]; pins: OpPin[]; circuit: Circuit | null; points: Map<string, number>; voltages: boolean; currents: boolean; scale: number; formats?: OpFormats }): OpLabel[] {
  const { wires, pins, circuit, points, scale } = args;
  const fmt = args.formats ?? DEFAULT_OP_FORMATS;
  const out: OpLabel[] = [];
  if (points.size === 0 || !circuit) return out;
  const size = opTextSize(scale);
  const offset = Math.round(size * 0.22);
  if (args.voltages) {
    // the SPICE net of each of the sheet's nets, through a pin on it
    const spiceOf = new Map<string, string>();
    for (const w of wires) for (const p of w.pins) if (!spiceOf.has(w.net) && circuit.pin_nets[p]) spiceOf.set(w.net, circuit.pin_nets[p]!);
    for (const p of pins) if (p.net && !spiceOf.has(p.net) && circuit.pin_nets[`${p.reference}.${p.number}`]) spiceOf.set(p.net, circuit.pin_nets[`${p.reference}.${p.number}`]!);
    const longest = new Map<string, { len: number; a: [number, number]; b: [number, number] }>();
    for (const w of wires) {
      if (w.bus) continue;
      for (let i = 0; i + 1 < w.pts.length; i++) {
        const a = w.pts[i]!;
        const b = w.pts[i + 1]!;
        const len = Math.hypot(b[0] - a[0], b[1] - a[1]);
        const cur = longest.get(w.net);
        if (!cur || len > cur.len) longest.set(w.net, { len, a, b });
      }
    }
    for (const [net, seg] of longest) {
      // KiCad asks every net of the connection graph by its name (`GetNetMap()`, `GetOperatingPoint( key.Name, .. )`); a pin's SPICE net comes first here, the
      // sheet's net name otherwise (a net that joins only labels).
      const name = spiceOf.get(net) ?? net;
      const op = operatingPoint(points, name, fmt.vPrecision, fmt.vRange);
      if (!usable(op)) continue;
      const mid: [number, number] = [Math.trunc((seg.a[0] + seg.b[0]) / 2), Math.trunc((seg.a[1] + seg.b[1]) / 2)];
      if (seg.a[1] === seg.b[1]) out.push({ text: op, x: mid[0], y: mid[1] - offset, vertical: false, h: "center", v: "bottom", kind: "voltage" });
      else out.push({ text: op, x: mid[0] + Math.round(offset * 1.2), y: mid[1], vertical: false, h: "left", v: "center", kind: "voltage" });
    }
  }
  if (args.currents) {
    // per symbol (`for( SCH_ITEM* item : GetScreen()->Items() ) .. symbol->GetPins( &sheetPath )`): each unit of a part on its own
    const bySymbol = new Map<string, { ref: string; pins: OpPin[] }>();
    for (const p of pins) {
      if (p.reference.startsWith("#")) continue;
      const key = p.symbol ?? p.reference;
      const g = bySymbol.get(key) ?? { ref: p.reference, pins: [] };
      g.pins.push(p);
      bySymbol.set(key, g);
    }
    for (const { ref, pins: ps } of bySymbol.values()) {
      const item = circuit.items.find((i) => i.ref === ref);
      if (!item) continue;
      const put = (pin: OpPin | undefined, op: string) => {
        if (!pin || !usable(op)) return;
        const len = Math.hypot(pin.tip[0] - pin.root[0], pin.tip[1] - pin.root[1]);
        if (len <= size) return;
        const mid: [number, number] = [Math.trunc((pin.root[0] + pin.tip[0]) / 2), Math.trunc((pin.root[1] + pin.tip[1]) / 2)];
        const dirX = Math.sign(pin.tip[0] - pin.root[0]);
        if (dirX === 0) out.push({ text: op, x: mid[0] + Math.round(offset * 1.2), y: mid[1], vertical: false, h: "left", v: "center", kind: "current" });
        else out.push({ text: op, x: mid[0], y: mid[1] - Math.round(offset * 1.2), vertical: true, h: "left", v: "center", kind: "current" });
      };
      if (ps.length === 2) {
        put(ps[0], operatingPoint(points, ref, fmt.iPrecision, fmt.iRange));
      } else {
        // the model's pins: the item's current names are `I(<item>:<model pin>)` in the model's order, a symbol pin's index says which
        for (const pin of ps) {
          const idx = item.pin_index[pin.number];
          const cur = idx !== undefined ? item.currents[idx] : undefined;
          const m = cur ? /^I\((.*)\)$/.exec(cur) : null;
          if (m) put(pin, operatingPoint(points, m[1]!, fmt.iPrecision, fmt.iRange));
        }
      }
    }
  }
  return out;
}
