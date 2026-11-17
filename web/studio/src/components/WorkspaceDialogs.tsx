// File > New Project... and File > Open Project... of a workspace (`eda board serve --workspace <dir>`), and the page shown while the workspace has no
// project open. KiCad 25ec2335 `kicad/tools/kicad_manager_control.cpp`: NewProject asks for the new project's file in a "Create New Project" file
// dialog (`newProjectDirectory`, with "Create a new folder for the project" on: the project gets a folder of its name) and OpenProject picks a
// `.kicad_pro` in an "Open Existing Project" file dialog. A browser cannot browse the server's disk, so both dialogs work inside the workspace: New asks
// for the name only (the folder is `<workspace>/<name>/`), Open lists the workspace's projects, the most recently changed first. Every edit is already
// in design.json, so closing the open project asks nothing (KiCad's `CloseProject` asks to save modified documents).
import { useEffect, useRef, useState } from "react";
import { closeWorkspaceDialog, createProject, openWorkspaceDialog, switchToProject, useWorkspace } from "../state/workspace";
import { modifiedText, projectCaption, projectNameProblem } from "../kicad-port/workspace";

function ErrorText({ text }: { text: string | null }) {
  if (!text) return null;
  return (
    <div data-testid="workspace-error" style={{ fontSize: 12, color: "var(--chrome-error, #e55)", whiteSpace: "pre-line" }}>
      {text}
    </div>
  );
}

function NewProjectDialog() {
  const ws = useWorkspace();
  const [name, setName] = useState("");
  const [touched, setTouched] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ws.dialog === "new") {
      setName("");
      setTouched(false);
      input.current?.focus();
    }
  }, [ws.dialog]);
  if (ws.dialog !== "new" || !ws.info?.workspace) return null;
  const taken = ws.info.projects.some((p) => p.name === name) ? `A project named '${name}' already exists in the workspace.` : null;
  const problem = projectNameProblem(name) ?? taken;
  const create = () => {
    setTouched(true);
    if (!problem && !ws.busy) void createProject(name);
  };
  const folder = `${ws.info.workspace}/${name}`;
  return (
    <div className="dialog-backdrop" onClick={closeWorkspaceDialog}>
      <div className="dialog" style={{ width: 480 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Create New Project" data-testid="new-project-dialog">
        <div className="dialog-header">
          <span>Create New Project</span>
        </div>
        <div className="dialog-body" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <label style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span>Project name:</span>
            <input
              ref={input}
              data-testid="new-project-name"
              style={{ flex: 1 }}
              value={name}
              onChange={(e) => {
                setName(e.target.value);
                setTouched(true);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") create();
                if (e.key === "Escape") closeWorkspaceDialog();
              }}
            />
          </label>
          <div style={{ fontSize: 11, color: "var(--chrome-text-dim)" }}>{`The project's folder: ${folder}/`}</div>
          <ErrorText text={(touched ? problem : null) ?? ws.error} />
        </div>
        <div className="dialog-footer">
          <button onClick={closeWorkspaceDialog} disabled={ws.busy}>
            Cancel
          </button>
          <button className="primary" data-testid="new-project-ok" onClick={create} disabled={ws.busy || (touched && problem != null)}>
            {ws.busy ? "Creating..." : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}

function OpenProjectDialog() {
  const ws = useWorkspace();
  const [chosen, setChosen] = useState<string | null>(null);
  useEffect(() => {
    if (ws.dialog === "open") setChosen(null);
  }, [ws.dialog]);
  if (ws.dialog !== "open" || !ws.info?.workspace) return null;
  const projects = ws.info.projects;
  const pick = chosen ?? projects.find((p) => !p.active)?.name ?? projects[0]?.name ?? null;
  const open = (name: string | null) => {
    if (name && !ws.busy) void switchToProject(name);
  };
  return (
    <div className="dialog-backdrop" onClick={closeWorkspaceDialog}>
      <div className="dialog" style={{ width: 560 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Open Existing Project" data-testid="open-project-dialog">
        <div className="dialog-header">
          <span>Open Existing Project</span>
        </div>
        <div className="dialog-body" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <div style={{ fontSize: 11, color: "var(--chrome-text-dim)" }}>{ws.info.workspace}</div>
          <div style={{ border: "1px solid var(--chrome-border)", maxHeight: 320, overflowY: "auto" }} role="listbox" aria-label="Projects">
            {projects.length === 0 && <div style={{ padding: 8, fontStyle: "italic", color: "var(--chrome-text-dim)" }}>The workspace has no projects yet: File &gt; New Project... makes one.</div>}
            {projects.map((p) => (
              <div
                key={p.name}
                role="option"
                aria-selected={pick === p.name}
                data-testid={`open-project-${p.name}`}
                onClick={() => setChosen(p.name)}
                onDoubleClick={() => open(p.name)}
                style={{ display: "flex", justifyContent: "space-between", gap: 12, padding: "3px 8px", cursor: "default", background: pick === p.name ? "var(--chrome-active)" : undefined }}
              >
                <span style={{ fontWeight: p.active ? 700 : 400 }}>{p.active ? `${p.name} (open)` : p.name}</span>
                <span style={{ fontSize: 11, color: "var(--chrome-text-dim)" }}>{modifiedText(p.modified_ms)}</span>
              </div>
            ))}
          </div>
          <ErrorText text={ws.error} />
        </div>
        <div className="dialog-footer">
          <button onClick={closeWorkspaceDialog} disabled={ws.busy}>
            Cancel
          </button>
          <button className="primary" data-testid="open-project-ok" onClick={() => open(pick)} disabled={ws.busy || pick == null}>
            {ws.busy ? "Opening..." : "Open"}
          </button>
        </div>
      </div>
    </div>
  );
}

export function WorkspaceDialogs() {
  return (
    <>
      <NewProjectDialog />
      <OpenProjectDialog />
    </>
  );
}

/**
 * The workspace with no project open (`eda board serve --workspace <dir>` with none opened before): the project manager's window with "[no project
 * loaded]" in its caption, and its two ways in.
 */
export function NoProjectScreen() {
  const ws = useWorkspace();
  useEffect(() => {
    document.title = projectCaption(null, "pcb");
  }, []);
  return (
    <div data-testid="no-project" style={{ height: "100vh", display: "flex", alignItems: "center", justifyContent: "center", background: "var(--chrome-bg, #1e1e1e)", color: "var(--chrome-text, #ddd)" }}>
      <div style={{ display: "flex", flexDirection: "column", gap: 12, alignItems: "center" }}>
        <div style={{ fontSize: 16 }}>No project is open.</div>
        <div style={{ fontSize: 12, color: "var(--chrome-text-dim)" }}>{ws.info?.workspace}</div>
        <div style={{ display: "flex", gap: 8 }}>
          <button data-testid="no-project-new" onClick={() => openWorkspaceDialog("new")}>
            New Project...
          </button>
          <button data-testid="no-project-open" onClick={() => openWorkspaceDialog("open")} disabled={(ws.info?.projects.length ?? 0) === 0}>
            Open Project...
          </button>
        </div>
      </div>
      <WorkspaceDialogs />
    </div>
  );
}
