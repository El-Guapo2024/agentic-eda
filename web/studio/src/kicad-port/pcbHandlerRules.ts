// Small rules of the board editor's action handlers (actions/pcbEditSweep.ts, actions/boardControlActions.ts), kept pure so they are tested here.
// Each cites the KiCad function it follows (pcbnew/tools at 8303b2ad).

/**
 * `EDIT_TOOL::BooleanPolygons`: the items go to the routine in selection order, except that the last-added one is swapped with the first
 * (`if( item == last_item ) std::swap( items_to_process.back(), items_to_process.front() )`) -- it is the base and the property donor.
 */
export function booleanProcessOrder(ids: readonly string[], lastAdded: string | undefined): string[] {
  const order = [...ids];
  const at = lastAdded === undefined ? -1 : order.indexOf(lastAdded);
  if (at > 0) [order[0], order[at]] = [order[at]!, order[0]!];
  return order;
}

/** A zone's fill priority, as the priority commands compare it. */
export interface PriorityZone {
  priority: number;
}

/**
 * `ZONE_PRIORITY_CONTEXT_MENU::update`'s `canRaise` / `canLower`, which are also the handlers' own first test (`ZonePriorityRaise`: no overlapping zone
 * above -> `return 0`, nothing said): Top and Raise need an overlapping zone of a higher priority, Lower and Bottom one of a lower priority.
 */
export function zonePriorityCanMove(zone: PriorityZone, overlapping: readonly PriorityZone[], to: "top" | "raise" | "lower" | "bottom"): boolean {
  const up = to === "top" || to === "raise";
  return overlapping.some((o) => (up ? o.priority > zone.priority : o.priority < zone.priority));
}
