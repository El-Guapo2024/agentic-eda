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
// (the status bar's Space-settable point, state.localOriginNm, untouched by
// translation), or "drill/place origin" (the board's aux origin, `board.aux_origin`).
// Any kind of item, locked ones left alone, the whole thing one undo step.
// The menu is KiCad's (`m_menuIDs = { aAnchor, ROTATE_AROUND_USER_ORIGIN, ROTATE_AROUND_AUX_ORIGIN }`, `buildRotationAnchorMenu`): the
// selection's own anchor first -- "Rotate around item anchor" for one item, "Rotate around selection center" for several -- then "Rotate
// around local coordinates origin" and "Rotate around drill/place origin". The dialog keeps what was entered the last time it was opened
// for that kind of selection (DIALOG_SHIM's saved state, keyed by `m_hash_key`: the title, "|with_selection" for several items).
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
import { nmFrom } from "../state/units";
import { editableSelection, planMoveExact } from "../kicad-port/pcbTransform";

type Anchor = "item" | "center" | "origin" | "aux";

/** What the dialog had when it was last closed, per `m_hash_key` (one item / several). */
interface Saved {
  polar: boolean;
  moveX: number;
  moveY: number;
  distance: number;
  angleDeg: number;
  rotate: number;
  /** The position in the menu (`m_anchorOptions->GetSelection()`). */
  anchorIndex: number;
}
const saved: Record<"single" | "with_selection", Saved | undefined> = { single: undefined, with_selection: undefined };

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

  const key: "single" | "with_selection" = items.length > 1 ? "with_selection" : "single";
  // EDIT_TOOL::MoveExact: `selection.Size() > 1 ? ROTATE_AROUND_SEL_CENTER : ROTATE_AROUND_ITEM_ANCHOR`, then the two origins.
  const menu: Anchor[] = [items.length > 1 ? "center" : "item", "origin", "aux"];
  const label: Record<Anchor, string> = {
    item: "Rotate around item anchor",
    center: "Rotate around selection center",
    origin: "Rotate around local coordinates origin",
    aux: "Rotate around drill/place origin",
  };

  useEffect(() => {
    if (!open) return;
    // `TransferDataToWindow`: the values of last time ("Force the evaluation when setting previous values"), else zeros on the first anchor.
    const s = saved[key];
    setPolar(s?.polar ?? false);
    setMoveX(s?.moveX ?? 0);
    setMoveY(s?.moveY ?? 0);
    setDistance(s?.distance ?? 0);
    setAngleDeg(s?.angleDeg ?? 0);
    setRotate(s?.rotate ?? 0);
    setAnchor(menu[s?.anchorIndex ?? 0] ?? menu[0]!);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open || items.length === 0) return null;

  const close = () => dispatch({ type: "SET_MOVE_EXACT_DIALOG_OPEN", open: false });

  const dxNm = polar ? nmFrom(distance, state.units) * Math.cos((angleDeg * Math.PI) / 180) : nmFrom(moveX, state.units);
  // Polar input uses the same CCW-positive/Y-down bridge as toPolar's own inverse.
  const dyNm = polar ? -nmFrom(distance, state.units) * Math.sin((angleDeg * Math.PI) / 180) : nmFrom(moveY, state.units);

  const submit = async () => {
    // One move_items and the turn(s) as one batch (kicad-port/pcbTransform.ts `planMoveExact`); the dialog's angle is counter-clockwise, the backend's clockwise.
    if (!state.board) return;
    saved[key] = { polar, moveX, moveY, distance, angleDeg, rotate, anchorIndex: Math.max(0, menu.indexOf(anchor)) };
    // ROTATE_AROUND_AUX_ORIGIN: the board's drill/place origin, a fixed point as the local origin is.
    const aux = state.board.aux_origin ?? [0, 0];
    const plan = anchor === "aux" ? planMoveExact(state.board, items, dxNm, dyNm, rotate, "origin", [aux[0], aux[1]]) : planMoveExact(state.board, items, dxNm, dyNm, rotate, anchor, [state.localOriginNm.x, state.localOriginNm.y]);
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
            <select value={anchor} onChange={(e) => setAnchor(e.target.value as Anchor)} data-testid="move-exact-anchor">
              {menu.map((a) => (
                <option key={a} value={a}>
                  {label[a]}
                </option>
              ))}
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
