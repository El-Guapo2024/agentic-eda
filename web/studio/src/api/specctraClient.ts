// The browser half of File > Export > Specctra DSN... (`POST /api/specctra/export`, crates/cli/src/specctra_api.rs): the server writes the DSN of the board (and keeps a copy
// under `export/specctra/`), and the page saves the text it answers as a download. The import is a command (`specctra`), sent with `postCmd`.
import { saveTextAs } from "./client";
import { postJson } from "./schControlClient";
import { dsnFileName, type SpecctraExportReply } from "../kicad-port/specctra";

/** `BOARD_EDITOR_CONTROL::ExportSpecctraDSN`: the DSN, saved as `<board>.dsn`. Answers the reply and the name it was saved under. */
export async function exportSpecctraDsn(boardName: string): Promise<{ reply: SpecctraExportReply; savedAs: string }> {
  const reply = await postJson<SpecctraExportReply>("/api/specctra/export", {});
  const savedAs = dsnFileName(boardName);
  if (reply.ok && typeof reply.text === "string") saveTextAs(reply.text, savedAs);
  return { reply, savedAs };
}
