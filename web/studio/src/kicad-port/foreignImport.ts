// File > Import > Non-KiCad Board File... and Non-KiCad Schematic File...: the pure half. Unit-tested in foreignImport.test.ts.
//
// Ported from:
//   pcbnew/files.cpp                       BOARD_EDITOR_CONTROL::OpenNonKicadBoard, AskLoadBoardFileName (the file chooser and its filters), PCB_EDIT_FRAME::OpenProjectFiles (what
//                                          happens to the board that is open: it is replaced; "Keep KiCad layer names" gives every layer its standard name back)
//   eeschema/files-io.cpp                  SCH_EDIT_FRAME::OnImportProject (the schematic that is open is replaced) and importFile
//   pcbnew/dialogs/dialog_map_layers.cpp   DIALOG_MAP_LAYERS (Unmatched Layers / Matched Layers, > < << and Auto-Match Layers; a layer marked * must be matched)
//   common/lset.cpp                        LSET::UIOrder (the order the KiCad Layers list shows the layers in)
//
// The converting is kicad-cli's (`POST /api/import_foreign`, crates/cli/src/foreign_api.rs): the file is posted as bytes, the server answers with the KiCad file, what the importer
// said, and for a board where each source layer went. kicad-cli answers the importers' layer callback itself, with the plugin's best guess, and takes a map of the layers that
// should go elsewhere (`--layer-map`): a layer the map does not name keeps the guess. So the dialog starts as KiCad's does after Auto-Match Layers -- the guesses are the Matched
// Layers -- and OK sends the layers the person moved or added, in a second run, and the layers left in the Unmatched list, which KiCad's callback answers with `UNDEFINED_LAYER`
// so that the importer leaves out what is on them: the server leaves them out (`drop_layers`; kicad-cli's map cannot say it, so they go to a user layer nothing else goes to,
// which is taken out of the board kicad-cli wrote: crates/kicad/src/drop_layer.rs).

import { standardLayerName } from "./boardLayers";

export type ImportKind = "board" | "schematic";

/** What the route is asked to convert: a board, a schematic, or a project's files together (a board and its schematic). */
export type ConvertKind = ImportKind | "project";

/** One thing kicad-cli's import said: an error, a warning, or (not shown) information; `count` is how often it was said. */
export interface ImportFinding {
  severity: "error" | "warning" | "info";
  message: string;
  count: number;
}

/** A source layer of the file and where kicad-cli put it (`null`: it left the layer out); `explicit` when a layer map named it. */
export interface SourceLayer {
  source: string;
  kicad: string | null;
  explicit: boolean;
}

/** `POST /api/import_foreign`'s reply. */
export interface ForeignReply {
  ok: boolean;
  message?: string;
  /** No importer of kicad-cli takes the file (as against one that failed on it). */
  unrecognized?: boolean;
  kind?: ConvertKind;
  name?: string;
  engine?: string;
  /** The format kicad-cli recognised: `Eagle`, `Altium Designer`, ... */
  format?: string;
  statistics?: Record<string, number>;
  errors?: number;
  warnings?: number;
  findings?: ImportFinding[];
  layers?: SourceLayer[];
  /** The importer asks which KiCad layer each source layer goes to (`LAYER_MAPPABLE_PLUGIN`). */
  maps_layers?: boolean;
  counts?: Record<string, number>;
  /** A board: the `.kicad_pcb`. */
  text?: string;
  /** A board: the `.kicad_pro` kicad-cli made with it -- the design rules and the net classes the importer set, which the board file does not hold. `null` when it could not be made. */
  project?: string | null;
  /** A board: the custom DRC rules (`.kicad_dru`) its importer wrote (Eagle's class clearances, PADS's diff-pair gaps, EasyEDA Pro's spacing rules). `null` for none. */
  rules?: string | null;
  /** A schematic, or a project's schematic: the sheet file (`eda_model::sheet_file::SheetFile`). */
  content?: unknown;
  notes?: string[];
  /** A project file of several boards was given with no board chosen: `boards` names them, in kicad-cli's order, and nothing was imported (`DIALOG_IMPORT_CHOOSE_PROJECT`). */
  choose_board?: boolean;
  boards?: string[];
  /** A project: what the schematic the project command made holds, and what its reader noted. */
  schematic_counts?: Record<string, number>;
  schematic_notes?: string[];
}

/** A file the page has read as bytes: its name and its content as base64. */
export interface ForeignFile {
  name: string;
  data: string;
  /** kicad-cli's `--format` (`altium`, `eagle`, ...); absent is `auto`. */
  format?: string;
}

