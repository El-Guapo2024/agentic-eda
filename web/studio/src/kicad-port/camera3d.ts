// Port of the KiCad 3D viewer's camera: include/gal/3d/camera.h + 3d-viewer's
// own common/gal/3d/camera.cpp (the CAMERA base class) and
// 3d-viewer/3d_rendering/track_ball.cpp (TRACK_BALL, the only CAMERA
// subclass the interactive 3D viewer actually uses -- see
// eda_3d_viewer_frame.h's `TRACK_BALL m_trackBallCamera`). Mouse-event
// plumbing (HIDPI_GL_3D_CANVAS::OnMouseMoveCamera/OnMouseWheelCamera,
// common/gal/hidpi_gl_3D_canvas.cpp) and the view-preset/zoom/rotate
// dispatch (EDA_3D_CANVAS::SetView3D, 3d_canvas/eda_3d_canvas.cpp, and
// EDA_3D_CONTROLLER::RotateView, 3d_viewer/tools/eda_3d_controller.cpp)
// are folded in here too, as methods, since this class is this app's only
// caller of any of that code.
//
// ------------------------------------------------------------- units
//
// KiCad normalizes the *geometry* for every loaded board into a fixed-size
// box (board_adapter.cpp: `m_biuTo3Dunits = RANGE_SCALE_3D/maxBoardDim *
// 1.6`, RANGE_SCALE_3D=8) and keeps the camera's own initial distance a
// hardcoded constant (`2*RANGE_SCALE_3D` = 16, eda_3d_viewer_frame.cpp's
// `m_trackBallCamera(2*RANGE_SCALE_3D)`) -- the board always ends up the
// same apparent size, viewed from the same fixed distance, regardless of
// its real dimensions. This app's board geometry is never rescaled (it
// stays in real millimetres, see scene.ts's own header comment), so the
// *camera's* distance is what has to scale with board size instead, to
// reproduce the same apparent framing. The constants below derive the
// right ratio from KiCad's own numbers so this is a faithful reproduction
// of the resulting picture, not a new guess:
//
//   boardSpan3DUnits = boardSpanBIU * m_biuTo3Dunits
//                    = boardSpanBIU * (RANGE_SCALE_3D/boardSpanBIU) * 1.6
//                    = RANGE_SCALE_3D * 1.6 = 12.8        (always, any board)
//   cameraDistance3DUnits = 2 * RANGE_SCALE_3D = 16        (always, fixed)
//   => distance-to-board-span ratio = 16 / 12.8 = 1.25     (dimensionless)
//
// So `initialDistanceMm = 1.25 * boardSpanMm`, where boardSpanMm is the
// board OUTLINE's longest single bounding-box edge (board_adapter.cpp:366
// `m_boardSize = bbbox.GetSize()` from `board->ComputeBoundingBox(
// boardEdgesOnly=true, ...)` -- the Edge.Cuts outline only, NOT component
// heights). near/far are similarly fixed *ratios* of that same distance
// (camera.cpp's rebuildProjection: nearD=0.10 constant, farD=
// length(cameraPosInit)*maxZoom*2 = 16*2*2 = 64 in KiCad's own units, both
// scale-invariant once expressed as a fraction of the (also fixed, there)
// initial distance): near/initialDistance = 0.10/16 = 0.00625,
// far/initialDistance = 64/16 = 4.
//
// ----------------------------------------------------- coordinate frame
//
// All of this module's own state (cameraPosLocal, lookAt's *offset* from
// it, the rotation quaternions) is kept in KiCad's own local/world axis
// convention: X/Y are the camera's local right/up (screen-space) axes,
// +Z is "toward the viewer" (CAMERA's ctor places the unrotated camera at
// local (0,0,-distance), OpenGL/glm's standard "camera looks down -Z"
// convention) -- ported variable-for-variable against source so it stays
// easy to check against it. Only `lookAt` itself is a genuine world-space
// point (this app's board-plane coordinates, fed in directly by the
// caller -- see setBoardGeometry).
//
// KiCad's OWN world axes happen to put board-plane X/Y on world X/Y and
// board thickness on world Z (board_adapter.cpp:371 "the y coord is
// inverted in 3D viewer", :561 `m_boardCenter.z = 0.0f` -- mid-thickness).
// This app's scene.ts instead puts thickness on world Y (to match
// Three.js's own default "up" axis -- see its header comment), board Y on
// world Z. Rather than rderive every CAMERA formula in a second axis
// convention, `BASIS_CHANGE` below is the one fixed rotation between the
// two (a -90 degree rotation about world X: KiCad's local +Z, where the
// unrotated camera sits, maps to this app's world +Y), applied only at
// the final getRenderPose() output boundary. Checked against source by
// construction: at identity rotation (fresh TOP view) this maps the
// camera to lookAt+worldY*distance looking down -Y (onto the board from
// above, screen-up = world -Z = decreasing board Y) -- independently
// re-derived for FRONT/RIGHT/BOTTOM too (see camera3d.test.ts), all of
// which land on this app's pre-existing PRESET_DIRECTIONS convention
// (scene.ts, now superseded by PRESET_AUX_DEG below) without needing to
// change it.
import {
  addV,
  conjugateQuat,
  type Quat,
  QUAT_IDENTITY,
  multiplyQuat,
  negV,
  normalizeQuat,
  type Vec3,
  rotateVecByQuat,
  quatFromAxisAngle,
  vec3,
} from "./math3d";
import { trackball } from "./trackball";

