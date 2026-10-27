// The preview of the 3D Models tab (`PANEL_PREVIEW_3D_MODEL`, 3d-viewer/dialogs/panel_preview_3d_model.cpp) and of the model chooser (`DIALOG_SELECT_3DMODEL`'s viewer): a small 3D view of
// one footprint -- its pads and drawings on a piece of board, "shown like in Fp editor: rot 0, not flipped" -- with the models of the list on it, placed the way KiCad places them
// (kicad-port/model3d.ts). It is the 3D view's own parts: the board is built by scene.ts's `buildBoardGroup` from a one-footprint board, the models are loaded one by one through
// components/viewer3d/modelCache.ts (`GET /api/3dmodel`, a STEP converted by kicad-cli once and cached), and the camera is the ported trackball (kicad-port/camera3d.ts).
//
// Only the models whose Show switch is on are drawn (`UpdateDummyFootprint` copies the shown ones to the dummy footprint). The buttons are the panel's: the views from the six
// sides, the orthographic switch, the board body switch (`preview_show_board_body`: off, the mask, paste and board body are hidden "to allows a good view of the 3D model and its
// pads") and reload ("Reload board and 3D models").
import { useCallback, useEffect, useRef, useState } from "react";
import * as THREE from "three";
import type { BoardState, Part } from "../../api/types";
import { FOV_DEG, TrackballCamera, type ViewPreset } from "../../kicad-port/camera3d";
import { isVisible } from "../../kicad-port/appearance3d";
import type { ModelRow } from "../../kicad-port/fpModels";
import { modelWorldMatrix, previewStack, previewWheel } from "../../kicad-port/model3d";
import { useStudioState } from "../../state/store";
import { ModelCache } from "../viewer3d/modelCache";
import { boardOutlineBounds, buildBackgroundTexture, buildBoardGroup, disposeObject3D } from "../viewer3d/scene";
import { useViewerColors } from "../viewer3d/useViewerColors";

/** The margin of board round the footprint, µm. */
const MARGIN_NM = 1_500_000;
/** The rows of the Appearance manager that `preview_show_board_body` turns off. */
const BODY_ROWS = new Set(["board", "soldermask_top", "soldermask_bottom", "solder_paste"]);

/** The box of the footprint's pads, courtyard and drawings, µm: where the piece of board goes. */
export function previewExtent(part: Part | null): [number, number, number, number] {
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  const add = (x: number, y: number) => {
    x0 = Math.min(x0, x);
    y0 = Math.min(y0, y);
    x1 = Math.max(x1, x);
    y1 = Math.max(y1, y);
  };
  for (const p of part?.pads ?? []) {
    add(p.x - p.w / 2, p.y - p.h / 2);
    add(p.x + p.w / 2, p.y + p.h / 2);
  }
  if (part?.courtyard) {
    add(part.courtyard[0], part.courtyard[1]);
    add(part.courtyard[2], part.courtyard[3]);
  }
  if (!Number.isFinite(x0)) return [-3_000_000, -3_000_000, 3_000_000, 3_000_000];
  return [x0, y0, x1, y1];
}

