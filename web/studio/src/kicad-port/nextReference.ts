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
//
// A reference is one part for the whole design, not for one sheet: the references the design's other sheets use (its parts, `others`) count too,
// or a resistor placed on the MCU sheet would be called R1 as the LED channels sheet's is.
const REF_RE = /^([A-Za-z]+)(\d+)$/;

export function nextReference(symbols: readonly { readonly id: string }[], prefix: string, others: readonly string[] = []): string {
  let max = 0;
  for (const id of [...symbols.map((s) => s.id), ...others]) {
    const m = REF_RE.exec(id);
    if (m && m[1] === prefix) max = Math.max(max, Number(m[2]));
  }
  return `${prefix}${max + 1}`;
}
