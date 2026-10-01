// Shapes for the data files under src/kicad/*.json, produced by the
// extraction scripts in web/studio/tools/ (see tools/README inline
// comments in each script). Nothing in this file is KiCad-derived data
// itself -- it is only the schema the extractors fill in and the app
// reads. Keep this in sync with tools/extract-*.js if either side's
// shape changes.

export interface ExtractionMeta {
  /** False until a tools/extract-*.js run has actually populated this file. */
  generated: boolean;
  /** Full commit hash of KiCad's origin/master this was extracted from. */
  kicadCommit: string | null;
  /** That commit's author/commit date, as `git show -s --format=%cI`. */
  kicadCommitDate: string | null;
  /** When the extraction script last ran, ISO 8601. */
  extractedAt: string | null;
  /** Paths read, relative to the KiCad repo root, for traceability. */
  sourceFiles: string[];
  /** Free-text: why this is empty, or anything a re-run should know. */
  note?: string;
}

// ------------------------------------------------------------- actions

/** One `TOOL_ACTION` registration (pcbnew/tools/pcb_actions.cpp, common/tool/actions.cpp, ...). */
export interface KicadAction {
  /** Dotted action id, e.g. "pcbnew.InteractiveRouter.Route". */
  name: string;
  /** Menu/tooltip label (`_( "..." )` in the ctor). */
  label: string;
  tooltip: string;
  /** Default hotkey, normalized (best-effort) to "Ctrl+Shift+X" style. Already "the Windows/Linux default"; macOS uses this same string with Cmd standing in for Ctrl UNLESS `macHotkey` overrides it (see below). */
  hotkey: string | null;
  altHotkey: string | null;
  /** The C++ expression the normalizer read `hotkey` from, e.g. "MD_CTRL + 'X'" -- kept so a wrong guess is visible and fixable rather than silently swallowed. */
  hotkeyRaw: string | null;
  altHotkeyRaw: string | null;
  /**
   * A handful of actions (redo, delete, the F1/F2/F5/Home zoom actions)
   * give macOS a genuinely different default in source
   * (`#if defined( __WXMAC__ )`), not just Cmd standing in for Ctrl --
   * e.g. zoomIn is bare F1 on Windows/Linux but Cmd+'+' on macOS, and
   * delete is Del vs. Backspace. Null means there's no such override:
   * macOS uses `hotkey`/`altHotkey` as-is (with Cmd for Ctrl, per usual).
   * Use `effectiveHotkey()` (actions/hotkeys.ts) rather than reading
   * `hotkey`/`macHotkey` directly, so every call site resolves this the
   * same way.
   */
  macHotkey: string | null;
  macAltHotkey: string | null;
  macHotkeyRaw: string | null;
  macAltHotkeyRaw: string | null;
  /** `BITMAPS::` enumerator name, or null for text-only actions. */
  icon: string | null;
  /** AF_* flags and similar (e.g. checkable/toggle markers). */
  flags: string[];
}

export interface ActionsFile {
  meta: ExtractionMeta;
  actions: KicadAction[];
}

// ------------------------------------------------------------ toolbars

export type ToolbarItem =
  | { type: "action"; action: string }
  | { type: "separator" }
  /** A dropdown/split button grouping several related actions (e.g. the Grid menu on the options toolbar). */
  | { type: "group"; label: string; icon: string | null; items: string[] }
  /** A non-action widget: a combo box or text control (grid size, zoom %, track width, via size, net class, ...). */
  | { type: "control"; control: string; label: string | null };

export type ToolbarId = "main" | "options" | "drawing" | "auxiliary";

export interface ToolbarConfig {
  id: ToolbarId;
  orientation: "horizontal" | "vertical";
  items: ToolbarItem[];
}

export interface ToolbarsFile {
  meta: ExtractionMeta;
  toolbars: ToolbarConfig[];
}

// --------------------------------------------------------------- menus

export type MenuNode = { type: "separator" } | { type: "item"; action: string } | { type: "submenu"; label: string; items: MenuNode[] };

export interface MenuConfig {
  label: string;
  items: MenuNode[];
}

export interface MenusFile {
  meta: ExtractionMeta;
  menus: MenuConfig[];
}

// -------------------------------------------------------------- colors

export interface ColorsFile {
  meta: ExtractionMeta;
  themeName: string;
  /** Layer/GAL-layer id -> CSS color, e.g. "F_Cu" -> "#c83434e6" (KiCad packs colors RGBA). */
  colors: Record<string, string>;
}

// -------------------------------------------------------------- layers

export interface LayerInfo {
  /** Symbolic id as KiCad's PCB_LAYER_ID enum names it, e.g. "F_Cu". */
  id: string;
  numericId: number | null;
  /** Canonical short name, e.g. "F.Cu". */
  name: string;
  /** Appearance panel description, e.g. "Front copper". */
  description: string;
}

export interface LayersFile {
  meta: ExtractionMeta;
  layers: LayerInfo[];
  /** GAL draw order, back to front, by layer/GAL-layer id. */
  drawOrder: string[];
}

// --------------------------------------------------------------- icons

export interface IconsFile {
  meta: ExtractionMeta;
  /** `BITMAPS::` enumerator name -> filename under public/icons/{light,dark}/. */
  icons: Record<string, string>;
}
