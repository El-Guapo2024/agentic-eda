// pcbnew/dialogs/dialog_pad_properties.cpp -- "Pad Properties" of a pad on the board (`pcbnew.InteractiveEdit.properties` on a selected pad, or a double-click):
// the pad type, the shape, the size, the shape's offset from the pad's position, the pad's own rotation, the corner radius of a rounded rectangle, the hole
// (round or oblong) and the local clearance, solder mask margin, solder paste margin and paste ratio (the "Clearance Overrides and Settings" panel).
//
// OK sends one `edit_board_pad`: the pad's stored overrides with the ones that changed laid over them -- an override that is switched off is taken away, so the
// library's value comes back -- and so is one undo step (`Edit Pad Properties`). The checks are `padValuesOK`'s (`PAD::CheckPad`), in its words.
//
// Not ported: the trapezoid, chamfered and custom shapes, padstack modes (front/inner/back differing), the fabrication property, the pad number and pin
// function, the pad-to-die length and the layer checkboxes (the type picks the layers), backdrill and post-machining, and the zone connection and thermal relief
// overrides (the zone fill's). "Push Pad Properties to other pads" is the footprint editor's.
import { useEffect, useState } from "react";
import type { Cmd, PadEditCmd } from "../api/types";
import { checkPadValues, padEditWith } from "../kicad-port/fpFields";
import { padById } from "../kicad-port/pcbItems";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { formatLength, umFrom, umTo } from "../state/units";

type Shape = "circle" | "rect" | "oval" | "round_rect";
type Kind = "smd" | "through_hole" | "non_plated_hole";

interface Form {
  kind: Kind;
  shape: Shape;
  w: number;
  h: number;
  ox: number;
  oy: number;
  rot: number;
  ratio: number;
  hole: "round" | "oblong";
  drill: number;
  slotW: number;
  slotH: number;
  clearance: number | null;
  mask: number | null;
  paste: number | null;
  pasteRatio: number | null;
}

