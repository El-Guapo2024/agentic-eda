// Preferences > PCB Editor > Colors: `PANEL_PCBNEW_COLOR_SETTINGS` (pcbnew/dialogs/panel_pcbnew_color_settings.cpp) over `PANEL_COLOR_SETTINGS`
// (common/dialogs/panel_color_settings.cpp), KiCad 8303b2ad. The rules are kicad-port/colorTheme.ts'; the themes are the person's
// (state/colorThemeStore.ts).
//
//   * "Theme:" lists the themes (`createThemeList`), "---" and "New Theme..."; picking a theme first saves the one shown (`saveCurrentTheme( false )`)
//     and shows the other's colours (`OnThemeChanged`); picking "---" puts the selection back.
//   * New Theme... asks "New theme name:" in "Add Color Theme" (`FOOTPRINT_NAME_VALIDATOR`: `%$<>\t\n\r"\/:` cannot be typed); a name taken says
//     "Theme already exists!"; the new theme has the shown theme's colours and is selected.
//   * A swatch per layer in `createSwatches`' order, each labelled with `LayerName()` or the board's name for the layer; a double click or a middle
//     click opens the colour dialog with KiCad's default as its Reset to Default (`createSwatch`: `selected->GetDefaultColor( aLayer )`), and OK
//     sets the colour and saves the theme at once (`updateColor`: "Colors must be persisted when edited"); a read-only theme's swatches do not open.
//     A right click: Copy color, Paste color, Revert to saved color (`ShowColorContextMenu`).
//   * The dialog's "Reset Colors to Defaults" (`PAGED_DIALOG`: "Reset %s to Defaults", tooltip "Reset all colors in this theme to the KiCad
//     defaults") is `ResetPanel`: every colour of the page to KiCad's default, nothing on a read-only theme.
//   * OK (`TransferDataFromWindow`) makes the shown theme the board editor's (`m_ColorTheme`); Cancel keeps the old one.
//
// Different from KiCad: no preview board (`m_preview`, the `g_previewBoard` sample): the page's colours are drawn on the board itself while the
// dialog is up (`setThemePreview`), and the board goes back to the editor's theme on Cancel. No "Open Theme Folder" (the themes are the browser's,
// not files). A reset is saved by OK here; in KiCad it waits in the page's working copy until the next swatch edit or theme change saves it
// (`ResetPanel` does not call `saveCurrentTheme`, and the PCB page's `TransferDataFromWindow` saves only the theme's name).
import { useEffect, useMemo, useState, type MouseEvent, type ReactNode } from "react";
import { useStudioState } from "../state/store";
import { THEME_DATA, getColorThemes, replaceStoredThemes, selectPcbTheme, setThemeColor, setThemePreview, useColorThemes } from "../state/colorThemeStore";
import { SWATCH_MENU_LABELS, addTheme, defaultColor, findTheme, pcbColorPageKeys, pcbColorPageRows, resetThemeColors, sanitizeThemeName, swatchMenu, themeColor, themeListEntries, type ColorTheme } from "../kicad-port/colorTheme";
import { layerDisplayName } from "../kicad-port/boardLayers";
import { parseCssColor, toCssColor, toHex8 } from "../kicad-port/appearance";
import { ColorDialog, MessageDialog, NameDialog } from "./panels/appearance/shared";
import { ContextMenu } from "./canvas/ContextMenu";

const PAGE_KEYS = pcbColorPageKeys();

/** The page's working copy (`m_currentSettings`) of a theme: every colour of the page resolved. */
function workingCopy(theme: ColorTheme): ColorTheme {
  const colors: Record<string, string> = {};
  for (const k of PAGE_KEYS) {
    const c = themeColor(theme, k, THEME_DATA);
    if (c) colors[k] = c;
  }
  return { ...theme, colors };
}

/** `saveCurrentTheme`: the page's colours into the theme of the same filename, unless it is read-only. */
function saveCurrentTheme(working: ColorTheme): void {
  if (!working.readOnly) setThemeColor(working.filename, working.colors);
}

export interface PcbColorsPage {
  element: ReactNode;
  /** `ResetPanel`. */
  reset: () => void;
  /** OK: `saveCurrentTheme( true )` and `cfg->m_ColorTheme = m_currentSettings->GetFilename()`. */
  commit: () => void;
  /** Cancel: the board goes back to the editor's theme. */
  cancel: () => void;
}

/** The page's state lives with the Preferences dialog (it outlives a switch to another page, as a `wxTreebook` page does). `open`: the dialog is up. */
export function usePcbColorsPage(open: boolean, shown: boolean): PcbColorsPage {
  const state = useStudioState();
  const themes = useColorThemes();
  const [working, setWorking] = useState<ColorTheme>(() => workingCopy(themes.pcb));
  const [copied, setCopied] = useState<string | null>(null);
  const [asking, setAsking] = useState<"name" | "exists" | null>(null);
  const [editing, setEditing] = useState<{ key: string; label: string } | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; key: string } | null>(null);

  // Opening the dialog starts the page on the editor's theme, as KiCad's constructor does (`createThemeList( cfg->m_ColorTheme )`).
  useEffect(() => {
    if (open) setWorking(workingCopy(getColorThemes().pcb));
    else setThemePreview(null);
  }, [open]);
  // The board shows the page's colours while the dialog is up.
  useEffect(() => {
    if (open) setThemePreview(working);
  }, [open, working]);
  useEffect(() => () => setThemePreview(null), []);

  const setup = state.board?.board_rules?.layer_setup;
  const rows = useMemo(() => pcbColorPageRows((file) => layerDisplayName(setup, file)), [setup]);
  const saved = findTheme(themes.registry, working.filename);
  const readOnly = working.readOnly;
  const background = working.colors.LAYER_PCB_BACKGROUND ?? "#000000ff";

  /** `updateColor`: the colour, saved at once with the rest of the page. */
  const updateColor = (key: string, hex8: string) => {
    if (readOnly) return;
    const next = { ...working, colors: { ...working.colors, [key]: hex8 } };
    setWorking(next);
    saveCurrentTheme(next);
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
    setWorking(workingCopy(findTheme(getColorThemes().registry, value)));
  };

  const addNewTheme = (text: string) => {
    const name = sanitizeThemeName(text);
    const snap = getColorThemes();
    const r = addTheme(snap.stored, name, working, PAGE_KEYS, THEME_DATA);
    if (r.error) {
      setAsking("exists");
      return;
    }
    replaceStoredThemes(r.stored);
    setAsking(null);
    setWorking(workingCopy(findTheme(getColorThemes().registry, name)));
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
    <div data-testid="pcb-colors-page" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div className="filter-row" style={{ gap: 8 }}>
        <span>Theme:</span>
        <select aria-label="Theme" data-testid="pcb-colors-theme" value={working.filename} onChange={(e) => onTheme(e.target.value)} style={{ minWidth: 220 }}>
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
      {/* `m_colorsListWindow`: one scrolled column of swatch and name (KiCad's preview board would be on its right) */}
      <div role="list" aria-label="Colors" style={{ height: 420, overflowY: "auto", border: "1px solid var(--chrome-border)", padding: "4px 8px", maxWidth: 360 }}>
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
    </div>
  );

  return {
    element,
    reset: () => setWorking((w) => resetThemeColors(w, PAGE_KEYS, THEME_DATA)),
    commit: () => {
      saveCurrentTheme(working);
      selectPcbTheme(working.filename);
      setThemePreview(null);
    },
    cancel: () => setThemePreview(null),
  };
}
