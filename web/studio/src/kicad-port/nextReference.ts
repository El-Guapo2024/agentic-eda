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
const REF_RE = /^(.*?)(\d+)$/;

/**
 * The reference a symbol placed with `A`, or repeated, gets: the first number of its prefix no symbol of the design holds, from `minRefId` up (r2 D-A1:
 * `SCH_DRAWING_TOOLS::PlaceSymbol` annotates the new symbol with `ReannotateByOptions` against every reference of the hierarchy, and
 * `SCH_REFERENCE_LIST::FindFirstUnusedReference` takes the first free one -- with R1 and R3 placed, a resistor is R2). `minRefId` is
 * [`placementMinRef`]'s. **Differs**: a unit of a multi-unit part does not join a part that has the unit free (`areUnitsAvailable`); it takes a number of
 * its own.
 */
export function nextReference(symbols: readonly { readonly id: string }[], prefix: string, others: readonly string[] = [], minRefId = 1): string {
  const used = new Set<number>();
  for (const id of [...symbols.map((s) => s.id), ...others]) {
    const m = REF_RE.exec(id);
    if (m && m[1] === prefix) used.add(Number(m[2]));
  }
  let n = Math.max(1, minRefId);
  while (used.has(n)) n++;
  return `${prefix}${n}`;
}

/** `SCH_REFERENCE_LIST::Annotate`'s `minRefId` for the project's numbering: the start number + 1 ("Use first free number after"), else the sheet's number x 100
 *  or x 1000 + 1 (`AnnotateByOptions`: `aStartAtCurrent = false` there). */
export function placementMinRef(algo: "first_free" | "sheet_x100" | "sheet_x1000", start: number, sheetNumber = 1): number {
  if (algo === "sheet_x100") return sheetNumber * 100 + 1;
  if (algo === "sheet_x1000") return sheetNumber * 1000 + 1;
  return start + 1;
}
