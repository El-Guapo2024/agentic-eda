// A footprint's Extruded 3D Body as KiCad's OpenGL 3D view draws it: `RENDER_3D_OPENGL::renderExtrudedBodies` (3d-viewer/3d_rendering/opengl/create_scene.cpp at 8303b2ad),
// `ApplyExtrusionTransform` and `GetMaterialProps` (3d-viewer/3d_rendering/3d_placeholder_utils.cpp), the colours of `EXTRUDED_3D_BODY::GetDefaultColor` (pcbnew/footprint.h)
// and the pins' material of `RENDER_3D_OPENGL::render` (render_3d_opengl.cpp, "Render extruded pad standoffs (metallic pins)").
//
// The server sends the body with the outline KiCad extrudes (`GetExtrusionOutline`, crates/cli/src/extruded_body_api.rs) and the pins (`GetExtrusionPinOutline`), in board
// nanometres; this module is what is done with them -- the body's own transform, the heights, the materials. Pure: no three.js (components/viewer3d/extrudedBody.ts builds
// the meshes).
import type { ExtrudedBodyInfo } from "../api/types";
import { footprintZ, type Stack } from "./model3d";
import type { Material3d, Rgb } from "./material3d";

/** A contour in board nanometres, `[x, y]` pairs; a polygon is its outline followed by its holes. */
export type Contour = Array<[number, number]>;
export type Polygon = Contour[];

/** `KiROUND`: half away from zero. */
export function kiRound(v: number): number {
  return v < 0 ? Math.trunc(v - 0.5) : Math.trunc(v + 0.5);
}

/** `EDA_ANGLE::Normalize`: into [0, 360). */
function normalize(deg: number): number {
  let a = deg;
  while (a < -0) a += 360;
  while (a >= 360) a -= 360;
  return a;
}

/** `EDA_ANGLE::Sin` and `Cos`, exact at the multiples of 45 degrees. */
function sinCos(deg: number): [number, number] {
  const a = normalize(deg);
  const r = Math.SQRT1_2;
  const exact: Record<number, [number, number]> = { 0: [0, 1], 45: [r, r], 90: [1, 0], 135: [r, -r], 180: [0, -1], 225: [-r, -r], 270: [-1, 0], 315: [-r, r] };
  const e = exact[a];
  if (e) return e;
  const rad = (deg * Math.PI) / 180;
  return [Math.sin(rad), Math.cos(rad)];
}

/**
 * `RotatePoint( VECTOR2I& point, const VECTOR2I& centre, const EDA_ANGLE& angle )` (libs/kimath/src/trigo.cpp): the point turned about `centre`, integer nanometres -- the
 * quarter turns exact, any other angle `x' = y sin + x cos`, `y' = y cos - x sin` rounded (`KiROUND`). Positive is counter-clockwise on the screen (y down).
 */
export function rotatePoint([x, y]: [number, number], [cx, cy]: [number, number], deg: number): [number, number] {
  const ox = x - cx;
  const oy = y - cy;
  const a = normalize(deg);
  let p: [number, number];
  if (a === 0) p = [ox, oy];
  else if (a === 90) p = [oy, -ox];
  else if (a === 180) p = [-ox, -oy];
  else if (a === 270) p = [-oy, ox];
  else {
    const [s, c] = sinCos(deg);
    p = [kiRound(oy * s + ox * c), kiRound(oy * c - ox * s)];
  }
  return [p[0] + cx, p[1] + cy];
}

/**
 * `ApplyExtrusionTransform( aOutline, aBody, aFpPos )`: the outline turned by the body's z rotation about the footprint's position (`SHAPE_POLY_SET::Rotate`), scaled by its x and
 * y scale about the same point (`SHAPE_POLY_SET::Scale`: `KiROUND( centre + ( pt - centre ) * factor )`), and moved by its x and y offset (`Move( VECTOR2I( mmToIU( x ),
 * mmToIU( y ) ) )`) -- in the board's frame, whatever the footprint's turn or side. Each step only when it does something, as KiCad's.
 */
export function applyExtrusionTransform(polys: readonly Polygon[], body: Pick<ExtrudedBodyInfo, "rotate" | "scale" | "offset">, at: [number, number]): Polygon[] {
  const [sx, sy] = body.scale;
  const [ox, oy] = [kiRound(body.offset[0] * 1e6), kiRound(body.offset[1] * 1e6)];
  const turn = body.rotate[2] !== 0;
  const scale = sx !== 1 || sy !== 1;
  const move = body.offset[0] !== 0 || body.offset[1] !== 0;
  return polys.map((poly) =>
    poly.map((chain) =>
      chain.map((pt) => {
        let p: [number, number] = [pt[0], pt[1]];
        if (turn) p = rotatePoint(p, at, body.rotate[2]);
        if (scale) p = [kiRound(at[0] + (p[0] - at[0]) * sx), kiRound(at[1] + (p[1] - at[1]) * sy)];
        if (move) p = [p[0] + ox, p[1] + oy];
        return p;
      })
    )
  );
}

/**
 * The heights of the body, millimetres above the board's centre plane (`renderExtrudedBodies`): from the footprint's mounting surface (`GetFootprintZPos`), past the standoff and
 * the z offset, the body's thickness `( height - standoff ) * scale.z` -- up on the front, down on the back.
 */
