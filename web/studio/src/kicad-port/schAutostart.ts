// Automatically start wires on unconnected pins (UI-LEFT-3 S3-1): `SCH_SELECTION_TOOL::autostartEvent` (eeschema/tools/sch_selection_tool.cpp at KiCad 8303b2ad) and
// the dangling state it reads (`SCH_SCREEN::TestDanglingEnds`: every item's `GetEndPoints`, then each item's `UpdateDanglingState`).
//
// A click of the selection tool with no modifier on one item (`collector.GetCount() == 1 && !m_isSymbolEditor && !hasModifier()`) whose snapped point
// (`BestSnapAnchor( pos, GetItemGrid( item ) )`) is a "clickable anchor" of it (`IsPointClickableAnchor`) starts the wire tool there (`drawWire` with
// `quitOnDraw`: the tool goes back to the selection tool once the wire is finished or cancelled), when `m_Drawing.auto_start_wires` is on and the point editor has
// no point under the cursor. The clickable anchors:
//   * a pin of a symbol (`SCH_PIN::IsPointClickableAnchor`: dangling and at the point) of the symbol's unit and body style, shown (or hidden pins shown);
//   * a local, global or hierarchical label, or a sheet pin (`SCH_LABEL`, `SCH_GLOBALLABEL`, `SCH_HIERLABEL`, `SCH_SHEET_PIN`): dangling, at its anchor; a label whose
//     text is a bus (`SCH_CONNECTION::ConfigureFromLabel( .. ).IsBus()`: a bus vector or a bus group; a bus alias is a bus only inside a group, `{ALIAS}`, as
//     `ConfigureFromLabel` looks aliases up among a group's members -- a label of a bare alias's name is a net in 8303b2ad and 25ec2335 alike) starts a bus;
//   * an end of a wire or a bus: the new wire takes the clicked one's stroke (`startSegments`' `Duplicate`, `SchCmd::AddWireLike`);
//   * an end of a wire or a bus (`SCH_LINE::IsPointClickableAnchor`: that end dangling); a bus starts a bus; a graphic line is never dangling;
//   * an end of a bus entry (`SCH_BUS_ENTRY_BASE::IsPointClickableAnchor`): a wire, or a bus when the entry has no bus on it.
// A directive label, a junction, a no-connect, a power symbol (its pin hidden, as KiCad's power libraries have it) or a sheet are none.
//
// The motion half (`evt->IsMotion()`): the cursor shows the wire (bus, line) cursor over such a point (`displayWireCursor`).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Schematic } from "../api/types";
import { parseBusGroup, parseBusVector } from "./busPattern";
import { allPinItems, parsePinItemId, symbolKey } from "./schPins";

type P = readonly [number, number];

/** `DANGLING_END_T`. */
type EndType = "pin" | "label" | "sheet_label" | "wire" | "bus" | "wire_entry" | "junction" | "no_connect";

/** `DANGLING_END_ITEM`: an end point, its kind, the item it is of (and, for a pin, the symbol it belongs to). */
interface EndItem {
  type: EndType;
  item: string;
  at: P;
  parent?: string;
}

const same = (a: P, b: P) => a[0] === b[0] && a[1] === b[1];

/** The ends of every item of the sheet (`GetEndPoints`), and the wire and bus segments (each `SCH_LINE`: a polyline is drawn as its segments). */
function endItems(sch: Schematic): { ends: EndItem[]; segs: Array<{ id: string; index: number; a: P; b: P; bus: boolean }> } {
  const ends: EndItem[] = [];
  const segs: Array<{ id: string; index: number; a: P; b: P; bus: boolean }> = [];
  for (const w of sch.wires) {
    for (let i = 0; i + 1 < w.pts.length; i++) {
      const a = w.pts[i]!;
      const b = w.pts[i + 1]!;
      segs.push({ id: w.id, index: i, a, b, bus: w.bus });
      ends.push({ type: w.bus ? "bus" : "wire", item: `${w.id}#${i}`, at: a }, { type: w.bus ? "bus" : "wire", item: `${w.id}#${i}`, at: b });
    }
  }
  // `SCH_SYMBOL::GetEndPoints`: every pin of the placed unit, hidden ones too
  for (const p of allPinItems(sch)) ends.push({ type: "pin", item: p.id, at: p.tip, parent: p.key });
  // a power symbol's pin is where it is placed
  for (const ps of sch.power_symbols) ends.push({ type: "pin", item: `power:${ps.id}`, at: ps.at, parent: ps.id });
  for (const l of sch.labels) ends.push({ type: "label", item: l.id, at: l.at });
  for (const s of sch.sheets) for (const p of s.pins ?? []) ends.push({ type: "sheet_label", item: p.id, at: p.at });
  for (const be of sch.bus_entries) {
    ends.push({ type: "wire_entry", item: be.id, at: be.at }, { type: "wire_entry", item: be.id, at: [be.at[0] + be.size[0], be.at[1] + be.size[1]] });
  }
  for (const j of sch.junctions ?? []) ends.push({ type: "junction", item: j.id, at: j.at });
  for (const n of sch.no_connects) ends.push({ type: "no_connect", item: n.id, at: n.at });
  return { ends, segs };
}

