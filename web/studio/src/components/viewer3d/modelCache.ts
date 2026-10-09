// The 3D models of the board's parts, loaded one by one: `GET /api/3dmodel?name=<the footprint's (model ...) path>` (crates/cli/src/model3d_api.rs) answers with the
// model as VRML -- KiCad's own library format, which the server writes from a STEP once and keeps -- and three.js's own `VRMLLoader` reads it. No full-board export
// is waited for: the first model of a board is on screen in a couple of seconds, and a model that was converted before is there as soon as it is parsed.
//
// A VRML model is KiCad's: units of 0.1 inch, z up from the footprint's mounting surface, one `Shape` per colour patch. Each is turned into one object, once:
// scaled to millimetres, its shapes' transforms baked in, its patches that share a material merged into one mesh (a 0603 resistor is 26 shapes of 2 materials), the
// materials set up the way KiCad lights them (`OglSetMaterial`'s shininess, `128 * min(shininess, 1)`). Every part that uses the model gets a clone that shares that
// geometry and those materials (partModels.ts) -- 30 parts of 5 packages are 5 loads and 5 sets of buffers.
import * as THREE from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import { VRML_UNIT_MM, modelUrl } from "../../kicad-port/model3d";

export type ModelStatus = "loading" | "ready" | "missing" | "failed";

export interface ModelRecord {
  name: string;
  status: ModelStatus;
  /** Why it is missing or failed. */
  error?: string;
  /** The model in millimetres, KiCad's own frame (z up from the footprint). Shared by every instance: never modified, disposed with the cache. */
  proto?: THREE.Group;
  /** The model's bounding box, millimetres, in the same frame. */
  bbox?: THREE.Box3;
  triangles?: number;
  /** Milliseconds from the request to the file (the server converting it counts), and the time `VRMLLoader` took on it. */
  fetchMs?: number;
  parseMs?: number;
  /** `performance.now()` when it became ready. */
  readyAt?: number;
}

/** What the page can see of the cache, for the test hook (`window.__eda.state().viewer3d`). */
export interface CacheStats {
  requested: number;
  loading: number;
  ready: number;
  missing: number;
  failed: number;
  triangles: number;
}

/** A model of 50 MB of VRML would take `VRMLLoader` (a chevrotain parser) the better part of a minute on the main thread: it is left as a placeholder box. */
const MAX_VRML_BYTES = 24 * 1024 * 1024;

/** How soon to ask again when the server answered "pending" at once instead of holding the request (an older server): the page's own polling. */
const POLL_MS = 250;

/**
 * Requests held open at once. The server holds a request for a model until it is converted (`&wait=1`), and the browser keeps six connections to one server: the
 * studio's own polls and edits share them, so models take three, and the rest wait their turn. The server converts every model of the board in one run whatever
 * the page's pace is: `prepare` (below) queues them all first.
 */
const MAX_OUTSTANDING = 3;

/** `MeshPhongMaterial`'s own default shininess: what a VRML material that gives none keeps (`VRMLLoader` only overrides it when the file says). */
const THREE_DEFAULT_SHININESS = 30;

/** KiCad's lighting of a VRML material (`OglSetMaterial`): `shininess = 128 * min(shininess, 1)`, VRML's own default (0.2) when the file gave none. */
function kicadShininess(material: THREE.MeshPhongMaterial): number {
  // A VRML shininess is in [0, 1]; three's default is an exponent.
  const s = material.shininess === THREE_DEFAULT_SHININESS ? 0.2 : material.shininess;
  return 128 * Math.min(s, 1);
}

/**
 * VRML scene -> one object in millimetres: shapes baked into world space, scaled by `VRML_UNIT_MM`, merged per material. Exported for the tests of the loader's
 * shape handling that need no network.
 */
