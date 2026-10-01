// pcbnew.InteractiveDrawing.zone ("Draw Filled Zones") and "E"/double-click
// on an existing zone -- port of pcbnew/dialogs/panel_zone_properties.cpp
// (dialog_copper_zones.cpp's one real panel), covering every
// `ZONE_SETTINGS` field the IR actually models (crates/model/src/ir.rs
// `Zone`). Two fields real KiCad also shows -- zone Name and per-layer
// hatch-offset overrides -- have no IR equivalent (no `name`/layer-set
// concept on this model's single-layer `Zone`) and are left out, same
// "nothing to show, not a bug" convention this app's other dialogs use for
// a field with no backing data.
//
// KiCad asks for a zone's full settings *before* you draw the outline;
// this app's canvas draws the outline first (Canvas.tsx's finishDraw()
// moves it into state.zonePending once it has 3+ points), then this
// dialog turns it into a real `add_zone` (+ an immediate `edit_zone` if
// any setting was actually customized -- see `api.addZone`'s own doc).
// The same dialog, in "edit" mode (state.zoneEditId), re-opens on an
// existing zone's current settings and commits everything through
// `edit_zone` alone (outline unchanged -- no point editor yet).
import { useEffect, useMemo, useState } from "react";
import type { FillMode, IslandRemovalMode, PadConnection, ZoneSettingsFields } from "../api/types";
import { DEFAULT_ZONE_SETTINGS, useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { umFrom, umTo } from "../state/units";

const PAD_CONNECTION_OPTIONS: { value: PadConnection; label: string }[] = [
  { value: "Full", label: "Solid" },
  { value: "Thermal", label: "Thermal reliefs" },
  { value: "ThtThermal", label: "Thermal reliefs for PTH only" },
  { value: "None", label: "None" },
];

const ISLAND_REMOVAL_OPTIONS: { value: IslandRemovalMode; label: string }[] = [
  { value: "Always", label: "Remove islands" },
  { value: "Never", label: "Keep islands" },
  { value: "Area", label: "Remove islands below area limit" },
];

/** A length field bound to one `ZoneSettingsFields` key, shown/edited in the status bar's current unit -- converts to/from µm only at the input boundary, same spirit `MoveExactDialog.tsx` already uses. */
function LengthInput({ valueUm, unit, onChange, disabled }: { valueUm: number; unit: Parameters<typeof umTo>[1]; onChange: (um: number) => void; disabled?: boolean }) {
  return (
    <input
      type="number"
      step="any"
      disabled={disabled}
      value={umTo(valueUm, unit)}
      onChange={(e) => {
        const n = Number(e.target.value);
        if (Number.isFinite(n)) onChange(Math.round(umFrom(n, unit)));
      }}
      style={{ width: 90 }}
    />
  );
}

export function ZoneDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const units = state.units;

  const addOutline = state.zonePending;
  const editId = state.zoneEditId;
  const editZone = editId ? api.zoneById(editId) : undefined;
  const open = addOutline != null || (editId != null && editZone != null);

  const nets = useMemo(() => {
    const set = new Set<string>();
    for (const p of state.board?.parts ?? []) for (const pad of p.pads ?? []) if (pad.net) set.add(pad.net);
    return [...set].sort();
  }, [state.board]);

  const [net, setNet] = useState("");
  const [layer, setLayer] = useState("");
  const [settings, setSettings] = useState<ZoneSettingsFields>(DEFAULT_ZONE_SETTINGS);

  // (Re)populate whenever the dialog opens on a *different* target --
  // never on every render, or a mid-edit keystroke would be clobbered by
  // this same effect re-reading the (not yet re-fetched) board.
  useEffect(() => {
    if (!open) return;
    if (editZone) {
      setNet(editZone.net);
      setLayer(editZone.layer);
      setSettings({ ...editZone });
    } else {
      setNet("");
      setLayer("");
      setSettings(DEFAULT_ZONE_SETTINGS);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, editId]);

  if (!open) return null;

  const effectiveNet = net || nets[0] || "";
  const effectiveLayer = layer || state.activeLayer || state.board?.layers[0] || "F.Cu";
  const set = <K extends keyof ZoneSettingsFields>(key: K, value: ZoneSettingsFields[K]) => setSettings((s) => ({ ...s, [key]: value }));

  const close = () => {
    dispatch({ type: "SET_ZONE_PENDING", outline: null });
    dispatch({ type: "SET_ZONE_EDIT_ID", id: null });
  };

  // panel_zone_properties.cpp's AcceptOptions(): the one cross-field check
  // this app's backend also enforces (`ops_bad_zone`) -- caught here too
  // so the dialog can disable OK instead of round-tripping a refusal.
  const thermalSpokeTooNarrow = settings.thermal_spoke_width < settings.min_thickness;
  const canSubmit = !!effectiveNet && !thermalSpokeTooNarrow && settings.clearance >= 0 && settings.min_thickness > 0;

  const submit = async () => {
    if (!canSubmit) return;
    if (editZone) {
      const ok = await api.cmd({ op: "edit_zone", id: editZone.id, net: effectiveNet, layer: effectiveLayer, ...settings });
      if (ok) close();
    } else if (addOutline) {
      await api.addZone(effectiveNet, effectiveLayer, addOutline, settings);
      close();
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 460 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{editZone ? "Zone Properties" : "Zone Properties -- New Zone"}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "70vh", overflowY: "auto" }}>
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
            <span>Net</span>
            <select value={effectiveNet} onChange={(e) => setNet(e.target.value)}>
              {nets.length === 0 && <option value="">(no nets on this board)</option>}
              {nets.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
            <span>Layer</span>
            <select value={effectiveLayer} onChange={(e) => setLayer(e.target.value)}>
              {(state.board?.layers ?? []).map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>

            <span>Clearance</span>
            <span>
              <LengthInput valueUm={settings.clearance} unit={units} onChange={(v) => set("clearance", v)} /> {units}
            </span>
            <span>Minimum width</span>
            <span>
              <LengthInput valueUm={settings.min_thickness} unit={units} onChange={(v) => set("min_thickness", v)} /> {units}
            </span>
            <span>Priority</span>
            <span>
              <input type="number" step={1} min={0} value={settings.priority} onChange={(e) => set("priority", Math.max(0, Math.round(Number(e.target.value) || 0)))} style={{ width: 90 }} />
            </span>
          </div>

          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>Pad connections</p>
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
            <span>Connection</span>
            <select value={settings.pad_connection} onChange={(e) => set("pad_connection", e.target.value as PadConnection)}>
              {PAD_CONNECTION_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
            {/* panel_zone_properties.cpp's own comment: thermal gap/spoke width are never disabled by the connection choice above -- a per-pad override can still need them even when the zone itself is Full/None. */}
            <span>Thermal relief gap</span>
            <span>
              <LengthInput valueUm={settings.thermal_gap} unit={units} onChange={(v) => set("thermal_gap", v)} /> {units}
            </span>
            <span>Thermal spoke width</span>
            <span>
              <LengthInput valueUm={settings.thermal_spoke_width} unit={units} onChange={(v) => set("thermal_spoke_width", v)} /> {units}
              {thermalSpokeTooNarrow && <span style={{ color: "#ef5b5b", marginLeft: 8 }}>cannot be smaller than minimum width</span>}
            </span>
          </div>

          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>Islands</p>
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
            <span>Remove islands</span>
            <select value={settings.island_removal_mode} onChange={(e) => set("island_removal_mode", e.target.value as IslandRemovalMode)}>
              {ISLAND_REMOVAL_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
            {settings.island_removal_mode === "Area" && (
              <>
                <span>Minimum island area</span>
                <span>
                  <input
                    type="number"
                    step="any"
                    value={settings.min_island_area / 1_000_000}
                    onChange={(e) => {
                      const n = Number(e.target.value);
                      if (Number.isFinite(n)) set("min_island_area", Math.round(n * 1_000_000));
                    }}
                    style={{ width: 90 }}
                  />{" "}
                  mm&sup2;
                </span>
              </>
            )}
          </div>

          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>Fill</p>
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
            <span>Fill type</span>
            <select value={settings.fill_mode} onChange={(e) => set("fill_mode", e.target.value as FillMode)}>
              <option value="Polygons">Solid fill</option>
              <option value="HatchPattern">Hatch pattern</option>
            </select>
            {settings.fill_mode === "HatchPattern" && (
              <>
                <span>Hatch width</span>
                <span>
                  <LengthInput valueUm={settings.hatch_thickness} unit={units} onChange={(v) => set("hatch_thickness", v)} /> {units}
                </span>
                <span>Hatch gap</span>
                <span>
                  <LengthInput valueUm={settings.hatch_gap} unit={units} onChange={(v) => set("hatch_gap", v)} /> {units}
                </span>
                <span>Orientation</span>
                <span>
                  <input
                    type="number"
                    step="any"
                    value={settings.hatch_orientation_mdeg / 1000}
                    onChange={(e) => {
                      const n = Number(e.target.value);
                      if (Number.isFinite(n)) set("hatch_orientation_mdeg", Math.round(n * 1000));
                    }}
                    style={{ width: 90 }}
                  />
                  °
                </span>
                <span>Smoothing level</span>
                <span>
                  <input
                    type="number"
                    step={1}
                    min={0}
                    max={3}
                    value={settings.hatch_smoothing_level}
                    onChange={(e) => set("hatch_smoothing_level", Math.max(0, Math.min(3, Math.round(Number(e.target.value) || 0))))}
                    style={{ width: 90 }}
                  />
                </span>
                <span>Smoothing value</span>
                <span>
                  <input
                    type="number"
                    step="any"
                    min={0}
                    max={1}
                    value={settings.hatch_smoothing_value}
                    onChange={(e) => {
                      const n = Number(e.target.value);
                      if (Number.isFinite(n)) set("hatch_smoothing_value", Math.max(0, Math.min(1, n)));
                    }}
                    style={{ width: 90 }}
                  />
                </span>
              </>
            )}
          </div>

          {addOutline && <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>{addOutline.length} point outline.</p>}
        </div>
        <div className="dialog-footer">
          {editZone && (
            <button
              onClick={() => {
                api.cmd({ op: "delete_zone", id: editZone.id });
                close();
              }}
            >
              Delete Zone
            </button>
          )}
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!canSubmit} onClick={submit}>
            {editZone ? "OK" : "Add Zone"}
          </button>
        </div>
      </div>
    </div>
  );
}
