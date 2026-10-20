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
// Layers -- and OK sends the layers the person moved or added, in a second run; a layer taken out of the matched list that has a guess keeps the guess (a map cannot say "leave
// this layer out"), and the dialog says so.

import { standardLayerName } from "./boardLayers";

export type ImportKind = "board" | "schematic";

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
  kind?: ImportKind;
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
  /** A schematic: the sheet file (`eda_model::sheet_file::SheetFile`). */
  content?: unknown;
  notes?: string[];
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

export function acceptFor(kind: ImportKind): string {
  return kind === "board" ? BOARD_ACCEPT : SCHEMATIC_ACCEPT;
}

/** The parameter of the import actions: the file the page read (`window.__eda.run(id, { name, data })`; the chooser's own file has the same shape), and what to do with the layers. */
export interface ForeignParam extends ForeignFile {
  /** Source layer -> KiCad layer: answers the layer dialog without opening it. */
  layerMap?: Record<string, string>;
  /** Do not ask which layer each source layer goes to: kicad-cli's guess is used (`ADVANCED_CFG::m_ImportSkipLayerMapping`). */
  skipLayerMapping?: boolean;
  /** "Keep KiCad layer names". */
  keepKicadLayerNames?: boolean;
  /** Do not open the report of the import's errors and warnings ("Show import issues" off). */
  skipReport?: boolean;
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

/** What an import brought, in a sentence: "Imported x.brd (Eagle): 19 footprints, 6 nets, 82 tracks, 11 vias, 2 zones." */
export function describeImport(reply: ForeignReply): string {
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
  return reply.kind === "board" && reply.ok && !reply.project ? "The design rules and net classes of the file could not be read." : "";
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

/** The layers left unmatched that the importer has a guess for: kicad-cli maps them by the guess all the same (its map file can name a layer's place but not leave it out). */
export function keptGuesses(state: LayerMapState): string[] {
  return state.unmatched.filter((s) => state.guess[s]);
}

/** A layer in a list of the dialog: its name, with the `*` of a required layer (`WrapRequired`). */
export function wrapRequired(name: string, required: boolean): string {
  return required ? `${name} *` : name;
}
