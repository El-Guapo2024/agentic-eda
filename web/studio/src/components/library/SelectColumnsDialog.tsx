// "Select Columns" (`EDA_REORDERABLE_LIST_DIALOG`, common/dialogs/eda_reorderable_list_dialog.cpp at KiCad 8303b2ad), opened by the library tree's header menu
// (`LIB_TREE::onHeaderContextMenu` -> `ACTIONS::selectLibTreeColumns`, "Select Columns..."): "Available:" and "Enabled:", `>` and `<`, Move up and Move down,
// OK and Cancel. OK gives the enabled list to the tree (`SetShownColumns`). The list rules are kicad-port/libTreeColumns.ts's.
import { useEffect, useState } from "react";
import { addColumn, moveColumnDown, moveColumnUp, openListDialog, removeColumn, type ListDialog } from "../../kicad-port/libTreeColumns";

const box = { border: "1px solid var(--chrome-border)", minHeight: 180, width: 220, overflowY: "auto", background: "var(--chrome-bg)" } as const;

export function SelectColumnsDialog({ available, shown, onOk, onClose }: { available: readonly string[]; shown: readonly string[]; onOk: (columns: string[]) => void; onClose: () => void }) {
  const [d, setD] = useState<ListDialog>(() => openListDialog(available, shown));
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);
  const row = (text: string, selected: boolean, onClick: () => void) => (
    <div key={text} role="option" aria-selected={selected} onClick={onClick} style={{ padding: "2px 6px", cursor: "pointer", background: selected ? "var(--chrome-selected-bg)" : undefined, color: selected ? "var(--chrome-selected-text)" : undefined }}>
      {text}
    </div>
  );
  return (
    <div className="dialog-backdrop" onClick={onClose} style={{ zIndex: 1000 }}>
      <div className="dialog" onClick={(e) => e.stopPropagation()} data-select-columns-dialog style={{ width: "auto" }}>
        <div className="dialog-header">
          <span>Select Columns</span>
        </div>
        <div className="dialog-body" style={{ display: "flex", gap: 8, alignItems: "center", fontSize: 12 }}>
          <div>
            <div>Available:</div>
            <div style={box} role="listbox" aria-label="Available" data-columns-available>
              {d.available.map((c, i) => row(c, i === d.selectedAvailable, () => setD({ ...d, selectedAvailable: i })))}
            </div>
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            <button onClick={() => setD(addColumn(d))} disabled={d.available.length === 0} data-columns-add>
              {">"}
            </button>
            <button onClick={() => setD(removeColumn(d))} disabled={d.enabled.length === 0} data-columns-remove>
              {"<"}
            </button>
          </div>
          <div>
            <div>Enabled:</div>
            <div style={box} role="listbox" aria-label="Enabled" data-columns-enabled>
              {d.enabled.map((c, i) => row(c, i === d.selectedEnabled, () => setD({ ...d, selectedEnabled: i })))}
            </div>
            <div style={{ display: "flex", gap: 4, marginTop: 4 }}>
              <button title="Move up" aria-label="Move up" onClick={() => setD(moveColumnUp(d))}>
                {"▲"}
              </button>
              <button title="Move down" aria-label="Move down" onClick={() => setD(moveColumnDown(d))}>
                {"▼"}
              </button>
            </div>
          </div>
        </div>
        <div className="dialog-footer">
          <button
            className="primary"
            onClick={() => {
              onOk(d.enabled);
              onClose();
            }}
            data-columns-ok
          >
            OK
          </button>
          <button onClick={onClose}>Cancel</button>
        </div>
      </div>
    </div>
  );
}
