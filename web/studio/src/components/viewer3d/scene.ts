// Pure Three.js geometry builders for the KiCad-style 3D PCB viewer.
// No React, no DOM access beyond THREE's own APIs -- Viewer3D.tsx owns
// the renderer/scene/camera lifecycle and just calls into here to
// (re)build the "board group" whenever `state.board` changes, and to get
// the board outline's own bounds (boardOutlineBounds, below) to re-home
// the camera (kicad-port/camera3d.ts -- NOT this file; the actual camera
// state/math is a faithful port of KiCad's own CAMERA/TRACK_BALL classes,
// dependency-free on purpose, see that file's header comment).
//
// -------------------------------------------------------------- units
//
// All board data arrives in micrometres (see api/types.ts). Three.js
// units here are millimetres (`mm()` below) -- using raw micrometres as
// Three.js units makes the camera/near/far/OrbitControls math unusable
// (distances in the hundreds of thousands, floating-point precision
// problems, etc.), so every builder in this file converts up front.
//
// -------------------------------------------------------- coordinates
//
// The board/API data has no inherent 3rd dimension, only a flat XY
// board plane. This file's own (only sensible) choice of axes:
//
//   world X = board X, in mm (board nm / 1e6), unflipped.
//   world Z = board Y, in mm (board nm / 1e6), unflipped.
//   world Y = board thickness, "up" out of the top (F.Cu) side.
//
// world Y = thickness is deliberate, not arbitrary: it matches Three.js's
// own default "up" axis (camera.up, OrbitControls' orbit axis), and it
// means THREE.CylinderGeometry's default orientation (axis along Y) and
// THREE.BoxGeometry's `height` argument (its own Y extent) are *already*
// the stack-up direction -- vias and pads need zero extra rotation to
// "stand up" through the board.
//
// world Z = board Y unflipped (not mirrored) matches
// components/canvas/view.ts's `worldToScreen` (board Y maps straight
// through to screen Y with a positive scale) and
// crates/model/src/footprint.rs's own doc comment ("+y down"): board Y
// already behaves like a screen-space axis, so this keeps the 3D scene's
// X/Z layout an undistorted copy of the 2D canvas's X/Y layout instead
// of introducing a second, differently-mirrored convention.
//
// Every board-space point this file consumes (`board.outline`,
// `Pad.x/y`, `Part.courtyard`, `Track.pts`, `Via.x/y`) is *already* in
// this same board-global micrometre space -- confirmed by reading
// crates/model/src/footprint.rs's `to_board`/`placed_pads`/
// `placed_courtyard` (not just api/types.ts's comments: `CourtyardBox`'s
// own doc comment there, "footprint-local", is stale for this call path
// -- `placed_courtyard` builds its box straight from `fp.at`, the
// footprint's already-placed board position, so the JSON's `courtyard`
// is board-global, same as `outline`). No per-part rotation/mirroring
// needs to be re-applied here -- the backend already baked `part.rot`
// and the bottom-side mirror into `pads[].x/y` and `courtyard`.
import * as THREE from "three";
import type { BoardState, FillZone, Part, Side } from "../../api/types";
import { buildTextBoxMesh } from "./pcbItems3d";
import { layerColor } from "../canvas/layers";
import { THEME_COLORS, userRowOfLayer, type Rgba } from "../../kicad-port/appearance3d";
import { boardOutlinePolygons } from "../../kicad-port/pcbOutline";
import { fallbackBodyHeightMm, partBodyBoxNm } from "../../kicad-port/partBody";
import { DEFAULT_STACK, stackOfBoard, type Stack } from "../../kicad-port/model3d";
import { MAX_ERROR_NM, arcToSegmentCount, circlePolygon, nestContours, type Contour, type Pt } from "../../kicad-port/padShape3d";
import { boardMaterial3d, copperLayerMaterial3d, glShininess, innerLayersDrawn, maskMaterial3d, nonPlatedCopperMaterial3d, pasteMaterial3d, plasticMaterial3d, silkMaterial3d, type Material3d, type Rgb } from "../../kicad-port/material3d";
import { buildLayers3d, makeJob, padHoleContour, type Cylinder, type Layers3d, type PolyJob, type Prim, type ZRef } from "../../kicad-port/layers3d";
import { fpTextBoxes } from "../../kicad-port/fpBoardItems";
import { readyPaths } from "../../kicad-port/barcodeShapes";
import { Culler, MASK_BITS, contoursToShapes, listShapes, maskOpenings, primShapes, type JobResults, type Subtract } from "./cull3d";
import { cloneLit, kicadLit, scaledColor } from "./glLighting";

// ---------------------------------------------------------------------
// Units
// ---------------------------------------------------------------------

/** Micrometres -> millimetres (this scene's Three.js unit). */
function mm(um: number): number {
  return um / 1e6;
}

// ---------------------------------------------------------------------
// Stack-up (all mm)
// ---------------------------------------------------------------------

/**
 * The board's vertical build-up for the build in progress: the body (`m_boardBodyThickness3DU`), the outer copper and the solder masks, from the
 * board's stackup as `BOARD_ADAPTER::InitSettings` reads it (kicad-port/model3d.ts `stackOfBoard`). Set once at the start of every build, which is
 * synchronous, like `palette` below.
 */
let stack: Stack = DEFAULT_STACK;
/** Half the body: the outer copper's inner face (`m_layerZcoordBottom[F_Cu]` / `[B_Cu]`). */
const halfThickness = () => stack.boardThicknessMm / 2;
/** The outer copper's thickness on `side` (`m_frontCopperThickness3DU` / `m_backCopperThickness3DU`). */
const copperMm = (side: Side) => (side === "top" ? stack.copperMm : stack.copperBackMm ?? stack.copperMm);
/** The solder mask's thickness on `side` (`m_frontMaskThickness3DU` / `m_backMaskThickness3DU`). */
const maskMm = (side: Side) => (side === "top" ? stack.maskFrontMm : stack.maskBackMm) ?? MASK_MM;
/** The front copper, for the items that are not on one side (a track on an inner layer). */
const COPPER_MM = DEFAULT_STACK.copperMm;
/** board_adapter.cpp's DEFAULT_TECH_LAYER_THICKNESS (`pcbIUScale.mmToIU(0.025)`): the "technical layer" thickness (`m_nonCopperLayerThickness3DU`), and the solder mask's when the stack names none (a board always has one from its stackup, `stackOfBoard`). */
const MASK_MM = 0.025;
/** DEFAULT_COPPER_THICKNESS is 0.035mm already (COPPER_MM above matches); SOLDERPASTE_LAYER_THICKNESS is its own, thicker constant (`pcbIUScale.mmToIU(0.04)`) -- see addSolderPaste. */
const SOLDERPASTE_MM = 0.04;
/**
 * Part body height. THERE IS NO REAL PER-PART 3D MODEL DATA (the task
 * spec is explicit: "There are no 3D models yet") -- every placed part
 * renders as a plain extruded box of this fixed height. It is a
 * stand-in, not measured from anything.
 */
const PART_HEIGHT_MM = 2;

function outwardSign(side: Side): 1 | -1 {
  return side === "top" ? 1 : -1;
}

/** The board slab's own outer surface for `side` (+0.8mm / -0.8mm), independent of any copper/mask stack-up on top of it. */
function surfaceY(side: Side): number {
  return outwardSign(side) * halfThickness();
}

/** The solder mask's plane: on the outer copper's face (`InitSettings`: F.Mask from `zpos_copperTop_front` to it plus the mask's thickness), the middle of that. */
function maskCenterY(side: Side): number {
  return surfaceY(side) + outwardSign(side) * (copperMm(side) + maskMm(side) / 2);
}

/** Part bodies sit directly on the board surface (task spec: "top parts sit above +0.8mm ... extending further up"), not stacked on top of the copper/mask -- those are sub-0.05mm and not worth the extra offset the spec didn't ask for. */
function partBodyCenterY(side: Side, heightMm = PART_HEIGHT_MM): number {
  return surfaceY(side) + outwardSign(side) * (heightMm / 2);
}

