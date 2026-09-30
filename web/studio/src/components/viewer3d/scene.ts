// Pure Three.js geometry builders for the KiCad-style 3D PCB viewer.
// No React, no DOM access beyond THREE's own APIs -- Viewer3D.tsx owns
// the renderer/scene/camera/controls lifecycle and just calls into here
// to (re)build the "board group" whenever `state.board` changes, and to
// compute a camera pose for each of the 8 toolbar view presets.
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
import { layerColor, copperColorKey } from "../canvas/layers";
import { circleThrough, normalizeSweep } from "../canvas/painter";

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
/** Solder mask tint thickness -- thin translucent layer over the copper, a real-ish value. */
const MASK_MM = 0.015;
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
// Colors. Copper (tracks) reuses this app's own source-verified 2D
// theme (colors.json via layerColor()/copperColorKey()) -- KiCad's real
// 2D editor colors F.Cu/B.Cu red/blue to tell layers apart, not by
// physical appearance, and this view keeps that same distinction rather
// than inventing a third, unverified "3D copper" color. Mask, pads and
// silk get KiCad's usual realistic 3D-viewer look instead (green/gold
// ENIG/white, per explicit instruction): this app's 2D theme colors for
// those layers are bright layer-distinction tints for the 2D editor
// (e.g. F_Mask is magenta, B_Mask cyan, F_SilkS pale yellow), not real
// physical colors, so reusing them here would look wrong.
// ---------------------------------------------------------------------

const REALISTIC_LOOK = {
  maskGreen: 0x1a5c2e,
  maskOpacity: 0.82,
  platedGold: 0xd4af37,
  silkWhite: 0xf2f2f2,
};

/**
 * KiCad theme colors are `#RRGGBBAA`; THREE.Color only understands RGB.
 * The theme itself already encodes translucency this way -- mask
 * entries are ~0x66 alpha, everything else ~0xff -- so this alpha *is*
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

function copperMaterial(bucketKey: string): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color: hexToColor(layerColor(bucketKey)), metalness: 0.7, roughness: 0.4 });
}

/** Pads and via barrels: both are plated/finished copper features (ENIG = gold, KiCad's usual 3D-viewer default finish), sharing one material rather than two identical ones. */
function platedMaterial(): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color: REALISTIC_LOOK.platedGold, metalness: 0.85, roughness: 0.3 });
}

function maskMaterial(): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({
    color: REALISTIC_LOOK.maskGreen,
    transparent: true,
    opacity: REALISTIC_LOOK.maskOpacity,
    // ShapeGeometry is one-sided and we do not control (or trust) the
    // winding direction of an arbitrary board outline -- see buildSlab's
    // comment. DoubleSide + no depthWrite keeps a translucent tint
    // layer correct from either side without needing to inspect/repair
    // the outline's winding.
    side: THREE.DoubleSide,
    depthWrite: false,
    roughness: 0.8,
    metalness: 0.1,
  });
}

function silkMaterial(): THREE.LineBasicMaterial {
  return new THREE.LineBasicMaterial({ color: REALISTIC_LOOK.silkWhite });
}

/**
 * Edge_Cuts (this app's only board-outline-adjacent verified color) is a
 * bright 2D drawing-layer stroke color, not a laminate/substrate color
 * -- using it would tint the whole slab a garish outline-yellow. There
 * is no verified substrate color in this app's palette (colors.json is
 * 2D-canvas layer colors; raw FR4 laminate is never one of those
 * layers), so this is an explicit, arbitrary, unverified placeholder
 * (a plausible raw-fibreglass tan), not a KiCad color -- called out here
 * rather than silently presented as sourced.
 */
function boardMaterial(): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color: 0xc9b380, roughness: 0.85, metalness: 0.05, side: THREE.DoubleSide });
}

/**
 * No real per-part color data (see PART_HEIGHT_MM), so this app's own
 * KiCad-package heuristic (already used for the schematic's passive
 * glyphs -- schematic/layout.ts's isTwoPinPassive) stands in: dark grey
 * for anything else (ICs, connectors, switches...), tan for a
 * recognized 2-pin passive, matching KiCad's own default placeholder
 * body colors.
 */
