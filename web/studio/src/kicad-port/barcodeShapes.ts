// The board's barcodes (`PCB_BARCODE`, GAPS.md item 24, package P16 step 4): what their shape is and what the pickers make of it.
//
// KiCad works the modules of a barcode out with zint; this studio has no zint, and the file has only the parameters, so the shape is what kicad-cli plots of the barcode
// (`GET /api/barcode_shape?id=..`, crates/cli/src/barcode_api.rs): filled polygons in micrometres with the barcode's centre at the origin, already turned and mirrored as the barcode is.
// This file holds them (`setBarcodeShape`) and the geometry that needs them or the barcode's own box: its bounding box, the boxes a click hits (`PCB_BARCODE::HitTest`: the symbol's
// box and the text's, turned with the barcode), the placeholder drawn until the shape arrives.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`); the fetching is components/canvas/barcodeCache.ts.
import type { BarcodeEcc, BarcodeKind, BoardBarcode, CmdBarcode } from "../api/pcbItemTypes";

/** One path of the plot (it is filled even-odd) as its contours, each a closed ring of `[x, y]`, micrometres, relative to the centre of the barcode. */
export type BarcodePath = [number, number][][];

export type BarcodeShapeState = { status: "ready"; paths: BarcodePath[] } | { status: "pending" } | { status: "failed"; error: string };

export type Box = [number, number, number, number];

/** What the shape of a barcode depends on and nothing it does not (not its place): the cache key, as `barcode_api::key` is the server's. */
export function barcodeShapeKey(b: Pick<BoardBarcode, "id" | "kind" | "ecc" | "layer" | "w" | "h" | "text_height" | "show_text" | "knockout" | "margin_x" | "margin_y" | "angle" | "text" | "shown">): string {
  return [b.id, b.kind, b.ecc, b.layer, b.w, b.h, b.text_height, b.show_text, b.knockout, b.margin_x, b.margin_y, b.angle, b.shown ?? b.text].join("|");
}

const shapes = new Map<string, BarcodeShapeState>();

/** The shape the server answered for the barcode's parameters, if it has. */
export function barcodeShape(key: string): BarcodeShapeState | undefined {
  return shapes.get(key);
}

export function setBarcodeShape(key: string, state: BarcodeShapeState): void {
  shapes.set(key, state);
  // the most recent last; the least recently set go first
  if (shapes.size > 128) {
    const oldest = shapes.keys().next().value;
    if (oldest !== undefined) shapes.delete(oldest);
  }
}

/** The shape when it has arrived. */
export function readyPaths(b: Parameters<typeof barcodeShapeKey>[0]): BarcodePath[] | null {
  const s = shapes.get(barcodeShapeKey(b));
  return s?.status === "ready" ? s.paths : null;
}

/** `RotatePoint( p, angle )` of KiCad's y-down frame, `angle` in millidegrees, counter-clockwise on the screen. */
function turn(x: number, y: number, angleMdeg: number): [number, number] {
  const a = (angleMdeg / 1000) * (Math.PI / 180);
  const [s, c] = [Math.sin(a), Math.cos(a)];
  return [y * s + x * c, y * c - x * s];
}

/** A point of the board in the barcode's own frame (its centre at the origin, not turned). */
export function toLocal(b: Pick<BoardBarcode, "x" | "y" | "angle">, x: number, y: number): [number, number] {
  return turn(x - b.x, y - b.y, -b.angle);
}

/** The box of the symbol alone, turned with the barcode: its four corners (`GetBoundingHull`'s first hull). */
export function symbolCorners(b: Pick<BoardBarcode, "x" | "y" | "w" | "h" | "angle">): [number, number][] {
  const [hw, hh] = [b.w / 2, b.h / 2];
  return [[-hw, -hh], [hw, -hh], [hw, hh], [-hw, hh]].map(([x, y]) => {
    const [tx, ty] = turn(x!, y!, b.angle);
    return [b.x + tx, b.y + ty] as [number, number];
  });
}

function boxOf(points: readonly (readonly [number, number])[]): Box {
  let [x0, y0, x1, y1] = [Infinity, Infinity, -Infinity, -Infinity];
  for (const [x, y] of points) {
    x0 = Math.min(x0, x);
    y0 = Math.min(y0, y);
    x1 = Math.max(x1, x);
    y1 = Math.max(y1, y);
  }
  return [x0, y0, x1, y1];
}

/** `PCB_BARCODE::GetBoundingBox` (`m_bbox = m_poly.BBox()`): everything the barcode draws, text and knockout included; the symbol's turned box until the shape is known. */
export function barcodeBounds(b: Parameters<typeof barcodeShapeKey>[0] & { x: number; y: number }): Box {
  const paths = readyPaths(b);
  if (!paths || paths.length === 0) return boxOf(symbolCorners(b));
  const pts: [number, number][] = [];
  for (const path of paths) for (const ring of path) for (const [x, y] of ring) pts.push([b.x + x, b.y + y]);
  return boxOf(pts);
}

