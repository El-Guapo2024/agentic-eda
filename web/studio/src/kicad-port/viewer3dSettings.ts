// The 3D viewer's layer presets, its saved viewports and its preferences.
//
// Ported from KiCad 8303b2ad:
// - 3d-viewer/dialogs/appearance_controls_3D.cpp: `rebuildLayerPresetsWidget` (the list: "Follow PCB Editor", "Follow PCB Plot Settings", the person's presets, "Save
//   preset...", "Delete preset..."), `onLayerPresetChanged`, `doApplyLayerPreset`, `syncLayerPresetSelection` (which preset the rows and colours match),
//   `OnLayerVisibilityChanged` (which rows take the view off a "Follow" preset: `killFollow`), `rebuildViewportsWidget`, `onViewportChanged`, `doApplyViewport`;
// - 3d-viewer/3d_canvas/board_adapter.cpp: `GetVisibleLayers` (the FOLLOW_PCB and FOLLOW_PLOT_SETTINGS branches), `GetDefaultVisibleLayers`;
// - 3d-viewer/3d_viewer/eda_3d_viewer_frame.cpp `LoadSettings` (the "legacy colors" preset made when the viewer first opens);
// - 3d-viewer/3d_viewer/eda_3d_viewer_settings.cpp (the settings and their defaults), dialogs/panel_3D_display_options.cpp and panel_3D_opengl_options.cpp (the
//   Preferences pages).
//
// Where KiCad keeps them: the presets and the preferences are the person's (`3d_viewer.json`: `layer_presets`, `render.*`, `camera.*`), the viewports the project's
// (`board.3dviewports` in the `.kicad_pro`). The studio keeps the first in this browser (state/viewer3dUserSettings.ts) and the viewports in `appearance.json`'s
// `viewer3d` section beside the design (crates/cli/src/appearance_api.rs). Pure: no React, no three.js.
import { GRID3D_TYPES, type Grid3DType } from "./grid3d";
import { DEFAULT_COLORS, ROWS, USER_ROWS, isVisible, parseColor, rowLayer, toHex, withVisibility, type Rgba } from "./appearance3d";
import type { CameraView } from "./camera3d";
import { multiplyQuat, normalizeQuat, quatFromAxisAngle, rotateVecByQuat, vec3 } from "./math3d";

/** `FOLLOW_PCB` and `FOLLOW_PLOT_SETTINGS` (eda_3d_viewer_settings.h): the two built-in presets, by their internal names. */
export const FOLLOW_PCB = "follow_pcb_editor";
export const FOLLOW_PLOT_SETTINGS = "follow_plot_settings";

/** `_( "legacy colors" )`: the preset `EDA_3D_VIEWER_FRAME::LoadSettings` makes of the default rows and colours the first time the viewer opens. */
export const LEGACY_COLORS = "legacy colors";

/** `LAYER_PRESET_3D`: a name, every row's visibility and the colours. */
export interface LayerPreset3D {
  name: string;
  layers: Record<string, boolean>;
  /** Row id -> `#rrggbbaa`. */
  colors: Record<string, string>;
}

/** `BOARD_ADAPTER::GetDefaultVisibleLayers`: what the "legacy colors" preset shows. */
export function defaultVisibleLayers(): Record<string, boolean> {
  const v: Record<string, boolean> = {
    board: true,
    plated_barrels: true,
    copper_top: true,
    copper_bottom: true,
    silkscreen_top: true,
    silkscreen_bottom: true,
    soldermask_top: true,
    soldermask_bottom: true,
    solder_paste: true,
    adhesive: true,
    user_comments: false,
    user_drawings: false,
    user_eco1: false,
    user_eco2: false,
    // KiCad 25ec2335 `GetDefaultVisibleLayers`
    f_fab: false,
    b_fab: false,
    f_courtyard: false,
    b_courtyard: false,
    references: true,
    values: true,
    footprint_text: true,
    th_models: true,
    smd_models: true,
    virtual_models: true,
    not_in_pos_models: false,
    dnp_models: false,
    bounding_boxes: false,
    off_board_silk: false,
    navigator: true,
    // The studio's Zones row is KiCad's `render.show_zones` preference (true).
    zones: true,
  };
  for (const r of USER_ROWS) if (/^user_\d+$/.test(r.id)) v[r.id] = false;
  return v;
}

