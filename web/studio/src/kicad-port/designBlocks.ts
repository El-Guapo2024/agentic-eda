// The Design Blocks panes (`DESIGN_BLOCK_PANE`, `PANEL_DESIGN_BLOCK_CHOOSER`, `DESIGN_BLOCK_TREE_MODEL_ADAPTER`; common/widgets/design_block_pane.cpp,
// common/widgets/panel_design_block_chooser.cpp, common/design_block_tree_model_adapter.cpp, common/design_block.cpp, commit 8303b2ad): what the tree lists and in what order,
// what a search keeps, and how a block placed on the sheet or the board is asked for. Pure, so every rule is unit tested without a browser or a server.
//
// The tree is the footprint chooser's (a `LIB_TREE` over a model adapter): libraries, their blocks, a search that scores the nickname (weight 4), the name (8), `Lib:Name` (16), every
// keyword (4), the keywords as one text (1) and the description (1) -- `DESIGN_BLOCK::GetSearchTerms` is `FOOTPRINT_INFO::GetSearchTerms` -- so the matching and the ordering are
// `libChooser.ts`'s. What is a design block's own: the "Recently Used" group is the frame's `m_designBlockHistoryList` (seven: `while( m_historyList.size() >= 8 ) pop_back()`), the four check boxes under the tree, and a
// block's name that Place as group gives its group.
import type { Block, BlockLibrary } from "../api/designBlockClient";
import { naturalCompare, orderGroups, parseQuery, scoreItemTerms, scoreItems, searchTerm, splitId, RECENT_LABEL, type ChooserGroup, type ChooserItem } from "./libChooser";

/** The four check boxes under the tree (`design_block_chooser.*`, `SCH_DESIGN_BLOCK_PANE`, `PCB_DESIGN_BLOCK_PANE`). */
export interface PlaceOptions {
  /** "Place repeated copies". */
  repeated: boolean;
  /** "Place as group". */
  asGroup: boolean;
  /** "Place as sheet" (the schematic's only). */
  asSheet: boolean;
  /** "Keep annotations". */
  keepAnnotations: boolean;
}

/** The id of a block (`LIB_ID::Format`): `Lib:Name`. A library row is `Lib:`. */
export function blockId(lib: string, name: string): string {
  return `${lib}:${name}`;
}

/** The library an id names, the block (empty for a library row). */
export function parseBlockId(id: string | null): { lib: string; name: string } {
  return id ? splitId(id) : { lib: "", name: "" };
}

/** A block as a tree item. */
export function itemOf(lib: string, block: Block): ChooserItem {
  return { id: blockId(lib, block.name), lib, name: block.name, description: block.description, keywords: block.keywords };
}

/**
 * Every group the tree shows: the recently used blocks first, then the libraries (pinned ones before the others, then in natural order of their names; while a search is on the
 * best score first). A search keeps the blocks that score and hides a library with none (`GetChildren`: a node with no score is hidden). A library node without children
 * scores itself (`LIB_TREE_NODE_LIBRARY::UpdateScore`: 1 plus each word's score against its name, weight 8, or 0 when one word scores nothing), so an empty library is listed
 * while nothing is searched and while its name matches; "-- Recently Used --" is always added (`rebuildHistoryNode` calls `DoAddLibrary` with no blocks too), and scores
 * the same way when it is empty.
 */
