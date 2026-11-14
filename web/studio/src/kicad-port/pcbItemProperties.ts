// The Properties panel's rows for the board items GAPS.md item 24 (package P16) added: the text box (`PCB_TEXTBOX_DESC` and the `EDA_TEXT_DESC` it takes after, pcbnew/pcb_textbox.cpp,
// common/eda_text.cpp at KiCad 8303b2ad); reference images, points and barcodes. Called from `build()` of pcbProperties.ts, which keeps the registry; this file only
// adds classes to it. A text box takes after `BOARD_ITEM` (position, layer, lock) and has its own text rows: the same names and groups as `EDA_TEXT`'s, which edit one `pcb_item` verb of
// the whole box (`edit_text_box`) -- the free text's setters are the text's own verbs and a field's the footprint's.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BarcodeEcc, BarcodeKind, BoardBarcode, BoardGridItem, BoardPoint, BoardRefImage, BoardTextBox, BoardViaStack, BoardViaStitch } from "../api/pcbItemTypes";
import type { BoardDrillChart, BoardDrillMap, TableUnits } from "../api/pcbItemTypes";
import { spanChoiceOf, spanChoices, withSpanChoice } from "./drillChart";
import { LAYOUT_PROPERTY_CHOICES, layoutOf, MODE_CHOICES, modeOf, viaStitchCmd, withLayoutAndMode, type StitchLayout, type StitchMode } from "./viaStitch";
import { gridItemCmd, normalizedGridItem } from "./gridItem";
import type { Cmd } from "../api/types";
import { BARCODE_KINDS, barcodeToCmd, clampEcc, eccChoices, hasErrorCorrection } from "./barcodeShapes";
import type { PcbCtx, PcbItem } from "./pcbProperties";
import { tooSmall } from "./propertyGrid";
import type { Choice, PropValue, PropertyDef, PropertyManager } from "./propertyManager";
import { autoPenNm, clampPenNm, clampTextSize, textBoxToCmd, withTextAngle } from "./pcbTextBox";
import { SCALE_PPM, scaleForHeightNm, scaleForWidthNm } from "./refImage";

type PM = PropertyManager<PcbItem, PcbCtx, Cmd>;
type Add = (def: PropertyDef<PcbItem, PcbCtx, Cmd>, group?: string) => void;

const num = (v: PropValue): number => (typeof v === "number" ? v : Number(v));
const norm360 = (a: number): number => ((a % 360) + 360) % 360;

/** The cmd that replaces text box `b` with `b` changed by `patch`. */
function boxCmd(b: BoardTextBox, patch: Partial<BoardTextBox>): Cmd[] {
  const next: BoardTextBox = { ...b, ...patch };
  return [{ op: "pcb_item", verb: "edit_text_box", id: b.id, text_box: textBoxToCmd(next) }];
}

/** The cmd that puts image `i` on `layer`, with `scale_ppm`: its centre and pixels stay. */
function imageCmd(i: BoardRefImage, patch: { layer?: string; scale_ppm?: number }): Cmd[] {
  return [{ op: "pcb_item", verb: "edit_ref_image", id: i.id, layer: patch.layer ?? i.layer, at: { x: i.x, y: i.y }, scale_ppm: patch.scale_ppm ?? i.scale_ppm }];
}

/** `KiROUND`. */
const kiRound = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v));

/**
 * `REFERENCE_IMAGE::SetImageScale` -> `scaleBy( ratio )`: the image is scaled about its transform origin (its centre plus the offset), so its centre moves and the offset
 * scales with it: `edit_ref_image` with the new centre and scale, and `set_ref_image_offset` when there is an offset, in one step (UI-LEFT-3 P3-10). A scale that is not
 * positive is ignored (`if( aScale <= 0 ) return;`).
 */
export function rescaleImageCmds(i: BoardRefImage, scalePpm: number): Cmd[] {
  if (!(scalePpm > 0) || scalePpm === i.scale_ppm) return [];
  const ratio = scalePpm / i.scale_ppm;
  const ox = i.offset?.x ?? 0;
  const oy = i.offset?.y ?? 0;
  const origin = { x: i.x + ox, y: i.y + oy };
  const at = { x: kiRound(origin.x - ox * ratio), y: kiRound(origin.y - oy * ratio) };
  const cmds: Cmd[] = [{ op: "pcb_item", verb: "edit_ref_image", id: i.id, layer: i.layer, at, scale_ppm: scalePpm }];
  if (ox !== 0 || oy !== 0) cmds.push({ op: "pcb_item", verb: "set_ref_image_offset", id: i.id, offset: { x: kiRound(ox * ratio), y: kiRound(oy * ratio) } });
  return cmds;
}

