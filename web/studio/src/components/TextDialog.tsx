// pcbnew.InteractiveDrawing.text ("Draw Text", Ctrl+Shift+T) for a fresh
// text, and pcbnew.InteractiveEdit.properties ("E") on a selected text
// for editing an existing one -- state.textDialog's "add"/"edit" modes
// share this one form since every field but the position is the same
// (position comes from where you clicked for "add"; edit_text has no
// position field at all, see api/types.ts's Cmd -- move_text is
// separate, matching item 7's generic move).
import { useEffect, useRef, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import type { Cmd, TextJustify } from "../api/types";
import { boldTogglePenNm, inferBold, newTextStyle } from "../kicad-port/layerDefaults";
import { FontSelect, useFontFamilies } from "./TextLookControls";
import { KICAD_FONT } from "../kicad-port/textLooks";
import { activeLayerName } from "../kicad-port/layerPresets";
import { boardLayerList, layerDisplayName } from "../kicad-port/boardLayers";

// A new text starts as `DRAWING_TOOL::PlaceText` makes it: the size and thickness Board Setup > Text & Graphics > Defaults gives the layer's class, bold as
// `InferBold` says of them, mirrored on a back layer, left aligned (kicad-port/layerDefaults.ts `newTextStyle`).
//
// Bold is `DIALOG_TEXT_PROPERTIES`' Bold button (pcbnew/dialogs/dialog_text_properties.cpp at 8303b2ad; round 7 R7-3, P4): it starts as the text is
// (`m_bold->Check( m_item->IsBold() )`, :347), checking it sets the thickness to the bold pen and unchecking it to the normal one (`onBoldToggle`, :396),
// a thickness typed in sets it to what `InferBold` calls that pen (`onThickness`, :430), and OK writes it (`SetBoldFlag`, :570).
// The Font choice (`m_fontCtrl`: "KiCad Font" and the server's families, `SetFontSelection( m_item->GetFont() )`, :322; OK's `SetFont( GetFontSelection( bold, italic ) )`,
// :535) is the text's face: a new text's in its extra, an edit's through `edit_text_style` in the same undo step (UI-LEFT-2 CC1). A face the list lacks shows
// "<not found>" and is kept (`HaveFontSelection`).

export function TextDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const dialog = state.textDialog;

  const editing = dialog?.mode === "edit" ? api.textById(dialog.id) : undefined;
  // E on the text Draw Text carries (`PlaceText`'s `OnEditItemRequest( text )`): the dialog edits the text in the hand, which stays there.
  const carried = dialog?.mode === "carried" && state.drawState?.kind === "text" ? state.drawState : null;
  const [content, setContent] = useState("");
  const defaults = state.board?.board_rules?.text_graphics;
  const [layer, setLayer] = useState("F.SilkS");
  const [size, setSize] = useState(() => newTextStyle(defaults, "F.SilkS").sizeNm);
  const [strokeWidth, setStrokeWidth] = useState(() => newTextStyle(defaults, "F.SilkS").strokeWidthNm);
  const [justify, setJustify] = useState<TextJustify>("left");
  const [mirror, setMirror] = useState(false);
  const [bold, setBold] = useState(false);
  const [angle, setAngle] = useState(0);
  const [face, setFace] = useState(KICAD_FONT);
  const families = useFontFamilies();
  // Once the size, the thickness or the mirror was edited by hand, a change of layer leaves them alone.
  const styled = useRef(false);

  // Re-seed the form fields whenever the dialog (re)opens -- both for a
  // fresh "add" (reset to defaults) and for "edit" (load the real text's
  // current values, once `editing` has resolved from the board).
  useEffect(() => {
    if (!dialog) return;
    if (dialog.mode === "edit" && editing) {
      setContent(editing.content);
      setLayer(editing.layer);
      setSize(editing.size);
      setStrokeWidth(editing.stroke_width);
      setJustify(editing.justify);
      setMirror(editing.mirror);
      setBold(editing.bold ?? false);
      setAngle(editing.angle);
      setFace(editing.face || KICAD_FONT);
    } else if (dialog.mode === "carried" && carried) {
      const t = carried.text;
      styled.current = true;
      setContent(t.content);
      setLayer(t.layer);
      setSize(t.size_nm);
      setStrokeWidth(t.stroke_width);
      setJustify(t.justify);
      setMirror(t.mirror);
      setBold(!!(carried.extra as { bold?: boolean }).bold);
      setAngle(t.angle);
      setFace((carried.extra as { face?: string }).face || KICAD_FONT);
    } else if (dialog.mode === "add") {
      const start = activeLayerName(state.activeLayer, "F.SilkS");
      const style = newTextStyle(defaults, start);
      styled.current = false;
      setContent("");
      setLayer(start);
      setSize(style.sizeNm);
      setStrokeWidth(style.strokeWidthNm);
      setJustify(style.justify);
      setMirror(style.mirror);
      setBold(style.bold);
      setAngle(0);
      setFace(KICAD_FONT);
    }
    // Only when the dialog itself changes identity (opens/switches item) -- not on every keystroke into the fields it seeds.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dialog?.mode, dialog?.mode === "edit" ? dialog.id : null, !!editing]);

  if (!dialog) return null;
  if (dialog.mode === "edit" && !editing) return null;
  if (dialog.mode === "carried" && !carried) return null;
  const close = () => dispatch({ type: "SET_TEXT_DIALOG", dialog: null });

  const submit = () => {
    if (!content.trim()) return;
    // the face the text keeps in its extra (none: the stroke font)
    const faceExtra = face === KICAD_FONT ? {} : { face };
    if (dialog.mode === "add") {
      // `PlaceText`: the text now follows the cursor until the next click places it ("Draw Text"; components/canvas/Canvas.tsx), with the class's glyph width and
      // italic and the bottom justification `textAttrs` gave it (round 5 B5-2), and the bold `InferBold` gave it (round 6 A6-3). (The dialog has no position field, so it never "modified the location".)
      const style = newTextStyle(defaults, layer);
      // A size typed in the dialog is square (the dialog has one size field); the class's own size keeps the class's width.
      const extra = { id: "", ...(size === style.sizeNm && style.widthNm !== size ? { width_nm: style.widthNm } : {}), ...(bold ? { bold: true } : {}), ...(style.italic ? { italic: true } : {}), valign: style.valign, ...faceExtra };
      dispatch({ type: "SET_DRAW_STATE", draw: { kind: "text", text: { content, at: { x: dialog.at[0], y: dialog.at[1] }, angle, layer, size_nm: size, stroke_width: strokeWidth, justify, mirror }, extra } });
    } else if (dialog.mode === "carried") {
      if (!carried) return close();
      // The text in the hand takes the dialog's values and stays carried; a size typed in is square (the class's own width goes with the size it was for).
      const { bold: _bold, face: _face, width_nm, ...rest } = carried.extra as typeof carried.extra & { bold?: boolean; face?: string };
      const extra = { ...rest, ...(width_nm !== undefined && size === carried.text.size_nm ? { width_nm } : {}), ...(bold ? { bold: true } : {}), ...faceExtra };
      dispatch({ type: "SET_DRAW_STATE", draw: { kind: "text", text: { ...carried.text, content, angle, layer, size_nm: size, stroke_width: strokeWidth, justify, mirror }, extra } });
    } else {
      const edit: Cmd = { op: "edit_text", id: dialog.id, content, angle, layer, size_nm: size, stroke_width: strokeWidth, justify, mirror, bold };
      const faceNow = editing?.face || KICAD_FONT;
      const restyle: Cmd[] = face !== faceNow ? [{ op: "edit_text_style", id: dialog.id, face: face === KICAD_FONT ? "" : face }] : [];
      api.cmd(restyle.length ? { op: "batch", cmds: [edit, ...restyle] } : edit);
    }
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 380 }} onClick={(e) => e.stopPropagation()} data-testid="text-dialog">
        <div className="dialog-header">
          <span>Text Properties</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "80px 1fr" }}>
            <span>Text</span>
            <input autoFocus value={content} onChange={(e) => setContent(e.target.value)} data-testid="text-content" />
            <span>Layer</span>
            <select
              value={layer}
              onChange={(e) => {
                setLayer(e.target.value);
                if (dialog.mode === "add" && !styled.current) {
                  const style = newTextStyle(defaults, e.target.value);
                  setSize(style.sizeNm);
                  setStrokeWidth(style.strokeWidthNm);
                  setBold(style.bold);
                  setMirror(style.mirror);
                }
              }}
            >
              {/* The layers the board has (Board Setup > Board Editor Layers), by the names the user gave them. */}
              {boardLayerList(state.board?.layers ?? [], state.board?.board_rules?.layer_setup).map((l) => (
                <option key={l} value={l}>
                  {layerDisplayName(state.board?.board_rules?.layer_setup, l)}
                </option>
              ))}
            </select>
            <span>Size</span>
            <input type="number" min={100} step={50} value={size} onChange={(e) => ((styled.current = true), setSize(Number(e.target.value)))} />
            <span>Thickness</span>
            <input
              type="number"
              min={50}
              step={10}
              value={strokeWidth}
              data-testid="text-thickness"
              onChange={(e) => {
                // `onThickness`: the Bold button follows the pen.
                const pen = Number(e.target.value);
                styled.current = true;
                setStrokeWidth(pen);
                setBold(inferBold(pen, size, size));
              }}
            />
            <span>Bold</span>
            <input
              type="checkbox"
              checked={bold}
              data-testid="text-bold"
              style={{ justifySelf: "start" }}
              onChange={(e) => {
                // `onBoldToggle`: the bold or the normal pen of the size.
                styled.current = true;
                setBold(e.target.checked);
                setStrokeWidth(boldTogglePenNm(e.target.checked, size, size));
              }}
            />
            <span>Orientation</span>
            <input type="number" step={90} value={angle / 1000} onChange={(e) => setAngle(Number(e.target.value) * 1000)} />
            <span>Justify</span>
            <select value={justify} onChange={(e) => setJustify(e.target.value as TextJustify)}>
              <option value="left">Left</option>
              <option value="center">Center</option>
              <option value="right">Right</option>
            </select>
            <span>Font</span>
            <FontSelect families={families} current={face === KICAD_FONT ? null : face} value={face} onChange={setFace} withDefault={false} />
            <span>Mirrored</span>
            <input type="checkbox" checked={mirror} onChange={(e) => ((styled.current = true), setMirror(e.target.checked))} style={{ justifySelf: "start" }} />
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!content.trim()} onClick={submit}>
            {dialog.mode === "add" ? "Add" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