/** The "legacy colors" preset: the default rows and `BOARD_ADAPTER::GetDefaultColors()`. */
export function legacyColorsPreset(): LayerPreset3D {
  const colors: Record<string, string> = {};
  for (const [id, c] of Object.entries(DEFAULT_COLORS)) colors[id] = toHex(c);
  return { name: LEGACY_COLORS, layers: defaultVisibleLayers(), colors };
}

/**
 * `GetVisibleLayers`' FOLLOW_PCB branch: the board body, and the rows of the layers the PCB editor shows (`m_board->IsLayerVisible`: F.Cu, B.Cu, the silkscreens, the
 * masks, F.Paste for the paste, F.Adhes for the adhesive, the four named user layers, KiCad 25ec2335's fab and courtyard layers) and of the elements it shows (`IsElementVisible( LAYER_FP_REFERENCES )`, values,
 * footprint text). User.1 .. User.45 are not in the branch: they keep the viewer's own.
 */
export function followPcbVisibility(layerShown: (kicadLayer: string) => boolean, elementShown: (element: "references" | "values" | "footprint_text") => boolean): Record<string, boolean> {
  const v: Record<string, boolean> = {
    board: true,
    copper_top: layerShown("F.Cu"),
    copper_bottom: layerShown("B.Cu"),
    silkscreen_top: layerShown("F.SilkS"),
    silkscreen_bottom: layerShown("B.SilkS"),
    soldermask_top: layerShown("F.Mask"),
    soldermask_bottom: layerShown("B.Mask"),
    solder_paste: layerShown("F.Paste"),
    adhesive: layerShown("F.Adhes"),
    user_comments: layerShown("Cmts.User"),
    user_drawings: layerShown("Dwgs.User"),
    user_eco1: layerShown("Eco1.User"),
    user_eco2: layerShown("Eco2.User"),
    // KiCad 25ec2335
    f_fab: layerShown("F.Fab"),
    b_fab: layerShown("B.Fab"),
    f_courtyard: layerShown("F.CrtYd"),
    b_courtyard: layerShown("B.CrtYd"),
  };
  for (const el of ["references", "values", "footprint_text"] as const) v[el] = elementShown(el);
  return v;
}

/** The rows a "Follow" preset decides (`GetVisibleLayers`' two branches): everything else stays the viewer's own (`m_Render`). */
export function followRows(preset: string): ReadonlySet<string> {
  const pcb = ["board", "copper_top", "copper_bottom", "silkscreen_top", "silkscreen_bottom", "soldermask_top", "soldermask_bottom", "solder_paste", "adhesive", "user_comments", "user_drawings", "user_eco1", "user_eco2", "f_fab", "b_fab", "f_courtyard", "b_courtyard", "references", "values", "footprint_text"];
  if (preset === FOLLOW_PCB) return new Set(pcb);
  if (preset === FOLLOW_PLOT_SETTINGS) return new Set([...pcb, ...USER_ROWS.filter((r) => /^user_\d+$/.test(r.id)).map((r) => r.id)]);
  return new Set();
}

/** What a preset follows, for the board: the PCB editor's rows and the plot settings' (`plotFollowVisibility`). */
export interface FollowSources {
  pcb: Readonly<Record<string, boolean>>;
  plot: Readonly<Record<string, boolean>>;
}

/** `GetVisibleLayers`: whether row `id` is shown -- a "Follow" preset's row from what it follows, any other from the viewer's own rows (`layers`, else the row's default). */
export function rowShown(layers: Readonly<Record<string, boolean>>, preset: string, follow: FollowSources, id: string): boolean {
  if (followRows(preset).has(id)) {
    const src = preset === FOLLOW_PCB ? follow.pcb : follow.plot;
    const v = src[id];
    if (v !== undefined) return v;
  }
  return isVisible(layers, id);
}

/** Every row as `GetVisibleLayers` has it now: what `SetVisibleLayers( GetVisibleLayers() )` writes into the viewer's own rows. */
export function visibleSnapshot(layers: Readonly<Record<string, boolean>>, preset: string, follow: FollowSources): Record<string, boolean> {
  const out: Record<string, boolean> = {};
  for (const r of ROWS) out[r.id] = rowShown(layers, preset, follow, r.id);
  return out;
}

