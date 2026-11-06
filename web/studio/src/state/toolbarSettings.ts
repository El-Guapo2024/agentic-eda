// The editors' toolbars: KiCad's defaults (the extracted src/kicad/*toolbars.json, `<FRAME>_TOOLBAR_SETTINGS::DefaultToolbarConfig`) and the person's customised
// ones (Preferences > <editor> > Toolbars, components/ToolbarsPage.tsx), with the rules of kicad-port/toolbarCustomization.ts. components/Toolbar.tsx and
// components/viewer3d/Viewer3DToolbar.tsx draw what `resolvedToolbar` gives: with "Customize toolbars" on (`m_CustomToolbars`) a toolbar that has a stored
// configuration shows it, otherwise the default (`TOOLBAR_SETTINGS::GetToolbarConfig( loc, config()->m_CustomToolbars )`, EDA_BASE_FRAME::configureToolbars).
//
// Where they are kept: in the browser's local storage (`eda-studio.toolbars.v1`), as the studio keeps the person's colour themes and other preferences
// (state/colorThemeStore.ts). KiCad keeps a frame's toolbars in the user's configuration folder (`pcbnew-toolbars.json`, `fpedit-toolbars.json`, ...) and its
// `appearance.custom_toolbars` in the frame's settings file: per user, never per project; the store's text holds each frame's under that file's name, in
// KiCad's JSON form.
import { useSyncExternalStore } from "react";
import toolbarsData from "../kicad/toolbars.json";
import schToolbarsData from "../kicad/sch_toolbars.json";
import fpToolbarsData from "../kicad/fp_toolbars.json";
import symToolbarsData from "../kicad/sym_toolbars.json";
import viewer3dToolbarsData from "../kicad/viewer3d_toolbars.json";
import actionsData from "../kicad/actions.json";
import { toolbarsWithoutAdvanced } from "../kicad/advancedConfig";
import { KICAD10_ACTIONS, SCH_ARC_GROUP } from "../kicad/kicad10Actions";
import type { ActionsFile, ToolbarConfig, ToolbarId, ToolbarItem, ToolbarsFile, Viewer3dToolbarsFile } from "../kicad/types";
import {
  EMPTY_FRAME_TOOLBAR_SETTINGS,
  LOC_OF_TOOLBAR_ID,
  configOfStudioItems,
  parseToolbarSettings,
  serializeToolbarSettings,
  studioItemsOfConfig,
  toolbarConfig,
  type FrameToolbarSettings,
  type RenderToolbarItem,
  type StoredToolbarSettings,
  type TbConfig,
  type ToolbarAction,
  type ToolbarFrame,
  type ToolbarLoc,
} from "../kicad-port/toolbarCustomization";

const STORAGE_KEY = "eda-studio.toolbars.v1";

/** The schematic's drawing toolbar with KiCad 25ec2335's Arc group (the five draw modes) where 8303b2ad's Draw Arcs button is. */
function withKicad10ArcGroup(file: ToolbarsFile): ToolbarsFile {
  const swap = (it: ToolbarItem): ToolbarItem => (it.type === "action" && it.action === "eeschema.InteractiveDrawing.drawArc" ? SCH_ARC_GROUP : it);
  return { ...file, toolbars: file.toolbars.map((t) => ({ ...t, items: t.items.map(swap) })) };
}

const viewer3dFile = viewer3dToolbarsData as unknown as Viewer3dToolbarsFile;

/** Each frame's default toolbars: the extracted ones (the board's without the buttons KiCad shows only with an advanced setting: kicad/advancedConfig.ts). */
export const DEFAULT_TOOLBAR_FILES: Record<ToolbarFrame, ToolbarsFile> = {
  pcb: toolbarsWithoutAdvanced(toolbarsData as ToolbarsFile),
  // eeschema has no auxiliary toolbar at all (toolbars_sch_editor.cpp's TOOLBAR_LOC::TOP_AUX case returns std::nullopt)
  schematic: withKicad10ArcGroup(schToolbarsData as ToolbarsFile),
  footprint: fpToolbarsData as ToolbarsFile,
  symbol: symToolbarsData as ToolbarsFile,
  "3d": viewer3dFile,
};