export function bodyZRange(body: Pick<ExtrudedBodyInfo, "height" | "standoff" | "scale" | "offset">, stack: Stack, bottom: boolean): [number, number] {
  const surface = footprintZ(stack, bottom);
  const standoff = body.standoff / 1e6;
  const thickness = ((body.height - body.standoff) / 1e6) * body.scale[2];
  const zOffset = body.offset[2];
  if (!bottom) {
    const zBot = surface + standoff + zOffset;
    return [zBot, zBot + thickness];
  }
  const zTop = surface - standoff - zOffset;
  return [zTop - thickness, zTop];
}

/** How far a pin sticks out of the far side of the board: `protrusion = 1.0f * pcbIUScale.IU_PER_MM * biuTo3d`, one millimetre. */
export const PIN_PROTRUSION_MM = 1;

/**
 * The heights of the body's pins ("Create metallic pin extrusions for THT pads / Start from opposite board side to standoff height"): from a millimetre past the far side's mounting
 * surface to the standoff -- only when there is a standoff (`if( standoff3d > 0.0f )`), else null.
 */
export function pinZRange(body: Pick<ExtrudedBodyInfo, "standoff">, stack: Stack, bottom: boolean): [number, number] | null {
  const standoff = body.standoff / 1e6;
  if (!(standoff > 0)) return null;
  const surface = footprintZ(stack, bottom);
  const opposite = footprintZ(stack, !bottom);
  return bottom ? [surface - standoff, opposite + PIN_PROTRUSION_MM] : [opposite - PIN_PROTRUSION_MM, surface + standoff];
}

export type ExtrusionMaterial = ExtrudedBodyInfo["material"];

/** `EXTRUDED_3D_BODY::GetDefaultColor`: a body's colour when it has none of its own (the server sends the colour to draw; this is the box's swatch when the material changes). */
export function defaultBodyColor(material: ExtrusionMaterial): [number, number, number, number] {
  switch (material) {
    case "matte":
      return [0.4, 0.4, 0.4, 1];
    case "metal":
      return [0.7, 0.7, 0.7, 1];
    case "copper":
      return [0.72, 0.45, 0.2, 1];
    default:
      return [0.2, 0.2, 0.2, 1];
  }
}

const rgb = (r: number, g: number, b: number): Rgb => ({ r, g, b });
const scaled = (c: Rgb, k: number): Rgb => rgb(c.r * k, c.g * k, c.b * k);
const plus = (c: Rgb, k: number): Rgb => rgb(c.r + k, c.g + k, c.b + k);

/**
 * The material the body is drawn with (`renderExtrudedBodies`' draw: `mat.m_Diffuse = diffuse`, `GetMaterialProps( body->m_material, diffuse )` for the ambient, specular and
 * shininess, no emission, `m_Transparency = 1 - c.a`). `GetMaterialProps`: plastic `{ diffuse * 0.1, 0.4 grey, 0.3 * 128 }`, matte `{ diffuse * 0.1, 0.05 grey, 0.05 * 128 }`,
 * metal `{ diffuse * 0.15, diffuse * 0.5 + 0.4, 0.5 * 128 }`, copper `{ diffuse * 0.1, diffuse * 0.75 + 0.25, 0.4 * 128 }`. The shininess is as KiCad sets it; `OglSetMaterial`
 * makes 128 of every one of them (`128 * min( m_Shininess, 1 )`).
 */
export function bodyMaterial3d(material: ExtrusionMaterial, color: readonly [number, number, number, number]): Material3d {
  const diffuse = rgb(color[0], color[1], color[2]);
  const transparency = 1 - color[3];
  const base = { diffuse, emissive: rgb(0, 0, 0), transparency };
  switch (material) {
    case "matte":
      return { ...base, ambient: scaled(diffuse, 0.1), specular: rgb(0.05, 0.05, 0.05), shininess: 0.05 * 128 };
    case "metal":
      return { ...base, ambient: scaled(diffuse, 0.15), specular: plus(scaled(diffuse, 0.5), 0.4), shininess: 0.5 * 128 };
    case "copper":
      return { ...base, ambient: scaled(diffuse, 0.1), specular: plus(scaled(diffuse, 0.75), 0.25), shininess: 0.4 * 128 };
    default:
      return { ...base, ambient: scaled(diffuse, 0.1), specular: rgb(0.4, 0.4, 0.4), shininess: 0.3 * 128 };
  }
}

/**
 * The pins' material: `m_materials.m_Copper` (`setupMaterials`: ambient the copper colour times 0.1, no emission) with `m_Diffuse = 0.75 grey`, `m_Specular = 0.85 grey`,
 * `m_Shininess = 0.6 * 128`, opaque. `copper` is the copper colour (`m_CopperColor`).
 */
export function pinMaterial3d(copper: Rgb): Material3d {
  return { ambient: scaled(copper, 0.1), emissive: rgb(0, 0, 0), diffuse: rgb(0.75, 0.75, 0.75), specular: rgb(0.85, 0.85, 0.85), shininess: 0.6 * 128, transparency: 0 };
}

/** Whether the body is drawn: `if( !body->m_show || !m_boardAdapter.IsFootprintShown( fp ) ) continue;` (the second half is the part's own visibility, the caller's), and it has an outline. */
export function bodyDrawn(body: Pick<ExtrudedBodyInfo, "show" | "outline">): boolean {
  return body.show && body.outline.some((poly) => (poly[0]?.length ?? 0) >= 3);
}
