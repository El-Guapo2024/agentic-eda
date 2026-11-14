// Port of pcbnew/dialogs/dialog_pad_properties.cpp (GAPS.md #8 step 3, UI-LEFT F6) -- shape/size (the nine shape choices, a trapezoid's delta, a chamfer, a custom
// pad's basic shapes: the rows are the board dialog's own, `PadShapeFields.tsx`), offset, the hole (Round / Oblong keeping its size, `OnDrillShapeSelected`), the
// layers (the type's presets, `m_rbCopperLayersSel`'s copper choice -- a plated hole's is its unconnected-layer mode -- and the technical layers' boxes:
// kicad-port/padLayers.ts), the fabrication property, the pad-to-die length and delay, the clearance/thermal overrides, and for a plated through-hole pad the
// padstack (`kicad-port/padStack.ts`: "Padstack mode", "Edit layer").
import { useEffect, useState } from "react";
import { fromDisplay, toDisplay, type CoordType } from "../../kicad-port/originTransforms";
import { useOriginTransforms } from "../../state/originTransforms";
import type { LibraryPad, PadConnection } from "../../api/types";
import { useFpApi, useFpState, useFpDispatch } from "../../state/footprintEditorStore";
import { useStudioState } from "../../state/store";
import { formatLength, nmFrom, nmTo } from "../../state/units";
import { padCheck } from "../../kicad-port/fpFields";
import { PAD_CONNECTION_OPTIONS, defaultSpokeAngleDeg, importPadSettings, settingsOf } from "../../kicad-port/padSettings";
import { chooseShape, customPadBoxes, libraryFieldsOf, shapeFormOfLibraryPad, type PadShapeChoice, type PadShapeForm } from "../../kicad-port/padShape";
import { FRONT, changeMode, checkStack, layerFormOf, padstackControlsShown, stackOf, stackStateOf, withLayerForm, type LayerForm, type PadStackState } from "../../kicad-port/padStack";
import { PAD_DLG_TYPES, padDlgTypeOf, padTypeHas, padTypeSelected, type PadDlgType } from "../../kicad-port/padType";
import { FIRST_LAST_PAD_NUMBER } from "../../kicad-port/padNumbering";
import { PadPrimitivePanel, PadShapeRows, PadStackRows } from "../PadShapeFields";
import { FAB_PROPERTY_CHOICES, PAD_TECH_LAYERS, copperChoiceEnabled, copperChoices, holeShapeSelected, layersOfForm, padLayerForm, padToDieOf, spreadLayers, withPadFab, type CopperMode } from "../../kicad-port/padLayers";

/** `pad.cpp`'s static layer masks (`PTHMask`/`SMDMask`/`ConnSMDMask`/`UnplatedHoleMask`) as quick presets beside the layer controls; a preset shows as the current one when the
 *  pad's set is the preset's (wildcards spread: `*.Mask` is `F.Mask B.Mask`, E7-F7). */
const LAYER_PRESETS: { label: string; type: PadDlgType; layers: string[] }[] = [
  { label: "SMD", type: "smd", layers: ["F.Cu", "F.Paste", "F.Mask"] },
  { label: "Connector (no paste)", type: "conn", layers: ["F.Cu", "F.Mask"] },
  { label: "THT", type: "pth", layers: ["*.Cu", "*.Mask"] },
  { label: "NPTH, mechanical", type: "npth", layers: ["*.Cu", "*.Mask"] },
];

