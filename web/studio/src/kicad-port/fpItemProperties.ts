// The Footprint Editor's Properties pane (UI-LEFT Y3, the footprint half): KiCad's footprint editor runs the board's `PCB_PROPERTIES_PANEL` (pcbnew/widgets/
// pcb_properties_panel.cpp at KiCad 8303b2ad) on the one footprint of its board, so the rows are the board item classes' own `PROPERTY_MANAGER` registrations --
// `BOARD_ITEM_DESC`, `BOARD_CONNECTED_ITEM_DESC`, `FOOTPRINT_DESC`, `PAD_DESC`, `EDA_TEXT_DESC`, `PCB_TEXT_DESC`, `PCB_FIELD_DESC`, `EDA_SHAPE_DESC`, `PCB_SHAPE_DESC` (the
// table kicad-port/pcbKicadRegistrations.ts holds, which the board's panel, kicad-port/pcbProperties.ts, is tested against too) -- shown as a library editor shows them
// (`PROPERTIES_PANEL::rebuildProperties`' `isLibraryEditor`: the Net, Pin Name, Pin Type and Component Class rows are hidden from it). With nothing selected the panel shows
// the footprint itself (`PCB_PROPERTIES_PANEL::getSelection`: `if( selection.Empty() && m_frame->IsType( FRAME_FOOTPRINT_EDITOR ) )` the board's first footprint).
//
// The getters read the library footprint the editor holds; the setters return the Footprint Editor's verbs -- `edit_pad` (the whole pad), `fp_item set_graphic` / `set_text`
// (with the looks the footprint keeps by id, UI-LEFT F5) / `edit_field` / `move`, `edit_footprint_properties`, `fp_editor set_settings` -- which an edit sends as ONE batch on
// the editor's own undo stack (`BOARD_COMMIT::Push( "Edit Properties" )`). What the library footprint here has no place for, or no verb sets, is read-only with the reason as
// the row's tooltip.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`). Lengths are nm in the grid (the panel's unit).
import type { Cmd, CmdShape, CmdText, FieldInfo, LibraryFootprint, LibraryPad, PadConnection, Shape } from "../api/types";
import type { FootprintSettings, TextLook } from "../api/fpEditorTypes";
import type { LengthUnit } from "../state/units";
import { buildGrid, planEdit, type EditPlan, type GridModel } from "./propertyGrid";
import { PropertyManager, type Choice, type PropValue, type PropertyDef } from "./propertyManager";
import { fieldLayoutOf, fpFieldById, fpFields } from "./fpItems";
import { shapeLookOf, textLookOf, isLockedItem, LINE_STYLE_CHOICES, FILL_CHOICES, fillChoiceOf, fillOfChoice } from "./fpKeep";
import { cmdShapeToShape } from "./itemBoxes";
import { shapeWith } from "./pcbProperties";
import { FAB_PROPERTY_CHOICES, padToDieOf, withPadFab } from "./padLayers";
import { PAD_CONNECTION_OPTIONS } from "./padSettings";
import { FP_EDITOR_LAYERS } from "./fpDrawDefaults";

// ------------------------------------------------------------------------------------------------------------------------------- items

/** The class of a footprint editor item (`TYPE_HASH( *item )`). */
export type FpPropType = "FOOTPRINT" | "PAD" | "PCB_SHAPE" | "PCB_TEXT" | "PCB_FIELD";

export interface FpPropItem {
  readonly type: FpPropType;
  readonly id: string;
  readonly pad?: LibraryPad;
  readonly shape?: CmdShape;
  readonly text?: CmdText;
  readonly field?: FieldInfo;
}

/** What the getters read besides the item. */
export interface FpPropCtx {
  fp: LibraryFootprint;
  /** The key the editor holds the footprint by (a board copy's is not its name). */
  name: string;
  units: LengthUnit;
}

/** The item a selection id names (a pad, a graphic, a text, a field `field:<name>`), or null for the kinds the pane does not list (dimensions, zones ...). */
export function fpPropItemOf(fp: LibraryFootprint, id: string): FpPropItem | null {
  const pad = fp.pads.find((p) => p.id === id);
  if (pad) return { type: "PAD", id, pad };
  const shape = fp.graphics.find((g) => g.id === id);
  if (shape) return { type: "PCB_SHAPE", id, shape };
  const text = fp.texts.find((t) => t.id === id);
  if (text) return { type: "PCB_TEXT", id, text };
  const field = fpFieldById(fp, id);
  if (field) return { type: "PCB_FIELD", id, field };
  return null;
}

/** `getSelection`: the selected items the pane lists, in selection order -- or, with nothing selected, the footprint. */
export function fpPropItemsOf(fp: LibraryFootprint, ids: Iterable<string>): FpPropItem[] {
  const all = [...ids];
  if (all.length === 0) return [{ type: "FOOTPRINT", id: "" }];
  return all.map((id) => fpPropItemOf(fp, id)).filter((x): x is FpPropItem => !!x);
}

/** `GetFriendlyName()`. */
export function fpFriendlyName(i: FpPropItem): string {
  switch (i.type) {
    case "FOOTPRINT":
      return "Footprint";
    case "PAD":
      return "Pad";
    case "PCB_TEXT":
      return "Text";
    case "PCB_FIELD":
      return "Field";
    case "PCB_SHAPE":
      switch (i.shape?.kind) {
        case "segment":
          return "Segment";
        case "rect":
          return "Rectangle";
        case "circle":
          return "Circle";
        case "arc":
          return "Arc";
        case "bezier":
          return "Curve";
        default:
          return "Polygon";
      }
  }
}

// ------------------------------------------------------------------------------------------------------------------------------- helpers

type PD = PropertyDef<FpPropItem, FpPropCtx, Cmd>;
const num = (v: PropValue): number => (typeof v === "number" ? v : Number(v));
const TEXT = "Text Properties";
const SHAPE = "Shape Properties";
const PAD_GROUP = "Pad Properties";
const OVERRIDES = "Overrides";

