// The parts of the board in the 3D scene: each placed part is its KiCad 3D models (modelCache.ts loads them; kicad-port/model3d.ts places them the way KiCad's
// renderer does), or -- until a part's first model is in, or when its footprint names none -- the box the instant scene always drew (its F.Fab body, scene.ts
// `buildPartPlaceholder`). The board body, copper, silk and mask stay in scene.ts's group.
//
// A part is rebuilt only when what it draws changes (its placement, its models, which of them are in, the bounding-box switch): a board edit that moves one part
// rebuilds one group, and a model arriving rebuilds the parts that use it. Every instance of a model shares that model's geometry and materials (modelCache.ts), so
// nothing here disposes them -- only what this file made: the placeholder boxes, the bounding-box outlines, the materials of a model with opacity under 1 and
// the highlight materials.
//
// Hover (`RENDER_3D_OPENGL`'s rollover: `highlight_on_rollover`, default on, `opengl_selection_color`, default green): the part under the pointer has the diffuse colour
// of every material of its models swapped for the selection colour (`OglSetMaterial`'s `aUseSelectedMaterial`); `pick` finds the part for a ray.
import * as THREE from "three";
import type { BoardState, Part } from "../../api/types";
import { DEFAULT_STACK, inferKind, kindShown, modelWorldMatrix, type Model3dEntry, type ModelKind, type Stack } from "../../kicad-port/model3d";
import type { ModelCache, ModelRecord } from "./modelCache";
import { buildPartPlaceholder, placeholderMaterials } from "./scene";

/** `render.opengl_selection_color`'s default (eda_3d_viewer_settings.cpp): `COLOR4D( 0.0, 1.0, 0.0, 1.0 )`. */
export const SELECTION_COLOR = 0x00ff00;

export interface PartModelOptions {
  /** The layer tree's through-hole / SMD / virtual model rows. */
  showTHT: boolean;
  showSMD: boolean;
  showVirtual: boolean;
  /** render.opengl_show_model_bbox: an outline of each model's bounding box. */
  showBoundingBoxes: boolean;
}

export interface PartStats {
  total: number;
  /** Parts drawn with at least one real model. */
  asModels: number;
  /** Parts drawn as their placeholder box. */
  asBoxes: number;
}

interface PartEntry {
  ref: string;
  signature: string;
  group: THREE.Group;
  kind: ModelKind;
  withModels: boolean;
  /** What the group made and owns: geometry of the placeholder box and the outlines, materials of a model with opacity under 1. */
  owned: Array<{ dispose(): void }>;
}

const BBOX_MATERIAL = new THREE.LineBasicMaterial({ color: 0xffff00 });

function modelSignature(m: Model3dEntry): string {
  return `${m.name}|${m.offset.join(",")}|${m.scale.join(",")}|${m.rotate.join(",")}|${m.opacity}`;
}

export class PartModels {
  readonly group = new THREE.Group();
  private readonly parts = new Map<string, PartEntry>();
  private readonly materials = placeholderMaterials();
  private readonly highlights = new Map<THREE.Material, THREE.Material>();
  private hovered: string | null = null;
  private last: { board: BoardState | null; opts: PartModelOptions; stack: Stack } | null = null;
  private readonly unsubscribe: () => void;

  constructor(
    private readonly cache: ModelCache,
    /** Told after every sync how the parts are drawn. */
    private readonly onStats: (stats: PartStats) => void = () => {}
  ) {
    this.group.name = "viewer3d-parts";
    this.unsubscribe = cache.onChange(() => this.refresh());
  }

  /** Draws `board`'s placed parts: asks the cache for every model they use, and rebuilds the parts whose drawing changed. */
  sync(board: BoardState | null, opts: PartModelOptions, stack: Stack = DEFAULT_STACK): void {
    this.last = { board, opts, stack };
    const seen = new Set<string>();
    const stats: PartStats = { total: 0, asModels: 0, asBoxes: 0 };
    for (const part of board?.parts ?? []) {
      if (!part.placed || !part.side || !part.at) continue;
      seen.add(part.ref);
      const wanted = (part.models ?? []).filter((m) => m.show);
      const ready: Array<{ model: Model3dEntry; record: ModelRecord }> = [];
      for (const model of wanted) {
        const record = this.cache.request(model.name);
        if (record.status === "ready" && record.proto) ready.push({ model, record });
      }
      const kind: ModelKind = part.kind3d ?? inferKind(part.pads);
      const signature = [part.at.join(","), part.rot ?? 0, part.side, stack.boardThicknessMm, stack.copperMm, opts.showBoundingBoxes, ready.map((r) => modelSignature(r.model)).join(";"), ready.length === 0 ? JSON.stringify([part.body, part.courtyard, part.pads?.length]) : ""].join("#");
      let entry = this.parts.get(part.ref);
      if (!entry || entry.signature !== signature) {
        if (entry) this.disposeEntry(entry);
        entry = this.build(part, ready, signature, kind, opts, stack);
        this.parts.set(part.ref, entry);
        this.group.add(entry.group);
        if (this.hovered === part.ref) this.paint(entry, true);
      }
      entry.group.visible = kindShown(kind, { tht: opts.showTHT, smd: opts.showSMD, virtual: opts.showVirtual });
      stats.total++;
      if (entry.withModels) stats.asModels++;
      else stats.asBoxes++;
    }
    for (const [ref, entry] of [...this.parts]) {
      if (seen.has(ref)) continue;
      this.disposeEntry(entry);
      this.parts.delete(ref);
      if (this.hovered === ref) this.hovered = null;
    }
    this.onStats(stats);
  }

