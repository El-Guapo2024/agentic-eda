// What a schematic selection is made of, in the terms `SCH_SELECTION_TOOL`'s menu conditions use (kicad-port/schContextMenu.ts).
import type { Schematic } from "../../api/types";
import { emptySummary, type SchSelectionSummary } from "../../kicad-port/schContextMenu";

export function summarizeSelection(sch: Schematic, ids: readonly string[]): SchSelectionSummary {
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
    } else known = false;
    if (!known) continue;
    s.total++;
    if (locked.has(id)) s.locked++;
    else s.unlocked++;
  }
  // `GetSameSymbolMultiUnitSelection`: one reference selected, with several placed units.
  if (symbolIds.length === 1) s.sameReferenceUnits = symbolUnits.get(symbolIds[0]!) ?? 0;
  return s;
}