export function buildModelObject(scene: THREE.Object3D): { proto: THREE.Group; bbox: THREE.Box3; triangles: number } {
  scene.updateMatrixWorld(true);
  const units = new THREE.Matrix4().makeScale(VRML_UNIT_MM, VRML_UNIT_MM, VRML_UNIT_MM);
  const byMaterial = new Map<THREE.Material, THREE.BufferGeometry[]>();
  scene.traverse((obj) => {
    if (!(obj instanceof THREE.Mesh) || !obj.geometry) return;
    const material = Array.isArray(obj.material) ? obj.material[0] : obj.material;
    if (!material) return;
    let g = obj.geometry as THREE.BufferGeometry;
    if (!g.attributes.position) return;
    g = g.clone();
    if (g.index) g = g.toNonIndexed();
    g.applyMatrix4(units.clone().multiply(obj.matrixWorld));
    // Merging needs every geometry to carry the same attributes: positions and normals only (a KiCad model has no textures or vertex colours worth keeping).
    for (const name of Object.keys(g.attributes)) if (name !== "position" && name !== "normal") g.deleteAttribute(name);
    if (!g.attributes.normal) g.computeVertexNormals();
    const list = byMaterial.get(material) ?? [];
    list.push(g);
    byMaterial.set(material, list);
  });

  const proto = new THREE.Group();
  let triangles = 0;
  for (const [material, geoms] of byMaterial) {
    const merged = geoms.length === 1 ? geoms[0]! : mergeGeometries(geoms, false);
    if (!merged) continue;
    if (geoms.length > 1) for (const g of geoms) g.dispose();
    if (material instanceof THREE.MeshPhongMaterial) {
      material.shininess = kicadShininess(material);
      // A model drawn from inside (a hollow with its normals turned the other way) still shows: KiCad draws both faces of every material.
      material.side = THREE.DoubleSide;
    }
    triangles += merged.attributes.position!.count / 3;
    proto.add(new THREE.Mesh(merged, material));
  }
  const bbox = new THREE.Box3().setFromObject(proto);
  return { proto, bbox, triangles };
}

/** `VRMLLoader` (and the chevrotain parser it carries, 150 kB) is its own chunk: the studio's other tabs never load it. */
let vrmlLoader: Promise<typeof import("three/examples/jsm/loaders/VRMLLoader.js")> | null = null;
function loadVrmlLoader() {
  vrmlLoader ??= import("three/examples/jsm/loaders/VRMLLoader.js");
  return vrmlLoader;
}

/** `VRMLLoader` on the text of a `.wrl`: the model object, or throws. */
export async function parseModel(text: string): Promise<{ proto: THREE.Group; bbox: THREE.Box3; triangles: number }> {
  const { VRMLLoader } = await loadVrmlLoader();
  const scene = new VRMLLoader().parse(text, "");
  const built = buildModelObject(scene);
  // The loader's own geometries were cloned into the merged ones: the scene's are not wanted any more.
  scene.traverse((o) => {
    if (o instanceof THREE.Mesh) o.geometry?.dispose();
  });
  if (built.proto.children.length === 0) throw new Error("the VRML has no geometry");
  return built;
}

export class ModelCache {
  private readonly records = new Map<string, ModelRecord>();
  private readonly listeners = new Set<() => void>();
  private disposed = false;
  private readonly timers = new Set<ReturnType<typeof setTimeout>>();
  /** Records asked for this tick, to be queued on the server together (`prepare`) before the first is requested. */
  private fresh: Array<{ rec: ModelRecord; retry: boolean }> = [];
  private flushing = false;
  /** Records ready to be requested, in order, and how many requests are open. */
  private line: Array<{ rec: ModelRecord; retry: boolean }> = [];
  private outstanding = 0;

  constructor(
    private readonly fetchFn: typeof fetch = (...a) => fetch(...a),
    private readonly now: () => number = () => performance.now()
  ) {
    // The parser's chunk loads while the first model is being fetched (and converted) -- not after.
    void loadVrmlLoader().catch(() => {});
  }

  get(name: string): ModelRecord | undefined {
    return this.records.get(name);
  }

  /** The record of `name`, starting its load when nothing asked for it yet. */
  request(name: string): ModelRecord {
    let rec = this.records.get(name);
    if (!rec) {
      rec = { name, status: "loading" };
      this.records.set(name, rec);
      this.enqueue(rec, false);
    }
    return rec;
  }

  /** Models asked for in the same tick are told to the server in one `prepare`, then requested three at a time. */
  private enqueue(rec: ModelRecord, retry: boolean): void {
    this.fresh.push({ rec, retry });
    if (this.flushing) return;
    this.flushing = true;
    queueMicrotask(() => void this.flush());
  }

