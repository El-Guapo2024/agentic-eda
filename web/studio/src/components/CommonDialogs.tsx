// The dialogs of the shared tools (state/commonDialogs.ts holds which is open): Group Properties and About.
import { useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { closeGroupDialog, updateGroupDialog, useCommonDialogs } from "../state/commonDialogs";
import { removeMemberAt } from "../kicad-port/groupEdit";
import { linkProblem } from "../kicad-port/designBlocks";
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
 * selects the group again; Cancel changes nothing. The "Library link:" box is the group's design block (`m_libraryLink`, `LIB_ID::Parse`: "Invalid library link:
 * '...'" keeps the dialog open); empty clears the link. Not here: the dialog's "Locked" box, which this model has no counterpart for.
 */
function GroupPropertiesDialog() {
  const dlg = useCommonDialogs().group;
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const { run } = useActionRunner();
  const [row, setRow] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [linkError, setLinkError] = useState<string | null>(null);
  if (!dlg || dlg.hidden) return null;

  const close = () => {
    dispatch({ type: "SET_HOT", refs: [] });
    setRow(null);
    setLinkError(null);
    closeGroupDialog();
  };
  const ok = async () => {
    // `TransferDataFromWindow`: a link that is no library id is refused and the dialog stays
    const bad = linkProblem(dlg.designBlock);
    setLinkError(bad);
    if (bad) return;
    setBusy(true);
    try {
      // the group of the sheet in view is the sheet's (`SCH_GROUP`), the board's otherwise
      const design_block = dlg.designBlock.trim();
      const done = await api.cmd(
        state.tab === "schematic"
          ? { op: "sch_edit", verb: "edit_group", id: dlg.id, name: dlg.name, member_ids: dlg.members, design_block }
          : { op: "edit_group", id: dlg.id, name: dlg.name, member_ids: dlg.members, design_block }
      );
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
            <span>Library link:</span>
            <input
              value={dlg.designBlock}
              aria-label="Library link"
              placeholder="Library:Design block"
              spellCheck={false}
              onChange={(e) => {
                setLinkError(null);
                updateGroupDialog({ designBlock: e.target.value });
              }}
              onKeyDown={(e) => e.key === "Enter" && void ok()}
            />
          </div>
          {linkError && (
            <div role="alert" style={{ color: "var(--chrome-error, #c0392b)", fontSize: 12, marginTop: 4 }}>
              {linkError}
            </div>
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
