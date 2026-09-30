// Symbol box size + pin-side layout, ported from crates/engine/src/
// geometry.rs (node_size/build_ports and their helpers) so this app's own
// schematic renderer lays symbols out the same way the backend's SVG
// renderer does, instead of a second, independently-invented heuristic
// that could quietly drift from it. GET /api/schematic (studio.rs) only
// sends each symbol's raw pin list -- not a resolved box/port layout, to
// keep that endpoint small -- so this is where the resolution happens on
// this side. Ported line-for-line from the Rust source read this
// session (crates/engine/src/geometry.rs); the passive-glyph/ref/value
// text placement is deliberately left out (rendered simplified, see
// painter.ts) since only pin/box layout is needed to draw the symbol
// itself and route wires to it.
import type { SchematicPin, SchematicSymbol } from "../../api/types";

export const GRID = 1270; // 1.27mm, eda_layout::DEFAULT_GRID
export const BASE_WIDTH = 10_160; // 10.16mm, 8 * GRID
export const BASE_HEIGHT = 7_620; // 7.62mm, 6 * GRID
export const HEIGHT_STEP = 2_540; // 2.54mm per pair of pins beyond 4
export const STUB = 1_270; // pin stub length, um, drawn from the box edge outward
export const CHAR_WIDTH_FACTOR = 0.6;
export const PIN_FONT_MM = 1.1;
export const PIN_TEXT_MARGIN_MM = 0.8;

export type PortSide = "top" | "bottom" | "left" | "right";

export interface Port {
  side: PortSide;
  /** Offset (um) from the box's top-left corner, along `side`. */
  offset: number;
}

function isGroundName(name: string): boolean {
  const n = name.toUpperCase();
  return n.includes("GND") || n.includes("VSS") || n.includes("AGND");
}

export function isControlPinName(name: string): boolean {
  const n = name.toUpperCase();
  return ["EN", "CE", "SHDN", "RESET", "RST", "CS"].some((p) => n.includes(p));
}

/** True for a part drawn as a 2-pin passive glyph (R/C/L/D) rather than a generic IC box -- crates/engine/src/geometry.rs `is_two_pin_passive`. */
export function isTwoPinPassive(pins: SchematicPin[], reference: string, pkg: string | null, value: string | null): boolean {
  if (pins.length !== 2) return false;
  for (const c of [reference, pkg, value]) {
    if (!c) continue;
    const first = c.replace(/^[+-]/, "").charAt(0).toUpperCase();
    if (first === "R" || first === "C" || first === "L" || first === "D") return true;
  }
  return false;
}

export type PassiveKind = "resistor" | "capacitor" | "inductor" | "diode";

/** Which glyph, if any -- crates/render/src/lib.rs `passive_kind` (checked in the same ref/package/value order). */
export function passiveKind(pins: SchematicPin[], reference: string, pkg: string | null, value: string | null): PassiveKind | null {
  if (pins.length !== 2) return null;
  for (const c of [reference, pkg, value]) {
    if (!c) continue;
    const first = c.replace(/^[+-]/, "").charAt(0).toUpperCase();
    if (first === "R") return "resistor";
    if (first === "C") return "capacitor";
    if (first === "L") return "inductor";
    if (first === "D") return "diode";
  }
  return null;
}

/** Evenly spaced, grid-snapped offsets along a `length`-um edge -- geometry.rs `distribute_offsets`. */
export function distributeOffsets(n: number, length: number): number[] {
  if (n === 0) return [];
  const maxOff = Math.max(Math.floor(length / GRID) - 1, 1) * GRID;
  const out: number[] = [];
  for (let i = 0; i < n; i++) {
    const raw = (length * (i + 1)) / (n + 1);
    const snapped = Math.round((raw + GRID / 2) / GRID) * GRID;
    out.push(Math.min(Math.max(snapped, GRID), maxOff));
  }
  return out;
}

interface PortAssignment {
  ports: Port[];
  /** pin index -> port index, or null for an NC pin. */
  pinPort: (number | null)[];
}

/**
 * Side assignment is name+kind driven, not kind-alone: a Ground-kind pin
 * (or any pin named like GND/VSS/AGND) goes South; a Power-kind pin named
 * like an input/output goes West/East, any other Power-kind pin (a bare
 * rail) goes North; Signal/Passive pins follow the same IN/OUT
 * convention, then control-ish names (EN/CE/SHDN/RESET/CS) go West below
 * the named inputs; anything left over falls back to an even West/East
 * split -- geometry.rs `build_ports`, ported as-is.
 */