/**
 * `AskLoadBoardFileName`'s filters: the extensions of the formats `kicad-cli pcb import` and `sch import` take, as a file chooser's `accept`. A browser takes an extension in any case
 * of letters, but not in every browser, so the mixed-case ones are listed as the programs write them.
 */
export const BOARD_ACCEPT = ".brd,.PcbDoc,.pcbdoc,.cpa,.asc,.pcb,.PCB,.txt,.json,.zip,.epro,.SwPcbDoc,.CMPcbDoc,.CSPcbDoc,.efoo";
export const SCHEMATIC_ACCEPT = ".sch,.SchDoc,.schdoc,.csa,.json,.zip,.epro,.asc,.dsn,.dch";

/** The project import's chooser: the boards' and the schematics' files of every format, chosen together (an Eagle `.brd` and its `.sch`). */
export const PROJECT_ACCEPT = ".brd,.sch,.cpa,.csa,.json,.zip,.epro,.PcbDoc,.pcbdoc,.SchDoc,.schdoc,.asc,.txt,.pcb,.PCB,.dsn,.dch";

export function acceptFor(kind: ConvertKind): string {
  return kind === "board" ? BOARD_ACCEPT : kind === "project" ? PROJECT_ACCEPT : SCHEMATIC_ACCEPT;
}

/** The parameter of the import actions: the file the page read (`window.__eda.run(id, { name, data })`; the chooser's own file has the same shape), and what to do with the layers. */
export interface ForeignParam extends ForeignFile {
  /** Source layer -> KiCad layer: answers the layer dialog without opening it. */
  layerMap?: Record<string, string>;
  /** Source layers to leave out with what is on them (the dialog's Unmatched list at OK), with `layerMap`. */
  dropLayers?: string[];
  /** Do not ask which layer each source layer goes to: kicad-cli's guess is used (`ADVANCED_CFG::m_ImportSkipLayerMapping`). */
  skipLayerMapping?: boolean;
  /** "Keep KiCad layer names". */
  keepKicadLayerNames?: boolean;
  /** Do not open the report of the import's errors and warnings ("Show import issues" off). */
  skipReport?: boolean;
  /** The board of a project file of several (its place in the list kicad-cli gives): answers `DIALOG_IMPORT_CHOOSE_PROJECT` without opening it. */
  board?: number;
}

export function asForeignParam(param: unknown): ForeignParam | null {
  if (!param || typeof param !== "object") return null;
  const p = param as Partial<ForeignParam>;
  if (typeof p.name !== "string" || typeof p.data !== "string") return null;
  return p as ForeignParam;
}

/** "3 footprints, 12 tracks": the non-zero counts of `kinds`, singular or plural. */
function listCounts(counts: Record<string, number> | undefined, kinds: Array<[string, string, string]>): string {
  const parts = kinds.filter(([k]) => (counts?.[k] ?? 0) > 0).map(([k, one, many]) => `${counts![k]} ${counts![k] === 1 ? one : many}`);
  return parts.length > 0 ? parts.join(", ") : "nothing";
}

/** What an import brought, in a sentence: "Imported x.brd (Eagle): 19 footprints, 6 nets, 82 tracks, 11 vias, 2 zones." -- and for a project, the schematic's too. */
export function describeImport(reply: ForeignReply): string {
  if (reply.kind === "project") {
    const board = describeImport({ ...reply, kind: "board" }).replace(/\.$/, "");
    const sch = listCounts(reply.schematic_counts, [
      ["symbols", "symbol", "symbols"],
      ["sheets", "sheet", "sheets"],
      ["wires", "wire", "wires"],
      ["labels", "label", "labels"],
    ]);
    return `${board}; schematic: ${sch}.`;
  }
  const what =
    reply.kind === "schematic"
      ? listCounts(reply.counts, [
          ["symbols", "symbol", "symbols"],
          ["sheets", "sheet", "sheets"],
          ["wires", "wire", "wires"],
          ["labels", "label", "labels"],
        ])
      : listCounts(reply.counts, [
          ["footprints", "footprint", "footprints"],
          ["nets", "net", "nets"],
          ["tracks", "track", "tracks"],
          ["vias", "via", "vias"],
          ["zones", "zone", "zones"],
        ]);
  const format = reply.format ? ` (${reply.format})` : "";
  return `Imported ${reply.name ?? "the file"}${format}: ${what}.`;
}

/** What is said when the board came without its rules: kicad-cli could not make the project that holds the importer's design rules and net classes (a kicad-cli without the command, a file its project import does not take). */
export function describeRules(reply: ForeignReply): string {
  return (reply.kind === "board" || reply.kind === "project") && reply.ok && !reply.project ? "The design rules and net classes of the file could not be read." : "";
}

