// `DIALOG_TEXT_PROPERTIES` of a symbol's text (eeschema/dialogs/dialog_text_properties.cpp, a `SCH_TEXT` on `LAYER_DEVICE` in the symbol editor), opened two ways:
//
// - by the Text tool's first click (`SYMBOL_EDITOR_DRAWING_TOOLS::TwoClickPlace`, SCH_TEXT_T): a new text at the click, with the default size and the last angle; OK hands it to
//   the canvas, where it follows the cursor until the click that places it; Cancel -- or a text with nothing printable in it -- drops it;
// - on a text of the symbol (a double click, `E`, Properties in the context menu: `SYMBOL_EDITOR_EDIT_TOOL::editTextProperties`): OK is one undo step, "Edit Text Properties".
//
// The text, the Font choice, "Text size:", Bold and Italic, the text colour, the horizontal and vertical justification, Horizontal or Vertical, and in the symbol editor
// "Private to Symbol Editor", "Common to all units" and "Common to all body styles" (enabled for a symbol of several units / body styles). The hyperlink is the schematic's only
// (`m_hyperlinkCb->Hide()` outside it). The font and colour are kept and written (UI-LEFT Y4); the canvas draws the text in its face (UI-LEFT-2 CC1: kicad-port/schTextSpecs.ts, an outline font once the browser has it).
import { useEffect, useRef, useState } from "react";
import { drawTarget, useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { DEFAULT_SYMBOL_TEXT_SIZE_MM, noPrintableChars, textFormOf, textFromForm, type LibText, type TextForm } from "../../kicad-port/symText";
import { H_ALIGNS, V_ALIGNS } from "../../kicad-port/symTextBox";
import { nmPointToMm } from "./symbolPainter";
import { isMultiBodyStyle } from "../../kicad-port/symBodyStyles";
import { ColorSwatch, FontSelect, useFontFamilies } from "../TextLookControls";

const H_TIPS: Record<string, string> = { left: "Align left", center: "Align horizontal center", right: "Align right" };
const V_TIPS: Record<string, string> = { top: "Align top", center: "Align vertical center", bottom: "Align bottom" };
const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

export function SymbolTextDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const req = state.textDialog;
  const symbol = state.symbol;
  const [form, setForm] = useState<TextForm | null>(null);
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);
  const families = useFontFamilies();

  /** The text the dialog is on: the symbol's (by id), or the new one of the Text tool -- at the click, `DEFAULT_TEXT_SIZE`, `m_lastTextAngle`, on the unit and body style the
   * drawing tools put new items on (`m_drawSpecificUnit`, `m_drawSpecificBodyStyle`). */
  const target = (): LibText | null => {
    if (!req || !symbol) return null;
    if ("id" in req) {
      const g = symbol.graphics.find((x) => x.id === req.id);
      return g?.kind === "text" ? g : null;
    }
    const at = drawTarget(state);
    return { kind: "text", unit: at.unit, body_style: at.bodyStyle, text: "", at: nmPointToMm(req.atNm[0], req.atNm[1]), angle_deg: state.lastTextAngle, size_mm: DEFAULT_SYMBOL_TEXT_SIZE_MM };
  };

  // the form starts from the text when the dialog opens, and keeps what is typed through the editor's polling
  useEffect(() => {
    const t = target();
    setForm(t && symbol ? textFormOf(t, symbol) : null);
    setError(null);
    ref.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [req]);

  const text = target();
  if (!req || !symbol || !form || !text) return null;
  const isNew = !("id" in req);
  const set = <K extends keyof TextForm>(key: K, value: TextForm[K]) => {
    setForm((f) => (f ? { ...f, [key]: value } : f));
    setError(null);
  };
  const close = () => dispatch({ type: "SET_TEXT_DIALOG", dialog: null });
  const ok = async () => {
    const out = textFromForm(text, form, state.activeUnit, state.activeBodyStyle);
    if (!out.ok) return setError(out.error);
    if (isNew) {
      // `dlg.ShowModal() != wxID_OK || NoPrintableChars( text->GetText() )`: no item
      if (noPrintableChars(out.text.text)) return close();
      api.carryText(out.text);
      return;
    }
    if (await api.replaceGraphic(text.id!, out.text)) close();
  };
  const toggle = (key: "bold" | "italic", tip: string, glyph: string) => (
    <button type="button" className={form[key] ? "primary" : undefined} aria-pressed={form[key]} title={tip} data-testid={`text-${key}`} onClick={() => set(key, !form[key])} style={{ minWidth: 28 }}>
      {glyph}
    </button>
  );
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 440 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Text Properties" data-testid="text-properties">
        <div className="dialog-header">
          <span>Text Properties</span>
        </div>
        <div className="dialog-body">
          <label style={{ display: "block", marginBottom: 4 }}>Text:</label>
          <textarea
            ref={ref}
            value={form.text}
            rows={3}
            style={{ width: "100%", boxSizing: "border-box" }}
            data-testid="text-text"
            onChange={(e) => set("text", e.target.value)}
            onKeyDown={(e) => {
              // `SCINTILLA_TRICKS` of a multi-line text: Shift+Enter accepts (`m_singleLine || aEvent.ShiftDown()`), Enter is a new line
              if (e.key === "Enter" && e.shiftKey) {
                e.preventDefault();
                void ok();
              }
              if (e.key === "Escape") close();
            }}
          />
          <div style={{ display: "flex", gap: 4, alignItems: "center", flexWrap: "wrap", marginTop: 8 }}>
            {toggle("bold", "Bold", "B")}
            {toggle("italic", "Italic", "I")}
            <span style={{ width: 8 }} />
            {H_ALIGNS.map((a) => (
              <label key={a} title={H_TIPS[a]} style={{ marginRight: 6 }}>
                <input type="radio" name="sym-text-h" checked={form.h === a} onChange={() => set("h", a)} data-testid={`text-h-${a}`} /> {cap(a)}
              </label>
            ))}
          </div>
          <div style={{ display: "flex", gap: 4, alignItems: "center", flexWrap: "wrap", marginTop: 6 }}>
            {V_ALIGNS.map((a) => (
              <label key={a} title={V_TIPS[a]} style={{ marginRight: 6 }}>
                <input type="radio" name="sym-text-v" checked={form.v === a} onChange={() => set("v", a)} data-testid={`text-v-${a}`} /> {cap(a)}
              </label>
            ))}
            <span style={{ width: 8 }} />
            <label title="Horizontal text" style={{ marginRight: 6 }}>
              <input type="radio" name="sym-text-orient" checked={!form.vertical} onChange={() => set("vertical", false)} data-testid="text-horizontal" /> Horizontal
            </label>
            <label title="Vertical text">
              <input type="radio" name="sym-text-orient" checked={form.vertical} onChange={() => set("vertical", true)} data-testid="text-vertical" /> Vertical
            </label>
          </div>
          <div className="kv-grid" style={{ gridTemplateColumns: "150px 1fr", marginTop: 10 }}>
            <span>Font:</span>
            <span data-testid="text-font">
              <FontSelect families={families} current={text.font} value={form.font} onChange={(v) => set("font", v)} />
            </span>
            <span>Text size:</span>
            <span>
              <input type="number" step="0.01" value={form.size} onChange={(e) => set("size", e.target.value)} style={{ width: 90 }} data-testid="text-size" /> mm
            </span>
            <span>Color:</span>
            <span data-testid="text-color">
              <ColorSwatch label="Text color" color={form.color} onChange={(c) => set("color", c)} />
            </span>
            <span>Private to symbol editor</span>
            <label>
              <input type="checkbox" checked={form.private} onChange={(e) => set("private", e.target.checked)} data-testid="text-private" />
            </label>
            <span>Common to all units</span>
            <label title={symbol.unit_count > 1 ? undefined : "This symbol only has one unit. This control has no effect."}>
              <input type="checkbox" checked={form.commonUnits} disabled={symbol.unit_count <= 1} onChange={(e) => set("commonUnits", e.target.checked)} data-testid="text-common-units" />
            </label>
            <span>Common to all body styles</span>
            <label title={isMultiBodyStyle(symbol) ? undefined : "This symbol only has one body style. This control has no effect."}>
              <input type="checkbox" checked={form.commonStyles} disabled={!isMultiBodyStyle(symbol)} onChange={(e) => set("commonStyles", e.target.checked)} data-testid="text-common-styles" />
            </label>
          </div>
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="text-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close} data-testid="text-cancel">
            Cancel
          </button>
          <button className="primary" onClick={() => void ok()} data-testid="text-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