/** `OnLayerVisibilityChanged`'s `killFollow`: the rows whose change takes the view off a "Follow" preset (the board's rows; not the models', the bounding boxes', the
 * off-board silkscreen's, the navigator's or the background's). */
export function killsFollow(id: string): boolean {
  if (["footprint_text", "references", "values"].includes(id)) return true;
  if (["board", "copper_top", "copper_bottom", "plated_barrels", "silkscreen_top", "silkscreen_bottom", "soldermask_top", "soldermask_bottom", "solder_paste", "adhesive", "user_comments", "user_drawings", "user_eco1", "user_eco2", "f_fab", "b_fab", "f_courtyard", "b_courtyard"].includes(id)) return true;
  return /^user_\d+$/.test(id);
}

/** The colours a preset compares and saves: every row's (`GetLayerColors`), as `#rrggbbaa`. */
export function colorsOf(resolved: Readonly<Record<string, Rgba>>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [id, c] of Object.entries(resolved)) out[id] = toHex(c);
  return out;
}

/**
 * `syncLayerPresetSelection`: the person's preset whose every row (`LAYER_3D_BOARD` .. `LAYER_3D_END`, and the three footprint-text rows) and every colour both have
 * match what is shown, or "" (none: the list sits on its separator). "legacy colors" never matches while "Use board stackup colors" is on.
 */
export function matchingPreset(presets: readonly LayerPreset3D[], visible: Readonly<Record<string, boolean>>, colors: Readonly<Record<string, string>>, useStackupColors: boolean): string {
  const same = (a: string, b: string) => {
    const x = parseColor(a);
    const y = parseColor(b);
    if (!x || !y) return a === b;
    const k = (v: number) => Math.round(v * 255);
    return k(x.r) === k(y.r) && k(x.g) === k(y.g) && k(x.b) === k(y.b) && k(x.a) === k(y.a);
  };
  const hit = presets.find((p) => {
    if (p.name.toLowerCase() === LEGACY_COLORS && useStackupColors) return false;
    for (const r of ROWS) if ((p.layers[r.id] ?? isVisible({}, r.id)) !== (visible[r.id] ?? isVisible({}, r.id))) return false;
    for (const [id, c] of Object.entries(p.colors)) if (colors[id] !== undefined && !same(c, colors[id]!)) return false;
    return true;
  });
  return hit?.name ?? "";
}

/** The viewer's own rows and its preset after row `id` is shown or hidden (`OnLayerVisibilityChanged`): every row as shown now, the change (and the Footprint Text meta-control's), and the preset the result matches (`syncLayerPresetSelection`) when the change takes the view off a "Follow" preset or there was none. */
export function afterVisibilityChange(
  state: { layers: Readonly<Record<string, boolean>>; preset: string },
  id: string,
  shown: boolean,
  ctx: { follow: FollowSources; presets: readonly LayerPreset3D[]; colors: Readonly<Record<string, string>>; useStackupColors: boolean }
): { layers: Record<string, boolean>; preset: string } {
  const following = state.preset === FOLLOW_PCB || state.preset === FOLLOW_PLOT_SETTINGS;
  const layers = withVisibility(visibleSnapshot(state.layers, state.preset, ctx.follow), id, shown);
  if (following && !killsFollow(id)) return { layers, preset: state.preset };
  return { layers, preset: matchingPreset(ctx.presets, layers, ctx.colors, ctx.useStackupColors) };
}

/** One entry of the Presets list (`rebuildLayerPresetsWidget`). */
export type PresetEntry = { kind: "follow"; name: typeof FOLLOW_PCB | typeof FOLLOW_PLOT_SETTINGS; label: string } | { kind: "preset"; name: string } | { kind: "separator" } | { kind: "save" } | { kind: "delete" };

/** `rebuildLayerPresetsWidget`: "Follow PCB Editor", "Follow PCB Plot Settings", "---" and the person's presets when there are any, "---", "Save preset...", "Delete preset...". */
export function presetEntries(presets: readonly LayerPreset3D[]): PresetEntry[] {
  const out: PresetEntry[] = [
    { kind: "follow", name: FOLLOW_PCB, label: "Follow PCB Editor" },
    { kind: "follow", name: FOLLOW_PLOT_SETTINGS, label: "Follow PCB Plot Settings" },
  ];
  if (presets.length > 0) out.push({ kind: "separator" });
  for (const p of presets) out.push({ kind: "preset", name: p.name });
  out.push({ kind: "separator" }, { kind: "save" }, { kind: "delete" });
  return out;
}

