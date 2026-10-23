// The File > Export entries whose kicad-cli command an older kicad-cli lacks: IDFv3... (`kicad-cli pcb export idf`, newer than the Mac's
// 10.99.0-5334) and GenCAD... (`kicad-cli pcb export gencad`). The server probes the kicad-cli in use once (`GET /api/fab/exporters`,
// `eda_kicad_engine::cli_export_kinds`) and names the ones it lacks with the reason; an entry whose command is missing is disabled, and its
// tooltip says why ("This kicad-cli (10.99.0) has no IDF exporter; KiCad 10.99 nightly or later has one."). Pure: `state/cliExporters.ts`
// holds the server's answer, and the action runner, the menu and the test hook ask here.

/** Each such action, by its `pcb export` command. */
export const CLI_EXPORTER_ACTIONS: Readonly<Record<string, string>> = {
  "pcbnew.EditorControl.exportIDF": "idf",
  "pcbnew.EditorControl.exportGenCAD": "gencad",
};

/** `GET /api/fab/exporters`: the kicad-cli it asked, and the commands it lacks with the reason. */
export interface CliExportersReply {
  engine: string | null;
  unavailable: Record<string, string>;
}

/**
 * Why `action` cannot run with this kicad-cli, or null when it can. Before the server has answered (`reply` null), or for an action that is
 * not one of these, nothing is refused: the export itself then says what went wrong.
 */
export function cliExporterReason(action: string, reply: CliExportersReply | null): string | null {
  const kind = CLI_EXPORTER_ACTIONS[action];
  if (!kind || !reply) return null;
  return reply.unavailable[kind] ?? null;
}
