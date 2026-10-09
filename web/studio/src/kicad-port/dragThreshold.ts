// The click-versus-drag rule: when a mouse button that went down becomes a drag instead of a click. A port of
// common/tool/tool_dispatcher.cpp (TOOL_DISPATCHER::handleMouseButton and BUTTON_STATE) and include/tool/tool_dispatcher.h, commit 8303b2ad.
//
//   handleMouseButton, in the order the C++ runs it:
//     * a press (`down`): `st->downTimestamp = now`; `dragOriginScreen` / `dragOrigin` are where the pointer was; `downPosition` too; `pressed = true`.
//     * a motion with the button held and no drag yet:
//           #ifdef __WXMAC__  if( now - downTimestamp > DragTimeThreshold ) dragging = true;        -- 300 ms, macOS builds only
//           offset = lastMousePosScreen - dragOriginScreen;
//           if( abs( offset.x ) > m_sysDragMinX || abs( offset.y ) > m_sysDragMinY ) dragging = true;  -- 8 px
//       The time test runs only inside a motion event: a button held still for a second is not a drag, the first movement after the 300 ms is.
//     * a release: a drag that started is a mouse-up (TA_MOUSE_UP), anything else a click (TA_MOUSE_CLICK). A click carries `downPosition`,
//       the point where the button went DOWN, not where it came up: a press with a few pixels of jitter before the release is a click at the press.
//     * once a drag has started it stays one until the release, however close the pointer comes back to where it began.
//
//   `m_sysDragMinX/Y` is `wxSystemSettings::GetMetric( wxSYS_DRAG_X / Y )` when the platform has one and `DragDistanceThreshold` (8) when it says -1; a
//   browser has no such metric, so the fallback is the constant. The comparison is per axis and strict: 8 px along one axis is a click, 9 is a drag, and
//   (6, 6) -- 8.5 px away along the diagonal -- is a click too (it is not the length of the offset that is compared).
//
// Nothing here knows about a canvas: the editors keep one `ClickDragGesture` per pressed button, feed it the pointer events and ask it whether the
// press has become a drag. Each of the four editors (board, schematic, footprint, symbol) decides what a drag does; this is only when it starts.

/** `TOOL_DISPATCHER::DragDistanceThreshold`: pixels of travel along either axis (strictly more than) after which a held button is a drag. */
export const DRAG_DISTANCE_PX = 8;

/** `TOOL_DISPATCHER::DragTimeThreshold`: milliseconds held after which the next motion starts a drag -- in the macOS build only (`#ifdef __WXMAC__`). */
export const DRAG_TIME_MS = 300;

export interface DragRule {
  /** Pixels; strictly more than this along x or along y starts a drag. */
  distancePx: number;
  /** Milliseconds; strictly longer than this since the press, at a motion event, starts a drag. null: the platform has no time rule. */
  holdMs: number | null;
}

/** The rule of a platform: `DragTimeThreshold` is compiled in for macOS (`__WXMAC__`) only. */
export function dragRuleFor(mac: boolean): DragRule {
  return { distancePx: DRAG_DISTANCE_PX, holdMs: mac ? DRAG_TIME_MS : null };
}

/** How a press ended: `TA_MOUSE_CLICK` (it never became a drag) or `TA_MOUSE_UP` (the end of a drag). */
export type ButtonRelease = "click" | "dragEnd";

/** What the editors do with a press: nothing yet, a drag has started, or it is a drag that goes on. */
export interface MotionResult {
  dragging: boolean;
  /** True only for the motion that turned the press into a drag. */
  started: boolean;
}

/**
 * One button's `BUTTON_STATE` (`pressed`, `dragging`, `dragOriginScreen`, `downTimestamp`): the editors create one per canvas for the left button and, where
 * the right button pans or opens a menu, one for the right.
 */
export class ClickDragGesture {
  private isPressed = false;
  private isDragging = false;
  private originX = 0;
  private originY = 0;
  private downTime = 0;

  constructor(private readonly rule: DragRule) {}

  get pressed(): boolean {
    return this.isPressed;
  }

  get dragging(): boolean {
    return this.isDragging;
  }

  /** `dragOriginScreen`: where the press happened, in the screen pixels the motions are given in (null while nothing is pressed). */
  get origin(): readonly [number, number] | null {
    return this.isPressed ? [this.originX, this.originY] : null;
  }

  /** `downTimestamp`. */
  get downAt(): number {
    return this.downTime;
  }

  /** The press: the C++ saves the origin "on the first click only" (`if( !st->pressed )`), and a second press while one is down is not seen at all. */
  down(x: number, y: number, t: number): void {
    if (this.isPressed) return;
    this.isPressed = true;
    this.isDragging = false;
    this.originX = x;
    this.originY = y;
    this.downTime = t;
  }

  /** A pointer motion while this button is down (a motion with the button up is not the gesture's business: nothing is pressed). */
  move(x: number, y: number, t: number): MotionResult {
    if (!this.isPressed) return { dragging: false, started: false };
    if (this.isDragging) return { dragging: true, started: false };
    if (this.rule.holdMs !== null && t - this.downTime > this.rule.holdMs) this.isDragging = true;
    if (Math.abs(x - this.originX) > this.rule.distancePx || Math.abs(y - this.originY) > this.rule.distancePx) this.isDragging = true;
    return { dragging: this.isDragging, started: this.isDragging };
  }

  /** The release (or a cancelled pointer): `pressed = false`; a drag that began ends as mouse-up, anything else is a click. */
  up(): ButtonRelease {
    const result: ButtonRelease = this.isDragging ? "dragEnd" : "click";
    this.isPressed = false;
    this.isDragging = false;
    return result;
  }

  /** `BUTTON_STATE::Reset()`: forget the press (a tool was cancelled under it). */
  reset(): void {
    this.isPressed = false;
    this.isDragging = false;
  }
}
