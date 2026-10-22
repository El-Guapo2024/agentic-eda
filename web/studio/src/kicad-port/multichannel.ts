// The multichannel tool (GAPS.md items 24 and 32, package P24), the studio's side: pcbnew/tools/multichannel_tool.cpp, pcbnew/dialogs/dialog_multichannel_generate_rule_areas.cpp,
// dialog_multichannel_repeat_layout.cpp and dialog_rule_area_properties.cpp at 8303b2ad. What the tool does is the server's (crates/ops/src/multichannel*.rs: the matching of the
// footprints of two areas by topology, the copy of the layout, the rule areas made from the sheets, classes and groups of the board); this file is the dialogs' arithmetic --
// what a row of the Generate dialog becomes, which tab it opens on, what the Placement tab of Rule Area Properties stores, which rule area a Repeat Layout starts from.
//
// Pure: no store, no DOM. Unit-tested in multichannel.test.ts.
import type { Cmd, Group, PlacementArea, PlacementSourceType, RepeatLayoutOptions, Zone } from "../api/types";

/** The choices of the "Source Type" row of the Properties panel (`ENUM_MAP<PLACEMENT_SOURCE_T>`). */
export const PLACEMENT_SOURCE_CHOICES: { label: string; value: PlacementSourceType }[] = [
  { label: "Sheet Name", value: "sheetname" },
  { label: "Component Class", value: "component_class" },
  { label: "Group", value: "group" },
];

// ------------------------------------------------------------------------------------------------------------------------------------ Rule Area Properties

/** What the Placement tab of Rule Area Properties has chosen: the radio, and what each of the three combo boxes shows. */
export interface PlacementForm {
  /** `m_DisabledRb`, `m_SheetRb`, `m_ComponentsRb` or `m_GroupRb`. */
  mode: "none" | PlacementSourceType;
  /** The source each combo box holds (`m_sheetCombo`, `m_componentClassCombo`, `m_groupCombo`). */
  sheet: string;
  component_class: string;
  group: string;
  /** `m_lastPlacementSourceType`: the radio last chosen, which a rule area with placement turned off still names. */
  last: PlacementSourceType;
  /** The source the area had that the board no longer has (`m_notFoundPlacementSource`), by type: it is the first entry of its combo box, "Not found on board: ...". */
  missing: Partial<Record<PlacementSourceType, string>>;
}

/** The three lists the combo boxes offer: the sheets of the footprints (the empty one included), the component classes and the names of the groups the footprints are in. */
export interface PlacementChoices {
  sheets: string[];
  classes: string[];
  groups: string[];
}

export const NO_CHOICES: PlacementChoices = { sheets: [], classes: [], groups: [] };

const listOf = (c: PlacementChoices, t: PlacementSourceType): string[] => (t === "sheetname" ? c.sheets : t === "component_class" ? c.classes : c.groups);

/** The entry shown for a source the board does not have (`_( "Not found on board: " ) + curSourceName`). */
export const notFoundLabel = (name: string): string => `Not found on board: ${name}`;

/** `DIALOG_RULE_AREA_PROPERTIES::TransferDataToWindow`'s placement half: the radio is the area's source type when placement is on, and each combo box starts on its first entry, the area's own source selected in the one of its type (or put first as "not found" when the board lacks it). */
export function placementFormOf(placement: PlacementArea | undefined, choices: PlacementChoices): PlacementForm {
  const p = placement ?? { enabled: false, source_type: "sheetname" as PlacementSourceType, source: "" };
  const form: PlacementForm = { mode: p.enabled ? p.source_type : "none", sheet: choices.sheets[0] ?? "", component_class: choices.classes[0] ?? "", group: choices.groups[0] ?? "", last: p.source_type, missing: {} };
  if (p.source !== "") {
    if (!listOf(choices, p.source_type).includes(p.source)) form.missing[p.source_type] = p.source;
    form[p.source_type === "sheetname" ? "sheet" : p.source_type] = p.source;
  }
  return form;
}