const sameLayerSet = (a: readonly string[], b: readonly string[]): boolean => {
  const x = spreadLayers(a);
  const y = spreadLayers(b);
  return x.size === y.size && [...x].every((l) => y.has(l));
};

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
  const pad: LibraryPad | undefined = defaultMode ? { id: "", number: state.lastPadNumber ?? FIRST_LAST_PAD_NUMBER, at: { x: 0, y: 0 }, ...state.defaultPad } : id ? state.footprint?.pads.find((p) => p.id === id) : undefined;
  const close = () => (defaultMode ? dispatch({ type: "SET_DEFAULT_PAD_OPEN", open: false }) : dispatch({ type: "SET_PAD_PROPERTIES_ID", id: null }));

  const [form, setForm] = useState<LibraryPad | null>(null);
  /** The shape of the pad, with what goes with it (`kicad-port/padShape.ts`); laid back on the pad by OK. */
  const [shapeForm, setShapeForm] = useState<PadShapeForm | null>(null);
  /** The padstack (`kicad-port/padStack.ts`): the mode, the layer being edited and the shapes of the layers but the front, whose shape is `shapeForm` and offset `form.offset`. */
  const [stack, setStack] = useState<PadStackState | null>(null);
  /** `m_holeY`: the oblong hole's Y, which the control keeps while the hole is round. */
  const [holeY, setHoleY] = useState<number | null>(null);
  /** "Specify pad to die length" / "delay" and their values (nm, ps). */
  const [dieLength, setDieLength] = useState<number | null>(null);
  const [dieDelay, setDieDelay] = useState<number | null>(null);

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
      setHoleY(pad.drill_slot?.[1] ?? pad.drill ?? null);
      const die = padToDieOf(pad.kicad as { tokens?: string[] } | undefined);
      setDieLength(die.length);
      setDieDelay(die.delay);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, defaultMode]);

  // the position's coordinate fields' transforms (`m_posX.SetCoordType( ORIGIN_TRANSFORMS::ABS_X_COORD )`, UI-LEFT-3 F3-8)
  const xf = useOriginTransforms();
  if ((!defaultMode && !id) || !pad || !form || !shapeForm || !stack) return null;

  const set = <K extends keyof LibraryPad>(key: K, value: LibraryPad[K]) => setForm((f) => (f ? { ...f, [key]: value } : f));
  /** A coordinate field (`UNIT_BINDER` with a coordinate type): shown and read through the origin transforms. */
  const coord = (valueNm: number, type: CoordType, onChange: (nm: number) => void, width = 80) => (
    <span>
      <input type="number" step="any" value={nmTo(toDisplay(xf, valueNm, type), units)} onChange={(e) => Number.isFinite(Number(e.target.value)) && onChange(Math.round(fromDisplay(xf, nmFrom(Number(e.target.value), units), type)))} style={{ width }} /> {units}
    </span>
  );
  const len = (valueNm: number, onChange: (um: number) => void, width = 80) => (
    <span>
      <input type="number" step="any" value={nmTo(valueNm, units)} onChange={(e) => Number.isFinite(Number(e.target.value)) && onChange(Math.round(nmFrom(Number(e.target.value), units)))} style={{ width }} /> {units}
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
    const shape = libraryFieldsOf(shapeForm);
    // `padValuesOK` (`PAD::doCheckPad( .., aForPadProperties = true )`): an error keeps the dialog open, the warnings are shown ("Warning: ..") and the pad is applied.
    const keep = form.kicad as { solder_mask_margin?: number; solder_paste_margin?: number; solder_paste_margin_ratio?: number } | undefined;
    const checked = padCheck(
      { kind: form.kind, shape: shape.shape, size: shape.size, drill: form.drill, slot: form.drill_slot, ratio: shape.roundrect_ratio, pasteRatio: keep?.solder_paste_margin_ratio ?? null, offset: [form.offset.x, form.offset.y], clearance: form.clearance_override, maskMargin: keep?.solder_mask_margin ?? null, pasteMargin: keep?.solder_paste_margin ?? null, ...customPadBoxes(shapeForm, shape.size) },
      (nm) => formatLength(nm, units),
      units
    );
    const message = checked.errors[0] ?? (stackShown ? checkStack(stack) : null);
    if (message) {
      dispatch({ type: "TOAST", message, kind: "error" });
      return;
    }
    if (checked.warnings.length > 0) dispatch({ type: "TOAST", message: checked.warnings.join("\n"), kind: "info" });
    // `transferDataToPad`: an NPTH (and an aperture) pad has no number, an SMD or connector pad no hole.
    const has = padTypeHas(padDlgTypeOf(form));
    const edited: LibraryPad = { ...form, ...shape, number: has.connection ? form.number : "", ...(has.hole ? {} : { drill: null, drill_slot: null }) };
    // the fabrication property and the pad-to-die length and delay (`SetProperty`, `SetPadToDieLength`, `SetPadToDieDelay`): kept forms of the pad, none for a pad without a
    // connection (`OnUpdateUI` turns the boxes off)
    const fab = (form.kicad ?? {}) as { property?: string | null; tokens?: string[] };
    edited.kicad = withPadFab(fab, fab.property ?? null, has.connection ? dieLength : null, has.connection ? dieDelay : null) as LibraryPad["kicad"];
    // The padstack travels with the pad (`PAD::SetPadstack`); a pad that has none sends none.
    const stackNow = stackOf(stack);
    edited.stack = Object.keys(stackNow).length > 0 ? stackNow : undefined;
    // `TransferDataFromWindow`: `transferDataToPad( m_masterPad )` and `padTool->SetLastPadNumber( m_masterPad->GetNumber() )` whichever pad the dialog is on --
    // the Add Pad tool's next pad is the one just edited (the circle / SMD fix-ups of `ImportSettingsFrom` included); then, on a pad, the pad takes it.
    const master = settingsOf(importPadSettings(edited, edited));
    if (defaultMode) {
      dispatch({ type: "SET_DEFAULT_PAD", pad: master });
      dispatch({ type: "SET_LAST_PAD_NUMBER", number: edited.number });
      close();
      return;
    }
    if (id && (await api.editPad(id, edited))) {
      dispatch({ type: "SET_DEFAULT_PAD", pad: master });
      dispatch({ type: "SET_LAST_PAD_NUMBER", number: edited.number });
      close();
    }
  };

  /** `onPadShapeSelection`: the choice, with the defaults it sets up and the spoke angle that follows a circle. */
  const chooseShapeOf = (choice: PadShapeChoice) => {
    const was = form.thermal_spoke_angle_mdeg ?? null;
    const r = chooseShape(shapeForm, choice, was === null ? 90 : was / 1000);
    setShapeForm(r.form);
    if (was !== null) set("thermal_spoke_angle_mdeg", Math.round(r.spokeAngleDeg * 1000));
  };

  // `OnUpdateUI`: the hole and the number are enabled by the type.
  const type = padDlgTypeOf(form);
  const has = padTypeHas(type);
  const hasDrill = has.hole;
  /** `PadTypeSelected` on the type chosen (`current`: the pad the dialog is on, none for the default pad). */
  const chooseType = (next: PadDlgType, layers?: string[]) =>
    setForm((f) => {
      if (!f) return f;
      const r = padTypeSelected(f, next, defaultMode ? null : (pad ?? null));
      return layers ? { ...r, layers: [...layers] } : r;
    });
  const isSlot = form.drill_slot != null;
  // The layer controls show the pad's set (`updatePadLayersList`) and write it back as `transferDataToPad` does: a plated hole's copper choice is its unconnected-layer mode.
  const unconnected: CopperMode = stack.unconnected === "start_end_only" ? "remove_except_start_and_end" : stack.unconnected;
  const layerForm = padLayerForm(type, form.layers, unconnected);
  const setLayerForm = (next: typeof layerForm) => {
    const r = layersOfForm(type, next);
    setForm((f) => (f ? { ...f, layers: r.layers } : f));
    if (type === "pth") setStack({ ...stack, unconnected: r.mode });
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 480 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{defaultMode ? "Default Pad Properties for Add Pad Tool" : "Pad Properties"}</span>
          <span>{defaultMode ? "" : pad.number || "(no number)"}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "72vh", overflowY: "auto" }}>
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
            <span>Pad number</span>
            <input value={form.number} disabled={!has.connection} onChange={(e) => set("number", e.target.value)} style={{ width: 80 }} data-testid="fp-pad-number" />
            <span>Pad type</span>
            <select value={type} onChange={(e) => chooseType(e.target.value as PadDlgType)} data-testid="fp-pad-type">
              {PAD_DLG_TYPES.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>

            {!defaultMode && (
              <>
                <span>Position</span>
                <span>
                  {/* `m_posX.SetCoordType( ABS_X_COORD )`: through the Footprint Editor's Origins & Axes (UI-LEFT-3 F3-8) */}
                  {coord(form.at.x, "abs_x", (x) => set("at", { ...form.at, x }))} {coord(form.at.y, "abs_y", (y) => set("at", { ...form.at, y }))}
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
                <button key={p.label} className={type === p.type && sameLayerSet(form.layers, p.layers) ? "primary" : undefined} style={{ marginRight: 6, marginBottom: 4 }} onClick={() => chooseType(p.type, p.layers)}>
                  {p.label}
                </button>
              ))}
            </span>
            {/* `m_rbCopperLayersSel` (UpdateLayersDropdown): a plated hole's choice is its unconnected-layer mode; the technical layers' boxes below */}
            <span>Copper layers:</span>
            <select value={layerForm.copper} disabled={!copperChoiceEnabled(type)} onChange={(e) => setLayerForm({ ...layerForm, copper: Number(e.target.value) })} data-testid="pad-copper-layers">
              {copperChoices(type, boardLayers[0] ?? "F.Cu", boardLayers[boardLayers.length - 1] ?? "B.Cu").map((c, i) => (
                <option key={c} value={i}>
                  {c}
                </option>
              ))}
            </select>
            <span>Technical layers:</span>
            <span style={{ display: "grid", gridTemplateColumns: "repeat(2, auto)", gap: "2px 12px", justifyContent: "start" }} data-testid="pad-tech-layers">
              {PAD_TECH_LAYERS.map((l) => (
                <label key={l}>
                  <input
                    type="checkbox"
                    checked={layerForm.tech.has(l)}
                    data-testid={`pad-layer-${l}`}
                    onChange={(e) => {
                      const tech = new Set(layerForm.tech);
                      if (e.target.checked) tech.add(l);
                      else tech.delete(l);
                      setLayerForm({ ...layerForm, tech });
                    }}
                  />{" "}
                  {l}
                </label>
              ))}
            </span>

            {hasDrill && (
              <>
                {/* "Hole shape:" Round / Oblong (`m_holeShapeCtrl`): a change keeps the size (`OnDrillShapeSelected`, E7-F6); "Diameter:" or "Hole size X:" and "Y:" (`updateHoleControls`) */}
                <span>Hole shape:</span>
                <select
                  value={isSlot ? "oblong" : "round"}
                  data-testid="pad-hole-shape"
                  onChange={(e) => {
                    const shape = e.target.value as "round" | "oblong";
                    if (form.drill_slot) setHoleY(form.drill_slot[1]);
                    setForm((f) => (f ? { ...f, ...holeShapeSelected(f.drill, f.drill_slot ?? null, shape, holeY) } : f));
                  }}
                >
                  <option value="round">Round</option>
                  <option value="oblong">Oblong</option>
                </select>
                {!isSlot && (
                  <>
                    <span>Diameter:</span>
                    <span>{len(form.drill ?? 0, (d) => set("drill", d))}</span>
                  </>
                )}
                {isSlot && (
                  <>
                    <span>Hole size X:</span>
                    <span>
                      {len(form.drill_slot?.[0] ?? 0, (w) => set("drill_slot", [w, form.drill_slot?.[1] ?? w]))} Y: {len(form.drill_slot?.[1] ?? 0, (h) => set("drill_slot", [form.drill_slot?.[0] ?? h, h]))}
                    </span>
                  </>
                )}
              </>
            )}
            {/* "Fabrication property:" (`m_choiceFabProperty`) and the pad-to-die length and delay (`m_padToDieOpt`, `m_padToDieDelayOpt`: a pad without a connection has none) */}
            <span>Fabrication property:</span>
            <select
              value={((form.kicad ?? {}) as { property?: string | null }).property ?? ""}
              data-testid="pad-fab-property"
              onChange={(e) => setForm((f) => (f ? { ...f, kicad: { ...((f.kicad ?? {}) as object), property: e.target.value || null } as LibraryPad["kicad"] } : f))}
            >
              {FAB_PROPERTY_CHOICES.map((c) => (
                <option key={c.label} value={c.token ?? ""}>
                  {c.label}
                </option>
              ))}
            </select>
            <span />
            <span>
              <label>
                <input type="checkbox" disabled={!has.connection} checked={has.connection && dieLength !== null} data-testid="pad-to-die-on" onChange={(e) => setDieLength(e.target.checked ? 0 : null)} /> Specify pad to die length
              </label>{" "}
              {has.connection && dieLength !== null && len(dieLength, (v) => setDieLength(v))}
            </span>
            <span />
            <span>
              <label>
                <input type="checkbox" disabled={!has.connection} checked={has.connection && dieDelay !== null} data-testid="pad-to-die-delay-on" onChange={(e) => setDieDelay(e.target.checked ? 0 : null)} /> Specify pad to die delay
              </label>{" "}
              {has.connection && dieDelay !== null && (
                <span>
                  <input type="number" step="any" value={dieDelay} data-testid="pad-to-die-delay" onChange={(e) => Number.isFinite(Number(e.target.value)) && setDieDelay(Number(e.target.value))} style={{ width: 80 }} /> ps
                </span>
              )}
            </span>
          </div>

          <PadPrimitivePanel form={active.s} onForm={(next) => setActive({ ...active, s: next })} units={units} />

          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>Clearance overrides and zone connection (blank = inherit)</p>
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
            <span>Clearance</span>
            <OverrideInput valueNm={form.clearance_override} units={units} onChange={(v) => set("clearance_override", v)} />
            <span>Pad connection</span>
            <select value={form.zone_connection ?? ""} onChange={(e) => set("zone_connection", e.target.value === "" ? null : (e.target.value as PadConnection))} title="PAD::GetLocalZoneConnection: how a copper zone connects to this pad, over its footprint's and the zone's own setting">
              {PAD_CONNECTION_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
            <span>Thermal relief gap</span>
            <OverrideInput valueNm={form.thermal_gap_override} units={units} onChange={(v) => set("thermal_gap_override", v)} />
            <span>Thermal spoke width</span>
            <OverrideInput valueNm={form.thermal_spoke_width_override} units={units} onChange={(v) => set("thermal_spoke_width_override", v)} />
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
export function OverrideInput({ valueNm, units, onChange, testid }: { valueNm: number | null; units: Parameters<typeof nmTo>[1]; onChange: (v: number | null) => void; testid?: string }) {
  const on = valueNm != null;
  return (
    <span>
      <label style={{ marginRight: 8 }}>
        <input type="checkbox" checked={on} onChange={(e) => onChange(e.target.checked ? 0 : null)} data-testid={testid ? `${testid}-on` : undefined} /> override
      </label>
      {on && (
        <input
          type="number"
          step="any"
          value={nmTo(valueNm, units)}
          onChange={(e) => Number.isFinite(Number(e.target.value)) && onChange(Math.round(nmFrom(Number(e.target.value), units)))}
          style={{ width: 80 }}
          data-testid={testid}
        />
      )}
      {on && ` ${units}`}
    </span>
  );
}