/** `onLayerPresetChanged`'s save: the presets with `name` set to what is shown (a new one at the end, an existing one overwritten in its place -- the caller asks "Overwrite existing preset?" first). */
export function savePreset(presets: readonly LayerPreset3D[], name: string, visible: Readonly<Record<string, boolean>>, colors: Readonly<Record<string, string>>): LayerPreset3D[] {
  const preset: LayerPreset3D = { name, layers: { ...visible }, colors: { ...colors } };
  const at = presets.findIndex((p) => p.name === name);
  if (at < 0) return [...presets, preset];
  return presets.map((p, i) => (i === at ? preset : p));
}

/** The "Delete preset..." list's choice taken out (`std::erase_if`); the current preset is "" when it was that one. */
export function deletePreset(presets: readonly LayerPreset3D[], name: string, current: string): { presets: LayerPreset3D[]; current: string } {
  return { presets: presets.filter((p) => p.name !== name), current: current === name ? "" : current };
}

/**
 * `doApplyLayerPreset`: a "Follow" preset is only made current; one of the person's sets every row and the colours (`SetVisibleLayers( preset->layers )`,
 * `SetLayerColors( preset->colors )`), and "legacy colors" turns "Use board stackup colors" off. `null` for a name that is no preset.
 */
export function applyPreset(presets: readonly LayerPreset3D[], name: string): { preset: string; layers?: Record<string, boolean>; colors?: Record<string, string>; useStackupColors?: false } | null {
  if (name === FOLLOW_PCB || name === FOLLOW_PLOT_SETTINGS) return { preset: name };
  const p = presets.find((x) => x.name === name);
  if (!p) return null;
  return { preset: name, layers: { ...p.layers }, colors: { ...p.colors }, ...(p.name.toLowerCase() === LEGACY_COLORS ? { useStackupColors: false as const } : {}) };
}

/** A preset read back from storage: a name, rows that are booleans, colours that are colours; anything else is dropped (`PARAM_LAYER_PRESET_3D::jsonToPresets`). */
export function presetFromJson(j: unknown): LayerPreset3D | null {
  if (typeof j !== "object" || j === null) return null;
  const o = j as Record<string, unknown>;
  if (typeof o.name !== "string" || o.name === "") return null;
  const layers: Record<string, boolean> = {};
  if (o.layers && typeof o.layers === "object") for (const [k, v] of Object.entries(o.layers as Record<string, unknown>)) if (typeof v === "boolean") layers[k] = v;
  const colors: Record<string, string> = {};
  if (o.colors && typeof o.colors === "object") for (const [k, v] of Object.entries(o.colors as Record<string, unknown>)) if (typeof v === "string" && parseColor(v)) colors[k] = v;
  return { name: o.name, layers, colors };
}

// ------------------------------------------------------------------------------------------------------------------------------- viewports

/** `VIEWPORT3D`: a name and the camera ([`CameraView`], what `GetViewMatrix` is made of); a viewport read from a KiCad project (`board.3dviewports`, UI-LEFT-3 D3-5) has
 *  only its `matrix`, the camera put at it when it is chosen ([`viewFromKicadMatrix`]). */
export interface Viewport3D {
  name: string;
  camera?: CameraView;
  /** `VIEWPORT3D::matrix`, the camera's `GetViewMatrix()` in KiCad's 3D units, column-major ([`kicadViewMatrix`]): what the derived `.kicad_pro`'s `board.3dviewports` gets. */
  matrix?: number[];
}

/** `RANGE_SCALE_3D` (board_adapter.h) and `BOARD_ADAPTER::InitSettings`' "Hack to keep "home" zoom from being too small" (`m_biuTo3Dunits *= 1.6f`). */
const RANGE_SCALE_3D = 8;
const HOME_ZOOM_HACK = 1.6;