/** Register `PCB_REFERENCE_IMAGE` and its properties (`PCB_REFERENCE_IMAGE_DESC`, pcbnew/pcb_reference_image.cpp). */
function registerReferenceImage(pm: PM, add: Add, layerChoices: (c: PcbCtx) => Choice[]): void {
  pm.registerType("PCB_REFERENCE_IMAGE");
  pm.inheritsAfter("PCB_REFERENCE_IMAGE", "BOARD_ITEM");
  const own = (i: PcbItem): BoardRefImage => i.image!;
  // `ReplaceProperty( BOARD_ITEM, "Layer", .. "Associated Layer" )`
  pm.replaceProperty("BOARD_ITEM", "Layer", {
    owner: "PCB_REFERENCE_IMAGE",
    name: "Associated Layer",
    kind: "enum",
    choices: (_i, c) => layerChoices(c),
    get: (i) => own(i).layer,
    set: (i, v) => (own(i).layer === String(v) ? [] : imageCmd(own(i), { layer: String(v) })),
  });
  const IMAGE = "Image Properties";
  const row = (def: Omit<PropertyDef<PcbItem, PcbCtx, Cmd>, "owner">): void => add({ owner: "PCB_REFERENCE_IMAGE", ...def } as PropertyDef<PcbItem, PcbCtx, Cmd>, IMAGE);
  // `SetImageScale( aScale )` is ignored for a scale that is not positive (`REFERENCE_IMAGE::SetImageScale`); `PCB_REFERENCE_IMAGE_DESC` registers no validator, so
  // a 0 is taken by the grid and changes nothing (UI-LEFT-3 P3-8).
  row({
    name: "Scale",
    kind: "double",
    get: (i) => own(i).scale_ppm / SCALE_PPM,
    set: (i, v) => rescaleImageCmds(own(i), Math.round(num(v) * SCALE_PPM)),
  });
  // "Transform Offset X/Y" (`SetTransformOriginOffsetX` / `Y`, between Scale and Width as `PCB_REFERENCE_IMAGE_DESC` registers them, `PT_COORD` with `ABS_X_COORD` /
  // `ABS_Y_COORD`): the point the image is scaled about, from its centre (`BoardItems::image_offsets`, `set_ref_image_offset`; UI-LEFT-3 P3-10).
  (["X", "Y"] as const).forEach((axis) =>
    row({
      name: `Transform Offset ${axis}`,
      kind: "int",
      display: "coord",
      coordType: axis === "X" ? "abs_x" : "abs_y",
      get: (i) => (axis === "X" ? own(i).offset?.x : own(i).offset?.y) ?? 0,
      set: (i, v) => {
        const image = own(i);
        const [ox, oy] = [image.offset?.x ?? 0, image.offset?.y ?? 0];
        const n = Math.round(num(v));
        if ((axis === "X" ? ox : oy) === n) return [];
        return [{ op: "pcb_item", verb: "set_ref_image_offset", id: image.id, offset: axis === "X" ? { x: n, y: oy } : { x: ox, y: n } }];
      },
    })
  );
  // `SetWidth` / `SetHeight`: `scaleBy( width / current width )`, ignored for a size that is not positive (`REFERENCE_IMAGE::SetWidth`); the rows are `PT_COORD`
  // with no validator, so a 0 is taken and changes nothing (UI-LEFT-3 P3-8).
  const side = (name: "Width" | "Height"): void =>
    row({
      name,
      kind: "int",
      display: "size",
      get: (i) => (name === "Width" ? own(i).w : own(i).h),
      set: (i, v) => {
        const image = own(i);
        const want = Math.round(num(v));
        if (want <= 0 || want === (name === "Width" ? image.w : image.h)) return [];
        return rescaleImageCmds(image, name === "Width" ? scaleForWidthNm(image.px, image.ppi, want) : scaleForHeightNm(image.px, image.ppi, want));
      },
    });
  side("Width");
  side("Height");
}

/** The cmd that puts point `p` on `layer`, at `at`, at `size`. */
export function pointCmd(p: BoardPoint, patch: { layer?: string; size?: number }): Cmd[] {
  return [{ op: "pcb_item", verb: "edit_point", id: p.id, layer: patch.layer ?? p.layer, at: { x: p.x, y: p.y }, size_nm: patch.size ?? p.size }];
}

/** Register `PCB_POINT` and its properties (`PCB_POINT_DESC`, pcbnew/pcb_point.cpp): the ones of a board item and a Size. The lock is not the point's: the file form has none. */
function registerPoint(pm: PM, add: Add): void {
  pm.registerType("PCB_POINT");
  pm.inheritsAfter("PCB_POINT", "BOARD_ITEM");
  const own = (i: PcbItem): BoardPoint => i.point!;
  // `PCB_POINT_DESC` keeps `BOARD_ITEM`'s Layer (no `ReplaceProperty`), so its row stays before Locked: the board item's Layer reads and sets a point's (`pointCmd`).
  pm.overrideWriteability("PCB_POINT", "BOARD_ITEM", "Locked", () => false);
  add({
    owner: "PCB_POINT",
    name: "Size",
    kind: "int",
    display: "size",
    get: (i) => own(i).size,
    // `PCB_POINT::SetSize` takes any size and `PCB_POINT_DESC` registers no validator: the `PT_SIZE` row (an unsigned number) refuses only a negative one (UI-LEFT-3 P3-8).
    validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
    set: (i, v) => {
      const size = Math.round(num(v));
      return size < 0 || size === own(i).size ? [] : pointCmd(own(i), { size });
    },
  });
}

/** The cmd that replaces barcode `b` with `b` changed by `patch`: every field of it goes, its id, lock and groups stay (`edit_barcode`). */
export function barcodeCmd(b: BoardBarcode, patch: Partial<BoardBarcode>): Cmd[] {
  const next: BoardBarcode = { ...b, ...patch };
  // `PCB_BARCODE::SetErrorCorrection`: a Micro QR code has no High.
  next.ecc = clampEcc(next.kind, next.ecc);
  return [{ op: "pcb_item", verb: "edit_barcode", id: b.id, barcode: barcodeToCmd(next) }];
}

/** Register `PCB_BARCODE` and its properties (`PCB_BARCODE_DESC`, pcbnew/pcb_barcode.cpp): the rows of a board item and, under "Barcode Properties", the text, its size and whether it is shown, the size and
 * turn of the symbol, the kind, the error correction of the QR kinds (Micro QR has no High) and the knockout with its margins. */
