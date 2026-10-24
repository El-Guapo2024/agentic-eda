// The board's expression language: what `A.Width > 0.25mm`, `A.Type == 'Via' && A.NetName == 'GND'` and `A.hasNetclass('HV')` mean on the studio's items. This is
// `pcbnew/pcbexpr_evaluator.cpp` and `pcbexpr_functions.cpp` (8303b2ad) over `libeval.ts`: the property references (`PCBEXPR_VAR_REF`, its net, net class, type and
// layer special cases), the values that compare as KiCad's do (a layer matches the layers a pattern names, a net class matches any class of a composite one), the units,
// and the functions that need no geometry. Find by Properties' Query tab (`findByProperties.ts`) compiles with it; the properties are the Properties panel's own, read
// from `PCB_PROPERTIES` by the class of the item as `m_matchingTypes` does.
//
//   - A length is in nanometres, as KiCad's internal units are, so `0.25mm` is 250000, as the design holds it; an area in nm^2.
//   - A property an item of the class has but cannot show (the panel's `available` says no) reads as undefined, which compares false whatever it is compared with;
//     KiCad's getter would answer with a number that means nothing.
//   - `true` and `false` are the numbers 1 and 0 (Find by Properties writes `A.Locked == false` for a flag; KiCad's compiler has no such names and calls them unrecognized items).
//   - `A.Net` is the net's name here (it is the net code in KiCad, so `A.Net == 'GND'` -- what Create from Selection Tab writes for a Net row -- matches nothing there).
//
// Not ported: `intersectsCourtyard`, `intersectsArea`, `enclosedByArea` and their old names (they need the shapes of the items; they are known to the compiler and say
// false), `fromTo` (the from-to cache), the sheet functions (a footprint here has no sheet path) and component classes (none exist in the IR yet).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardState, Cmd, NetClass } from "../api/types";
import { effectiveClassName } from "./boardSetupRules";
import { enabledOptionalLayers, layerDisplayName, standardLayerName } from "./boardLayers";
import { compile, Context, doubleValueFromString, Value, wildCompare, type CompileOutcome, type FuncCall, type Ucode, type UcodeHost, type UnitResolver, type Units, type VarRef, type VarType } from "./libeval";
import { PCB_PROPERTIES, pcbContext, pcbItemsOf, typeName, type PcbCtx, type PcbItem } from "./pcbProperties";
import type { PropertyDef } from "./propertyManager";

// ------------------------------------------------------------------------------------------------------------------------------------------ units

/** `PCBEXPR_UNIT_RESOLVER`: `mil`, `mm`, `in`, `deg`, `fs` and `ps`; lengths become nanometres and times attoseconds (`pcbIUScale`). */
export const PCB_UNITS: UnitResolver = {
  names: ["mil", "mm", "in", "deg", "fs", "ps"],
  types: ["mil", "mm", "in", "deg", "fs", "ps"] as readonly Units[],
  message: "must be mm, in, mil, deg, fs, or ps",
  convert(text: string, unitId: number): number {
    const v = doubleValueFromString(text);
    // (rounded to a whole internal unit so that `==` works on `0.07mm`: 0.07 * 1000000 is 70000.00000000001 in floating point)
    switch (unitId) {
      case 0:
        return Math.round(v * 25_400);
      case 1:
        return Math.round(v * 1_000_000);
      case 2:
        return Math.round(v * 25_400_000);
      case 3:
        return v;
      case 4:
        return Math.round(v * 1_000);
      case 5:
        return Math.round(v * 1_000_000);
      default:
        return v;
    }
  },
};

/**
 * `EDA_UNIT_UTILS::UI::DoubleValueFromString( pcbIUScale, text, value )`: a string that is a number with a unit ("0.2mm", "5 mil", "90ra") is that many internal units;
 * a bare number or any other text is not one (null). This is what makes a text property such as a pad's size compare as a number.
 */
