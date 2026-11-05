// The Footprint Editor's Group Properties: `DIALOG_GROUP_PROPERTIES` (common/dialogs/dialog_group_properties.cpp) on a group of the open footprint (`FOOTPRINT_EDIT_FRAME::OnEditItemRequest`:
// `case PCB_GROUP_T: RunAction( ACTIONS::groupProperties )`), opened by E with the group selected: the group's name and its members. OK is one `fp_item set_group_name`, one undo step.
// Not ported here: the Library link (a footprint's group links to no design block), Locked (a footprint's items keep no lock), adding and removing members.
import { useEffect, useState } from "react";
import { useFpApi, useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { fpItemKind } from "../../kicad-port/fpItems";

export function FpGroupDialog() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const id = state.itemDialog?.kind === "group" ? state.itemDialog.id : null;
  const group = id ? state.footprint?.items?.groups?.find((g) => g.id === id) : undefined;
  const [name, setName] = useState("");
  useEffect(() => {
    if (group) setName(group.name);
    // only when the dialog opens on a group
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  if (!id || !group || !state.footprint) return null;
  const fp = state.footprint;
  const close = () => dispatch({ type: "SET_ITEM_DIALOG", dialog: null });
  const ok = async () => {
    if (name === group.name) return close();
    if (await api.cmd({ op: "fp_item", verb: "set_group_name", footprint: state.name ?? fp.name, id, name })) close();
  };
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 360 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Group Properties" data-testid="fp-group-dialog">
        <div className="dialog-header">
          <span>Group Properties</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "90px 1fr" }}>
            <span>Name</span>
            <input value={name} autoFocus data-testid="fp-group-name" onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void ok()} />
          </div>
          <div style={{ marginTop: 10, fontSize: 12 }}>Group members</div>
          <div style={{ border: "1px solid var(--chrome-border)", minHeight: 80, maxHeight: 200, overflowY: "auto", marginTop: 4 }} role="listbox" aria-label="Group members">
            {group.member_ids.map((m) => (
              <div key={m} role="option" aria-selected={false} style={{ padding: "3px 8px", fontSize: 12 }}>
                {`${fpItemKind(fp, m) ?? "item"} ${m}`}
              </div>
            ))}
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" data-testid="fp-group-ok" onClick={() => void ok()}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