const DEG2RAD = Math.PI / 180;

/** camera.h CAMERA::DEFAULT_MIN_ZOOM/DEFAULT_MAX_ZOOM (camera.cpp:50-51), unchanged. */
export const DEFAULT_MIN_ZOOM = 0.02;
export const DEFAULT_MAX_ZOOM = 2.0;

/** camera.cpp rebuildProjection: `m_frustum.angle = 45.0f` for BOTH projection types (also the basis for the ortho half-extent below). Never changes -- KiCad's 3D viewer has no user-facing FOV setting. */
export const FOV_DEG = 45;
const HALF_FOV_TAN = Math.tan((FOV_DEG * DEG2RAD) / 2);

/** See header comment: distance = 1.25 * the board outline's longest bbox edge. */
export const DISTANCE_TO_BOARD_SPAN_RATIO = 1.25;
/** camera.cpp: `m_frustum.nearD = 0.10f`, expressed as a ratio of the (fixed, there) initial distance -- see header comment. */
const NEAR_TO_DISTANCE_RATIO = 0.10 / (2 * 8.0 /* RANGE_SCALE_3D */);
/** camera.cpp: `m_frustum.farD = length(cameraPosInit) * maxZoom * 2`, as a ratio of the initial distance. */
const FAR_TO_DISTANCE_RATIO = DEFAULT_MAX_ZOOM * 2;

/** eda_3d_viewer_settings.cpp: `camera.rotation_increment` default, degrees -- EDA_3D_CONTROLLER's m_rotationIncrement ctor default (eda_3d_controller.h) and the value eda_3d_viewer_frame.cpp actually loads it from. The task brief's "+-45 degrees" does not match source; source (10.0) is what's ported. */
export const ROTATION_STEP_DEG = 10;

/** hidpi_gl_3D_canvas.cpp OnMouseWheelCamera's plain-wheel zoom factor. */
const WHEEL_ZOOM_FACTOR = 1.1;
/** eda_3d_canvas.cpp SetView3D's VIEW3D_ZOOM_IN/OUT factor ("3 steps per doubling": 1.26^3 = 2.000376). Used by keyboard (F1/F2) and toolbar zoom in/out. */
export const KEY_ZOOM_FACTOR = 1.26;
/** hidpi_gl_3D_canvas.h: `m_delta_move_step_factor = 0.7f` -- shared by arrow-key pan (scaled by current zoom) and as the wheel-pan speed's own base term. */
const PAN_STEP_FACTOR = 0.7;
/** hidpi_gl_3D_canvas.cpp OnMouseWheelCamera: `delta_move *= 0.01f * event.GetWheelRotation()` (the `aPan` branch only). */
const WHEEL_PAN_SPEED = 0.01;
/** camera.cpp: "180 - epsilon" literal, everywhere source wants a half-turn without risking a full 360-degree spin if the previous angle was already exactly the opposite extreme. */
const FLIP_EPSILON_DEG = 179.999;

