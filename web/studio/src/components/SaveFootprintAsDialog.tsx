// `SAVE_AS_DIALOG` ("Save Footprint As", pcbnew/footprint_libraries_utils.cpp), the dialog behind the Footprint Editor's Save As (`common.Control.saveAs`,
// `FOOTPRINT_EDITOR_CONTROL::SaveAs` -> `FOOTPRINT_EDIT_FRAME::SaveFootprintAs`): the name the footprint is saved under, the library it goes in ("Save in
// library:", the nicknames of the tree) and "New Library...". OK ("Save") runs the validator in KiCad's order -- "A library must be specified.", "Footprint must
// have a name." -- and a footprint the library already has asks "Footprint %s already exists in %s." with an Overwrite button before it is replaced. The footprint
// is stored as one `put_library_footprint` (undoable); when it was the loaded one the editor opens the copy, as `SetFPID` makes the loaded footprint the new one.
// The two questions (a new library's nickname, the overwrite) are asked inside this dialog, so nothing sits behind it. See kicad-port/saveFootprintAs.ts.
import { useMemo, useState } from "react";
import { fetchFootprintLibraryNames } from "../api/client";
import { fetchAnyFootprint } from "../api/libraryClient";
import { useFpApi, useFpDispatch } from "../state/footprintEditorStore";
import { setSaveFootprintAs, useCommonDialogs } from "../state/commonDialogs";
import { libraryGroups, libraryNicknameIllegalChar, PROJECT_LIBRARY } from "../kicad-port/libraryNames";
import { existsMessage, saveAsDefaults, saveAsError, savedFootprintId, savedMessage } from "../kicad-port/saveFootprintAs";
import { notifyLibraryChanged, useLibraryNames } from "./library/useLibraryNames";

export function SaveFootprintAsDialog() {
  const req = useCommonDialogs().saveFootprintAs;
  if (!req) return null;
  return <SaveFootprintAsBody req={req} />;
}

function SaveFootprintAsBody({ req }: { req: { name: string; loaded: boolean } }) {
  const api = useFpApi();
  const dispatch = useFpDispatch();
  const { items } = useLibraryNames("footprint");
  const start = saveAsDefaults(req.name);
  const [name, setName] = useState(start.item);
  const [lib, setLib] = useState(start.lib);
  const [extraLibs, setExtraLibs] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** "New Library...": the nickname being typed, or null when that row is not shown. */
  const [newLib, setNewLib] = useState<string | null>(null);
  /** The footprint exists in the library: the dialog is asking before it replaces it. */
  const [confirm, setConfirm] = useState(false);

  const libs = useMemo(() => {
    const out = new Set<string>([PROJECT_LIBRARY, start.lib]);
    for (const g of libraryGroups(items)) out.add(g.lib);
    for (const l of extraLibs) out.add(l);
    return [...out].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
  }, [items, extraLibs, start.lib]);

  const close = () => setSaveFootprintAs(null);

  const createLibrary = () => {
    const nick = (newLib ?? "").trim();
    if (nick === "") return setError("A library must be specified.");
    const bad = libraryNicknameIllegalChar(nick);
    if (bad) return setError(`A library nickname cannot contain "${bad}".`);
    setExtraLibs((l) => [...l, nick]);
    setLib(nick);
    setNewLib(null);
    setError(null);
  };

  /** `SaveFootprintInLibrary` and what follows it in `SaveFootprintAs`: the footprint is stored under its new id, the editor moves to it. */
  const store = async (replacing: boolean) => {
    setBusy(true);
    try {
      const id = savedFootprintId(lib, name);
      const { footprint } = await fetchAnyFootprint(req.name);
      const ok = await api.cmd({ op: "put_library_footprint", footprint: { ...footprint, name: id, published: false }, overwrite: replacing });
      notifyLibraryChanged();
      if (!ok) return;
      if (req.loaded) await api.openFootprint(id);
      dispatch({ type: "SET_TREE_SELECTION", name: id });
      dispatch({ type: "TOAST", message: savedMessage(lib, name, replacing), kind: "info" });
      close();
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
      exists = (await fetchFootprintLibraryNames()).names.includes(savedFootprintId(lib, name));
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
          <div role="listbox" aria-label="Save in library" style={{ border: "1px solid var(--chrome-border)", maxHeight: 150, overflowY: "auto" }}>
            {libs.map((l) => (
              <div
                key={l}
                role="option"
                aria-selected={l === lib}
                onClick={() => !confirm && setLib(l)}
                style={{ padding: "3px 8px", cursor: "pointer", background: l === lib ? "var(--chrome-selected-bg)" : undefined, color: l === lib ? "var(--chrome-selected-text)" : undefined }}
              >
                {l}
              </div>
            ))}
          </div>
          {newLib !== null && (
            <div className="kv-grid" style={{ gridTemplateColumns: "100px 1fr auto", marginTop: 8 }}>
              <span>Library nickname:</span>
              <input
                aria-label="Library nickname"
                autoFocus
                value={newLib}
                onChange={(e) => {
                  setNewLib(e.target.value);
                  setError(null);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    createLibrary();
                  } else if (e.key === "Escape") {
                    e.stopPropagation();
                    setNewLib(null);
                    setError(null);
                  }
                }}
              />
              <button onClick={createLibrary}>Create</button>
            </div>
          )}
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
              <button onClick={() => (setNewLib(""), setError(null))} style={{ marginRight: "auto" }}>
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
