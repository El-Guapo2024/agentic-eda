// The message panel: a row of key/value fields describing the current
// selection, or the board when nothing is selected. KiCad builds this
// from each item type's GetMsgPanelInfo (common/widgets/msgpanel.cpp);
// this session could not read that file, so the field set below is
// this app's own reasonable approximation from data the API exposes,
// not a transcription of KiCad's exact fields.
import React from "react";
import { useStudioState } from "../state/store";
import { formatLength, formatXY } from "../state/units";

export function MessagePanel() {
  const state = useStudioState();
  const board = state.board;
  const selectedRefs = [...state.selection];

  if (!board) return <div className="message-panel" />;

  if (selectedRefs.length === 1) {
    const p = board.parts.find((x) => x.ref === selectedRefs[0]);
    if (p) {
      const nets = [...new Set((p.pads ?? []).map((q) => q.net).filter((n): n is string => !!n))];
      return (
        <div className="message-panel">
          <span className="kv">
            <b>Ref</b>
            {p.ref}
          </span>
          <span className="kv">
            <b>Value</b>
            {p.value ?? "–"}
          </span>
          <span className="kv">
            <b>Footprint</b>
            {p.package ?? "–"}
          </span>
          {p.placed && p.at && (
            <span className="kv">
              <b>At</b>
              {formatXY(p.at[0], p.at[1], state.units)}
            </span>
          )}
          {p.placed && p.rot !== undefined && (
            <span className="kv">
              <b>Orient</b>
              {`${p.rot}°${p.side === "bottom" ? " (bottom)" : ""}`}
            </span>
          )}
          <span className="kv">
            <b>Nets</b>
            {nets.length}
          </span>
        </div>
      );
    }
  }

  if (selectedRefs.length > 1) {
    return (
      <div className="message-panel">
        <span className="kv">
          <b>Selected</b>
          {selectedRefs.length} items
        </span>
      </div>
    );
  }

  const placed = board.parts.filter((p) => p.placed).length;
  const fails = board.checks.filter((c) => c.fail).length;
  return (
    <div className="message-panel">
      <span className="kv">
        <b>Board</b>
        {board.name}
      </span>
      <span className="kv">
        <b>Parts</b>
        {placed}/{board.parts.length} placed
      </span>
      <span className="kv">
        <b>Failures</b>
        {fails}
      </span>
      <span className="kv">
        <b>Grid</b>
        {formatLength(board.snap, state.units)}
      </span>
    </div>
  );
}
