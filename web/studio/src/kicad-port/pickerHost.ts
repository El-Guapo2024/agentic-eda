// The interactive picker: what KiCad's PICKER_TOOL_BASE / PCB_PICKER_TOOL (common/tool/picker_tool.cpp,
// pcbnew/tools/pcb_picker_tool.cpp) give the tools that need "one more point (or item) from the user": a
// session is started with a prompt and a click handler, the next left click on the canvas answers it, and
// Escape (or another tool taking over) cancels it.
//
// The C++ runs the session as a tool coroutine with a click handler (`SetClickHandler`, which returns
// `getNext` -- true to keep picking), a cancel handler and a finalize handler. The same three handlers are
// here, plus one convenience the C++ gets from lambdas that start new picker runs: a click handler may hand
// back the NEXT session instead of a boolean (InteractiveOffset's two clicks).
//
// This file is the framework-free state machine; the React side (the prompt popup, the canvas hook and the
// Promise helpers the dialogs use) is actions/pcbPicker.ts.

export interface PickPoint {
  x: number;
  y: number;
}

/** What a canvas click offers the running session. */
export interface PickHit {
  /** The click, snapped the way `PCB_GRID_HELPER::BestSnapAnchor` does (`SelectPointInteractively` sends `grid_helper.GetSnappedPoint()`). */
  point: PickPoint;
  /** The item under the click -- looked up lazily, only an item session needs it (`SelectItemInteractively`: `RequestSelection`). */
  item: () => string | null;
}

/** `true`: another pick is wanted (`getNext`); `false`: done; a session: carry on with that one. */
export type PickResult = boolean | PickerSession;

export interface PickerSession {
  /** `SelectPointInteractively` / `SelectItemInteractively`. */
  kind: "point" | "item";
  /** The `STATUS_TEXT_POPUP` text. */
  prompt: string;
  /** `SetClickHandler` of a point session. */
  onPoint?: (at: PickPoint) => PickResult;
  /** `SetClickHandler` of an item session; not called when the click found no item ("still looking for an item"). */
  onItem?: (id: string) => PickResult;
  /**
   * `SetCancelHandler`: Escape, or another tool being activated, before a click finished the session.
   * `activated` tells the two apart (`evt->IsActivate()`): a tool that backs out one stage on Escape ends for good
   * when another tool takes over.
   */
  onCancel?: (activated: boolean) => void;
  /** `SetFinalizeHandler`: the session is over, however it ended. */
  onFinalize?: () => void;
  /**
   * `SetMotionHandler` of a session that highlights what a click would take (the interactive delete tool's `BrightenItem`): the id under the
   * pointer, or null when there is nothing -- or more than one thing -- there. The host (components/CommonToolHost.tsx) draws it.
   */
  hover?: (at: PickPoint) => string | null;
  /** `SetCursor`: the pointer over the canvas while the session runs; the delete tool asks for `REMOVE`. */
  cursor?: "default" | "remove";
}

export class PickerHost {
  private current: PickerSession | null = null;
  private readonly listeners = new Set<() => void>();

  /** The running session, if any. */
  session = (): PickerSession | null => this.current;

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  private emit(): void {
    for (const l of this.listeners) l();
  }

  /** The cancel and finalize handlers of a session that is no longer current. `cancelled`: null when a click finished it. */
  private end(session: PickerSession, cancelled: "escape" | "activate" | null): void {
    if (cancelled) session.onCancel?.(cancelled === "activate");
    session.onFinalize?.();
  }

  /**
   * Run `session`. A session already running is cancelled first -- activating a tool ends the running one
   * (`evt->IsActivate()` in `PICKER_TOOL::Main`). A session its cancel handler starts is cancelled in turn: the
   * last one asked for wins.
   */
  start(session: PickerSession): void {
    const old = this.current;
    this.current = null;
    if (old) this.end(old, "activate");
    const nested = this.current as PickerSession | null;
    if (nested) {
      this.current = null;
      this.end(nested, "activate");
    }
    this.current = session;
    this.emit();
  }

  /**
   * A left click on the canvas. Returns whether the picker took it (a click while a session runs belongs to the
   * picker and to nothing else).
   */
  click(hit: PickHit): boolean {
    const session = this.current;
    if (!session) return false;
    let result: PickResult = false;
    if (session.kind === "point") {
      result = session.onPoint ? session.onPoint(hit.point) : false;
    } else {
      const id = hit.item();
      // `if( sel.Empty() ) return true; // still looking for an item`
      if (id === null) return true;
      result = session.onItem ? session.onItem(id) : false;
    }
    // The handler may have ended or replaced the session itself.
    if (this.current !== session) return true;
    if (result === true) return true;
    this.current = null;
    this.end(session, null);
    if (typeof result === "object") this.current = result;
    this.emit();
    return true;
  }

  /**
   * The cancel handler, then the finalize handler. `activated`: another tool took over rather than Escape being
   * pressed. Returns whether a session was running.
   */
  cancel(activated = false): boolean {
    const session = this.current;
    if (!session) return false;
    this.current = null;
    this.end(session, activated ? "activate" : "escape");
    this.emit();
    return true;
  }
}
