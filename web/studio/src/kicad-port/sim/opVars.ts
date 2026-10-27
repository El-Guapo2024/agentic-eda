// `${OP}` text variables: the last operating-point run's values in a symbol's fields and in a label's text (commit 8303b2ad).
//
//   eeschema/sch_symbol.cpp  SCH_SYMBOL::ResolveTextVar: `^OP(:[^.]*)?(.([0-9])?([a-zA-Z]*))?$` -- `${OP}` the item's current, `${OP:power}` its power
//                            ("~W"), `${OP:<pin>}` (a pin's name or number, case ignored) the current of that model pin (the item's own for a two-pin
//                            model, `<item>:<model pin>` otherwise), "?" for a pin that is no model pin; 3 digits and "~A" unless the token says
//   eeschema/sch_label.cpp   SCH_LABEL_BASE::ResolveTextVar: `^OP(.([0-9])?([a-zA-Z]*))?$` -- the voltage of the label's net (`connection->Name( false )`),
//                            3 digits and "~V" unless the token says; "?" when the label is on no net
//   eeschema/schematic.cpp   SCHEMATIC::GetOperatingPoint (opOverlay.ts `operatingPoint`): "--" before any run
//
// The server leaves `${OP..}` as written (crates/model/src/text_vars.rs); the values live here, so the schematic view resolves the token in what it draws
// (`withOpVars`). The regexes are KiCad's as written (its `.` is any character). Pure: no React, no DOM.
import type { Schematic } from "../../api/types";
import { pinItems } from "../schPins";
import { operatingPoint } from "./opOverlay";
import type { Circuit, CircuitItem } from "./simSignals";

const SYMBOL_OP = /^OP(:[^.]*)?(.([0-9])?([a-zA-Z]*))?$/;
const LABEL_OP = /^OP(.([0-9])?([a-zA-Z]*))?$/;

/** `ExpandTextVars` for the tokens `resolve` answers (`null`: left as written); an escaped `\${..}` is kept as it is. */
export function expandOpVars(text: string, resolve: (token: string) => string | null): string {
  if (!text.includes("${")) return text;
  return text.replace(/(\\?)\$\{([^{}]*)\}/g, (all, esc: string, token: string) => {
    if (esc) return all;
    const v = resolve(token);
    return v === null ? all : v;
  });
}

/** `SCH_LABEL_BASE::ResolveTextVar`'s `OP` branch; `null` for any other token. `net` is the label's connection's name. */
export function labelOpToken(token: string, ctx: { points: Map<string, number>; net: string | null }): string | null {
  const m = LABEL_OP.exec(token);
  if (!m) return null;
  const precision = m[2] ? Number(m[2]) : 3;
  const range = m[3] ? m[3] : "~V";
  return ctx.net === null ? "?" : operatingPoint(ctx.points, ctx.net, precision, range);
}

