// pcbnew.InteractiveEdit.properties ("E") on a selected footprint. KiCad's
// real dialog (pcb_properties_panel.cpp's sibling) has several tabs
// (General/Fields/3D Models/...) and lets you edit most of what it
// shows -- this session read pcb_properties_panel.cpp for the docked
// panel, not the modal dialog's exact layout, so this is a single-page
// view of the same fields the docked Properties panel already has real
// data for. Reference/Value/Footprint/MPN live on the *intent*-derived
// model (crates/model/src/lib.rs `Part`), not the editable `design.json`
// IR this app's `Cmd`s mutate, so they stay read-only (same reasoning
// BoardSetupDialog's Net Classes panel documents for the same model
// split) -- editable here: the refdes "Text Placement" side (new this
// session, `set_label_side`), plus Rotate/Flip/Move Exactly/Delete
// through the same commands the docked panel's own buttons use.
//
// Also the zone fields of the Clearances tab and of Pad Properties (`dialog_footprint_properties.cpp`,
// `dialog_pad_properties.cpp`): the footprint's "Zone connection" and "Clearance", and, for each pad number, its "Pad
// connection", "Relief gap", "Spoke width", "Spoke angle" and "Clearance" -- the overrides the zone filler reads
// (`kicad-port/padZone.ts`; `set_footprint_zone_connection` / `set_pad_zone_overrides`, each undoable).
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { formatXY } from "../state/units";
import type { LabelSide, PadConnection } from "../api/types";
import { FootprintZoneForm, NO_PAD_ZONE_FACTS, PadZoneForm, footprintZoneForm, padNumbers, padZoneForm, setFootprintZoneCmd, setPadZoneCmd } from "../kicad-port/padZone";
import { PAD_CONNECTION_OPTIONS } from "../kicad-port/padSettings";
import { AngleOverrideInput, OverrideInput } from "./footprint/PadPropertiesDialog";

const LABEL_SIDE_OPTIONS: { value: LabelSide; label: string }[] = [
  { value: "above", label: "Above" },
  { value: "below", label: "Below" },
  { value: "left", label: "Left" },
  { value: "right", label: "Right" },
];

