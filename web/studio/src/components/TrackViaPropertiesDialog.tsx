// `DIALOG_TRACK_VIA_PROPERTIES` over the tracks and vias the dialog was opened on (kicad-port/trackViaDialog.ts holds the form and the edits): a field the items share
// shows its value, one where they differ is empty (indeterminate) and is left alone; OK sends what changed as one batch, one undo step.
import { useEffect, useMemo, useRef, useState } from "react";
import type { BoardState, Nm } from "../api/types";
import { boardLayerList, layerDisplayName } from "../kicad-port/boardLayers";
import { pcbContext } from "../kicad-port/pcbProperties";
import {
  ANNULAR_RING_CHOICES,
  BACKDRILL_CHOICES,
  editLayerChoices,
  PADSTACK_MODE_CHOICES,
  POST_MACHINE_CHOICES,
  sizeOn,
  trackViaCmds,
  trackViaForm,
  VIA_TYPE_CHOICES,
  viaLayerDepth,
  withNetAnswers,
  type BackdrillChoice,
  type PadstackMode,
  type PostChoice,
  type TrackViaForm,
  type ViaType,
} from "../kicad-port/trackViaDialog";
import { IPC4761_PRESETS } from "../kicad-port/globalEditTracksVias";
import type { Ipc4761Preset, UnconnectedLayers } from "../api/types";
import { TeardropFields } from "./TeardropFields";
import { NumberInput } from "./NumberInput";
import { postTrackViaPreview } from "../api/client";
import { askConfirm } from "./library/libraryDialogs";
import { useStudioApi } from "../state/store";
import { formatLength, nmFrom, nmTo, type LengthUnit } from "../state/units";

type LengthKey = "startX" | "startY" | "endX" | "endY" | "width" | "viaX" | "viaY" | "diameter" | "drill";

