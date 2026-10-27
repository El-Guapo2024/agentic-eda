// Find by Properties' Query tab, studio side: the request `POST /api/pcb/query` answers (`crates/drc/src/query.rs`). The expression language runs on the
// backend, in Rust (`eda_drc::pcbexpr` over `eda_drc::libeval`), the one copy the workspace keeps; the TypeScript copy (`pcbExpr.ts`, `libeval.ts`) is
// INACTIVE since 2026-10-28 and only its tests import it.
//
// What stays here is what only the studio has: the Properties panel's model of each item (`PCB_PROPERTIES`), which the language reads. Each item goes with
// the facts `PCBEXPR_VAR_REF::GetValue` and `pcbexpr_functions.cpp` ask of a `BOARD_ITEM`:
//   - its class and the name `A.Type` gives it (`typeName`), its layers (`GetLayerSet`), its net;
//   - the properties the panel has for it and that it can show (`IsAvailableFor`), each with its value by the kind of the property (a layer, a net, a choice's
//     label, a number, a flag, a text); a pad or a field also carries its footprint's, under its own ("If the property isn't defined on the item itself but is
//     defined on its parent footprint, resolve against the parent");
//   - its footprint (reference, library id, fields) for a footprint, a pad and a field; its groups, innermost first (`memberOfGroup`: the item's group, else
//     its footprint's, and every group above); whether it is plated; a via's type.
// and the board's: the names of the layers (`ENUM_MAP<PCB_LAYER_ID>`: canonical and the board's own), their standard names, the nets and each net's class.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardState, Cmd } from "../api/types";
import { effectiveClassName } from "./boardSetupRules";
import { enabledOptionalLayers, layerDisplayName, standardLayerName } from "./boardLayers";
import { PCB_PROPERTIES, pcbContext, pcbItemsOf, typeName, type PcbCtx, type PcbItem } from "./pcbProperties";
import type { PropertyDef } from "./propertyManager";

export interface PcbQueryProp {
  name: string;
  kind: "int" | "double" | "bool" | "str" | "enum" | "net" | "layer";
  value: string | number | boolean;
}

export interface PcbQueryItem {
  id: string;
  class: string;
  type_name: string;
  layers: string[];
  net: string;
  props: PcbQueryProp[];
  footprint: { reference: string; fpid: string; fields: [string, string][] } | null;
  groups: string[];
  plated: boolean;
  via_type: "Through" | "Blind" | "Buried" | null;
}

export interface PcbQueryRequest {
  expression: string;
  items: PcbQueryItem[];
  env: {
    layer_entries: [string, string][];
    standard_names: Record<string, string>;
    nets: string[];
    classes: Record<string, [string, string[]]>;
    /** Every property name the registry has for a board item: `CreateVarRef` calls any other an unrecognized property. */
    properties: string[];
  };
}

/** The answer: `ok` is `Compile`'s (the text lexes and parses), `errors` what the compiler told, `ids` the matches in the order of the items (null when it does not compile). */
export interface PcbQueryResponse {
  ok: boolean;
  errors: string[];
  ids: string[] | null;
}

/** `BOARD_ITEM::GetLayerSet()` of an item: where it physically is. */
export function itemLayers(item: PcbItem, board: BoardState, ctx: PcbCtx, seen: Set<string> = new Set()): string[] {
  const copper = ctx.copperLayers;
  switch (item.type) {
    case "FOOTPRINT":
      return [item.part!.side === "bottom" ? "B.Cu" : "F.Cu"];
    case "PAD": {
      const pad = item.pad!;
      const kind = pad.kind ?? (pad.th ? "through_hole" : "smd");
      if (kind === "smd") return item.part!.side === "bottom" ? ["B.Cu", "B.Paste", "B.Mask"] : ["F.Cu", "F.Paste", "F.Mask"];
      return [...copper, "F.Mask", "B.Mask"];
    }
    case "PCB_TRACK":
    case "PCB_ARC":
      return [item.track!.layer];
    case "PCB_VIA": {
      const v = item.via!;
      const a = copper.indexOf(v.from);
      const b = copper.indexOf(v.to);
      if (a < 0 || b < 0) return [...copper];
      return copper.slice(Math.min(a, b), Math.max(a, b) + 1);
    }
    case "ZONE":
      return [item.zone!.layer];
    case "PCB_TEXT":
      return [item.text!.layer];
    case "PCB_FIELD":
      return [item.field!.layer];
    case "PCB_SHAPE":
      return [item.shape!.layer];
    case "PCB_GROUP": {
      if (seen.has(item.id)) return [];
      seen.add(item.id);
      const out = new Set<string>();
      for (const m of pcbItemsOf(board, item.group!.member_ids)) for (const l of itemLayers(m, board, ctx, seen)) out.add(l);
      return [...out];
    }
    default:
      return item.dim ? [item.dim.layer] : [];
  }
}

/** `BOARD_CONNECTED_ITEM::GetNetname()`. */
const netNameOf = (item: PcbItem): string => item.track?.net ?? item.via?.net ?? item.zone?.net ?? item.pad?.net ?? "";

/** The footprint an item belongs to (`GetParentFootprint`): a pad's or a field's. */
const parentFootprint = (item: PcbItem): PcbItem | null =>
  (item.type === "PAD" || item.type === "PCB_FIELD") && item.part ? { type: "FOOTPRINT", id: item.part.ref, part: item.part } : null;