const READ_ONLY = {
  locked: "A footprint's items keep the lock their file gave them; the Footprint Editor does not set one.",
  font: "A footprint text is drawn in KiCad's stroke font here; the font is not chosen.",
  autoThickness: "A footprint text keeps its file's automatic pen; it is not switched here.",
  color: "A footprint item keeps no colour.",
  textVisible: "A footprint text's Show is not offered for it (`DIALOG_TEXT_PROPERTIES` hides it); a field's is the field row's.",
  padTypeShape: "Choose the pad's type, shape and hole in Pad Properties: a choice resets what goes with it (`PadTypeSelected`, `onPadShapeSelection`).",
  padstack: "Post-machining and backdrilling are a board's; a library pad keeps none.",
  teardrops: "Teardrops are a board's; a library pad keeps the file's teardrop settings as they are.",
  copperLayers: "The copper layers of a pad are set in Pad Properties' copper choice.",
  shapeKind: "A shape's kind is not changed here (draw the other kind).",
  corner: "A rectangle of a footprint keeps no corner radius here.",
  endings: "Line endings are KiCad 10's; a library footprint's lines have none here.",
  primitive: "The proxy items of a custom pad are the pad's (Edit Pad as Graphic Shapes).",
  soldermask: "A graphic's solder mask opening is a board's.",
  footprintOrientation: "A library footprint has no orientation: its items turn.",
  libraryLink: "The footprint's name is changed in Footprint Properties (it renames the library entry).",
  pad_to_die_delay: "The pad-to-die delay is set in Pad Properties.",
};

const layerChoices = (): Choice[] => FP_EDITOR_LAYERS.map((l) => ({ label: l, value: l }));

/** `BOARD_ITEM::GetPosition()` of a footprint item: a pad's and a text's place, a field's anchor, a shape's start (a circle's or an arc's centre, a polygon's first corner). */
function positionOf(i: FpPropItem): [number, number] {
  if (i.pad) return [i.pad.at.x, i.pad.at.y];
  if (i.text) return [i.text.at.x, i.text.at.y];
  if (i.field) return [i.field.x, i.field.y];
  const s = i.shape;
  if (!s) return [0, 0];
  switch (s.kind) {
    case "circle":
      return [s.center.x, s.center.y];
    case "arc": {
      const c = cmdShapeToShape(s);
      return c.kind === "arc" ? arcCentre(c) : [s.start.x, s.start.y];
    }
    case "polygon":
      return s.pts[0] ? [s.pts[0].x, s.pts[0].y] : [0, 0];
    default:
      return [s.start.x, s.start.y];
  }
}

function arcCentre(a: Extract<Shape, { kind: "arc" }>): [number, number] {
  const [ax, ay] = a.start;
  const [bx, by] = a.mid;
  const [cx, cy] = a.end;
  const d = 2 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
  if (d === 0) return [ax, ay];
  const ux = ((ax * ax + ay * ay) * (by - cy) + (bx * bx + by * by) * (cy - ay) + (cx * cx + cy * cy) * (ay - by)) / d;
  const uy = ((ax * ax + ay * ay) * (cx - bx) + (bx * bx + by * by) * (ax - cx) + (cx * cx + cy * cy) * (bx - ax)) / d;
  return [Math.round(ux), Math.round(uy)];
}

/** The board `Shape` back as the editor's `CmdShape` (the inverse of `cmdShapeToShape`). */
function toCmdShape(s: Shape, like: CmdShape): CmdShape {
  const p = (q: [number, number]) => ({ x: q[0], y: q[1] });
  const common = { id: like.id, layer: s.layer, stroke_width: s.stroke_width, filled: s.filled };
  switch (s.kind) {
    case "segment":
      return { ...common, kind: "segment", start: p(s.start), end: p(s.end) };
    case "arc":
      return { ...common, kind: "arc", start: p(s.start), mid: p(s.mid), end: p(s.end) };
    case "rect":
      return { ...common, kind: "rect", start: p(s.start), end: p(s.end) };
    case "circle":
      return { ...common, kind: "circle", center: p(s.center), end: p(s.end) };
    case "polygon":
      return { ...common, kind: "polygon", pts: s.pts.map(p) };
    case "bezier":
      return { ...common, kind: "bezier", start: p(s.start), c1: p(s.c1), c2: p(s.c2), end: p(s.end) };
  }
}

const graphicCmd = (c: FpPropCtx, s: CmdShape, look?: ReturnType<typeof shapeLookOf>): Cmd => ({ op: "fp_item", verb: "set_graphic", footprint: c.name, id: s.id!, shape: s, ...(look ? { look } : {}) });
const textCmd = (c: FpPropCtx, t: CmdText, look?: TextLook): Cmd => ({ op: "fp_item", verb: "set_text", footprint: c.name, id: t.id!, text: t, ...(look ? { look } : {}) });
const padCmd = (c: FpPropCtx, p: LibraryPad): Cmd => ({ op: "edit_pad", footprint: c.name, id: p.id!, pad: p });
const fieldCmd = (c: FpPropCtx, f: FieldInfo, patch: Partial<ReturnType<typeof fieldLayoutOf>>, text?: string): Cmd => ({
  op: "fp_item",
  verb: "edit_field",
  footprint: c.name,
  name: f.name,
  text: text ?? "",
  layout: { ...fieldLayoutOf(f), ...patch },
});

/** A pad with `patch`, or nothing when it already is. */
const padWith = (i: FpPropItem, c: FpPropCtx, patch: Partial<LibraryPad>): Cmd[] => {
  const next = { ...i.pad!, ...patch };
  return JSON.stringify(next) === JSON.stringify(i.pad) ? [] : [padCmd(c, next)];
};
/** A text with `patch` and, when given, its looks. */
const textWith = (i: FpPropItem, c: FpPropCtx, patch: Partial<CmdText>, look?: Partial<TextLook>): Cmd[] => {
  const t = i.text!;
  const next = { ...t, ...patch };
  const was = textLookOf(c.fp, t.id!);
  const nextLook = look ? { ...was, ...look } : undefined;
  if (JSON.stringify(next) === JSON.stringify(t) && (!nextLook || JSON.stringify(nextLook) === JSON.stringify(was))) return [];
  return [textCmd(c, next, nextLook)];
};

