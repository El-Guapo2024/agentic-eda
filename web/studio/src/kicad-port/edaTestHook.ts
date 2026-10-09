// The pure parts of `window.__eda`, the scripted test hook (actions/useEdaTestHook.ts installs it): what an agent or a test script uses to drive
// the studio without clicking toolbar buttons and reading screenshots.
//
//   window.__eda.actions({ all? })   every action id the runner handles on the current tab, as { id, label, enabled, reason? } -- `reason` says why a
//                                    disabled one is (not offered on this tab / no handler); `all: true` adds the KiCad actions that have no handler.
//   await window.__eda.run(id, args?) runs the action exactly as a menu click does (a disabled one is refused, not run), waits for every /api/ round
//                                    trip it started to finish and the view to catch up, and resolves to { ok, error?, revision, dialog?, toast?, pending? }:
//                                    `revision` is the design revision after it, `dialog` the title of the dialog it opened, `toast` the last toast it showed,
//                                    `pending` true when it stopped waiting for a request that is still running (a long kicad-cli run).
//   window.__eda.state()             a small snapshot: { tab, revision, tool, picker, selection: [{ id, kind }], counts: { footprints, tracks, vias, zones,
//                                    symbols, wires, labels }, entered: the group worked in on the board (else null), grid: the editor's grid in um (null where it is not a choice), dialogs: [titles of the dialogs on
//                                    screen], open: [names of the open dialog/panel flags] };
//                                    `picker` is the prompt of the picker session running (the delete tool's), else null.
//   window.__eda.errors(since?)      the errors since the page loaded as { time, message }: console.error, uncaught errors and rejected promises, the
//                                    error toasts and notices of the three stores. Passive-listener noise is left out. `since` is an epoch ms to filter from.
//
// Typical use from the Browser pane's javascript tool: `await __eda.run("common.Control.zoomFitScreen"); __eda.state()`. Nothing here changes what the
// studio does; with the page open `window.__eda` is just there. This file is the pure model (unit tested, no React and no DOM); the page glue is
// actions/useEdaTestHook.ts.
import { isActionEnabledForTab } from "./actionTabGate";
import { padIds } from "./pcbItems";
import { DEFAULT_OPACITY, type Opacity } from "./appearance";

// ------------------------------------------------------------------------------------------------------------------------------------ errors

export interface ErrorEntry {
  time: number;
  message: string;
}

/** Messages that are browser chatter and not an error of the studio. */
const NOISE = [/Unable to preventDefault inside passive event listener/i];

export function isNoise(message: string): boolean {
  return NOISE.some((re) => re.test(message));
}

/** The errors seen since the page loaded, oldest first, bounded so a runaway error cannot grow without end. */
export class ErrorLog {
  private entries: ErrorEntry[] = [];

  constructor(
    private readonly capacity = 500,
    private readonly clock: () => number = Date.now
  ) {}

  add(message: string, time = this.clock()): void {
    const text = message.trim();
    if (text === "" || isNoise(text)) return;
    this.entries.push({ time, message: text });
    if (this.entries.length > this.capacity) this.entries.splice(0, this.entries.length - this.capacity);
  }

  /** Every entry at or after `since` (epoch ms); all of them when it is left out. */
  list(since = 0): ErrorEntry[] {
    return this.entries.filter((e) => e.time >= since).map((e) => ({ ...e }));
  }
}

/** The text of the arguments of a `console.error( ... )` call. */
export function formatArgs(args: readonly unknown[]): string {
  return args
    .map((a) => {
      if (a instanceof Error) return a.message;
      if (typeof a === "string") return a;
      try {
        return JSON.stringify(a) ?? String(a);
      } catch {
        return String(a);
      }
    })
    .join(" ");
}

// ---------------------------------------------------------------------------------------------------------------------------------- actions

export interface ActionInfo {
  id: string;
  label: string | null;
  enabled: boolean;
  /** Why a disabled action is: only set when it is. */
  reason?: string;
}