/** A property's value as the language reads it (`PCBEXPR_VAR_REF::GetValue` by the type of the property); null when the getter cannot answer. */
function queryProp(prop: PropertyDef<PcbItem, PcbCtx, Cmd>, item: PcbItem, ctx: PcbCtx): PcbQueryProp | null {
  let raw: unknown;
  try {
    raw = prop.get(item, ctx);
  } catch {
    return null;
  }
  switch (prop.kind) {
    case "int":
      return { name: prop.name, kind: "int", value: typeof raw === "number" ? raw : Number(raw) };
    case "double":
      return { name: prop.name, kind: "double", value: typeof raw === "number" ? raw : Number(raw) };
    case "bool":
      return { name: prop.name, kind: "bool", value: !!raw };
    case "enum":
    case "net": {
      if (prop.name === "Layer" || prop.name === "Layer Top" || prop.name === "Layer Bottom") return { name: prop.name, kind: "layer", value: String(raw) };
      if (prop.kind === "net") return { name: prop.name, kind: "net", value: String(raw) };
      const hit = (prop.choices ? prop.choices(item, ctx) : []).find((c) => c.value === raw);
      return { name: prop.name, kind: "enum", value: hit ? hit.label : String(raw) };
    }
    default:
      return { name: prop.name, kind: "str", value: String(raw) };
  }
}

/** The properties of `item` it can show, by name (the first of several of one name that it has, as `GetProperty` picks). */
function availableProps(item: PcbItem, ctx: PcbCtx): Map<string, PcbQueryProp> {
  const out = new Map<string, PcbQueryProp>();
  for (const p of PCB_PROPERTIES.getProperties(item.type)) {
    const key = p.name.toLowerCase();
    if (out.has(key)) continue;
    const prop = PCB_PROPERTIES.getProperty(item.type, p.name, item, ctx);
    if (!prop || !PCB_PROPERTIES.isAvailableFor(item.type, prop, item, ctx)) continue;
    const v = queryProp(prop, item, ctx);
    if (v) out.set(key, v);
  }
  return out;
}

/** `memberOfGroup`'s walk: the item's group, else its footprint's, then every group above it. */
function groupChain(board: BoardState, item: PcbItem): string[] {
  const groups = board.drawings?.groups ?? [];
  const parentOf = (id: string) => groups.find((g) => g.member_ids.includes(id));
  let group = parentOf(item.id);
  if (!group) {
    const fp = parentFootprint(item);
    if (fp) group = parentOf(fp.id);
  }
  const names: string[] = [];
  const seen = new Set<string>();
  while (group && !seen.has(group.id)) {
    names.push(group.name);
    seen.add(group.id);
    group = parentOf(group.id);
  }
  return names;
}

/** `PCB_VIA::GetViaType` as the studio tells it from the via's span (no micro vias). */
function viaType(item: PcbItem, ctx: PcbCtx): PcbQueryItem["via_type"] {
  if (item.type !== "PCB_VIA") return null;
  const c = ctx.copperLayers;
  const v = item.via!;
  const first = c[0];
  const last = c[c.length - 1];
  if ((v.from === first && v.to === last) || (v.from === last && v.to === first)) return "Through";
  const outer = (l: string) => l === first || l === last;
  return outer(v.from) || outer(v.to) ? "Blind" : "Buried";
}

export function queryItem(item: PcbItem, board: BoardState, ctx: PcbCtx): PcbQueryItem {
  const parent = parentFootprint(item);
  const props = new Map<string, PcbQueryProp>(parent ? availableProps(parent, ctx) : []);
  for (const [k, v] of availableProps(item, ctx)) props.set(k, v);
  const fpItem = item.type === "FOOTPRINT" ? item : parent;
  const part = fpItem?.part;
  const pad = item.pad;
  return {
    id: item.id,
    class: item.type,
    type_name: typeName(item),
    layers: itemLayers(item, board, ctx),
    net: netNameOf(item),
    props: [...props.values()],
    footprint: part ? { reference: part.ref, fpid: part.footprint ?? "", fields: (part.fields ?? []).map((f) => [f.name, f.text] as [string, string]) } : null,
    groups: groupChain(board, item),
    plated: item.type === "PAD" && !!pad && (pad.kind ?? (pad.th ? "through_hole" : "smd")) === "through_hole",
    via_type: viaType(item, ctx),
  };
}

/** The request for `expression` over `items` of `board`. */
export function pcbQueryRequest(expression: string, board: BoardState, items: readonly PcbItem[], units: "mm" | "mil" | "in" = "mm"): PcbQueryRequest {
  const ctx = pcbContext(board, units);
  const setup = board.board_rules?.layer_setup;
  const entries: [string, string][] = [];
  const standard: Record<string, string> = {};
  for (const layer of new Set<string>([...ctx.allLayers, ...enabledOptionalLayers(setup)])) {
    entries.push([layer, layer]);
    const shown = layerDisplayName(setup, layer);
    if (shown !== layer) entries.push([shown, layer]);
    const std = standardLayerName(layer);
    if (std !== layer && std !== shown) entries.push([std, layer]);
    if (std !== layer) standard[layer] = std;
  }
  const netClasses = board.board_rules?.net_classes ?? [];
  const queryItems = items.map((i) => queryItem(i, board, ctx));
  const nets = new Set<string>([...ctx.nets, ...queryItems.map((i) => i.net).filter((n) => n !== "")]);
  const classes: Record<string, [string, string[]]> = {};
  for (const net of nets) {
    const name = effectiveClassName(netClasses, net);
    classes[net] = [name, name.split(",")];
  }
  const properties = new Set<string>();
  for (const cls of PCB_PROPERTIES.allClasses()) {
    if (!PCB_PROPERTIES.isOfType(cls, "BOARD_ITEM")) continue;
    for (const p of PCB_PROPERTIES.getProperties(cls)) properties.add(p.name);
  }
  return { expression, items: queryItems, env: { layer_entries: entries, standard_names: standard, nets: [...ctx.nets], classes, properties: [...properties].sort() } };
}
