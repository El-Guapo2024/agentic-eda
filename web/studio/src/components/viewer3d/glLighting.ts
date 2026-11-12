// KiCad's OpenGL 3D view lights its scene with fixed-function OpenGL, and so does this: a material of the 3D view computes its colour per vertex with OpenGL's lighting
// equation (Gouraud shading, `glShadeModel( GL_SMOOTH )`), from KiCad's three lights and the material's ambient, diffuse, specular and emissive colours and shininess.
//
// Ported from KiCad 8303b2ad 3d-viewer/3d_rendering/opengl/render_3d_opengl.cpp:
//   init_lights()      LIGHT0 "front": ambient 0.084, diffuse 0.3, specular 0.5, a point light. LIGHT1 "top" and LIGHT2 "bottom": ambient 0.084, diffuse and specular
//                      0.7, directional, `SphericalToCartesian( pi * 0.03, pi * 0.25 )` and the same with z negated. `GL_LIGHT_MODEL_AMBIENT` (0, 0, 0, 1),
//                      `GL_LIGHT_MODEL_TWO_SIDE` false, the local viewer off (OpenGL's default). init_lights() runs once, from initializeOpenGL(), before any
//                      matrix is loaded: OpenGL keeps a light's position in eye coordinates as the modelview matrix was then (the identity), so LIGHT1 and LIGHT2
//                      stay where they are relative to the camera, LIGHT1 from just above the eye and LIGHT2 from behind the scene.
//   Redraw()           LIGHT0's position, after the view matrix is loaded (so in world coordinates): the camera's x and y, and "a minimum Z so the diffuse factor
//                      will not drop" -- `max( z, 0.5 ) + z * z` above the board, `min( z, -0.5 ) - z * z` below it -- in KiCad's 3D units (the board's
//                      longest side is 12.8 of them).
// and 3d-viewer/common_ogl/ogl_utils.cpp `OglSetMaterial` (the diffuse's alpha the opacity, `128 * min( m_Shininess, 1 )`).
//
// OpenGL's equation, per light: `ambient_l * ambient_m + max( N.L, 0 ) * diffuse_l * diffuse_m + ( N.L > 0 ? max( N.H, 0 )^shininess : 0 ) * specular_l * specular_m`,
// `H = normalize( L + ( 0, 0, 1 ) )` (the viewer at infinity), plus the material's emission, clamped to [0, 1]; no attenuation (OpenGL's default constant 1). It works on
// the colours as given and writes them to the frame as they are: no colour management, so the shader takes three.js's linear colours back to the file's values and
// writes its result without three.js's output conversion. A back face is lit with its front normal (`GL_LIGHT_MODEL_TWO_SIDE` false).
//
// The materials stay three.js's `MeshPhongMaterial` (for the stencil, the blending, the polygon offset and the selection colour their users set): `kicadLit` replaces
// their lighting in the shader. Their colour is `material.color`, their specular `material.specular`, their emission `material.emissive`, their shininess
// `material.shininess` (already OpenGL's exponent), their alpha `material.opacity`; the ambient colour, which three.js has not, is `userData.kicadAmbient`.
import * as THREE from "three";

/** `SphericalToCartesian( glm::pi<float>() * 0.03f, glm::pi<float>() * 0.25f )`: LIGHT1's direction in eye coordinates; LIGHT2's is the same with z negated. */
const LIGHT1_EYE = (() => {
  const inc = Math.PI * 0.03;
  const az = Math.PI * 0.25;
  return new THREE.Vector3(Math.sin(inc) * Math.cos(az), Math.sin(inc) * Math.sin(az), Math.cos(inc));
})();

/** The lights' uniforms, shared by every material: LIGHT0's position in eye coordinates is set before each frame ([`setKicadHeadlight`]). */
const lightUniforms = {
  kLight0Pos: { value: new THREE.Vector3(0, 0, 1) },
  kLight1Dir: { value: LIGHT1_EYE.clone() },
  kLight2Dir: { value: new THREE.Vector3(LIGHT1_EYE.x, LIGHT1_EYE.y, -LIGHT1_EYE.z) },
};

/**
 * `Redraw()`'s headlight: the camera at `cameraWorld` (this scene's world: millimetres, y up from the board's middle), KiCad's 3D unit `mmPerUnit` millimetres. Call it
 * with the camera that is about to draw (its `matrixWorldInverse` current).
 */
export function setKicadHeadlight(camera: THREE.Camera, cameraWorld: THREE.Vector3, mmPerUnit: number): void {
  const z = cameraWorld.y / mmPerUnit;
  const zpos = z > 0 ? Math.max(z, 0.5) + z * z : Math.min(z, -0.5) - z * z;
  lightUniforms.kLight0Pos.value.set(cameraWorld.x, zpos * mmPerUnit, cameraWorld.z).applyMatrix4(camera.matrixWorldInverse);
}

