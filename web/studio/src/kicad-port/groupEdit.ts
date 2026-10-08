// The group tool's membership edits: Add Items, Remove Items and the member list of Group Properties.
//
//   common/tool/group_tool.cpp                GROUP_TOOL::AddToGroup / RemoveFromGroup and GROUP_CONTEXT_MENU::update (when each is offered)
//   common/dialogs/dialog_group_properties.cpp DIALOG_GROUP_PROPERTIES::DoAddMember / OnRemoveMember / TransferDataFromWindow
//   pcbnew/tools/pcb_group_tool.cpp           PCB_GROUP_TOOL::PickNewMember

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
 * The item a click on a new member answers with (`PickNewMember`'s click handler): the picked item, promoted past a group that was
 * not entered the way every pick is -- but a group dialog cannot hold another group, so a group id is refused (null keeps picking).
 */
export function newMemberFromPick(pickedId: string, groups: readonly GroupLike[]): string | null {
  return groups.some((g) => g.id === pickedId) ? null : pickedId;
}