/** The footprint's settings with `patch` (`fp_editor set_settings` takes them whole). */
const settingsWith = (c: FpPropCtx, patch: Partial<FootprintSettings>): Cmd[] => {
  const before = c.fp.settings ?? {};
  const next = { ...before, ...patch };
  return JSON.stringify(next) === JSON.stringify(before) ? [] : [{ op: "fp_editor", verb: "set_settings", footprint: c.name, settings: next }];
};
/** `edit_footprint_properties` with every field as the footprint has it, and `patch`. */
const propertiesWith = (c: FpPropCtx, patch: Partial<Pick<LibraryFootprint, "description" | "keywords" | "attributes">>): Cmd[] => {
  const fp = c.fp;
  const before = { description: fp.description ?? "", keywords: fp.keywords ?? "", attributes: fp.attributes };
  const next = { ...before, ...patch };
  if (JSON.stringify(next) === JSON.stringify(before)) return [];
  return [{ op: "edit_footprint_properties", name: c.name, ...next, reference_visible: fp.reference_visible, value_visible: fp.value_visible, model: fp.model ?? null, zone_connection: fp.zone_connection ?? null }];
};

const connectionChoices: Choice[] = PAD_CONNECTION_OPTIONS.map((o) => ({ label: o.label, value: o.value }));

// ------------------------------------------------------------------------------------------------------------------------------- the registry

