// What snapping looks at in the Footprint Editor: the open footprint's pads, graphics and texts as the board's snapping scene (kicad-port/snapScene.ts) holds them --
// `PCB_GRID_HELPER` queries the same kind of items in `FOOTPRINT_EDIT_FRAME`, where the board is the one footprint. Not the footprint itself: `PCB_GRID_HELPER::queryVisible` skips it
// there ("If we are in the footprint editor, don't use the footprint itself"), so its origin and the centre of its box are no anchors, and every pad is an item of its own.
//
// A pad is the box of its copper (a turned pad's bounding box, `round` for a circle or an oval) with the layers it is on (`*.Cu` is every copper layer); a graphic is a
// `PCB_SHAPE` on its layer; a text is its anchor. The ids are the editor's own (a pad's id, a graphic's, a text's): a skip list names what is being moved by them.
//
// The Footprint Editor's magnetic settings are its own (`FOOTPRINT_EDITOR_SETTINGS`): pads "always", tracks "never" (there are none), graphics on, the active layer only.
//
// Pure: no store, no DOM. Unit-tested in fpSnapScene.test.ts.
import type { LibraryFootprint, LibraryPad } from "../api/types";
import { cmdShapeToShape, padBox } from "./itemBoxes";
import { padContains } from "./libEditorHit";
import { box, type Pt } from "./snapGeom";
import type { PcbMagnetic } from "./snapAnchors";
import { shapeGeom, shapeHit, type PadItem, type ShapeItem, type SnapItem, type SnapScene, type TextItem } from "./snapScene";
import { shapeBoundingBox } from "../components/canvas/itemHitTest";

/** `FOOTPRINT_EDITOR_SETTINGS` (pcbnew/footprint_editor_settings.cpp): pads `CAPTURE_ALWAYS`, tracks `NO_EFFECT`, graphics on, `allLayers` off. */
export const FOOTPRINT_EDITOR_MAGNETIC: PcbMagnetic = { pads: "always", tracks: "never", graphics: true, allLayers: false };

/** The `footprint` a pad item names: the pads of the Footprint Editor belong to the one footprint open. */
export const FOOTPRINT_ITEM_ID = "(footprint)";

const COPPER = ["F.Cu", "B.Cu"] as const;

/** A pad's layers as names the scene compares (`*.Cu` is every copper layer; the other wildcards stand for both sides). */
export function padSnapLayers(pad: Pick<LibraryPad, "layers" | "kind">): string[] {
  const out = new Set<string>();
  for (const l of pad.layers) {
    if (l.startsWith("*.")) {
      const kind = l.slice(2);
      out.add(`F.${kind}`);
      out.add(`B.${kind}`);
    } else {
      out.add(l);
    }
  }
  // a through-hole pad is copper on every layer, whatever its list says
  if (pad.kind !== "smd") for (const c of COPPER) out.add(c);
  return [...out];
}

/** The pad as the scene's `PadItem`. `id` is the pad's own (an unsaved pad has none: its place in the list names it). */
function padItem(pad: LibraryPad, id: string): PadItem {
  const [x0, y0, x1, y1] = padBox(pad);
  const at: Pt = [(x0 + x1) / 2, (y0 + y1) / 2];
  return {
    id,
    owner: id,
    type: "pad",
    layers: padSnapLayers(pad),
    bbox: box(x0, y0, x1, y1),
    hit: (p) => padContains(pad, p[0], p[1]),
    at,
    w: x1 - x0,
    h: y1 - y0,
    round: pad.shape === "circle" || pad.shape === "oval",
    th: pad.kind !== "smd",
    footprint: FOOTPRINT_ITEM_ID,
  };
}

/** Everything the snapping of the open footprint looks at, in drawing order. */
export function footprintScene(fp: LibraryFootprint | null): SnapScene {
  if (!fp) return { items: [] };
  const items: SnapItem[] = fp.pads.map((p, i) => padItem(p, p.id ?? `pad#${i}`));
  fp.graphics.forEach((g, i) => {
    const s = cmdShapeToShape(g);
    const id = g.id ?? `shape#${i}`;
    const bb = shapeBoundingBox(s);
    const half = s.stroke_width / 2;
    const item: ShapeItem = { id, owner: id, type: "shape", layers: [s.layer], bbox: box(bb[0] - half, bb[1] - half, bb[2] + half, bb[3] + half), geom: shapeGeom(s), hit: shapeHit(s), shape: s };
    items.push(item);
  });
  fp.texts.forEach((t, i) => {
    const id = t.id ?? `text#${i}`;
    // the anchor is all a snap uses of a text; the box only has to catch the cursor near it
    const r = Math.max(t.size_nm, (t.size_nm * t.content.length) / 2);
    const item: TextItem = { id, owner: id, type: "text", layers: [t.layer], bbox: box(t.at.x - r, t.at.y - r, t.at.x + r, t.at.y + r), hit: (p) => Math.abs(p[0] - t.at.x) <= r && Math.abs(p[1] - t.at.y) <= r, at: [t.at.x, t.at.y] };
    items.push(item);
  });
  return { items };
}
