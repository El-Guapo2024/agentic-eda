// The schematic editor's own UI state for the "control" actions (`eeschema.EditorControl.*`, `eeschema.InspectionTool.*`, `eeschema.NavigateTool.*`):
// the View menu's display toggles (EESCHEMA_SETTINGS::m_Appearance, saved in the browser like KiCad saves them in its settings file), which docked
// panel is open, and which of the control dialogs is showing. A small store of its own so `store.tsx` -- shared by every editor -- does not grow a
// dozen schematic-only fields; `App.tsx` mounts the provider and `useActionRunner` reads it.
import React, { createContext, useContext, useEffect, useReducer } from "react";
import { DEFAULT_SCH_DISPLAY, sanitizeDisplay, type SchDisplayOptions } from "../components/schematic/displayOptions";

/** A control dialog that is open, with whatever it was opened for. */
export type SchControlDialog =
  | { kind: "bus_syntax" }
  | { kind: "increment_annotations" }
  /** Edit Sheet Page Number for the sheet at this path (root to it); `[]` is the root sheet. */
  | { kind: "page_number"; path: string[] }
  | { kind: "library_links" }
  | { kind: "assign_footprints" }
  | { kind: "export_symbols" }
  | { kind: "legacy_bom" }
  | { kind: "symbol_check" }
  /** Compare a placed symbol (by reference) with its library symbol. */
  | { kind: "symbol_diff"; ref: string };

export interface SchControlState {
  display: SchDisplayOptions;
  /** View > Panels > Net Navigator. */
  netNavigatorOpen: boolean;
  /** The Net Navigator's filter text (`m_netNavigatorFilterValue`). */
  netFilter: string;
  dialog: SchControlDialog | null;
  /** Generate Bill of Materials: open the Symbol Fields Table on its Export tab (`ShowExportTab`) -- the dialog takes this once when it opens. */
  fieldsTableOnExport: boolean;
}

export type SchControlAction =
  | { type: "TOGGLE_DISPLAY"; key: keyof SchDisplayOptions }
  | { type: "SET_NET_NAVIGATOR"; open: boolean }
  | { type: "SET_NET_FILTER"; text: string }
  | { type: "OPEN_DIALOG"; dialog: SchControlDialog }
  | { type: "CLOSE_DIALOG" }
  | { type: "SET_FIELDS_TABLE_ON_EXPORT"; on: boolean };

const STORAGE_KEY = "eda-studio.sch-display";

/** The browser's local storage, or null where it is absent or reading it throws (private windows, blocked site data, node tests). */
function browserStorage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

function loadDisplay(): SchDisplayOptions {
  try {
    const raw = browserStorage()?.getItem(STORAGE_KEY);
    return sanitizeDisplay(raw ? JSON.parse(raw) : null);
  } catch {
    return DEFAULT_SCH_DISPLAY;
  }
}

export function initialSchControlState(display: SchDisplayOptions = DEFAULT_SCH_DISPLAY): SchControlState {
  return { display, netNavigatorOpen: false, netFilter: "", dialog: null, fieldsTableOnExport: false };
}

export function schControlReducer(state: SchControlState, action: SchControlAction): SchControlState {
  switch (action.type) {
    case "TOGGLE_DISPLAY":
      return { ...state, display: { ...state.display, [action.key]: !state.display[action.key] } };
    case "SET_NET_NAVIGATOR":
      return { ...state, netNavigatorOpen: action.open };
    case "SET_NET_FILTER":
      return { ...state, netFilter: action.text };
    case "OPEN_DIALOG":
      return { ...state, dialog: action.dialog };
    case "CLOSE_DIALOG":
      return { ...state, dialog: null };
    case "SET_FIELDS_TABLE_ON_EXPORT":
      return { ...state, fieldsTableOnExport: action.on };
    default:
      return state;
  }
}

const StateContext = createContext<SchControlState | null>(null);
const DispatchContext = createContext<React.Dispatch<SchControlAction> | null>(null);

export function SchControlProvider({ children }: { children: React.ReactNode }) {
  const [state, dispatch] = useReducer(schControlReducer, undefined, () => initialSchControlState(loadDisplay()));
  // The toggles persist (KiCad keeps them in eeschema.json); the panels and dialogs do not.
  useEffect(() => {
    try {
      browserStorage()?.setItem(STORAGE_KEY, JSON.stringify(state.display));
    } catch {
      /* blocked site data: the toggles simply start from the defaults next time */
    }
  }, [state.display]);
  return (
    <StateContext.Provider value={state}>
      <DispatchContext.Provider value={dispatch}>{children}</DispatchContext.Provider>
    </StateContext.Provider>
  );
}

export function useSchControlState(): SchControlState {
  const ctx = useContext(StateContext);
  if (!ctx) throw new Error("useSchControlState outside SchControlProvider");
  return ctx;
}

export function useSchControlDispatch(): React.Dispatch<SchControlAction> {
  const ctx = useContext(DispatchContext);
  if (!ctx) throw new Error("useSchControlDispatch outside SchControlProvider");
  return ctx;
}
