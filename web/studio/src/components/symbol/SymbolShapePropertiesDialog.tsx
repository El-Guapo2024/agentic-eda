// `DIALOG_SHAPE_PROPERTIES` (eeschema/dialogs/dialog_shape_properties.cpp) of a rectangle, circle, arc or polygon of the open symbol: "<Shape> Properties" with a Border box and its
// width (0 is the schematic's default line width), the Fill Style (do not fill, the body's outline colour, its background colour) and the two "Common to all ..." boxes -- enabled for
// a symbol with several units or body styles. The border's colour and style, a custom fill colour and Private to Symbol Editor are not modelled by the library symbol. A derived
// symbol has no drawing of its own, so its OK says "Read Only" and is off, as `IsSymbolAlias()` makes it in KiCad. The rules live in kicad-port/symShapeProps.ts.
import { useEffect, useState } from "react";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { BORDER_HELP, FILL_CHOICES, shapeEditOf, shapeFormOf, shapeTitle, type ShapeForm } from "../../kicad-port/symShapeProps";

export function SymbolShapePropertiesDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const id = state.shapeDialogId;
  const symbol = state.symbol;
  const g = id ? symbol?.graphics.find((x) => x.id === id) : undefined;
  const shape = g && g.kind !== "text" && g.kind !== "text_box" ? g : undefined;
  const [form, setForm] = useState<ShapeForm | null>(null);

  // the form is filled when the dialog opens on a shape, and keeps what is typed through the editor's polling
  useEffect(() => {
    if (shape && symbol) setForm(shapeFormOf(shape, symbol));
    else setForm(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  if (!id || !shape || !symbol || !form) return null;
  const readOnly = !!state.derivedOwn;
  const set = <K extends keyof ShapeForm>(key: K, value: ShapeForm[K]) => setForm((f) => (f ? { ...f, [key]: value } : f));
  const close = () => dispatch({ type: "SET_SHAPE_DIALOG_ID", id: null });
  const ok = async () => {
    const edit = shapeEditOf(form, { unit: state.activeUnit, bodyStyle: state.activeBodyStyle });
    if (await api.editGraphic(id, edit.strokeMm, edit.fill, { unit: edit.unit, bodyStyle: edit.bodyStyle })) {
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
          <p style={{ margin: "4px 0 10px", fontStyle: "italic", fontSize: 11, opacity: 0.8 }}>{BORDER_HELP}</p>
          <fieldset style={{ border: "1px solid var(--chrome-border)", margin: "0 0 10px", padding: "6px 10px" }}>
            <legend>Fill Style</legend>
            {FILL_CHOICES.map((f) => (
              <label key={f.value} style={{ display: "block" }}>
                <input type="radio" name="shape-fill" checked={form.fill === f.value} onChange={() => set("fill", f.value)} data-testid={`shape-fill-${f.value}`} /> {f.label}
              </label>
            ))}
          </fieldset>
          <label style={{ display: "block" }} title={symbol.unit_count > 1 ? undefined : "This symbol only has one unit. This control has no effect."}>
            <input type="checkbox" checked={form.commonUnits} disabled={symbol.unit_count <= 1} onChange={(e) => set("commonUnits", e.target.checked)} data-testid="shape-common-units" /> Common to all units in symbol
          </label>
          <label style={{ display: "block" }} title={symbol.has_alternate_body_style ? undefined : "This symbol only has one body style. This control has no effect."}>
            <input type="checkbox" checked={form.commonBodyStyles} disabled={!symbol.has_alternate_body_style} onChange={(e) => set("commonBodyStyles", e.target.checked)} data-testid="shape-common-styles" /> Common to all body styles
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
