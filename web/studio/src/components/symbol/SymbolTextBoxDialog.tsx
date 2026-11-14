// `DIALOG_TEXT_PROPERTIES` of a `SCH_TEXTBOX` (eeschema/dialogs/dialog_text_properties.cpp, "Text Box Properties") as the Symbol Editor opens it: after the Text Box tool's second click
// -- a box of the two corners, added by OK, and gone with Cancel (`SYMBOL_EDITOR_DRAWING_TOOLS::doDrawShape`) -- or on a box that is there (a double click, `E`, Properties in the
// context menu). The text, its font, size, bold and italic and colour, its alignment in the box and the way it runs, the border with its width and colour, "Background fill" with
// its colour (`FILLED_WITH_COLOR` or `NO_FILL`: a box filled with the body's background before is one filled with the default colour after, drawn the same), "Private to symbol
// editor" and the two "Common to all ..." boxes (UI-LEFT Y4), and the border's Style (`m_borderStyleCombo`, kept beside the box: `set_symbol_graphic_looks`, UI-LEFT-2 YY2). Not
// here: the hyperlink (`m_hyperlinkCb` is hidden outside the schematic editor).
// OK makes the box as big as its text needs (`GetMinSize`, kicad-port/symTextBox.ts) and, for a box the tool drew, remembers its looks for the next one.
import { useEffect, useRef, useState } from "react";
import type { FieldHAlign, FieldVAlign } from "../../api/types";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { textSizeError } from "../../kicad-port/symText";
import { H_ALIGNS, NO_BORDER_MM, V_ALIGNS, grownToFit, isVertical, lastOf, textBoxMarginMm, type LibTextBox } from "../../kicad-port/symTextBox";
import { measureStrokeText } from "../text/strokeFont";
import { isMultiBodyStyle, type BodyStyled } from "../../kicad-port/symBodyStyles";
import type { SchColor } from "../../api/schEditTypes";
import { looksFromForm, withoutLooks } from "../../kicad-port/symText";
import { DEFAULT_FONT } from "../../kicad-port/textLooks";
import { isUnspecified } from "../../kicad-port/schProperties";
import { ColorSwatch, FontSelect, useFontFamilies } from "../TextLookControls";
import { graphicLooksCmd, graphicStateOf, LINE_STYLE_CHOICES } from "../../kicad-port/symLooks";
import type { Cmd } from "../../api/types";

const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
/** A swatch's colour as the box keeps it: `COLOR4D::UNSPECIFIED` (the swatch's "Default") is none. */
const specified = (c: SchColor | null): SchColor | undefined => (c && !isUnspecified(c) ? c : undefined);

interface Form {
  /** "Private to symbol editor", the Font choice and the text colour (kicad-port/symText.ts `looksFromForm`). */
  private: boolean;
  font: string;
  color: SchColor | null;
  /** The border's "Color:" and the "Fill color:" swatches (`m_borderColorSwatch`, `m_fillColorSwatch`); `null` is "Default". */
  borderColor: SchColor | null;
  fillColor: SchColor | null;
  text: string;
  size: string;
  bold: boolean;
  italic: boolean;
  h: FieldHAlign;
  v: FieldVAlign;
  vertical: boolean;
  border: boolean;
  width: string;
  /** `m_borderStyleCombo`: the box's line style, Solid for the default one. */
  style: string;
  filled: boolean;
  commonUnits: boolean;
  commonStyles: boolean;
}

/** `TransferDataToWindow` for a text box in the symbol editor. */
function formOf(box: LibTextBox, symbol: { unit_count: number } & BodyStyled, lineStyle: string | null): Form {
  return {
    private: !!box.private,
    font: box.font || DEFAULT_FONT,
    color: box.text_color ?? null,
    borderColor: box.stroke_color ?? null,
    fillColor: box.fill_color ?? null,
    text: box.text,
    size: String(box.size_mm),
    bold: !!box.bold,
    italic: !!box.italic,
    h: box.h_align,
    v: box.v_align,
    vertical: isVertical(box),
    border: box.stroke_mm >= 0,
    width: String(box.stroke_mm >= 0 ? box.stroke_mm : 0),
    // `style == -1` (the default) shows `DEFAULT_LINE_STYLE_LABEL`, Solid
    style: lineStyle ?? "solid",
    // `m_filledCtrl->SetValue( textBox->IsSolidFill() )`
    filled: box.fill !== "none",
    commonUnits: symbol.unit_count > 1 && box.unit === 0,
    commonStyles: isMultiBodyStyle(symbol) && box.body_style === 0,
  };
}

