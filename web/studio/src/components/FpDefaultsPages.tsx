// Preferences > Footprint Editor > Footprint Defaults and > User Layer Names (UI-LEFT-2 FF7), KiCad 8303b2ad:
//
//   * Footprint Defaults: `PANEL_FP_EDITOR_FIELD_DEFAULTS` (pcbnew/dialogs/panel_fp_editor_field_defaults.cpp and its _base): "Default Field Properties for New
//     Footprints", a grid of the Reference designator and the Value with Value, Show and Layer (`TEXT_ITEMS_GRID_TABLE( true )`), and "Default Text Items for New
//     Footprints", a grid of Text Items and Layer (`( false )`) with the Add and Delete buttons (`OnAddTextItem`: the row before it's layer; `OnDeleteTextItem`: the
//     rows selected). OK keeps the list (`m_DefaultFPTextItems`); the dialog's reset button is `ResetPanel` (KiCad's list). New Footprint makes the new footprint's
//     texts from it (state/footprintEditorStore.tsx `newFootprint`, kicad-port/fpFieldDefaults.ts `newFootprintTexts`).
//   * User Layer Names: `PANEL_FP_USER_LAYER_NAMES` (panel_fp_user_layer_names.cpp and its _base): "User layers:" (0 .. 9) and "User Layer Names", a grid of Layer and
//     Name with Add (the first free User.n) and Delete; a layer chosen twice moves the row to the next free one, a name given twice is "Layer name %s already in use."
//     (`onLayerChange`, the dialog's error, which does not stop OK). OK keeps the count and the named rows (`SetUserDefinedLayerCount`, `m_UserLayerNames`); the
//     Footprint Editor's Appearance panel lists the first so many user layers and every named one, a named one by its name.
//
// The rules are kicad-port/fpFieldDefaults.ts'; the settings are the studio's preferences (`fpDefaultTextItems`, `fpUserLayerCount`, `fpUserLayerNames`), kept per
// person as KiCad keeps them in fpedit.json. A grid's cells are inputs and choices here (KiCad's are a `wxGrid`'s editors); Delete takes the rows checked on the left.
import { useEffect, useState, type CSSProperties } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { layerDisplayName } from "../kicad-port/boardLayers";
import {
  addLayerNameRow,
  addTextItemRow,
  changeLayerNameRow,
  deleteTextItemRows,
  fieldDefaultsItems,
  fpTextItemsOf,
  layerNamesOf,
  loadFieldDefaults,
  loadLayerNames,
  resetFieldDefaults,
  DEFAULT_USER_LAYER_COUNT,
  TEXT_ITEM_LAYERS,
  USER_LAYER_COUNT_CHOICES,
  USER_NAME_LAYERS,
  type FieldDefaultsPage,
  type FpTextItem,
  type LayerNameRow,
} from "../kicad-port/fpFieldDefaults";
import type { PreferencesPageHandle } from "./preferencesPage";

const heading = (text: string) => <div style={{ fontWeight: 600, margin: "10px 0 4px" }}>{text}</div>;
const cell: CSSProperties = { padding: "2px 6px", borderBottom: "1px solid var(--chrome-border)" };
const head: CSSProperties = { ...cell, fontWeight: 500, textAlign: "left" };

/** A layer cell's choice (`GRID_CELL_LAYER_SELECTOR`), the layers by their standard names. */
function LayerChoice({ value, layers, onChange, label }: { value: string; layers: readonly string[]; onChange: (layer: string) => void; label: string }) {
  return (
    <select aria-label={label} value={value} onChange={(e) => onChange(e.target.value)}>
      {layers.map((l) => (
        <option key={l} value={l}>
          {layerDisplayName(undefined, l)}
        </option>
      ))}
    </select>
  );
}

/** The rows checked for Delete (the grid's row selection). */
function useRowSelection(): [ReadonlySet<number>, (row: number, on: boolean) => void, () => void] {
  const [rows, setRows] = useState<ReadonlySet<number>>(new Set());
  const toggle = (row: number, on: boolean) =>
    setRows((s) => {
      const next = new Set(s);
      if (on) next.add(row);
      else next.delete(row);
      return next;
    });
  return [rows, toggle, () => setRows(new Set())];
}