export function buildPorts(pins: SchematicPin[], width: number, height: number): PortAssignment {
  const north: number[] = [];
  const south: number[] = [];
  const westNamed: number[] = [];
  const westCtrl: number[] = [];
  const east: number[] = [];
  const sigpassFallback: number[] = [];

  pins.forEach((pin, i) => {
    const name = pin.name ?? "";
    const upper = name.toUpperCase();
    switch (pin.kind) {
      case "nc":
        break;
      case "ground":
        south.push(i);
        break;
      case "power":
        if (isGroundName(name)) south.push(i);
        else if (upper.includes("IN")) westNamed.push(i);
        else if (upper.includes("OUT")) east.push(i);
        else north.push(i);
        break;
      case "signal":
      case "passive":
        if (isGroundName(name)) south.push(i);
        else if (upper.includes("OUT")) east.push(i);
        else if (upper.includes("IN")) westNamed.push(i);
        else if (isControlPinName(name)) westCtrl.push(i);
        else sigpassFallback.push(i);
        break;
    }
  });

  const westCount = Math.floor(sigpassFallback.length / 2);
  const fbWest = sigpassFallback.slice(0, westCount);
  const fbEast = sigpassFallback.slice(westCount);

  const west = [...westNamed, ...westCtrl, ...fbWest];
  const eastAll = [...east, ...fbEast];

  const northOffsets = distributeOffsets(north.length, width);
  const southOffsets = distributeOffsets(south.length, width);
  const westOffsets = distributeOffsets(west.length, height);
  const eastOffsets = distributeOffsets(eastAll.length, height);

  const ports: Port[] = [];
  const pinPort: (number | null)[] = new Array(pins.length).fill(null);
  north.forEach((pinI, k) => {
    ports.push({ side: "top", offset: northOffsets[k]! });
    pinPort[pinI] = ports.length - 1;
  });
  south.forEach((pinI, k) => {
    ports.push({ side: "bottom", offset: southOffsets[k]! });
    pinPort[pinI] = ports.length - 1;
  });
  west.forEach((pinI, k) => {
    ports.push({ side: "left", offset: westOffsets[k]! });
    pinPort[pinI] = ports.length - 1;
  });
  eastAll.forEach((pinI, k) => {
    ports.push({ side: "right", offset: eastOffsets[k]! });
    pinPort[pinI] = ports.length - 1;
  });

  return { ports, pinPort };
}

/** Box size (width, height), um -- geometry.rs `node_size`. */
export function nodeSize(pins: SchematicPin[]): { width: number; height: number } {
  const extra = Math.max(pins.length - 4, 0);
  const pairs = Math.ceil(extra / 2);
  const height = BASE_HEIGHT + pairs * HEIGHT_STEP;

  // A throwaway BASE_WIDTH first pass tells us which pins land on which
  // side (side assignment never depends on the box's own width) before
  // the final width -- which depends on that assignment -- is known.
  const { ports, pinPort } = buildPorts(pins, BASE_WIDTH, height);
  const northNames: string[] = [];
  const southNames: string[] = [];
  const westNames: string[] = [];
  const eastNames: string[] = [];
  pinPort.forEach((portIdx, i) => {
    if (portIdx === null) return;
    const pin = pins[i]!;
    const name = pin.name ?? pin.number;
    switch (ports[portIdx]!.side) {
      case "top":
        northNames.push(name);
        break;
      case "bottom":
        southNames.push(name);
        break;
      case "left":
        westNames.push(name);
        break;
      case "right":
        eastNames.push(name);
        break;
    }
  });

  const nameWidthMm = (s: string) => s.length * CHAR_WIDTH_FACTOR * PIN_FONT_MM;
  const widthForRow = (names: string[]): number => {
    if (names.length < 2) return 0;
    let worstPitch = 0;
    for (let i = 0; i < names.length - 1; i++) {
      const pitch = (nameWidthMm(names[i]!) + nameWidthMm(names[i + 1]!)) / 2 + PIN_FONT_MM;
      worstPitch = Math.max(worstPitch, pitch);
    }
    return worstPitch * (names.length + 1);
  };
  const widthFromRows = Math.max(widthForRow(northNames), widthForRow(southNames));

  const longest = (names: string[]) => names.reduce((m, n) => Math.max(m, nameWidthMm(n)), 0);
  const westReach = westNames.length === 0 ? 0 : longest(westNames) + PIN_TEXT_MARGIN_MM;
  const eastReach = eastNames.length === 0 ? 0 : longest(eastNames) + PIN_TEXT_MARGIN_MM;
  const widthFromReach = westNames.length === 0 || eastNames.length === 0 ? 0 : westReach + eastReach + PIN_FONT_MM;

  const widthMm = Math.max(BASE_WIDTH / 1000, widthFromRows, widthFromReach);
  const widthUm = Math.ceil(widthMm * 1000);
  const width = Math.ceil(widthUm / GRID) * GRID;

  return { width, height };
}

export interface ResolvedSymbol {
  symbol: SchematicSymbol;
  width: number;
  height: number;
  ports: Port[];
  pinPort: (number | null)[];
  passive: PassiveKind | null;
}

export function resolveSymbol(symbol: SchematicSymbol): ResolvedSymbol {
  const { width, height } = nodeSize(symbol.pins);
  const { ports, pinPort } = buildPorts(symbol.pins, width, height);
  const passive = passiveKind(symbol.pins, symbol.id, symbol.package, symbol.value);
  return { symbol, width, height, ports, pinPort, passive };
}

