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
// Rotation sign: `EDIT_TOOL::MoveExact` copies the dialog's angle (`EDA_ANGLE angle = rotation;`) before its `if( !m_DisplayInvertYAxis ) rotation =
// -rotation;`, and turns every item by the copy, so the Y axis of Origins & Axes changes nothing (8303b2ad and 25ec2335 alike): a positive angle is
// counter-clockwise on the screen (`BOARD_ITEM::Rotate`, `RotatePoint`), whatever way Y grows. The backend's `rotate_items` turns clockwise for a positive
// angle, so the plan negates it.
//
// Origins & Axes (UI-LEFT-2 PP9): Move X and Move Y are `REL_X_COORD` / `REL_Y_COORD` (`m_moveX.SetCoordType`), so an axis that grows the other way negates what
// is typed; in polar mode the distance is still `REL_X_COORD` and the angle field reads minus the angle typed whatever Y does (`UNIT_BINDER::GetAngleValue`,
// kicad-port/originTransforms.ts `angleFieldValue`): 90 moves up the screen.
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { coordValue, useOriginTransforms } from "../state/originTransforms";
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
  const xf = useOriginTransforms();

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

  // `GetTranslationInIU`: `r = m_moveX.GetDoubleValue()`, `q = m_moveY.GetAngleValue()` (minus the angle typed), `( r cos q, r sin q )`; or the two fields read back.
  const r = coordValue(xf, distance, "rel_x", state.units);
  const dxNm = polar ? r * Math.cos((angleDeg * Math.PI) / 180) : coordValue(xf, moveX, "rel_x", state.units);
  const dyNm = polar ? -r * Math.sin((angleDeg * Math.PI) / 180) : coordValue(xf, moveY, "rel_y", state.units);

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
                <input autoFocus type="number" aria-label="Distance" value={distance} onChange={(e) => setDistance(Number(e.target.value))} />
                <button onClick={() => clear(setDistance)}>0</button>
                <span>Angle:</span>
                <input type="number" step={1} aria-label="Angle" value={angleDeg} onChange={(e) => setAngleDeg(Number(e.target.value))} />
                <button onClick={() => clear(setAngleDeg)}>0</button>
              </>
            ) : (
              <>
                <span>Move X:</span>
                <input autoFocus type="number" aria-label="Move X" value={moveX} onChange={(e) => setMoveX(Number(e.target.value))} />
                <button onClick={() => clear(setMoveX)}>0</button>
                <span>Move Y:</span>
                <input type="number" aria-label="Move Y" value={moveY} onChange={(e) => setMoveY(Number(e.target.value))} />
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
