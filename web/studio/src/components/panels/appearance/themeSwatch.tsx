// The Appearance panel's theme swatches (`APPEARANCE_CONTROLS::rebuildLayers` / `rebuildObjects`' `COLOR_SWATCH`es): a double click or a middle click
// opens the colour dialog on the layer's colour in the board editor's theme (`COLOR_SWATCH::GetNewSwatchColor`), and OK sets it in that theme
// (`OnColorSwatchChanged`: `cs->SetColor( layer, newColor )`, `SaveColorSettings( cs, "board" )`, the canvas redrawn). On a read-only theme (the
// built-ins) the swatch does not open: `onReadOnlySwatch` says so in the info bar for ten seconds.
//
// A layer's swatch is made with the theme's colour as its default (`new COLOR_SWATCH( panel, .., bgColor, theme->GetColor( layer ), .. )`), so its
// dialog's Reset to Default goes back to the colour the row was built with; an object's with `theme->GetDefaultColor( layer )`, KiCad's default.
import { useState, type ReactNode } from "react";
import { useStudioDispatch } from "../../../state/store";
import { THEME_DATA, setThemeColor, useColorThemes } from "../../../state/colorThemeStore";
import { defaultColor } from "../../../kicad-port/colorTheme";
import { parseCssColor, toHex8 } from "../../../kicad-port/appearance";
import { layerColorIn, realColorKey } from "../../canvas/layers";
import { ColorDialog } from "./shared";

/** `onReadOnlySwatch`'s message, word for word (two spaces after the first sentence). */
export const READ_ONLY_THEME_MESSAGE = "The current color theme is read-only.  Create a new theme in Preferences to enable color editing.";

/** `editor`: the frame whose theme the swatches edit (`m_frame->GetColorSettings()`): the board editor's, or the Footprint Editor's own (UI-LEFT-2 FF5). */
export function useThemeSwatchEdit(editor: "pcb" | "footprint" = "pcb"): { edit: (bucketOrKey: string, kind: "layer" | "object") => void; dialog: ReactNode; readOnly: boolean } {
  const themes = useColorThemes();
  const dispatch = useStudioDispatch();
  const [editing, setEditing] = useState<{ key: string; initial: string; def: string } | null>(null);
  const theme = editor === "pcb" ? themes.pcb : themes.themes[editor];
  const readOnly = theme.readOnly;
  const edit = (bucketOrKey: string, kind: "layer" | "object") => {
    if (readOnly) {
      dispatch({ type: "TOAST", message: READ_ONLY_THEME_MESSAGE, kind: "info", ms: 10000 });
      return;
    }
    const key = realColorKey(bucketOrKey);
    const now = layerColorIn(editor, key);
    setEditing({ key, initial: now, def: kind === "layer" ? now : (defaultColor(key, THEME_DATA) ?? now) });
  };
  const dialog = editing && (
    <ColorDialog
      title="Color Picker"
      initial={editing.initial}
      defaultColor={editing.def}
      onCancel={() => setEditing(null)}
      onPick={(css) => {
        const c = css ? parseCssColor(css) : null;
        if (c) setThemeColor(theme.filename, { [editing.key]: toHex8(c) });
        setEditing(null);
      }}
    />
  );
  return { edit, dialog, readOnly };
}
