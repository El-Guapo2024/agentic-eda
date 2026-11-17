// The workspace routes (crates/cli/src/workspace.rs, studio.rs `workspace_route`): the projects of the folder `eda board serve --workspace <dir>`
// serves, the one open, and opening or creating one. Without `--workspace` the GET still names the one project served (`workspace: null`).

/** One project of the workspace, as File > Open Project... lists it. */
export interface WorkspaceProject {
  name: string;
  path: string;
  /** The last change to its design, milliseconds since 1970. */
  modified_ms: number;
  active: boolean;
}

/** `GET /api/workspace`. */
export interface WorkspaceInfo {
  /** The workspace folder, or null when the studio serves one project (`eda board serve -C dir`). */
  workspace: string | null;
  workspace_name?: string;
  /** The open project's name, or null when a workspace has none open. */
  active: string | null;
  /** Newest first. */
  projects: WorkspaceProject[];
  /** The projects opened, most recent first (File > Open Recent). */
  recent: string[];
}

/** `POST /api/workspace/open|new`'s answer. */
export interface WorkspaceReply {
  ok: boolean;
  active?: string;
  message?: string;
}

export async function getWorkspace(): Promise<WorkspaceInfo> {
  const r = await fetch("/api/workspace");
  if (!r.ok) throw new Error(`GET /api/workspace: ${r.status}`);
  return (await r.json()) as WorkspaceInfo;
}

async function post(path: string, name: string): Promise<WorkspaceReply> {
  const r = await fetch(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ name }) });
  if (!r.ok) return { ok: false, message: `${path}: ${r.status}` };
  return (await r.json()) as WorkspaceReply;
}

/** File > Open Project...: close the open project and open `name`. */
export function openProject(name: string): Promise<WorkspaceReply> {
  return post("/api/workspace/open", name);
}

/** File > New Project...: `<workspace>/<name>/` with an empty board and schematic, opened. */
export function newProject(name: string): Promise<WorkspaceReply> {
  return post("/api/workspace/new", name);
}
