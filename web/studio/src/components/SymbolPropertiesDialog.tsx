// `E` (sch_edit_tool.cpp::Properties -- dispatches to one of 6 dialogs by
// item type; this app only has symbols selectable so far, so this is
// always DIALOG_SYMBOL_PROPERTIES's own shape, trimmed to the fields this
// IR has: Reference/Value/Footprint/Datasheet, no unit/DeMorgan/pin-table
// editing) and `U`/`V`/`F` (sch_edit_tool.cpp::EditField's own quick
// single-field edits: editReference/editValue/editFootprint) -- real
// eeschema uses a different, smaller dialog for U/V/F
// (DIALOG_FIELD_PROPERTIES, one field only); this app reuses the same
// form and just autofocuses the one field the hotkey named, a deliberate
// simplification (one component to maintain, same backend Cmds either
// way) rather than building two dialogs with identical plumbing.
import { useEffect, useMemo, useRef, useState } from "react";
import { fetchAnySymbol } from "../api/libraryClient";
import { uniquePinCount } from "../kicad-port/footprintFilter";
import { expandStackedPinNotation } from "../kicad-port/stackedPins";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { FootprintChooserDialog } from "./FootprintChooserDialog";

export function SymbolPropertiesDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.symbolProperties;
  const symbol = open ? api.symbolById(open.id) : undefined;

  const [reference, setReference] = useState("");
  const [value, setValue] = useState("");
  const [footprint, setFootprint] = useState("");
  const [datasheet, setDatasheet] = useState("");
  const [error, setError] = useState<string | null>(null);
  // The Footprint field's browse button: the Footprint Chooser, narrowed by this symbol's pin count and its library symbol's footprint filters.
  const [choosingFootprint, setChoosingFootprint] = useState(false);
  const [libInfo, setLibInfo] = useState<{ filters: string[]; pins: number } | null>(null);
  const libId = symbol?.lib_id ?? null;
  useEffect(() => {
    setLibInfo(null);
    if (!libId) return;
    let cancelled = false;
    // The pins of the whole library symbol, every unit (`GetGraphicalPins( 0, 1 )`): the placed instance lists the pins of its own unit only.
    fetchAnySymbol(libId)
      .then((r) => !cancelled && setLibInfo({ filters: r.symbol.footprint_filters ?? [], pins: uniquePinCount(r.symbol.pins, (n) => expandStackedPinNotation(n).numbers) }))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [libId]);
  const instancePins = useMemo(() => new Set((symbol?.pins ?? []).map((p) => p.number)).size, [symbol]);

  const refInput = useRef<HTMLInputElement>(null);
  const valueInput = useRef<HTMLInputElement>(null);
  const footprintInput = useRef<HTMLInputElement>(null);
  const datasheetInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!symbol) return;
    setReference(symbol.id);
    setValue(symbol.value ?? "");
    setFootprint(symbol.footprint ?? "");
    setDatasheet(symbol.datasheet ?? "");
    setError(null);
    const focusTarget = { reference: refInput, value: valueInput, footprint: footprintInput, datasheet: datasheetInput }[open?.field ?? "reference"];
    // Let the dialog mount before stealing focus.
    const t = setTimeout(() => focusTarget.current?.select(), 0);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open?.id, open?.field]);

  if (!open || !symbol) return null;
  const close = () => dispatch({ type: "SET_SYMBOL_PROPERTIES", value: null });

  const submit = async () => {
    const trimmedRef = reference.trim();
    if (!trimmedRef) {
      setError("Reference cannot be blank.");
      return;
    }
    if (trimmedRef !== symbol.id) {
      const ok = await api.cmd({ op: "rename_symbol", id: symbol.id, new_id: trimmedRef });
      if (!ok) {
        setError(`"${trimmedRef}" is already used by another symbol.`);
        return;
      }
    }
    await api.cmd({ op: "edit_symbol_fields", id: trimmedRef, value, footprint, datasheet });
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 380 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Symbol Properties</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "90px 1fr" }}>
            <span>Reference</span>
            <input
              ref={refInput}
              value={reference}
              onChange={(e) => setReference(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            <span>Value</span>
            <input
              ref={valueInput}
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            <span>Footprint</span>
            <div style={{ display: "flex", gap: 4 }}>
              <input
                ref={footprintInput}
                style={{ flex: 1, minWidth: 0 }}
                value={footprint}
                onChange={(e) => setFootprint(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") submit();
                }}
              />
              <button onClick={() => setChoosingFootprint(true)} title="Choose a footprint from KiCad's libraries (Footprint Chooser)" data-browse-footprint>
                Browse…
              </button>
            </div>
            <span>Datasheet</span>
            <input
              ref={datasheetInput}
              value={datasheet}
              onChange={(e) => setDatasheet(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            <span>Library</span>
            <span style={{ opacity: 0.7 }}>{symbol.lib_id ?? "(generic)"}</span>
          </div>
          {error && (
            <p style={{ color: "var(--chrome-danger)", fontSize: 11, marginTop: 8 }}>{error}</p>
          )}
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>
            Renaming only relabels this schematic symbol and the wires/power-symbols/no-connects already on this sheet that name it -- it does not retarget anything already placed on the PCB tab under the old reference.
          </p>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={submit}>
            OK
          </button>
        </div>
      </div>
      {choosingFootprint && (
        <FootprintChooserDialog
          preselect={footprint.includes(":") ? footprint : null}
          pinCount={libInfo?.pins ?? instancePins}
          fpFilters={libInfo?.filters}
          onCancel={() => setChoosingFootprint(false)}
          onChoose={(pick) => {
            setChoosingFootprint(false);
            if (pick.kind === "footprint") setFootprint(pick.name);
          }}
        />
      )}
    </div>
  );
}
