// Migrate 3D Models when the board opens: `PCB_EDIT_FRAME::OnBoardLoaded` (pcbnew/pcb_edit_frame.cpp at 8303b2ad) -- "Migrate obsolete WRL 3D model references to current STEP models.
// Only runs in the GUI: CLI and scripting load paths must not mutate board state on load."
//
//   1. `DIALOG_MIGRATE_3D_MODELS::AutoMigrateByFilename( this )`: every `.wrl` / `.wrz` reference of the footprints that no search finds and that a STEP-family file of the same name stands
//      for is replaced by it, in one undo step that marks the board modified (`commit.Push( _( "Auto-migrate 3D model references" ) )`) -- no step when there is nothing to replace. The
//      server works it out (`GET /api/3dmodel/automigrate`, crates/cli/src/model3d_tab_api.rs `auto_migrate`); the page sends the step it answers (`migrate_board_models`), so it is an
//      edit of the person's like every other, and the agent's CLI never migrates on load.
//   2. `CountUnresolvedWrlReferences`: when references are left that nothing stands for, the load notice bar says "%d WRL 3D model(s) could not be matched ..." for ten seconds
//      (`ShowMessageFor( msg, 10000, wxICON_INFORMATION )`) with a "Show options" link -- the Migrate 3D Models dialog; when it closes and nothing is left unresolved the bar goes
//      ("Dismiss the infobar if nothing remains to resolve; otherwise leave it so the user can try again") -- and a close button.
//
// It runs once per page load, the first time the board is there (a page opened is the board opened). The studio's info bar is its toast, which has no place for a link: the load notice is a
// bar of its own with the toast's look, at the top of the canvas where KiCad's load notice bar is. Test hook: `window.__edaAutoMigrate3d` (`runs`, the last `notice`, `rerun()` -- the
// load-time pass again, for a check that prepares the board in the page).
import { useCallback, useEffect, useRef, useState, createElement } from "react";
import { fetchAutoMigration } from "../api/model3dClient";
import { postCmd } from "../api/client";
import { openSweepDialog, useSweepDialog } from "../actions/pcbSweepDialogs";
import { AUTO_MIGRATE_NOTICE_MS, autoMigrateNotice, savedSearchDirs } from "../kicad-port/migrate3d";
import { useStudioApi, useStudioState } from "../state/store";
import { Migrate3dModelsDialog } from "./Migrate3dModelsDialog";

/** One load-time pass, for the test hook. */
export interface AutoMigrateRun {
  /** The references replaced, old name to new. */
  replaced: Record<string, string>;
  /** Whether the step was made (false when there was nothing to replace, or the board refused it). */
  stepped: boolean;
  unresolved: number;
  notice: string | null;
  error?: string;
}

interface Hook {
  runs: AutoMigrateRun[];
  notice: string | null;
  rerun: () => Promise<AutoMigrateRun>;
}

declare global {
  interface Window {
    __edaAutoMigrate3d?: Hook;
  }
}

export function AutoMigrate3dModels() {
  const state = useStudioState();
  const api = useStudioApi();
  const ran = useRef(false);
  const [notice, setNotice] = useState<string | null>(null);
  /** The Migrate 3D Models dialog the link opened is up. */
  const [optionsOpen, setOptionsOpen] = useState(false);
  const sweep = useSweepDialog();

  const pass = useCallback(async (): Promise<AutoMigrateRun> => {
    const dirs = savedSearchDirs();
    try {
      const plan = await fetchAutoMigration(dirs);
      let stepped = false;
      let unresolved = plan.unresolved;
      if (plan.cmd) {
        // Not `api.cmd`: a refusal (another page of the board migrated it first) is no error to show; the step is the person's, as the dialog's Replace is.
        const reply = await postCmd(plan.cmd, false);
        stepped = reply.ok;
        if (reply.ok) await api.refresh();
        // `CountUnresolvedWrlReferences` after the commit
        unresolved = (await fetchAutoMigration(dirs)).unresolved;
      }
      const text = autoMigrateNotice(unresolved);
      setNotice(text);
      return { replaced: stepped ? plan.replacements : {}, stepped, unresolved, notice: text };
    } catch (e) {
      return { replaced: {}, stepped: false, unresolved: 0, notice: null, error: e instanceof Error ? e.message : String(e) };
    }
  }, [api]);

  const record = useCallback(
    async (): Promise<AutoMigrateRun> => {
      const run = await pass();
      const hook = window.__edaAutoMigrate3d;
      if (hook) {
        hook.runs.push(run);
        hook.notice = run.notice;
      }
      return run;
    },
    [pass]
  );

  useEffect(() => {
    window.__edaAutoMigrate3d = { runs: window.__edaAutoMigrate3d?.runs ?? [], notice: null, rerun: record };
  }, [record]);

  // `OnBoardLoaded`: once, the first time the page has the board
  useEffect(() => {
    if (ran.current || !state.board) return;
    ran.current = true;
    void record();
  }, [state.board, record]);

  // `ShowMessageFor( msg, 10000, .. )`
  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), AUTO_MIGRATE_NOTICE_MS);
    return () => clearTimeout(t);
  }, [notice]);

  // the dialog the link opened has closed: the bar goes when nothing is left unresolved
  useEffect(() => {
    if (!optionsOpen || sweep !== null) return;
    setOptionsOpen(false);
    void fetchAutoMigration(savedSearchDirs())
      .then((m) => {
        if (m.unresolved === 0) setNotice(null);
      })
      .catch(() => {});
  }, [optionsOpen, sweep]);

  useEffect(() => {
    const hook = window.__edaAutoMigrate3d;
    if (hook) hook.notice = notice;
  }, [notice]);

  if (!notice) return null;
  return (
    <div
      data-testid="automigrate-infobar"
      role="status"
      style={{
        position: "absolute",
        left: "50%",
        top: 8,
        transform: "translateX(-50%)",
        background: "#16263a",
        border: "1px solid #4aa3ff",
        color: "#d6e8ff",
        padding: "6px 10px",
        borderRadius: 6,
        maxWidth: "80%",
        boxShadow: "0 6px 20px rgba(0,0,0,.4)",
        zIndex: 2900,
        display: "flex",
        gap: 12,
        alignItems: "center",
      }}
    >
      <span>{notice}</span>
      <a
        href="#"
        data-testid="automigrate-show-options"
        style={{ color: "inherit" }}
        onClick={(e) => {
          e.preventDefault();
          setOptionsOpen(true);
          openSweepDialog({ kind: "element", element: createElement(Migrate3dModelsDialog) });
        }}
      >
        Show options
      </a>
      <button
        data-testid="automigrate-close"
        aria-label="Close"
        title="Close"
        onClick={() => setNotice(null)}
        style={{ background: "none", border: "none", color: "inherit", cursor: "pointer", fontSize: 14, lineHeight: 1 }}
      >
        ×
      </button>
    </div>
  );
}
