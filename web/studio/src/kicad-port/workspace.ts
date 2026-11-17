// The pure parts of the workspace's project management (KiCad 25ec2335 `kicad/`: KICAD_MANAGER_CONTROL's NewProject / OpenProject, the File menu of
// `kicad/menubar.cpp`, the caption of `KICAD_MANAGER_FRAME::LoadProject` and of the editors' `UpdateTitle`). The server checks a name again
// (crates/cli/src/workspace.rs `check_name`); the dialog checks it first so the person sees why at once.
import type { MenuNode, MenusFile } from "../kicad/types";

/** The action an Open Recent entry runs, with the project's name as its argument. */
export const OPEN_RECENT_ACTION = "studio.Workspace.openRecent";

/** `check_name` of crates/cli/src/workspace.rs, word for word: a project is a folder of the workspace, named, never a path. Null when the name is good. */
export function projectNameProblem(name: string): string | null {
  const trimmed = name.trim();
  if (trimmed === "") return "Please enter a project name.";
  if (name !== trimmed) return `Project name '${name}' must not start or end with spaces.`;
  if (/[/\\]/.test(name) || name.includes("..")) return `Project name '${name}' must not contain path separators or '..'.`;
  // eslint-disable-next-line no-control-regex
  if (name.startsWith(".") || /[\u0000-\u001f\u007f]/.test(name)) return `Project name '${name}' is not a valid folder name.`;
  return null;
}

/** The editors' captions (`GetAboutTitle()` of each frame), the window's title after the project's name. */
const EDITOR_TITLES: Record<string, string> = {
  pcb: "PCB Editor",
  schematic: "Schematic Editor",
  footprint: "Footprint Editor",
  symbol: "Symbol Editor",
  "3d": "3D Viewer",
};

/**
 * The page's title, as KiCad's window caption: `PCB_EDIT_FRAME::UpdateTitle` "<name> — PCB Editor" (the board is named after its project), the other
 * editors alike; with no project open, `KICAD_MANAGER_FRAME::LoadProject`'s "[no project loaded] — KiCad" with this program's name.
 */
export function projectCaption(project: string | null, tab: string): string {
  if (!project) return "[no project loaded] — EDA Studio";
  return `${project} — ${EDITOR_TITLES[tab] ?? EDITOR_TITLES.pcb}`;
}

/**
 * The board and schematic editors' File menu when the studio serves a workspace: its New... and Open... are the project manager's (the editors run under
 * it, where KiCad's own editor frames leave them out: `Kiface().IsSingle()`), so they read "New Project..." and "Open Project..."
 * (`KICAD_MANAGER_ACTIONS::newProject` / `openProject`), and "Open Recent" follows them (`kicad/menubar.cpp`: the file history's submenu, the projects
 * opened most recently first, disabled when empty). The generated menus stay KiCad's; this is applied when the menu is shown.
 */
export function withWorkspaceFileMenu(file: MenusFile, recent: readonly string[]): MenusFile {
  const recentMenu: MenuNode = {
    type: "submenu",
    label: "Open Recent",
    items: recent.length > 0 ? recent.map((name) => ({ type: "item", action: OPEN_RECENT_ACTION, arg: name, label: name })) : [{ type: "item", action: OPEN_RECENT_ACTION, label: "(empty)", disabled: true }],
  };
  return {
    ...file,
    menus: file.menus.map((m) => {
      if (m.label !== "File") return m;
      const items: MenuNode[] = [];
      for (const n of m.items) {
        if (n.type === "item" && n.action === "common.Control.new") items.push({ ...n, label: "New Project..." });
        else if (n.type === "item" && n.action === "common.Control.open") items.push({ ...n, label: "Open Project..." }, recentMenu);
        else items.push(n);
      }
      return { ...m, items };
    }),
  };
}

/** File > Open Project...'s "Modified" column: the project's last change, in the browser's locale. */
export function modifiedText(ms: number): string {
  return ms > 0 ? new Date(ms).toLocaleString() : "";
}
