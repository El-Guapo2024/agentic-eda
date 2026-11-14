// Board Setup > Board Stackup > Board Finish: the pure parts of KiCad 25ec2335 pcbnew/board_stackup_manager/panel_board_finish.cpp (`synchronizeWithBoard`,
// `TransferDataFromWindow( BOARD_STACKUP& )`) and stackup_predefined_prms.cpp (`copperFinishType[]`). The page is components/boardSetup/BoardFinishPage.tsx
// (UI-LEFT-3 B3-8).
import type { Stackup } from "../api/types";

/** `copperFinishType[]` (stackup_predefined_prms.cpp): `NotSpecifiedPrm()`, the names a `.gbrjob` file knows, and "User defined" kept at the end. */
export const COPPER_FINISH_CHOICES = ["Not specified", "ENIG", "ENEPIG", "HAL SnPb", "HAL lead-free", "Hard gold", "Immersion tin", "Immersion nickel", "Immersion silver", "Immersion gold", "HT_OSP", "OSP", "None", "User defined"];
/** `m_choiceEdgeConnChoices`. */
export const EDGE_CONNECTOR_CHOICES = ["None", "Yes", "Yes, bevelled"];

export interface FinishDraft {
  /** An entry of [`COPPER_FINISH_CHOICES`]. */
  finish: string;
  plated: boolean;
  /** `BS_EDGE_CONNECTOR_CONSTRAINTS`: 0 none, 1 yes, 2 bevelled. */
  edge: number;
}

/** `synchronizeWithBoard`: the stackup's finish as a choice of the list ("Not specified" for none, "User defined" for a name the list lacks). */
export function finishDraftOf(s: Pick<Stackup, "copper_finish" | "edge_plating" | "edge_connector"> | undefined): FinishDraft {
  const f = s?.copper_finish;
  const finish = f === undefined || f === "" ? "Not specified" : COPPER_FINISH_CHOICES.includes(f) ? f : "User defined";
  return { finish, plated: s?.edge_plating ?? false, edge: s?.edge_connector ?? 0 };
}

/** `TransferDataFromWindow( BOARD_STACKUP& )`: the stackup with the page's finish, plating and edge connectors ("Not specified" is no finish here). */
export function withFinish(s: Stackup, d: FinishDraft): Stackup {
  const out: Stackup = { ...s, copper_finish: d.finish === "Not specified" ? undefined : d.finish, edge_plating: d.plated, edge_connector: d.edge };
  if (out.copper_finish === undefined) delete out.copper_finish;
  return out;
}