/**
 * Y for a named copper layer, given the board's stack-up order
 * (`board.layers`, assumed top-to-bottom -- standard KiCad convention:
 * F.Cu first, B.Cu last). Used for both pads (always F.Cu/B.Cu, since
 * `Pad` has no layer field, only `Part.side`) and tracks (whatever
 * `Track.layer` says), so the two always land on the exact same plane
 * for a given named layer -- no separate "pads sit here, tracks sit
 * there" drift.
 */
function layerY(layerName: string, layerOrder: readonly string[]): number {
  const idx = layerOrder.indexOf(layerName);
  if (idx < 0) {
    // Unknown layer name (shouldn't happen for real board data): fall
    // back to whichever outer surface the name itself suggests.
    return layerName.toLowerCase().startsWith("b.") ? -halfThickness() : halfThickness();
  }
  if (layerOrder.length <= 1) return halfThickness();
  const t = idx / (layerOrder.length - 1); // 0 at the first (top) entry, 1 at the last (bottom)
  return halfThickness() - t * stack.boardThicknessMm;
}


// ---------------------------------------------------------------------
// Colors and materials. Every material below reads a real value out of colors.json (common/settings/builtin_color_themes.h's LAYER_3D_* keys: KiCad's own default
// 3D-viewer theme) through the palette, and is lit as KiCad's OpenGL view lights it (glLighting.ts).
// ---------------------------------------------------------------------

/** The placeholder body of a part with no model (not a board layer: it stands in for a model file, which has its own material). */
const IC_BODY_GREY = 0x3a3a3a;

/**
 * The colours of the board being built, by Appearance-manager row (kicad-port/appearance3d.ts `resolveColors`): the theme's unless `buildBoardGroup` is given
 * others (a swatch, the board's stackup). Set once at the start of every build, which is synchronous, and read by the material factories below.
 */
let palette: Record<string, Rgba> = { ...THEME_COLORS };

/** Whether an Appearance-manager row is shown, for the build in progress (everything is, unless `buildBoardGroup` is given a function). */
let rowShown: (row: string) => boolean = () => true;

function paletteRgba(row: string): Rgba {
  return palette[row] ?? THEME_COLORS[row] ?? { r: 0.5, g: 0.5, b: 0.5, a: 1 };
}

function hexColor(hex: string): THREE.Color {
  return new THREE.Color(`#${hex.startsWith("#") ? hex.slice(1, 7) : hex.slice(0, 6)}`);
}

const srgb = (c: Rgb): THREE.Color => new THREE.Color().setRGB(c.r, c.g, c.b, THREE.SRGBColorSpace);

/**
 * A board layer's material as KiCad's OpenGL view sets it (kicad-port/material3d.ts: `setupMaterials`, `setLayerMaterial`, `OglSetMaterial`): the layer's ambient,
 * diffuse, specular and emissive colours, OpenGL's shininess (`128 * min( m_Shininess, 1 )`) and the transparency, lit with OpenGL's own equation and KiCad's three
 * lights (glLighting.ts).
 */
export function kicadMaterial(m: Material3d, opts: { side?: THREE.Side; depthWrite?: boolean } = {}): THREE.MeshPhongMaterial {
  const transparent = m.transparency > 0;
  const mat = new THREE.MeshPhongMaterial({
    color: srgb(m.diffuse),
    specular: srgb(m.specular),
    emissive: srgb(m.emissive),
    shininess: glShininess(m),
    transparent,
    opacity: 1 - m.transparency,
    side: opts.side ?? THREE.FrontSide,
    ...(opts.depthWrite !== undefined ? { depthWrite: opts.depthWrite } : transparent ? { depthWrite: false } : {}),
  });
  kicadLit(mat, srgb(m.ambient));
  mat.userData.kicad = m;
  return mat;
}

const rgbOf = (c: Rgba): Rgb => ({ r: c.r, g: c.g, b: c.b });

/**
 * The copper of layer `layer` (`setLayerMaterial`): `m_materials.m_Copper` of the copper colour (`m_CopperColor`, the F.Cu row's: "The internals of the 3D viewer only
 * supports a single color for copper"), its diffuse the layer's colour in the PCB editor when "Use PCB editor copper colors" is on (`m_BoardEditorColors[aLayerID]`; the
 * F.Cu and B.Cu rows already carry those). Tracks, pads, vias, zones and the plated barrels (`setLayerMaterial( B_Cu )` before `m_padHoles->DrawAll()`) are all copper.
 */
function copperMaterialOf(layer: string): THREE.MeshPhongMaterial {
  const base = rgbOf(paletteRgba("copper_top"));
  // `if( m_boardAdapter.GetUseBoardEditorCopperLayerColors() && IsCopperLayer( aLayerID ) )`: the PCB editor's colour of the layer, whatever the swatches say.
  const own = innerCopperColor(layer) ?? (layer === "B.Cu" ? paletteRgba("copper_bottom") : paletteRgba("copper_top"));
  return kicadMaterial(copperLayerMaterial3d(base, rgbOf(own)));
}

/** Each copper layer's own colour for the build in progress: the PCB editor's with "Use PCB editor copper colors", else none (the copper rows' colour). */
let innerCopperColor: (layer: string) => Rgba | undefined = () => undefined;

/** `m_materials.m_Paste` of the paste colour (`setLayerMaterial`'s `F_Paste` / `B_Paste`). */
function pasteMaterial(): THREE.MeshPhongMaterial {
  return kicadMaterial(pasteMaterial3d(rgbOf(paletteRgba("solder_paste"))));
}

/** The solder mask (`setLayerMaterial`'s `F_Mask` / `B_Mask`), drawn from both sides where [`maskOpenings`] left its side's stencil bit clear (`renderSolderMaskLayer`). */
function maskMaterial(side: Side): THREE.MeshPhongMaterial {
  const m = kicadMaterial(maskMaterial3d(paletteRgba(side === "top" ? "soldermask_top" : "soldermask_bottom")), { side: THREE.DoubleSide, depthWrite: false });
  m.stencilWrite = true;
  m.stencilWriteMask = 0;
  m.stencilRef = MASK_BITS[side];
  m.stencilFuncMask = MASK_BITS[side];
  m.stencilFunc = THREE.NotEqualStencilFunc;
  return m;
}

/** `m_materials.m_SilkSTop` / `m_SilkSBot` (`setupMaterials`, `setLayerMaterial`'s `F_SilkS` / `B_SilkS`). */
function silkMaterial(side: Side): THREE.MeshPhongMaterial {
  return kicadMaterial(silkMaterial3d(rgbOf(paletteRgba(side === "top" ? "silkscreen_top" : "silkscreen_bottom"))), { side: THREE.DoubleSide });
}

/** The board body: `m_materials.m_EpoxyBoard` with `renderBoardBody`'s diffuse and transparency (`1 - m_BoardBodyColor.a`, the theme's body is 90 % opaque). */
function boardMaterial(): THREE.MeshPhongMaterial {
  const body = paletteRgba("board");
  return kicadMaterial(boardMaterial3d(body), { side: THREE.DoubleSide, ...(body.a < 1 ? { depthWrite: true } : {}) });
}

/**
 * No real per-part color data (see PART_HEIGHT_MM), so this app's own
 * KiCad-package heuristic (already used for the schematic's passive
 * glyphs -- schematic/layout.ts's isTwoPinPassive) stands in: a mid grey
 * for anything else (ICs, connectors, switches...) -- IC_BODY_GREY,
 * given verbatim by the task ("a KiCad-like mid grey (about #3a3a3a)")
 * because a near-black body was reading as a hole in the board -- tan
 * for a recognized 2-pin passive. Lit as KiCad lights a model's material (`OglSetMaterial`; VRML's default ambient intensity, 0.2 of the colour).
 */
function bodyMaterial(hex: number): THREE.MeshPhongMaterial {
  const color = new THREE.Color(hex);
  return kicadLit(new THREE.MeshPhongMaterial({ color, specular: new THREE.Color(0x202020), shininess: 128 * 0.2 }), scaledColor(color, 0.2));
}
function icBodyMaterial(): THREE.MeshPhongMaterial {
  return bodyMaterial(IC_BODY_GREY);
}
function passiveBodyMaterial(): THREE.MeshPhongMaterial {
  return bodyMaterial(0xc9a876);
}

