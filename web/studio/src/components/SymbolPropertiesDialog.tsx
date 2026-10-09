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
//
// The "Show" column of the fields table (`DIALOG_SYMBOL_PROPERTIES`) shows or hides each of the four fields, and the Body style choice (Standard / Alternate, only for a symbol
// whose library symbol has two) puts the symbol in the other body style (`SCH_EDIT_FRAME::SelectBodyStyle`): both go in the same undo step as the texts.
import { useEffect, useRef, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { bodyStyleCount, setBodyStyleCmd } from "../kicad-port/schBodyStyle";
import { fieldVisibilityCmds, ownerKey } from "../kicad-port/schFieldEdit";
import type { Cmd } from "../api/types";

const MAIN_FIELDS = ["Reference", "Value", "Footprint", "Datasheet"] as const;

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
  /** The "Show" check of each main field, by name; `undefined` for a field the server did not send (an older one). */
  const [shown, setShown] = useState<Record<string, boolean>>({});
  const [bodyStyle, setBodyStyle] = useState(1);

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
    setShown(Object.fromEntries((symbol.fields ?? []).filter((f) => (MAIN_FIELDS as readonly string[]).includes(f.name)).map((f) => [f.name, f.visible])));
    setBodyStyle(symbol.body_style || 1);
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
    // the texts, the shown fields and the body style are one undo step (fields are keyed by the reference the symbol has now)
    const cmds: Cmd[] = [{ op: "edit_symbol_fields", id: trimmedRef, value, footprint, datasheet }, ...fieldVisibilityCmds(ownerKey({ id: trimmedRef, unit: symbol.unit }), symbol.fields, shown)];
    if (state.schematic && bodyStyle !== (symbol.body_style || 1)) {
      const change = setBodyStyleCmd(state.schematic, [trimmedRef], bodyStyle);
      if (change) cmds.push(change);
    }
    await api.cmdBatch(cmds);
    close();
  };
  const bodyStyles = state.schematic ? bodyStyleCount(state.schematic, symbol) : 1;
  /** The "Show" check of a main field's row; nothing for a field the server did not send. */
  const showBox = (name: string) =>
    shown[name] === undefined ? (
      <span />
    ) : (
      <label title={`Show the ${name} field on the sheet`} style={{ whiteSpace: "nowrap" }}>
        <input type="checkbox" checked={shown[name]} onChange={(e) => setShown({ ...shown, [name]: e.target.checked })} /> Show
      </label>
    );

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 440 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Symbol Properties</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "90px 1fr auto" }}>
            <span>Reference</span>
            <input
              ref={refInput}
              value={reference}
              onChange={(e) => setReference(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            {showBox("Reference")}
            <span>Value</span>
            <input
              ref={valueInput}
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            {showBox("Value")}
            <span>Footprint</span>
            <input
              ref={footprintInput}
              value={footprint}
              onChange={(e) => setFootprint(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            {showBox("Footprint")}
            <span>Datasheet</span>
            <input
              ref={datasheetInput}
              value={datasheet}
              onChange={(e) => setDatasheet(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            {showBox("Datasheet")}
            <span>Library</span>
            <span style={{ opacity: 0.7 }}>{symbol.lib_id ?? "(generic)"}</span>
            <span />
            {bodyStyles > 1 && (
              <>
                <span>Body style</span>
                <select value={bodyStyle} onChange={(e) => setBodyStyle(Number(e.target.value))}>
                  {Array.from({ length: bodyStyles }, (_, i) => i + 1).map((n) => (
                    <option key={n} value={n}>
                      {/* `LIB_SYMBOL::GetBodyStyleDescription`: a De Morgan pair is Standard and Alternate */}
                      {n === 1 ? "Standard" : n === 2 ? "Alternate" : `Style ${n}`}
                    </option>
                  ))}
                </select>
                <span />
              </>
            )}
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
    </div>
  );
}