function registerBarcode(pm: PM, add: Add): void {
  pm.registerType("PCB_BARCODE");
  pm.inheritsAfter("PCB_BARCODE", "BOARD_ITEM");
  const own = (i: PcbItem): BoardBarcode => i.barcode!;
  // `PCB_BARCODE_DESC` keeps `BOARD_ITEM`'s Layer too (no `ReplaceProperty`): the board item's row reads and sets a barcode's (`barcodeCmd`).
  const GROUP = "Barcode Properties";
  const row = (def: Omit<PropertyDef<PcbItem, PcbCtx, Cmd>, "owner">): void => add({ owner: "PCB_BARCODE", ...def } as PropertyDef<PcbItem, PcbCtx, Cmd>, GROUP);
  // `PCB_BARCODE_DESC` registers no validator: the `PT_SIZE` rows refuse only a negative size (UI-LEFT-3 P3-8). `SetTextSize` takes at least 1 nm
  // (`std::max( 1, aTextSize )`); `SetBarcodeWidth` / `SetBarcodeHeight` take 0 and set both sides of a square kind (`KeepSquare`: QR, Micro QR, Data Matrix).
  const keepSquare = (b: BoardBarcode): boolean => b.kind === "qr" || b.kind === "microqr" || b.kind === "datamatrix";
  const size = (name: string, get: (b: BoardBarcode) => number, key: "text_height" | "w" | "h"): void =>
    row({
      name,
      kind: "int",
      display: "size",
      get: (i) => get(own(i)),
      validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
      set: (i, v) => {
        const b = own(i);
        const typed = Math.round(num(v));
        if (typed < 0) return [];
        if (key === "text_height") {
          const want = Math.max(1, typed);
          return want === b.text_height ? [] : barcodeCmd(b, { text_height: want });
        }
        const patch: Partial<BoardBarcode> = keepSquare(b) ? { w: typed, h: typed } : { [key]: typed };
        return (patch.w ?? b.w) === b.w && (patch.h ?? b.h) === b.h ? [] : barcodeCmd(b, patch);
      },
    });
  row({ name: "Text", kind: "string", get: (i) => own(i).text, set: (i, v) => (own(i).text === String(v) ? [] : barcodeCmd(own(i), { text: String(v) })) });
  row({ name: "Show Text", kind: "bool", get: (i) => own(i).show_text, set: (i, v) => (own(i).show_text === Boolean(v) ? [] : barcodeCmd(own(i), { show_text: Boolean(v) })) });
  size("Text Size", (b) => b.text_height, "text_height");
  size("Width", (b) => b.w, "w");
  size("Height", (b) => b.h, "h");
  row({
    name: "Orientation",
    kind: "double",
    display: "degree",
    get: (i) => own(i).angle / 1000,
    set: (i, v) => {
      const mdeg = Math.round(norm360(num(v)) * 1000) % 360_000;
      return mdeg === own(i).angle ? [] : barcodeCmd(own(i), { angle: mdeg });
    },
  });
  row({
    name: "Barcode Type",
    kind: "enum",
    choices: () => BARCODE_KINDS.map((k) => ({ label: k.property, value: k.value })),
    get: (i) => own(i).kind,
    set: (i, v) => (own(i).kind === v ? [] : barcodeCmd(own(i), { kind: String(v) as BarcodeKind })),
  });
  row({
    name: "Error Correction",
    kind: "enum",
    choices: (i) => eccChoices(own(i).kind).map((c) => ({ label: c.label, value: c.value })),
    get: (i) => own(i).ecc,
    available: (i) => hasErrorCorrection(own(i).kind),
    set: (i, v) => (own(i).ecc === v ? [] : barcodeCmd(own(i), { ecc: String(v) as BarcodeEcc })),
  });
  row({ name: "Knockout", kind: "bool", get: (i) => own(i).knockout, set: (i, v) => (own(i).knockout === Boolean(v) ? [] : barcodeCmd(own(i), { knockout: Boolean(v) })) });
  const margin = (name: string, get: (b: BoardBarcode) => number, key: "margin_x" | "margin_y"): void =>
    row({
      name,
      kind: "int",
      display: "size",
      get: (i) => get(own(i)),
      available: (i) => own(i).knockout,
      validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
      set: (i, v) => {
        const want = Math.round(num(v));
        return want < 0 || want === get(own(i)) ? [] : barcodeCmd(own(i), { [key]: want } as Partial<BoardBarcode>);
      },
    });
  margin("Margin X", (b) => b.margin_x, "margin_x");
  margin("Margin Y", (b) => b.margin_y, "margin_y");
}

/** Register `PCB_TEXTBOX` and its properties, and the other classes of the family. */
/** The cmd that replaces grid item `g` with `g` changed by `patch`, through the item's setters (`edit_grid_item`). */
function gridCmd(g: BoardGridItem, patch: Partial<BoardGridItem>): Cmd[] {
  return [{ op: "pcb_item", verb: "edit_grid_item", id: g.id, grid_item: gridItemCmd(normalizedGridItem({ ...g, ...patch })) }];
}

/**
 * Register KiCad 10's `PCB_GRID_ITEM` (`PCB_GRIDITEM_DESC`, pcbnew/pcb_grid_item.cpp of KiCad 25ec2335): a board item without a layer (the Layer row is not available), then Grid Type,
 * Orientation, the cartesian Extent X / Y (the whole width, twice the half-extent the item keeps) and Spacing X / Y, the polar Radius Extent, Radius Spacing, Phi Extent and Phi Spacing,
 * Priority, Tick Interval and what it affects.
 */
