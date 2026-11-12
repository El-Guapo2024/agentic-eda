// The materials of the board's own layers in KiCad's OpenGL 3D view: the ambient, diffuse, specular and emissive colours, the shininess and the transparency each layer is
// drawn with.
//
// Ported from KiCad 8303b2ad 3d-viewer/3d_rendering/opengl/render_3d_opengl.cpp: `RENDER_3D_OPENGL::setupMaterials` (copper, non-plated copper, paste, the two
// silkscreens, the solder mask's fixed part, the epoxy board), `setLayerMaterial` (the solder mask from its colour: `TransparencyControl`, a darker mask more opaque and
// shinier; the paste and the silkscreens; the "plastic" of the adhesive, user and technical layers; any copper layer, in the PCB editor's colour with "Use PCB editor copper
// colors"), `renderBoardBody` (the body's diffuse and transparency from its colour), `TransparencyControl`; 3d-viewer/3d_math.h `mapf`; and
// 3d-viewer/common_ogl/ogl_utils.cpp `OglSetMaterial` (`shininess = 128 * min( m_Shininess, 1 )`, the value OpenGL is given).
//
// Pure: no three.js. components/viewer3d/scene.ts makes the three.js materials from these.
import type { Rgba } from "./appearance3d";

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

/** `SMATERIAL` as `OglSetMaterial` hands it to OpenGL. */
export interface Material3d {
  ambient: Rgb;
  diffuse: Rgb;
  specular: Rgb;
  emissive: Rgb;
  /** `m_Shininess` as the material sets it (often `x * 128`). */
  shininess: number;
  /** `m_Transparency`: 0 opaque. */
  transparency: number;
}

const rgb = (r: number, g: number, b: number): Rgb => ({ r, g, b });
const scale = (c: Rgb, k: number): Rgb => rgb(c.r * k, c.g * k, c.b * k);
const BLACK = rgb(0, 0, 0);
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** 3d_math.h `mapf`: `x` clamped to [in_min, in_max], mapped onto [out_min, out_max]. */
export function mapf(x: number, inMin: number, inMax: number, outMin: number, outMax: number): number {
  const v = clamp(x, inMin, inMax);
  return ((v - inMin) * (outMax - outMin)) / (inMax - inMin) + outMin;
}

/** `TransparencyControl( aGrayColorValue, aTransparency )`: `clamp( gray * ( 1 - 1.05 * ( 1 - t )^3 ) + t^3, 0, 1 )`. */
export function transparencyControl(gray: number, transparency: number): number {
  const aaa = transparency * transparency * transparency;
  let ca = 1 - transparency;
  ca = 1 - 1.05 * ca * ca * ca;
  return clamp(gray * ca + aaa, 0, 1);
}

/** `OglSetMaterial`: the shininess OpenGL is given, `128 * ( m_Shininess > 1 ? 1 : m_Shininess )` -- every board layer sets more than 1, so it is 128 for each. */
export function glShininess(m: Pick<Material3d, "shininess">): number {
  return 128 * (m.shininess > 1 ? 1 : m.shininess);
}

/** `m_materials.m_Copper` of `setupMaterials` for the copper colour `m_CopperColor`, its diffuse that colour (`setLayerMaterial`'s default branch). */
export function copperMaterial3d(copper: Rgb): Material3d {
  // "This guess the material type(ex: copper vs gold) to determine the shininess factor between 0.1 and 0.4"
  const shininessFactor = 0.4 - mapf(Math.abs(copper.r - copper.g), 0.15, 1.0, 0.0, 0.3);
  return {
    ambient: scale(copper, 0.1),
    diffuse: rgb(copper.r, copper.g, copper.b),
    specular: rgb(copper.r * 0.75 + 0.25, copper.g * 0.75 + 0.25, copper.b * 0.75 + 0.25),
    emissive: BLACK,
    shininess: shininessFactor * 128,
    transparency: 0,
  };
}

/**
 * The material of copper layer `layer` (`setLayerMaterial`): with "Use PCB editor copper colors" its diffuse is the layer's colour in the PCB editor
 * (`m_BoardEditorColors[aLayerID]`), the rest the copper material's of `m_CopperColor`; otherwise the copper material of `m_CopperColor`.
 */
export function copperLayerMaterial3d(copperColor: Rgb, editorColor: Rgb | null): Material3d {
  const m = copperMaterial3d(copperColor);
  return editorColor ? { ...m, diffuse: rgb(editorColor.r, editorColor.g, editorColor.b) } : m;
}