/** "2 errors and 1 warning were reported." -- the counts of messages said, not of distinct ones; empty when kicad-cli said nothing. */
export function describeIssues(findings: ImportFinding[] | undefined): string {
  const total = (sev: ImportFinding["severity"]) => (findings ?? []).filter((f) => f.severity === sev).reduce((n, f) => n + f.count, 0);
  const [e, w] = [total("error"), total("warning")];
  const parts = [e > 0 ? `${e} error${e === 1 ? "" : "s"}` : "", w > 0 ? `${w} warning${w === 1 ? "" : "s"}` : ""].filter(Boolean);
  return parts.length === 0 ? "" : `${parts.join(" and ")} ${e + w === 1 ? "was" : "were"} reported.`;
}

/** The errors and the warnings, which is what the report dialog lists (an information line of the importers stays in kicad-cli's log). */
export function issuesOf(findings: ImportFinding[] | undefined): ImportFinding[] {
  return (findings ?? []).filter((f) => f.severity !== "info");
}

// ----------------------------------------------------------------------------------------------------------------- DIALOG_MAP_LAYERS

/** `LSET::TechAndUserUIOrder`: the non-copper layers in the order the dialog lists them (by the names the board file writes). */
const TECH_AND_USER_ORDER: readonly string[] = [
  "F.Adhes",
  "B.Adhes",
  "F.Paste",
  "B.Paste",
  "F.SilkS",
  "B.SilkS",
  "F.Mask",
  "B.Mask",
  "Dwgs.User",
  "Cmts.User",
  "Eco1.User",
  "Eco2.User",
  "Edge.Cuts",
  "Margin",
  "F.CrtYd",
  "B.CrtYd",
  "F.Fab",
  "B.Fab",
  ...Array.from({ length: 9 }, (_, i) => `User.${i + 1}`),
];

function copperRank(layer: string): number {
  if (layer === "F.Cu") return 0;
  if (layer === "B.Cu") return 1_000;
  const m = /^In(\d+)\.Cu$/.exec(layer);
  return m ? Number(m[1]) : -1;
}

/**
 * `LSET::UIOrder`: the KiCad layers the dialog's right-hand list offers -- the copper layers the board has (`F.Cu`, `In1.Cu` .., `B.Cu`: the ones the importer
 * mapped something to, and the two outer ones always), then the technical and user layers. The importers' own lists of layers a source layer may go to (`PermittedLayers`) are
 * not in kicad-cli's report, so every layer is offered for every source layer.
 */
export function kicadLayerList(layers: SourceLayer[]): string[] {
  const copper = new Set<string>(["F.Cu", "B.Cu"]);
  for (const l of layers) if (l.kicad && copperRank(l.kicad) >= 0) copper.add(l.kicad);
  return [...[...copper].sort((a, b) => copperRank(a) - copperRank(b)), ...TECH_AND_USER_ORDER];
}

/** What `LayerName` shows for a layer of the board file (`F.SilkS` is `F.Silkscreen`). */
export const kicadLayerLabel = standardLayerName;

/** A layer marked `*` in the dialog (`INPUT_LAYER_DESC::Required`): what the importer maps to a copper layer -- the board loses its copper without it. */
export function isRequiredLayer(layer: SourceLayer): boolean {
  return !!layer.kicad && copperRank(layer.kicad) >= 0;
}

export interface MatchedLayer {
  source: string;
  kicad: string;
}

/** The dialog's lists (`m_unmatched_layers_list`, `m_matched_layers_list`) and what kicad-cli guessed for each layer (`INPUT_LAYER_DESC::AutoMapLayer`). */
export interface LayerMapState {
  unmatched: string[];
  matched: MatchedLayer[];
  /** The importer's guess for every source layer; `null` where it has none. */
  guess: Record<string, string | null>;
}

/** The dialog as it opens here: the importer's guesses are the Matched Layers (the state KiCad's dialog is in after "Auto-Match Layers"), the layers it has no guess for the Unmatched. */
export function initialMapping(layers: SourceLayer[]): LayerMapState {
  const guess: Record<string, string | null> = {};
  for (const l of layers) guess[l.source] = l.kicad;
  // In the order kicad-cli's report lists them (by name).
  return {
    unmatched: layers.filter((l) => !l.kicad).map((l) => l.source),
    matched: layers.filter((l) => l.kicad).map((l) => ({ source: l.source, kicad: l.kicad! })),
    guess,
  };
}