/** A lighter edge outline on every part body, per the task ("a lighter edge, or a soft outline") -- cheap (one THREE.EdgesGeometry per box) and keeps a mid-grey body from reading as a flat hole against the board. */
const PART_EDGE_MATERIAL = new THREE.LineBasicMaterial({ color: 0x8a8a8a });
/** Same heuristic as schematic/layout.ts's isTwoPinPassive, adapted to a PCB Part (pad count stands in for pin count -- this type has no pin list). */
function isPassivePart(part: Part): boolean {
  if ((part.pads?.length ?? 0) !== 2) return false;
  for (const c of [part.ref, part.package, part.value]) {
    if (!c) continue;
    const first = c.replace(/^[+-]/, "").charAt(0).toUpperCase();
    if (first === "R" || first === "C" || first === "L" || first === "D") return true;
  }
  return false;
}

// ---------------------------------------------------------------------
// Outline -> Shape, slab, mask
// ---------------------------------------------------------------------

function polygonAreaMm2(pts: ReadonlyArray<[number, number]>): number {
  let sum = 0;
  for (let i = 0; i < pts.length; i++) {
    const [x0, y0] = pts[i]!;
    const [x1, y1] = pts[(i + 1) % pts.length]!;
    sum += x0 * y1 - x1 * y0;
  }
  return sum / 2;
}

/** Null for a missing/degenerate outline (fewer than 3 points, or zero area) -- callers must skip the slab/mask entirely rather than fake one, per the task spec. */
function buildOutlineShape(outlineMm: ReadonlyArray<[number, number]>): THREE.Shape | null {
  if (outlineMm.length < 3) return null;
  if (Math.abs(polygonAreaMm2(outlineMm)) < 1e-6) return null;
  return new THREE.Shape(outlineMm.map(([x, y]) => new THREE.Vector2(x, y)));
}

/**
 * One `THREE.Shape` per outline of the board, its cutouts as holes: the outline the way KiCad builds it from Edge.Cuts
 * (`kicad-port/pcbOutline.ts` `boardOutlinePolygons`: arcs flattened, cutouts and several outlines kept), so the slab and the mask
 * have the board's holes and each separate piece, not only the first polygon.
 */
function buildOutlineShapes(board: BoardState): THREE.Shape[] {
  const out: THREE.Shape[] = [];
  for (const poly of boardOutlinePolygons(board)) {
    const outer: Array<[number, number]> = poly.outer.map(([x, y]) => [mm(x), mm(y)]);
    const shape = buildOutlineShape(outer);
    if (!shape) continue;
    const outerSign = Math.sign(polygonAreaMm2(outer));
    for (const hole of poly.holes) {
      const pts: Array<[number, number]> = hole.map(([x, y]) => [mm(x), mm(y)]);
      if (pts.length < 3 || Math.abs(polygonAreaMm2(pts)) < 1e-6) continue;
      // A hole winds the other way round from its outline.
      if (Math.sign(polygonAreaMm2(pts)) === outerSign) pts.reverse();
      shape.holes.push(new THREE.Path(pts.map(([x, y]) => new THREE.Vector2(x, y))));
    }
    out.push(shape);
  }
  return out;
}

/**
 * The board slab: `shape`'s polygon extruded to the body's thickness (`m_boardBodyThickness3DU`).
 *
 * Axis note: ExtrudeGeometry extrudes a Shape built in its own local
 * (x,y) plane along local +Z. Rotating the resulting mesh +90° about X
 * (`Rx(90°): (x,y,z) -> (x,-z,y)`) maps local x -> world X (board X,
 * unchanged), local y -> world Z (board Y, unchanged -- matches every
 * other builder in this file), and local z -> world -Y, spanning
 * [-depth,0]; shifting by `+depth/2` recenters that to [-0.8,+0.8],
 * i.e. the slab is centred on world Y=0 as intended.
 *
 * Winding note: `shape`'s point order comes from `board.outline`, whose
 * winding direction this app has no control over (nor a spec for it) --
 * an unexpected winding flips ExtrudeGeometry's face normals inward,
 * which would make the slab invisible from outside under normal
 * front-face culling. Rather than inspect/reverse the polygon (more
 * moving parts, still guessing), the material below is DoubleSide, so
 * the slab renders correctly regardless of the input winding.
 */
function buildSlab(shape: THREE.Shape): THREE.Mesh | null {
  try {
    const geometry = new THREE.ExtrudeGeometry(shape, { depth: stack.boardThicknessMm, bevelEnabled: false });
    const mesh = new THREE.Mesh(geometry, boardMaterial());
    mesh.rotation.x = Math.PI / 2;
    mesh.position.y = stack.boardThicknessMm / 2;
    mesh.name = "board-slab";
    return mesh;
  } catch (err) {
    // Never crash on a pathological (e.g. self-intersecting) outline --
    // skip the slab, same as the missing-outline case.
    console.warn("Viewer3D: failed to build board slab from outline", err);
    return null;
  }
}

/**
 * A thin translucent tint over the whole outline on `side` -- a flat
 * (zero-thickness) ShapeGeometry, not a second extrusion: solder mask
 * really is just a coating, so a flat double-sided plane is an honest
 * representation, not a simplification of one. Same rotation as
 * buildSlab (for the same reason: local z=0 always maps to world Y=0
 * relative to the mesh, so `position.y` alone becomes the final,
 * absolute world Y).
 *
 * The pads' apertures are not cut out of the geometry: the mask's material
 * draws only where [`buildMaskOpenings`] left the stencil clear, as KiCad's
 * `renderSolderMaskLayer` culls the mask layer's items out of `m_board`.
 */
function buildMask(shape: THREE.Shape, side: Side): THREE.Mesh {
  const geometry = new THREE.ShapeGeometry(shape);
  const mesh = new THREE.Mesh(geometry, maskMaterial(side));
  mesh.rotation.x = Math.PI / 2;
  mesh.position.y = maskCenterY(side);
  mesh.name = `solder-mask-${side}`;
  return mesh;
}

// ---------------------------------------------------------------------
// Copper: pads, tracks, vias, the plated barrels, the holes
// ---------------------------------------------------------------------

/** `HOLE_PLATING_THICKNESS` of the backend (`crates/cli/src/board3d_api.rs`): `ADVANCED_CFG::m_HoleWallThickness`, 0.020 mm, when the route has not answered yet. */
const HOLE_PLATING_NM = 20_000;

/** The outer copper's height range on `side`, `[low, high]`: from the body's face out by the copper's thickness (`m_layerZcoordBottom[F_Cu]` .. `m_layerZcoordTop[F_Cu]`). */
function outerCopperRange(side: Side): [number, number] {
  const s = surfaceY(side);
  const c = copperMm(side);
  return side === "top" ? [s, s + c] : [s - c, s];
}

/** Whether the build in progress draws the copper and paste with their thickness (`opengl_copper_thickness`). */
let withThickness = false;
/** How thin a layer drawn without its thickness is (its two faces, no walls to speak of). */
const FLAT_MM = 1e-4;

/** `range` without its thickness when the build draws none: its outer face (the top of a top layer, the bottom of a bottom one, the middle of an inner one). */
function flatten([low, high]: [number, number], face: "top" | "bottom" | "middle"): [number, number] {
  if (withThickness) return [low, high];
  if (face === "top") return [high - FLAT_MM, high];
  if (face === "bottom") return [low, low + FLAT_MM];
  const m = (low + high) / 2;
  return [m - FLAT_MM / 2, m + FLAT_MM / 2];
}

/** The height range of copper layer `layer`: an outer layer's from [`outerCopperRange`]; an inner one copper-thick about its place in the stack-up. */
function copperRange(layer: string, layerOrder: readonly string[]): [number, number] {
  if (layer === "F.Cu") return flatten(outerCopperRange("top"), "top");
  if (layer === "B.Cu") return flatten(outerCopperRange("bottom"), "bottom");
  const y = layerY(layer, layerOrder);
  return flatten([y - COPPER_MM / 2, y + COPPER_MM / 2], "middle");
}