export function SymbolTextBoxDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const req = state.textBoxDialog;
  const symbol = state.symbol;
  const [form, setForm] = useState<Form | null>(null);
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);
  const families = useFontFamilies();

  // the form starts from the box the dialog was opened on, and keeps what is typed through the editor's polling
  useEffect(() => {
    if (!req || !symbol) {
      setForm(null);
      return;
    }
    // a new box starts with the tool's last line style (`item->SetStroke( m_lastStroke )`, UI-LEFT-3 Y3-4)
    setForm(formOf(req.box, symbol, req.id ? graphicStateOf(symbol, req.id).lineStyle : (state.lastTextBox.lineStyle ?? null)));
    setError(null);
    ref.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [req]);

  if (!req || !symbol || !form) return null;
  const set = <K extends keyof Form>(key: K, value: Form[K]) => {
    setForm((f) => (f ? { ...f, [key]: value } : f));
    setError(null);
  };
  const close = () => dispatch({ type: "SET_TEXT_BOX_DIALOG", dialog: null });
  const ok = async () => {
    // `TransferDataFromWindow`: "Don't allow text to disappear"
    const sizeMm = Number(form.size);
    const bad = textSizeError(sizeMm);
    if (bad) return setError(bad);
    const prev = req.box;
    const width = Number(form.width);
    let box: LibTextBox = {
      ...withoutLooks(prev),
      text: form.text,
      size_mm: sizeMm,
      bold: form.bold,
      italic: form.italic,
      h_align: form.h,
      v_align: form.v,
      angle_deg: form.vertical ? 90 : 0,
      stroke_mm: form.border ? Math.max(0, Number.isFinite(width) ? width : 0) : NO_BORDER_MM,
      // `stroke.SetColor( m_borderColorSwatch->GetSwatchColor() )`
      ...(specified(form.borderColor) ? { stroke_color: specified(form.borderColor) } : { stroke_color: undefined }),
      // `SetFillMode( m_filledCtrl->GetValue() ? FILL_T::FILLED_WITH_COLOR : FILL_T::NO_FILL )`, `SetFillColor( m_fillColorSwatch->GetSwatchColor() )`
      fill: form.filled ? "color" : "none",
      ...(specified(form.fillColor) ? { fill_color: specified(form.fillColor) } : { fill_color: undefined }),
      ...looksFromForm(prev, form),
      // the margins are numbers a box has: a change of the text size leaves them where they were
      margin_mm: (prev.margin_mm ?? 0) > 0 ? prev.margin_mm : textBoxMarginMm(prev),
      unit: form.commonUnits ? 0 : state.activeUnit,
      body_style: form.commonStyles ? 0 : state.activeBodyStyle,
    };
    box = grownToFit(box, measureStrokeText);
    // `stroke.SetLineStyle( it->first )` of the Style chosen; the fill is a colour or none (a hatch goes), the link and the pen stay
    const before = req.id ? graphicStateOf(symbol, req.id) : { looks: {}, lineStyle: null, thicknessMm: null };
    const looks = graphicLooksCmd(state.libId!, req.id ?? "", { looks: { ...before.looks, hatch: null }, lineStyle: form.style, thicknessMm: before.thicknessMm }, req.id ? before : undefined);
    const cmds: Cmd[] = req.id ? [{ op: "replace_symbol_graphic", lib_id: state.libId!, id: req.id, graphic: box }, ...looks] : [{ op: "add_symbol_graphic", lib_id: state.libId!, graphic: box }, ...looks];
    if (!req.id) dispatch({ type: "SET_LAST_TEXT_BOX", last: lastOf(box, form.style) });
    if (!(await api.runCmds(cmds)) && req.id) return;
    close();
  };
  const radio = (name: string, testid: string, checked: boolean, label: string, onChange: () => void) => (
    <label style={{ marginRight: 10 }}>
      <input type="radio" name={name} checked={checked} onChange={onChange} data-testid={testid} /> {label}
    </label>
  );
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 460 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Text Box Properties" data-testid="text-box-properties">
        <div className="dialog-header">
          <span>Text Box Properties</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "74vh", overflowY: "auto" }}>
          <label style={{ display: "block", marginBottom: 4 }}>Text</label>
          <textarea
            ref={ref}
            value={form.text}
            rows={4}
            style={{ width: "100%", boxSizing: "border-box" }}
            data-testid="tb-text"
            onChange={(e) => set("text", e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void ok();
              if (e.key === "Escape") close();
            }}
          />
          <div className="kv-grid" style={{ gridTemplateColumns: "120px 1fr", marginTop: 10 }}>
            <span>Font:</span>
            <span data-testid="tb-font">
              <FontSelect families={families} current={req.box.font} value={form.font} onChange={(v) => set("font", v)} />
            </span>
            <span>Text size (mm)</span>
            <span>
              <input type="number" step="0.01" value={form.size} onChange={(e) => set("size", e.target.value)} style={{ width: 80 }} data-testid="tb-size" />
              <label style={{ marginLeft: 12 }}>
                <input type="checkbox" checked={form.bold} onChange={(e) => set("bold", e.target.checked)} data-testid="tb-bold" /> Bold
              </label>
              <label style={{ marginLeft: 12 }}>
                <input type="checkbox" checked={form.italic} onChange={(e) => set("italic", e.target.checked)} data-testid="tb-italic" /> Italic
              </label>
            </span>
            <span>Color:</span>
            <span data-testid="tb-color">
              <ColorSwatch label="Text color" color={form.color} onChange={(c) => set("color", c)} />
            </span>
            <span>Horizontal alignment</span>
            <span>{H_ALIGNS.map((a) => radio("tb-h", `tb-h-${a}`, form.h === a, cap(a), () => set("h", a)))}</span>
            <span>Vertical alignment</span>
            <span>{V_ALIGNS.map((a) => radio("tb-v", `tb-v-${a}`, form.v === a, cap(a), () => set("v", a)))}</span>
            <span>Orientation</span>
            <span>
              {radio("tb-orient", "tb-horizontal", !form.vertical, "Horizontal", () => set("vertical", false))}
              {radio("tb-orient", "tb-vertical", form.vertical, "Vertical", () => set("vertical", true))}
            </span>
            <span>Border</span>
            <span>
              <input type="checkbox" checked={form.border} onChange={(e) => set("border", e.target.checked)} data-testid="tb-border" aria-label="Border" />
              <span style={{ marginLeft: 8, opacity: form.border ? 1 : 0.5 }}>
                Width:{" "}
                <input type="number" step="0.01" min={0} value={form.width} disabled={!form.border} onChange={(e) => set("width", e.target.value)} style={{ width: 70 }} data-testid="tb-width" /> mm
              </span>
            </span>
            <span style={{ opacity: form.border ? 1 : 0.5 }}>Color:</span>
            <span data-testid="tb-border-color" style={{ opacity: form.border ? 1 : 0.5, pointerEvents: form.border ? undefined : "none" }}>
              <ColorSwatch label="Border color" color={form.borderColor} onChange={(c) => set("borderColor", c)} />
            </span>
            <span style={{ opacity: form.border ? 1 : 0.5 }}>Style:</span>
            <select value={form.style} disabled={!form.border} onChange={(e) => set("style", e.target.value)} data-testid="tb-border-style" aria-label="Border style">
              {LINE_STYLE_CHOICES.map((c) => (
                <option key={c.value} value={c.value}>
                  {c.label}
                </option>
              ))}
            </select>
            <span>Fill</span>
            <label>
              <input type="checkbox" checked={form.filled} onChange={(e) => set("filled", e.target.checked)} data-testid="tb-filled" /> Background fill
            </label>
            <span style={{ opacity: form.filled ? 1 : 0.5 }}>Fill color:</span>
            <span data-testid="tb-fill-color" style={{ opacity: form.filled ? 1 : 0.5, pointerEvents: form.filled ? undefined : "none" }}>
              <ColorSwatch label="Fill color" color={form.fillColor} onChange={(c) => set("fillColor", c)} />
            </span>
            <span>Private to symbol editor</span>
            <label>
              <input type="checkbox" checked={form.private} onChange={(e) => set("private", e.target.checked)} data-testid="tb-private" />
            </label>
            <span>Common to all units</span>
            <label title={symbol.unit_count > 1 ? undefined : "This symbol only has one unit. This control has no effect."}>
              <input type="checkbox" checked={form.commonUnits} disabled={symbol.unit_count <= 1} onChange={(e) => set("commonUnits", e.target.checked)} data-testid="tb-common-units" />
            </label>
            <span>Common to all body styles</span>
            <label title={isMultiBodyStyle(symbol) ? undefined : "This symbol only has one body style. This control has no effect."}>
              <input type="checkbox" checked={form.commonStyles} disabled={!isMultiBodyStyle(symbol)} onChange={(e) => set("commonStyles", e.target.checked)} data-testid="tb-common-styles" />
            </label>
          </div>
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="tb-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close} data-testid="tb-cancel">
            Cancel
          </button>
          <button className="primary" onClick={() => void ok()} data-testid="tb-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
