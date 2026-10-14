// The info bar the Symbol Editor shows over a derived symbol (`SYMBOL_EDIT_FRAME::LoadSymbol...`, `IsSymbolAlias()`): "Symbol %s is a derived symbol. Symbol graphics will not be
// editable." with a link "Open <root>" that loads the root symbol of the chain (`LoadSymbolFromCurrentLib( rootSymbolName )`). Edits of the drawing are refused by the verbs
// (`ops_derived_symbol`); the fields stay editable, in Symbol Properties and in the fields table.
import { useEffect, useState } from "react";
import { fetchSymbolEditorNames } from "../../api/client";
import { bareName, rootOf } from "../../kicad-port/symFields";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { notifyLibraryChanged } from "../library/useLibraryNames";

export function DerivedSymbolBanner() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const own = state.derivedOwn;
  const [root, setRoot] = useState<string | null>(null);

  useEffect(() => {
    setRoot(null);
    if (!own) return;
    let stopped = false;
    void fetchSymbolEditorNames()
      .then((r) => !stopped && setRoot(rootOf(r.derived ?? {}, own.lib_id)))
      .catch(() => undefined);
    return () => {
      stopped = true;
    };
  }, [own?.lib_id, own?.extends]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!own) return null;
  const openRoot = () => {
    if (!root) return;
    dispatch({ type: "SET_TREE_SELECTION", names: [root] });
    void api.openSymbol(root).then(notifyLibraryChanged);
  };
  return (
    <div
      role="status"
      data-testid="derived-banner"
      style={{ position: "absolute", top: 0, left: 0, right: 0, zIndex: 5, padding: "4px 10px", fontSize: 12, background: "var(--chrome-bg-raised)", borderBottom: "1px solid var(--chrome-border)", display: "flex", gap: 12, alignItems: "center" }}
    >
      <span>Symbol {bareName(own.lib_id)} is a derived symbol. Symbol graphics will not be editable.</span>
      {root && root !== own.lib_id && (
        <a href="#open-root" data-testid="derived-open-root" onClick={(e) => { e.preventDefault(); openRoot(); }} style={{ color: "var(--chrome-accent, #4aa3ff)" }}>
          Open {bareName(root)}
        </a>
      )}
    </div>
  );
}
