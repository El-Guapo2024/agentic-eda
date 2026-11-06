// A part's Extruded 3D Body in the 3D view, as `RENDER_3D_OPENGL::renderExtrudedBodies` (3d-viewer/3d_rendering/opengl/create_scene.cpp at 8303b2ad) builds it and `render`
// (render_3d_opengl.cpp, "Render extruded 3D bodies") draws it: the outline the server sends (`GetExtrusionOutline`, crates/cli/src/extruded_body_api.rs) moved by the body's own
// transform (`ApplyExtrusionTransform`), filled top and bottom and walled between (`addTopAndBottomTriangles`, `AddToMiddleContours`) from the standoff to the body's height, in
// its material and colour (`GetMaterialProps`); and, when the body stands off the board, its pins (`GetExtrusionPinOutline`: the plated holes at nine tenths of the drill) in
// the pins' metal from a millimetre past the far side up to the standoff. The numbers are kicad-port/extrudedBody3d.ts's.
//
// The scene's frame is scene.ts's: world X = board x, world Y = up, world Z = board y, millimetres. The meshes go in the part's group (partModels.ts), so the part's visibility
// (`IsFootprintShown`) and its hover highlight (`highlight_on_rollover`: `OglSetMaterial( mat, 1.0f, highlight, extSelColor )`) are the models'.
//
// The material is KiCad's `SMATERIAL` the way scene.ts's `kicadMaterial` makes a board layer's (Blinn-Phong with OpenGL's shininess, the specular scaled to OpenGL's
// unnormalised highlight, the material's own ambient colour through a shader hook), repeated here since that one is scene.ts's own. The pins' copper ambient is the default
// theme's copper colour (`builtin_color_themes.h`), not the one the Appearance panel may have set.
import * as THREE from "three";
import type { Part } from "../../api/types";
import { THEME_COLORS } from "../../kicad-port/appearance3d";
import { applyExtrusionTransform, bodyDrawn, bodyMaterial3d, bodyZRange, pinMaterial3d, pinZRange, type Polygon } from "../../kicad-port/extrudedBody3d";
import { glShininess, type Material3d, type Rgb } from "../../kicad-port/material3d";
import type { Stack } from "../../kicad-port/model3d";

/** What was drawn for each part's body, for the test hook `window.__edaExtrudedBodies` (a browser check reads it; a part's entry goes with its meshes). */
export interface DrawnBody {
  /** The heights, scene millimetres (`zBot`, `zTop`), and the box of the outline drawn, board millimetres. */
  low: number;
  high: number;
  box: [number, number, number, number];
  pins: boolean;
  material: string;
  color: string;
}
const drawn = new Map<string, DrawnBody>();
declare global {
  interface Window {
    __edaExtrudedBodies?: Map<string, DrawnBody>;
  }
}
if (typeof window !== "undefined") window.__edaExtrudedBodies = drawn;

export interface BuiltBody {
  object: THREE.Group;
  /** What was made here and is to be disposed with the part. */
  owned: Array<{ dispose(): void }>;
}

const srgb = (c: Rgb): THREE.Color => new THREE.Color().setRGB(c.r, c.g, c.b, THREE.SRGBColorSpace);

/** `OglSetMaterial` of an `SMATERIAL` (see the header). */
function kicadMaterial(m: Material3d): THREE.MeshPhongMaterial {
  const transparent = m.transparency > 0;
  const shininess = glShininess(m);
  const norm = (shininess / 2 + 1) / (4 * Math.PI);
  const mat = new THREE.MeshPhongMaterial({
    color: srgb(m.diffuse),
    specular: srgb({ r: m.specular.r / norm, g: m.specular.g / norm, b: m.specular.b / norm }),
    emissive: srgb(m.emissive),
    shininess,
    transparent,
    opacity: 1 - m.transparency,
    // `GL_FRONT_AND_BACK`
    side: THREE.DoubleSide,
    ...(transparent ? { depthWrite: false } : {}),
  });
  const ambient = srgb(m.ambient);
  mat.onBeforeCompile = (shader) => {
    shader.uniforms.kicadAmbient = { value: ambient };
    shader.fragmentShader = shader.fragmentShader.replace(
      "#include <lights_phong_pars_fragment>",
      `#include <lights_phong_pars_fragment>
uniform vec3 kicadAmbient;
void RE_IndirectDiffuse_KiCadBody( const in vec3 irradiance, const in vec3 geometryPosition, const in vec3 geometryNormal, const in vec3 geometryViewDir, const in vec3 geometryClearcoatNormal, const in BlinnPhongMaterial material, inout ReflectedLight reflectedLight ) {
  reflectedLight.indirectDiffuse += irradiance * BRDF_Lambert( kicadAmbient );
}
#undef RE_IndirectDiffuse
#define RE_IndirectDiffuse RE_IndirectDiffuse_KiCadBody`
    );
  };
  mat.customProgramCacheKey = () => "kicad-extruded-body-material";
  mat.userData.kicad = m;
  return mat;
}