/**
 * `CAMERA::GetViewMatrix()` of a studio camera, as KiCad would have it for the same view: `CAMERA::updateViewMatrix`'s
 * `translate( m_camera_pos ) * m_rotationMatrix * m_rotationMatrixAux * translate( -m_lookat_pos )` (common/gal/3d/camera.cpp), in KiCad's 3D units
 * (`m_biuTo3Dunits = RANGE_SCALE_3D / max( board width, height ) * 1.6`) and world axes (board x, board y inverted, the thickness up: board_adapter.cpp
 * "The y coord is inverted in 3D viewer"). The camera's own state is kept in KiCad's local axes already (camera3d.ts: `cameraPosLocal`, the trackball and
 * auxiliary rotations, `getRenderPose`); its distances are millimetres scaled by the board span (`initialDistanceMm = 1.25 * span`), so a KiCad unit is
 * `span / 12.8` mm. Column-major, as glm stores it and `PARAM_VIEWPORT3D::viewportsToJson` writes it (`matrix[0].x` first). `spanMm` is the board outline's
 * longest edge, the one the camera was homed with (`setBoardGeometry`).
 */
export function kicadViewMatrix(view: CameraView, spanMm: number): number[] {
  const k = (RANGE_SCALE_3D * HOME_ZOOM_HACK) / Math.max(spanMm, 1e-9);
  const d2r = Math.PI / 180;
  // `updateRotationMatrix`: rotate x, then y, then z, about the camera's own axes; the trackball's rotation before it.
  const aux = multiplyQuat(multiplyQuat(quatFromAxisAngle(vec3(1, 0, 0), view.auxDeg.x * d2r), quatFromAxisAngle(vec3(0, 1, 0), view.auxDeg.y * d2r)), quatFromAxisAngle(vec3(0, 0, 1), view.auxDeg.z * d2r));
  const r = normalizeQuat(multiplyQuat(view.quat, aux));
  const cx = rotateVecByQuat(vec3(1, 0, 0), r);
  const cy = rotateVecByQuat(vec3(0, 1, 0), r);
  const cz = rotateVecByQuat(vec3(0, 0, 1), r);
  // The look-at point in KiCad's world: the studio's world is (board x, thickness, board y) in mm.
  const look = vec3(view.lookAt.x * k, -view.lookAt.z * k, view.lookAt.y * k);
  const rl = rotateVecByQuat(look, r);
  const t = vec3(view.pos.x * k - rl.x, view.pos.y * k - rl.y, view.pos.z * k - rl.z);
  return [cx.x, cx.y, cx.z, 0, cy.x, cy.y, cy.z, 0, cz.x, cz.y, cz.z, 0, t.x, t.y, t.z, 1];
}

/** A rotation matrix's quaternion (its columns the turned axes, as [`kicadViewMatrix`] writes them), Shepperd's method. */
function quatOfColumns(m: readonly number[]): { x: number; y: number; z: number; w: number } {
  // m[col * 4 + row]
  const r = (row: number, col: number) => m[col * 4 + row]!;
  const tr = r(0, 0) + r(1, 1) + r(2, 2);
  if (tr > 0) {
    const s = Math.sqrt(tr + 1) * 2;
    return normalizeQuat({ w: s / 4, x: (r(2, 1) - r(1, 2)) / s, y: (r(0, 2) - r(2, 0)) / s, z: (r(1, 0) - r(0, 1)) / s });
  }
  if (r(0, 0) > r(1, 1) && r(0, 0) > r(2, 2)) {
    const s = Math.sqrt(1 + r(0, 0) - r(1, 1) - r(2, 2)) * 2;
    return normalizeQuat({ w: (r(2, 1) - r(1, 2)) / s, x: s / 4, y: (r(0, 1) + r(1, 0)) / s, z: (r(0, 2) + r(2, 0)) / s });
  }
  if (r(1, 1) > r(2, 2)) {
    const s = Math.sqrt(1 + r(1, 1) - r(0, 0) - r(2, 2)) * 2;
    return normalizeQuat({ w: (r(0, 2) - r(2, 0)) / s, x: (r(0, 1) + r(1, 0)) / s, y: s / 4, z: (r(1, 2) + r(2, 1)) / s });
  }
  const s = Math.sqrt(1 + r(2, 2) - r(0, 0) - r(1, 1)) * 2;
  return normalizeQuat({ w: (r(1, 0) - r(0, 1)) / s, x: (r(0, 2) + r(2, 0)) / s, y: (r(1, 2) + r(2, 1)) / s, z: s / 4 });
}

