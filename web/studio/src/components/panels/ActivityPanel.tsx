// Not a KiCad panel -- docked where KiCad's Search panel goes (right
// dock, see RightDock.tsx), per the task. Shows activity.jsonl live: CLI
// and UI edits together, actor labeled, most recent first. The data
// comes straight through GET /api/state's `activity` field (already
// serialized by crates/cli/src/board.rs `log_activity`); no backend
// change was needed for this panel.
import React from "react";
import { useStudioState } from "../../state/store";

export function ActivityPanel() {
  const state = useStudioState();
  const activity = state.board?.activity ?? [];

  if (activity.length === 0) {
    return (
      <div className="panel-section">
        <h3>Activity</h3>
        <div className="panel-empty">Nothing yet.</div>
      </div>
    );
  }

  return (
    <div className="panel-section">
      <h3>Activity</h3>
      <div className="activity-list">
        {activity.map((a, i) => (
          <div key={i} className={`activity-entry${a.ok ? "" : " fail"}`}>
            <span className="who">{a.by === "ui" ? "you" : a.by}</span>
            <code>{a.cmd}</code> {a.ok ? "✓" : "✗"}
            <span className="msg">{`${new Date(a.t).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })} · ${a.message}`}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
