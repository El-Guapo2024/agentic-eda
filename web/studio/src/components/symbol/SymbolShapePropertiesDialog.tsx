// `DIALOG_SHAPE_PROPERTIES` (eeschema/dialogs/dialog_shape_properties.cpp) of a rectangle, circle, arc or polygon of the open symbol: "<Shape> Properties" with a Border box and its
// width (0 is the schematic's default line width), the Fill Style (do not fill, the body's outline colour, its background colour) and the two "Common to all ..." boxes -- enabled for
// a symbol with several units or body styles; the border's Color and Style, "Fill with:" a colour of its own and "Private to Symbol Editor" (UI-LEFT-2 YY2), kept beside the shape
// (`set_symbol_graphic_looks`) in the dialog's one batch. A derived
// symbol has no drawing of its own, so its OK says "Read Only" and is off, as `IsSymbolAlias()` makes it in KiCad. The rules live in kicad-port/symShapeProps.ts.
import { useEffect, useState } from "react";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { BORDER_HELP, FILL_CHOICES, schColorOfCss, shapeEditOf, shapeFormOf, shapeTitle, withBorderColor, withFillChoice, withFillColor, type DeviceColors, type ShapeForm } from "../../kicad-port/symShapeProps";
import { layerColorIn } from "../canvas/layers";
import { isMultiBodyStyle } from "../../kicad-port/symBodyStyles";
import { graphicLooksCmd, graphicStateOf, LINE_STYLE_CHOICES } from "../../kicad-port/symLooks";
import { ColorSwatch } from "../TextLookControls";
import type { Cmd } from "../../api/types";

