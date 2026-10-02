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
//   world X = board X, in mm (board µm / 1000), unflipped.
//   world Z = board Y, in mm (board µm / 1000), unflipped.
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
import type { BoardState, BoardText, Part, Shape, Side } from "../../api/types";
import { layerColor } from "../canvas/layers";
import { circleThrough, normalizeSweep } from "../canvas/painter";
import { bezierPolyline } from "../../kicad-port/bezierPoly";
import { drawStrokeText, measureStrokeText } from "../text/strokeFont";

// ---------------------------------------------------------------------
// Units
// ---------------------------------------------------------------------

/** Micrometres -> millimetres (this scene's Three.js unit). */
function mm(um: number): number {
  return um / 1000;
}

// ---------------------------------------------------------------------
// Stack-up (all mm)
// ---------------------------------------------------------------------

/** Board slab thickness -- standard 1.6mm FR4. */
export const BOARD_THICKNESS_MM = 1.6;
const HALF_THICKNESS_MM = BOARD_THICKNESS_MM / 2;
/** Copper foil thickness -- ~1oz copper (35µm), a real value, not tuned for looks. */
const COPPER_MM = 0.035;
/** Track/via-cap joints rendered a hair taller than straight copper so a
 * joint sitting exactly on a pad or another joint (very common -- tracks
 * terminate at pad centres) doesn't z-fight with it: both would
 * otherwise share the exact same top/bottom Y-planes. Purely a rendering
 * nicety, not a stack-up dimension. */
const JOINT_COPPER_MM = COPPER_MM + 0.002;
/** Solder mask tint thickness -- board_adapter.cpp's DEFAULT_TECH_LAYER_THICKNESS (`pcbIUScale.mmToIU(0.025)`), the same constant KiCad uses for both solder mask and silkscreen "technical layer" thickness. */
const MASK_MM = 0.025;
/** DEFAULT_COPPER_THICKNESS is 0.035mm already (COPPER_MM above matches); SOLDERPASTE_LAYER_THICKNESS is its own, thicker constant (`pcbIUScale.mmToIU(0.04)`) -- see addSolderPaste. */
const SOLDERPASTE_MM = 0.04;
/** Cosmetic-only gap between the mask surface and the silkscreen line loop, so the line never exactly coincides with the mask plane (z-fighting). Not a real stack-up dimension. */
const SILK_GAP_MM = 0.01;
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
  return outwardSign(side) * HALF_THICKNESS_MM;
}

function maskCenterY(side: Side): number {
  return surfaceY(side) + outwardSign(side) * (COPPER_MM / 2 + MASK_MM / 2);
}

function silkY(side: Side): number {
  return surfaceY(side) + outwardSign(side) * (COPPER_MM + MASK_MM + SILK_GAP_MM);
}

/** Part bodies sit directly on the board surface (task spec: "top parts sit above +0.8mm ... extending further up"), not stacked on top of the copper/mask -- those are sub-0.05mm and not worth the extra offset the spec didn't ask for. */
function partBodyCenterY(side: Side): number {
  return surfaceY(side) + outwardSign(side) * (PART_HEIGHT_MM / 2);
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
    return layerName.toLowerCase().startsWith("b.") ? -HALF_THICKNESS_MM : HALF_THICKNESS_MM;
  }
  if (layerOrder.length <= 1) return HALF_THICKNESS_MM;
  const t = idx / (layerOrder.length - 1); // 0 at the first (top) entry, 1 at the last (bottom)
  return HALF_THICKNESS_MM - t * BOARD_THICKNESS_MM;
}

// ---------------------------------------------------------------------
// Colors. Every material below reads a real value out of colors.json
// (extracted from common/settings/builtin_color_themes.h): the
// LAYER_3D_* keys are KiCad's own default 3D-viewer theme (background
// gradient, board, copper, mask, silkscreen) -- a *different* set of
// keys from the 2D editor's per-layer F_Cu/B_Cu/F_SilkS/etc (used
// elsewhere in this app via copperColorKey()), which exist to tell
// layers apart at a glance in the 2D canvas, not to look physically
// real (e.g. F_Mask is magenta, B_Mask cyan). The realistic 3D view
// uses the LAYER_3D_* set instead, matching real KiCad's own default 3D
// look rather than recoloring the 2D editor's distinguishing tints.
// ---------------------------------------------------------------------

/**
 * A plain silver-grey for SMD pads (HASL/tin finish) -- this app's
 * verified palette (colors.json, from builtin_color_themes.h) has no
 * dedicated "SMD finish" 3D color the way it does for the mask/copper/
 * silkscreen/background entries below, so this one hex is this file's
 * own reasonable, unsourced placeholder, called out rather than silently
 * presented as extracted. Everything else in this block (mask, copper,
 * silkscreen, background, IC body) is a real value, either from
 * colors.json's LAYER_3D_* keys or given verbatim by the task ("a
 * KiCad-like mid grey (about #3a3a3a)").
 */
const SMD_FINISH_GREY = 0xc8c8c8;
const IC_BODY_GREY = 0x3a3a3a;

