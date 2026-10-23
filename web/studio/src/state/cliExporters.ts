// Which of the exporters an older kicad-cli lacks (IDF, GenCAD: `kicad-port/cliExporters.ts`) the server's kicad-cli has, asked once per
// page (`GET /api/fab/exporters`; the server itself probes kicad-cli once per process). Plain module state with subscribers, like
// `state/designBlocks.ts`, so the action runner, the menu bar and the test hook see the same answer.
import { useSyncExternalStore } from "react";
import { cliExporterReason, type CliExportersReply } from "../kicad-port/cliExporters";

let reply: CliExportersReply | null = null;
let asked = false;
const listeners = new Set<() => void>();

function ask(): void {
  if (asked) return;
  asked = true;
  void fetch("/api/fab/exporters", { cache: "no-store" })
    .then((r) => r.json() as Promise<Partial<CliExportersReply>>)
    .then((r) => {
      reply = { engine: r.engine ?? null, unavailable: r.unavailable && typeof r.unavailable === "object" ? r.unavailable : {} };
      for (const l of listeners) l();
    })
    // No answer: nothing is refused, and the export says what went wrong.
    .catch(() => undefined);
}

function subscribe(l: () => void): () => void {
  listeners.add(l);
  ask();
  return () => listeners.delete(l);
}

/** The server's answer, null until it has come (the first call asks). */
export function useCliExporters(): CliExportersReply | null {
  return useSyncExternalStore(subscribe, () => reply);
}

/** Why `action` cannot run with the server's kicad-cli, or null (see `kicad-port/cliExporters.ts`). */
export function cliExporterUnavailable(action: string): string | null {
  ask();
  return cliExporterReason(action, reply);
}
