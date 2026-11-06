// The controls Text, Label and Text Box Properties share for a text's looks (UI-LEFT S6; `DIALOG_TEXT_PROPERTIES`, `DIALOG_LABEL_PROPERTIES`): the Font choice
// (`FONT_CHOICE`: "Default Font", "KiCad Font", the machine's families from `GET /api/fonts`), a colour swatch with its "Default" (`COLOR_SWATCH` with
// `SetDefaultColor( COLOR4D::UNSPECIFIED )`) and the Link check and combo (`m_hyperlinkCb`, `m_hyperlinkCombo`: a page of the hierarchy or a URL start).
// What they mean is kicad-port/textLooks.ts.
import { useEffect, useState } from "react";
import { fetchFontFamilies } from "../api/client";
import { fetchHierarchy } from "../api/schControlClient";
import type { SchColor } from "../api/schEditTypes";
import { colorToHex, hexToColor, isUnspecified, UNSPECIFIED_COLOR } from "../kicad-port/schProperties";
import { fontChoices, linkChoices, NOT_FOUND, type LinkChoice } from "../kicad-port/textLooks";

/** The machine's font families, once per dialog. */
export function useFontFamilies(): string[] {
  const [families, setFamilies] = useState<string[]>([]);
  useEffect(() => {
    let cancelled = false;
    fetchFontFamilies()
      .then((f) => !cancelled && setFamilies(f))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);
  return families;
}

/** The Link combo's entries: the pages of the hierarchy (`schematic.Hierarchy()`), then the URL starts. */
export function useLinkChoices(): LinkChoice[] {
  const [choices, setChoices] = useState<LinkChoice[]>(() => linkChoices([]));
  useEffect(() => {
    let cancelled = false;
    fetchHierarchy()
      .then((sheets) => !cancelled && setChoices(linkChoices(sheets.map((s, i) => ({ page: s.page || String(i + 1), name: s.name, depth: s.path.length })))))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);
  return choices;
}

/**
 * The Font choice: `value` is the font's name ("Default Font" for none), `onChange` gets the name picked. A font the machine's list lacks is shown "<not found>"
 * (`SetFontSelection`), and picking it again keeps the font as it is (`HaveFontSelection` is false for it).
 */
export function FontSelect({ families, current, value, onChange, label = "Font" }: { families: readonly string[]; current: string | null | undefined; value: string; onChange: (v: string) => void; label?: string }) {
  const { choices } = fontChoices(families, current);
  const shown = choices.includes(value) ? value : `${value}${NOT_FOUND}`;
  const all = choices.includes(shown) ? choices : [...choices, shown];
  return (
    <select aria-label={label} value={shown} onChange={(e) => onChange(e.target.value.endsWith(NOT_FOUND) ? e.target.value.slice(0, -NOT_FOUND.length) : e.target.value)}>
      {all.map((c) => (
        <option key={c} value={c}>
          {c}
        </option>
      ))}
    </select>
  );
}

/** A colour swatch and its "Default" tick (`COLOR4D::UNSPECIFIED`: the layer's colour). `null` is the default. */
export function ColorSwatch({ color, onChange, label }: { color: SchColor | null; onChange: (c: SchColor | null) => void; label: string }) {
  const unspecified = !color || isUnspecified(color);
  return (
    <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
      <input type="color" aria-label={label} value={unspecified ? "#000000" : colorToHex(color!)} disabled={unspecified} onChange={(e) => onChange(hexToColor(e.target.value) ?? color)} />
      <label>
        <input type="checkbox" aria-label={`${label} default`} checked={unspecified} onChange={(e) => onChange(e.target.checked ? null : { ...UNSPECIFIED_COLOR, a: 255 })} /> Default
      </label>
    </span>
  );
}

/**
 * The Link check and combo (`onHyperlinkChecked`: ticking an empty one brings back the last link, unticking keeps it aside and empties the box; typing or picking an
 * entry ticks it). `"---"` is a separator, not a link.
 */
export function LinkField({ href, onChange, choices }: { href: string; onChange: (h: string) => void; choices: readonly LinkChoice[] }) {
  const [last, setLast] = useState("");
  const on = href.length > 0;
  return (
    <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
      <label title="Make this text item a clickable hyperlink">
        <input
          type="checkbox"
          aria-label="Link"
          checked={on}
          onChange={(e) => {
            if (e.target.checked && !href) onChange(last);
            else if (!e.target.checked && href) {
              setLast(href);
              onChange("");
            }
          }}
        />{" "}
        Link:
      </label>
      <input aria-label="Hyperlink" list="text-look-links" value={href} onChange={(e) => onChange(e.target.value === "---" ? href : e.target.value)} style={{ flex: 1 }} />
      <datalist id="text-look-links">
        {choices
          .filter((c) => c.value !== "---")
          .map((c) => (
            <option key={c.value} value={c.value}>
              {c.label}
            </option>
          ))}
      </datalist>
    </span>
  );
}