  /** Re-runs the last sync (a model came in). */
  refresh(): void {
    if (this.last) this.sync(this.last.board, this.last.opts, this.last.stack);
  }

  private build(part: Part, ready: Array<{ model: Model3dEntry; record: ModelRecord }>, signature: string, kind: ModelKind, opts: PartModelOptions, stack: Stack): PartEntry {
    const group = new THREE.Group();
    group.name = "part";
    group.userData.ref = part.ref;
    const owned: PartEntry["owned"] = [];
    if (ready.length === 0) {
      const box = buildPartPlaceholder(part, this.materials);
      if (box) {
        group.add(box);
        owned.push(box.geometry);
        box.traverse((o) => {
          if (o instanceof THREE.LineSegments) owned.push(o.geometry);
        });
      }
    } else {
      const placement = { xMm: part.at![0] / 1000, yMm: part.at![1] / 1000, rotDeg: part.rot ?? 0, bottom: part.side === "bottom" };
      for (const { model, record } of ready) {
        const instance = new THREE.Group();
        instance.name = "part-model";
        instance.matrixAutoUpdate = false;
        instance.matrix.fromArray(modelWorldMatrix(placement, model, stack));
        instance.matrixWorldNeedsUpdate = true;
        for (const child of record.proto!.children) {
          const mesh = child.clone() as THREE.Mesh;
          if (model.opacity < 1) {
            // `(opacity v)`: this instance's own materials, a fraction as opaque as the model's.
            const base = mesh.material as THREE.Material;
            const faded = base.clone();
            faded.transparent = true;
            faded.opacity = base.opacity * model.opacity;
            mesh.material = faded;
            owned.push(faded);
          }
          mesh.userData.baseMaterial = mesh.material;
          instance.add(mesh);
        }
        if (opts.showBoundingBoxes && record.bbox) {
          const size = record.bbox.getSize(new THREE.Vector3());
          const box = new THREE.BoxGeometry(size.x, size.y, size.z);
          const outline = new THREE.LineSegments(new THREE.EdgesGeometry(box), BBOX_MATERIAL);
          box.dispose();
          outline.position.copy(record.bbox.getCenter(new THREE.Vector3()));
          outline.name = "part-bbox";
          outline.raycast = () => {};
          instance.add(outline);
          owned.push(outline.geometry);
        }
        group.add(instance);
      }
    }
    // The placeholder box keeps its own materials (shared, `this.materials`): the base to go back to when the hover leaves.
    group.traverse((o) => {
      if (o instanceof THREE.Mesh && !o.userData.baseMaterial) o.userData.baseMaterial = o.material;
    });
    return { ref: part.ref, signature, group, kind, withModels: ready.length > 0, owned };
  }

  private disposeEntry(entry: PartEntry): void {
    this.group.remove(entry.group);
    for (const o of entry.owned) o.dispose();
  }

  /** `highlight_on_rollover`: the part `ref` under the pointer is drawn in the selection colour; `null` clears it. Returns whether anything changed. */
  setHover(ref: string | null): boolean {
    if (ref === this.hovered) return false;
    const before = this.hovered ? this.parts.get(this.hovered) : undefined;
    if (before) this.paint(before, false);
    this.hovered = ref && this.parts.has(ref) ? ref : null;
    const after = this.hovered ? this.parts.get(this.hovered) : undefined;
    if (after) this.paint(after, true);
    return true;
  }

  get hoveredRef(): string | null {
    return this.hovered;
  }

  /** `OglSetMaterial` with `aUseSelectedMaterial`: the diffuse colour of the material is the selection colour; ambient, specular and emission stay. */
  private highlightOf(base: THREE.Material): THREE.Material {
    let h = this.highlights.get(base);
    if (!h) {
      h = base.clone();
      if (h instanceof THREE.MeshPhongMaterial || h instanceof THREE.MeshStandardMaterial) h.color.set(SELECTION_COLOR);
      this.highlights.set(base, h);
    }
    return h;
  }

  private paint(entry: PartEntry, on: boolean): void {
    entry.group.traverse((o) => {
      if (!(o instanceof THREE.Mesh)) return;
      const base = o.userData.baseMaterial as THREE.Material | undefined;
      if (base) o.material = on ? this.highlightOf(base) : base;
    });
  }

  /** The part a ray (from the camera through the pointer) hits first, with the distance, or null. The bounding-box outlines are not hit. */
  pick(raycaster: THREE.Raycaster): { ref: string; distance: number } | null {
    const targets = [...this.parts.values()].filter((e) => e.group.visible).map((e) => e.group);
    for (const hit of raycaster.intersectObjects(targets, true)) {
      let o: THREE.Object3D | null = hit.object;
      while (o && !o.userData.ref) o = o.parent;
      if (o) return { ref: o.userData.ref as string, distance: hit.distance };
    }
    return null;
  }

  /** The reference designators of the parts drawn as real models right now, for the test hook. */
  drawnAsModels(): string[] {
    return [...this.parts.values()].filter((e) => e.withModels).map((e) => e.ref).sort();
  }

  dispose(): void {
    this.unsubscribe();
    for (const entry of this.parts.values()) this.disposeEntry(entry);
    this.parts.clear();
    for (const h of this.highlights.values()) h.dispose();
    this.highlights.clear();
    this.materials.ic.dispose();
    this.materials.passive.dispose();
    this.hovered = null;
    this.last = null;
  }
}