/**
 * render_3d_opengl.cpp's own per-layer materials (setupMaterials/
 * setLayerMaterial, ogl_utils.cpp:OglSetMaterial) set a real per-layer
 * GL_SHININESS intended to vary by layer (copper ~13-51, soldermask
 * ~109-512, silkscreen/paste/board body all ~13) -- but OglSetMaterial's
 * own clamp (`shininess = 128 * min(m_Shininess, 1.0)`) is fed a value
 * that was *already* pre-multiplied by 128 at every one of those call
 * sites, so the `min(..., 1.0)` branch is unreachable and EVERY board
 * layer material actually renders with flat GL_SHININESS=128 (a tight,
 * glossy highlight) regardless of its intended value -- a real,
 * independently-confirmed quirk in KiCad's own code, not a port
 * approximation. This app's board-layer materials (copper/plated/zone
 * fill/mask/paste/board body -- NOT the placeholder part-body boxes,
 * which stand in for a real per-part 3D model file with its own
 * unrelated material, outside this code path entirely) use this single
 * roughness constant to match that *actual rendered* look rather than
 * the varied-but-never-reached intent. Blinn-Phong shininess 128 maps to
 * a PBR roughness of roughly sqrt(2/(128+2)) =~ 0.124 (standard
 * Beckmann-ish conversion); rounded up slightly for a small safety
 * margin in Three.js's own (different) BRDF.
 */
const BOARD_LAYER_ROUGHNESS = 0.15;

/**
 * KiCad theme colors are `#RRGGBBAA`; THREE.Color only understands RGB.
 * The theme itself already encodes translucency this way -- mask
 * entries are ~0xd4 alpha, everything else ~0xff -- so this alpha *is*
 * this app's verified "low opacity" for the solder mask, not a second,
 * separately-invented constant.
 */
function hexToColorAlpha(hex: string): { color: THREE.Color; alpha: number } {
  const clean = hex.startsWith("#") ? hex.slice(1) : hex;
  const rgb = clean.slice(0, 6);
  const a = clean.length >= 8 ? Number.parseInt(clean.slice(6, 8), 16) / 255 : 1;
  return { color: new THREE.Color(`#${rgb}`), alpha: Number.isFinite(a) ? a : 1 };
}

function hexToColor(hex: string): THREE.Color {
  return hexToColorAlpha(hex).color;
}

/**
 * One shared copper look for every track regardless of layer --
 * colors.json's LAYER_3D_COPPER_TOP (from builtin_color_themes.h) is
 * KiCad's *default* 3D-viewer copper color, a single gold-brass tone,
 * not the 2D editor's per-layer red/blue/etc (copperColorKey/layerColor
 * elsewhere in this app): the 2D canvas colors layers to tell them
 * apart at a glance, the realistic 3D view colors copper as copper.
 * Slightly raised + visible through the translucent mask above it (see
 * layerY/maskCenterY) is what reads as "copper under mask" here.
 */
function copperMaterial(): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color: hexToColor(layerColor("LAYER_3D_COPPER_TOP")), metalness: 0.75, roughness: BOARD_LAYER_ROUGHNESS });
}

/** Via barrels and through-hole pad rings: plated copper (ENIG/HASL gold). Slightly higher metalness than a flat trace (plating reads a little more mirror-like) -- roughness itself is the same flat BOARD_LAYER_ROUGHNESS as every other board-layer material (see that constant's own comment). */
function platedMaterial(): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color: hexToColor(layerColor("LAYER_3D_COPPER_TOP")), metalness: 0.9, roughness: BOARD_LAYER_ROUGHNESS });
}

/** SMD pad lands: silver-grey HASL/tin finish, distinct from a via/TH pad's gold plating -- see the reference renders this task was matched against. */
function smdPadMaterial(): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color: SMD_FINISH_GREY, metalness: 0.6, roughness: BOARD_LAYER_ROUGHNESS });
}

/** render_3d_opengl.cpp's own solder-paste material (setLayerMaterial, F_Paste/B_Paste): a dull grey solder-cream look -- metalness kept moderate (it's a cream of tiny metal particles, not a solid plated surface) rather than as shiny as plated copper. New in this port (phase 3) -- this app previously drew no solder-paste geometry at all. */
function pasteMaterial(): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color: hexToColor(layerColor("LAYER_3D_SOLDERPASTE")), metalness: 0.4, roughness: BOARD_LAYER_ROUGHNESS });
}

function maskMaterial(side: Side): THREE.MeshStandardMaterial {
  const { color, alpha } = hexToColorAlpha(layerColor(side === "top" ? "LAYER_3D_SOLDERMASK_TOP" : "LAYER_3D_SOLDERMASK_BOTTOM"));
  return new THREE.MeshStandardMaterial({
    color,
    transparent: true,
    opacity: alpha,
    // ShapeGeometry is one-sided and we do not control (or trust) the
    // winding direction of an arbitrary board outline -- see buildSlab's
    // comment. DoubleSide + no depthWrite keeps a translucent tint
    // layer correct from either side without needing to inspect/repair
    // the outline's winding.
    side: THREE.DoubleSide,
    depthWrite: false,
    roughness: BOARD_LAYER_ROUGHNESS,
    metalness: 0.1,
  });
}

function silkMaterial(side: Side): THREE.LineBasicMaterial {
  return new THREE.LineBasicMaterial({ color: hexToColor(layerColor(side === "top" ? "LAYER_3D_SILKSCREEN_TOP" : "LAYER_3D_SILKSCREEN_BOTTOM")) });
}