/** `TestSegmentHit( pos, a, b, 1 )` / `IsPointOnSegment`: the point lies on the segment (within a unit). */
function onSegment(p: P, a: P, b: P, accuracy = 1): boolean {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2));
  const qx = a[0] + t * dx - p[0];
  const qy = a[1] + t * dy - p[1];
  return qx * qx + qy * qy <= accuracy * accuracy;
}

/** What `TestDanglingEnds` leaves dangling: the pins (by pin id), the labels and sheet pins (by id), the ends of the wire and bus segments (`<wire id>#<segment>:start|end`) and of
 *  the bus entries (`<id>:start|end`). */
export interface DanglingState {
  pins: Set<string>;
  labels: Set<string>;
  segEnds: Set<string>;
  entryEnds: Set<string>;
}

const cache = new WeakMap<Schematic, DanglingState>();

/** [`danglingState`] once per sheet object (the pointer's motion asks on every event). */
export function danglingStateOf(sch: Schematic): DanglingState {
  let d = cache.get(sch);
  if (!d) {
    d = danglingState(sch);
    cache.set(sch, d);
  }
  return d;
}

export function danglingState(sch: Schematic): DanglingState {
  const { ends, segs } = endItems(sch);
  const at = (p: P) => ends.filter((e) => same(e.at, p));
  const out: DanglingState = { pins: new Set(), labels: new Set(), segEnds: new Set(), entryEnds: new Set() };
  // `SCH_SYMBOL::UpdateDanglingState`: a pin is connected by any pin of another symbol, a label, a sheet pin, a wire end, a no-connect or a junction at it
  for (const e of ends.filter((x) => x.type === "pin")) {
    const linked = at(e.at).some((o) => o.parent !== e.parent && ["pin", "label", "sheet_label", "wire", "no_connect", "junction"].includes(o.type));
    if (!linked) out.pins.add(e.item);
  }
  // `SCH_LABEL_BASE::UpdateDanglingState` (a sheet pin is an `SCH_HIERLABEL`): a pin, a label, a sheet pin or a no-connect at its anchor, else a bus or a wire passing
  // through it
  for (const e of ends.filter((x) => x.type === "label" || x.type === "sheet_label")) {
    const linked = at(e.at).some((o) => o.item !== e.item && ["pin", "label", "sheet_label", "no_connect"].includes(o.type)) || segs.some((s) => onSegment(e.at, s.a, s.b));
    if (!linked) out.labels.add(e.item);
  }
  // `SCH_LINE::UpdateDanglingState`: a wire end is joined by anything at it but a bus or a bus entry's bus end; a bus end by anything but a wire or a pin
  for (const s of segs) {
    const self = `${s.id}#${s.index}`;
    const joined = (p: P) =>
      at(p).some((o) => o.item !== self && (s.bus ? o.type !== "wire" && o.type !== "pin" : o.type !== "bus"));
    if (!joined(s.a)) out.segEnds.add(`${self}:start`);
    if (!joined(s.b)) out.segEnds.add(`${self}:end`);
  }
  // `SCH_BUS_WIRE_ENTRY::UpdateDanglingState`: an end with a wire end on it or on a bus; with a wire at one end and a bus at the other, neither end dangles
  for (const be of sch.bus_entries) {
    const ends2: [P, P] = [be.at, [be.at[0] + be.size[0], be.at[1] + be.size[1]]];
    const wire = ends2.map((p) => ends.some((o) => o.type === "wire" && same(o.at, p)));
    const bus = ends2.map((p) => segs.some((s) => s.bus && onSegment(p, s.a, s.b, 0)));
    let start = true;
    let end = true;
    if ((wire[0] && bus[1]) || (wire[1] && bus[0])) start = end = false;
    else if (wire[0] || bus[0]) start = false;
    else if (wire[1] || bus[1]) end = false;
    if (start) out.entryEnds.add(`${be.id}:start`);
    if (end) out.entryEnds.add(`${be.id}:end`);
  }
  return out;
}

