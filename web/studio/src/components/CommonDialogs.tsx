// The dialogs of the shared tools (state/commonDialogs.ts holds which is open): Group Properties and About.
import { useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { closeGroupDialog, updateGroupDialog, useCommonDialogs } from "../state/commonDialogs";
import { removeMemberAt } from "../kicad-port/groupEdit";
import { ancestors } from "../kicad-port/groupTree";
import { describeBoardItem, describeSchematicItem } from "../actions/editorAdapter";
import { useActionRunner } from "../actions/useActionRunner";
import { AboutDialog } from "./AboutDialog";
import { PageSettingsDialog } from "./PageSettingsDialog";
import { GridOriginDialog } from "./GridOriginDialog";
import { GridsDialog } from "./GridsDialog";
import { SaveFootprintAsDialog } from "./SaveFootprintAsDialog";

/**
 * `DIALOG_GROUP_PROPERTIES` (common/dialogs/dialog_group_properties.cpp): the group's name and its member list. The plus button hides the
 * dialog and asks for a click on a new member (`ACTIONS::pickNewGroupMember`, PCB_GROUP_TOOL::PickNewMember -- common.Groups.selectNewGroupMember),
 * the trash button drops the selected row, a click on a row shows the item on the canvas (`FocusOnItem`). OK is one undo step (`EditGroup`) and
 * selects the group again; Cancel changes nothing. The "Library link:" box is the group's design block (`m_libraryLink`, `LIB_ID::Parse( .., true )`, which never
 * fails: a name with no library is a link too, illegal characters become `_`); empty clears the link. "Locked" (`m_locked`, shown for a `PCB_GROUP_T` only) is the
 * board group's lock, sent with the edit in one batch.
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
  const onBoard = state.tab !== "schematic";
  // `m_locked->SetValue( aGroup->AsEdaItem()->IsLocked() )`: the group's own lock or that of a group above it (`BOARD_ITEM::IsLocked`); OK sets the group's own
  // lock to the box (`SetLocked( m_locked->GetValue() )`), so a group shown locked by the group above becomes locked itself
  const lockedSet = new Set(state.board?.locked ?? []);
  const ownLocked = onBoard && lockedSet.has(dlg.id);
  const wasLocked = ownLocked || (onBoard && ancestors(state.board?.drawings?.groups ?? [], dlg.id).some((g) => lockedSet.has(g.id)));
  const locked = dlg.locked ?? wasLocked;
  const ok = async () => {
    setBusy(true);
    try {
      // the group of the sheet in view is the sheet's (`SCH_GROUP`), the board's otherwise
      const design_block = dlg.designBlock.trim();
      const done = !onBoard
        ? await api.cmd({ op: "sch_edit", verb: "edit_group", id: dlg.id, name: dlg.name, member_ids: dlg.members, design_block })
        : await api.cmdBatch([
            { op: "edit_group", id: dlg.id, name: dlg.name, member_ids: dlg.members, design_block },
            // `m_group->AsEdaItem()->SetLocked( m_locked->GetValue() )`
            ...(locked !== ownLocked ? [{ op: "set_locked" as const, ids: [dlg.id], locked }] : []),
          ]);
      if (!done) return;
      dispatch({ type: "SET_HOT", refs: [] });
      setRow(null);
      closeGroupDialog();
      // `ACTIONS::selectItem( group )`: a group of one is still a group; one left empty is gone
      dispatch({ type: "SET_SELECTION", refs: dlg.members.length >= 1 ? [dlg.id] : [] });
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
            <span>Library link:</span>
            <input
              value={dlg.designBlock}
              aria-label="Library link"
              placeholder="Library:Design block"
              spellCheck={false}
              onChange={(e) => updateGroupDialog({ designBlock: e.target.value })}
              onKeyDown={(e) => e.key === "Enter" && void ok()}
            />
          </div>
          {onBoard && (
            <label className="filter-row" style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 6 }}>
              <input type="checkbox" checked={locked} aria-label="Locked" onChange={(e) => updateGroupDialog({ locked: e.target.checked })} /> Locked
            </label>
          )}
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
                {state.tab === "schematic" ? describeSchematicItem(state.schematic, { id, kind: id }) : describeBoardItem(state.board, { id, kind: id })}
              </div>
            ))}
            {dlg.members.length === 0 && <div style={{ padding: "6px 8px", color: "var(--chrome-text-dim)", fontSize: 12 }}>No members.</div>}
          </div>
          <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
            <button title={state.tab === "schematic" ? "Add a member: click it on the sheet" : "Add a member: click it on the board"} onClick={() => run("common.Groups.selectNewGroupMember")}>
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
      <AboutDialog />
      <PageSettingsDialog />
      <GridOriginDialog />
      <GridsDialog />
      <SaveFootprintAsDialog />
    </>
  );
}
