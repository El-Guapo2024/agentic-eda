// `PCB_SELECTION_TOOL::RequestSelection` for every board tool (UI-LEFT-3 P3-2): with nothing selected it runs `selectionCursor`, whose `selectPoint` asks with the
// clarification menu when the heuristics leave several items under the cursor (`if( !doSelectionMenu( &collector ) ) return false;`, pcbnew/tools/
// pcb_selection_tool.cpp at 8303b2ad) -- and the tool goes on with what was chosen, or with nothing when the menu is dismissed. KiCad's menu is modal, so
// `RequestSelection` returns only after it; a browser menu cannot block a handler. So a handler that asks for the selection while the menu has to ask is
// stopped there (`SelectionMenuPending` is thrown out of `requestSelection`), the menu opens, and the chosen items replay the same action with
// `requestSelection` answering them (`ForcedSelection`). A dismissed menu runs nothing, as the C++ tool returns on an empty selection.

/** commonActions.ts' `ActionHandler` (not imported: this module stays free of React for the unit tests). */
export type ActionHandler = (arg?: unknown) => void;

/** Thrown by `requestSelection` when the clarification menu has to ask: `ask` opens it and calls `choose` with the items picked (never when dismissed). */
export class SelectionMenuPending extends Error {
  constructor(readonly ask: (choose: (ids: string[]) => void) => void) {
    super("the clarification menu is asking");
    this.name = "SelectionMenuPending";
  }
}

/** The items the clarification menu chose, answered by `requestSelection` while the action replays; `depth` counts the actions running (only they may ask). */
export interface ForcedSelection {
  ids: string[] | null;
  depth: number;
}

export function newForcedSelection(): ForcedSelection {
  return { ids: null, depth: 0 };
}

const isThenable = (v: unknown): v is PromiseLike<unknown> => !!v && typeof (v as { then?: unknown }).then === "function";

/**
 * `handler` as an action that may ask: run it; when it stops on `SelectionMenuPending` the menu opens, and its choice runs `handler` again with `forced.ids` set
 * (until the replay returns, or settles when it is async). An async handler's rejection with the pending menu is caught the same way, its promise resolving.
 */
export function askingHandler(handler: ActionHandler, forced: ForcedSelection): ActionHandler {
  const run = (arg: unknown, ids: string[] | null): unknown => {
    const previous = forced.ids;
    if (ids) forced.ids = ids;
    forced.depth++;
    const done = () => {
      forced.depth--;
      forced.ids = previous;
    };
    // the menu's choice replays the action with the items chosen; a dismissed menu (or nothing chosen) runs nothing
    const pending = (e: SelectionMenuPending) => e.ask((chosen) => void (chosen.length > 0 && run(arg, chosen)));
    let r: unknown;
    try {
      r = handler(arg);
    } catch (e) {
      done();
      if (e instanceof SelectionMenuPending) return ids ? undefined : void pending(e);
      throw e;
    }
    if (!isThenable(r)) {
      done();
      return r;
    }
    // `forced` holds until the promise settles: a handler may ask for the selection after an await
    return Promise.resolve(r).then(
      (v) => {
        done();
        return v;
      },
      (e: unknown) => {
        done();
        if (e instanceof SelectionMenuPending) return ids ? undefined : void pending(e);
        throw e;
      },
    );
  };
  return (arg?: unknown) => run(arg, null) as void;
}

/** Every handler of the registry made `askingHandler` (they share one `forced`). */
export function askingRegistry(m: Map<string, ActionHandler>, forced: ForcedSelection): void {
  for (const [id, handler] of m) m.set(id, askingHandler(handler, forced));
}
