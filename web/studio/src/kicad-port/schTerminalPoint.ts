// `SCH_SCREEN::IsTerminalPoint` (eeschema/sch_screen.cpp, 8303b2ad), which the wire, bus and line tools ask at every click: "Terminate the command if
// the end point is on a pin, junction, label, or another wire or bus" (`SCH_LINE_WIRE_BUS_TOOL::doDrawSegments`: `if( screen->IsTerminalPoint(
// cursorPos, segment->GetLayer() ) ) finishSegments( aCommit );`), r3 C3-M3. By the layer of the segment being drawn:
//  * a wire (`LAYER_WIRE`): a bus entry, a junction, a pin, a wire (its end or its middle), a label that is connected, a sheet pin that is connected;
//  * a bus (`LAYER_BUS`): a bus, a sheet pin or a label that is connected -- never a pin;
//  * a graphic line (`LAYER_NOTES`): another graphic line.
// A label or sheet pin is connected when a wire, a bus or a pin of the sheet is at its point (`IsConnected( aPosition )`, the items the connection map gives it).
import type { Schematic } from "../api/types";

export type TerminalLayer = "wire" | "bus" | "notes";

type Pt = readonly [number, number];

/** `p` on the segment `a`-`b` (its ends included): `SCH_LINE::HitTest( aPosition, 0 )` of a horizontal, vertical or slanted line. */
function onSegment(a: Pt, b: Pt, p: Pt): boolean {
  const cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
  if (cross !== 0) return false;
  return Math.min(a[0], b[0]) <= p[0] && p[0] <= Math.max(a[0], b[0]) && Math.min(a[1], b[1]) <= p[1] && p[1] <= Math.max(a[1], b[1]);
}

function onPolyline(pts: readonly Pt[], p: Pt): boolean {
  for (let i = 0; i + 1 < pts.length; i++) if (onSegment(pts[i]!, pts[i + 1]!, p)) return true;
  return false;
}

const same = (a: Pt, b: Pt) => a[0] === b[0] && a[1] === b[1];

/** Is `p` a point the tool drawing on `layer` finishes at? `pins` are the sheet's pin tips. */
export function isTerminalPoint(sch: Pick<Schematic, "wires" | "labels" | "bus_entries" | "junctions" | "sheets" | "lines">, pins: readonly Pt[], p: Pt, layer: TerminalLayer): boolean {
  const onWire = (bus: boolean) => sch.wires.some((w) => w.bus === bus && onPolyline(w.pts as Pt[], p));
  // `SCH_ITEM::IsConnected( aPosition )`: any item the connection map joins there -- a wire or bus, or a pin (r4 C4-L3: a label put on a pin end is connected)
  const reached = () => sch.wires.some((w) => onPolyline(w.pts as Pt[], p)) || pins.some((q) => same(q, p));
  const sheetPinConnected = () => sch.sheets.some((s) => s.pins.some((sp) => same(sp.at, p))) && reached();
  const labelConnected = () => sch.labels.some((l) => same(l.at, p)) && reached();
  switch (layer) {
    case "bus":
      return onWire(true) || sheetPinConnected() || labelConnected();
    case "notes":
      return (sch.lines ?? []).some((l) => onPolyline(l.pts, p));
    case "wire":
      return (
        sch.bus_entries.some((e) => onSegment(e.at, [e.at[0] + e.size[0], e.at[1] + e.size[1]], p)) ||
        (sch.junctions ?? []).some((j) => same(j.at, p)) ||
        pins.some((q) => same(q, p)) ||
        onWire(false) ||
        labelConnected() ||
        sheetPinConnected()
      );
  }
}