/** The board substrate -- colors.json's LAYER_3D_BOARD (from builtin_color_themes.h), a near-black dark brown that reads almost black on the slab's vertical edge under normal lighting, matching KiCad's own 3D render. */
function boardMaterial(): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color: hexToColor(layerColor("LAYER_3D_BOARD")), roughness: BOARD_LAYER_ROUGHNESS, metalness: 0.05, side: THREE.DoubleSide });
}

/**
 * No real per-part color data (see PART_HEIGHT_MM), so this app's own
 * KiCad-package heuristic (already used for the schematic's passive
 * glyphs -- schematic/layout.ts's isTwoPinPassive) stands in: a mid grey
 * for anything else (ICs, connectors, switches...) -- IC_BODY_GREY,
 * given verbatim by the task ("a KiCad-like mid grey (about #3a3a3a)")
 * because a near-black body was reading as a hole in the board -- tan
 * for a recognized 2-pin passive, matching KiCad's own default
 * placeholder body colors.
 */
function icBodyMaterial(): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color: IC_BODY_GREY, roughness: 0.7, metalness: 0.2 });
}
function passiveBodyMaterial(): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color: 0xc9a876, roughness: 0.6, metalness: 0.1 });
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
 * The board slab: `shape`'s polygon extruded to BOARD_THICKNESS_MM.
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
    const geometry = new THREE.ExtrudeGeometry(shape, { depth: BOARD_THICKNESS_MM, bevelEnabled: false });
    const mesh = new THREE.Mesh(geometry, boardMaterial());
    mesh.rotation.x = Math.PI / 2;
    mesh.position.y = BOARD_THICKNESS_MM / 2;
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
 * Simplification: covers the *entire* outline uniformly, not just the
 * copper-adjacent areas / minus pad apertures the way real solder mask
 * does -- this data model has no mask-aperture geometry to draw
 * instead.
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
// Copper: pads, tracks, vias
// ---------------------------------------------------------------------

function addPad(group: THREE.Group, xMm: number, zMm: number, wMm: number, hMm: number, round: boolean, y: number, material: THREE.Material, thicknessMm: number = COPPER_MM): void {
  const w = Math.max(wMm, 0.001);
  const h = Math.max(hMm, 0.001);
  let mesh: THREE.Mesh;
  if (round) {
    // Unit-radius cylinder non-uniformly scaled into an ellipse -- exact
    // for a circular pad (w === h) and a reasonable stand-in for an
    // oval one (PadShape::Oval also reports `round: true`; the data
    // model gives no separate "capsule" shape).
    const geometry = new THREE.CylinderGeometry(1, 1, thicknessMm, 24);
    mesh = new THREE.Mesh(geometry, material);
    mesh.scale.set(w / 2, 1, h / 2);
  } else {
    const geometry = new THREE.BoxGeometry(w, thicknessMm, h);
    mesh = new THREE.Mesh(geometry, material);
  }
  mesh.position.set(xMm, y, zMm);
  group.add(mesh);
}

/**
 * Solder paste (`SOLDERPASTE_MM` thick, board_adapter.cpp's
 * SOLDERPASTE_LAYER_THICKNESS): rendered on every SMD pad (`!pad.th`),
 * same footprint as the pad itself -- real KiCad derives a paste-stencil
 * aperture from the pad's own paste-margin setting (usually a touch
 * smaller than the pad), but this data model has no such margin, so the
 * pad's own outline stands in, per this file's established "use what the
 * model actually has" convention (see e.g. PART_HEIGHT_MM's own comment).
 * Sits just outside the copper surface, same side as the pad -- paste
 * covers exposed copper, not anything under the solder mask.
 */
function pasteCenterY(side: Side): number {
  return surfaceY(side) + outwardSign(side) * (COPPER_MM + SOLDERPASTE_MM / 2);
}

function addVia(group: THREE.Group, xMm: number, zMm: number, diameterMm: number, material: THREE.Material): void {
  const radius = Math.max(diameterMm / 2, 0.001);
  // Through every layer -- this app's model has no blind/buried vias.
  const geometry = new THREE.CylinderGeometry(radius, radius, BOARD_THICKNESS_MM, 24);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.position.set(xMm, 0, zMm);
  group.add(mesh);
}

/**
 * A track polyline rendered as a box per segment plus a cylinder cap at
 * every point (including endpoints) -- the caps round off each joint,
 * approximating KiCad's rounded-corner traces without needing a real
 * stroked-polyline mesh.
 */
function addTrack(group: THREE.Group, ptsMm: ReadonlyArray<[number, number]>, y: number, widthMm: number, material: THREE.Material): void {
  if (ptsMm.length === 0) return;
  const radius = Math.max(widthMm / 2, 0.001);
  const jointGeometry = new THREE.CylinderGeometry(radius, radius, JOINT_COPPER_MM, 12);
  for (const [x, z] of ptsMm) {
    const mesh = new THREE.Mesh(jointGeometry, material);
    mesh.position.set(x, y, z);
    group.add(mesh);
  }
  for (let i = 0; i + 1 < ptsMm.length; i++) {
    const [x0, z0] = ptsMm[i]!;
    const [x1, z1] = ptsMm[i + 1]!;
    const dx = x1 - x0;
    const dz = z1 - z0;
    const len = Math.hypot(dx, dz);
    if (len < 1e-6) continue;
    const geometry = new THREE.BoxGeometry(len, COPPER_MM, Math.max(widthMm, 0.001));
    const mesh = new THREE.Mesh(geometry, material);
    mesh.position.set((x0 + x1) / 2, y, (z0 + z1) / 2);
    // Rotate the box's local +X (its "length" axis) to point from p0 to p1.
    mesh.rotation.y = -Math.atan2(dz, dx);
    group.add(mesh);
  }
}

