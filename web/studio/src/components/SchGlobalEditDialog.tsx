// Edit Text and Graphics Properties (`SCH_EDIT_TOOL::GlobalEdit` -> `DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS`, eeschema/dialogs/dialog_global_edit_text_and_graphics.cpp
// and its _base.cpp at 8303b2ad): the Scope box (what kinds of item), Filter Items (by field name, by the parent's reference designator, library id and type, by
// net, the selection) and Set To (font, text colour, size, bold, italic, a label's orientation, a field's alignment, visibility and shown name, line width,
// colour and style, fill colour, junction size and colour; each "-- leave unchanged --" until set). OK sends one verb, `global_edit_text_graphics`, which
// reaches every sheet of the hierarchy as KiCad's `TransferDataFromWindow` does, one undo step (crates/ops/src/sch_global_edit.rs; the rules of the boxes are
// kicad-port/schGlobalEdit.ts). Lengths are in mm, as the studio's units show them. The four filter texts are kept for the session (`g_*Filter`).
import { useEffect, useState } from "react";
import { fetchFontFamilies } from "../api/client";
import type { SchToolDialog } from "../api/schEditTypes";
import { globalEditCmd, H_ALIGN_CHOICES, initialForm, LEAVE_UNCHANGED, LINE_STYLE_CHOICES, nextTri, ORIENTATION_CHOICES, rememberedFilters, SCOPE_BOXES, TYPE_FILTER_CHOICES, V_ALIGN_CHOICES, type GlobalEditForm, type Tri } from "../kicad-port/schGlobalEdit";
import { fontChoices } from "../kicad-port/symbolPropsDialog";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";

/** A three-state box: undetermined (leave unchanged), checked, unchecked. */
function TriBox({ label, value, onChange, note }: { label: string; value: Tri; onChange: (t: Tri) => void; note?: string }) {
  return (
    <label style={{ display: "flex", alignItems: "center", gap: 4 }}>
      <input
        type="checkbox"
        aria-label={label}
        checked={value === true}
        ref={(el) => {
          if (el) el.indeterminate = value === null;
        }}
        onChange={() => onChange(nextTri(value))}
      />
      {label}
      {note && <span style={{ fontSize: 10, fontStyle: "italic", color: "var(--chrome-text-dim)" }}>{note}</span>}
    </label>
  );
}

/** A colour swatch with the box that says it is set (`m_setTextColor` + `m_textColorSwatch`); an empty colour is `COLOR4D::UNSPECIFIED`. */
function ColorRow({ label, set, color, onSet, onColor }: { label: string; set: boolean; color: string; onSet: (b: boolean) => void; onColor: (c: string) => void }) {
  return (
    <>
      <label style={{ display: "flex", alignItems: "center", gap: 4 }}>
        <input type="checkbox" aria-label={label} checked={set} onChange={(e) => onSet(e.target.checked)} />
        {label}
      </label>
      <span style={{ display: "flex", alignItems: "center", gap: 4 }}>
        <input type="color" aria-label={`${label} swatch`} value={color || "#000000"} style={{ width: 32, padding: 0, opacity: color ? 1 : 0.35 }} onChange={(e) => (onColor(e.target.value), onSet(true))} />
        {color && (
          <button type="button" title="Unspecified (the layer's colour)" aria-label={`${label} unspecified`} onClick={() => onColor("")}>
            ×
          </button>
        )}
      </span>
    </>
  );
}

const NOTE = { fontSize: 10, fontStyle: "italic", color: "var(--chrome-text-dim)" } as const;