/** `DIALOG_MAP_LAYERS::AddMappings` (the `>` button): the selected unmatched layers go to the selected KiCad layer. */
export function addMappings(state: LayerMapState, sources: string[], kicad: string): LayerMapState {
  const moving = state.unmatched.filter((s) => sources.includes(s));
  if (moving.length === 0 || !kicad) return state;
  return {
    ...state,
    unmatched: state.unmatched.filter((s) => !moving.includes(s)),
    matched: [...moving.map((source) => ({ source, kicad })), ...state.matched],
  };
}

/** `DIALOG_MAP_LAYERS::RemoveMappings` (`<` for the selected pairs, `<<` for all): the layers go back to the unmatched list. */
export function removeMappings(state: LayerMapState, sources: string[] | "all"): LayerMapState {
  const leaving = state.matched.filter((m) => sources === "all" || sources.includes(m.source));
  if (leaving.length === 0) return state;
  return {
    ...state,
    unmatched: [...state.unmatched, ...leaving.map((m) => m.source)],
    matched: state.matched.filter((m) => !leaving.includes(m)),
  };
}

/** `DIALOG_MAP_LAYERS::OnAutoMatchLayersClicked`: every unmatched layer the importer has a guess for goes to it. */
export function autoMatch(state: LayerMapState): LayerMapState {
  const guessed = state.unmatched.filter((s) => state.guess[s]);
  if (guessed.length === 0) return state;
  return {
    ...state,
    unmatched: state.unmatched.filter((s) => !guessed.includes(s)),
    matched: [...guessed.map((source) => ({ source, kicad: state.guess[source]! })), ...state.matched],
  };
}

/** `GetUnmappedRequiredLayers`: the layers marked `*` that are still unmatched. OK is refused while there are any: "All required layers (marked with '*') must be matched." */
export function unmatchedRequired(state: LayerMapState, layers: SourceLayer[]): string[] {
  const required = new Set(layers.filter(isRequiredLayer).map((l) => l.source));
  return state.unmatched.filter((s) => required.has(s));
}

export const REQUIRED_LAYERS_MESSAGE = "All required layers (marked with '*') must be matched. Please click 'Auto-Match Layers' to automatically match the remaining layers";

/** The layer map to send kicad-cli (`--layer-map`): the pairs that are not the importer's guess -- a layer the person moved or added. */
export function layerMapRequest(state: LayerMapState): Record<string, string> {
  const map: Record<string, string> = {};
  for (const m of state.matched) if (state.guess[m.source] !== m.kicad) map[m.source] = m.kicad;
  return map;
}

/**
 * The layers left unmatched that the importer has a guess for: kicad-cli would map them by the guess (its map file can name a layer's place but not leave it out), so they are
 * the layers OK asks the server to leave out (`drop_layers`); a layer with no guess is left out by kicad-cli itself.
 */
export function keptGuesses(state: LayerMapState): string[] {
  return state.unmatched.filter((s) => state.guess[s]);
}

/** A layer in a list of the dialog: its name, with the `*` of a required layer (`WrapRequired`). */
export function wrapRequired(name: string, required: boolean): string {
  return required ? `${name} *` : name;
}

// ----------------------------------------------------------------------------------------------------------------- the questions before an import

/** `UnsavedChangesDialog` (common/confirm.cpp): the title, the extended message and the three buttons of `HandleUnsavedChanges`. */
export const SAVE_CHANGES = {
  title: "Save Changes?",
  detail: "If you don't save, all your changes will be permanently lost.",
  save: "Save",
  discard: "Discard Changes",
  cancel: "Cancel",
} as const;

/** `PCB_EDIT_FRAME::OpenProjectFiles`: asked when the board is modified (`IsContentModified()`) before another board -- a converted one too -- replaces it. Two spaces, as KiCad's. */
export const PCB_MODIFIED_MESSAGE = "The current PCB has been modified.  Save changes?";

/** `SCH_EDIT_FRAME::AskToSaveChanges` (`OpenProjectFiles`): the schematic's twin, which a project import asks of the schematic it replaces. */
export const SCHEMATIC_MODIFIED_MESSAGE = "The current schematic has been modified.  Save changes?";

/** `SCH_EDIT_FRAME::OnImportProject`: `IsOK` (title "Confirmation", Yes / No) when the root sheet has items, before the file is chosen. */
export const REPLACE_SCHEMATIC_QUESTION = "This operation replaces the contents of the current schematic, which will be permanently lost.\n\nDo you want to proceed?";

// ----------------------------------------------------------------------------------------------------------------- DIALOG_IMPORT_CHOOSE_PROJECT

