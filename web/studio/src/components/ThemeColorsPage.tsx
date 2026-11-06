// The Colors pages of Preferences that edit a theme's swatches: `PANEL_COLOR_SETTINGS` (common/dialogs/panel_color_settings.cpp at KiCad 8303b2ad) as
// PCB Editor > Colors (`PANEL_PCBNEW_COLOR_SETTINGS`, the board namespace), Footprint Editor > Colors (`PANEL_FP_EDITOR_COLOR_SETTINGS`, the board
// namespace over the Footprint Editor's own theme) and Schematic Editor > Colors (`PANEL_EESCHEMA_COLOR_SETTINGS`, the schematic namespace, with "Override
// individual item colors"). The rules are kicad-port/colorTheme.ts'; the themes are the person's (state/colorThemeStore.ts).
//
//   * "Theme:" lists the themes (`createThemeList`), "---" and "New Theme..."; picking a theme first saves the one shown (`saveCurrentTheme( false )`)
//     and shows the other's colours (`OnThemeChanged`); picking "---" puts the selection back.
//   * New Theme... asks "New theme name:" in "Add Color Theme" (`FOOTPRINT_NAME_VALIDATOR`: `%$<>\t\n\r"\/:` cannot be typed); a name taken says
//     "Theme already exists!"; the new theme has the shown theme's colours of the page's layers and is selected.
//   * A swatch per layer in the page's order, each on the theme's background; a double click or a middle click opens the colour dialog with KiCad's
//     default as its Reset to Default (`createSwatch`: `selected->GetDefaultColor( aLayer )`), and OK sets the colour and saves the theme at once
//     (`updateColor`: "Colors must be persisted when edited"); a read-only theme's swatches do not open. A right click: Copy color, Paste color, Revert to
//     saved color (`ShowColorContextMenu`).
//   * The schematic page saves no layer but the backgrounds pure white (`PANEL_EESCHEMA_COLOR_SETTINGS::saveCurrentTheme`), and shows Sheet borders and
//     Sheet backgrounds only while "Override individual item colors" is on (`updateAllowedSwatches`); the box is dimmed on a read-only theme.
//   * The dialog's "Reset Colors to Defaults" (`PAGED_DIALOG`: "Reset %s to Defaults", tooltip "Reset all colors in this theme to the KiCad
//     defaults") is `ResetPanel`: every colour of the page to KiCad's default, nothing on a read-only theme.
//   * OK (`TransferDataFromWindow`) makes the shown theme the editor's (`m_ColorTheme`); Cancel keeps the old one.
//
// Different from KiCad: no preview board or sheet (`m_preview`): the page's colours are drawn on the editor's canvas itself while the dialog is up
// (`setEditorThemePreview`), and the canvas goes back to the editor's theme on Cancel. A reset is saved by OK here; in KiCad it waits in the page's
// working copy until the next swatch edit or theme change saves it (`ResetPanel` does not call `saveCurrentTheme`). The schematic page's OK does not ask
// `validateSave`'s "Some items have the same color as the background ..." question. In place of "Open Theme Folder" (the themes are the browser's, not
// files in a folder) the page has Import Theme... and Export Theme...: a KiCad theme file (`colors/<name>.json`) read into the person's themes and
// selected, or the shown theme written as KiCad writes it (kicad-port/colorThemeFile.ts).
import { useEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { THEME_DATA, THEME_PARAMS, getColorThemes, replaceStoredThemes, selectEditorTheme, setEditorThemePreview, setThemeColor, useColorThemes } from "../state/colorThemeStore";
import { SWATCH_MENU_LABELS, addTheme, defaultColor, findTheme, importThemes, resetThemeColors, sanitizeThemeName, swatchMenu, themeColor, themeListEntries, type ColorTheme, type ThemeEditor } from "../kicad-port/colorTheme";
import { parseThemeFile, themeFilenameOf, writeThemeFile } from "../kicad-port/colorThemeFile";
import { parseCssColor, toCssColor, toHex8 } from "../kicad-port/appearance";
import { saveTextFile } from "../api/libraryClient";
import { ColorDialog, MessageDialog, NameDialog } from "./panels/appearance/shared";
import { ContextMenu } from "./canvas/ContextMenu";
import type { PreferencesPageHandle } from "./preferencesPage";

/** What makes one editor's page: its editor, its swatches (they may depend on the override box) and its layers. */
export interface ThemeColorsPageConfig {
  editor: Exclude<ThemeEditor, "symbol">;
  /** The `data-testid`s' stem: `<stem>-page`, `<stem>-theme`. */
  testId: string;
  /** The swatches shown, in order (`createSwatches`). */
  rows: (overrideItemColors: boolean) => Array<{ key: string; label: string }>;
  /** `m_validLayers`: what New Theme... copies, Reset resets and a save writes. */
  pageKeys: readonly string[];
  /** `m_backgroundLayer`: what each swatch is drawn on. */
  backgroundKey: string;
  /** The schematic page's "Override individual item colors" (`m_optOverrideColors`, hidden on the board pages). */
  overrideOption?: boolean;
  /** The page's own `saveCurrentTheme` step before the save (the schematic's no-white rule). */
  saveColors?: (colors: Readonly<Record<string, string>>) => Record<string, string>;
}

/** The page's working copy (`m_currentSettings`) of a theme: every colour of the page resolved, and its override flag. */
function workingCopy(theme: ColorTheme, keys: readonly string[]): ColorTheme {
  const colors: Record<string, string> = {};
  for (const k of keys) {
    const c = themeColor(theme, k, THEME_DATA);
    if (c) colors[k] = c;
  }
  return { ...theme, colors, overrideSchItemColors: theme.overrideSchItemColors ?? false };
}

/** A theme file as the shown theme: the saved theme with the page's colours and override flag over it. */
export function themeFileText(working: ColorTheme): string {
  const saved = findTheme(getColorThemes().registry, working.filename);
  return writeThemeFile({ ...saved, colors: { ...saved.colors, ...working.colors }, overrideSchItemColors: working.overrideSchItemColors }, THEME_PARAMS, THEME_DATA);
}

/** Reads the files picked for Import Theme... into the person's themes: the filenames of the themes they gave, or the first file's error. */
export async function importThemeFiles(files: readonly File[]): Promise<{ filenames: string[]; error?: string }> {
  const read = [];
  for (const f of files) {
    const r = parseThemeFile(await f.text(), themeFilenameOf(f.name), THEME_PARAMS);
    if (r.error) return { filenames: [], error: `${f.name}: ${r.error}` };
    read.push(...r.themes);
  }
  const r = importThemes(getColorThemes().stored, read);
  replaceStoredThemes(r.stored);
  return { filenames: r.filenames };
}

/** The page's state lives with the Preferences dialog (it outlives a switch to another page, as a `wxTreebook` page does). `open`: the dialog is up. */
export function useThemeColorsPage(config: ThemeColorsPageConfig, open: boolean, shown: boolean, rowLabel?: (key: string, label: string) => string): PreferencesPageHandle & { working: ColorTheme } {
  const { editor, pageKeys } = config;
  const themes = useColorThemes();
  const [working, setWorking] = useState<ColorTheme>(() => workingCopy(themes.themes[editor], pageKeys));
  const [copied, setCopied] = useState<string | null>(null);
  const [asking, setAsking] = useState<"name" | "exists" | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ key: string; label: string } | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; key: string } | null>(null);
  const fileInput = useRef<HTMLInputElement | null>(null);
  /** The page shows what its theme has not saved (a reset, the override box): OK saves it. An untouched page saves nothing on OK, so the PCB and the Footprint
   *  Editor pages, which may show one theme's board colours, never put back a colour the other saved. */
  const dirty = useRef(false);

  // Opening the dialog starts the page on the editor's theme, as KiCad's constructor does (`createThemeList( cfg->m_ColorTheme )`).
  useEffect(() => {
    dirty.current = false;
    if (open) setWorking(workingCopy(getColorThemes().themes[editor], pageKeys));
    else setEditorThemePreview(editor, null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);
  // `PANEL_COLOR_SETTINGS::Show`: "In case changes have been made to the current theme in another panel", a page shown again reads its theme again.
  useEffect(() => {
    if (open && shown && !dirty.current) setWorking((w) => workingCopy(findTheme(getColorThemes().registry, w.filename), pageKeys));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shown]);
  // The editor's canvas shows the page's colours while the dialog is up.
  useEffect(() => {
    if (open) setEditorThemePreview(editor, working);
  }, [open, working, editor]);
  useEffect(() => () => setEditorThemePreview(editor, null), [editor]);

  const saved = findTheme(themes.registry, working.filename);
  const readOnly = working.readOnly;
  const override = working.overrideSchItemColors ?? false;
  const rows = useMemo(() => config.rows(override).map((r) => ({ ...r, label: rowLabel ? rowLabel(r.key, r.label) : r.label })), [config, override, rowLabel]);
  const background = working.colors[config.backgroundKey] ?? "#000000ff";

  /** `saveCurrentTheme`: the page's colours (and the override flag) into the theme of the same filename, unless it is read-only. Returns what was saved. */
  const saveCurrentTheme = (w: ColorTheme): ColorTheme => {
    if (w.readOnly) return w;
    const colors = config.saveColors ? config.saveColors(w.colors) : w.colors;
    const out = colors === w.colors ? w : { ...w, colors };
    setThemeColor(out.filename, out.colors, config.overrideOption ? out.overrideSchItemColors : undefined);
    dirty.current = false;
    return out;
  };

  /** `updateColor`: the colour, saved at once with the rest of the page. */
  const updateColor = (key: string, hex8: string) => {
    if (readOnly) return;
    setWorking(saveCurrentTheme({ ...working, colors: { ...working.colors, [key]: hex8 } }));
  };

  const onTheme = (value: string) => {
    if (value === "---") return; // "if( idx == GetCount() - 2 )": the separator puts the selection back
    if (value === "new") {
      saveCurrentTheme(working);
      setAsking("name");
      return;
    }
    if (value === working.filename) return;
    saveCurrentTheme(working);
    dirty.current = false;
    setWorking(workingCopy(findTheme(getColorThemes().registry, value), pageKeys));
  };

  const addNewTheme = (text: string) => {
    const name = sanitizeThemeName(text);
    const r = addTheme(getColorThemes().stored, name, working, pageKeys, THEME_DATA);
    if (r.error) {
      setAsking("exists");
      return;
    }
    replaceStoredThemes(r.stored);
    setAsking(null);
    setWorking(workingCopy(findTheme(getColorThemes().registry, name), pageKeys));
  };

  const onImport = async (files: FileList | null) => {
    if (!files || files.length === 0) return;
    saveCurrentTheme(working);
    const r = await importThemeFiles([...files]);
    if (r.error) {
      setProblem(r.error);
      return;
    }
    const first = r.filenames[0];
    if (first) setWorking(workingCopy(findTheme(getColorThemes().registry, first), pageKeys));
  };

  const swatchCss = (key: string) => {
    const c = working.colors[key] ? parseCssColor(working.colors[key]!) : null;
    return c ? toCssColor(c) : null;
  };

  const openEditor = (key: string, label: string) => {
    if (readOnly) return; // a read-only swatch of the page has no callback: nothing happens
    setEditing({ key, label });
  };

  const element = !shown ? null : (
    <div data-testid={`${config.testId}-page`} style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div className="filter-row" style={{ gap: 8 }}>
        <span>Theme:</span>
        <select aria-label="Theme" data-testid={`${config.testId}-theme`} value={working.filename} onChange={(e) => onTheme(e.target.value)} style={{ minWidth: 220 }}>
          {themeListEntries(themes.registry).map((e, i) =>
            e.kind === "theme" ? (
              <option key={e.filename} value={e.filename}>
                {e.label}
              </option>
            ) : e.kind === "separator" ? (
              <option key={`sep${i}`} value="---">
                ---
              </option>
            ) : (
              <option key="new" value="new">
                New Theme...
              </option>
            )
          )}
        </select>
      </div>
      {config.overrideOption && (
        <label className="filter-row" title="Show all items in their default color even if they have specific colors set in their properties.">
          <input
            type="checkbox"
            data-testid={`${config.testId}-override`}
            checked={override}
            disabled={readOnly}
            onChange={(e) => {
              dirty.current = true;
              const on = e.target.checked;
              setWorking((w) => ({ ...w, overrideSchItemColors: on }));
            }}
          />
          Override individual item colors
        </label>
      )}
      {/* `m_colorsListWindow`: one scrolled column of swatch and name (KiCad's preview would be on its right) */}
      <div role="list" aria-label="Colors" style={{ height: 400, overflowY: "auto", border: "1px solid var(--chrome-border)", padding: "4px 8px", maxWidth: 380 }}>
        {rows.map((r) => (
          <div key={r.key} role="listitem" data-color-key={r.key} className="filter-row" style={{ gap: 8, margin: 0, padding: "2px 0" }}>
            <span
              role="button"
              aria-label={`${r.label} color`}
              title={readOnly ? undefined : "Double click or middle click for color change, right click for menu"}
              style={{ width: 28, height: 16, flexShrink: 0, borderRadius: 2, border: "1px solid var(--chrome-border)", background: parseCssColor(background) ? toCssColor(parseCssColor(background)!) : undefined, position: "relative", cursor: readOnly ? "default" : "pointer" }}
              onDoubleClick={() => openEditor(r.key, r.label)}
              onMouseUp={(e: MouseEvent) => {
                if (e.button === 1) {
                  e.preventDefault();
                  openEditor(r.key, r.label);
                }
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ x: e.clientX, y: e.clientY, key: r.key });
              }}
            >
              <span style={{ position: "absolute", inset: 3, borderRadius: 1, background: swatchCss(r.key) ?? undefined }} />
            </span>
            <span style={{ fontSize: 12, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{r.label}</span>
          </div>
        ))}
      </div>
      <div className="filter-row" style={{ gap: 8 }}>
        <button title="Read a KiCad color theme file (colors/<name>.json) into the themes and show it" onClick={() => fileInput.current?.click()} data-testid={`${config.testId}-import`}>
          Import Theme...
        </button>
        <button title="Save the shown theme as a KiCad color theme file" onClick={() => saveTextFile(themeFileText(working), `${working.filename}.json`, "application/json")} data-testid={`${config.testId}-export`}>
          Export Theme...
        </button>
        <input
          ref={fileInput}
          type="file"
          accept=".json,application/json"
          multiple
          style={{ display: "none" }}
          data-testid={`${config.testId}-import-file`}
          onChange={(e) => {
            const files = e.target.files;
            void onImport(files).finally(() => {
              if (fileInput.current) fileInput.current.value = "";
            });
          }}
        />
      </div>
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          entries={swatchMenu(readOnly, copied, working.colors[menu.key], themeColor(saved, menu.key, THEME_DATA)).map((id) => ({
            label: SWATCH_MENU_LABELS[id],
            onSelect: () => {
              const key = menu.key;
              if (id === "copy") setCopied(working.colors[key] ?? null);
              else if (id === "paste" && copied) updateColor(key, copied);
              else if (id === "revert") {
                const back = themeColor(saved, key, THEME_DATA);
                if (back) updateColor(key, back);
              }
            },
          }))}
        />
      )}
      {editing && (
        <ColorDialog
          title="Color Picker"
          initial={working.colors[editing.key] ?? null}
          defaultColor={defaultColor(editing.key, THEME_DATA)}
          onCancel={() => setEditing(null)}
          onPick={(css) => {
            const c = css ? parseCssColor(css) : null;
            if (c) updateColor(editing.key, toHex8(c));
            setEditing(null);
          }}
        />
      )}
      {asking === "name" && <NameDialog title="Add Color Theme" label="New theme name:" onCancel={() => setAsking(null)} onOk={addNewTheme} />}
      {asking === "exists" && <MessageDialog title="Message" message="Theme already exists!" onOk={() => setAsking(null)} onCancel={() => setAsking(null)} />}
      {problem && <MessageDialog title="Import Theme" message={problem} onOk={() => setProblem(null)} onCancel={() => setProblem(null)} />}
    </div>
  );

  return {
    element,
    working,
    reset: () => {
      if (working.readOnly) return;
      dirty.current = true;
      setWorking((w) => resetThemeColors(w, pageKeys, THEME_DATA));
    },
    resetLabel: "Reset Colors to Defaults",
    resetTooltip: "Reset all colors in this theme to the KiCad defaults",
    commit: () => {
      // `TransferDataFromWindow`: the schematic page saves the theme (`saveCurrentTheme( true )`); here every page saves what it has not (a reset, the
      // override box) and then makes the shown theme its editor's
      if (dirty.current) saveCurrentTheme(working);
      selectEditorTheme(editor, working.filename);
      setEditorThemePreview(editor, null);
    },
    cancel: () => setEditorThemePreview(editor, null),
  };
}
