// The 3D PCB viewer tab -- a KiCad-style "3D Viewer" for the board this
// studio is editing. Self-contained: owns its own Three.js
// renderer/scene/camera(s)/render loop, reading board data straight from
// the store. See App.tsx's `state.tab === "3d"` slot in canvas-col, which
// mounts this component full-bleed the same way Canvas/SchematicView are
// mounted there.
//
// Viewer3DToolbar (the view-preset/visibility buttons) is mounted
// *separately* by App.tsx, in the main-toolbar-row (replacing the normal
// <Toolbar id="main"/> for this tab -- the 3D tab has no real icon-toolbar
// action set of its own). The two talk over a small imperative handle
// (`Viewer3DApi`) rather than a shared prop/context: this component
// creates the handle once its camera exists and hands it up via
// `onReady`, App.tsx holds it in state, and passes it down to
// Viewer3DToolbar as `api`.
//
// Geometry building lives in scene.ts (pure, no React/DOM); the camera
// itself is kicad-port/camera3d.ts's TrackballCamera, a faithful port of
// KiCad's own CAMERA/TRACK_BALL (see that file's header comment for the
// full derivation) -- this file is orchestration only: container/resize
// lifecycle (same pattern as components/canvas/Canvas.tsx), the Three.js
// object lifecycle, translating DOM pointer/wheel events into
// TrackballCamera calls (mirroring HIDPI_GL_3D_CANVAS::OnMouseMoveCamera/
// OnMouseWheelCamera's own structure), the render loop, and the
// view-preset handle.
import { useCallback, useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { fetchBoardGlb } from "../../api/client";
import { useStudioDispatch, useStudioState } from "../../state/store";
import { TrackballCamera, FOV_DEG, KEY_ZOOM_FACTOR, ROTATION_STEP_DEG, type ViewPreset as FacePreset } from "../../kicad-port/camera3d";
import { resolve3DAction, ROTATE_SIGN, type Action3D } from "../../kicad-port/actions3d";
import { isMac } from "../../platform";
import { buildBoardGroup, buildBackgroundTexture, countByName, disposeObject3D, boardOutlineBounds } from "./scene";
import { MODEL_ROWS, ROWS, isVisible, toggled } from "../../kicad-port/appearance3d";
import { useViewerColors } from "./useViewerColors";
import { ModelCache } from "./modelCache";
import { PartModels, type PartStats } from "./partModels";
import { viewer3dProbe } from "./viewer3dProbe";

const ROTATION_STEP_RAD = (ROTATION_STEP_DEG * Math.PI) / 180;

/** glTF's own unit is meters; every other builder in this file (scene.ts) works in millimetres, so the loaded GLB scene is scaled up to match rather than rescaling everything else down to meters. */
const GLB_METERS_TO_MM = 1000;
/** How often to re-poll GET /api/board.glb while the backend reports `{"status":"pending"}`. The export itself can take minutes (see studio.rs's build_glb), so this is deliberately slower than the app's main 700ms state poll (store.tsx) -- there is no reason to hammer the endpoint every tick for something this slow. */
const GLB_POLL_MS = 1500;
/** Placeholder board span (mm) the camera is seeded with before any real board data has arrived -- promptly corrected by the board-data effect below (setBoardGeometry) the moment `board.outline` is known, the same bootstrap sequence the old Box3-based fit used (`lastFitBoundsRef` starting at null). */
const PLACEHOLDER_BOARD_SPAN_MM = 100;
/** boardOutlineBounds's fallback span/lookAt when a board has no usable outline at all -- an arbitrary but reasonable finite box, same role as scene.ts's old EMPTY_BOX_HALF_EXTENT_MM. */
const EMPTY_BOARD_SPAN_MM = 20;

/** Small, unobtrusive corner badge while the background kicad-cli export runs -- the procedural scene stays fully interactive underneath it the whole time (see this file's fetch effect), so this is a status note, not a loading overlay that blocks the view. */
const GLB_STATUS_BADGE_STYLE: React.CSSProperties = {
  position: "absolute",
  left: 10,
  bottom: 10,
  padding: "4px 8px",
  borderRadius: 4,
  background: "var(--chrome-bg, #1e1e1e)",
  color: "var(--chrome-text-dim, #999)",
  font: "11px/1 inherit",
  border: "1px solid var(--chrome-border, #444)",
  pointerEvents: "none",
};

/** KiCad's "hovered item" status text (`EDA_3D_VIEWER_STATUSBAR::HOVERED_ITEM`: the reference and value of the footprint under the pointer), over the view's top-left corner. */
const HOVER_BADGE_STYLE: React.CSSProperties = {
  position: "absolute",
  left: 10,
  top: 10,
  padding: "4px 8px",
  borderRadius: 4,
  background: "var(--chrome-bg, #1e1e1e)",
  color: "var(--chrome-text, #ddd)",
  font: "12px/1 inherit",
  border: "1px solid var(--chrome-border, #444)",
  pointerEvents: "none",
};

interface ThreeContext {
  renderer: THREE.WebGLRenderer;
  scene: THREE.Scene;
  /** Both kept in sync with `camera3d` every frame (position/quaternion/projection params); the render loop picks whichever `camera3d.projection` says is active. Two real THREE.Camera objects rather than one camera with a hand-patched projection matrix, so every other Three.js API (raycasting, frustum culling) keeps working normally regardless of projection mode. */
  perspCamera: THREE.PerspectiveCamera;
  orthoCamera: THREE.OrthographicCamera;
  /** The ported KiCad camera (kicad-port/camera3d.ts) -- owns all actual position/rotation/zoom/projection state; the two THREE cameras above are just read out of it every frame. */
  camera3d: TrackballCamera;
  boardGroup: THREE.Group;
  /** GET /api/board.glb's real KiCad-rendered board, loaded into its own group so it can be shown/hidden independently of the procedural boardGroup rather than swapped in and out of the scene (cheaper, and keeps whichever one is hidden ready to reappear instantly). Empty until a GLB successfully loads. */
  glbGroup: THREE.Group;
  /** The 3D models of the parts, loaded one by one from GET /api/3dmodel (modelCache.ts) -- no full-board export waited for -- and the parts drawn with them (partModels.ts). */
  cache: ModelCache;
  partModels: PartModels;
}

export type ViewPreset = FacePreset | "reset";

/** The imperative handle Viewer3D hands up via `onReady`, for Viewer3DToolbar (or anything else) to drive the camera without owning it. */
export interface Viewer3DApi {
  /** Moves the camera to one of KiCad's 6 face-view presets, or (`"reset"`) back to its home pose -- KiCad's own Home/"zoom to fit" is observably identical to the Top preset (see camera3d.ts's `reset()` doc comment), ported as the same call here. */
  setView(preset: ViewPreset): void;
  /** Every other camera action the KiCad 3D toolbar exposes (zoom in/out, rotate X/Y/Z CW/CCW, flip, move L/R/U/D) -- the same Action3D union the keyboard handler below dispatches, so the toolbar and the keyboard can never drift apart on what a given action actually does. "pivot" is accepted but is a no-op from the toolbar (it needs a live pointer position this handle doesn't carry -- KiCad's own Space hotkey and middle-click share this same limitation-free path only because they both originate from a real mouse event). */
  dispatchAction(action: Exclude<Action3D, { kind: "viewPreset" } | { kind: "reset" }>): void;
  /** `EDA_3D_ACTIONS::reloadBoard`: build KiCad's render of the board again now (a failed export is otherwise kept until the board changes). */
  reload(): void;
  /** `EDA_3D_ACTIONS::copyToClipboard`: the current view as a PNG on the clipboard; resolves whether the browser took it. */
  copyImage(): Promise<boolean>;
}

/**
 * Runs one kicad-port/actions3d.ts Action3D against `camera3d` -- shared
 * by the keyboard handler (mount effect below) and Viewer3DApi.dispatchAction
 * (Viewer3DToolbar's rotate/zoom/move/flip buttons) so there is exactly
 * one place that knows e.g. which sign CW/CCW maps to per axis.
 */
function runAction3D(camera3d: TrackballCamera, action: Action3D, pivot: () => void): void {
  // `EDA_3D_CANVAS::SetView3D`: the views, flip, zoom steps, arrow pans and the pivot are animated moves (camera3d.ts, "animation"), and a view command made while
  // one is going is ignored (`if( m_camera_is_moving ) return false;`). The rotate steps are made at once, as `EDA_3D_CONTROLLER::RotateView` makes them.
  const now = performance.now();
  switch (action.kind) {
    case "viewPreset":
      camera3d.animateViewPreset(action.preset, now);
      break;
    case "reset":
      camera3d.animateReset(now);
      break;
    case "flip":
      camera3d.animateFlip(now);
      break;
    case "pivot":
      pivot();
      break;
    case "zoomIn":
      camera3d.animateZoom(KEY_ZOOM_FACTOR, now);
      break;
    case "zoomOut":
      camera3d.animateZoom(1 / KEY_ZOOM_FACTOR, now);
      break;
    case "zoomRedraw":
      // eda_3d_canvas.cpp's ZoomRedraw just calls Request_refresh() --
      // this app's render loop already repaints every frame
      // unconditionally, so there is nothing to invalidate. A real
      // no-op, same convention as the 2D port's own zoomRedraw
      // (PARITY-pcb.md).
      break;
    case "rotate": {
      if (camera3d.isMoving()) break;
      const sign = ROTATE_SIGN[action.axis][action.dir];
      const angle = sign * ROTATION_STEP_RAD;
      if (action.axis === "x") camera3d.rotateX(angle);
      else if (action.axis === "y") camera3d.rotateY(angle);
      else camera3d.rotateZ(angle);
      break;
    }
    case "pan":
      camera3d.animatePan(action.direction, now);
      break;
  }
}

/** Converts a kicad-port Vec3/Quat into the THREE.Vector3/THREE.Quaternion a camera actually needs -- the one place camera3d.ts's deliberately dependency-free output touches three.js (see camera3d.ts's header comment for why that module itself never imports three). */
function applyPoseToCamera(cam: THREE.Camera, pose: ReturnType<TrackballCamera["getRenderPose"]>): void {
  cam.position.set(pose.position.x, pose.position.y, pose.position.z);
  cam.quaternion.set(pose.quaternion.x, pose.quaternion.y, pose.quaternion.z, pose.quaternion.w);
}

type DragButton = "left" | "middle" | "right" | null;

export function Viewer3D({ onReady }: { onReady?: (api: Viewer3DApi | null) => void }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const board = state.board;

  const containerRef = useRef<HTMLDivElement>(null);
  const threeRef = useRef<ThreeContext | null>(null);
  const lastFitBoundsRef = useRef<{ minX: number; minZ: number; maxX: number; maxZ: number } | null>(null);
  /** GET /api/board.glb's own loaded-state -- see syncActiveGroupRef below. */
  const glbLoadedRef = useRef(false);
  // `onReady` kept in a ref and read only inside effects, not listed as
  // an effect dependency, so an unstable inline callback from a caller
  // can never tear down and recreate the whole WebGL context on every
  // render -- only mount/unmount and board changes should do that.
  const onReadyRef = useRef(onReady);
  useEffect(() => {
    onReadyRef.current = onReady;
  }, [onReady]);
  // "Reload board": bumps the nonce the GLB fetch effect below depends on, and asks its first request to build again (`?retry=1`).
  const [reloadNonce, setReloadNonce] = useState(0);
  /** The models still loading, for the corner note: how many of the board's models are in. */
  const [modelProgress, setModelProgress] = useState<{ done: number; total: number } | null>(null);
  /** The part under the pointer: its reference and value, KiCad's "hovered item" text (`EDA_3D_CANVAS::OnMouseMove`). */
  const [hoverText, setHoverText] = useState<string | null>(null);
  const retryNextRef = useRef(false);
  // Same ref-not-dependency reasoning for the view-option toggles: read
  // fresh inside effects (stable, empty deps) rather than closed over.
  const viewer3dRef = useRef(state.viewer3d);
  useEffect(() => {
    viewer3dRef.current = state.viewer3d;
  }, [state.viewer3d]);
  /** Same reasoning again, for the mount effect's pivot-hotkey hit test below (empty deps -- `board` itself would otherwise be captured stale from whatever it was on first mount, almost always `null`). */
  const boardRef = useRef(board);
  useEffect(() => {
    boardRef.current = board;
  }, [board]);
  /** Tracks the previous `flipped` value so the flip effect below (keyed on that boolean) can tell "the user just toggled it" apart from "this effect also runs once on mount" -- `camera3d.flip()` is additive (KiCad's own flipView action, see camera3d.ts), not an absolute set, so it must fire exactly once per real toggle, never on mount. */
  const prevFlippedRef = useRef(state.viewer3d.flipped);

  /**
   * Moves the camera to `preset` using the ported KiCad camera engine
   * (kicad-port/camera3d.ts) -- shared by the initial/on-change auto-fit
   * below and by every Viewer3DToolbar button. Reads `threeRef.current`
   * fresh on every call rather than closing over anything from render
   * scope, so it stays referentially stable (empty dep array).
   */
  const applyPreset = useCallback((preset: ViewPreset) => {
    const three = threeRef.current;
    if (!three) return;
    const now = performance.now();
    if (preset === "reset") three.camera3d.animateReset(now);
    else three.camera3d.animateViewPreset(preset, now);
  }, []);

  // Mount: create the renderer/scene/cameras/camera3d engine once and
  // start the render loop. Mirrors Canvas.tsx's container-tracks-its-
  // own-box ResizeObserver pattern, just driving a WebGL canvas instead
  // of a 2D one -- and since the render loop already repaints every
  // frame regardless, there's no need to round-trip a resize through
  // React state the way Canvas.tsx's 2D repaint effect does.
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const scene = new THREE.Scene();
    scene.background = buildBackgroundTexture();

    const camera3d = new TrackballCamera(PLACEHOLDER_BOARD_SPAN_MM * 1.25, { x: 0, y: 0, z: 0 });
    // Initial fov/near/far are placeholders -- the render loop below
    // overwrites them from camera3d.getProjectionParams() every frame, so
    // these only matter for the very first paint before that loop runs.
    const perspCamera = new THREE.PerspectiveCamera(FOV_DEG, 1, 0.1, 1000);
    const orthoCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 1000);

    const renderer = new THREE.WebGLRenderer({ antialias: true });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.domElement.tabIndex = 0; // forward-compatible with keyboard hotkeys (not wired up in this pass)
    renderer.domElement.style.touchAction = "none"; // this component drives drag/pan itself; don't let the browser also scroll/zoom the page on touch
    renderer.domElement.style.outline = "none";
    const rect = container.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) {
      renderer.setSize(rect.width, rect.height);
      perspCamera.aspect = rect.width / rect.height;
      perspCamera.updateProjectionMatrix();
      camera3d.setWindowSize(rect.width, rect.height);
    }
    container.appendChild(renderer.domElement);

    // render_3d_opengl.cpp's init_lights(): exactly 3 fixed-function GL
    // lights, always all enabled together -- "top" and "bottom" fixed
    // directional lights at a shallow 5.4-degree elevation (so they
    // mostly skim the board, giving its copper/silk a visible highlight
    // rather than flooding it from directly overhead), and "front", a
    // positional headlight that moves with the camera every frame (see
    // the render loop below). Source's own light colors are all neutral
    // grey/white (ambient 0.084, diffuse/specular 0.7 for top & bottom,
    // 0.3/0.5 for the headlight) with NO separate scene-wide ambient term
    // (`GL_LIGHT_MODEL_AMBIENT=(0,0,0)`) -- all ambient comes from the 3
    // lights' own small ambient terms, folded here into one AmbientLight
    // at roughly their combined strength (0.084*3 ~= 0.25) since
    // Three.js's MeshStandardMaterial has no per-light ambient slot.
    //
    // Direction vectors are given in KiCad's own axis convention
    // (SphericalToCartesian(pi*0.03, pi*0.25), 3d_math.h/render_3d_opengl.
    // cpp:457-474) and converted through this file's camera basis change
    // (kicad-port/camera3d.ts's header comment: KiCad-local/world (x,y,z)
    // -> this app's world (x, z, -y)) rather than re-deriving the angle
    // in this app's own axes.
    scene.add(new THREE.AmbientLight(0xffffff, 0.25));
    const TOP_LIGHT_DIR = new THREE.Vector3(0.0665, 0.9956, -0.0665); // KiCad (0.0665, 0.0665, 0.9956)
    const topLight = new THREE.DirectionalLight(0xffffff, 1.0);
    topLight.position.copy(TOP_LIGHT_DIR).multiplyScalar(1000);
    scene.add(topLight);
    const bottomLight = new THREE.DirectionalLight(0xffffff, 1.0);
    bottomLight.position.copy(TOP_LIGHT_DIR).multiplyScalar(-1000); // source: same x/y, z negated -- a full negation is the same thing once converted to this app's axes
    scene.add(bottomLight);
    // Headlight ("front"): positional, re-aimed at the camera's own
    // position every frame below. `decay=0` (no inverse-square falloff)
    // deliberately: a physically-decaying point light would make this
    // swing from imperceptible to blinding across this viewer's actual
    // zoom range (a few mm to hundreds of mm from the board), which
    // source's own fixed-function GL point light never did either (GL's
    // legacy attenuation was left at its default 1/0/0 = no falloff).
    const headlight = new THREE.PointLight(0xffffff, 0.45, 0, 0);
    scene.add(headlight);

    const boardGroup = new THREE.Group();
    scene.add(boardGroup);
    const glbGroup = new THREE.Group();
    glbGroup.name = "viewer3d-glb";
    glbGroup.visible = false;
    scene.add(glbGroup);

    // The parts: each is its KiCad 3D models as they load, a box until its first one is in.
    viewer3dProbe.opened();
    let partStats: PartStats = { total: 0, asModels: 0, asBoxes: 0, shown: 0 };
    const cache = new ModelCache();
    const publish = () => {
      viewer3dProbe.update({
        models: cache.stats(),
        parts: partStats,
        hovered: partModels.hoveredRef,
        detail: cache.entries().map((r) => ({ name: r.name, status: r.status, fetchMs: r.fetchMs && Math.round(r.fetchMs), parseMs: r.parseMs && Math.round(r.parseMs), triangles: r.triangles, error: r.error })),
      });
      const m = cache.stats();
      setModelProgress(m.requested > 0 && m.loading > 0 ? { done: m.requested - m.loading, total: m.requested } : null);
    };
    const partModels = new PartModels(cache, (stats) => {
      partStats = stats;
      publish();
    });
    scene.add(partModels.group);
    const stopPublishing = cache.onChange(publish);
    // For `window.__eda.viewer3dScreen(ref)`: where the part is on the canvas right now.
    viewer3dProbe.setScreenOf((ref) => {
      const part = boardRef.current?.parts.find((p) => p.ref === ref);
      if (!part?.at) return null;
      const cam = camera3d.projection === "perspective" ? perspCamera : orthoCamera;
      cam.updateMatrixWorld();
      const p = new THREE.Vector3(part.at[0] / 1000, part.side === "bottom" ? -1 : 1, part.at[1] / 1000).project(cam);
      return { x: ((p.x + 1) / 2) * renderer.domElement.clientWidth, y: ((1 - p.y) / 2) * renderer.domElement.clientHeight };
    });

    threeRef.current = { renderer, scene, perspCamera, orthoCamera, camera3d, boardGroup, glbGroup, cache, partModels };

    // ---------------------------------------------------------- input
    // Ports HIDPI_GL_3D_CANVAS::OnMouseMoveCamera/OnMouseWheelCamera
    // (common/gal/hidpi_gl_3D_canvas.cpp): left-drag rotates (trackball),
    // middle/right-drag pans (KiCad's own `drag_middle`/`drag_right`
    // default PAN), wheel zooms (or pans with a modifier held -- see
    // TrackballCamera.handleWheel's own header comment for exactly which
    // modifier does what, ported condition-for-condition against source).
    let dragButton: DragButton = null;
    /** Whether the live drag actually moved the camera -- distinguishes a middle-click from a middle-drag, below (EDA_3D_CANVAS::OnMiddleUp's own `if (m_mouse_is_moving) ... else move_pivot_based_on_cur_mouse_position()`). */
    let draggedThisPress = false;
    /** The pointer's last known canvas-pixel position, for the Space/pivot hotkey and the middle-click pivot below -- KiCad's own pivot action (EDA_3D_ACTIONS::pivotCenter / OnMiddleUp -> move_pivot_based_on_cur_mouse_position) uses "wherever the mouse last was", not a position the triggering event itself carries. */
    let lastPointerPx: { x: number; y: number } | null = null;
    const el = renderer.domElement;
    const raycaster = new THREE.Raycaster();
    const boardPlane = new THREE.Plane(new THREE.Vector3(0, 1, 0), 0); // world Y=0: the board's own mid-thickness plane (scene.ts's convention)

    /**
     * eda_3d_canvas.cpp move_pivot_based_on_cur_mouse_position's hit test,
     * simplified from "intersect the board's real 3D bounding box" to
     * "intersect the board's flat mid-thickness plane within its outline's
     * 2D bounds" -- this app's board model has no single 3D collision mesh
     * to test against, and a flat board is thin enough that the
     * difference is not visually meaningful. No-op (matches source's own
     * `if (Intersect(...))` guard) when the ray misses the plane entirely
     * (looking edge-on) or lands outside the board.
     */
    const pivotAtPointer = (px: { x: number; y: number }) => {
      const three = threeRef.current;
      if (!three || three.renderer.domElement.clientWidth <= 0) return;
      const ndc = new THREE.Vector2((px.x / el.clientWidth) * 2 - 1, -(px.y / el.clientHeight) * 2 + 1);
      const activeCam = camera3d.projection === "perspective" ? three.perspCamera : three.orthoCamera;
      raycaster.setFromCamera(ndc, activeCam);
      const hit = new THREE.Vector3();
      if (!raycaster.ray.intersectPlane(boardPlane, hit)) return;
      const currentBoard = boardRef.current;
      const bounds = currentBoard ? boardOutlineBounds(currentBoard) : null;
      if (bounds && (hit.x < bounds.minX || hit.x > bounds.maxX || hit.z < bounds.minZ || hit.z > bounds.maxZ)) return;
      camera3d.animatePivot({ x: hit.x, y: 0, z: hit.z }, performance.now());
    };

    const onPointerDown = (e: PointerEvent) => {
      el.focus();
      camera3d.setCurMousePosition(e.offsetX, e.offsetY);
      lastPointerPx = { x: e.offsetX, y: e.offsetY };
      draggedThisPress = false;
      if (e.button === 0) dragButton = "left";
      else if (e.button === 1) dragButton = "middle";
      else if (e.button === 2) dragButton = "right";
      else return;
      el.setPointerCapture(e.pointerId);
    };
    // `EDA_3D_CANVAS::OnMouseMove`'s rollover: when the pointer is not dragging, the part under it is highlighted (`highlight_on_rollover`) and its reference and value are
    // reported. One ray per frame at most, and none while the camera moves (`if( m_camera_is_moving ) return;`).
    let hoverPx: { x: number; y: number } | null = null;
    let hoverFrame = 0;
    const hoverRay = () => {
      hoverFrame = 0;
      const three = threeRef.current;
      if (!three || !hoverPx || three.camera3d.isMoving()) return;
      const activeCam = camera3d.projection === "perspective" ? three.perspCamera : three.orthoCamera;
      raycaster.setFromCamera(new THREE.Vector2((hoverPx.x / el.clientWidth) * 2 - 1, -(hoverPx.y / el.clientHeight) * 2 + 1), activeCam);
      const hit = three.partModels.pick(raycaster);
      if (three.partModels.setHover(hit?.ref ?? null)) {
        const part = hit ? boardRef.current?.parts.find((p) => p.ref === hit.ref) : undefined;
        setHoverText(part ? `${part.ref}  ${part.value ?? ""}`.trimEnd() : null);
        viewer3dProbe.update({ hovered: three.partModels.hoveredRef });
      }
    };
    const requestHover = (px: { x: number; y: number } | null) => {
      hoverPx = px;
      if (px === null) {
        if (hoverFrame) cancelAnimationFrame(hoverFrame);
        hoverFrame = 0;
        if (threeRef.current?.partModels.setHover(null)) {
          setHoverText(null);
          viewer3dProbe.update({ hovered: null });
        }
        return;
      }
      if (!hoverFrame) hoverFrame = requestAnimationFrame(hoverRay);
    };
    const onPointerLeave = () => requestHover(null);
    const onPointerMove = (e: PointerEvent) => {
      if (dragButton === null) {
        // From the client position and the canvas's box, which is the pointer's place on the canvas for a real event and for one a script dispatches alike (a script's `offsetX` is not reliable).
        const box = el.getBoundingClientRect();
        requestHover({ x: e.clientX - box.left, y: e.clientY - box.top });
      } else if (hoverPx !== null) requestHover(null);
      // `if( m_camera_is_moving ) return;` -- the mouse does nothing to a camera that is on its way to a view.
      if (camera3d.isMoving()) {
        camera3d.setCurMousePosition(e.offsetX, e.offsetY);
        lastPointerPx = { x: e.offsetX, y: e.offsetY };
        return;
      }
      if (dragButton === "left") {
        camera3d.drag(e.offsetX, e.offsetY);
        draggedThisPress = true;
      } else if (dragButton === "middle" || dragButton === "right") {
        camera3d.pan(e.offsetX, e.offsetY);
        draggedThisPress = true;
      }
      camera3d.setCurMousePosition(e.offsetX, e.offsetY);
      lastPointerPx = { x: e.offsetX, y: e.offsetY };
    };
    const onPointerUp = (e: PointerEvent) => {
      if (dragButton === "middle" && !draggedThisPress && lastPointerPx) pivotAtPointer(lastPointerPx);
      dragButton = null;
      if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
    };
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      if (camera3d.isMoving()) return;
      // Cmd is accepted alongside Ctrl for the pan-horizontal modifier:
      // source's own default (view_controls.cpp's WXK_CONTROL) means the
      // *physical* Control key on every platform including macOS, but
      // physical-Ctrl+scroll is also macOS's system "zoom the screen"
      // accessibility gesture -- accepting Cmd too avoids that clash on a
      // trackpad without changing the Windows/Linux behavior at all.
      camera3d.handleWheel({ deltaX: e.deltaX, deltaY: e.deltaY, shiftKey: e.shiftKey, ctrlKey: e.ctrlKey || e.metaKey, altKey: e.altKey });
    };
    // KiCad shows a real right-click context menu (view presets, rotate
    // submenu, flip, move submenu -- eda_3d_controller.cpp's Init()) when
    // a right-click wasn't a drag; this app has no such menu yet, so the
    // browser's own is suppressed unconditionally rather than popping up
    // and fighting with right-drag-to-pan. See PARITY-3d.md.
    const onContextMenu = (e: MouseEvent) => e.preventDefault();

    const macPlatform = isMac();
    /**
     * kicad-port/actions3d.ts's resolve3DAction, dispatched here (not in
     * useGlobalHotkeys.ts/actions.json -- see actions3d.ts's own header
     * comment for why the 3D tab's hotkeys are kept fully separate from
     * this app's shared hotkey system). Attached to the canvas itself
     * (focused on pointerdown and once right after mount, below) rather
     * than `window`, so these keys only ever fire while the 3D view
     * actually has focus; `stopPropagation()` on every key this resolves
     * to something keeps it from *also* reaching that shared window-level
     * listener and maybe firing an unrelated same-key 2D action.
     */
    const onKeyDown = (e: KeyboardEvent) => {
      // eda_3d_actions.cpp's showTHT ('T') / showSMD ('S') / showVirtual ('V') -- visibility
      // view-OPTIONS, not camera actions, so these go straight to the
      // store (Viewer3DOptions) rather than through Action3D/camera3d.
      // (showNotInPosFile/showDNP -- 'P'/'D' in source -- have no
      // equivalent in this app's data model: no pick-and-place or DNP
      // concept exists here, so those 2 hotkeys are not bound; see PARITY-3d.md.)
      const modelRow = e.key === "t" || e.key === "T" ? "th_models" : e.key === "s" || e.key === "S" ? "smd_models" : e.key === "v" || e.key === "V" ? "virtual_models" : null;
      if (modelRow && !e.ctrlKey && !e.metaKey && !e.altKey) {
        e.preventDefault();
        e.stopPropagation();
        dispatch({ type: "SET_VIEWER3D_OPTIONS", options: { layers: toggled(viewer3dRef.current.layers, modelRow) } });
        return;
      }

      const action = resolve3DAction(e, macPlatform);
      if (!action) return;
      e.preventDefault();
      e.stopPropagation();
      runAction3D(camera3d, action, () => {
        if (lastPointerPx) pivotAtPointer(lastPointerPx);
      });
    };

    onReadyRef.current?.({
      setView: applyPreset,
      dispatchAction: (action) =>
        runAction3D(camera3d, action, () => {
          if (lastPointerPx) pivotAtPointer(lastPointerPx);
        }),
      reload: () => {
        // `EDA_3D_ACTIONS::reloadBoard`: every model is read again (a model file that changed on disk, a conversion that failed), and KiCad's export builds again when it is the view.
        cache.clear();
        partModels.refresh();
        retryNextRef.current = true;
        setReloadNonce((n) => n + 1);
      },
      copyImage: async () => {
        // Render and read the canvas in the same task: a WebGL canvas without a preserved drawing buffer is only guaranteed to hold the frame until it is composited.
        renderer.render(scene, camera3d.projection === "perspective" ? perspCamera : orthoCamera);
        const blob = await new Promise<Blob | null>((resolve) => renderer.domElement.toBlob(resolve, "image/png"));
        if (!blob) return false;
        try {
          await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
          return true;
        } catch {
          return false;
        }
      },
    });

    el.addEventListener("pointerdown", onPointerDown);
    el.addEventListener("pointermove", onPointerMove);
    el.addEventListener("pointerup", onPointerUp);
    el.addEventListener("pointercancel", onPointerUp);
    el.addEventListener("pointerleave", onPointerLeave);
    el.addEventListener("wheel", onWheel, { passive: false });
    el.addEventListener("contextmenu", onContextMenu);
    el.addEventListener("keydown", onKeyDown);
    // Auto-focus once on mount, so hotkeys work the moment the 3D tab
    // becomes visible without requiring a click first (matching a native
    // desktop app's own active-pane focus behavior).
    el.focus();

    const ro = new ResizeObserver((entries) => {
      const box = entries[0]?.contentRect;
      if (!box || box.width <= 0 || box.height <= 0) return;
      renderer.setSize(box.width, box.height);
      perspCamera.aspect = box.width / box.height;
      perspCamera.updateProjectionMatrix();
      camera3d.setWindowSize(box.width, box.height);
    });
    ro.observe(container);

    let raf = 0;
    let wasMoving = false;
    const animate = () => {
      // The move in flight (`EDA_3D_CANVAS::OnPaint`): the camera at this frame's time.
      const moving = camera3d.tick(performance.now());
      if (moving !== wasMoving) {
        wasMoving = moving;
        viewer3dProbe.update({ cameraMoving: moving });
      }
      const pose = camera3d.getRenderPose();
      const proj = camera3d.getProjectionParams();
      // render_3d_opengl.cpp's init_lights() "front" headlight moves with
      // the camera every Redraw() -- same here, once per frame rather
      // than wiring it through every single camera-mutating method.
      headlight.position.set(pose.position.x, pose.position.y, pose.position.z);
      let active: THREE.Camera;
      if (proj.kind === "perspective") {
        perspCamera.fov = proj.fovDeg;
        perspCamera.near = proj.near;
        perspCamera.far = proj.far;
        perspCamera.updateProjectionMatrix();
        applyPoseToCamera(perspCamera, pose);
        active = perspCamera;
      } else {
        orthoCamera.left = proj.left;
        orthoCamera.right = proj.right;
        orthoCamera.top = proj.top;
        orthoCamera.bottom = proj.bottom;
        orthoCamera.near = proj.near;
        orthoCamera.far = proj.far;
        orthoCamera.updateProjectionMatrix();
        applyPoseToCamera(orthoCamera, pose);
        active = orthoCamera;
      }
      renderer.render(scene, active);
      raf = requestAnimationFrame(animate);
    };
    raf = requestAnimationFrame(animate);

    return () => {
      onReadyRef.current?.(null);
      cancelAnimationFrame(raf);
      ro.disconnect();
      el.removeEventListener("pointerdown", onPointerDown);
      el.removeEventListener("pointermove", onPointerMove);
      el.removeEventListener("pointerup", onPointerUp);
      el.removeEventListener("pointercancel", onPointerUp);
      el.removeEventListener("pointerleave", onPointerLeave);
      el.removeEventListener("wheel", onWheel);
      el.removeEventListener("contextmenu", onContextMenu);
      el.removeEventListener("keydown", onKeyDown);
      disposeObject3D(boardGroup);
      scene.remove(boardGroup);
      disposeObject3D(glbGroup);
      scene.remove(glbGroup);
      stopPublishing();
      viewer3dProbe.setScreenOf(null);
      partModels.dispose();
      scene.remove(partModels.group);
      cache.dispose();
      viewer3dProbe.closed();
      (scene.background as THREE.Texture | null)?.dispose();
      renderer.dispose();
      if (renderer.domElement.parentNode === container) container.removeChild(renderer.domElement);
      threeRef.current = null;
    };
    // applyPreset is stable (useCallback with an empty dep array) and
    // onReady is read from a ref, not closed over -- this effect
    // intentionally runs once per mount only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Rebuild the board group whenever the board data or a show/hide
  // option changes, and re-home the camera's distance/lookAt (see
  // camera3d.ts's setBoardGeometry) from the board OUTLINE every time --
  // cheap, and keeps the camera's idea of "the board" anchored to this
  // app's actual source of truth (design.json's own outline) rather than
  // to whichever Three.js group happens to be visible (the old Box3-based
  // fit fit whatever was on screen, procedural or GLB, which could differ
  // slightly between the two). Only actually *moves* the live camera
  // (reset()) when the outline bounds changed meaningfully (or the first
  // time a usable outline appears at all) -- including on first mount,
  // since `lastFitBoundsRef` starts at null. Toggling a show/hide option
  // alone never changes `board.outline` (boardOutlineBounds only looks at
  // that), so it rebuilds geometry without moving the camera -- the same
  // way KiCad's own show/hide toggles don't reset your view.
  const { layers } = state.viewer3d;
  // Which of the Appearance manager's rows that gate the board's own geometry are shown, as one string: toggling a model row (T / S / V, bounding boxes) must not rebuild the board.
  const sceneRows = ROWS.filter((r) => !MODEL_ROWS.has(r.id)).map((r) => (isVisible(layers, r.id) ? "1" : "0")).join("");
  // The colour of every row: the theme's, the board's stackup when "Use board stackup colors" is on, the swatches (kicad-port/appearance3d.ts, BOARD_ADAPTER::GetLayerColors).
  const resolved = useViewerColors();
  useEffect(() => {
    const three = threeRef.current;
    if (!three) return;

    disposeObject3D(three.boardGroup);
    three.scene.remove(three.boardGroup);
    // The parts are not in this group: partModels draws them (the real models, a box until they are in).
    const nextGroup = board ? buildBoardGroup(board, { visible: (row) => isVisible(layers, row), colors: resolved }) : new THREE.Group();
    three.scene.add(nextGroup);
    three.boardGroup = nextGroup;
    syncActiveGroupRef.current();
    viewer3dProbe.update({ scene: countByName(nextGroup) });

    const bounds = board ? boardOutlineBounds(board) : null;
    const spanMm = bounds ? Math.max(bounds.maxX - bounds.minX, bounds.maxZ - bounds.minZ, 1e-3) : EMPTY_BOARD_SPAN_MM;
    const lookAt = bounds ? { x: (bounds.minX + bounds.maxX) / 2, y: 0, z: (bounds.minZ + bounds.maxZ) / 2 } : { x: 0, y: 0, z: 0 };
    three.camera3d.setBoardGeometry(spanMm, lookAt);

    const prev = lastFitBoundsRef.current;
    const EPS_MM = 0.05;
    const boundsChanged =
      (prev === null) !== (bounds === null) ||
      (prev !== null &&
        bounds !== null &&
        (Math.abs(prev.minX - bounds.minX) > EPS_MM || Math.abs(prev.minZ - bounds.minZ) > EPS_MM || Math.abs(prev.maxX - bounds.maxX) > EPS_MM || Math.abs(prev.maxZ - bounds.maxZ) > EPS_MM));
    lastFitBoundsRef.current = bounds;
    if (boundsChanged) three.camera3d.reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [board, sceneRows, resolved]);

  // The parts: the through-hole, SMD and virtual rows and the bounding boxes only touch their models.
  const thtShown = isVisible(layers, "th_models");
  const smdShown = isVisible(layers, "smd_models");
  const virtualShown = isVisible(layers, "virtual_models");
  const bboxShown = isVisible(layers, "bounding_boxes");
  useEffect(() => {
    threeRef.current?.partModels.sync(board, { showTHT: thtShown, showSMD: smdShown, showVirtual: virtualShown, showBoundingBoxes: bboxShown });
  }, [board, thtShown, smdShown, virtualShown, bboxShown]);

  // The background gradient (the Appearance manager's Background Start / End swatches).
  useEffect(() => {
    const three = threeRef.current;
    if (!three) return;
    (three.scene.background as THREE.Texture | null)?.dispose();
    three.scene.background = buildBackgroundTexture(resolved.background_top, resolved.background_bottom);
  }, [resolved]);

  // Flip/orthographic are camera-only now (KiCad's own flipView/
  // toggleOrtho actions move the camera, not the board -- eda_3d_actions.
  // cpp, camera.cpp's ToggleProjection/ViewCommand_T1(FLIP); the old
  // "rotate the board group 180 degrees" approximation this file used
  // before camera3d.ts existed is gone). `flipped` is a plain on/off
  // toggle in this app's store, but KiCad's flip is *additive* (pressing
  // F again un-flips only because it happens to add another 180 degrees)
  // -- `prevFlippedRef` makes sure `camera3d.flip()` fires exactly once
  // per real toggle, not once on every mount too.
  useEffect(() => {
    const three = threeRef.current;
    if (!three) return;
    if (state.viewer3d.flipped !== prevFlippedRef.current) {
      if (three.camera3d.animateFlip(performance.now())) prevFlippedRef.current = state.viewer3d.flipped;
      // A move is going, so the command is refused (`SetView3D` returns false): the switch goes back to what the camera is.
      else dispatch({ type: "SET_VIEWER3D_OPTIONS", options: { flipped: prevFlippedRef.current } });
    }
  }, [state.viewer3d.flipped]);
  useEffect(() => {
    const three = threeRef.current;
    if (!three) return;
    three.camera3d.setProjection(state.viewer3d.orthographic ? "ortho" : "perspective");
  }, [state.viewer3d.orthographic]);

  // Exactly one of boardGroup/glbGroup is visible at a time: the GLB
  // when the toggle wants it and one has actually loaded, the procedural
  // scene otherwise (toggle off, or still loading/failed -- see the
  // fetch effect below). A ref (not just inline logic where it's used)
  // because both the GLB-load effect and the toggle-change effect below
  // need to re-run the exact same decision.
  const syncActiveGroupRef = useRef<() => void>(() => {});
  syncActiveGroupRef.current = () => {
    const three = threeRef.current;
    if (!three) return;
    const showGlb = viewer3dRef.current.kicadModels && glbLoadedRef.current;
    three.glbGroup.visible = showGlb;
    three.boardGroup.visible = !showGlb;
    three.partModels.group.visible = !showGlb;
    viewer3dProbe.update({ source: showGlb ? "export" : "live" });
  };

  // GET /api/board.glb -- KiCad's own render of the current board (real
  // 3D models, kicad-cli's own colors/materials), fetched in the
  // background whenever the board actually changes (state.version, the
  // same change signal the rest of the app polls for) rather than on
  // every render. Loaded into its own group (glbGroup, created once at
  // mount) instead of replacing boardGroup outright, so a slow or failed
  // fetch never clears what's already on screen -- syncActiveGroupRef
  // decides which group is actually shown.
  //
  // The backend runs the (potentially multi-minute) kicad-cli export on
  // its own thread and never blocks on it (studio.rs's serve_board_glb),
  // answering 202 pending / 200 the GLB / 200 a failure -- see
  // BoardGlbResult in api/types.ts. This effect mirrors that: it polls
  // while pending, and stops outright (no retry loop) the moment it
  // sees either an actual result or a failure for the version it asked
  // about. Gated on `kicadModels` too, not just `state.version`: with
  // the toggle off there is no reason to ever start that export at all,
  // whether or not the 3D tab happens to be open (this whole component
  // only exists while it is, per App.tsx's `is3d &&` guard).
  useEffect(() => {
    const three = threeRef.current;
    if (!three) return;
    if (!state.viewer3d.kicadModels) {
      // Nothing to poll for and nothing in flight to report on -- match
      // GlbStatus's doc comment ("idle" = toggle off or never asked).
      dispatch({ type: "SET_GLB_STATUS", status: "idle" });
      return;
    }
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    dispatch({ type: "SET_GLB_STATUS", status: "pending" });

    const fail = (error: string) => {
      if (cancelled) return;
      glbLoadedRef.current = false;
      syncActiveGroupRef.current();
      dispatch({ type: "SET_GLB_STATUS", status: "failed", error });
    };

    const poll = (retry = false) => {
      fetchBoardGlb(retry)
        .then((result) => {
          if (cancelled) return;
          if (result.status === "pending") {
            timer = setTimeout(poll, GLB_POLL_MS);
            return;
          }
          if (result.status === "failed") {
            fail(result.error);
            return;
          }
          new GLTFLoader().parse(
            result.bytes,
            "",
            (gltf) => {
              if (cancelled) return;
              disposeObject3D(three.glbGroup);
              three.glbGroup.clear();
              gltf.scene.scale.setScalar(GLB_METERS_TO_MM);
              three.glbGroup.add(gltf.scene);
              glbLoadedRef.current = true;
              syncActiveGroupRef.current();
              dispatch({ type: "SET_GLB_STATUS", status: "loaded" });
            },
            (e) => fail(e instanceof ErrorEvent ? e.message : String(e))
          );
        })
        .catch((e) => fail(String(e)));
    };
    const retry = retryNextRef.current;
    retryNextRef.current = false;
    poll(retry);

    return () => {
      cancelled = true;
      if (timer !== null) clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.version, state.viewer3d.kicadModels, reloadNonce]);

  useEffect(() => {
    syncActiveGroupRef.current();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.viewer3d.kicadModels]);

  return (
    <div ref={containerRef} className="pcb-canvas-container">
      {!board && <div className="pcb-canvas-empty">{state.boardError ?? "Loading board…"}</div>}
      {state.viewer3d.kicadModels && state.glbStatus === "pending" && (
        <div style={GLB_STATUS_BADGE_STYLE}>Building KiCad's export…</div>
      )}
      {!(state.viewer3d.kicadModels && state.glbStatus === "loaded") && modelProgress && (
        <div style={GLB_STATUS_BADGE_STYLE}>Loading 3D models… {modelProgress.done} of {modelProgress.total}</div>
      )}
      {hoverText && <div style={HOVER_BADGE_STYLE}>{hoverText}</div>}
    </div>
  );
}
