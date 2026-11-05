// `T` (sch_drawing_tools.cpp TwoClickPlace/createNewText): the dialog a new text is made with, KiCad's order -- the tool's first click (or its hotkey,
// where the cursor is) opens it, and OK puts the text in the hand: it rides the cursor (actions/schPlaceActions.ts, drawn by SchematicView) until the
// click that places it with one `place_text` command, one undo step. Cancel makes nothing and the tool stays armed (`createNewText` returns null).
// The dialog confirms the content, size and orientation and how the text is justified about its anchor (`DIALOG_TEXT_PROPERTIES`). The
// justification the tool starts with is KiCad's: centre and centre (`m_lastTextHJustify`, `m_lastTextVJustify`), and what was last chosen is the next
// dialog's start (kicad-port/schLastPlaced.ts). Properties (`E`) of the text in the hand opens this dialog on it: OK changes the text that rides.
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { lastPlaced, rememberPlaced } from "../kicad-port/schLastPlaced";
import type { SchHAlign, SchVAlign } from "../api/schEditTypes";
import type { CarriedText } from "../kicad-port/schPlaceTool";
import { carryItem, changeCarried } from "../actions/schPlaceActions";
import { getSchCarry, setSchCarry, useSchCarry } from "../state/schCarryStore";

const DEFAULT_SIZE_NM = 1_270_000; // 1.27mm -- eeschema's own SCHEMATIC_SETTINGS::m_DefaultTextSize default (50 mil)

export function SchTextDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const pending = state.schTextPending;
  const carry = useSchCarry();
  const editing = carry?.editing && carry.item.kind === "text" ? carry.item : null;

  const [content, setContent] = useState("");
  const [size, setSize] = useState(DEFAULT_SIZE_NM);
  const [vertical, setVertical] = useState(false);
  const [h, setH] = useState<SchHAlign>("center");
  const [v, setV] = useState<SchVAlign>("center");

  useEffect(() => {
    if (!pending) return;
    const c = getSchCarry();
    const from = c?.editing && c.item.kind === "text" ? (c.item as CarriedText) : null;
    const last = lastPlaced();
    setContent(from?.content ?? "");
    setSize(from?.sizeNm ?? DEFAULT_SIZE_NM);
    setVertical(from?.vertical ?? last.textVertical);
    setH(from?.h ?? last.textH);
    setV(from?.v ?? last.textV);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending?.at[0], pending?.at[1]]);

  if (!pending) return null;
  const close = () => {
    const now = getSchCarry();
    if (now?.editing) setSchCarry({ ...now, editing: false });
    dispatch({ type: "SET_SCH_TEXT_PENDING", pending: null });
  };

  const submit = () => {
    if (!content.trim()) return;
    const item: CarriedText = { kind: "text", content, sizeNm: size, vertical, h, v };
    dispatch({ type: "SET_SCH_TEXT_PENDING", pending: null });
    if (editing) {
      changeCarried(item);
      return;
    }
    // `m_lastTextHJustify`, `m_lastTextVJustify`, `m_lastTextAngle`
    rememberPlaced({ textH: h, textV: v, textVertical: vertical });
    carryItem({ api }, "sch_text", item, [pending.at[0], pending.at[1]]);
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 340 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Text Properties</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "100px 1fr" }}>
            <span>Text</span>
            <input
              autoFocus
              value={content}
              onChange={(e) => setContent(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            <span>Size</span>
            <input type="number" min={100} step={50} value={size} onChange={(e) => setSize(Number(e.target.value))} />
            <span>Orientation</span>
            <select aria-label="Orientation" value={vertical ? "v" : "h"} onChange={(e) => setVertical(e.target.value === "v")}>
              <option value="h">Horizontal</option>
              <option value="v">Vertical</option>
            </select>
            <span>Horizontal justification</span>
            <select aria-label="Horizontal justification" value={h} onChange={(e) => setH(e.target.value as SchHAlign)}>
              <option value="left">Left</option>
              <option value="center">Center</option>
              <option value="right">Right</option>
            </select>
            <span>Vertical justification</span>
            <select aria-label="Vertical justification" value={v} onChange={(e) => setV(e.target.value as SchVAlign)}>
              <option value="top">Top</option>
              <option value="center">Center</option>
              <option value="bottom">Bottom</option>
            </select>
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!content.trim()} onClick={submit}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
