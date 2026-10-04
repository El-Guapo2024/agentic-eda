// `ErcViolation.location`-string parsing, shared by SchematicView.tsx's
// ERC canvas markers (painter.ts's `drawErcMarkers`) and ErcDialog.tsx's
// cross-probe (`jumpTo`) -- the one place this app turns a violation's
// `location` back into something on the canvas, so a canvas marker and a
// dialog row's click-to-jump can never disagree about where a given
// finding lives. See types.ts's `ErcViolation.location` doc.
//
// Two producers feed it:
//   * kicad-cli's ERC (GET /api/erc): `location` is OUR id for the first
//     item the violation names -- a symbol "REF", a pin "REF.PIN" (PIN is a
//     pin *number*), a power symbol, a wire, a label, a no-connect or a text
//     id.
//   * crates/lint's schematic readability checks (GET /api/lint), whose
//     locations keep the shapes those checks have always used: "x,y",
//     "NET:x,y", "NET:REF.PIN", "NET:ID", a bare net name, ...
//
// Tried in order, most structurally specific first:
//   1. A literal "x,y" point (after stripping an optional "NET:" prefix).
//   2. "REF.PIN" (after stripping an optional "NET:" prefix).
//   3. "NET:ID" where ID has no dot -- a power symbol's own `PowerSymbol.id`.
//   4. A bare symbol ref.
//   5. A bare id of a power symbol, wire, label, no-connect or text.
//   6. A bare net name -- resolved to the first anchor point this app can
//      find for that net (a label, then a power symbol, then a wire
//      endpoint) since there is no single "right" point for a whole net.
// Anything none of the above resolves (an item the schematic no longer
// has, or a genuinely unrecognized shape) returns `null` -- the dialog
// still shows/selects the row, it just can't additionally re-frame the
// canvas on it.
import type { LibSymbols, Schematic } from "../../api/types";
import { resolveLibSymbol, symbolBounds } from "./libSymbol";

export interface ErcLocationResolution {
  /** World-space, um -- a single point this finding is "at", for framing the view. */
  at: [number, number];
  /** Symbol refs this finding names, for SET_SELECTION/SET_HOT -- empty when the location resolved to a bare point or a net with no specific item. */
  refs: string[];
}

function parsePointLiteral(s: string): [number, number] | null {
  const m = /^(-?\d+),(-?\d+)$/.exec(s);
  if (!m) return null;
  return [Number(m[1]), Number(m[2])];
}

function symbolCenter(ref: string, sch: Schematic, lib: LibSymbols): ErcLocationResolution | null {
  const sym = sch.symbols.find((s) => s.id === ref);
  if (!sym) return null;
  const b = symbolBounds(sym, lib);
  return { at: [(b.minX + b.maxX) / 2, (b.minY + b.maxY) / 2], refs: [ref] };
}

function resolveRefPin(ref: string, pinNumber: string, sch: Schematic, lib: LibSymbols): ErcLocationResolution | null {
  const sym = sch.symbols.find((s) => s.id === ref);
  if (!sym) return null;
  const resolved = resolveLibSymbol(sym, lib);
  const pin = resolved?.pins.find((p) => p.pin.number === pinNumber);
  if (pin) return { at: pin.tip, refs: [ref] };
  // No real lib_symbols geometry for this instance -- the same graceful
  // "fall back to the generic box" every other lib_symbols consumer in
  // this app already does (see libSymbol.ts's own `symbolBounds` doc).
  return symbolCenter(ref, sch, lib);
}

function resolvePowerSymbolId(id: string, sch: Schematic): ErcLocationResolution | null {
  const ps = sch.power_symbols.find((p) => p.id === id);
  return ps ? { at: ps.at, refs: [] } : null;
}

/** An id of a power symbol, wire, label, no-connect or text -- what kicad-cli's ERC reports for those items. A wire resolves to its first point. */
function resolveItemId(id: string, sch: Schematic): ErcLocationResolution | null {
  const ps = sch.power_symbols.find((p) => p.id === id);
  if (ps) return { at: ps.at, refs: [] };
  const wire = sch.wires.find((w) => w.id === id);
  if (wire && wire.pts.length > 0) return { at: wire.pts[0]!, refs: [] };
  const label = sch.labels.find((l) => l.id === id);
  if (label) return { at: label.at, refs: [] };
  const nc = sch.no_connects.find((n) => n.id === id);
  if (nc) return { at: nc.at, refs: [] };
  const text = sch.texts.find((t) => t.id === id);
  if (text) return { at: text.at, refs: [] };
  return null;
}

/** The first anchor point this app can find for `net` -- a label, then a power symbol, then a wire endpoint, in that order (an arbitrary but stable tie-break; `wire_dangling` names a whole net, not one item, so there is no single "right" point the way every other shape above has). */
function firstAnchorForNet(net: string, sch: Schematic): [number, number] | null {
  const label = sch.labels.find((l) => l.net === net);
  if (label) return label.at;
  const ps = sch.power_symbols.find((p) => p.net === net);
  if (ps) return ps.at;
  for (const w of sch.wires) {
    if (w.net === net && w.pts.length > 0) return w.pts[0]!;
  }
  return null;
}

export function ercMarkerPosition(location: string | null | undefined, sch: Schematic | null | undefined): ErcLocationResolution | null {
  if (!location || !sch) return null;
  const lib = sch.lib_symbols;

  const colon = location.indexOf(":");
  const net = colon >= 0 ? location.slice(0, colon) : null;
  const rest = colon >= 0 ? location.slice(colon + 1) : location;

  const pt = parsePointLiteral(rest);
  if (pt) return { at: pt, refs: [] };

  const dot = rest.lastIndexOf(".");
  if (dot > 0) {
    const resolved = resolveRefPin(rest.slice(0, dot), rest.slice(dot + 1), sch, lib);
    if (resolved) return resolved;
  }

  if (net !== null) {
    // "NET:ID": only reachable through a colon -- a bare power-symbol id
    // never appears outside that shape (see this file's own header doc).
    const byPower = resolvePowerSymbolId(rest, sch);
    if (byPower) return byPower;
  }

  const byRef = symbolCenter(rest, sch, lib);
  if (byRef) return byRef;

  const byItem = resolveItemId(rest, sch);
  if (byItem) return byItem;

  const anchor = firstAnchorForNet(net ?? location, sch);
  return anchor ? { at: anchor, refs: [] } : null;
}