export function numberWithUnit(text: string): number | null {
  const t = text.trim();
  const m = /^([0-9.,+-]+)(.*)$/.exec(t);
  if (!m) return null;
  const num = Number.parseFloat((m[1] as string).replace(/,/g, "."));
  const v = Number.isFinite(num) ? num : 0;
  const unit = (m[2] as string).trim().toLowerCase();
  switch (unit) {
    case "um":
    case "µm":
    case "μm":
      return v * 1_000;
    case "mm":
      return v * 1_000_000;
    case "cm":
      return v * 10_000_000;
    case "mil":
    case "mils":
    case "thou":
      return v * 25_400;
    case "in":
    case '"':
      return v * 25_400_000;
    case "oz":
      return v * 1.37 * 25_400;
    case "ra":
      return v * (180 / Math.PI);
    case "fs":
      return v * 1_000;
    case "ps":
      return v * 1_000_000;
    default:
      return null;
  }
}

// ------------------------------------------------------------------------------------------------------------------------------------ environment

/** What the evaluator needs of the board besides the items: the Properties panel's context, the net classes and the layer names the language knows. */
export interface PcbExprEnv {
  board: BoardState;
  ctx: PcbCtx;
  netClasses: readonly NetClass[];
  /** `ENUM_MAP<PCB_LAYER_ID>`: for each layer its canonical name and the name the board gives it, each an entry. */
  layerEntries: ReadonlyArray<{ text: string; layer: string }>;
  /** `BOARD::m_LayerExpressionCache`: the layers a pattern names. */
  layerCache: Map<string, ReadonlySet<string>>;
  items: Map<string, PcbItem>;
}

export function pcbExprEnv(board: BoardState, units: "mm" | "mil" | "in" = "mm"): PcbExprEnv {
  const ctx = pcbContext(board, units);
  const setup = board.board_rules?.layer_setup;
  const layers = new Set<string>([...ctx.allLayers, ...enabledOptionalLayers(setup)]);
  const entries: Array<{ text: string; layer: string }> = [];
  for (const layer of layers) {
    entries.push({ text: layer, layer });
    const shown = layerDisplayName(setup, layer);
    if (shown !== layer) entries.push({ text: shown, layer });
    const standard = standardLayerName(layer);
    if (standard !== layer && standard !== shown) entries.push({ text: standard, layer });
  }
  return { board, ctx, netClasses: board.board_rules?.net_classes ?? [], layerEntries: entries, layerCache: new Map(), items: new Map() };
}

/** `wxString::Matches( mask )`: `*` and `?`, case sensitive. */
const wxMatches = (text: string, mask: string): boolean => wildCompare(mask, text, true);

// ------------------------------------------------------------------------------------------------------------------------------------- layers

/** `BOARD_ITEM::GetLayerSet()` of an item: where it physically is. */
export function itemLayers(item: PcbItem, env: PcbExprEnv, seen: Set<string> = new Set()): string[] {
  const copper = env.ctx.copperLayers;
  switch (item.type) {
    case "FOOTPRINT":
      return [item.part!.side === "bottom" ? "B.Cu" : "F.Cu"];
    case "PAD": {
      const pad = item.pad!;
      const kind = pad.kind ?? (pad.th ? "through_hole" : "smd");
      if (kind === "smd") {
        const bottom = item.part!.side === "bottom";
        return bottom ? ["B.Cu", "B.Paste", "B.Mask"] : ["F.Cu", "F.Paste", "F.Mask"];
      }
      return [...copper, "F.Mask", "B.Mask"];
    }
    case "PCB_TRACK":
    case "PCB_ARC":
      return [item.track!.layer];
    case "PCB_VIA": {
      const v = item.via!;
      const a = copper.indexOf(v.from);
      const b = copper.indexOf(v.to);
      if (a < 0 || b < 0) return [...copper];
      return copper.slice(Math.min(a, b), Math.max(a, b) + 1);
    }
    case "ZONE":
      return [item.zone!.layer];
    case "PCB_TEXT":
      return [item.text!.layer];
    case "PCB_FIELD":
      return [item.field!.layer];
    case "PCB_SHAPE":
      return [item.shape!.layer];
    case "PCB_GROUP": {
      if (seen.has(item.id)) return [];
      seen.add(item.id);
      const out = new Set<string>();
      for (const m of pcbItemsOf(env.board, item.group!.member_ids)) for (const l of itemLayers(m, env, seen)) out.add(l);
      return [...out];
    }
    default:
      return item.dim ? [item.dim.layer] : [];
  }
}

