// `DIALOG_TEXT_PROPERTIES` (pcbnew/dialogs/dialog_text_properties.cpp at KiCad 8303b2ad) on a footprint's own text on the board (UI-LEFT P15: `PCB_SELECTION_TOOL`
// picks a footprint's `PCB_TEXT`, and `PCB_EDIT_FRAME::OnEditItemRequest` opens its Text Properties): the text as stored (variables unresolved, as `GetText()`),
// the layer, the size and thickness, italic, bold, the position and orientation in the footprint's frame (as the Footprint Properties grid's offsets), keep
// upright, mirrored, knockout, the justification and the Font (`m_fontCtrl`: "KiCad Font" and the server's families, the layout's face; UI-LEFT-2 CC1). OK sends one
// `edit_board_footprint_text` (the instance's own copy of its footprint); "The text can't be empty." keeps the dialog open.
import { useEffect, useState } from "react";
import type { FieldLayoutCmd } from "../api/types";
import { fpTextById } from "../kicad-port/pcbItems";
import { boardLayerList, layerDisplayName } from "../kicad-port/boardLayers";
import { closeFpTextDialog, useFpTextDialog } from "../state/fpTextDialog";
import { useStudioApi, useStudioState } from "../state/store";
import { nmFrom, nmTo } from "../state/units";
import { NumberInput } from "./NumberInput";
import { FontSelect, useFontFamilies } from "./TextLookControls";
import { KICAD_FONT } from "../kicad-port/textLooks";
import { useOriginTransforms } from "../state/originTransforms";
import { fromDisplay, toDisplay, type CoordType } from "../kicad-port/originTransforms";

export function FootprintTextDialog() {
  const id = useFpTextDialog();
  const state = useStudioState();
  const api = useStudioApi();
  const units = state.units;
  const hit = id && state.board ? fpTextById(state.board, id) : null;
  const [text, setText] = useState("");
  const [l, setL] = useState<FieldLayoutCmd | null>(null);
  const [error, setError] = useState<string | null>(null);
  const families = useFontFamilies();
  // A footprint's text: `m_posX` / `m_posY` are `REL_X_COORD` / `REL_Y_COORD` (`m_item->GetParentFootprint()`), so Origins & Axes' axis directions turn the
  // offset and its origin does not (UI-LEFT-2 PP9).
  const xf = useOriginTransforms();
  useEffect(() => {
    if (!hit) return;
    const t = hit.text;
    setText(t.raw ?? t.text);
    setL({ at: { x: t.lx ?? 0, y: t.ly ?? 0 }, angle: t.langle ?? 0, size: [t.w, t.h], thickness: t.thickness, layer: t.layer, visible: true, halign: t.halign, valign: t.valign, mirror: t.mirror, bold: t.bold, italic: t.italic, keep_upright: t.upright, knockout: t.knockout, face: t.face ?? null });
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  if (!id || !hit || !l) return null;
  const layers = boardLayerList(state.board?.layers ?? [], state.board?.board_rules?.layer_setup);
  const len = (label: string, v: number, set: (nm: number) => void, type: CoordType = "not") => (
    <>
      <span>{label}</span>
      <span>
        <NumberInput label={label} value={v} scale={(x) => nmTo(toDisplay(xf, x, type), units)} unscale={(x) => Math.round(fromDisplay(xf, nmFrom(x, units), type))} onChange={(x) => x != null && set(x)} /> {units}
      </span>
    </>
  );
  const check = (label: string, key: "italic" | "bold" | "keep_upright" | "mirror" | "knockout") => (
    <label style={{ display: "block" }}>
      <input type="checkbox" aria-label={label} checked={!!l[key]} onChange={(e) => setL({ ...l, [key]: e.target.checked })} /> {label}
    </label>
  );
  const ok = async () => {
    if (!text.trim()) return setError("The text can't be empty.");
    if (await api.cmdBatch([{ op: "edit_board_footprint_text", id, text, layout: l }])) closeFpTextDialog();
  };
  return (
    <div className="dialog-backdrop" onClick={closeFpTextDialog}>
      <div className="dialog" style={{ width: 440 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Text Properties">
        <div className="dialog-header">
          <span>Text Properties</span>
          <span>{hit.part.ref}</span>
        </div>
        <div className="dialog-body">
          <textarea aria-label="Text" value={text} onChange={(e) => setText(e.target.value)} rows={2} style={{ width: "100%" }} />
          <div className="kv-grid">
            <span>Layer:</span>
            <select aria-label="Layer" value={l.layer} onChange={(e) => setL({ ...l, layer: e.target.value })}>
              {(layers.includes(l.layer) ? layers : [...layers, l.layer]).map((x) => (
                <option key={x} value={x}>
                  {layerDisplayName(state.board?.board_rules?.layer_setup, x)}
                </option>
              ))}
            </select>
            {len("Width:", l.size[0], (w) => setL({ ...l, size: [w, l.size[1]] }))}
            {len("Height:", l.size[1], (h) => setL({ ...l, size: [l.size[0], h] }))}
            {len("Thickness:", l.thickness ?? 0, (t) => setL({ ...l, thickness: t }))}
            {len("Position X:", l.at.x, (x) => setL({ ...l, at: { ...l.at, x } }), "rel_x")}
            {len("Position Y:", l.at.y, (y) => setL({ ...l, at: { ...l.at, y } }), "rel_y")}
            <span>Orientation:</span>
            <span>
              <NumberInput label="Orientation" value={(l.angle ?? 0) / 1000} onChange={(v) => v != null && setL({ ...l, angle: Math.round(v * 1000) })} /> deg
            </span>
            <span>Font:</span>
            <FontSelect families={families} current={l.face} value={l.face || KICAD_FONT} onChange={(v) => setL({ ...l, face: v === KICAD_FONT ? null : v })} withDefault={false} />
            <span>Justification:</span>
            <select aria-label="Justification" value={l.halign ?? 0} onChange={(e) => setL({ ...l, halign: Number(e.target.value) as -1 | 0 | 1 })}>
              <option value={-1}>Left</option>
              <option value={0}>Center</option>
              <option value={1}>Right</option>
            </select>
          </div>
          {check("Italic", "italic")}
          {check("Bold", "bold")}
          {check("Keep upright", "keep_upright")}
          {check("Mirrored", "mirror")}
          {check("Knockout", "knockout")}
          {error && (
            <div role="alert" style={{ color: "var(--chrome-error, #c0392b)", fontSize: 12, marginTop: 6 }}>
              {error}
            </div>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={closeFpTextDialog}>Cancel</button>
          <button className="primary" onClick={() => void ok()}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
