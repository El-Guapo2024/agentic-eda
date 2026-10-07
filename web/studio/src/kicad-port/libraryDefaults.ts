// What the server leaves out of a library symbol, put back. The Rust types (`LibrarySymbol`, `LibrarySymbolPin`, `LibrarySymbolGraphic` in
// crates/model/src/ir.rs) skip a field that has its default value when they write JSON: an empty `reference_prefix` / `description` / `keywords` /
// `datasheet` / `footprint_filters`, a `unit_count` of 1, a pin's `unit` and `body_style` of 1, a graphic's `body_style` of 1. A pin on unit 1 and body
// style 1 -- nearly every pin there is -- arrives with neither field, so `p.unit === state.activeUnit` would never be true, and a symbol with no keywords
// has no `keywords` to send back in a properties edit ("missing field `keywords`"). Read a symbol with this, and it has every field its type says.
import type { LibrarySymbol } from "../api/types";

/** A pin's default is unit 1 and body style 1 (`d_unit_one`, `d_body_style_one`); a graphic's is unit 0 ("every unit") and body style 1. */
export function withDefaults(sym: LibrarySymbol): LibrarySymbol {
  return {
    ...sym,
    reference_prefix: sym.reference_prefix ?? "",
    description: sym.description ?? "",
    keywords: sym.keywords ?? "",
    datasheet: sym.datasheet ?? "",
    unit_count: sym.unit_count ?? 1,
    footprint_filters: sym.footprint_filters ?? [],
    pins: (sym.pins ?? []).map((p) => ({ ...p, unit: p.unit ?? 1, body_style: p.body_style ?? 1 })),
    graphics: (sym.graphics ?? []).map((g) => ({ ...g, unit: g.unit ?? 0, body_style: g.body_style ?? 1 })),
  };
}
