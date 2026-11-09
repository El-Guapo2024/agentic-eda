// `SAVE_SYMBOL_AS_DIALOG` (eeschema/symbol_editor/symbol_editor.cpp, "Save Symbol As"): the name of the copy, the library it goes in, a "New
// Library..." button and the validator `saveSymbolCopyAs` runs on OK -- "A library must be specified.", "Symbol must have a name.", a library that cannot be
// written ("Library '%s' is read-only. Choose a different library to save the symbol '%s' to."), and for a name the library already has -- in any case
// (`SymbolNameInUse`), the symbol's own name included -- "Symbol '%s' already exists in library '%s'. Do you want to overwrite it?" (OK says "Overwrite";
// cancelling it keeps this dialog open, as the validator returning false does). `eeschema.SymbolLibraryControl.saveSymbolAs` also opens the copy
// afterwards, `saveSymbolCopyAs` leaves the editor where it was. The library list is `GetLibraryItemsForListDialog`'s: every library of the tables with its
// description, the pinned ones first, and the project library; the names a library has are the design's and its file's. New Library... is `AddLibraryFile( true )`:
// this dialog closes, the New Library dialog makes the file and its row, and this dialog comes back on the new library with the name typed ("Go round again").
// The copy goes into the library's buffer (the design), as `SYMBOL_SAVE_AS_HANDLER::DoSave` puts it with `UpdateSymbol`: Save writes the library's file.
//
// A derived symbol (`symbol->IsDerived()`) adds the check box "Flatten/remove symbol inheritance (current parent symbols: ...)", and the validator's other questions: a name in the
// symbol's own ancestry or descent is refused, and when the parents the copy takes along conflict too the question is "Overwrite All / Rename All / Cancel"
// (kicad-port/symSaveAsPlan.ts holds the rules; `SYMBOL_SAVE_AS_HANDLER::DoSave` is what `performSaveAs` writes).
import { useEffect, useMemo, useRef, useState } from "react";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { PROJECT_LIBRARY, splitLibName } from "../../kicad-port/libraryNames";
import { bareName } from "../../kicad-port/symFields";
import { chainConflicts, savePlan, savingIntoOwnInheritance, type ConflictStrategy } from "../../kicad-port/symSaveAsPlan";
import { isTableLibrary, libraryRowLabel, saveAsLibraryRows, symbolSaveAsCheck, takenInLibrary } from "../../kicad-port/librarySave";
import { askSaveAsConflicts, performSaveAs, saveAsContext } from "../../actions/symbolLibraryOps";
import { fetchLibraryItemsNow, fetchWritableLibraries, type WritableLibrary } from "../../api/librarySaveClient";
import { openLibraryFile } from "../../state/libraryTableDialogs";
import { useLibraryTree } from "../../state/libraryTree";

export function SaveSymbolAsDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const { pinned } = useLibraryTree("symbol");
  const req = state.saveAs;
  const [name, setName] = useState("");
  const [lib, setLib] = useState(PROJECT_LIBRARY);
  const [extraLibs, setExtraLibs] = useState<string[]>([]);
  const [tables, setTables] = useState<WritableLibrary[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** The names of the symbol's parents, nearest first (empty unless it is derived), and the check box that drops them. */
  const [parents, setParents] = useState<string[]>([]);
  const [flatten, setFlatten] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!req) return;
    const { lib: l, item } = splitLibName(req.libId);
    setName(req.again?.name ?? item);
    setLib(req.again ? req.again.lib : l || PROJECT_LIBRARY);
    setExtraLibs(req.again?.lib ? [req.again.lib] : []);
    setError(null);
    setFlatten(false);
    setParents([]);
    nameRef.current?.focus();
    nameRef.current?.select();
    let stopped = false;
    void saveAsContext({ api, dispatch }, req.libId)
      .then((ctx) => !stopped && setParents(ctx.chain.slice(1).map((p) => bareName(p.lib_id))))
      .catch(() => undefined);
    void fetchWritableLibraries("symbol")
      .then((l) => !stopped && setTables(l))
      .catch(() => undefined);
    return () => {
      stopped = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [req]);

  const libs = useMemo(() => saveAsLibraryRows(tables, pinned, extraLibs), [tables, pinned, extraLibs]);

  if (!req) return null;
  const close = () => dispatch({ type: "SET_SAVE_AS", request: null });

  // `ID_MAKE_NEW_LIBRARY`: the dialog ends, `AddLibraryFile( true )` makes the library, and the dialog is shown again on it (`params.m_LibraryName =
  // newLibrary.GetName()`, empty when none was made) with the name typed
  const newLibrary = () => {
    const typed = name;
    close();
    openLibraryFile("symbol", true, (made) => dispatch({ type: "SET_SAVE_AS", request: { ...req, again: { name: typed, lib: made ?? "" } } }));
  };

  const save = async () => {
    // `dialogValidatorFunc`, in its order.
    const checked = symbolSaveAsCheck(lib, name, libs);
    if ("error" in checked) return setError(checked.error);
    const { name: typed, libId: newLibId } = checked;
    setBusy(true);
    try {
      const ctx = await saveAsContext({ api, dispatch }, req.libId);
      // the names the library has: the design's and, for a library of the tables, its file's
      ctx.taken = takenInLibrary(lib, isTableLibrary(lib) ? await fetchLibraryItemsNow("symbol", lib) : [], [...ctx.taken]);
      const { inAncestry, inDescendants } = savingIntoOwnInheritance(ctx.chain, newLibId, ctx.taken, (id) => ctx.derived[id]);
      if (inAncestry) return setError(`Symbol '${bareName(req.libId)}' cannot replace another symbol '${typed}' that it descends from`);
      if (inDescendants) return setError(`Symbol '${bareName(req.libId)}' cannot replace another symbol '${typed}' that is a descendent of it.`);
      // a name the library has (and, for a copy that takes its parents along, a parent's name it has) is overwritten only after the question; the symbol under its own
      // name just replaces the entry with the same content
      let strategy: ConflictStrategy = "overwrite";
      const conflicts = chainConflicts({ chain: ctx.chain, newLibId, flatten, taken: ctx.taken });
      if (conflicts.length > 0) {
        const answer = await askSaveAsConflicts(lib, bareName(req.libId), typed, conflicts);
        if (answer === null) return;
        strategy = answer;
      }
      const puts = savePlan({ chain: ctx.chain, flat: ctx.flat, newLibId, flatten, strategy, taken: ctx.taken });
      if (await performSaveAs({ api, dispatch }, req, puts, ctx.taken)) {
        // the copy is in the library's buffer (the tree marks the library " *"); Save writes the library's file
        // (`saveSymbolCopyAs` says nothing: the copy is in the library's buffer, marked " *" in the tree, until Save writes it; UI-LEFT-2 LL6)
        close();
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 420 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Save Symbol As">
        <div className="dialog-header">
          <span>Save Symbol As</span>
        </div>
        <div className="dialog-body">
          <label style={{ display: "block", marginBottom: 4 }}>Name:</label>
          <input
            ref={nameRef}
            value={name}
            style={{ width: "100%", boxSizing: "border-box" }}
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
          {parents.length > 0 && (
            <label style={{ display: "block", marginTop: 10 }}>
              <input type="checkbox" checked={flatten} onChange={(e) => setFlatten(e.target.checked)} data-testid="save-as-flatten" /> Flatten/remove symbol inheritance (current parent symbols: {parents.join(", ")})
            </label>
          )}
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="save-as-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={newLibrary} style={{ marginRight: "auto" }}>
            New Library...
          </button>
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={busy} onClick={() => void save()}>
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
