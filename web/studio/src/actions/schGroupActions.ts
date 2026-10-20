// The group tool on the schematic: Group, Ungroup, Add Items, Remove Items, Group Properties, Enter Group and Leave Group -- `SCH_GROUP_TOOL::Group`
// (eeschema/tools/sch_group_tool.cpp at 8303b2ad) and the `GROUP_TOOL` actions of common/tool/group_tool.cpp, which the board runs too (actions/commonGroupActions.ts,
// useActionRunner.ts). The work is the `sch_edit` verbs `group`, `ungroup`, `add_to_group`, `remove_from_group` and `edit_group` (crates/ops/src/sch_groups.rs), one undo
// step each; this file is the conditions the tools put on the selection before they run, and what is selected afterwards.
//
// The selection holds a group by its own id (a click on a member selects the group: kicad-port/schGroups.ts), so every handler below reads the groups of the sheet in
// view (`Schematic.groups`) and the group worked in is `StudioState.schEnteredGroupId`.
import { fetchSchematic } from "../api/client";
import { addMember, newMemberFromPick, planAddToGroup, planRemoveFromGroup } from "../kicad-port/groupEdit";
import { groupsOf, ownerOf, planGroup, substituteSchSelection } from "../kicad-port/schGroups";
import { pickItem } from "../components/schematic/schPick";
import { schSelectable } from "../kicad-port/schSelectionFilter";
import { getCommonDialogs, openGroupDialog, updateGroupDialog } from "../state/commonDialogs";
import { picker } from "./pcbPicker";
import { schematicActions, type ActionMap } from "./schActionRegistry";
import type { SchEditContext } from "./schEditActions";