const actionsFile = actionsData as ActionsFile;

/**
 * Every action a toolbar can name (`ACTION_MANAGER::GetActionList()`): the extracted table's, KiCad 10's newer ones (kicad/kicad10Actions.ts) and the 3D viewer's
 * (`EDA_3D_ACTIONS`, from viewer3d_toolbars.json), each with its friendly name, tooltip and icon.
 */
export const TOOLBAR_ACTIONS: readonly ToolbarAction[] = (() => {
  const out = new Map<string, ToolbarAction>();
  for (const a of actionsFile.actions) out.set(a.name, { name: a.name, label: a.label, tooltip: a.tooltip, icon: a.icon });
  for (const a of KICAD10_ACTIONS) if (!out.has(a.name)) out.set(a.name, { name: a.name, label: a.label, tooltip: a.tooltip, icon: a.icon });
  for (const a of [...viewer3dFile.actions, ...(viewer3dFile.otherActions ?? [])]) if (!out.has(a.name)) out.set(a.name, { name: a.name, label: a.label, tooltip: a.tooltip, icon: a.icon });
  return [...out.values()];
})();

const actionByName = new Map(TOOLBAR_ACTIONS.map((a) => [a.name, a]));

export function toolbarAction(name: string): ToolbarAction | undefined {
  return actionByName.get(name);
}

/** The default toolbar of a frame at a location, as KiCad's configuration (`DefaultToolbarConfig`), or none where the frame has no such toolbar. */
export function defaultToolbarConfigs(frame: ToolbarFrame): Partial<Record<ToolbarLoc, TbConfig>> {
  const out: Partial<Record<ToolbarLoc, TbConfig>> = {};
  for (const tb of DEFAULT_TOOLBAR_FILES[frame].toolbars) {
    // the 3D file has one toolbar with an id; empty entries carry none
    if (!tb || !tb.id || !(tb.id in LOC_OF_TOOLBAR_ID)) continue;
    const loc = LOC_OF_TOOLBAR_ID[tb.id];
    if (!out[loc]) out[loc] = configOfStudioItems(frame, tb.items);
  }
  return out;
}

const DEFAULT_CONFIGS: Record<ToolbarFrame, Partial<Record<ToolbarLoc, TbConfig>>> = {
  pcb: defaultToolbarConfigs("pcb"),
  schematic: defaultToolbarConfigs("schematic"),
  footprint: defaultToolbarConfigs("footprint"),
  symbol: defaultToolbarConfigs("symbol"),
  "3d": defaultToolbarConfigs("3d"),
};

export function defaultConfigsOf(frame: ToolbarFrame): Partial<Record<ToolbarLoc, TbConfig>> {
  return DEFAULT_CONFIGS[frame];
}

/** The label the default toolbars give a control of a frame (`Track Width`, `Grid selector`, ...), kept when the person moves it. */
const CONTROL_LABELS: Record<ToolbarFrame, Map<string, string>> = (() => {
  const out = {} as Record<ToolbarFrame, Map<string, string>>;
  for (const f of Object.keys(DEFAULT_TOOLBAR_FILES) as ToolbarFrame[]) {
    const m = new Map<string, string>();
    for (const tb of DEFAULT_TOOLBAR_FILES[f].toolbars) for (const it of tb?.items ?? []) if (it.type === "control" && it.label && !m.has(it.control)) m.set(it.control, it.label);
    out[f] = m;
  }
  return out;
})();

// ------------------------------------------------------------------------------------------------ the store

function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

interface Snapshot {
  stored: StoredToolbarSettings;
  version: number;
}

