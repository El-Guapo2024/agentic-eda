// Port of the Net Navigator's item list and `SCH_EDIT_FRAME::SelectNextPrevNetNavigatorItem`
// (eeschema/net_navigator.cpp) -- `eeschema.EditorControl.nextNetItem` / `previousNetItem`
// (Tab / Shift+Tab). With a net highlighted and one of its items selected, Tab selects the
// next item of that net and Shift+Tab the previous, wrapping at both ends.
//
// What "the net's items" are: `MakeNetNavigatorNode` lists every item of the net's subgraphs
// except wires/buses (`SCH_LINE_T`), junctions and bus entries -- so symbol pins, labels
// (local/global/hierarchical), power-symbol pins and no-connect flags -- one tree node per item
// with the text `GetNetNavigatorItemText` builds, and `SortChildren` orders the siblings by
// that text (wxTreeCtrl's default compare: plain, case-sensitive string order). The tree has
// one parent node per sheet, so the walk in `SelectNextPrevNetNavigatorItem` (breadth first,
// leaves only) visits sheet 1's sorted items, then sheet 2's. The studio shows one sheet at a
// time, so the list here is that sheet's items; the sheet nodes of a hierarchy are not walked.
//
// Not ported: nothing is highlighted -> nothing happens (`IsBrightened()` guard), the tree
// control's own selection state (the position is derived from the selected item instead), the
// sheet-pin item kind (this studio's sheet pins carry no net).
import { messageTextFromValue, type MessageUnits } from "./messageText";
import { referenceUnitSuffix, subpartIdsOf } from "./schUnits";
import { resolveLibSymbol } from "../components/schematic/libSymbol";
import type { Schematic } from "../api/types";

export interface NavPin {
  /** Reference of the owning symbol (the id it is selected by, and the `REF` of the wires' "REF.PIN" ends). */
  ref: string;
  /** The reference as the item's text shows it, `SCH_SYMBOL::GetRef( &aSheetPath, true )`: with the unit's sub-reference for a symbol of more than one unit
   *  (`U1B`, r7 A7-M1). Absent: `ref`. */
  shownRef?: string;
  number: string;
  name: string | null;
  /** The alternate function the placed pin picked, which `SCH_PIN::GetShownName` gives in place of the library pin's name (`m_alt`, r7 A7-M1). */
  alt?: string | null;
  /** World position of the pin's connection end, um (matches a no-connect flag's `at`). */
  tip: readonly [number, number];
  /** The net the tracer put the pin on (the placed pin's `net`), when known: a pin joined by a label on its end or by touching another pin is on
   *  its net with no wire ending on it (r6 C6-M1). Absent: the pin is on the nets of the wires that end on it. */
  net?: string | null;
}

export interface NavSchematic {
  /** Every symbol pin on the sheet. */
  pins: readonly NavPin[];
  /** Wires and buses with the net they carry and the "REF.PIN" ends they land on. */
  wires: ReadonlyArray<{ net: string; pins: readonly string[]; bus?: boolean }>;
  /** `net` is the label's text; `net_name` the net it is traced to (the wires' name, `/BAR` for a local `BAR`), absent for a label on no wire. */
  labels: ReadonlyArray<{ id: string; net: string; net_name?: string | null; at: readonly [number, number]; scope: "local" | "global" | "hierarchical" }>;
  powerSymbols: ReadonlyArray<{ id: string; net: string; pin: { number: string; name: string | null } }>;
  noConnects: ReadonlyArray<{ id: string; at: readonly [number, number] }>;
}

export interface NavItem {
  /** Unique within the list. */
  key: string;
  /** The id this item is selected as (a symbol reference for a pin, a label/power/no-connect id otherwise). */
  owner: string;
  /** `GetNetNavigatorItemText`. */
  text: string;
}

/** The net a label is an item of: the traced one (`net_name`), or its text for a label on no wire. */
export const labelNet = (l: { net: string; net_name?: string | null }): string => l.net_name || l.net;

const plain = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/** Every named net of the sheet, in name order (`NET_MAP` skips the nameless ones): a label's is the net it is traced to (r6 C6-M1: `/BAR`, not its text
 *  `BAR`), and a placed pin's own net counts (one joined by a label or another pin with no wire). */
export function sheetNets(s: {
  wires: ReadonlyArray<{ net: string }>;
  labels: ReadonlyArray<{ net: string; net_name?: string | null }>;
  power_symbols: ReadonlyArray<{ net: string }>;
  symbols: ReadonlyArray<{ pins?: ReadonlyArray<{ net?: string | null }> }>;
}): string[] {
  const names = new Set<string>();
  for (const w of s.wires) if (w.net) names.add(w.net);
  for (const l of s.labels) if (labelNet(l)) names.add(labelNet(l));
  for (const sym of s.symbols) for (const p of sym.pins ?? []) if (p.net) names.add(p.net);
  for (const p of s.power_symbols) if (p.net) names.add(p.net);
  return [...names].sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
}