/** `PANEL_FP_EDITOR_FIELD_DEFAULTS`. */
/**
 * Whether the page was shown while the dialog is up: KiCad builds these pages lazily (`AddLazySubPage`), so a page never opened is never transferred and OK
 * leaves its settings as they were (here: nothing written, so a default stays a default).
 */
function useSeen(open: boolean, shown: boolean): boolean {
  const [seen, setSeen] = useState(false);
  useEffect(() => {
    if (!open) setSeen(false);
    else if (shown) setSeen(true);
  }, [open, shown]);
  return seen;
}

export function useFpFieldDefaultsPage(open: boolean, shown: boolean): PreferencesPageHandle {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const [page, setPage] = useState<FieldDefaultsPage>(() => loadFieldDefaults(fpTextItemsOf(state.prefs.fpDefaultTextItems)));
  const [selected, toggle, clearSelected] = useRowSelection();
  const seen = useSeen(open, shown);
  // `TransferDataToWindow` when the dialog opens: the settings as they are
  useEffect(() => {
    if (open) {
      setPage(loadFieldDefaults(fpTextItemsOf(state.prefs.fpDefaultTextItems)));
      clearSelected();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const setField = (i: 0 | 1, patch: Partial<FpTextItem>) => setPage((p) => ({ ...p, fields: (i === 0 ? [{ ...p.fields[0], ...patch }, p.fields[1]] : [p.fields[0], { ...p.fields[1], ...patch }]) as FieldDefaultsPage["fields"] }));
  const setText = (i: number, patch: Partial<FieldDefaultsPage["texts"][number]>) => setPage((p) => ({ ...p, texts: p.texts.map((t, j) => (j === i ? { ...t, ...patch } : t)) }));

  const element = !shown ? null : (
    <div data-testid="fp-defaults-page">
      {heading("Default Field Properties for New Footprints")}
      <table style={{ borderCollapse: "collapse" }}>
        <thead>
          <tr>
            <th style={head} />
            <th style={head}>Value</th>
            <th style={head}>Show</th>
            <th style={head}>Layer</th>
          </tr>
        </thead>
        <tbody>
          {(["Reference designator", "Value"] as const).map((label, i) => (
            <tr key={label}>
              <td style={{ ...cell, fontWeight: 500 }}>{label}</td>
              <td style={cell}>
                <input aria-label={`${label} value`} data-testid={`fp-defaults-field-${i}-text`} value={page.fields[i]!.text} onChange={(e) => setField(i as 0 | 1, { text: e.target.value })} />
              </td>
              <td style={{ ...cell, textAlign: "center" }}>
                <input type="checkbox" aria-label={`${label} show`} checked={page.fields[i]!.visible} onChange={(e) => setField(i as 0 | 1, { visible: e.target.checked })} />
              </td>
              <td style={cell}>
                <LayerChoice label={`${label} layer`} value={page.fields[i]!.layer} layers={TEXT_ITEM_LAYERS} onChange={(layer) => setField(i as 0 | 1, { layer })} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {heading("Default Text Items for New Footprints")}
      <table style={{ borderCollapse: "collapse" }} data-testid="fp-defaults-texts">
        <thead>
          <tr>
            <th style={head} />
            <th style={head}>Text Items</th>
            <th style={head}>Layer</th>
          </tr>
        </thead>
        <tbody>
          {page.texts.map((t, i) => (
            <tr key={i}>
              <td style={cell}>
                <input type="checkbox" aria-label={`Select text item ${i + 1}`} checked={selected.has(i)} onChange={(e) => toggle(i, e.target.checked)} />
              </td>
              <td style={cell}>
                <input aria-label={`Text item ${i + 1}`} value={t.text} onChange={(e) => setText(i, { text: e.target.value })} />
              </td>
              <td style={cell}>
                <LayerChoice label={`Text item ${i + 1} layer`} value={t.layer} layers={TEXT_ITEM_LAYERS} onChange={(layer) => setText(i, { layer })} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
        <button aria-label="Add Text Item" title="Add text item" onClick={() => setPage(addTextItemRow)}>
          +
        </button>
        <button
          aria-label="Delete Text Item"
          title="Delete text item"
          disabled={selected.size === 0}
          onClick={() => {
            setPage((p) => deleteTextItemRows(p, selected));
            clearSelected();
          }}
        >
          −
        </button>
      </div>
    </div>
  );

  return {
    element,
    commit: () => {
      if (seen) dispatch({ type: "PATCH_PREFERENCES", patch: { fpDefaultTextItems: fieldDefaultsItems(page) } });
    },
    cancel: () => {},
    reset: () => {
      setPage(resetFieldDefaults());
      clearSelected();
    },
  };
}

/** `PANEL_FP_USER_LAYER_NAMES`. */
export function useFpLayerNamesPage(open: boolean, shown: boolean): PreferencesPageHandle {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const [count, setCount] = useState(state.prefs.fpUserLayerCount);
  const [rows, setRows] = useState<LayerNameRow[]>(() => loadLayerNames(state.prefs.fpUserLayerNames));
  const [error, setError] = useState<string | null>(null);
  const [selected, toggle, clearSelected] = useRowSelection();
  const seen = useSeen(open, shown);
  useEffect(() => {
    if (open) {
      setCount(state.prefs.fpUserLayerCount);
      setRows(loadLayerNames(state.prefs.fpUserLayerNames));
      setError(null);
      clearSelected();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const change = (row: number, patch: Partial<LayerNameRow>) => {
    const r = changeLayerNameRow(rows, row, patch);
    setRows(r.rows);
    setError(r.error);
  };

  const element = !shown ? null : (
    <div data-testid="fp-layer-names-page">
      <div className="filter-row" style={{ gap: 8 }}>
        <span>User layers:</span>
        <select aria-label="User layers" data-testid="fp-user-layer-count" value={count} onChange={(e) => setCount(Number(e.target.value))}>
          {USER_LAYER_COUNT_CHOICES.map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>
      </div>
      {heading("User Layer Names")}
      <table style={{ borderCollapse: "collapse" }} data-testid="fp-layer-names">
        <thead>
          <tr>
            <th style={head} />
            <th style={head}>Layer</th>
            <th style={head}>Name</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i}>
              <td style={cell}>
                <input type="checkbox" aria-label={`Select layer name ${i + 1}`} checked={selected.has(i)} onChange={(e) => toggle(i, e.target.checked)} />
              </td>
              <td style={cell}>
                <LayerChoice label={`Layer name ${i + 1} layer`} value={r.layer} layers={USER_NAME_LAYERS} onChange={(layer) => change(i, { layer })} />
              </td>
              <td style={cell}>
                <input aria-label={`Layer name ${i + 1}`} value={r.name} onChange={(e) => change(i, { name: e.target.value })} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
        <button aria-label="Add Layer Name" title="Add layer name" onClick={() => setRows(addLayerNameRow)}>
          +
        </button>
        <button
          aria-label="Delete Layer Name"
          title="Delete layer name"
          disabled={selected.size === 0}
          onClick={() => {
            setRows((rs) => rs.filter((_, i) => !selected.has(i)));
            clearSelected();
            setError(null);
          }}
        >
          −
        </button>
      </div>
      <div role="alert" style={{ minHeight: 18, marginTop: 4, fontSize: 12, color: "#ff6666" }}>
        {error ?? ""}
      </div>
    </div>
  );

  return {
    element,
    commit: () => {
      if (seen) dispatch({ type: "PATCH_PREFERENCES", patch: { fpUserLayerCount: count, fpUserLayerNames: layerNamesOf(rows) } });
    },
    cancel: () => {},
    // `ResetPanel`: a fresh `FOOTPRINT_EDITOR_SETTINGS`' -- four user layers, none named
    reset: () => {
      setCount(DEFAULT_USER_LAYER_COUNT);
      setRows([]);
      setError(null);
      clearSelected();
    },
  };
}