/** Why `name` cannot run on `tab`, or null when it can: no handler, or an action of another editor (`actionTabGate.ts`). */
export function disabledReason(name: string, tab: string, registered: boolean): string | null {
  if (!registered) return `no handler on the ${tab} tab: the action is not wired in the studio, or that editor does not offer it`;
  if (!isActionEnabledForTab(name, tab, true)) return `not offered on the ${tab} tab: it belongs to another editor`;
  return null;
}

/**
 * The action list: every id the runner has a handler for, sorted, with whether it is enabled on `tab` and why not. `unhandled` (the KiCad action ids with no
 * handler) are listed too when given -- each disabled, "no handler".
 */
export function describeActions(handled: Iterable<string>, tab: string, labels: ReadonlyMap<string, string>, unhandled: Iterable<string> = []): ActionInfo[] {
  const out: ActionInfo[] = [];
  for (const id of handled) {
    const reason = disabledReason(id, tab, true);
    out.push({ id, label: labels.get(id) ?? null, enabled: reason === null, ...(reason ? { reason } : {}) });
  }
  for (const id of unhandled) out.push({ id, label: labels.get(id) ?? null, enabled: false, reason: disabledReason(id, tab, false)! });
  return out.sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
}

// ------------------------------------------------------------------------------------------------------------------------------------- state

/** The id lists of each kind of item an editor selects from, by kind name. */
export type ItemLists = Record<string, readonly string[]>;

/** id -> kind, from the lists (the first list that has an id names its kind). */
export function kindIndex(lists: ItemLists): Map<string, string> {
  const index = new Map<string, string>();
  for (const [kind, ids] of Object.entries(lists)) for (const id of ids) if (!index.has(id)) index.set(id, kind);
  return index;
}

export interface SelectedItem {
  id: string;
  kind: string;
}

/** The selection as `{ id, kind }` in id order; an id no list knows is kind "unknown". */
export function selectionWithKinds(ids: Iterable<string>, index: ReadonlyMap<string, string>): SelectedItem[] {
  return [...ids].sort().map((id) => ({ id, kind: index.get(id) ?? "unknown" }));
}

/** A list of items that carry an id (the library editors' items may lack one: those cannot be selected, so they are left out). */
type IdItems = readonly { id?: string }[] | undefined;

/** The board's state JSON as far as the hook reads it. */
export interface BoardLike {
  parts?: readonly { ref: string; pads?: readonly { num: string }[] }[];
  routing?: { tracks?: IdItems; vias?: IdItems; zones?: IdItems } | null;
  drawings?: { shapes?: IdItems; texts?: IdItems; dimensions?: IdItems; groups?: IdItems } | null;
}

/** The schematic's state JSON as far as the hook reads it. */
export interface SchematicLike {
  symbols?: IdItems;
  wires?: IdItems;
  labels?: IdItems;
  texts?: IdItems;
  power_symbols?: IdItems;
  no_connects?: IdItems;
  junctions?: IdItems;
  lines?: IdItems;
  sheets?: IdItems;
  graphics?: IdItems;
  bus_entries?: IdItems;
}

const ids = (items: IdItems): string[] => (items ?? []).flatMap((i) => (i.id ? [i.id] : []));

export function boardLists(board: BoardLike | null | undefined): ItemLists {
  return {
    footprint: (board?.parts ?? []).map((p) => p.ref),
    pad: (board?.parts ?? []).flatMap((p) => padIds(p)),
    track: ids(board?.routing?.tracks),
    via: ids(board?.routing?.vias),
    zone: ids(board?.routing?.zones),
    shape: ids(board?.drawings?.shapes),
    text: ids(board?.drawings?.texts),
    dimension: ids(board?.drawings?.dimensions),
    group: ids(board?.drawings?.groups),
  };
}

export function schematicLists(sch: SchematicLike | null | undefined): ItemLists {
  return {
    symbol: ids(sch?.symbols),
    wire: ids(sch?.wires),
    label: ids(sch?.labels),
    text: ids(sch?.texts),
    power: ids(sch?.power_symbols),
    no_connect: ids(sch?.no_connects),
    junction: ids(sch?.junctions),
    line: ids(sch?.lines),
    sheet: ids(sch?.sheets),
    graphic: ids(sch?.graphics),
    bus_entry: ids(sch?.bus_entries),
  };
}