export type Projection = "perspective" | "ortho";

export type ViewPreset = "top" | "bottom" | "front" | "back" | "left" | "right";

/**
 * camera.cpp:CAMERA::ViewCommand_T1, the aux-rotation (degrees) each
 * preset ends up with after its own `Reset_T1()` (zeroes aux to (0,0,0))
 * plus whichever `RotateX_T1`/`RotateY_T1`/`RotateZ_T1` calls follow it --
 * independent per-axis accumulators, so call order in source doesn't
 * matter, only the final per-axis sum does:
 *   RIGHT:  RotateZ_T1(-90) + RotateX_T1(-90)          -> (x:-90, z:-90)
 *   LEFT:   RotateZ_T1(90)  + RotateX_T1(-90)          -> (x:-90, z:90)
 *   FRONT:  RotateX_T1(-90)                            -> (x:-90)
 *   BACK:   RotateX_T1(-90) + RotateZ_T1(179.999)      -> (x:-90, z:179.999)
 *   TOP:    (nothing further)                          -> (0,0,0)
 *   BOTTOM: RotateY_T1(179.999)                        -> (y:179.999)
 */
const PRESET_AUX_DEG: Record<ViewPreset, Vec3> = {
  top: vec3(0, 0, 0),
  bottom: vec3(0, FLIP_EPSILON_DEG, 0),
  front: vec3(-90, 0, 0),
  back: vec3(-90, 0, FLIP_EPSILON_DEG),
  left: vec3(-90, 0, 90),
  right: vec3(-90, 0, -90),
};

/** See header comment's "coordinate frame" section. */
const BASIS_CHANGE: Quat = quatFromAxisAngle(vec3(1, 0, 0), -Math.PI / 2);

export interface RenderPose {
  /** World-space camera position (this app's world: X=board X mm, Y=thickness mm, Z=board Y mm). */
  position: Vec3;
  /** World-space camera orientation. */
  quaternion: Quat;
}

export type ProjectionParams =
  | { kind: "perspective"; fovDeg: number; near: number; far: number }
  | { kind: "ortho"; left: number; right: number; top: number; bottom: number; near: number; far: number };

/**
 * Port of TRACK_BALL (the CAMERA subclass the interactive 3D viewer uses)
 * plus the mouse/keyboard/view-preset glue listed in this file's header
 * comment. One instance per Viewer3D mount; `setBoardGeometry` re-homes it
 * (without moving the live camera -- same policy Viewer3D.tsx already had
 * for its old OrbitControls-based presets) whenever the board's outline
 * bounds change meaningfully.
 */
export class TrackballCamera {
  private zoomValue = 1;
  private readonly minZoom: number;
  private readonly maxZoom: number;

  /** KiCad-local space (see header comment): x/y = accumulated pan, z = -initialDistance*zoom. */
  private cameraPosLocal: Vec3;
  private initialDistanceMm: number;
  private nearMm: number;
  private farMm: number;

  /** World-space orbit target; `lookAtInit` is what reset()/view presets restore it to. */
  private lookAt: Vec3;
  private lookAtInit: Vec3;

  /** m_rotationMatrix: cumulative free rotation from trackball drags. */
  private trackballQuat: Quat = { ...QUAT_IDENTITY };
  /** m_rotationMatrixAux's generating angles (degrees, source uses radians internally but tests/readouts are friendlier in degrees -- converted at point of use): rebuilt fresh into a quaternion on every read, never composed incrementally, matching updateRotationMatrix()'s own from-scratch rebuild. */
  private auxDeg: Vec3 = vec3(0, 0, 0);

