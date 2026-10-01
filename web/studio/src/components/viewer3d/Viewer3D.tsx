// The 3D PCB viewer tab -- a KiCad-style "3D Viewer" for the board this
// studio is editing. Self-contained: owns its own Three.js
// renderer/scene/camera/OrbitControls and render loop, reading board
// data straight from the store. See App.tsx's `state.tab === "3d"`
// slot in canvas-col, which mounts this component full-bleed the same
// way Canvas/SchematicView are mounted there.
//
// Viewer3DToolbar (the 8 view-preset buttons) is mounted *separately* by
// App.tsx, in the main-toolbar-row (replacing the normal <Toolbar
// id="main"/> for this tab -- the 3D tab has no real icon-toolbar
// action set of its own). The two talk over a small imperative handle
// (`Viewer3DApi`) rather than a shared prop/context: this component
// creates the handle once its camera/controls exist and hands it up via
// `onReady`, App.tsx holds it in state, and passes it down to
// Viewer3DToolbar as `api`.
//
// Geometry building lives in scene.ts (pure, no React/DOM); this file
// is orchestration only: container/resize lifecycle (same pattern as
// components/canvas/Canvas.tsx), the Three.js object lifecycle, the
// render loop, and the view-preset handle.
import { useCallback, useEffect, useRef } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { fetchBoardGlb } from "../../api/client";
import { useStudioDispatch, useStudioState } from "../../state/store";
import { buildBoardGroup, buildBackgroundTexture, disposeObject3D, presetCameraPose, boardOutlineBounds, type ViewPreset, type OutlineBounds } from "./scene";

const CAMERA_FOV_DEG = 50;
/** "Orthographic" toggle: this app has no separate OrthographicCamera wired up (Viewer3DApi/applyPreset math is all perspective-FOV-based) -- narrowing the FOV this far while presetCameraPose pulls the camera back to compensate is a well-known way to approximate an orthographic look with a plain PerspectiveCamera, not a real projection swap. */
const ORTHO_FOV_DEG = 4;
/** glTF's own unit is meters; every other builder in this file (scene.ts) works in millimetres, so the loaded GLB scene is scaled up to match rather than rescaling everything else down to meters. */
const GLB_METERS_TO_MM = 1000;
/** How often to re-poll GET /api/board.glb while the backend reports `{"status":"pending"}`. The export itself can take minutes (see studio.rs's build_glb), so this is deliberately slower than the app's main 700ms state poll (store.tsx) -- there is no reason to hammer the endpoint every tick for something this slow. */
const GLB_POLL_MS = 1500;

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

interface ThreeContext {
  renderer: THREE.WebGLRenderer;
  scene: THREE.Scene;
  camera: THREE.PerspectiveCamera;
  controls: OrbitControls;
  boardGroup: THREE.Group;
  /** GET /api/board.glb's real KiCad-rendered board, loaded into its own group so it can be shown/hidden independently of the procedural boardGroup rather than swapped in and out of the scene (cheaper, and keeps whichever one is hidden ready to reappear instantly). Empty until a GLB successfully loads. */
  glbGroup: THREE.Group;
}

/** The imperative handle Viewer3D hands up via `onReady`, for Viewer3DToolbar (or anything else) to drive the camera without owning it. */
export interface Viewer3DApi {
  /** Moves the camera to one of the 8 KiCad-style view presets, framing the whole board. */
  setView(preset: ViewPreset): void;
}

