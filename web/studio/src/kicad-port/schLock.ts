// Lock / Unlock / Toggle Lock for the schematic -- `SCH_EDIT_TOOL::modifyLockSelected` -- and the way a locked
// item is kept out of edits (`FilterSelectionForLockedItems`, which Move, Rotate, Mirror, Swap and Delete call first;
// keeping it out of selections is the selection filter's "Locked items", kicad-port/schSelectionFilter.ts).
import type { SchEditCmd } from "../api/schEditTypes";

export type LockMode = "toggle" | "lock" | "unlock";

/** Pins, fields and sheet pins inherit their lock from their parent and are never locked on their own (`modifyLockSelected`'s `continue`). */
export function isLockableId(id: string): boolean {
  return !id.startsWith("shpin_");
}

/** `modifyLockSelected`'s TOGGLE: lock everything, unless any selected item is already locked, in which case unlock. */
export function resolveLockMode(mode: LockMode, ids: readonly string[], locked: ReadonlySet<string>): "lock" | "unlock" {
  if (mode !== "toggle") return mode;
  return ids.some((id) => isLockableId(id) && locked.has(id)) ? "unlock" : "lock";
}

/**
 * The one verb a Lock / Unlock / Toggle Lock press sends: only the items whose state actually changes (the backend refuses
 * a verb that changes nothing), or null when there is nothing to do.
 */
export function lockCmd(mode: LockMode, ids: readonly string[], locked: ReadonlySet<string>): Extract<SchEditCmd, { verb: "set_locked" }> | null {
  const lockable = ids.filter(isLockableId);
  if (lockable.length === 0) return null;
  const target = resolveLockMode(mode, lockable, locked) === "lock";
  const changing = lockable.filter((id) => locked.has(id) !== target);
  return changing.length === 0 ? null : { verb: "set_locked", ids: changing, locked: target };
}

/** `FilterSelectionForLockedItems`: the selection with every locked item removed. */
export function withoutLocked(ids: readonly string[], locked: ReadonlySet<string>): string[] {
  return ids.filter((id) => !locked.has(id));
}