export function composeBlockGroups(input: { libraries: readonly BlockLibrary[]; query: string; history: readonly string[]; pinned: ReadonlySet<string> }): ChooserGroup[] {
  const searching = parseQuery(input.query).length > 0;
  const groups: ChooserGroup[] = [];
  const known = new Map<string, ChooserItem>();
  for (const lib of input.libraries) for (const b of lib.blocks) known.set(blockId(lib.name, b.name), itemOf(lib.name, b));
  // the blocks that were deleted since are gone from the history (`LoadDesignBlock` answers nothing for them)
  const recent = input.history.map((id) => known.get(id)).filter((i): i is ChooserItem => i !== undefined);
  let kept = searching ? scoreItems("footprint", recent, input.query) : recent;
  if (searching) kept = [...kept].sort((a, b) => (b.score ?? 0) - (a.score ?? 0));
  const matchers = parseQuery(input.query);
  /** `LIB_TREE_NODE_LIBRARY::UpdateScore` of a node with no children: its name (`SEARCH_TERM( aName, 8 )`) is its only term. */
  const childlessScore = (name: string) => (searching ? scoreItemTerms(matchers, [searchTerm(name, 8)]) : 0);
  const recentShown = recent.length === 0 ? !searching || childlessScore(RECENT_LABEL) > 0 : kept.length > 0;
  if (recentShown) groups.push({ lib: RECENT_LABEL, pseudo: "recent", pinned: false, items: [...kept], lazy: false, score: recent.length === 0 ? childlessScore(RECENT_LABEL) : Math.max(0, ...kept.map((i) => i.score ?? 0)) });
  for (const lib of input.libraries) {
    const items = lib.blocks.map((b) => itemOf(lib.name, b));
    if (items.length === 0) {
      const score = childlessScore(lib.name);
      if (searching && score === 0) continue;
      groups.push({ lib: lib.name, description: lib.description, pinned: input.pinned.has(lib.name), items: [], count: 0, lazy: false, score });
      continue;
    }
    const scored = searching ? scoreItems("footprint", items, input.query) : items;
    if (searching && scored.length === 0) continue;
    scored.sort((a, b) => (searching && a.score !== b.score ? (b.score ?? 0) - (a.score ?? 0) : naturalCompare(a.name, b.name)));
    groups.push({ lib: lib.name, description: lib.description, pinned: input.pinned.has(lib.name), items: scored, count: items.length, lazy: false, score: Math.max(0, ...scored.map((i) => i.score ?? 0)) });
  }
  // the pseudo-library first, then pinned libraries, then by score (while searching) or name
  return orderGroups(groups, searching);
}

/** `PANEL_DESIGN_BLOCK_CHOOSER::addDesignBlockToHistory`'s list for the tree: the block just placed first. Kept in state/designBlocks.ts; this is the same rule for tests. */
export function pushHistory(history: readonly string[], id: string, max = 7): string[] {
  return [id, ...history.filter((h) => h !== id)].slice(0, max);
}

/** The name `Place as group` gives the group: the block's, and with "Place repeated copies" a number counting up from 1 (`group->GetName() + wxString::Format( "%d", suffix++ )`). */
export function groupNameFor(blockName: string, repeated: boolean, suffix: number): string {
  return repeated ? `${blockName}${suffix}` : blockName;
}

/** Why a block cannot be placed here, in KiCad's words, or null (`ImportSheet`: "Design block has no schematic to place."; `AppendDesignBlock`: "Design block has no layout to place."). `id` is the block asked for (`Lib:Name`). */
export function placeProblem(block: Block | undefined, editor: "schematic" | "pcb", id = ""): string | null {
  if (!block) return `Could not find design block ${id}.`.replace(" .", ".");
  if (editor === "schematic") return block.schematic ? null : "Design block has no schematic to place.";
  return block.board ? null : "Design block has no layout to place.";
}

/** `GenerateInfo`: the block's text for the details area -- name, description, keywords, then the default fields. */
export function blockInfo(block: Block): { name: string; description: string; keywords: string; fields: { name: string; value: string }[] } {
  return { name: block.name, description: block.description, keywords: block.keywords, fields: block.fields };
}

/** What the four check boxes mean for a placement on the schematic (`SCH_DESIGN_BLOCK_PANE`), as the plan Import Sheet's tool holds. */
export function schematicPlacement(path: string, block: string, options: PlaceOptions): { path: string; block: string; asSheet: boolean; keepAnnotations: boolean; repeated: boolean; asGroup: boolean } {
  return { path, block, asSheet: options.asSheet, keepAnnotations: options.keepAnnotations, repeated: options.repeated, asGroup: options.asGroup };
}

/** The first block with this id in the libraries, or undefined. */
export function findBlock(libraries: readonly BlockLibrary[], id: string | null): Block | undefined {
  const { lib, name } = parseBlockId(id);
  if (!name) return undefined;
  return libraries.find((l) => l.name === lib)?.blocks.find((b) => b.name === name);
}

/** `DIALOG_DESIGN_BLOCK_PROPERTIES::TransferDataFromWindow`: the characters a name may not have (`LIB_ID::FindIllegalLibraryNameChar` and `/`), and a dot (see design_block.rs). */
export function nameProblem(name: string): string | null {
  for (const c of name) {
    if (c.charCodeAt(0) < 0x20 || c === "\\" || c === ":" || c === "/") return `Illegal character '${c}' in name '${name}'.`;
  }
  if (name === "") return "Design block does not have a valid library ID.";
  if (name.includes(".")) return `Illegal character '.' in name '${name}': KiCad lists a design block by the part of its folder's name before the first dot, so a dot would hide it from KiCad.`;
  return null;
}

