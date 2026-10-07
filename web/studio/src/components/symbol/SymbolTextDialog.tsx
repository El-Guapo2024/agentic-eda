// `DIALOG_TEXT_PROPERTIES` as the Symbol Editor's Text tool opens it (`TwoClickPlace`, SCH_TEXT_T): the text, its size and whether it reads
// horizontally or vertically. (The dialog's bold / italic / justification / hyperlink are not part of a library symbol's text here: the
// symbol library keeps a text's content, position, angle and size.) OK hands the text to the canvas, where it follows the cursor until the
// click that places it; Cancel -- or a text with nothing printable in it -- drops it.
import { useEffect, useRef, useState } from "react";
import { useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { DEFAULT_SYMBOL_TEXT_SIZE_MM, noPrintableChars, textAngleDeg, textSizeError } from "../../kicad-port/symText";

export function SymbolTextDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const [text, setText] = useState("");
  const [size, setSize] = useState(DEFAULT_SYMBOL_TEXT_SIZE_MM);
  const [vertical, setVertical] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);
  const open = state.textDialog != null;

  useEffect(() => {
    if (!open) return;
    setText("");
    setVertical(state.lastTextAngle === 90); // `text->SetTextAngle( m_lastTextAngle )`
    setError(null);
    ref.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_TEXT_DIALOG", at: null });
  const ok = () => {
    const sizeError = textSizeError(size);
    if (sizeError) return setError(sizeError);
    if (noPrintableChars(text)) return close(); // `NoPrintableChars( text->GetText() )`: no item
    dispatch({ type: "SET_PENDING_TEXT", pending: { text, sizeMm: size, angleDeg: textAngleDeg(vertical) } });
  };
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 380 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Text Properties">
        <div className="dialog-header">
          <span>Text Properties</span>
        </div>
        <div className="dialog-body">
          <label style={{ display: "block", marginBottom: 4 }}>Text</label>
          <textarea
            ref={ref}
            value={text}
            rows={3}
            style={{ width: "100%", boxSizing: "border-box" }}
            onChange={(e) => {
              setText(e.target.value);
              setError(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) ok();
              if (e.key === "Escape") close();
            }}
          />
          <div className="kv-grid" style={{ gridTemplateColumns: "100px 1fr", marginTop: 10 }}>
            <span>Text size (mm)</span>
            <input type="number" step="0.01" value={size} onChange={(e) => setSize(Number(e.target.value))} style={{ width: 90 }} />
            <span>Orientation</span>
            <span>
              <label style={{ marginRight: 12 }}>
                <input type="radio" name="sym-text-orient" checked={!vertical} onChange={() => setVertical(false)} /> Horizontal
              </label>
              <label>
                <input type="radio" name="sym-text-orient" checked={vertical} onChange={() => setVertical(true)} /> Vertical
              </label>
            </span>
          </div>
          {error && <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }}>{error}</p>}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={ok}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