  private async flush(): Promise<void> {
    const batch = this.fresh;
    this.fresh = [];
    this.flushing = false;
    if (this.disposed || batch.length === 0) return;
    try {
      // The server converts what it was told about in one kicad-cli run; a failure here (an older server, a dropped request) only loses that batching.
      await this.fetchFn("/api/3dmodel/prepare", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ names: batch.map((b) => b.rec.name) }) });
    } catch {
      /* the requests below ask for each model on their own */
    }
    this.line.push(...batch);
    this.pump();
  }

  private pump(): void {
    while (!this.disposed && this.outstanding < MAX_OUTSTANDING && this.line.length > 0) {
      const next = this.line.shift()!;
      if (this.records.get(next.rec.name) !== next.rec) continue; // cleared or replaced since
      this.outstanding++;
      void this.load(next.rec, next.retry).finally(() => {
        this.outstanding--;
        this.pump();
      });
    }
  }

  /** Every record, for the measurements. */
  entries(): ModelRecord[] {
    return [...this.records.values()];
  }

  /** Called whenever a model becomes ready, missing or failed. Returns the unsubscribe. */
  onChange(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  /** Asks again for every model that failed (the server keeps a failed conversion until a retry is asked for: `retry=1`). */
  retryFailed(): void {
    for (const rec of [...this.records.values()]) {
      if (rec.status === "failed" || rec.status === "missing") {
        const fresh: ModelRecord = { name: rec.name, status: "loading" };
        this.records.set(rec.name, fresh);
        this.enqueue(fresh, true);
      }
    }
    this.emit();
  }

  /** Forgets everything (the board's models changed on disk, or "Reload board"): the next request loads again. */
  clear(): void {
    for (const rec of this.records.values()) this.disposeRecord(rec);
    this.records.clear();
    this.fresh = [];
    this.line = [];
    this.emit();
  }

  stats(): CacheStats {
    const s: CacheStats = { requested: this.records.size, loading: 0, ready: 0, missing: 0, failed: 0, triangles: 0 };
    for (const r of this.records.values()) {
      s[r.status]++;
      s.triangles += r.triangles ?? 0;
    }
    return s;
  }

  dispose(): void {
    this.disposed = true;
    for (const t of this.timers) clearTimeout(t);
    this.timers.clear();
    this.clear();
    this.listeners.clear();
  }

  private emit(): void {
    for (const l of [...this.listeners]) l();
  }

  private disposeRecord(rec: ModelRecord): void {
    rec.proto?.traverse((o) => {
      if (o instanceof THREE.Mesh) {
        o.geometry.dispose();
        (Array.isArray(o.material) ? o.material : [o.material]).forEach((m) => m.dispose());
      }
    });
    rec.proto = undefined;
  }

  private settle(rec: ModelRecord, patch: Partial<ModelRecord>): void {
    if (this.disposed || this.records.get(rec.name) !== rec) return;
    Object.assign(rec, patch);
    this.emit();
  }

  private wait(ms: number): Promise<void> {
    return new Promise((resolve) => {
      const t = setTimeout(() => {
        this.timers.delete(t);
        resolve();
      }, ms);
      this.timers.add(t);
    });
  }

  private async load(rec: ModelRecord, retry = false): Promise<void> {
    const started = this.now();
    try {
      for (let ask = 0; ; ask++) {
        if (this.disposed || this.records.get(rec.name) !== rec) return;
        // `&wait=1`: the server holds the request until the model is converted and answers the moment it is in -- no polling, so no timer to be throttled in a background tab.
        const url = modelUrl(rec.name) + "&wait=1" + (retry && ask === 0 ? "&retry=1" : "");
        const askedAt = this.now();
        const r = await this.fetchFn(url, { cache: "no-store" });
        if (r.status === 202) {
          // The server held it as long as it holds a request and the model is still not in: ask again at once. An answer that came straight back is an older server
          // that does not hold requests: poll, gently.
          if (this.now() - askedAt < 400) await this.wait(POLL_MS);
          continue;
        }
        if (r.status === 404 || r.status === 403) {
          const why = await r.json().then((j: { error?: string }) => j.error).catch(() => undefined);
          this.settle(rec, { status: "missing", error: why ?? `HTTP ${r.status}` });
          return;
        }
        if ((r.headers.get("content-type") ?? "").includes("application/json")) {
          const j = (await r.json()) as { status?: string; error?: string };
          if (j.status === "pending") {
            if (this.now() - askedAt < 400) await this.wait(POLL_MS);
            continue;
          }
          this.settle(rec, { status: "failed", error: j.error ?? "the model could not be converted" });
          return;
        }
        if (!r.ok) {
          this.settle(rec, { status: "failed", error: `HTTP ${r.status}` });
          return;
        }
        const text = await r.text();
        const fetched = this.now();
        if (text.length > MAX_VRML_BYTES) {
          this.settle(rec, { status: "failed", error: `the model is ${(text.length / 1e6).toFixed(0)} MB of VRML: too large to parse in the page` });
          return;
        }
        if (this.disposed || this.records.get(rec.name) !== rec) return;
        // One macrotask for the parse (it holds the page for as long as the model is big), then the models that arrived meanwhile are shown.
        const built = await parseModel(text);
        const done = this.now();
        this.settle(rec, { status: "ready", ...built, fetchMs: fetched - started, parseMs: done - fetched, readyAt: done });
        return;
      }
    } catch (e) {
      this.settle(rec, { status: "failed", error: e instanceof Error ? e.message : String(e) });
    }
  }
}