// ------------------------------------------------------------------------------------------------------------------------------------ the context

/** `PCBEXPR_CONTEXT`: the constraint being evaluated, the layer, and the items `A` and `B`. */
export class PcbExprContext extends Context {
  readonly items: [PcbItem | null, PcbItem | null] = [null, null];
  readonly typeOverrides = new Map<PcbItem, string>();

  constructor(
    readonly env: PcbExprEnv | null,
    readonly constraint: number = 0,
    public layer: string = "F.Cu"
  ) {
    super();
  }

  setItems(a: PcbItem | null, b: PcbItem | null = null): void {
    this.items[0] = a;
    this.items[1] = b;
  }
}

const envOf = (ctx: Context): PcbExprEnv | null => (ctx instanceof PcbExprContext ? ctx.env : null);

// ------------------------------------------------------------------------------------------------------------------------------------ item facts

/** A `BOARD_CONNECTED_ITEM`: it has a net (and a net class). */
export const isConnected = (item: PcbItem): boolean => item.type === "PAD" || item.type === "PCB_TRACK" || item.type === "PCB_ARC" || item.type === "PCB_VIA" || item.type === "ZONE" || item.type === "PCB_SHAPE";

/** `BOARD_CONNECTED_ITEM::GetNetname()`. */
export function netNameOf(item: PcbItem): string {
  return item.track?.net ?? item.via?.net ?? item.zone?.net ?? item.pad?.net ?? "";
}

/** The footprint an item belongs to (`GetParentFootprint`): a pad's or a field's; null for the rest and for a footprint. */
function parentFootprint(item: PcbItem): PcbItem | null {
  return (item.type === "PAD" || item.type === "PCB_FIELD") && item.part ? { type: "FOOTPRINT", id: item.part.ref, part: item.part } : null;
}

/** The names of the classes a net is in (`NETCLASS::GetConstituentNetclasses` of its effective class): the classes it matches, then Default when some setting comes from it. */
export function constituentClasses(env: PcbExprEnv, net: string): string[] {
  return classNameOf(env, net).split(",");
}

/** `NET_SETTINGS::GetEffectiveNetClass`' name: an item with no net is in the Default class alone. */
export function classNameOf(env: PcbExprEnv, net: string): string {
  return net === "" ? "Default" : effectiveClassName(env.netClasses, net);
}

// ------------------------------------------------------------------------------------------------------------------------------------- values

/** `PCBEXPR_LAYER_VALUE`: a layer, which equals the layers a pattern matches the names of. */
class LayerValue extends Value {
  constructor(
    private readonly layer: string,
    private readonly env: PcbExprEnv | null
  ) {
    super();
    this.kind = "string";
    this.str = standardLayerName(layer);
  }

  override equalTo(_ctx: Context, b: Value): boolean {
    const name = b.asString();
    const env = this.env;
    if (!env) return name === this.layer;
    let mask = env.layerCache.get(name);
    if (!mask) {
      const set = new Set<string>();
      for (const e of env.layerEntries) if (wxMatches(e.text, name)) set.add(e.layer);
      mask = set;
      env.layerCache.set(name, mask);
    }
    return mask.has(this.layer);
  }
}

/** `PCBEXPR_NETCLASS_VALUE`: a net class, which equals any class a composite one is made of. */
class NetclassValue extends Value {
  constructor(
    private readonly env: PcbExprEnv,
    private readonly net: string
  ) {
    super();
    this.kind = "string";
  }

  override asString(): string {
    return classNameOf(this.env, this.net);
  }

  override equalTo(ctx: Context, b: Value): boolean {
    if (b instanceof NetclassValue) return this.asString() === b.asString();
    if (b.type === "string") {
      for (const name of constituentClasses(this.env, this.net)) {
        if (b.wildcard ? wildCompare(b.asString(), name, false) : name.toUpperCase() === b.asString().toUpperCase()) return true;
      }
    }
    return Value.string(this.asString()).equalTo(ctx, b);
  }