const mmVec = ([x, y]: Pt): THREE.Vector2 => new THREE.Vector2(mm(x), mm(y));

function inside(c: readonly Pt[], [x, y]: Pt): boolean {
  let hit = false;
  for (let i = 0, j = c.length - 1; i < c.length; j = i++) {
    const [xi, yi] = c[i]!;
    const [xj, yj] = c[j]!;
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) hit = !hit;
  }
  return hit;
}

/**
 * Even-odd contours (nm, board) as three.js shapes in mm, each outline with its holes (`nestContours`), with `cuts` (a drilled hole) taken out of the outline that holds
 * all of it -- the copper less `m_TH_ODPolys` / `m_NPTH_ODPolys` (`RENDER_3D_OPENGL::reload`: `polyListSubtracted.BooleanSubtract( m_boardAdapter.GetTH_ODPolys() )`). A
 * cut that is not wholly inside one outline is left out (the copper then covers that part of the hole).
 */
function shapesOf(contours: readonly Contour[], cuts: readonly Contour[] = []): THREE.Shape[] {
  const nested = nestContours(contours);
  const shapes = nested.map(({ outer, holes }) => {
    const s = new THREE.Shape(outer.map(mmVec));
    for (const h of holes) s.holes.push(new THREE.Path(h.map(mmVec)));
    return s;
  });
  for (const cut of cuts) {
    const k = nested.findIndex((n) => cut.every((p) => inside(n.outer, p)) && !n.holes.some((h) => cut.some((p) => inside(h, p))));
    if (k >= 0) shapes[k]!.holes.push(new THREE.Path(cut.map(mmVec)));
  }
  return shapes;
}

/** `shapes` extruded through the height range `[low, high]` (same local-XY -> world-XZ mapping as buildSlab: local z runs down from `high`). */
/** The walls' material of a layer's material (its copy, marked). */
const walls = new WeakMap<THREE.Material, THREE.Material>();
function wallsOf(m: THREE.Material): THREE.Material {
  let w = walls.get(m);
  if (!w) {
    w = m instanceof THREE.MeshPhongMaterial ? cloneLit(m) : m.clone();
    w.userData.walls = true;
    walls.set(m, w);
  }
  return w;
}

function extruded(shapes: readonly THREE.Shape[], [low, high]: [number, number], material: THREE.Material, name: string): THREE.Mesh | null {
  if (shapes.length === 0) return null;
  try {
    const geometry = new THREE.ExtrudeGeometry(shapes as THREE.Shape[], { depth: Math.max(high - low, 1e-5), bevelEnabled: false, curveSegments: 1 });
    // With the thickness shown, the walls (ExtrudeGeometry's second group: `DrawMiddle`) have a material of their own, which "Disable thickness" hides while the camera
    // moves (`setMoving3d`).
    const mesh = new THREE.Mesh(geometry, withThickness ? [material, wallsOf(material)] : material);
    mesh.rotation.x = Math.PI / 2;
    mesh.position.y = high;
    mesh.name = name;
    return mesh;
  } catch (err) {
    console.warn(`Viewer3D: could not build ${name}`, err);
    return null;
  }
}

// ---------------------------------------------------------------------
// Silkscreen (courtyard-outline stand-in) + part bodies
// ---------------------------------------------------------------------

/**
 * Part body simplification, for when KiCad's own render (the GLB, with the
 * real 3D models) is not there -- still being exported, or it failed. Every
 * placed part renders as a plain box on the part's BODY (the footprint's
 * `F.Fab` box, `part.body`; the courtyard only for a footprint with none --
 * kicad-port/partBody.ts) and a plausible height (`fallbackBodyHeightMm`),
 * sitting on the board surface on its own side. A lighter edge
 * outline (PART_EDGE_MATERIAL) is drawn on top of the box so a mid-grey
 * body still reads as a raised part against the board, not a flat patch
 * -- see PART_EDGE_MATERIAL's own comment.
 */
function partBodyMesh(bodyMm: readonly [number, number, number, number], side: Side, material: THREE.Material): THREE.Mesh {
  const [minX, minY, maxX, maxY] = bodyMm;
  const width = Math.max(maxX - minX, 0.01);
  const depth = Math.max(maxY - minY, 0.01);
  const heightMm = fallbackBodyHeightMm(width, depth);
  const geometry = new THREE.BoxGeometry(width, heightMm, depth);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.position.set((minX + maxX) / 2, partBodyCenterY(side, heightMm), (minY + maxY) / 2);
  mesh.name = "part-body";
  const edges = new THREE.LineSegments(new THREE.EdgesGeometry(geometry), PART_EDGE_MATERIAL);
  mesh.add(edges);
  return mesh;
}

/** The two body materials of the placeholder boxes: one set is shared by every box a `PartModels` (partModels.ts) draws, and disposed with it. */
export interface PlaceholderMaterials {
  ic: THREE.Material;
  passive: THREE.Material;
}

export function placeholderMaterials(): PlaceholderMaterials {
  return { ic: icBodyMaterial(), passive: passiveBodyMaterial() };
}

/**
 * The box a part is drawn as until its 3D models are in (or when it has none): its footprint's body (F.Fab), a plausible height, on the board surface of its own
 * side. `null` when the part has no placed body or courtyard to size it from. The mesh sits in world coordinates (no transform of its own).
 */
export function buildPartPlaceholder(part: Part, materials: PlaceholderMaterials): THREE.Mesh | null {
  const side = part.side;
  const body = partBodyBoxNm(part);
  if (!part.placed || !side || !body) return null;
  return partBodyMesh([mm(body[0]), mm(body[1]), mm(body[2]), mm(body[3])], side, isPassivePart(part) ? materials.passive : materials.ic);
}

// ---------------------------------------------------------------------
// Top-level: board -> Group
// ---------------------------------------------------------------------

/** The Render Options `createLayers` and `Redraw` read (Preferences > 3D Viewer > General, `EDA_3D_VIEWER_SETTINGS::RENDER_SETTINGS`), KiCad's defaults all off. */
export interface RenderOptions3d {
  /** `render.clip_silk_on_via_annuli` ("Clip silkscreen at via annuli"). */
  clipSilkOnViaAnnuli?: boolean;
  /** `render.subtract_mask_from_silk` ("Clip silkscreen at solder mask edges"). */
  subtractMaskFromSilk?: boolean;
  /** `render.differentiate_plated_copper` ("Differentiate plated copper"). */
  differentiatePlatedCopper?: boolean;
}

export interface BuildBoardOptions {
  /** Whether a row of the Appearance manager is shown (kicad-port/appearance3d.ts `isVisible` over the person's choices). Everything is, by default. */
  visible?: (row: string) => boolean;
  /** The colour of every row (`resolveColors`: the theme, the board's stackup, the swatches). The theme's, by default. */
  colors?: Record<string, Rgba>;
  /** The zones' fills as KiCad made them (`GET /api/fill`): a zone with none has no copper. Absent: each zone's outline stands in for its copper. */
  zoneFills?: readonly FillZone[];
  /** The board's build-up (`stackOfBoard`). Absent: `stackOfBoard(board.board_rules)`. */
  stack?: Stack;
  /** `GET /api/board3d`: the body with its holes (`m_boardWithHoles`) and the plating. Absent (not fetched yet): the outline without holes. */
  board3d?: Board3d | null;
  /** `render.opengl_copper_thickness` ("Show copper and tech layers thickness", off by default): copper and paste with their thickness, else flat at their outer face. */
  copperThickness?: boolean;
  /** The copper layers: whether the PCB editor shows an inner one (`Is3dLayerEnabled`'s default branch, `m_board->IsLayerVisible( aLayer )`) and each one's own colour (the PCB editor's, with "Use PCB editor copper colors"). */
  innerCopper?: { visible: (layer: string) => boolean; color: (layer: string) => Rgba | undefined };
  /** "Use PCB editor copper colors" (`use_board_editor_copper_colors`): `DifferentiatePlatedCopper()` is off with it. */
  boardEditorCopperColors?: boolean;
  /** The Render Options. */
  render?: RenderOptions3d;
  /** The server's answers to the polygon jobs the layers asked for (`group.userData.jobs`, `POST /api/board3d/polys`). */
  jobs?: JobResults;
}

