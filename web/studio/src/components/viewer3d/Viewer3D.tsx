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
import { useStudioState } from "../../state/store";
import { buildBoardGroup, disposeObject3D, presetCameraPose, boardOutlineBounds, type ViewPreset, type OutlineBounds } from "./scene";

const CAMERA_FOV_DEG = 50;

interface ThreeContext {
  renderer: THREE.WebGLRenderer;
  scene: THREE.Scene;
  camera: THREE.PerspectiveCamera;
  controls: OrbitControls;
  boardGroup: THREE.Group;
}

/** The imperative handle Viewer3D hands up via `onReady`, for Viewer3DToolbar (or anything else) to drive the camera without owning it. */
export interface Viewer3DApi {
  /** Moves the camera to one of the 8 KiCad-style view presets, framing the whole board. */
  setView(preset: ViewPreset): void;
}

export function Viewer3D({ onReady }: { onReady?: (api: Viewer3DApi | null) => void }) {
  const state = useStudioState();
  const board = state.board;

  const containerRef = useRef<HTMLDivElement>(null);
  const threeRef = useRef<ThreeContext | null>(null);
  const lastFitBoundsRef = useRef<OutlineBounds | null>(null);
  // `onReady` kept in a ref and read only inside effects, not listed as
  // an effect dependency, so an unstable inline callback from a caller
  // can never tear down and recreate the whole WebGL context on every
  // render -- only mount/unmount and board changes should do that.
  const onReadyRef = useRef(onReady);
  useEffect(() => {
    onReadyRef.current = onReady;
  }, [onReady]);

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
    const box = new THREE.Box3().setFromObject(three.boardGroup);
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
    scene.background = new THREE.Color(0x1e1e1f);

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

    threeRef.current = { renderer, scene, camera, controls, boardGroup };
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
      renderer.dispose();
      if (renderer.domElement.parentNode === container) container.removeChild(renderer.domElement);
      threeRef.current = null;
    };
    // applyPreset is stable (useCallback with an empty dep array, see
    // below) and onReady is read from a ref, not closed over -- this
    // effect intentionally runs once per mount only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Rebuild the board group whenever the board data changes, and
  // re-fit the camera whenever the outline bounds change meaningfully
  // (or the first time a usable outline appears at all) -- including on
  // first mount, since `lastFitBoundsRef` starts at null.
  useEffect(() => {
    const three = threeRef.current;
    if (!three) return;

    disposeObject3D(three.boardGroup);
    three.scene.remove(three.boardGroup);
    const nextGroup = board ? buildBoardGroup(board) : new THREE.Group();
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
  }, [board, applyPreset]);

  return (
    <div ref={containerRef} className="pcb-canvas-container">
      {!board && <div className="pcb-canvas-empty">{state.boardError ?? "Loading board…"}</div>}
    </div>
  );
}