  override notEqualTo(ctx: Context, b: Value): boolean {
    if (b instanceof NetclassValue) return this.asString() !== b.asString();
    if (b.type === "string") {
      let inConstituents = false;
      for (const name of constituentClasses(this.env, this.net)) {
        if (b.wildcard ? wildCompare(b.asString(), name, false) : name.toUpperCase() === b.asString().toUpperCase()) {
          inConstituents = true;
          break;
        }
      }
      const isFullName = Value.string(this.asString()).equalTo(ctx, b);
      return !inConstituents && !isFullName;
    }
    return Value.string(this.asString()).notEqualTo(ctx, b);
  }
}

/** `PCBEXPR_NET_VALUE`: a net, which equals another net of the same name. */
class NetValue extends Value {
  constructor(readonly net: string) {
    super();
    this.kind = "string";
    this.str = net;
  }

  override equalTo(ctx: Context, b: Value): boolean {
    return b instanceof NetValue ? this.net === b.net : super.equalTo(ctx, b);
  }

  override notEqualTo(ctx: Context, b: Value): boolean {
    return b instanceof NetValue ? this.net !== b.net : super.notEqualTo(ctx, b);
  }
}

// ----------------------------------------------------------------------------------------------------------------------------------- references

/** `PCBEXPR_VAR_REF`: `A.Width`, `B.Layer`, or just `A`. `index` is 0 for A (and AB), 1 for B and 2 for L, the layer. */
class PcbVarRef implements VarRef {
  type: VarType = "undefined";
  /** The name of the property, as the registry has it (the `_`s of the text are blanks there). */
  field = "";

  constructor(readonly index: number) {}

  getType(): VarType {
    return this.type;
  }

  getObject(ctx: Context): PcbItem | null {
    return ctx instanceof PcbExprContext ? (ctx.items[this.index] ?? null) : null;
  }

  getValue(ctx: Context): Value {
    if (this.type === "null") return Value.nullValue();
    const pctx = ctx as PcbExprContext;
    if (this.index === 2) return new LayerValue(pctx.layer, pctx.env);
    let item = this.getObject(ctx);
    if (!item || !pctx.env) return Value.undefinedValue();
    const env = pctx.env;
    let prop = PCB_PROPERTIES.getProperty(item.type, this.field, item, env.ctx);
    if (!prop || !PCB_PROPERTIES.isAvailableFor(item.type, prop, item, env.ctx)) {
      // "If the property isn't defined on the item itself but is defined on its parent footprint (e.g. Reference, Value), resolve against the parent."
      const parent = parentFootprint(item);
      const inherited = parent ? PCB_PROPERTIES.getProperty(parent.type, this.field, parent, env.ctx) : undefined;
      if (!parent || !inherited || !PCB_PROPERTIES.isAvailableFor(parent.type, inherited, parent, env.ctx)) return Value.undefinedValue();
      item = parent;
      prop = inherited;
    }
    try {
      return propertyValue(prop, item, env);
    } catch {
      return Value.undefinedValue();
    }
  }
}

/** The value of a property of an item as the language sees it (`PCBEXPR_VAR_REF::GetValue` by the type of the property). */
export function propertyValue(prop: PropertyDef<PcbItem, PcbCtx, Cmd>, item: PcbItem, env: PcbExprEnv): Value {
  const raw = prop.get(item, env.ctx);
  switch (prop.kind) {
    case "int": {
      const n = typeof raw === "number" ? raw : Number(raw);
      const scale = 1;
      return Value.number(n * scale);
    }
    case "double":
      return Value.number(typeof raw === "number" ? raw : Number(raw));
    case "bool":
      return Value.number(raw ? 1 : 0);
    case "enum":
    case "net": {
      if (prop.name === "Layer" || prop.name === "Layer Top" || prop.name === "Layer Bottom") return new LayerValue(String(raw), env);
      if (prop.kind === "net") return new NetValue(String(raw));
      const choices = prop.choices ? prop.choices(item, env.ctx) : [];
      const hit = choices.find((c) => c.value === raw);
      return Value.string(hit ? hit.label : String(raw));
    }
    default: {
      // "If it quacks like a duck, it is a duck": a text such as "0.2mm" is a length.
      const s = String(raw);
      const n = numberWithUnit(s);
      return n !== null ? Value.number(n) : Value.string(s);
    }
  }
}

