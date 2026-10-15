// What snapping looks at in the Symbol Editor: the pins and the texts of the unit and body style being edited, as the schematic's grid helper
// (kicad-port/schGridHelper.ts, `EE_GRID_HELPER`) holds the items of a sheet. In KiCad the symbol editor runs the same helper over the library symbol's items
// (`EE_GRID_HELPER::queryVisible`: "If we are in the symbol editor, don't use the symbol itself"), and `computeAnchors` gives
//   * a pin (`SCH_PIN_T`): its position -- the end where it connects -- as `SNAPPABLE | ORIGIN`;
//   * a text (`SCH_TEXT_T`) or a field (`SCH_FIELD_T`): its anchor as `ORIGIN`, only when picking items up and none of them is connectable (`aIncludeText`);
//   * a shape (`SCH_SHAPE_T`): nothing -- a rectangle, a circle, an arc or a polyline has no anchors.
// So the anchors a click can snap to are the other pins' ends; the grid decides unless it is off (Ctrl) or coarse, exactly as on a sheet.
//
// Coordinates are the editor's own: internal micrometres, +y down (the symbol's mm, +y up, as `libPointToInternalUm` turns them). A hidden pin counts only while "Show Hidden Pins"
// is on (`pinShown`). Pure: no React, no DOM. Unit-tested in symSnapItems.test.ts.
import type { LibrarySymbol, SchField } from "../api/types";
import { libPointToInternalUm } from "../components/schematic/transform";
import type { SchSnapItem } from "./schGridHelper";
import { box } from "./snapGeom";
import { pinShown } from "./symPinText";

const visibleHere = (itemUnit: number, itemBody: number, unit: number, body: number): boolean => (itemUnit === 0 || itemUnit === unit) && (itemBody === 0 || itemBody === body);

export function symbolSnapItems(sym: LibrarySymbol | null, unit: number, bodyStyle: number, showHiddenPins: boolean, fields: readonly SchField[] = []): SchSnapItem[] {
  if (!sym) return [];
  const out: SchSnapItem[] = [];
  sym.pins.forEach((p, i) => {
    if (!pinShown(p.hidden, showHiddenPins) || !visibleHere(p.unit, p.body_style, unit, bodyStyle)) return;
    const id = p.id ?? `pin#${i}`;
    const tip = libPointToInternalUm([p.at.x, p.at.y]);
    const reach = Math.max(p.length_mm * 1000, 1);
    out.push({ id, owner: id, kind: "pin", connectable: true, position: tip, connectionPoints: [], bbox: box(tip[0] - reach, tip[1] - reach, tip[0] + reach, tip[1] + reach) });
  });
  sym.graphics.forEach((g, i) => {
    if (g.kind !== "text" || !visibleHere(g.unit, g.body_style, unit, bodyStyle)) return;
    const id = g.id ?? `text#${i}`;
    const at = libPointToInternalUm([g.at.x, g.at.y]);
    const r = Math.max(g.size_mm * 1000, (g.size_mm * 1000 * g.text.length) / 2);
    out.push({ id, owner: id, kind: "text", connectable: false, position: at, connectionPoints: [], bbox: box(at[0] - r, at[1] - r, at[0] + r, at[1] + r) });
  });
  // the fields on show (`SCH_FIELD_T`): like a text, an anchor to hold a move by
  for (const f of fields) {
    if (!f.id) continue;
    const size = f.size_um && f.size_um > 0 ? f.size_um : 1270;
    const r = Math.max(size, (size * f.text.length) / 2);
    out.push({ id: f.id, owner: f.id, kind: "field", connectable: false, position: [f.at[0], f.at[1]], connectionPoints: [], bbox: box(f.at[0] - r, f.at[1] - r, f.at[0] + r, f.at[1] + r) });
  }
  return out;
}
