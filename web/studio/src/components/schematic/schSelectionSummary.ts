// What a schematic selection is made of, in the terms `SCH_SELECTION_TOOL`'s menu conditions use (kicad-port/schContextMenu.ts).
import type { Schematic } from "../../api/types";
import { emptySummary, type SchSelectionSummary } from "../../kicad-port/schContextMenu";
import { canAddCorner, canRemoveCorner, type P, type PolyKind } from "../../kicad-port/schPolyCorners";
import { isFieldId } from "../../kicad-port/schFieldEdit";

/** The outline of a drawn polygon or rule area, or null for anything else. */
export function polygonOutline(sch: Schematic, id: string): { kind: PolyKind; pts: P[] } | null {
  const g = (sch.graphics ?? []).find((x) => x.id === id);
  if (g?.shape.type === "polygon") return { kind: "polygon", pts: g.shape.pts.map((p): P => [p.x, p.y]) };
  if (g?.shape.type === "rule_area") return { kind: "rule_area", pts: g.shape.pts.map((p): P => [p.x, p.y]) };
  return null;
}

/** `cursor` (um) and `tolUm` (the point editor's handle size at this zoom) decide the corner entries, which depend on where the pointer is. */
export function summarizeSelection(sch: Schematic, ids: readonly string[], cursor?: readonly [number, number], tolUm = 0): SchSelectionSummary {
  const s = emptySummary();
  const locked = new Set(sch.locked ?? []);
  const wire = new Map(sch.wires.map((w) => [w.id, w]));
  const label = new Map(sch.labels.map((l) => [l.id, l]));
  const text = new Set(sch.texts.map((t) => t.id));
  const power = new Set(sch.power_symbols.map((p) => p.id));
  const noConnect = new Set(sch.no_connects.map((n) => n.id));
  const busEntry = new Set(sch.bus_entries.map((b) => b.id));
  const junction = new Set((sch.junctions ?? []).map((j) => j.id));
  const line = new Set((sch.lines ?? []).map((l) => l.id));
  const graphic = new Map((sch.graphics ?? []).map((g) => [g.id, g]));
  const sheet = new Set(sch.sheets.map((x) => x.id));
  const symbolUnits = new Map<string, number>();
  for (const sym of sch.symbols) symbolUnits.set(sym.id, (symbolUnits.get(sym.id) ?? 0) + 1);

  const symbolIds: string[] = [];
  for (const id of ids) {
    let known = true;
    const w = wire.get(id);
    const l = label.get(id);
    const g = graphic.get(id);
    if (symbolUnits.has(id)) {
      s.symbols++;
      symbolIds.push(id);
    } else if (power.has(id)) s.powerSymbols++;
    else if (w) w.bus ? s.buses++ : s.wires++;
    else if (l) l.scope === "global" ? s.globalLabels++ : l.scope === "hierarchical" ? s.hierLabels++ : s.localLabels++;
    else if (text.has(id)) s.texts++;
    else if (noConnect.has(id)) s.noConnects++;
    else if (busEntry.has(id)) s.busEntries++;
    else if (junction.has(id)) s.junctions++;
    else if (line.has(id)) s.lines++;
    else if (sheet.has(id)) s.sheets++;
    else if (g) {
      if (g.shape.type === "text_box") s.textBoxes++;
      else if (g.shape.type === "directive") s.directiveLabels++;
      else if (g.shape.type === "rule_area") s.ruleAreas++;
      else s.shapes++;
    } else if (isFieldId(id)) s.fields++;
    else known = false;
    if (!known) continue;
    s.total++;
    if (locked.has(id)) s.locked++;
    else s.unlocked++;
  }
  // `GetSameSymbolMultiUnitSelection`: one reference selected, with several placed units.
  if (symbolIds.length === 1) s.sameReferenceUnits = symbolUnits.get(symbolIds[0]!) ?? 0;
  // The point editor's corner entries: one selected polygon or rule area, the pointer on its outline (create) or on one of its corners (remove).
  const poly = ids.length === 1 && cursor ? polygonOutline(sch, ids[0]!) : null;
  if (poly && cursor) {
    s.canAddCorner = canAddCorner(poly.pts, cursor, tolUm);
    s.canRemoveCorner = canRemoveCorner(poly.kind, poly.pts, cursor, tolUm);
  }
  // `SCH_SHEET::HasUndefinedPins` needs the sheet's own file (read when the action runs); a sheet with at least one pin may have an unreferenced one, so Cleanup Sheet Pins is offered for it.
  if (s.sheets === 1) {
    const sheet = sch.sheets.find((x) => ids.includes(x.id));
    s.sheetHasUndefinedPins = (sheet?.pins.length ?? 0) > 0;
  }
  return s;
}