function registerGridItem(pm: PM, add: Add): void {
  pm.registerType("PCB_GRID_ITEM");
  pm.inheritsAfter("PCB_GRID_ITEM", "BOARD_ITEM");
  pm.overrideAvailability("PCB_GRID_ITEM", "BOARD_ITEM", "Layer", () => false);
  const own = (i: PcbItem): BoardGridItem => i.grid!;
  const isCartesian = (i: PcbItem): boolean => own(i).kind === "cartesian";
  const isPolar = (i: PcbItem): boolean => own(i).kind === "polar";
  const same = (a: BoardGridItem, b: BoardGridItem): boolean => JSON.stringify(gridItemCmd(a)) === JSON.stringify(gridItemCmd(b));
  const edit = (i: PcbItem, patch: Partial<BoardGridItem>): Cmd[] => (same(normalizedGridItem({ ...own(i), ...patch }), own(i)) ? [] : gridCmd(own(i), patch));
  const GRID_TYPES: Choice[] = [
    { label: "Cartesian", value: "cartesian" },
    { label: "Polar", value: "polar" },
  ];
  add({ owner: "PCB_GRID_ITEM", name: "Grid Type", kind: "enum", choices: () => GRID_TYPES, get: (i) => own(i).kind, set: (i, v) => edit(i, { kind: String(v) as BoardGridItem["kind"] }) });
  add({ owner: "PCB_GRID_ITEM", name: "Orientation", kind: "double", display: "degree", get: (i) => own(i).angle, set: (i, v) => edit(i, { angle: num(v) }) });
  const size = (name: string, available: (i: PcbItem) => boolean, get: (g: BoardGridItem) => number, patch: (g: BoardGridItem, v: number) => Partial<BoardGridItem>): void =>
    add({ owner: "PCB_GRID_ITEM", name, kind: "int", display: "size", available, get: (i) => get(own(i)), set: (i, v) => edit(i, patch(own(i), Math.round(num(v)))) });
  // `GetExtentX` / `SetExtentX`: "2x to show total width to the user".
  size("Extent X", isCartesian, (g) => g.extent[0] * 2, (g, v) => ({ extent: [Math.abs(Math.trunc(v / 2)), g.extent[1]] }));
  size("Extent Y", isCartesian, (g) => g.extent[1] * 2, (g, v) => ({ extent: [g.extent[0], Math.abs(Math.trunc(v / 2))] }));
  size("Spacing X", isCartesian, (g) => g.spacing[0], (g, v) => ({ spacing: [Math.max(v, 1), g.spacing[1]] }));
  size("Spacing Y", isCartesian, (g) => g.spacing[1], (g, v) => ({ spacing: [g.spacing[0], Math.max(v, 1)] }));
  size("Radius Extent", isPolar, (g) => g.extent[0], (g, v) => ({ extent: [Math.abs(v), g.extent[1]] }));
  size("Radius Spacing", isPolar, (g) => g.spacing[0], (g, v) => ({ spacing: [Math.max(v, 1), g.spacing[1]] }));
  add({ owner: "PCB_GRID_ITEM", name: "Phi Extent", kind: "double", display: "degree", available: isPolar, get: (i) => own(i).phi_extent, set: (i, v) => edit(i, { phi_extent: num(v) }) });
  add({ owner: "PCB_GRID_ITEM", name: "Phi Spacing", kind: "double", display: "degree", available: isPolar, get: (i) => own(i).phi_spacing, set: (i, v) => edit(i, { phi_spacing: num(v) }) });
  add({ owner: "PCB_GRID_ITEM", name: "Priority", kind: "int", get: (i) => own(i).priority, set: (i, v) => edit(i, { priority: Math.max(0, Math.trunc(num(v))) }) });
  add({ owner: "PCB_GRID_ITEM", name: "Tick Interval", kind: "int", get: (i) => own(i).tick_interval, set: (i, v) => edit(i, { tick_interval: Math.max(0, Math.trunc(num(v))) }) });
  const affects = (name: string, key: keyof BoardGridItem["affects"]): void =>
    add({ owner: "PCB_GRID_ITEM", name, kind: "bool", get: (i) => own(i).affects[key], set: (i, v) => edit(i, { affects: { ...own(i).affects, [key]: Boolean(v) } }) });
  affects("Affects Cursor Snap", "cursor");
  affects("Affects Routing", "routing");
  affects("Affects Placement", "placement");
}

/**
 * Register KiCad 10's `PCB_VIA_STITCH` (`PCB_VIA_STITCH_DESC`): a board item with its Layer masked ("The stitch lives on its own GAL layer"), then Size, Drill, Pitch, Layout (a stitch's),
 * Mode, Seed (a Poisson stitch's), Net and Guarded Net (a guard's). Each edit gives the stitching the setting and makes its vias again (`set_via_stitch`, the server placing them).
 */
