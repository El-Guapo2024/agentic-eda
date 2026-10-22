// pcbnew/dialogs/dialog_pad_properties.cpp -- "Pad Properties" of a pad on the board (`pcbnew.InteractiveEdit.properties` on a selected pad, or a double-click):
// the pad type, the shape (KiCad's nine choices: circular, oval, rectangular, trapezoidal, rounded rectangle, chamfered rectangle, chamfered with other corners
// rounded, custom with a circular or a rectangular base), the size, the shape's offset from the pad's position, the pad's own rotation, the shape's own fields
// (a trapezoid's delta and axis, the corner size, the chamfer's size and corners, a custom pad's zone knockout and its list of basic shapes), the hole (round or
// oblong), the solder mask margin, solder paste margin and paste ratio (the "Clearance Overrides and Settings" panel), and the zone filler's overrides of the
// same panel: the pad's local clearance, "Pad connection", "Relief gap", "Spoke width" and "Spoke angle" (`kicad-port/padZone.ts`).
//
// The shape rules are `kicad-port/padShape.ts`'s (what choosing a shape sets up, the corner and chamfer limits, the trapezoid's clamping, the basic shapes of a
// custom pad: add, edit, delete and duplicate a line, an arc, a circle, a rectangle, a polygon or a curve).
//
// OK sends ONE batch, so one undo step (`Edit Pad Properties`): an `edit_board_pad` -- the pad's stored overrides with the ones that changed laid over them; an
// override that is switched off is taken away, so the library's value comes back -- and, if a zone field changed, a `set_pad_zone_overrides` (a pad has one
// clearance, and it is the zone overlay's). The zone fields are those of every pad that carries this pad's number, as the zone overlay keeps them. The checks are
// `padValuesOK`'s (`PAD::CheckPad`), in its words.
//
// The padstack (`kicad-port/padStack.ts`): for a plated through-hole pad "Padstack mode" (Normal, Front/Inner/Back, Custom) and "Edit layer" pick the copper layer the shape
// rows and the shape offset show, "Copper layers" the layers that keep their copper when nothing is connected there; one OK sends the whole stack with the edit.
//
// Not ported: the fabrication property, the pad number and pin function, the pad-to-die length and the layer checkboxes (the type picks the layers), and backdrill and
// post-machining. "Push Pad Properties to other pads" is the footprint editor's.
import { useEffect, useState } from "react";
import type { Cmd, PadConnection, PadEditCmd } from "../api/types";
import { checkPadValues, padEditWith } from "../kicad-port/fpFields";
import { chooseShape, checkShape, hasSizeY, shapeFormOf, shapeNameOf, shapePatch, sizeOf, type PadShapeChoice, type PadShapeForm } from "../kicad-port/padShape";
import { FRONT, changeMode, checkStack, layerFormOf, padstackControlsShown, stackDiffers, stackOf, stackStateOf, withLayerForm, type LayerForm, type PadStackState } from "../kicad-port/padStack";
import { PAD_CONNECTION_OPTIONS } from "../kicad-port/padSettings";
import { footprintZoneForm, padZoneForm, zoneCmds, type PadZoneForm } from "../kicad-port/padZone";
import { padById } from "../kicad-port/pcbItems";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { formatLength, umFrom, umTo } from "../state/units";
import { AngleOverrideInput, OverrideInput } from "./footprint/PadPropertiesDialog";
import { CopperLayersRow, PadPrimitivePanel, PadShapeRows, PadStackRows } from "./PadShapeFields";

type Kind = "smd" | "through_hole" | "non_plated_hole";

interface Form {
  kind: Kind;
  /** The shape and what goes with it: the choice, the size, the corner ratio, a trapezoid's delta, the chamfer, a custom pad's knockout and basic shapes. */
  s: PadShapeForm;
  ox: number;
  oy: number;
  rot: number;
  hole: "round" | "oblong";
  drill: number;
  slotW: number;
  slotH: number;
  /** The zone filler's overrides of this pad: its local clearance, zone connection, relief gap, spoke width and angle. */
  zone: PadZoneForm;
  mask: number | null;
  paste: number | null;
  pasteRatio: number | null;
  /** The padstack: the mode, the edit layer and the shapes of the layers but the front, whose shape is `s` and offset `ox`, `oy`. */
  stack: PadStackState;
}

const KINDS: { value: Kind; label: string }[] = [
  { value: "through_hole", label: "Through-hole" },
  { value: "smd", label: "SMD" },
  { value: "non_plated_hole", label: "NPTH, mechanical" },
];