/** `SCH_SYMBOL::ResolveTextVar`'s `OP` branch; `null` for any other token. `item` is the symbol's item in the circuit (its SPICE name, its model pins). */
export function symbolOpToken(token: string, ctx: { points: Map<string, number>; ref: string; item: CircuitItem | undefined; pins: ReadonlyArray<{ number: string; name: string | null }> }): string | null {
  const m = SYMBOL_OP.exec(token);
  if (!m) return null;
  const pin = (m[1] ?? "").toLowerCase();
  const precision = m[3] ? Number(m[3]) : 3;
  let range = m[4] ? m[4] : "~A";
  // `model.SpiceGenerator().ItemName( spiceItem )`, lower-cased
  const spiceRef = (ctx.item?.name ?? ctx.ref).toLowerCase();
  if (pin === "") return operatingPoint(ctx.points, spiceRef, precision, range);
  if (pin === ":power") {
    if (!m[4]) range = "~W";
    return operatingPoint(ctx.points, `${spiceRef}:power`, precision, range);
  }
  const want = pin.slice(1);
  const item = ctx.item;
  if (!item) {
    // Not in the circuit (no run yet, or a symbol the netlist leaves out): KiCad still builds the symbol's model (`simLibMgr.CreateModel`), whose pins are the
    // symbol's, so a pin it names is asked for -- `GetOperatingPoint` gives "--" before any run, "?" for a signal the run has not.
    if (ctx.pins.some((p) => want === p.number.toLowerCase() || want === (p.name ?? "").toLowerCase())) {
      return operatingPoint(ctx.points, ctx.pins.length === 2 ? spiceRef : `${spiceRef}:${want}`, precision, range);
    }
  }
  if (item) {
    // the model's pins, as the symbol pins they stand for (`pin_index`: symbol pin number -> model pin)
    for (const [number, idx] of Object.entries(item.pin_index)) {
      const name = ctx.pins.find((p) => p.number === number)?.name ?? "";
      if (want !== number.toLowerCase() && want !== name.toLowerCase()) continue;
      if (item.pin_count === 2) return operatingPoint(ctx.points, spiceRef, precision, range);
      const cur = item.currents[idx];
      const inner = cur ? /^I\((.*)\)$/i.exec(cur)?.[1] : undefined;
      return inner ? operatingPoint(ctx.points, inner.toLowerCase(), precision, range) : "?";
    }
  }
  return "?";
}

const hasOp = (s: string | undefined | null) => !!s && s.includes("${OP");

/** A field to resolve: its shown text has a `${OP..}` that its raw text did not escape. The server's shown text has `\${..}` already undone
 * (`text_vars.rs::shown_text`), but KiCad resolves `${OP}` inside `ResolveTextVars`, before the `<<<ESC_DOLLAR:` markers are undone (`common.cpp`), so an
 * escaped one stays as written. */
