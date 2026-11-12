// A SPICE library's models as KiCad reads them for the Simulation Model dialog, with their parameters (`SIM_LIBRARY_SPICE`'s models, KiCad
// 8303b2ad):
// - `baseModelOf` is `SIM_MODEL_SPICE::Create( aLibrary, aSpiceCode, .. )`: the model of the type the server read off the `.model` line
//   (`SPICE_MODEL_PARSER::ReadType` / `ReadTypeFromSpiceStrings`, eda_sim::library), given its parameters (`SPICE_MODEL_PARSER::ReadModel`:
//   each `name=value` through `SetParamFromSpiceCode`, an "ako:" model first taking its parent's values), or a subcircuit with its ports and
//   `params:` (`SPICE_MODEL_PARSER_SUBCKT::ReadModel`); a model whose class is not a SPICE one, or whose card does not read, is kept as its raw
//   code (`SIM_MODEL_SPICE_FALLBACK`).
// - `libraryModel` is `SIM_MODEL::Create( aBaseModel, aPins[, aFields], .. )` as the dialog's `loadLibrary` makes each model: one of the
//   base model's type with the base model behind it, so a parameter left empty shows the library's value and `Sim.Params` holds only what the
//   instance changes (`GenerateParams` leaves out a value equal to the base model's).
//
// The card is tokenized as `SPICE_GRAMMAR` does: `sep` is a run of ' ', '\t', '=', '(', ')' and ',' (and continuations: a `+` line, a `\\`
// line end), `*` lines are comments, ';', '$' and "//" start an end-of-line comment; a value is a `{..}` or `[..]` expression or a token.
//
// Pure: no React, no DOM.
import { SimModelError, SimModelNgspice, SimModelRawSpice, SimModelSpiceFallback, SimModelSubckt, createModel, type Fields, type ParamInfo, type SimModel, type SimModelTables } from "./simModelBuiltin";

/** A library model as `POST /api/sim/model/library` sends it (the part this file reads). */
export interface LibraryModelSource {
  name: string;
  kind: "subckt" | "model" | "ibis";
  /** `SIM_MODEL::TYPE`'s name the `.model` line reads as (absent: `TYPE::RAWSPICE`). */
  modelType?: string;
  /** A subcircuit's ports. */
  pins: string[];
  code: string;
}

