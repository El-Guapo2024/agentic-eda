// Save Library As of the two library editors (state/librarySaveDialogs.ts says which library):
//  * Symbols: `SYMBOL_EDIT_FRAME::saveLibrary( aLibrary, true )` (eeschema/symbol_editor/symbol_editor.cpp) -- the file dialog "Save Library '%s' As..." with the
//    library's name for the file (`fn.SetName( aLibrary )`, `.kicad_sym`) and the table options of `SYMBOL_LIBRARY_SAVE_AS_FILEDLG_HOOK`; every symbol of the
//    library, the modified ones as the editor has them, goes into the new file, and the table gets a row for it ("Add new project library table entry",
//    `addLibTableEntry`) or the library's row is pointed at it ("Update existing library table entry", `replaceLibTableEntry`). A file that is there is replaced
//    only after the file dialog's question (`wxFD_OVERWRITE_PROMPT`).
//  * Footprints: `FOOTPRINT_EDIT_FRAME::SaveLibraryAs` (pcbnew/load_select_footprint.cpp) -- `CreateNewLibrary( _( "Save Footprint Library As" ), .. )` makes the
//    `.pretty` folder and its row, then every footprint of the library is saved into it; "Footprint library '%s' saved as '%s'.".
// Differs: the file dialog is a folder and a name over the server's folders (a page cannot see the machine's); KiCad's "Do not update library tables" and "Add new
// global library table entry" are not offered (a library file no table names is not written, and KiCad's global table is read only here).
import { useEffect, useState } from "react";
import { saveLibraryAs, type SaveLibraryAsRequest } from "../../api/librarySaveClient";
import { fetchLibraryTables } from "../../api/libraryTablesClient";
import { FOOTPRINT_SAVE_LIBRARY_AS_TITLE, isTableLibrary, saveLibraryAsDefaultName, saveLibraryAsTitle, TABLE_OPTIONS, type TableOption } from "../../kicad-port/librarySave";
import { closeSaveLibraryAs, useSaveLibraryAs, type SaveLibraryAsUi } from "../../state/librarySaveDialogs";
import { selectLibraries, setLibraryOpen } from "../../state/libraryTree";
import { useStudioDispatch } from "../../state/store";
import { FolderBrowser } from "../libraryTables/FolderBrowser";
import { notifyLibraryChanged } from "./useLibraryNames";
import "../../styles/libraryTables.css";

export function SaveLibraryAsDialog() {
  const req = useSaveLibraryAs();
  return req ? <Body key={`${req.kind}:${req.lib}`} req={req} /> : null;
}