const SHAPES: { value: Shape; label: string }[] = [
  { value: "circle", label: "Circle" },
  { value: "rect", label: "Rectangle" },
  { value: "oval", label: "Oval" },
  { value: "round_rect", label: "Rounded rectangle" },
];

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
    const { pad } = hit;
    const size = pad.size ?? [pad.w, pad.h];
    setForm({
      kind: pad.kind ?? (pad.th ? "through_hole" : "smd"),
      shape: pad.shape ?? (pad.round ? (Math.abs(pad.w - pad.h) < 1 ? "circle" : "oval") : "rect"),
      w: size[0],
      h: size[1],
      ox: pad.offset?.[0] ?? 0,
      oy: pad.offset?.[1] ?? 0,
      rot: pad.rot ?? 0,
      ratio: pad.ratio ?? 0.25,
      hole: pad.slot ? "oblong" : "round",
      drill: pad.drill ?? 800,
      slotW: pad.slot?.[0] ?? 600,
      slotH: pad.slot?.[1] ?? 1000,
      clearance: pad.clearance ?? null,
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
  const hasHole = form.kind !== "smd";
  const circle = form.shape === "circle";

  const submit = async () => {
    setError(null);
    const size: [number, number] = [form.w, circle ? form.w : form.h];
    const message = checkPadValues(
      { kind: form.kind, shape: form.shape, size, drill: form.hole === "round" ? form.drill : null, slot: form.hole === "oblong" ? [form.slotW, form.slotH] : null, ratio: form.shape === "round_rect" ? form.ratio : null, pasteRatio: form.pasteRatio },
      fmt
    );
    if (message) return setError(message);

    // What differs from the pad as it is becomes a key of the pad's edit; an override that was switched off becomes `undefined` and goes.
    const was = {
      kind: pad.kind ?? (pad.th ? "through_hole" : "smd"),
      shape: pad.shape ?? (pad.round ? (Math.abs(pad.w - pad.h) < 1 ? "circle" : "oval") : "rect"),
      size: pad.size ?? [pad.w, pad.h],
    };
    const patch: Partial<PadEditCmd> = {};
    if (form.kind !== was.kind) patch.kind = form.kind;
    if (form.shape !== was.shape) patch.shape = form.shape;
    if (size[0] !== was.size[0] || size[1] !== was.size[1]) patch.size = size;
    if (hasHole) {
      if (form.hole === "round" && (pad.slot || pad.drill !== form.drill)) patch.drill = form.drill;
      if (form.hole === "oblong" && (!pad.slot || pad.slot[0] !== form.slotW || pad.slot[1] !== form.slotH)) patch.drill_slot = [form.slotW, form.slotH];
    }
    const offset = pad.offset ?? [0, 0];
    if (form.ox !== offset[0] || form.oy !== offset[1]) patch.offset = { x: form.ox, y: form.oy };
    if (Math.abs(form.rot - (pad.rot ?? 0)) > 1e-9) patch.rot = Math.round((((form.rot % 360) + 360) % 360) * 1000);
    if (form.shape === "round_rect" && (pad.ratio ?? 0.25) !== form.ratio) patch.roundrect_ratio = form.ratio;
    const override = <T,>(key: keyof PadEditCmd, now: T | null, before: T | null | undefined) => {
      if (now === (before ?? null)) return;
      (patch as Record<string, unknown>)[key] = now === null ? undefined : now;
    };
    override("clearance", form.clearance, pad.clearance);
    override("solder_mask_margin", form.mask, pad.mask_margin);
    override("solder_paste_margin", form.paste, pad.paste_margin);
    override("solder_paste_margin_ratio", form.pasteRatio, pad.paste_ratio);
    // An offset of nothing is no offset.
    if (patch.offset && patch.offset.x === 0 && patch.offset.y === 0) patch.offset = undefined;

    const edit = padEditWith(part, pad, patch);
    const cmd: Cmd = { op: "edit_board_pad", part: part.ref, edit };
    setBusy(true);
    try {
      if (await api.cmd(cmd)) close();
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
      <div className="dialog" style={{ width: 500 }} onClick={(e) => e.stopPropagation()}>
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
            <span>Pad shape</span>
            <select value={form.shape} onChange={(e) => set("shape", e.target.value as Shape)} data-testid="pad-shape">
              {SHAPES.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
            <span>Pad size</span>
            <span>
              {len(form.w, (w) => set("w", w), 74, "pad-size-x")}
              {!circle && (
                <>
                  {" × "}
                  {len(form.h, (h) => set("h", h), 74, "pad-size-y")}
                </>
              )}{" "}
              {units}
            </span>
            <span>Shape offset</span>
            <span>
              {len(form.ox, (v) => set("ox", v), 74, "pad-offset-x")} {len(form.oy, (v) => set("oy", v), 74, "pad-offset-y")} {units}
            </span>
            <span>Rotation (clockwise)</span>
            <span>
              <input type="number" step="any" value={form.rot} onChange={(e) => Number.isFinite(Number(e.target.value)) && set("rot", Number(e.target.value))} style={{ width: 74 }} data-testid="pad-rot" />°
            </span>
            {form.shape === "round_rect" && (
              <>
                <span>Corner radius ratio</span>
                <span>
                  <input type="number" step="any" min={0} max={0.5} value={form.ratio} onChange={(e) => Number.isFinite(Number(e.target.value)) && set("ratio", Number(e.target.value))} style={{ width: 74 }} data-testid="pad-ratio" />{" "}
                  (radius {fmt(Math.round(form.ratio * Math.min(form.w, form.h)))})
                </span>
              </>
            )}
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
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>Clearance overrides and settings (blank = the board&apos;s)</p>
          <div className="kv-grid" style={{ gridTemplateColumns: "150px 1fr" }}>
            {override("Pad clearance", form.clearance, (v) => set("clearance", v), "pad-clearance")}
            {override("Solder mask margin", form.mask, (v) => set("mask", v), "pad-mask")}
            {override("Solder paste margin", form.paste, (v) => set("paste", v), "pad-paste")}
            {override("Solder paste ratio", form.pasteRatio, (v) => set("pasteRatio", v), "pad-paste-ratio", true)}
          </div>
          {error && (
            <p style={{ color: "var(--error, #e5534b)", margin: "10px 0 0", fontSize: 12 }} data-testid="board-pad-properties-error">
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
