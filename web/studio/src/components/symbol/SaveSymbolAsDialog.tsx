// `SAVE_SYMBOL_AS_DIALOG` (eeschema/symbol_editor/symbol_editor.cpp, "Save Symbol As"): the name of the copy, the library it goes in, a "New
// Library..." button and the validator `saveSymbolCopyAs` runs on OK -- "A library must be specified.", "Symbol must have a name.", and for a
// name the library already has "Symbol '%s' already exists in library '%s'. Do you want to overwrite it?" (OK says "Overwrite"; cancelling
// it keeps this dialog open, as the validator returning false does). `eeschema.SymbolLibraryControl.saveSymbolAs` also opens the copy
// afterwards, `saveSymbolCopyAs` leaves the editor where it was. The library list is the library nicknames of the tree: the project
// library's own and the ones of library files its symbols come from -- a new nickname starts a new library in the project.
//
// A derived symbol (`symbol->IsDerived()`) adds the check box "Flatten/remove symbol inheritance (current parent symbols: ...)", and the validator's other questions: a name in the
// symbol's own ancestry or descent is refused, and when the parents the copy takes along conflict too the question is "Overwrite All / Rename All / Cancel"
// (kicad-port/symSaveAsPlan.ts holds the rules; `SYMBOL_SAVE_AS_HANDLER::DoSave` is what `performSaveAs` writes).
import { useEffect, useMemo, useRef, useState } from "react";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { joinLibName, libraryGroups, libraryNicknameIllegalChar, PROJECT_LIBRARY, saveAsSymbolName, splitLibName, symbolLibIdError } from "../../kicad-port/libraryNames";
import { bareName } from "../../kicad-port/symFields";
import { chainConflicts, savePlan, savingIntoOwnInheritance, type ConflictStrategy } from "../../kicad-port/symSaveAsPlan";
import { askSaveAsConflicts, performSaveAs, saveAsContext } from "../../actions/symbolLibraryOps";
import { askText } from "../library/libraryDialogs";
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
  /** The names of the symbol's parents, nearest first (empty unless it is derived), and the check box that drops them. */
  const [parents, setParents] = useState<string[]>([]);
  const [flatten, setFlatten] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!req) return;
    const { lib: l, item } = splitLibName(req.libId);
    setName(item);
    setLib(l || PROJECT_LIBRARY);
    setExtraLibs([]);
    setError(null);
    setFlatten(false);
    setParents([]);
    nameRef.current?.focus();
    nameRef.current?.select();
    let stopped = false;
    void saveAsContext({ api, dispatch }, req.libId)
      .then((ctx) => !stopped && setParents(ctx.chain.slice(1).map((p) => bareName(p.lib_id))))
      .catch(() => undefined);
    return () => {
      stopped = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
      const ctx = await saveAsContext({ api, dispatch }, req.libId);
      const { inAncestry, inDescendants } = savingIntoOwnInheritance(ctx.chain, newLibId, ctx.taken, (id) => ctx.derived[id]);
      if (inAncestry) return setError(`Symbol '${bareName(req.libId)}' cannot replace another symbol '${typed}' that it descends from`);
      if (inDescendants) return setError(`Symbol '${bareName(req.libId)}' cannot replace another symbol '${typed}' that is a descendent of it.`);
      // a name the library has (and, for a copy that takes its parents along, a parent's name it has) is overwritten only after the question; the symbol under its own
      // name just replaces the entry with the same content
      let strategy: ConflictStrategy = "overwrite";
      const conflicts = chainConflicts({ chain: ctx.chain, newLibId, flatten, taken: ctx.taken });
      if (conflicts.length > 0 && newLibId !== req.libId) {
        const answer = await askSaveAsConflicts(lib, bareName(req.libId), typed, conflicts);
        if (answer === null) return;
        strategy = answer;
      }
      const puts = savePlan({ chain: ctx.chain, flat: ctx.flat, newLibId, flatten, strategy, taken: ctx.taken });
      if (await performSaveAs({ api, dispatch }, req, puts, ctx.taken)) close();
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