/** The tool an autostart arms (`drawWire`, `drawBus`, `drawLines`). */
export type AutostartTool = "wire" | "bus";

export interface Autostart {
  tool: AutostartTool;
  /** Where the wire starts: the snapped point (`newEvt->SetMousePosition( pos )`). */
  at: [number, number];
  /** The wire or bus clicked (`params->sourceSegment = dynamic_cast<SCH_LINE*>( collector[0] )`): `startSegments` duplicates it, so the new segments have its stroke. */
  like?: string;
}

/** `ConfigureFromLabel( text ).IsBus()`: a bus vector (`D[0..7]`) or a bus group (`{A B}`, which a bus alias goes in); anything else -- a bare alias's name too -- is a net. */
export function labelIsBus(text: string): boolean {
  return parseBusVector(text) !== null || parseBusGroup(text) !== null;
}

/**
 * `SCH_SELECTION_TOOL::autostartEvent`: what a click of the selection tool on the item `id` at the snapped point `pos` starts, or null. `showHiddenPins` is the display option
 * (`m_Appearance.show_hidden_pins` / `m_ShowHiddenPins`); `dangling` the sheet's state, when the caller has it already.
 */
export function autostartEvent(sch: Schematic, id: string, pos: P, opts: { showHiddenPins: boolean }, dangling: DanglingState = danglingStateOf(sch)): Autostart | null {
  const start = (tool: AutostartTool): Autostart => ({ tool, at: [pos[0], pos[1]] });
  // a pin, or the symbol it is of (`case SCH_SYMBOL_T`: `GetPin( pos )`): of the unit placed, dangling, at the point, and shown
  const pinParsed = parsePinItemId(id);
  const symbol = !pinParsed ? sch.symbols.find((s) => s.id === id || symbolKey(s.id, s.unit || 1) === id) : undefined;
  if (pinParsed || symbol) {
    const key = pinParsed ? pinParsed.key : symbolKey(symbol!.id, symbol!.unit || 1);
    const pin = allPinItems(sch).find((p) => p.key === key && same(p.tip, pos) && (!pinParsed || p.number === pinParsed.number));
    if (!pin || !dangling.pins.has(pin.id)) return null;
    if (pin.hidden && !opts.showHiddenPins) return null;
    return start("wire");
  }
  // a power symbol is a symbol whose pin is where it is placed; KiCad's power libraries hide it, so only with hidden pins shown
  const power = sch.power_symbols.find((ps) => ps.id === id);
  if (power) return opts.showHiddenPins && same(power.at, pos) && dangling.pins.has(`power:${power.id}`) ? start("wire") : null;
  const label = sch.labels.find((l) => l.id === id);
  if (label) return dangling.labels.has(label.id) && same(label.at, pos) ? start(labelIsBus(label.net) ? "bus" : "wire") : null;
  for (const s of sch.sheets) {
    const p = (s.pins ?? []).find((x) => x.id === id);
    if (p) return dangling.labels.has(p.id) && same(p.at, pos) ? start(labelIsBus(p.name) ? "bus" : "wire") : null;
  }
  const wire = sch.wires.find((w) => w.id === id);
  if (wire) {
    for (let i = 0; i + 1 < wire.pts.length; i++) {
      if ((same(wire.pts[i]!, pos) && dangling.segEnds.has(`${wire.id}#${i}:start`)) || (same(wire.pts[i + 1]!, pos) && dangling.segEnds.has(`${wire.id}#${i}:end`))) return { ...start(wire.bus ? "bus" : "wire"), like: wire.id };
    }
    return null;
  }
  const entry = sch.bus_entries.find((b) => b.id === id);
  if (entry) {
    const end: P = [entry.at[0] + entry.size[0], entry.at[1] + entry.size[1]];
    const hit = (same(entry.at, pos) && dangling.entryEnds.has(`${entry.id}:start`)) || (same(end, pos) && dangling.entryEnds.has(`${entry.id}:end`));
    if (!hit) return null;
    // `if( !busEntry->m_connected_bus_item ) newEvt = drawBus`: an entry with no bus on it starts a bus
    const onBus = [entry.at, end].some((p) => sch.wires.some((w) => w.bus && w.pts.some((q, i) => i + 1 < w.pts.length && onSegment(p, q, w.pts[i + 1]!, 0))));
    return start(onBus ? "wire" : "bus");
  }
  return null;
}
