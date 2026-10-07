// Small dialogs for the "pcbnew parity" action batch (docs/parity/
// UI-ACTIONS.md), one component each, all driven by `state.pcbx.pcbDialog`:
//
//   find_move          pcbnew.InteractiveEdit.FindMove ("T") --
//                      DIALOG_GET_FOOTPRINT_BY_NAME (a reference entry plus
//                      the sorted "REF    ( value )" list), then
//                      EDIT_TOOL::GetAndPlace selects that footprint and
//                      starts Move with its anchor on the cursor.
//   position_relative  pcbnew.PositionRelative.positionRelative ("Shift+P")
//                      -- DIALOG_POSITION_RELATIVE: reference location (local
//                      origin / grid origin / a picked item / a point), a
//                      Cartesian or polar offset, "Clear" buttons that reset an
//                      offset to the current one, OK applies
//                      `RelativeItemSelectionMove`.
//   place_footprint    pcbnew.EditorControl.placeFootprint ("A") -- arms an
//                      unplaced footprint for click-to-place (this project's
//                      footprints come from the schematic, not a library
//                      chooser).
import { useEffect, useMemo, useRef, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { umFrom, umTo } from "../state/units";
import { movableItem, polarTranslation, positionRelativeSelectionAnchor, relativeMoveVector, toPolarDeg, type MovableKind } from "../kicad-port/pcbEditActions";
import { itemPosition } from "../kicad-port/pcbReference";
import { picker, pickItem, pickPoint } from "../actions/pcbPicker";

export function PcbParityDialogs() {
  const which = useStudioState().pcbx.pcbDialog;
  if (which === "find_move") return <FindMoveDialog />;
  if (which === "position_relative") return <PositionRelativeDialog />;
  if (which === "place_footprint") return <PlaceFootprintDialog />;
  return null;
}

function useClose() {
  const dispatch = useStudioDispatch();
  return () => dispatch({ type: "PCBX", patch: { pcbDialog: null } });
}

/** `DIALOG_GET_FOOTPRINT_BY_NAME` + `EDIT_TOOL::GetAndPlace`. */
function FindMoveDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const close = useClose();
  const [text, setText] = useState("");

  // `fplist`: "REF    ( value )" for every footprint, sorted (wxArrayString::Sort).
  const entries = useMemo(
    () =>
      (state.board?.parts ?? [])
        .filter((p) => p.placed)
        .map((p) => ({ ref: p.ref, label: `${p.ref}    ( ${p.value ?? ""} )` }))
        .sort((a, b) => (a.label < b.label ? -1 : a.label > b.label ? 1 : 0)),
    [state.board]
  );

  const submit = (name: string) => {
    // GetFootprintFromBoardByReference: trimmed, case-insensitive reference match.
    const wanted = name.trim().toLowerCase();
    const part = wanted ? state.board?.parts.find((p) => p.placed && p.ref.toLowerCase() === wanted) : undefined;
    close();
    if (!part?.at) {
      if (wanted) dispatch({ type: "TOAST", message: `No footprint with reference "${name.trim()}" on the board.`, kind: "error" });
      return;
    }
    // selectionClear + selectItem(fp), SetReferencePoint(fp->GetPosition()), PostAction(move).
    dispatch({ type: "SET_SELECTION", refs: [part.ref] });
    dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
    dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
    dispatch({ type: "SET_MOVE_ORIGIN", at: { x: part.at[0], y: part.at[1] } });
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 340 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Get and Move Footprint</span>
        </div>
        <div className="dialog-body">
          <div style={{ fontSize: 12, marginBottom: 6 }}>Footprint reference:</div>
          <input
            autoFocus
            style={{ width: "100%" }}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") submit(text);
              if (e.key === "Escape") close();
            }}
          />
          <div style={{ maxHeight: 180, overflowY: "auto", marginTop: 8, border: "1px solid var(--border, #444)" }}>
            {entries.map((en) => (
              <div key={en.ref} style={{ padding: "2px 6px", cursor: "pointer", whiteSpace: "pre", fontFamily: "monospace", background: en.ref === text.trim() ? "var(--accent-bg, #2a4a6a)" : undefined }} onClick={() => setText(en.ref)} onDoubleClick={() => submit(en.ref)}>
                {en.label}
              </div>
            ))}
            {entries.length === 0 && <div style={{ padding: 6, opacity: 0.7 }}>No footprints are placed.</div>}
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={() => submit(text)}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

