// Preferences > PCB Editor > Editing Options > "Automatically refill zones" (UI-LEFT-2 PP10): KiCad 8303b2ad's `PCBNEW_SETTINGS::m_AutoRefillZones`
// (`editing.auto_fill_zones`, off by default; pcbnew/pcbnew_settings.cpp, pcbnew/dialogs/panel_edit_options.cpp). With it on, `BOARD_COMMIT::Push` (pcbnew/board_commit.cpp)
// marks the zones an edit damaged (`propagateDamage`, for every changed item as it was and as it is: the zone itself when it is one; else every zone that is not a
// rule area whose layers share the item's copper layers -- every physical layer for an item on Edge.Cuts or Margin -- and whose box, as it was before the commit
// (`CacheBoundingBox`), meets the item's) and posts `zoneFillDirty`, which fills those and every zone not filled yet (`ZONE_FILLER_TOOL::ZoneFillDirty`:
// `!zone->IsFilled() || m_dirtyZoneIDs.count( zone->m_Uuid )`). The fill here is kicad-cli's (`POST /api/fill/kicad` with the zones), run by the store after each
// command of the board (`runCmd`), as the commit's own `ZONE_FILL_OP` is not one.
//
// Differs: the changed items are found by comparing the board before and after the command (KiCad's commit lists them); an item's box is the studio's
// (kicad-port/itemBoxes.ts), a footprint's its courtyard or pads.
import type { BoardState } from "../api/types";
import { pcbItemBoxes, type Box } from "./itemBoxes";
import { isCopper } from "./layerPresets";
import { zoneLayers } from "./zoneLayers";

/** Every item of the board that can damage a fill, by id: what it is (its JSON, to tell a change) and the layers it is on. */
function itemsOf(board: BoardState): Map<string, { sig: string; layers: readonly string[] }> {
  const out = new Map<string, { sig: string; layers: readonly string[] }>();
  const copper = board.layers;
  const between = (a: string, b: string): string[] => {
    const i = copper.indexOf(a);
    const j = copper.indexOf(b);
    return i < 0 || j < 0 ? [...copper] : copper.slice(Math.min(i, j), Math.max(i, j) + 1);
  };
  for (const t of board.routing?.tracks ?? []) out.set(t.id, { sig: JSON.stringify(t), layers: [t.layer] });
  for (const v of board.routing?.vias ?? []) out.set(v.id, { sig: JSON.stringify(v), layers: between(v.from, v.to) });
  for (const z of board.routing?.zones ?? []) out.set(z.id, { sig: JSON.stringify(z), layers: zoneLayers(z) });
  for (const s of board.drawings?.shapes ?? []) out.set(s.id, { sig: JSON.stringify(s), layers: [s.layer] });
  for (const t of board.drawings?.texts ?? []) out.set(t.id, { sig: JSON.stringify(t), layers: [t.layer] });
  for (const d of board.drawings?.dimensions ?? []) out.set(d.id, { sig: JSON.stringify(d), layers: [d.layer] });
  for (const t of board.drawings?.text_boxes ?? []) out.set(t.id, { sig: JSON.stringify(t), layers: [t.layer] });
  for (const t of board.drawings?.tables ?? []) out.set(t.id, { sig: JSON.stringify(t), layers: [t.layer] });
  for (const p of board.parts) {
    if (!p.placed) continue;
    // A footprint's pads: a plated or drilled one on every copper layer, a surface one on its side's.
    const side = p.side === "bottom" ? "B.Cu" : "F.Cu";
    const layers = (p.pads ?? []).some((pad) => pad.th) ? [...copper] : [side];
    out.set(p.ref, { sig: JSON.stringify({ at: p.at, rot: p.rot, side: p.side, pads: p.pads }), layers });
  }
  return out;
}

const meets = (a: Box, b: Box): boolean => a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3];

/**
 * The zones `zoneFillDirty` fills after a command took the board from `before` to `after`: the ones the command damaged (`propagateDamage`) and the ones not filled
 * (`!zone->IsFilled()`: not in `filled`, the zones with a current fill), rule areas left out; ids of the board as it is now.
 */
export function zonesToRefill(before: BoardState, after: BoardState, filled: ReadonlySet<string>): string[] {
  const was = itemsOf(before);
  const now = itemsOf(after);
  const boxesBefore = pcbItemBoxes(before);
  const boxesAfter = pcbItemBoxes(after);
  const zones = (after.routing?.zones ?? []).filter((z) => !z.is_rule_area && z.outline.length >= 3);
  const stale = new Set<string>();
  const damage = (id: string, layers: readonly string[], box: Box | undefined): void => {
    if (zones.some((z) => z.id === id)) stale.add(id);
    if (!box) return;
    const all = layers.includes("Edge.Cuts") || layers.includes("Margin");
    const hit = all ? null : new Set(layers.filter(isCopper));
    if (hit && hit.size === 0) return;
    for (const z of zones) {
      // the zone's box as it was before the commit (`CacheBoundingBox`), else as it is
      const zb = boxesBefore.get(z.id) ?? boxesAfter.get(z.id);
      if (!zb || !meets(zb, box)) continue;
      if (hit && !zoneLayers(z).some((l) => hit.has(l))) continue;
      stale.add(z.id);
    }
  };
  for (const id of new Set([...was.keys(), ...now.keys()])) {
    const a = was.get(id);
    const b = now.get(id);
    if (a && b && a.sig === b.sig) continue;
    // `propagateDamage( boardItemCopy )` before and `propagateDamage( boardItem )` after
    if (a) damage(id, a.layers, boxesBefore.get(id));
    if (b) damage(id, b.layers, boxesAfter.get(id));
  }
  for (const z of zones) if (!filled.has(z.id)) stale.add(z.id);
  return zones.filter((z) => stale.has(z.id)).map((z) => z.id);
}