function build(): PropertyManager<FpPropItem, FpPropCtx, Cmd> {
  const pm = new PropertyManager<FpPropItem, FpPropCtx, Cmd>();
  const add = (def: PD, group = "") => pm.addProperty(def, group);
  const ro = (owner: string, name: string, group: string, reason: string, get: PD["get"] = () => "", extra: Partial<PD> = {}) => add({ owner, name, kind: "string", get, readOnlyReason: reason, ...extra }, group);

  // ------------------------------------------------------------------------------------------------------- EDA_ITEM, BOARD_ITEM
  pm.registerType("EDA_ITEM");
  add({ owner: "EDA_ITEM", name: "Type", kind: "string", hidden: true, get: (i) => i.type });
  pm.inheritsAfter("BOARD_ITEM", "EDA_ITEM");
  add({ owner: "BOARD_ITEM", name: "Parent", kind: "string", hidden: true, hiddenFromLibraryEditors: true, get: () => "" });
  /** A move of the item: `fp_item move` (a pad, a graphic, a text, a field alike). */
  const setPosition = (axis: 0 | 1) => (i: FpPropItem, v: PropValue, c: FpPropCtx): Cmd[] => {
    const delta = Math.round(num(v)) - positionOf(i)[axis];
    if (delta === 0 || !Number.isFinite(delta)) return [];
    const [dx, dy] = axis === 0 ? [delta, 0] : [0, delta];
    return [{ op: "fp_item", verb: "move", footprint: c.name, ids: [i.id], dx, dy }];
  };
  add({ owner: "BOARD_ITEM", name: "Position X", kind: "int", display: "coord", get: (i) => positionOf(i)[0], set: setPosition(0), available: (i) => i.type !== "FOOTPRINT" });
  add({ owner: "BOARD_ITEM", name: "Position Y", kind: "int", display: "coord", get: (i) => positionOf(i)[1], set: setPosition(1), available: (i) => i.type !== "FOOTPRINT" });
  add({
    owner: "BOARD_ITEM",
    name: "Layer",
    kind: "enum",
    choices: () => layerChoices(),
    get: (i) => i.text?.layer ?? i.field?.layer ?? "",
    set: (i, v, c) => {
      const layer = String(v);
      if (i.text) return textWith(i, c, { layer });
      if (i.field) return i.field.layer === layer ? [] : [fieldCmd(c, i.field, { layer })];
      return [];
    },
  });
  add({ owner: "BOARD_ITEM", name: "Locked", kind: "bool", get: (i, c) => isLockedItem(c.fp, i.id), readOnlyReason: READ_ONLY.locked });

  // ------------------------------------------------------------------------------------------------------ BOARD_CONNECTED_ITEM
  pm.inheritsAfter("BOARD_CONNECTED_ITEM", "BOARD_ITEM");
  pm.replaceProperty("BOARD_ITEM", "Layer", { owner: "BOARD_CONNECTED_ITEM", name: "Layer", kind: "enum", get: () => "" });
  add({ owner: "BOARD_CONNECTED_ITEM", name: "Net", kind: "net", hiddenFromLibraryEditors: true, get: () => "" });
  for (const n of ["Net Class", "NetClass", "NetName"]) add({ owner: "BOARD_CONNECTED_ITEM", name: n, kind: "string", hidden: true, hiddenFromLibraryEditors: true, get: () => "" });
  for (const n of ["Enable Teardrops", "Best Length Ratio", "Max Length", "Best Width Ratio", "Max Width", "Curved Teardrops", "Prefer Zone Connections", "Allow Teardrops To Span Two Tracks", "Max Width Ratio"]) {
    ro("BOARD_CONNECTED_ITEM", n, "Teardrops", READ_ONLY.teardrops, () => "", { available: (i) => i.type === "PAD" });
  }

  // ------------------------------------------------------------------------------------------------------------------ FOOTPRINT
  pm.inheritsAfter("FOOTPRINT", "BOARD_ITEM");
  pm.inheritsAfter("FOOTPRINT", "BOARD_ITEM_CONTAINER");
  pm.replaceProperty("BOARD_ITEM", "Layer", { owner: "FOOTPRINT", name: "Layer", kind: "enum", get: () => "F.Cu", readOnlyReason: "A library footprint is on the front." });
  add({ owner: "FOOTPRINT", name: "Orientation", kind: "double", display: "degree", get: () => 0, readOnlyReason: READ_ONLY.footprintOrientation });
  add(
    {
      owner: "FOOTPRINT",
      name: "Reference",
      kind: "string",
      get: (_i, c) => fpFields(c.fp).find((f) => f.name === "Reference")?.text ?? "",
      set: (_i, v, c) => {
        const f = fpFields(c.fp).find((x) => x.name === "Reference")!;
        return f.text === String(v) || String(v) === "" ? [] : [fieldCmd(c, f, {}, String(v))];
      },
    },
    "Fields"
  );
  const PROPS = "Footprint Properties";
  ro("FOOTPRINT", "Library Link", PROPS, READ_ONLY.libraryLink, (_i, c) => c.fp.board_link?.fpid ?? c.fp.name);
  add({ owner: "FOOTPRINT", name: "Library Description", kind: "string", get: (_i, c) => c.fp.description ?? "", set: (_i, v, c) => propertiesWith(c, { description: String(v) }) }, PROPS);
  add({ owner: "FOOTPRINT", name: "Keywords", kind: "string", get: (_i, c) => c.fp.keywords ?? "", set: (_i, v, c) => propertiesWith(c, { keywords: String(v) }) }, PROPS);
  add({ owner: "FOOTPRINT", name: "Component Class", kind: "string", hiddenFromLibraryEditors: true, get: () => "" }, PROPS);
  const attr = (name: string, key: "board_only" | "exclude_from_position_files" | "exclude_from_bom" | "dnp" | "allow_missing_courtyard", group: string) =>
    add({ owner: "FOOTPRINT", name, kind: "bool", get: (_i, c) => !!c.fp.attributes[key], set: (_i, v, c) => propertiesWith(c, { attributes: { ...c.fp.attributes, [key]: Boolean(v) } }) }, group);
  attr("Not in Schematic", "board_only", "Attributes");
  attr("Exclude From Position Files", "exclude_from_position_files", "Attributes");
  attr("Exclude From Bill of Materials", "exclude_from_bom", "Attributes");
  attr("Do not Populate", "dnp", "Attributes");
  attr("Exempt From Courtyard Requirement", "allow_missing_courtyard", OVERRIDES);
  const fpLength = (name: string, key: "clearance" | "solder_paste_margin") =>
    add({ owner: "FOOTPRINT", name, kind: "int", display: "size", optional: true, get: (_i, c) => c.fp.settings?.[key] ?? "", set: (_i, v, c) => settingsWith(c, { [key]: v === "" ? null : Math.round(num(v)) }) }, OVERRIDES);
  fpLength("Clearance Override", "clearance");
  fpLength("Solderpaste Margin Override", "solder_paste_margin");
  add(
    { owner: "FOOTPRINT", name: "Solderpaste Margin Ratio Override", kind: "double", display: "ratio", optional: true, get: (_i, c) => c.fp.settings?.solder_paste_margin_ratio ?? "", set: (_i, v, c) => settingsWith(c, { solder_paste_margin_ratio: v === "" ? null : num(v) }) },
    OVERRIDES
  );
  add(
    {
      owner: "FOOTPRINT",
      name: "Zone Connection Style",
      kind: "enum",
      choices: () => [{ label: "Inherited", value: "" }, ...connectionChoices.filter((o) => o.value !== "")],
      get: (_i, c) => c.fp.zone_connection ?? "",
      set: (_i, v, c) => {
        const zc = (v === "" ? null : (String(v) as PadConnection)) ?? null;
        if ((c.fp.zone_connection ?? null) === zc) return [];
        const fp = c.fp;
        return [{ op: "edit_footprint_properties", name: c.name, description: fp.description ?? "", keywords: fp.keywords ?? "", attributes: fp.attributes, reference_visible: fp.reference_visible, value_visible: fp.value_visible, model: fp.model ?? null, zone_connection: zc }];
      },
    },
    OVERRIDES
  );

  // ------------------------------------------------------------------------------------------------------------------------ PAD
  pm.inheritsAfter("PAD", "BOARD_ITEM");
  pm.inheritsAfter("PAD", "BOARD_CONNECTED_ITEM");
  pm.mask("PAD", "BOARD_CONNECTED_ITEM", "Layer");
  pm.mask("PAD", "BOARD_ITEM", "Locked");
  add({ owner: "PAD", name: "Orientation", kind: "double", display: "degree", get: (i) => (i.pad ? -i.pad.rot / 1000 : 0), set: (i, v, c) => padWith(i, c, { rot: (((Math.round(-num(v) * 1000) % 360_000) + 360_000) % 360_000) }) });
  ro("PAD", "Pad Type", PAD_GROUP, READ_ONLY.padTypeShape, (i) => i.pad?.kind ?? "");
  ro("PAD", "Pad Shape", PAD_GROUP, READ_ONLY.padTypeShape, (i) => i.pad?.shape ?? "");
  add({ owner: "PAD", name: "Pad Number", kind: "string", get: (i) => i.pad?.number ?? "", set: (i, v, c) => padWith(i, c, { number: String(v) }) }, PAD_GROUP);
  add({ owner: "PAD", name: "Pin Name", kind: "string", hiddenFromLibraryEditors: true, get: () => "" }, PAD_GROUP);
  add({ owner: "PAD", name: "Pin Type", kind: "string", hiddenFromLibraryEditors: true, get: () => "" }, PAD_GROUP);
  const sizeRow = (name: "Size X" | "Size Y", axis: 0 | 1) =>
    add(
      {
        owner: "PAD",
        name,
        kind: "int",
        display: "size",
        get: (i) => i.pad?.size[axis] ?? 0,
        validate: (v) => (num(v) > 0 ? null : "The pad size must be greater than zero."),
        set: (i, v, c) => {
          const size: [number, number] = [...i.pad!.size];
          size[axis] = Math.round(num(v));
          return padWith(i, c, { size });
        },
      },
      PAD_GROUP
    );
  sizeRow("Size X", 0);
  sizeRow("Size Y", 1);
  const isRoundRect = (i: FpPropItem) => i.pad?.shape === "round_rect" || i.pad?.shape === "chamfered_rect";
  add({ owner: "PAD", name: "Corner Radius Ratio", kind: "double", display: "ratio", available: isRoundRect, get: (i) => i.pad?.roundrect_ratio ?? 0.25, set: (i, v, c) => padWith(i, c, { roundrect_ratio: Math.min(0.5, Math.max(0, num(v))) }) }, PAD_GROUP);
  add({
    owner: "PAD",
    name: "Corner Radius Size",
    kind: "int",
    display: "size",
    available: isRoundRect,
    get: (i) => Math.round((i.pad?.roundrect_ratio ?? 0.25) * Math.min(i.pad?.size[0] ?? 0, i.pad?.size[1] ?? 0)),
    set: (i, v, c) => {
      const shorter = Math.min(i.pad!.size[0], i.pad!.size[1]);
      return shorter > 0 ? padWith(i, c, { roundrect_ratio: Math.min(0.5, Math.max(0, num(v) / shorter)) }) : [];
    },
  }, PAD_GROUP);
  const hasHole = (i: FpPropItem) => i.pad?.kind === "through_hole" || i.pad?.kind === "non_plated_hole";
  ro("PAD", "Hole Shape", PAD_GROUP, READ_ONLY.padTypeShape, (i) => (i.pad?.drill_slot ? "Oblong" : "Round"), { available: hasHole });
  const holeRow = (name: "Hole Size X" | "Hole Size Y", axis: 0 | 1) =>
    add(
      {
        owner: "PAD",
        name,
        kind: "int",
        display: "size",
        available: hasHole,
        get: (i) => (i.pad?.drill_slot ? i.pad.drill_slot[axis] : (i.pad?.drill ?? 0)),
        validate: (v) => (num(v) > 0 ? null : "The hole size must be greater than zero."),
        set: (i, v, c) => {
          const p = i.pad!;
          const d = Math.round(num(v));
          if (p.drill_slot) {
            const slot: [number, number] = [...p.drill_slot];
            slot[axis] = d;
            return padWith(i, c, { drill_slot: slot });
          }
          return padWith(i, c, { drill: d });
        },
      },
      PAD_GROUP
    );
  holeRow("Hole Size X", 0);
  holeRow("Hole Size Y", 1);
  for (const n of ["Top Post-machining", "Top Post-machining Size", "Top Counterbore Depth", "Top Countersink Angle", "Bottom Post-machining", "Bottom Post-machining Size", "Bottom Counterbore Depth", "Bottom Countersink Angle"]) ro("PAD", n, "Post-machining Properties", READ_ONLY.padstack, () => "", { available: hasHole });
  for (const n of ["Backdrill Mode", "Bottom Backdrill Size", "Bottom Backdrill Must-Cut", "Top Backdrill Size", "Top Backdrill Must-Cut"]) ro("PAD", n, "Backdrill Properties", READ_ONLY.padstack, () => "", { available: hasHole });
  const keepOf = (i: FpPropItem) => (i.pad?.kicad ?? {}) as { property?: string | null; tokens?: string[]; solder_mask_margin?: number | null; solder_paste_margin?: number | null; solder_paste_margin_ratio?: number | null };
  add(
    {
      owner: "PAD",
      name: "Fabrication Property",
      kind: "enum",
      choices: () => FAB_PROPERTY_CHOICES.map((c) => ({ label: c.label, value: c.token ?? "" })),
      get: (i) => keepOf(i).property ?? "",
      set: (i, v, c) => {
        const k = keepOf(i);
        const die = padToDieOf(k);
        return padWith(i, c, { kicad: withPadFab(k, String(v) || null, die.length, die.delay) as LibraryPad["kicad"] });
      },
    },
    PAD_GROUP
  );
  ro("PAD", "Copper Layers", PAD_GROUP, READ_ONLY.copperLayers, (i) => (i.pad?.layers ?? []).filter((l) => l.endsWith(".Cu")).join(", "));
  add(
    {
      owner: "PAD",
      name: "Pad To Die Length",
      kind: "int",
      display: "size",
      get: (i) => padToDieOf(keepOf(i)).length ?? 0,
      set: (i, v, c) => {
        const k = keepOf(i);
        const die = padToDieOf(k);
        return padWith(i, c, { kicad: withPadFab(k, k.property ?? null, Math.round(num(v)) || null, die.delay) as LibraryPad["kicad"] });
      },
    },
    PAD_GROUP
  );
  ro("PAD", "Pad To Die Delay", PAD_GROUP, READ_ONLY.pad_to_die_delay, (i) => padToDieOf(keepOf(i)).delay ?? 0);
  const padOverride = (name: string, key: "clearance_override" | "thermal_gap_override" | "thermal_spoke_width_override") =>
    add({ owner: "PAD", name, kind: "int", display: "size", optional: true, get: (i) => i.pad?.[key] ?? "", set: (i, v, c) => padWith(i, c, { [key]: v === "" ? null : Math.round(num(v)) }) }, OVERRIDES);
  const keepMargin = (name: string, key: "solder_mask_margin" | "solder_paste_margin" | "solder_paste_margin_ratio", ratio = false) =>
    add(
      {
        owner: "PAD",
        name,
        kind: ratio ? "double" : "int",
        display: ratio ? "ratio" : "size",
        optional: true,
        get: (i) => keepOf(i)[key] ?? "",
        set: (i, v, c) => {
          const k = { ...keepOf(i) } as Record<string, unknown>;
          if (v === "") delete k[key];
          else k[key] = ratio ? num(v) : Math.round(num(v));
          return padWith(i, c, { kicad: k as LibraryPad["kicad"] });
        },
      },
      OVERRIDES
    );
  padOverride("Clearance Override", "clearance_override");
  keepMargin("Soldermask Margin Override", "solder_mask_margin");
  keepMargin("Solderpaste Margin Override", "solder_paste_margin");
  keepMargin("Solderpaste Margin Ratio Override", "solder_paste_margin_ratio", true);
  add(
    {
      owner: "PAD",
      name: "Zone Connection Style",
      kind: "enum",
      choices: () => connectionChoices,
      get: (i) => i.pad?.zone_connection ?? "",
      set: (i, v, c) => padWith(i, c, { zone_connection: v === "" ? null : (String(v) as PadConnection) }),
    },
    OVERRIDES
  );
  padOverride("Thermal Relief Spoke Width", "thermal_spoke_width_override");
  add(
    { owner: "PAD", name: "Thermal Relief Spoke Angle", kind: "double", display: "degree", optional: true, get: (i) => (i.pad?.thermal_spoke_angle_mdeg != null ? i.pad.thermal_spoke_angle_mdeg / 1000 : ""), set: (i, v, c) => padWith(i, c, { thermal_spoke_angle_mdeg: v === "" ? null : Math.round(num(v) * 1000) }) },
    OVERRIDES
  );
  padOverride("Thermal Relief Gap", "thermal_gap_override");

  // ------------------------------------------------------------------------------------------------------------- EDA_TEXT
  /** The text's or the field's value of a text row. */
  const look = (i: FpPropItem, c: FpPropCtx): TextLook => (i.text ? textLookOf(c.fp, i.id) : { bold: !!i.field?.bold, italic: !!i.field?.italic, valign: i.field?.valign ?? 0, width_nm: i.field?.w ?? null, knockout: !!i.field?.knockout, keep_upright: !!i.field?.upright });
  /** A text row's edit: the text with its looks, or the field with its layout. */
  const textEdit = (i: FpPropItem, c: FpPropCtx, textPatch: Partial<CmdText>, lookPatch: Partial<TextLook>, fieldPatch: Partial<ReturnType<typeof fieldLayoutOf>>): Cmd[] => {
    if (i.text) return textWith(i, c, textPatch, Object.keys(lookPatch).length > 0 ? lookPatch : undefined);
    if (i.field) {
      const next = { ...fieldLayoutOf(i.field), ...fieldPatch };
      return JSON.stringify(next) === JSON.stringify(fieldLayoutOf(i.field)) ? [] : [fieldCmd(c, i.field, fieldPatch)];
    }
    return [];
  };
  const angleOf = (i: FpPropItem) => (i.text ? i.text.angle / 1000 : (i.field?.angle ?? 0) / 1000);
  add({
    owner: "EDA_TEXT",
    name: "Orientation",
    kind: "double",
    display: "degree",
    get: angleOf,
    set: (i, v, c) => {
      const a = (((Math.round(num(v) * 1000) % 360_000) + 360_000) % 360_000);
      return textEdit(i, c, { angle: a }, {}, { angle: a });
    },
  });
  add(
    {
      owner: "EDA_TEXT",
      name: "Text",
      kind: "string",
      get: (i) => i.text?.content ?? i.field?.text ?? "",
      set: (i, v, c) => {
        const t = String(v);
        if (t === "") return [];
        if (i.text) return textWith(i, c, { content: t });
        if (i.field) return i.field.text === t ? [] : [fieldCmd(c, i.field, {}, t)];
        return [];
      },
    },
    TEXT
  );
  add({ owner: "EDA_TEXT", name: "Font", kind: "string", get: () => "KiCad Font", readOnlyReason: READ_ONLY.font }, TEXT);
  add({ owner: "EDA_TEXT", name: "Auto Thickness", kind: "bool", get: () => false, readOnlyReason: READ_ONLY.autoThickness }, TEXT);
  add(
    {
      owner: "EDA_TEXT",
      name: "Thickness",
      kind: "int",
      display: "size",
      get: (i) => i.text?.stroke_width ?? i.field?.thickness ?? 0,
      set: (i, v, c) => textEdit(i, c, { stroke_width: Math.round(num(v)) }, {}, { thickness: Math.round(num(v)) }),
    },
    TEXT
  );
  add({ owner: "EDA_TEXT", name: "Italic", kind: "bool", get: (i, c) => look(i, c).italic, set: (i, v, c) => textEdit(i, c, {}, { italic: Boolean(v) }, { italic: Boolean(v) }) }, TEXT);
  add({ owner: "EDA_TEXT", name: "Bold", kind: "bool", get: (i, c) => look(i, c).bold, set: (i, v, c) => textEdit(i, c, {}, { bold: Boolean(v) }, { bold: Boolean(v) }) }, TEXT);
  add({ owner: "EDA_TEXT", name: "Mirrored", kind: "bool", get: (i) => !!(i.text?.mirror ?? i.field?.mirror), set: (i, v, c) => textEdit(i, c, { mirror: Boolean(v) }, {}, { mirror: Boolean(v) }) }, TEXT);
  add({
    owner: "EDA_TEXT",
    name: "Visible",
    kind: "bool",
    get: (i) => (i.field ? i.field.visible : true),
    writeable: (i) => !!i.field,
    readOnlyReason: (i) => (i.field ? null : READ_ONLY.textVisible),
    set: (i, v, c) => (i.field ? textEdit(i, c, {}, {}, { visible: Boolean(v) }) : []),
  }, TEXT);
  add(
    {
      owner: "EDA_TEXT",
      name: "Width",
      kind: "int",
      display: "size",
      get: (i, c) => (i.text ? (textLookOf(c.fp, i.id).width_nm ?? i.text.size_nm) : (i.field?.w ?? 0)),
      set: (i, v, c) => {
        const w = Math.round(num(v));
        if (i.text) return textWith(i, c, {}, { width_nm: w === i.text.size_nm ? null : w });
        return textEdit(i, c, {}, {}, { size: [w, i.field!.h] });
      },
    },
    TEXT
  );
  add(
    {
      owner: "EDA_TEXT",
      name: "Height",
      kind: "int",
      display: "size",
      get: (i) => i.text?.size_nm ?? i.field?.h ?? 0,
      set: (i, v, c) => {
        const h = Math.round(num(v));
        if (i.text) {
          // the width the text kept stays its own (a text with none has the height for its width)
          const w = textLookOf(c.fp, i.id).width_nm ?? i.text.size_nm;
          return textWith(i, c, { size_nm: h }, { width_nm: w === h ? null : w });
        }
        return textEdit(i, c, {}, {}, { size: [i.field!.w, h] });
      },
    },
    TEXT
  );
  const H_JUSTIFY: Choice[] = [
    { label: "Left", value: -1 },
    { label: "Center", value: 0 },
    { label: "Right", value: 1 },
  ];
  const V_JUSTIFY: Choice[] = [
    { label: "Top", value: -1 },
    { label: "Center", value: 0 },
    { label: "Bottom", value: 1 },
  ];
  add(
    {
      owner: "EDA_TEXT",
      name: "Horizontal Justification",
      kind: "enum",
      choices: () => H_JUSTIFY,
      get: (i) => (i.text ? (i.text.justify === "left" ? -1 : i.text.justify === "right" ? 1 : 0) : (i.field?.halign ?? 0)),
      set: (i, v, c) => {
        const h = num(v) as -1 | 0 | 1;
        return textEdit(i, c, { justify: h < 0 ? "left" : h > 0 ? "right" : "center" }, {}, { halign: h });
      },
    },
    TEXT
  );
  add({ owner: "EDA_TEXT", name: "Vertical Justification", kind: "enum", choices: () => V_JUSTIFY, get: (i, c) => look(i, c).valign, set: (i, v, c) => textEdit(i, c, {}, { valign: num(v) as -1 | 0 | 1 }, { valign: num(v) as -1 | 0 | 1 }) }, TEXT);
  add({ owner: "EDA_TEXT", name: "Color", kind: "color", get: () => "", readOnlyReason: READ_ONLY.color }, TEXT);
  add({ owner: "EDA_TEXT", name: "Hyperlink", kind: "string", get: () => "" }, TEXT);

  // ------------------------------------------------------------------------------------------------------------- PCB_TEXT, PCB_FIELD
  pm.inheritsAfter("PCB_TEXT", "BOARD_ITEM");
  pm.inheritsAfter("PCB_TEXT", "EDA_TEXT");
  pm.mask("PCB_TEXT", "EDA_TEXT", "Color");
  add({ owner: "PCB_TEXT", name: "Knockout", kind: "bool", get: (i, c) => look(i, c).knockout, set: (i, v, c) => textEdit(i, c, {}, { knockout: Boolean(v) }, { knockout: Boolean(v) }) }, TEXT);
  add({ owner: "PCB_TEXT", name: "Keep Upright", kind: "bool", get: (i, c) => look(i, c).keep_upright, set: (i, v, c) => textEdit(i, c, {}, { keep_upright: Boolean(v) }, { keep_upright: Boolean(v) }) }, TEXT);
  pm.mask("PCB_TEXT", "EDA_TEXT", "Hyperlink");
  pm.inheritsAfter("PCB_FIELD", "BOARD_ITEM");
  pm.inheritsAfter("PCB_FIELD", "PCB_TEXT");
  pm.inheritsAfter("PCB_FIELD", "EDA_TEXT");
  add({ owner: "PCB_FIELD", name: "Name", kind: "string", hidden: true, hiddenFromLibraryEditors: true, get: (i) => i.field?.name ?? "" });
  pm.mask("PCB_FIELD", "EDA_TEXT", "Hyperlink");
  pm.mask("PCB_FIELD", "EDA_TEXT", "Color");

  // ------------------------------------------------------------------------------------------------------------- EDA_SHAPE, PCB_SHAPE
  const kindOf = (i: FpPropItem) => i.shape?.kind;
  const geometry = (name: "Start X" | "Start Y" | "End X" | "End Y" | "Center X" | "Center Y" | "Radius" | "Width" | "Height", get: (s: Shape) => number, available: (i: FpPropItem) => boolean) =>
    add(
      {
        owner: "EDA_SHAPE",
        name,
        kind: "int",
        display: name === "Radius" || name === "Width" || name === "Height" ? "size" : "coord",
        available,
        get: (i) => (i.shape ? get(cmdShapeToShape(i.shape)) : 0),
        set: (i, v, c) => {
          const next = shapeWith(cmdShapeToShape(i.shape!), name, Math.round(num(v)));
          return next ? [graphicCmd(c, toCmdShape(next, i.shape!))] : [];
        },
      },
      SHAPE
    );
  add({ owner: "EDA_SHAPE", name: "Shape", kind: "string", get: (i) => fpFriendlyName(i), readOnlyReason: READ_ONLY.shapeKind }, SHAPE);
  const startEnd = (i: FpPropItem) => kindOf(i) === "segment" || kindOf(i) === "rect" || kindOf(i) === "arc" || kindOf(i) === "bezier";
  const pt = (s: Shape, which: "start" | "end", axis: 0 | 1) => ("start" in s ? (which === "start" ? s.start : s.end)[axis] : 0);
  geometry("Start X", (s) => pt(s, "start", 0), startEnd);
  geometry("Start Y", (s) => pt(s, "start", 1), startEnd);
  geometry("Center X", (s) => (s.kind === "circle" ? s.center[0] : 0), (i) => kindOf(i) === "circle");
  geometry("Center Y", (s) => (s.kind === "circle" ? s.center[1] : 0), (i) => kindOf(i) === "circle");
  geometry("Radius", (s) => (s.kind === "circle" ? Math.round(Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1])) : 0), (i) => kindOf(i) === "circle");
  geometry("End X", (s) => pt(s, "end", 0), startEnd);
  geometry("End Y", (s) => pt(s, "end", 1), startEnd);
  geometry("Width", (s) => (s.kind === "rect" ? s.end[0] - s.start[0] : 0), (i) => kindOf(i) === "rect");
  geometry("Height", (s) => (s.kind === "rect" ? s.end[1] - s.start[1] : 0), (i) => kindOf(i) === "rect");
  add({ owner: "EDA_SHAPE", name: "Corner Radius", kind: "int", display: "size", get: () => 0, available: (i) => kindOf(i) === "rect", readOnlyReason: READ_ONLY.corner }, SHAPE);
  add(
    {
      owner: "EDA_SHAPE",
      name: "Line Width",
      kind: "int",
      display: "size",
      get: (i) => i.shape?.stroke_width ?? 0,
      set: (i, v, c) => (i.shape!.stroke_width === Math.round(num(v)) ? [] : [graphicCmd(c, { ...i.shape!, stroke_width: Math.round(num(v)) })]),
    },
    SHAPE
  );
  add(
    {
      owner: "EDA_SHAPE",
      name: "Line Style",
      kind: "enum",
      choices: () => LINE_STYLE_CHOICES.map((o) => ({ label: o.label, value: o.value })),
      get: (i, c) => {
        const s = shapeLookOf(c.fp, i.id).line_style;
        return s === "default" ? "solid" : s;
      },
      set: (i, v, c) => {
        const was = shapeLookOf(c.fp, i.id);
        return was.line_style === String(v) ? [] : [graphicCmd(c, i.shape!, { ...was, line_style: String(v) })];
      },
    },
    SHAPE
  );
  add({ owner: "EDA_SHAPE", name: "Line Color", kind: "color", get: () => "" }, SHAPE);
  add({ owner: "EDA_SHAPE", name: "Angle", kind: "double", display: "degree", get: () => 0, available: (i) => kindOf(i) === "arc", readOnlyReason: "An arc's ends are dragged on the canvas or set in its Properties." }, SHAPE);
  const closed = (i: FpPropItem) => kindOf(i) === "polygon" || kindOf(i) === "rect" || kindOf(i) === "circle";
  add(
    {
      owner: "EDA_SHAPE",
      name: "Fill",
      kind: "enum",
      available: closed,
      choices: () => FILL_CHOICES.map((o) => ({ label: o.label, value: o.value })),
      get: (i, c) => fillChoiceOf(!!i.shape?.filled, shapeLookOf(c.fp, i.id).hatch),
      set: (i, v, c) => {
        const { filled, hatch } = fillOfChoice(String(v) as (typeof FILL_CHOICES)[number]["value"]);
        const was = shapeLookOf(c.fp, i.id);
        if (i.shape!.filled === filled && was.hatch === hatch) return [];
        return [graphicCmd(c, { ...i.shape!, filled }, { ...was, hatch })];
      },
    },
    SHAPE
  );
  add({ owner: "EDA_SHAPE", name: "Fill Color", kind: "color", get: () => "", available: closed }, SHAPE);
  for (const n of ["Start Shape", "Start Length", "Start Width", "Start Stroke Width", "End Shape", "End Length", "End Width", "End Stroke Width"]) {
    add({ owner: "EDA_SHAPE", name: n, kind: "string", get: () => "", available: (i) => kindOf(i) === "segment" || kindOf(i) === "arc" || kindOf(i) === "bezier", readOnlyReason: READ_ONLY.endings }, SHAPE);
  }
  pm.inheritsAfter("PCB_SHAPE", "BOARD_CONNECTED_ITEM");
  pm.inheritsAfter("PCB_SHAPE", "EDA_SHAPE");
  pm.replaceProperty("BOARD_CONNECTED_ITEM", "Layer", {
    owner: "PCB_SHAPE",
    name: "Layer",
    kind: "enum",
    choices: () => layerChoices(),
    get: (i) => i.shape?.layer ?? "",
    set: (i, v, c) => (i.shape!.layer === String(v) ? [] : [graphicCmd(c, { ...i.shape!, layer: String(v) })]),
  });
  pm.mask("PCB_SHAPE", "EDA_SHAPE", "Line Color");
  pm.mask("PCB_SHAPE", "EDA_SHAPE", "Fill Color");
  ro("PCB_SHAPE", "Number Box", "Pad Primitives", READ_ONLY.primitive, () => "", { kind: "bool", get: () => false, available: () => false });
  ro("PCB_SHAPE", "Thermal Spoke Template", "Pad Primitives", READ_ONLY.primitive, () => "", { kind: "bool", get: () => false, available: () => false });
  ro("PCB_SHAPE", "Soldermask", "Technical Layers", READ_ONLY.soldermask, () => "", { kind: "bool", get: () => false, available: (i) => !!i.shape?.layer.endsWith(".Cu") });
  ro("PCB_SHAPE", "Soldermask Margin Override", "Technical Layers", READ_ONLY.soldermask, () => "", { available: (i) => !!i.shape?.layer.endsWith(".Cu") });

  pm.rebuild();
  return pm;
}

/** The registry (built once). */
export const FP_PROPERTIES = build();

/** The grid of the selection `ids` (the footprint when it is empty), as the library editor's panel shows it. */
export function fpGrid(ctx: FpPropCtx, ids: Iterable<string>): GridModel {
  return buildGrid(FP_PROPERTIES, fpPropItemsOf(ctx.fp, ids), ctx, fpFriendlyName, { libraryEditor: true });
}

/** The commands an edit of the row `name` makes (`PROPERTIES_PANEL::valueChanged`), one batch the caller sends. */
export function fpEdit(ctx: FpPropCtx, ids: Iterable<string>, name: string, value: PropValue): EditPlan<Cmd> {
  return planEdit(FP_PROPERTIES, fpPropItemsOf(ctx.fp, ids), name, value, ctx);
}
