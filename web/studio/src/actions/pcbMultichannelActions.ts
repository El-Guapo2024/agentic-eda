// The multichannel tool's two menu actions (Tools > Multi-Channel; GAPS.md items 24 and 32, package P24), at 8303b2ad:
//
//   pcbnew.Multichannel.generatePlacementRuleAreas   MULTICHANNEL_TOOL::AutogenerateRuleAreas: GeneratePotentialRuleAreas lists the sheets, component classes and groups of the
//                                                    board; with a single row or none the tool says there is nothing to generate, else DIALOG_MULTICHANNEL_GENERATE_RULE_AREAS
//   pcbnew.Multichannel.repeatLayout                 MULTICHANNEL_TOOL::repeatLayout: the reference rule area is the one placement rule area selected (or in a selected group), else
//                                                    the picker asks for one ("Select a reference Rule Area to copy from..."); CheckRACompatibility matches it against the others,
//                                                    then DIALOG_MULTICHANNEL_REPEAT_LAYOUT
//
// Apply Design Block Layout (`pcbnew.InteractiveDrawing.applyDesignBlockLayout`) is in designBlockActions.ts, beside the other actions of a linked group. What the tool does is the
// server's (crates/ops/src/multichannel*.rs, `/api/multichannel/*`), one undoable command; the dialogs are components/MultichannelDialogs.tsx.
import { fetchPotentialAreas, fetchRepeatPlan } from "../api/multichannelClient";
import { NO_TARGET_AREAS, NOTHING_TO_GENERATE, PICK_REFERENCE_PROMPT, isPlacementRuleArea, referenceAreaOf, type GenerateRow } from "../kicad-port/multichannel";
import { openGenerateDialog, openRepeatDialog } from "../state/multichannelDialogs";
import { pickItem } from "./pcbPicker";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";

export function registerPcbMultichannelActions(m: Map<string, (param?: unknown) => void>, ctx: SweepCtx): void {
  const { pcbOnly, toast } = createSweepHelpers(ctx);
  const { api, dispatch } = ctx;

  /** `AutogenerateRuleAreas` up to the dialog. */
  const generate = async () => {
    let reply;
    try {
      reply = await fetchPotentialAreas();
    } catch (e) {
      return toast(e instanceof Error ? e.message : String(e), "error");
    }
    if (!reply.ok) return toast(reply.message, "error");
    // `if( m_areas.m_areas.size() <= 1 )`
    if (reply.areas.length <= 1) return toast(NOTHING_TO_GENERATE, "error");
    const rows: GenerateRow[] = reply.areas.map((a) => ({ kind: a.kind, key: a.key, file: a.file, components: a.components, generate: false }));
    openGenerateDialog({ rows });
  };

  /** `repeatLayout` up to the dialog; `reference` is the rule area the picker was asked for (the tool runs again when it answers). */
  const repeat = async (picked?: string) => {
    const st = api.getState();
    const zones = st.board?.routing?.zones ?? [];
    const groups = st.board?.drawings?.groups ?? [];
    const reference = picked ?? referenceAreaOf(st.selection, zones, groups);
    if (!reference) {
      const id = await pickItem(PICK_REFERENCE_PROMPT, (candidate) => isPlacementRuleArea(api.getState().board?.routing?.zones.find((z) => z.id === candidate)));
      if (!id) return;
      dispatch({ type: "SET_SELECTION", refs: [id] });
      return void repeat(id);
    }
    let plan;
    try {
      plan = await fetchRepeatPlan(reference);
    } catch (e) {
      return toast(e instanceof Error ? e.message : String(e), "error");
    }
    if (!plan.ok) return toast(plan.message, "error");
    // `if( m_areas.m_areas.size() <= 1 )`: the reference area is the only one
    if (plan.targets.length === 0) return toast(NO_TARGET_AREAS, "error");
    openRepeatDialog({ plan });
  };

  m.set("pcbnew.Multichannel.generatePlacementRuleAreas", pcbOnly(() => void generate()));
  m.set("pcbnew.Multichannel.repeatLayout", pcbOnly(() => void repeat()));
}