/** The entries of one combo box, "not found" first. */
export function placementOptions(form: PlacementForm, choices: PlacementChoices, type: PlacementSourceType): { value: string; label: string }[] {
  const missing = form.missing[type];
  const list = listOf(choices, type).map((v) => ({ value: v, label: v === "" ? "(no sheet)" : v }));
  return missing === undefined ? list : [{ value: missing, label: notFoundLabel(missing) }, ...list];
}

/** What the combo box of `type` has selected. */
export const placementSelection = (form: PlacementForm, type: PlacementSourceType): string => (type === "sheetname" ? form.sheet : type === "component_class" ? form.component_class : form.group);

/** `DIALOG_RULE_AREA_PROPERTIES::TransferDataFromWindow`'s placement half: placement is on when one of the three radios is, with the source of its combo box; with none it is off and the area still names the source type last chosen and what its combo box shows. */
export function placementOf(form: PlacementForm): PlacementArea {
  if (form.mode === "none") return { enabled: false, source_type: form.last, source: placementSelection(form, form.last) };
  return { enabled: true, source_type: form.mode, source: placementSelection(form, form.mode) };
}

/** A radio chosen (`OnSheetNameClicked`, `OnComponentClassClicked`, `OnGroupClicked` remember the type; "No placement" leaves it). */
export function chooseMode(form: PlacementForm, mode: PlacementForm["mode"]): PlacementForm {
  return { ...form, mode, last: mode === "none" ? form.last : mode };
}

// -------------------------------------------------------------------------------------------------------------------------------- Generate Placement Rule Areas

/** One row of the Generate dialog (`MULTICHANNEL_TOOL::GeneratePotentialRuleAreas`'s `RULE_AREA`). */
export interface GenerateRow {
  kind: "sheet" | "component_class" | "group";
  key: string;
  /** The sheet's file (the "Sheet Name" column), empty for the others. */
  file: string;
  components: string[];
  /** The "Generate" check box. */
  generate: boolean;
}

/** The three tabs of the dialog, in the order they are added: Sheets, Component Classes, Groups. */
export const GENERATE_TABS: { kind: GenerateRow["kind"]; label: string }[] = [
  { kind: "sheet", label: "Sheets" },
  { kind: "component_class", label: "Component Classes" },
  { kind: "group", label: "Groups" },
];

/** The tab the dialog opens on: the first, unless there is a single sheet, when the component classes (else the groups) are shown (`m_sheetGrid->GetNumberRows() == 1`). */
export function initialGenerateTab(rows: readonly GenerateRow[]): GenerateRow["kind"] {
  const count = (k: GenerateRow["kind"]) => rows.filter((r) => r.kind === k).length;
  if (count("sheet") === 1) {
    if (count("component_class") > 0) return "component_class";
    if (count("group") > 0) return "group";
  }
  return "sheet";
}

/** "Group footprints with their placement rule areas" is not offered for the Groups tab (`OnNotebookPageChanged`), and a generation from it never groups (`TransferDataFromWindow`). */
export const groupItemsAvailable = (tab: GenerateRow["kind"]): boolean => tab !== "group";

/** The command `AutogenerateRuleAreas` is after the dialog: the rows ticked, and the two check boxes. `null` when no row is ticked (KiCad makes nothing of it). */
export function generateCommand(rows: readonly GenerateRow[], replaceExisting: boolean, groupItems: boolean, tab: GenerateRow["kind"]): Cmd | null {
  const picked = rows.filter((r) => r.generate);
  if (picked.length === 0) return null;
  return { op: "multichannel", verb: "generate_rule_areas", areas: picked.map((r) => ({ kind: r.kind, key: r.key })), replace_existing: replaceExisting, group_items: groupItemsAvailable(tab) && groupItems };
}

/** `_( "Cannot auto-generate any placement areas because the schematic has only one or no hierarchical sheets, groups, or component classes." )`: shown when the board offers one area or none. */
export const NOTHING_TO_GENERATE = "Cannot auto-generate any placement areas because the schematic has only one or no hierarchical sheets, groups, or component classes.";

// ------------------------------------------------------------------------------------------------------------------------------------------ Repeat Layout

