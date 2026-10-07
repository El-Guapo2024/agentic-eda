// Port of pcbnew/dialogs/dialog_pad_properties.cpp (GAPS.md #8 step 3) --
// shape/size, offset, drill, layer presets, clearance/thermal overrides,
// roundrect ratio. Not ported: padstack mode (front/inner/back-differs --
// this app's board has no inner-copper-layer concept to differ over),
// the fabrication-property dropdown (BGA/fiducial/testpoint/...; cosmetic
// metadata with no consumer anywhere in this app yet), and a true custom
// (primitive-based) pad shape -- see `LibraryPadShape`'s own doc.
import { useEffect, useState } from "react";
import type { ChamferCorners, LibraryPad, LibraryPadShape, PadKind } from "../../api/types";
import { useFpApi, useFpState, useFpDispatch } from "../../state/footprintEditorStore";
import { useStudioState } from "../../state/store";
import { formatLength, umFrom, umTo } from "../../state/units";
import { importPadSettings, settingsOf } from "../../kicad-port/padSettings";

const SHAPE_OPTIONS: { value: LibraryPadShape; label: string }[] = [
  { value: "circle", label: "Circle" },
  { value: "rect", label: "Rectangle" },
  { value: "oval", label: "Oval" },
  { value: "round_rect", label: "Rounded rectangle" },
  { value: "trapezoid", label: "Trapezoid" },
  { value: "chamfered_rect", label: "Chamfered rectangle" },
];

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

function emptyCorners(): ChamferCorners {
  return { top_left: false, top_right: false, bottom_left: false, bottom_right: false };
}

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

  // Populate the editable copy only when the dialog opens on a (possibly
  // different) pad -- not every render, same convention every other
  // dialog in this app uses so a mid-edit keystroke survives the next
  // ~700ms poll.
  useEffect(() => {
    if (pad) setForm(pad);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, defaultMode]);

  if ((!defaultMode && !id) || !pad || !form) return null;

  const set = <K extends keyof LibraryPad>(key: K, value: LibraryPad[K]) => setForm((f) => (f ? { ...f, [key]: value } : f));
  const len = (valueUm: number, onChange: (um: number) => void, width = 80) => (
    <span>
      <input type="number" step="any" value={umTo(valueUm, units)} onChange={(e) => Number.isFinite(Number(e.target.value)) && onChange(Math.round(umFrom(Number(e.target.value), units)))} style={{ width }} /> {units}
    </span>
  );

  const submit = async () => {
    if (defaultMode) {
      // `m_Pad_Master` takes what the dialog holds (the circle / SMD fix-ups of `ImportSettingsFrom` included).
      dispatch({ type: "SET_DEFAULT_PAD", pad: settingsOf(importPadSettings(form, form)) });
      close();
      return;
    }
    if (id && (await api.editPad(id, form))) close();
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

            <span>Shape</span>
            <select value={form.shape} onChange={(e) => set("shape", e.target.value as LibraryPadShape)}>
              {SHAPE_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
            <span>Size</span>
            <span>
              {len(form.size[0], (w) => set("size", form.shape === "circle" ? [w, w] : [w, form.size[1]]))}
              {form.shape !== "circle" && (
                <>
                  {" × "}
                  {len(form.size[1], (h) => set("size", [form.size[0], h]))}
                </>
              )}
            </span>
            <span>Offset</span>
            <span>
              {len(form.offset.x, (x) => set("offset", { ...form.offset, x }))} {len(form.offset.y, (y) => set("offset", { ...form.offset, y }))}
            </span>

            {form.shape === "round_rect" && (
              <>
                <span>Corner ratio</span>
                <span>
                  <input
                    type="number"
                    step="any"
                    min={0}
                    max={0.5}
                    value={form.roundrect_ratio ?? 0.25}
                    onChange={(e) => Number.isFinite(Number(e.target.value)) && set("roundrect_ratio", Number(e.target.value))}
                    style={{ width: 80 }}
                  />{" "}
                  (radius {formatLength((form.roundrect_ratio ?? 0.25) * Math.min(...form.size), units)})
                </span>
              </>
            )}
            {form.shape === "chamfered_rect" && (
              <>
                <span>Chamfer ratio</span>
                <span>
                  <input
                    type="number"
                    step="any"
                    min={0}
                    max={0.5}
                    value={form.chamfer_ratio ?? 0.2}
                    onChange={(e) => Number.isFinite(Number(e.target.value)) && set("chamfer_ratio", Number(e.target.value))}
                    style={{ width: 80 }}
                  />
                </span>
                <span>Chamfered corners</span>
                <span>
                  {(["top_left", "top_right", "bottom_left", "bottom_right"] as const).map((c) => (
                    <label key={c} style={{ marginRight: 10 }}>
                      <input
                        type="checkbox"
                        checked={form.chamfer_corners[c]}
                        onChange={(e) => set("chamfer_corners", { ...(form.chamfer_corners ?? emptyCorners()), [c]: e.target.checked })}
                      />{" "}
                      {c.replace("_", " ")}
                    </label>
                  ))}
                </span>
              </>
            )}
            {form.shape === "trapezoid" && (
              <>
                <span>Trapezoid delta</span>
                <span>
                  <select
                    value={(form.trapezoid_delta?.[0] ?? 0) !== 0 ? "x" : "y"}
                    onChange={(e) => {
                      const mag = form.trapezoid_delta ? Math.max(Math.abs(form.trapezoid_delta[0]), Math.abs(form.trapezoid_delta[1])) : 0;
                      set("trapezoid_delta", e.target.value === "x" ? [mag, 0] : [0, mag]);
                    }}
                    style={{ marginRight: 8 }}
                  >
                    <option value="x">X axis</option>
                    <option value="y">Y axis</option>
                  </select>
                  {len(Math.abs(form.trapezoid_delta?.[0] ?? form.trapezoid_delta?.[1] ?? 0), (v) =>
                    set("trapezoid_delta", (form.trapezoid_delta?.[0] ?? 0) !== 0 ? [v, 0] : [0, v])
                  )}
                </span>
              </>
            )}

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

          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>Clearance overrides (blank = board default)</p>
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
            <span>Clearance</span>
            <OverrideInput valueUm={form.clearance_override} units={units} onChange={(v) => set("clearance_override", v)} />
            <span>Thermal relief gap</span>
            <OverrideInput valueUm={form.thermal_gap_override} units={units} onChange={(v) => set("thermal_gap_override", v)} />
            <span>Thermal spoke width</span>
            <OverrideInput valueUm={form.thermal_spoke_width_override} units={units} onChange={(v) => set("thermal_spoke_width_override", v)} />
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

/** A length field that can also be "unset" (board default) -- the Pad Properties dialog's own clearance/thermal override convention (`None` = inherit). */
function OverrideInput({ valueUm, units, onChange }: { valueUm: number | null; units: Parameters<typeof umTo>[1]; onChange: (v: number | null) => void }) {
  const on = valueUm != null;
  return (
    <span>
      <label style={{ marginRight: 8 }}>
        <input type="checkbox" checked={on} onChange={(e) => onChange(e.target.checked ? 0 : null)} /> override
      </label>
      {on && (
        <input
          type="number"
          step="any"
          value={umTo(valueUm, units)}
          onChange={(e) => Number.isFinite(Number(e.target.value)) && onChange(Math.round(umFrom(Number(e.target.value), units)))}
          style={{ width: 80 }}
        />
      )}
      {on && ` ${units}`}
    </span>
  );
}
