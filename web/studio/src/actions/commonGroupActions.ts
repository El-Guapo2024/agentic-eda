// The group tool's actions beyond Group / Ungroup / Enter / Leave (which `useActionRunner.ts` registers): Group Properties, Add Items, Remove Items
// and picking a new member for the properties dialog. Board only: the footprint editor and the schematic have no groups here.
//
//   common/tool/group_tool.cpp                 GROUP_TOOL::GroupProperties / AddToGroup / RemoveFromGroup   ACTIONS::groupProperties, addToGroup, removeFromGroup
//   pcbnew/tools/pcb_group_tool.cpp            PCB_GROUP_TOOL::PickNewMember                                ACTIONS::pickNewGroupMember (common.Groups.selectNewGroupMember)
//   common/dialogs/dialog_group_properties.cpp DIALOG_GROUP_PROPERTIES                                      components/CommonDialogs.tsx
import type { ActionHandler, CommonActionContext } from "./commonActions";
import { picker } from "./pcbPicker";
import { addMember, newMemberFromPick, planAddToGroup, planRemoveFromGroup } from "../kicad-port/groupEdit";
import { getCommonDialogs, openGroupDialog, updateGroupDialog } from "../state/commonDialogs";

export function registerGroupActions(m: Map<string, ActionHandler>, ctx: CommonActionContext): void {
  if (ctx.tab !== "pcb") return;
  const { api, dispatch } = ctx;
  const groups = () => api.getState().board?.drawings?.groups ?? [];

  // ACTIONS::groupProperties -- GROUP_TOOL::GroupProperties( group ): the dialog on that group (the event parameter), else the one group that is selected.
  m.set("common.Groups.groupProperties", (arg) => {
    const selected = [...api.getState().selection];
    const id = typeof arg === "string" ? arg : selected.length === 1 ? selected[0]! : null;
    const group = groups().find((g) => g.id === id);
    if (group) openGroupDialog({ id: group.id, name: group.name, members: [...group.member_ids] });
  });

  // ACTIONS::pickNewGroupMember -- PCB_GROUP_TOOL::PickNewMember: the dialog is hidden, "Click on new member..." shows, the first click on an item adds it to
  // the dialog's list (not the group itself or one that holds it, and not one already there) and the dialog comes back; Escape brings it back unchanged. Needs the dialog open.
  m.set("common.Groups.selectNewGroupMember", () => {
    const dlg = getCommonDialogs().group;
    if (!dlg) return;
    updateGroupDialog({ hidden: true });
    picker.start({
      kind: "item",
      prompt: "Click on new member...",
      onItem: (id) => {
        const member = newMemberFromPick(id, groups(), dlg.id);
        if (!member) return true; // "still looking for an item"
        const now = getCommonDialogs().group;
        updateGroupDialog({ members: addMember(now?.members ?? dlg.members, member, dlg.id), hidden: false });
        return false;
      },
      onCancel: () => updateGroupDialog({ hidden: false }),
    });
  });

  // ACTIONS::addToGroup ("Add Items") -- GROUP_TOOL::AddToGroup: with one group and some ungrouped items selected, the items join the group, and the
  // group alone is selected afterwards ("Add Items to Group" is one undo step).
  m.set("common.Interactive.addToGroup", () => {
    const plan = planAddToGroup([...api.getState().selection], groups());
    if (!plan) return;
    void api.cmd({ op: "add_to_group", group_id: plan.groupId, ids: plan.ids }).then((ok) => {
      if (ok) dispatch({ type: "SET_SELECTION", refs: [plan.groupId], raw: true });
    });
  });

  // ACTIONS::removeFromGroup ("Remove Items") -- GROUP_TOOL::RemoveFromGroup: every selected item that is a group member leaves its group; a group that
  // is left with fewer than two members is dissolved ("Remove Group Items" is one undo step). Members are selectable one by one inside an entered group.
  m.set("common.Interactive.removeFromGroup", () => {
    const ids = planRemoveFromGroup([...api.getState().selection], groups());
    if (ids.length > 0) void api.cmd({ op: "remove_from_group", ids });
  });
}
