// Where a footprint's 3D model goes: KiCad's placement of a `(model ...)` on its footprint, ported from the 3D viewer's OpenGL renderer
// (3d-viewer/3d_rendering/opengl/render_3d_opengl.cpp, `RENDER_3D_OPENGL::get3dModelsFromFootprint`) and the board adapter's layer heights
// (3d-viewer/3d_canvas/board_adapter.cpp, `GetFootprintZPos`). Pure matrices, no three.js (see math3d.ts's header for why kicad-port/ never imports
// it): the viewer hands the 16 numbers to `THREE.Matrix4.fromArray`, which reads them in the same column-major order.
//
// ------------------------------------------------------------------------------------------------------------------- KiCad's chain
//
//   get3dModelsFromFootprint builds, for a model of a footprint at (x, y) with orientation `a` on side `s`:
//
//     fp    = T(x, -y, zpos) * Rz(a) * [flipped: Ry(pi) * Rz(pi)] * S(units)
//     model = T(offset) * Rz(-rotate.z) * Ry(-rotate.y) * Rx(-rotate.x) * S(scale)
//     world = fp * model                                  (a vertex v of the model: world * v)
//
//   in KiCad's 3D frame: x to the right, y UP (the board's y is negated: a footprint at board y 20 is at y -20), z up out of the board. `offset` is
//   millimetres, `rotate` degrees, `units` the model's own unit in 3D units (a VRML model is in 0.1 inch: 2.54 mm; this module leaves it to the caller, who
//   scales the loaded model once). `zpos` is the footprint's mounting height: the top of the front copper (`m_layerZcoordTop[F_Paste]`) for a footprint on
//   the front, plus the paste layer's 0.04 mm, and the back copper's face (`m_layerZcoordBottom[B_Paste]`) on the back; the thicknesses are the stackup's
//   (`stackOfBoard`).
//
// ------------------------------------------------------------------------------------------------- this studio's frame, and what differs
//
//   The scene (components/viewer3d/scene.ts) has world X = board x, world Y = up out of the top, world Z = board y (y down, unflipped): KiCad's frame turned
//   by `SCENE_FROM_KICAD`, a proper rotation (-90 degrees about x), so no handedness changes.
//
//   The studio's footprint frame is the part seen from the top, and a bottom-side instance mirrors x before it rotates (`footprint::to_board`):
//   `orientation` is the studio's `rot`, clockwise on screen (y down), which is `Rz(-rot)` in the y-up frame. Where KiCad stores a flipped footprint
//   mirrored in y and turns its model by `Ry(pi) Rz(pi)` = diag(1, -1, -1), the studio's flip of the model is the mirror in x that matches its pads,
//   diag(-1, 1, -1) -- a half turn about z apart. The model's own placement is written for the library footprint, so it is what `Footprint::models3d` holds,
//   and only the footprint's flip differs; `crates/model/src/footprint.rs` `Model3d::flipped_frame` is the half turn the KiCad file needs in its place.
//   A top footprint has no difference at all.

/** Column-major 4x4, the order `THREE.Matrix4.elements` and `fromArray` use. */
export type Mat4 = number[];

export type Vec3Tuple = [number, number, number];

const DEG = Math.PI / 180;