/** `IMPORT_PROJECT_DESC`: a row of the chooser -- a "combination" of a board and a schematic of an EasyEDA Pro project, or a board or a schematic of its own. */
export interface ImportProjectDesc {
  comboName: string;
  comboId: string;
  pcbName: string;
  pcbId: string;
  schematicName: string;
  schematicId: string;
}

/** `DIALOG_IMPORT_CHOOSE_PROJECT`'s title and the text above its list (`dialog_import_choose_project_base.cpp`). */
export const CHOOSE_PROJECT_TITLE = "Choose Project to Import";
export const CHOOSE_PROJECT_TEXT = "This project file contains multiple PCB+Schematic combinations.\nChoose which one should be imported to KiCad.";
/** Its columns: Project Name, PCB, Schematic. */
export const CHOOSE_PROJECT_COLUMNS = ["Project Name", "PCB", "Schematic"] as const;

const byKey = (o: unknown): Array<[string, unknown]> =>
  o && typeof o === "object" && !Array.isArray(o) ? Object.entries(o as Record<string, unknown>).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)) : [];
const str = (v: unknown): string => (typeof v === "string" ? v : "");

/**
 * `EASYEDAPRO::ProjectToSelectorDialog( aProject, aPcbOnly, aSchOnly )` (common/io/easyedapro/easyedapro_import_utils.cpp): the rows of the chooser from an EasyEDA Pro
 * `project.json` -- each combination of `boards` (by its name, the map's order), then the boards no combination names (unless `schOnly`), then the schematics no combination
 * names (unless `pcbOnly`); a name that is empty is the id. A board import asks with `pcbOnly` (`PCB_IO_EASYEDAPRO::LoadBoard`), a project import with neither.
 */
export function projectToSelectorRows(project: unknown, pcbOnly: boolean, schOnly: boolean): ImportProjectDesc[] {
  const p = (project ?? {}) as Record<string, unknown>;
  const schematics = new Map(byKey(p.schematics).map(([id, s]) => [id, str((s as Record<string, unknown> | null)?.name)]));
  const pcbNames = new Map<string, string>();
  for (const [id, entry] of byKey(p.pcbs)) {
    if (typeof entry === "string") pcbNames.set(id, entry);
    else if (entry && typeof entry === "object") pcbNames.set(id, str((entry as Record<string, unknown>).title));
  }
  const rows: ImportProjectDesc[] = [];
  for (const [name, b] of byKey(p.boards)) {
    const board = (b ?? {}) as Record<string, unknown>;
    const desc: ImportProjectDesc = { comboName: name, comboId: name, pcbId: str(board.pcb), pcbName: "", schematicId: str(board.schematic), schematicName: "" };
    if (pcbNames.has(desc.pcbId)) {
      desc.pcbName = pcbNames.get(desc.pcbId) || desc.pcbId;
      pcbNames.delete(desc.pcbId);
    }
    if (schematics.has(desc.schematicId)) {
      desc.schematicName = schematics.get(desc.schematicId) || desc.schematicId;
      schematics.delete(desc.schematicId);
    }
    rows.push(desc);
  }
  if (!schOnly) for (const [id, name] of pcbNames) rows.push({ comboName: "", comboId: "", pcbId: id, pcbName: name || id, schematicId: "", schematicName: "" });
  if (!pcbOnly) for (const [id, name] of schematics) rows.push({ comboName: "", comboId: "", pcbId: "", pcbName: "", schematicId: id, schematicName: name || id });
  return rows;
}

/** The three cells of a row (`DIALOG_IMPORT_CHOOSE_PROJECT`'s `convertName`: a name with no id is shown empty). */
export function chooserCells(d: ImportProjectDesc): [string, string, string] {
  const shown = (name: string, id: string) => (id ? name : "");
  return [shown(d.comboName, d.comboId), shown(d.pcbName, d.pcbId), shown(d.schematicName, d.schematicId)];
}

/**
 * Which of the boards kicad-cli converted a row's board is: kicad-cli's project command converts every board of the project in the order of its `pcbs` index
 * (`EnumerateProjectBoards`, by id), which is the order of the `boards` the server names. -1 for a row with no board.
 */
export function boardIndexOf(project: unknown, pcbId: string): number {
  if (!pcbId) return -1;
  return byKey((project as Record<string, unknown> | null)?.pcbs).findIndex(([id]) => id === pcbId);
}

/** The rows when the project file cannot be read here: the boards kicad-cli named, each a row of its own with its board only. */
export function rowsOfBoards(boards: string[]): ImportProjectDesc[] {
  return boards.map((name, i) => ({ comboName: "", comboId: "", pcbName: name, pcbId: String(i), schematicName: "", schematicId: "" }));
}