// ---------------------------------------------------------------------
// Zones: translucent copper
// ---------------------------------------------------------------------

/**
 * Fill isn't computed anywhere in this model (same reason the 2D painter
 * only draws a zone's outline) -- this renders the *outline polygon
 * itself* as a flat, translucent copper-colored fill, an honest stand-in
 * for "the pour would go here" rather than a real clearance-aware fill
 * shape. Same LAYER_3D_COPPER_TOP tone as tracks/pads (not the 2D
 * editor's per-layer red/blue), distinguished from solid copper only by
 * opacity.
 */
function zoneMaterial(): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({
    color: hexToColor(layerColor("LAYER_3D_COPPER_TOP")),
    transparent: true,
    opacity: 0.4,
    // Outline winding (board.routing.zones[].outline) is just as
    // untrusted as the board outline's -- see buildSlab's comment.
    // DoubleSide + no depthWrite keeps the translucent tint correct from
    // either side without needing to inspect the winding.
    side: THREE.DoubleSide,
    depthWrite: false,
    metalness: 0.3,
    roughness: BOARD_LAYER_ROUGHNESS,
  });
}

/**
 * Same layer height as `layerY`, nudged a hair toward the board's
 * vertical center so a track or pad sitting exactly on this layer (very
 * common -- a zone usually fills in around existing copper on the same
 * layer) doesn't z-fight with the zone fill beneath it. Cosmetic only,
 * same convention as JOINT_COPPER_MM/SILK_GAP_MM above.
 */
function zoneY(layerName: string, layerOrder: readonly string[]): number {
  const y = layerY(layerName, layerOrder);
  const eps = 0.004;
  if (y > 0) return y - eps;
  if (y < 0) return y + eps;
  return y;
}

function addZoneFill(group: THREE.Group, outlineMm: ReadonlyArray<[number, number]>, y: number, material: THREE.Material): void {
  const shape = buildOutlineShape(outlineMm);
  if (!shape) return; // degenerate outline (still being drawn, or <3 points) -- skip rather than fake one
  const geometry = new THREE.ShapeGeometry(shape);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.rotation.x = Math.PI / 2; // same local-XY -> world-XZ mapping as buildMask
  mesh.position.y = y;
  mesh.name = "zone-fill";
  group.add(mesh);
}

// ---------------------------------------------------------------------
// Silkscreen (courtyard-outline stand-in) + part bodies
// ---------------------------------------------------------------------

/**
 * Part body simplification: no real 3D model data exists for any part
 * (task spec: "There are no 3D models yet"). Every placed part renders
 * as a plain box sized to its courtyard footprint and PART_HEIGHT_MM
 * tall, sitting on the board surface on its own side. A lighter edge
 * outline (PART_EDGE_MATERIAL) is drawn on top of the box so a mid-grey
 * body still reads as a raised part against the board, not a flat patch
 * -- see PART_EDGE_MATERIAL's own comment.
 */
function addPartBody(group: THREE.Group, courtyardMm: readonly [number, number, number, number], side: Side, material: THREE.Material): void {
  const [minX, minY, maxX, maxY] = courtyardMm;
  const width = Math.max(maxX - minX, 0.01);
  const depth = Math.max(maxY - minY, 0.01);
  const geometry = new THREE.BoxGeometry(width, PART_HEIGHT_MM, depth);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.position.set((minX + maxX) / 2, partBodyCenterY(side), (minY + maxY) / 2);
  mesh.name = "part-body";
  const edges = new THREE.LineSegments(new THREE.EdgesGeometry(geometry), PART_EDGE_MATERIAL);
  mesh.add(edges);
  group.add(mesh);
}

// ---------------------------------------------------------------------
// Drawing-tool shapes and text, rendered as silk
// ---------------------------------------------------------------------

/** Same "B." prefix convention as layerY's own fallback branch above. */
function silkSide(layerName: string): Side {
  return layerName.toLowerCase().startsWith("b.") ? "bottom" : "top";
}

function silkFillMaterial(side: Side): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color: hexToColor(layerColor(side === "top" ? "LAYER_3D_SILKSCREEN_TOP" : "LAYER_3D_SILKSCREEN_BOTTOM")), roughness: 0.9, metalness: 0, side: THREE.DoubleSide });
}

const ARC_SEGMENTS = 24;

/**
 * mm-space polyline for one drawing-tool shape, plus whether it's a
 * closed loop (rect/circle/polygon) or open (segment/arc). Arc samples
 * the *true* circumcircle through start/mid/end -- circleThrough/
 * normalizeSweep, imported from the 2D painter rather than
 * re-implemented here -- so this traces the same curve the 2D canvas
 * draws, not the two-chord stand-in itemHitTest.ts uses for hit-testing
 * (that one only needs a distance, not a shape to render).
 */