/** `GET /api/board3d` (crates/cli/src/board3d_api.rs): the body polygons in nm, each outline with its holes, and the hole plating in nm. */
export interface Board3d {
  body: Array<{ outer: [number, number][]; holes: [number, number][][] }>;
  plating: number;
}

/** `BOARD_ADAPTER::InitSettings`' `m_nonCopperLayerThickness3DU` (DEFAULT_TECH_LAYER_THICKNESS) and `layerThicknessMargin`: where the technical layers sit. */
const NON_COPPER_MM = MASK_MM;
const LAYER_THICKNESS_MARGIN = 1.1;

/** `InitSettings`' height range of a technical layer (`m_layerZcoordBottom` .. `m_layerZcoordTop`): the masks and the paste on the copper's face, the silkscreen 1.1 of a technical layer's thickness off it, every other layer twice that. */
function techRange(layer: string): [number, number] {
  const back = layer.startsWith("B.");
  const side: Side = back ? "bottom" : "top";
  const s = outwardSign(side);
  const copperTop = surfaceY(side) + s * copperMm(side);
  const offset = NON_COPPER_MM * LAYER_THICKNESS_MARGIN;
  let a: number;
  let b: number;
  if (layer === "F.Mask" || layer === "B.Mask") [a, b] = [copperTop, copperTop + s * maskMm(side)];
  else if (layer === "F.Paste" || layer === "B.Paste") [a, b] = [copperTop, copperTop + s * SOLDERPASTE_MM];
  else if (layer === "F.SilkS" || layer === "B.SilkS") [a, b] = [copperTop + s * offset, copperTop + s * (offset + NON_COPPER_MM)];
  else [a, b] = [copperTop + s * 2 * offset, copperTop + s * (2 * offset + NON_COPPER_MM)];
  return flatten([Math.min(a, b), Math.max(a, b)], back ? "bottom" : "top");
}

/** `m_antiBoard`: everything off the board (`m_antiBoardPolys`, a huge square less `GetBoardPoly()`), as shapes in mm: the square with the outlines as holes, and the cutouts. */
function antiBoardShapes(board: BoardState): THREE.Shape[] {
  const polys = boardOutlinePolygons(board);
  if (polys.length === 0) return [];
  const big = 1e5;
  const outer = new THREE.Shape([new THREE.Vector2(-big, -big), new THREE.Vector2(big, -big), new THREE.Vector2(big, big), new THREE.Vector2(-big, big)]);
  const out = [outer];
  for (const p of polys) {
    outer.holes.push(new THREE.Path(p.outer.map(([x, y]) => new THREE.Vector2(mm(x), mm(y)))));
    for (const h of p.holes) out.push(new THREE.Shape(h.map(([x, y]) => new THREE.Vector2(mm(x), mm(y)))));
  }
  return out;
}

/** The pieces of one layer, by what they are (`Prim.tag`): one mesh per kind, a pad one mesh each (the test hook reads their shapes), named by `nameOf`. */
function layerMeshes(list: readonly Prim[], range: [number, number], material: THREE.Material, nameOf: (tag: string) => string, jobs: JobResults | undefined): THREE.Mesh[] {
  const byTag = new Map<string, Prim[]>();
  const meshes: THREE.Mesh[] = [];
  for (const p of list) {
    const tag = p.tag ?? "item";
    if (tag === "pad" && p.meta?.padShape) {
      const mesh = extruded(primShapes(p, jobs), range, material, nameOf("pad"));
      if (mesh) {
        mesh.userData.padShape = p.meta.padShape;
        mesh.userData.corners = p.meta.corners;
        meshes.push(mesh);
      }
      continue;
    }
    const l = byTag.get(tag);
    if (l) l.push(p);
    else byTag.set(tag, [p]);
  }
  for (const [tag, prims] of byTag) {
    const mesh = extruded(listShapes(prims, jobs), range, material, nameOf(tag));
    if (mesh) meshes.push(mesh);
  }
  return meshes;
}

/** A clone of meshes with another material (the plated copper, the off-board pads), sharing their geometry. */
function withMaterial(meshes: readonly THREE.Mesh[], material: THREE.Material, name: string): THREE.Mesh[] {
  return meshes.map((m) => {
    const c = new THREE.Mesh(m.geometry, material);
    c.position.copy(m.position);
    c.rotation.copy(m.rotation);
    c.name = name;
    return c;
  });
}

/** `glPolygonOffset( -0.1f, -2.0f )` (`setPlatedCopperAndDepthOffset`): the plated copper and the off-board pads drawn over the copper they lie on. */
function platedOffset(m: THREE.Material): THREE.Material {
  m.polygonOffset = true;
  m.polygonOffsetFactor = -0.1;
  m.polygonOffsetUnits = -2;
  return m;
}

/** A height of the stack (`ZRef`) in this scene's mm: a copper layer's face, moved `depth` into the board. */
function zOf(ref: ZRef, layerOrder: readonly string[]): number {
  const [low, high] = ref.layer === "F.Cu" ? outerCopperRange("top") : ref.layer === "B.Cu" ? outerCopperRange("bottom") : copperRangeThick(ref.layer, layerOrder);
  const face = ref.face === "top" ? high : low;
  return face + (ref.face === "top" ? -1 : 1) * mm(ref.depth ?? 0);
}

/** An inner copper layer's range with its thickness (the post-machining is measured from its real faces). */
function copperRangeThick(layer: string, layerOrder: readonly string[]): [number, number] {
  const y = layerY(layer, layerOrder);
  return [y - COPPER_MM / 2, y + COPPER_MM / 2];
}

/**
 * `generateCylinder` (a ring between two heights), `generateInvCone` (its top a cone from the inner radius, `coneDdeg` the half-angle from the vertical) and
 * `appendPostMachiningGeometry`'s countersink plating (a cone wall `coneWall` thick, `wideAt` its wide end): one solid of revolution about the hole's axis.
 */
function cylinderMesh(c: Cylinder, layerOrder: readonly string[], material: THREE.Material, name: string): THREE.Mesh | null {
  const top = zOf(c.top, layerOrder);
  const bottom = zOf(c.bottom, layerOrder);
  const [zt, zb] = [Math.max(top, bottom), Math.min(top, bottom)];
  if (!(zt > zb) || !(c.ro > c.ri)) return null;
  const ri = mm(c.ri);
  const ro = mm(c.ro);
  let profile: THREE.Vector2[];
  if (c.coneWall !== undefined) {
    const t = mm(c.coneWall);
    profile = c.wideAt === "bottom" ? [new THREE.Vector2(ro, zb), new THREE.Vector2(ri, zt), new THREE.Vector2(ri + t, zt), new THREE.Vector2(ro + t, zb)] : [new THREE.Vector2(ri, zb), new THREE.Vector2(ro, zt), new THREE.Vector2(ro + t, zt), new THREE.Vector2(ri + t, zb)];
  } else if (c.coneDdeg !== undefined) {
    // "innerHeight = ( outerRadius - innerRadius ) / tan( angle )", no taller than the plug
    const angle = Math.max((c.coneDdeg / 10) * (Math.PI / 180), 0.01);
    const inner = Math.min((ro - ri) / Math.tan(angle), zt - zb);
    profile = [new THREE.Vector2(ri, zb), new THREE.Vector2(ro, zb), new THREE.Vector2(ro, zt), new THREE.Vector2(ri, zb + inner)];
  } else {
    profile = [new THREE.Vector2(ri, zb), new THREE.Vector2(ro, zb), new THREE.Vector2(ro, zt), new THREE.Vector2(ri, zt)];
  }
  profile.push(profile[0]!.clone());
  const segments = Math.max(12, arcToSegmentCount(c.ro, MAX_ERROR_NM, 360));
  const geometry = new THREE.LatheGeometry(profile, segments).toNonIndexed();
  geometry.computeVertexNormals();
  const mesh = new THREE.Mesh(geometry, material);
  mesh.position.set(mm(c.c[0]), 0, mm(c.c[1]));
  mesh.name = name;
  return mesh;
}