/**
 * `CAMERA::SetViewMatrix( aViewport.matrix )` (common/gal/3d/camera.cpp; `APPEARANCE_CONTROLS_3D::doApplyViewport`) on the studio's camera `current`: the trackball's
 * rotation is the matrix's less the auxiliary one the camera has (`SetRotationMatrix`: `aRotation * glm::inverse( m_rotationMatrixAux )`), the look-at point stays, the
 * camera goes where the matrix puts the look-at point (`m_camera_pos`), and the zoom is that point's depth over the home distance (`lookat.z / m_camera_pos_init.z`), held
 * in `[minZoom, maxZoom]` with the depth moved to match. `spanMm` is the board's span [`kicadViewMatrix`] scales by.
 */
export function viewFromKicadMatrix(matrix: readonly number[], spanMm: number, current: CameraView, minZoom = 0.02, maxZoom = 2): CameraView {
  const k = (RANGE_SCALE_3D * HOME_ZOOM_HACK) / Math.max(spanMm, 1e-9);
  const d2r = Math.PI / 180;
  const aux = multiplyQuat(multiplyQuat(quatFromAxisAngle(vec3(1, 0, 0), current.auxDeg.x * d2r), quatFromAxisAngle(vec3(0, 1, 0), current.auxDeg.y * d2r)), quatFromAxisAngle(vec3(0, 0, 1), current.auxDeg.z * d2r));
  const total = quatOfColumns(matrix);
  const quat = normalizeQuat(multiplyQuat(total, { x: -aux.x, y: -aux.y, z: -aux.z, w: aux.w }));
  // `glm::vec4 lookat = aViewMatrix * glm::vec4( m_lookat_pos, 1.0f )`, in KiCad's 3D units
  const look = vec3(current.lookAt.x * k, -current.lookAt.z * k, current.lookAt.y * k);
  const rl = rotateVecByQuat(look, total);
  const p = { x: rl.x + matrix[12]!, y: rl.y + matrix[13]!, z: rl.z + matrix[14]! };
  // `m_camera_pos_init` is ( 0, 0, -initial distance ): the studio's 1.25 spans (camera3d.ts DISTANCE_TO_BOARD_SPAN_RATIO)
  const home = 1.25 * spanMm * k;
  let zoom = -p.z / home;
  if (zoom > maxZoom) zoom = maxZoom;
  else if (zoom < minZoom) zoom = minZoom;
  return { pos: { x: p.x / k, y: p.y / k, z: -zoom * home / k }, lookAt: { ...current.lookAt }, auxDeg: { ...current.auxDeg }, zoom, quat };
}

/** `onViewportChanged`'s "Save viewport...": a new name goes before the separator (the end of the list), an existing one gets the camera as it is now. */
export function saveViewport(viewports: readonly Viewport3D[], name: string, camera: CameraView, spanMm?: number): Viewport3D[] {
  const vp: Viewport3D = spanMm !== undefined ? { name, camera, matrix: kicadViewMatrix(camera, spanMm) } : { name, camera };
  const at = viewports.findIndex((v) => v.name === name);
  if (at < 0) return [...viewports, vp];
  return viewports.map((v, i) => (i === at ? vp : v));
}

/** "Delete viewport...": the one chosen taken out. */
export function deleteViewport(viewports: readonly Viewport3D[], name: string): Viewport3D[] {
  return viewports.filter((v) => v.name !== name);
}

const isVec = (v: unknown): v is { x: number; y: number; z: number } => typeof v === "object" && v !== null && ["x", "y", "z"].every((k) => typeof (v as Record<string, unknown>)[k] === "number");