function registerViaStitch(pm: PM, add: Add): void {
  pm.registerType("PCB_VIA_STITCH");
  pm.inheritsAfter("PCB_VIA_STITCH", "BOARD_ITEM");
  pm.mask("PCB_VIA_STITCH", "BOARD_ITEM", "Layer");
  const own = (i: PcbItem): BoardViaStitch => i.stitch!;
  const edit = (i: PcbItem, next: BoardViaStitch): Cmd[] => (JSON.stringify(next) === JSON.stringify(own(i)) ? [] : [viaStitchCmd(own(i).id, next)]);
  const via = (i: PcbItem) => own(i).via ?? { size_nm: 0, drill_nm: 0, from_layer: "F.Cu", to_layer: "B.Cu" };
  add({ owner: "PCB_VIA_STITCH", name: "Size", kind: "int", display: "size", get: (i) => via(i).size_nm, set: (i, v) => edit(i, { ...own(i), via: { ...via(i), size_nm: Math.round(num(v)) } }) });
  add({ owner: "PCB_VIA_STITCH", name: "Drill", kind: "int", display: "size", get: (i) => via(i).drill_nm, set: (i, v) => edit(i, { ...own(i), via: { ...via(i), drill_nm: Math.round(num(v)) } }) });
  add({ owner: "PCB_VIA_STITCH", name: "Pitch", kind: "int", display: "size", get: (i) => own(i).pitch_nm ?? 0, set: (i, v) => edit(i, { ...own(i), pitch_nm: Math.round(num(v)) }) });
  add({
    owner: "PCB_VIA_STITCH",
    name: "Layout",
    kind: "enum",
    choices: () => LAYOUT_PROPERTY_CHOICES.map((c) => ({ label: c.label, value: c.value })),
    available: (i) => modeOf(own(i)) === "stitch",
    get: (i) => layoutOf(own(i)),
    set: (i, v) => edit(i, withLayoutAndMode(own(i), String(v) as StitchLayout, modeOf(own(i)))),
  });
  add({
    owner: "PCB_VIA_STITCH",
    name: "Mode",
    kind: "enum",
    choices: () => MODE_CHOICES.map((c) => ({ label: c.label, value: c.value })),
    get: (i) => modeOf(own(i)),
    set: (i, v) => edit(i, withLayoutAndMode(own(i), layoutOf(own(i)), String(v) as StitchMode)),
  });
  add({
    owner: "PCB_VIA_STITCH",
    name: "Seed",
    kind: "int",
    available: (i) => modeOf(own(i)) === "stitch" && layoutOf(own(i)) === "poisson",
    get: (i) => own(i).seed ?? 0,
    set: (i, v) => edit(i, { ...own(i), seed: Math.max(0, Math.trunc(num(v))) >>> 0 }),
  });
  const netChoices = (c: PcbCtx): Choice[] => [{ label: "<no net>", value: "" }, ...c.nets.map((n) => ({ label: n, value: n }))];
  add({ owner: "PCB_VIA_STITCH", name: "Net", kind: "net", choices: (_i, c) => netChoices(c), get: (i) => own(i).net ?? "", set: (i, v) => edit(i, { ...own(i), net: String(v) }) });
  add({
    owner: "PCB_VIA_STITCH",
    name: "Guarded Net",
    kind: "net",
    choices: (_i, c) => netChoices(c),
    available: (i) => modeOf(own(i)) === "guard",
    get: (i) => own(i).guarded_net ?? "",
    set: (i, v) => edit(i, { ...own(i), guarded_net: String(v) }),
  });
}

/**
 * Register KiCad 10's `PCB_DRILL_MAP` (`PCB_DRILL_MAP_DESC`): a board item with Position X / Y masked ("Relative, not absolute"), then Offset X, Offset Y, Symbol Size, Hole Span (the
 * spans the board has, `PCB_PROPERTIES_PANEL::updateLists`), Outline Slots and Guide Cross, under "Drill Map Properties". Each is one `edit_drill_map`.
 */
function registerDrillMap(pm: PM, add: Add): void {
  pm.registerType("PCB_DRILL_MAP");
  pm.inheritsAfter("PCB_DRILL_MAP", "BOARD_ITEM");
  pm.mask("PCB_DRILL_MAP", "BOARD_ITEM", "Position X");
  pm.mask("PCB_DRILL_MAP", "BOARD_ITEM", "Position Y");
  const GROUP = "Drill Map Properties";
  const own = (i: PcbItem): BoardDrillMap => i.drillMap!;
  const edit = (i: PcbItem, next: BoardDrillMap): Cmd[] => (JSON.stringify(next) === JSON.stringify(own(i)) ? [] : [{ op: "pcb_item", verb: "edit_drill_map", id: own(i).id, map: next }]);
  // `PT_COORD` with `REL_X_COORD` / `REL_Y_COORD` (KiCad 25ec2335): "Relative, not absolute" -- only the axis directions of Origins & Axes turn them
  add({ owner: "PCB_DRILL_MAP", name: "Offset X", kind: "int", display: "coord", coordType: "rel_x", get: (i) => own(i).offset.x, set: (i, v) => edit(i, { ...own(i), offset: { ...own(i).offset, x: Math.round(num(v)) } }) }, GROUP);
  add({ owner: "PCB_DRILL_MAP", name: "Offset Y", kind: "int", display: "coord", coordType: "rel_y", get: (i) => own(i).offset.y, set: (i, v) => edit(i, { ...own(i), offset: { ...own(i).offset, y: Math.round(num(v)) } }) }, GROUP);
  add({ owner: "PCB_DRILL_MAP", name: "Symbol Size", kind: "int", display: "size", get: (i) => own(i).size_nm, set: (i, v) => edit(i, { ...own(i), size_nm: Math.max(1, Math.round(num(v))) }) }, GROUP);
  const spans = (c: PcbCtx) => c.board.drawings?.drill_spans ?? [];
  add(
    {
      owner: "PCB_DRILL_MAP",
      name: "Hole Span",
      kind: "enum",
      choices: (_i, c) => spanChoices(spans(c)).map((s) => ({ label: s.label, value: s.value })),
      get: (i, c) => spanChoiceOf(own(i), spans(c)),
      set: (i, v, c) => edit(i, withSpanChoice(own(i), spans(c), Number(v))),
    },
    GROUP
  );
  add({ owner: "PCB_DRILL_MAP", name: "Outline Slots", kind: "bool", get: (i) => own(i).outline_slots !== false, set: (i, v) => edit(i, { ...own(i), outline_slots: Boolean(v) }) }, GROUP);
  add({ owner: "PCB_DRILL_MAP", name: "Guide Cross", kind: "bool", get: (i) => !!own(i).guide_cross, set: (i, v) => edit(i, { ...own(i), guide_cross: Boolean(v) }) }, GROUP);
}

