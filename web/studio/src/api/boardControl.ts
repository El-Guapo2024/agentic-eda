// The HTTP side of the board-control actions: the export / output dialogs kicad-cli has a command for
// (`crates/cli/src/board_output_api.rs`), the repair report and the footprint associations
// (`crates/cli/src/board_control_api.rs`). Every option is the one KiCad's own dialog has, under the name the backend reads.

import type { FabReply } from "./client";

async function postJson<T>(url: string, body: unknown): Promise<T> {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  return (await r.json()) as T;
}

// ------------------------------------------------------------------ 3D formats

/** `DIALOG_EXPORT_STEP`'s format choice, by its kicad-cli command. */
export type ThreeDFormat = "step" | "glb" | "xao" | "brep" | "ply" | "stl" | "stpz" | "u3d" | "3dpdf";

export const THREE_D_FORMATS: { value: ThreeDFormat; label: string }[] = [
  { value: "step", label: "STEP" },
  { value: "glb", label: "GLB (Binary glTF)" },
  { value: "xao", label: "XAO" },
  { value: "brep", label: "BREP (OCCT)" },
  { value: "ply", label: "PLY (ASCII)" },
  { value: "stl", label: "STL" },
  { value: "stpz", label: "STPZ" },
  { value: "u3d", label: "U3D" },
  { value: "3dpdf", label: "PDF" },
];

/** Where the 3D export is measured from: `DIALOG_EXPORT_STEP`'s origin radio buttons. */
export type ThreeDOrigin = "drill" | "grid" | "user" | "board_center";

export interface ThreeDOptions {
  format: ThreeDFormat;
  board_body: boolean;
  components: boolean;
  tracks: boolean;
  pads: boolean;
  zones: boolean;
  inner_copper: boolean;
  silkscreen: boolean;
  soldermask: boolean;
  fuse_shapes: boolean;
  cut_vias_in_body: boolean;
  fill_all_vias: boolean;
  /** "Remove components with 'Unspecified' footprint type". */
  no_unspecified: boolean;
  /** "Remove components with 'Do not populate' attribute". */
  no_dnp: boolean;
  subst_models: boolean;
  optimize: boolean;
  net_filter: string;
  /** Reference designators, comma separated, wildcards allowed (the dialog's "Components matching filter", or the selected ones). */
  component_filter: string;
  origin: ThreeDOrigin;
  origin_x: number;
  origin_y: number;
  /** Board outline chaining tolerance, mm. */
  tolerance: number;
}

/** `dialog_export_step_base.cpp`'s own defaults. */
export const DEFAULT_3D_OPTIONS: ThreeDOptions = {
  format: "step",
  board_body: true,
  components: true,
  tracks: false,
  pads: false,
  zones: false,
  inner_copper: false,
  silkscreen: false,
  soldermask: false,
  fuse_shapes: false,
  cut_vias_in_body: false,
  fill_all_vias: false,
  no_unspecified: false,
  no_dnp: false,
  subst_models: true,
  optimize: true,
  net_filter: "",
  component_filter: "",
  origin: "drill",
  origin_x: 0,
  origin_y: 0,
  tolerance: 0.001,
};

export function postFab3d(options: ThreeDOptions): Promise<FabReply> {
  return postJson<FabReply>("/api/fab/3d", options);
}

// ------------------------------------------------------------------------ VRML

export type VrmlUnits = "mm" | "m" | "tenths" | "in";

export interface VrmlOptions {
  units: VrmlUnits;
  no_unspecified: boolean;
  no_dnp: boolean;
  /** "User defined origin"; off = the board centre. */
  user_origin: boolean;
  origin_x: number;
  origin_y: number;
  /** "Copy 3D model files to 3D model path": a folder of models beside the board file; off embeds them. */
  copy_models: boolean;
  models_dir: string;
  relative_paths: boolean;
}

