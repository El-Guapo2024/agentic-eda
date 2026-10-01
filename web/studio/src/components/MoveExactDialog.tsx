// Port of pcbnew/dialogs/dialog_move_exact.cpp (Shift+M, "Move Exactly...").
//
// Cartesian (Move X/Move Y) or polar (Distance/Angle) offset, an
// independent rotation angle, and an anchor for that rotation --
// "item anchor" (default for a single-item selection: each part spins
// about its own position, EDIT_TOOL::MoveExact's
// `boardItem->Rotate(boardItem->GetPosition(), angle)`, evaluated per
// item so a multi-selection with this anchor still spins each one about
// its own anchor, not a shared point -- encoded here as the backend's
// `pivot: null`), "selection center" (default for 2+ items: one shared
// point, the bbox center *after* the translation -- source: `selCenter
// = rp + translation`), or "local coordinates origin" (the status bar's
// Space-settable point, state.localOriginUm, untouched by translation).
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

type Anchor = "item" | "center" | "origin";

function selectionBoundsCenter(parts: ReadonlyArray<{ ref: string; placed: boolean; courtyard?: readonly [number, number, number, number] | null }>, refs: readonly string[]): { x: number; y: number } | null {
  let x0 = Infinity,
    y0 = Infinity,
    x1 = -Infinity,
    y1 = -Infinity;
  let any = false;
  for (const ref of refs) {
    const p = parts.find((q) => q.ref === ref);
    if (!p?.placed || !p.courtyard) continue;
    any = true;
    x0 = Math.min(x0, p.courtyard[0]);
    y0 = Math.min(y0, p.courtyard[1]);
    x1 = Math.max(x1, p.courtyard[2]);
    y1 = Math.max(y1, p.courtyard[3]);
  }
  return any ? { x: (x0 + x1) / 2, y: (y0 + y1) / 2 } : null;
}

export function MoveExactDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.moveExactDialogOpen;

  const refs = [...state.selection];
  const placedParts = state.board ? refs.filter((r) => api.partByRef(r)?.placed) : [];

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
    setAnchor(placedParts.length > 1 ? "center" : "item");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open || placedParts.length === 0) return null;

  const close = () => dispatch({ type: "SET_MOVE_EXACT_DIALOG_OPEN", open: false });

  const dxUm = polar ? umFrom(distance, state.units) * Math.cos((angleDeg * Math.PI) / 180) : umFrom(moveX, state.units);
  // Polar input uses the same CCW-positive/Y-down bridge as toPolar's own inverse.
  const dyUm = polar ? -umFrom(distance, state.units) * Math.sin((angleDeg * Math.PI) / 180) : umFrom(moveY, state.units);

  const center = selectionBoundsCenter(state.board?.parts ?? [], placedParts);
  const pivot = anchor === "item" ? null : anchor === "center" ? (center ? { x: center.x + dxUm, y: center.y + dyUm } : null) : state.localOriginUm;

  const submit = async () => {
    // Negate for the backend's clockwise-positive convention -- see this file's header comment.
    const ok = await api.moveExact(placedParts, Math.round(dxUm), Math.round(dyUm), Math.round(-rotate * 1000), pivot);
    if (ok) close();
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
              <option value="item">{placedParts.length > 1 ? "Each item's own anchor" : "Item anchor"}</option>
              <option value="center">Selection center</option>
              <option value="origin">Local coordinates origin</option>
            </select>
          </div>
          <div style={{ fontSize: 11, opacity: 0.7, marginTop: 6 }}>
            {placedParts.length} part{placedParts.length === 1 ? "" : "s"} · units: {state.units}
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