export function registerSchGroupActions(registry: ActionMap, ctx: SchEditContext): void {
  const { state, api, dispatch } = ctx;
  const m = schematicActions(registry, state.tab);
  /** The sheet as it is now (a handler that waits for a command reads what the command made, not what the registry was built with). */
  const now = () => api.getState().schematic;

  // ACTIONS::group -- SCH_GROUP_TOOL::Group: the selection without what belongs to a symbol and what cannot be grouped, at least two; the new group is what is selected afterwards
  // (`RunAction( ACTIONS::selectItem, group )`).
  m.set("common.Interactive.group", () => {
    const sch = state.schematic;
    if (!sch) return;
    const ids = planGroup(sch, ctx.requestSelection());
    if (!ids) return;
    const before = new Set(groupsOf(sch).map((g) => g.id));
    void api.cmd({ op: "sch_edit", verb: "group", ids }).then(async (ok) => {
      if (!ok) return;
      // the sheet as the backend has it now (the state only catches up on the next render)
      const after = await fetchSchematic(state.currentSheetPath).catch(() => null);
      const made = groupsOf(after).find((g) => !before.has(g.id));
      if (made) dispatch({ type: "SET_SELECTION", refs: [made.id], raw: true });
    });
  });

  // ACTIONS::ungroup -- GROUP_TOOL::Ungroup: every selected group dissolves and its members are what is selected (and, with nothing selected, the group under the cursor).
  m.set("common.Interactive.ungroup", () => {
    const sch = state.schematic;
    if (!sch) return;
    const groups = groupsOf(sch);
    const selected = ctx.requestSelection().filter((id) => groups.some((g) => g.id === id));
    if (selected.length === 0) return;
    const members = selected.flatMap((id) => groups.find((g) => g.id === id)?.member_ids ?? []);
    void api.cmd({ op: "sch_edit", verb: "ungroup", ids: selected }).then((ok) => {
      if (ok) dispatch({ type: "SET_SELECTION", refs: members, raw: true });
    });
  });

  // ACTIONS::groupEnter -- GROUP_TOOL::EnterGroup: only for one selected group (`selection.GetSize() == 1 && selection[0]->Type() == SCH_GROUP_T`); its members are selected on the way in.
  m.set("common.Interactive.groupEnter", () => {
    const sel = [...state.selection];
    if (sel.length === 1 && groupsOf(state.schematic).some((g) => g.id === sel[0])) dispatch({ type: "ENTER_SCH_GROUP", id: sel[0]! });
  });

  // ACTIONS::groupLeave -- GROUP_TOOL::LeaveGroup: `ExitGroup( true )`, the group is selected.
  m.set("common.Interactive.groupLeave", () => {
    const left = state.schEnteredGroupId;
    dispatch({ type: "SET_SCH_ENTERED_GROUP", id: null });
    if (left) dispatch({ type: "SET_SELECTION", refs: [left], raw: true });
  });

  // ACTIONS::addToGroup ("Add Items") -- GROUP_TOOL::AddToGroup: one group and some items that are in no group are selected; the items join the group and the group alone is selected.
  m.set("common.Interactive.addToGroup", () => {
    const sch = state.schematic;
    if (!sch) return;
    const groups = groupsOf(sch);
    // an item in a group is a member by its symbol (`GetParentGroup`), so a pin or a field is as grouped as the symbol it belongs to
    const selection = ctx.requestSelection().map((id) => ownerOf(sch, id));
    const plan = planAddToGroup(selection, groups);
    if (!plan) return;
    void api.cmd({ op: "sch_edit", verb: "add_to_group", group_id: plan.groupId, ids: plan.ids }).then((ok) => {
      if (ok) dispatch({ type: "SET_SELECTION", refs: [plan.groupId], raw: true });
    });
  });

  // ACTIONS::removeFromGroup ("Remove Items") -- GROUP_TOOL::RemoveFromGroup: every selected member leaves its group; a group left with fewer than two dissolves.
  m.set("common.Interactive.removeFromGroup", () => {
    const sch = state.schematic;
    if (!sch) return;
    const ids = planRemoveFromGroup(ctx.requestSelection().map((id) => ownerOf(sch, id)), groupsOf(sch));
    if (ids.length > 0) void api.cmd({ op: "sch_edit", verb: "remove_from_group", ids });
  });

  // ACTIONS::groupProperties -- GROUP_TOOL::GroupProperties( group ): the dialog on that group (the event parameter), else the one group that is selected.
  m.set("common.Groups.groupProperties", (arg) => {
    const selected = [...state.selection];
    const id = typeof arg === "string" ? arg : selected.length === 1 ? selected[0]! : null;
    const group = groupsOf(state.schematic).find((g) => g.id === id);
    if (group) openGroupDialog({ id: group.id, name: group.name, members: [...group.member_ids], designBlock: group.design_block ?? "" });
  });

  // ACTIONS::pickNewGroupMember -- SCH_GROUP_TOOL::PickNewMember: the dialog is hidden, "Click on new member..." shows, the first click on an item adds it to the dialog's list (not the
  // group itself or one that holds it, and not one already there) and the dialog comes back; Escape brings it back unchanged. Needs the dialog open. The item is the click's
  // outermost one (a pin is its symbol: "while( elem->GetParent() && elem->GetParent()->Type() != SCH_SCREEN_T ) elem = elem->GetParent()") and, in a group, that group.
  m.set("common.Groups.selectNewGroupMember", () => {
    const dlg = getCommonDialogs().group;
    if (!dlg) return;
    updateGroupDialog({ hidden: true });
    picker.start({
      kind: "item",
      prompt: "Click on new member...",
      onItem: (id) => {
        const sch = now();
        const member = sch ? newMemberFromPick(id, groupsOf(sch), dlg.id) : null;
        if (!member) return true; // "still looking for an item"
        const current = getCommonDialogs().group;
        updateGroupDialog({ members: addMember(current?.members ?? dlg.members, member, dlg.id), hidden: false });
        return false;
      },
      onCancel: () => updateGroupDialog({ hidden: false }),
    });
  });
}

/**
 * The item a click at (`x`, `y`) hands the picker (`PickNewMember`'s click handler): what the selection tool would select there -- the outermost group of the item under the
 * point, else the item (a pin or a field is its symbol or sheet).
 */
export function pickNewMemberAt(sch: Parameters<typeof pickItem>[0], x: number, y: number, scale: number, filter: Parameters<typeof schSelectable>[1], entered: string | null): string | null {
  const hit = pickItem(sch, x, y, scale, schSelectable(sch, filter), "none");
  if (!hit) return null;
  const top = substituteSchSelection(sch, [hit], entered).ids[0] ?? hit;
  return ownerOf(sch, top);
}
