// The 3D viewer's grid (`RENDER_3D_OPENGL::generate3dGrid`, 3d-viewer/3d_rendering/opengl/render_3d_opengl.cpp at KiCad 8303b2ad), picked by View > 3D Grid's
// actions (`EDA_3D_ACTIONS::noGrid`, `show10mmGrid`, `show5mmGrid`, `show2_5mmGrid`, `show1mmGrid` -> `EDA_3D_CONTROLLER::On3DGridSelection`: `render.grid_type`,
// default NONE). Pure: the lines in the 3D viewer's scene (components/viewer3d/scene.ts: X = board X, Z = board Y, Y = up, in mm; the board's middle plane at
// half its thickness, which is KiCad's z = 0), drawn by Viewer3D.tsx as line segments with 0.35 alpha.
//   * a horizontal grid on the board's middle plane, centred on the board's box (`GetBoardPos`), `max( board size, 100 mm ) * 1.2` each way, a line every step
//     from the centre outwards, every fifth one (the centre's included) LIGHTBLUE, the others DARKGRAY;
//   * a vertical grid through the board's centre across X (KiCad's plane `y = -boardPos.y`), from 50 mm under the middle plane to 100 mm over it: lines up the
//     Z axis every step across X, and lines across X every step up and down.

/** `GRID3D_TYPE`, as `render.grid_type` holds it. */
export type Grid3DType = "none" | "10mm" | "5mm" | "2.5mm" | "1mm";

export const GRID3D_TYPES: readonly Grid3DType[] = ["none", "10mm", "5mm", "2.5mm", "1mm"];

/** The action of each grid (`On3DGridSelection`'s parameter). */
export const GRID3D_ACTIONS: Readonly<Record<string, Grid3DType>> = {
  "3DViewer.Control.noGrid": "none",
  "3DViewer.Control.show10mmGrid": "10mm",
  "3DViewer.Control.show5mmGrid": "5mm",
  "3DViewer.Control.show2_5mmGrid": "2.5mm",
  "3DViewer.Control.show1mmGrid": "1mm",
};

const STEP_MM: Readonly<Record<Exclude<Grid3DType, "none">, number>> = { "10mm": 10, "5mm": 5, "2.5mm": 2.5, "1mm": 1 };

/** `gridColor` (`GetColor( DARKGRAY )`: 132, 132, 132) and `gridColor_marker` (`GetColor( LIGHTBLUE )`: 0, 0, 194), at `transparency` 0.35. */
export const GRID3D_COLOR = "#848484";
export const GRID3D_MARKER_COLOR = "#0000c2";
export const GRID3D_ALPHA = 0.35;

export type Vec3 = [number, number, number];

/** One line of the grid, and whether it is a fifth (marker) line. */
export interface GridLine3D {
  a: Vec3;
  b: Vec3;
  marker: boolean;
}

/**
 * `generate3dGrid( aGridType )`: the lines for a board whose box is `[minX, minZ, maxX, maxZ]` (mm) and whose middle plane is at `midY` (mm); none for NONE. The
 * board-unit rounding of `KiROUND( ii * griSizeMM * IU_PER_MM )` is kept to the nanometre.
 */
export function grid3dLines(type: Grid3DType, box: readonly [number, number, number, number], midY: number): GridLine3D[] {
  if (type === "none") return [];
  const step = STEP_MM[type];
  const [x0, z0, x1, z1] = box;
  const cx = (x0 + x1) / 2;
  const cz = (z0 + z1) / 2;
  const xsize = Math.max(x1 - x0, 100) * 1.2;
  const zsize = Math.max(z1 - z0, 100) * 1.2;
  const xmin = cx - xsize / 2, xmax = cx + xsize / 2;
  const zmin = cz - zsize / 2, zmax = cz + zsize / 2;
  const lowY = midY - 50, highY = midY + 100;
  const out: GridLine3D[] = [];
  // the horizontal grid on the board's plane
  for (let ii = 0; ; ii++) {
    const marker = ii % 5 === 0;
    const delta = Math.round(ii * step * 1e6) / 1e6;
    if (delta <= xsize / 2) {
      out.push({ a: [cx + delta, midY, zmin], b: [cx + delta, midY, zmax], marker });
      if (ii !== 0) out.push({ a: [cx - delta, midY, zmin], b: [cx - delta, midY, zmax], marker });
    }
    if (delta <= zsize / 2) {
      out.push({ a: [xmin, midY, cz + delta], b: [xmax, midY, cz + delta], marker });
      if (ii !== 0) out.push({ a: [xmin, midY, cz - delta], b: [xmax, midY, cz - delta], marker });
    }
    if (delta > zsize / 2 && delta > xsize / 2) break;
  }
  // the vertical grid through the board's centre: lines up the axis across X ...
  let vxmin = cx, vxmax = cx;
  for (let ii = 0; ; ii++) {
    const marker = ii % 5 === 0;
    const delta = ii * step;
    vxmax = cx + delta;
    out.push({ a: [vxmax, lowY, cz], b: [vxmax, highY, cz], marker });
    if (ii !== 0) {
      vxmin = cx - delta;
      out.push({ a: [vxmin, lowY, cz], b: [vxmin, highY, cz], marker });
    }
    if (delta > xsize / 2) break;
  }
  // ... and lines across X up and down the axis, between the last vertical lines (`xmin` / `xmax` as the loop above left them)
  for (let ii = 0; ; ii++) {
    const marker = ii % 5 === 0;
    const delta = ii * step;
    if (delta <= 100) out.push({ a: [vxmin, midY + delta, cz], b: [vxmax, midY + delta, cz], marker });
    if (delta <= 50 && ii !== 0) out.push({ a: [vxmin, midY - delta, cz], b: [vxmax, midY - delta, cz], marker });
    if (delta > 100 && delta > 50) break;
  }
  return out;
}
