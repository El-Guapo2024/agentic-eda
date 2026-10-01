// Port of eeschema/dialogs/dialog_pin_properties.cpp -- number, name,
// electrical type, graphic shape, orientation, length, name/number text
// size, position, visible, "common to all units"/"common to all body
// styles". Not ported: the "Alternate pin function definitions" grid
// (a real but rarely-used sub-feature -- defining extra name/type/shape
// combinations the same physical pin can switch between, e.g. a
// microcontroller pin with both a GPIO and a UART alternate function;
// this app's own `LibrarySymbolPin` has no field for it, a documented
// gap, see PARITY-symedit.md) and the live preview pane (the main canvas
// already shows the pin as drawn, one dialog-width away).
import { useEffect, useState } from "react";
import type { LibrarySymbolPin, PinElectricalType, PinShape } from "../../api/types";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { useStudioState } from "../../state/store";
import { umFrom, umTo } from "../../state/units";
import { angleDegToOrientation, orientationToAngleDeg, PIN_ORIENTATIONS, type PinOrientation } from "../../kicad-port/pinOrientation";

/** `pin_type.cpp::InitTables`'s own combo order, confirmed directly from source this session. */
const ELECTRICAL_TYPE_OPTIONS: { value: PinElectricalType; label: string }[] = [
  { value: "input", label: "Input" },
  { value: "output", label: "Output" },
  { value: "bidirectional", label: "Bidirectional" },
  { value: "tri_state", label: "Tri-state" },
  { value: "passive", label: "Passive" },
  { value: "free", label: "Free" },
  { value: "unspecified", label: "Unspecified" },
  { value: "power_in", label: "Power input" },
  { value: "power_out", label: "Power output" },
  { value: "open_collector", label: "Open collector" },
  { value: "open_emitter", label: "Open emitter" },
  { value: "no_connect", label: "Unconnected" },
];

/** Same source, the Graphic Style combo's own order. */
const SHAPE_OPTIONS: { value: PinShape; label: string }[] = [
  { value: "line", label: "Line" },
  { value: "inverted", label: "Inverted" },
  { value: "clock", label: "Clock" },
  { value: "inverted_clock", label: "Inverted clock" },
  { value: "input_low", label: "Input low" },
  { value: "clock_low", label: "Clock low" },
  { value: "output_low", label: "Output low" },
  { value: "edge_clock_high", label: "Falling edge clock" },
  { value: "non_logic", label: "NonLogic" },
];

const ORIENTATION_LABELS: Record<PinOrientation, string> = { right: "Right", left: "Left", up: "Up", down: "Down" };

/** mm<->display-unit, reusing `state/units.ts`'s own per-user preference (mm/mil/in) by routing through its µm-scaled helpers -- a library symbol's own fields are already mm, not µm (see `LibrarySymbolPin`'s own doc), so this is the one seam that lets this dialog honor the same unit choice every other length field in the app does. */
function mmTo(mm: number, unit: Parameters<typeof umTo>[1]): number {
  return umTo(mm * 1000, unit);
}
function mmFrom(display: number, unit: Parameters<typeof umFrom>[1]): number {
  return umFrom(display, unit) / 1000;
}

export function PinPropertiesDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const units = useStudioState().units;
  const id = state.pinPropertiesId;
  const pin = id ? state.symbol?.pins.find((p) => p.id === id) : undefined;
  const close = () => dispatch({ type: "SET_PIN_PROPERTIES_ID", id: null });

  const [form, setForm] = useState<LibrarySymbolPin | null>(null);

  // Populate the editable copy only when the dialog opens on a (possibly
  // different) pin -- same convention every other dialog in this app
  // uses, so a mid-edit keystroke survives the next ~700ms poll.
  useEffect(() => {
    if (pin) setForm(pin);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  if (!id || !pin || !form) return null;

  const set = <K extends keyof LibrarySymbolPin>(key: K, value: LibrarySymbolPin[K]) => setForm((f) => (f ? { ...f, [key]: value } : f));
  const len = (mm: number, onChange: (mm: number) => void, width = 80) => (
    <span>
      <input type="number" step="any" value={mmTo(mm, units)} onChange={(e) => Number.isFinite(Number(e.target.value)) && onChange(mmFrom(Number(e.target.value), units))} style={{ width }} /> {units}
    </span>
  );

  const submit = async () => {
    if (await api.editPin(id, form)) close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 460 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Pin Properties</span>
          <span>{pin.number || "(no number)"}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "72vh", overflowY: "auto" }}>
          <div className="kv-grid" style={{ gridTemplateColumns: "150px 1fr" }}>
            <span>Pin name</span>
            <input value={form.name} onChange={(e) => set("name", e.target.value)} style={{ width: 140 }} />
            <span>Pin number</span>
            <input value={form.number} onChange={(e) => set("number", e.target.value)} style={{ width: 80 }} />

            <span>Electrical type</span>
            <select value={form.electrical_type} onChange={(e) => set("electrical_type", e.target.value as PinElectricalType)}>
              {ELECTRICAL_TYPE_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
            <span>Graphic style</span>
            <select value={form.shape} onChange={(e) => set("shape", e.target.value as PinShape)}>
              {SHAPE_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
            <span>Orientation</span>
            <select value={angleDegToOrientation(form.angle_deg)} onChange={(e) => set("angle_deg", orientationToAngleDeg(e.target.value as PinOrientation))}>
              {PIN_ORIENTATIONS.map((o) => (
                <option key={o} value={o}>
                  {ORIENTATION_LABELS[o]}
                </option>
              ))}
            </select>

            <span>Pin length</span>
            {len(form.length_mm, (v) => set("length_mm", Math.max(0, v)))}
            <span>Name text size</span>
            {len(form.name_size_mm ?? 1.27, (v) => set("name_size_mm", Math.max(0.1, v)))}
            <span>Number text size</span>
            {len(form.number_size_mm ?? 1.27, (v) => set("number_size_mm", Math.max(0.1, v)))}

            <span>Position</span>
            <span>
              {len(form.at.x, (x) => set("at", { ...form.at, x }))} {len(form.at.y, (y) => set("at", { ...form.at, y }))}
            </span>

            <span>Visible</span>
            <label>
              <input type="checkbox" checked={!form.hidden} onChange={(e) => set("hidden", !e.target.checked)} />
            </label>

            <span>Common to all units</span>
            <label>
              <input type="checkbox" checked={form.unit === 0} onChange={(e) => set("unit", e.target.checked ? 0 : state.activeUnit)} />
            </label>
            <span>Common to all body styles</span>
            <label>
              <input type="checkbox" checked={form.body_style === 0} onChange={(e) => set("body_style", e.target.checked ? 0 : state.activeBodyStyle)} />
            </label>
          </div>
        </div>
        <div className="dialog-footer">
          <button
            onClick={() => {
              void api.deletePin(id);
              close();
            }}
          >
            Delete Pin
          </button>
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={submit}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
