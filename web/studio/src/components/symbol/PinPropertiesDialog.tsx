// Port of eeschema/dialogs/dialog_pin_properties.cpp -- number, name,
// electrical type, graphic shape, orientation, length, name/number text
// size, position, visible, "common to all units"/"common to all body
// styles", and the "Alternate pin function definitions" pane (the name,
// electrical type and graphic style a microcontroller pin switches to as
// a GPIO or a UART pin, `PinAlternatesGrid.tsx`). Not ported: the live
// preview pane (the main canvas already shows the pin as drawn, one
// dialog-width away).
//
// It is also where the Pin tool's click lands: `SYMBOL_EDITOR_PIN_TOOL::CreatePin` makes the pin and opens this dialog before adding it,
// so Cancel drops the pin and OK adds it (after `PlacePin`'s Synchronized Pins Mode confirmation). An existing pin has an id and is edited;
// the pending one (`state.pendingPin`) is added by OK and has no Delete Pin.
import { useEffect, useState } from "react";
import type { LibrarySymbolPin, PinElectricalType, PinShape } from "../../api/types";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { useStudioState } from "../../state/store";
import { nmFrom, nmTo } from "../../state/units";
import { angleDegToOrientation, orientationToAngleDeg, PIN_ORIENTATIONS, type PinOrientation } from "../../kicad-port/pinOrientation";
import { pinOccupiedMessage, pinPlacementOccupant } from "../../kicad-port/pinPlacement";
import { alternatesError, PIN_SHAPE_OPTIONS as SHAPE_OPTIONS, PIN_TYPE_OPTIONS as ELECTRICAL_TYPE_OPTIONS } from "../../kicad-port/symPinAlternates";
import { pinTemplateOf } from "../../kicad-port/symPinTemplate";
import { synchronizePins } from "../../kicad-port/symPinSync";
import { commonBodyStylesBox, commonUnitsBox, pinOffGridMessage, pinUnitsOnOk, syncPinsMessage } from "../../kicad-port/symPinDialog";
import { askConfirm } from "../library/libraryDialogs";
import { PinAlternatesGrid } from "./PinAlternatesGrid";

const ORIENTATION_LABELS: Record<PinOrientation, string> = { right: "Right", left: "Left", up: "Up", down: "Down" };

/** mm<->display-unit, reusing `state/units.ts`'s own per-user preference (mm/mil/in) by routing through its µm-scaled helpers -- a library symbol's own fields are already mm, not µm (see `LibrarySymbolPin`'s own doc), so this is the one seam that lets this dialog honor the same unit choice every other length field in the app does. */
function mmTo(mm: number, unit: Parameters<typeof nmTo>[1]): number {
  return nmTo(mm * 1e6, unit);
}
function mmFrom(display: number, unit: Parameters<typeof nmFrom>[1]): number {
  return nmFrom(display, unit) / 1e6;
}