/** `PCBEXPR_NETCLASS_REF`. */
class NetclassRef extends PcbVarRef {
  constructor(index: number) {
    super(index);
    this.type = "string";
  }

  override getValue(ctx: Context): Value {
    const item = this.getObject(ctx);
    const env = envOf(ctx);
    if (!item || !env || !isConnected(item)) return Value.undefinedValue();
    return new NetclassValue(env, netNameOf(item));
  }
}

/** `PCBEXPR_COMPONENT_CLASS_REF`: no component classes exist in the IR, so the class of a footprint is the empty one. */
class ComponentClassRef extends PcbVarRef {
  constructor(index: number) {
    super(index);
    this.type = "string";
  }

  override getValue(ctx: Context): Value {
    const item = this.getObject(ctx);
    if (!item) return Value.undefinedValue();
    return item.type === "FOOTPRINT" || parentFootprint(item) ? Value.string("") : Value.undefinedValue();
  }
}

/** `PCBEXPR_NETNAME_REF`. */
class NetnameRef extends PcbVarRef {
  constructor(index: number) {
    super(index);
    this.type = "string";
  }

  override getValue(ctx: Context): Value {
    const item = this.getObject(ctx);
    if (!item || !isConnected(item)) return Value.undefinedValue();
    return new NetValue(netNameOf(item));
  }
}

/** `PCBEXPR_TYPE_REF`: `A.Type`, the class of the item (`SetTypeOverride` lets a teardrop count as a track). */
class TypeRef extends PcbVarRef {
  constructor(index: number) {
    super(index);
    this.type = "string";
  }

  override getValue(ctx: Context): Value {
    const item = this.getObject(ctx);
    if (!item) return Value.undefinedValue();
    const override = ctx instanceof PcbExprContext ? ctx.typeOverrides.get(item) : undefined;
    return Value.string(override ?? typeName(item));
  }
}

// ----------------------------------------------------------------------------------------------------------------------------------- functions

type ItemFn = (ctx: PcbExprContext, item: PcbItem, arg: Value) => number;

/** The shape every one-argument function of `pcbexpr_functions.cpp` has: take the argument, leave a result, work it out when it is asked for. */
function oneArg(missing: string, fn: ItemFn, needsArg = true): FuncCall {
  return (ctx, self) => {
    const arg = needsArg ? ctx.pop() : null;
    const result = ctx.allocValue();
    result.set(0);
    ctx.push(result);
    if (needsArg && (!arg || arg.asString() === "")) {
      if (ctx.hasErrorCallback()) ctx.reportError(missing);
      return;
    }
    const item = self instanceof PcbVarRef ? self.getObject(ctx) : null;
    if (!item || !(ctx instanceof PcbExprContext) || !ctx.env) return;
    result.setDeferredNumber(() => fn(ctx, item, arg ?? Value.undefinedValue()));
  };
}

/** `memberOfGroup`: a group the item is in, or one of its footprint's, or any group above those, whose name matches. */
function inGroup(env: PcbExprEnv, item: PcbItem, pattern: string): boolean {
  const groups = env.board.drawings?.groups ?? [];
  const parentOf = (id: string) => groups.find((g) => g.member_ids.includes(id));
  let group = parentOf(item.id);
  if (!group) {
    const fp = parentFootprint(item);
    if (fp) group = parentOf(fp.id);
  }
  const seen = new Set<string>();
  while (group && !seen.has(group.id)) {
    if (wxMatches(group.name, pattern)) return true;
    seen.add(group.id);
    group = parentOf(group.id);
  }
  return false;
}

/** The lowest (A or B) item-less functions: one that says false, for what this build cannot answer. */
const notAnswered = (): FuncCall => (ctx) => {
  // Take the arguments the call pushed (it pushes none we can count: leave the stack as a call to a function of one argument would).
  ctx.pop();
  const result = ctx.allocValue();
  result.set(0);
  ctx.push(result);
};