/** The shapes of a side's mask openings, the mask layer's items (`m_layers[ F_Mask ]`). */
function maskShapes(L: Layers3d, side: Side, jobs: JobResults | undefined): THREE.Shape[] {
  return listShapes(L.layers.get(side === "top" ? "F.Mask" : "B.Mask") ?? [], jobs);
}

/** The Appearance-manager row of a technical or user layer (`Is3dLayerEnabled`). */
function techRow(layer: string): string | null {
  switch (layer) {
    case "F.Adhes":
    case "B.Adhes":
      return "adhesive";
    case "F.Paste":
    case "B.Paste":
      return "solder_paste";
    case "F.SilkS":
      return "silkscreen_top";
    case "B.SilkS":
      return "silkscreen_bottom";
    case "F.Mask":
      return "soldermask_top";
    case "B.Mask":
      return "soldermask_bottom";
    default:
      return userRowOfLayer(layer);
  }
}

/**
 * Builds the board as a single Group as KiCad's OpenGL view draws it (`RENDER_3D_OPENGL::reload` / `bgWorker` and `Redraw`): the body with its holes and the plugs of its
 * post-machining, each copper layer's items (kicad-port/layers3d.ts, `BOARD_ADAPTER::createLayers`) culled by the holes and by everything off the board, the plated
 * copper and the off-board pads, the barrels, the technical layers (the paste, the silkscreen culled by the holes or the via annuli, the mask's openings and the board's
 * outside, the adhesive, the user layers), and the solder masks with their openings and the vias' covers -- each as its Appearance-manager row says (`opts.visible`) and in
 * its colour (`opts.colors`). The parts' 3D models are not in it: partModels.ts draws them. `group.userData.jobs` lists the polygons the server has to work out
 * (`opts.jobs` brings their answers to the next build). Safe to call with a board that has no outline and/or no placed parts.
 */
