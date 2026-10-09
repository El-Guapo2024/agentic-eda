// The Nets and Net Classes tabs of the Appearance panel (pcbnew/widgets/appearance_controls.cpp: NET_GRID_TABLE, rebuildNets, showNetclass,
// onNetContextMenu, onNetclassContextMenu, buildNetClassMenu) as pure functions: which nets and classes the lists hold and in what order, what
// the eye of a net or a class does (it hides the ratsnest of the net, or of every net of the class: `ratsnest_view_item.cpp` is the one place
// `GetHiddenNets` is read -- KiCad does not hide a net's copper), and the right-click menus' entries.
//
// Hidden nets are `StudioState.bcx.hiddenRatsnestNets` (the same set `pcbnew.EditorControl.hideNet` / `showNet` edit, `rs->GetHiddenNets()`); the hidden
// classes are `AppearanceState.hiddenNetclasses` (`PROJECT_LOCAL_SETTINGS::m_HiddenNetclasses`).
//
// A net belongs to one class here: the first class whose pattern matches it, else Default (`classOfNet`). KiCad builds a composite class for a net
// several classes claim (`ContainsNetclassWithName` is true for each of them); one owner per net is what the studio's net class model has.

import { boardNetNames, classOfNet } from "./boardSetupRules";
import type { BoardState, NetClass } from "../api/types";

/** The board facts the two tabs and the painter need, derived once per board. */
export interface NetsContext {
  /** The copper layers, front to back. */
  copper: string[];
  /** Every net the lists show, sorted as `NET_GRID_TABLE::Rebuild` sorts (by name, character code). */
  nets: string[];
  /** The Default class's name and the other classes, in the order Board Setup keeps them (their order is their precedence). */
  defaultName: string;
  classes: NetClass[];
  /** The class that owns a net. */
  classOf: (net: string) => string;
}

/** `wxString::operator<`-style order, as `std::sort( m_nets, a.name < b.name )`. */
const byName = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0);

/** `NET_GRID_TABLE::Rebuild`: the nets with a code above 0, without the `unconnected-(` pad nets KiCad invents, sorted by name. */
export function listedNets(all: readonly string[]): string[] {
  return all.filter((n) => n !== "" && !n.startsWith("unconnected-(")).sort(byName);
}

export function netsContext(board: BoardState | null): NetsContext {
  if (!board) return { copper: [], nets: [], defaultName: "Default", classes: [], classOf: () => "Default" };
  const rules = board.board_rules;
  const classes = rules?.net_classes ?? [];
  const defaultName = rules?.default_class?.name ?? "Default";
  // Memoised: the painter asks once per item.
  const cache = new Map<string, string>();
  const classOf = (net: string): string => {
    let hit = cache.get(net);
    if (hit === undefined) {
      hit = classOfNet(classes, net, defaultName);
      cache.set(net, hit);
    }
    return hit;
  };
  return { copper: board.layers, nets: listedNets(boardNetNames(board)), defaultName, classes, classOf };
}

// ----------------------------------------------------------------------------------------------------------------------------------- nets list

export interface NetRow {
  name: string;
  /** `NET_GRID_ENTRY::visible`: false when the net's ratsnest is hidden. */
  visible: boolean;
  /** The net's own colour text, else null. */
  color: string | null;
}

export function netRows(ctx: Pick<NetsContext, "nets">, hidden: readonly string[], colors: Readonly<Record<string, string>>, filter = ""): NetRow[] {
  const off = new Set(hidden);
  const needle = filter.trim().toLowerCase();
  return ctx.nets.filter((n) => needle === "" || n.toLowerCase().includes(needle)).map((name) => ({ name, visible: !off.has(name), color: colors[name] ?? null }));
}

/** `NET_GRID_TABLE::updateNetVisibility` (showNetInRatsnest / hideNetInRatsnest): the hidden set with one net shown or hidden. */
export function setNetVisible(hidden: readonly string[], net: string, visible: boolean): string[] {
  const rest = hidden.filter((n) => n !== net);
  return visible ? rest : [...rest, net];
}

/** `NET_GRID_TABLE::ShowAllNets`: every listed net shown (a hidden name the list does not hold is left as it is). */
export function showAllNets(hidden: readonly string[], listed: readonly string[]): string[] {
  const all = new Set(listed);
  return hidden.filter((n) => !all.has(n));
}

/** `NET_GRID_TABLE::HideOtherNets`: every listed net hidden but `keep`. */
export function hideOtherNets(hidden: readonly string[], listed: readonly string[], keep: string): string[] {
  const all = new Set(listed);
  return [...hidden.filter((n) => !all.has(n)), ...listed.filter((n) => n !== keep)];
}

/** The nets map with `net`'s colour set, or removed when `color` is null or unspecified (`updateNetColor`: "netColors.erase( aNet.code )"). */
export function withNetColor(colors: Readonly<Record<string, string>>, net: string, color: string | null): Record<string, string> {
  const out = { ...colors };
  if (color === null) delete out[net];
  else out[net] = color;
  return out;
}

// ----------------------------------------------------------------------------------------------------------------------------- net classes list

export interface ClassRow {
  name: string;
  isDefault: boolean;
  /** `HasPcbColor`: the class's own colour text, else null. The Default class never has one. */
  color: string | null;
  /** `!m_HiddenNetclasses.count( name )`. */
  visible: boolean;
  /** How many of the board's nets the class owns. */
  nets: number;
}