/**
 * KiCad 10's `PCB_GENERATED_TABLE` and `PCB_DRILL_CHART` rows of a table that is a drill chart: Units and Decimal Places ("Table Generator"), Show Totals ("Drill Chart Properties").
 * Each edit is one `set_drill_chart`, which builds the chart again from the board.
 */
export function registerDrillChart(add: Add): void {
  const chartOf = (i: PcbItem, c: PcbCtx): BoardDrillChart | undefined => (i.type === "PCB_TABLE" ? c.board.drawings?.drill_charts?.find((x) => x.table_id === i.id) : undefined);
  const isChart = (i: PcbItem, c?: PcbCtx): boolean => !!c && !!chartOf(i, c);
  const edit = (i: PcbItem, c: PcbCtx, patch: { units?: TableUnits; precision?: number; show_totals?: boolean }): Cmd[] => {
    const ch = chartOf(i, c);
    if (!ch) return [];
    const cur = { units: ch.units ?? "mm", precision: ch.precision ?? 3, show_totals: ch.show_totals ?? true };
    const next = { ...cur, ...patch };
    if (JSON.stringify(next) === JSON.stringify(cur)) return [];
    return [{ op: "pcb_item", verb: "set_drill_chart", id: ch.table_id, ...next, filter: ch.filter ?? { plated: true, non_plated: true, vias: true, slots: true, backdrills: true, castellated: true } }];
  };
  const UNITS: Choice[] = [
    { label: "Millimeters", value: "mm" },
    { label: "Mils", value: "mils" },
    { label: "Inches", value: "inch" },
  ];
  // KiCad's drill chart is a class below the generated table, below the table: its own group comes first, then the generator's, then the table's
  // (`GetGroupDisplayOrder`). The studio's one PCB_TABLE class gets that order by introducing these groups before the table's (`registerTableProperties` runs after this).
  add({ owner: "PCB_TABLE", name: "Show Totals", kind: "bool", available: (i, c) => isChart(i, c), get: (i, c) => chartOf(i, c)?.show_totals ?? true, set: (i, v, c) => edit(i, c, { show_totals: Boolean(v) }) }, "Drill Chart Properties");
  add(
    { owner: "PCB_TABLE", name: "Units", kind: "enum", choices: () => UNITS, available: (i, c) => isChart(i, c), get: (i, c) => chartOf(i, c)?.units ?? "mm", set: (i, v, c) => edit(i, c, { units: String(v) as TableUnits }) },
    "Table Generator"
  );
  add(
    {
      owner: "PCB_TABLE",
      name: "Decimal Places",
      kind: "int",
      available: (i, c) => isChart(i, c),
      get: (i, c) => chartOf(i, c)?.precision ?? 3,
      // `SetPrecision`: clamped to 0..6.
      set: (i, v, c) => edit(i, c, { precision: Math.min(6, Math.max(0, Math.trunc(num(v)))) }),
    },
    "Table Generator"
  );
}

/**
 * Register KiCad 10's `PCB_VIA_STACK` (`PCB_VIA_STACK_DESC`, UI-LEFT-3 X3-1): a board item with its Layer masked ("A stack spans layers, so the single inherited layer is
 * meaningless, as for a via"), then in "Microvia Stack Properties" Start Layer, End Layer and Style, the Net, Use Netclass Values, Via Diameter, Via Hole, Pitch (a
 * staggered stack's), Copper-filled and Capped -- all read-only: "a stack's geometry is rebuilt from these, and the Properties panel has no way to trigger that rebuild.
 * Editing goes through the stack's own dialog."
 */
function registerViaStack(pm: PM, add: Add): void {
  pm.registerType("PCB_VIA_STACK");
  pm.inheritsAfter("PCB_VIA_STACK", "BOARD_ITEM");
  pm.mask("PCB_VIA_STACK", "BOARD_ITEM", "Layer");
  const own = (i: PcbItem): BoardViaStack => i.viaStack!;
  const group = "Microvia Stack Properties";
  const layer = (name: string, get: (s: BoardViaStack) => string) => add({ owner: "PCB_VIA_STACK", name, kind: "enum", choices: (_i, c) => c.allLayers.map((l) => ({ label: l, value: l })), get: (i) => get(own(i)) }, group);
  layer("Start Layer", (s) => s.start_layer);
  layer("End Layer", (s) => s.end_layer);
  add({ owner: "PCB_VIA_STACK", name: "Style", kind: "enum", choices: () => [{ label: "Stacked", value: "stacked" }, { label: "Staggered", value: "staggered" }], get: (i) => own(i).style ?? "stacked" }, group);
  add({ owner: "PCB_VIA_STACK", name: "Net", kind: "net", choices: (_i, c) => [{ label: "<no net>", value: "" }, ...c.nets.map((n) => ({ label: n, value: n }))], get: (i) => own(i).net ?? "" });
  add({ owner: "PCB_VIA_STACK", name: "Use Netclass Values", kind: "bool", get: (i) => own(i).use_netclass ?? false }, group);
  add({ owner: "PCB_VIA_STACK", name: "Via Diameter", kind: "int", display: "size", get: (i) => own(i).via_size_nm ?? 0 }, group);
  add({ owner: "PCB_VIA_STACK", name: "Via Hole", kind: "int", display: "size", get: (i) => own(i).via_drill_nm ?? 0 }, group);
  add({ owner: "PCB_VIA_STACK", name: "Pitch", kind: "int", display: "size", available: (i) => own(i).style === "staggered", get: (i) => own(i).pitch_nm ?? 0 }, group);
  add({ owner: "PCB_VIA_STACK", name: "Copper-filled", kind: "bool", get: (i) => own(i).filled ?? true }, group);
  add({ owner: "PCB_VIA_STACK", name: "Capped", kind: "bool", get: (i) => own(i).capped ?? false }, group);
}

