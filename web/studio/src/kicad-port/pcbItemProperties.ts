// The Properties panel's rows for the board items GAPS.md item 24 (package P16) added: the text box (`PCB_TEXTBOX_DESC` and the `EDA_TEXT_DESC` it takes after, pcbnew/pcb_textbox.cpp,
// common/eda_text.cpp at KiCad 8303b2ad); reference images, points and barcodes. Called from `build()` of pcbProperties.ts, which keeps the registry; this file only
// adds classes to it. A text box takes after `BOARD_ITEM` (position, layer, lock) and has its own text rows: the same names and groups as `EDA_TEXT`'s, which edit one `pcb_item` verb of
// the whole box (`edit_text_box`) -- the free text's setters are the text's own verbs and a field's the footprint's.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BarcodeEcc, BarcodeKind, BoardBarcode, BoardGridItem, BoardPoint, BoardRefImage, BoardTextBox, BoardViaStitch } from "../api/pcbItemTypes";
import { LAYOUT_PROPERTY_CHOICES, layoutOf, MODE_CHOICES, modeOf, viaStitchCmd, withLayoutAndMode, type StitchLayout, type StitchMode } from "./viaStitch";
import { gridItemCmd, normalizedGridItem } from "./gridItem";
import type { Cmd } from "../api/types";
import { BARCODE_KINDS, barcodeToCmd, clampEcc, eccChoices, hasErrorCorrection } from "./barcodeShapes";
import type { PcbCtx, PcbItem } from "./pcbProperties";
import { tooSmall } from "./propertyGrid";
import type { Choice, PropValue, PropertyDef, PropertyManager } from "./propertyManager";
import { autoPenUm, clampPenUm, textBoxToCmd, TEXT_MAX_SIZE_UM, TEXT_MIN_SIZE_UM, withTextAngle } from "./pcbTextBox";
import { SCALE_PPM, scaleForHeightUm, scaleForWidthUm } from "./refImage";

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
  // `SetImageScale( aScale )` is ignored for a scale that is not positive; the image grows or shrinks about its centre (no transform origin offset).
  row({
    name: "Scale",
    kind: "double",
    get: (i) => own(i).scale_ppm / SCALE_PPM,
    validate: (v) => (num(v) <= 0 ? "Scale must be a positive number." : null),
    set: (i, v) => {
      const ppm = Math.round(num(v) * SCALE_PPM);
      return ppm <= 0 || ppm === own(i).scale_ppm ? [] : imageCmd(own(i), { scale_ppm: ppm });
    },
  });
  // `SetWidth` / `SetHeight`: `scaleBy( width / current width )`.
  const side = (name: "Width" | "Height"): void =>
    row({
      name,
      kind: "int",
      display: "size",
      get: (i) => (name === "Width" ? own(i).w : own(i).h),
      validate: (v, _i, c) => (num(v) <= 0 ? tooSmall(1, "size", c.units) : null),
      set: (i, v) => {
        const image = own(i);
        const want = Math.round(num(v));
        if (want <= 0 || want === (name === "Width" ? image.w : image.h)) return [];
        const ppm = name === "Width" ? scaleForWidthUm(image.px, image.ppi, want) : scaleForHeightUm(image.px, image.ppi, want);
        return ppm <= 0 || ppm === image.scale_ppm ? [] : imageCmd(image, { scale_ppm: ppm });
      },
    });
  side("Width");
  side("Height");
}

/** The cmd that puts point `p` on `layer`, at `at`, at `size`. */
function pointCmd(p: BoardPoint, patch: { layer?: string; size?: number }): Cmd[] {
  return [{ op: "pcb_item", verb: "edit_point", id: p.id, layer: patch.layer ?? p.layer, at: { x: p.x, y: p.y }, size_um: patch.size ?? p.size }];
}

/** Register `PCB_POINT` and its properties (`PCB_POINT_DESC`, pcbnew/pcb_point.cpp): the ones of a board item and a Size. The lock is not the point's: the file form has none. */
function registerPoint(pm: PM, add: Add, layerChoices: (c: PcbCtx) => Choice[]): void {
  pm.registerType("PCB_POINT");
  pm.inheritsAfter("PCB_POINT", "BOARD_ITEM");
  const own = (i: PcbItem): BoardPoint => i.point!;
  pm.replaceProperty("BOARD_ITEM", "Layer", {
    owner: "PCB_POINT",
    name: "Layer",
    kind: "enum",
    choices: (_i, c) => layerChoices(c),
    get: (i) => own(i).layer,
    set: (i, v) => (own(i).layer === String(v) ? [] : pointCmd(own(i), { layer: String(v) })),
  });
  pm.overrideWriteability("PCB_POINT", "BOARD_ITEM", "Locked", () => false);
  add({
    owner: "PCB_POINT",
    name: "Size",
    kind: "int",
    display: "size",
    get: (i) => own(i).size,
    validate: (v, _i, c) => (num(v) <= 0 ? tooSmall(1, "size", c.units) : null),
    set: (i, v) => {
      const size = Math.round(num(v));
      return size <= 0 || size === own(i).size ? [] : pointCmd(own(i), { size });
    },
  });
}

/** The cmd that replaces barcode `b` with `b` changed by `patch`: every field of it goes, its id, lock and groups stay (`edit_barcode`). */
function barcodeCmd(b: BoardBarcode, patch: Partial<BoardBarcode>): Cmd[] {
  const next: BoardBarcode = { ...b, ...patch };
  // `PCB_BARCODE::SetErrorCorrection`: a Micro QR code has no High.
  next.ecc = clampEcc(next.kind, next.ecc);
  return [{ op: "pcb_item", verb: "edit_barcode", id: b.id, barcode: barcodeToCmd(next) }];
}

