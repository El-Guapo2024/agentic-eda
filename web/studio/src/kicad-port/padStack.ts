// The padstack half of pcbnew/dialogs/dialog_pad_properties.cpp: the "Padstack mode" choice (Normal, Front/Inner/Back, Custom), the "Edit layer" choice that picks which
// copper layer the shape rows show (`m_editLayer`, `OnEditLayerChanged`, `afterPadstackModeChanged`), what changing the mode does to the layers
// (`transferDataToPad`'s `setMode` and `newLayerMap`), the "Copper layers" choice of a plated through-hole pad (`UpdateLayersDropdown`: all, front/back and connected,
// connected only) and what OK sends (`PadStack`). Pure -- no React -- so the two Pad Properties dialogs stay thin views of it and the rules are tested. The shape of each
// layer is a `PadShapeForm` (kicad-port/padShape.ts) with the copper's offset, which is per layer (`initPadstackLayerValues`).
//
// The pad's own fields are its front shape (the `F.Cu` of `PADSTACK::ALL_LAYERS`), kept by the dialog as it always has; this state holds the other layers.
import type { PadLayerShape, PadStack, PadStackMode, Um, UnconnectedLayers } from "../api/types";
import { NO_CORNERS, checkShape, shapeEdit, shapeFormOf, type PadShapeForm } from "./padShape";

/** The key of the pad's own layer, the front (`PADSTACK::ALL_LAYERS` is `F_Cu`). */
export const FRONT = "F.Cu";
/** The key of the inner layers of a front/inner/back padstack (`PADSTACK::INNER_LAYERS`); the back is `B.Cu`. */
export const INNER = "Inner";
export const BACK = "B.Cu";

/** One copper layer's shape as the dialog holds it: the shape form, the copper's offset from the pad's position and what the file said of the layer besides. */
export interface LayerForm {
  s: PadShapeForm;
  ox: Um;
  oy: Um;
  tokens: string[];
}

export interface PadStackState {
  mode: PadStackMode;
  unconnected: UnconnectedLayers;
  /** `m_editLayer`: which layer the shape rows are showing. */
  editLayer: string;
  /** The layers but the front, by key (`Inner`, `B.Cu` of a front/inner/back stack; the layer names of a custom one). */
  layers: Record<string, LayerForm>;
}

export const PADSTACK_MODES: { value: PadStackMode; label: string }[] = [
  { value: "normal", label: "Normal" },
  { value: "front_inner_back", label: "Front/Inner/Back" },
  { value: "custom", label: "Custom" },
];

/** `UpdateLayersDropdown` for a plated through-hole pad: the choices of "Copper layers" (the "None" one is the layer list's, not the stack's). */
export function copperLayersChoices(front = "F.Cu", back = "B.Cu"): { value: UnconnectedLayers; label: string }[] {
  return [
    { value: "keep_all", label: "All copper layers" },
    { value: "remove_except_start_and_end", label: `${front}, ${back} and connected layers` },
    { value: "remove_all", label: "Connected layers only" },
  ];
}

/** `m_padstackControls->Show( m_padType->GetSelection() == PTH_DLG_TYPE )`: "For now, padstack controls only enabled for PTH pads". */
export function padstackControlsShown(kind: string): boolean {
  return kind === "through_hole";
}

/** The form of one layer shape (`initPadstackLayerValues`). */
export function layerFormOfShape(shape: PadLayerShape): LayerForm {
  return {
    s: shapeFormOf({
      shape: shape.shape,
      size: shape.size,
      w: shape.size[0],
      h: shape.size[1],
      ratio: shape.roundrect_ratio ?? null,
      delta: shape.trapezoid_delta ?? null,
      chamfer_ratio: shape.chamfer_ratio ?? null,
      chamfer_corners: shape.chamfer_corners ?? null,
      custom: shape.custom ?? null,
    }),
    ox: shape.offset?.[0] ?? 0,
    oy: shape.offset?.[1] ?? 0,
    tokens: [...(shape.tokens ?? [])],
  };
}