  private projectionValue: Projection;
  private windowWidth = 1;
  private windowHeight = 1;
  private lastMouse: { x: number; y: number } | null = null;

  constructor(
    initialDistanceMm: number,
    lookAt: Vec3,
    opts: { minZoom?: number; maxZoom?: number; projection?: Projection } = {}
  ) {
    this.minZoom = opts.minZoom ?? DEFAULT_MIN_ZOOM;
    this.maxZoom = opts.maxZoom ?? DEFAULT_MAX_ZOOM;
    // eda_3d_viewer_settings.cpp: `camera.projection_mode` default 1 =
    // PROJECTION_TYPE::PERSPECTIVE (0=ORTHO, 1=PERSPECTIVE, 3d_enums.h).
    this.projectionValue = opts.projection ?? "perspective";
    this.initialDistanceMm = Math.max(initialDistanceMm, 1e-6);
    this.nearMm = this.initialDistanceMm * NEAR_TO_DISTANCE_RATIO;
    this.farMm = this.initialDistanceMm * FAR_TO_DISTANCE_RATIO;
    this.lookAtInit = { ...lookAt };
    this.lookAt = { ...lookAt };
    this.cameraPosLocal = vec3(0, 0, -this.initialDistanceMm);
  }

  /**
   * Re-homes the camera's distance/near/far and reset-target without
   * moving the *live* camera (board_adapter.cpp's InitSettings() recomputes
   * m_boardSize/m_boardPos the same way on every reload; KiCad's own
   * CAMERA::m_camera_pos_init never changes after construction because
   * its geometry is rescaled instead of its camera -- see header comment
   * for why this app has to re-home the camera itself). Callers that want
   * the camera to actually re-fit (board changed enough to warrant it)
   * call `reset()` afterward -- the same boundsChanged-gated policy
   * Viewer3D.tsx already had.
   */
  setBoardGeometry(boardSpanMm: number, lookAt: Vec3): void {
    this.initialDistanceMm = Math.max(boardSpanMm * DISTANCE_TO_BOARD_SPAN_RATIO, 1e-6);
    this.nearMm = this.initialDistanceMm * NEAR_TO_DISTANCE_RATIO;
    this.farMm = this.initialDistanceMm * FAR_TO_DISTANCE_RATIO;
    this.lookAtInit = { ...lookAt };
  }

  setWindowSize(width: number, height: number): void {
    if (width > 0 && height > 0) {
      this.windowWidth = width;
      this.windowHeight = height;
    }
  }

  /** CAMERA::SetCurMousePosition. Call after every pointer move/down, unconditionally -- same order HIDPI_GL_3D_CANVAS::OnMouseMoveCamera uses (Drag/Pan first, against the OLD position, then this). */
  setCurMousePosition(x: number, y: number): void {
    this.lastMouse = { x, y };
  }

  get zoom(): number {
    return this.zoomValue;
  }

  get projection(): Projection {
    return this.projectionValue;
  }

  /** EDA_3D_CANVAS::DisplayStatus's "zoom %.2f" field: `1 / m_camera.GetZoom()`. */
  getZoomPercent(): number {
    return (1 / this.zoomValue) * 100;
  }

  /**
   * TRACK_BALL::Drag. `newX,newY` are pointer-canvas pixel coordinates;
   * reads the previous position from `setCurMousePosition`. A no-op until
   * both a window size and a previous position are known.
   */
  drag(newX: number, newY: number): void {
    if (!this.lastMouse || this.windowWidth <= 0 || this.windowHeight <= 0) return;
    const w = this.windowWidth;
    const h = this.windowHeight;
    // source: `(2*px - W)/W, (H - 2*py)/H` -- NDC with Y flipped (screen Y
    // grows downward; trackball.cpp's own space has Y growing upward).
    const p1x = (2 * this.lastMouse.x - w) / w;
    const p1y = (h - 2 * this.lastMouse.y) / h;
    const p2x = (2 * newX - w) / w;
    const p2y = (h - 2 * newY) / h;
    const spin = trackball(p1x, p1y, p2x, p2y);
    // source: `m_rotationMatrix = spin_matrix * m_rotationMatrix` (premultiply).
    this.trackballQuat = normalizeQuat(multiplyQuat(spin, this.trackballQuat));
  }