export function PinPropertiesDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const units = useStudioState().units;
  const id = state.pinPropertiesId;
  /** The pin the Pin tool has just dropped (`SYMBOL_EDITOR_PIN_TOOL::CreatePin`): not in the document yet; OK adds it. */
  const pending = state.pendingPin;
  const pin = id ? state.symbol?.pins.find((p) => p.id === id) : pending ?? undefined;
  const close = () => {
    if (pending) dispatch({ type: "SET_PENDING_PIN", pin: null });
    else dispatch({ type: "SET_PIN_PROPERTIES_ID", id: null });
  };

  const [form, setForm] = useState<LibrarySymbolPin | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Populate the editable copy only when the dialog opens on a (possibly
  // different) pin -- same convention every other dialog in this app
  // uses, so a mid-edit keystroke survives the next ~700ms poll.
  useEffect(() => {
    if (pin) setForm(pin);
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, pending]);

  if (!pin || !form) return null;

  const set = <K extends keyof LibrarySymbolPin>(key: K, value: LibrarySymbolPin[K]) => setForm((f) => (f ? { ...f, [key]: value } : f));
  const len = (mm: number, onChange: (mm: number) => void, width = 80) => (
    <span>
      <input type="number" step="any" value={mmTo(mm, units)} onChange={(e) => Number.isFinite(Number(e.target.value)) && onChange(mmFrom(Number(e.target.value), units))} style={{ width }} /> {units}
    </span>
  );

  const sym = state.symbol;
  const unitCount = sym?.unit_count ?? 1;
  // `TransferDataToWindow`: the two "Common to all ..." boxes, their tooltips, and the info bar while `m_SyncPinEdit` is on
  const unitsBox = commonUnitsBox(form, unitCount, state.syncPins);
  const stylesBox = commonBodyStylesBox(form, !!sym?.has_alternate_body_style);

  const submit = async () => {
    // `TransferDataFromWindow`: a row of the alternates grid needs a name
    const problem = alternatesError(form.alternates ?? []);
    if (problem) return setError(problem);
    // "Only show the warning if the position has been changed": a pin off KiCad's standard grid asks first; No keeps the dialog open
    const offGrid = pinOffGridMessage(pin.at, form.at);
    if (offGrid && !(await askConfirm({ title: "Confirmation", message: offGrid, okLabel: "Yes", cancelLabel: "No" }))) return; // `IsOK`: Yes / No
    // `SetUnit( m_checkApplyToAllParts->GetValue() ? 0 : m_frame->GetUnit() )`, the same for the body style: a disabled box is unchecked for the unit
    const edited: LibrarySymbolPin = { ...form, ...pinUnitsOnOk(unitsBox.checked, stylesBox.checked, state.activeUnit, state.activeBodyStyle) };
    // `EditPinProperties` saves the sticky defaults (`g_LastPin*`) when OK is pressed, for an edited pin and for a new one before `PlacePin` places it -- so a
    // declined "Place Pin Anyway" keeps the new look too. Merely selecting a pin saves nothing.
    dispatch({ type: "SET_PIN_TEMPLATE", pin: pinTemplateOf(edited) });
    if (id) {
      if (await api.editPin(id, edited)) close();
      return;
    }
    // A pin the tool has just dropped: `PlacePin` asks first when it lands on another pin (Synchronized Pins Mode), and only then is it added.
    const syncMode = !!sym && synchronizePins(state.syncPins, sym.unit_count, !!sym.units_locked);
    const occupant = pinPlacementOccupant(edited, sym?.pins ?? [], syncMode);
    if (occupant && !(await askConfirm({ title: "Confirmation", message: pinOccupiedMessage(occupant.unit), okLabel: "Place Pin Anyway" }))) {
      dispatch({ type: "SET_PENDING_PIN", pin: null }); // PlacePin deletes the new pin and places nothing
      return;
    }
    if (!(await api.addPin(edited))) return; // the verb's refusal is the editor's toast; the dialog stays open on the pin
    dispatch({ type: "PIN_PLACED", pin: edited });
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 520 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Pin Properties</span>
          <span>{form.number || "(no number)"}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "72vh", overflowY: "auto" }}>
          {state.syncPins && (
            // the info bar of `m_SyncPinEdit`, with its "Exit sync pins mode" link (`m_frame->m_SyncPinEdit = false`)
            <div className="panel-empty" style={{ marginBottom: 8, display: "flex", justifyContent: "space-between", gap: 8 }} data-testid="pin-sync-infobar">
              <span>{syncPinsMessage(unitsBox.checked, !id)}</span>
              <a href="#" onClick={(e) => (e.preventDefault(), dispatch({ type: "SET_SYNC_PINS", on: false }))}>
                Exit sync pins mode
              </a>
            </div>
          )}
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

            <span>Common to all units in symbol</span>
            <label title={unitsBox.tooltip ?? undefined}>
              <input type="checkbox" data-testid="pin-common-units" disabled={!unitsBox.enabled} checked={unitsBox.checked} onChange={(e) => set("unit", e.target.checked ? 0 : state.activeUnit)} />
            </label>
            <span>Common to all body styles</span>
            <label>
              <input type="checkbox" data-testid="pin-common-styles" disabled={!stylesBox.enabled} checked={stylesBox.checked} onChange={(e) => set("body_style", e.target.checked ? 0 : state.activeBodyStyle)} />
            </label>
          </div>
          <PinAlternatesGrid rows={form.alternates ?? []} onChange={(rows) => set("alternates", rows)} pin={pin} available={!state.symbol?.has_alternate_body_style} onError={setError} />
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="pin-props-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          {id !== null && (
            <button
              onClick={() => {
                void api.deletePin(id);
                close();
              }}
            >
              Delete Pin
            </button>
          )}
          <button onClick={close} data-testid="pin-props-cancel">
            Cancel
          </button>
          <button className="primary" onClick={submit} data-testid="pin-props-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
