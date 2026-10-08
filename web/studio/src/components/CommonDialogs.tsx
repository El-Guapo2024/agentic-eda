// The dialogs of the shared tools (state/commonDialogs.ts holds which is open): Group Properties and About.
import { useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { closeGroupDialog, updateGroupDialog, useCommonDialogs } from "../state/commonDialogs";
import { removeMemberAt } from "../kicad-port/groupEdit";
import { describeBoardItem } from "../actions/editorAdapter";
import { useActionRunner } from "../actions/useActionRunner";

/**
 * `DIALOG_GROUP_PROPERTIES` (common/dialogs/dialog_group_properties.cpp): the group's name and its member list. The plus button hides the
 * dialog and asks for a click on a new member (`ACTIONS::pickNewGroupMember`, PCB_GROUP_TOOL::PickNewMember -- common.Groups.selectNewGroupMember),
 * the trash button drops the selected row, a click on a row shows the item on the canvas (`FocusOnItem`). OK is one undo step (`EditGroup`) and
 * selects the group again; Cancel changes nothing. Not here: the dialog's "Locked" box and design-block library link, which this model has no
 * counterpart for.
 */
function GroupPropertiesDialog() {
  const dlg = useCommonDialogs().group;
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const { run } = useActionRunner();
  const [row, setRow] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  if (!dlg || dlg.hidden) return null;

  const close = () => {
    dispatch({ type: "SET_HOT", refs: [] });
    setRow(null);
    closeGroupDialog();
  };
  const ok = async () => {
    setBusy(true);
    try {
      const done = await api.cmd({ op: "edit_group", id: dlg.id, name: dlg.name, member_ids: dlg.members });
      if (!done) return;
      dispatch({ type: "SET_HOT", refs: [] });
      setRow(null);
      closeGroupDialog();
      // `ACTIONS::selectItem( group )`; a group left with fewer than two members is gone
      dispatch({ type: "SET_SELECTION", refs: dlg.members.length >= 2 ? [dlg.id] : [] });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 380 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Group Properties">
        <div className="dialog-header">
          <span>Group Properties</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "90px 1fr" }}>
            <span>Name</span>
            <input value={dlg.name} autoFocus onChange={(e) => updateGroupDialog({ name: e.target.value })} onKeyDown={(e) => e.key === "Enter" && void ok()} />
          </div>
          <div style={{ marginTop: 10, fontSize: 12 }}>Group members</div>
          <div style={{ border: "1px solid var(--chrome-border)", minHeight: 120, maxHeight: 220, overflowY: "auto", marginTop: 4 }} role="listbox" aria-label="Group members">
            {dlg.members.map((id, i) => (
              <div
                key={id}
                role="option"
                aria-selected={row === i}
                onClick={() => {
                  setRow(i);
                  dispatch({ type: "SET_HOT", refs: [id] });
                }}
                style={{ padding: "3px 8px", cursor: "default", background: row === i ? "var(--chrome-active)" : undefined, fontSize: 12 }}
              >
                {describeBoardItem(state.board, { id, kind: id })}
              </div>
            ))}
            {dlg.members.length === 0 && <div style={{ padding: "6px 8px", color: "var(--chrome-text-dim)", fontSize: 12 }}>No members.</div>}
          </div>
          <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
            <button title="Add a member: click it on the board" onClick={() => run("common.Groups.selectNewGroupMember")}>
              +
            </button>
            <button
              title="Remove the selected member"
              disabled={row === null}
              onClick={() => {
                if (row === null) return;
                updateGroupDialog({ members: removeMemberAt(dlg.members, row) });
                setRow(null);
                dispatch({ type: "SET_HOT", refs: [] });
              }}
            >
              Remove
            </button>
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={busy} onClick={() => void ok()}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

export function CommonDialogs() {
  return (
    <>
      <GroupPropertiesDialog />
    </>
  );
}
