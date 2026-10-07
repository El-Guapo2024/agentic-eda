// Swap Unit Labels -- `SCH_EDIT_TOOL::SwapUnitLabels` with `findSingleNetLabelForPin` (eeschema/tools/sch_edit_tool.cpp at 8303b2ad): the units of one multi-unit
// reference trade the net labels on their pins. Each pin must have exactly one net label on its connection and no other pin there; the labels of each unit are
// ordered by pin position (x, then y), and for every pin index the label texts are passed one unit along, the last unit's text going to the first.
import { expandWithStop, type ConnLabel, type ConnWire } from "./schConnection";

export type P = readonly [number, number];

export interface SwapSheet {
  wires: readonly ConnWire[];
  labels: ReadonlyArray<ConnLabel & { net: string }>;
  /** Every pin tip on the sheet (and power-symbol anchor), the pin being asked about included. */
  pinPoints: readonly P[];
}

const eq = (a: P, b: P) => a[0] === b[0] && a[1] === b[1];

function onSegment(p: P, a: P, b: P): boolean {
  if ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) !== 0) return false;
  return p[0] >= Math.min(a[0], b[0]) && p[0] <= Math.max(a[0], b[0]) && p[1] >= Math.min(a[1], b[1]) && p[1] <= Math.max(a[1], b[1]);
}

const onWire = (w: ConnWire, p: P): boolean => (w.pts.length === 1 ? eq(w.pts[0]!, p) : w.pts.some((q, i) => i + 1 < w.pts.length && onSegment(p, q, w.pts[i + 1]!)));

/**
 * `findSingleNetLabelForPin`: the id of the one net label on the connection of the pin at `tip` -- its wires, and a label right on the pin -- or null when the
 * connection has no label, more than one, or reaches another pin.
 */
export function singleNetLabelForPin(sheet: SwapSheet, tip: P): string | null {
  const start = sheet.wires.filter((w) => onWire(w, tip)).map((w) => w.id);
  const { wireIds, labelIds } = start.length > 0 ? expandWithStop({ wires: sheet.wires, labels: sheet.labels, pinPoints: sheet.pinPoints }, start, "never") : { wireIds: new Set<string>(), labelIds: new Set<string>() };
  for (const l of sheet.labels) if (eq(l.at, tip)) labelIds.add(l.id);
  const reached = sheet.wires.filter((w) => wireIds.has(w.id));
  const pins = sheet.pinPoints.filter((q) => eq(q, tip) || reached.some((w) => onWire(w, q)));
  if (pins.length !== 1 || labelIds.size !== 1) return null;
  return [...labelIds][0]!;
}

export interface UnitPins {
  unit: number;
  tips: readonly P[];
}

export type SwapResult = { ok: true; edits: Array<{ id: string; net: string }> } | { ok: false; message: string };

const FAIL = "Each pin of selected units must have exactly one attached net label and no other pin connections.";

/** The label texts after the swap, as the label edits that make it: `[ { id, net: new text } ]`, nothing for a label whose text does not change. */
export function swapUnitLabels(sheet: SwapSheet, units: readonly UnitPins[]): SwapResult {
  if (units.length < 2) return { ok: false, message: "Select a symbol with several units placed." };
  const ordered = [...units].sort((a, b) => a.unit - b.unit);
  const vectors: string[][] = [];
  for (const u of ordered) {
    const byPos = [...u.tips].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
    const ids: string[] = [];
    for (const tip of byPos) {
      const id = singleNetLabelForPin(sheet, tip);
      if (id === null) return { ok: false, message: FAIL };
      ids.push(id);
    }
    vectors.push(ids);
  }
  if (vectors.some((v) => v.length !== vectors[0]!.length)) return { ok: false, message: "The selected units have different pin counts." };
  const net = new Map(sheet.labels.map((l) => [l.id, l.net]));
  const edits: Array<{ id: string; net: string }> = [];
  for (let pin = 0; pin < vectors[0]!.length; pin++) {
    // "carry = last unit's text; for each unit: swap( its text, carry )".
    let carry = net.get(vectors[vectors.length - 1]![pin]!)!;
    for (const v of vectors) {
      const id = v[pin]!;
      const next = net.get(id)!;
      if (next !== carry) edits.push({ id, net: carry });
      carry = next;
    }
  }
  return { ok: true, edits };
}