/** The layer shape a form stands for (`transferDataToPad` for one layer): the shape and size, what belongs to the shape, the copper's offset. */
export function layerShapeOf(form: LayerForm): PadLayerShape {
  const e = shapeEdit(form.s);
  const out: PadLayerShape = { shape: e.shape!, size: e.size! };
  if (e.roundrect_ratio !== undefined) out.roundrect_ratio = e.roundrect_ratio;
  if (e.trapezoid_delta !== undefined) out.trapezoid_delta = e.trapezoid_delta;
  if (e.chamfer_ratio !== undefined) {
    out.chamfer_ratio = e.chamfer_ratio;
    out.chamfer_corners = e.chamfer_corners ?? { ...NO_CORNERS };
  }
  if (e.custom !== undefined) out.custom = e.custom;
  if (form.ox !== 0 || form.oy !== 0) out.offset = [form.ox, form.oy];
  if (form.tokens.length > 0) out.tokens = [...form.tokens];
  return out;
}

const copy = (f: LayerForm): LayerForm => ({ s: { ...f.s, corners: { ...f.s.corners }, primitives: f.s.primitives.map((p) => ({ ...p, shape: structuredClone(p.shape) })) }, ox: f.ox, oy: f.oy, tokens: [...f.tokens] });

/**
 * The state the dialog opens with for a pad's stack (`afterPadstackModeChanged` after `initPadstackLayerValues`). A layer the stack does not give has the front's shape
 * (`PADSTACK::CopperLayer` falls back to `ALL_LAYERS`), so a front/inner/back stack always shows its two other layers.
 */
export function stackStateOf(stack: PadStack | null | undefined, front: LayerForm): PadStackState {
  const mode = stack?.mode ?? "normal";
  const layers: Record<string, LayerForm> = {};
  if (mode === "front_inner_back") {
    layers[INNER] = stack?.inner ? layerFormOfShape(stack.inner) : copy(front);
    layers[BACK] = stack?.back ? layerFormOfShape(stack.back) : copy(front);
  } else if (mode === "custom") {
    for (const [name, shape] of Object.entries(stack?.layers ?? {})) layers[name] = layerFormOfShape(shape);
  }
  return { mode, unconnected: stack?.unconnected ?? "keep_all", editLayer: FRONT, layers };
}

/**
 * The layers the "Edit layer" choice offers (`afterPadstackModeChanged`): the front alone for a normal stack (the choice disabled); the front, `Inner Layers` and the back for a
 * front/inner/back one; every copper layer of the board, front to back, for a custom one. `boardLayers` are the board's copper layers in its own order.
 */
export function editLayerChoices(mode: PadStackMode, boardLayers: readonly string[]): { key: string; label: string }[] {
  const all = boardLayers.length >= 2 ? [...boardLayers] : [FRONT, BACK];
  const first = all[0] ?? FRONT;
  const last = all[all.length - 1] ?? BACK;
  switch (mode) {
    case "normal":
      return [{ key: FRONT, label: first }];
    case "front_inner_back":
      return [
        { key: FRONT, label: first },
        { key: INNER, label: "Inner Layers" },
        { key: BACK, label: last },
      ];
    case "custom":
      return all.map((name) => ({ key: name === first ? FRONT : name, label: name }));
  }
}

/** The shape form being edited: the front's is the dialog's own, any other layer's is the stack state's. */
export function layerFormOf(state: PadStackState, front: LayerForm, key: string): LayerForm {
  if (key === FRONT) return front;
  return state.layers[key] ?? front;
}

/** The state and front after the edited layer's form changed. */
export function withLayerForm(state: PadStackState, front: LayerForm, key: string, next: LayerForm): { state: PadStackState; front: LayerForm } {
  if (key === FRONT) return { state, front: next };
  return { state: { ...state, layers: { ...state.layers, [key]: next } }, front };
}

/**
 * Choosing the padstack mode (`transferDataToPad`'s `setMode`): the layers the new mode has that the old one did not name start as copies of the layer they are initialised
 * from -- an inner layer from the inner layers' shape (`PADSTACK::INNER_LAYERS`, `In1.Cu`), any other from the front -- and the layers it does not have are dropped. The edit
 * layer stays when the new mode has it, else it is the inner layers (`if( m_editLayer != F_Cu && m_editLayer != B_Cu ) m_editLayer = INNER_LAYERS`) or the front.
 */