function shapePolylinePts(s: Shape): { pts: Array<[number, number]>; closed: boolean } {
  switch (s.kind) {
    case "segment":
      return { pts: [[mm(s.start[0]), mm(s.start[1])], [mm(s.end[0]), mm(s.end[1])]], closed: false };
    case "rect": {
      const [x0, y0] = s.start;
      const [x1, y1] = s.end;
      return {
        pts: [[mm(x0), mm(y0)], [mm(x1), mm(y0)], [mm(x1), mm(y1)], [mm(x0), mm(y1)]],
        closed: true,
      };
    }
    case "circle": {
      const r = Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1]);
      const pts: Array<[number, number]> = [];
      for (let i = 0; i < ARC_SEGMENTS; i++) {
        const a = (i / ARC_SEGMENTS) * Math.PI * 2;
        pts.push([mm(s.center[0] + r * Math.cos(a)), mm(s.center[1] + r * Math.sin(a))]);
      }
      return { pts, closed: true };
    }
    case "polygon":
      return { pts: s.pts.map(([x, y]) => [mm(x), mm(y)] as [number, number]), closed: true };
    case "arc": {
      const circle = circleThrough(s.start, s.mid, s.end);
      if (!circle) {
        // Degenerate (collinear) points: a straight line is the same
        // fallback the 2D painter uses.
        return { pts: [[mm(s.start[0]), mm(s.start[1])], [mm(s.end[0]), mm(s.end[1])]], closed: false };
      }
      const [cx, cy, r] = circle;
      const a0 = Math.atan2(s.start[1] - cy, s.start[0] - cx);
      const aMid = Math.atan2(s.mid[1] - cy, s.mid[0] - cx);
      const a1 = Math.atan2(s.end[1] - cy, s.end[0] - cx);
      const ccw = normalizeSweep(a0, aMid, a1);
      const twoPi = Math.PI * 2;
      const fwd = (x: number) => ((x % twoPi) + twoPi) % twoPi;
      const span = ccw ? fwd(a1 - a0) : -fwd(a0 - a1);
      const pts: Array<[number, number]> = [];
      for (let i = 0; i <= ARC_SEGMENTS; i++) {
        const a = a0 + (span * i) / ARC_SEGMENTS;
        pts.push([mm(cx + r * Math.cos(a)), mm(cy + r * Math.sin(a))]);
      }
      return { pts, closed: false };
    }
    case "bezier":
      // `BEZIER_POLY::GetPoly` at the board's max error -- the same flattening the 2D canvas draws.
      return { pts: bezierPolyline(s.start, s.c1, s.c2, s.end, 5).map(([x, y]) => [mm(x), mm(y)] as [number, number]), closed: false };
  }
}

/**
 * One drawing-tool shape (Place > Line/Arc/Rectangle/Circle/Polygon),
 * rendered as silk regardless of its actual `layer` string -- the
 * drawing tools can place a shape on any layer (whatever's active when
 * drawn), but per the task spec this view always treats them as
 * silkscreen artwork, on whichever side the layer name's "B."/"F."
 * prefix suggests (silkSide), the same simplification already used for
 * the courtyard-outline stand-in below. A filled shape gets a flat white
 * fill (real screen-printed silkscreen fills are commonly solid); an
 * unfilled one gets the same white line-loop/line-strip treatment as the
 * courtyard outline.
 */
function addSilkShape(group: THREE.Group, s: Shape, lineMaterial: THREE.LineBasicMaterial, fillMaterial: THREE.Material): void {
  const y = silkY(silkSide(s.layer));
  const { pts, closed } = shapePolylinePts(s);
  if (pts.length < 2) return;
  if (s.filled && closed) {
    const shape = buildOutlineShape(pts);
    if (shape) {
      const geometry = new THREE.ShapeGeometry(shape);
      const mesh = new THREE.Mesh(geometry, fillMaterial);
      mesh.rotation.x = Math.PI / 2; // same local-XY -> world-XZ mapping as buildMask/addZoneFill
      mesh.position.y = y;
      mesh.name = "silk-shape-fill";
      group.add(mesh);
      return;
    }
  }
  const points = pts.map(([x, z]) => new THREE.Vector3(x, y, z));
  const geometry = new THREE.BufferGeometry().setFromPoints(points);
  const line = closed ? new THREE.LineLoop(geometry, lineMaterial) : new THREE.Line(geometry, lineMaterial);
  line.name = "silk-shape-line";
  group.add(line);
}

/** Texture resolution for silk text -- pixels per mm of text height, chosen crisp enough for typical zoom levels (this becomes strokeFont.ts's own "sizeUm" unit for the offscreen canvas, in pixels rather than um -- see buildSilkTextMesh). */
const TEXT_TEXTURE_PX_PER_MM = 48;

/**
 * Free-standing board text (Place > Text) in 3D, and every placed
 * part's own reference designator (buildRefDesignatorMesh, below) --
 * both draw KiCad's real Newstroke font (strokeFont.ts) into an
 * offscreen canvas, then map that canvas onto a flat plane in the silk
 * layer. A texture rather than real 3D glyph geometry (extruded stroke
 * paths) -- legible and correctly shaped either way, and far cheaper
 * than building/disposing real geometry per character on every board
 * edit.
 */
interface SilkTextSpec {
  content: string;
  xMm: number;
  zMm: number;
  sizeMm: number;
  /** Real per-item stroke width when the caller has one (BoardText does; a part's reference designator doesn't, so buildRefDesignatorMesh leaves this undefined and drawStrokeText falls back to its own default). */
  thicknessMm?: number;
  angleDeg: number;
  side: Side;
  justify: "left" | "center" | "right";
  mirror: boolean;
  name: string;
}