/** `m_materials.m_NonPlatedCopper` (`setupMaterials`: "Non plated copper (raw copper)"), what `setCopperMaterial` draws every copper layer with when "Differentiate plated copper" is on. */
export function nonPlatedCopperMaterial3d(): Material3d {
  return { ambient: rgb(0.191, 0.073, 0.022), diffuse: rgb(184 / 255, 115 / 255, 50 / 255), specular: rgb(0.256, 0.137, 0.086), emissive: BLACK, shininess: 0.1 * 128, transparency: 0 };
}

/** `m_materials.m_Paste` (`setupMaterials`, `setLayerMaterial`'s `F_Paste`/`B_Paste`): ambient the paste colour, specular its square. */
export function pasteMaterial3d(paste: Rgb): Material3d {
  return { ambient: rgb(paste.r, paste.g, paste.b), diffuse: rgb(paste.r, paste.g, paste.b), specular: rgb(paste.r * paste.r, paste.g * paste.g, paste.b * paste.b), emissive: BLACK, shininess: 0.1 * 128, transparency: 0 };
}

/** `m_materials.m_SilkSTop` / `m_SilkSBot`: ambient the silkscreen colour, specular its square plus 0.1. */
export function silkMaterial3d(silk: Rgb): Material3d {
  return {
    ambient: rgb(silk.r, silk.g, silk.b),
    diffuse: rgb(silk.r, silk.g, silk.b),
    specular: rgb(silk.r * silk.r + 0.1, silk.g * silk.g + 0.1, silk.b * silk.b + 0.1),
    emissive: BLACK,
    shininess: 0.078125 * 128,
    transparency: 0,
  };
}

/**
 * The solder mask of colour `mask` (`setLayerMaterial`'s `F_Mask`/`B_Mask`): "Use TransparencyControl to make darker colors more opaque, preventing copper show-through on
 * dark solder masks"; ambient 0.3 of the colour; "Darker solder masks need a higher specular floor" (`max( diffuse^2, 0.30 )`); "Darker colors get higher shininess"
 * (0.85 * 128 to 512 by darkness).
 */
export function maskMaterial3d(mask: Rgba): Material3d {
  const diffuse = rgb(mask.r, mask.g, mask.b);
  const gray = (mask.r + mask.g + mask.b) / 3;
  const baseTransparency = 1 - mask.a;
  const minShininess = 0.85 * 128;
  const maxShininess = 512;
  return {
    ambient: scale(diffuse, 0.3),
    diffuse,
    specular: rgb(Math.max(mask.r * mask.r, 0.3), Math.max(mask.g * mask.g, 0.3), Math.max(mask.b * mask.b, 0.3)),
    emissive: BLACK,
    shininess: minShininess + (maxShininess - minShininess) * (1 - gray),
    transparency: transparencyControl(gray, baseTransparency),
  };
}

/** `m_materials.m_EpoxyBoard` (`setupMaterials`) with `renderBoardBody`'s diffuse and transparency from the body colour (`m_BoardBodyColor`, its alpha the opacity). */
export function boardMaterial3d(body: Rgba): Material3d {
  return { ambient: rgb(117 / 255, 97 / 255, 47 / 255), diffuse: rgb(body.r, body.g, body.b), specular: rgb(18 / 255, 3 / 255, 20 / 255), emissive: BLACK, shininess: 0.1 * 128, transparency: 1 - body.a };
}

/** `m_materials.m_Plastic` of `setLayerMaterial` (the adhesive, the user drawings, comments and ECO layers, User.1 .. User.45): ambient 0.05 and specular 0.7 of the colour. */
export function plasticMaterial3d(color: Rgb): Material3d {
  return { ambient: scale(color, 0.05), diffuse: rgb(color.r, color.g, color.b), specular: scale(color, 0.7), emissive: BLACK, shininess: 0.078125 * 128, transparency: 0 };
}

/**
 * Whether the inner copper layers are drawn (`RENDER_3D_OPENGL::Redraw`): "Do not show inner layers when it is displaying the board and board body is opaque enough" --
 * not while the Board Body row is on and the body's opacity is above 0.8 (`opacity_min`).
 */
export function innerLayersDrawn(boardShown: boolean, body: Pick<Rgba, "a">): boolean {
  return !(boardShown && body.a > 0.8);
}