  /** TRACK_BALL::Pan(wxPoint) -- screen-space drag-to-pan (middle/right-drag), both projection branches. */
  pan(newX: number, newY: number): void {
    if (!this.lastMouse || this.windowWidth <= 0 || this.windowHeight <= 0) return;
    const w = this.windowWidth;
    const h = this.windowHeight;
    const dxPx = this.lastMouse.x - newX;
    const dyPx = newY - this.lastMouse.y;
    if (this.projectionValue === "ortho") {
      const { nw, nh } = this.orthoFullExtent();
      this.cameraPosLocal.x -= (nw * dxPx) / w;
      this.cameraPosLocal.y -= (nh * dyPx) / h;
    } else {
      const aspect = w / h;
      const panFactor = -this.cameraPosLocal.z * HALF_FOV_TAN * 2;
      this.cameraPosLocal.x -= (panFactor * aspect * dxPx) / w;
      this.cameraPosLocal.y -= (panFactor * dyPx) / h;
    }
  }

  /** TRACK_BALL::Pan(SFVEC3F) -- a direct local-space offset, used by arrow-key pan (VIEW3D_PAN_*) and the wheel-pan branches below. */
  panByWorld(dx: number, dy: number, dz = 0): void {
    this.cameraPosLocal.x += dx;
    this.cameraPosLocal.y += dy;
    this.cameraPosLocal.z += dz;
  }

  /** eda_3d_canvas.cpp SetView3D's VIEW3D_PAN_LEFT/RIGHT/UP/DOWN (arrow keys): `delta_move = m_delta_move_step_factor * zoom`, signed per direction. */
  panArrow(direction: "left" | "right" | "up" | "down"): void {
    const delta = PAN_STEP_FACTOR * this.zoomValue;
    switch (direction) {
      case "left":
        this.panByWorld(-delta, 0);
        break;
      case "right":
        this.panByWorld(delta, 0);
        break;
      case "up":
        this.panByWorld(0, delta);
        break;
      case "down":
        this.panByWorld(0, -delta);
        break;
    }
  }

  /** camera.cpp CAMERA::Zoom. Returns false (no-op) at a zoom limit or for a no-op factor, exactly like source -- callers (e.g. keyboard zoom in/out) use this to decide whether anything happened. */
  zoomBy(factor: number): boolean {
    if ((this.zoomValue <= this.minZoom && factor > 1) || (this.zoomValue >= this.maxZoom && factor < 1) || factor === 1) {
      return false;
    }
    const prevZoom = this.zoomValue;
    this.zoomValue /= factor;
    let effectiveFactor = factor;
    if (this.zoomValue <= this.minZoom && factor > 1) {
      effectiveFactor = prevZoom / this.minZoom;
      this.zoomValue = this.minZoom;
    } else if (this.zoomValue >= this.maxZoom && factor < 1) {
      effectiveFactor = prevZoom / this.maxZoom;
      this.zoomValue = this.maxZoom;
    }
    this.cameraPosLocal.z /= effectiveFactor;
    return true;
  }