export const DEFAULT_VRML_OPTIONS: VrmlOptions = { units: "m", no_unspecified: false, no_dnp: false, user_origin: false, origin_x: 0, origin_y: 0, copy_models: false, models_dir: "shapes3D", relative_paths: false };

export function postFabVrml(options: VrmlOptions): Promise<FabReply> {
  return postJson<FabReply>("/api/fab/vrml", options);
}

// ---------------------------------------------------------------------- GenCAD

export interface GencadOptions {
  flip_bottom_pads: boolean;
  unique_pins: boolean;
  unique_footprints: boolean;
  use_drill_origin: boolean;
  store_origin: boolean;
}

export const DEFAULT_GENCAD_OPTIONS: GencadOptions = { flip_bottom_pads: false, unique_pins: false, unique_footprints: false, use_drill_origin: false, store_origin: false };

export function postFabGencad(options: GencadOptions): Promise<FabReply> {
  return postJson<FabReply>("/api/fab/gencad", options);
}

// -------------------------------------------------------------------- IPC-2581

export interface Ipc2581Options {
  units: "mm" | "in";
  precision: number;
  version: "B" | "C";
  compress: boolean;
  bom_rev: string;
  /** BOM columns: field names, "" = omitted / generated. */
  col_id: string;
  col_mpn: string;
  col_mfg: string;
  col_dist_pn: string;
  col_dist: string;
}

export const DEFAULT_IPC2581_OPTIONS: Ipc2581Options = { units: "mm", precision: 6, version: "C", compress: false, bom_rev: "", col_id: "", col_mpn: "", col_mfg: "", col_dist_pn: "", col_dist: "" };

export function postFabIpc2581(options: Ipc2581Options): Promise<FabReply> {
  return postJson<FabReply>("/api/fab/ipc2581", options);
}

// ----------------------------------------------------------------------- ODB++

export interface OdbOptions {
  units: "mm" | "in";
  precision: number;
  compression: "none" | "zip" | "tgz";
}

export const DEFAULT_ODB_OPTIONS: OdbOptions = { units: "mm", precision: 6, compression: "zip" };

export function postFabOdb(options: OdbOptions): Promise<FabReply> {
  return postJson<FabReply>("/api/fab/odb", options);
}

// ------------------------------------------------------- no options: D356, BOM, cmp

export function postFabIpcD356(): Promise<FabReply> {
  return postJson<FabReply>("/api/fab/ipcd356", {});
}

export function postFabPcbBom(): Promise<FabReply> {
  return postJson<FabReply>("/api/fab/pcb_bom", {});
}

export function postFabCmp(): Promise<FabReply> {
  return postJson<FabReply>("/api/fab/cmp", {});
}

// ----------------------------------------------------------------- repair board

export interface RepairReply {
  ok: boolean;
  repaired?: number;
  /** "%d potential problems repaired." or "No board problems found."; the failure's reason when `ok` is false. */
  message: string;
  /** The lines KiCad lists under it ("N duplicate IDs replaced.", "Orphaned net X re-parented."). */
  details?: string[];
}

export function postRepairBoard(): Promise<RepairReply> {
  return postJson<RepairReply>("/api/repair_board", {});
}

// ------------------------------------------------------- footprint associations

export interface FootprintAssociations {
  ok: boolean;
  message?: string;
  reference: string;
  /** `LIB_ID`: the library nickname ("" for a footprint with none) and the item name. */
  library: string;
  library_description: string;
  footprint: string;
  footprint_description: string;
  /** The schematic symbol the footprint belongs to: its sheet, reference, library symbol and value; null when the schematic has none for it. */
  symbol: { sheet: string; reference: string; lib_id: string; value: string } | null;
}

export async function fetchFootprintAssociations(reference: string): Promise<FootprintAssociations> {
  const r = await fetch(`/api/footprint_associations?ref=${encodeURIComponent(reference)}`, { cache: "no-store" });
  return (await r.json()) as FootprintAssociations;
}