/** A viewport read back from `appearance.json`, or null when it is not one (`PARAM_VIEWPORT3D::jsonToViewports`: a name and a matrix). */
export function viewportFromJson(j: unknown): Viewport3D | null {
  if (typeof j !== "object" || j === null) return null;
  const o = j as Record<string, unknown>;
  if (typeof o.name !== "string") return null;
  const matrix = Array.isArray(o.matrix) && o.matrix.length === 16 && o.matrix.every((x) => typeof x === "number") ? { matrix: [...(o.matrix as number[])] } : {};
  const c = o.camera as Record<string, unknown> | undefined;
  const q = (c?.quat ?? null) as Record<string, unknown> | null;
  const camera = c && isVec(c.pos) && isVec(c.lookAt) && isVec(c.auxDeg) && typeof c.zoom === "number" && q && ["x", "y", "z", "w"].every((k) => typeof q[k] === "number") ? { pos: { ...c.pos }, lookAt: { ...c.lookAt }, auxDeg: { ...c.auxDeg }, zoom: c.zoom, quat: { x: q.x as number, y: q.y as number, z: q.z as number, w: q.w as number } } : null;
  // a KiCad project's viewport (UI-LEFT-3 D3-5) has its matrix alone
  if (!camera) return "matrix" in matrix ? { name: o.name, ...matrix } : null;
  return { name: o.name, camera, ...matrix };
}

// ----------------------------------------------------------------------------------------------------------------------------- preferences

/** `ANTIALIASING_MODE`: the Preferences' "Anti-aliasing" choice. */
export type AntialiasingMode = "none" | "2x" | "4x" | "8x";

/** `MATERIAL_MODE` (`render.material_mode`, `PARAM_ENUM` NORMAL .. CAD_MODE): "Material properties:" Realistic, Solid colors, CAD colors. */
export type MaterialMode = "realistic" | "solid" | "cad";

/**
 * The 3D viewer's settings the studio's viewer honours (`EDA_3D_VIEWER_SETTINGS`), with KiCad's defaults: Preferences > 3D Viewer > General
 * (`PANEL_3D_DISPLAY_OPTIONS`) and > Realtime Rendering (`PANEL_3D_OPENGL_OPTIONS`). The rows these share with the Appearance manager (bounding boxes, zones)
 * are the rows themselves.
 */
export interface Viewer3DPrefs {
  /** `camera.rotation_increment` ("Rotation increment:", 0 .. 359 deg, default 10): what the Rotate commands turn by. */
  rotationIncrementDeg: number;
  /** `camera.animation_enabled` ("Redraw while moving", default on): the view moves, or jumps. */
  animationEnabled: boolean;
  /** `camera.moving_speed_multiplier` ("Redraw speed:", 1 .. 5, default 3). */
  movingSpeedMultiplier: number;
  /** `render.opengl_copper_thickness` ("Show copper and tech layers thickness (slow)", default off): copper and paste have their thickness, else they are flat. */
  copperThickness: boolean;
  /** `render.opengl_highlight_on_rollover` ("Highlight items on rollover", default on). */
  highlightOnRollover: boolean;
  /** `render.opengl_AA_mode` ("Anti-aliasing:", default 8x): "3D-Viewer must be closed and re-opened to apply this setting". */
  antialiasing: AntialiasingMode;
  /** `render.opengl_selection_color` ("Selection color:", default green), `#rrggbb`. */
  selectionColor: string;
  /** `render.clip_silk_on_via_annulus` ("Clip silkscreen at via annuli", default off): the silkscreen is culled by the vias' and plated holes' rings, not their holes. */
  clipSilkOnViaAnnuli: boolean;
  /** `render.subtract_mask_from_silk` ("Clip silkscreen at solder mask edges", default off): the silkscreen is culled by the mask's openings. */
  subtractMaskFromSilk: boolean;
  /** `render.differentiate_plated_copper` ("Use bare copper color for unplated copper (slow)", default off); `DifferentiatePlatedCopper()` is off while "Use PCB editor copper colors" is on. */
  differentiatePlatedCopper: boolean;
  /** `render.material_mode` ("Material properties:", default Realistic): how the models' materials are drawn. */
  materialMode: MaterialMode;
  /** `render.opengl_AA_disableOnMove` ("Disable anti-aliasing" while moving, default off). */
  aaDisableOnMove: boolean;
  /** `render.opengl_thickness_disableOnMove` ("Disable thickness" while moving, default off). */
  thicknessDisableOnMove: boolean;
  /** `render.opengl_vias_disableOnMove` ("Disable uVia holes" while moving, default off). */
  microviasDisableOnMove: boolean;
  /** `render.opengl_holes_disableOnMove` ("Disable all plated holes" while moving, default off). */
  holesDisableOnMove: boolean;
  /** `render.grid_type` (View > 3D Grid: `EDA_3D_CONTROLLER::On3DGridSelection`, default NONE): kicad-port/grid3d.ts. */
  gridType: Grid3DType;
}

