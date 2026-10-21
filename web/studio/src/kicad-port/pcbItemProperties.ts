// The Properties panel's rows for the board items GAPS.md item 24 (package P16) added: the text box (`PCB_TEXTBOX_DESC` and the `EDA_TEXT_DESC` it takes after, pcbnew/pcb_textbox.cpp,
// common/eda_text.cpp at KiCad 8303b2ad); reference images, points and barcodes as their steps land. Called from `build()` of pcbProperties.ts, which keeps the registry; this file only
// adds classes to it. A text box takes after `BOARD_ITEM` (position, layer, lock) and has its own text rows: the same names and groups as `EDA_TEXT`'s, which edit one `pcb_item` verb of
// the whole box (`edit_text_box`) -- the free text's setters are the text's own verbs and a field's the footprint's.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardTextBox } from "../api/pcbItemTypes";
import type { Cmd } from "../api/types";
import type { PcbCtx, PcbItem } from "./pcbProperties";
import { tooSmall } from "./propertyGrid";
import type { Choice, PropValue, PropertyDef, PropertyManager } from "./propertyManager";
import { autoPenUm, clampPenUm, textBoxToCmd, TEXT_MAX_SIZE_UM, TEXT_MIN_SIZE_UM, withTextAngle } from "./pcbTextBox";

type PM = PropertyManager<PcbItem, PcbCtx, Cmd>;
type Add = (def: PropertyDef<PcbItem, PcbCtx, Cmd>, group?: string) => void;

const num = (v: PropValue): number => (typeof v === "number" ? v : Number(v));
const norm360 = (a: number): number => ((a % 360) + 360) % 360;

/** The cmd that replaces text box `b` with `b` changed by `patch`. */
function boxCmd(b: BoardTextBox, patch: Partial<BoardTextBox>): Cmd[] {
  const next: BoardTextBox = { ...b, ...patch };
  return [{ op: "pcb_item", verb: "edit_text_box", id: b.id, text_box: textBoxToCmd(next) }];
}

/** Register `PCB_TEXTBOX` and its properties. */
export function registerExtraItemProperties(pm: PM, add: Add, layerChoices: (c: PcbCtx) => Choice[]): void {
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
