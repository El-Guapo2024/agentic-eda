// "Update Symbol Fields..." (`eeschema.SymbolLibraryControl.updateSymbolFields`, `DIALOG_UPDATE_SYMBOL_FIELDS`): a derived symbol's fields made to follow its parent's. The list is the
// five mandatory fields and every user field the parent (flattened) or the symbol has, all checked to begin with (`updateFieldsList`); the options are KiCad's: "Remove fields if
// not in parent symbol", "Reset fields if empty in parent symbol", "Update/reset field text", "... visibilities", "... sizes and styles", "... positions" (the last four on). OK is one
// `update_symbol_fields` (`DIALOG_UPDATE_SYMBOL_FIELDS::onOkButtonClicked`). On a symbol that is not derived KiCad only says "Symbol is not derived from another symbol."
import { useEffect, useState } from "react";
import { fetchLibrarySymbol, fetchLibrarySymbolFlat } from "../../api/client";
import { MANDATORY_FIELDS, allFields, bareName, isMandatoryField } from "../../kicad-port/symFields";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { notifyLibraryChanged } from "../library/useLibraryNames";

export function UpdateSymbolFieldsDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const open = state.updateFieldsOpen;
  const own = state.derivedOwn;
  const [rows, setRows] = useState<{ name: string; checked: boolean }[]>([]);
  const [removeExtras, setRemoveExtras] = useState(false);
  const [resetEmpty, setResetEmpty] = useState(false);
  const [resetText, setResetText] = useState(true);
  const [resetVisibility, setResetVisibility] = useState(true);
  const [resetEffects, setResetEffects] = useState(true);
  const [resetPositions, setResetPositions] = useState(true);
  const [busy, setBusy] = useState(false);

  // The editor re-reads the symbol every moment, and `own` is a new object each time: the dialog starts over only when it opens or the symbol it is about changes.
  const ownId = own?.lib_id;
  const parentId = own?.extends;
  useEffect(() => {
    if (!open || !ownId || !parentId) return;
    setRemoveExtras(false);
    setResetEmpty(false);
    setResetText(true);
    setResetVisibility(true);
    setResetEffects(true);
    setResetPositions(true);
    // the five mandatory fields, then the non-mandatory ones of the flattened parent and of the symbol, by name (`std::set`: sorted)
    setRows(MANDATORY_FIELDS.map((name) => ({ name, checked: true })));
    let stopped = false;
    void Promise.all([fetchLibrarySymbolFlat(parentId), fetchLibrarySymbol(ownId)])
      .then(([parent, mine]) => {
        if (stopped) return;
        const extra = new Set<string>();
        for (const f of allFields(parent)) if (!isMandatoryField(f.name)) extra.add(f.name);
        for (const f of allFields(mine)) if (!isMandatoryField(f.name)) extra.add(f.name);
        setRows([...MANDATORY_FIELDS.map((name) => ({ name: name as string, checked: true })), ...[...extra].sort().map((name) => ({ name, checked: true }))]);
      })
      .catch(() => undefined);
    return () => {
      stopped = true;
    };
  }, [open, ownId, parentId]);

  if (!open || !own?.extends) return null;
  const close = () => dispatch({ type: "SET_UPDATE_FIELDS_OPEN", open: false });
  const checkAll = (checked: boolean) => setRows((r) => r.map((x) => ({ ...x, checked })));
  const ok = async () => {
    setBusy(true);
    try {
      const done = await api.cmd({
        op: "update_symbol_fields",
        lib_id: own.lib_id,
        fields: rows.filter((r) => r.checked).map((r) => r.name),
        remove_extras: removeExtras,
        reset_empty: resetEmpty,
        reset_text: resetText,
        reset_visibility: resetVisibility,
        reset_effects: resetEffects,
        reset_positions: resetPositions,
      });
      notifyLibraryChanged();
      if (done) close();
    } finally {
      setBusy(false);
    }
  };
  const box = (label: string, value: boolean, set: (v: boolean) => void, id: string, title?: string) => (
    <label title={title} style={{ display: "block", marginBottom: 3 }}>
      <input type="checkbox" checked={value} onChange={(e) => set(e.target.checked)} data-testid={id} /> {label}
    </label>
  );
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 440 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Update Symbol Fields" data-testid="update-symbol-fields">
        <div className="dialog-header">
          <span>Update Symbol Fields</span>
          <span>{own.lib_id}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "110px 1fr", marginBottom: 8 }}>
            <span>Parent symbol:</span>
            <input value={bareName(own.extends)} readOnly data-testid="update-fields-parent" style={{ width: 260 }} />
          </div>
          <fieldset style={{ marginBottom: 8 }}>
            <legend>Update/reset Fields</legend>
            <div style={{ maxHeight: 150, overflowY: "auto", marginBottom: 6 }} data-testid="update-fields-list">
              {rows.map((r, i) => (
                <label key={r.name} style={{ display: "block" }}>
                  <input type="checkbox" checked={r.checked} onChange={(e) => setRows((all) => all.map((x, j) => (j === i ? { ...x, checked: e.target.checked } : x)))} data-testid={`update-fields-row-${r.name}`} /> {r.name}
                </label>
              ))}
            </div>
            <button onClick={() => checkAll(true)} data-testid="update-fields-all">
              Select All
            </button>{" "}
            <button onClick={() => checkAll(false)} data-testid="update-fields-none">
              Select None
            </button>
          </fieldset>
          <fieldset>
            <legend>Update Options</legend>
            {box("Remove fields if not in parent symbol", removeExtras, setRemoveExtras, "update-fields-remove", "Removes fields that do not occur in the original library symbols")}
            {box("Reset fields if empty in parent symbol", resetEmpty, setResetEmpty, "update-fields-empty")}
            {box("Update/reset field text", resetText, setResetText, "update-fields-text")}
            {box("Update/reset field visibilities", resetVisibility, setResetVisibility, "update-fields-visibility")}
            {box("Update/reset field sizes and styles", resetEffects, setResetEffects, "update-fields-effects")}
            {box("Update/reset field positions", resetPositions, setResetPositions, "update-fields-positions")}
          </fieldset>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={busy} onClick={() => void ok()} data-testid="update-fields-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
