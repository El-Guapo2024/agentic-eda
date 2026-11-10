// Port of pcbnew/dialogs/dialog_global_edit_text_and_graphics{,_base}.cpp -- "Edit Text & Graphics Properties..." (pcbnew.GlobalEdit.editTextAndGraphics):
// the Scope box (the footprints' Reference designators, Values, Other footprint fields, Footprint graphic items, Footprint text items and Footprint dimensions; the
// board's PCB graphic items, PCB text items and PCB dimensions), the Filter Items box (By layer, By parent reference designator, By parent footprint library link,
// Selected items only; the two parent filters are remembered for the session as `g_referenceFilter` / `g_footprintFilter` are) -- kicad-port/globalEditTextGraphics.ts
// -- and the Action box: "Set to specified values:" with KiCad's rows (Layer, Visible (fields only), Line thickness, Font, Bold, Text width, Italic, Text height,
// Keep upright, Text thickness with its auto button, Center on footprint), each "-- leave unchanged --" (`INDETERMINATE_ACTION`) or undetermined to begin with, or
// "Set to layer default values:" over the layer-class grid of Board Setup > Text & Graphics > Defaults (`StyleFromSettings`). "Apply and Close" is one
// `Cmd::EditTextAndGraphics`, one undo step (`commit.Push( _( "Edit Text and Graphics" ) )`); crates/ops/src/pcb_global_edit.rs does the change.
//
// A value box is a `UNIT_BINDER` with its `NUMERIC_EVALUATOR` (kicad-port/numericEvaluator.ts, UI-LEFT-2 PP23): "0.1+0.05", "10mil", "w=0.2; w*2" are worked out
// in the editor's units when the box loses the focus (`onKillFocus`, the box then shows the result; the focus back shows the text as typed, `onSetFocus`) and
// when the dialog reads it (`GetValue`).
//
// Different from KiCad: the font list is the server machine's families (`GET /api/fonts`).
import { useEffect, useRef, useState } from "react";
import { fetchFontFamilies } from "../api/client";
import type { Cmd } from "../api/types";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { nmTo, type LengthUnit } from "../state/units";
import { evaluatedBoxText, lengthBoxValue, NumericEvaluator } from "../kicad-port/numericEvaluator";
import {
  actionChangesSomething,
  autoThickness,
  fontChoices,
  initialTextGraphicsFilter,
  layerClassRows,
  LEAVE_UNCHANGED,
  matchCount,
  matchingTextAndGraphics,
  nextTriState,
  NOTHING_IN_TEXT_SCOPE,
  textAndGraphicsCmd,
  textGraphicsLayers,
  textSizeMessage,
  UNCHANGED_ACTION,
  type TextGraphicsAction,
  type TextGraphicsFilter,
  type TextGraphicsScope,
  type TriState,
} from "../kicad-port/globalEditTextGraphics";

type Mode = "specified" | "layer_defaults";

// `g_referenceFilter` / `g_footprintFilter`: kept from one opening of the dialog to the next.
let remembered = { reference: "", footprint: "" };

const GROUP_TITLE = { fontWeight: 600, fontSize: 11, marginBottom: 4 } as const;
const ROW = { display: "flex", alignItems: "center", gap: 6, marginBottom: 4 } as const;

/** A length as a value box shows it: the number in the editor's units, without a conversion's noise. */
const shown = (nm: number, unit: LengthUnit): string => String(Number(nmTo(nm, unit).toFixed(4)));

/**
 * A `UNIT_BINDER` box that can say "-- leave unchanged --": empty is that, anything else a length or an expression in the editor's units. Its evaluator keeps the
 * variables it was given (`m_eval`); losing the focus shows the result (`onKillFocus`), taking it again the text as typed (`onSetFocus`: `m_eval.OriginalText()`).
 */
function LengthBox({ label, value, unit, disabled, onChange, placeholder }: { label: string; value: string; unit: LengthUnit; disabled?: boolean; onChange: (v: string) => void; placeholder?: string }) {
  const evaluator = useRef<NumericEvaluator | null>(null);
  evaluator.current ??= new NumericEvaluator(unit);
  const ev = evaluator.current;
  return (
    <>
      <span style={{ opacity: disabled ? 0.5 : 1 }}>{label}</span>
      <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
        <input
          type="text"
          value={value}
          placeholder={placeholder ?? LEAVE_UNCHANGED}
          disabled={disabled}
          onChange={(e) => onChange(e.target.value)}
          onFocus={() => {
            const old = ev.originalText();
            if (old !== "" && old !== value) onChange(old);
          }}
          onBlur={() => {
            const shownText = evaluatedBoxText(value, unit, ev);
            if (shownText !== value) onChange(shownText);
          }}
          style={{ width: 130 }}
          aria-label={label.replace(/:$/, "")}
        />
        <span style={{ opacity: disabled ? 0.5 : 1 }}>{unit}</span>
      </span>
    </>
  );
}