export function mat4Identity(): Mat4 {
  return [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
}

/** `a * b`: apply b first, then a (the order of `THREE.Matrix4.multiplyMatrices(a, b)`). */
export function mat4Mul(a: readonly number[], b: readonly number[]): Mat4 {
  const out = new Array<number>(16).fill(0);
  for (let col = 0; col < 4; col++) {
    for (let row = 0; row < 4; row++) {
      let sum = 0;
      for (let k = 0; k < 4; k++) sum += a[k * 4 + row]! * b[col * 4 + k]!;
      out[col * 4 + row] = sum;
    }
  }
  return out;
}

export function mat4Translation(x: number, y: number, z: number): Mat4 {
  return [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, x, y, z, 1];
}

export function mat4Scaling(x: number, y: number, z: number): Mat4 {
  return [x, 0, 0, 0, 0, y, 0, 0, 0, 0, z, 0, 0, 0, 0, 1];
}

/** A turn of `rad` radians, counter-clockwise looking down the axis towards the origin (the right-hand rule), as glm::rotate. */
export function mat4RotationX(rad: number): Mat4 {
  const c = Math.cos(rad);
  const s = Math.sin(rad);
  return [1, 0, 0, 0, 0, c, s, 0, 0, -s, c, 0, 0, 0, 0, 1];
}

export function mat4RotationY(rad: number): Mat4 {
  const c = Math.cos(rad);
  const s = Math.sin(rad);
  return [c, 0, -s, 0, 0, 1, 0, 0, s, 0, c, 0, 0, 0, 0, 1];
}

export function mat4RotationZ(rad: number): Mat4 {
  const c = Math.cos(rad);
  const s = Math.sin(rad);
  return [c, s, 0, 0, -s, c, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
}

export function transformPoint(m: readonly number[], p: Vec3Tuple): Vec3Tuple {
  const [x, y, z] = p;
  return [m[0]! * x + m[4]! * y + m[8]! * z + m[12]!, m[1]! * x + m[5]! * y + m[9]! * z + m[13]!, m[2]! * x + m[6]! * y + m[10]! * z + m[14]!];
}

/** KiCad's 3D frame (x, y up, z up) to the scene's (x, z, -y): -90 degrees about x. */
export const SCENE_FROM_KICAD: Readonly<Mat4> = Object.freeze(mat4RotationX(-90 * DEG));

/** A VRML model's own unit is 0.1 inch: its coordinates times this are millimetres (`KiCad: S3D plugin, UNITS3D_TO_UNITSPCB`). */
export const VRML_UNIT_MM = 2.54;

/** One `(model ...)` of a footprint, as `/api/state`'s `parts[].models` carries it (crates/cli/src/model3d_api.rs `part_json`). */
export interface Model3dEntry {
  /** The path the footprint file writes: `${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.step`. */
  name: string;
  /** Millimetres, in the model's own frame. */
  offset: Vec3Tuple;
  scale: Vec3Tuple;
  /** Degrees. */
  rotate: Vec3Tuple;
  opacity: number;
  show: boolean;
}

/** KiCad's own default placement (`FP_3DMODEL`'s constructor): offset 0, scale 1, rotation 0, opaque, shown. */
export function plainModel(name: string): Model3dEntry {
  return { name, offset: [0, 0, 0], scale: [1, 1, 1], rotate: [0, 0, 0], opacity: 1, show: true };
}

/** `T(offset) * Rz(-rz) * Ry(-ry) * Rx(-rx) * S(scale)`: how a `(model ...)` sits on its footprint (render_3d_opengl.cpp, the `mtx` of get3dModelsFromFootprint). */
export function modelPlacementMatrix(m: Pick<Model3dEntry, "offset" | "scale" | "rotate">): Mat4 {
  let mtx = mat4Translation(m.offset[0], m.offset[1], m.offset[2]);
  mtx = mat4Mul(mtx, mat4RotationZ(-m.rotate[2] * DEG));
  mtx = mat4Mul(mtx, mat4RotationY(-m.rotate[1] * DEG));
  mtx = mat4Mul(mtx, mat4RotationX(-m.rotate[0] * DEG));
  return mat4Mul(mtx, mat4Scaling(m.scale[0], m.scale[1], m.scale[2]));
}

/**
 * The board's vertical build-up, in millimetres, as `BOARD_ADAPTER::InitSettings` (3d-viewer/3d_canvas/board_adapter.cpp) keeps it: the body
 * (`m_boardBodyThickness3DU`: the dielectrics and the inner copper), the outer copper (`m_frontCopperThickness3DU`, `m_backCopperThickness3DU`) and the
 * solder masks (`m_frontMaskThickness3DU`, `m_backMaskThickness3DU`). The back copper and the masks default to the front copper and the technical layer
 * thickness when absent.
 */
export interface Stack {
  boardThicknessMm: number;
  copperMm: number;
  copperBackMm?: number;
  maskFrontMm?: number;
  maskBackMm?: number;
}

/** InitSettings' values before the stackup is read (`DEFAULT_BOARD_THICKNESS`, `DEFAULT_COPPER_THICKNESS`, `DEFAULT_TECH_LAYER_THICKNESS`): a board with no stackup at all, e.g. the footprint preview's dummy board. */
export const DEFAULT_STACK: Readonly<Stack> = Object.freeze({ boardThicknessMm: 1.6, copperMm: 0.035, copperBackMm: 0.035, maskFrontMm: 0.025, maskBackMm: 0.025 });

/** board_adapter.cpp `SOLDERPASTE_LAYER_THICKNESS`: `pcbIUScale.mmToIU( 0.04 )`. */
export const SOLDERPASTE_LAYER_MM = 0.04;

/** `BOARD_STACKUP_ITEM::GetCopperDefaultThickness` (35 um) and `GetMaskDefaultThickness` (10 um), the stackup items' own defaults. */
const STACKUP_COPPER_NM = 35_000;
const STACKUP_MASK_NM = 10_000;
/** InitSettings: "The copper thickness must be > 0 to avoid draw issues ... We use a minimal arbitrary value = 1 micrometer here" (the mask's too). */
const MIN_LAYER_NM = 1_000;

/** The parts of the board's rules the build-up is read from (`BoardState["board_rules"]`). */
export interface StackRules {
  stackup?: { layers: ReadonlyArray<{ name: string; material?: string | null; thickness_mm: number | null; kind?: string }> } | null;
  board_thickness_nm?: number;
  copper_layers?: number;
}

/**
 * `BOARD_ADAPTER::InitSettings`' stackup loop: the dielectrics (every sublayer; ours holds them summed) and the enabled inner copper make the body, the
 * F.Cu / B.Cu items give the outer copper and the F.Mask / B.Mask items the masks (each at least 1 um). A layer with no thickness keeps its item's default
 * (`BOARD_STACKUP_ITEM`'s ctor: copper 35 um, mask 10 um, a dielectric none).
 *
 * A KiCad board always has a stackup: with no `(stackup ..)` block, `PCB_IO_KICAD_SEXPR_PARSER::parseSetup` builds
 * `BOARD_STACKUP::BuildDefaultStackupList( &bds, copperCount )`, whose dielectrics share `GetBoardThickness() - 35 um * copper - 10 um * masks`
 * (integer division by `copper - 1`). That is what a design with no stackup gets here, from `board_thickness_nm` (default 1.6 mm, BOARD_DESIGN_SETTINGS)
 * and `copper_layers` (default 2), with both masks enabled (ours has no way to disable F.Mask/B.Mask).
 */
export function stackOfBoard(rules: StackRules | null | undefined): Stack {
  const layers = rules?.stackup?.layers ?? [];
  const nm = (mm: number | null | undefined, fallback: number) => (mm === null || mm === undefined || !Number.isFinite(mm) ? fallback : Math.round(mm * 1e6));
  if (layers.length === 0) {
    const copper = Math.max(1, Math.round(rules?.copper_layers ?? 2));
    const thickness = rules?.board_thickness_nm ?? 1_600_000;
    const diel = Math.trunc((thickness - STACKUP_COPPER_NM * copper - STACKUP_MASK_NM * 2) / Math.max(1, copper - 1));
    const body = diel * Math.max(0, copper - 1) + STACKUP_COPPER_NM * Math.max(0, copper - 2);
    return { boardThicknessMm: body / 1e6, copperMm: STACKUP_COPPER_NM / 1e6, copperBackMm: STACKUP_COPPER_NM / 1e6, maskFrontMm: STACKUP_MASK_NM / 1e6, maskBackMm: STACKUP_MASK_NM / 1e6 };
  }
  const out: Stack = { ...DEFAULT_STACK };
  let body = 0;
  for (const item of layers) {
    const kind = (item.kind ?? "").toLowerCase();
    const isCopper = kind === "copper" || (kind === "" && /\.Cu$/.test(item.name));
    if (kind === "core" || kind === "prepreg" || kind === "dielectric") {
      body += nm(item.thickness_mm, 0);
    } else if (isCopper) {
      const t = Math.max(nm(item.thickness_mm, STACKUP_COPPER_NM), MIN_LAYER_NM);
      if (item.name === "F.Cu") out.copperMm = t / 1e6;
      else if (item.name === "B.Cu") out.copperBackMm = t / 1e6;
      else body += t;
    } else if (kind.includes("solder mask") || kind === "soldermask") {
      const t = Math.max(nm(item.thickness_mm, STACKUP_MASK_NM), MIN_LAYER_NM);
      if (item.name === "F.Mask") out.maskFrontMm = t / 1e6;
      else if (item.name === "B.Mask") out.maskBackMm = t / 1e6;
    }
  }
  out.boardThicknessMm = body / 1e6;
  return out;
}

/**
 * The 3D model preview's dummy board (panel_preview_3d_model.cpp's ctor): the footprint editor's board thickness (BOARD_DESIGN_SETTINGS' 1.6 mm), enabled
 * layers `LSET::FrontMask() | LSET::BackMask()` (so `m_copperLayerCount` is 0, `SetEnabledLayers`) and `BuildDefaultStackupList( &dummy_bds, 2 )`: one
 * dielectric of the thickness less the two 10 um masks (no copper is subtracted at a count of 0), F.Cu and B.Cu at 35 um.
 */
export function previewStack(boardThicknessNm = 1_600_000): Stack {
  const mm = (nm: number) => nm / 1e6;
  return { boardThicknessMm: mm(boardThicknessNm - STACKUP_MASK_NM * 2), copperMm: mm(STACKUP_COPPER_NM), copperBackMm: mm(STACKUP_COPPER_NM), maskFrontMm: mm(STACKUP_MASK_NM), maskBackMm: mm(STACKUP_MASK_NM) };
}

/**
 * `BOARD_ADAPTER::GetFootprintZPos`: on the front, `m_layerZcoordTop[F_Paste]`, the front copper's top plus `SOLDERPASTE_LAYER_THICKNESS`; on the back,
 * `m_layerZcoordBottom[B_Paste]`, which InitSettings sets to the back copper's top itself (negative). KiCad's two sides differ by the paste, and so do ours.
 */
export function footprintZ(stack: Stack, bottom: boolean): number {
  const half = stack.boardThicknessMm / 2;
  return bottom ? -(half + (stack.copperBackMm ?? stack.copperMm)) : half + stack.copperMm + SOLDERPASTE_LAYER_MM;
}

/** Where a footprint is on the board: its position in millimetres (board coordinates, y down), the studio's rotation in degrees (clockwise on screen) and its side. */
export interface FootprintPlacement {
  xMm: number;
  yMm: number;
  rotDeg: number;
  bottom: boolean;
}

/** `T(x, -y, zpos) * Rz(-rot) * [bottom: diag(-1, 1, -1)]`, in KiCad's 3D frame (see the header for the studio's side of the flip). */
export function footprintMatrixKicad(fp: FootprintPlacement, stack: Stack): Mat4 {
  let m = mat4Translation(fp.xMm, -fp.yMm, footprintZ(stack, fp.bottom));
  m = mat4Mul(m, mat4RotationZ(-fp.rotDeg * DEG));
  if (fp.bottom) m = mat4Mul(m, mat4Scaling(-1, 1, -1));
  return m;
}

/**
 * The matrix that puts a model's vertices (millimetres: a loaded VRML scaled by `VRML_UNIT_MM`) where the scene wants them: the scene's frame from KiCad's, times
 * the footprint's placement, times the model's own.
 */
export function modelWorldMatrix(fp: FootprintPlacement, model: Pick<Model3dEntry, "offset" | "scale" | "rotate">, stack: Stack = DEFAULT_STACK): Mat4 {
  return mat4Mul(mat4Mul(SCENE_FROM_KICAD, footprintMatrixKicad(fp, stack)), modelPlacementMatrix(model));
}

/**
 * KiCad's own chain for a footprint stored the way a `.kicad_pcb` stores it, `T(x, -y, zpos) * Rz(orient) * [flipped: Ry(pi) Rz(pi)] * model`, scene-framed.
 * `orient` is KiCad's, counter-clockwise on screen. The viewer does not use this; it is what `kicad-cli pcb export glb` does with the file, kept here so a test can
 * hold the studio's chain against it (crates/kicad's writer turns a bottom footprint's model by `Model3d::flipped_frame` so the two agree).
 */
export function modelWorldMatrixKicadFile(file: { xMm: number; yMm: number; orientDeg: number; flipped: boolean }, model: Pick<Model3dEntry, "offset" | "scale" | "rotate">, stack: Stack = DEFAULT_STACK): Mat4 {
  let m = mat4Translation(file.xMm, -file.yMm, footprintZ(stack, file.flipped));
  m = mat4Mul(m, mat4RotationZ(file.orientDeg * DEG));
  if (file.flipped) {
    m = mat4Mul(m, mat4RotationY(Math.PI));
    m = mat4Mul(m, mat4RotationZ(Math.PI));
  }
  return mat4Mul(mat4Mul(SCENE_FROM_KICAD, m), modelPlacementMatrix(model));
}

/** `Model3d::flipped_frame` (crates/model/src/footprint.rs): the half turn about z a bottom-side footprint's model carries in a KiCad file. */
export function flippedFrame(m: Pick<Model3dEntry, "offset" | "rotate">): { offset: Vec3Tuple; rotate: Vec3Tuple } {
  let z = (m.rotate[2] - 180) % 360;
  if (z <= -180) z += 360;
  if (z > 180) z -= 360;
  return { offset: [-m.offset[0], -m.offset[1], m.offset[2]], rotate: [m.rotate[0], m.rotate[1], z] };
}

/** How KiCad's 3D viewer sorts a footprint's models (`BOARD_ADAPTER::IsFootprintShown`): a through-hole, an SMD or a virtual model. */
export type ModelKind = "smd" | "tht" | "virtual";

/** Whether the layer tree's through-hole / SMD / virtual rows let a model of `kind` show (`IsFootprintShown`'s last three lines). */
export function kindShown(kind: ModelKind, shown: { tht: boolean; smd: boolean; virtual: boolean }): boolean {
  return kind === "smd" ? shown.smd : kind === "tht" ? shown.tht : shown.virtual;
}

/** `BOARD_ADAPTER::IsFootprintShown`, in its order: a footprint excluded from the position files shows only with "Models not in POS File" on
 * (`show_footprints_not_in_posfile`), one marked Do Not Populate only with "Models marked DNP" on (`show_footprints_dnp`), and then its kind's row decides. */
export function footprintShown(part: { kind: ModelKind; dnp?: boolean; notInPos?: boolean }, shown: { tht: boolean; smd: boolean; virtual: boolean; notInPos: boolean; dnp: boolean }): boolean {
  if (part.notInPos && !shown.notInPos) return false;
  if (part.dnp && !shown.dnp) return false;
  return kindShown(part.kind, shown);
}

/** The kind of a part the backend did not classify: through-hole when it has a through-hole pad, else SMD (the studio's footprints carry no `(attr ...)`). */
export function inferKind(pads: ReadonlyArray<{ th: boolean }> | undefined): ModelKind {
  return (pads ?? []).some((p) => p.th) ? "tht" : "smd";
}

/** The URL a model is fetched from: `GET /api/3dmodel?name=` (crates/cli/src/model3d_api.rs), the name percent-encoded. */
export function modelUrl(name: string): string {
  return `/api/3dmodel?name=${encodeURIComponent(name)}`;
}

/**
 * The models the placed parts show, the one most parts use first (ties in the order the parts first use them). The page loads them three at a time, parsing each
 * on the main thread, so the packages most of the board is made of are the first drawn while the rest are still coming in.
 */
export function modelsByUse(parts: ReadonlyArray<{ placed?: boolean; models?: ReadonlyArray<Pick<Model3dEntry, "name" | "show">> }>): string[] {
  const uses = new Map<string, number>();
  for (const part of parts) {
    if (!part.placed) continue;
    for (const m of part.models ?? []) if (m.show) uses.set(m.name, (uses.get(m.name) ?? 0) + 1);
  }
  // Array.prototype.sort is stable: equal counts stay in first-use order.
  return [...uses.keys()].sort((a, b) => uses.get(b)! - uses.get(a)!);
}

/** Whether two model lists place and show the same models (the viewer rebuilds a part's models only when they differ). */
export function sameModels(a: readonly Model3dEntry[] | undefined, b: readonly Model3dEntry[] | undefined): boolean {
  if (a === b) return true;
  if (!a || !b || a.length !== b.length) return false;
  const same3 = (x: Vec3Tuple, y: Vec3Tuple) => x[0] === y[0] && x[1] === y[1] && x[2] === y[2];
  return a.every((m, i) => {
    const n = b[i]!;
    return m.name === n.name && same3(m.offset, n.offset) && same3(m.scale, n.scale) && same3(m.rotate, n.rotate) && m.opacity === n.opacity && m.show === n.show;
  });
}