/** `EDA_3D_VIEWER_SETTINGS::RENDER_SETTINGS::DifferentiatePlatedCopper` (OpenGL engine): never with the board editor's copper colours. */
export function differentiatePlatedCopper(prefs: Pick<Viewer3DPrefs, "differentiatePlatedCopper">, useBoardEditorCopperColors: boolean): boolean {
  if (useBoardEditorCopperColors) return false;
  return prefs.differentiatePlatedCopper;
}

export const DEFAULT_VIEWER3D_PREFS: Viewer3DPrefs = {
  rotationIncrementDeg: 10,
  animationEnabled: true,
  movingSpeedMultiplier: 3,
  copperThickness: false,
  highlightOnRollover: true,
  antialiasing: "8x",
  selectionColor: "#00ff00",
  clipSilkOnViaAnnuli: false,
  subtractMaskFromSilk: false,
  differentiatePlatedCopper: false,
  materialMode: "realistic",
  aaDisableOnMove: false,
  thicknessDisableOnMove: false,
  microviasDisableOnMove: false,
  holesDisableOnMove: false,
  gridType: "none",
};

const MATERIAL_MODES: readonly MaterialMode[] = ["realistic", "solid", "cad"];

const AA: readonly AntialiasingMode[] = ["none", "2x", "4x", "8x"];

/** Settings read back from storage, each in its range or its default (`PARAM`'s min and max: the rotation 0 .. 359, the speed 1 .. 5). */
export function sanitizeViewer3DPrefs(raw: unknown): Viewer3DPrefs {
  const o = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const d = DEFAULT_VIEWER3D_PREFS;
  const num = (v: unknown, lo: number, hi: number, def: number) => (typeof v === "number" && Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : def);
  const bool = (v: unknown, def: boolean) => (typeof v === "boolean" ? v : def);
  return {
    rotationIncrementDeg: num(o.rotationIncrementDeg, 0, 359, d.rotationIncrementDeg),
    animationEnabled: bool(o.animationEnabled, d.animationEnabled),
    movingSpeedMultiplier: Math.round(num(o.movingSpeedMultiplier, 1, 5, d.movingSpeedMultiplier)),
    copperThickness: bool(o.copperThickness, d.copperThickness),
    highlightOnRollover: bool(o.highlightOnRollover, d.highlightOnRollover),
    antialiasing: AA.includes(o.antialiasing as AntialiasingMode) ? (o.antialiasing as AntialiasingMode) : d.antialiasing,
    selectionColor: typeof o.selectionColor === "string" && parseColor(o.selectionColor) ? o.selectionColor.slice(0, 7) : d.selectionColor,
    clipSilkOnViaAnnuli: bool(o.clipSilkOnViaAnnuli, d.clipSilkOnViaAnnuli),
    subtractMaskFromSilk: bool(o.subtractMaskFromSilk, d.subtractMaskFromSilk),
    differentiatePlatedCopper: bool(o.differentiatePlatedCopper, d.differentiatePlatedCopper),
    materialMode: MATERIAL_MODES.includes(o.materialMode as MaterialMode) ? (o.materialMode as MaterialMode) : d.materialMode,
    gridType: GRID3D_TYPES.includes(o.gridType as Grid3DType) ? (o.gridType as Grid3DType) : d.gridType,
    aaDisableOnMove: bool(o.aaDisableOnMove, d.aaDisableOnMove),
    thicknessDisableOnMove: bool(o.thicknessDisableOnMove, d.thicknessDisableOnMove),
    microviasDisableOnMove: bool(o.microviasDisableOnMove, d.microviasDisableOnMove),
    holesDisableOnMove: bool(o.holesDisableOnMove, d.holesDisableOnMove),
  };
}

/** The board layer a row names, for the PCB editor's visibility ([`followPcbVisibility`]): `rowLayer`'s, the paste's F.Paste, the adhesive's F.Adhes. */
export function followLayerOfRow(id: string): string | null {
  if (id === "solder_paste") return "F.Paste";
  if (id === "adhesive") return "F.Adhes";
  return rowLayer(id);
}
