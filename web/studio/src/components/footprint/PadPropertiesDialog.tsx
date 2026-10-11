// Port of pcbnew/dialogs/dialog_pad_properties.cpp (GAPS.md #8 step 3) --
// shape/size, offset, drill, layer presets, clearance/thermal overrides,
// roundrect ratio. Not ported: padstack mode (front/inner/back-differs --
// this app's board has no inner-copper-layer concept to differ over),
// the fabrication-property dropdown (BGA/fiducial/testpoint/...; cosmetic
// metadata with no consumer anywhere in this app yet), and a true custom
// (primitive-based) pad shape -- see `LibraryPadShape`'s own doc.
import { useEffect, useState } from "react";
import type { LibraryPad, PadConnection, PadKind } from "../../api/types";
import { useFpApi, useFpState, useFpDispatch } from "../../state/footprintEditorStore";
import { useStudioState } from "../../state/store";
import { umFrom, umTo } from "../../state/units";
import { PAD_CONNECTION_OPTIONS, defaultSpokeAngleDeg, importPadSettings, settingsOf } from "../../kicad-port/padSettings";
import { chooseShape, libraryFieldsOf, shapeFormOfLibraryPad, type PadShapeChoice, type PadShapeForm } from "../../kicad-port/padShape";
import { PadPrimitivePanel, PadShapeRows } from "../PadShapeFields";

const KIND_OPTIONS: { value: PadKind; label: string }[] = [
  { value: "smd", label: "SMD" },
  { value: "through_hole", label: "Through hole" },
  { value: "non_plated_hole", label: "NPTH, mechanical" },
];

/** `pad.cpp`'s static layer masks (`PTHMask`/`SMDMask`/`ConnSMDMask`/`UnplatedHoleMask`) -- the dialog's own "preset" buttons rather than a full per-layer checkbox grid, matching this task's own "layers (SMD/THT/NPTH/connector presets)" scope. */
const LAYER_PRESETS: { label: string; kind: PadKind; layers: string[] }[] = [
  { label: "SMD", kind: "smd", layers: ["F.Cu", "F.Paste", "F.Mask"] },
  { label: "Connector (no paste)", kind: "smd", layers: ["F.Cu", "F.Mask"] },
  { label: "THT", kind: "through_hole", layers: ["*.Cu", "F.Mask", "B.Mask"] },
  { label: "NPTH, mechanical", kind: "non_plated_hole", layers: ["*.Cu", "*.Mask"] },
];

