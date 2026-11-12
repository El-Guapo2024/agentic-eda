// `OPENGL_RENDER_LIST::DrawCulled` (KiCad 8303b2ad, 3d-viewer/3d_rendering/opengl/layer_triangles.cpp) in three.js: a layer is drawn where none of its "subtract lists"
// is. KiCad clears the stencil buffer, draws each subtract list's faces into it at the layer's height with no depth test and no colour, and then draws the layer where the
// stencil is clear; the copper is culled by the through holes, the layer's own holes and everything off the board (`m_antiBoard`), the silkscreen by the holes (or the via
// annuli), the solder mask's openings and the board's outside, the solder mask by its openings and the holes (`RENDER_3D_OPENGL::Redraw`, `renderSolderMaskLayer`).
//
// three.js draws its opaque objects in `renderOrder`, so a culled layer here is a group of that order: first its subtract pieces (`colorWrite` and `depthTest` off), each
// writing the group's own value into the stencil's low six bits, then the layer, drawn where the stencil is not that value. A value is a group's alone, so no clear is
// needed between two groups (a group that comes after another overwrites the other's value, and has drawn by then); after 63 groups the low bits are cleared by a
// full-screen piece and the values start again. The two high bits are the solder masks' openings ([`MASK_BITS`]), which the masks read after every opaque object is
// drawn (they are transparent): nothing else writes them.
import * as THREE from "three";
import { MAX_ERROR_NM, circlePolygon, nestContours, ovalPolygon, type Contour, type Pt } from "../../kicad-port/padShape3d";
import type { Prim } from "../../kicad-port/layers3d";

/** The stencil bits of a side's solder mask openings (written early, read by the transparent masks and by the plated copper). */
export const MASK_BITS = { top: 0x40, bottom: 0x80 } as const;
const GROUP_MASK = 0x3f;

const mmVec = ([x, y]: Pt): THREE.Vector2 => new THREE.Vector2(x / 1e6, y / 1e6);

/** The server's answers to the layers' polygon jobs, by key. */
export type JobResults = ReadonlyMap<string, ReadonlyArray<{ outer: Contour; holes: Contour[] }>>;

/** Contours filled even-odd as three.js shapes (mm). */
export function contoursToShapes(contours: readonly Contour[]): THREE.Shape[] {
  return nestContours(contours).map(({ outer, holes }) => {
    const s = new THREE.Shape(outer.map(mmVec));
    for (const h of holes) s.holes.push(new THREE.Path(h.map(mmVec)));
    return s;
  });
}

/** One primitive as three.js shapes (mm): `ROUND_SEGMENT_2D` as its stadium, `FILLED_CIRCLE_2D`, `RING_2D` with its hole, a polygon, a job once answered. */
export function primShapes(p: Prim, jobs?: JobResults): THREE.Shape[] {
  switch (p.k) {
    case "seg": {
      if (!(p.w > 0)) return [];
      const c = p.a[0] === p.b[0] && p.a[1] === p.b[1] ? circlePolygon(p.a, p.w / 2, MAX_ERROR_NM) : ovalPolygon(p.a, p.b, p.w, MAX_ERROR_NM);
      return [new THREE.Shape(c.map(mmVec))];
    }
    case "circle":
      return p.r > 0 ? [new THREE.Shape(circlePolygon(p.c, p.r, MAX_ERROR_NM).map(mmVec))] : [];
    case "ring": {
      if (!(p.ro > 0)) return [];
      const s = new THREE.Shape(circlePolygon(p.c, p.ro, MAX_ERROR_NM).map(mmVec));
      if (p.ri > 0) s.holes.push(new THREE.Path(circlePolygon(p.c, p.ri, MAX_ERROR_NM).map(mmVec).reverse()));
      return [s];
    }
    case "poly":
      return contoursToShapes(p.contours);
    case "job": {
      const r = jobs?.get(p.key);
      if (!r) return [];
      return r.map(({ outer, holes }) => {
        const s = new THREE.Shape(outer.map(mmVec));
        for (const h of holes) s.holes.push(new THREE.Path(h.map(mmVec)));
        return s;
      });
    }
  }
}

export const listShapes = (list: readonly Prim[], jobs?: JobResults): THREE.Shape[] => list.flatMap((p) => primShapes(p, jobs));

/** A flat piece at height `y` that only writes the stencil (`colorWrite`, depth off), both faces. */
function stencilPiece(shapes: readonly THREE.Shape[], y: number, ref: number, writeMask: number, name: string): THREE.Mesh {
  const material = new THREE.MeshBasicMaterial({
    colorWrite: false,
    depthWrite: false,
    depthTest: false,
    side: THREE.DoubleSide,
    stencilWrite: true,
    stencilRef: ref,
    stencilWriteMask: writeMask,
    stencilFunc: THREE.AlwaysStencilFunc,
    stencilZPass: THREE.ReplaceStencilOp,
    stencilFail: THREE.ReplaceStencilOp,
    stencilZFail: THREE.ReplaceStencilOp,
  });
  const mesh = new THREE.Mesh(new THREE.ShapeGeometry(shapes as THREE.Shape[]), material);
  mesh.rotation.x = Math.PI / 2;
  mesh.position.y = y;
  mesh.name = name;
  mesh.frustumCulled = false;
  return mesh;
}