/** The footprint open in the Footprint Editor: its pads, graphics and texts. */
export interface FootprintLike {
  pads?: IdItems;
  graphics?: IdItems;
  texts?: IdItems;
}

export function footprintLists(fp: FootprintLike | null | undefined): ItemLists {
  return { pad: ids(fp?.pads), graphic: ids(fp?.graphics), text: ids(fp?.texts) };
}

/** The symbol open in the Symbol Editor: its pins and graphics (text is a graphic there). */
export interface SymbolLike {
  pins?: IdItems;
  graphics?: IdItems;
}

export function symbolLists(sym: SymbolLike | null | undefined): ItemLists {
  return { pin: ids(sym?.pins), graphic: ids(sym?.graphics) };
}

export interface Counts {
  footprints: number;
  tracks: number;
  vias: number;
  zones: number;
  symbols: number;
  wires: number;
  labels: number;
}

export function countsOf(board: BoardLike | null | undefined, sch: SchematicLike | null | undefined): Counts {
  return {
    footprints: board?.parts?.length ?? 0,
    tracks: board?.routing?.tracks?.length ?? 0,
    vias: board?.routing?.vias?.length ?? 0,
    zones: board?.routing?.zones?.length ?? 0,
    symbols: sch?.symbols?.length ?? 0,
    wires: sch?.wires?.length ?? 0,
    labels: sch?.labels?.length ?? 0,
  };
}

/**
 * The names of the dialog and panel flags a store has switched on: a key ending in `Open` or `Dialog` whose value is set -- `drcOpen`, a string value as
 * `schDialog:find`, an object as `textDialog`. Sorted.
 */
export function openFlags(store: Record<string, unknown>): string[] {
  const out: string[] = [];
  for (const [key, value] of Object.entries(store)) {
    if (!/(Open|Dialog)$/.test(key)) continue;
    if (value === true) out.push(key);
    else if (typeof value === "string" && value !== "") out.push(`${key}:${value}`);
    else if (value && typeof value === "object") out.push(key);
  }
  return out.sort();
}

/** The names in `after` that `before` did not have, in order: the dialogs an action opened. */
export function newNames(before: readonly string[], after: readonly string[]): string[] {
  const had = new Set(before);
  return after.filter((n) => !had.has(n));
}

// -------------------------------------------------------------------------------------------------------------------------------------- runs

export interface CmdOutcome {
  ok: boolean;
  message?: string;
}

export interface RunResult {
  ok: boolean;
  error?: string;
  /** The design revision (`/api/version`'s stamp) after the action settled. */
  revision: string | null;
  /** The title of the first dialog the action opened, when it opened one. */
  dialog?: string;
  /** The last toast shown while it ran (a one-line result such as "Saved ..."), any kind. */
  toast?: string;
  /** True when requests were still in flight when the run stopped waiting (a long kicad-cli run): the work goes on, the result is not final. */
  pending?: boolean;
}

/**
 * What a run came to: not ok when it threw, when a command it sent was refused or when an error was logged while it ran. The first of those is the
 * message.
 */
export function runOutcome(thrown: unknown, cmds: readonly CmdOutcome[], errors: readonly ErrorEntry[]): { ok: boolean; error?: string } {
  if (thrown !== undefined && thrown !== null) return { ok: false, error: thrown instanceof Error ? thrown.message : String(thrown) };
  const refused = cmds.find((c) => !c.ok);
  if (refused) return { ok: false, error: refused.message ?? "the command was refused" };
  if (errors.length > 0) return { ok: false, error: errors[0]!.message };
  return { ok: true };
}

export interface SettleDeps {
  /** How many tracked requests are in flight right now. */
  pending: () => number;
  delay: (ms: number) => Promise<void>;
  now: () => number;
  /** How long nothing may be in flight before the run counts as done, ms. */
  quietMs?: number;
  /** Give up after this long, ms. */
  timeoutMs?: number;
  /** How often to look, ms. */
  stepMs?: number;
}

