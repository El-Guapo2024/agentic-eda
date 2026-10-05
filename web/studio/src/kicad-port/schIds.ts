// The deterministic item ids the backend assigns (`crates/model/src/ir.rs` `next_item_id`): `<prefix>_<12 hex chars of
// FNV-1a-64(seed)>`, `_2`, `_3`... on a collision. Knowing them ahead of time lets a tool select the items it just
// created (KiCad re-selects what Change To / Draw adds) without waiting for the next poll. A prediction that misses
// (a collision with an item the client has not seen) only means nothing gets selected, never a wrong item.
import type { SchGraphicShape } from "../api/schEditTypes";

const FNV_OFFSET = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const MASK = 0xffffffffffffffffn;

/** FNV-1a over the UTF-8 bytes of `s`, as 16 lowercase hex digits. */
export function fnv1aHex(s: string): string {
  let h = FNV_OFFSET;
  for (const b of new TextEncoder().encode(s)) {
    h ^= BigInt(b);
    h = (h * FNV_PRIME) & MASK;
  }
  return h.toString(16).padStart(16, "0");
}

/** `next_item_id`: the id for `seed` among the ids already `taken`. */
export function itemId(prefix: string, seed: string, taken: ReadonlySet<string> = new Set()): string {
  const base = `${prefix}_${fnv1aHex(seed).slice(0, 12)}`;
  if (!taken.has(base)) return base;
  for (let n = 2; ; n++) {
    const candidate = `${base}_${n}`;
    if (!taken.has(candidate)) return candidate;
  }
}

const xy = (p: { x: number; y: number }) => `${p.x},${p.y}`;

/** The id `assign_missing_ids` gives a new drawn graphic (`SchGraphicKind::id_prefix` + `SchGraphic::id_seed`). */
export function graphicId(shape: SchGraphicShape, taken?: ReadonlySet<string>): string {
  switch (shape.type) {
    case "rectangle":
      return itemId("shp", `rect|${xy(shape.start)}|${xy(shape.end)}`, taken);
    case "circle":
      return itemId("shp", `circle|${xy(shape.center)}|${shape.radius_um}`, taken);
    case "arc":
      return itemId("shp", `arc|${xy(shape.start)}|${xy(shape.mid)}|${xy(shape.end)}`, taken);
    case "bezier":
      return itemId("shp", `bezier|${xy(shape.start)}|${xy(shape.c1)}|${xy(shape.c2)}|${xy(shape.end)}`, taken);
    case "polygon":
      return itemId("shp", `poly|${shape.pts.map(xy).join(";")}`, taken);
    case "text_box":
      return itemId("tbox", `tbox|${xy(shape.start)}|${xy(shape.end)}|${shape.text}`, taken);
    case "rule_area":
      return itemId("rarea", `rarea|${shape.pts.map(xy).join(";")}`, taken);
    case "directive":
      return itemId("dirl", `dirl|${xy(shape.at)}|${shape.netclass ?? ""}`, taken);
  }
}

/** `NetLabel::id_seed`. */
export const labelId = (net: string, at: readonly [number, number], taken?: ReadonlySet<string>) => itemId("lbl", `${net}|${at[0]},${at[1]}`, taken);
/** `SchematicText::id_seed`. */
export const textId = (content: string, at: readonly [number, number], taken?: ReadonlySet<string>) => itemId("txt", `${content}|${at[0]},${at[1]}`, taken);
