// `window.__eda`: the scripted test hook. Installed once by `App.tsx` (`useEdaTestHook()` next to the global hotkeys), it lets an agent or a test script drive and read the
// studio without clicking toolbar buttons and reading screenshots. The contract, and the pure model behind it, is in kicad-port/edaTestHook.ts:
//
//   __eda.actions({ all? })    -> [{ id, label, enabled, reason? }]   the actions the runner handles on this tab (all: plus the KiCad actions with no handler)
//   await __eda.run(id, args?) -> { ok, error?, revision, dialog?, toast? }   runs the action as a menu click does and waits for its /api/ round trips
//   __eda.state()              -> { tab, revision, tool, picker, selection: [{ id, kind }], entered, counts, grid, dialogs, open }
//   __eda.errors(since?)       -> [{ time, message }]   console.error, uncaught errors, rejected promises, 5xx replies and the error toasts since the page loaded
//
// This file is the page glue: the capture that has to start at load (console.error, window errors, the fetch wrapper that counts the studio's requests and reads the
// replies of /api/cmd), and the hook that reads the stores. Importing it starts the capture; nothing here changes what the studio does.
import { useEffect, useRef } from "react";
import actionsData from "../kicad/actions.json";
import type { ActionsFile } from "../kicad/types";
import { fetchVersion } from "../api/client";
import { useStudioApi, useStudioState } from "../state/store";
import { useFpState } from "../state/footprintEditorStore";
import { useSymState } from "../state/symbolEditorStore";
import { useCommonDialogs } from "../state/commonDialogs";
import { useActionRunner } from "./useActionRunner";
import { picker } from "./pcbPicker";
import {
  ErrorLog,
  boardLists,
  countsOf,
  describeActions,
  disabledReason,
  footprintLists,
  formatArgs,
  isTrackedUrl,
  kindIndex,
  newNames,
  openFlags,
  runOutcome,
  schematicLists,
  selectionWithKinds,
  settle,
  symbolLists,
  type ActionInfo,
  type CmdOutcome,
  type Counts,
  type ErrorEntry,
  type ItemLists,
  type RunResult,
  type SelectedItem,
} from "../kicad-port/edaTestHook";

export interface HookState {
  tab: string;
  revision: string | null;
  tool: string | null;
  /** The prompt of the picker session running (the delete tool's "Delete: click an item to delete it"), or null. */
  picker: string | null;
  selection: SelectedItem[];
  /** The group being worked in on the board (`PCB_SELECTION_TOOL::m_enteredGroup`), or null. */
  entered: string | null;
  counts: Counts;
  /** The grid of the editor on screen, in um; null on the tabs whose grid is not a choice (the schematic's is the fixed 50 mil, the 3D viewer has none). */
  grid: number | null;
  dialogs: string[];
  open: string[];
  /** The view of the canvas on screen (the schematic's or the board's): a point `(x, y)` of the sheet is at `(view.x + x * view.scale, view.y + y * view.scale)` pixels from the canvas's top-left corner; null on the other tabs. */
  view: { x: number; y: number; scale: number } | null;
}

export interface EdaTestHook {
  actions(opts?: { all?: boolean }): ActionInfo[];
  run(id: string, args?: unknown): Promise<RunResult>;
  state(): HookState;
  errors(since?: number): ErrorEntry[];
}

declare global {
  interface Window {
    __eda?: EdaTestHook;
  }
}

// ------------------------------------------------------------------------------------------------------------------------------ capture (at load)

const errorLog = new ErrorLog();
/** The studio's own requests in flight (`isTrackedUrl`); the replies of the commands it sent since the run began; the last toast shown since then (any kind). */
let inFlight = 0;
const cmdReplies: CmdOutcome[] = [];
let lastToast: string | null = null;

function installCapture(): void {
  if (typeof window === "undefined") return;
  const marker = "__edaCaptureInstalled";
  const w = window as unknown as Record<string, unknown>;
  if (w[marker]) return;
  w[marker] = true;

  const originalError = console.error.bind(console);
  console.error = (...args: unknown[]) => {
    errorLog.add(formatArgs(args));
    originalError(...args);
  };
  window.addEventListener("error", (e) => errorLog.add(e.message || (e.error ? String(e.error) : "")));
  window.addEventListener("unhandledrejection", (e) => errorLog.add(`unhandled rejection: ${e.reason instanceof Error ? e.reason.message : String(e.reason)}`));

  const originalFetch = window.fetch.bind(window);
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    if (!isTrackedUrl(url)) return originalFetch(input, init);
    inFlight++;
    try {
      const response = await originalFetch(input, init);
      if (response.status >= 500) errorLog.add(`${init?.method ?? "GET"} ${url} -> ${response.status}`);
      // A command's reply says whether it was accepted (`{ ok, message }`): read from a copy so the studio still gets the body.
      if (/\/api\/cmd(\?|$)/.test(url)) {
        const reply = (await response.clone().json().catch(() => null)) as { ok?: boolean; message?: string } | null;
        if (reply) cmdReplies.push({ ok: reply.ok === true, message: reply.message });
      }
      return response;
    } catch (e) {
      errorLog.add(`${init?.method ?? "GET"} ${url}: ${e instanceof Error ? e.message : String(e)}`);
      throw e;
    } finally {
      inFlight--;
    }
  };
}

installCapture();

// ------------------------------------------------------------------------------------------------------------------------------------------ hook

const delay = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
const labels = new Map((actionsData as ActionsFile).actions.map((a) => [a.name, a.label]));
const everyKicadAction = (actionsData as ActionsFile).actions.map((a) => a.name);