export function SymbolShapePropertiesDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const id = state.shapeDialogId;
  const symbol = state.symbol;
  const g = id ? symbol?.graphics.find((x) => x.id === id) : undefined;
  const shape = g && g.kind !== "text" && g.kind !== "text_box" ? g : undefined;
  const [form, setForm] = useState<ShapeForm | null>(null);
  // the fill swatch's colours of the body fills, in the editor's theme (`m_frame->GetRenderSettings()->GetLayerColor( LAYER_DEVICE / LAYER_DEVICE_BACKGROUND )`, UI-LEFT-3 Y3-5)
  const layer = (): DeviceColors => ({ device: schColorOfCss(layerColorIn("symbol", "LAYER_DEVICE")), deviceBackground: schColorOfCss(layerColorIn("symbol", "LAYER_DEVICE_BACKGROUND")) });

  // the form is filled when the dialog opens on a shape, and keeps what is typed through the editor's polling
  useEffect(() => {
    if (shape && symbol && id) setForm(shapeFormOf(shape, symbol, graphicStateOf(symbol, id), layer()));
    else setForm(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  if (!id || !shape || !symbol || !form) return null;
  const readOnly = !!state.derivedOwn;
  const set = <K extends keyof ShapeForm>(key: K, value: ShapeForm[K]) => setForm((f) => (f ? { ...f, [key]: value } : f));
  const close = () => dispatch({ type: "SET_SHAPE_DIALOG_ID", id: null });
  const ok = async () => {
    const before = graphicStateOf(symbol, id);
    const edit = shapeEditOf(form, { unit: state.activeUnit, bodyStyle: state.activeBodyStyle }, before);
    const libId = state.libId!;
    const cmds: Cmd[] = [{ op: "edit_symbol_graphic", lib_id: libId, id, stroke_mm: edit.strokeMm, fill: edit.fill, unit: edit.unit, body_style: edit.bodyStyle }, ...graphicLooksCmd(libId, id, edit.state, before)];
    if (await api.runCmds(cmds)) {
      // `editShapeProperties`: `SetDrawSpecificBodyStyle( !dlg.GetApplyToAllConversions() )`, `SetDrawSpecificUnit( !dlg.GetApplyToAllUnits() )`
      dispatch({ type: "SET_DRAW_SPECIFIC", unit: !form.commonUnits, bodyStyle: !form.commonBodyStyles });
      close();
    }
  };
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 400 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label={shapeTitle(shape)} data-testid="shape-properties">
        <div className="dialog-header">
          <span>{shapeTitle(shape)}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "100px 1fr" }}>
            <span>Border</span>
            <span>
              <input type="checkbox" checked={form.border} onChange={(e) => set("border", e.target.checked)} data-testid="shape-border" aria-label="Border" />
              <span style={{ marginLeft: 8, opacity: form.border ? 1 : 0.5 }}>
                Width:{" "}
                <input
                  type="number"
                  step="0.01"
                  min={0}
                  autoFocus
                  value={form.widthMm}
                  disabled={!form.border}
                  onChange={(e) => Number.isFinite(Number(e.target.value)) && set("widthMm", Number(e.target.value))}
                  style={{ width: 70 }}
                  data-testid="shape-width"
                />{" "}
                mm
              </span>
            </span>
          </div>
          <div className="kv-grid" style={{ gridTemplateColumns: "100px 1fr", opacity: form.border ? 1 : 0.5, pointerEvents: form.border ? undefined : "none" }}>
            <span>Color:</span>
            <span data-testid="shape-border-color">
              <ColorSwatch label="Border color" color={form.borderColor} onChange={(c) => setForm((x) => (x ? withBorderColor(x, c, layer()) : x))} />
            </span>
            <span>Style:</span>
            <select value={form.style} disabled={!form.border} onChange={(e) => set("style", e.target.value)} data-testid="shape-border-style" aria-label="Border style">
              {LINE_STYLE_CHOICES.map((c) => (
                <option key={c.value} value={c.value}>
                  {c.label}
                </option>
              ))}
            </select>
          </div>
          <p style={{ margin: "4px 0 10px", fontStyle: "italic", fontSize: 11, opacity: 0.8 }}>{BORDER_HELP}</p>
          <fieldset style={{ border: "1px solid var(--chrome-border)", margin: "0 0 10px", padding: "6px 10px" }}>
            <legend>Fill Style</legend>
            {FILL_CHOICES.map((f) => (
              <label key={f.value} style={{ display: "block" }}>
                <input
                  type="radio"
                  name="shape-fill"
                  checked={form.fill === f.value}
                  onChange={(e) => {
                    setForm((x) => (x ? withFillChoice(x, f.value, layer()) : x));
                    // `m_customColorSwatch->GetNewSwatchColor()`: "Fill with:" asks for the colour at once
                    if (f.value === "color") (e.currentTarget.closest("label")?.querySelector('input[type="color"]') as (HTMLInputElement & { showPicker?: () => void }) | null)?.showPicker?.();
                  }}
                  data-testid={`shape-fill-${f.value}`}
                />{" "}
                {f.label}
                {f.value === "color" && (
                  // `m_customColorSwatch`: the colour of the fill chosen (the layer's for the body fills); picking a colour chooses the radio (`onCustomColorSwatch`)
                  <span data-testid="shape-fill-swatch" style={{ display: "inline-block", marginLeft: 8 }}>
                    <ColorSwatch label="Fill color" color={form.fillColor} onChange={(c) => setForm((x) => (x ? withFillColor(x, c) : x))} />
                  </span>
                )}
              </label>
            ))}
          </fieldset>
          <label style={{ display: "block" }}>
            <input type="checkbox" checked={form.private} onChange={(e) => set("private", e.target.checked)} data-testid="shape-private" /> Private to Symbol Editor
          </label>
          <label style={{ display: "block" }} title={symbol.unit_count > 1 ? undefined : "This symbol only has one unit. This control has no effect."}>
            <input type="checkbox" checked={form.commonUnits} disabled={symbol.unit_count <= 1} onChange={(e) => set("commonUnits", e.target.checked)} data-testid="shape-common-units" /> Common to all units in symbol
          </label>
          <label style={{ display: "block" }} title={isMultiBodyStyle(symbol) ? undefined : "This symbol only has one body style. This control has no effect."}>
            <input type="checkbox" checked={form.commonBodyStyles} disabled={!isMultiBodyStyle(symbol)} onChange={(e) => set("commonBodyStyles", e.target.checked)} data-testid="shape-common-styles" /> Common to all body styles
          </label>
        </div>
        <div className="dialog-footer">
          <button onClick={close} data-testid="shape-cancel">
            Cancel
          </button>
          <button className="primary" onClick={() => void ok()} disabled={readOnly} data-testid="shape-ok">
            {readOnly ? "Read Only" : "OK"}
          </button>
        </div>
      </div>
    </div>
  );
}
