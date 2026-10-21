// The Properties panel's rows for the board items GAPS.md item 24 (package P16) added: the text box (`PCB_TEXTBOX_DESC` and the `EDA_TEXT_DESC` it takes after, pcbnew/pcb_textbox.cpp,
// common/eda_text.cpp at KiCad 8303b2ad); reference images, points and barcodes as their steps land. Called from `build()` of pcbProperties.ts, which keeps the registry; this file only
// adds classes to it. A text box takes after `BOARD_ITEM` (position, layer, lock) and has its own text rows: the same names and groups as `EDA_TEXT`'s, which edit one `pcb_item` verb of
// the whole box (`edit_text_box`) -- the free text's setters are the text's own verbs and a field's the footprint's.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardPoint, BoardRefImage, BoardTextBox } from "../api/pcbItemTypes";
import type { Cmd } from "../api/types";
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

/** Register `PCB_TEXTBOX` and its properties, and the other classes of the family. */
export function registerExtraItemProperties(pm: PM, add: Add, layerChoices: (c: PcbCtx) => Choice[]): void {
  registerReferenceImage(pm, add, layerChoices);
  registerPoint(pm, add, layerChoices);
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
