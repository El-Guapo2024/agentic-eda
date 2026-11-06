// Auto-completion in the custom rules editors (KiCad 8303b2ad). Each editor reads the text from the start of the rule (or of the text) to the
// caret every time a character is typed (`wxEVT_STC_CHARADDED`, or Ctrl+Space), works out what is being written there and offers the words
// that fit, through `SCINTILLA_TRICKS::DoAutocomplete` (common/scintilla_tricks.cpp) and Scintilla's list:
//
//   - `PANEL_SETUP_RULES::onScintillaCharAdded` (pcbnew/dialogs/panel_setup_rules.cpp): Board Setup > Design Rules > Custom Rules, the whole
//     `.kicad_dru` -- `rulesCharAdded`;
//   - `DRC_RE_CUSTOM_RULE_PANEL::onScintillaCharAdded` (pcbnew/drc/rule_editor/drc_re_custom_rule_panel.cpp): the DRC Rule Editor's Custom
//     rule, the body of one rule -- `customRuleCharAdded` (its own, shorter lists: no condition strings, `max_error`, no `bridged_mask`);
//   - `PANEL_DRC_RULE_EDITOR::onScintillaCharAdded` (pcbnew/drc/rule_editor/panel_drc_rule_editor.cpp): the DRC Rule Editor's condition
//     text -- `ruleConditionCharAdded`. KiCad hides that control (the condition rows replaced it) and its rows' Custom Query editors get a
//     SCINTILLA_TRICKS whose char-added function does nothing, so this one runs nowhere in KiCad either; it is here for completeness (its
//     `Via_Type` list is "Through|Blind/buried|Micro", the setup panel's "Through|Blind|Buried|Micro").
//
// Each returns what the C++ leaves behind: the words (`tokens`, split on `|` as `wxSplit` does), the word being written (`partial`),
// whether the scan called `AutoCompCancel()`, and the quote `AddText( "\"" )` puts after `(condition ` / `(assertion `. `doAutocomplete`
// is `DoAutocomplete` with what Scintilla then does with the list (`AutoCompShow`: KiCad sorts it for Scintilla's binary search; Scintilla
// selects the first item the typed word begins, case ignored, and hides the list when there is none). The positions are the string's
// (UTF-16) where Scintilla's are UTF-8 bytes: a character outside ASCII is one character here, several garbled ones in KiCad.
//
// Pure: no React, no DOM.

/** What the editor knows of the board: `BOARD_DESIGN_SETTINGS::m_NetSettings->GetNetclasses()` and `BOARD::GetNetClassAssignmentCandidates()`. */
export interface RuleBoardNames {
  /** The net classes other than Default (`GetNetclasses()` is a `std::map`: by name). */
  netClasses: readonly string[];
  /** Every net with a name. */
  netNames: readonly string[];
}

/** src/kicad/rule_completions.json: the property names (`PROPERTY_MANAGER`, not hidden from the rules editor) and `GetSignatures()`. */
export interface RuleVocabulary {
  properties: readonly string[];
  functions: readonly string[];
}

export interface CharAdded {
  /** The words the context offers (empty: nothing to offer). */
  tokens: string[];
  /** The word under the caret the list completes. */
  partial: string;
  /** The scan called `AutoCompCancel()` (it does each time it opens a new s-expression). */
  cancel: boolean;
  /** Text `AddText` puts at the caret (the `"` after `(condition ` and `(assertion `). */
  insert: string | null;
}

/** `wxSplit( str, sep )` with its default escape `\`: a separator after a backslash is kept (the backslash dropped). */
export function wxSplit(str: string, sep: string): string[] {
  const ret: string[] = [];
  let curr = "";
  let prev = "";
  for (const ch of str) {
    if (ch === sep) {
      if (prev === "\\") curr = curr.slice(0, -1) + sep;
      else {
        ret.push(curr);
        curr = "";
      }
    } else curr += ch;
    prev = ch;
  }
  if (curr !== "" || prev === sep) ret.push(curr);
  return ret;
}