/** A `wxCHK_3STATE | wxCHK_ALLOW_3RD_STATE_FOR_USER` box. */
function TriBox({ label, value, disabled, onChange }: { label: string; value: TriState; disabled?: boolean; onChange: (v: TriState) => void }) {
  return (
    <label style={{ display: "inline-flex", alignItems: "center", gap: 4, opacity: disabled ? 0.5 : 1 }}>
      <input
        type="checkbox"
        checked={value === true}
        disabled={disabled}
        ref={(el) => {
          if (el) el.indeterminate = value === null;
        }}
        onChange={() => onChange(nextTriState(value))}
        aria-label={label}
        data-state={value === null ? "undetermined" : value ? "checked" : "unchecked"}
      />
      {label}
    </label>
  );
}

type Rows = Omit<TextGraphicsAction, "lineWidth" | "textWidth" | "textHeight" | "textThickness">;

export function GlobalEditTextAndGraphicsDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.editTextAndGraphicsDialogOpen;

  const [scope, setScope] = useState<TextGraphicsScope>(NOTHING_IN_TEXT_SCOPE);
  // null until a filter is touched: the layer follows the active layer of the moment the dialog opens.
  const [box, setBox] = useState<TextGraphicsFilter | null>(null);
  const [mode, setMode] = useState<Mode>("specified");
  // The value boxes as typed (empty = "-- leave unchanged --"), and the rest of the Action box.
  const [lineText, setLineText] = useState("");
  const [widthText, setWidthText] = useState("");
  const [heightText, setHeightText] = useState("");
  const [thicknessText, setThicknessText] = useState("");
  const [rows, setRows] = useState<Rows>(UNCHANGED_ACTION);
  const [families, setFamilies] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    let live = true;
    fetchFontFamilies()
      .then((f) => {
        if (live) setFamilies(f);
      })
      .catch(() => {
        if (live) setFamilies([]);
      });
    return () => {
      live = false;
    };
  }, [open]);

  if (!open) return null;

  const unit: LengthUnit = state.units;
  const layers = textGraphicsLayers(state.board);
  const filter = box ?? initialTextGraphicsFilter(state.board, state.activeLayer, remembered);
  const setFilter = (patch: Partial<TextGraphicsFilter>) => setBox({ ...filter, ...patch });
  // `UNIT_BINDER::GetValue`: the box's text worked out, else read as it is
  const length = (t: string): number | null => lengthBoxValue(t, unit);
  const action: TextGraphicsAction = { ...rows, lineWidth: length(lineText), textWidth: length(widthText), textHeight: length(heightText), textThickness: length(thicknessText) };
  const setRow = (patch: Partial<Rows>) => setRows({ ...rows, ...patch });

  const close = () => {
    remembered = { reference: filter.reference, footprint: filter.footprint };
    setScope(NOTHING_IN_TEXT_SCOPE);
    setBox(null);
    setMode("specified");
    setLineText("");
    setWidthText("");
    setHeightText("");
    setThicknessText("");
    setRows(UNCHANGED_ACTION);
    setError(null);
    dispatch({ type: "SET_EDIT_TEXT_AND_GRAPHICS_DIALOG_OPEN", open: false });
  };

  const match = matchingTextAndGraphics(state.board, scope, filter, state.selection);
  const total = matchCount(match);
  const specified = mode === "specified";
  const anyChange = !specified || actionChangesSomething(action);
  // `onAutoTextThickness` / `onTextSize`: the box shows the pen the sizes give, "(auto)" while a size is left unchanged.
  const autoPen = autoThickness(action.textWidth, action.textHeight, rows.bold);

  const apply = async () => {
    if (specified) {
      const message = textSizeMessage(action.textWidth, action.textHeight);
      if (message) {
        setError(message);
        return;
      }
    }
    if (total === 0 || !anyChange) return;
    setBusy(true);
    try {
      const ok = await api.cmd(textAndGraphicsCmd(match, !specified, action) as Cmd);
      if (ok) close();
    } finally {
      setBusy(false);
    }
  };

  const scopeBox = (key: keyof TextGraphicsScope, label: string, title?: string, disabledWhy?: string) => (
    <label className="filter-row" style={{ display: "block", opacity: disabledWhy ? 0.5 : 1 }} title={disabledWhy ?? title}>
      <input type="checkbox" checked={scope[key]} onChange={(e) => setScope({ ...scope, [key]: e.target.checked })} aria-label={label} /> {label}
    </label>
  );
  const grid = layerClassRows(state.board?.board_rules?.text_graphics);
  const cell = (v: number | null) => (v == null ? "" : `${shown(v, unit)} ${unit}`);
  const tick = (v: boolean | null) => (v == null ? "" : v ? "✓" : "");

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 640, maxWidth: "95vw" }} onClick={(e) => e.stopPropagation()} data-testid="global-edit-text-graphics">
        <div className="dialog-header">
          <span>Edit Text & Graphics Properties</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "78vh", overflow: "auto" }}>
          <div style={GROUP_TITLE}>Scope</div>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", columnGap: 30, rowGap: 2, marginBottom: 8 }}>
            {scopeBox("references", "Reference designators")}
            {scopeBox("graphics", "PCB graphic items")}
            {scopeBox("values", "Values")}
            {scopeBox("text", "PCB text items")}
            {scopeBox("otherFields", "Other footprint fields", "Footprint fields that are not the reference designator or value field")}
            {scopeBox("dimensions", "PCB dimensions")}
            {scopeBox("footprintGraphics", "Footprint graphic items")}
            <span />
            {scopeBox("footprintTexts", "Footprint text items", "Footprint text items not associated with a field")}
            <span />
            {scopeBox("footprintDimensions", "Footprint dimensions")}
          </div>

          <div style={GROUP_TITLE}>Filter Items</div>
          <label className="filter-row" style={ROW}>
            <input type="checkbox" checked={filter.layerOn} onChange={(e) => setFilter({ layerOn: e.target.checked })} aria-label="Filter by layer" /> By layer:
            <select value={filter.layer} onChange={(e) => setFilter({ layer: e.target.value, layerOn: true })} style={{ flex: 1 }} aria-label="Layer filter">
              {layers.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
          </label>
          <label className="filter-row" style={ROW}>
            <input type="checkbox" checked={filter.referenceOn} onChange={(e) => setFilter({ referenceOn: e.target.checked })} aria-label="By parent reference designator" /> By parent reference designator:
            <input type="text" value={filter.reference} onChange={(e) => setFilter({ reference: e.target.value, referenceOn: true })} style={{ flex: 1 }} aria-label="Reference filter" />
          </label>
          <label className="filter-row" style={ROW}>
            <input type="checkbox" checked={filter.footprintOn} onChange={(e) => setFilter({ footprintOn: e.target.checked })} aria-label="By parent footprint library link" /> By parent footprint library link:
            <input type="text" value={filter.footprint} onChange={(e) => setFilter({ footprint: e.target.value, footprintOn: true })} style={{ flex: 1 }} aria-label="Footprint filter" />
          </label>
          <label className="filter-row" style={{ display: "block", marginBottom: 8 }}>
            <input type="checkbox" checked={filter.selectedOnly} onChange={(e) => setFilter({ selectedOnly: e.target.checked })} aria-label="Selected items only" /> Selected items only
          </label>

          <div style={GROUP_TITLE}>Action</div>
          <label className="filter-row" style={{ display: "block" }}>
            <input type="radio" checked={specified} onChange={() => setMode("specified")} aria-label="Set to specified values" /> Set to specified values:
          </label>
          <div style={{ display: "grid", gridTemplateColumns: "110px auto 1fr", columnGap: 10, rowGap: 4, marginLeft: 20, marginBottom: 8, alignItems: "center" }}>
            <span style={{ opacity: specified ? 1 : 0.5 }}>Layer:</span>
            <select value={rows.layer ?? ""} disabled={!specified} onChange={(e) => setRow({ layer: e.target.value === "" ? null : e.target.value })} aria-label="Layer">
              <option value="">{LEAVE_UNCHANGED}</option>
              {layers.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
            <TriBox label="Visible  (fields only)" value={rows.visible} disabled={!specified} onChange={(v) => setRow({ visible: v })} />

            <LengthBox label="Line thickness:" value={lineText} unit={unit} disabled={!specified} onChange={setLineText} />
            <span />

            <span style={{ opacity: specified ? 1 : 0.5 }}>Font:</span>
            <select value={rows.font ?? LEAVE_UNCHANGED} disabled={!specified} onChange={(e) => setRow({ font: e.target.value === LEAVE_UNCHANGED ? null : e.target.value })} aria-label="Font">
              {fontChoices(families).map((f) => (
                <option key={f} value={f}>
                  {f}
                </option>
              ))}
            </select>
            <TriBox label="Bold" value={rows.bold} disabled={!specified} onChange={(v) => setRow({ bold: v })} />

            <LengthBox label="Text width:" value={widthText} unit={unit} disabled={!specified} onChange={setWidthText} />
            <TriBox label="Italic" value={rows.italic} disabled={!specified} onChange={(v) => setRow({ italic: v })} />

            <LengthBox label="Text height:" value={heightText} unit={unit} disabled={!specified} onChange={setHeightText} />
            <TriBox label="Keep upright" value={rows.keepUpright} disabled={!specified} onChange={(v) => setRow({ keepUpright: v })} />

            <LengthBox
              label="Text thickness:"
              value={rows.autoThickness ? (autoPen == null ? "" : shown(autoPen, unit)) : thicknessText}
              placeholder={rows.autoThickness ? "(auto)" : undefined}
              unit={unit}
              disabled={!specified || rows.autoThickness}
              onChange={setThicknessText}
            />
            <span style={{ display: "inline-flex", alignItems: "center", gap: 12 }}>
              <button
                type="button"
                className={rows.autoThickness ? "primary" : undefined}
                aria-pressed={rows.autoThickness}
                disabled={!specified}
                title="Adjust the text thickness"
                aria-label="Auto thickness"
                onClick={() => {
                  // `onAutoTextThickness`: off again puts the box back to "-- leave unchanged --"
                  if (rows.autoThickness) setThicknessText("");
                  setRow({ autoThickness: !rows.autoThickness });
                }}
              >
                auto
              </button>
              <label style={{ display: "inline-flex", alignItems: "center", gap: 4, opacity: specified ? 1 : 0.5 }} title="Move footprint texts to the center of their parent">
                <input type="checkbox" checked={rows.centerOnFootprint} disabled={!specified} onChange={(e) => setRow({ centerOnFootprint: e.target.checked })} aria-label="Center on footprint" />
                Center on footprint
              </label>
            </span>
          </div>

          <label className="filter-row" style={{ display: "block" }}>
            <input type="radio" checked={!specified} onChange={() => setMode("layer_defaults")} aria-label="Set to layer default values" />{" "}
            {scope.footprintDimensions || scope.dimensions ? "Set to layer and dimension default values:" : "Set to layer default values:"}
          </label>
          <table style={{ marginLeft: 20, fontSize: 11, borderCollapse: "collapse", opacity: specified ? 0.5 : 1 }} aria-label="Layer class defaults">
            <thead>
              <tr>
                {["", "Line Thickness", "Text Width", "Text Height", "Text Thickness", "Italic", "Keep Upright"].map((h) => (
                  <th key={h} style={{ padding: "1px 6px", fontWeight: 600, textAlign: h === "" ? "left" : "center" }}>
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {grid.map((r) => (
                <tr key={r.name}>
                  <td style={{ padding: "1px 6px" }}>{r.name}</td>
                  <td style={{ padding: "1px 6px", textAlign: "right" }}>{cell(r.line)}</td>
                  <td style={{ padding: "1px 6px", textAlign: "right" }}>{cell(r.textWidth)}</td>
                  <td style={{ padding: "1px 6px", textAlign: "right" }}>{cell(r.textHeight)}</td>
                  <td style={{ padding: "1px 6px", textAlign: "right" }}>{cell(r.textThickness)}</td>
                  <td style={{ padding: "1px 6px", textAlign: "center" }}>{tick(r.italic)}</td>
                  <td style={{ padding: "1px 6px", textAlign: "center" }}>{tick(r.upright)}</td>
                </tr>
              ))}
            </tbody>
          </table>

          {error && (
            <div style={{ color: "var(--chrome-error, #d33)", marginTop: 8 }} role="alert">
              {error}
            </div>
          )}
          <div style={{ fontSize: 11, opacity: 0.7, marginTop: 10 }} data-testid="global-edit-text-graphics-count">
            {match.fieldIds.length} field(s), {match.fpTextIds.length + match.fpItemTextIds.length} footprint text(s), {match.fpShapeIds.length + match.fpItemShapeIds.length} footprint graphic(s),{" "}
            {match.fpDimensionIds.length} footprint dimension(s), {match.shapeIds.length} graphic item(s), {match.textIds.length} text(s) and {match.dimensionIds.length} dimension(s) match the current
            scope and filters.
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Close</button>
          <button className="primary" onClick={apply} disabled={busy || total === 0 || !anyChange}>
            Apply and Close
          </button>
        </div>
      </div>
    </div>
  );
}
