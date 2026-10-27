// File > Export > Specctra DSN... and File > Import > Specctra Session... (`pcbnew.EditorControl.exportSpecctraDSN`, `importSpecctraSession`), the rules of
// pcbnew/tools/board_editor_control.cpp's `ExportSpecctraDSN` / `ImportSpecctraSession` and of `PCB_EDIT_FRAME::ExportSpecctraFile` / `ImportSpecctraSession`
// (pcbnew/specctra_import_export/). The DSN is written by the studio's server (`POST /api/specctra/export`, crates/cli/src/specctra_api.rs: `kicad-cli pcb export
// specctra`, or, where kicad-cli has no such command, the port of `FromBOARD` in crates/kicad/src/specctra); the session is read by the `specctra` command (crates/ops/src/specctra.rs: `FromSESSION`), one undo step.
//
// A browser has no Save or Open dialog that hands back a path: the DSN is a download named as KiCad's file selector names it (the board's name, `.dsn`), and the
// session file is chosen with the browser's file chooser, its text sent in the command.
import { fileStem } from "./saveAs";

/** `FILEEXT::SpecctraDsnFileExtension` and `SpecctraSessionFileExtension`. */
export const DSN_EXTENSION = "dsn";
export const SES_EXTENSION = "ses";

/** The file chooser's filter for a session (`FILEEXT::SpecctraSessionFileWildcard()`). */
export const SESSION_ACCEPT = `.${SES_EXTENSION}`;

/** `ExportSpecctraDSN`: the board's file name with the DSN extension (`fn.SetExt( FILEEXT::SpecctraDsnFileExtension )`). */
export function dsnFileName(boardName: string): string {
  return `${fileStem(boardName)}.${DSN_EXTENSION}`;
}

/** `POST /api/specctra/export`'s reply. */
export interface SpecctraExportReply {
  ok: boolean;
  message: string;
  files?: string[];
  /** What `FromBOARD` warned about (`wxLogWarning`): a malformed board outline. */
  warnings?: string[];
  name?: string;
  text?: string;
  /** Which engine wrote the file: kicad-cli, or our own exporter where kicad-cli has no `pcb export specctra`. The page shows the same either way. */
  engine?: "kicad-cli" | "eda";
}

/** The status line after an export (`SetStatusText( _( "BOARD exported OK." ) )`), or the error dialog's two lines in one ("Unable to export, please fix and try again" and why). */
export function describeExport(reply: SpecctraExportReply, savedAs: string): { message: string; kind: "info" | "error" } {
  if (!reply.ok) return { message: reply.message || "Unable to export, please fix and try again", kind: "error" };
  const warnings = (reply.warnings ?? []).join(" ");
  return { message: `BOARD exported OK. Saved ${savedAs}.${warnings ? ` ${warnings}` : ""}`, kind: warnings ? "error" : "info" };
}

/** The command a session file becomes: `Cmd::Specctra`, the text of the file and its name (for messages). */
export function specctraImportCmd(file: { name: string; text: string }): { op: "specctra"; session: string; name: string } {
  return { op: "specctra", session: file.text, name: file.name };
}

/** What `ImportSpecctraSession` says: "Session file imported and merged OK." in the status bar, or, when `FromSESSION` threw, the error KiCad's dialog shows
 * (`DisplayErrorMessage( this, _( "Board may be corrupted, do not save it.\n Fix problem and try again" ), ioe.What() )`). Differs: the first sentence is left out,
 * since a refused import here leaves the board as it was; "Fix problem and try again" comes first, the error under it. */
export function describeImport(reply: { ok: boolean; message: string }): { message: string; kind: "info" | "error" } {
  if (reply.ok) return { message: "Session file imported and merged OK. Undo takes the import back.", kind: "info" };
  return { message: reply.message ? `Fix problem and try again\n${reply.message}` : "Fix problem and try again", kind: "error" };
}
