// `A`: this app's own choice for a newly chosen-and-clicked symbol is to
// land with a real, already-numbered reference immediately (e.g. "R7")
// rather than KiCad's own "U?"-style placeholder left for a later
// Annotate pass -- see `Cmd::AddSymbol`'s doc and PARITY-sch.md for why:
// `add_symbol` refuses an exact duplicate id, so placing the *second*
// instance of a freshly-chosen symbol in the same session would be
// refused outright if every placement reused the same bare "R?" (and
// `annotate`'s own numbering only ever recognizes an id that ends
// *exactly* in "?", not "R?1"/"R?2", so inventing a disambiguated
// placeholder wouldn't be picked up later either). This mirrors
// `crates/ops/src/lib.rs::annotate`'s own per-prefix "next free number"
// logic, just run once at placement time instead of in bulk over the
// whole sheet.
import type { SchematicSymbol } from "../api/types";

const REF_RE = /^([A-Za-z]+)(\d+)$/;

export function nextReference(symbols: readonly SchematicSymbol[], prefix: string): string {
  let max = 0;
  for (const s of symbols) {
    const m = REF_RE.exec(s.id);
    if (m && m[1] === prefix) max = Math.max(max, Number(m[2]));
  }
  return `${prefix}${max + 1}`;
}
