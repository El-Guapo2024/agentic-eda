// "Derive from Existing Symbol..." (`eeschema.SymbolLibraryControl.deriveFromExistingSymbol`, `SYMBOL_EDIT_FRAME::CreateNewSymbol( parentName )`): `DIALOG_LIB_NEW_SYMBOL` as it
// stands when a parent is chosen -- the symbol's name (it starts as `<parent>_1`, `getDerivativeName`, until the person types one), the parent to derive from, and what the new
// symbol keeps of it: "Keep linked datasheet", "Keep linked footprint", "Transfer user-defined fields" and, with that, "Keep content of user-defined fields". (The controls
// that describe a symbol with no parent -- the reference, the units, the pin text, power -- are the parent's for a derived symbol, `syncControls`, and are not shown.)
// OK is one `derive_symbol` (`LIB_SYMBOL_LIBRARY_MANAGER::CreateSymbol`); the new symbol is opened. A name the library already has is replaced only after the same "Overwrite"
// confirmation the dialog's validator asks.
import { useEffect, useState } from "react";
import { fetchSymbolEditorNames } from "../../api/client";
import { ensureSymbolInProject } from "../../api/libraryClient";
import { joinLibName, libItemNameIllegalChar, splitLibName, symbolLibIdError } from "../../kicad-port/libraryNames";
import { bareName, libraryOf } from "../../kicad-port/symFields";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { askConfirm } from "../library/libraryDialogs";
import { notifyLibraryChanged } from "../library/useLibraryNames";

/** `getDerivativeName`. */
export const derivativeName = (parent: string): string => `${bareName(parent)}_1`;

export function DeriveSymbolDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const request = state.deriveRequest;
  const [parent, setParent] = useState("");
  const [name, setName] = useState("");
  /** `m_nameIsDefaulted`: the name follows the parent until the person types one. */
  const [defaulted, setDefaulted] = useState(true);
  const [keepDatasheet, setKeepDatasheet] = useState(false);
  const [keepFootprint, setKeepFootprint] = useState(false);
  const [transfer, setTransfer] = useState(false);
  const [keepContent, setKeepContent] = useState(false);
  const [candidates, setCandidates] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!request) return;
    setParent(request.parent);
    setName(derivativeName(request.parent));
    setDefaulted(true);
    setKeepDatasheet(false);
    setKeepFootprint(false);
    setTransfer(false);
    setKeepContent(false);
    setError(null);
    let stopped = false;
    // the symbols of the parent's library are what it can be derived from (`m_comboInheritanceSelect` lists the library's names)
    void fetchSymbolEditorNames()
      .then((r) => !stopped && setCandidates(r.names.filter((n) => libraryOf(n) === libraryOf(request.parent)).sort((a, b) => bareName(a).localeCompare(bareName(b), undefined, { numeric: true, sensitivity: "base" }))))
      .catch(() => !stopped && setCandidates([request.parent]));
    return () => {
      stopped = true;
    };
  }, [request]);

  if (!request) return null;
  const close = () => dispatch({ type: "SET_DERIVE_REQUEST", parent: null });
  const lib = splitLibName(parent).lib;

  const choose = (p: string) => {
    setParent(p);
    if (defaulted) setName(derivativeName(p)); // `onParentSymbolSelect`
  };

  const ok = async () => {
    const trimmed = name.trim();
    const bad = trimmed === "" ? "Symbol must have a name." : libItemNameIllegalChar(trimmed) ? symbolLibIdError(joinLibName(lib, trimmed)) : null;
    if (bad) return setError(bad);
    const libId = joinLibName(lib, trimmed);
    setBusy(true);
    setError(null);
    try {
      const { names, project } = await fetchSymbolEditorNames().then((r) => ({ names: r.names, project: r.project ?? [] }));
      let overwrite = false;
      if (names.includes(libId) || project.includes(libId)) {
        // the validator of `CreateNewSymbol`: "Symbol '%s' already exists in library '%s'." with an Overwrite button
        if (!(await askConfirm({ title: "Confirmation", message: `Symbol '${trimmed}' already exists in library '${lib}'.`, okLabel: "Overwrite" }))) return;
        overwrite = true;
      }
      await ensureSymbolInProject(parent); // a symbol of an installed library is copied into the project library first, like opening it
      const done = await api.cmd({ op: "derive_symbol", parent, lib_id: libId, keep_footprint: keepFootprint, keep_datasheet: keepDatasheet, transfer_user_fields: transfer, keep_content_user_fields: transfer && keepContent, overwrite });
      notifyLibraryChanged();
      if (!done) return; // the verb's refusal is the editor's toast
      close();
      await api.openSymbol(libId);
      dispatch({ type: "SET_TREE_SELECTION", names: [libId] });
      notifyLibraryChanged();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 440 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Derive from Existing Symbol" data-testid="derive-symbol">
        <div className="dialog-header">
          <span>Derive from Existing Symbol</span>
        </div>
        <div className="dialog-body" onKeyDown={(e) => e.key === "Enter" && e.target instanceof HTMLInputElement && e.target.type === "text" && void ok()}>
          <div role="status" style={{ background: "var(--chrome-bg-raised)", border: "1px solid var(--chrome-border)", padding: "4px 8px", marginBottom: 8, fontSize: 12 }} data-testid="derive-info">
            Deriving from symbol '{bareName(parent)}'.
          </div>
          <div className="kv-grid" style={{ gridTemplateColumns: "190px 1fr" }}>
            <span title={"The symbol name in library and also the default\nsymbol value when loaded in the schematic."}>Symbol name:</span>
            <input
              value={name}
              autoFocus
              onChange={(e) => {
                setName(e.target.value);
                setDefaulted(false);
              }}
              aria-label="Symbol name"
              data-testid="derive-name"
              style={{ width: 220 }}
            />
            <span>Derive from existing symbol:</span>
            <select value={parent} onChange={(e) => choose(e.target.value)} aria-label="Derive from existing symbol" data-testid="derive-parent" style={{ width: 232 }}>
              {(candidates.includes(parent) ? candidates : [parent, ...candidates]).map((c) => (
                <option key={c} value={c}>
                  {bareName(c)}
                </option>
              ))}
            </select>
          </div>
          <div style={{ marginTop: 10, display: "grid", gap: 4 }}>
            <label>
              <input type="checkbox" checked={keepDatasheet} onChange={(e) => setKeepDatasheet(e.target.checked)} data-testid="derive-keep-datasheet" /> Keep linked datasheet
            </label>
            <label>
              <input type="checkbox" checked={keepFootprint} onChange={(e) => setKeepFootprint(e.target.checked)} data-testid="derive-keep-footprint" /> Keep linked footprint
            </label>
            <label>
              <input type="checkbox" checked={transfer} onChange={(e) => setTransfer(e.target.checked)} data-testid="derive-transfer" /> Transfer user-defined fields
            </label>
            <label style={{ marginLeft: 20, opacity: transfer ? 1 : 0.5 }}>
              <input type="checkbox" checked={keepContent} disabled={!transfer} onChange={(e) => setKeepContent(e.target.checked)} data-testid="derive-keep-content" /> Keep content of user-defined fields
            </label>
          </div>
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="derive-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={busy} onClick={() => void ok()} data-testid="derive-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