export function TrackViaPropertiesDialog({ board, ids, units, onClose }: { board: BoardState; ids: string[]; units: LengthUnit; onClose: () => void }) {
  const api = useStudioApi();
  const before = useMemo(() => trackViaForm(board, ids), [board, ids]);
  const [form, setForm] = useState<TrackViaForm>(before);
  const [error, setError] = useState<string | null>(null);
  // the fields as typed, in display units ("" is indeterminate)
  const shown = (v: Nm | null) => (v == null ? "" : String(Number(nmTo(v, units).toFixed(6))));
  const [texts, setTexts] = useState<Record<LengthKey, string>>(() => ({ startX: "", startY: "", endX: "", endY: "", width: "", viaX: "", viaY: "", diameter: "", drill: "" }));
  const opened = useRef(false);
  useEffect(() => {
    if (opened.current) return;
    opened.current = true;
    setForm(before);
    setTexts({ startX: shown(before.startX), startY: shown(before.startY), endX: shown(before.endX), endY: shown(before.endY), width: shown(before.width), viaX: shown(before.viaX), viaY: shown(before.viaY), diameter: shown(before.diameter), drill: shown(before.drill) });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const nets = pcbContext(board, units).nets;
  const layerSetup = board.board_rules?.layer_setup;
  const copper = boardLayerList(board.layers, layerSetup).filter((l) => l.endsWith(".Cu"));
  const hasTracks = form.tracks.length > 0;
  const hasVias = form.vias.length > 0;
  const lengthRow = (label: string, key: LengthKey) => (
    <>
      <span>{label}</span>
      <span>
        <input
          aria-label={label}
          value={texts[key]}
          placeholder={form[key] == null && before[key] == null ? "<...>" : undefined}
          onChange={(e) => {
            const t = e.target.value;
            setTexts((x) => ({ ...x, [key]: t }));
            const n = Number(t);
            setForm((f) => ({ ...f, [key]: t.trim() === "" || !Number.isFinite(n) ? null : Math.round(nmFrom(n, units)) }));
          }}
          style={{ width: 100 }}
        />{" "}
        {units}
      </span>
    </>
  );
  const ok = async () => {
    const out = trackViaCmds(board, before, form, (nm) => formatLength(nm, units));
    if ("error" in out) return setError(out.error);
    if (out.cmds.length === 0) return onClose();
    // `confirmShortingNets` (Cancel Changes: `commit.Revert()`, the dialog closes with nothing done), then `confirmPadChange`
    let cmds = out.cmds;
    for (;;) {
      const pre = await postTrackViaPreview(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
      if (!pre.ok) return setError(pre.message);
      if (!pre.ask) break;
      if (pre.ask.check === "ops_net_change_shorts") {
        if (!(await askConfirm({ title: "Confirmation", message: pre.ask.message, okLabel: "Apply Anyway", cancelLabel: "Cancel Changes", doNotShowKey: "dialog_track_via_properties.cpp:confirmShortingNets" }))) return onClose();
        cmds = withNetAnswers(cmds, { apply_anyway: true });
      } else {
        cmds = withNetAnswers(cmds, { change_pads: await askConfirm({ title: "Confirmation", message: pre.ask.message, okLabel: "Change Nets", cancelLabel: "Leave Nets Unchanged", doNotShowKey: "dialog_track_via_properties.cpp:confirmPadChange" }) });
        break;
      }
    }
    if (await api.cmdBatch(cmds)) onClose();
  };
  // `DIALOG_TRACK_VIA_PROPERTIES_BASE`'s title, whatever is selected (no `SetTitle`)
  const title = "Track & Via Properties";
  return (
    <div className="dialog-backdrop" onClick={onClose}>
      <div className="dialog" style={{ width: 440, maxHeight: "90vh", overflowY: "auto" }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Track & Via Properties">
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid">
            <span>Net</span>
            <span>
              <select aria-label="Net" value={form.net ?? "\u0000"} onChange={(e) => setForm((f) => ({ ...f, net: e.target.value === "\u0000" ? null : e.target.value }))}>
                {form.net == null && <option value={"\u0000"}>{"<...>"}</option>}
                <option value="">{"<no net>"}</option>
                {nets.map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </select>
            </span>
            <span>Locked</span>
            <span>
              <input
                type="checkbox"
                aria-label="Locked"
                checked={form.locked === true}
                ref={(el) => {
                  if (el) el.indeterminate = form.locked == null;
                }}
                onChange={(e) => setForm((f) => ({ ...f, locked: e.target.checked }))}
              />
            </span>
            {hasTracks && (
              <>
                {lengthRow("Start X", "startX")}
                {lengthRow("Start Y", "startY")}
                {lengthRow("End X", "endX")}
                {lengthRow("End Y", "endY")}
                {lengthRow("Track width", "width")}
                <span>Layer</span>
                <span>
                  <select aria-label="Layer" value={form.layer ?? "\u0000"} onChange={(e) => setForm((f) => ({ ...f, layer: e.target.value === "\u0000" ? null : e.target.value }))}>
                    {form.layer == null && <option value={"\u0000"}>{"<...>"}</option>}
                    {copper.map((l) => (
                      <option key={l} value={l}>
                        {layerDisplayName(layerSetup, l)}
                      </option>
                    ))}
                  </select>
                </span>
              </>
            )}
            {hasVias && (
              <>
                {lengthRow("Via X", "viaX")}
                {lengthRow("Via Y", "viaY")}
                {/* "Padstack mode:" and "Edit layer:" (`afterPadstackModeChanged`): the diameter shown is the edited layer's (`onEditLayerChanged`) */}
                <span>Padstack mode:</span>
                <span>
                  <select
                    aria-label="Padstack mode"
                    value={form.padstackMode}
                    onChange={(e) => {
                      const mode = e.target.value as PadstackMode;
                      // `PADSTACK::SetMode`: the layers the new mode names start from the front's diameter; the edit layer stays where it can
                      setForm((f) => ({ ...f, padstackMode: mode, editLayer: editLayerChoices(mode, copper).includes(f.editLayer) ? f.editLayer : "F.Cu" }));
                    }}
                  >
                    {PADSTACK_MODE_CHOICES.map(([k, label]) => (
                      <option key={k} value={k}>
                        {label}
                      </option>
                    ))}
                  </select>
                </span>
                <span>Edit layer:</span>
                <span>
                  <select aria-label="Edit layer" disabled={form.padstackMode === "normal"} value={form.editLayer} onChange={(e) => setForm((f) => ({ ...f, editLayer: e.target.value }))}>
                    {editLayerChoices(form.padstackMode, copper).map((l) => (
                      <option key={l} value={l}>
                        {form.padstackMode === "normal" ? "All layers" : l === "Inner" ? "Inner Layers" : layerDisplayName(layerSetup, l)}
                      </option>
                    ))}
                  </select>
                </span>
                {form.editLayer === "F.Cu" || form.padstackMode === "normal" ? (
                  lengthRow("Via diameter", "diameter")
                ) : (
                  <>
                    <span>Via diameter</span>
                    <span>
                      <NumberInput
                        label="Via diameter"
                        value={sizeOn(form, form.editLayer)}
                        scale={(v) => nmTo(v, units)}
                        unscale={(v) => Math.round(nmFrom(v, units))}
                        onChange={(nm) => setForm((f) => ({ ...f, layerSizes: { ...f.layerSizes, ...(nm != null ? { [f.padstackMode === "front_inner_back" && f.editLayer !== "B.Cu" ? "Inner" : f.editLayer]: nm } : {}) } }))}
                      />{" "}
                      {units}
                    </span>
                  </>
                )}
                {lengthRow("Via hole", "drill")}
                <span>Via type:</span>
                <span>
                  <select
                    aria-label="Via type"
                    value={form.viaType ?? "\u0000"}
                    onChange={(e) => {
                      const t = e.target.value === "\u0000" ? null : (e.target.value as ViaType);
                      // `onViaEdit`: a through via spans F.Cu..B.Cu, its layer choices disabled
                      setForm((f) => ({ ...f, viaType: t, ...(t === "through" ? { viaFrom: "F.Cu", viaTo: "B.Cu" } : {}) }));
                    }}
                  >
                    {form.viaType == null && <option value={"\u0000"}>{"<...>"}</option>}
                    {VIA_TYPE_CHOICES.map(([k, label]) => (
                      <option key={k} value={k}>
                        {label}
                      </option>
                    ))}
                  </select>
                </span>
                {(["viaFrom", "viaTo"] as const).map((key) => (
                  <span key={key} style={{ display: "contents" }}>
                    <span>{key === "viaFrom" ? "Start layer:" : "End layer:"}</span>
                    <span>
                      <select
                        aria-label={key === "viaFrom" ? "Start layer" : "End layer"}
                        disabled={form.viaType === "through"}
                        value={form[key] ?? "\u0000"}
                        onChange={(e) => setForm((f) => ({ ...f, [key]: e.target.value === "\u0000" ? null : e.target.value }))}
                      >
                        {form[key] == null && <option value={"\u0000"}>{"<...>"}</option>}
                        {copper.map((l) => (
                          <option key={l} value={l}>
                            {layerDisplayName(layerSetup, l)}
                          </option>
                        ))}
                      </select>
                    </span>
                  </span>
                ))}
                {viaLayerDepth(form.viaType, form.viaFrom, form.viaTo, copper) > 1 && (
                  <>
                    <span>Annular rings:</span>
                    <span>
                      <select aria-label="Annular rings" value={form.rings ?? "\u0000"} onChange={(e) => setForm((f) => ({ ...f, rings: e.target.value === "\u0000" ? null : (e.target.value as UnconnectedLayers) }))}>
                        {form.rings == null && <option value={"\u0000"}>{"<...>"}</option>}
                        {ANNULAR_RING_CHOICES.map(([k, label]) => (
                          <option key={k} value={k}>
                            {label}
                          </option>
                        ))}
                      </select>
                    </span>
                  </>
                )}
                {/* "Protection features:" (`m_protectionFeatures`): the IPC-4761 presets, a via whose settings are none of them "Custom", vias that differ "<...>" */}
                <span>Protection features:</span>
                <span>
                  <select
                    aria-label="Protection features"
                    title="Select which protection feature according to IPC-4761 the via should have."
                    value={form.protection ?? "\u0000"}
                    onChange={(e) => setForm((f) => ({ ...f, protection: e.target.value === "\u0000" ? null : (e.target.value as Ipc4761Preset | "custom") }))}
                  >
                    {IPC4761_PRESETS.map(([k, label]) => (
                      <option key={k} value={k}>
                        {label}
                      </option>
                    ))}
                    {form.protection === "custom" && <option value="custom">Custom</option>}
                    {form.protection == null && <option value={"\u0000"}>{"<...>"}</option>}
                  </select>
                </span>
                {/* "Backdrill:" (`m_backdrillChoice`) and its rows: the front backdrill is the secondary drill from F.Cu, the back one the tertiary from B.Cu */}
                <span>Backdrill:</span>
                <span>
                  <select aria-label="Backdrill" value={form.backdrill ?? "\u0000"} onChange={(e) => setForm((f) => ({ ...f, backdrill: e.target.value === "\u0000" ? null : (e.target.value as BackdrillChoice) }))}>
                    {form.backdrill == null && <option value={"\u0000"}>{"<...>"}</option>}
                    {BACKDRILL_CHOICES.map(([k, label]) => (
                      <option key={k} value={k}>
                        {label}
                      </option>
                    ))}
                  </select>
                </span>
                {(["Front", "Back"] as const).map((side) => {
                  const shown = side === "Front" ? form.backdrill === "top" || form.backdrill === "both" || form.backdrill == null : form.backdrill === "bottom" || form.backdrill === "both" || form.backdrill == null;
                  if (!shown) return null;
                  const sizeKey = side === "Front" ? "backdrillFrontSize" : "backdrillBackSize";
                  const layerKey = side === "Front" ? "backdrillFrontLayer" : "backdrillBackLayer";
                  return (
                    <span key={side} style={{ display: "contents" }}>
                      <span>{side} backdrill size:</span>
                      <span>
                        <NumberInput label={`${side} backdrill size`} value={form[sizeKey]} scale={(v) => nmTo(v, units)} unscale={(v) => Math.round(nmFrom(v, units))} onChange={(nm) => setForm((f) => ({ ...f, [sizeKey]: nm }))} /> {units}
                      </span>
                      <span>{side} backdrill must-cut:</span>
                      <span>
                        <select aria-label={`${side} backdrill must-cut`} title="The backdrill must pass through this layer" value={form[layerKey] ?? "\u0000"} onChange={(e) => setForm((f) => ({ ...f, [layerKey]: e.target.value === "\u0000" ? null : e.target.value }))}>
                          {form[layerKey] == null && <option value={"\u0000"}>{"<...>"}</option>}
                          <option value="">None</option>
                          {copper.map((l) => (
                            <option key={l} value={l}>
                              {layerDisplayName(layerSetup, l)}
                            </option>
                          ))}
                        </select>
                      </span>
                    </span>
                  );
                })}
                {/* "Hole Post-machining": Front and Back, each None, Countersink (size, angle) or Counterbore (size, depth) */}
                {(["Front", "Back"] as const).map((side) => {
                  const [mk, sk, s2k] = side === "Front" ? (["topPost", "topPostSize", "topPostSize2"] as const) : (["bottomPost", "bottomPostSize", "bottomPostSize2"] as const);
                  const mode = form[mk];
                  return (
                    <span key={`pm${side}`} style={{ display: "contents" }}>
                      <span>Post-machining {side.toLowerCase()}:</span>
                      <span>
                        <select aria-label={`Post-machining ${side.toLowerCase()}`} value={mode ?? "\u0000"} onChange={(e) => setForm((f) => ({ ...f, [mk]: e.target.value === "\u0000" ? null : (e.target.value as PostChoice) }))}>
                          {mode == null && <option value={"\u0000"}>{"<...>"}</option>}
                          {POST_MACHINE_CHOICES.map(([k, label]) => (
                            <option key={k} value={k}>
                              {label}
                            </option>
                          ))}
                        </select>
                      </span>
                      {mode != null && mode !== "none" && (
                        <>
                          <span>Size:</span>
                          <span>
                            <NumberInput label={`${side} post-machining size`} value={form[sk]} scale={(v) => nmTo(v, units)} unscale={(v) => Math.round(nmFrom(v, units))} onChange={(nm) => setForm((f) => ({ ...f, [sk]: nm }))} /> {units}
                          </span>
                          <span>{mode === "countersink" ? "Angle:" : "Depth:"}</span>
                          <span>
                            {mode === "countersink" ? (
                              <>
                                <NumberInput label={`${side} post-machining angle`} value={form[s2k]} onChange={(v) => setForm((f) => ({ ...f, [s2k]: v }))} /> deg
                              </>
                            ) : (
                              <>
                                <NumberInput label={`${side} post-machining depth`} value={form[s2k]} scale={(v) => nmTo(v, units)} unscale={(v) => Math.round(nmFrom(v, units))} onChange={(nm) => setForm((f) => ({ ...f, [s2k]: nm }))} /> {units}
                              </>
                            )}
                          </span>
                        </>
                      )}
                    </span>
                  );
                })}
              </>
            )}
          </div>
          {hasVias && <TeardropFields form={form.teardrops} units={units} what="via" onChange={(teardrops) => setForm((f) => ({ ...f, teardrops }))} />}
          {error && (
            <div role="alert" style={{ color: "var(--chrome-error, #c0392b)", fontSize: 12, marginTop: 6 }}>
              {error}
            </div>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={onClose}>Cancel</button>
          <button className="primary" onClick={() => void ok()}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
