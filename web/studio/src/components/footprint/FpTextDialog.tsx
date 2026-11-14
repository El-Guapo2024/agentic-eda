// The Footprint Editor's Text tool dialog: `DIALOG_TEXT_PROPERTIES` (pcbnew/dialogs/dialog_text_properties.cpp) on the text `DRAWING_TOOL::PlaceText`'s first click made
// (kicad-port/fpTextTool.ts). For a footprint's text it is "Footprint Text Properties", one line of text, "Offset X:" / "Offset Y:" for the position, no Visible box.
// OK with no printable character drops the text (`NoPrintableChars`); a text whose offset was changed here is placed at once ("If the user modified the location then go
// ahead and place it there") and selected (`ACTIONS::selectItem`), any other follows the cursor until the click that commits it. A size outside KiCad's limits gets
// `UNIT_BINDER::Validate`'s message ("Width must be at least 0.001 mm."); a pen too thick for the size is clamped with KiCad's message.
// The model's text has one size (square), no italic, bold, knockout or keep-upright, and no vertical justification: those controls are left out.
import { textFaceOf, textLookOf } from "../../kicad-port/fpKeep";
import { FontSelect, useFontFamilies } from "../TextLookControls";
import { KICAD_FONT } from "../../kicad-port/textLooks";
import { useCallback, useEffect, useState, type KeyboardEvent } from "react";
import { useFpApi, useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { fpTextSizeMessage, noPrintableChars } from "../../kicad-port/fpTextTool";
import { useFpTextLayerChoices } from "./useFpLayerChoices";
import type { TextJustify } from "../../api/types";
import { fieldLayoutOf, fpFieldById } from "../../kicad-port/fpItems";


const mm = (nm: number) => String(Math.round(nm) / 1e6);
const nm = (v: string) => Math.round(Number(v) * 1e6);

/**
 * `DIALOG_TEXT_PROPERTIES`' `SetInitialFocus( m_SingleLineText )`: the text box has the focus when the dialog opens. The text tool opens the dialog from the
 * canvas' pointerdown, and the mousedown that follows would focus the canvas (it takes the keys) after `autoFocus` ran, so the focus is given once more
 * after that event (UI-LEFT-2 FF10's click-through: typing went to the canvas, not to the text).
 */
function useInitialFocus(): (el: HTMLInputElement | null) => void {
  return useCallback((el: HTMLInputElement | null) => {
    if (!el) return;
    el.focus();
    window.setTimeout(() => {
      if (el.isConnected && document.activeElement !== el && !(document.activeElement instanceof HTMLInputElement)) el.focus();
    }, 0);
  }, []);
}

export function FpTextDialog() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const text = state.textDialog;
  const [content, setContent] = useState("");
  const initialFocus = useInitialFocus();
  const [layer, setLayer] = useState("F.SilkS");
  // the editor board's enabled layers by their names, every layer when the text was on one not enabled as the dialog opened (`ShowNonActivatedLayers`; UI-LEFT-3 F3-1)
  const [openLayer, setOpenLayer] = useState("F.SilkS");
  const layerChoices = useFpTextLayerChoices(openLayer);
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
    setOpenLayer(text.layer);
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
    // `DIALOG_TEXT_PROPERTIES::TransferDataFromWindow`: the size in KiCad's limits, through UNIT_BINDER::Validate's words; a pen thicker than ClampTextPenSize
    // allows is clamped, with the message.
    const sizeNm = nm(size);
    const sizeError = fpTextSizeMessage(sizeNm);
    if (sizeError) return setError(sizeError);
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
      // the offset was edited: placed there now, "Draw Text", and the tool stays; the commit is followed by `RunAction( ACTIONS::selectItem, text )`, as the
      // second click's placement does on the canvas.
      void api.addText(done).then((id) => {
        if (id) dispatch({ type: "SET_SELECTION", refs: [id] });
      });
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
            <input ref={initialFocus} autoFocus value={content} data-testid="fp-text-content" onChange={(e) => setContent(e.target.value)} onKeyDown={enter} />
            <span>Layer:</span>
            <select value={layer} data-testid="fp-text-layer" onChange={(e) => setLayer(e.target.value)}>
              {layerChoices.map((l) => (
                <option key={l.value} value={l.value}>
                  {l.label}
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

/** `UNIT_BINDER::Validate`'s message for the size field `label` ("Width", "Height") of `sizeNm` (see `fpTextSizeMessage`, whose wording is the width's). */
function sizeMessage(label: string, sizeNm: number): string | null {
  const m = fpTextSizeMessage(sizeNm);
  return m ? m.replace(/^Width/, label) : null;
}

/**
 * `DIALOG_TEXT_PROPERTIES` on an item of the open footprint (`FOOTPRINT_EDIT_FRAME::OnEditItemRequest`: `case PCB_FIELD_T: case PCB_TEXT_T: ShowTextPropertiesDialog`), opened by a
 * double click or E: "Footprint Reference Properties" (label "Reference:"), "Footprint Value Properties" ("Value:"), "Footprint Field Properties" ("Text:") for a field, with its Show
 * box; "Footprint Text Properties" for a text, with no Show box. A field has KiCad's whole look -- width and height, thickness, Bold, Italic, the horizontal and vertical justification,
 * Mirrored, Knockout, Keep upright, the Font (`m_fontCtrl`, UI-LEFT-2 CC1) -- (`fp_item edit_field`); a text has the same, its looks kept by its id (`fp_item set_text`).
 * An empty box leaves the text as it was (`if( !m_SingleLineText->GetValue().IsEmpty() ) SetText`), a size outside KiCad's limits is refused with `UNIT_BINDER::Validate`'s
 * words, a pen too thick is clamped with KiCad's message. OK is one undo step ("Edit Text Properties").
 */
export function FpTextEditDialog() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const id = state.textEditId;
  const fp = state.footprint;
  const field = fp && id ? fpFieldById(fp, id) : undefined;
  const text = fp && id && !field ? fp.texts.find((t) => t.id === id) : undefined;
  const [content, setContent] = useState("");
  const initialFocus = useInitialFocus();
  const [layer, setLayer] = useState("F.SilkS");
  // the editor board's enabled layers by their names, every layer when the text was on one not enabled as the dialog opened (`ShowNonActivatedLayers`; UI-LEFT-3 F3-1)
  const [openLayer, setOpenLayer] = useState("F.SilkS");
  const layerChoices = useFpTextLayerChoices(openLayer);
  const [width, setWidth] = useState("1");
  const [height, setHeight] = useState("1");
  const [thickness, setThickness] = useState("0.1");
  const [x, setX] = useState("0");
  const [y, setY] = useState("0");
  const [angle, setAngle] = useState("0");
  const [halign, setHalign] = useState<-1 | 0 | 1>(0);
  const [valign, setValign] = useState<-1 | 0 | 1>(0);
  const [mirror, setMirror] = useState(false);
  const [bold, setBold] = useState(false);
  const [italic, setItalic] = useState(false);
  const [visible, setVisible] = useState(true);
  const [knockout, setKnockout] = useState(false);
  const [upright, setUpright] = useState(true);
  const [face, setFace] = useState(KICAD_FONT);
  const families = useFontFamilies();
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (field) {
      setFace(field.face || KICAD_FONT);
      setContent(field.text);
      setLayer(field.layer);
      setOpenLayer(field.layer);
      setWidth(mm(field.w));
      setHeight(mm(field.h));
      setThickness(mm(field.thickness));
      setX(mm(field.x));
      setY(mm(field.y));
      setAngle(String(field.angle / 1000));
      setHalign(field.halign);
      setValign(field.valign);
      setMirror(field.mirror);
      setBold(field.bold);
      setItalic(field.italic);
      setVisible(field.visible);
      setKnockout(field.knockout);
      setUpright(field.upright);
    } else if (text) {
      // what the text keeps beyond `CmdText` (UI-LEFT F5): its width, bold, italic, vertical justification, knockout and keep-upright
      const look = textLookOf(fp, text.id ?? "");
      setFace(textFaceOf(fp, text.id) || KICAD_FONT);
      setContent(text.content);
      setLayer(text.layer);
      setOpenLayer(text.layer);
      setWidth(mm(look.width_nm ?? text.size_nm));
      setHeight(mm(text.size_nm));
      setValign(look.valign);
      setBold(look.bold);
      setItalic(look.italic);
      setKnockout(look.knockout);
      setUpright(look.keep_upright);
      setThickness(mm(text.stroke_width));
      setX(mm(text.at.x));
      setY(mm(text.at.y));
      setAngle(String(text.angle / 1000));
      setHalign(text.justify === "left" ? -1 : text.justify === "right" ? 1 : 0);
      setMirror(text.mirror);
    }
    setError(null);
    // only when the dialog opens on an item, not on every poll of the footprint
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  if (!id || (!field && !text)) return null;
  const close = () => dispatch({ type: "SET_TEXT_EDIT_ID", id: null });
  const title = field ? (field.name === "Reference" ? "Footprint Reference Properties" : field.name === "Value" ? "Footprint Value Properties" : "Footprint Field Properties") : "Footprint Text Properties";
  const label = field?.name === "Reference" ? "Reference:" : field?.name === "Value" ? "Value:" : "Text:";
  const ok = async () => {
    const w = nm(width);
    const h = nm(height);
    const sizeError = sizeMessage("Width", w) ?? sizeMessage("Height", h);
    if (sizeError) return setError(sizeError);
    // `ClampTextPenSize( thickness, size )`: a quarter of the smaller side at most
    let pen = Math.max(0, nm(thickness) || 0);
    const maxPen = Math.round(Math.min(w, h) * 0.25);
    if (pen > maxPen) {
      dispatch({ type: "TOAST", message: "The text thickness is too large for the text size.\nIt will be clamped.", kind: "error" });
      pen = maxPen;
    }
    const at = { x: nm(x) || 0, y: nm(y) || 0 };
    const angleMdeg = (((Math.round((Number(angle) || 0) * 1000) % 360_000) + 360_000) % 360_000);
    let done: boolean;
    if (field) {
      const layout = { ...fieldLayoutOf(field), at, angle: angleMdeg, size: [w, h] as [number, number], thickness: pen, layer, visible, halign, valign, mirror, bold, italic, keep_upright: upright, knockout, face: face === KICAD_FONT ? null : face };
      done = await api.editField(field.name, content, layout);
    } else {
      const justify: TextJustify = halign < 0 ? "left" : halign > 0 ? "right" : "center";
      // the height is the text's size (`CmdText.size_nm`); the width, the vertical justification, bold, italic, knockout and keep-upright are kept by its id
      done = await api.setText(id, { ...text!, content, layer, size_nm: h, stroke_width: pen, angle: angleMdeg, justify, mirror, at }, { bold, italic, valign, width_nm: w === h ? null : w, knockout, keep_upright: upright, face: face === KICAD_FONT ? "" : face });
    }
    if (done) close();
  };
  const enter = (e: KeyboardEvent) => {
    if (e.key === "Enter") void ok();
  };
  const num = (value: string, set: (v: string) => void, testid: string, unit = "mm") => (
    <span>
      <input type="number" step="any" value={value} data-testid={testid} onChange={(e) => set(e.target.value)} onKeyDown={enter} style={{ width: 80 }} /> {unit}
    </span>
  );
  const check = (lbl: string, value: boolean, set: (v: boolean) => void, testid: string) => (
    <label style={{ display: "inline-flex", gap: 4, alignItems: "center", marginRight: 10 }}>
      <input type="checkbox" checked={value} data-testid={testid} onChange={(e) => set(e.target.checked)} />
      {lbl}
    </label>
  );
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 420 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label={title} data-testid="fp-text-edit-dialog">
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "100px 1fr" }}>
            <span>{label}</span>
            <input ref={initialFocus} autoFocus value={content} data-testid="fp-text-edit-content" onChange={(e) => setContent(e.target.value)} onKeyDown={enter} />
            <span>Layer:</span>
            <select value={layer} data-testid="fp-text-edit-layer" onChange={(e) => setLayer(e.target.value)}>
              {layerChoices.map((l) => (
                <option key={l.value} value={l.value}>
                  {l.label}
                </option>
              ))}
            </select>
            <span>Width:</span>
            {num(width, setWidth, "fp-text-edit-width")}
            <span>Height:</span>
            {num(height, setHeight, "fp-text-edit-height")}
            <span>Thickness:</span>
            {num(thickness, setThickness, "fp-text-edit-thickness")}
            <span>Offset X:</span>
            {num(x, setX, "fp-text-edit-x")}
            <span>Offset Y:</span>
            {num(y, setY, "fp-text-edit-y")}
            <span>Orientation:</span>
            {num(angle, setAngle, "fp-text-edit-angle", "°")}
            <span>Justification:</span>
            <span>
              <select value={halign} data-testid="fp-text-edit-halign" onChange={(e) => setHalign(Number(e.target.value) as -1 | 0 | 1)}>
                <option value={-1}>Left</option>
                <option value={0}>Center</option>
                <option value={1}>Right</option>
              </select>
              <select value={valign} data-testid="fp-text-edit-valign" onChange={(e) => setValign(Number(e.target.value) as -1 | 0 | 1)} style={{ marginLeft: 6 }}>
                <option value={-1}>Top</option>
                <option value={0}>Center</option>
                <option value={1}>Bottom</option>
              </select>
            </span>
            <span>Font:</span>
            <FontSelect families={families} current={face === KICAD_FONT ? null : face} value={face} onChange={setFace} withDefault={false} />
          </div>
          <div style={{ marginTop: 8 }}>
            {/* `DIALOG_TEXT_PROPERTIES` for a footprint text hides only Show (`m_Visible->Show( false )`) and Locked (UI-LEFT F5) */}
            {field && check("Show", visible, setVisible, "fp-text-edit-visible")}
            {check("Bold", bold, setBold, "fp-text-edit-bold")}
            {check("Italic", italic, setItalic, "fp-text-edit-italic")}
            {check("Mirrored", mirror, setMirror, "fp-text-edit-mirror")}
            {check("Knockout", knockout, setKnockout, "fp-text-edit-knockout")}
            {check("Keep upright", upright, setUpright, "fp-text-edit-upright")}
          </div>
          {error && <p style={{ color: "var(--error, #e66)", margin: "8px 0 0" }}>{error}</p>}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" data-testid="fp-text-edit-ok" onClick={() => void ok()}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