/** Waits until no tracked request has been in flight for `quietMs` (the view has caught up with the replies), or `timeoutMs` passed. */
export async function settle(deps: SettleDeps): Promise<"settled" | "timeout"> {
  const { pending, delay, now, quietMs = 90, timeoutMs = 10_000, stepMs = 20 } = deps;
  const deadline = now() + timeoutMs;
  let quietSince: number | null = null;
  while (now() < deadline) {
    await delay(stepMs);
    if (pending() === 0) {
      quietSince ??= now();
      if (now() - quietSince >= quietMs) return "settled";
    } else {
      quietSince = null;
    }
  }
  return "timeout";
}

// ------------------------------------------------------------------------------------------------------------------------------ appearance

/** What the Appearance panel has set, as `__eda.state().appearance` reports it: only what differs from a new project, so a check reads what it changed. */
export interface AppearanceSummary {
  /** The objects switched off (the `VISIBILITY_LAYER` names) and the layers switched off (their state keys). */
  hiddenObjects: string[];
  hiddenLayers: string[];
  opacity: Record<string, number>;
  /** Inactive layers: normal, dimmed or hidden. */
  contrast: "normal" | "dimmed" | "hidden";
  /** Where net colours show: all, ratsnest (the default) or off; and which ratsnest lines: all, visible or none. */
  netColorMode: string;
  ratsnest: "all" | "visible" | "none";
  netColors: Record<string, string>;
  netclassColors: Record<string, string>;
  hiddenNets: string[];
  hiddenNetclasses: string[];
  /** The preset the list shows ("" = the blank entry), the user's own, and the saved viewports. */
  preset: string;
  presets: string[];
  viewports: string[];
  flipped: boolean;
}

/** The studio fields `appearanceSummary` reads (structural, so this file keeps no React or store import). */
export interface AppearanceSource {
  appearance: {
    visible: Record<string, boolean>;
    opacity: Opacity;
    contrastHidden: boolean;
    netColorMode: string;
    netColors: Record<string, string>;
    netclassColors: Record<string, string>;
    hiddenNetclasses: string[];
    presets: { name: string }[];
    activePreset: string;
    viewports: { name: string }[];
  };
  layerVisible: Record<string, boolean>;
  highContrast: boolean;
  showRatsnest: boolean;
  gridVisible: boolean;
  bcx: { ratsnestMode: string; hiddenRatsnestNets: string[]; boardFlipped: boolean };
}

export function appearanceSummary(s: AppearanceSource): AppearanceSummary {
  const objects = { ...s.appearance.visible, ratsnest: s.showRatsnest, grid: s.gridVisible };
  const opacity: Record<string, number> = {};
  for (const [k, v] of Object.entries(s.appearance.opacity)) if (v !== (DEFAULT_OPACITY as Record<string, number>)[k]) opacity[k] = v;
  return {
    hiddenObjects: Object.entries(objects).filter(([, on]) => !on).map(([k]) => k).sort(),
    hiddenLayers: Object.entries(s.layerVisible).filter(([k, on]) => !on && !k.startsWith("obj:")).map(([k]) => k).sort(),
    opacity,
    contrast: !s.highContrast ? "normal" : s.appearance.contrastHidden ? "hidden" : "dimmed",
    netColorMode: s.appearance.netColorMode,
    ratsnest: !s.showRatsnest ? "none" : s.bcx.ratsnestMode === "visible" ? "visible" : "all",
    netColors: { ...s.appearance.netColors },
    netclassColors: { ...s.appearance.netclassColors },
    hiddenNets: [...s.bcx.hiddenRatsnestNets].sort(),
    hiddenNetclasses: [...s.appearance.hiddenNetclasses].sort(),
    preset: s.appearance.activePreset,
    presets: s.appearance.presets.map((p) => p.name),
    viewports: s.appearance.viewports.map((v) => v.name),
    flipped: s.bcx.boardFlipped,
  };
}

/** The requests the hook waits for: the studio's own API calls, not its two polls (`/api/version`, `/api/view`), which never stop. */
export function isTrackedUrl(url: string): boolean {
  return url.includes("/api/") && !/\/api\/(version|view)(\?|$)/.test(url);
}
