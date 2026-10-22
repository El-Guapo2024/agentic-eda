// The shape fields of pcbnew/dialogs/dialog_pad_properties.cpp, shared by the Pad Properties dialog of a pad on the board and the footprint editor's: the nine shape
// choices, the size, a trapezoid's delta and axis, the corner size, the chamfer's size and corners, a custom pad's zone knockout, and the list of the custom pad's
// basic shapes (add, edit, delete, duplicate). The rules are `kicad-port/padShape.ts`'s; this is the view of them.
import { Fragment, useState } from "react";
import type { PadPrimitive, PadStackMode, UnconnectedLayers } from "../api/types";
import {
  PAD_SHAPE_CHOICES,
  PRIMITIVE_KINDS,
  canFill,
  checkPrimitive,
  chooseShape,
  describePrimitive,
  enabledCorners,
  hasSizeY,
  isChamfered,
  isCustom,
  maxChamferRatio,
  newPrimitive,
  polygonWith,
  primitiveFields,
  primitivesWith,
  setChamferRatio,
  setCornerRatio,
  setFilled,
  setPrimitiveField,
  type PadShapeChoice,
  type PadShapeForm,
  type PrimitiveKind,
} from "../kicad-port/padShape";
import { PADSTACK_MODES, copperLayersChoices, editLayerChoices, type PadStackState } from "../kicad-port/padStack";
import { formatLength, umFrom, umTo, type LengthUnit } from "../state/units";

type PolygonShape = Extract<PadPrimitive["shape"], { kind: "polygon" }>;

/** A length field in the display unit. */
function Len({ um, units, onChange, testid, width = 74 }: { um: number; units: LengthUnit; onChange: (um: number) => void; testid?: string; width?: number }) {
  return (
    <input
      type="number"
      step="any"
      value={Number(umTo(um, units).toFixed(4))}
      data-testid={testid}
      onChange={(e) => Number.isFinite(Number(e.target.value)) && e.target.value !== "" && onChange(Math.round(umFrom(Number(e.target.value), units)))}
      style={{ width }}
    />
  );
}

/** A percentage of the shorter side: the ratio is kept as a fraction. */
function Percent({ ratio, onChange, testid }: { ratio: number; onChange: (ratio: number) => void; testid: string }) {
  return (
    <input
      type="number"
      step="any"
      min={0}
      value={Number((ratio * 100).toFixed(3))}
      data-testid={testid}
      onChange={(e) => Number.isFinite(Number(e.target.value)) && e.target.value !== "" && onChange(Number(e.target.value) / 100)}
      style={{ width: 74 }}
    />
  );
}

/**
 * The padstack's two rows of a `kv-grid`: "Padstack mode" (Normal, Front/Inner/Back, Custom) and "Edit layer", which picks the copper layer the shape rows below show
 * (`m_cbPadstackMode`, `m_cbEditLayer`; the edit layer is disabled for a normal stack). Shown for a plated through-hole pad only, as KiCad does. The rules are `kicad-port/padStack.ts`'s.
 */
export function PadStackRows({ state, boardLayers, onMode, onEditLayer }: { state: PadStackState; boardLayers: readonly string[]; onMode: (mode: PadStackMode) => void; onEditLayer: (key: string) => void }) {
  return (
    <>
      <span>Padstack mode</span>
      <select value={state.mode} onChange={(e) => onMode(e.target.value as PadStackMode)} data-testid="pad-stack-mode">
        {PADSTACK_MODES.map((m) => (
          <option key={m.value} value={m.value}>
            {m.label}
          </option>
        ))}
      </select>
      <span>Edit layer</span>
      <select value={state.editLayer} disabled={state.mode === "normal"} onChange={(e) => onEditLayer(e.target.value)} data-testid="pad-stack-layer">
        {editLayerChoices(state.mode, boardLayers).map((c) => (
          <option key={c.key} value={c.key}>
            {c.label}
          </option>
        ))}
      </select>
    </>
  );
}