/** The titles of the dialogs on screen: each `.dialog`'s header, or its aria-label. */
function dialogTitles(): string[] {
  return [...document.querySelectorAll<HTMLElement>(".dialog-backdrop .dialog, [role='dialog']")].map((d) => (d.querySelector(".dialog-header")?.textContent ?? d.getAttribute("aria-label") ?? "dialog").trim());
}

interface Latest {
  runner: ReturnType<typeof useActionRunner>;
  studio: ReturnType<typeof useStudioState>;
  fp: ReturnType<typeof useFpState>;
  sym: ReturnType<typeof useSymState>;
  common: ReturnType<typeof useCommonDialogs>;
  getStudio: () => ReturnType<typeof useStudioState>;
}

function listsFor(L: Latest): { tool: string | null; selection: ReadonlySet<string>; lists: ItemLists } {
  const studio = L.getStudio();
  switch (studio.tab) {
    case "pcb":
      return { tool: studio.activeTool, selection: studio.selection, lists: boardLists(studio.board) };
    case "schematic":
      return { tool: studio.activeTool, selection: studio.selection, lists: schematicLists(studio.schematic) };
    case "footprint":
      return { tool: L.fp.activeTool, selection: L.fp.selection, lists: footprintLists(L.fp.footprint) };
    case "symbol":
      return { tool: L.sym.activeTool, selection: L.sym.selection, lists: symbolLists(L.sym.symbol) };
    default:
      return { tool: null, selection: new Set<string>(), lists: {} };
  }
}

function openNames(L: Latest): string[] {
  const out = [...openFlags(L.getStudio() as unknown as Record<string, unknown>), ...openFlags(L.fp as unknown as Record<string, unknown>).map((n) => `fp.${n}`), ...openFlags(L.sym as unknown as Record<string, unknown>).map((n) => `sym.${n}`)];
  if (L.common.about) out.push("common.about");
  if (L.common.page) out.push(`common.page:${L.common.page}`);
  if (L.common.group) out.push("common.group");
  if (L.common.gridOrigin) out.push("common.gridOrigin");
  if (L.common.grids) out.push(`common.grids:${L.common.grids}`);
  return out.sort();
}

function build(latest: { current: Latest }): EdaTestHook {
  const version = () => fetchVersion().catch(() => null);
  return {
    actions(opts) {
      const L = latest.current;
      const handled = L.runner.actionNames;
      const have = new Set(handled);
      return describeActions(handled, L.getStudio().tab, labels, opts?.all ? everyKicadAction.filter((id) => !have.has(id)) : []);
    },

    async run(id, args) {
      const L = latest.current;
      const tab = L.getStudio().tab;
      if (!L.runner.isEnabled(id)) return { ok: false, error: disabledReason(id, tab, L.runner.actionNames.includes(id)) ?? "the action is not enabled", revision: await version() };
      const started = Date.now();
      const dialogsBefore = dialogTitles();
      cmdReplies.length = 0;
      lastToast = null;
      let thrown: unknown;
      try {
        await L.runner.run(id, args);
      } catch (e) {
        thrown = e ?? new Error("the action threw");
      }
      // The round trips it started, and the render that shows their result.
      const settled = await settle({ pending: () => inFlight, delay, now: Date.now });
      await delay(30);
      const outcome = runOutcome(thrown, cmdReplies, errorLog.list(started));
      const opened = newNames(dialogsBefore, dialogTitles())[0];
      return { ...outcome, revision: await version(), ...(opened ? { dialog: opened } : {}), ...(lastToast ? { toast: lastToast } : {}), ...(settled === "timeout" ? { pending: true } : {}) };
    },

    state() {
      const L = latest.current;
      const studio = L.getStudio();
      const { tool, selection, lists } = listsFor(L);
      return {
        tab: studio.tab,
        revision: studio.version,
        tool,
        picker: picker.session()?.prompt ?? null,
        selection: selectionWithKinds(selection, kindIndex(lists)),
        entered: studio.tab === "pcb" ? studio.enteredGroupId : null,
        counts: countsOf(studio.board, studio.schematic),
        grid: studio.tab === "pcb" ? studio.gridUm : studio.tab === "footprint" ? L.fp.gridUm : studio.tab === "symbol" ? L.sym.gridUm : null,
        dialogs: dialogTitles(),
        open: openNames(L),
        view: studio.tab === "schematic" ? { ...studio.schematicView } : studio.tab === "pcb" ? { ...studio.view } : null,
      };
    },

    errors(since) {
      return errorLog.list(since);
    },
  };
}

/** Installs `window.__eda` while the studio is mounted. Call once, near the app's root. */
export function useEdaTestHook(): void {
  const api = useStudioApi();
  const runner = useActionRunner();
  const studio = useStudioState();
  const fp = useFpState();
  const sym = useSymState();
  const common = useCommonDialogs();
  const latest = useRef<Latest>(null as unknown as Latest);
  latest.current = { runner, studio, fp, sym, common, getStudio: () => api.getState() };

  // The error toasts and the board error are errors the person sees: they go into the log, and the last toast of any kind is what a run reports.
  const noteToast = (toast: { message: string; kind: "error" | "info" } | null) => {
    if (!toast) return;
    lastToast = toast.message;
    if (toast.kind === "error") errorLog.add(toast.message);
  };
  useEffect(() => noteToast(studio.toast), [studio.toast]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => noteToast(fp.toast), [fp.toast]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => noteToast(sym.toast), [sym.toast]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (studio.boardError) errorLog.add(studio.boardError);
  }, [studio.boardError]);

  useEffect(() => {
    const hook = build(latest as { current: Latest });
    window.__eda = hook;
    return () => {
      if (window.__eda === hook) delete window.__eda;
    };
  }, []);
}
