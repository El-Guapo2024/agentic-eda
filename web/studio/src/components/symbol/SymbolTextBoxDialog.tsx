// `DIALOG_TEXT_PROPERTIES` of a `SCH_TEXTBOX` (eeschema/dialogs/dialog_text_properties.cpp, "Text Box Properties") as the Symbol Editor opens it: after the Text Box tool's second click
// -- a box of the two corners, added by OK, and gone with Cancel (`SYMBOL_EDITOR_DRAWING_TOOLS::doDrawShape`) -- or on a box that is there (a double click, `E`, Properties in the
// context menu). The text, its size, bold and italic, its alignment in the box and the way it runs, the border and its width, the fill, and the two "Common to all ..." boxes; the
// font, the text and border colours, the border style, the hyperlink and Private to Symbol Editor are not part of a library symbol's text box here. OK makes the box as big as its
// text needs (`GetMinSize`, kicad-port/symTextBox.ts) and, for a box the tool drew, remembers its looks for the next one.
import { useEffect, useRef, useState } from "react";
import type { FieldHAlign, FieldVAlign } from "../../api/types";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { textSizeError } from "../../kicad-port/symText";
import { H_ALIGNS, NO_BORDER_MM, V_ALIGNS, grownToFit, isVertical, lastOf, textBoxMarginMm, type LibTextBox } from "../../kicad-port/symTextBox";
import { measureStrokeText } from "../text/strokeFont";

const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

interface Form {
  text: string;
  size: string;
  bold: boolean;
  italic: boolean;
  h: FieldHAlign;
  v: FieldVAlign;
  vertical: boolean;
  border: boolean;
  width: string;
  filled: boolean;
  commonUnits: boolean;
  commonStyles: boolean;
}

/** `TransferDataToWindow` for a text box in the symbol editor. */
function formOf(box: LibTextBox, symbol: { unit_count: number; has_alternate_body_style: boolean }): Form {
  return {
    text: box.text,
    size: String(box.size_mm),
    bold: !!box.bold,
    italic: !!box.italic,
    h: box.h_align,
    v: box.v_align,
    vertical: isVertical(box),
    border: box.stroke_mm >= 0,
    width: String(box.stroke_mm >= 0 ? box.stroke_mm : 0),
    filled: box.fill !== "none",
    commonUnits: symbol.unit_count > 1 && box.unit === 0,
    commonStyles: symbol.has_alternate_body_style && box.body_style === 0,
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

  // the form starts from the box the dialog was opened on, and keeps what is typed through the editor's polling
  useEffect(() => {
    if (!req || !symbol) {
      setForm(null);
      return;
    }
    setForm(formOf(req.box, symbol));
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
      ...prev,
      text: form.text,
      size_mm: sizeMm,
      bold: form.bold,
      italic: form.italic,
      h_align: form.h,
      v_align: form.v,
      angle_deg: form.vertical ? 90 : 0,
      stroke_mm: form.border ? Math.max(0, Number.isFinite(width) ? width : 0) : NO_BORDER_MM,
      // the box keeps the fill it had when it stays filled; a new fill is the body's background (the dialog's fill colour is the one the symbol is drawn on)
      fill: form.filled ? (prev.fill === "none" ? "background" : prev.fill) : "none",
      // the margins are numbers a box has: a change of the text size leaves them where they were
      margin_mm: (prev.margin_mm ?? 0) > 0 ? prev.margin_mm : textBoxMarginMm(prev),
      unit: form.commonUnits ? 0 : state.activeUnit,
      body_style: form.commonStyles ? 0 : state.activeBodyStyle,
    };
    box = grownToFit(box, measureStrokeText);
    if (req.id) {
      if (!(await api.replaceGraphic(req.id, box))) return;
    } else {
      dispatch({ type: "SET_LAST_TEXT_BOX", last: lastOf(box) });
      await api.addGraphic(box);
    }
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
            <span>Fill</span>
            <label>
              <input type="checkbox" checked={form.filled} onChange={(e) => set("filled", e.target.checked)} data-testid="tb-filled" /> Filled
            </label>
            <span>Common to all units</span>
            <label title={symbol.unit_count > 1 ? undefined : "This symbol only has one unit. This control has no effect."}>
              <input type="checkbox" checked={form.commonUnits} disabled={symbol.unit_count <= 1} onChange={(e) => set("commonUnits", e.target.checked)} data-testid="tb-common-units" />
            </label>
            <span>Common to all body styles</span>
            <label title={symbol.has_alternate_body_style ? undefined : "This symbol only has one body style. This control has no effect."}>
              <input type="checkbox" checked={form.commonStyles} disabled={!symbol.has_alternate_body_style} onChange={(e) => set("commonStyles", e.target.checked)} data-testid="tb-common-styles" />
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