export function GlobalEditDialog({ dialog }: { dialog: Extract<SchToolDialog, { kind: "edit_text_graphics" }> }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const [f, setF] = useState<GlobalEditForm>(() => initialForm(state.tab === "schematic" ? state.netHighlight : null));
  const [error, setError] = useState<string | null>(null);
  const [families, setFamilies] = useState<string[]>([]);
  useEffect(() => {
    let live = true;
    fetchFontFamilies()
      .then((fams) => live && setFamilies(fams))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, []);
  const set = (patch: Partial<GlobalEditForm>) => setF((x) => ({ ...x, ...patch }));
  // `~DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS`: the filter texts are kept for the next time, whichever button closed it
  const keepFilters = () => Object.assign(rememberedFilters, { fieldName: f.fieldName, reference: f.reference, symbol: f.symbol, net: f.net });
  const close = () => {
    keepFilters();
    dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  };
  const apply = async () => {
    const r = globalEditCmd(f, dialog.selected);
    if ("error" in r) return setError(r.error);
    keepFilters();
    dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
    await api.cmd(r.cmd);
  };

  const box = { border: "1px solid var(--chrome-border)", margin: "0 0 8px", padding: "6px 8px" } as const;
  // a filter's text ticks its box (`OnReferenceFilterText`, `OnSymbolFilterText`, `OnFieldNameFilterText`, `OnNetFilterText`)
  const filterRow = (label: string, opt: keyof GlobalEditForm, text: keyof GlobalEditForm) => (
    <>
      <label style={{ display: "flex", alignItems: "center", gap: 4 }}>
        <input type="checkbox" aria-label={label} checked={f[opt] as boolean} onChange={(e) => set({ [opt]: e.target.checked } as Partial<GlobalEditForm>)} />
        {label}
      </label>
      <input aria-label={`${label} text`} value={f[text] as string} onChange={(e) => set({ [text]: e.target.value, [opt]: true } as Partial<GlobalEditForm>)} />
    </>
  );
  const leave = <option value="">{LEAVE_UNCHANGED}</option>;

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 760, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()} data-testid="sch-global-edit">
        <div className="dialog-header">
          <span>Edit Text and Graphics Properties</span>
        </div>
        <div className="dialog-body">
          <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
            <fieldset style={{ ...box, minWidth: 230 }} data-testid="global-edit-scope">
              <legend>Scope</legend>
              {SCOPE_BOXES.map(([key, label], i) => (
                <label key={key} style={{ display: "block", marginTop: i === 3 || i === 8 || i === 12 ? 8 : 0 }}>
                  <input type="checkbox" aria-label={label} checked={f.scope[key]} onChange={(e) => set({ scope: { ...f.scope, [key]: e.target.checked } })} /> {label}
                </label>
              ))}
            </fieldset>
            <fieldset style={{ ...box, flex: 1, minWidth: 320 }} data-testid="global-edit-filters">
              <legend>Filter Items</legend>
              <div style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "4px 8px", alignItems: "center" }}>
                {filterRow("By field name:", "fieldNameOpt", "fieldName")}
                {filterRow("By parent reference designator:", "referenceOpt", "reference")}
                {filterRow("By parent symbol library id:", "symbolOpt", "symbol")}
                <label style={{ display: "flex", alignItems: "center", gap: 4 }}>
                  <input type="checkbox" aria-label="By parent symbol type:" checked={f.typeOpt} onChange={(e) => set({ typeOpt: e.target.checked })} />
                  By parent symbol type:
                </label>
                <select aria-label="Parent symbol type" value={f.type} onChange={(e) => set({ type: e.target.value as GlobalEditForm["type"] })}>
                  {TYPE_FILTER_CHOICES.map(([v, l]) => (
                    <option key={v} value={v}>
                      {l}
                    </option>
                  ))}
                </select>
                {filterRow("By net:", "netOpt", "net")}
                <label style={{ display: "flex", alignItems: "center", gap: 4, gridColumn: "1 / span 2", marginTop: 6 }}>
                  <input type="checkbox" aria-label="Selected items only" checked={f.selectedOnly} onChange={(e) => set({ selectedOnly: e.target.checked })} />
                  Selected items only
                </label>
              </div>
            </fieldset>
          </div>
          <fieldset style={box} data-testid="global-edit-set-to">
            <legend>Set To</legend>
            <datalist id="global-edit-fonts">
              {fontChoices(families).map((name) => (
                <option key={name} value={name} />
              ))}
            </datalist>
            <div style={{ display: "grid", gridTemplateColumns: "auto 1fr auto 1fr", gap: "4px 10px", alignItems: "center" }}>
              <span>Font:</span>
              <input aria-label="Font" list="global-edit-fonts" placeholder={LEAVE_UNCHANGED} value={f.font} onChange={(e) => set({ font: e.target.value })} />
              <ColorRow label="Text color:" set={f.setTextColor} color={f.textColor} onSet={(b) => set({ setTextColor: b })} onColor={(c) => set({ textColor: c })} />
              <span>Text size (mm):</span>
              <input aria-label="Text size" placeholder={LEAVE_UNCHANGED} value={f.textSize} onChange={(e) => set({ textSize: e.target.value })} />
              <TriBox label="Bold" value={f.bold} onChange={(t) => set({ bold: t })} />
              <TriBox label="Italic" value={f.italic} onChange={(t) => set({ italic: t })} />
              <span>Orientation:</span>
              <span style={{ display: "flex", gap: 4, alignItems: "center" }}>
                <select aria-label="Orientation" value={f.orientation} onChange={(e) => set({ orientation: Number(e.target.value) })}>
                  {ORIENTATION_CHOICES.map((o, i) => (
                    <option key={o} value={i}>
                      {o}
                    </option>
                  ))}
                  <option value={-1}>{LEAVE_UNCHANGED}</option>
                </select>
                <span style={NOTE}>(labels only)</span>
              </span>
              <TriBox label="Visible" value={f.visible} onChange={(t) => set({ visible: t })} note="(fields only)" />
              <span />
              <span>H Align:</span>
              <span style={{ display: "flex", gap: 4, alignItems: "center" }}>
                <select aria-label="H Align" value={f.hAlign} onChange={(e) => set({ hAlign: e.target.value as GlobalEditForm["hAlign"] })}>
                  {H_ALIGN_CHOICES.map(([v, l]) => (
                    <option key={v} value={v}>
                      {l}
                    </option>
                  ))}
                  {leave}
                </select>
                <span style={NOTE}>(fields only)</span>
              </span>
              <TriBox label="Show field name" value={f.showFieldNames} onChange={(t) => set({ showFieldNames: t })} note="(fields only)" />
              <span />
              <span>V Align:</span>
              <span style={{ display: "flex", gap: 4, alignItems: "center" }}>
                <select aria-label="V Align" value={f.vAlign} onChange={(e) => set({ vAlign: e.target.value as GlobalEditForm["vAlign"] })}>
                  {V_ALIGN_CHOICES.map(([v, l]) => (
                    <option key={v} value={v}>
                      {l}
                    </option>
                  ))}
                  {leave}
                </select>
                <span style={NOTE}>(fields only)</span>
              </span>
              <span />
              <span />
              <span>Line width (mm):</span>
              <input aria-label="Line width" placeholder={LEAVE_UNCHANGED} value={f.lineWidth} onChange={(e) => set({ lineWidth: e.target.value })} />
              <ColorRow label="Line color:" set={f.setLineColor} color={f.lineColor} onSet={(b) => set({ setLineColor: b })} onColor={(c) => set({ lineColor: c })} />
              <span>Line style:</span>
              <select aria-label="Line style" value={f.lineStyle} onChange={(e) => set({ lineStyle: e.target.value as GlobalEditForm["lineStyle"] })}>
                {LINE_STYLE_CHOICES.map(([v, l]) => (
                  <option key={v} value={v}>
                    {l}
                  </option>
                ))}
                {leave}
              </select>
              <ColorRow label="Fill color:" set={f.setFillColor} color={f.fillColor} onSet={(b) => set({ setFillColor: b })} onColor={(c) => set({ fillColor: c })} />
              <span>Junction size (mm):</span>
              <input aria-label="Junction size" placeholder={LEAVE_UNCHANGED} value={f.junctionSize} onChange={(e) => set({ junctionSize: e.target.value })} />
              <ColorRow label="Junction color:" set={f.setJunctionColor} color={f.junctionColor} onSet={(b) => set({ setJunctionColor: b })} onColor={(c) => set({ junctionColor: c })} />
            </div>
          </fieldset>
          {error && (
            <p style={{ color: "var(--chrome-danger)", fontSize: 11, margin: 0 }} data-testid="global-edit-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={() => void apply()}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