export function PadPropertiesDialog() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const units = useStudioState().units;
  // `pcbnew.PadTool.defaultPadProperties`: the same dialog on the default pad (`ShowPadPropertiesDialog( nullptr )` -> `m_Pad_Master`), which has
  // no number and no position, and OK stores into the default pad instead of editing a pad of the footprint.
  const defaultMode = state.defaultPadOpen;
  const id = state.padPropertiesId;
  const pad: LibraryPad | undefined = defaultMode ? { id: "", number: "", at: { x: 0, y: 0 }, ...state.defaultPad } : id ? state.footprint?.pads.find((p) => p.id === id) : undefined;
  const close = () => (defaultMode ? dispatch({ type: "SET_DEFAULT_PAD_OPEN", open: false }) : dispatch({ type: "SET_PAD_PROPERTIES_ID", id: null }));

  const [form, setForm] = useState<LibraryPad | null>(null);
  /** The shape of the pad, with what goes with it (`kicad-port/padShape.ts`); laid back on the pad by OK. */
  const [shapeForm, setShapeForm] = useState<PadShapeForm | null>(null);

  // Populate the editable copy only when the dialog opens on a (possibly
  // different) pad -- not every render, same convention every other
  // dialog in this app uses so a mid-edit keystroke survives the next
  // ~700ms poll.
  useEffect(() => {
    if (pad) {
      setForm(pad);
      setShapeForm(shapeFormOfLibraryPad(pad));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, defaultMode]);

  if ((!defaultMode && !id) || !pad || !form || !shapeForm) return null;

  const set = <K extends keyof LibraryPad>(key: K, value: LibraryPad[K]) => setForm((f) => (f ? { ...f, [key]: value } : f));
  const len = (valueUm: number, onChange: (um: number) => void, width = 80) => (
    <span>
      <input type="number" step="any" value={umTo(valueUm, units)} onChange={(e) => Number.isFinite(Number(e.target.value)) && onChange(Math.round(umFrom(Number(e.target.value), units)))} style={{ width }} /> {units}
    </span>
  );

  const submit = async () => {
    // The shape, the size and what goes with them are the shape form's.
    const edited: LibraryPad = { ...form, ...libraryFieldsOf(shapeForm) };
    if (defaultMode) {
      // `m_Pad_Master` takes what the dialog holds (the circle / SMD fix-ups of `ImportSettingsFrom` included).
      dispatch({ type: "SET_DEFAULT_PAD", pad: settingsOf(importPadSettings(edited, edited)) });
      close();
      return;
    }
    if (id && (await api.editPad(id, edited))) close();
  };

  /** `onPadShapeSelection`: the choice, with the defaults it sets up and the spoke angle that follows a circle. */
  const chooseShapeOf = (choice: PadShapeChoice) => {
    const was = form.thermal_spoke_angle_mdeg ?? null;
    const r = chooseShape(shapeForm, choice, was === null ? 90 : was / 1000);
    setShapeForm(r.form);
    if (was !== null) set("thermal_spoke_angle_mdeg", Math.round(r.spokeAngleDeg * 1000));
  };

  const hasDrill = form.kind !== "smd";
  const isSlot = form.drill_slot != null;

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 480 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{defaultMode ? "Default Pad Properties" : "Pad Properties"}</span>
          <span>{defaultMode ? "used by Add Pad" : pad.number || "(no number)"}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "72vh", overflowY: "auto" }}>
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
            {!defaultMode && (
              <>
                <span>Number</span>
                <input value={form.number} onChange={(e) => set("number", e.target.value)} style={{ width: 80 }} />
              </>
            )}
            <span>Pad type</span>
            <select value={form.kind} onChange={(e) => set("kind", e.target.value as PadKind)}>
              {KIND_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>

            {!defaultMode && (
              <>
                <span>Position</span>
                <span>
                  {len(form.at.x, (x) => set("at", { ...form.at, x }))} {len(form.at.y, (y) => set("at", { ...form.at, y }))}
                </span>
              </>
            )}
            <span>Rotation</span>
            <span>
              <input type="number" step="any" value={form.rot / 1000} onChange={(e) => Number.isFinite(Number(e.target.value)) && set("rot", Math.round(Number(e.target.value) * 1000))} style={{ width: 80 }} />°
            </span>

            <PadShapeRows form={shapeForm} onForm={setShapeForm} units={units} onChoose={chooseShapeOf} />
            <span>Offset</span>
            <span>
              {len(form.offset.x, (x) => set("offset", { ...form.offset, x }))} {len(form.offset.y, (y) => set("offset", { ...form.offset, y }))}
            </span>

            <span>Layer preset</span>
            <span>
              {LAYER_PRESETS.map((p) => (
                <button
                  key={p.label}
                  className={form.kind === p.kind && form.layers.join(",") === p.layers.join(",") ? "primary" : undefined}
                  style={{ marginRight: 6, marginBottom: 4 }}
                  onClick={() => setForm((f) => (f ? { ...f, kind: p.kind, layers: p.layers } : f))}
                >
                  {p.label}
                </button>
              ))}
            </span>
            <span>Layers</span>
            <span style={{ color: "var(--chrome-text-dim)" }}>{form.layers.join(", ") || "(none set)"}</span>

            {hasDrill && (
              <>
                <span>Drill</span>
                <span>
                  <label style={{ marginRight: 10 }}>
                    <input type="radio" checked={!isSlot} onChange={() => setForm((f) => (f ? { ...f, drill: f.drill ?? 800, drill_slot: null } : f))} /> Round
                  </label>
                  <label>
                    <input type="radio" checked={isSlot} onChange={() => setForm((f) => (f ? { ...f, drill_slot: f.drill_slot ?? [600, 1000], drill: null } : f))} /> Slot
                  </label>
                </span>
                {!isSlot && (
                  <>
                    <span />
                    <span>{len(form.drill ?? 800, (d) => set("drill", d))}</span>
                  </>
                )}
                {isSlot && (
                  <>
                    <span />
                    <span>
                      {len(form.drill_slot?.[0] ?? 600, (w) => set("drill_slot", [w, form.drill_slot?.[1] ?? 1000]))} × {len(form.drill_slot?.[1] ?? 1000, (h) => set("drill_slot", [form.drill_slot?.[0] ?? 600, h]))}
                    </span>
                  </>
                )}
              </>
            )}
          </div>

          <PadPrimitivePanel form={shapeForm} onForm={setShapeForm} units={units} />

          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>Clearance overrides and zone connection (blank = inherit)</p>
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
            <span>Clearance</span>
            <OverrideInput valueUm={form.clearance_override} units={units} onChange={(v) => set("clearance_override", v)} />
            <span>Pad connection</span>
            <select value={form.zone_connection ?? ""} onChange={(e) => set("zone_connection", e.target.value === "" ? null : (e.target.value as PadConnection))} title="PAD::GetLocalZoneConnection: how a copper zone connects to this pad, over its footprint's and the zone's own setting">
              {PAD_CONNECTION_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
            <span>Thermal relief gap</span>
            <OverrideInput valueUm={form.thermal_gap_override} units={units} onChange={(v) => set("thermal_gap_override", v)} />
            <span>Thermal spoke width</span>
            <OverrideInput valueUm={form.thermal_spoke_width_override} units={units} onChange={(v) => set("thermal_spoke_width_override", v)} />
            <span>Thermal spoke angle</span>
            <AngleOverrideInput valueMdeg={form.thermal_spoke_angle_mdeg ?? null} defaultDeg={defaultSpokeAngleDeg(libraryFieldsOf(shapeForm).shape, libraryFieldsOf(shapeForm).custom?.anchor)} onChange={(v) => set("thermal_spoke_angle_mdeg", v)} />
          </div>
        </div>
        <div className="dialog-footer">
          {!defaultMode && id && (
            <button
              onClick={() => {
                void api.deletePad(id);
                close();
              }}
            >
              Delete Pad
            </button>
          )}
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={submit}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

/** The spoke angle: "unset" is the shape's default (90 degrees for an oval or rectangle, 45 for a circle), set is an angle in degrees as the `.kicad_pcb` stores it. */
export function AngleOverrideInput({ valueMdeg, defaultDeg, onChange, testid }: { valueMdeg: number | null; defaultDeg: number; onChange: (v: number | null) => void; testid?: string }) {
  const on = valueMdeg != null;
  return (
    <span>
      <label style={{ marginRight: 8 }}>
        <input type="checkbox" checked={on} onChange={(e) => onChange(e.target.checked ? defaultDeg * 1000 : null)} data-testid={testid ? `${testid}-on` : undefined} /> override
      </label>
      {on ? (
        <>
          <input type="number" step="any" value={valueMdeg / 1000} onChange={(e) => Number.isFinite(Number(e.target.value)) && onChange((((Math.round(Number(e.target.value) * 1000) % 360_000) + 360_000) % 360_000))} style={{ width: 80 }} data-testid={testid} />°
        </>
      ) : (
        <span style={{ color: "var(--chrome-text-dim)" }}>{defaultDeg}° (shape default)</span>
      )}
    </span>
  );
}

/** A length field that can also be "unset" (board default) -- the Pad Properties dialog's own clearance/thermal override convention (`None` = inherit). */
export function OverrideInput({ valueUm, units, onChange, testid }: { valueUm: number | null; units: Parameters<typeof umTo>[1]; onChange: (v: number | null) => void; testid?: string }) {
  const on = valueUm != null;
  return (
    <span>
      <label style={{ marginRight: 8 }}>
        <input type="checkbox" checked={on} onChange={(e) => onChange(e.target.checked ? 0 : null)} data-testid={testid ? `${testid}-on` : undefined} /> override
      </label>
      {on && (
        <input
          type="number"
          step="any"
          value={umTo(valueUm, units)}
          onChange={(e) => Number.isFinite(Number(e.target.value)) && onChange(Math.round(umFrom(Number(e.target.value), units)))}
          style={{ width: 80 }}
          data-testid={testid}
        />
      )}
      {on && ` ${units}`}
    </span>
  );
}
