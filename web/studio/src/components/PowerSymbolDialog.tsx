// `P` (sch_drawing_tools.cpp PlaceSymbol, power filter): real eeschema
// opens the full symbol chooser filtered to power-type symbols *before*
// the item follows the cursor; this app has no searchable chooser yet
// (that's item 3's own, separate task -- `A`), so `P` gets its own small
// quick-pick instead, after the click (same "click/draw first, small
// dialog last" shape LabelDialog/ZoneDialog/TextDialog already use). Any
// "power:<net>" lib_id resolves to a real rail symbol on the backend
// (eda_model::symbol::builtin's `power_rail` fallback -- see PARITY-sch.md),
// so the presets below are just the common ones; typing any other name
// into the box works too.
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";

const PRESETS = ["power:GND", "power:+5V", "power:+3V3", "power:+12V", "power:-12V", "power:VCC", "power:VDD", "power:VSS", "power:PWR_FLAG"];

/** eeschema's own 4-way orientation choices for a power symbol (R/no-hotkey-equivalent during this app's click-then-dialog flow -- see this file's header comment on why rotation is a dialog field here instead of a live mid-placement spin). */
const ORIENTATIONS: Array<{ label: string; millideg: number }> = [
  { label: "0°", millideg: 0 },
  { label: "90°", millideg: 90_000 },
  { label: "180°", millideg: 180_000 },
  { label: "270°", millideg: 270_000 },
];

function netOf(libId: string): string {
  return libId.startsWith("power:") ? libId.slice("power:".length) : libId;
}

export function PowerSymbolDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const pending = state.schPowerPending;

  const [libId, setLibId] = useState(state.lastPowerLibId);
  const [rotMillideg, setRotMillideg] = useState(0);

  useEffect(() => {
    if (!pending) return;
    setLibId(state.lastPowerLibId);
    setRotMillideg(0);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending?.at[0], pending?.at[1]]);

  if (!pending) return null;
  const close = () => dispatch({ type: "SET_SCH_POWER_PENDING", pending: null });

  const submit = () => {
    const trimmed = libId.trim();
    if (!trimmed) return;
    const full = trimmed.includes(":") ? trimmed : `power:${trimmed}`;
    api.cmd({ op: "add_power_symbol", lib_id: full, at: { x: pending.at[0], y: pending.at[1] }, rot_millideg: rotMillideg, net: netOf(full), pin: "" });
    dispatch({ type: "SET_LAST_POWER_LIB_ID", libId: full });
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 320 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Power Symbol</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "80px 1fr" }}>
            <span>Symbol</span>
            <input
              autoFocus
              list="power-symbol-presets"
              value={libId}
              onChange={(e) => setLibId(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            <datalist id="power-symbol-presets">
              {PRESETS.map((p) => (
                <option key={p} value={p} />
              ))}
            </datalist>
            <span>Orientation</span>
            <select value={rotMillideg} onChange={(e) => setRotMillideg(Number(e.target.value))}>
              {ORIENTATIONS.map((o) => (
                <option key={o.millideg} value={o.millideg}>
                  {o.label}
                </option>
              ))}
            </select>
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!libId.trim()} onClick={submit}>
            Add
          </button>
        </div>
      </div>
    </div>
  );
}