export function registerExtraItemProperties(pm: PM, add: Add, layerChoices: (c: PcbCtx) => Choice[]): void {
  registerGridItem(pm, add);
  registerViaStitch(pm, add);
  registerViaStack(pm, add);
  registerDrillMap(pm, add);
  registerReferenceImage(pm, add, layerChoices);
  registerPoint(pm, add);
  registerBarcode(pm, add);
  pm.registerType("PCB_TEXTBOX");
  pm.inheritsAfter("PCB_TEXTBOX", "BOARD_ITEM");
  // KiCad's text box is a PCB_SHAPE, so a BOARD_CONNECTED_ITEM: its Net row (no availability of its own; PCB_SHAPE's `isCopper` override is not inherited) comes
  // before the Layer. The studio's text box has no net, so the row reads none and is read-only.
  add(
    { owner: "PCB_TEXTBOX", name: "Net", kind: "net", choices: () => [{ label: "<no net>", value: "" }], get: () => "", readOnlyReason: "A text box carries no net in the studio's model." },
    ""
  );
  // A text box is on any layer (`BOARD_ITEM`'s row would read the layer off a text or a dimension).
  pm.replaceProperty("BOARD_ITEM", "Layer", {
    owner: "PCB_TEXTBOX",
    name: "Layer",
    kind: "enum",
    choices: (_i, c) => layerChoices(c),
    get: (i) => i.textBox!.layer,
    set: (i, v) => (i.textBox!.layer === String(v) ? [] : boxCmd(i.textBox!, { layer: String(v) })),
  });

  const TEXT = "Text Properties";
  const own = (i: PcbItem): BoardTextBox => i.textBox!;
  const row = (def: Omit<PropertyDef<PcbItem, PcbCtx, Cmd>, "owner">, group: string): void => add({ owner: "PCB_TEXTBOX", ...def } as PropertyDef<PcbItem, PcbCtx, Cmd>, group);
  // EDA_TEXT's Orientation is in the basic group (`EDA_TEXT_DESC` adds it with no group).
  // `PCB_TEXTBOX::SetTextAngle`: the box turns about its position.
  row(
    {
      name: "Orientation",
      kind: "double",
      display: "degree",
      get: (i) => own(i).angle / 1000,
      set: (i, v) => {
        const angle = Math.round(norm360(num(v)) * 1000);
        const b = own(i);
        return b.angle === angle ? [] : boxCmd(withTextAngle(b, angle), {});
      },
    },
    ""
  );


  row(
    {
      name: "Text",
      kind: "string",
      get: (i) => own(i).text,
      set: (i, v) => (own(i).text === String(v) ? [] : boxCmd(own(i), { text: String(v) })),
    },
    TEXT
  );
  // "Font" (`EDA_TEXT_DESC`'s, after Text): KiCad's stroke font or a face, wrapped and drawn in it (UI-LEFT-2 CC1).
  row(
    {
      name: "Font",
      kind: "string",
      get: (i) => own(i).face || "KiCad Font",
      set: (i, v) => {
        const next = String(v).trim();
        const face = next === "" || next.startsWith("KiCad Font") ? null : next;
        return (own(i).face ?? null) === face ? [] : boxCmd(own(i), { face });
      },
    },
    TEXT
  );
  // "Auto Thickness" (`EDA_TEXT::SetAutoThickness`): on, the pen is `GetPenSizeForNormal` / `ForBold` of the smaller size; off, the pen stays as it was.
  row(
    {
      name: "Auto Thickness",
      kind: "bool",
      get: (i) => !!own(i).thickness_auto,
      set: (i, v) => {
        const b = own(i);
        if (!!b.thickness_auto === Boolean(v)) return [];
        return boxCmd(b, Boolean(v) ? { thickness: autoPenNm(Math.min(b.size, b.width), b.bold), thickness_auto: true } : { thickness_auto: false });
      },
    },
    TEXT
  );
  row(
    {
      name: "Thickness",
      kind: "int",
      display: "size",
      get: (i) => own(i).thickness,
      validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
      set: (i, v) => {
        const b = own(i);
        const w = Math.round(num(v));
        if (b.thickness === w && !b.thickness_auto) return [];
        // `ClampTextPenSize`: a pen wider than a sixth of the glyph is clamped
        const pen = w > 0 ? clampPenNm(w, Math.min(b.size, b.width)) : 0;
        return boxCmd(b, pen > 0 ? { thickness: pen, thickness_auto: false } : { thickness: autoPenNm(Math.min(b.size, b.width), b.bold), thickness_auto: true });
      },
    },
    TEXT
  );
  const flag = (name: string, key: "italic" | "bold" | "mirror" | "knockout"): void =>
    row(
      {
        name,
        kind: "bool",
        get: (i) => own(i)[key],
        set: (i, v) => {
          const b = own(i);
          if (b[key] === Boolean(v)) return [];
          // The automatic pen follows the weight (`GetPenSizeForBold`)
          const pen = key === "bold" && b.thickness_auto ? { thickness: autoPenNm(Math.min(b.size, b.width), Boolean(v)) } : {};
          return boxCmd(b, { [key]: Boolean(v), ...pen });
        },
      },
      TEXT
    );
  flag("Italic", "italic");
  flag("Bold", "bold");
  flag("Mirrored", "mirror");
  // "Visible" (`EDA_TEXT_DESC`, available on a field only: `isField`).
  row({ name: "Visible", kind: "bool", get: () => true, available: () => false }, TEXT);
  const glyph = (name: "Width" | "Height"): void =>
    row(
      {
        name,
        kind: "int",
        display: "size",
        get: (i) => (name === "Width" ? own(i).width : own(i).size),
        // `EDA_TEXT_DESC`'s `PT_SIZE` rows have no validator: a negative size is refused, any other is clamped to `TEXT_MIN_SIZE_MM` .. `TEXT_MAX_SIZE_MM`
        // (`EDA_TEXT::SetTextWidth` / `SetTextHeight`; UI-LEFT-3 P3-8)
        validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
        set: (i, v) => {
          const b = own(i);
          const s = clampTextSize(Math.round(num(v)));
          if ((name === "Width" ? b.width : b.size) === s) return [];
          const patch = name === "Width" ? { width: s } : { size: s };
          const size = Math.min(patch.width ?? b.width, patch.size ?? b.size);
          return boxCmd(b, { ...patch, ...(b.thickness_auto ? { thickness: autoPenNm(size, b.bold) } : {}) });
        },
      },
      TEXT
    );
  glyph("Width");
  glyph("Height");
  row(
    {
      name: "Horizontal Justification",
      kind: "enum",
      choices: () => [
        { label: "Left", value: "left" },
        { label: "Center", value: "center" },
        { label: "Right", value: "right" },
      ],
      get: (i) => own(i).halign,
      set: (i, v) => (own(i).halign === v ? [] : boxCmd(own(i), { halign: String(v) as BoardTextBox["halign"] })),
    },
    TEXT
  );
  row(
    {
      name: "Vertical Justification",
      kind: "enum",
      choices: () => [
        { label: "Top", value: "top" },
        { label: "Center", value: "center" },
        { label: "Bottom", value: "bottom" },
      ],
      get: (i) => own(i).valign,
      set: (i, v) => (own(i).valign === v ? [] : boxCmd(own(i), { valign: String(v) as BoardTextBox["valign"] })),
    },
    TEXT
  );
  // "Knockout" (`PCB_TEXTBOX_DESC`'s own, after the EDA_TEXT rows it takes after).
  flag("Knockout", "knockout");

  const BORDER = "Border Properties";
  row({ name: "Border", kind: "bool", get: (i) => own(i).border, set: (i, v) => (own(i).border === Boolean(v) ? [] : boxCmd(own(i), { border: Boolean(v) })) }, BORDER);
  row(
    {
      name: "Border Style",
      kind: "enum",
      choices: () => [
        { label: "Solid", value: "solid" },
        { label: "Dashed", value: "dash" },
        { label: "Dotted", value: "dot" },
        { label: "Dash-Dot", value: "dash_dot" },
        { label: "Dash-Dot-Dot", value: "dash_dot_dot" },
      ],
      get: (i) => (own(i).line_style === "default" ? "solid" : own(i).line_style),
      set: (i, v) => (own(i).line_style === v ? [] : boxCmd(own(i), { line_style: String(v) as BoardTextBox["line_style"] })),
    },
    BORDER
  );
  row(
    {
      name: "Border Width",
      kind: "int",
      display: "size",
      get: (i) => own(i).stroke_width,
      validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
      set: (i, v) => (own(i).stroke_width === Math.round(num(v)) ? [] : boxCmd(own(i), { stroke_width: Math.round(num(v)) })),
    },
    BORDER
  );

  const MARGINS = "Margins";
  (["Left", "Top", "Right", "Bottom"] as const).forEach((side, k) =>
    row(
      {
        name: `Margin ${side}`,
        kind: "int",
        display: "size",
        get: (i) => own(i).margins[k]!,
        validate: (v, _i, c) => (num(v) < 0 ? tooSmall(0, "size", c.units) : null),
        set: (i, v) => {
          const b = own(i);
          const m = Math.round(num(v));
          if (b.margins[k] === m) return [];
          const margins = [...b.margins] as BoardTextBox["margins"];
          margins[k] = m;
          return boxCmd(b, { margins });
        },
      },
      MARGINS
    )
  );
  // What a text box takes after PCB_SHAPE, BOARD_CONNECTED_ITEM and EDA_SHAPE and does not mask, all unavailable on one: the pad primitive rows (pad edit mode
  // only), the teardrops (`supportsTeardrops`: a pad or a via), and the shape rows a rectangle has not (`isCircle`, the arc's Angle, `isOpenShape`'s line
  // endings, and Fill and Fill Color, which `fillAvailable` refuses a PCB_TEXTBOX_T outright).
  const absent = (group: string, ...names: string[]): void => names.forEach((name) => row({ name, kind: "string", get: () => "", available: () => false }, group));
  absent("Pad Primitives", "Number Box", "Thermal Spoke Template");
  absent("Teardrops", "Enable Teardrops", "Best Length Ratio", "Max Length", "Best Width Ratio", "Max Width", "Curved Teardrops", "Prefer Zone Connections", "Allow Teardrops To Span Two Tracks", "Max Width Ratio");
  absent("Shape Properties", "Center X", "Center Y", "Radius", "Angle", "Fill", "Start Shape", "Start Length", "Start Width", "Start Stroke Width", "End Shape", "End Length", "End Width", "End Stroke Width", "Fill Color");
}