/** `rebuildNets`: the Default class first, then the others sorted by name (not in precedence order). */
export function classRows(ctx: Pick<NetsContext, "nets" | "defaultName" | "classes" | "classOf">, colors: Readonly<Record<string, string>>, hidden: readonly string[]): ClassRow[] {
  const off = new Set(hidden);
  const count = new Map<string, number>();
  for (const n of ctx.nets) count.set(ctx.classOf(n), (count.get(ctx.classOf(n)) ?? 0) + 1);
  const row = (name: string, isDefault: boolean): ClassRow => ({ name, isDefault, color: isDefault ? null : (colors[name] ?? null), visible: !off.has(name), nets: count.get(name) ?? 0 });
  return [row(ctx.defaultName, true), ...ctx.classes.map((c) => c.name).sort(byName).map((n) => row(n, false))];
}

/** The listed nets a class owns (`net->GetNetClass()->ContainsNetclassWithName( name )`). */
export function netsOfClass(ctx: Pick<NetsContext, "nets" | "classOf">, className: string): string[] {
  return ctx.nets.filter((n) => ctx.classOf(n) === className);
}

export interface NetHiding {
  hiddenNets: string[];
  hiddenNetclasses: string[];
}

/**
 * `showNetclass( name, show )`: every net of the class has its ratsnest shown or hidden, and the class is added to or dropped from the hidden
 * classes -- the classes are what is saved, so a net another class names later is not hidden by it.
 */
export function showNetclass(cur: NetHiding, ctx: Pick<NetsContext, "nets" | "classOf">, name: string, show: boolean): NetHiding {
  const mine = new Set(netsOfClass(ctx, name));
  const rest = cur.hiddenNets.filter((n) => !mine.has(n));
  return {
    hiddenNets: show ? rest : [...rest, ...mine],
    hiddenNetclasses: show ? cur.hiddenNetclasses.filter((c) => c !== name) : cur.hiddenNetclasses.includes(name) ? cur.hiddenNetclasses : [...cur.hiddenNetclasses, name],
  };
}

/** "Show All Netclasses": Default and every other class shown. */
export function showAllNetclasses(cur: NetHiding, ctx: Pick<NetsContext, "nets" | "classOf" | "defaultName" | "classes">): NetHiding {
  let next = showNetclass(cur, ctx, ctx.defaultName, true);
  for (const c of ctx.classes) next = showNetclass(next, ctx, c.name, true);
  return next;
}

/** "Hide All Other Netclasses": only `keep` stays shown (Default is hidden unless it is the one clicked). */
export function hideOtherNetclasses(cur: NetHiding, ctx: Pick<NetsContext, "nets" | "classOf" | "defaultName" | "classes">, keep: string): NetHiding {
  let next = showNetclass(cur, ctx, ctx.defaultName, keep === ctx.defaultName);
  for (const c of ctx.classes) next = showNetclass(next, ctx, c.name, c.name === keep);
  return next;
}

/**
 * `PCB_EDIT_FRAME::LoadProjectSettings`: the nets hidden when a project opens are the ones it names and every net of a hidden class. A name the board
 * does not have is dropped (`GetNetItem( hidden )` finds nothing).
 */
export function hiddenNetsOnLoad(saved: readonly string[], hiddenClasses: readonly string[], ctx: Pick<NetsContext, "nets" | "classOf">): string[] {
  const known = new Set(ctx.nets);
  const out = new Set(saved.filter((n) => known.has(n)));
  const classes = new Set(hiddenClasses);
  for (const n of ctx.nets) if (classes.has(ctx.classOf(n))) out.add(n);
  return [...out];
}

// ------------------------------------------------------------------------------------------------------------------------------- right-click menus

export type NetMenuAction = "set_color" | "clear_color" | "highlight" | "select" | "deselect" | "show_all" | "hide_others" | "use_schematic_color";

export interface NetMenuItem {
  action: NetMenuAction | "separator";
  label: string;
  disabled?: boolean;
}

/** `OnNetGridRightClick`: the menu of a net. */
export function netMenu(net: string): NetMenuItem[] {
  return [
    { action: "set_color", label: "Set Net Color" },
    { action: "clear_color", label: "Clear Net Color" },
    { action: "separator", label: "" },
    { action: "highlight", label: `Highlight ${net}` },
    { action: "select", label: `Select Tracks and Vias in ${net}` },
    { action: "deselect", label: `Unselect Tracks and Vias in ${net}` },
    { action: "separator", label: "" },
    { action: "show_all", label: "Show All Nets" },
    { action: "hide_others", label: "Hide All Other Nets" },
  ];
}

/**
 * `buildNetClassMenu`: the menu of a class. The Default class has no colour entries. "Use Color from Schematic" is there and disabled while the
 * class has no schematic colour (`ncColor == COLOR4D::UNSPECIFIED`) -- the studio keeps none.
 */
export function netclassMenu(name: string, isDefault: boolean, schematicColor: string | null = null): NetMenuItem[] {
  const head: NetMenuItem[] = isDefault
    ? []
    : [
        { action: "set_color", label: "Set Netclass Color" },
        { action: "use_schematic_color", label: "Use Color from Schematic", disabled: schematicColor === null },
        { action: "clear_color", label: "Clear Netclass Color" },
        { action: "separator", label: "" },
      ];
  return [
    ...head,
    { action: "highlight", label: `Highlight Nets in ${name}` },
    { action: "select", label: `Select Tracks and Vias in ${name}` },
    { action: "deselect", label: `Unselect Tracks and Vias in ${name}` },
    { action: "separator", label: "" },
    { action: "show_all", label: "Show All Netclasses" },
    { action: "hide_others", label: "Hide All Other Netclasses" },
  ];
}

/** The tooltip of a net's eye (`OnNetGridMouseEvent`). */
export function netEyeTip(net: string, visible: boolean): string {
  return `${visible ? "Click to hide ratsnest for" : "Click to show ratsnest for"} ${net}`;
}