/** `wxString::Matches( mask )`: `*` any run, `?` any one character, case as written. */
export function wxMatches(text: string, mask: string): boolean {
  const t = [...text];
  const m = [...mask];
  let ti = 0;
  let mi = 0;
  let star = -1;
  let mark = 0;
  while (ti < t.length) {
    if (mi < m.length && (m[mi] === "?" || (m[mi] !== "*" && m[mi] === t[ti]))) {
      ti++;
      mi++;
    } else if (mi < m.length && m[mi] === "*") {
      star = mi++;
      mark = ti;
    } else if (star >= 0) {
      mi = star + 1;
      ti = ++mark;
    } else return false;
  }
  while (mi < m.length && m[mi] === "*") mi++;
  return mi === m.length;
}

/** `wxString::CmpNoCase`. */
export const cmpNoCase = (a: string, b: string): number => {
  const x = a.toLowerCase();
  const y = b.toLowerCase();
  return x < y ? -1 : x > y ? 1 : 0;
};

/** Scintilla's `CompareNCaseInsensitive( a, b, len )` (ASCII upper case, as `MakeUpperCase`). */
function compareNCaseInsensitive(a: string, b: string, len: number): number {
  const up = (c: number) => (c >= 97 && c <= 122 ? c - 32 : c);
  let i = 0;
  while (i < a.length && i < b.length && len) {
    const ca = a.charCodeAt(i);
    const cb = b.charCodeAt(i);
    if (ca !== cb && up(ca) !== up(cb)) return up(ca) - up(cb);
    i++;
    len--;
  }
  if (len === 0) return 0;
  return (i < a.length ? a.charCodeAt(i) : 0) - (i < b.length ? b.charCodeAt(i) : 0);
}

/**
 * Scintilla's `AutoComplete::Select( word )` over a presorted list, case ignored, case respected among equals
 * (`SC_CASEINSENSITIVEBEHAVIOUR_RESPECTCASE`): the binary search for an item `word` begins, back to the first such item, then the first of those
 * that matches its case. -1 when no item begins with it -- and the list hides (`autoHide`). The search assumes the list is in the order
 * `CompareNCaseInsensitive` gives (upper case); KiCad sorts it with `CmpNoCase` (lower case), so where they disagree (`_` sorts after the
 * letters in one and before them in the other) the search can miss, as it does in KiCad.
 */
export function autoCompSelect(items: readonly string[], word: string): number {
  const lenWord = word.length;
  let location = -1;
  let start = 0;
  let end = items.length - 1;
  while (start <= end && location === -1) {
    let pivot = (start + end) >> 1;
    let cond = compareNCaseInsensitive(word, items[pivot]!, lenWord);
    if (!cond) {
      // Find first match
      while (pivot > start) {
        cond = compareNCaseInsensitive(word, items[pivot - 1]!, lenWord);
        if (cond !== 0) break;
        --pivot;
      }
      location = pivot;
      // Check for exact-case match
      for (; pivot <= end; pivot++) {
        if (items[pivot]!.startsWith(word)) {
          location = pivot;
          break;
        }
        if (compareNCaseInsensitive(word, items[pivot]!, lenWord)) break;
      }
    } else if (cond < 0) end = pivot - 1;
    else start = pivot + 1;
  }
  return location;
}

/** An open completion list: Scintilla's after `AutoCompShow( lenEntered, list )`. */
export interface AutoCompList {
  items: string[];
  /** The selected item. */
  selected: number;
  /** `lenEntered`: how much of the word before the caret the accepted item replaces. */
  lenEntered: number;
}

/** What `DoAutocomplete` does to the list: nothing (no token matched, `AutoCompShow` is not called and an open list stays as it is), or shows
 *  a new one -- which Scintilla at once hides again when no item begins with the word (`list` null). */
export type AutocompleteOutcome = { show: false } | { show: true; list: AutoCompList | null };

/**
 * `SCINTILLA_TRICKS::DoAutocomplete( aPartial, aTokens )`: the tokens matching `*partial*` (case ignored, `wxString::Matches`), sorted with
 * `CmpNoCase`, shown with `AutoCompShow( partial.size(), ... )`, where Scintilla selects the first item the word begins (`autoCompSelect`)
 * and hides the list when there is none -- so a word found only inside items shows nothing. The list drops empty items: `wxJoin( .., '\t' )`
 * is split back with a `wxStringTokenizer` on whitespace, which skips them (KiCad's lists built as `"|" + name ...` start with one).
 * (`m_suppressAutocomplete` is the editor's: it does not call this while it is set.)
 */
