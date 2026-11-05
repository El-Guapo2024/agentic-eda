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
  /** kicad-cli's zone filler is running (B). */
  zoneFillRunning?: boolean;
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
  // Zones with no fill get no standing note (R7-Z2): KiCad says so only when the zone display is switched to filled (`unfilledZoneWarning`).
  if (s.tab === "pcb" && s.zoneFillRunning) notes.push("Filling zones…");
  return notes;
}

/**
 * `PCB_CONTROL::unfilledZoneCheck`'s message, `KeyNameFromKeyCode( PCB_ACTIONS::zoneFillAll.GetHotKey() )` being B. KiCad shows it in the info bar for 5 s
 * (`ShowMessageFor( msg, 5000, wxICON_WARNING )`) with a "Don't show again" link.
 */
export const ZONE_FILL_WARNING = "Not all zones are filled. Use Edit > Fill All Zones (B) if you wish to see all fills.";
/** `infobar->ShowMessageFor( msg, 5000, .. )`. */
export const ZONE_FILL_WARNING_MS = 5000;

/**
 * `PCB_CONTROL::unfilledZoneCheck`, run by `ZoneDisplayMode` on `zoneDisplayFilled` only (not on the toggle, not on opening a board): the warning when a zone of the
 * board that is not a rule area is not filled (`!zone->GetIsRuleArea() && !zone->IsFilled()`), unless "Don't show again" was clicked
 * (`m_DoNotShowAgain.zone_fill_warning`); else null. `fills` is what `GET /api/fill` lists: an entry per zone and layer of the board's pours -- not the rule areas, not a
 * footprint's own zones (`board()->Zones()` has none of those) -- `filled` when the zone has a fill that is current; `null` when it is not known, then `zones` (the
 * board's, rule areas left out) all count as not filled.
 */
export function unfilledZoneWarning(fills: readonly { filled: boolean }[] | null, zones: readonly { is_rule_area?: boolean }[], dismissed: boolean): string | null {
  if (dismissed) return null;
  const unfilled = fills ? fills.some((f) => !f.filled) : zones.some((z) => !z.is_rule_area);
  return unfilled ? ZONE_FILL_WARNING : null;
}
