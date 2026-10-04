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

export interface NavPin {
  /** Reference of the owning symbol. */
  ref: string;
  number: string;
  name: string | null;
  /** World position of the pin's connection end, um (matches a no-connect flag's `at`). */
  tip: readonly [number, number];
}

export interface NavSchematic {
  /** Every symbol pin on the sheet. */
  pins: readonly NavPin[];
  /** Wires and buses with the net they carry and the "REF.PIN" ends they land on. */
  wires: ReadonlyArray<{ net: string; pins: readonly string[]; bus?: boolean }>;
  labels: ReadonlyArray<{ id: string; net: string; at: readonly [number, number]; scope: "local" | "global" | "hierarchical" }>;
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

const plain = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/** The net's items in navigator order. */
export function netNavigatorItems(sch: NavSchematic, net: string, units: MessageUnits): NavItem[] {
  const out: NavItem[] = [];
  const xy = (at: readonly [number, number]) => `(${messageTextFromValue(at[0], units)}, ${messageTextFromValue(at[1], units)})`;

  // symbol pins: every "REF.PIN" end of a wire on this net
  const onNet = new Set<string>();
  for (const w of sch.wires) if (w.net === net) for (const p of w.pins) onNet.add(p);
  const pinTips = new Set<string>();
  for (const p of sch.pins) {
    if (!onNet.has(`${p.ref}.${p.number}`)) continue;
    const text = `Symbol '${p.ref}' pin '${p.number}'` + (p.name ? ` (${p.name})` : "");
    out.push({ key: `pin:${p.ref}.${p.number}`, owner: p.ref, text });
    pinTips.add(`${p.tip[0]},${p.tip[1]}`);
  }
  // power symbols are symbols with one pin
  for (const ps of sch.powerSymbols) {
    if (ps.net !== net) continue;
    const text = `Symbol '${ps.id}' pin '${ps.pin.number}'` + (ps.pin.name ? ` (${ps.pin.name})` : "");
    out.push({ key: `power:${ps.id}`, owner: ps.id, text });
  }
  for (const l of sch.labels) {
    if (l.net !== net) continue;
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