/** `MatchDpSuffix`: 1 for a positive net (`..P`, `..+`), -1 for a negative one, 0 for neither; with the name the pair has without the suffix. */
export function matchDpSuffix(netName: string): { polarity: number; complement: string; base: string } {
  let polarity = 0;
  let count = 0;
  let complementSuffix = "";
  for (let i = netName.length - 1; i >= 0 && polarity === 0; i--, count++) {
    const ch = netName[i] as string;
    if ((ch >= "0" && ch <= "9") || ch === "_") continue;
    if (ch === "+") {
      complementSuffix = "-";
      polarity = 1;
    } else if (ch === "-") {
      complementSuffix = "+";
      polarity = -1;
    } else if (ch === "N") {
      complementSuffix = "P";
      polarity = -1;
    } else if (ch === "P") {
      complementSuffix = "N";
      polarity = 1;
    } else {
      break;
    }
  }
  if (polarity !== 0 && count >= 1) {
    const base = netName.slice(0, netName.length - count);
    return { polarity, base, complement: base + complementSuffix + netName.slice(netName.length - (count - 1)) };
  }
  return { polarity, complement: "", base: "" };
}

const hasNet = (env: PcbExprEnv, name: string): boolean => env.ctx.nets.includes(name);

const BUILTINS: Record<string, FuncCall> = {
  existsonlayer: oneArg("Missing layer name argument to existsOnLayer().", (ctx, item, arg) => {
    const env = ctx.env!;
    const pattern = arg.asString();
    const layers = new Set(itemLayers(item, env));
    for (const e of env.layerEntries) if (wxMatches(e.text, pattern) && layers.has(e.layer)) return 1;
    return 0;
  }),
  isplated: oneArg(
    "",
    (_ctx, item) => {
      if (item.type === "PAD") return (item.pad!.kind ?? (item.pad!.th ? "through_hole" : "smd")) === "through_hole" ? 1 : 0;
      return item.type === "PCB_VIA" ? 1 : 0;
    },
    false
  ),
  ismicrovia: oneArg("", () => 0, false),
  isblindvia: oneArg(
    "",
    (ctx, item) => {
      if (item.type !== "PCB_VIA") return 0;
      const c = ctx.env!.ctx.copperLayers;
      const v = item.via!;
      const outer = (l: string) => l === c[0] || l === c[c.length - 1];
      return !((v.from === c[0] && v.to === c[c.length - 1]) || (v.from === c[c.length - 1] && v.to === c[0])) && (outer(v.from) || outer(v.to)) ? 1 : 0;
    },
    false
  ),
  isburiedvia: oneArg(
    "",
    (ctx, item) => {
      if (item.type !== "PCB_VIA") return 0;
      const c = ctx.env!.ctx.copperLayers;
      const v = item.via!;
      const outer = (l: string) => l === c[0] || l === c[c.length - 1];
      return !outer(v.from) && !outer(v.to) ? 1 : 0;
    },
    false
  ),
  isblindburiedvia: oneArg(
    "",
    (ctx, item) => {
      if (item.type !== "PCB_VIA") return 0;
      const c = ctx.env!.ctx.copperLayers;
      const v = item.via!;
      return (v.from === c[0] && v.to === c[c.length - 1]) || (v.from === c[c.length - 1] && v.to === c[0]) ? 0 : 1;
    },
    false
  ),
  memberof: oneArg("Missing group name argument to memberOfGroup().", (ctx, item, arg) => (inGroup(ctx.env!, item, arg.asString()) ? 1 : 0)),
  memberofgroup: oneArg("Missing group name argument to memberOfGroup().", (ctx, item, arg) => (inGroup(ctx.env!, item, arg.asString()) ? 1 : 0)),
  memberoffootprint: oneArg("Missing footprint argument (reference designator) to memberOfFootprint().", (_ctx, item, arg) => {
    const fp = parentFootprint(item);
    return fp && footprintSelected(fp, arg.asString()) ? 1 : 0;
  }),
  memberofsheet: oneArg("Missing sheet name argument to memberOfSheet().", () => 0),
  memberofsheetorchildren: oneArg("Missing sheet name argument to memberOfSheetOrChildren().", () => 0),
  getfield: (ctx, self) => {
    const arg = ctx.pop();
    const result = ctx.allocValue();
    result.setText("");
    ctx.push(result);
    if (!arg) {
      if (ctx.hasErrorCallback()) ctx.reportError("Missing field name argument to getField().");
      return;
    }
    const item = self instanceof PcbVarRef ? self.getObject(ctx) : null;
    if (!item) return;
    result.setDeferredString(() => {
      if (item.type !== "FOOTPRINT") return "";
      const name = arg.asString();
      const field = item.part!.fields?.find((f) => f.name === name);
      return field ? field.text : "";
    });
  },
  hasnetclass: oneArg("Missing netclass name argument to hasNetclass()", (ctx, item, arg) => {
    if (!isConnected(item)) return 0;
    return constituentClasses(ctx.env!, netNameOf(item)).includes(arg.asString()) ? 1 : 0;
  }),
  hasexactnetclass: oneArg("Missing netclass name argument to hasExactNetclass()", (ctx, item, arg) => {
    if (!isConnected(item)) return 0;
    return classNameOf(ctx.env!, netNameOf(item)) === arg.asString() ? 1 : 0;
  }),
  hascomponentclass: oneArg("Missing component class name argument to hasComponentClass()", () => 0),
  indiffpair: oneArg("Missing diff-pair name argument to inDiffPair().", (ctx, item, arg) => {
    if (!isConnected(item)) return 0;
    const dp = matchDpSuffix(netNameOf(item));
    if (dp.polarity === 0 || !hasNet(ctx.env!, dp.complement)) return 0;
    const pattern = arg.asString();
    if (wxMatches(dp.base, pattern)) return 1;
    return dp.base.endsWith("_") && wxMatches(dp.base.slice(0, dp.base.lastIndexOf("_")), pattern) ? 1 : 0;
  }),
  iscoupleddiffpair: (ctx) => {
    const result = ctx.allocValue();
    result.set(0);
    ctx.push(result);
    if (!(ctx instanceof PcbExprContext) || !ctx.env) return;
    const env = ctx.env;
    const a = ctx.items[0];
    const b = ctx.items[1];
    result.setDeferredNumber(() => {
      if (!a || !isConnected(a)) return 0;
      const dp = matchDpSuffix(netNameOf(a));
      if (dp.polarity === 0) return 0;
      // (`DIFF_PAIR_GAP`, `LENGTH` and `SKEW` constraints are asked with no B: a pair is there when the complement net exists. The Query tab asks none of them.)
      if ((ctx.constraint === DIFF_PAIR_GAP || ctx.constraint === LENGTH || ctx.constraint === SKEW) && hasNet(env, dp.complement)) return 1;
      return b && isConnected(b) && netNameOf(b) === dp.complement ? 1 : 0;
    });
  },
  fromto: (ctx) => {
    ctx.pop();
    ctx.pop();
    const result = ctx.allocValue();
    result.set(0);
    ctx.push(result);
  },
  insidecourtyard: notAnswered(),
  insidefrontcourtyard: notAnswered(),
  insidebackcourtyard: notAnswered(),
  intersectscourtyard: notAnswered(),
  intersectsfrontcourtyard: notAnswered(),
  intersectsbackcourtyard: notAnswered(),
  insidearea: notAnswered(),
  intersectsarea: notAnswered(),
  enclosedbyarea: notAnswered(),
};

