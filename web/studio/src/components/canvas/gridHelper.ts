// Snap helper for the move/place tools. KiCad's real PCB_GRID_HELPER
// (pcbnew/tools/pcb_grid_helper.cpp) snaps to the grid *and* to nearby
// anchors -- other pads, part origins, existing track endpoints -- and
// picks whichever candidate is closest. This session could not read
// that file (see the report), so only plain grid snapping is
// implemented; anchor snapping is a follow-up (tracked in the report's
// gap list). Grid snap alone is still what the backend enforces anyway:
// `board::step()` snaps every placement command to `meta.snap_um`
// server-side (crates/cli/src/board.rs), so this is a *preview*
// convenience, not the source of truth.

export function snap(valueUm: number, gridUm: number): number {
  if (gridUm <= 0) return Math.round(valueUm);
  return Math.round(valueUm / gridUm) * gridUm;
}

export function snapPoint(xUm: number, yUm: number, gridUm: number): [number, number] {
  return [snap(xUm, gridUm), snap(yUm, gridUm)];
}