function icBodyMaterial(): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color: 0x2a2a2a, roughness: 0.7, metalness: 0.2 });
}
function passiveBodyMaterial(): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color: 0xc9a876, roughness: 0.6, metalness: 0.1 });
}
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
  const mesh = new THREE.Mesh(geometry, maskMaterial());
  mesh.rotation.x = Math.PI / 2;
  mesh.position.y = maskCenterY(side);
  mesh.name = `solder-mask-${side}`;
  return mesh;
}

// ---------------------------------------------------------------------
// Copper: pads, tracks, vias
// ---------------------------------------------------------------------

function addPad(group: THREE.Group, xMm: number, zMm: number, wMm: number, hMm: number, round: boolean, y: number, material: THREE.Material): void {
  const w = Math.max(wMm, 0.001);
  const h = Math.max(hMm, 0.001);
  let mesh: THREE.Mesh;
  if (round) {
    // Unit-radius cylinder non-uniformly scaled into an ellipse -- exact
    // for a circular pad (w === h) and a reasonable stand-in for an
    // oval one (PadShape::Oval also reports `round: true`; the data
    // model gives no separate "capsule" shape).
    const geometry = new THREE.CylinderGeometry(1, 1, COPPER_MM, 24);
    mesh = new THREE.Mesh(geometry, material);
    mesh.scale.set(w / 2, 1, h / 2);
  } else {
    const geometry = new THREE.BoxGeometry(w, COPPER_MM, h);
    mesh = new THREE.Mesh(geometry, material);
  }
  mesh.position.set(xMm, y, zMm);
  group.add(mesh);
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
 * shape. Reuses the same per-layer copper color as tracks/pads
 * (copperColorKey/layerColor) so a zone reads as "the same layer" as the
 * copper it's on, distinguished from solid copper only by opacity.
 */
function zoneMaterial(bucketKey: string): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({
    color: hexToColor(layerColor(bucketKey)),
    transparent: true,
    opacity: 0.4,
    // Outline winding (board.routing.zones[].outline) is just as
    // untrusted as the board outline's -- see buildSlab's comment.
    // DoubleSide + no depthWrite keeps the translucent tint correct from
    // either side without needing to inspect the winding.
    side: THREE.DoubleSide,
    depthWrite: false,
    metalness: 0.3,
    roughness: 0.6,
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
 * Silkscreen simplification: this data model has no font/stroke glyph
 * data (no per-character outlines), so drawing actual silkscreen TEXT
 * in 3D is out of scope. As a stand-in that still shows where every
 * part sits, this draws each placed part's courtyard rectangle as a
 * thin line loop in the silkscreen color. This is not real silkscreen
 * art.
 */
function addSilkOutline(group: THREE.Group, courtyardMm: readonly [number, number, number, number], side: Side, material: THREE.LineBasicMaterial): void {
  const [minX, minY, maxX, maxY] = courtyardMm;
  const y = silkY(side);
  const points = [new THREE.Vector3(minX, y, minY), new THREE.Vector3(maxX, y, minY), new THREE.Vector3(maxX, y, maxY), new THREE.Vector3(minX, y, maxY)];
  const geometry = new THREE.BufferGeometry().setFromPoints(points);
  const loop = new THREE.LineLoop(geometry, material);
  group.add(loop);
}

/**
 * Part body simplification: no real 3D model data exists for any part
 * (task spec: "There are no 3D models yet"). Every placed part renders
 * as a plain box sized to its courtyard footprint and PART_HEIGHT_MM
 * tall, sitting on the board surface on its own side.
 */
function addPartBody(group: THREE.Group, courtyardMm: readonly [number, number, number, number], side: Side, material: THREE.Material): void {
  const [minX, minY, maxX, maxY] = courtyardMm;
  const width = Math.max(maxX - minX, 0.01);
  const depth = Math.max(maxY - minY, 0.01);
  const geometry = new THREE.BoxGeometry(width, PART_HEIGHT_MM, depth);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.position.set((minX + maxX) / 2, partBodyCenterY(side), (minY + maxY) / 2);
  group.add(mesh);
}

// ---------------------------------------------------------------------
// Drawing-tool shapes and text, rendered as silk
// ---------------------------------------------------------------------

/** Same "B." prefix convention as layerY's own fallback branch above. */
function silkSide(layerName: string): Side {
  return layerName.toLowerCase().startsWith("b.") ? "bottom" : "top";
}

function silkFillMaterial(): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color: REALISTIC_LOOK.silkWhite, roughness: 0.9, metalness: 0, side: THREE.DoubleSide });
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

/** Texture resolution for silk text -- arbitrary but crisp-enough pixels per mm of text height, not a real font metric (there is no font/glyph data in this model, see below). */
const TEXT_TEXTURE_PX_PER_MM = 48;

/**
 * Free-standing board text (Place > Text) in 3D. Like the courtyard
 * outline below, this data model has no font/stroke glyph outlines to
 * draw as real vector silkscreen art (same limitation the 2D painter's
 * own drawTexts notes) -- instead of a bare bounding-box line loop,
 * though, this renders the actual string into an offscreen canvas and
 * maps it onto a flat plane in the silk layer, so the label is legible
 * in the 3D view rather than just a placeholder box.
 */
function buildTextMesh(t: BoardText): THREE.Mesh {
  const side = silkSide(t.layer);
  const sizeMm = Math.max(mm(t.size), 0.01);
  const px = Math.max(TEXT_TEXTURE_PX_PER_MM * sizeMm, 1);
  const font = `${px}px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`;
  const content = t.content || " ";

  const canvas = document.createElement("canvas");
  const measureCtx = canvas.getContext("2d")!;
  measureCtx.font = font;
  const textWidthPx = Math.max(measureCtx.measureText(content).width, 1);
  canvas.width = Math.ceil(textWidthPx) + 8;
  canvas.height = Math.ceil(px * 1.3);
  const ctx = canvas.getContext("2d")!; // resizing a canvas resets its context state -- re-fetch and re-set font
  ctx.font = font;
  ctx.fillStyle = "#ffffff";
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  ctx.fillText(content, 4, canvas.height / 2);

  const texture = new THREE.CanvasTexture(canvas);
  texture.minFilter = THREE.LinearFilter;

  const widthMm = (canvas.width / px) * sizeMm;
  const heightMm = (canvas.height / px) * sizeMm;
  const geometry = new THREE.PlaneGeometry(widthMm, heightMm);
  // The texture itself was always drawn left-aligned -- anchor (t.x,t.y)
  // at the plane's left/center/right edge per `justify` by shifting the
  // geometry in its own local space, before the flatten/yaw below, the
  // same way the 2D canvas's translate-then-rotate-then-textAlign order
  // keeps the alignment offset in the text's own (unrotated) frame.
  const offsetX = t.justify === "left" ? widthMm / 2 : t.justify === "right" ? -widthMm / 2 : 0;
  geometry.translate(offsetX, 0, 0);
  // Flatten the plane (local XY, facing local +Z) into this file's
  // world board plane (X/Z), facing outward from `side` -- the same
  // local-XY -> world-XZ mapping as buildMask/addZoneFill, mirrored for
  // the bottom side so the printed face looks outward from the board
  // rather than into it.
  geometry.rotateX(side === "top" ? -Math.PI / 2 : Math.PI / 2);

  const material = new THREE.MeshBasicMaterial({ map: texture, transparent: true, side: THREE.DoubleSide, depthWrite: false });
  const mesh = new THREE.Mesh(geometry, material);
  mesh.position.set(mm(t.x), silkY(side), mm(t.y));
  // Millidegrees, CCW-positive in board space -- same conversion and
  // sign the 2D painter's drawTexts uses for its own (canvas) rotation;
  // kept consistent here even though this file's world axes are a
  // separate (if parallel/unmirrored) convention from the 2D canvas's.
  mesh.rotation.y = -(t.angle / 1000) * (Math.PI / 180);
  if (t.mirror) mesh.scale.x *= -1;
  mesh.name = "silk-text";
  return mesh;
}

// ---------------------------------------------------------------------
// Top-level: board -> Group
// ---------------------------------------------------------------------

/** Builds the whole board as a single Group. Safe to call with a board that has no outline and/or no placed parts -- renders whatever subset of the geometry makes sense, never throws. */
export function buildBoardGroup(board: BoardState): THREE.Group {
  const group = new THREE.Group();
  group.name = "viewer3d-board";

  const outlineMm: Array<[number, number]> = (board.outline ?? []).map(([x, y]) => [mm(x), mm(y)]);
  const shape = buildOutlineShape(outlineMm);
  if (shape) {
    const slab = buildSlab(shape);
    if (slab) group.add(slab);
    group.add(buildMask(shape, "top"));
    group.add(buildMask(shape, "bottom"));
  }

  const layerOrder = board.layers.length > 0 ? board.layers : ["F.Cu", "B.Cu"];

  // One material per copper bucket key, reused across every pad/track/via
  // on that layer instead of allocating a new one per element.
  const copperMaterials = new Map<string, THREE.MeshStandardMaterial>();
  const copperMatFor = (bucketKey: string): THREE.MeshStandardMaterial => {
    let material = copperMaterials.get(bucketKey);
    if (!material) {
      material = copperMaterial(bucketKey);
      copperMaterials.set(bucketKey, material);
    }
    return material;
  };

  if (board.routing) {
    for (const track of board.routing.tracks) {
      const y = layerY(track.layer, layerOrder);
      const ptsMm: Array<[number, number]> = track.pts.map(([x, yy]) => [mm(x), mm(yy)]);
      addTrack(group, ptsMm, y, mm(track.width), copperMatFor(copperColorKey(track.layer)));
    }
    if (board.routing.vias.length > 0) {
      const viaMat = platedMaterial();
      for (const via of board.routing.vias) {
        addVia(group, mm(via.x), mm(via.y), mm(via.d), viaMat);
      }
    }
    if (board.routing.zones.length > 0) {
      const zoneMaterials = new Map<string, THREE.MeshStandardMaterial>();
      const zoneMatFor = (bucketKey: string): THREE.MeshStandardMaterial => {
        let material = zoneMaterials.get(bucketKey);
        if (!material) {
          material = zoneMaterial(bucketKey);
          zoneMaterials.set(bucketKey, material);
        }
        return material;
      };
      for (const zone of board.routing.zones) {
        const y = zoneY(zone.layer, layerOrder);
        const outlineMm: Array<[number, number]> = zone.outline.map(([x, yy]) => [mm(x), mm(yy)]);
        addZoneFill(group, outlineMm, y, zoneMatFor(copperColorKey(zone.layer)));
      }
    }
  }

  const topCuY = layerY("F.Cu", layerOrder);
  const botCuY = layerY("B.Cu", layerOrder);
  const padMat = platedMaterial();
  const silkMat = silkMaterial();
  const icMat = icBodyMaterial();
  const passiveMat = passiveBodyMaterial();

  for (const part of board.parts) {
    const side = part.side;
    if (!part.placed || !side) continue;

    for (const pad of part.pads ?? []) {
      addPad(group, mm(pad.x), mm(pad.y), mm(pad.w), mm(pad.h), pad.round, side === "top" ? topCuY : botCuY, padMat);
    }

    const courtyard = part.courtyard;
    if (courtyard) {
      const courtyardMm: [number, number, number, number] = [mm(courtyard[0]), mm(courtyard[1]), mm(courtyard[2]), mm(courtyard[3])];
      addSilkOutline(group, courtyardMm, side, silkMat);
      addPartBody(group, courtyardMm, side, isPassivePart(part) ? passiveMat : icMat);
    }
  }

  if (board.drawings) {
    const silkFillMat = silkFillMaterial();
    for (const s of board.drawings.shapes) {
      addSilkShape(group, s, silkMat, silkFillMat);
    }
    for (const t of board.drawings.texts) {
      group.add(buildTextMesh(t));
    }
  }

  return group;
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
// Camera view presets
// ---------------------------------------------------------------------

export type ViewPreset = "top" | "bottom" | "front" | "back" | "left" | "right" | "iso" | "reset";

// Exact KiCad 3D-viewer view-preset semantics, per the task spec: each
// snaps the camera to look straight along that axis at the board.
// Front/Back/Left/Right are this file's own (necessarily arbitrary --
// nothing in the board data names a "front" edge) but internally
// consistent choice: +Z/-Z/-X/+X respectively, in this file's own
// world-axis convention (see the header comment). Iso and Reset use the
// same classic 3/4 isometric-ish direction -- Reset is just the name
// used for the initial/on-demand re-fit (also what gets called on first
// mount), Iso the toolbar button; their effect is identical, same as
// KiCad's own two actions.
const PRESET_DIRECTIONS: Record<ViewPreset, THREE.Vector3> = {
  top: new THREE.Vector3(0, 1, 0),
  bottom: new THREE.Vector3(0, -1, 0),
  front: new THREE.Vector3(0, 0, 1),
  back: new THREE.Vector3(0, 0, -1),
  left: new THREE.Vector3(-1, 0, 0),
  right: new THREE.Vector3(1, 0, 0),
  iso: new THREE.Vector3(1, 1, 1).normalize(),
  reset: new THREE.Vector3(1, 1, 1).normalize(),
};

/** Used only when there is nothing yet to frame (empty board group) so the camera still gets a sane, finite pose instead of NaN/Infinity. */
const EMPTY_BOX_HALF_EXTENT_MM = 10;

/**
 * Camera position + look-at target for `preset`, framing `box` (the
 * whole board group's world-space bounds) from a distance that keeps
 * box's circumscribing sphere fully inside the camera's frustum on
 * *both* axes (accounts for `aspect`, so a narrow/tall viewport doesn't
 * clip a wide board or vice versa), for any of the 8 preset directions
 * -- including the diagonal Iso/Reset view, which a purely
 * per-axis-projected fit would under-size for.
 *
 * Never mutates `box`. Pure and independent of any live Three.js scene
 * state, so it's trivially callable both for the initial auto-fit and
 * for every toolbar button.
 */
export function presetCameraPose(box: THREE.Box3, preset: ViewPreset, fovDeg: number, aspect: number): { position: THREE.Vector3; target: THREE.Vector3 } {
  const safeBox = box.isEmpty()
    ? new THREE.Box3(
        new THREE.Vector3(-EMPTY_BOX_HALF_EXTENT_MM, -EMPTY_BOX_HALF_EXTENT_MM, -EMPTY_BOX_HALF_EXTENT_MM),
        new THREE.Vector3(EMPTY_BOX_HALF_EXTENT_MM, EMPTY_BOX_HALF_EXTENT_MM, EMPTY_BOX_HALF_EXTENT_MM)
      )
    : box;

  const center = safeBox.getCenter(new THREE.Vector3());
  const size = safeBox.getSize(new THREE.Vector3());
  // Half the box's diagonal: the radius of a sphere that contains the
  // whole box from *any* viewing angle, not just the 6 axis-aligned ones
  // -- needed for Iso/Reset, which view it diagonally.
  const radius = Math.max(size.length() / 2, 1);

  const vFov = (fovDeg * Math.PI) / 180;
  const hFov = 2 * Math.atan(Math.tan(vFov / 2) * Math.max(aspect, 1e-6));
  const margin = 1.25; // headroom so the board doesn't touch the viewport edges
  const distance = Math.max(radius / Math.sin(vFov / 2), radius / Math.sin(hFov / 2)) * margin;

  const position = center.clone().addScaledVector(PRESET_DIRECTIONS[preset], distance);
  return { position, target: center };
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