/** The card as one line: comment lines dropped, end-of-line comments cut, `+` and `\\` continuations joined (`newline`, `continuation`). */
export function joinCard(code: string): string {
  const out: string[] = [];
  for (const raw of code.split(/\r?\n/)) {
    if (/^[ \t=(),]*\*/.test(raw)) continue; // `commentLine`
    let line = raw;
    const c = line.search(/;|\$|\/\//);
    if (c >= 0) line = line.slice(0, c);
    const backslash = /\\\\[ \t\v\f]*$/.exec(line);
    if (backslash) line = line.slice(0, backslash.index);
    const plus = /^[ \t]*\+/.exec(line);
    if (plus && out.length > 0) out[out.length - 1] += " " + line.slice(plus[0].length);
    else out.push(line);
  }
  return out.join(" ");
}

const SEP = /[ \t=(),]/;

/** The card's tokens: a `{..}` or `[..]` expression whole, else a run up to a separator (`bracedExpr`, `vectorExpr`, `token`). */
export function cardTokens(card: string): string[] {
  const out: string[] = [];
  let i = 0;
  while (i < card.length) {
    const ch = card[i]!;
    if (SEP.test(ch)) {
      i++;
      continue;
    }
    if (ch === "{" || ch === "[") {
      const close = card.indexOf(ch === "{" ? "}" : "]", i + 1);
      const end = close < 0 ? card.length : close + 1;
      out.push(card.slice(i, end));
      i = end;
      continue;
    }
    let j = i;
    while (j < card.length && !SEP.test(card[j]!) && card[j] !== ";") j++;
    out.push(card.slice(i, j));
    i = j;
  }
  return out;
}

/** `identifier`: a letter or '_', then letters, digits and '_'. */
const isIdentifier = (s: string) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(s);

/** A `.model` card's (or an "ako:" card's) `paramValuePairs`, in order. `paramValuePairs` is a list: it ends at the first name that is not an
 * identifier or has no value (PEGTL then refuses the card, `must<..>`): `ok` false. */
export function modelCardParams(code: string): { params: [string, string][]; ok: boolean; ako: string | null } {
  const tokens = cardTokens(joinCard(code));
  // ".model NAME TYPE" (VDMOS: "VDMOS NCHAN"), or ".model NAME ako: PARENT [TYPE]"
  let i = 2;
  let ako: string | null = null;
  if ((tokens[2] ?? "").toLowerCase() === "ako:") {
    ako = tokens[3] ?? "";
    i = 4;
    // `opt<sep, dotModelType>`: the word after the parent is the type
    if (i < tokens.length) i += /^vdmos$/i.test(tokens[i]!) && /^(n|p)chan$/i.test(tokens[i + 1] ?? "") ? 2 : 1;
  } else {
    i += /^vdmos$/i.test(tokens[2] ?? "") && /^(n|p)chan$/i.test(tokens[3] ?? "") ? 2 : 1;
  }
  const params: [string, string][] = [];
  while (i < tokens.length) {
    const name = tokens[i]!;
    const value = tokens[i + 1];
    if (!isIdentifier(name) || value === undefined) return { params, ok: false, ako };
    params.push([name, value]);
    i += 2;
  }
  return { params, ok: true, ako };
}

/** A subcircuit's `dotSubcktParams` from its head line: the pairs after "params:" (or from the first `name=value`). */
export function subcktParams(code: string): [string, string][] {
  const head = joinCard(code.split(/\r?\n(?![ \t]*\+)/)[0] ?? "");
  const m = /\bparams:/i.exec(head);
  let rest: string;
  if (m) rest = head.slice(m.index + m[0].length);
  else {
    const eq = /[A-Za-z_][A-Za-z0-9_]*[ \t]*=/.exec(head);
    if (!eq) return [];
    rest = head.slice(eq.index);
  }
  const tokens = cardTokens(rest);
  const out: [string, string][] = [];
  for (let i = 0; i + 1 < tokens.length; i += 2) {
    if (!isIdentifier(tokens[i]!)) break;
    out.push([tokens[i]!, tokens[i + 1]!]);
  }
  return out;
}

/** `SPICE_MODEL_PARSER_SUBCKT::ReadModel`'s parameter: an instance parameter with the `params:` value as its default (the rest of `PARAM::INFO`'s
 * defaults: id 0, DIR_INOUT, TYPE_FLOAT, no unit, PRINCIPAL). */
function subcktParamInfo(name: string, defaultValue: string): ParamInfo {
  return {
    name,
    id: 0,
    dir: "DIR_INOUT",
    type: "TYPE_FLOAT",
    unit: "",
    category: "PRINCIPAL",
    defaultValue,
    description: "",
    isSpiceInstanceParam: true,
    isInstanceParam: true,
    spiceModelName: "",
    spiceInstanceName: "",
    enumValues: [],
  };
}

/** `SIM_MODEL_SPICE::Create( aLibrary, aSpiceCode, .. )`: the library's model. `findModel` is `SIM_LIBRARY::FindModel` over the models read
 * before it (an "ako:" model's parent). */
export function baseModelOf(tables: SimModelTables, src: LibraryModelSource, findModel: (name: string) => SimModel | null): SimModel {
  if (src.kind === "subckt") {
    const m = new SimModelSubckt(tables);
    for (const port of src.pins) m.addPin({ modelPinName: port, symbolPinNumber: String(m.getPinCount() + 1) });
    for (const [name, value] of subcktParams(src.code)) m.addParam(subcktParamInfo(name, value));
    m.spiceCode = src.code;
    return m;
  }
  const type = src.modelType ?? "RAWSPICE";
  const model = createModel(tables, type);
  if (model instanceof SimModelNgspice) {
    try {
      const card = modelCardParams(src.code);
      if (!card.ok) throw new SimModelError("parse error");
      if (card.ako !== null) {
        const parent = findModel(card.ako);
        if (!parent) throw new SimModelError(`Could not find parent model '${card.ako}' for "A Kind Of" model '${src.name}'`);
        for (let i = 0; i < parent.getParamCount() && i < model.getParamCount(); i++) model.setParamValue(i, parent.getParam(i).value);
      }
      for (const [name, value] of card.params) model.setParamFromSpiceCode(name, value);
      model.spiceCode = src.code;
      return model;
    } catch (e) {
      if (!(e instanceof SimModelError)) throw e;
      // "Fall back to raw spice code"
    }
  }
  return new SimModelSpiceFallback(tables, type, src.code);
}

/** The base models of a library, in its order (`SPICE_LIBRARY_PARSER::ReadFile`: an "ako:" model finds a model read before it, by name without
 * case). */
export function libraryBaseModels(tables: SimModelTables, sources: readonly LibraryModelSource[]): Map<string, SimModel> {
  const out = new Map<string, SimModel>();
  const find = (name: string) => {
    for (const [n, m] of out) if (n.toLowerCase() === name.toLowerCase()) return m;
    return null;
  };
  for (const src of sources) if (src.kind !== "ibis" && !out.has(src.name)) out.set(src.name, baseModelOf(tables, src, find));
  return out;
}

/** `SIM_MODEL::Create( aBaseModel, aPins[, aFields], .. )` as `loadLibrary` makes the dialog's model of a library model: one of the base model's
 * type (a fallback's or a raw element's own kind) with the base model behind it, given the fields when they name it. A field that does not read
 * is reported ("Error reading simulation model from symbol '%s':\n%s"), the model kept as far as it got. */
export function libraryModel(
  tables: SimModelTables,
  base: SimModel,
  symbolPins: readonly { number: string }[],
  fields: Fields | null,
  report?: (message: string) => void
): SimModel {
  let model: SimModel;
  if (base instanceof SimModelSpiceFallback) model = new SimModelSpiceFallback(tables, base.type);
  else if (base instanceof SimModelRawSpice) model = new SimModelRawSpice(tables);
  else model = createModel(tables, base.type);
  model.setBaseModel(base);
  try {
    model.readDataFields(fields, symbolPins);
  } catch (e) {
    if (!(e instanceof SimModelError)) throw e;
    report?.(`Error reading simulation model from symbol '${fields?.Reference ?? ""}':\n${e.message}`);
  }
  return model;
}