/** Shared by buildTextMesh (Place > Text) and buildRefDesignatorMesh (every placed part's silk reference) -- see either caller's own comment for why a rendered-string texture stands in for real glyph outlines. */
function buildSilkTextMesh(spec: SilkTextSpec): THREE.Mesh {
  const sizeMm = Math.max(spec.sizeMm, 0.01);
  const px = Math.max(TEXT_TEXTURE_PX_PER_MM * sizeMm, 1);
  const content = spec.content || " ";

  // Measure in the same "mm" unit the caller already works in (this
  // function's own xMm/zMm/sizeMm), then convert to texture pixels --
  // strokeFont.ts's layout doesn't care what unit its caller uses, as
  // long as thickness/positions are all expressed in that same unit.
  const textWidthPx = Math.max(measureStrokeText(content, sizeMm), 0.01) * px;
  const canvas = document.createElement("canvas");
  const padPx = px * 0.3; // room for stroke width + descenders past the nominal em box
  canvas.width = Math.ceil(textWidthPx) + padPx * 2;
  canvas.height = Math.ceil(px * 1.3);
  const ctx = canvas.getContext("2d")!;
  // Baseline sits a bit above vertical center (descenders need room
  // below it too), matching the old fillText version's rough centering.
  drawStrokeText(ctx, content, padPx, canvas.height * 0.6, {
    sizeUm: px, // "Um" in name only -- strokeFont.ts is unit-agnostic; this call's unit is texture pixels
    thicknessUm: spec.thicknessMm !== undefined ? spec.thicknessMm * (px / sizeMm) : undefined,
    justify: "left",
    color: "#ffffff",
  });

  const texture = new THREE.CanvasTexture(canvas);
  texture.minFilter = THREE.LinearFilter;

  // Re-derived from the actual (padded) canvas size, not the raw text
  // measurement, so the plane's aspect ratio always matches the
  // texture's -- otherwise the padding added above would stretch the
  // mapped text horizontally.
  const widthMm = (canvas.width / px) * sizeMm;
  const heightMm = (canvas.height / px) * sizeMm;
  const geometry = new THREE.PlaneGeometry(widthMm, heightMm);
  // The texture itself was always drawn left-aligned -- anchor (x,z) at
  // the plane's left/center/right edge per `justify` by shifting the
  // geometry in its own local space, before the flatten/yaw below, the
  // same way the 2D canvas's translate-then-rotate-then-textAlign order
  // keeps the alignment offset in the text's own (unrotated) frame.
  const offsetX = spec.justify === "left" ? widthMm / 2 : spec.justify === "right" ? -widthMm / 2 : 0;
  geometry.translate(offsetX, 0, 0);
  // Flatten the plane (local XY, facing local +Z) into this file's
  // world board plane (X/Z), facing outward from `side` -- the same
  // local-XY -> world-XZ mapping as buildMask/addZoneFill, mirrored for
  // the bottom side so the printed face looks outward from the board
  // rather than into it.
  geometry.rotateX(spec.side === "top" ? -Math.PI / 2 : Math.PI / 2);

  const material = new THREE.MeshBasicMaterial({ map: texture, transparent: true, side: THREE.DoubleSide, depthWrite: false });
  const mesh = new THREE.Mesh(geometry, material);
  mesh.position.set(spec.xMm, silkY(spec.side), spec.zMm);
  mesh.rotation.y = -(spec.angleDeg * Math.PI) / 180;
  if (spec.mirror) mesh.scale.x *= -1;
  mesh.name = spec.name;
  return mesh;
}

/** Free-standing board text (Place > Text) in 3D -- see buildSilkTextMesh's own comment. */
function buildTextMesh(t: BoardText): THREE.Mesh {
  return buildSilkTextMesh({
    content: t.content,
    xMm: mm(t.x),
    zMm: mm(t.y),
    sizeMm: mm(t.size),
    thicknessMm: mm(t.stroke_width),
    // Millidegrees, CCW-positive in board space -- same conversion and
    // sign the 2D painter's drawTexts uses for its own (canvas)
    // rotation; kept consistent here even though this file's world axes
    // are a separate (if parallel/unmirrored) convention from the 2D
    // canvas's.
    angleDeg: t.angle / 1000,
    side: silkSide(t.layer),
    justify: t.justify,
    mirror: t.mirror,
    name: "silk-text",
  });
}

/**
 * Every placed part's own reference designator ("R1", "U1", ...) as
 * white silk text -- KiCad's real 3D render shows this for every part
 * (no courtyard rectangle; this app drew one as a stand-in before real
 * text rendering existed here, which doesn't match and has been
 * removed). Position/size/side-of-board logic (courtyard center, font
 * size clamped to the courtyard's own size, `part.label` choosing
 * above/below/left/right of the box) is ported straight from
 * components/canvas/painter.ts's drawFootprint -- same convention as the
 * 2D PCB canvas's own reference-designator label, including *not*
 * rotating the text with `part.rot` (that 2D convention, kept here
 * rather than inventing a second one for 3D).
 */
