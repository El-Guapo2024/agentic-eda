// The "Alternate pin function definitions" pane of Pin Properties (`DIALOG_PIN_PROPERTIES`' `m_alternatesTurndown` and `ALT_PIN_DATA_MODEL`): a turndown that holds a grid of
// rows "Alternate Pin Name / Electrical Type / Graphic Style", a button to add a row (no name, the pin's own type and style) and one to delete the selected row. The pane starts
// open when the pin has alternates or when it was left open the last time, and is disabled -- with a tooltip -- for a symbol with several body styles. The rules live in
// kicad-port/symPinAlternates.ts.
import { useState } from "react";
import type { LibrarySymbolPin, PinAlternate, PinElectricalType, PinShape } from "../../api/types";
import { ALTERNATES_NOT_AVAILABLE, ALTERNATES_TITLE, ALTERNATE_COLUMNS, PIN_SHAPE_OPTIONS, PIN_TYPE_OPTIONS, alternatesPaneOpen, newAlternate } from "../../kicad-port/symPinAlternates";

/** `s_alternatesTurndownOpen`: whether the pane was left open, for the next time the dialog opens. */
let turndownOpenBefore = false;

export interface PinAlternatesGridProps {
  rows: readonly PinAlternate[];
  onChange: (rows: PinAlternate[]) => void;
  /** The pin as stored: a new row takes its electrical type and graphic style. */
  pin: Pick<LibrarySymbolPin, "electrical_type" | "shape">;
  /** `!IsMultiBodyStyle()`. */
  available: boolean;
  onError: (message: string | null) => void;
}

export function PinAlternatesGrid({ rows, onChange, pin, available, onError }: PinAlternatesGridProps) {
  const [open, setOpen] = useState(() => available && alternatesPaneOpen(rows.length, turndownOpenBefore));
  const [selected, setSelected] = useState<number | null>(null);
  const [focusLast, setFocusLast] = useState(false);
  const toggle = () => {
    if (!available) return;
    turndownOpenBefore = !open;
    setOpen(!open);
  };
  const edit = (index: number, patch: Partial<PinAlternate>) => onChange(rows.map((r, i) => (i === index ? { ...r, ...patch } : r)));
  const add = () => {
    onChange([...rows, newAlternate(pin)]);
    setSelected(rows.length);
    setFocusLast(true);
    onError(null);
  };
  const remove = () => {
    if (selected === null) return;
    onChange(rows.filter((_, i) => i !== selected));
    setSelected(null);
    onError(null);
  };

  return (
    <div data-testid="pin-alternates" style={{ marginTop: 10 }}>
      <button
        type="button"
        onClick={toggle}
        disabled={!available}
        title={available ? undefined : ALTERNATES_NOT_AVAILABLE}
        aria-expanded={available && open}
        data-testid="pin-alternates-toggle"
        style={{ background: "none", border: "none", padding: 0, cursor: available ? "pointer" : "default", color: available ? "inherit" : "var(--chrome-text-dim)" }}
      >
        {available && open ? "▾" : "▸"} {ALTERNATES_TITLE}
        {available && rows.length > 0 ? ` (${rows.length})` : ""}
      </button>
      {available && open && (
        <div style={{ marginTop: 4 }}>
          <div style={{ display: "flex", gap: 4, marginBottom: 4 }}>
            <button type="button" onClick={add} title="Add alternate" aria-label="Add alternate" data-testid="pin-alternates-add">
              +
            </button>
            <button type="button" onClick={remove} disabled={selected === null} title="Delete alternate" aria-label="Delete alternate" data-testid="pin-alternates-delete">
              ✕
            </button>
          </div>
          <div style={{ maxHeight: 160, overflow: "auto", border: "1px solid var(--chrome-border)" }}>
            <table className="setup-table" style={{ margin: 0, width: "100%" }}>
              <thead>
                <tr>
                  {ALTERNATE_COLUMNS.map((c) => (
                    <th key={c}>{c}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => (
                  <tr key={i} data-testid={`alt-row-${i}`} aria-selected={selected === i} onClick={() => setSelected(i)} style={{ background: selected === i ? "var(--chrome-selected-bg)" : undefined }}>
                    <td>
                      <input
                        value={r.name}
                        aria-label={`Alternate pin name ${i + 1}`}
                        data-testid={`alt-name-${i}`}
                        style={{ width: 150 }}
                        autoFocus={focusLast && i === rows.length - 1}
                        onFocus={() => setSelected(i)}
                        onChange={(e) => edit(i, { name: e.target.value })}
                      />
                    </td>
                    <td>
                      <select value={r.electrical_type} aria-label={`Electrical type of alternate ${i + 1}`} data-testid={`alt-type-${i}`} onChange={(e) => edit(i, { electrical_type: e.target.value as PinElectricalType })}>
                        {PIN_TYPE_OPTIONS.map((o) => (
                          <option key={o.value} value={o.value}>
                            {o.label}
                          </option>
                        ))}
                      </select>
                    </td>
                    <td>
                      <select value={r.shape} aria-label={`Graphic style of alternate ${i + 1}`} data-testid={`alt-shape-${i}`} onChange={(e) => edit(i, { shape: e.target.value as PinShape })}>
                        {PIN_SHAPE_OPTIONS.map((o) => (
                          <option key={o.value} value={o.value}>
                            {o.label}
                          </option>
                        ))}
                      </select>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}
