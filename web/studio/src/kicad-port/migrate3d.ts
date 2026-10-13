// Migrate 3D Models (`pcbnew.GlobalEdit.migrate3DModels`, `DIALOG_MIGRATE_3D_MODELS`, pcbnew/dialogs/dialog_migrate_3d_models.cpp): the `.wrl` and `.wrz` models the board's footprints name
// that no search finds, a list of STEP files that could stand for each, and what Replace sends. The server finds and ranks (`GET /api/3dmodel/migrate`, crates/cli/src/model3d_tab_api.rs and
// crates/model/src/model_subst.rs: `collectMissingModels`, `buildCatalog`, `rankCandidatesFor`); this module is the dialog's own state -- which candidate each row has chosen, the
// file picked by hand ("Open External File..."), the folders added ("Add Search Directory...") -- and the command.
import type { Cmd } from "../api/types";
import { plainRow, type ModelRow } from "./fpModels";

/** One replacement offered for a missing model: the path to write in the footprint, what the list shows, and its points (`MATCH_CANDIDATE`). */
export interface MigrateCandidate {
  path: string;
  display: string;
  score: number;
}

/** A model the board names that is not there (`m_missing`, with `m_missingRepFp` and `m_missingXform`). */
export interface MissingModel {
  name: string;
  /** The footprints that have it, in board order. */
  footprints: string[];
  /** The first of them: the one the preview shows. */
  rep: string | null;
  /** That footprint's placement of the model, which a replacement keeps in the preview. */
  model: { name: string; offset: [number, number, number]; scale: [number, number, number]; rotate: [number, number, number]; opacity: number; show: boolean };
  candidates: MigrateCandidate[];
}

export interface Migration {
  missing: MissingModel[];
  /** How many STEP-family files the folders held. */
  scanned: number;
  dirs: string[];
}

/** The score `OnOpenExternalFileClick` gives a file picked by hand: above every ranked one. */
export const USER_SELECTED_SCORE = 2000;

/** "Auto-select the top real (non-"keep existing") candidate for every missing row": `-1` is no choice (`(keep existing reference)`). */
export function defaultChoices(missing: readonly MissingModel[]): number[] {
  return missing.map((m) => (m.candidates.length > 0 ? 0 : -1));
}

/** `OnReplaceClick`: the old name -> the new for each row that has a candidate chosen. */
export function replacementsOf(missing: readonly MissingModel[], choices: readonly number[]): Record<string, string> {
  const out: Record<string, string> = {};
  missing.forEach((m, i) => {
    const c = m.candidates[choices[i] ?? -1];
    if (c) out[m.name] = c.path;
  });
  return out;
}

/** What Replace sends: one `migrate_board_models` for the rows that chose a candidate, and nothing when none did ("User clicked Replace without choosing anything; treat as Keep"). */
export function migrateCmds(missing: readonly MissingModel[], choices: readonly number[]): Cmd[] {
  const replacements = replacementsOf(missing, choices);
  return Object.keys(replacements).length === 0 ? [] : [{ op: "migrate_board_models", replacements }];
}

/** `OnOpenExternalFileClick`: a file picked by hand becomes the row's first candidate, user-selected, chosen -- or, when the row has it already, that one is chosen. */
export function withPickedFile(m: MissingModel, path: string, display: string): { model: MissingModel; choice: number } {
  const at = m.candidates.findIndex((c) => c.path === path);
  if (at >= 0) return { model: m, choice: at };
  return { model: { ...m, candidates: [{ path, display, score: USER_SELECTED_SCORE }, ...m.candidates] }, choice: 0 };
}

/** The display of a file picked by hand: its name, a dash and "user-selected". */
export function userSelectedDisplay(path: string): string {
  return `${path.split("/").pop() ?? path}   —   user-selected`;
}

/**
 * After the folders searched changed (`OnAddSearchDirectoryClick`): the rows are ranked again against the bigger catalog, and each keeps the choice it had when that same file is still among its
 * candidates; a row with none, or whose choice went, takes the new best (so a folder added "immediately un-bolds rows that just acquired a plausible match"), and a row with no candidate has none.
 */
export function rechoose(before: readonly MissingModel[], choices: readonly number[], after: readonly MissingModel[]): number[] {
  return after.map((m) => {
    const old = before.find((b) => b.name === m.name);
    const was = old ? old.candidates[choices[before.indexOf(old)] ?? -1]?.path : undefined;
    const kept = was ? m.candidates.findIndex((c) => c.path === was) : -1;
    return kept >= 0 ? kept : m.candidates.length > 0 ? 0 : -1;
  });
}

/** The preview's row for a candidate: it sits as the original did (`replacement.m_Scale = m_missingXform...`), shown. */
export function previewRow(m: MissingModel, candidate: MigrateCandidate | undefined): ModelRow | null {
  if (!candidate) return null;
  return { ...plainRow(candidate.path), offset: [...m.model.offset], scale: [...m.model.scale], rotate: [...m.model.rotate], opacity: m.model.opacity, show: true };
}

/** `m_Extra3DSearchDirs`, kept between sessions (the studio keeps them in the browser, a convenience). */
const DIRS_KEY = "eda-studio.model3d-search-dirs";

export function savedSearchDirs(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(DIRS_KEY) ?? "[]") as unknown;
    return Array.isArray(raw) ? raw.filter((d): d is string => typeof d === "string" && d !== "") : [];
  } catch {
    return [];
  }
}

export function saveSearchDirs(dirs: readonly string[]): void {
  try {
    localStorage.setItem(DIRS_KEY, JSON.stringify([...dirs]));
  } catch {
    /* it is a convenience */
  }
}

/** The folder added, once: `if( std::find( dirs.begin(), dirs.end(), chosen ) == dirs.end() ) dirs.push_back( chosen )`. */
export function withSearchDir(dirs: readonly string[], dir: string): string[] {
  return dirs.includes(dir) ? [...dirs] : [...dirs, dir];
}
