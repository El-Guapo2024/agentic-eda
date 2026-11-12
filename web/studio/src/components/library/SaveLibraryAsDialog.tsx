// Save Library As of the two library editors (state/librarySaveDialogs.ts says which library):
//  * Symbols: `SYMBOL_EDIT_FRAME::saveLibrary( aLibrary, true )` (eeschema/symbol_editor/symbol_editor.cpp) -- the file dialog "Save Library '%s' As..." with the
//    library's name for the file (`fn.SetName( aLibrary )`, `.kicad_sym`) and the table options of `SYMBOL_LIBRARY_SAVE_AS_FILEDLG_HOOK`; every symbol of the
//    library, the modified ones as the editor has them, goes into the new file, and the table gets a row for it ("Add new project library table entry",
//    `addLibTableEntry`) or the library's row is pointed at it ("Update existing library table entry", `replaceLibTableEntry`). A file that is there is replaced
//    only after the file dialog's question (`wxFD_OVERWRITE_PROMPT`).
//  * Footprints: `FOOTPRINT_EDIT_FRAME::SaveLibraryAs` (pcbnew/load_select_footprint.cpp) -- `CreateNewLibrary( _( "Save Footprint Library As" ), .. )` makes the
//    `.pretty` folder and its row, then every footprint of the library is saved into it; "Footprint library '%s' saved as '%s'.".
//  * KiCad 25ec2335's Save Library As Packed (a folder library into one `.kicad_sym`, the same file dialog and table choice, "Save Library '%s' As Packed
//    Library...") and Save Library As Unpacked (a file library into a folder, a symbol per file, `wxDirDialog` with no table choice: the tables are not updated).
// The table choice is the hook's four, "Do not update library tables" first and selected; the footprint one is `FILEDLG_HOOK_NEW_LIBRARY`'s two (project first
// selected, `isGlobal = false`). UI-LEFT-2 LL5. Differs: the file dialog is a folder and a name over the server's folders (a page cannot see the machine's).
import { useEffect, useState } from "react";
import { saveLibraryAs, type SaveLibraryAsRequest } from "../../api/librarySaveClient";
import { fetchLibraryTables } from "../../api/libraryTablesClient";
import { FOOTPRINT_SAVE_LIBRARY_AS_TITLE, isTableLibrary, NEW_LIBRARY_TABLE_OPTIONS, saveLibraryAsDefaultName, saveLibraryAsPackedTitle, saveLibraryAsTitle, saveLibraryAsUnpackedTitle, TABLE_OPTIONS, type TableOption } from "../../kicad-port/librarySave";
import { closeSaveLibraryAs, useSaveLibraryAs, type SaveLibraryAsUi } from "../../state/librarySaveDialogs";
import { selectLibraries, setLibraryOpen } from "../../state/libraryTree";
import { useStudioDispatch } from "../../state/store";
import { FolderBrowser } from "../libraryTables/FolderBrowser";
import { notifyLibraryChanged } from "./useLibraryNames";
import "../../styles/libraryTables.css";

export function SaveLibraryAsDialog() {
  const req = useSaveLibraryAs();
  return req ? <Body key={`${req.kind}:${req.lib}:${req.mode ?? ""}`} req={req} /> : null;
}

function Body({ req }: { req: SaveLibraryAsUi }) {
  const dispatch = useStudioDispatch();
  const symbol = req.kind === "symbol";
  const unpacked = symbol && req.mode === "unpacked";
  const title = !symbol ? FOOTPRINT_SAVE_LIBRARY_AS_TITLE : req.mode === "packed" ? saveLibraryAsPackedTitle(req.lib) : unpacked ? saveLibraryAsUnpackedTitle(req.lib) : saveLibraryAsTitle(req.lib);
  const [folder, setFolder] = useState("");
  // Unpacked: a folder (`wxDirDialog`), named after the library to begin with
  const [name, setName] = useState(unpacked ? `${req.lib}.kicad_symdir` : saveLibraryAsDefaultName(req.kind, req.lib));
  // the hook's choice: "Do not update library tables" to begin with (`NORMAL_SAVE_AS`); the footprint one's project table (`isGlobal = false`)
  const [table, setTable] = useState<TableOption>("none");
  const [fpGlobal, setFpGlobal] = useState(false);
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
      const request: SaveLibraryAsRequest = { kind: req.kind, lib: req.lib, path: target, ...(symbol ? (unpacked ? { mode: "unpacked" as const } : { table, ...(req.mode ? { mode: req.mode } : {}) }) : { global: fpGlobal }) };
      let answer = await saveLibraryAs(request);
      if (!answer.ok && answer.confirm) {
        if (!(await ask(answer.error, answer.confirm))) return;
        answer = await saveLibraryAs({ ...request, overwrite: true });
      }
      if (answer.ok) {
        // "Footprint library '%s' saved as '%s'." is `DisplayInfoMessage`; the Symbol Editor's `saveLibrary` formats "Symbol library file '%s' saved." and shows
        // nothing (UI-LEFT-2 LL6)
        if (!symbol) dispatch({ type: "TOAST", message: answer.message, kind: "info" });
        // `SyncLibraries( true )` / `FocusOnLibID`: the new library is listed and selected in the tree (none when the tables were not updated)
        if (answer.nickname) {
          setLibraryOpen(req.kind, answer.nickname, true);
          selectLibraries(req.kind, [answer.nickname]);
        }
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
          {symbol && !unpacked && (
            <div role="radiogroup" aria-label="Library table" style={{ marginTop: 10, display: "grid", gap: 4 }} data-save-library-table>
              {TABLE_OPTIONS.filter((o) => o.value !== "replace" || isTableLibrary(req.lib)).map((o) => (
                <label key={o.value}>
                  <input type="radio" name="sla-table" value={o.value} checked={table === o.value} onChange={() => setTable(o.value)} /> {o.label}
                </label>
              ))}
            </div>
          )}
          {!symbol && (
            <div role="radiogroup" aria-label="Library table" style={{ marginTop: 10, display: "grid", gap: 4 }} data-save-library-table>
              {NEW_LIBRARY_TABLE_OPTIONS.map((o) => (
                <label key={o.value}>
                  <input type="radio" name="sla-fp-table" value={o.value} checked={fpGlobal === (o.value === "global")} onChange={() => setFpGlobal(o.value === "global")} /> {o.label}
                </label>
              ))}
            </div>
          )}
          <div className="lt-note" style={{ marginTop: 8 }}>
            {unpacked
              ? `Writes every symbol of '${req.lib}' (with the changes not saved yet) into the folder ${target}, a file per symbol; the library tables are not updated.`
              : symbol
                ? `Writes every symbol of '${req.lib}' (with the changes not saved yet) to ${target}${table === "replace" ? " and points the library's table row at it." : table === "global" ? ", and adds it to KiCad's global sym-lib-table." : table === "project" ? ", and adds it to the project's sym-lib-table." : "; the library tables are not updated."}`
                : `Makes ${target}${target.endsWith(".pretty") ? "" : ".pretty"}, adds it to the ${fpGlobal ? "global" : "project's"} fp-lib-table and saves every footprint of '${req.lib}' into it.`}
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
