// The control flow of router_tool.cpp `ROUTER_TOOL::RouteSelected`'s loop (pcbnew/router/router_tool.cpp
// at 8303b2ad), without the router: the connections of the selection are worked through one after the other;
// a connection the user routes by hand holds the loop until it ends (finished, or skipped with
// `cancelCurrentItem`), one that completes on its own lets the loop go straight on, and Escape (`m_cancelled`)
// drops whatever is left.
//
//     for( item : itemList )
//         for( anchor : anchors )
//         {
//             performRouting();            // "live": the loop waits here
//             if( m_cancelled ) break;
//         }
//     frame->PopTool( pushedEvent );       // `onEnd`

/** What running one connection ended in: a live interactive session (the loop waits for it), or nothing left to wait for. */
export type QueueOutcome = "live" | "done";

export class RouteSelectedLoop {
  private rest: Array<() => Promise<QueueOutcome>>;
  private running = true;

  /** `runs[i]` starts connection i; `onEnd` runs once when the last one is over (never after `cancel`). */
  constructor(runs: Array<() => Promise<QueueOutcome>>, private readonly onEnd: () => void) {
    this.rest = runs.slice();
  }

  /** `m_inRouteSelected`. */
  get active(): boolean {
    return this.running;
  }

  /** How many connections have not been started yet. */
  get remaining(): number {
    return this.rest.length;
  }

  /** Start the next connection(s): runs until one leaves a live session, then returns; the caller calls this again when that session ends. */
  async advance(): Promise<void> {
    while (this.running) {
      const next = this.rest.shift();
      if (!next) {
        this.running = false;
        this.onEnd();
        return;
      }
      const outcome = await next();
      if (!this.running) return; // cancelled while that connection was starting
      if (outcome === "live") return;
    }
  }

  /** `m_cancelled = true`: the rest of the loop is dropped and `onEnd` does not run. */
  cancel(): void {
    this.running = false;
    this.rest = [];
  }
}