export function Viewer3D({ onReady }: { onReady?: (api: Viewer3DApi | null) => void }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const board = state.board;

  const containerRef = useRef<HTMLDivElement>(null);
  const threeRef = useRef<ThreeContext | null>(null);
  const lastFitBoundsRef = useRef<OutlineBounds | null>(null);
  const lastPresetRef = useRef<ViewPreset>("iso");
  /** Whether glbGroup currently holds a successfully loaded board (as opposed to being empty -- fetch not finished yet, or failed). Read by the visibility-sync effect and by applyPreset (to frame whichever group is actually showing) without needing it in React state. */
  const glbLoadedRef = useRef(false);
  // `onReady` kept in a ref and read only inside effects, not listed as
  // an effect dependency, so an unstable inline callback from a caller
  // can never tear down and recreate the whole WebGL context on every
  // render -- only mount/unmount and board changes should do that.
  const onReadyRef = useRef(onReady);
  useEffect(() => {
    onReadyRef.current = onReady;
  }, [onReady]);
  // Same ref-not-dependency reasoning for the view-option toggles: read
  // fresh inside applyPreset (stable, empty deps) rather than closed over.
  const viewer3dRef = useRef(state.viewer3d);
  useEffect(() => {
    viewer3dRef.current = state.viewer3d;
  }, [state.viewer3d]);

  /**
   * Moves the camera to `preset`, framing whatever is currently in the
   * board group. Shared by the initial/on-change auto-fit below and by
   * every Viewer3DToolbar button -- "Reset" and "Iso" both just call
   * this with their own preset name (see scene.ts's PRESET_DIRECTIONS
   * comment for why they're identical).
   *
   * Reads `threeRef.current` fresh on every call rather than closing
   * over anything from render scope, so it stays referentially stable
   * (empty dep array) while always acting on the live camera/controls/
   * board group.
   */
  const applyPreset = useCallback((preset: ViewPreset) => {
    const three = threeRef.current;
    if (!three) return;
    lastPresetRef.current = preset;
    three.camera.fov = viewer3dRef.current.orthographic ? ORTHO_FOV_DEG : CAMERA_FOV_DEG;
    // Frame whichever group is actually visible -- an empty/hidden
    // boardGroup while the GLB is showing would fit an empty box (no-op)
    // instead of the board actually on screen.
    const activeGroup = viewer3dRef.current.kicadModels && glbLoadedRef.current ? three.glbGroup : three.boardGroup;
    const box = new THREE.Box3().setFromObject(activeGroup);
    const { position, target } = presetCameraPose(box, preset, three.camera.fov, three.camera.aspect);
    three.camera.position.copy(position);
    three.controls.target.copy(target);
    three.camera.updateProjectionMatrix();
    three.controls.update();
  }, []);

  // Mount: create the renderer/scene/camera/controls/lights once and
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

    const camera = new THREE.PerspectiveCamera(CAMERA_FOV_DEG, 1, 0.1, 10000);
    camera.position.set(80, 80, 80);

    const renderer = new THREE.WebGLRenderer({ antialias: true });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    const rect = container.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) {
      renderer.setSize(rect.width, rect.height);
      camera.aspect = rect.width / rect.height;
      camera.updateProjectionMatrix();
    }
    container.appendChild(renderer.domElement);

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    controls.dampingFactor = 0.08;

    scene.add(new THREE.AmbientLight(0xffffff, 0.6));
    const keyLight = new THREE.DirectionalLight(0xffffff, 0.9);
    keyLight.position.set(1, 2, 1.5);
    scene.add(keyLight);
    const fillLight = new THREE.DirectionalLight(0xffffff, 0.35);
    fillLight.position.set(-1.5, -0.6, -1);
    scene.add(fillLight);

    const boardGroup = new THREE.Group();
    scene.add(boardGroup);
    const glbGroup = new THREE.Group();
    glbGroup.name = "viewer3d-glb";
    glbGroup.visible = false;
    scene.add(glbGroup);

    threeRef.current = { renderer, scene, camera, controls, boardGroup, glbGroup };
    onReadyRef.current?.({ setView: applyPreset });

    const ro = new ResizeObserver((entries) => {
      const box = entries[0]?.contentRect;
      if (!box || box.width <= 0 || box.height <= 0) return;
      renderer.setSize(box.width, box.height);
      camera.aspect = box.width / box.height;
      camera.updateProjectionMatrix();
    });
    ro.observe(container);

    let raf = 0;
    const animate = () => {
      controls.update();
      renderer.render(scene, camera);
      raf = requestAnimationFrame(animate);
    };
    raf = requestAnimationFrame(animate);

    return () => {
      onReadyRef.current?.(null);
      cancelAnimationFrame(raf);
      ro.disconnect();
      controls.dispose();
      disposeObject3D(boardGroup);
      scene.remove(boardGroup);
      disposeObject3D(glbGroup);
      scene.remove(glbGroup);
      (scene.background as THREE.Texture | null)?.dispose();
      renderer.dispose();
      if (renderer.domElement.parentNode === container) container.removeChild(renderer.domElement);
      threeRef.current = null;
    };
    // applyPreset is stable (useCallback with an empty dep array, see
    // below) and onReady is read from a ref, not closed over -- this
    // effect intentionally runs once per mount only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Rebuild the board group whenever the board data or a show/hide
  // option changes, and re-fit the camera whenever the outline bounds
  // change meaningfully (or the first time a usable outline appears at
  // all) -- including on first mount, since `lastFitBoundsRef` starts at
  // null. Toggling a show/hide option alone never changes `board.outline`
  // (boardOutlineBounds only looks at that), so it rebuilds geometry
  // without moving the camera -- the same way KiCad's own show/hide
  // toggles don't reset your view.
  const { showComponents, showSilkscreen, showSolderMask } = state.viewer3d;
  useEffect(() => {
    const three = threeRef.current;
    if (!three) return;

    disposeObject3D(three.boardGroup);
    three.scene.remove(three.boardGroup);
    const nextGroup = board ? buildBoardGroup(board, { showComponents, showSilkscreen, showSolderMask }) : new THREE.Group();
    nextGroup.rotation.x = viewer3dRef.current.flipped ? Math.PI : 0;
    three.scene.add(nextGroup);
    three.boardGroup = nextGroup;

    const bounds = board ? boardOutlineBounds(board) : null;
    const prev = lastFitBoundsRef.current;
    const EPS_MM = 0.05;
    const boundsChanged =
      (prev === null) !== (bounds === null) ||
      (prev !== null &&
        bounds !== null &&
        (Math.abs(prev.minX - bounds.minX) > EPS_MM || Math.abs(prev.minZ - bounds.minZ) > EPS_MM || Math.abs(prev.maxX - bounds.maxX) > EPS_MM || Math.abs(prev.maxZ - bounds.maxZ) > EPS_MM));
    lastFitBoundsRef.current = bounds;
    if (boundsChanged) applyPreset("reset");
  }, [board, showComponents, showSilkscreen, showSolderMask, applyPreset]);

  // Flip/orthographic are camera-or-orientation-only -- no geometry
  // rebuild needed, just re-pose what's already there. Flip spins the
  // *board group itself* 180 degrees about the in-plane X axis (like
  // turning a page over a horizontal hinge: left/right stays put, top
  // and front/back swap), so the bottom side reads right-way-up rather
  // than mirrored -- not a new camera preset; the re-fit afterward is
  // needed because rotating the group changes its world bounding box,
  // which would otherwise leave the framing from before the flip stale.
  useEffect(() => {
    const three = threeRef.current;
    if (!three) return;
    const rot = state.viewer3d.flipped ? Math.PI : 0;
    three.boardGroup.rotation.x = rot;
    three.glbGroup.rotation.x = rot;
    applyPreset(lastPresetRef.current);
    // Deliberately only on the flipped flag -- see the orthographic
    // effect below for why lastPresetRef/applyPreset aren't dependencies.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.viewer3d.flipped]);
  useEffect(() => {
    if (!threeRef.current) return;
    applyPreset(lastPresetRef.current);
    // Deliberately only on the orthographic flag: re-applying
    // lastPresetRef's own preset must not itself become a dependency (it's
    // a ref, and applyPreset already reads viewer3dRef fresh).
    // eslint-disable-next-line react-hooks/exhaustive-deps
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

    const poll = () => {
      fetchBoardGlb()
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
              applyPreset(lastPresetRef.current);
              dispatch({ type: "SET_GLB_STATUS", status: "loaded" });
            },
            (e) => fail(e instanceof ErrorEvent ? e.message : String(e))
          );
        })
        .catch((e) => fail(String(e)));
    };
    poll();

    return () => {
      cancelled = true;
      if (timer !== null) clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.version, state.viewer3d.kicadModels]);

  useEffect(() => {
    syncActiveGroupRef.current();
    applyPreset(lastPresetRef.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.viewer3d.kicadModels]);

  return (
    <div ref={containerRef} className="pcb-canvas-container">
      {!board && <div className="pcb-canvas-empty">{state.boardError ?? "Loading board…"}</div>}
      {state.viewer3d.kicadModels && state.glbStatus === "pending" && (
        <div style={GLB_STATUS_BADGE_STYLE}>Loading KiCad models…</div>
      )}
    </div>
  );
}
