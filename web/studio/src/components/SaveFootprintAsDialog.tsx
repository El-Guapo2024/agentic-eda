// `SAVE_AS_DIALOG` ("Save Footprint As", pcbnew/footprint_libraries_utils.cpp), the dialog behind the Footprint Editor's Save As (`common.Control.saveAs`,
// `FOOTPRINT_EDITOR_CONTROL::SaveAs` -> `FOOTPRINT_EDIT_FRAME::SaveFootprintAs`): the name the footprint is saved under, the library it goes in ("Save in
// library:", `GetLibraryItemsForListDialog`: every library of the tables with its description, the pinned ones first, and the project library) and "New
// Library...". OK ("Save") runs the validator in KiCad's order -- "A library must be specified.", "Footprint must have a name." -- and a footprint the library
// already has (`FootprintExists`: the design's copy or the library's file) asks "Footprint %s already exists in %s." with an Overwrite button before it is replaced.
// The footprint is stored as one `put_library_footprint` (undoable) and, for a library of the tables, written into its folder (`SaveFootprintInLibrary`; a folder
// that cannot be written: "Library '%s' is read only."); when it was the loaded one the editor opens the copy, as `SetFPID` makes the loaded footprint the new one.
// New Library... is `CreateNewLibrary( _( "New Footprint Library" ) )`: this dialog closes, the New Library dialog makes the `.pretty` and its row, and this
// dialog comes back on it with the name typed. See kicad-port/saveFootprintAs.ts.
import { useEffect, useMemo, useState } from "react";
import { fetchFootprintLibraryNames } from "../api/client";
import { fetchAnyFootprint } from "../api/libraryClient";
import { fetchLibraryItemsNow, fetchWritableLibraries, saveToLibrary, type WritableLibrary } from "../api/librarySaveClient";
import { useFpApi, useFpDispatch } from "../state/footprintEditorStore";
import { setSaveFootprintAs, useCommonDialogs, type SaveFootprintAsRequest } from "../state/commonDialogs";
import { openLibraryFile } from "../state/libraryTableDialogs";
import { useLibraryTree } from "../state/libraryTree";
import { footprintExists, isTableLibrary, libraryRowLabel, saveAsLibraryRows } from "../kicad-port/librarySave";
import { typedFootprintName, existsMessage, saveAsDefaults, saveAsError, savedFootprintId, savedMessage } from "../kicad-port/saveFootprintAs";
import { notifyLibraryChanged } from "./library/useLibraryNames";
import { storableFootprint } from "../kicad-port/fpLibFields";
import { setLibraryStatusText } from "../state/libraryStatus";

export function SaveFootprintAsDialog() {
  const req = useCommonDialogs().saveFootprintAs;
  if (!req) return null;
  return <SaveFootprintAsBody req={req} />;
}

