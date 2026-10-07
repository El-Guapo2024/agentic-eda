// `SAVE_SYMBOL_AS_DIALOG` (eeschema/symbol_editor/symbol_editor.cpp, "Save Symbol As"): the name of the copy, the library it goes in, a "New
// Library..." button and the validator `saveSymbolCopyAs` runs on OK -- "A library must be specified.", "Symbol must have a name.", and for a
// name the library already has "Symbol '%s' already exists in library '%s'. Do you want to overwrite it?" (OK says "Overwrite"; cancelling
// it keeps this dialog open, as the validator returning false does). `eeschema.SymbolLibraryControl.saveSymbolAs` also opens the copy
// afterwards, `saveSymbolCopyAs` leaves the editor where it was. The library list is the library nicknames of the tree: the project
// library's own and the ones of library files its symbols come from -- a new nickname starts a new library in the project.
import { useEffect, useMemo, useRef, useState } from "react";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { fetchSymbolEditorNames } from "../../api/client";
import { joinLibName, libraryGroups, libraryNicknameIllegalChar, PROJECT_LIBRARY, saveAsSymbolName, splitLibName, symbolLibIdError } from "../../kicad-port/libraryNames";
import { performSaveAs } from "../../actions/symbolLibraryOps";
import { askConfirm, askText } from "../library/libraryDialogs";
import { useLibraryNames } from "../library/useLibraryNames";

export function SaveSymbolAsDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const { items } = useLibraryNames("symbol");
  const req = state.saveAs;
  const [name, setName] = useState("");
  const [lib, setLib] = useState(PROJECT_LIBRARY);
  const [extraLibs, setExtraLibs] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!req) return;
    const { lib: l, item } = splitLibName(req.libId);
    setName(item);
    setLib(l || PROJECT_LIBRARY);
    setExtraLibs([]);
    setError(null);
    nameRef.current?.focus();
    nameRef.current?.select();
  }, [req]);

  const libs = useMemo(() => {
    const out = new Set<string>([PROJECT_LIBRARY]);
    for (const g of libraryGroups(items)) out.add(g.lib);
    for (const l of extraLibs) out.add(l);
    return [...out].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
  }, [items, extraLibs]);

  if (!req) return null;
  const close = () => dispatch({ type: "SET_SAVE_AS", request: null });

  const newLibrary = async () => {
    const entered = await askText({
      title: "New Library",
      label: "Library nickname:",
      okLabel: "Create",
      validate: (v) => {
        const t = v.trim();
        if (t === "") return "A library must be specified.";
        const c = libraryNicknameIllegalChar(t);
        return c ? `A library nickname cannot contain "${c}".` : null;
      },
    });
    if (entered === null) return;
    const t = entered.trim();
    setExtraLibs((l) => [...l, t]);
    setLib(t);
  };

  const save = async () => {
    // `dialogValidatorFunc`, in its order.
    if (lib === "") return setError("A library must be specified.");
    const typed = saveAsSymbolName(name);
    if (typed === "") return setError("Symbol must have a name.");
    const newLibId = joinLibName(lib, typed);
    const illegal = symbolLibIdError(newLibId);
    if (illegal) return setError(illegal);
    setBusy(true);
    try {
      let exists = false;
      try {
        exists = (await fetchSymbolEditorNames()).names.includes(newLibId);
      } catch {
        /* the verb refuses a taken name anyway */
      }
      let overwrite = false;
      if (exists && newLibId !== req.libId) {
        if (!(await askConfirm({ title: "Confirmation", message: `Symbol '${typed}' already exists in library '${lib}'. Do you want to overwrite it?`, okLabel: "Overwrite" }))) return;
        overwrite = true;
      } else if (newLibId === req.libId) {
        overwrite = true; // saving a symbol under its own name: the copy replaces the entry with the same content
      }
      if (await performSaveAs({ api, dispatch }, req, newLibId, overwrite)) close();
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
          <div role="listbox" aria-label="Save in library" style={{ border: "1px solid var(--chrome-border)", maxHeight: 150, overflowY: "auto" }}>
            {libs.map((l) => (
              <div
                key={l}
                role="option"
                aria-selected={l === lib}
                onClick={() => setLib(l)}
                style={{ padding: "3px 8px", cursor: "pointer", background: l === lib ? "var(--chrome-selected-bg)" : undefined, color: l === lib ? "var(--chrome-selected-text)" : undefined }}
              >
                {l}
              </div>
            ))}
          </div>
          {error && <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }}>{error}</p>}
        </div>
        <div className="dialog-footer">
          <button onClick={() => void newLibrary()} style={{ marginRight: "auto" }}>
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
