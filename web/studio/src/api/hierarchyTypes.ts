// The wire types of the hierarchy dialogs (crates/cli/src/sch_hierarchy_api.rs, crates/ops/src/sch_annotate.rs): Annotate Schematic here;
// Import Sheet and the rest as they come.

/** `AnnotateOptions` of `Cmd::Annotate`: the part of `DIALOG_ANNOTATE` that is not the selection, the order and the reset. */
export interface AnnotateOptions {
  /** Absent: the selection when `ids` are given, else the sheet in view. */
  scope?: "all" | "current_sheet" | "selection";
  /** "Use first free number after" (the default), or counting from the sheet number times 100 or 1000. */
  algo?: "first_free" | "sheet_x100" | "sheet_x1000";
  start_number?: number;
  recursive?: boolean;
  regroup_units?: boolean;
  /** The sheet symbols selected on the sheet in view (ids of its `SheetInstance`s). */
  sheets?: string[];
}

/** One line of the message window (`RPT_SEVERITY`). */
export interface AnnotateMessage {
  severity: "action" | "error" | "warning";
  text: string;
}

/** `POST /api/sch/annotate`: what the annotation would do, from `AnnotateSymbols`' reporter and `CheckAnnotate`. */
export interface AnnotateReport {
  ok: boolean;
  message?: string;
  /** How many references would change; 0 means nothing to do. */
  changed: number;
  messages: AnnotateMessage[];
  problems: { kind: string; message: string; first: number; second: number | null }[];
}
