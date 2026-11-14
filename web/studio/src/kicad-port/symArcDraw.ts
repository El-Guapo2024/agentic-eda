// The Symbol Editor's Draw Arc in KiCad 25ec2335's five modes (UI-LEFT-3 Y3-8): `EE_GRAPHIC_TOOL::DrawArc` runs for the Symbol Editor too, so the session is the schematic's
// (kicad-port/arcDrawSession.ts) with the symbol's own two parts -- its own `m_ArcDrawMode` (symbol_editor.json's `editing.arc_draw_mode`, apart from eeschema's) and the
// graphics a TANGENT arc's first click may start from (`findTangentSeed`'s `IsSymbolEditor()` branch: the symbol's draw items of the unit and body style being edited, on
// `LAYER_DEVICE`). The five actions are the schematic's names (`SCH_ACTIONS::drawArcCenter` .. `drawArcStartDirEnd`, kicad-port/schArcDraw.ts `ARC_MODE_ACTIONS`).
//
// Pure: no React, no DOM. The symbol's graphics are its own mm (+y up); the seed is in the editor's internal nm (+y down).
import type { LibrarySymbol, LibrarySymbolGraphic } from "../api/types";
import { arcDrawModeSetting } from "./arcDrawSession";
import { arcTangentSeedAt, ARC_SEED_PICK_PIXELS, type ArcTangentSeed, type SeedShape } from "./arcTangentSeed";

/** The Symbol Editor's `m_ArcDrawMode`. */
export const symArcDrawMode = arcDrawModeSetting("eda.sym.arcDrawMode");

const nm = (p: { x: number; y: number }): [number, number] => [Math.round(p.x * 1e6), Math.round(-p.y * 1e6)];

/** A graphic of the symbol as a seed can start from it (`ArcTangentSeedAt( const EDA_SHAPE& )`: a segment, an arc, a bezier, a polyline; a rectangle, a circle and a text box give none). */
function seedShapeOf(g: LibrarySymbolGraphic): SeedShape | null {
  switch (g.kind) {
    case "arc":
      return { kind: "arc", start: nm(g.start), mid: nm(g.mid), end: nm(g.end) };
    case "bezier":
      return { kind: "bezier", start: nm(g.start), c1: nm(g.c1), c2: nm(g.c2), end: nm(g.end) };
    case "polyline":
      return g.pts.length === 2 ? { kind: "segment", start: nm(g.pts[0]!), end: nm(g.pts[1]!) } : { kind: "polygon", pts: g.pts.map(nm), closed: false };
    default:
      return null;
  }
}

/**
 * `findTangentSeed` in the Symbol Editor: the first draw item of the unit and body style being edited (`ALL_UNITS` / `ALL_BODY_STYLES` ones too) that gives a seed at `at`,
 * within `ARC_SEED_PICK_PIXELS` at this zoom (`scale`: px per nm); null when the click is on none.
 */
export function symTangentSeedAt(sym: Pick<LibrarySymbol, "graphics">, at: readonly [number, number], scale: number, unit: number, bodyStyle: number): ArcTangentSeed | null {
  const tolerance = Math.round(ARC_SEED_PICK_PIXELS / (scale || 1));
  for (const g of sym.graphics) {
    if (!(g.unit === 0 || g.unit === unit) || !(g.body_style === 0 || g.body_style === bodyStyle)) continue;
    const shape = seedShapeOf(g);
    const seed = shape ? arcTangentSeedAt(shape, at, tolerance) : null;
    if (seed) return seed;
  }
  return null;
}