function buildRefDesignatorMesh(part: Part): THREE.Mesh | null {
  const courtyard = part.courtyard;
  const side = part.side;
  if (!courtyard || !side) return null;
  const [x0, y0, x1, y1] = courtyard;
  const sizeUm = Math.max(500, Math.min(900, Math.min(x1 - x0, y1 - y0) * 0.45));
  const cx = (x0 + x1) / 2;
  const cy = (y0 + y1) / 2;
  let tx = cx;
  let ty = y0 - sizeUm * 0.3;
  let justify: "left" | "center" | "right" = "center";
  if (part.label === "below") ty = y1 + sizeUm * 0.95;
  if (part.label === "left") {
    tx = x0 - sizeUm * 0.3;
    ty = cy;
    justify = "right";
  }
  if (part.label === "right") {
    tx = x1 + sizeUm * 0.3;
    ty = cy;
    justify = "left";
  }
  return buildSilkTextMesh({
    content: part.ref,
    xMm: mm(tx),
    zMm: mm(ty),
    sizeMm: mm(sizeUm),
    angleDeg: 0,
    side,
    justify,
    mirror: false,
    name: "part-ref",
  });
}

// ---------------------------------------------------------------------
// Top-level: board -> Group
// ---------------------------------------------------------------------

export interface BuildBoardOptions {
  /** KiCad's 3D viewer can hide its rendered component models independently of the board/copper/silk -- there are no real models here yet (see PART_HEIGHT_MM), so this hides the placeholder boxes instead. Independent of showSilkscreen: a part's reference designator is silkscreen ink, not a 3D model, same as real KiCad. Default true. Further filtered per part by showTHT/showSMD below. */
  showComponents?: boolean;
  /** render.show_footprints_normal's rough equivalent: a part counts as "TH" if any of its pads has `th: true`. Default true. */
  showTHT?: boolean;
  /** The SMD counterpart: a part with no through-hole pads at all. Default true. */
  showSMD?: boolean;
  /** Default true. */
  showSilkscreen?: boolean;
  /** Default true. */
  showSolderMask?: boolean;
  /** render.show_solderpaste. Default true. */
  showSolderPaste?: boolean;
  /** render.show_board_body -- the dielectric slab itself. Default true. */
  showBoardBody?: boolean;
  /** render.opengl_show_model_bbox, default false (matches source). */
  showBoundingBoxes?: boolean;
}

/** A part counts as through-hole if it places at least one `th` pad -- matches real KiCad's own per-footprint "attribute" (Through hole / SMD / Virtual), which this data model doesn't carry directly, so it's inferred from pad data instead. */
function partIsThroughHole(part: Part): boolean {
  return (part.pads ?? []).some((p) => p.th);
}

/** A translucent yellow wireframe box, matching KiCad's own debug bounding-box color closely enough for a toggle most users leave off (render.opengl_show_model_bbox default false, see BuildBoardOptions). */
const BOUNDING_BOX_MATERIAL = new THREE.LineBasicMaterial({ color: 0xffff00 });

function addBoundingBox(group: THREE.Group, courtyardMm: readonly [number, number, number, number], side: Side): void {
  const [minX, minY, maxX, maxY] = courtyardMm;
  const width = Math.max(maxX - minX, 0.01);
  const depth = Math.max(maxY - minY, 0.01);
  const geometry = new THREE.BoxGeometry(width, PART_HEIGHT_MM, depth);
  const edges = new THREE.LineSegments(new THREE.EdgesGeometry(geometry), BOUNDING_BOX_MATERIAL);
  edges.position.set((minX + maxX) / 2, partBodyCenterY(side), (minY + maxY) / 2);
  edges.name = "part-bbox";
  group.add(edges);
}