  /**
   * hidpi_gl_3D_canvas.cpp HIDPI_GL_3D_CANVAS::OnMouseWheelCamera, ported
   * for this app's one real default: `m_MousewheelPanning` is hardcoded
   * `true` in board_adapter.cpp's ctor and never exposed as a user
   * preference in the interactive 3D viewer, so `aPan` is always true here
   * (see PARITY-3d.md). DOM has neither wx's single signed "wheel
   * rotation" value nor its per-event horizontal/vertical axis flag --
   * same dominant-delta translation this app's 2D port already uses
   * (kicad-port/viewControls.ts's own header comment): the larger-
   * magnitude of deltaX/deltaY is treated as "the" rotation, sign-flipped
   * so scrolling up/left is a positive rotation.
   *
   * With the real default settings (scrollModifierZoom=NONE,
   * scrollModifierPanH=CTRL, scrollReverseZoom/PanH=false), source's own
   * branch structure collapses to: a plain wheel with NO modifier always
   * ZOOMS, regardless of which axis it came from -- a native horizontal
   * trackpad swipe with no modifier is not special-cased into a pan the
   * way it is in the 2D viewport, because the outer branch that handles
   * it is gated on a modifier being held (`modifiers !=
   * m_scrollModifierZoom`, and NONE != NONE is false). Only
   * Shift/Ctrl/Alt+wheel pans. This is source's actual behavior, not a
   * simplification -- see PARITY-3d.md.
   */
  handleWheel(input: { deltaX: number; deltaY: number; shiftKey: boolean; ctrlKey: boolean; altKey: boolean }): void {
    const modifier: "none" | "shift" | "ctrl" | "alt" = input.shiftKey ? "shift" : input.ctrlKey ? "ctrl" : input.altKey ? "alt" : "none";
    const isHorizontalAxis = Math.abs(input.deltaX) > Math.abs(input.deltaY);
    const rotation = isHorizontalAxis ? -input.deltaX : -input.deltaY;

    if (modifier !== "none") {
      const deltaMove = PAN_STEP_FACTOR * this.zoomValue * WHEEL_PAN_SPEED * rotation;
      if (isHorizontalAxis || modifier === "ctrl") {
        this.panByWorld(-deltaMove, 0);
      } else {
        // shift or alt: source's own "else" branch doesn't distinguish them further.
        this.panByWorld(0, -deltaMove);
      }
      return;
    }

    this.zoomBy(rotation > 0 ? WHEEL_ZOOM_FACTOR : 1 / WHEEL_ZOOM_FACTOR);
  }

  /** eda_3d_controller.cpp EDA_3D_CONTROLLER::RotateView's X_CW/X_CCW/etc -> CAMERA::RotateX/Y/Z, radian angle, sign already resolved by the caller (ROTATION_STEP_DEG * +-1) -- see actions3d.ts for the exact CW/CCW-to-sign table (eda_3d_controller.cpp's own comment: "Y rotations are backward b/c the RHR has Y pointing into the screen"). */
  rotateX(angleRad: number): void {
    this.auxDeg.x += angleRad / DEG2RAD;
  }
  rotateY(angleRad: number): void {
    this.auxDeg.y += angleRad / DEG2RAD;
  }
  rotateZ(angleRad: number): void {
    this.auxDeg.z += angleRad / DEG2RAD;
  }

  toggleProjection(): void {
    this.projectionValue = this.projectionValue === "perspective" ? "ortho" : "perspective";
  }

  /** Absolute form of toggleProjection, for a caller (e.g. a checkbox-style UI toggle bound to persisted app state) that already knows the target mode rather than wanting to flip it. Not a direct CAMERA::SetProjection port concern -- source's own SetProjection is a plain setter too (camera.h). */
  setProjection(projection: Projection): void {
    this.projectionValue = projection;
  }

  /**
   * eda_3d_canvas.cpp SetView3D's TOP/BOTTOM/LEFT/RIGHT/FRONT/BACK cases:
   * `Reset_T1()` (zeroes aux, resets pos/lookat/zoom, resets the trackball
   * quaternion to identity) then the preset's own Rotate*_T1 calls --
   * applied here immediately rather than as an animated T1 target (no
   * camera-move animation is ported; see PARITY-3d.md).
   */
  applyViewPreset(preset: ViewPreset): void {
    this.trackballQuat = { ...QUAT_IDENTITY };
    this.auxDeg = { ...PRESET_AUX_DEG[preset] };
    this.cameraPosLocal = vec3(0, 0, -this.initialDistanceMm);
    this.lookAt = { ...this.lookAtInit };
    this.zoomValue = 1;
  }

