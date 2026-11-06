// The 3D Navigator (`LAYER_3D_NAVIGATOR`, `EDA_3D_ACTIONS::showNavigator`): six spheres on the axes in the view's bottom-left corner, turned with the camera; a click on one
// goes to that face's view.
//
// Ported from KiCad 8303b2ad 3d-viewer/3d_rendering/opengl/3d_spheres_gizmo.{h,cpp} (`SPHERES_GIZMO`: the spheres' places, sizes and colours, the square viewport of
// `m_viewportH / 8` at (4, 4), the 60-degree perspective and the camera `m_arrowSize * 2.75` away, `handleMouseInput`, `updateSelection`'s ray against the spheres) and
// 3d-viewer/3d_canvas/eda_3d_canvas.cpp (`OnLeftUp` hands the click to the gizmo; `DoRePaint`'s `viewTable`: the sphere clicked is the view set). Pure: no three.js.
// components/viewer3d/navigator.ts draws it.
import type { ViewPreset } from "./camera3d";

/** `RANGE_SCALE_3D` (board_adapter.h): the size of KiCad's 3D world, in which the gizmo is measured. */
export const RANGE_SCALE_3D = 8;
/** `m_arrowSize = RANGE_SCALE_3D * 0.20f`: how far out the spheres are. */
export const ARROW_SIZE = RANGE_SCALE_3D * 0.2;
/** `m_sphereRadius = 0.05f * RANGE_SCALE_3D`. */
export const SPHERE_RADIUS = 0.05 * RANGE_SCALE_3D;
/** The gizmo camera's distance: `glm::translate( .., -( m_arrowSize * 2.75f ) )`. */
export const CAMERA_DISTANCE = ARROW_SIZE * 2.75;
/** `float fov = 60.0f;`. */
export const FOV_DEG = 60;
/** `SPHERES_GIZMO( 4, 4 )`: the viewport's corner, pixels from the bottom left. */
export const GIZMO_POS = 4;

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

/** `GizmoSphereSelection`: Right (+X), Left (-X), Back (+Y), Front (-Y), Top (+Z), Bottom (-Z), in KiCad's 3D axes (x right, y away from the viewer of the top, z up). */
export interface GizmoSphere {
  id: "right" | "left" | "back" | "front" | "top" | "bottom";
  /** In KiCad's axes. */
  position: Vec3;
  /** `m_originalColor` (0..1). */
  color: Vec3;
  /** The letter drawn on it (`labels = { "X", "", "Y", "", "Z", "" }`). */
  label: "X" | "Y" | "Z" | "";
}

const v = (x: number, y: number, z: number): Vec3 => ({ x, y, z });

/** `m_spheres`, in `GizmoSphereSelection` order. */
export const SPHERES: readonly GizmoSphere[] = [
  { id: "right", position: v(ARROW_SIZE, 0, 0), color: v(0.9, 0, 0), label: "X" },
  { id: "left", position: v(-ARROW_SIZE, 0, 0), color: v(0.4, 0, 0), label: "" },
  { id: "back", position: v(0, ARROW_SIZE, 0), color: v(0, 0.9, 0), label: "Y" },
  { id: "front", position: v(0, -ARROW_SIZE, 0), color: v(0, 0.4, 0), label: "" },
  { id: "top", position: v(0, 0, ARROW_SIZE), color: v(0, 0, 0.9), label: "Z" },
  { id: "bottom", position: v(0, 0, -ARROW_SIZE), color: v(0, 0, 0.4), label: "" },
];

/** `DoRePaint`'s `viewTable`: the view each sphere goes to (VIEW3D_RIGHT, LEFT, BACK, FRONT, TOP, BOTTOM). */
export const VIEW_OF: Readonly<Record<GizmoSphere["id"], ViewPreset>> = { right: "right", left: "left", back: "back", front: "front", top: "top", bottom: "bottom" };

/** KiCad's 3D axes to the studio's scene (camera3d.ts: KiCad (x, y, z) is the scene's (x, z, -y): the scene's y is up out of the board, its z the board's y). */
export function toScene(p: Vec3): Vec3 {
  return { x: p.x, y: p.z, z: 0 - p.y };
}

/** The gizmo's square (`glViewport( m_gizmoPosX, m_gizmoPosY, m_viewportH / 8, m_viewportH / 8 )`): its side for a view `height` pixels high. */
export function gizmoSize(height: number): number {
  return Math.trunc(height / 8);
}

/**
 * `handleMouseInput` for a click at `(x, y)` (pixels from the view's top left, as a pointer event gives them; KiCad's are from the bottom, `logicalH - event.GetY()`):
 * the point in the gizmo's own normalised coordinates, or null when the click is not in its square.
 */
export function gizmoNdc(x: number, y: number, height: number): { x: number; y: number } | null {
  const size = gizmoSize(height);
  const mx = x;
  const my = height - y;
  const inside = mx >= GIZMO_POS && mx <= GIZMO_POS + size && my >= GIZMO_POS && my <= GIZMO_POS + size;
  if (!inside || size <= 0) return null;
  return { x: (2 * (mx - GIZMO_POS)) / size - 1, y: (2 * (my - GIZMO_POS)) / size - 1 };
}

const sub = (a: Vec3, b: Vec3): Vec3 => v(a.x - b.x, a.y - b.y, a.z - b.z);
const dot = (a: Vec3, b: Vec3): number => a.x * b.x + a.y * b.y + a.z * b.z;

/** `updateSelection`'s `intersectDist`: the distance along the ray to the sphere's centre's foot, or -1 when the ray passes farther than the radius. */
export function intersectDist(origin: Vec3, dir: Vec3, center: Vec3, radius: number): number {
  const l = sub(center, origin);
  const tca = dot(l, dir);
  const d2 = dot(l, l) - tca * tca;
  if (d2 > radius * radius) return -1;
  return tca;
}

/** `updateSelection`: the sphere the ray hits nearest (positions in whatever frame the ray is in, given by `positionOf`), or null. */
export function pickSphere(origin: Vec3, dir: Vec3, positionOf: (s: GizmoSphere) => Vec3 = (s) => s.position): GizmoSphere | null {
  let best: GizmoSphere | null = null;
  let closest = Infinity;
  for (const s of SPHERES) {
    const d = intersectDist(origin, dir, positionOf(s), SPHERE_RADIUS);
    if (d >= 0 && d < closest) {
      closest = d;
      best = s;
    }
  }
  return best;
}
