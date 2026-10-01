// Port of eeschema/dialogs/dialog_lib_symbol_properties.cpp's General +
// Units&&Body Styles + Footprint Filters tabs (one scrolling panel here,
// not a real tab control -- same "simpler single-panel dialog" choice
// this app's other multi-section dialogs already make, e.g.
// FootprintPropertiesDialog.tsx). Not ported: "Derive from symbol"
// (library inheritance via `extends` -- this editor always authors a
// standalone symbol, never one that borrows another's graphics/pins --
// see PARITY-symedit.md), "Exclude from simulation"/"from position
// files" (no simulation or position-file concept for a schematic symbol
// in this app), and per-body-style custom names (KiCad's "Custom" body-
// style mode beyond plain DeMorgan Standard/Alternate).
//
// Named `LibrarySymbolPropertiesDialog` (not `SymbolPropertiesDialog`,
// already taken by the top-level placed-instance Reference/Value/
// Footprint/Datasheet dialog) -- same naming split
// `components/footprint/FootprintPropertiesDialog.tsx`'s own
// `FootprintLibraryPropertiesDialog` already establishes for the
// Footprint Editor.
import { useEffect, useState } from "react";
import type { SymbolPropertiesFields } from "../../api/types";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";

export function LibrarySymbolPropertiesDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const sym = state.symbol;
  const close = () => dispatch({ type: "SET_PROPERTIES_OPEN", open: false });

  const [form, setForm] = useState<SymbolPropertiesFields | null>(null);
  const [filtersText, setFiltersText] = useState("");

  useEffect(() => {
    if (sym && state.propertiesOpen) {
      setForm(sym);
      setFiltersText(sym.footprint_filters.join(" "));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.propertiesOpen]);

  if (!state.propertiesOpen || !sym || !form) return null;

  const set = <K extends keyof SymbolPropertiesFields>(key: K, value: SymbolPropertiesFields[K]) => setForm((f) => (f ? { ...f, [key]: value } : f));

  const submit = async () => {
    const footprint_filters = filtersText
      .split(/\s+/)
      .map((s) => s.trim())
      .filter(Boolean);
    if (await api.editProperties({ ...form, footprint_filters })) close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 480 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Symbol Properties</span>
          <span>{sym.lib_id}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "72vh", overflowY: "auto" }}>
          <div className="kv-grid" style={{ gridTemplateColumns: "180px 1fr" }}>
            <span>Reference prefix</span>
            <input value={form.reference_prefix} onChange={(e) => set("reference_prefix", e.target.value)} style={{ width: 80 }} />
            <span>Description</span>
            <input value={form.description} onChange={(e) => set("description", e.target.value)} style={{ width: 260 }} />
            <span>Keywords</span>
            <input value={form.keywords} onChange={(e) => set("keywords", e.target.value)} style={{ width: 260 }} />
            <span>Datasheet</span>
            <input value={form.datasheet} onChange={(e) => set("datasheet", e.target.value)} style={{ width: 260 }} />

            <span>Define as power symbol</span>
            <label>
              <input type="checkbox" checked={form.power} onChange={(e) => set("power", e.target.checked)} />
            </label>
            <span>Show pin numbers</span>
            <label>
              <input type="checkbox" checked={!form.pin_numbers_hidden} onChange={(e) => set("pin_numbers_hidden", !e.target.checked)} />
            </label>
            <span>Show pin names</span>
            <label>
              <input type="checkbox" checked={!form.pin_names_hidden} onChange={(e) => set("pin_names_hidden", !e.target.checked)} />
            </label>
            <span>Pin name offset (mm)</span>
            <input type="number" step="any" min={0} value={form.pin_name_offset_mm} onChange={(e) => Number.isFinite(Number(e.target.value)) && set("pin_name_offset_mm", Number(e.target.value))} style={{ width: 80 }} />
            <span>In BOM</span>
            <label>
              <input type="checkbox" checked={form.in_bom} onChange={(e) => set("in_bom", e.target.checked)} />
            </label>
            <span>On board</span>
            <label>
              <input type="checkbox" checked={form.on_board} onChange={(e) => set("on_board", e.target.checked)} />
            </label>

            <span>Number of units</span>
            <input type="number" min={1} max={64} value={form.unit_count} onChange={(e) => set("unit_count", Math.max(1, Math.min(64, Math.round(Number(e.target.value) || 1))))} style={{ width: 60 }} />
            <span>Has alternate body style (DeMorgan)</span>
            <label>
              <input type="checkbox" checked={form.has_alternate_body_style} onChange={(e) => set("has_alternate_body_style", e.target.checked)} />
            </label>

            <span>Footprint filters</span>
            <input value={filtersText} onChange={(e) => setFiltersText(e.target.value)} placeholder="SOIC* TSSOP*" style={{ width: 260 }} />
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={submit}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