  /**
   * camera.cpp CAMERA::Reset(). Note this is observably identical to
   * `applyViewPreset("top")` -- both zero the trackball quaternion and the
   * aux angles and reset pos/lookat/zoom to their init values, because
   * TOP's own aux target (above) is already (0,0,0). That is a real KiCad
   * fact (Home/"zoom to fit" really is the same orientation as the Top
   * view, confirmed directly against source), not an approximation made
   * here.
   */
  reset(): void {
    this.applyViewPreset("top");
  }

  /**
   * camera.cpp CAMERA::ViewCommand_T1's VIEW3D_FLIP case: `RotateY_T1(
   * 179.999deg)` with NO preceding Reset_T1 -- i.e. additive to whatever
   * aux/trackball state is already live, unlike the 6 face presets above.
   * Position/lookat/zoom are untouched.
   */
  flip(): void {
    this.auxDeg.y += FLIP_EPSILON_DEG;
  }

  /**
   * eda_3d_canvas.cpp move_pivot_based_on_cur_mouse_position's effect
   * (the camera repose itself; the ray-vs-board-bbox hit test that
   * produces `worldPoint` is this app's own responsibility at the
   * Viewer3D.tsx integration layer, via THREE.Raycaster, since it needs
   * the live Three.js scene/camera matrices this module deliberately
   * doesn't own). Bound to Space (EDA_3D_ACTIONS::pivotCenter) and to a
   * middle-mouse click that wasn't a drag.
   */
  pivotAt(worldPoint: Vec3): void {
    this.lookAt = { ...worldPoint };
  }

  private orthoFullExtent(): { nw: number; nh: number } {
    const aspect = this.windowWidth > 0 && this.windowHeight > 0 ? this.windowWidth / this.windowHeight : 1;
    // camera.cpp rebuildProjection's ORTHO branch.
    const orthoReductionFactor = this.initialDistanceMm * this.zoomValue * HALF_FOV_TAN;
    return { nw: 2 * aspect * orthoReductionFactor, nh: 2 * orthoReductionFactor };
  }

  /**
   * The camera's world-space position/orientation for this app's actual
   * renderer -- see this file's header comment for the basis-change
   * derivation. `Rtotal = trackballQuat * auxQuat` mirrors `m_rotationMatrix
   * * m_rotationMatrixAux`; `auxQuat` is rebuilt fresh from `auxDeg` every
   * call (Rx*Ry*Rz product), matching updateRotationMatrix()'s own
   * from-scratch rebuild rather than composing incrementally.
   */
  getRenderPose(): RenderPose {
    const auxQuat = multiplyQuat(
      multiplyQuat(quatFromAxisAngle(vec3(1, 0, 0), this.auxDeg.x * DEG2RAD), quatFromAxisAngle(vec3(0, 1, 0), this.auxDeg.y * DEG2RAD)),
      quatFromAxisAngle(vec3(0, 0, 1), this.auxDeg.z * DEG2RAD)
    );
    const rTotal = normalizeQuat(multiplyQuat(this.trackballQuat, auxQuat));
    const rTotalInv = conjugateQuat(rTotal);
    const offsetLocal = rotateVecByQuat(negV(this.cameraPosLocal), rTotalInv);
    const offsetWorld = rotateVecByQuat(offsetLocal, BASIS_CHANGE);
    return {
      position: addV(this.lookAt, offsetWorld),
      quaternion: normalizeQuat(multiplyQuat(BASIS_CHANGE, rTotalInv)),
    };
  }

  getProjectionParams(): ProjectionParams {
    if (this.projectionValue === "perspective") {
      return { kind: "perspective", fovDeg: FOV_DEG, near: this.nearMm, far: this.farMm };
    }
    const { nw, nh } = this.orthoFullExtent();
    return { kind: "ortho", left: -nw / 2, right: nw / 2, top: nh / 2, bottom: -nh / 2, near: this.nearMm, far: this.farMm };
  }
}