export function buildBoardGroup(board: BoardState, opts: BuildBoardOptions = {}): THREE.Group {
  palette = { ...THEME_COLORS, ...(opts.colors ?? {}) };
  rowShown = opts.visible ?? (() => true);
  stack = opts.stack ?? stackOfBoard(board.board_rules);
  innerCopperColor = opts.innerCopper?.color ?? (() => undefined);
  const shown = rowShown;
  withThickness = opts.copperThickness ?? false;
  const render = opts.render ?? {};
  const jobs = opts.jobs;
  const differentiate = !!render.differentiatePlatedCopper && !opts.boardEditorCopperColors;
  const offBoardSilk = shown("off_board_silk");
  const layerOrder = board.layers.length > 0 ? board.layers : ["F.Cu", "B.Cu"];
  // `Is3dLayerEnabled`: F.Cu and B.Cu by their rows; an inner layer as the PCB editor shows it, and only when the body does not hide it ("Do not show inner layers when it
  // is displaying the board and board body is opaque enough", `RENDER_3D_OPENGL::Redraw`).
  const innerDrawn = innerLayersDrawn(shown("board"), paletteRgba("board"));
  const copperShown = (layer: string) => (layer === "F.Cu" ? shown("copper_top") : layer === "B.Cu" ? shown("copper_bottom") : innerDrawn && (opts.innerCopper?.visible(layer) ?? true));
  const plating = opts.board3d?.plating ?? HOLE_PLATING_NM;

  const group = new THREE.Group();
  group.name = "viewer3d-board";
  const culler = new Culler(group);

  // The layers' items (`createLayers`).
  const L = buildLayers3d(
    board,
    { layerOrder, ...(opts.zoneFills ? { zoneFills: opts.zoneFills } : {}), barcodePaths: (b) => readyPaths(b) },
    {
      layerShown: (layer) => (/\.Cu$/.test(layer) ? copperShown(layer) : ((r) => r !== null && shown(r))(techRow(layer))),
      fpText: shown("footprint_text"),
      references: shown("references"),
      values: shown("values"),
      showZones: shown("zones"),
      showOffBoardSilk: offBoardSilk,
      clipSilkOnViaAnnuli: !!render.clipSilkOnViaAnnuli,
      subtractMaskFromSilk: !!render.subtractMaskFromSilk,
      differentiatePlatedCopper: differentiate,
      plating,
    }
  );
  const allJobs: PolyJob[] = [...L.jobs];
  const stats = { silkMaskCulled: 0, silkBoardCulled: 0, silkAnnuliCulled: 0, plated: 0 };

  // The body: the board's outlines with their cutouts and, once `GET /api/board3d` has answered, every through and non-plated hole (`m_boardWithHoles`), less the
  // counterbores, countersinks and backdrills (`board_poly_with_holes.BooleanSubtract( GetFrontCounterborePolys() )` ..: the server's), gated by the Board Body row.
  // The masks are drawn over the same polygon (`renderSolderMaskLayer` culls the through holes out of `m_board`).
  let body = opts.board3d && opts.board3d.body.length > 0 ? opts.board3d.body.map((p) => ({ outer: p.outer as Contour, holes: p.holes as Contour[] })) : null;
  if (body && L.bodyCuts.length > 0) {
    const cut = makeJob({ a: body, b: L.bodyCuts.map((c) => ({ outer: c, holes: [] })) });
    allJobs.push(cut);
    const answered = jobs?.get(cut.key);
    if (answered) body = answered.map((p) => ({ outer: p.outer, holes: p.holes }));
  }
  const bodyShapes = body ? body.flatMap((p) => contoursToShapes([p.outer, ...p.holes])) : buildOutlineShapes(board);
  for (const shape of bodyShapes) {
    if (shown("board")) {
      const slab = buildSlab(shape);
      if (slab) {
        slab.userData.holes = shape.holes.length;
        group.add(slab);
      }
    }
    if (shown("soldermask_top")) group.add(buildMask(shape, "top"));
    if (shown("soldermask_bottom")) group.add(buildMask(shape, "bottom"));
  }
  // `m_postMachinePlugs`: the board material left under a counterbore or countersink and beside a backdrill, drawn with the body.
  if (shown("board") && L.plugs.length > 0) {
    const bm = boardMaterial();
    for (const p of L.plugs) {
      const mesh = cylinderMesh(p, layerOrder, bm, "post-machine-plug");
      if (mesh) group.add(mesh);
    }
  }

  const antiBoard = antiBoardShapes(board);
  const thOD = listShapes(L.thODs);
  const holeShapes = (layer: string) => listShapes(L.layerHoles.get(layer) ?? []);

  // The solder masks' openings, each side's stencil bit (`renderSolderMaskLayer`: `m_board->DrawCulled( .., solder_mask, via_holes )`).
  for (const s of ["top", "bottom"] as const) {
    if (!shown(s === "top" ? "soldermask_top" : "soldermask_bottom")) continue;
    const mesh = maskOpenings(maskShapes(L, s, jobs), (techRange(s === "top" ? "F.Mask" : "B.Mask")[0] + techRange(s === "top" ? "F.Mask" : "B.Mask")[1]) / 2, s);
    if (mesh) group.add(mesh);
  }

  // Copper, layer by layer (`Redraw`: `pLayerDispList->DrawCulled( showThickness, outerTH, viaHoles, anti_board )`): with "Differentiate plated copper" every copper layer
  // is the bare copper (`setCopperMaterial`) and the plated copper, the outer copper inside the mask's openings, is drawn over it in the layer's colour.
  const copperMats = new Map<string, THREE.Material>();
  const copperMat = (layer: string): THREE.Material => {
    let m = copperMats.get(layer);
    if (!m) copperMats.set(layer, (m = copperMaterialOf(layer)));
    return m;
  };
  for (const layer of layerOrder) {
    const list = L.layers.get(layer);
    if (!list || !copperShown(layer)) continue;
    const range = copperRange(layer, layerOrder);
    const y = (range[0] + range[1]) / 2;
    const material = differentiate ? kicadMaterial(nonPlatedCopperMaterial3d()) : copperMat(layer);
    const meshes = layerMeshes(list, range, material, (tag) => (["track", "via", "pad", "zone-fill"].includes(tag) ? tag : `copper-${tag}`), jobs);
    if (meshes.length > 0) group.add(...meshes);
    const holes = holeShapes(layer);
    culler.culled(meshes, [{ shapes: thOD, y, hole: true }, { shapes: holes, y, hole: true }, { shapes: antiBoard, y }]);
    const side: Side | null = layer === "F.Cu" ? "top" : layer === "B.Cu" ? "bottom" : null;
    if (!side) continue;
    if (differentiate) {
      // `m_platedPadsFront`: the copper less the holes and the outside, inside the mask's openings (`BooleanIntersection( *m_layers_poly.at( F_Mask ) )`).
      const plated = withMaterial(meshes, platedOffset(copperMaterialOf(layer)), "plated-copper");
      stats.plated += plated.length;
      if (plated.length > 0) group.add(...plated);
      culler.culled(plated, [{ shapes: maskShapes(L, side, jobs), y, keep: true }, { shapes: thOD, y, hole: true }, { shapes: holes, y, hole: true }, { shapes: antiBoard, y }], true);
    }
    // `m_offboardPadsFront` / `Back`: not culled by the board's outside.
    const off = side === "top" ? L.offboardPads.front : L.offboardPads.back;
    if (off.length > 0) {
      const pads = layerMeshes(off, range, platedOffset(copperMaterialOf(layer)), () => "offboard-pad", jobs);
      if (pads.length > 0) group.add(...pads);
      culler.culled(pads, [{ shapes: thOD, y, hole: true }, { shapes: holes, y, hole: true }]);
    }
  }

  // The barrels of the vias and the plated holes (`generatePlatedHoleShells`: the hole grown by the plating less the hole, from B.Cu's face to F.Cu's, for a through via
  // and a plated pad; `generateViaBarrels` for a blind, buried or micro via and for one that is backdrilled or post-machined, with the plating of its counterbore or
  // countersink): the "Plated Barrels" row, `setLayerMaterial( B_Cu )`.
  if (shown("plated_barrels")) {
    const barrelRange: [number, number] = [outerCopperRange("bottom")[0], outerCopperRange("top")[1]];
    const bm = copperMaterialOf("B.Cu");
    for (const via of board.routing?.vias ?? []) {
      if (via.type || L.barrelVias.has(via.id)) continue;
      const hole = circlePolygon([via.x, via.y], Math.trunc(via.drill / 2) + plating, MAX_ERROR_NM);
      // A capped via's hole is filled: `tht_outer_holes_poly.BooleanAdd( capped_outer_vias_poly )`, its whole disk.
      const mesh = extruded(shapesOf(via.props?.capping ? [hole] : [hole, circlePolygon([via.x, via.y], Math.trunc(via.drill / 2), MAX_ERROR_NM)]), barrelRange, bm, "plated-barrel");
      if (mesh) group.add(mesh);
    }
    for (const c of L.barrels) {
      const mesh = cylinderMesh(c, layerOrder, bm, "plated-barrel");
      if (mesh) {
        // `m_microviaHoles`, what "Disable micro vias" leaves out while the camera moves
        mesh.userData.microvia = true;
        group.add(mesh);
      }
    }
    for (const part of board.parts) {
      if (!part.placed || !part.side) continue;
      for (const pad of part.pads ?? []) {
        if (!pad.th || pad.kind === "non_plated_hole") continue;
        const outer = padHoleContour(pad, part, plating);
        const inner = padHoleContour(pad, part, 0);
        if (outer && inner) {
          const mesh = extruded(shapesOf([outer, inner]), barrelRange, bm, "plated-barrel");
          if (mesh) group.add(mesh);
        }
      }
    }
  }

  // The technical layers (`Redraw`'s other branch: `pLayerDispList->DrawCulled( showThickness, solder_mask, throughHolesOuter, anti_board )`).
  const annuli = render.clipSilkOnViaAnnuli ? listShapes(L.viaAnnuli) : thOD;
  for (const [layer, list] of L.layers) {
    if (/\.Cu$/.test(layer) || layer === "F.Mask" || layer === "B.Mask") continue;
    const row = techRow(layer);
    if (!row || !shown(row) || list.length === 0) continue;
    const range = techRange(layer);
    const y = (range[0] + range[1]) / 2;
    const silk = layer === "F.SilkS" || layer === "B.SilkS";
    const side: Side = layer.startsWith("B.") ? "bottom" : "top";
    const physical = silk || /Paste$|Adhes$/.test(layer);
    let material: THREE.Material;
    let nameOf: (tag: string) => string;
    if (silk) {
      material = silkMaterial(side);
      nameOf = (tag) => (tag === "part-ref" || tag === "part-value" || tag === "part-field" ? tag : `silk-${tag}`);
    } else if (layer === "F.Paste" || layer === "B.Paste") {
      material = pasteMaterial();
      nameOf = () => "solder-paste";
    } else {
      // `m_materials.m_Plastic` of the layer's colour (`setLayerMaterial`: the adhesive, user and technical layers).
      const color = row.startsWith("user_") ? srgbOfRow(row) : hexColor(layerColor(layer.startsWith("B.") ? "B_Adhes" : "F_Adhes"));
      material = kicadMaterial(plasticMaterial3d({ r: color.r, g: color.g, b: color.b }), { side: THREE.DoubleSide });
      material.userData.ink = color;
      nameOf = (tag) => `${row}-${tag}`;
    }
    const meshes: THREE.Object3D[] = layerMeshes(list, range, material, nameOf, jobs);
    // The text boxes on the layer, the board's and the footprints' (`addShape( const PCB_TEXTBOX* )`): a picture of the box (pcbItems3d.ts), culled with the layer.
    for (const b of [...(board.drawings?.text_boxes ?? []), ...board.parts.filter((p) => p.placed).flatMap((p) => fpTextBoxes(p))]) {
      if (b.layer !== layer) continue;
      const tint = silk ? paletteColor(row) : ((material as THREE.MeshPhongMaterial).userData.ink as THREE.Color | undefined);
      const mesh = buildTextBoxMesh(b, range[side === "top" ? 1 : 0], tint, silk ? "silk" : row);
      if (mesh) {
        const m = mesh.material as THREE.MeshBasicMaterial;
        // drawn with the layer (an opaque object), its empty texels left out
        m.transparent = false;
        m.alphaTest = 0.5;
        m.depthWrite = true;
        meshes.push(mesh);
      }
    }
    if (meshes.length === 0) continue;
    if (meshes.length > 0) group.add(...meshes);
    const subtracts: Subtract[] = [{ shapes: silk ? annuli : thOD, y, hole: true }];
    if (silk && render.subtractMaskFromSilk && !offBoardSilk) {
      subtracts.push({ shapes: maskShapes(L, side, jobs), y });
      stats.silkMaskCulled++;
    }
    if (physical && !(silk && offBoardSilk)) {
      subtracts.push({ shapes: antiBoard, y });
      if (silk) stats.silkBoardCulled++;
    }
    if (silk && render.clipSilkOnViaAnnuli) stats.silkAnnuliCulled++;
    culler.culled(meshes, subtracts);
  }

  // `generateViaCovers`: a covered or tented via's hole closed by the mask (`renderSolderMaskLayer`'s `m_viaFrontCover->DrawTop()`, the mask's material).
  for (const s of ["top", "bottom"] as const) {
    const covers = s === "top" ? L.covers.front : L.covers.back;
    if (!shown(s === "top" ? "soldermask_top" : "soldermask_bottom") || covers.length === 0) continue;
    const r = techRange(s === "top" ? "F.Mask" : "B.Mask");
    const shapes = covers.map((c) => new THREE.Shape(circlePolygon(c.c, c.r, MAX_ERROR_NM).map(([x, yy]) => new THREE.Vector2(mm(x), mm(yy)))));
    const m = kicadMaterial(maskMaterial3d(paletteRgba(s === "top" ? "soldermask_top" : "soldermask_bottom")), { side: THREE.DoubleSide, depthWrite: false });
    const mesh = new THREE.Mesh(new THREE.ShapeGeometry(shapes), m);
    mesh.rotation.x = Math.PI / 2;
    mesh.position.y = s === "top" ? r[1] : r[0];
    mesh.name = `via-cover-${s}`;
    group.add(mesh);
  }

  // For the test hook: the colour each copper layer drawn has (its material's diffuse), and the polygons the server is asked for.
  group.userData.copperColors = Object.fromEntries([...copperMats].map(([layer, m]) => [layer, `#${(m as THREE.MeshPhongMaterial).color.getHexString()}`]));
  group.userData.jobs = allJobs;
  const tagCount = (layer: string, tag: string) => (L.layers.get(layer) ?? []).filter((p) => p.tag === tag).length;
  group.userData.layers3d = {
    ...stats,
    thODs: L.thODs.length,
    viaAnnuli: L.viaAnnuli.length,
    bodyCuts: L.bodyCuts.length,
    plugs: L.plugs.length,
    offboard: L.offboardPads.front.length + L.offboardPads.back.length,
    coversTop: L.covers.front.length,
    coversBottom: L.covers.back.length,
    maskTracksTop: tagCount("F.Mask", "track") + tagCount("B.Mask", "track"),
    maskViasTop: tagCount("F.Mask", "via"),
    maskViasBottom: tagCount("B.Mask", "via"),
    cullPieces: group.children.filter((o) => o.name === "cull-piece").length,
  };
  return group;
}