export function BoardPadPropertiesDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const units = state.units;
  const id = state.boardPadPropertiesId;
  const hit = id && state.board ? padById(state.board, id) : null;
  const [form, setForm] = useState<Form | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Fill the form when the dialog opens on a pad -- not on every poll.
  useEffect(() => {
    if (!hit) return;
    const { pad, part } = hit;
    const size = pad.size ?? [pad.w, pad.h];
    const s = shapeFormOf({ ...pad, size });
    const [ox, oy] = [pad.offset?.[0] ?? 0, pad.offset?.[1] ?? 0];
    setForm({
      kind: pad.kind ?? (pad.th ? "through_hole" : "smd"),
      s,
      ox,
      oy,
      stack: stackStateOf(pad.stack, { s, ox, oy, tokens: [] }),
      rot: pad.rot ?? 0,
      hole: pad.slot ? "oblong" : "round",
      drill: pad.drill ?? 800,
      slotW: pad.slot?.[0] ?? 600,
      slotH: pad.slot?.[1] ?? 1000,
      zone: padZoneForm(part, pad.num),
      mask: pad.mask_margin ?? null,
      paste: pad.paste_margin ?? null,
      pasteRatio: pad.paste_ratio ?? null,
    });
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  if (!id || !hit || !form) return null;
  const { part, pad } = hit;
  const close = () => dispatch({ type: "SET_BOARD_PAD_PROPERTIES_ID", id: null });
  const set = <K extends keyof Form>(key: K, value: Form[K]) => setForm((f) => (f ? { ...f, [key]: value } : f));
  const setZone = (patch: Partial<PadZoneForm>) => setForm((f) => (f ? { ...f, zone: { ...f.zone, ...patch } } : f));
  const fmt = (um: number) => formatLength(um, units);

  const len = (um: number, onChange: (um: number) => void, width = 74, testid?: string) => (
    <input
      type="number"
      step="any"
      value={Number(umTo(um, units).toFixed(4))}
      data-testid={testid}
      onChange={(e) => Number.isFinite(Number(e.target.value)) && e.target.value !== "" && onChange(Math.round(umFrom(Number(e.target.value), units)))}
      style={{ width }}
    />
  );
  const s = form.s;
  // The padstack: the front's shape is the form's own; the layer being edited is the one the shape rows and the offset show.
  const boardLayers = (state.board?.layers ?? ["F.Cu", "B.Cu"]).filter((l) => l.endsWith(".Cu"));
  const stackShown = padstackControlsShown(form.kind);
  const frontForm: LayerForm = { s: form.s, ox: form.ox, oy: form.oy, tokens: [] };
  const editLayer = stackShown ? form.stack.editLayer : FRONT;
  const active = layerFormOf(form.stack, frontForm, editLayer);
  /** The edited layer's form replaced: the front's is the form's own shape and offset, any other layer's is the stack's. */
  const setActive = (next: LayerForm) =>
    setForm((f) => {
      if (!f) return f;
      const r = withLayerForm(f.stack, { s: f.s, ox: f.ox, oy: f.oy, tokens: [] }, editLayer, next);
      return { ...f, stack: r.state, s: r.front.s, ox: r.front.ox, oy: r.front.oy };
    });
  const hasHole = form.kind !== "smd";
  // The zone overlay addresses a pad by its number: a pad with none (a mounting hole) has no zone fields, and a number several pads share is one set of fields.
  const numbered = pad.num !== "";
  const sharesNumber = numbered && (part.pads ?? []).filter((q) => q.num === pad.num).length > 1;

  /** `onPadShapeSelection`: the choice, with the defaults it sets up and the spoke angle that follows a circle. */
  const chooseShapeOf = (choice: PadShapeChoice) =>
    setForm((f) => {
      if (!f) return f;
      const was = f.zone.spokeAngleMdeg;
      const r = chooseShape(f.s, choice, was === null ? (hasSizeY(f.s.choice) ? 90 : 45) : was / 1000);
      // An angle the user never set stays unset (the shape's default); one that was set follows the dialog's rule.
      return { ...f, s: r.form, zone: was === null ? f.zone : { ...f.zone, spokeAngleMdeg: Math.round(r.spokeAngleDeg * 1000) } };
    });

  const submit = async () => {
    setError(null);
    const size = sizeOf(s);
    const message =
      checkPadValues(
        { kind: form.kind, shape: shapeNameOf(s.choice), size, drill: form.hole === "round" ? form.drill : null, slot: form.hole === "oblong" ? [form.slotW, form.slotH] : null, ratio: s.choice === "round_rect" ? s.ratio : null, pasteRatio: form.pasteRatio },
        fmt
      ) ?? checkShape(s) ?? (stackShown ? checkStack(form.stack) : null);
    if (message) return setError(message);

    // What differs from the pad as it is becomes a key of the pad's edit; an override that was switched off becomes `undefined` and goes.
    const was = { kind: pad.kind ?? (pad.th ? "through_hole" : "smd") };
    const patch: Partial<PadEditCmd> = { ...shapePatch(s, { ...pad, size: pad.size ?? [pad.w, pad.h] }) };
    if (form.kind !== was.kind) patch.kind = form.kind;
    if (hasHole) {
      if (form.hole === "round" && (pad.slot || pad.drill !== form.drill)) patch.drill = form.drill;
      if (form.hole === "oblong" && (!pad.slot || pad.slot[0] !== form.slotW || pad.slot[1] !== form.slotH)) patch.drill_slot = [form.slotW, form.slotH];
    }
    const offset = pad.offset ?? [0, 0];
    if (form.ox !== offset[0] || form.oy !== offset[1]) patch.offset = { x: form.ox, y: form.oy };
    if (Math.abs(form.rot - (pad.rot ?? 0)) > 1e-9) patch.rot = Math.round((((form.rot % 360) + 360) % 360) * 1000);
    const override = <T,>(key: keyof PadEditCmd, now: T | null, before: T | null | undefined) => {
      if (now === (before ?? null)) return;
      (patch as Record<string, unknown>)[key] = now === null ? undefined : now;
    };
    override("solder_mask_margin", form.mask, pad.mask_margin);
    override("solder_paste_margin", form.paste, pad.paste_margin);
    override("solder_paste_margin_ratio", form.pasteRatio, pad.paste_ratio);
    // An offset of nothing is no offset.
    if (patch.offset && patch.offset.x === 0 && patch.offset.y === 0) patch.offset = undefined;
    // The padstack, replaced whole when it is not what the pad has (`PAD::SetPadstack`).
    const stackNow = stackOf(form.stack);
    if (form.kind === "through_hole" && stackDiffers(stackNow, pad.stack)) patch.stack = stackNow;

    const cmds: Cmd[] = [];
    if (Object.keys(patch).length > 0) cmds.push({ op: "edit_board_pad", part: part.ref, edit: padEditWith(part, pad, patch) });
    // The zone filler's overrides (a pad has one clearance, and it is the zone overlay's): those of every pad that carries this pad's number.
    if (numbered) cmds.push(...zoneCmds(part, footprintZoneForm(part), { [pad.num]: form.zone }));
    if (cmds.length === 0) return close();
    setBusy(true);
    try {
      if (await api.cmdBatch(cmds)) close();
    } finally {
      setBusy(false);
    }
  };

  const override = (label: string, value: number | null, onChange: (v: number | null) => void, testid: string, ratio = false) => (
    <>
      <span>{label}</span>
      <span>
        <label style={{ marginRight: 8 }}>
          <input type="checkbox" checked={value !== null} onChange={(e) => onChange(e.target.checked ? 0 : null)} data-testid={`${testid}-on`} /> override
        </label>
        {value !== null &&
          (ratio ? (
            <>
              <input type="number" step="any" value={Number((value * 100).toFixed(3))} onChange={(e) => Number.isFinite(Number(e.target.value)) && onChange(Number(e.target.value) / 100)} style={{ width: 74 }} data-testid={testid} /> %
            </>
          ) : (
            <>
              {len(value, onChange, 74, testid)} {units}
            </>
          ))}
      </span>
    </>
  );

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 560 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Pad Properties</span>
          <span>
            {part.ref} pad {pad.num || "(no number)"}
          </span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "74vh", overflowY: "auto" }} data-testid="board-pad-properties">
          <div className="kv-grid" style={{ gridTemplateColumns: "150px 1fr" }}>
            <span>Pad number</span>
            <span>{pad.num || "–"}</span>
            <span>Net</span>
            <span>{pad.net ?? "–"}</span>
            <span>Pad type</span>
            <select value={form.kind} onChange={(e) => set("kind", e.target.value as Kind)} data-testid="pad-kind">
              {KINDS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
            <span>Position</span>
            <span style={{ fontSize: 11 }}>{`${fmt(pad.px ?? pad.x)}, ${fmt(pad.py ?? pad.y)}`} (on the board)</span>
            {stackShown && (
              <PadStackRows
                state={form.stack}
                boardLayers={boardLayers}
                onMode={(mode) => setForm((f) => (f ? { ...f, stack: changeMode(f.stack, mode, { s: f.s, ox: f.ox, oy: f.oy, tokens: [] }, boardLayers) } : f))}
                onEditLayer={(key) => setForm((f) => (f ? { ...f, stack: { ...f.stack, editLayer: key } } : f))}
              />
            )}
            <PadShapeRows form={active.s} onForm={(next) => setActive({ ...active, s: next })} units={units} onChoose={editLayer === FRONT ? chooseShapeOf : undefined} />
            <span>Shape offset</span>
            <span>
              {len(active.ox, (v) => setActive({ ...active, ox: v }), 74, "pad-offset-x")} {len(active.oy, (v) => setActive({ ...active, oy: v }), 74, "pad-offset-y")} {units}
            </span>
            <span>Rotation (clockwise)</span>
            <span>
              <input type="number" step="any" value={form.rot} onChange={(e) => Number.isFinite(Number(e.target.value)) && set("rot", Number(e.target.value))} style={{ width: 74 }} data-testid="pad-rot" />°
            </span>
            {hasHole && (
              <>
                <span>Hole shape</span>
                <select value={form.hole} onChange={(e) => set("hole", e.target.value as "round" | "oblong")} data-testid="pad-hole-shape">
                  <option value="round">Round</option>
                  <option value="oblong">Oblong</option>
                </select>
                {form.hole === "round" ? (
                  <>
                    <span>Hole diameter</span>
                    <span>
                      {len(form.drill, (d) => set("drill", d), 74, "pad-drill")} {units}
                    </span>
                  </>
                ) : (
                  <>
                    <span>Hole size</span>
                    <span>
                      {len(form.slotW, (v) => set("slotW", v), 74, "pad-slot-w")} × {len(form.slotH, (v) => set("slotH", v), 74, "pad-slot-h")} {units}
                    </span>
                  </>
                )}
              </>
            )}
          </div>
          <PadPrimitivePanel form={active.s} onForm={(next) => setActive({ ...active, s: next })} units={units} />
          {stackShown && (
            <div className="kv-grid" style={{ gridTemplateColumns: "150px 1fr", marginTop: 8 }}>
              <CopperLayersRow value={form.stack.unconnected} boardLayers={boardLayers} onChange={(v) => setForm((f) => (f ? { ...f, stack: { ...f.stack, unconnected: v } } : f))} />
            </div>
          )}
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>Clearance overrides and settings (blank = the board&apos;s)</p>
          <div className="kv-grid" style={{ gridTemplateColumns: "150px 1fr" }}>
            {numbered && (
              <>
                <span>Pad clearance</span>
                <OverrideInput testid="pad-clearance" valueUm={form.zone.clearance} units={units} onChange={(v) => setZone({ clearance: v })} />
              </>
            )}
            {override("Solder mask margin", form.mask, (v) => set("mask", v), "pad-mask")}
            {override("Solder paste margin", form.paste, (v) => set("paste", v), "pad-paste")}
            {override("Solder paste ratio", form.pasteRatio, (v) => set("pasteRatio", v), "pad-paste-ratio", true)}
          </div>
          {numbered && (
            <>
              <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>Zone connection</p>
              <div className="kv-grid" style={{ gridTemplateColumns: "150px 1fr" }} data-testid="pad-zone">
                <span>Pad connection</span>
                <select
                  value={form.zone.connection ?? ""}
                  onChange={(e) => setZone({ connection: e.target.value === "" ? null : (e.target.value as PadConnection) })}
                  title="PAD::GetLocalZoneConnection: over the footprint's and the zone's own setting"
                  data-testid="pad-zone-connection"
                >
                  {PAD_CONNECTION_OPTIONS.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
                <span>Relief gap</span>
                <OverrideInput testid="pad-zone-gap" valueUm={form.zone.gap} units={units} onChange={(v) => setZone({ gap: v })} />
                <span>Spoke width</span>
                <OverrideInput testid="pad-zone-spoke-width" valueUm={form.zone.spokeWidth} units={units} onChange={(v) => setZone({ spokeWidth: v })} />
                <span>Spoke angle</span>
                <AngleOverrideInput testid="pad-zone-spoke-angle" valueMdeg={form.zone.spokeAngleMdeg} defaultDeg={s.choice === "circle" || s.choice === "custom_circle" ? 45 : 90} onChange={(v) => setZone({ spokeAngleMdeg: v })} />
              </div>
              {sharesNumber && <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "4px 0 0" }}>The clearance and the zone fields are those of every pad of this footprint numbered {pad.num}.</p>}
            </>
          )}
          {error && (
            <p style={{ color: "var(--chrome-danger, #e5534b)", margin: "10px 0 0", fontSize: 12 }} data-testid="board-pad-properties-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={() => void submit()} disabled={busy} data-testid="board-pad-properties-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