function Body({ req }: { req: SaveLibraryAsUi }) {
  const dispatch = useStudioDispatch();
  const symbol = req.kind === "symbol";
  const title = symbol ? saveLibraryAsTitle(req.lib) : FOOTPRINT_SAVE_LIBRARY_AS_TITLE;
  const [folder, setFolder] = useState("");
  const [name, setName] = useState(saveLibraryAsDefaultName(req.kind, req.lib));
  // the project library has no row of its own to point elsewhere
  const [table, setTable] = useState<TableOption>("project");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [browse, setBrowse] = useState(false);
  const [question, setQuestion] = useState<{ message: string; okLabel: string; resolve: (yes: boolean) => void } | null>(null);
  const ask = (message: string, okLabel: string) => new Promise<boolean>((resolve) => setQuestion({ message, okLabel, resolve }));

  useEffect(() => {
    let cancelled = false;
    fetchLibraryTables()
      .then((v) => !cancelled && setFolder(v.project_dir))
      .catch((e: unknown) => !cancelled && setError(e instanceof Error ? e.message : String(e)));
    return () => {
      cancelled = true;
    };
  }, []);

  const close = () => closeSaveLibraryAs();
  const target = `${folder.replace(/\/+$/, "")}/${name.trim()}`;

  const save = async () => {
    setError(null);
    if (name.trim() === "") return setError("Give the library a file name.");
    setBusy(true);
    try {
      const request: SaveLibraryAsRequest = { kind: req.kind, lib: req.lib, path: target, ...(symbol ? { table } : {}) };
      let answer = await saveLibraryAs(request);
      if (!answer.ok && answer.confirm) {
        if (!(await ask(answer.error, answer.confirm))) return;
        answer = await saveLibraryAs({ ...request, overwrite: true });
      }
      if (answer.ok) {
        dispatch({ type: "TOAST", message: answer.message, kind: "info" });
        // `SyncLibraries( true )` / `FocusOnLibID`: the new library is listed and selected in the tree
        setLibraryOpen(req.kind, answer.nickname, true);
        selectLibraries(req.kind, [answer.nickname]);
        notifyLibraryChanged();
        close();
      } else {
        setError(answer.detail ? `${answer.error}\n${answer.detail}` : answer.error);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog lt-small" onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Escape" && !browse && !question && close()} role="dialog" aria-label={title}>
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body">
          <div className="lt-form">
            <label htmlFor="sla-folder">Folder:</label>
            <input id="sla-folder" type="text" value={folder} spellCheck={false} onChange={(e) => setFolder(e.target.value)} />
            <button onClick={() => setBrowse(true)}>Browse...</button>
            <label htmlFor="sla-name">Name:</label>
            <input id="sla-name" type="text" autoFocus value={name} spellCheck={false} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void save()} />
            <span />
          </div>
          {symbol && (
            <div role="radiogroup" aria-label="Library table" style={{ marginTop: 10, display: "grid", gap: 4 }}>
              {TABLE_OPTIONS.filter((o) => o.value !== "replace" || isTableLibrary(req.lib)).map((o) => (
                <label key={o.value}>
                  <input type="radio" name="sla-table" checked={table === o.value} onChange={() => setTable(o.value)} /> {o.label}
                </label>
              ))}
            </div>
          )}
          <div className="lt-note" style={{ marginTop: 8 }}>
            {symbol
              ? `Writes every symbol of '${req.lib}' (with the changes not saved yet) to ${target}${table === "replace" ? ` and points the library's table row at it.` : `, and adds it to the project's sym-lib-table.`}`
              : `Makes ${target}${target.endsWith(".pretty") ? "" : ".pretty"}, adds it to the project's fp-lib-table and saves every footprint of '${req.lib}' into it.`}
          </div>
          {error && (
            <div className="lt-error" role="alert" style={{ marginTop: 8, whiteSpace: "pre-wrap" }}>
              {error}
            </div>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={busy} onClick={() => void save()}>
            Save
          </button>
        </div>
      </div>
      {question && (
        <div className="dialog-backdrop" onClick={(e) => e.stopPropagation()}>
          <div className="dialog" style={{ width: 440 }} role="dialog" aria-label="Confirm Save As" onKeyDown={(e) => e.key === "Escape" && (question.resolve(false), setQuestion(null))}>
            <div className="dialog-header">
              <span>Confirm Save As</span>
            </div>
            <div className="dialog-body" style={{ whiteSpace: "pre-wrap" }}>
              {question.message}
            </div>
            <div className="dialog-footer">
              <button
                onClick={() => {
                  question.resolve(false);
                  setQuestion(null);
                }}
              >
                Cancel
              </button>
              <button
                className="primary"
                autoFocus
                onClick={() => {
                  question.resolve(true);
                  setQuestion(null);
                }}
              >
                {question.okLabel}
              </button>
            </div>
          </div>
        </div>
      )}
      {browse && (
        <div onClick={(e) => e.stopPropagation()}>
          <FolderBrowser
            title="Select Folder"
            what="folder"
            start={folder}
            onPick={(p) => {
              setBrowse(false);
              setFolder(p);
            }}
            onCancel={() => setBrowse(false)}
          />
        </div>
      )}
    </div>
  );
}
