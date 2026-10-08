// Port of pcbnew/dialogs/dialog_move_exact.cpp (Shift+M, "Move Exactly...").
//
// Cartesian (Move X/Move Y) or polar (Distance/Angle) offset, an
// independent rotation angle, and an anchor for that rotation --
// "item anchor" (default for a single-item selection: each part spins
// about its own position, EDIT_TOOL::MoveExact's
// `boardItem->Rotate(boardItem->GetPosition(), angle)`, evaluated per
// item so a multi-selection with this anchor still spins each one about
// its own anchor, not a shared point -- one rotate_items per item,
// kicad-port/pcbTransform.ts `planMoveExact`), "selection center" (default
// for 2+ items: one shared point, the bbox center *after* the translation
// -- source: `selCenter = rp + translation`), or "local coordinates origin"
// (the status bar's Space-settable point, state.localOriginUm, untouched by
// translation). Any kind of item, locked ones left alone, the whole thing
// one undo step.
// KiCad's fourth choice, "drill/place origin" (a board-wide aux origin
// setting), has no equivalent in this app's model and is left out.
//
// Rotation sign: source negates the dialog's own angle before calling
// Rotate() whenever the display's Y axis is not inverted (the default) --
// dialog_move_exact.cpp's `if (!m_Display.m_DisplayInvertYAxis) rotation
// = -rotation;`. That's the same bridge this app's own status bar
// (state/units.ts's `toPolar`) already builds for its polar-coordinate
// display: positive = counter-clockwise on screen, the ordinary CAD
// convention, even though the raw rotation matrix underneath
// (crates/model/src/footprint.rs `to_board`, and this Cmd's own
// `rotate_point_about`) is clockwise-positive in this app's Y-down board
// coordinates. This dialog negates the same way, so typing +90 here
// turns a part the same visual direction +90 would in the status bar's
// polar readout.
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { umFrom } from "../state/units";
import { editableSelection, planMoveExact } from "../kicad-port/pcbTransform";

type Anchor = "item" | "center" | "origin";

export function MoveExactDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.moveExactDialogOpen;

  // `RequestSelection` with the free-pad and locked-item filters: any kind of item, a pad standing for its footprint, locked ones staying where they are.
  const items = state.board ? editableSelection(state.board, [...state.selection]).ids : [];

  const [polar, setPolar] = useState(false);
  const [moveX, setMoveX] = useState(0); // display units
  const [moveY, setMoveY] = useState(0);
  const [distance, setDistance] = useState(0);
  const [angleDeg, setAngleDeg] = useState(0); // move direction (polar mode), CCW-positive, 0 = east
  const [rotate, setRotate] = useState(0); // rotation, CCW-positive degrees
  const [anchor, setAnchor] = useState<Anchor>("item");

  useEffect(() => {
    if (!open) return;
    setPolar(false);
    setMoveX(0);
    setMoveY(0);
    setDistance(0);
    setAngleDeg(0);
    setRotate(0);
    // EDIT_TOOL::MoveExact: `selection.Size() > 1 ? ROTATE_AROUND_SEL_CENTER : ROTATE_AROUND_ITEM_ANCHOR`.
    setAnchor(items.length > 1 ? "center" : "item");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open || items.length === 0) return null;

  const close = () => dispatch({ type: "SET_MOVE_EXACT_DIALOG_OPEN", open: false });

  const dxUm = polar ? umFrom(distance, state.units) * Math.cos((angleDeg * Math.PI) / 180) : umFrom(moveX, state.units);
  // Polar input uses the same CCW-positive/Y-down bridge as toPolar's own inverse.
  const dyUm = polar ? -umFrom(distance, state.units) * Math.sin((angleDeg * Math.PI) / 180) : umFrom(moveY, state.units);

  const submit = async () => {
    // One move_items and the turn(s) as one batch (kicad-port/pcbTransform.ts `planMoveExact`); the dialog's angle is counter-clockwise, the backend's clockwise.
    if (!state.board) return;
    const plan = planMoveExact(state.board, items, dxUm, dyUm, rotate, anchor, [state.localOriginUm.x, state.localOriginUm.y]);
    if (plan.cmds.length === 0 || (await api.cmdBatch(plan.cmds))) close();
  };

  const clear = (setter: (v: number) => void) => setter(0);

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 360 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Move Exactly...</span>
        </div>
        <div className="dialog-body">
          <label className="filter-row" style={{ marginBottom: 8 }}>
            <input type="checkbox" checked={polar} onChange={(e) => setPolar(e.target.checked)} />
            Polar coordinates
          </label>
          <div className="kv-grid" style={{ gridTemplateColumns: "110px 1fr auto" }}>
            {polar ? (
              <>
                <span>Distance:</span>
                <input autoFocus type="number" value={distance} onChange={(e) => setDistance(Number(e.target.value))} />
                <button onClick={() => clear(setDistance)}>0</button>
                <span>Angle:</span>
                <input type="number" step={1} value={angleDeg} onChange={(e) => setAngleDeg(Number(e.target.value))} />
                <button onClick={() => clear(setAngleDeg)}>0</button>
              </>
            ) : (
              <>
                <span>Move X:</span>
                <input autoFocus type="number" value={moveX} onChange={(e) => setMoveX(Number(e.target.value))} />
                <button onClick={() => clear(setMoveX)}>0</button>
                <span>Move Y:</span>
                <input type="number" value={moveY} onChange={(e) => setMoveY(Number(e.target.value))} />
                <button onClick={() => clear(setMoveY)}>0</button>
              </>
            )}
            <span>Rotate:</span>
            <input type="number" step={90} value={rotate} onChange={(e) => setRotate(Number(e.target.value))} />
            <button onClick={() => clear(setRotate)}>0</button>
          </div>
          <div className="kv-grid" style={{ gridTemplateColumns: "110px 1fr", marginTop: 8 }}>
            <span>Rotate around:</span>
            <select value={anchor} onChange={(e) => setAnchor(e.target.value as Anchor)}>
              <option value="item">{items.length > 1 ? "Each item's own anchor" : "Item anchor"}</option>
              <option value="center">Selection center</option>
              <option value="origin">Local coordinates origin</option>
            </select>
          </div>
          <div style={{ fontSize: 11, opacity: 0.7, marginTop: 6 }}>
            {items.length} item{items.length === 1 ? "" : "s"} · units: {state.units}
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={submit}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
