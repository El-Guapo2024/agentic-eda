// Board Setup > Board Stackup > Board Finish -- port of KiCad 25ec2335 pcbnew/board_stackup_manager/panel_board_finish.cpp (`PANEL_SETUP_BOARD_FINISH`:
// `synchronizeWithBoard`, `TransferDataFromWindow`, `ImportSettingsFrom`), its own page after Physical Stackup (`DIALOG_BOARD_SETUP`'s `m_boardFinishPage`):
// "Plated board edge", "Copper finish:" (`GetStandardCopperFinishes`: "Not specified", the `.gbrjob` names, "User defined" last; a finish the list does not
// know shows as "User defined", and is written so once the page is applied) and "Edge card connectors:" (None, Yes, Yes, bevelled). The finish is part of the
// board's stackup (`BOARD_STACKUP::m_FinishType`, `m_EdgePlating`, `m_EdgeConnectorConstraints`), so Apply sends the board's stackup as it is with the page's
// three values: one undoable `set_stackup` (UI-LEFT-3 B3-8). "Import Settings from Another Board..." brings it with the layers, as KiCad's does.
import { useEffect, useMemo } from "react";
import { useStudioState } from "../../state/store";
import { defaultStackup, withKnownKinds } from "../../kicad-port/boardSetupRules";
import { COPPER_FINISH_CHOICES, EDGE_CONNECTOR_CHOICES, finishDraftOf, withFinish, type FinishDraft } from "../../kicad-port/boardFinish";
import { Check, FieldRow, PageFrame, usePageApply, usePageDraft, type PageProps, useSetupImport } from "./fields";

export function BoardFinishPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const board = state.board;
  const rules = board?.board_rules;
  const copper = rules?.copper_layers ?? rules?.layer_names?.length ?? board?.layers.length ?? 2;
  const stackup = useMemo(() => (rules ? withKnownKinds(rules.stackup ?? defaultStackup(copper, rules.board_thickness_nm ?? 1_600_000)) : undefined), [rules, copper]);
  const source = useMemo(() => (stackup ? finishDraftOf(stackup) : undefined), [stackup]);
  const page = usePageDraft<FinishDraft>(source);
  // "Import Settings from Another Board..." > "Board layers and physical stackup" (`m_boardFinish->ImportSettingsFrom( other )`)
  useSetupImport("layers", (b) => finishDraftOf(b.board_rules?.stackup ?? undefined), page.importDraft);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const d = page.draft;
  const set = (patch: Partial<FinishDraft>) => page.edit((x) => ({ ...x, ...patch }));

  const onApply = () => {
    if (!d || !stackup || !page.dirty) return;
    void apply.run({ op: "set_stackup", settings: { copper_layers: copper, stackup: withFinish(stackup, d) } }, page.markClean);
  };

  return (
    <PageFrame title="Board Finish" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!d ? (
        <p className="bs-note">The board is not loaded.</p>
      ) : (
        <>
          <Check checked={d.plated} onChange={(v) => set({ plated: v })}>
            Plated board edge
          </Check>
          <FieldRow label="Copper finish:">
            <select aria-label="Copper finish" value={d.finish} onChange={(e) => set({ finish: e.target.value })}>
              {COPPER_FINISH_CHOICES.map((f) => (
                <option key={f} value={f}>
                  {f}
                </option>
              ))}
            </select>
          </FieldRow>
          <FieldRow label="Edge card connectors:" title="Options for edge card connectors.">
            <select aria-label="Edge card connectors" value={d.edge} onChange={(e) => set({ edge: Number(e.target.value) })}>
              {EDGE_CONNECTOR_CHOICES.map((label, i) => (
                <option key={label} value={i}>
                  {label}
                </option>
              ))}
            </select>
          </FieldRow>
        </>
      )}
    </PageFrame>
  );
}
