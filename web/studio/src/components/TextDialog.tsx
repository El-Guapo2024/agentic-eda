// pcbnew.InteractiveDrawing.text ("Draw Text", Ctrl+Shift+T) for a fresh
// text, and pcbnew.InteractiveEdit.properties ("E") on a selected text
// for editing an existing one -- state.textDialog's "add"/"edit" modes
// share this one form since every field but the position is the same
// (position comes from where you clicked for "add"; edit_text has no
// position field at all, see api/types.ts's Cmd -- move_text is
// separate, matching item 7's generic move).
import { useEffect, useRef, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import type { TextJustify } from "../api/types";
import { newTextStyle } from "../kicad-port/layerDefaults";
import { activeLayerName } from "../kicad-port/layerPresets";

// A new text starts as `DRAWING_TOOL::PlaceText` makes it: the size and thickness Board Setup > Text & Graphics > Defaults gives the layer's class, mirrored on a
// back layer, left aligned (kicad-port/layerDefaults.ts `newTextStyle`).

export function TextDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const dialog = state.textDialog;

  const editing = dialog?.mode === "edit" ? api.textById(dialog.id) : undefined;
  const [content, setContent] = useState("");
  const defaults = state.board?.board_rules?.text_graphics;
  const [layer, setLayer] = useState("F.SilkS");
  const [size, setSize] = useState(() => newTextStyle(defaults, "F.SilkS").sizeUm);
  const [strokeWidth, setStrokeWidth] = useState(() => newTextStyle(defaults, "F.SilkS").strokeWidthUm);
  const [justify, setJustify] = useState<TextJustify>("left");
  const [mirror, setMirror] = useState(false);
  const [angle, setAngle] = useState(0);
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
      setAngle(editing.angle);
    } else if (dialog.mode === "add") {
      const start = activeLayerName(state.activeLayer, "F.SilkS");
      const style = newTextStyle(defaults, start);
      styled.current = false;
      setContent("");
      setLayer(start);
      setSize(style.sizeUm);
      setStrokeWidth(style.strokeWidthUm);
      setJustify(style.justify);
      setMirror(style.mirror);
      setAngle(0);
    }
    // Only when the dialog itself changes identity (opens/switches item) -- not on every keystroke into the fields it seeds.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dialog?.mode, dialog?.mode === "edit" ? dialog.id : null, !!editing]);

  if (!dialog) return null;
  if (dialog.mode === "edit" && !editing) return null;
  const close = () => dispatch({ type: "SET_TEXT_DIALOG", dialog: null });

  const submit = () => {
    if (!content.trim()) return;
    if (dialog.mode === "add") {
      api.cmd({ op: "add_text", text: { content, at: { x: dialog.at[0], y: dialog.at[1] }, angle, layer, size_um: size, stroke_width: strokeWidth, justify, mirror } });
    } else {
      api.cmd({ op: "edit_text", id: dialog.id, content, angle, layer, size_um: size, stroke_width: strokeWidth, justify, mirror });
    }
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 380 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Text Properties</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "80px 1fr" }}>
            <span>Text</span>
            <input autoFocus value={content} onChange={(e) => setContent(e.target.value)} />
            <span>Layer</span>
            <select
              value={layer}
              onChange={(e) => {
                setLayer(e.target.value);
                if (dialog.mode === "add" && !styled.current) {
                  const style = newTextStyle(defaults, e.target.value);
                  setSize(style.sizeUm);
                  setStrokeWidth(style.strokeWidthUm);
                  setMirror(style.mirror);
                }
              }}
            >
              {[...new Set([...(state.board?.layers ?? []), "F.SilkS", "B.SilkS", "F.Fab", "B.Fab"])].map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
            <span>Size</span>
            <input type="number" min={100} step={50} value={size} onChange={(e) => ((styled.current = true), setSize(Number(e.target.value)))} />
            <span>Thickness</span>
            <input type="number" min={50} step={10} value={strokeWidth} onChange={(e) => ((styled.current = true), setStrokeWidth(Number(e.target.value)))} />
            <span>Orientation</span>
            <input type="number" step={90} value={angle / 1000} onChange={(e) => setAngle(Number(e.target.value) * 1000)} />
            <span>Justify</span>
            <select value={justify} onChange={(e) => setJustify(e.target.value as TextJustify)}>
              <option value="left">Left</option>
              <option value="center">Center</option>
              <option value="right">Right</option>
            </select>
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