export function changeMode(state: PadStackState, mode: PadStackMode, front: LayerForm, boardLayers: readonly string[]): PadStackState {
  if (mode === state.mode) return state;
  const layers: Record<string, LayerForm> = {};
  const first = boardLayers[0] ?? FRONT;
  const isInner = (name: string) => /^In\d+\.Cu$/.test(name);
  if (mode === "front_inner_back") {
    // From a custom stack the inner layers' shape is In1.Cu's (`INNER_LAYERS`), the back's its `B.Cu`.
    layers[INNER] = copy(state.layers[INNER] ?? state.layers["In1.Cu"] ?? front);
    layers[BACK] = copy(state.layers[BACK] ?? front);
  } else if (mode === "custom") {
    const inner = state.layers[INNER] ?? state.layers["In1.Cu"];
    for (const name of boardLayers.length >= 2 ? boardLayers : [FRONT, BACK]) {
      if (name === first || name === FRONT) continue;
      const known = state.layers[name] ?? (name === "In1.Cu" ? state.layers[INNER] : undefined) ?? (name === BACK || name === boardLayers[boardLayers.length - 1] ? state.layers[BACK] : undefined);
      layers[name] = copy(known ?? (isInner(name) && inner ? inner : front));
    }
  }
  const choices = editLayerChoices(mode, boardLayers).map((c) => c.key);
  const editLayer = choices.includes(state.editLayer) ? state.editLayer : mode === "front_inner_back" ? INNER : FRONT;
  return { ...state, mode, layers, editLayer };
}

/**
 * What OK sends: the stack the dialog leaves (`PadStack`), with the mode, the layers' shapes and the unconnected-layer mode. A normal stack has no layers; a front/inner/back
 * one the inner and back; a custom one the layers of the board.
 */
export function stackOf(state: PadStackState): PadStack {
  const out: PadStack = {};
  if (state.mode !== "normal") out.mode = state.mode;
  if (state.mode === "front_inner_back") {
    if (state.layers[INNER]) out.inner = layerShapeOf(state.layers[INNER]);
    if (state.layers[BACK]) out.back = layerShapeOf(state.layers[BACK]);
  } else if (state.mode === "custom") {
    const layers: Record<string, PadLayerShape> = {};
    for (const [name, form] of Object.entries(state.layers)) layers[name] = layerShapeOf(form);
    if (Object.keys(layers).length > 0) out.layers = layers;
  }
  if (state.unconnected !== "keep_all") out.unconnected = state.unconnected;
  return out;
}

/** A stack as it is compared: what it holds, without the keys that say nothing. */
function canonical(stack: PadStack | null | undefined): string {
  const s = stack ?? {};
  const clean = (shape: PadLayerShape | null | undefined) => (shape ? JSON.parse(JSON.stringify(shape, (_k, v) => (v === null || v === undefined || (Array.isArray(v) && v.length === 0) ? undefined : v))) : undefined);
  return JSON.stringify({
    mode: s.mode && s.mode !== "normal" ? s.mode : undefined,
    inner: clean(s.inner),
    back: clean(s.back),
    layers: s.layers && Object.keys(s.layers).length > 0 ? Object.fromEntries(Object.entries(s.layers).sort(([a], [b]) => a.localeCompare(b)).map(([k, v]) => [k, clean(v)])) : undefined,
    unconnected: s.unconnected && s.unconnected !== "keep_all" ? s.unconnected : undefined,
  });
}

/** Whether two stacks say different things (a missing one is the default: a normal stack that keeps every layer). */
export function stackDiffers(a: PadStack | null | undefined, b: PadStack | null | undefined): boolean {
  return canonical(a) !== canonical(b);
}

/** `PAD::CheckPad` for every layer of the stack (`padValuesOK` runs `doCheckPad` for each unique layer): the first message, with the layer named, or null. */
export function checkStack(state: PadStackState): string | null {
  const names: Record<string, string> = { [INNER]: "the inner layers", [BACK]: "B.Cu" };
  for (const [key, form] of Object.entries(state.layers)) {
    if (form.s.w < 0 || form.s.h < 0) return `Error: pad size on ${names[key] ?? key} must not be negative`;
    const why = checkShape(form.s);
    if (why) return `${why} (${names[key] ?? key})`;
  }
  return null;
}

/**
 * The groups of layers of a padstack pad in the order they are drawn: the inner layers first, then the far side's outer layer, the footprint's own side last (on top). A group
 * is on a side by its layers: `F.Cu` and `B.Cu` are the outer ones, any other copper layer is inner.
 */
export function orderedStackLayers<T extends { layers: string[] }>(groups: readonly T[], side: "top" | "bottom"): T[] {
  const near = side === "bottom" ? "B.Cu" : "F.Cu";
  const far = side === "bottom" ? "F.Cu" : "B.Cu";
  const rank = (g: T) => (g.layers.includes(near) ? 2 : g.layers.includes(far) ? 1 : 0);
  return [...groups].sort((a, b) => rank(a) - rank(b));
}