/** `TransferDataFromGrid`: a field's name is stripped, its line breaks removed, double spaces made single; two of one name are refused. Returns the cleaned list or the refusal. */
export function cleanFields(fields: readonly { name: string; value: string }[]): { fields: { name: string; value: string }[] } | { problem: string } {
  const out: { name: string; value: string }[] = [];
  for (const f of fields) {
    const name = f.name.trim().replace(/\n/g, "").replace(/ {2}/g, " ");
    if (out.some((o) => o.name === name)) return { problem: "Duplicate fields are not allowed." };
    out.push({ name, value: f.value });
  }
  return { fields: out };
}

/** `LIB_ID::Parse`-ish check of a group's library link as Group Properties takes it: `Lib:Name`, both parts there. */
export function linkProblem(text: string): string | null {
  const t = text.trim();
  if (t === "") return null;
  const at = t.indexOf(":");
  const bad = at <= 0 || at === t.length - 1 || /[\\\u0000-\u001f]/.test(t.slice(0, at) + t.slice(at + 1));
  return bad ? `Invalid library link: '${t}'` : null;
}

/** The entries of the pane's context menu, by what each does (the pane runs the action of that name for the editor it is in). */
export type BlockMenuId = "pin" | "unpin" | "newLibrary" | "place" | "properties" | "saveSheet" | "saveSelection" | "updateSheet" | "updateSelection" | "delete" | "hide";

export interface BlockMenuItem {
  id?: BlockMenuId;
  label: string;
  separator?: boolean;
}

/** What the row the menu opens on is: a library, a design block, or nothing (the empty tree). */
export type BlockRowKind = "library" | "item" | "none";

/**
 * `DESIGN_BLOCK_CONTROL::AddContextMenuItems` with `SCH_DESIGN_BLOCK_CONTROL::Init` / `PCB_DESIGN_BLOCK_CONTROL::Init`: the conditional menu of the pane, entries in the order of
 * their priorities (1, 2, 50, 100, 400), each only while its condition holds (`isInLibrary`: the node is a library or a block; `isDesignBlock`: a block; `hasSelection`: the editor
 * has something selected), and no separator that would stand first, last or next to another (`CONDITIONAL_MENU::Evaluate`: "We try to avoid adding useless separators").
 * `pinned` is the library's; the "Recently Used" pseudo-library is none of KiCad's libraries to pin.
 */
export function blockContextMenu(editor: "schematic" | "pcb", row: { kind: BlockRowKind; pinned?: boolean; pseudo?: boolean }, hasSelection: boolean): BlockMenuItem[] {
  const isLibrary = row.kind === "library";
  const isBlock = row.kind === "item";
  const inLibrary = isLibrary || isBlock;
  const sheet = editor === "schematic";
  const all: { show: boolean; item: BlockMenuItem }[] = [
    { show: isLibrary && !row.pseudo && !row.pinned, item: { id: "pin", label: "Pin Library" } },
    { show: isLibrary && !row.pseudo && row.pinned === true, item: { id: "unpin", label: "Unpin Library" } },
    { show: true, item: { id: "newLibrary", label: "New Library..." } },
    { show: true, item: { label: "", separator: true } },
    { show: isBlock, item: { id: "place", label: "Place Design Block" } },
    { show: true, item: { label: "", separator: true } },
    { show: isBlock, item: { id: "properties", label: "Properties..." } },
    { show: inLibrary, item: { id: "saveSheet", label: sheet ? "Save Current Sheet as Design Block..." : "Save Board as Design Block..." } },
    { show: inLibrary && hasSelection, item: { id: "saveSelection", label: "Save Selection as Design Block..." } },
    { show: isBlock, item: { id: "updateSheet", label: sheet ? "Update Design Block from Current Sheet" : "Update Design Block from Board" } },
    { show: isBlock && hasSelection, item: { id: "updateSelection", label: "Update Design Block from Selection" } },
    { show: isBlock, item: { id: "delete", label: "Delete Design Block" } },
    { show: true, item: { label: "", separator: true } },
    { show: true, item: { label: "", separator: true } },
    { show: true, item: { id: "hide", label: "Hide Library Tree" } },
  ];
  const out: BlockMenuItem[] = [];
  let sinceSeparator = 0;
  for (const { show, item } of all) {
    if (!show) continue;
    if (item.separator) {
      if (sinceSeparator > 0) out.push(item);
      sinceSeparator = 0;
      continue;
    }
    out.push(item);
    sinceSeparator++;
  }
  while (out.length > 0 && out[out.length - 1]!.separator) out.pop();
  return out;
}