function SaveFootprintAsBody({ req }: { req: SaveFootprintAsRequest }) {
  const api = useFpApi();
  const dispatch = useFpDispatch();
  const { pinned } = useLibraryTree("footprint");
  const start = saveAsDefaults(req.name);
  const [name, setName] = useState(req.again?.name ?? start.item);
  const [lib, setLib] = useState(req.again ? req.again.lib : start.lib);
  const [tables, setTables] = useState<WritableLibrary[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** The footprint exists in the library: the dialog is asking before it replaces it. */
  const [confirm, setConfirm] = useState(false);

  useEffect(() => {
    let stopped = false;
    void fetchWritableLibraries("footprint")
      .then((l) => !stopped && setTables(l))
      .catch(() => undefined);
    return () => {
      stopped = true;
    };
  }, []);

  const libs = useMemo(() => saveAsLibraryRows(tables, pinned, [start.lib, ...(req.again?.lib ? [req.again.lib] : [])]), [tables, pinned, start.lib, req.again]);

  const close = () => setSaveFootprintAs(null);

  // `ID_MAKE_NEW_LIBRARY`: the dialog ends, `CreateNewLibrary` makes the library and its row, and the dialog is shown again on it (`libraryName = fn.GetName()`)
  const newLibrary = () => {
    const typed = name;
    close();
    openLibraryFile("footprint", true, (made) => setSaveFootprintAs({ ...req, again: { name: typed, lib: made ?? "" } }));
  };

  /** `SaveFootprintInLibrary` and what follows it in `SaveFootprintAs`: the footprint is stored under its new id (and in its library's folder), the editor moves to it. */
  const store = async (replacing: boolean) => {
    setBusy(true);
    try {
      const id = savedFootprintId(lib, name);
      const { footprint } = await fetchAnyFootprint(req.name);
      // the dialog has ended (`ret == wxID_OK`) and the footprint takes its new id (`SetFPID`): a library that cannot be written refuses the save after that
      // (`SaveFootprintInLibrary` -> `FootprintSave`'s "Library '%s' is read only." in a message box), and the footprint keeps the new id (UI-LEFT-2 LL6)
      const ok = await api.cmd({ op: "put_library_footprint", footprint: { ...storableFootprint(footprint), name: id, published: false }, overwrite: replacing });
      notifyLibraryChanged();
      if (!ok) return;
      close();
      if (req.loaded) await api.openFootprint(id);
      dispatch({ type: "SET_TREE_SELECTION", name: id });
      if (isTableLibrary(lib)) {
        const written = await saveToLibrary({ kind: "footprint", id });
        if (!written.ok) return dispatch({ type: "TOAST", message: written.error, kind: "error" });
      }
      // "Footprint '%s' added to '%s'" / "replaced in", the status bar's (`SetStatusText`)
      setLibraryStatusText(savedMessage(lib, name, replacing));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setConfirm(false);
    }
  };

  const save = async () => {
    const bad = saveAsError(lib, name);
    if (bad) return setError(bad);
    setError(null);
    setBusy(true);
    let exists = false;
    try {
      // `FootprintExists( newLib, newName )`: the design's copy, or the library's file
      const [names, items] = await Promise.all([fetchFootprintLibraryNames(), isTableLibrary(lib) ? fetchLibraryItemsNow("footprint", lib) : Promise.resolve([])]);
      exists = footprintExists(lib, typedFootprintName(name), items, names.names);
    } catch {
      /* the verb refuses a taken name anyway */
    } finally {
      setBusy(false);
    }
    if (exists) setConfirm(true);
    else await store(false);
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 420 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Save Footprint As">
        <div className="dialog-header">
          <span>Save Footprint As</span>
        </div>
        <div className="dialog-body">
          <label style={{ display: "block", marginBottom: 4 }}>Name:</label>
          <input
            aria-label="Name"
            autoFocus
            value={name}
            disabled={confirm}
            style={{ width: "100%", boxSizing: "border-box" }}
            onFocus={(e) => e.currentTarget.select()}
            onChange={(e) => {
              setName(e.target.value);
              setError(null);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") void save();
              if (e.key === "Escape") close();
            }}
          />
          <label style={{ display: "block", margin: "10px 0 4px" }}>Save in library:</label>
          <div role="listbox" aria-label="Save in library" style={{ border: "1px solid var(--chrome-border)", maxHeight: 180, overflowY: "auto" }}>
            {libs.map((r) => (
              <div
                key={r.name}
                role="option"
                aria-selected={r.name === lib}
                data-lib={r.name}
                onClick={() => {
                  if (confirm) return;
                  setLib(r.name);
                  setError(null);
                }}
                style={{ display: "flex", gap: 12, padding: "3px 8px", cursor: "pointer", background: r.name === lib ? "var(--chrome-selected-bg)" : undefined, color: r.name === lib ? "var(--chrome-selected-text)" : undefined }}
              >
                <span style={{ minWidth: 120 }}>{libraryRowLabel(r)}</span>
                <span style={{ opacity: 0.7, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{r.description}</span>
              </div>
            ))}
          </div>
          {confirm && (
            <p role="alert" style={{ margin: "10px 0 0" }}>
              {existsMessage(lib, name)}
            </p>
          )}
          {error && <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }}>{error}</p>}
        </div>
        <div className="dialog-footer">
          {confirm ? (
            <>
              <button onClick={() => setConfirm(false)}>Cancel</button>
              <button className="primary" disabled={busy} onClick={() => void store(true)}>
                Overwrite
              </button>
            </>
          ) : (
            <>
              <button onClick={newLibrary} style={{ marginRight: "auto" }}>
                New Library...
              </button>
              <button onClick={close}>Cancel</button>
              <button className="primary" disabled={busy} onClick={() => void save()}>
                Save
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
