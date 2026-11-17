// The workspace the server serves (`eda board serve --workspace <dir>`), the project open in it, and which of File > New Project... / Open Project...
// is showing. Plain module state with subscribers (like `state/commonDialogs.ts`), so the action registry and the menus read it without a place in the
// big reducer. `App.tsx` loads it before the editors mount (a workspace may have no project open, and then there are no editors to show) and
// `components/WorkspaceDialogs.tsx` renders the dialogs.
//
// Opening or creating a project switches the server's one open project in place (same address); the page then loads again, so every store starts
// from the new project (KiCad closes the editors' frames with the project: `KICAD_MANAGER_FRAME::CloseProject`). A page left open on the old project
// -- another browser tab, or an agent's `eda workspace open` -- sees the switch on its next look at the workspace and loads again too.
import { useSyncExternalStore } from "react";
import { getWorkspace, newProject, openProject, type WorkspaceInfo, type WorkspaceReply } from "../api/workspaceClient";

export type WorkspaceDialog = "new" | "open" | null;

export interface WorkspaceState {
  /** Null until the first answer (or when the server has no workspace route: an older server). */
  info: WorkspaceInfo | null;
  loaded: boolean;
  dialog: WorkspaceDialog;
  /** A switch is under way: the dialogs wait for the server, then the page loads again. */
  busy: boolean;
  /** The server's refusal of the last New / Open, shown in the dialog. */
  error: string | null;
}

let current: WorkspaceState = { info: null, loaded: false, dialog: null, busy: false, error: null };
const listeners = new Set<() => void>();

function set(patch: Partial<WorkspaceState>): void {
  current = { ...current, ...patch };
  for (const l of listeners) l();
}

export function getWorkspaceState(): WorkspaceState {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useWorkspace(): WorkspaceState {
  return useSyncExternalStore(subscribe, getWorkspaceState, getWorkspaceState);
}

/** Whether the server serves a workspace: File > New Project... / Open Project... are offered only then. */
export function inWorkspace(): boolean {
  return current.info?.workspace != null;
}

/** How often a page looks whether the workspace's open project is still the one it shows. */
const WATCH_MS = 3000;
let watching = false;

/** Read the workspace once (App.tsx, before the editors mount), and, in a workspace, keep looking whether another page or an agent opened another project. */
export async function loadWorkspace(): Promise<void> {
  try {
    set({ info: await getWorkspace(), loaded: true });
  } catch {
    set({ info: null, loaded: true });
  }
  if (inWorkspace() && !watching) {
    watching = true;
    const shown = current.info?.active ?? null;
    window.setInterval(() => {
      if (current.busy) return;
      getWorkspace()
        .then((info) => {
          if ((info.active ?? null) !== shown) window.location.reload();
          else set({ info });
        })
        .catch(() => {});
    }, WATCH_MS);
  }
}

export function openWorkspaceDialog(dialog: WorkspaceDialog): void {
  set({ dialog, error: null });
  // the list as it is now (a project made by an agent since the page loaded)
  if (dialog) getWorkspace().then((info) => set({ info }), () => {});
}

export function closeWorkspaceDialog(): void {
  if (!current.busy) set({ dialog: null, error: null });
}

async function switchTo(run: () => Promise<WorkspaceReply>): Promise<void> {
  set({ busy: true, error: null });
  const reply = await run().catch((e: unknown) => ({ ok: false, message: String(e) }) as WorkspaceReply);
  if (reply.ok) {
    // every store starts again from the project now open
    window.location.reload();
    return;
  }
  // shown in the dialog (Open Recent has none of its own: the Open Project list says why)
  set({ busy: false, error: reply.message ?? "The project could not be opened.", dialog: current.dialog ?? "open" });
}

/** File > Open Project... / Open Recent: the server closes the open project and opens `name`; the page loads again. */
export function switchToProject(name: string): Promise<void> {
  return switchTo(() => openProject(name));
}

/** File > New Project...: `<workspace>/<name>/`, empty, opened; the page loads again. */
export function createProject(name: string): Promise<void> {
  return switchTo(() => newProject(name));
}