/** A piece covering the whole screen that writes `ref` into the bits of `writeMask` (a clear, or the "everywhere" of the plated copper's culling). */
function screenPiece(ref: number, writeMask: number, name: string): THREE.Mesh {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute([-1, -1, 0, 3, -1, 0, -1, 3, 0], 3));
  const material = new THREE.ShaderMaterial({
    vertexShader: "void main() { gl_Position = vec4( position.xy, 0.0, 1.0 ); }",
    fragmentShader: "void main() { gl_FragColor = vec4( 0.0 ); }",
    colorWrite: false,
    depthWrite: false,
    depthTest: false,
    side: THREE.DoubleSide,
    stencilWrite: true,
    stencilRef: ref,
    stencilWriteMask: writeMask,
    stencilFunc: THREE.AlwaysStencilFunc,
    stencilZPass: THREE.ReplaceStencilOp,
  });
  const mesh = new THREE.Mesh(geometry, material);
  mesh.frustumCulled = false;
  mesh.name = name;
  return mesh;
}

/** A subtract list: its shapes and the height they are drawn at (the culled layer's). */
export interface Subtract {
  shapes: readonly THREE.Shape[];
  y: number;
  /** Write 0 instead of the group's value: a hole in the subtract lists drawn before it (the plated copper: everywhere but the mask's openings). */
  keep?: boolean;
  /** The holes (`m_outerThroughHoles`, `m_outerLayerHoles`): what "Disable holes" leaves out while the camera moves. */
  hole?: boolean;
}

/** Allots the stencil values and the draw order of the culled layers of one board build. */
export class Culler {
  private value = 0;
  private order = 10;
  constructor(private readonly group: THREE.Group) {}

  /**
   * Draws `draws` culled by `subtracts` (`DrawCulled`): the subtract pieces first, then every mesh of `draws` with the stencil test "not this group's value". `everywhere`
   * starts the group with the whole screen culled (the subtract pieces marked `keep` then open it). With nothing to subtract the layer is drawn as it is, in its order.
   */
  culled(draws: readonly THREE.Object3D[], subtracts: readonly Subtract[], everywhere = false): void {
    const used = subtracts.filter((s) => s.shapes.length > 0);
    const order = this.order;
    this.order += 2;
    if (used.length === 0 && !everywhere) {
      for (const d of draws) d.traverse((o) => (o.renderOrder = order + 1));
      return;
    }
    if (++this.value > GROUP_MASK) {
      const clear = screenPiece(0, GROUP_MASK, "stencil-clear");
      clear.renderOrder = order - 0.5;
      this.group.add(clear);
      this.value = 1;
    }
    const k = this.value;
    let sub = 0;
    if (everywhere) {
      const all = screenPiece(k, GROUP_MASK, "cull-everywhere");
      all.renderOrder = order + (sub++ + 1) * 0.01;
      this.group.add(all);
    }
    for (const s of used) {
      const piece = stencilPiece(s.shapes, s.y, s.keep ? 0 : k, GROUP_MASK, "cull-piece");
      if (s.hole) piece.userData.hole = true;
      piece.renderOrder = order + (sub++ + 1) * 0.01;
      this.group.add(piece);
    }
    for (const d of draws) {
      d.traverse((o) => {
        o.renderOrder = order + 1;
        const m = (o as THREE.Mesh).material;
        if (!m) return;
        for (const mat of Array.isArray(m) ? m : [m]) {
          mat.stencilWrite = true; // three.js turns the stencil test on with it; the write mask keeps the buffer as it is
          mat.stencilWriteMask = 0;
          mat.stencilRef = k;
          mat.stencilFuncMask = GROUP_MASK;
          mat.stencilFunc = THREE.NotEqualStencilFunc;
        }
      });
    }
  }
}

/** The openings of a side's solder mask (`m_layers[ F_Mask ]`, the mask layer's items), drawn into that side's bit before anything else, on the mask's plane. */
export function maskOpenings(shapes: readonly THREE.Shape[], y: number, side: "top" | "bottom"): THREE.Mesh | null {
  if (shapes.length === 0) return null;
  const mesh = stencilPiece(shapes, y, MASK_BITS[side], MASK_BITS[side], `mask-opening-${side}`);
  mesh.renderOrder = -10;
  return mesh;
}

/** A material drawn only where a side's mask is open (`stencilFunc` equal on that side's bit): the plated copper's second test, after its group's. */
export function onlyInMaskOpening(m: THREE.Material, side: "top" | "bottom"): void {
  m.stencilWrite = true;
  m.stencilWriteMask = 0;
  m.stencilRef = MASK_BITS[side];
  m.stencilFuncMask = MASK_BITS[side];
  m.stencilFunc = THREE.EqualStencilFunc;
}
