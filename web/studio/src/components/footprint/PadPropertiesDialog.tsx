// Port of pcbnew/dialogs/dialog_pad_properties.cpp (GAPS.md #8 step 3) --
// shape/size (the nine shape choices, a trapezoid's delta, a chamfer, a custom
// pad's basic shapes: the rows are the board dialog's own, `PadShapeFields.tsx`),
// offset, drill, layer presets, clearance/thermal overrides, and for a plated
// through-hole pad the padstack (`kicad-port/padStack.ts`: "Padstack mode", "Edit
// layer", "Copper layers"). Not ported: the fabrication-property dropdown
// (BGA/fiducial/testpoint/...; cosmetic metadata with no consumer anywhere in
// this app yet).
import { useEffect, useState } from "react";
import type { LibraryPad, PadConnection, PadKind } from "../../api/types";
import { useFpApi, useFpState, useFpDispatch } from "../../state/footprintEditorStore";
import { useStudioState } from "../../state/store";
import { umFrom, umTo } from "../../state/units";
import { PAD_CONNECTION_OPTIONS, defaultSpokeAngleDeg, importPadSettings, settingsOf } from "../../kicad-port/padSettings";
import { chooseShape, libraryFieldsOf, shapeFormOfLibraryPad, type PadShapeChoice, type PadShapeForm } from "../../kicad-port/padShape";
import { FRONT, changeMode, checkStack, layerFormOf, padstackControlsShown, stackOf, stackStateOf, withLayerForm, type LayerForm, type PadStackState } from "../../kicad-port/padStack";
import { CopperLayersRow, PadPrimitivePanel, PadShapeRows, PadStackRows } from "../PadShapeFields";

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
  const studio = useStudioState();
  const units = studio.units;
  // `pcbnew.PadTool.defaultPadProperties`: the same dialog on the default pad (`ShowPadPropertiesDialog( nullptr )` -> `m_Pad_Master`), which has
  // no number and no position, and OK stores into the default pad instead of editing a pad of the footprint.
  const defaultMode = state.defaultPadOpen;
  const id = state.padPropertiesId;
  const pad: LibraryPad | undefined = defaultMode ? { id: "", number: "", at: { x: 0, y: 0 }, ...state.defaultPad } : id ? state.footprint?.pads.find((p) => p.id === id) : undefined;
  const close = () => (defaultMode ? dispatch({ type: "SET_DEFAULT_PAD_OPEN", open: false }) : dispatch({ type: "SET_PAD_PROPERTIES_ID", id: null }));

  const [form, setForm] = useState<LibraryPad | null>(null);
  /** The shape of the pad, with what goes with it (`kicad-port/padShape.ts`); laid back on the pad by OK. */
  const [shapeForm, setShapeForm] = useState<PadShapeForm | null>(null);
  /** The padstack (`kicad-port/padStack.ts`): the mode, the layer being edited and the shapes of the layers but the front, whose shape is `shapeForm` and offset `form.offset`. */
  const [stack, setStack] = useState<PadStackState | null>(null);

  // Populate the editable copy only when the dialog opens on a (possibly
  // different) pad -- not every render, same convention every other
  // dialog in this app uses so a mid-edit keystroke survives the next
  // ~700ms poll.
  useEffect(() => {
    if (pad) {
      const s = shapeFormOfLibraryPad(pad);
      setForm(pad);
      setShapeForm(s);
      setStack(stackStateOf(pad.stack, { s, ox: pad.offset.x, oy: pad.offset.y, tokens: [] }));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, defaultMode]);

  if ((!defaultMode && !id) || !pad || !form || !shapeForm || !stack) return null;

  const set = <K extends keyof LibraryPad>(key: K, value: LibraryPad[K]) => setForm((f) => (f ? { ...f, [key]: value } : f));
  const len = (valueUm: number, onChange: (um: number) => void, width = 80) => (
    <span>
      <input type="number" step="any" value={umTo(valueUm, units)} onChange={(e) => Number.isFinite(Number(e.target.value)) && onChange(Math.round(umFrom(Number(e.target.value), units)))} style={{ width }} /> {units}
    </span>
  );

  // The padstack: the front's shape is the form's own; the layer being edited is the one the shape rows and the offset show (plated through-hole pads only).
  const boardLayers = (studio.board?.layers ?? ["F.Cu", "B.Cu"]).filter((l) => l.endsWith(".Cu"));
  const stackShown = padstackControlsShown(form.kind);
  const frontForm: LayerForm = { s: shapeForm, ox: form.offset.x, oy: form.offset.y, tokens: [] };
  const editLayer = stackShown ? stack.editLayer : FRONT;
  const active = layerFormOf(stack, frontForm, editLayer);
  const setActive = (next: LayerForm) => {
    const r = withLayerForm(stack, frontForm, editLayer, next);
    setStack(r.state);
    setShapeForm(r.front.s);
    setForm((f) => (f ? { ...f, offset: { x: r.front.ox, y: r.front.oy } } : f));
  };

  const submit = async () => {
    // The shape, the size and what goes with them are the shape form's.
    const message = stackShown ? checkStack(stack) : null;
    if (message) {
      dispatch({ type: "TOAST", message, kind: "error" });
      return;
    }
    const edited: LibraryPad = { ...form, ...libraryFieldsOf(shapeForm) };
    // The padstack travels with the pad (`PAD::SetPadstack`); a pad that has none sends none.
    const stackNow = stackOf(stack);
    edited.stack = Object.keys(stackNow).length > 0 ? stackNow : undefined;
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

            {stackShown && (
              <PadStackRows
                state={stack}
                boardLayers={boardLayers}
                onMode={(mode) => setStack(changeMode(stack, mode, frontForm, boardLayers))}
                onEditLayer={(key) => setStack({ ...stack, editLayer: key })}
              />
            )}
            <PadShapeRows form={active.s} onForm={(next) => setActive({ ...active, s: next })} units={units} onChoose={editLayer === FRONT ? chooseShapeOf : undefined} />
            <span>Offset</span>
            <span>
              {len(active.ox, (x) => setActive({ ...active, ox: x }))} {len(active.oy, (y) => setActive({ ...active, oy: y }))}
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
            {stackShown && <CopperLayersRow value={stack.unconnected} boardLayers={boardLayers} onChange={(v) => setStack({ ...stack, unconnected: v })} />}

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

          <PadPrimitivePanel form={active.s} onForm={(next) => setActive({ ...active, s: next })} units={units} />

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