/** A board that is the one footprint at the origin on a margin of board: what `buildBoardGroup` draws. */
export function previewBoard(part: Part | null): BoardState {
  const [x0, y0, x1, y1] = previewExtent(part);
  const m = MARGIN_NM;
  const outline: [number, number][] = [
    [x0 - m, y0 - m],
    [x1 + m, y0 - m],
    [x1 + m, y1 + m],
    [x0 - m, y1 + m],
  ];
  return { name: "preview", dir: "", outline, layers: ["F.Cu", "B.Cu"], snap: 100_000, parts: part ? [part] : [], rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle" } as BoardState;
}

interface Scene3D {
  renderer: THREE.WebGLRenderer;
  scene: THREE.Scene;
  persp: THREE.PerspectiveCamera;
  ortho: THREE.OrthographicCamera;
  camera: TrackballCamera;
  board: THREE.Group;
  models: THREE.Group;
  cache: ModelCache;
}

export interface ModelPreview3DProps {
  /** The footprint to show under the models (`GET /api/3dmodel/preview`); null shows the models alone, on a bare piece of board. */
  part: Part | null;
  /** The list; the rows whose Show switch is off are not drawn. */
  rows: readonly ModelRow[];
  height?: number;
  testId?: string;
}

export function ModelPreview3D({ part, rows, height = 250, testId = "model-preview" }: ModelPreview3DProps) {
  const state = useStudioState();
  const colors = useViewerColors();
  const layers = state.viewer3d.layers;
  const containerRef = useRef<HTMLDivElement>(null);
  const three = useRef<Scene3D | null>(null);
  const rowsRef = useRef(rows);
  rowsRef.current = rows;
  const [bodyShown, setBodyShown] = useState(true);
  const [ortho, setOrtho] = useState(false);
  const [note, setNote] = useState("");
  /** What the view fitted to last: the camera moves when this changes, not when a field does. */
  const fitted = useRef("");
  // The wheel reads the Preferences' zoom modifier when a notch arrives (`PANEL_PREVIEW_3D_MODEL::loadSettings`), as the main viewer's does.
  const scrollPrefsRef = useRef(state.prefs);
  useEffect(() => {
    scrollPrefsRef.current = state.prefs;
  }, [state.prefs]);

  /** The models as the list says, loaded and placed; the camera is fitted when `refit` or what is drawn changed in kind. */
  const syncModels = useCallback((refit: boolean) => {
    const t = three.current;
    if (!t) return;
    disposeOwned(t.models);
    let drawn = 0;
    let waiting = 0;
    let missing = 0;
    for (const row of rowsRef.current) {
      if (!row.show || row.path.trim() === "") continue;
      const record = t.cache.request(row.path);
      if (record.status === "loading") waiting++;
      else if (record.status !== "ready" || !record.proto) missing++;
      if (record.status !== "ready" || !record.proto) continue;
      const instance = new THREE.Group();
      instance.name = "preview-model";
      instance.matrixAutoUpdate = false;
      instance.matrix.fromArray(modelWorldMatrix({ xMm: 0, yMm: 0, rotDeg: 0, bottom: false }, { offset: row.offset, scale: row.scale, rotate: row.rotate }, previewStack()));
      instance.matrixWorldNeedsUpdate = true;
      for (const child of record.proto.children) {
        const mesh = child.clone() as THREE.Mesh;
        if (row.opacity < 1) {
          // `(opacity v)`: this instance's own materials, a fraction as opaque as the model's.
          const faded = (mesh.material as THREE.Material).clone();
          faded.transparent = true;
          faded.opacity = (mesh.material as THREE.Material).opacity * row.opacity;
          faded.userData.owned = true;
          mesh.material = faded;
        }
        instance.add(mesh);
      }
      t.models.add(instance);
      drawn++;
    }
    const el = containerRef.current;
    if (el) {
      el.dataset.drawn = String(drawn);
      el.dataset.waiting = String(waiting);
      el.dataset.missing = String(missing);
    }
    setNote(waiting > 0 ? `Loading ${waiting} model${waiting === 1 ? "" : "s"}...` : missing > 0 ? `${missing} model${missing === 1 ? "" : "s"} not shown: not found or not readable` : "");
    // the camera: fitted to the board and what the models cover, when the footprint or the set of files changed -- not when a field is typed in
    const key = `${part?.ref ?? ""}|${part?.footprint ?? ""}|${rowsRef.current.filter((r) => r.show).map((r) => r.path).join(";")}|${drawn}`;
    if (refit || fitted.current !== key) {
      fitted.current = key;
      fitCamera(t, part);
    }
  }, [part]);
  // The cache tells the latest `syncModels`, not the one the scene was made with.
  const syncRef = useRef(syncModels);
  syncRef.current = syncModels;

  // Mount: the renderer, the scene, the lights, the camera, the loop.
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const scene = new THREE.Scene();
    scene.background = buildBackgroundTexture();
    const camera = new TrackballCamera(60, { x: 0, y: 0, z: 0 });
    const persp = new THREE.PerspectiveCamera(FOV_DEG, 1, 0.1, 1000);
    const ortho = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 1000);
    const renderer = new THREE.WebGLRenderer({ antialias: true });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.domElement.style.touchAction = "none";
    renderer.domElement.style.outline = "none";
    renderer.domElement.style.display = "block";
    const box = container.getBoundingClientRect();
    if (box.width > 0 && box.height > 0) {
      renderer.setSize(box.width, box.height);
      persp.aspect = box.width / box.height;
      persp.updateProjectionMatrix();
      camera.setWindowSize(box.width, box.height);
    }
    container.appendChild(renderer.domElement);
    // The 3D view's lights (render_3d_opengl.cpp `init_lights`): two fixed directional ones skimming the board and a headlight that follows the camera.
    scene.add(new THREE.AmbientLight(0xffffff, 0.25));
    const top = new THREE.DirectionalLight(0xffffff, 1.0);
    top.position.set(0.0665, 0.9956, -0.0665).multiplyScalar(1000);
    scene.add(top);
    const bottom = new THREE.DirectionalLight(0xffffff, 1.0);
    bottom.position.set(0.0665, 0.9956, -0.0665).multiplyScalar(-1000);
    scene.add(bottom);
    const headlight = new THREE.PointLight(0xffffff, 0.45, 0, 0);
    scene.add(headlight);
    const board = new THREE.Group();
    const models = new THREE.Group();
    scene.add(board, models);
    const cache = new ModelCache();
    three.current = { renderer, scene, persp, ortho, camera, board, models, cache };
    const stop = cache.onChange(() => syncRef.current(false));

    // Input (HIDPI_GL_3D_CANVAS): left drag turns the view, middle and right drag pan it, the wheel zooms.
    const el = renderer.domElement;
    let drag: "left" | "pan" | null = null;
    const onDown = (e: PointerEvent) => {
      camera.setCurMousePosition(e.offsetX, e.offsetY);
      drag = e.button === 0 ? "left" : e.button === 1 || e.button === 2 ? "pan" : null;
      if (drag) el.setPointerCapture(e.pointerId);
    };
    const onMove = (e: PointerEvent) => {
      if (camera.isMoving()) {
        camera.setCurMousePosition(e.offsetX, e.offsetY);
        return;
      }
      if (drag === "left") camera.drag(e.offsetX, e.offsetY);
      else if (drag === "pan") camera.pan(e.offsetX, e.offsetY);
      camera.setCurMousePosition(e.offsetX, e.offsetY);
    };
    const onUp = (e: PointerEvent) => {
      drag = null;
      if (el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
    };
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      if (!camera.isMoving()) {
        const { scroll, aPan } = previewWheel(scrollPrefsRef.current);
        camera.handleWheel({ deltaX: e.deltaX, deltaY: e.deltaY, shiftKey: e.shiftKey, ctrlKey: e.ctrlKey || e.metaKey, altKey: e.altKey }, scroll, aPan);
      }
    };
    const onContext = (e: MouseEvent) => e.preventDefault();
    el.addEventListener("pointerdown", onDown);
    el.addEventListener("pointermove", onMove);
    el.addEventListener("pointerup", onUp);
    el.addEventListener("pointercancel", onUp);
    el.addEventListener("wheel", onWheel, { passive: false });
    el.addEventListener("contextmenu", onContext);

    const ro = new ResizeObserver((entries) => {
      const b = entries[0]?.contentRect;
      if (!b || b.width <= 0 || b.height <= 0) return;
      renderer.setSize(b.width, b.height);
      persp.aspect = b.width / b.height;
      persp.updateProjectionMatrix();
      camera.setWindowSize(b.width, b.height);
    });
    ro.observe(container);

    let raf = 0;
    const animate = () => {
      camera.tick(performance.now());
      const pose = camera.getRenderPose();
      const proj = camera.getProjectionParams();
      headlight.position.set(pose.position.x, pose.position.y, pose.position.z);
      let active: THREE.Camera;
      if (proj.kind === "perspective") {
        persp.fov = proj.fovDeg;
        persp.near = proj.near;
        persp.far = proj.far;
        persp.updateProjectionMatrix();
        active = persp;
      } else {
        ortho.left = proj.left;
        ortho.right = proj.right;
        ortho.top = proj.top;
        ortho.bottom = proj.bottom;
        ortho.near = proj.near;
        ortho.far = proj.far;
        ortho.updateProjectionMatrix();
        active = ortho;
      }
      active.position.set(pose.position.x, pose.position.y, pose.position.z);
      active.quaternion.set(pose.quaternion.x, pose.quaternion.y, pose.quaternion.z, pose.quaternion.w);
      renderer.render(scene, active);
      raf = requestAnimationFrame(animate);
    };
    raf = requestAnimationFrame(animate);

    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      stop();
      el.removeEventListener("pointerdown", onDown);
      el.removeEventListener("pointermove", onMove);
      el.removeEventListener("pointerup", onUp);
      el.removeEventListener("pointercancel", onUp);
      el.removeEventListener("wheel", onWheel);
      el.removeEventListener("contextmenu", onContext);
      disposeObject3D(board);
      disposeOwned(models);
      cache.dispose();
      (scene.background as THREE.Texture | null)?.dispose();
      renderer.dispose();
      renderer.forceContextLoss();
      if (el.parentNode === container) container.removeChild(el);
      three.current = null;
    };
    // the scene is made once per mount; the models are read from a ref
  }, []);

  // The footprint and the piece of board under it.
  const colorKey = JSON.stringify(colors);
  useEffect(() => {
    const t = three.current;
    if (!t) return;
    disposeObject3D(t.board);
    t.scene.remove(t.board);
    const visible = (row: string) => isVisible(layers, row) && (bodyShown || !BODY_ROWS.has(row));
    // The preview's dummy board and its default stackup (previewStack), for the board and the models alike.
    t.board = buildBoardGroup(previewBoard(part), { visible, colors, stack: previewStack() });
    t.scene.add(t.board);
    syncModels(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [part, bodyShown, layers, colorKey, syncModels]);

  // The models: whenever the list changes.
  const listKey = JSON.stringify(rows);
  useEffect(() => {
    syncModels(false);
  }, [listKey, syncModels]);

  useEffect(() => {
    three.current?.camera.setProjection(ortho ? "ortho" : "perspective");
  }, [ortho]);

  const view = (preset: ViewPreset) => three.current?.camera.animateViewPreset(preset, performance.now());
  const reset = () => three.current?.camera.animateReset(performance.now());
  const reload = () => {
    three.current?.cache.clear();
    syncModels(true);
  };

  const button = { padding: "1px 6px", fontSize: 11 } as const;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 0 }} data-testid={`${testId}-panel`}>
      <div style={{ display: "flex", gap: 3, flexWrap: "wrap", alignItems: "center", fontSize: 11 }}>
        <span style={{ color: "var(--chrome-text-dim)", marginRight: 4 }}>Preview</span>
        {(["top", "bottom", "front", "back", "left", "right"] as const).map((p) => (
          <button key={p} style={button} title={`View ${p}`} onClick={() => view(p)} data-testid={`${testId}-view-${p}`}>
            {p[0]!.toUpperCase() + p.slice(1)}
          </button>
        ))}
        <button style={button} title="Fit the view" onClick={reset} data-testid={`${testId}-fit`}>
          Fit
        </button>
        <label title="Enable/disable orthographic projection" style={{ marginLeft: 4 }}>
          <input type="checkbox" checked={ortho} onChange={(e) => setOrtho(e.target.checked)} data-testid={`${testId}-ortho`} /> Ortho
        </label>
        <label title={"Show or hide the board body\nIf hidden, show only copper and silkscreen layers."}>
          <input type="checkbox" checked={bodyShown} onChange={(e) => setBodyShown(e.target.checked)} data-testid={`${testId}-body`} /> Board
        </label>
        <button style={button} title="Reload board and 3D models" onClick={reload} data-testid={`${testId}-reload`}>
          Reload
        </button>
      </div>
      <div style={{ position: "relative", height, border: "1px solid var(--chrome-border, #444)", background: "#333" }}>
        <div ref={containerRef} style={{ position: "absolute", inset: 0 }} data-testid={testId} />
        {note && (
          <div style={{ position: "absolute", left: 6, bottom: 4, fontSize: 11, color: "var(--chrome-text-dim)", pointerEvents: "none" }} data-testid={`${testId}-note`}>
            {note}
          </div>
        )}
      </div>
    </div>
  );
}