const VERTEX_DECLS = `
uniform vec3 kLight0Pos;
uniform vec3 kLight1Dir;
uniform vec3 kLight2Dir;
uniform vec3 kAmbient;
uniform vec3 kDiffuse;
uniform vec3 kSpecular;
uniform vec3 kEmissive;
uniform float kShininess;
varying vec3 vKicadColor;
vec3 kRaw( vec3 c ) {
  return mix( pow( max( c, vec3( 0.0 ) ), vec3( 1.0 / 2.4 ) ) * 1.055 - vec3( 0.055 ), c * 12.92, vec3( lessThanEqual( c, vec3( 0.0031308 ) ) ) );
}
vec3 kLight( vec3 L, vec3 N, vec3 ka, vec3 kd, vec3 ks, float la, float ld, float ls ) {
  vec3 c = la * ka;
  float nl = dot( N, L );
  if( nl > 0.0 ) {
    c += nl * ld * kd;
    vec3 H = normalize( L + vec3( 0.0, 0.0, 1.0 ) );
    float nh = max( dot( N, H ), 0.0 );
    c += ( kShininess > 0.0 ? pow( nh, kShininess ) : 1.0 ) * ls * ks;
  }
  return c;
}
`;

const VERTEX_LIGHTING = `
{
  vec3 kN = normalize( transformedNormal );
  vec3 ka = kRaw( kAmbient );
  vec3 kd = kRaw( kDiffuse );
  vec3 ks = kRaw( kSpecular );
  vec3 c = kRaw( kEmissive );
  c += kLight( normalize( kLight0Pos - mvPosition.xyz ), kN, ka, kd, ks, 0.084, 0.3, 0.5 );
  c += kLight( normalize( kLight1Dir ), kN, ka, kd, ks, 0.084, 0.7, 0.7 );
  c += kLight( normalize( kLight2Dir ), kN, ka, kd, ks, 0.084, 0.7, 0.7 );
  vKicadColor = clamp( c, 0.0, 1.0 );
}
`;

/** Lights `mat` as KiCad's OpenGL view lights it (the header); `ambient` is its `GL_AMBIENT` colour (three.js's linear values, as `mat.color` holds them). */
export function kicadLit<T extends THREE.MeshPhongMaterial>(mat: T, ambient: THREE.Color): T {
  mat.userData.kicadAmbient = ambient;
  mat.onBeforeCompile = (shader) => {
    Object.assign(shader.uniforms, lightUniforms, {
      kAmbient: { get value() { return mat.userData.kicadAmbient as THREE.Color; } },
      kDiffuse: { get value() { return mat.color; } },
      kSpecular: { get value() { return mat.specular; } },
      kEmissive: { get value() { return mat.emissive; } },
      kShininess: { get value() { return mat.shininess; } },
    });
    shader.vertexShader = shader.vertexShader.replace("#include <common>", `#include <common>\n${VERTEX_DECLS}`).replace("#include <fog_vertex>", `#include <fog_vertex>\n${VERTEX_LIGHTING}`);
    shader.fragmentShader = shader.fragmentShader
      .replace("#include <common>", "#include <common>\nvarying vec3 vKicadColor;")
      .replace("#include <opaque_fragment>", "gl_FragColor = vec4( vKicadColor, diffuseColor.a );")
      .replace("#include <tonemapping_fragment>", "")
      .replace("#include <colorspace_fragment>", "");
  };
  mat.customProgramCacheKey = () => "kicad-gl-lighting";
  return mat;
}

/** `k` times a colour's own values (as the file has them, not three.js's linear ones): VRML's ambient, `ambientIntensity * diffuseColor`. */
export function scaledColor(c: THREE.Color, k: number): THREE.Color {
  const raw = { r: 0, g: 0, b: 0 };
  c.getRGB(raw, THREE.SRGBColorSpace);
  return new THREE.Color().setRGB(raw.r * k, raw.g * k, raw.b * k, THREE.SRGBColorSpace);
}

/** Whether a material is lit by [`kicadLit`] (for the tests). */
export const isKicadLit = (m: THREE.Material): boolean => m instanceof THREE.MeshPhongMaterial && m.userData.kicadAmbient instanceof THREE.Color;

/** A clone of a lit material keeps its lighting (three.js's `clone` drops `onBeforeCompile`'s closure over the original). */
export function cloneLit<T extends THREE.MeshPhongMaterial>(mat: T): T {
  const c = mat.clone() as T;
  if (mat.userData.kicadAmbient instanceof THREE.Color) kicadLit(c, (mat.userData.kicadAmbient as THREE.Color).clone());
  return c;
}
