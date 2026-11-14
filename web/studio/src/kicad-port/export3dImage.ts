// Export Image... of the 3D viewer (`EDA_3D_ACTIONS::exportImage` -> `EDA_3D_CONTROLLER::ExportImage` -> `DIALOG_EXPORT_3D_IMAGE`, 3d-viewer/dialogs/
// dialog_export_3d_image.cpp at KiCad 8303b2ad, then `EDA_3D_VIEWER_FRAME::ExportImage`): "Export 3D View" -- Image Size (Width:, Height: 1 .. 50000, in pixels, %,
// mm or in; locked to the aspect ratio by default) and Resolution (X / Y resolution: 1 .. 10000, 300 by default, in pixels/in or pixels/mm), the pixel size shown
// under them ("%d × %d pixels"); OK renders the view at that size (`captureScreenshot`) and asks for the file ("3D Image File Name", JPEG or PNG by its extension,
// PNG by default). Pure: the dialog's arithmetic, its quirks kept (the percent label's `100 * original size`; OK converting mm and inches with the resolution as
// it was before the fields, and in pixels per inch whatever the unit).

export type SizeUnit = "pixels" | "percent" | "mm" | "in";
export type ResolutionUnit = "px/in" | "px/mm";

export const SIZE_UNITS: ReadonlyArray<[SizeUnit, string]> = [
  ["pixels", "pixels"],
  ["percent", "%"],
  ["mm", "mm"],
  ["in", "in"],
];
export const RESOLUTION_UNITS: ReadonlyArray<[ResolutionUnit, string]> = [
  ["px/in", "pixels/in"],
  ["px/mm", "pixels/mm"],
];

export interface Export3DState {
  /** `m_originalSize`: the canvas' size. */
  original: { width: number; height: number };
  width: number;
  height: number;
  xRes: number;
  yRes: number;
  /** `m_xResolution` / `m_yResolution` as the dialog started (OK's mm and inch conversions read these). */
  xResKept: number;
  yResKept: number;
  aspect: number;
  locked: boolean;
  sizeUnit: SizeUnit;
  resUnit: ResolutionUnit;
}

/** The constructor: the canvas' size in pixels, 300 pixels/in, the aspect locked. */
export function initialExport3D(width: number, height: number): Export3DState {
  return { original: { width, height }, width, height, xRes: 300, yRes: 300, xResKept: 300, yResKept: 300, aspect: width / height, locked: true, sizeUnit: "pixels", resUnit: "px/in" };
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** `OnWidthChange`: the height follows the width while locked (`width / aspect`, a percent through the original width), never under 1. */
export function setWidth(s: Export3DState, width: number): Export3DState {
  const w = clamp(width, 1, 50000);
  if (!s.locked) return { ...s, width: w };
  const h = s.sizeUnit === "percent" ? (s.original.width * w) / 100 / s.aspect : w / s.aspect;
  return { ...s, width: w, height: clamp(Math.max(h, 1), 1, 50000) };
}

/** `OnHeightChange`. */
export function setHeight(s: Export3DState, height: number): Export3DState {
  const h = clamp(height, 1, 50000);
  if (!s.locked) return { ...s, height: h };
  const w = s.sizeUnit === "percent" ? ((s.original.height * h) / 100) * s.aspect : h * s.aspect;
  return { ...s, height: h, width: clamp(Math.max(w, 1), 1, 50000) };
}

/** `OnLockToggle`: locking again takes the fields' ratio (`UpdateAspectRatio`). */
export function toggleLock(s: Export3DState): Export3DState {
  const locked = !s.locked;
  return locked ? { ...s, locked, aspect: s.width / s.height } : { ...s, locked };
}

/** The resolution in pixels per inch (`UpdatePixelSize` / `ConvertSizeUnits`). */
function perInch(s: Pick<Export3DState, "xRes" | "yRes" | "resUnit">): [number, number] {
  return s.resUnit === "px/mm" ? [s.xRes * 25.4, s.yRes * 25.4] : [s.xRes, s.yRes];
}

/** `GetPixelSize`: the size in pixels (truncated); for percent, `100 * original` (KiCad's own). */
export function pixelSize(s: Export3DState, unit: SizeUnit = s.sizeUnit): { width: number; height: number } {
  const [xr, yr] = perInch(s);
  switch (unit) {
    case "pixels":
      return { width: Math.trunc(s.width), height: Math.trunc(s.height) };
    case "percent":
      return { width: Math.trunc(100 * s.original.width), height: Math.trunc(100 * s.original.height) };
    case "mm":
      return { width: Math.trunc((s.width * xr) / 25.4), height: Math.trunc((s.height * yr) / 25.4) };
    case "in":
      return { width: Math.trunc(s.width * xr), height: Math.trunc(s.height * yr) };
  }
}

/** `UpdatePixelSize`'s label. */
export function pixelSizeLabel(s: Export3DState): string {
  const p = pixelSize(s);
  return `${p.width} × ${p.height} pixels`;
}

/** `OnSizeUnitChange` -> `ConvertSizeUnits`: the fields through pixels into the new unit. */
export function setSizeUnit(s: Export3DState, unit: SizeUnit): Export3DState {
  if (unit === s.sizeUnit) return s;
  const [xr, yr] = perInch(s);
  const p = pixelSize(s);
  const next = { ...s, sizeUnit: unit };
  switch (unit) {
    case "pixels":
      return { ...next, width: p.width, height: p.height };
    case "percent":
      return { ...next, width: (p.width * 100) / s.original.width, height: (p.height * 100) / s.original.height };
    case "mm":
      return { ...next, width: (p.width * 25.4) / xr, height: (p.height * 25.4) / yr };
    case "in":
      return { ...next, width: p.width / xr, height: p.height / yr };
  }
}

/** `OnResolutionUnitChange` -> `ConvertResolutionUnits`. */
export function setResolutionUnit(s: Export3DState, unit: ResolutionUnit): Export3DState {
  if (unit === s.resUnit) return s;
  const f = unit === "px/mm" ? 1 / 25.4 : 25.4;
  return { ...s, resUnit: unit, xRes: s.xRes * f, yRes: s.yRes * f };
}

/** `TransferDataFromWindow`: the image's size in pixels (`static_cast<int>`); mm and inches through the resolution as it was (`m_xResolution` before the fields). */
export function exportSize(s: Export3DState): { width: number; height: number } {
  switch (s.sizeUnit) {
    case "pixels":
      return { width: Math.trunc(s.width), height: Math.trunc(s.height) };
    case "percent":
      return { width: Math.trunc((s.width * s.original.width) / 100), height: Math.trunc((s.height * s.original.height) / 100) };
    case "mm":
      return { width: Math.trunc((s.width * s.xResKept) / 25.4), height: Math.trunc((s.height * s.yResKept) / 25.4) };
    case "in":
      return { width: Math.trunc(s.width * s.xResKept), height: Math.trunc(s.height * s.yResKept) };
  }
}

/** `getExportFileName`: JPEG for ".jpg" / ".jpeg", PNG for ".png"; any other name is PNG (the filter's default) with ".png" added. */
export function exportFileOf(name: string): { name: string; format: "png" | "jpeg" } {
  const ext = /\.([^./\\]+)$/.exec(name)?.[1]?.toLowerCase();
  if (ext === "jpg" || ext === "jpeg") return { name, format: "jpeg" };
  if (ext === "png") return { name, format: "png" };
  return { name: `${name}.png`, format: "png" };
}
