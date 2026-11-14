// The parts of the board in the 3D scene: each placed part is its KiCad 3D models (modelCache.ts loads them; kicad-port/model3d.ts places them the way KiCad's
// renderer does). A part whose footprint names no model, or whose models the viewer has not got (not found, or not in yet), is drawn as KiCad 25ec2335's
// placeholder -- an orange, see-through box on its pads -- only with "Show parts without 3D model" on (`render.show_missing_models`, off by default; kicad-port/
// placeholder3d.ts, UI-LEFT-3 X3-2), else not at all. The board body, copper, silk and mask stay in scene.ts's group.
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
import { DEFAULT_STACK, inferKind, footprintShown, modelsByUse, modelWorldMatrix, type Model3dEntry, type ModelKind, type Stack } from "../../kicad-port/model3d";
import type { ModelCache, ModelRecord } from "./modelCache";
import { placeholderMaterials } from "./scene";
import { PLACEHOLDER_MATERIAL, placeholderOf, wantsPlaceholder, type ModelState } from "../../kicad-port/placeholder3d";
import { cloneLit, kicadLit } from "./glLighting";
import { buildExtrudedBody } from "./extrudedBody";

/** `render.opengl_selection_color`'s default (eda_3d_viewer_settings.cpp): `COLOR4D( 0.0, 1.0, 0.0, 1.0 )`. */
export const SELECTION_COLOR = 0x00ff00;

export interface PartModelOptions {
  /** The layer tree's through-hole / SMD / virtual model rows. */
  showTHT: boolean;
  showSMD: boolean;
  showVirtual: boolean;
  /** "Models not in POS File" and "Models marked DNP" (`render.show_footprints_not_in_posfile`, `render.show_footprints_dnp`). */
  showNotInPos: boolean;
  showDNP: boolean;
  /** render.opengl_show_model_bbox: an outline of each model's bounding box. */
  showBoundingBoxes: boolean;
  /** `render.show_missing_models` ("Show parts without 3D model", off by default): the placeholder of a part without its models. */
  showMissingModels?: boolean;
}

export interface PartStats {
  total: number;
  /** Parts drawn with at least one real model. */
  asModels: number;
  /** Parts drawn as their placeholder box. */
  asBoxes: number;
  /** Parts with nothing drawn: no model in and no placeholder ("Show parts without 3D model" off). */
  asNothing?: number;
  /** Parts shown right now (the through-hole, SMD and virtual rows let their kind through). */
  shown: number;
}

interface PartEntry {
  ref: string;
  signature: string;
  group: THREE.Group;
  kind: ModelKind;
  withModels: boolean;
  /** Drawn with the placeholder ("Show parts without 3D model"). */
  withPlaceholder: boolean;
  /** What the group made and owns: geometry of the placeholder box and the outlines, materials of a model with opacity under 1. */
  owned: Array<{ dispose(): void }>;
}

const BBOX_MATERIAL = new THREE.LineBasicMaterial({ color: 0xffff00 });

/** `m_placeholderModel`'s mesh: the unit cube about the origin, scaled and placed per part. */
const PLACEHOLDER_BOX = new THREE.BoxGeometry(1, 1, 1);

function placeholderMaterial3d(): THREE.Material {
  const m = PLACEHOLDER_MATERIAL;
  const rgb = (c: readonly number[]) => new THREE.Color().setRGB(c[0]!, c[1]!, c[2]!, THREE.SRGBColorSpace);
  return kicadLit(new THREE.MeshPhongMaterial({ color: rgb(m.diffuse), specular: rgb(m.specular), shininess: m.shininess * 128, transparent: true, opacity: 1 - m.transparency, depthWrite: false, side: THREE.DoubleSide }), rgb(m.ambient));
}

function modelSignature(m: Model3dEntry): string {
  return `${m.name}|${m.offset.join(",")}|${m.scale.join(",")}|${m.rotate.join(",")}|${m.opacity}`;
}