export function doAutocomplete(partial: string, tokens: readonly string[]): AutocompleteOutcome {
  const filter = `*${partial.toLowerCase()}*`;
  const matched = tokens.filter((t) => wxMatches(t.toLowerCase(), filter));
  if (matched.length === 0) return { show: false };
  matched.sort(cmpNoCase);
  const items = matched.filter((t) => t !== "");
  const selected = autoCompSelect(items, partial);
  return { show: true, list: selected < 0 ? null : { items, selected, lenEntered: partial.length } };
}

/**
 * Where the scan starts: the last line, going up from the caret's, that begins with `prefix` (`GetTextRange( lineStart, lineStart + 10 )
 * .StartsWith( prefix )`; the first line is never looked at, which changes nothing as the scan then starts at 0 anyway). Only ten characters
 * are read, so a prefix longer than that -- the DRC Rule Editor's `(condition ` -- never matches.
 */
function scanStart(text: string, caret: number, prefix: string): number {
  const lineStarts = [0];
  for (let i = 0; i < caret; i++) if (text[i] === "\n") lineStarts.push(i + 1);
  for (let line = lineStarts.length - 1; line > 0; line--) {
    const lineStart = lineStarts[line]!;
    if (text.slice(lineStart, lineStart + 10).startsWith(prefix)) return lineStart;
  }
  return 0;
}

const enum Ctx {
  None,
  String,
  SexprOpen,
  SexprToken,
  SexprString,
  StructRef,
}

/** `isDisallowToken`. */
const DISALLOW_TOKENS = new Set(["blind_via", "buried_via", "graphic", "hole", "micro_via", "pad", "text", "through_via", "track", "via", "zone"]);

/** `isConstraintTypeToken`. */
const CONSTRAINT_TYPE_TOKENS = new Set([
  "annular_width", "assertion", "clearance", "connection_width", "courtyard_clearance", "diff_pair_gap", "diff_pair_uncoupled", "disallow",
  "edge_clearance", "length", "hole_clearance", "hole_size", "hole_to_hole", "min_resolved_spokes", "physical_clearance",
  "physical_hole_clearance", "silk_clearance", "skew", "solder_mask_expansion", "solder_paste_abs_margin", "solder_paste_rel_margin",
  "text_height", "text_thickness", "thermal_relief_gap", "thermal_spoke_width", "track_width", "track_angle", "track_segment_length",
  "via_count", "via_diameter", "zone_connection",
]);

/** `PANEL_SETUP_RULES::onScintillaCharAdded`'s lists after `(constraint `, `(disallow ...`, `(zone_connection `, ... word for word. */
const SETUP_CONSTRAINT_TOKENS =
  "annular_width|assertion|bridged_mask|clearance|connection_width|courtyard_clearance|creepage|diff_pair_gap|diff_pair_uncoupled|disallow|" +
  "edge_clearance|length|hole_clearance|hole_size|hole_to_hole|min_resolved_spokes|physical_clearance|physical_hole_clearance|silk_clearance|" +
  "skew|solder_mask_expansion|solder_paste_abs_margin|solder_paste_rel_margin|text_height|text_thickness|thermal_relief_gap|" +
  "thermal_spoke_width|track_width|track_angle|track_segment_length|via_count|via_diameter|zone_connection";
const DISALLOW_LIST = "blind_via|buried_via|footprint|graphic|hole|micro_via|pad|text|through_via|track|via|zone";