/** A user layer's colour (its row's), as three.js holds a colour. */
function srgbOfRow(row: string): THREE.Color {
  const c = paletteRgba(row);
  return new THREE.Color().setRGB(c.r, c.g, c.b, THREE.SRGBColorSpace);
}

/** A palette colour as three.js holds it (the RGB read as sRGB). */
function paletteColor(row: string): THREE.Color {
  const c = paletteRgba(row);
  return new THREE.Color().setRGB(c.r, c.g, c.b, THREE.SRGBColorSpace);
}

/**
 * The "While Moving" options (Preferences > 3D Viewer > Realtime Rendering): `Redraw( aIsMoving )`'s `skipThickness`, `skipRenderHoles` and `skipRenderMicroVias` -- while
 * the camera moves the layers are drawn without their walls (`DrawCulled( showThickness = false, .. )`), without their culling by the holes and with no barrels
 * (`m_padHoles`), and the micro, blind and post-machined vias' barrels are left out (`m_microviaHoles`). Called when what applies changes. Not ported: the body drawn
 * without its holes while moving (`renderBoardBody( skipRenderHoles )` draws `m_board`): the studio's body is one polygon with its holes.
 */
export function setMoving3d(root: THREE.Object3D, skip: { holes: boolean; microvias: boolean; thickness: boolean }): void {
  root.traverse((o) => {
    if (o.name === "cull-piece" && o.userData.hole) o.visible = !skip.holes;
    else if (o.name === "plated-barrel") o.visible = !skip.holes && !(skip.microvias && o.userData.microvia);
    const m = (o as THREE.Mesh).material;
    if (Array.isArray(m)) for (const x of m) if (x.userData.walls) x.visible = !skip.thickness;
  });
}


/**
 * KiCad's 3D viewer background: a vertical lavender-to-grey gradient
 * (LAYER_3D_BACKGROUND_TOP/BOTTOM, colors.json), not a flat color. A
 * flat Scene.background can only be one color, so this renders the
 * two-stop gradient into a tall, 1px-wide canvas and uses that as the
 * scene background texture instead -- cheap (built once per mount, not
 * per frame) and needs no skybox mesh/extra draw call.
 */
export function buildBackgroundTexture(top?: Rgba, bottom?: Rgba): THREE.CanvasTexture {
  const canvas = document.createElement("canvas");
  canvas.width = 1;
  canvas.height = 256;
  const ctx = canvas.getContext("2d")!;
  const gradient = ctx.createLinearGradient(0, 0, 0, canvas.height);
  // `OglDrawBackground( premultiplyAlpha( m_BgColorTop ), premultiplyAlpha( m_BgColorBot ) )`: each colour times its alpha, drawn opaque (blending off) over the
  // cleared frame -- a background colour with an alpha under 1 is that much darker, not see-through.
  const css = (c: Rgba) => `rgb(${Math.round(c.r * c.a * 255)}, ${Math.round(c.g * c.a * 255)}, ${Math.round(c.b * c.a * 255)})`;
  gradient.addColorStop(0, css(top ?? THEME_COLORS.background_top!));
  gradient.addColorStop(1, css(bottom ?? THEME_COLORS.background_bottom!));
  ctx.fillStyle = gradient;
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  return texture;
}

/** Traverses `root` disposing every geometry/material exactly once. Call on every rebuild (not just unmount) to avoid leaking GPU resources across board updates/tab switches. */
export function disposeObject3D(root: THREE.Object3D): void {
  const seenGeometries = new Set<THREE.BufferGeometry>();
  const seenMaterials = new Set<THREE.Material>();
  root.traverse((obj) => {
    if (obj instanceof THREE.Mesh || obj instanceof THREE.Line) {
      const geometry = obj.geometry;
      if (geometry && !seenGeometries.has(geometry)) {
        seenGeometries.add(geometry);
        geometry.dispose();
      }
      const material = obj.material;
      const materials = Array.isArray(material) ? material : [material];
      for (const m of materials) {
        if (m && !seenMaterials.has(m)) {
          seenMaterials.add(m);
          m.dispose();
        }
      }
    }
  });
}

// ---------------------------------------------------------------------
// Cheap outline-only bounds, for deciding *when* to auto-refit
// ---------------------------------------------------------------------

export interface OutlineBounds {
  minX: number;
  minZ: number;
  maxX: number;
  maxZ: number;
}

/** Board-space (mm, world X/Z) bounds of the board's outlines alone -- null when there's no usable outline. Cheap on purpose: computed on every board update just to decide whether the camera needs re-fitting, without building the full Three.js group first. */
export function boardOutlineBounds(board: BoardState): OutlineBounds | null {
  let minX = Infinity;
  let minZ = Infinity;
  let maxX = -Infinity;
  let maxZ = -Infinity;
  // Every outline of the board (a cutout is inside its outline, so only the outer rings count).
  for (const poly of boardOutlinePolygons(board)) {
    for (const [x, y] of poly.outer) {
      const mx = mm(x);
      const mz = mm(y);
      if (mx < minX) minX = mx;
      if (mz < minZ) minZ = mz;
      if (mx > maxX) maxX = mx;
      if (mz > maxZ) maxZ = mz;
    }
  }
  if (!Number.isFinite(minX)) return null;
  return { minX, minZ, maxX, maxZ };
}

/** The pads drawn by their shape and the corners of their polygons, for the test hook (a trapezoid has 4, a rounded rectangle dozens, a box would be 4). */
export function padShapeCounts(root: THREE.Object3D): Record<string, { meshes: number; corners: number }> {
  const out: Record<string, { meshes: number; corners: number }> = {};
  root.traverse((o) => {
    if (o.name !== "pad" || typeof o.userData.padShape !== "string") return;
    const e = (out[o.userData.padShape] ??= { meshes: 0, corners: 0 });
    e.meshes++;
    e.corners += (o.userData.corners as number) ?? 0;
  });
  return out;
}

/** The holes of the board body as drawn (each slab's outline's holes), for the test hook. */
export function bodyHoleCount(root: THREE.Object3D): number {
  let n = 0;
  root.traverse((o) => {
    if (o.name === "board-slab") n += (o.userData.holes as number) ?? 0;
  });
  return n;
}

/** How many objects of each name the group holds (the pieces `buildBoardGroup` names: `board-slab`, `solder-mask-top`, `track`, `via`, `pad`, `solder-paste`, `zone-fill`, `silk-text`, `part-ref`, ...), for the test hook. */
export function countByName(root: THREE.Object3D): Record<string, number> {
  const counts: Record<string, number> = {};
  root.traverse((o) => {
    if (o.name && o !== root) counts[o.name] = (counts[o.name] ?? 0) + 1;
  });
  return counts;
}