export class PartModels {
  readonly group = new THREE.Group();
  private readonly parts = new Map<string, PartEntry>();
  private readonly materials = placeholderMaterials();
  /** `m_placeholderModel`'s material (kicad-port/placeholder3d.ts `PLACEHOLDER_MATERIAL`): orange, 40% transparent. */
  private readonly placeholderMaterial = placeholderMaterial3d();
  private readonly highlights = new Map<THREE.Material, THREE.Material>();
  private hovered: string | null = null;
  /** The footprints selected in the PCB editor (`fp->IsSelected()`), drawn in the selection colour as well. */
  private selected = new Set<string>();
  /** `render.opengl_selection_color` (Preferences > 3D Viewer > Realtime Rendering), green by default. */
  private selectionColor: number = SELECTION_COLOR;
  private last: { board: BoardState | null; opts: PartModelOptions; stack: Stack } | null = null;
  /** `render.material_mode` (Preferences > 3D Viewer > General, "Material properties:"): the models' own materials, solid colours or CAD colours. */
  private materialMode: "realistic" | "solid" | "cad" = "realistic";
  private readonly modeMaterials = new Map<THREE.Material, THREE.Material>();
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
    const stats: PartStats = { total: 0, asModels: 0, asBoxes: 0, asNothing: 0, shown: 0 };
    // The models the board uses are asked for most-used first: the packages most of the board is made of are the first loaded and drawn.
    for (const name of modelsByUse(board?.parts ?? [])) this.cache.request(name);
    for (const part of board?.parts ?? []) {
      if (!part.placed || !part.side || !part.at) continue;
      seen.add(part.ref);
      const wanted = (part.models ?? []).filter((m) => m.show);
      const ready: Array<{ model: Model3dEntry; record: ModelRecord }> = [];
      const states: Array<{ show: boolean; name: string; state: ModelState }> = (part.models ?? []).map((m) => ({ show: m.show, name: m.name, state: "loading" as ModelState }));
      for (const model of wanted) {
        const record = this.cache.request(model.name);
        if (record.status === "ready" && record.proto) ready.push({ model, record });
        for (const st of states) if (st.name === model.name) st.state = record.status === "ready" && record.proto ? "ready" : record.status === "ready" ? "failed" : record.status;
      }
      const placeholder = wantsPlaceholder(states, opts.showMissingModels ?? false, !!part.extruded_body);
      const kind: ModelKind = part.kind3d ?? inferKind(part.pads);
      const signature = [part.at.join(","), part.rot ?? 0, part.side, stack.boardThicknessMm, stack.copperMm, stack.copperBackMm, opts.showBoundingBoxes, ready.map((r) => modelSignature(r.model)).join(";"), placeholder ? JSON.stringify([part.body, part.courtyard, (part.pads ?? []).map((q) => [q.px ?? q.x, q.py ?? q.y, q.size ?? [q.w, q.h]])]) : "", part.extruded_body ? JSON.stringify(part.extruded_body) : ""].join("#");
      let entry = this.parts.get(part.ref);
      if (!entry || entry.signature !== signature) {
        if (entry) this.disposeEntry(entry);
        entry = this.build(part, ready, placeholder, signature, kind, opts, stack);
        this.parts.set(part.ref, entry);
        this.group.add(entry.group);
        if (this.lit(part.ref)) this.paint(entry, true);
      }
      entry.group.visible = footprintShown({ kind, dnp: part.dnp, notInPos: part.not_in_pos }, { tht: opts.showTHT, smd: opts.showSMD, virtual: opts.showVirtual, notInPos: opts.showNotInPos, dnp: opts.showDNP });
      stats.total++;
      if (entry.group.visible) stats.shown++;
      if (entry.withModels) stats.asModels++;
      else if (entry.withPlaceholder) stats.asBoxes++;
      else stats.asNothing!++;
    }
    for (const [ref, entry] of [...this.parts]) {
      if (seen.has(ref)) continue;
      this.disposeEntry(entry);
      this.parts.delete(ref);
      if (this.hovered === ref) this.hovered = null;
    }
    this.onStats(stats);
  }

  /** "Material properties:" changed: every part with models is drawn again in the new mode. */
  setMaterialMode(mode: "realistic" | "solid" | "cad"): void {
    if (mode === this.materialMode) return;
    this.materialMode = mode;
    for (const entry of this.parts.values()) this.disposeEntry(entry);
    this.parts.clear();
    for (const m of this.modeMaterials.values()) m.dispose();
    this.modeMaterials.clear();
    for (const h of this.highlights.values()) h.dispose();
    this.highlights.clear();
    this.refresh();
  }

  /**
   * A model's material in the material mode (`MODEL_3D::Draw`): "realistic" its own (`OglSetMaterial`); "solid" `OglSetDiffuseMaterial` of its diffuse colour (ambient 0.2,
   * no specular, no emission, shininess 0, opaque but for the model's opacity); "cad" the same of `MaterialDiffuseToColorCAD( diffuse )` (the colour's grey, cut to four steps,
   * with an eighth of the colour).
   */
  private forMode(base: THREE.Material): THREE.Material {
    if (this.materialMode === "realistic" || !(base instanceof THREE.MeshPhongMaterial)) return base;
    let m = this.modeMaterials.get(base);
    if (!m) {
      const raw = { r: 0, g: 0, b: 0 };
      base.color.getRGB(raw, THREE.SRGBColorSpace);
      if (this.materialMode === "cad") {
        const gray = raw.r * 0.2126 + raw.g * 0.7152 + raw.b * 0.0722;
        const luminance = Math.min((Math.trunc(4 * gray) + 0.5) / 4, 1);
        const max = Math.max(raw.r, raw.g, raw.b, Number.EPSILON);
        for (const k of ["r", "g", "b"] as const) raw[k] = (raw[k] / max) * 0.125 + luminance * 0.875;
      }
      const diffuse = new THREE.Color().setRGB(raw.r, raw.g, raw.b, THREE.SRGBColorSpace);
      const ambient = new THREE.Color().setRGB(0.2, 0.2, 0.2, THREE.SRGBColorSpace);
      m = kicadLit(new THREE.MeshPhongMaterial({ color: diffuse, specular: 0x000000, emissive: 0x000000, shininess: 0, side: THREE.DoubleSide }), ambient);
      this.modeMaterials.set(base, m);
    }
    return m;
  }

  /** Re-runs the last sync (a model came in). */
  refresh(): void {
    if (this.last) this.sync(this.last.board, this.last.opts, this.last.stack);
  }

  private build(part: Part, ready: Array<{ model: Model3dEntry; record: ModelRecord }>, placeholder: boolean, signature: string, kind: ModelKind, opts: PartModelOptions, stack: Stack): PartEntry {
    const group = new THREE.Group();
    group.name = "part";
    group.userData.ref = part.ref;
    const owned: PartEntry["owned"] = [];
    // The footprint's Extruded 3D Body (extrudedBody.ts), beside its models; a part that has one has no placeholder box ("Suppress placeholder if a valid extruded body was built").
    const body = buildExtrudedBody(part, stack);
    if (body) {
      group.add(body.object);
      owned.push(...body.owned);
    }
    // `renderPlaceholderForFootprint`: one for the footprint, beside the models that are in (kicad-port/placeholder3d.ts)
    let withPlaceholder = false;
    const ph = placeholder ? placeholderOf(part) : null;
    if (ph) {
      const mesh = new THREE.Mesh(PLACEHOLDER_BOX, this.placeholderMaterial);
      mesh.name = "part-placeholder";
      mesh.matrixAutoUpdate = false;
      mesh.matrix.fromArray(modelWorldMatrix({ xMm: ph.centerNm[0] / 1e6, yMm: ph.centerNm[1] / 1e6, rotDeg: ph.rotDeg, bottom: ph.bottom }, { offset: [0, 0, ph.sizeMm[2] * 0.5], scale: ph.sizeMm, rotate: [0, 0, 0] }, stack));
      mesh.matrixWorldNeedsUpdate = true;
      group.add(mesh);
      withPlaceholder = true;
    }
    {
      const placement = { xMm: part.at![0] / 1e6, yMm: part.at![1] / 1e6, rotDeg: part.rot ?? 0, bottom: part.side === "bottom" };
      for (const { model, record } of ready) {
        const instance = new THREE.Group();
        instance.name = "part-model";
        instance.matrixAutoUpdate = false;
        instance.matrix.fromArray(modelWorldMatrix(placement, model, stack));
        instance.matrixWorldNeedsUpdate = true;
        for (const child of record.proto!.children) {
          const mesh = child.clone() as THREE.Mesh;
          mesh.material = this.forMode(mesh.material as THREE.Material);
          if (model.opacity < 1) {
            // `(opacity v)`: this instance's own materials, a fraction as opaque as the model's.
            const base = mesh.material as THREE.Material;
            const faded = base instanceof THREE.MeshPhongMaterial ? cloneLit(base) : base.clone();
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
    return { ref: part.ref, signature, group, kind, withModels: ready.length > 0, withPlaceholder, owned };
  }

  private disposeEntry(entry: PartEntry): void {
    this.group.remove(entry.group);
    for (const o of entry.owned) o.dispose();
  }

  /** `highlight_on_rollover`: the part `ref` under the pointer is drawn in the selection colour; `null` clears it. Returns whether anything changed. */
  setHover(ref: string | null): boolean {
    if (ref === this.hovered) return false;
    const before = this.hovered ? this.parts.get(this.hovered) : undefined;
    const was = this.hovered;
    this.hovered = ref && this.parts.has(ref) ? ref : null;
    if (before && was) this.paint(before, this.lit(was));
    const after = this.hovered ? this.parts.get(this.hovered) : undefined;
    if (after) this.paint(after, true);
    return true;
  }

  /** Whether part `ref` is drawn in the selection colour: `highlight = fp->IsSelected() || ( highlight_on_rollover && fp == m_currentRollOverItem )` (render_3d_opengl.cpp). */
  private lit(ref: string): boolean {
    return this.selected.has(ref) || this.hovered === ref;
  }

  /** The PCB editor's selection: the footprints in it are drawn in the selection colour (`fp->IsSelected()`). Returns whether anything changed. */
  setSelected(refs: Iterable<string>): boolean {
    const next = new Set(refs);
    const changed = [...new Set([...next, ...this.selected])].filter((r) => next.has(r) !== this.selected.has(r));
    if (changed.length === 0) return false;
    this.selected = next;
    for (const ref of changed) {
      const entry = this.parts.get(ref);
      if (entry) this.paint(entry, this.lit(ref));
    }
    return true;
  }

  /** The selected footprints drawn highlighted, for the test hook. */
  get selectedRefs(): string[] {
    return [...this.selected].filter((r) => this.parts.has(r)).sort();
  }

  /** `render.opengl_selection_color`: the colour a selected or hovered part is drawn in. */
  setSelectionColor(color: number): void {
    if (color === this.selectionColor) return;
    this.selectionColor = color;
    for (const h of this.highlights.values()) if (h instanceof THREE.MeshPhongMaterial || h instanceof THREE.MeshStandardMaterial) h.color.set(color);
  }

  get hoveredRef(): string | null {
    return this.hovered;
  }

  /** `OglSetMaterial` with `aUseSelectedMaterial`: the diffuse colour of the material is the selection colour; ambient, specular and emission stay. */
  private highlightOf(base: THREE.Material): THREE.Material {
    let h = this.highlights.get(base);
    if (!h) {
      h = base instanceof THREE.MeshPhongMaterial ? cloneLit(base) : base.clone();
      if (h instanceof THREE.MeshPhongMaterial || h instanceof THREE.MeshStandardMaterial) h.color.set(this.selectionColor);
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
    for (const m of this.modeMaterials.values()) m.dispose();
    this.modeMaterials.clear();
    this.materials.ic.dispose();
    this.materials.passive.dispose();
    this.placeholderMaterial.dispose();
    this.hovered = null;
    this.last = null;
  }
}