/** The value lists after `A.<property> == '` (both panels), by the property's regex (`^Name\s*[!=]=\s*$`). */
function valueTokens(last: string, names: RuleBoardNames, viaTypes: string): string | null {
  const is = (name: string) => new RegExp(`^${name}\\s*[!=]=\\s*$`).test(last);
  if (is("NetClass")) return names.netClasses.map((n) => `|${n}`).join("");
  if (is("NetName")) return names.netNames.map((n) => `|${n}`).join("");
  if (is("Type")) return "Bitmap|Dimension|Footprint|Graphic|Group|Leader|Pad|Target|Text|Text Box|Track|Via|Zone";
  if (is("Via_Type")) return viaTypes;
  if (is("Pad_Type")) return "Through-hole|SMD|Edge connector|NPTH, mechanical";
  if (is("Pin_Type")) return "Input|Output|Bidirectional|Tri-state|Passive|Free|Unspecified|Power input|Power output|Open collector|Open emitter|Unconnected";
  if (is("Fabrication_Property")) return "None|BGA pad|Fiducial, global to board|Fiducial, local to footprint|Test point pad|Heatsink pad|Castellated pad";
  if (is("Shape")) return "Segment|Rectangle|Arc|Circle|Polygon|Bezier";
  if (is("Pad_Shape")) return "Circle|Rectangle|Oval|Trapezoid|Rounded rectangle|Chamfered rectangle|Custom";
  if (is("Pad_Connections")) return "Inherited|None|Solid|Thermal reliefs|Thermal reliefs for PTH";
  if (is("Zone_Connection_Style")) return "Inherited|None|Solid|Thermal reliefs";
  if (is("Line_Style")) return "Default|Solid|Dashed|Dotted|Dash-Dot|Dash-Dot-Dot";
  if (is("Horizontal_Justification")) return "Left|Center|Right";
  if (is("Vertical_Justification")) return "Top|Center|Bottom";
  return null;
}

/** After `A.`: every property name (`std::set`: sorted, unique) and every function signature not marked DEPRECATED, each after a `|`. */
function structRefTokens(vocab: RuleVocabulary): string {
  let tokens = "";
  for (const p of vocab.properties) tokens += `|${p}`;
  for (const f of vocab.functions) if (!f.includes("DEPRECATED")) tokens += `|${f}`;
  return tokens;
}

const result = (tokens: string, partial: string, cancel: boolean, insert: string | null = null): CharAdded => ({
  tokens: tokens === "" ? [] : wxSplit(tokens, "|"),
  partial,
  cancel,
  insert,
});