/** `DRC_CONSTRAINT_T` values `isCoupledDiffPair` looks at; the Query tab evaluates with constraint 0. */
const DIFF_PAIR_GAP = 7;
const LENGTH = 8;
const SKEW = 9;

/** `testFootprintSelector`: the reference, or the library id when the selector has a colon, matches (a wildcard pattern). */
export function footprintSelected(fp: PcbItem, selector: string): boolean {
  if (selector === "") return false;
  if (wxMatches(fp.part!.ref, selector)) return true;
  return selector.includes(":") && wxMatches(fp.part!.footprint ?? "", selector);
}

// ---------------------------------------------------------------------------------------------------------------------------------- compiling

/** `PCBEXPR_UCODE`: what a name in an expression refers to. */
class PcbHost implements UcodeHost {
  constructor(private readonly env: PcbExprEnv) {}

  /** `PCBEXPR_UCODE::CreateVarRef`. */
  createVarRef(variable: string, field: string): VarRef | null {
    if (variable.toLowerCase() === "null") {
      const ref = new PcbVarRef(0);
      ref.type = "null";
      return ref;
    }
    // `true` and `false`: KiCad's language has neither (Create from Selection Tab writes them for a flag and the compiler then calls them unrecognized items).
    if (field === "" && (variable === "true" || variable === "false")) {
      const n = variable === "true" ? 1 : 0;
      return { getType: () => "numeric", getValue: () => Value.number(n) };
    }
    const f = field.toLowerCase();
    const index = variable === "A" || variable === "AB" ? 0 : variable === "B" ? 1 : variable === "L" ? 2 : -1;
    if (f === "netclass" || f === "componentclass" || f === "netname" || f === "type") {
      if (variable !== "A" && variable !== "B") return null;
      const i = variable === "A" ? 0 : 1;
      return f === "netclass" ? new NetclassRef(i) : f === "componentclass" ? new ComponentClassRef(i) : f === "netname" ? new NetnameRef(i) : new TypeRef(i);
    }
    if (index < 0) return null;
    const ref = new PcbVarRef(index);
    if (field.length === 0) return ref;
    const name = field.replace(/_/g, " ");
    ref.field = name;
    for (const cls of PCB_PROPERTIES.allClasses()) {
      if (!PCB_PROPERTIES.isOfType(cls, "BOARD_ITEM")) continue;
      const prop = PCB_PROPERTIES.getProperty(cls, name);
      if (!prop) continue;
      ref.type = prop.kind === "int" || prop.kind === "bool" ? "numeric" : prop.kind === "double" ? "numeric_double" : "string";
    }
    if (ref.type === "undefined") ref.type = "parse_error";
    return ref;
  }

