// The group tool's membership edits: Add Items, Remove Items and the member list of Group Properties.
//
//   common/tool/group_tool.cpp                GROUP_TOOL::AddToGroup / RemoveFromGroup and GROUP_CONTEXT_MENU::update (when each is offered)
//   common/dialogs/dialog_group_properties.cpp DIALOG_GROUP_PROPERTIES::DoAddMember / OnRemoveMember / TransferDataFromWindow
//   pcbnew/tools/pcb_group_tool.cpp           PCB_GROUP_TOOL::PickNewMember

import type { Cmd } from "../api/types";
import { groupHolds } from "./groupTree";

export interface GroupLike {
  id: string;
  member_ids: readonly string[];
}

const inAGroup = (id: string, groups: readonly GroupLike[]) => groups.some((g) => g.member_ids.includes(id));

/**
 * `GROUP_TOOL::AddToGroup`: the selection must hold exactly one group ("Only allow one group to be selected for adding to existing
 * group") and at least one item that is in no group; those items join that group. Null when it does not apply -- which is also
 * `GROUP_CONTEXT_MENU::update`'s `onlyOneGroup && hasUngroupedItems` for enabling the entry.
 */
export function planAddToGroup(selection: readonly string[], groups: readonly GroupLike[]): { groupId: string; ids: string[] } | null {
  let groupId: string | null = null;
  const ids: string[] = [];
  for (const id of selection) {
    if (groups.some((g) => g.id === id)) {
      if (groupId !== null) return null;
      groupId = id;
    } else if (!inAGroup(id, groups)) ids.push(id);
  }
  return groupId !== null && ids.length > 0 ? { groupId, ids } : null;
}

/**
 * `GROUP_TOOL::RemoveFromGroup`: every selected item that is a member of a group leaves it (a group left with fewer than two members
 * dissolves -- the verb does that). Empty when no selected item is a member, which is `hasMember` being false in the menu.
 */
export function planRemoveFromGroup(selection: readonly string[], groups: readonly GroupLike[]): string[] {
  return selection.filter((id) => inAGroup(id, groups));
}

/**
 * `DIALOG_GROUP_PROPERTIES::DoAddMember`: the picked item joins the list unless it is already there or is the group itself. (An item
 * that is in another group is taken from it when the dialog is accepted -- `EditGroup` does that.)
 */
export function addMember(members: readonly string[], id: string, groupId: string): string[] {
  if (id === groupId || members.includes(id)) return [...members];
  return [...members, id];
}

/** `OnRemoveMember`: the selected row leaves the list. */
export function removeMemberAt(members: readonly string[], index: number): string[] {
  return index >= 0 && index < members.length ? members.filter((_, i) => i !== index) : [...members];
}

/**
 * The item a click on a new member answers with (`PickNewMember`'s click handler): the picked item, promoted past a group that was not entered the way
 * every pick is. A group may be a member of another (groups nest), but not of itself or of a group below it -- that would be a loop -- so such a pick is
 * refused (null keeps picking).
 */
export function newMemberFromPick(pickedId: string, groups: readonly GroupLike[], groupId?: string): string | null {
  if (groupId !== undefined && groups.some((g) => g.id === pickedId) && groupHolds(groups, pickedId, groupId)) return null;
  return pickedId;
}

/**
 * `BOARD_COMMIT::Push`: with a group entered (`PCB_SELECTION_TOOL::GetEnteredGroup()`), whatever a command adds that is in no group yet joins it
 * (`if( enteredGroup && boardItem->IsGroupableType() && !boardItem->GetParentGroup() ) enteredGroup->AddItem( boardItem )`; `PCB_CONTROL::placeBoardItems`
 * does the same early for a paste). The group is the editor's state, not the design's, so the command is sent wrapped in `in_group` and the backend does the joining
 * (`Cmd::InGroup`, crates/ops/src/pcb_groups.rs). Without an entered group, or one the board no longer has, the command is sent as it is.
 */
export function inEnteredGroup(cmd: Cmd, entered: string | null | undefined, groups: readonly GroupLike[] | undefined): Cmd {
  if (!entered || !groups?.some((g) => g.id === entered)) return cmd;
  if (cmd.op === "in_group" || cmd.op === "on_sheet") return cmd;
  return { op: "in_group", group: entered, cmd };
}