type AnchorKind = "origin" | "grid" | "item" | "point";

/** `DIALOG_POSITION_RELATIVE` -- see this file's header. */
function PositionRelativeDialog() {
  const state = useStudioState();
  const api = useStudioApi();
  const close = useClose();
  const board = state.board;
  const units = state.units;

  // The selection the move acts on: what this app can move, in selection order.
  const movables = useMemo(() => {
    if (!board) return [] as { ref: string; kind: MovableKind; at: [number, number] }[];
    const out: { ref: string; kind: MovableKind; at: [number, number] }[] = [];
    for (const ref of state.selection) {
      const it = movableItem(board, ref);
      if (it) out.push({ ref, ...it });
    }
    return out;
  }, [board, state.selection]);

  // PositionRelative: "We prefer footprints, then pads, then anything else".
  const selectionAnchor = useMemo(() => positionRelativeSelectionAnchor(movables.map((m) => ({ isFootprint: m.kind === "part", x: m.at[0], y: m.at[1] }))), [movables]);

  const [anchorKind, setAnchorKind] = useState<AnchorKind>("origin");
  const [itemRef, setItemRef] = useState("");
  /** An item picked on the canvas ("Select Item..."): any board item, anchored on its position (`UpdatePickedItem`). */
  const [pickedItem, setPickedItem] = useState<{ id: string; x: number; y: number } | null>(null);
  const [pointX, setPointX] = useState(0); // display units
  const [pointY, setPointY] = useState(0);
  // `OnSelectItemClick` / `OnSelectPointClick`: "Hide, but do not close, the dialog" while the picker runs.
  const [picking, setPicking] = useState(false);
  const pickingRef = useRef(false);
  useEffect(
    () => () => {
      if (pickingRef.current) picker.cancel(true);
    },
    []
  );
  const runPick = async <T,>(pick: () => Promise<T | null>): Promise<T | null> => {
    pickingRef.current = true;
    setPicking(true);
    const result = await pick();
    pickingRef.current = false;
    setPicking(false);
    return result;
  };
  const shown = (um: number) => Number(umTo(um, units).toFixed(units === "mm" ? 4 : units === "mil" ? 2 : 5));
  const selectItem = async () => {
    const id = await runPick(() => pickItem("Select reference item..."));
    const at = id && board ? itemPosition(board, id) : null;
    if (!id || !at) return;
    setPickedItem({ id, x: at.x, y: at.y });
    setItemRef("");
    setAnchorKind("item");
  };
  const selectPoint = async () => {
    const p = await runPick(() => pickPoint("Select reference point..."));
    if (!p) return;
    setPointX(shown(p.x));
    setPointY(shown(p.y));
    setAnchorKind("point");
  };
  const [polar, setPolar] = useState(false);
  const [a, setA] = useState(0); // Offset X (or distance), display units
  const [b, setB] = useState(0); // Offset Y (or angle, degrees)

  const placed = (board?.parts ?? []).filter((p) => p.placed && p.at);

  // getAnchorPos()
  const referenceAnchor = (): { x: number; y: number } => {
    switch (anchorKind) {
      case "origin":
        return state.localOriginUm ?? { x: 0, y: 0 };
      case "grid":
        return { x: 0, y: 0 }; // BOARD_DESIGN_SETTINGS grid origin: this model has no grid-origin setting, so it is the board origin.
      case "item": {
        if (pickedItem && !itemRef) return { x: pickedItem.x, y: pickedItem.y };
        const p = placed.find((q) => q.ref === itemRef);
        return p?.at ? { x: p.at[0], y: p.at[1] } : { x: 0, y: 0 };
      }
      case "point":
      default:
        return { x: umFrom(pointX, units), y: umFrom(pointY, units) };
    }
  };

  // DIALOG_POSITION_RELATIVE::OnClear / OnPolarChanged: "Reset to the current X offset from the reference position."
  const currentOffset = () => {
    const ref = referenceAnchor();
    return selectionAnchor ? { x: selectionAnchor.x - ref.x, y: selectionAnchor.y - ref.y } : { x: 0, y: 0 };
  };
  const clearA = () => {
    const o = currentOffset();
    setA(polar ? umTo(toPolarDeg(o.x, o.y).r, units) : umTo(o.x, units));
  };
  const clearB = () => {
    const o = currentOffset();
    setB(polar ? toPolarDeg(o.x, o.y).deg : umTo(o.y, units));
  };
  const switchPolar = (nextPolar: boolean) => {
    // Convert the entered values across, as OnPolarChanged does.
    if (nextPolar) {
      const p = toPolarDeg(umFrom(a, units), umFrom(b, units));
      setA(umTo(p.r, units));
      setB(p.deg);
    } else {
      const t = polarTranslation(umFrom(a, units), b);
      setA(umTo(t.x, units));
      setB(umTo(t.y, units));
    }
    setPolar(nextPolar);
  };

  if (picking) return null;
  if (movables.length === 0 || !selectionAnchor) {
    // `PositionRelative`: "if( selection.Empty() ) return 0;"
    return (
      <div className="dialog-backdrop" onClick={close}>
        <div className="dialog" style={{ width: 320 }} onClick={(e) => e.stopPropagation()}>
          <div className="dialog-header">
            <span>Position Relative To...</span>
          </div>
          <div className="dialog-body" style={{ fontSize: 12 }}>
            Select a footprint, via, graphic or text item first.
          </div>
          <div className="dialog-footer">
            <button onClick={close}>Close</button>
          </div>
        </div>
      </div>
    );
  }

  const submit = async () => {
    // getTranslationInIU
    const translation = polar ? polarTranslation(umFrom(a, units), b) : { x: Math.round(umFrom(a, units)), y: Math.round(umFrom(b, units)) };
    const v = relativeMoveVector(referenceAnchor(), translation, selectionAnchor);
    close();
    if (v.x === 0 && v.y === 0) return;
    // moveSelectionBy: each item by the one aggregate vector (grouped per kind -- one Cmd per item).
    for (const kind of ["part", "via", "shape", "text"] as const) {
      const refs = movables.filter((m) => m.kind === kind).map((m) => m.ref);
      if (refs.length) await api.commitMove(refs, v.x, v.y, kind);
    }
  };

  const clearBtn = { padding: "0 6px" } as const;
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 400 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Position Relative To...</span>
        </div>
        <div className="dialog-body">
          <div style={{ fontSize: 12, marginBottom: 6 }}>Reference location:</div>
          <div style={{ display: "grid", gap: 4, marginBottom: 10 }}>
            <label className="filter-row">
              <input type="radio" checked={anchorKind === "origin"} onChange={() => setAnchorKind("origin")} /> Local coordinates origin
            </label>
            <label className="filter-row">
              <input type="radio" checked={anchorKind === "grid"} onChange={() => setAnchorKind("grid")} /> Grid origin
            </label>
            <label className="filter-row">
              <input type="radio" checked={anchorKind === "item"} onChange={() => setAnchorKind("item")} /> Item:
              <select value={itemRef} onChange={(e) => { setItemRef(e.target.value); setAnchorKind("item"); }} style={{ marginLeft: 6 }}>
                <option value="">{pickedItem ? `${pickedItem.id} (picked)` : "<none selected>"}</option>
                {placed.map((p) => (
                  <option key={p.ref} value={p.ref}>
                    {p.ref}
                  </option>
                ))}
              </select>
              <button type="button" style={{ marginLeft: 6, padding: "0 6px" }} title="Pick the reference item on the board" onClick={() => void selectItem()}>
                Select Item...
              </button>
            </label>
            <label className="filter-row">
              <input type="radio" checked={anchorKind === "point"} onChange={() => setAnchorKind("point")} /> Point:
              <input type="number" step="any" value={pointX} onChange={(e) => { setPointX(Number(e.target.value)); setAnchorKind("point"); }} style={{ width: 80, marginLeft: 6 }} />
              <input type="number" step="any" value={pointY} onChange={(e) => { setPointY(Number(e.target.value)); setAnchorKind("point"); }} style={{ width: 80, marginLeft: 4 }} />
              <button type="button" style={{ marginLeft: 6, padding: "0 6px" }} title="Pick the reference point on the board" onClick={() => void selectPoint()}>
                Select Point...
              </button>
            </label>
          </div>
          <label className="filter-row" style={{ marginBottom: 8 }}>
            <input type="checkbox" checked={polar} onChange={(e) => switchPolar(e.target.checked)} /> Polar coordinates
          </label>
          <div className="kv-grid" style={{ gridTemplateColumns: "110px 1fr auto" }}>
            <span>{polar ? "Distance:" : "Offset X:"}</span>
            <input autoFocus type="number" step="any" value={a} onChange={(e) => setA(Number(e.target.value))} onKeyDown={(e) => e.key === "Enter" && void submit()} />
            <button style={clearBtn} title={polar ? "Reset to the current distance from the reference position." : "Reset to the current X offset from the reference position."} onClick={clearA}>
              Clear
            </button>
            <span>{polar ? "Angle:" : "Offset Y:"}</span>
            <input type="number" step="any" value={b} onChange={(e) => setB(Number(e.target.value))} onKeyDown={(e) => e.key === "Enter" && void submit()} />
            <button style={clearBtn} title={polar ? "Reset to the current angle from the reference position." : "Reset to the current Y offset from the reference position."} onClick={clearB}>
              Clear
            </button>
          </div>
          <div style={{ fontSize: 11, opacity: 0.7, marginTop: 6 }}>
            {movables.length} item{movables.length === 1 ? "" : "s"} · units: {units}
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={() => void submit()}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

/** `BOARD_EDITOR_CONTROL::PlaceFootprint` -- pick one, then the next canvas click places it (Canvas.tsx's armed-part path). */
function PlaceFootprintDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const close = useClose();
  const unplaced = (state.board?.parts ?? []).filter((p) => !p.placed).sort((a, b) => (a.ref < b.ref ? -1 : a.ref > b.ref ? 1 : 0));
  const [sel, setSel] = useState(unplaced[0]?.ref ?? "");
  useEffect(() => {
    if (!sel && unplaced[0]) setSel(unplaced[0].ref);
  }, [sel, unplaced]);

  const choose = (ref: string) => {
    close();
    if (!ref) return;
    dispatch({ type: "SET_SELECTION", refs: [] });
    dispatch({ type: "SET_ARMED", ref });
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 340 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Place Footprints</span>
        </div>
        <div className="dialog-body">
          {unplaced.length === 0 ? (
            <div style={{ fontSize: 12 }}>Every footprint is already placed.</div>
          ) : (
            <div style={{ maxHeight: 220, overflowY: "auto", border: "1px solid var(--border, #444)" }}>
              {unplaced.map((p) => (
                <div key={p.ref} style={{ padding: "2px 6px", cursor: "pointer", whiteSpace: "pre", fontFamily: "monospace", background: p.ref === sel ? "var(--accent-bg, #2a4a6a)" : undefined }} onClick={() => setSel(p.ref)} onDoubleClick={() => choose(p.ref)}>
                  {`${p.ref}    ( ${p.value ?? p.package ?? ""} )`}
                </div>
              ))}
            </div>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!sel} onClick={() => choose(sel)}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