/** Register `PCB_BARCODE` and its properties (`PCB_BARCODE_DESC`, pcbnew/pcb_barcode.cpp): the rows of a board item and, under "Barcode Properties", the text, its size and whether it is shown, the size and
 * turn of the symbol, the kind, the error correction of the QR kinds (Micro QR has no High) and the knockout with its margins. */
function registerBarcode(pm: PM, add: Add, layerChoices: (c: PcbCtx) => Choice[]): void {
  pm.registerType("PCB_BARCODE");
  pm.inheritsAfter("PCB_BARCODE", "BOARD_ITEM");
  const own = (i: PcbItem): BoardBarcode => i.barcode!;
  pm.replaceProperty("BOARD_ITEM", "Layer", {
    owner: "PCB_BARCODE",
    name: "Layer",
    kind: "enum",
    choices: (_i, c) => layerChoices(c),
    get: (i) => own(i).layer,
    set: (i, v) => (own(i).layer === String(v) ? [] : barcodeCmd(own(i), { layer: String(v) })),
  });
  const GROUP = "Barcode Properties";
  const row = (def: Omit<PropertyDef<PcbItem, PcbCtx, Cmd>, "owner">): void => add({ owner: "PCB_BARCODE", ...def } as PropertyDef<PcbItem, PcbCtx, Cmd>, GROUP);
  const size = (name: string, get: (b: BoardBarcode) => number, key: keyof BoardBarcode): void =>
    row({
      name,
      kind: "int",
      display: "size",
      get: (i) => get(own(i)),
      validate: (v, _i, c) => (num(v) <= 0 ? tooSmall(1, "size", c.units) : null),
      set: (i, v) => {
        const want = Math.round(num(v));
        return want <= 0 || want === get(own(i)) ? [] : barcodeCmd(own(i), { [key]: want } as Partial<BoardBarcode>);
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
  const via = (i: PcbItem) => own(i).via ?? { size_um: 0, drill_um: 0, from_layer: "F.Cu", to_layer: "B.Cu" };
  add({ owner: "PCB_VIA_STITCH", name: "Size", kind: "int", display: "size", get: (i) => via(i).size_um, set: (i, v) => edit(i, { ...own(i), via: { ...via(i), size_um: Math.round(num(v)) } }) });
  add({ owner: "PCB_VIA_STITCH", name: "Drill", kind: "int", display: "size", get: (i) => via(i).drill_um, set: (i, v) => edit(i, { ...own(i), via: { ...via(i), drill_um: Math.round(num(v)) } }) });
  add({ owner: "PCB_VIA_STITCH", name: "Pitch", kind: "int", display: "size", get: (i) => own(i).pitch_um ?? 0, set: (i, v) => edit(i, { ...own(i), pitch_um: Math.round(num(v)) }) });
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

export function registerExtraItemProperties(pm: PM, add: Add, layerChoices: (c: PcbCtx) => Choice[]): void {
  registerGridItem(pm, add);
  registerViaStitch(pm, add);
  registerReferenceImage(pm, add, layerChoices);
  registerPoint(pm, add, layerChoices);
  registerBarcode(pm, add, layerChoices);
  pm.registerType("PCB_TEXTBOX");
  pm.inheritsAfter("PCB_TEXTBOX", "BOARD_ITEM");
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

  row(
    {
      name: "Text",
      kind: "string",
      get: (i) => own(i).text,
      set: (i, v) => (own(i).text === String(v) ? [] : boxCmd(own(i), { text: String(v) })),
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
        const pen = w > 0 ? clampPenUm(w, Math.min(b.size, b.width)) : 0;
        return boxCmd(b, pen > 0 ? { thickness: pen, thickness_auto: false } : { thickness: autoPenUm(Math.min(b.size, b.width), b.bold), thickness_auto: true });
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
          const pen = key === "bold" && b.thickness_auto ? { thickness: autoPenUm(Math.min(b.size, b.width), Boolean(v)) } : {};
          return boxCmd(b, { [key]: Boolean(v), ...pen });
        },
      },
      TEXT
    );
  flag("Italic", "italic");
  flag("Bold", "bold");
  flag("Mirrored", "mirror");
  flag("Knockout", "knockout");
  const glyph = (name: "Width" | "Height"): void =>
    row(
      {
        name,
        kind: "int",
        display: "size",
        get: (i) => (name === "Width" ? own(i).width : own(i).size),
        validate: (v, _i, c) => (num(v) < TEXT_MIN_SIZE_UM ? tooSmall(TEXT_MIN_SIZE_UM, "size", c.units) : num(v) > TEXT_MAX_SIZE_UM ? tooSmall(TEXT_MAX_SIZE_UM, "size", c.units) : null),
        set: (i, v) => {
          const b = own(i);
          const s = Math.round(num(v));
          if ((name === "Width" ? b.width : b.size) === s) return [];
          const patch = name === "Width" ? { width: s } : { size: s };
          const size = Math.min(patch.width ?? b.width, patch.size ?? b.size);
          return boxCmd(b, { ...patch, ...(b.thickness_auto ? { thickness: autoPenUm(size, b.bold) } : {}) });
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
    TEXT
  );

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
}
