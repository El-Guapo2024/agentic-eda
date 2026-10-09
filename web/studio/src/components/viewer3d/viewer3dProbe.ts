// What the 3D viewer is doing, for `window.__eda.state().viewer3d` (actions/useEdaTestHook.ts): the time to the first real model, how many models are in, which
// part is under the pointer, whether the camera is moving. The viewer writes it as it goes; nothing reads it but the test hook and the unit tests. No three.js and no
// React here (the hook and the tests import it).

export interface ModelCounts {
  requested: number;
  loading: number;
  ready: number;
  missing: number;
  failed: number;
  triangles: number;
}

export interface Viewer3dSnapshot {
  /** The 3D tab is open. */
  open: boolean;
  /** Milliseconds from the 3D tab opening to the first part drawn with a real model, or null while none is. */
  firstModelMs: number | null;
  /** ... to the moment every model the board asked for was settled (ready, missing or failed). */
  allModelsMs: number | null;
  models: ModelCounts;
  /** Placed parts: how many there are, how many are drawn with at least one real model, how many as a placeholder box. */
  parts: { total: number; asModels: number; asBoxes: number };
  /** The reference of the part under the pointer (it is drawn highlighted), or null. */
  hovered: string | null;
  /** The camera is animating towards a view (KiCad's `m_camera_is_moving`). */
  cameraMoving: boolean;
  /** Which source draws the board: the live scene with the models loaded here, or KiCad's own GLB export. */
  source: "live" | "export";
  /** Per model name: where it stands and what it cost, for the measurements. */
  detail: Array<{ name: string; status: string; fetchMs?: number; parseMs?: number; triangles?: number; error?: string }>;
}

const empty = (): Viewer3dSnapshot => ({
  open: false,
  firstModelMs: null,
  allModelsMs: null,
  models: { requested: 0, loading: 0, ready: 0, missing: 0, failed: 0, triangles: 0 },
  parts: { total: 0, asModels: 0, asBoxes: 0 },
  hovered: null,
  cameraMoving: false,
  source: "live",
  detail: [],
});

let state: Viewer3dSnapshot = empty();
let openedAt: number | null = null;
let screenOfPart: ((ref: string) => { x: number; y: number } | null) | null = null;

export const viewer3dProbe = {
  /** The 3D tab mounted: the clock for `firstModelMs` starts. */
  opened(now: number = performance.now()): void {
    state = { ...empty(), open: true };
    openedAt = now;
  },
  closed(): void {
    state = empty();
    openedAt = null;
  },
  /** What the viewer shows right now; stamps the two times the first time each is true. */
  update(patch: Partial<Omit<Viewer3dSnapshot, "firstModelMs" | "allModelsMs">>, now: number = performance.now()): void {
    state = { ...state, ...patch };
    if (openedAt === null) return;
    if (state.firstModelMs === null && state.parts.asModels > 0) state = { ...state, firstModelMs: Math.round(now - openedAt) };
    const m = state.models;
    if (state.allModelsMs === null && m.requested > 0 && m.loading === 0) state = { ...state, allModelsMs: Math.round(now - openedAt) };
  },
  /** The viewer registers where a part is on its canvas (CSS pixels from the canvas's top-left), so a script can hover it without guessing pixels. */
  setScreenOf(fn: ((ref: string) => { x: number; y: number } | null) | null): void {
    screenOfPart = fn;
  },
  screenOf(ref: string): { x: number; y: number } | null {
    return screenOfPart ? screenOfPart(ref) : null;
  },
  snapshot(): Viewer3dSnapshot {
    return { ...state, models: { ...state.models }, parts: { ...state.parts }, detail: state.detail.map((d) => ({ ...d })) };
  },
};
