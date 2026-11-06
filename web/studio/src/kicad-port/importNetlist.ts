// File > Import > Netlist... (`pcbnew.EditorControl.importNetlist`, `PCB_EDIT_FRAME::InstallNetlistFrame`): the dialog `DIALOG_IMPORT_NETLIST`
// (pcbnew/dialogs/dialog_import_netlist.cpp and _base.cpp at 8303b2ad), its choices, its words and its flow. The netlist is read and laid onto the board by the
// server (crates/ops/src/netlist_update.rs, `BOARD_NETLIST_UPDATER`): Load and Test Netlist is `POST /api/import_netlist` (the dry run), Update PCB the
// `import_netlist` command, one undo step. The footprints the update adds are spread (`SpreadFootprints( &newFootprints, { 0, 0 }, true )`, grouped by sheet)
// and handed to the Move tool when the dialog closes (`m_runDragCommand`).

import type { Cmd } from "../api/types";
import { COMPONENT_GAP_NM, GROUP_GAP_NM, PACK_SCALE_NM, packFootprints, type Box, type PackItem } from "./packFootprints";
import { spreadRectangles, type RectXYWHF } from "./rectpack2d";

/** The dialog's choices (`DIALOG_IMPORT_NETLIST_BASE`). */
export interface NetlistOptions {
  /** Link Method: selection 0 ("tstamps"), off is selection 1 ("reference designators"). */
  by_timestamp: boolean;
  delete_extra_footprints: boolean;
  replace_footprints: boolean;
  transfer_groups: boolean;
  override_locks: boolean;
  /** Shown by 8303b2ad's dialog and passed to nothing by `loadNetlist`. */
  delete_shorting_tracks: boolean;
}

/** As the dialog opens: `m_matchByTimestamp->SetSelection( 0 )`, `m_cbUpdateFootprints->SetValue( true )`, `m_cbTransferGroups->SetValue( true )`, the rest off. */
export const DEFAULT_NETLIST_OPTIONS: NetlistOptions = {
  by_timestamp: true,
  delete_extra_footprints: false,
  replace_footprints: true,
  transfer_groups: true,
  override_locks: false,
  delete_shorting_tracks: false,
};

export const DIALOG_TITLE = "Import Netlist";
export const FILE_LABEL = "Netlist file:";
export const LINK_METHOD_LABEL = "Link Method";
export const LINK_CHOICES = ["Link footprints using component tstamps (unique ids)", "Link footprints using reference designators"] as const;
export const LINK_TOOLTIP = "Select whether to update footprint references to match their currently-assigned symbols, or to re-assign footprints to symbols which match their current references.";
export const OPTIONS_LABEL = "Options";

/** The Options box's check boxes, in the dialog's order. */
export const OPTION_LABELS: ReadonlyArray<readonly [keyof NetlistOptions, string]> = [
  ["delete_extra_footprints", "Delete footprints with no components in netlist"],
  ["replace_footprints", "Replace footprints with those specified in netlist"],
  ["transfer_groups", "Group footprints based on symbol group"],
  ["override_locks", "Delete/replace footprints even if locked"],
  ["delete_shorting_tracks", "Delete tracks shorting multiple nets"],
];

/** `SetupStandardButtons( { { wxID_OK, _( "Load and Test Netlist" ) }, { wxID_CANCEL, _( "Close" ) }, { wxID_APPLY, _( "Update PCB" ) } } )`. */
export const BUTTON_TEST = "Load and Test Netlist";
export const BUTTON_UPDATE = "Update PCB";
export const BUTTON_CLOSE = "Close";

/** The message panel's label: before (`_("Changes to Be Applied")`) and after Update PCB (`_( "Changes Applied to PCB" )`). */
export const LABEL_TO_APPLY = "Changes to Be Applied";
export const LABEL_APPLIED = "Changes Applied to PCB";

/** `FILEEXT::NetlistFileWildcard()`: "KiCad netlist files (*.net)". The browse dialog's title is the dialog's ("Import Netlist"). */
export const NETLIST_ACCEPT = ".net";

/** `onUpdatePCB`'s message boxes and `onFilenameChanged`'s error line. */
export const NO_VALID_FILE = "Please choose a valid netlist file.";
export const NO_SUCH_FILE = "The netlist file does not exist.";

export type Severity = "info" | "action" | "warning" | "error";

export interface NetlistReportLine {
  severity: Severity;
  text: string;
}

/** What the server answers for a dry run, and in the `import_netlist` command's reply (`report`). */
export interface NetlistReport {
  ok: boolean;
  lines: NetlistReportLine[];
  /** The footprints the update added (`GetAddedFootprints`), and each one's path (`GetPath`, in the same order). */
  added: string[];
  added_paths?: string[];
  warnings: number;
  errors: number;
  /** The reader's refusal (`DisplayErrorMessage`), when the file could not be read. */
  error?: string;
  message?: string;
}

/** A netlist file the page holds: its name and its text (a browser gives the page the file, not its path). */
export interface NetlistFile {
  name: string;
  text: string;
}

/** Update PCB: the `import_netlist` command. The footprints it adds go below the board (`estimateFootprintInsertionPosition`) unless `at` says otherwise. */
export function importNetlistCmd(file: NetlistFile, options: NetlistOptions, at?: { x: number; y: number }): Cmd {
  return { op: "import_netlist", text: file.text, name: file.name, options, ...(at ? { at } : {}) };
}

/** The message panel's "Show:" boxes (`WX_HTML_REPORT_PANEL`: All, Errors, Warnings, Actions, Infos). */
export interface ShownSeverities {
  error: boolean;
  warning: boolean;
  action: boolean;
  info: boolean;
}

export const SHOW_ALL: ShownSeverities = { error: true, warning: true, action: true, info: true };