/** Takes the models out of the group, disposing what the instances own (the faded materials); the geometry and the materials of the models are the cache's. */
function disposeOwned(group: THREE.Group): void {
  for (const child of [...group.children]) {
    child.traverse((o) => {
      if (o instanceof THREE.Mesh) {
        const material = o.material as THREE.Material;
        if (material.userData.owned) material.dispose();
      }
    });
    group.remove(child);
  }
}

/** The camera home and the distance: the piece of board and the models on it. */
function fitCamera(t: Scene3D, part: Part | null): void {
  const [x0, y0, x1, y1] = previewExtent(part);
  const box = new THREE.Box3(new THREE.Vector3((x0 - MARGIN_NM) / 1e6, -2, (y0 - MARGIN_NM) / 1e6), new THREE.Vector3((x1 + MARGIN_NM) / 1e6, 2, (y1 + MARGIN_NM) / 1e6));
  t.models.updateMatrixWorld(true);
  const covered = new THREE.Box3().setFromObject(t.models);
  if (!covered.isEmpty()) box.union(covered);
  const size = box.getSize(new THREE.Vector3());
  const centre = box.getCenter(new THREE.Vector3());
  const bounds = boardOutlineBounds(previewBoard(part));
  const span = Math.max(size.x, size.y, size.z, bounds ? Math.max(bounds.maxX - bounds.minX, bounds.maxZ - bounds.minZ) : 0, 4);
  t.camera.setBoardGeometry(span, { x: centre.x, y: 0, z: centre.z });
  t.camera.reset();
}