/** The boxes a click can hit, in the barcode's frame: the symbol's (`[-w/2, w/2] x [-h/2, h/2]`) and, when the text is shown and the shape is known, the text's -- the polygons under the symbol. */
export function hitRects(b: Parameters<typeof barcodeShapeKey>[0]): Box[] {
  const rects: Box[] = [[-b.w / 2, -b.h / 2, b.w / 2, b.h / 2]];
  const paths = readyPaths(b);
  if (!b.show_text || !paths) return rects;
  // The polygons are turned with the barcode: turn them back, and the ones under the symbol are the text.
  const text: [number, number][] = [];
  for (const path of paths) {
    const local = path.flatMap((ring) => ring.map(([x, y]) => turn(x, y, -b.angle)));
    if (local.length > 0 && Math.min(...local.map((p) => p[1])) >= b.h / 2 - 1) text.push(...local);
  }
  if (text.length > 0) rects.push(boxOf(text));
  return rects;
}

/** How far `(x, y)` is from the barcode (`PCB_BARCODE::HitTest`: inside the hull, which is the symbol's box and the text's); 0 when it hits. */
export function barcodeHitDistance(b: Parameters<typeof barcodeShapeKey>[0] & { x: number; y: number }, x: number, y: number): number {
  const [lx, ly] = toLocal(b, x, y);
  let best = Infinity;
  for (const [x0, y0, x1, y1] of hitRects(b)) best = Math.min(best, Math.hypot(Math.max(x0 - lx, 0, lx - x1), Math.max(y0 - ly, 0, ly - y1)));
  return best;
}

/** The choices of the Barcode Type row and the dialog, in the order of `DIALOG_BARCODE_PROPERTIES` (`m_barcode`) with the names of the property (`BARCODE_T`). */
export const BARCODE_KINDS: { value: BarcodeKind; label: string; property: string }[] = [
  { value: "code39", label: "Code 39", property: "CODE_39" },
  { value: "code128", label: "Code 128", property: "CODE_128" },
  { value: "datamatrix", label: "Data Matrix", property: "DATA_MATRIX" },
  { value: "qr", label: "QR Code", property: "QR_CODE" },
  { value: "microqr", label: "Micro QR Code", property: "MICRO_QR_CODE" },
];

/** The kinds that have an error correction level (`m_errorCorrection->Enable( m_barcode->GetSelection() >= QR_CODE )`). */
export function hasErrorCorrection(kind: BarcodeKind): boolean {
  return kind === "qr" || kind === "microqr";
}

/** The levels a kind offers: Micro QR has no High (`PCB_BARCODE_DESC`'s choices function, `OnUpdateUI`). */
export function eccChoices(kind: BarcodeKind): { value: BarcodeEcc; label: string }[] {
  const all: { value: BarcodeEcc; label: string }[] = [
    { value: "L", label: "L (Low)" },
    { value: "M", label: "M (Medium)" },
    { value: "Q", label: "Q (Quartile)" },
    { value: "H", label: "H (High)" },
  ];
  return kind === "microqr" ? all.slice(0, 3) : all;
}

/** `PCB_BARCODE::SetErrorCorrection`: a Micro QR code takes Q for High. */
export function clampEcc(kind: BarcodeKind, ecc: BarcodeEcc): BarcodeEcc {
  return kind === "microqr" && ecc === "H" ? "Q" : ecc;
}

/** What the picker menu says of a barcode (`PCB_BARCODE::GetItemDescription`: "Barcode '%s' on %s"). */
export function barcodeLabel(b: Pick<BoardBarcode, "text" | "shown" | "layer">): string {
  const text = b.shown ?? b.text;
  return `Barcode '${text.length > 24 ? `${text.slice(0, 23)}...` : text}' on ${b.layer}`;
}

/** KiCad's own messages when the code cannot be made (`DIALOG_BARCODE_PROPERTIES::TransferDataFromWindow`, `PCB_BARCODE::ComputeBarcode`). */
export function barcodeProblem(kind: BarcodeKind, text: string): string {
  const ascii = [...text].every((c) => c.charCodeAt(0) < 128);
  if (!ascii && kind !== "qr" && kind !== "datamatrix") return "This barcode type does not support international characters. Use QR Code or Data Matrix instead.";
  return "The text cannot be encoded as this kind of barcode.";
}

/** The barcode as the verbs take it (`edit_barcode`'s replacement, the IR's own field names). */
export function barcodeToCmd(b: BoardBarcode): CmdBarcode {
  return {
    layer: b.layer,
    at: { x: b.x, y: b.y },
    angle: b.angle,
    width_nm: b.w,
    height_nm: b.h,
    text: b.text,
    text_height_nm: b.text_height,
    kind: b.kind,
    ecc: b.ecc,
    show_text: b.show_text,
    knockout: b.knockout,
    margin_x_nm: b.margin_x,
    margin_y_nm: b.margin_y,
  };
}

/** A barcode as the Add Barcode tool starts one (`PCB_BARCODE( parent )`: 40 mm square, a QR code of level L, the text shown, no knockout; `DRAWING_TOOL::DrawBarcode` sets the layer, the place and the layer's text size). */
export function newBarcode(layer: string, at: { x: number; y: number }, textHeightNm: number): CmdBarcode {
  return { layer, at: { x: at.x, y: at.y }, angle: 0, width_nm: 40_000, height_nm: 40_000, text: "", text_height_nm: textHeightNm, kind: "qr", ecc: "L", show_text: true, knockout: false, margin_x_nm: 0, margin_y_nm: 0 };
}

/** `transferDataToBarcode`'s `SetMargin` and the knockout's minimum (`AssembleBarcode`: at least a tenth of the smaller side, rounded up to 0.1 mm). */
export function knockoutMinMarginNm(w: number, h: number): number {
  const tenth = Math.ceil(Math.min(w, h) / 10);
  return Math.ceil(tenth / 100) * 100;
}