/** The net's items in navigator order. */
export function netNavigatorItems(sch: NavSchematic, net: string, units: MessageUnits): NavItem[] {
  const out: NavItem[] = [];
  const xy = (at: readonly [number, number]) => `(${messageTextFromValue(at[0], units)}, ${messageTextFromValue(at[1], units)})`;

  // symbol pins: every `SCH_PIN` of the net's subgraphs -- the pin's own traced net when the sheet carries it (r6 C6-M1: a pin joined by a label
  // or another pin, with no wire ending on it), else every "REF.PIN" end of a wire on this net
  const onNet = new Set<string>();
  for (const w of sch.wires) if (w.net === net) for (const p of w.pins) onNet.add(p);
  const pinTips = new Set<string>();
  for (const p of sch.pins) {
    if (p.net !== undefined ? p.net !== net : !onNet.has(`${p.ref}.${p.number}`)) continue;
    // `GetNetNavigatorItemText`: `GetRef( &aSheetPath, true )` and `UnescapeString( pin->GetShownName() )` (the alternate when the pin has one)
    const shownName = p.alt || p.name;
    const text = `Symbol '${p.shownRef ?? p.ref}' pin '${p.number}'` + (shownName ? ` (${shownName})` : "");
    out.push({ key: `pin:${p.ref}.${p.number}`, owner: p.ref, text });
    pinTips.add(`${p.tip[0]},${p.tip[1]}`);
  }
  // power symbols are symbols with one pin
  for (const ps of sch.powerSymbols) {
    if (ps.net !== net) continue;
    const text = `Symbol '${ps.id}' pin '${ps.pin.number}'` + (ps.pin.name ? ` (${ps.pin.name})` : "");
    out.push({ key: `power:${ps.id}`, owner: ps.id, text });
  }
  // a label is under the net it is traced to (`GetAllSubgraphs( aNetName )`), not under its own text (r6 C6-M1)
  for (const l of sch.labels) {
    if (labelNet(l) !== net) continue;
    const kind = l.scope === "global" ? "Global label" : l.scope === "hierarchical" ? "Hierarchical label" : "Label";
    out.push({ key: `label:${l.id}`, owner: l.id, text: `${kind} '${l.net}' at ${xy(l.at)}` });
  }
  // a no-connect flag belongs to the net of the pin it sits on
  for (const nc of sch.noConnects) {
    if (!pinTips.has(`${nc.at[0]},${nc.at[1]}`)) continue;
    out.push({ key: `nc:${nc.id}`, owner: nc.id, text: `No-Connect at ${xy(nc.at)}` });
  }
  return out.sort((a, b) => plain(a.text, b.text) || plain(a.key, b.key));
}

/**
 * `SelectNextPrevNetNavigatorItem( aNext )`: the item after (`forward`) or before the current one, wrapping.
 * The current one is `currentKey` when it is still in the list (the tree's own selection), else the first item
 * owned by `selected` (what a user selected by clicking). Null when neither is found ("nextId" stays unset).
 */
export function stepNetItem(items: readonly NavItem[], currentKey: string | null, selected: string | null, forward: boolean): NavItem | null {
  let i = currentKey == null ? -1 : items.findIndex((it) => it.key === currentKey);
  if (i < 0 && selected != null) i = items.findIndex((it) => it.owner === selected);
  if (i < 0) return null;
  const next = forward ? (i + 1) % items.length : (i + items.length - 1) % items.length;
  return items[next] ?? null;
}

/** The sheet as the net item list reads it. */
export function navSchematic(s: Schematic): NavSchematic {
  return {
    pins: s.symbols.flatMap((sym) => {
      // the placed pin's traced net (absent: the wires' ends decide)
      const netOf = new Map((sym.pins ?? []).map((p) => [p.number, p.net ?? null] as const));
      const altOf = new Map((sym.pins ?? []).filter((p) => p.alt).map((p) => [p.number, p.alt] as const));
      // the item's text names the symbol by `GetRef( &aSheetPath, true )`: its unit's sub-reference for a symbol of several units (r7 A7-M1)
      const shownRef = (sym.reference ?? sym.id) + referenceUnitSuffix(s.lib_symbols[sym.lib_id ?? ""], sym.unit, subpartIdsOf(s));
      return (resolveLibSymbol(sym, s.lib_symbols)?.pins ?? []).map((p) => ({ ref: sym.id, shownRef, number: p.pin.number, name: p.pin.name, alt: altOf.get(p.pin.number) ?? null, tip: p.tip, ...(netOf.has(p.pin.number) ? { net: netOf.get(p.pin.number) } : {}) }));
    }),
    wires: s.wires,
    labels: s.labels,
    powerSymbols: s.power_symbols,
    noConnects: s.no_connects,
  };
}

