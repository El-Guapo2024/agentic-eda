// The 3D Navigator drawn with three.js (`SPHERES_GIZMO::render3dSpheresGizmo`): the six spheres on the axes, each translucent with a ring round it facing the camera,
// the letters X, Y and Z on the positive ones, and lines from the centre to those -- in a square of its own in the view's bottom-left corner, turned with the camera.
// The numbers are kicad-port/navigator3d.ts's.
import * as THREE from "three";
import { ARROW_SIZE, CAMERA_DISTANCE, FOV_DEG, GIZMO_POS, SPHERES, SPHERE_RADIUS, gizmoNdc, gizmoSize, pickSphere, toScene, type GizmoSphere } from "../../kicad-port/navigator3d";

const colorOf = (c: { x: number; y: number; z: number }) => new THREE.Color(c.x, c.y, c.z);

/** The letter's strokes (`drawX`, `drawY`, `drawZ`) in the ring's plane, `size` across: pairs of points. */
function letterSegments(label: string, size: number): THREE.Vector3[] {
  const h = size * 0.5;
  const p = (x: number, y: number) => new THREE.Vector3(x, y, 0.02);
  switch (label) {
    case "X":
      return [p(h, -h), p(-h, h), p(h, h), p(-h, -h)];
    case "Y":
      return [p(-h, h), p(0, 0), p(h, h), p(0, 0), p(0, 0), p(0, -h)];
    case "Z":
      return [p(-h, h), p(h, h), p(h, h), p(-h, -h), p(-h, -h), p(h, -h)];
    default:
      return [];
  }
}

export class Navigator3D {
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(FOV_DEG, 1, 0.001, 2 * 8);
  /** Each sphere's ring and letter, turned to face the camera every frame (`drawBillboardCircle` uses the camera's right and up vectors). */
  private readonly billboards: THREE.Object3D[] = [];
  private readonly owned: Array<THREE.BufferGeometry | THREE.Material> = [];

  constructor() {
    // `setGizmoMaterial`: the colour is the material (GL_COLOR_MATERIAL), a faint specular (0.1, shininess 96).
    this.scene.add(new THREE.AmbientLight(0xffffff, 0.5));
    const light = new THREE.DirectionalLight(0xffffff, 1.2);
    light.position.set(0, 0, 1);
    this.camera.add(light);
    this.scene.add(this.camera);
    const sphereGeo = new THREE.SphereGeometry(SPHERE_RADIUS, 32, 32);
    // The ring: `thickness = aRadius * 0.4f`, between radius -/+ half of it.
    const ringGeo = new THREE.RingGeometry(SPHERE_RADIUS * 0.8, SPHERE_RADIUS * 1.2, 64);
    this.owned.push(sphereGeo, ringGeo);
    for (const s of SPHERES) {
      const at = toScene(s.position);
      const sphereMat = new THREE.MeshPhongMaterial({ color: colorOf(s.color), transparent: true, opacity: 0.3, specular: new THREE.Color(0.1, 0.1, 0.1), shininess: 96, depthWrite: false });
      const sphere = new THREE.Mesh(sphereGeo, sphereMat);
      sphere.position.set(at.x, at.y, at.z);
      sphere.userData.sphere = s.id;
      sphere.name = `navigator-${s.id}`;
      this.scene.add(sphere);
      const billboard = new THREE.Group();
      billboard.position.set(at.x, at.y, at.z);
      const ringMat = new THREE.MeshBasicMaterial({ color: colorOf(s.color), side: THREE.DoubleSide });
      billboard.add(new THREE.Mesh(ringGeo, ringMat));
      this.owned.push(sphereMat, ringMat);
      const letter = letterSegments(s.label, 0.3);
      if (letter.length > 0) {
        const geo = new THREE.BufferGeometry().setFromPoints(letter);
        const mat = new THREE.LineBasicMaterial({ color: 0x000000, depthTest: false });
        const lines = new THREE.LineSegments(geo, mat);
        lines.renderOrder = 2;
        billboard.add(lines);
        this.owned.push(geo, mat);
      }
      this.billboards.push(billboard);
      this.scene.add(billboard);
    }
    // "Draw lines only to the positive axis spheres": X+ red, Y+ green, Z+ blue.
    const axes: Array<[{ x: number; y: number; z: number }, number]> = [
      [{ x: ARROW_SIZE, y: 0, z: 0 }, 0xe60000],
      [{ x: 0, y: ARROW_SIZE, z: 0 }, 0x00e600],
      [{ x: 0, y: 0, z: ARROW_SIZE }, 0x0000e6],
    ];
    for (const [end, color] of axes) {
      const e = toScene(end);
      const geo = new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(0, 0, 0), new THREE.Vector3(e.x, e.y, e.z)]);
      const mat = new THREE.LineBasicMaterial({ color });
      this.scene.add(new THREE.Line(geo, mat));
      this.owned.push(geo, mat);
    }
  }

  /** The gizmo camera for the main camera's orientation: `ViewMatrix = Translate( 0, 0, -m_arrowSize * 2.75 ) * aCameraRotationMatrix`. */
  private aim(orientation: THREE.Quaternion): void {
    this.camera.quaternion.copy(orientation);
    this.camera.position.set(0, 0, CAMERA_DISTANCE).applyQuaternion(orientation);
    this.camera.updateMatrixWorld();
    for (const b of this.billboards) b.quaternion.copy(orientation);
  }

  /** Draws the gizmo over what `renderer` has drawn, in its corner of a view `width` x `height` CSS pixels, its depth cleared (`glClear( GL_DEPTH_BUFFER_BIT )`). */
  render(renderer: THREE.WebGLRenderer, orientation: THREE.Quaternion, width: number, height: number): void {
    const size = gizmoSize(height);
    if (size <= 0) return;
    this.aim(orientation);
    const autoClear = renderer.autoClear;
    renderer.autoClear = false;
    renderer.setViewport(GIZMO_POS, GIZMO_POS, size, size);
    renderer.setScissor(GIZMO_POS, GIZMO_POS, size, size);
    renderer.setScissorTest(true);
    renderer.clearDepth();
    renderer.render(this.scene, this.camera);
    renderer.setScissorTest(false);
    renderer.setViewport(0, 0, width, height);
    renderer.autoClear = autoClear;
  }

  /** `OnLeftUp` -> `handleMouseInput` -> `updateSelection`: the sphere under a click at `(x, y)` (pixels from the view's top left), or null when the click is not on one. */
  pick(x: number, y: number, height: number, orientation: THREE.Quaternion): GizmoSphere | null {
    const ndc = gizmoNdc(x, y, height);
    if (!ndc) return null;
    this.aim(orientation);
    const ray = new THREE.Raycaster();
    ray.setFromCamera(new THREE.Vector2(ndc.x, ndc.y), this.camera);
    const o = ray.ray.origin;
    const d = ray.ray.direction;
    return pickSphere({ x: o.x, y: o.y, z: o.z }, { x: d.x, y: d.y, z: d.z }, (s) => toScene(s.position));
  }

  dispose(): void {
    for (const o of this.owned) o.dispose();
  }
}