/** `PANEL_SETUP_RULES::onScintillaCharAdded`: Board Setup > Custom Rules. `caret` is `GetCurrentPos()`, after the character just typed. */
export function rulesCharAdded(text: string, caret: number, vocab: RuleVocabulary, names: RuleBoardNames): CharAdded {
  const startPos = scanStart(text, caret, "(rule ");
  const sexprs: string[] = [];
  const top = () => sexprs[sexprs.length - 1];
  let partial = "";
  let last = "";
  let constraintType = "";
  let context = Ctx.None;
  let exprContext = Ctx.None;
  let cancel = false;

  for (let i = startPos; i < caret; ++i) {
    const c = text[i];
    if (c === "\\") {
      i++; // skip escaped char
    } else if (context === Ctx.String) {
      if (c === '"') {
        context = Ctx.None;
      } else if (exprContext === Ctx.String) {
        if (c === "'") exprContext = Ctx.None;
        else partial += c;
      } else if (c === "'") {
        last = partial;
        partial = "";
        exprContext = Ctx.String;
      } else if (c === ".") {
        partial = "";
        exprContext = Ctx.StructRef;
      } else {
        partial += c;
      }
    } else if (c === '"') {
      last = partial;
      partial = "";
      context = Ctx.String;
    } else if (c === "(") {
      if (context === Ctx.SexprOpen && partial !== "") {
        cancel = true;
        sexprs.push(partial);
      }
      partial = "";
      context = Ctx.SexprOpen;
    } else if (c === ")") {
      while (sexprs.length && (top() === "assertion" || top() === "disallow" || DISALLOW_TOKENS.has(top()!) || top() === "min_resolved_spokes" || top() === "zone_connection")) {
        sexprs.pop();
      }
      if (sexprs.length) {
        // Ignore argument-less tokens
        if (partial === "within_diff_pairs") {
          partial = "";
        } else {
          if (top() === "constraint") constraintType = "";
          sexprs.pop();
        }
      }
      context = Ctx.None;
    } else if (c === " ") {
      if (context === Ctx.SexprOpen && partial !== "") {
        cancel = true;
        sexprs.push(partial);
        if (partial === "constraint" || partial === "layer" || partial === "severity") context = Ctx.SexprToken;
        else if (partial === "rule" || partial === "condition") context = Ctx.SexprString;
        else context = Ctx.None;
        partial = "";
        continue;
      } else if (partial === "disallow" || DISALLOW_TOKENS.has(partial) || partial === "min_resolved_spokes" || partial === "zone_connection") {
        cancel = true;
        sexprs.push(partial);
        partial = "";
        context = Ctx.SexprToken;
        continue;
      } else if (partial === "assertion") {
        cancel = true;
        sexprs.push(partial);
        partial = "";
        context = Ctx.SexprString;
        continue;
      } else if (CONSTRAINT_TYPE_TOKENS.has(partial)) {
        constraintType = partial;
      }
      context = Ctx.None;
    } else {
      partial += c;
    }
  }

  let tokens = "";
  if (context === Ctx.SexprOpen) {
    if (!sexprs.length) tokens = "rule|version";
    else if (top() === "rule") tokens = "condition|constraint|layer|severity";
    else if (top() === "constraint") tokens = constraintType === "skew" ? "max|min|opt|within_diff_pairs" : "max|min|opt";
  } else if (context === Ctx.SexprToken) {
    if (!sexprs.length) {
      /* badly formed grammar */
    } else if (top() === "constraint") tokens = SETUP_CONSTRAINT_TOKENS;
    else if (top() === "disallow" || DISALLOW_TOKENS.has(top()!)) tokens = DISALLOW_LIST;
    else if (top() === "zone_connection") tokens = "none|solid|thermal_reliefs";
    else if (top() === "min_resolved_spokes") tokens = "0|1|2|3|4";
    else if (top() === "layer") tokens = 'inner|outer|"x"';
    else if (top() === "severity") tokens = "warning|error|ignore|exclusion";
  } else if (context === Ctx.SexprString && sexprs.length && (top() === "condition" || top() === "assertion")) {
    return result("", partial, cancel, '"');
  } else if (context === Ctx.String && sexprs.length && (top() === "condition" || top() === "assertion")) {
    if (exprContext === Ctx.StructRef) tokens = structRefTokens(vocab);
    else if (exprContext === Ctx.String) tokens = valueTokens(last, names, "Through|Blind|Buried|Micro") ?? "";
  }
  return result(tokens, partial, cancel);
}

/** `DRC_RE_CUSTOM_RULE_PANEL::onScintillaCharAdded`: the DRC Rule Editor's Custom rule body (no strings, a scan from the start of the text). */
export function customRuleCharAdded(text: string, caret: number): CharAdded {
  const sexprs: string[] = [];
  const top = () => sexprs[sexprs.length - 1];
  let partial = "";
  let context = Ctx.None;
  let cancel = false;

  for (let i = 0; i < caret; ++i) {
    const c = text[i];
    if (c === "\\") {
      i++; // skip escaped char
    } else if (context === Ctx.String) {
      if (c === '"') context = Ctx.None;
    } else if (c === '"') {
      partial = "";
      context = Ctx.String;
    } else if (c === "(") {
      if (context === Ctx.SexprOpen && partial !== "") {
        cancel = true;
        sexprs.push(partial);
      }
      partial = "";
      context = Ctx.SexprOpen;
    } else if (c === ")") {
      if (sexprs.length) sexprs.pop();
      context = Ctx.None;
    } else if (c === " ") {
      if (context === Ctx.SexprOpen && partial !== "") {
        cancel = true;
        sexprs.push(partial);
        context = Ctx.SexprToken;
        partial = "";
        continue;
      }
      context = Ctx.None;
    } else {
      partial += c;
    }
  }

  let tokens = "";
  if (context === Ctx.SexprOpen) {
    // Inside (rule ...) - suggest sub-expressions; inside (constraint ...) - suggest constraint parameters
    if (!sexprs.length || top() === "rule") tokens = "condition|constraint|layer|severity";
    else if (top() === "constraint") tokens = "min|max|opt";
  } else if (context === Ctx.SexprToken) {
    if (sexprs.length && top() === "constraint") {
      // After (constraint - suggest constraint types
      tokens =
        "annular_width|assertion|clearance|connection_width|courtyard_clearance|creepage|diff_pair_gap|diff_pair_uncoupled|disallow|" +
        "edge_clearance|hole_clearance|hole_size|hole_to_hole|length|max_error|min_resolved_spokes|physical_clearance|" +
        "physical_hole_clearance|silk_clearance|skew|text_height|text_thickness|thermal_relief_gap|thermal_spoke_width|track_angle|" +
        "track_segment_length|track_width|via_count|via_diameter|zone_connection";
    } else if (sexprs.length && top() === "layer") tokens = "inner|outer"; // After (layer - suggest layer keywords
    else if (sexprs.length && top() === "severity") tokens = "error|warning|ignore|exclusion"; // After (severity - suggest severity levels
  }
  return result(tokens, partial, cancel);
}

