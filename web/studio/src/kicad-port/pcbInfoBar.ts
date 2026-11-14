// What the board editor says in its info bar over the canvas (`WX_INFOBAR`, components/PcbInfoBar.tsx) rather than in a toast: KiCad's `ShowInfoBarError` (the error
// icon, 8 s), `ShowInfoBarWarning` (6 s), `ShowInfoBarMsg` (8 s) and `ShowMessageFor( msg, ms, icon )` calls of pcbnew (src/kicad/pcb_infobar_messages.json,
// tools/extract-infobar-messages.js: every message as KiCad words it, its `%s` / `%d` as wildcards, with its icon, time and close button). A message of the
// board editor that is one of them goes to the bar (state/store.tsx's TOAST on the board), the server's error code before it (`ops_..: `) left out.
import table from "../kicad/pcb_infobar_messages.json";

export interface InfoBarMessage {
  message: string;
  kind: "error" | "warning" | "info";
  /** `ShowMessageFor`'s time, ms; null: shown until closed or replaced (`ShowMessage`). */
  ms: number | null;
  /** `AddCloseButton`. */
  close: boolean;
}

const PATTERNS = (table as { messages: Array<{ pattern: string; kind: InfoBarMessage["kind"]; ms: number; close: boolean }> }).messages.map((m) => ({ ...m, re: new RegExp(`^${m.pattern}$`) }));

/** The text the bar shows: the message without the server's error code before it ("ops_via_drc: Via location violates DRC." -> KiCad's words). */
export function withoutErrorCode(message: string): string {
  return message.replace(/^[a-z][a-z0-9_]*: /, "");
}

/** The info bar KiCad shows `message` in, or null for a message KiCad does not show there. */
export function infoBarFor(message: string): InfoBarMessage | null {
  const text = withoutErrorCode(message.trim());
  const m = PATTERNS.find((p) => p.re.test(text));
  return m ? { message: text, kind: m.kind, ms: m.ms, close: m.close } : null;
}

/** `EDA_BASE_FRAME::ShowInfoBarMsg( msg )`: the information icon, 8 s, no close button -- for the studio's own words a KiCad frame says in its bar. */
export function infoBarMsg(message: string): InfoBarMessage {
  return { message, kind: "info", ms: 8000, close: false };
}
