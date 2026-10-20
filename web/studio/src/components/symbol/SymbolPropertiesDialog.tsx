// Port of eeschema/dialogs/dialog_lib_symbol_properties.cpp's General + Units&&Body Styles + Footprint Filters tabs (one scrolling panel here, not a real tab control -- same
// "simpler single-panel dialog" choice this app's other multi-section dialogs already make, e.g. FootprintPropertiesDialog.tsx).
//
// The fields grid is the General tab's `m_grid`: the symbol's fields -- Reference (the prefix), Value, Footprint, Datasheet, Description and the user fields -- each with its look
// and place (components/symbol/SymbolFieldsGrid.tsx). Below it "Keywords" and "Derive from symbol", which is the parent choice of a derived symbol (disabled, "<None>", for a
// root symbol: a symbol is made derived with Derive from Existing Symbol). A derived symbol (`syncControlStates( true )`) hides the lower General panel -- power, pin text and
// attributes are its parent's -- and, here, the units: the drawing is the parent's. Its grid also lists the parent's fields as inherited rows, and a reference may be empty.
// OK is one undo step: `set_symbol_parent`, `edit_symbol_properties` and `edit_library_symbol_fields`.
//
// Not ported: "Exclude from simulation" / "from position files" (no simulation or position-file concept for a schematic symbol in this app), "Define as local power symbol",
// per-body-style custom names (KiCad's "Custom" body-style mode beyond plain DeMorgan Standard/Alternate), jumper pin groups and embedded files.
//
// Named `LibrarySymbolPropertiesDialog` (not `SymbolPropertiesDialog`, already taken by the top-level placed-instance Reference/Value/Footprint/Datasheet dialog) -- same naming
// split `components/footprint/FootprintPropertiesDialog.tsx`'s own `FootprintLibraryPropertiesDialog` already establishes for the Footprint Editor.
import { useEffect, useState } from "react";
import type { Cmd, LibrarySymbol, SymbolPropertiesFields } from "../../api/types";
import { fetchLibrarySymbolFlat, fetchSymbolEditorNames } from "../../api/client";
import { bareName, parentCandidates, wireField } from "../../kicad-port/symFields";
import { gridProblem, gridRowsFor, savedFields, type GridRow } from "../../kicad-port/symFieldsGrid";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { notifyLibraryChanged } from "../library/useLibraryNames";
import { SymbolFieldsGrid } from "./SymbolFieldsGrid";

const FORM_KEYS: (keyof SymbolPropertiesFields)[] = ["reference_prefix", "description", "keywords", "datasheet", "power", "in_bom", "on_board", "pin_numbers_hidden", "pin_names_hidden", "pin_name_offset_mm", "unit_count", "has_alternate_body_style", "footprint_filters"];

/** The properties `edit_symbol_properties` takes, from a symbol. */
function formOf(sym: LibrarySymbol): SymbolPropertiesFields {
  const out: Record<string, unknown> = {};
  for (const k of FORM_KEYS) out[k] = sym[k];
  return out as unknown as SymbolPropertiesFields;
}

export function LibrarySymbolPropertiesDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const own = state.derivedOwn;
  /** What the dialog edits: the symbol's own entry (a derived symbol's fields are its own; the editor draws the flat one). */
  const subject = own ?? state.symbol;
  const derived = own !== null;
  const close = () => dispatch({ type: "SET_PROPERTIES_OPEN", open: false });

  const [form, setForm] = useState<SymbolPropertiesFields | null>(null);
  const [filtersText, setFiltersText] = useState("");
  const [rows, setRows] = useState<GridRow[]>([]);
  const [parent, setParent] = useState("");
  const [candidates, setCandidates] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!subject || !state.propertiesOpen) return;
    let stopped = false;
    setForm(formOf(subject));
    setFiltersText(subject.footprint_filters.join(" "));
    setError(null);
    setParent(subject.extends ?? "");
    setRows(gridRowsFor(subject, null));
    if (subject.extends) {
      // the parent's own fields join the grid as inherited rows (`addInheritedFields`)
      void fetchLibrarySymbolFlat(subject.extends)
        .then((flat) => !stopped && setRows(gridRowsFor(subject, flat)))
        .catch(() => undefined);
      // the symbols it can derive from (`m_inheritanceSelectCombo`)
      void fetchSymbolEditorNames()
        .then((r) => !stopped && setCandidates(parentCandidates(subject.lib_id, r.names, r.derived ?? {})))
        .catch(() => undefined);
    }
    return () => {
      stopped = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.propertiesOpen]);

  if (!state.propertiesOpen || !subject || !form) return null;

  const set = <K extends keyof SymbolPropertiesFields>(key: K, value: SymbolPropertiesFields[K]) => setForm((f) => (f ? { ...f, [key]: value } : f));

  const submit = async () => {
    const problem = gridProblem(rows, !derived);
    if (problem) return setError(problem);
    if (derived && parent === "") return setError("Derived symbol must have a parent selected");
    const footprint_filters = filtersText
      .split(/\s+/)
      .map((s) => s.trim())
      .filter(Boolean);
    // "Power symbols must have value matching name for now"
    const out = !derived && form.power ? rows.map((r) => (r.name === "Value" ? { ...r, text: bareName(subject.lib_id) } : r)) : rows;
    const text = (name: string) => out.find((r) => r.name === name)?.text ?? "";
    const props: SymbolPropertiesFields = { ...form, reference_prefix: text("Reference"), description: text("Description"), datasheet: text("Datasheet"), footprint_filters };
    const cmds: Cmd[] = [];
    if (derived && parent !== subject.extends) cmds.push({ op: "set_symbol_parent", lib_id: subject.lib_id, parent });
    cmds.push({ op: "edit_symbol_properties", lib_id: subject.lib_id, ...props });
    cmds.push({ op: "edit_library_symbol_fields", lib_id: subject.lib_id, fields: savedFields(out).map(wireField) });
    setBusy(true);
    try {
      const ok = await api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
      notifyLibraryChanged();
      if (ok) close();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 760, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Symbol Properties" data-testid="symbol-properties">
        <div className="dialog-header">
          <span>Symbol Properties</span>
          <span>{subject.lib_id}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "74vh", overflowY: "auto" }}>
          <SymbolFieldsGrid rows={rows} onChange={setRows} parentName={parent ? bareName(parent) : undefined} onError={setError} initialField={state.propertiesField} />
          <div className="kv-grid" style={{ gridTemplateColumns: "180px 1fr", marginTop: 10 }}>
            <span>Keywords</span>
            <input value={form.keywords} onChange={(e) => set("keywords", e.target.value)} style={{ width: 300 }} data-testid="props-keywords" />
            <span title={"Select symbol to derive this symbol from or select\n<None> for root symbol."} style={{ opacity: derived ? 1 : 0.5 }}>
              Derive from symbol
            </span>
            <select value={derived ? parent : ""} disabled={!derived} onChange={(e) => setParent(e.target.value)} aria-label="Derive from symbol" data-testid="props-parent" style={{ width: 306 }}>
              {!derived && <option value="">&lt;None&gt;</option>}
              {derived && !candidates.includes(parent) && <option value={parent}>{bareName(parent)}</option>}
              {candidates.map((c) => (
                <option key={c} value={c}>
                  {bareName(c)}
                </option>
              ))}
            </select>
          </div>
          {!derived && (
            <div className="kv-grid" style={{ gridTemplateColumns: "180px 1fr", marginTop: 10 }}>
              <span>Define as power symbol</span>
              <label>
                <input type="checkbox" checked={form.power} onChange={(e) => set("power", e.target.checked)} data-testid="props-power" />
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
            </div>
          )}
          <div className="kv-grid" style={{ gridTemplateColumns: "180px 1fr", marginTop: 10 }}>
            <span>Footprint filters</span>
            <input value={filtersText} onChange={(e) => setFiltersText(e.target.value)} placeholder="SOIC* TSSOP*" style={{ width: 300 }} data-testid="props-filters" />
          </div>
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="props-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={busy} onClick={() => void submit()} data-testid="props-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
