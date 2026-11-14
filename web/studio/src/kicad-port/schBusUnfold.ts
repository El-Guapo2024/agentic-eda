// Geometry of `C` (SCH_LINE_WIRE_BUS_TOOL::UnfoldBus / doUnfoldBus): where the new bus entry roots.
//
//   VECTOR2I pos = aPos.value_or( getViewControls()->GetCursorPosition() );   // the grid-snapped cursor
//   pos = bus->GetSeg().NearestPoint( pos );                                  // exactly on the bus, to connect
//
// "If the bus segment is H or V, this will be on the selection grid, if it's not, it might not be" -- so the entry
// roots at the segment point nearest the SNAPPED cursor, not nearest the raw one (which would leave an off-grid entry
// and a wire that ERC flags as off-grid).

import { rotateSpin } from "./schPlaceTool";

/** `SEG::NearestPoint`: the point of segment a-b closest to q, rounded to whole micrometres (the schematic's unit). */
export function nearestPointOnSegment(a: readonly [number, number], b: readonly [number, number], q: readonly [number, number]): [number, number] {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const lenSq = dx * dx + dy * dy;
  if (lenSq === 0) return [a[0], a[1]];
  const t = Math.max(0, Math.min(1, ((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / lenSq));
  return [Math.round(a[0] + t * dx), Math.round(a[1] + t * dy)];
}

/** The bus entry's default size: a 100 mil diagonal (`SCH_BUS_WIRE_ENTRY( pos )` ends at `pos + ( 100 mil, 100 mil )`). */
export const BUS_ENTRY_SIZE_NM = 2_540_000;

/**
 * The size of the unfold's bus entry while the cursor moves before the first click (`doDrawSegments`, "Update the bus unfold posture based on the
 * mouse movement"): the entry leans toward the cursor, each axis negative when the cursor is on the minus side of the entry's root
 * (`flipX = cursor_delta.x < 0`, `size.x = std::abs( size.x ) * xSign`). The wire then starts at the entry's far end (`SetStartPoint( entry->GetEnd() )`).
 */
export function unfoldEntrySize(origin: readonly [number, number], cursor: readonly [number, number], size: readonly [number, number] = [BUS_ENTRY_SIZE_NM, BUS_ENTRY_SIZE_NM]): [number, number] {
  const xSign = cursor[0] - origin[0] < 0 ? -1 : 1;
  const ySign = cursor[1] - origin[1] < 0 ? -1 : 1;
  return [Math.abs(size[0]) * xSign, Math.abs(size[1]) * ySign];
}

/**
 * The first click of the unfold (`doDrawSegments`: "First click when unfolding places the label and wire-to-bus entry"): the entry keeps the posture
 * the click gives it, the wire's first point moves to the entry's end, and the member's label goes where the click is (the label ghost followed the
 * cursor, `m_busUnfold.label->SetPosition( cursorPos )`).
 */
export function placeUnfoldLabel(entryAt: readonly [number, number], size: readonly [number, number], click: readonly [number, number]): { size: [number, number]; start: [number, number]; labelAt: [number, number] } {
  const s = unfoldEntrySize(entryAt, click, size);
  return { size: s, start: [entryAt[0] + s[0], entryAt[1] + s[1]], labelAt: [click[0], click[1]] };
}

/**
 * `BUS_UNFOLD_PERSISTENT_SETTINGS::label_spin_style` (`static ... busUnfoldPersistentSettings = { SPIN_STYLE::RIGHT }`): the spin the member's label takes when an
 * unfold starts (`m_busUnfold.label->SetSpinStyle( busUnfoldPersistentSettings.label_spin_style )`), which R and Shift+R while unfolding set
 * (`busUnfoldPersistentSettings.label_spin_style = m_busUnfold.label->GetSpinStyle()`) -- kept for the session, across unfolds (UI-LEFT-2 SS18).
 */
export type UnfoldSpin = "right" | "up" | "left" | "bottom";
let persistentSpin: UnfoldSpin = "right";
export const unfoldLabelSpin = (): UnfoldSpin => persistentSpin;
export const setUnfoldLabelSpin = (s: UnfoldSpin): void => {
  persistentSpin = s;
};

/**
 * R / Shift+R in the wire tool (`doDrawSegments`' `rotateCW` / `rotateCCW` branch): while unfolding, the member's label turns a quarter (`Rotate90( clockwise )`: R is
 * `rotateCCW`) and the spin is kept for the next unfold; otherwise nothing turns (`wxBell()`). Returns the label's new spin, or null for the bell.
 */
export function unfoldRotate(unfolding: { spin?: UnfoldSpin } | null, ccw: boolean): UnfoldSpin | null {
  if (!unfolding) return null;
  const next = rotateSpin(unfolding.spin ?? "right", ccw);
  persistentSpin = next;
  return next;
}

/**
 * X / Y in the wire tool while the member's label rides the cursor (UI-LEFT-3 S3-13): `doDrawSegments` passes them on (`evt->SetPassEvent()`), and
 * `SCH_EDIT_TOOL::Mirror` mirrors the selection, which holds the riding label (`m_selectionTool->AddItemToSel( m_busUnfold.label, true )` until the click places
 * it): `SCH_TEXT::MirrorSpinStyle( !vertical )` flips a horizontal label's justification with X (`mirrorH`: right <-> left) and a vertical one's with Y
 * (`mirrorV`: up <-> bottom); the other key leaves it as it is. Unlike R it is not kept for the next unfold (`label_spin_style` is set by the rotate branch
 * only). Returns the label's new spin, or null when no label rides (the key is the edit tool's as ever).
 */
export function unfoldMirror(unfolding: { spin?: UnfoldSpin; labelAt?: unknown } | null, leftRight: boolean): UnfoldSpin | null {
  if (!unfolding || unfolding.labelAt) return null;
  const spin = unfolding.spin ?? "right";
  const flip: Record<UnfoldSpin, UnfoldSpin> = leftRight ? { right: "left", left: "right", up: "up", bottom: "bottom" } : { right: "right", left: "left", up: "bottom", bottom: "up" };
  return flip[spin];
}