/** `PANEL_DRC_RULE_EDITOR::onScintillaCharAdded`: the DRC Rule Editor's condition text (see the header: no control of KiCad's runs it). */
export function ruleConditionCharAdded(text: string, caret: number, vocab: RuleVocabulary, names: RuleBoardNames): CharAdded {
  const startPos = scanStart(text, caret, "(condition ");
  const sexprs: string[] = [];
  const top = () => sexprs[sexprs.length - 1];
  let partial = "";
  let last = "";
  let context = Ctx.None;
  let exprContext = Ctx.None;
  let cancel = false;

  for (let i = startPos; i < caret; ++i) {
    const c = text[i];
    if (c === "\\") {
      i++; // skip escaped char
    } else if (context === Ctx.String) {
      if (c === '"') {
        context = Ctx.None;
      } else if (exprContext === Ctx.String) {
        if (c === "'") exprContext = Ctx.None;
        else partial += c;
      } else if (c === "'") {
        last = partial;
        partial = "";
        exprContext = Ctx.String;
      } else if (c === ".") {
        partial = "";
        exprContext = Ctx.StructRef;
      } else {
        partial += c;
      }
    } else if (c === '"') {
      last = partial;
      partial = "";
      context = Ctx.String;
    } else if (c === "(") {
      if (context === Ctx.SexprOpen && partial !== "") {
        cancel = true;
        sexprs.push(partial);
      }
      partial = "";
      context = Ctx.SexprOpen;
    } else if (c === ")") {
      context = Ctx.None;
    } else if (c === " ") {
      if (context === Ctx.SexprOpen && partial !== "") {
        cancel = true;
        sexprs.push(partial);
        context = partial === "condition" ? Ctx.SexprString : Ctx.None;
        partial = "";
        continue;
      }
      context = Ctx.None;
    } else {
      partial += c;
    }
  }

  let tokens = "";
  if (context === Ctx.SexprOpen) {
    if (!sexprs.length) tokens = "condition";
  } else if (context === Ctx.SexprString && sexprs.length && top() === "condition") {
    return result("", partial, cancel, '"');
  } else if (context === Ctx.String && sexprs.length && top() === "condition") {
    if (exprContext === Ctx.StructRef) tokens = structRefTokens(vocab);
    else if (exprContext === Ctx.String) tokens = valueTokens(last, names, "Through|Blind/buried|Micro") ?? "";
  }
  return result(tokens, partial, cancel);
}

/**
 * Enter on the list (`SCINTILLA_TRICKS::onCharHook`): after `AutoCompComplete()` the last `<...>` of the inserted text, if there is one,
 * is selected (`FindText( finish, start, "<" )` and `">"`, both searched backwards from the end). `start` is `AutoCompPosStart()` (the caret
 * when the list opened), `finish` the caret after the completion. Null when nothing is to be selected.
 */
export function substitutionSelection(text: string, start: number, finish: number): [number, number] | null {
  if (finish <= start) return null;
  const range = text.slice(start, finish);
  const lt = range.lastIndexOf("<");
  const gt = range.lastIndexOf(">");
  const selStart = lt < 0 ? -1 : start + lt;
  const selEnd = gt < 0 ? -1 : start + gt;
  if (selStart > start && selEnd <= finish && selEnd > selStart) return [selStart, selEnd + 1];
  return null;
}
