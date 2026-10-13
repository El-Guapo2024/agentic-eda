// What the Footprint Checker dialog (`DIALOG_FOOTPRINT_CHECKER`, pcbnew/dialogs/dialog_footprint_checker.cpp at KiCad 8303b2ad) shows of the markers a run left: the rows the "Show"
// boxes let through, the counts of its three badges (a badge says "-" until the checks have run, `numErrors = -1`), and the layer a selected marker brings to the front
// (`OnSelectItem`). The checks themselves are kicad-cli's DRC on a scratch board that holds the footprint (crates/kicad-engine/src/footprint_check.rs); this reads their markers.
import type { FpCheckMarker } from "../api/types";

/** The "Show:" boxes: Errors, Warnings, Exclusions (`getSeverities`). */
export interface FpShow {
  errors: boolean;
  warnings: boolean;
  exclusions: boolean;
}

export const SHOW_ALL: FpShow = { errors: true, warnings: true, exclusions: true };

/** The "All" box: on when the three are (`OnSeverity`: turning it on turns them on, turning it off leaves Errors alone and clears the others). */
export const showAll = (s: FpShow): boolean => s.errors && s.warnings && s.exclusions;

export function setShowAll(on: boolean): FpShow {
  return { errors: true, warnings: on, exclusions: on };
}

/** Whether the "Show" boxes let a marker through. */
export function shown(m: FpCheckMarker, show: FpShow): boolean {
  if (m.severity === "error") return show.errors;
  if (m.severity === "warning") return show.warnings;
  if (m.severity === "exclusion") return show.exclusions;
  return false;
}

/** The marker indices the list shows, in the order kicad-cli reported them. */
export function visibleMarkers(markers: readonly FpCheckMarker[], show: FpShow): number[] {
  return markers.flatMap((m, i) => (shown(m, show) ? [i] : []));
}

export interface FpBadges {
  /** `null` is the "-" of a badge before the checks ran (`UpdateNumber( -1 )`): only while there is nothing to count. */
  errors: number | null;
  warnings: number | null;
  exclusions: number;
}

/** `updateDisplayedCounts`: the counts of the badges. */
export function badges(markers: readonly FpCheckMarker[], checksRun: boolean): FpBadges {
  const count = (severity: string) => markers.filter((m) => m.severity === severity).length;
  const errors = count("error");
  const warnings = count("warning");
  return { errors: !checksRun && errors === 0 ? null : errors, warnings: !checksRun && warnings === 0 ? null : warnings, exclusions: count("exclusion") };
}

/** The footprint editor's layer box offers these (silkscreen, fab, courtyard): the layer a marker brings to the front, when it is one of them. */
export function markerLayer(m: FpCheckMarker): string | null {
  switch (m.type) {
    case "malformed_courtyard":
    case "missing_courtyard":
      return "F.CrtYd";
    case "silk_over_copper":
      return "F.SilkS";
    default:
      return null;
  }
}

/** The editor items a marker names (`board->ResolveItem( rc_item->GetMainItemID() )` and the others), without repeats. */
export function markerItems(m: FpCheckMarker): string[] {
  return [...new Set(m.items.flatMap((i) => (i.id ? [i.id] : [])))];
}

/** The text of a badge. */
export const badgeText = (n: number | null): string => (n === null ? "-" : String(n));