/** The polygons (board nm) as three.js shapes in board millimetres, each with its holes. */
function shapesOf(polys: readonly Polygon[]): THREE.Shape[] {
  const v = ([x, y]: [number, number]) => new THREE.Vector2(x / 1e6, y / 1e6);
  return polys
    .filter((p) => (p[0]?.length ?? 0) >= 3)
    .map((p) => {
      const shape = new THREE.Shape(p[0]!.map(v));
      for (const hole of p.slice(1)) if (hole.length >= 3) shape.holes.push(new THREE.Path(hole.map(v)));
      return shape;
    });
}

/** The polygons filled top and bottom and walled between heights `low` and `high` (scene millimetres): local XY is the board's, local z runs down from `high`. */
function slab(polys: readonly Polygon[], low: number, high: number, material: THREE.Material, name: string): THREE.Mesh | null {
  const shapes = shapesOf(polys);
  if (shapes.length === 0) return null;
  try {
    const geometry = new THREE.ExtrudeGeometry(shapes, { depth: Math.max(high - low, 1e-5), bevelEnabled: false, curveSegments: 1 });
    const mesh = new THREE.Mesh(geometry, material);
    mesh.rotation.x = Math.PI / 2;
    mesh.position.y = high;
    mesh.name = name;
    return mesh;
  } catch (err) {
    console.warn(`Viewer3D: could not build ${name}`, err);
    return null;
  }
}

/** The body of `part` and its pins, or null when it has none to draw (none, hidden, no outline). */
export function buildExtrudedBody(part: Part, stack: Stack): BuiltBody | null {
  const body = part.extruded_body;
  if (!body || !part.at || !bodyDrawn(body)) return null;
  const bottom = part.side === "bottom";
  const group = new THREE.Group();
  group.name = "extruded-body";
  const owned: BuiltBody["owned"] = [];
  const [low, high] = bodyZRange(body, stack, bottom);
  const material = kicadMaterial(bodyMaterial3d(body.material, body.color));
  owned.push(material);
  const outline = applyExtrusionTransform(body.outline, body, body.at);
  const solid = slab(outline, low, high, material, "extruded-body-solid");
  if (solid) {
    group.add(solid);
    owned.push(solid.geometry);
  }
  const pinRange = pinZRange(body, stack, bottom);
  if (pinRange && body.pins.length > 0) {
    const copper = THEME_COLORS.copper_top ?? { r: 0.7, g: 0.61, b: 0, a: 1 };
    const pinMat = kicadMaterial(pinMaterial3d({ r: copper.r, g: copper.g, b: copper.b }));
    owned.push(pinMat);
    const pins = slab(applyExtrusionTransform(body.pins, body, body.at), pinRange[0], pinRange[1], pinMat, "extruded-body-pins");
    if (pins) {
      group.add(pins);
      owned.push(pins.geometry);
    }
  }
  if (group.children.length === 0) {
    for (const o of owned) o.dispose();
    return null;
  }
  const xs = outline.flatMap((p) => p[0] ?? []).map((q) => q[0] / 1e6);
  const ys = outline.flatMap((p) => p[0] ?? []).map((q) => q[1] / 1e6);
  const info: DrawnBody = { low, high, box: [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)], pins: group.children.length > 1, material: body.material, color: `#${material.color.getHexString(THREE.SRGBColorSpace)}` };
  drawn.set(part.ref, info);
  owned.push({ dispose: () => drawn.get(part.ref) === info && drawn.delete(part.ref) });
  group.userData.extrudedBody = info;
  return { object: group, owned };
}