/** "Copper layers" of a plated through-hole pad (`m_rbCopperLayersSel`): all, front and back and connected, connected only. */
export function CopperLayersRow({ value, boardLayers, onChange }: { value: UnconnectedLayers; boardLayers: readonly string[]; onChange: (v: UnconnectedLayers) => void }) {
  const choices = copperLayersChoices(boardLayers[0] ?? "F.Cu", boardLayers[boardLayers.length - 1] ?? "B.Cu");
  return (
    <>
      <span>Copper layers</span>
      <select value={value === "start_end_only" ? "remove_except_start_and_end" : value} onChange={(e) => onChange(e.target.value as UnconnectedLayers)} data-testid="pad-copper-layers">
        {choices.map((c) => (
          <option key={c.value} value={c.value}>
            {c.label}
          </option>
        ))}
      </select>
    </>
  );
}

/**
 * The rows of the shape part of a `kv-grid` (label, control, label, control ...): Pad shape, the size, and what the shape needs. `onChoose` is told the choice made (the
 * dialogs follow it with the spoke angle); without it the form takes the choice by `chooseShape` alone.
 */
export function PadShapeRows({ form, onForm, units, onChoose }: { form: PadShapeForm; onForm: (f: PadShapeForm) => void; units: LengthUnit; onChoose?: (choice: PadShapeChoice) => void }) {
  const set = (patch: Partial<PadShapeForm>) => onForm({ ...form, ...patch });
  const circular = !hasSizeY(form.choice);
  const shorter = Math.min(form.w, circular ? form.w : form.h);
  const enabled = enabledCorners(form);
  return (
    <>
      <span>Pad shape</span>
      <select value={form.choice} onChange={(e) => (onChoose ? onChoose(e.target.value as PadShapeChoice) : onForm(chooseShape(form, e.target.value as PadShapeChoice, 90).form))} data-testid="pad-shape">
        {PAD_SHAPE_CHOICES.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      <span>{circular ? "Diameter" : "Pad size X"}</span>
      <span>
        <Len um={form.w} units={units} onChange={(w) => set({ w })} testid="pad-size-x" />
        {!circular && (
          <>
            {" × "}
            <Len um={form.h} units={units} onChange={(h) => set({ h })} testid="pad-size-y" />
          </>
        )}{" "}
        {units}
      </span>
      {form.choice === "trapezoid" && (
        <>
          <span>Trapezoid delta</span>
          <span>
            <Len um={form.trapDelta} units={units} onChange={(trapDelta) => set({ trapDelta })} testid="pad-trap-delta" /> {units}
          </span>
          <span>Trapezoid axis</span>
          <select value={form.trapAxis} onChange={(e) => set({ trapAxis: e.target.value as "horizontal" | "vertical" })} data-testid="pad-trap-axis">
            <option value="horizontal">Horizontal</option>
            <option value="vertical">Vertical</option>
          </select>
        </>
      )}
      {(form.choice === "round_rect" || form.choice === "chamfered_round_rect") && (
        <>
          <span>Corner size</span>
          <span>
            <Percent ratio={form.ratio} onChange={(r) => onForm(setCornerRatio(form, r))} testid="pad-corner-ratio" /> % (radius {formatLength(Math.round(form.ratio * shorter), units)})
          </span>
        </>
      )}
      {isChamfered(form.choice) && (
        <>
          <span>Chamfer size</span>
          <span>
            <Percent ratio={form.chamferRatio} onChange={(r) => onForm(setChamferRatio(form, r))} testid="pad-chamfer-ratio" /> % (up to {Math.round(maxChamferRatio(form) * 100)}%)
          </span>
          <span>Chamfer corners</span>
          <span>
            {(
              [
                ["top_left", "Top left", "tl"],
                ["top_right", "Top right", "tr"],
                ["bottom_left", "Bottom left", "bl"],
                ["bottom_right", "Bottom right", "br"],
              ] as const
            ).map(([key, label, short]) => (
              <label key={key} style={{ marginRight: 10, opacity: enabled[key] ? 1 : 0.5 }}>
                <input type="checkbox" checked={form.corners[key]} disabled={!enabled[key]} data-testid={`pad-chamfer-${short}`} onChange={(e) => set({ corners: { ...form.corners, [key]: e.target.checked } })} /> {label}
              </label>
            ))}
          </span>
        </>
      )}
      {isCustom(form.choice) && (
        <>
          <span>Zone knockout</span>
          <select value={form.knockout} onChange={(e) => set({ knockout: e.target.value as "outline" | "convex_hull" })} data-testid="pad-knockout">
            <option value="outline">Pad shape</option>
            <option value="convex_hull">Pad convex hull</option>
          </select>
        </>
      )}
    </>
  );
}

/** The list of a custom pad's basic shapes, with the editor of the one being edited; nothing for any other shape. */
export function PadPrimitivePanel({ form, onForm, units }: { form: PadShapeForm; onForm: (f: PadShapeForm) => void; units: LengthUnit }) {
  /** The basic shape being edited (an index into the form's list) with the copy the editor holds, and the kind the Add button adds. */
  const [editing, setEditing] = useState<{ index: number; draft: PadPrimitive } | null>(null);
  const [addKind, setAddKind] = useState<PrimitiveKind>("segment");
  const [error, setError] = useState<string | null>(null);
  if (!isCustom(form.choice)) return null;
  const fmt = (um: number) => formatLength(um, units);
  const circular = !hasSizeY(form.choice);
  const setPrimitives = (list: PadPrimitive[]) => onForm({ ...form, primitives: list });
  const startEdit = (index: number, from: PadPrimitive[] = form.primitives) => {
    setError(null);
    setEditing({ index, draft: { ...from[index]!, shape: structuredClone(from[index]!.shape) } });
  };
  const draft = editing?.draft ?? null;
  const setDraft = (next: PadPrimitive) => setEditing((e) => (e ? { ...e, draft: next } : e));
  const finishEdit = () => {
    if (!editing || !draft) return;
    const why = checkPrimitive(draft);
    if (why) return setError(`Error: Basic shape ${editing.index + 1}: ${why}`);
    setError(null);
    setPrimitives(primitivesWith(form.primitives, { replace: [editing.index, draft] }));
    setEditing(null);
  };

  return (
    <>
      <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>Basic shapes of the custom pad (the pad&apos;s position is their origin)</p>
      <div data-testid="pad-primitives">
        {form.primitives.length === 0 && <p style={{ fontSize: 12, margin: "2px 0" }}>The pad is its base only.</p>}
        {form.primitives.map((prim, i) => (
          <div key={i} data-testid={`pad-prim-row-${i}`} style={{ display: "flex", gap: 6, alignItems: "center", padding: "2px 0", fontSize: 12 }}>
            <span style={{ flex: 1 }}>
              {i + 1}. {describePrimitive(prim, fmt)}
            </span>
            <button onClick={() => startEdit(i)} data-testid={`pad-prim-edit-${i}`}>
              Edit
            </button>
            <button onClick={() => setPrimitives(primitivesWith(form.primitives, { duplicate: i }))} data-testid={`pad-prim-dup-${i}`}>
              Duplicate
            </button>
            <button
              onClick={() => {
                setEditing(null);
                setPrimitives(primitivesWith(form.primitives, { remove: i }));
              }}
              data-testid={`pad-prim-delete-${i}`}
            >
              Delete
            </button>
          </div>
        ))}
        <div style={{ display: "flex", gap: 6, alignItems: "center", marginTop: 4 }}>
          <select value={addKind} onChange={(e) => setAddKind(e.target.value as PrimitiveKind)} data-testid="pad-prim-kind">
            {PRIMITIVE_KINDS.map((k) => (
              <option key={k.value} value={k.value}>
                {k.label}
              </option>
            ))}
          </select>
          <button
            onClick={() => {
              const list = primitivesWith(form.primitives, { add: newPrimitive(addKind, form.w, circular ? form.w : form.h) });
              setPrimitives(list);
              startEdit(list.length - 1, list);
            }}
            data-testid="pad-prim-add"
          >
            Add basic shape
          </button>
        </div>
        {editing && draft && (
          <div style={{ border: "1px solid var(--chrome-border, #555)", borderRadius: 4, padding: 8, marginTop: 8 }} data-testid="pad-prim-editor">
            <div style={{ fontWeight: 600, marginBottom: 4 }}>
              Basic shape {editing.index + 1}: {PRIMITIVE_KINDS.find((k) => k.value === draft.shape.kind)?.label}
            </div>
            <div className="kv-grid" style={{ gridTemplateColumns: "150px 1fr" }}>
              {primitiveFields(draft.shape).map((f) => (
                <Fragment key={f.key}>
                  <span>{f.label}</span>
                  <span>
                    <Len um={f.value} units={units} onChange={(v) => setDraft({ ...draft, shape: setPrimitiveField(draft.shape, f.key, v) })} testid={`pad-prim-field-${f.key}`} /> {units}
                  </span>
                </Fragment>
              ))}
              {canFill(draft.shape) && (
                <>
                  <span>Fill</span>
                  <select value={draft.shape.filled ? "solid" : "none"} onChange={(e) => setDraft({ ...draft, shape: setFilled(draft.shape, e.target.value === "solid") })} data-testid="pad-prim-fill">
                    <option value="none">None</option>
                    <option value="solid">Solid</option>
                  </select>
                </>
              )}
            </div>
            {draft.shape.kind === "polygon" && (
              <div style={{ marginTop: 6 }} data-testid="pad-prim-points">
                {(draft.shape as PolygonShape).pts.map((p, i) => (
                  <div key={i} style={{ display: "flex", gap: 6, alignItems: "center", padding: "1px 0" }}>
                    <span style={{ width: 54 }}>Point {i + 1}</span>
                    <Len um={p.x} units={units} onChange={(x) => setDraft({ ...draft, shape: polygonWith(draft.shape as PolygonShape, { set: [i, { x, y: p.y }] }) })} testid={`pad-prim-point-${i}-x`} />
                    <Len um={p.y} units={units} onChange={(y) => setDraft({ ...draft, shape: polygonWith(draft.shape as PolygonShape, { set: [i, { x: p.x, y }] }) })} testid={`pad-prim-point-${i}-y`} /> {units}
                    <button onClick={() => setDraft({ ...draft, shape: polygonWith(draft.shape as PolygonShape, { remove: i }) })} data-testid={`pad-prim-point-${i}-delete`}>
                      Delete
                    </button>
                  </div>
                ))}
                <button
                  onClick={() => {
                    const pts = (draft.shape as PolygonShape).pts;
                    const last = pts[pts.length - 1] ?? { x: 0, y: 0 };
                    setDraft({ ...draft, shape: polygonWith(draft.shape as PolygonShape, { add: { x: last.x + 200, y: last.y + 200 } }) });
                  }}
                  data-testid="pad-prim-point-add"
                >
                  Add point
                </button>
              </div>
            )}
            {error && (
              <p style={{ color: "var(--chrome-danger, #e5534b)", margin: "6px 0 0", fontSize: 12 }} data-testid="pad-prim-error">
                {error}
              </p>
            )}
            <div style={{ display: "flex", gap: 6, justifyContent: "flex-end", marginTop: 8 }}>
              <button
                onClick={() => {
                  setEditing(null);
                  setError(null);
                }}
                data-testid="pad-prim-cancel"
              >
                Cancel shape
              </button>
              <button className="primary" onClick={finishEdit} data-testid="pad-prim-ok">
                OK shape
              </button>
            </div>
          </div>
        )}
      </div>
    </>
  );
}