/** `REPEAT_LAYOUT_OPTIONS` as the dialog sets them. */
export type RepeatOptions = RepeatLayoutOptions;

/** `REPEAT_LAYOUT_OPTIONS`' defaults, which the dialog starts from (the tool keeps the last ones for the session). */
export const DEFAULT_REPEAT_OPTIONS: RepeatOptions = { copy_placement: true, copy_routing: true, connected_routing_only: false, copy_other_items: true, group_items: false, include_locked_items: true, anchor: null };

/** `_( "Select a reference Rule Area to copy from..." )`. */
export const PICK_REFERENCE_PROMPT = "Select a reference Rule Area to copy from...";
/** `_( "No Rule Areas to repeat layout to have been found." )`. */
export const NO_TARGET_AREAS = "No Rule Areas to repeat layout to have been found.";

/** `isSelectedItemAnRA`: a rule area with placement turned on. */
export const isPlacementRuleArea = (z: Zone | undefined): z is Zone => !!z && z.is_rule_area && !!z.placement?.enabled;

/**
 * `MULTICHANNEL_TOOL::repeatLayout`'s first step: the reference area is the one placement rule area in the selection, a group in it counting for the ones it holds. Anything else
 * (none, or several) is `null`, and the tool asks for one with the picker.
 */
export function referenceAreaOf(selection: Iterable<string>, zones: readonly Zone[], groups: readonly Group[]): string | null {
  const found: string[] = [];
  const zoneById = new Map(zones.map((z) => [z.id, z]));
  const groupById = new Map(groups.map((g) => [g.id, g]));
  for (const id of selection) {
    const z = zoneById.get(id);
    if (isPlacementRuleArea(z)) found.push(id);
    else {
      const g = groupById.get(id);
      for (const member of g?.member_ids ?? []) if (isPlacementRuleArea(zoneById.get(member))) found.push(member);
    }
  }
  return found.length === 1 ? found[0]! : null;
}

/** The target rows of the Repeat Layout dialog as the server sends them (`RULE_AREA_COMPAT_DATA`). */
export interface TargetRow {
  id: string;
  name: string;
  ok: boolean;
  message: string;
  reasons: string[];
  components: string[];
  /** The "Copy" check box: on for a target that matches (`ent.m_doCopy = ruleAreaData.m_isOk`). */
  copy: boolean;
}

export function targetRows(targets: readonly { id: string; name: string; ok: boolean; message: string; reasons: string[]; components: string[] }[]): TargetRow[] {
  return targets.map((t) => ({ ...t, copy: t.ok }));
}

/** The text of the dialog's Status column: "OK", or what is wrong. */
export const statusOf = (t: Pick<TargetRow, "ok" | "message">): string => (t.ok ? "OK" : t.message);

/** `_( "Rule area topologies do not match: %s" )` and its details (`ShowTopologyMismatchReasons`). */
export const mismatchSummary = (t: Pick<TargetRow, "message">): string => `Rule area topologies do not match: ${t.message}`;

/** The command `RepeatLayout( aEvent, aRefZone )` is after the dialog: `null` when no target is ticked. */
export function repeatCommand(reference: string, rows: readonly TargetRow[], options: RepeatOptions): Cmd | null {
  const targets = rows.filter((r) => r.copy && r.ok).map((r) => r.id);
  if (targets.length === 0) return null;
  return { op: "multichannel", verb: "repeat_layout", reference, targets, options: { ...options, anchor: options.anchor || null } };
}

/** `ShowInfoBarMsg( _( "Copied to %d Rule Areas." ) )`. */
export const copiedMessage = (n: number): string => `Copied to ${n} Rule Areas.`;

/** The anchor choices (`m_refAnchorFp`): an empty entry, then the references of the reference area's footprints, sorted. */
export function anchorChoices(components: readonly string[]): string[] {
  return ["", ...[...components].sort()];
}

/** The two check boxes of the Generate dialog, one changed. */
export function chooseSourceFlags(flags: { replace: boolean; group: boolean }, change: Partial<{ replace: boolean; group: boolean }>): { replace: boolean; group: boolean } {
  return { ...flags, ...change };
}