  createFuncCall(name: string): FuncCall | null {
    void this.env;
    return BUILTINS[name.toLowerCase()] ?? null;
  }
}

export interface QueryCompilation extends CompileOutcome {
  ucode: Ucode;
}

/** `PCBEXPR_COMPILER( new PCBEXPR_UNIT_RESOLVER() ).Compile( text, &ucode, &preflightContext )`. */
export function compilePcbExpr(text: string, env: PcbExprEnv, onError?: (message: string, pos: number) => void): QueryCompilation {
  const preflight = new PcbExprContext(env, 0, "F.Cu");
  return compile(text, new PcbHost(env), PCB_UNITS, preflight, onError);
}

/**
 * `DIALOG_FIND_BY_PROPERTIES::selectMatchingFromQuery`'s loop: an item matches when the expression is not zero on any layer it is on (`A` is the item, there is no `B`).
 * `ids` are the matches, in the order of `items`.
 */
export function itemsMatching(compiled: QueryCompilation, items: readonly PcbItem[], env: PcbExprEnv): string[] {
  const out: string[] = [];
  for (const item of items) {
    for (const layer of itemLayers(item, env)) {
      const ctx = new PcbExprContext(env, 0, layer);
      ctx.setItems(item, null);
      const result = compiled.ucode.run(ctx);
      if (result.asDouble() !== 0) {
        out.push(item.id);
        break;
      }
    }
  }
  return out;
}

/** `PCBEXPR_EVALUATOR`-style: the value of an expression on `a` and `b` on `layer` (tests, and the rule conditions of the Rust side are held to the same cases). */
export function evaluateFor(text: string, env: PcbExprEnv, a: PcbItem | null, b: PcbItem | null, layer: string): { compiled: QueryCompilation; value: Value } {
  const compiled = compilePcbExpr(text, env);
  const ctx = new PcbExprContext(env, 0, layer);
  ctx.setItems(a, b);
  return { compiled, value: compiled.ucode.run(ctx) };
}