/** Builds the whole board as a single Group. Safe to call with a board that has no outline and/or no placed parts -- renders whatever subset of the geometry makes sense, never throws. */
export function buildBoardGroup(board: BoardState, opts: BuildBoardOptions = {}): THREE.Group {
  const showComponents = opts.showComponents ?? true;
  const showTHT = opts.showTHT ?? true;
  const showSMD = opts.showSMD ?? true;
  const showSilkscreen = opts.showSilkscreen ?? true;
  const showSolderMask = opts.showSolderMask ?? true;
  const showSolderPaste = opts.showSolderPaste ?? true;
  const showBoardBody = opts.showBoardBody ?? true;
  const showBoundingBoxes = opts.showBoundingBoxes ?? false;

  const group = new THREE.Group();
  group.name = "viewer3d-board";

  const outlineMm: Array<[number, number]> = (board.outline ?? []).map(([x, y]) => [mm(x), mm(y)]);
  const shape = buildOutlineShape(outlineMm);
  if (shape) {
    if (showBoardBody) {
      const slab = buildSlab(shape);
      if (slab) group.add(slab);
    }
    if (showSolderMask) {
      group.add(buildMask(shape, "top"));
      group.add(buildMask(shape, "bottom"));
    }
  }

  const layerOrder = board.layers.length > 0 ? board.layers : ["F.Cu", "B.Cu"];

  // One shared material per kind, reused across every element instead of
  // allocating a new one per pad/track/via/zone (KiCad's default 3D
  // theme has a single copper/mask/silk tone each, not a per-layer set).
  const copperMat = copperMaterial();
  const zoneMat = zoneMaterial();

  if (board.routing) {
    for (const track of board.routing.tracks) {
      const y = layerY(track.layer, layerOrder);
      const ptsMm: Array<[number, number]> = track.pts.map(([x, yy]) => [mm(x), mm(yy)]);
      addTrack(group, ptsMm, y, mm(track.width), copperMat);
    }
    if (board.routing.vias.length > 0) {
      const viaMat = platedMaterial();
      for (const via of board.routing.vias) {
        addVia(group, mm(via.x), mm(via.y), mm(via.d), viaMat);
      }
    }
    if (board.routing.zones.length > 0) {
      for (const zone of board.routing.zones) {
        const y = zoneY(zone.layer, layerOrder);
        const outlineMm: Array<[number, number]> = zone.outline.map(([x, yy]) => [mm(x), mm(yy)]);
        addZoneFill(group, outlineMm, y, zoneMat);
      }
    }
  }

  const topCuY = layerY("F.Cu", layerOrder);
  const botCuY = layerY("B.Cu", layerOrder);
  const platedMat = platedMaterial(); // via barrels above already built their own; this one's for through-hole pad rings
  const smdMat = smdPadMaterial();
  const pasteMat = pasteMaterial();
  const silkLineMat = { top: silkMaterial("top"), bottom: silkMaterial("bottom") };
  const silkFillMat = { top: silkFillMaterial("top"), bottom: silkFillMaterial("bottom") };
  const icMat = icBodyMaterial();
  const passiveMat = passiveBodyMaterial();

  for (const part of board.parts) {
    const side = part.side;
    if (!part.placed || !side) continue;
    // render.show_footprints_normal/virtual's rough equivalent -- gates
    // the part's pads, body AND reference text together (real KiCad's
    // per-footprint TH/SMD attribute hides the whole footprint, not just
    // its 3D model).
    const partVisible = partIsThroughHole(part) ? showTHT : showSMD;
    if (!partVisible) continue;

    for (const pad of part.pads ?? []) {
      const y = side === "top" ? topCuY : botCuY;
      addPad(group, mm(pad.x), mm(pad.y), mm(pad.w), mm(pad.h), pad.round, y, pad.th ? platedMat : smdMat);
      // Solder paste only applies to exposed SMD pads (a through-hole pad
      // is soldered from the opposite side through the barrel, not pasted).
      if (showSolderPaste && !pad.th) {
        addPad(group, mm(pad.x), mm(pad.y), mm(pad.w), mm(pad.h), pad.round, pasteCenterY(side), pasteMat, SOLDERPASTE_MM);
      }
    }

    // Independent toggles, matching real KiCad: "show silkscreen" and
    // "show components" gate different things (the reference text is
    // silkscreen ink, not a 3D model), not one hiding the other.
    if (showSilkscreen) {
      const refMesh = buildRefDesignatorMesh(part);
      if (refMesh) group.add(refMesh);
    }
    if (showComponents) {
      const courtyard = part.courtyard;
      if (courtyard) {
        const courtyardMm: [number, number, number, number] = [mm(courtyard[0]), mm(courtyard[1]), mm(courtyard[2]), mm(courtyard[3])];
        addPartBody(group, courtyardMm, side, isPassivePart(part) ? passiveMat : icMat);
        if (showBoundingBoxes) addBoundingBox(group, courtyardMm, side);
      }
    }
  }

  if (board.drawings && showSilkscreen) {
    for (const s of board.drawings.shapes) {
      const side = silkSide(s.layer);
      addSilkShape(group, s, silkLineMat[side], silkFillMat[side]);
    }
    for (const t of board.drawings.texts) {
      group.add(buildTextMesh(t));
    }
  }

  return group;
}

/**
 * KiCad's 3D viewer background: a vertical lavender-to-grey gradient
 * (LAYER_3D_BACKGROUND_TOP/BOTTOM, colors.json), not a flat color. A
 * flat Scene.background can only be one color, so this renders the
 * two-stop gradient into a tall, 1px-wide canvas and uses that as the
 * scene background texture instead -- cheap (built once per mount, not
 * per frame) and needs no skybox mesh/extra draw call.
 */
export function buildBackgroundTexture(): THREE.CanvasTexture {
  const canvas = document.createElement("canvas");
  canvas.width = 1;
  canvas.height = 256;
  const ctx = canvas.getContext("2d")!;
  const gradient = ctx.createLinearGradient(0, 0, 0, canvas.height);
  gradient.addColorStop(0, layerColor("LAYER_3D_BACKGROUND_TOP"));
  gradient.addColorStop(1, layerColor("LAYER_3D_BACKGROUND_BOTTOM"));
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

/** Board-space (mm, world X/Z) bounds of `board.outline` alone -- null when there's no usable outline. Cheap on purpose: computed on every board update just to decide whether the camera needs re-fitting, without building the full Three.js group first. */
export function boardOutlineBounds(board: BoardState): OutlineBounds | null {
  const outline = board.outline;
  if (!outline || outline.length < 3) return null;
  let minX = Infinity;
  let minZ = Infinity;
  let maxX = -Infinity;
  let maxZ = -Infinity;
  for (const [x, y] of outline) {
    const mx = mm(x);
    const mz = mm(y);
    if (mx < minX) minX = mx;
    if (mz < minZ) minZ = mz;
    if (mx > maxX) maxX = mx;
    if (mz > maxZ) maxZ = mz;
  }
  return { minX, minZ, maxX, maxZ };
}