export function FootprintPropertiesDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.footprintPropertiesOpen;
  const ref = [...state.selection][0];
  const p = ref ? api.partByRef(ref) : undefined;

  // The zone fields' own form state, read from the part whenever the dialog opens on a (different) footprint -- never on
  // every render, so a keystroke is not overwritten by the next poll of the board.
  const [fpZone, setFpZone] = useState<FootprintZoneForm>({ connection: null, clearance: null });
  const [padNum, setPadNum] = useState("");
  const [padZone, setPadZone] = useState<PadZoneForm>(NO_PAD_ZONE_FACTS);
  useEffect(() => {
    if (!open || !p) return;
    setFpZone(footprintZoneForm(p));
    const first = padNumbers(p)[0] ?? "";
    setPadNum(first);
    setPadZone(padZoneForm(p, first));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, p?.ref]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_FOOTPRINT_PROPERTIES_OPEN", open: false });
  if (!p) return null;
  const units = state.units;
  // `PADSTACK::DefaultThermalSpokeAngleForShape`: an X for a circle, a + for everything else this view can tell apart.
  const selectedPad = p.pads?.find((q) => q.num === padNum);
  const selectedPadIsCircle = !!selectedPad && selectedPad.round && selectedPad.w === selectedPad.h;

  const nets = [...new Set((p.pads ?? []).map((q) => q.net).filter((n): n is string => !!n))];

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 520 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Footprint Properties</span>
          <span>{p.ref}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid">
            <span>Reference</span>
            <span>{p.ref}</span>
            <span>Value</span>
            <span>{p.value ?? "–"}</span>
            <span>Footprint</span>
            <span>{p.package ?? "–"}</span>
            {p.mpn && (
              <>
                <span>MPN</span>
                <span>{p.mpn}</span>
              </>
            )}
            {p.placed && p.at && (
              <>
                <span>Position</span>
                <span>{formatXY(p.at[0], p.at[1], state.units)}</span>
                <span>Orientation</span>
                <span>{`${p.rot ?? 0}°`}</span>
                <span>Side</span>
                <span>{p.side === "bottom" ? "Bottom" : "Top"}</span>
                <span>Text placement</span>
                <span>
                  <select value={p.label ?? "above"} onChange={(e) => api.cmd({ op: "set_label_side", part: p.ref, side: e.target.value as LabelSide })}>
                    {LABEL_SIDE_OPTIONS.map((o) => (
                      <option key={o.value} value={o.value}>
                        {o.label}
                      </option>
                    ))}
                  </select>
                </span>
              </>
            )}
            <span>Pads</span>
            <span>{p.pads?.length ?? 0}</span>
            <span>Nets</span>
            <span>{nets.join(", ") || "–"}</span>
          </div>
          {p.placed && (
            <>
              <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>Zone connection (footprint)</p>
              <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
                <span>Pad connection</span>
                <select value={fpZone.connection ?? ""} onChange={(e) => setFpZone((f) => ({ ...f, connection: e.target.value === "" ? null : (e.target.value as PadConnection) }))} title="FOOTPRINT::GetLocalZoneConnection: how a zone connects to the pads of this footprint that do not set their own">
                  {PAD_CONNECTION_OPTIONS.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.value === "" ? "Inherited" : o.label}
                    </option>
                  ))}
                </select>
                <span>Clearance</span>
                <OverrideInput valueUm={fpZone.clearance} units={units} onChange={(v) => setFpZone((f) => ({ ...f, clearance: v }))} />
                <span />
                <span>
                  <button onClick={() => void api.cmd(setFootprintZoneCmd(p.ref, fpZone))}>Apply to footprint</button>
                </span>
              </div>

              {padNumbers(p).length > 0 && (
                <>
                  <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>Zone connection (pad)</p>
                  <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
                    <span>Pad</span>
                    <select
                      value={padNum}
                      onChange={(e) => {
                        setPadNum(e.target.value);
                        setPadZone(padZoneForm(p, e.target.value));
                      }}
                    >
                      {padNumbers(p).map((n) => (
                        <option key={n} value={n}>
                          {n}
                        </option>
                      ))}
                    </select>
                    <span>Pad connection</span>
                    <select value={padZone.connection ?? ""} onChange={(e) => setPadZone((f) => ({ ...f, connection: e.target.value === "" ? null : (e.target.value as PadConnection) }))} title="PAD::GetLocalZoneConnection: over the footprint's and the zone's own setting">
                      {PAD_CONNECTION_OPTIONS.map((o) => (
                        <option key={o.value} value={o.value}>
                          {o.label}
                        </option>
                      ))}
                    </select>
                    <span>Relief gap</span>
                    <OverrideInput valueUm={padZone.gap} units={units} onChange={(v) => setPadZone((f) => ({ ...f, gap: v }))} />
                    <span>Spoke width</span>
                    <OverrideInput valueUm={padZone.spokeWidth} units={units} onChange={(v) => setPadZone((f) => ({ ...f, spokeWidth: v }))} />
                    <span>Spoke angle</span>
                    <AngleOverrideInput valueMdeg={padZone.spokeAngleMdeg} defaultDeg={selectedPadIsCircle ? 45 : 90} onChange={(v) => setPadZone((f) => ({ ...f, spokeAngleMdeg: v }))} />
                    <span>Clearance</span>
                    <OverrideInput valueUm={padZone.clearance} units={units} onChange={(v) => setPadZone((f) => ({ ...f, clearance: v }))} />
                    <span />
                    <span>
                      <button disabled={padNum === ""} onClick={() => void api.cmd(setPadZoneCmd(p.ref, padNum, padZone))}>
                        Apply to pad {padNum}
                      </button>
                    </span>
                  </div>
                </>
              )}
            </>
          )}
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>
            Reference/Value/Footprint/MPN have no edit command yet (they come from the BOM, not the board) -- Position/Orientation/Side/Text placement do.
          </p>
        </div>
        <div className="dialog-footer">
          <button onClick={() => dispatch({ type: "SET_MOVE_EXACT_DIALOG_OPEN", open: true })}>Move Exactly...</button>
          <button onClick={() => api.rotateSelection(1)}>Rotate</button>
          <button onClick={() => api.flipSelection()}>Flip</button>
          <button onClick={() => api.ripSelection()}>Delete</button>
          <button className="primary" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
