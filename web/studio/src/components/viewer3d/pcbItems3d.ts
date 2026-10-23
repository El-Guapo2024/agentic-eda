// The board items GAPS.md item 24 (package P16) added, in the 3D view: a text box is drawn as KiCad's 3D viewer draws it -- on the layer it is on, in that layer's ink: the silkscreen,
// an adhesive layer or a user layer (`BOARD_ADAPTER::addShape( const PCB_TEXTBOX* )`, create_layer_items.cpp). Like the free texts of scene.ts it is a texture on a flat plane: the box
// is painted by the 2D canvas's own painter (components/canvas/pcbItemsPaint.ts, so the two views cannot disagree about where a line of its text goes) into an offscreen canvas of its own
// extent, and a knockout box has its text cut out of the fill.
import * as THREE from "three";
import type { BoardTextBox } from "../../api/pcbItemTypes";
import { textBoxBounds } from "../../kicad-port/pcbTextBox";
import { drawTextBox } from "../canvas/pcbItemsPaint";

/** Texture resolution, pixels per mm of the box (scene.ts's text uses 48 per mm of text height), and the most a texture's side may be. */
const PX_PER_MM = 24;
const MAX_TEXTURE_PX = 4096;

/** The text box as a plane at height `y`, tinted with `tint` (white when none). Null for a box with no extent. */
export function buildTextBoxMesh(b: BoardTextBox, y: number, tint?: THREE.Color, kind = "silk"): THREE.Mesh | null {
  const [x0, y0, x1, y1] = textBoxBounds(b);
  const [wMm, hMm] = [(x1 - x0) / 1e6, (y1 - y0) / 1e6];
  if (!(wMm > 0 && hMm > 0)) return null;
  const pxPerMm = Math.min(PX_PER_MM, MAX_TEXTURE_PX / Math.max(wMm, hMm));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.ceil(wMm * pxPerMm));
  canvas.height = Math.max(1, Math.ceil(hMm * pxPerMm));
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  // micrometres of the board onto pixels of the texture, the box's top left at the texture's
  const s = pxPerMm / 1e6;
  ctx.setTransform(s, 0, 0, s, -x0 * s, -y0 * s);
  drawTextBox(ctx, { scale: s, x: 0, y: 0 }, b, false, false, { color: "#ffffff", cutOutText: true });
  const texture = new THREE.CanvasTexture(canvas);
  texture.minFilter = THREE.LinearFilter;
  const geometry = new THREE.PlaneGeometry(wMm, hMm);
  // local XY -> world XZ, the picture of the 2D canvas as it is (a back layer's text is mirrored in it, and reads right from below)
  geometry.rotateX(-Math.PI / 2);
  const material = new THREE.MeshBasicMaterial({ map: texture, transparent: true, side: THREE.DoubleSide, depthWrite: false });
  if (tint) material.color.copy(tint);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.position.set((x0 + x1) / 2000, y, (y0 + y1) / 2000);
  mesh.name = `${kind}-text-box`;
  return mesh;
}