const fieldHasOp = (f: { text: string; shown?: string }) => (f.shown === undefined ? hasOp(f.text) : hasOp(f.shown) && /(^|[^\\])\$\{OP/.test(f.text));

/** Whether `p` lies on the segment `a`-`b` (whole nanometres, so exactly). */
function onSegment(p: readonly [number, number], a: readonly [number, number], b: readonly [number, number]): boolean {
  const cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
  if (cross !== 0) return false;
  return Math.min(a[0], b[0]) <= p[0] && p[0] <= Math.max(a[0], b[0]) && Math.min(a[1], b[1]) <= p[1] && p[1] <= Math.max(a[1], b[1]);
}

/** The net of the item connected at `at` (`Connection()` of a label or a sheet pin, `connection_graph.cpp::updateItemConnectivity`): a wire it lies on, else the
 * pin of a symbol or a power symbol whose connection end is there; `null` when nothing is. */
export function netAt(sch: Schematic, at: readonly [number, number]): string | null {
  for (const w of sch.wires) {
    if (w.bus) continue;
    for (let i = 0; i + 1 < w.pts.length; i++) if (onSegment(at, w.pts[i]!, w.pts[i + 1]!)) return w.net;
    if (w.pts.length === 1 && w.pts[0]![0] === at[0] && w.pts[0]![1] === at[1]) return w.net;
  }
  const pin = pinItems(sch).find((p) => p.tip[0] === at[0] && p.tip[1] === at[1] && p.net);
  if (pin) return pin.net;
  const power = sch.power_symbols.find((p) => p.at[0] === at[0] && p.at[1] === at[1]);
  return power ? power.net : null;
}

/**
 * The net of a label's `Connection()` (`SCH_LABEL_BASE::ResolveTextVar`): the wire, pin or power symbol at its anchor ([`netAt`]), else the net its name joins. A label on
 * nothing is still a subgraph of its own, driven by itself (`CONNECTION_GRAPH::buildConnectionGraph`), and `m_local_label_cache[( sheet, name )]` (a local or
 * hierarchical label) or `m_global_label_cache[name]` (a global label, a global power symbol) merges it with the other subgraphs of that name: here, a label
 * of the sheet of the same name and kind that is on something, or a power symbol of that net for a global label. Alone, it is its own net,
 * `SCH_LABEL_BASE`'s driver name: the global label's text, a local one's sheet path (`SCH_SHEET_PATH::PathHumanReadable`, "/" at the root) and text (r4 S4-1).
 * Differs: labels of other sheets are not in the payload, so a label joined only from another sheet gets its own name -- the net's name for a global label,
 * but a hierarchical label on nothing that its sheet pin joins to the parent's net is named after itself, not after the parent's driver.
 */
export function labelNet(sch: Schematic, label: { net: string; at: readonly [number, number]; scope: string }): string | null {
  const own = netAt(sch, label.at);
  if (own !== null) return own;
  const global = label.scope === "global";
  for (const other of sch.labels) {
    if (other.net !== label.net || (other.scope === "global") !== global) continue;
    if (other.at[0] === label.at[0] && other.at[1] === label.at[1]) continue;
    const net = netAt(sch, other.at) ?? other.net_name ?? null;
    if (net !== null) return net;
  }
  if (global) {
    const power = sch.power_symbols.find((p) => p.net === label.net);
    return power ? power.net : label.net;
  }
  const path = (sch.sheet_path ?? []).map((e) => e.name);
  return `/${path.map((n) => `${n}/`).join("")}${label.net}`;
}

/** The sheet as drawn with every `${OP..}` of its symbols' fields, its labels and its sheet pins resolved (`SCH_SHEET_PIN` is an `SCH_HIERLABEL`, so it resolves
 * `${OP}` as a label does); the sheet itself when none has one. A label's net is the net of the wire or pin at its anchor (its connection, [`netAt`]), else the one its name joins ([`labelNet`]). */
export function withOpVars(sch: Schematic, circuit: Circuit | null, points: Map<string, number>): Schematic {
  const symbols = sch.symbols.some((s) => (s.fields ?? []).some(fieldHasOp));
  const labels = sch.labels.some((l) => hasOp(l.net) || (l.fields ?? []).some(fieldHasOp));
  const sheetPins = (sch.sheets ?? []).some((sh) => sh.pins.some((p) => hasOp(p.name)));
  if (!symbols && !labels && !sheetPins) return sch;
  const out: Schematic = { ...sch };
  if (symbols) {
    out.symbols = sch.symbols.map((s) => {
      if (!(s.fields ?? []).some(fieldHasOp)) return s;
      const ref = s.reference ?? s.id;
      const ctx = { points, ref, item: circuit?.items.find((i) => i.ref === ref), pins: s.pins };
      return { ...s, fields: (s.fields ?? []).map((f) => (fieldHasOp(f) ? { ...f, shown: expandOpVars(f.shown ?? f.text, (t) => symbolOpToken(t, ctx)) } : f)) };
    });
  }
  if (labels) {
    out.labels = sch.labels.map((l) => {
      if (!hasOp(l.net) && !(l.fields ?? []).some(fieldHasOp)) return l;
      const net = labelNet(sch, l);
      const resolve = (t: string) => labelOpToken(t, { points, net });
      return { ...l, net: expandOpVars(l.net, resolve), ...(l.fields ? { fields: l.fields.map((f) => (fieldHasOp(f) ? { ...f, shown: expandOpVars(f.shown ?? f.text, resolve) } : f)) } : {}) };
    });
  }
  if (sheetPins) {
    out.sheets = sch.sheets.map((sh) =>
      sh.pins.some((p) => hasOp(p.name)) ? { ...sh, pins: sh.pins.map((p) => (hasOp(p.name) ? { ...p, name: expandOpVars(p.name, (t) => labelOpToken(t, { points, net: netAt(sch, p.at) })) } : p)) } : sh
    );
  }
  return out;
}
