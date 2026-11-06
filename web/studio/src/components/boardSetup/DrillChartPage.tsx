// Board Setup > Board Stackup > Drill Chart -- port of KiCad 25ec2335's pcbnew/dialogs/panel_setup_drill_chart.cpp (labels from panel_setup_drill_chart_base.cpp): the board's
// drill symbol profile (`DRILL_SYMBOL_PROFILE`, `(setup (drill_symbol_profile ..))`), which every drill chart and drill map reads -- the keys that split the holes into groups
// (`g_groupKeyRows`), how a group with no mark gets one (Default marks), the size and line width of the marks, and whether the marks once given stay when the holes change.
// Apply is one `set_drill_profile`, which rebuilds every chart (`BumpDrillModelGeneration`). "Close and Edit Drill Groups..." applies the page (KiCad presses the dialog's OK,
// so every page is taken; here this page) and, when that was taken, closes Board Setup and opens Drill Groups (`PCB_ACTIONS::showDrillGroups`).
import { useEffect } from "react";
import { useStudioDispatch, useStudioState } from "../../state/store";
import type { BoardDrillProfile } from "../../api/pcbItemTypes";
import { GROUP_KEY_ROWS, MARK_POLICY_CHOICES } from "../../kicad-port/drillChartDialog";
import { openDrillGroupsDialog } from "../../state/kicad10Dialogs";
import { Check, FieldRow, LengthField, PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

/** `DRILL_SYMBOL_PROFILE()`: what a board whose file names no profile has. */
const DEFAULT_PROFILE: BoardDrillProfile = { group_by: ["size", "slot", "plating", "span", "operation", "post_machining"], mark_policy: "shapes_then_letters", size_nm: 1_500_000, width_nm: 150_000, freeze_assignments: true };

export function DrillChartPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const units = state.units;
  const source = state.board ? (state.board.drawings?.drill_profile ?? DEFAULT_PROFILE) : undefined;
  const page = usePageDraft<BoardDrillProfile>(source);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;
  const set = <K extends keyof BoardDrillProfile>(key: K, value: BoardDrillProfile[K]) => page.edit((d) => ({ ...d, [key]: value }));

  // `TransferDataFromWindow` checks nothing: the keys in the checklist's order, the policy, the two lengths and the freeze.
  const send = async (): Promise<boolean> => {
    if (!draft) return false;
    const same = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every((k) => b.includes(k));
    // Nothing that differs from the board: nothing to send (the verb refuses an edit that changes nothing).
    if (!page.dirty || (source && same(draft.group_by, source.group_by) && draft.mark_policy === source.mark_policy && draft.size_nm === source.size_nm && draft.width_nm === source.width_nm && draft.freeze_assignments === source.freeze_assignments)) {
      page.markClean();
      return true;
    }
    const size = Math.round(draft.size_nm);
    const width = Math.round(draft.width_nm);
    if (!Number.isFinite(size) || !Number.isFinite(width)) {
      apply.setMessage({ text: "The symbol size and line width are lengths.", bad: true });
      return false;
    }
    return apply.run(
      {
        op: "pcb_item",
        verb: "set_drill_profile",
        profile: { group_by: GROUP_KEY_ROWS.map(([k]) => k).filter((k) => draft.group_by.includes(k)), mark_policy: draft.mark_policy, size_nm: size, width_nm: width, freeze_assignments: draft.freeze_assignments },
      },
      page.markClean
    );
  };
  const editGroups = async () => {
    if (!(await send())) return;
    dispatch({ type: "SET_BOARD_SETUP_DIALOG_OPEN", open: false });
    openDrillGroupsDialog();
  };

  return (
    <PageFrame
      title="Drill Chart"
      hidden={hidden}
      dirty={page.dirty}
      busy={apply.busy}
      message={apply.message?.text ?? null}
      bad={apply.message?.bad}
      onApply={() => void send()}
      onRevert={() => (page.revert(), apply.setMessage(null))}
      extraActions={
        <button disabled={apply.busy} onClick={() => void editGroups()} data-testid="bs-edit-drill-groups">
          Close and Edit Drill Groups...
        </button>
      }
    >
      {!draft ? (
        <p className="bs-note">No board is open.</p>
      ) : (
        <>
          <div className="bs-section">Symbols</div>
          <FieldRow label="Group holes by:">
            <span style={{ display: "flex", flexDirection: "column" }}>
              {GROUP_KEY_ROWS.map(([k, label]) => (
                <Check key={k} checked={draft.group_by.includes(k)} onChange={(v) => set("group_by", v ? [...draft.group_by, k] : draft.group_by.filter((x) => x !== k))}>
                  {label}
                </Check>
              ))}
            </span>
          </FieldRow>
          <FieldRow label="Default marks:">
            <select className="bs-input" value={draft.mark_policy} onChange={(e) => set("mark_policy", e.target.value)} aria-label="Default marks">
              {MARK_POLICY_CHOICES.map(([p, label]) => (
                <option key={p} value={p}>
                  {label}
                </option>
              ))}
            </select>
          </FieldRow>
          <FieldRow label="Symbol size:" unit={units}>
            <LengthField ariaLabel="Symbol size" units={units} value={draft.size_nm} onChange={(v) => set("size_nm", v ?? Number.NaN)} />
          </FieldRow>
          <FieldRow label="Symbol line width:" unit={units}>
            <LengthField ariaLabel="Symbol line width" units={units} value={draft.width_nm} onChange={(v) => set("width_nm", v ?? Number.NaN)} />
          </FieldRow>
          <Check checked={draft.freeze_assignments} onChange={(v) => set("freeze_assignments", v)}>
            Keep existing symbol assignments when holes change
          </Check>
        </>
      )}
    </PageFrame>
  );
}
