// The Embedded Files panel that is showing, if one is, for the actions that act on it: `common.Embed.removeFile` and `common.Embed.extractFile` are what the panel's
// Remove and Export buttons do (`EMBED_TOOL::RemoveFile`, `PANEL_EMBEDDED_FILES::onExportFiles`), on the rows selected in it. Plain module state, like state/commonDialogs.ts.
import type { EmbeddedOwner } from "../api/embeddedTypes";

export interface ActiveEmbeddedPanel {
  owner: EmbeddedOwner;
  /** Remove the selected rows (the panel's Remove button). */
  remove: () => void;
  /** Extract the selected rows, or every row when none is selected (the panel's Export button). */
  extract: () => void;
}

let active: ActiveEmbeddedPanel | null = null;

/** The panel that is showing now; null when none is. */
export function activeEmbeddedPanel(): ActiveEmbeddedPanel | null {
  return active;
}

/** The panel says it is showing (`panel`) or no longer is (null); a panel only clears itself. */
export function setActiveEmbeddedPanel(panel: ActiveEmbeddedPanel | null, previous?: ActiveEmbeddedPanel | null): void {
  if (panel === null && previous !== undefined && active !== previous) return;
  active = panel;
}