/** The lines the "Show:" boxes let through. */
export function shownLines(lines: readonly NetlistReportLine[], show: ShownSeverities): NetlistReportLine[] {
  return lines.filter((l) => show[l.severity]);
}

/** `loadNetlist`'s head when the file cannot be read (`ReadNetlistFromFile` shows the reader's error in a message box and returns): the two head lines. */
export function headLines(name: string, byTimestamp: boolean): NetlistReportLine[] {
  return [
    { severity: "info", text: `Reading netlist file '${name}'.` },
    { severity: "info", text: byTimestamp ? "Using tstamps (unique IDs) to match symbols and footprints." : "Using reference designators to match symbols and footprints." },
  ];
}

/** `footprint->GetPath().AsString().BeforeLast( '/' )`: the sheet a footprint is grouped by when the new ones are spread. */
export function sheetOfPath(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? "" : path.slice(0, i);
}

export interface SpreadItem extends PackItem {
  /** `GetPath()`, for the sheet grouping. */
  path: string;
}

/**
 * `SpreadFootprints( &newFootprints, { 0, 0 }, aGroupBySheet = true )` (pcbnew/autorouter/spread_footprints.cpp): each sheet's footprints are packed as Pack and
 * Move packs them (`packFootprints`: blocks of equal size, by reference), then the sheets' areas, each inflated by `aGroupGap`, are packed against each other
 * (`spreadRectangles`, sheets in their map's order: by path) and the whole lands with its top-left on the footprints' own top-left (`aTargetBoxPosition`;
 * the Move tool then carries it to the cursor). Returns each footprint's shift.
 */
export function spreadBySheet(items: readonly SpreadItem[], componentGap = COMPONENT_GAP_NM, groupGap = GROUP_GAP_NM): Record<string, [number, number]> {
  if (items.length === 0) return {};
  const sheets = new Map<string, SpreadItem[]>();
  for (const it of items) {
    const key = sheetOfPath(it.path);
    const list = sheets.get(key);
    if (list) list.push(it);
    else sheets.set(key, [it]);
  }
  const keys = [...sheets.keys()].sort();
  const origin: [number, number] = [Math.min(...items.map((i) => i.bbox[0])), Math.min(...items.map((i) => i.bbox[1]))];
  // each sheet packed alone, its arrangement's top-left on the sheet's own top-left
  const packed = keys.map((k) => {
    const group = sheets.get(k)!;
    const { offsets } = packFootprints(group, componentGap, groupGap);
    const boxes: Box[] = group.map((g) => {
      const [dx, dy] = offsets[g.ref] ?? [0, 0];
      return [g.bbox[0] + dx, g.bbox[1] + dy, g.bbox[2] + dx, g.bbox[3] + dy] as const;
    });
    const box: Box = [Math.min(...boxes.map((b) => b[0])), Math.min(...boxes.map((b) => b[1])), Math.max(...boxes.map((b) => b[2])), Math.max(...boxes.map((b) => b[3]))];
    return { group, offsets, box };
  });
  if (packed.length === 1) return packed[0]!.offsets;
  // "Add a margin around the sheet placement area": `rect.Inflate( aGroupGap )`, the area summed without it
  const rects: RectXYWHF[] = packed.map((p) => ({ x: 0, y: 0, w: Math.trunc((p.box[2] - p.box[0] + 2 * groupGap) / PACK_SCALE_NM), h: Math.trunc((p.box[3] - p.box[1] + 2 * groupGap) / PACK_SCALE_NM), flipped: false }));
  const area = packed.reduce((s, p) => s + (p.box[2] - p.box[0]) * (p.box[3] - p.box[1]), 0);
  const side = Math.trunc(Math.sqrt(area));
  spreadRectangles(rects, side, side, PACK_SCALE_NM);
  const out: Record<string, [number, number]> = {};
  packed.forEach((p, i) => {
    // `footprint->Move( target_pos - src_bbox.GetPosition() )`, `target_pos = srect.x * scale + aTargetBoxPosition.x`
    const tx = rects[i]!.x * PACK_SCALE_NM + origin[0];
    const ty = rects[i]!.y * PACK_SCALE_NM + origin[1];
    for (const g of p.group) {
      const [dx, dy] = p.offsets[g.ref] ?? [0, 0];
      out[g.ref] = [dx + tx - p.box[0], dy + ty - p.box[1]];
    }
  });
  return out;
}

/** What the dialog does once Update PCB answered (`loadNetlist( false )` and `OnNetlistChanged`). */
export interface UpdateOutcome {
  /** The message panel's lines. */
  lines: NetlistReportLine[];
  /** A message box (`DisplayErrorMessage`): the file could not be read. */
  alert: string | null;
  /** The held step ends now (nothing to carry), judged by the Strict switch. */
  commitNow: boolean;
  /** The footprints the Move tool picks up when the dialog closes (`m_runDragCommand`), and their paths. */
  carry: { refs: string[]; paths: string[] } | null;
}

/** Update PCB's reply, read: a refusal is the panel's last line (the head lines before it); added footprints wait for the dialog to close; with none the step ends at once. */
export function updateOutcome(reply: { ok: boolean; message: string; report?: NetlistReport }, file: NetlistFile, byTimestamp: boolean): UpdateOutcome {
  const report = reply.report;
  const alert = report?.error ?? null;
  if (!reply.ok) return { lines: [...(report?.lines ?? headLines(file.name, byTimestamp)), { severity: "error", text: reply.message }], alert, commitNow: false, carry: null };
  const added = report?.added ?? [];
  return { lines: report?.lines ?? [], alert, commitNow: added.length === 0, carry: added.length > 0 ? { refs: added, paths: report?.added_paths ?? [] } : null };
}