function load(): StoredToolbarSettings {
  try {
    return parseToolbarSettings(storage()?.getItem(STORAGE_KEY) ?? null);
  } catch {
    return {};
  }
}

let current: Snapshot = { stored: load(), version: 0 };
const listeners = new Set<() => void>();
/** The resolved toolbars, by frame and location, until the store changes. */
let cache = new Map<string, ResolvedToolbar | null>();

function commit(stored: StoredToolbarSettings): void {
  current = { stored, version: current.version + 1 };
  cache = new Map();
  try {
    storage()?.setItem(STORAGE_KEY, serializeToolbarSettings(stored));
  } catch {
    /* private window or quota: the toolbars still apply for this session */
  }
  for (const l of [...listeners]) l();
}

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

function getSnapshot(): Snapshot {
  return current;
}

/** The person's toolbar settings, re-rendering the caller when they change. */
export function useToolbarSettings(): Snapshot {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

/** One frame's settings: "Customize toolbars" and the stored toolbars. */
export function frameToolbarSettings(frame: ToolbarFrame): FrameToolbarSettings {
  return current.stored[frame] ?? EMPTY_FRAME_TOOLBAR_SETTINGS;
}

/** The page's OK (`TransferDataFromWindow`): the frame's `m_CustomToolbars` and every toolbar it stores (`SetStoredToolbarConfig`). */
export function setFrameToolbarSettings(frame: ToolbarFrame, settings: FrameToolbarSettings): void {
  commit({ ...current.stored, [frame]: settings });
}

/** For the tests and the scripted hook: no customised toolbars anywhere. */
export function resetToolbarSettingsForTest(): void {
  commit({});
}

/** A toolbar as an editor draws it; `custom`: the person's stored toolbar, not the default. */
export interface ResolvedToolbar {
  items: RenderToolbarItem[];
  orientation: ToolbarConfig["orientation"];
  custom: boolean;
}

/**
 * The items a frame's toolbar draws (`configureToolbars` -> `ApplyConfiguration`), and its orientation: the default toolbar as extracted, or the stored one when the
 * frame's "Customize toolbars" is on and it has one; null where the frame has no such toolbar.
 */
export function resolvedToolbar(frame: ToolbarFrame, id: ToolbarId): ResolvedToolbar | null {
  const key = `${frame}/${id}`;
  if (cache.has(key)) return cache.get(key)!;
  const file = DEFAULT_TOOLBAR_FILES[frame].toolbars.find((t) => t?.id === id);
  const orientation: ToolbarConfig["orientation"] = file?.orientation ?? (id === "options" || id === "drawing" ? "vertical" : "horizontal");
  let out: ResolvedToolbar | null = null;
  if (file) {
    const s = frameToolbarSettings(frame);
    const stored = s.custom ? s.toolbars[LOC_OF_TOOLBAR_ID[id]] : undefined;
    if (!stored) out = { items: file.items, orientation, custom: false };
    else {
      const config = toolbarConfig(DEFAULT_CONFIGS[frame], s.toolbars, LOC_OF_TOOLBAR_ID[id], true) ?? [];
      const items = studioItemsOfConfig(frame, config, {
        hasTool: (name) => actionByName.has(name),
        iconOf: (name) => actionByName.get(name)?.icon ?? null,
        controlLabel: (cid) => CONTROL_LABELS[frame].get(cid),
      });
      out = { items, orientation, custom: true };
    }
  }
  cache.set(key, out);
  return out;
}

/** What the scripted hook reports (`__eda.state().toolbars`): each frame's "Customize toolbars" and the locations it stores. */
export function toolbarSettingsSummary(): Record<string, { custom: boolean; stored: string[] }> {
  const out: Record<string, { custom: boolean; stored: string[] }> = {};
  for (const [f, v] of Object.entries(current.stored)) if (v) out[f] = { custom: v.custom, stored: Object.keys(v.toolbars) };
  return out;
}
