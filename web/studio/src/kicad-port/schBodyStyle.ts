// Body styles of a placed symbol: Cycle Body Style (`SCH_EDIT_TOOL::CycleBodyStyle`, `SCH_EDIT_FRAME::SelectBodyStyle`), the context menu's "Body Style" condition
// (`SCH_CONDITIONS::SingleMultiBodyStyleSymbol`). A library symbol with an alternate ("De Morgan") body style has two (`LIB_SYMBOL::GetBodyStyleCount`); the one a
// placed symbol is drawn in is `body_style` (1 normal, 2 alternate), which `resolveLibSymbol` picks the items of `lib_symbols[lib_id]` by. The verb is `sch_edit`
// `set_body_style` (crates/ops/src/sch_edit.rs).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd, Schematic, SchematicSymbol } from "../api/types";

type SymbolsAndLibrary = Pick<Schematic, "symbols" | "lib_symbols">;

/** How many body styles the library symbol of a placed symbol has: 2 for one with an alternate body, 1 for any other (or when the server did not say). */
export function bodyStyleCount(sch: Pick<Schematic, "lib_symbols">, sym: Pick<SchematicSymbol, "lib_id">): number {
  const lib = sym.lib_id ? sch.lib_symbols?.[sym.lib_id] : undefined;
  return Math.max(1, lib?.body_style_count ?? 1);
}

/** `SingleMultiBodyStyleSymbol`: the selection is one symbol whose library symbol has more than one body style. */
export function singleMultiBodyStyleSymbol(sch: SymbolsAndLibrary, selection: readonly string[]): boolean {
  if (selection.length !== 1) return false;
  const sym = sch.symbols.find((s) => s.id === selection[0]);
  return !!sym && bodyStyleCount(sch, sym) > 1;
}

/**
 * The body style `sym` is drawn in after Cycle Body Style: "nextBodyStyle = GetBodyStyle() + 1; if( nextBodyStyle > GetBodyStyleCount() ) nextBodyStyle = 1".
 * A symbol with one body style stays in it.
 */
export function nextBodyStyle(sch: Pick<Schematic, "lib_symbols">, sym: Pick<SchematicSymbol, "lib_id" | "body_style">): number {
  const count = bodyStyleCount(sch, sym);
  const next = (sym.body_style || 1) + 1;
  return count <= 1 ? 1 : next > count ? 1 : next;
}

/**
 * Cycle Body Style on `selection`. KiCad cycles the first selected symbol ("symbol = selection.Front()") when it has another body style to go to; so does this, in
 * selection order. Null when the selection holds no symbol with more than one body style.
 */
export function cycleBodyStyleCmd(sch: SymbolsAndLibrary, selection: readonly string[]): Cmd | null {
  for (const id of selection) {
    const sym = sch.symbols.find((s) => s.id === id);
    if (sym && bodyStyleCount(sch, sym) > 1) return { op: "sch_edit", verb: "set_body_style", ids: [id] } as unknown as Cmd;
  }
  return null;
}

/** Select Body Style `style` on the symbols of `selection` that have more than one (the Properties choice), or null when none. */
export function setBodyStyleCmd(sch: SymbolsAndLibrary, selection: readonly string[], style: number): Cmd | null {
  const ids = [...new Set(selection)].filter((id) => sch.symbols.some((s) => s.id === id && bodyStyleCount(sch, s) > 1));
  return ids.length === 0 ? null : ({ op: "sch_edit", verb: "set_body_style", ids, style } as unknown as Cmd);
}
