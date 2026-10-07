// Which design revision a kicad-cli report (DRC, ERC) was computed on, and
// whether the board has moved on since.
//
// A kicad-cli run takes seconds and the studio stays editable while it runs
// (the server answers edits and the /api/version poll meanwhile -- see
// crates/cli/src/kicad_lane.rs), so the board can change under a run. The
// server stamps every kicad-cli reply with `revision`: the same stamp
// GET /api/version serves, of the design the run started from. A report is
// out of date once the board's revision is no longer that one -- a number
// the page cannot work out itself: the version it last polled can be a poll
// behind the design the run actually read (an edit made a moment before
// "Run DRC").
//
// Pure and dependency-free (like the rest of kicad-port), so the rule has
// unit tests; DrcDialog/ErcDialog, the canvas markers and the status bar
// all read it through here.
//
// While a run is going the board stays editable, with the dialog closed or
// open: nothing here, or anywhere in the studio, locks the canvas for a run.

/** The notice shown next to an out-of-date report. */
export const STALE_NOTICE = "design changed since this check — rerun";

/**
 * The revision a reply was computed on: the server's own stamp. A server that
 * does not stamp (an older build) leaves the page's own guess, the version it
 * had seen when it asked.
 */
export function revisionOf(reply: { revision?: string | null } | null | undefined, askedAt: string | null): string | null {
  return reply?.revision ?? askedAt;
}

/**
 * Out of date: the board's revision is not the one the report was computed
 * on. Nothing is out of date while the page does not know the board's
 * revision yet (its first /api/version answer is still on the way); a report
 * of unknown revision is.
 */
export function isStale(reportRevision: string | null, boardRevision: string | null): boolean {
  return boardRevision !== null && reportRevision !== boardRevision;
}

/**
 * The revision to keep for a report after an edit that does not change what
 * it says (ERC's "exclude this violation" patches the report on screen
 * instead of running kicad-cli again). A report that was current stays current
 * -- the edit moved the board's revision, and the report with it -- while one
 * that was already out of date stays out of date: the exclusion does not
 * bring it up to date with edits made before it.
 */
export function revisionAfterOwnEdit(reportRevision: string | null, boardBefore: string | null, boardAfter: string | null): string | null {
  return isStale(reportRevision, boardBefore) ? reportRevision : boardAfter;
}

export interface CheckStatus {
  /** The editor tab on screen: DRC speaks about the PCB, ERC about the schematic. */
  tab: string;
  drcRunning: boolean;
  ercRunning: boolean;
  /** A DRC/ERC report is on screen and its revision is no longer the board's. */
  drcStale: boolean;
  ercStale: boolean;
}

/**
 * What the status bar says about kicad-cli's checks, so a person who closed
 * the dialog while one runs still sees it going, and sees that the markers on
 * their tab no longer describe the board. A run in progress wins over "out of
 * date": its result is on the way.
 */
export function statusNotes(s: CheckStatus): string[] {
  const notes: string[] = [];
  if (s.drcRunning) notes.push("DRC running…");
  if (s.ercRunning) notes.push("ERC running…");
  if (!s.drcRunning && s.drcStale && s.tab === "pcb") notes.push("DRC out of date");
  if (!s.ercRunning && s.ercStale && s.tab === "schematic") notes.push("ERC out of date");
  return notes;
}
