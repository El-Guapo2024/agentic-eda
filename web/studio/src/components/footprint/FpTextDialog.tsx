// The Footprint Editor's Text tool dialog: `DIALOG_TEXT_PROPERTIES` (pcbnew/dialogs/dialog_text_properties.cpp) on the text `DRAWING_TOOL::PlaceText`'s first click made
// (kicad-port/fpTextTool.ts). For a footprint's text it is "Footprint Text Properties", one line of text, "Offset X:" / "Offset Y:" for the position, no Visible box.
// OK with no printable character drops the text (`NoPrintableChars`); a text whose offset was changed here is placed at once ("If the user modified the location then go
// ahead and place it there"), any other follows the cursor until the click that commits it. A pen too thick for the size is clamped with KiCad's message.
// The model's text has one size (square), no italic, bold, knockout or keep-upright, and no vertical justification: those controls are left out.
import { useEffect, useState, type KeyboardEvent } from "react";
import { useFpApi, useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { noPrintableChars } from "../../kicad-port/fpTextTool";
import { FP_EDITOR_LAYERS } from "../../kicad-port/fpDrawDefaults";
import { TEXT_MAX_NM, TEXT_MIN_NM } from "../../kicad-port/fpFields";
import type { TextJustify } from "../../api/types";

/** The layers a footprint text can be put on: the Footprint Editor's layer box. */
const FP_TEXT_LAYERS = FP_EDITOR_LAYERS;

const mm = (nm: number) => String(Math.round(nm) / 1e6);
const nm = (v: string) => Math.round(Number(v) * 1e6);

export function FpTextDialog() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const text = state.textDialog;
  const [content, setContent] = useState("");
  const [layer, setLayer] = useState("F.SilkS");
  const [size, setSize] = useState("1");
  const [thickness, setThickness] = useState("0.1");
  const [x, setX] = useState("0");
  const [y, setY] = useState("0");
  const [angle, setAngle] = useState("0");
  const [justify, setJustify] = useState<TextJustify>("left");
  const [mirror, setMirror] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!text) return;
    setContent(text.content);
    setLayer(text.layer);
    setSize(mm(text.size_nm));
    setThickness(mm(text.stroke_width));
    setX(mm(text.at.x));
    setY(mm(text.at.y));
    setAngle(String(text.angle / 1000));
    setJustify(text.justify);
    setMirror(text.mirror);
    setError(null);
  }, [text]);

  if (!text) return null;
  const close = () => dispatch({ type: "SET_TEXT_DIALOG", text: null });
  const ok = () => {
    // `DIALOG_TEXT_PROPERTIES::TransferDataFromWindow`: the size in KiCad's limits; a pen thicker than ClampTextPenSize allows is clamped, with the message.
    const sizeNm = nm(size);
    if (!(sizeNm >= TEXT_MIN_NM && sizeNm <= TEXT_MAX_NM)) return setError(`The text size must be between ${TEXT_MIN_NM / 1e6} mm and ${TEXT_MAX_NM / 1e6} mm.`);
    let pen = Math.max(0, nm(thickness) || 0);
    const maxPen = Math.round(sizeNm * 0.25);
    if (pen > maxPen) {
      dispatch({ type: "TOAST", message: "The text thickness is too large for the text size.\nIt will be clamped.", kind: "error" }); // `DisplayError`
      pen = maxPen;
    }
    close();
    // `cancelled || NoPrintableChars( text->GetText() )`: the text is dropped
    if (noPrintableChars(content)) return;
    const at = { x: nm(x) || 0, y: nm(y) || 0 };
    const done = { ...text, content, layer, size_nm: sizeNm, stroke_width: pen, angle: Math.round((Number(angle) || 0) * 1000), justify, mirror, at };
    if (at.x !== text.at.x || at.y !== text.at.y) {
      // the offset was edited: placed there now, "Draw Text", and the tool stays
      void api.addText(done);
    } else {
      dispatch({ type: "SET_CARRIED_TEXT", text: done });
    }
  };
  const enter = (e: KeyboardEvent) => {
    if (e.key === "Enter") ok();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 380 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Footprint Text Properties" data-testid="fp-text-dialog">
        <div className="dialog-header">
          <span>Footprint Text Properties</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "90px 1fr" }}>
            <span>Text:</span>
            <input autoFocus value={content} data-testid="fp-text-content" onChange={(e) => setContent(e.target.value)} onKeyDown={enter} />
            <span>Layer:</span>
            <select value={layer} data-testid="fp-text-layer" onChange={(e) => setLayer(e.target.value)}>
              {(FP_TEXT_LAYERS.includes(layer) ? FP_TEXT_LAYERS : [layer, ...FP_TEXT_LAYERS]).map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
            <span>Size:</span>
            <span>
              <input type="number" step="any" value={size} data-testid="fp-text-size" onChange={(e) => setSize(e.target.value)} onKeyDown={enter} style={{ width: 80 }} /> mm
            </span>
            <span>Thickness:</span>
            <span>
              <input type="number" step="any" value={thickness} data-testid="fp-text-thickness" onChange={(e) => setThickness(e.target.value)} onKeyDown={enter} style={{ width: 80 }} /> mm
            </span>
            <span>Offset X:</span>
            <span>
              <input type="number" step="any" value={x} data-testid="fp-text-x" onChange={(e) => setX(e.target.value)} onKeyDown={enter} style={{ width: 80 }} /> mm
            </span>
            <span>Offset Y:</span>
            <span>
              <input type="number" step="any" value={y} data-testid="fp-text-y" onChange={(e) => setY(e.target.value)} onKeyDown={enter} style={{ width: 80 }} /> mm
            </span>
            <span>Orientation:</span>
            <span>
              <input type="number" step="any" value={angle} data-testid="fp-text-angle" onChange={(e) => setAngle(e.target.value)} onKeyDown={enter} style={{ width: 80 }} /> °
            </span>
            <span>Justification:</span>
            <select value={justify} data-testid="fp-text-justify" onChange={(e) => setJustify(e.target.value as TextJustify)}>
              <option value="left">Left</option>
              <option value="center">Center</option>
              <option value="right">Right</option>
            </select>
            <span>Mirrored</span>
            <input type="checkbox" checked={mirror} data-testid="fp-text-mirror" onChange={(e) => setMirror(e.target.checked)} style={{ justifySelf: "start" }} />
          </div>
          {error && <p style={{ color: "var(--error, #e66)", margin: "8px 0 0" }}>{error}</p>}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" data-testid="fp-text-ok" onClick={ok}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
