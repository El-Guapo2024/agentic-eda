import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  autoCompSelect,
  cmpNoCase,
  customRuleCharAdded,
  doAutocomplete,
  ruleConditionCharAdded,
  rulesCharAdded,
  substitutionSelection,
  wxMatches,
  wxSplit,
  type RuleVocabulary,
} from "./ruleCompletion";

const vocab = JSON.parse(readFileSync("src/kicad/rule_completions.json", "utf8")) as RuleVocabulary & { hiddenFromRulesEditor: string[] };
const names = { netClasses: ["HV", "power"], netNames: ["/CLK_N", "/CLK_P", "GND"] };
/** The panel at the end of `text` (the caret after the last character typed). */
const at = (text: string) => rulesCharAdded(text, text.length, vocab, names);

const SETUP_CONSTRAINTS =
  "annular_width assertion bridged_mask clearance connection_width courtyard_clearance creepage diff_pair_gap diff_pair_uncoupled disallow edge_clearance length hole_clearance hole_size hole_to_hole min_resolved_spokes physical_clearance physical_hole_clearance silk_clearance skew solder_mask_expansion solder_paste_abs_margin solder_paste_rel_margin text_height text_thickness thermal_relief_gap thermal_spoke_width track_width track_angle track_segment_length via_count via_diameter zone_connection".split(" ");

test("PANEL_SETUP_RULES: the s-expression keywords where each opens", () => {
  assert.deepEqual(at("(").tokens, ["rule", "version"]);
  assert.deepEqual(at("(version 1)\n(ru"), { tokens: ["rule", "version"], partial: "ru", cancel: true, insert: null });
  assert.deepEqual(at("(rule x (").tokens, ["condition", "constraint", "layer", "severity"]);
  const c = at("(rule x (constraint ");
  assert.deepEqual(c.tokens, SETUP_CONSTRAINTS);
  assert.equal(c.partial, "");
  assert.equal(c.cancel, true, "opening (rule and (constraint calls AutoCompCancel()");
  assert.deepEqual(at("(rule x (constraint clearance (").tokens, ["max", "min", "opt"]);
  assert.deepEqual(at("(rule x (constraint skew (").tokens, ["max", "min", "opt", "within_diff_pairs"]);
  assert.deepEqual(at("(rule x (constraint skew (max 1mm) (within_diff_pairs) (").tokens, ["max", "min", "opt", "within_diff_pairs"], "within_diff_pairs takes no argument");
  assert.deepEqual(at("(rule x (layer ").tokens, ["inner", "outer", '"x"']);
  assert.deepEqual(at("(rule x (severity ").tokens, ["warning", "error", "ignore", "exclusion"]);
  assert.deepEqual(at("(rule x (constraint zone_connection ").tokens, ["none", "solid", "thermal_reliefs"]);
  assert.deepEqual(at("(rule x (constraint min_resolved_spokes ").tokens, ["0", "1", "2", "3", "4"]);
  const disallow = "blind_via buried_via footprint graphic hole micro_via pad text through_via track via zone".split(" ");
  assert.deepEqual(at("(rule x (constraint disallow ").tokens, disallow);
  assert.deepEqual(at("(rule x (constraint disallow track via ").tokens, disallow, "each disallowed item opens another");
  assert.deepEqual(at("(rule x (constraint clearance (min 1mm)) (").tokens, ["condition", "constraint", "layer", "severity"], "back in the rule after the constraint closes");
});

test("PANEL_SETUP_RULES: the quote after (condition and (assertion, the properties after A., the values after a comparison", () => {
  assert.deepEqual(at("(rule x (condition "), { tokens: [], partial: "", cancel: true, insert: '"' });
  assert.equal(at("(rule x (constraint assertion ").insert, '"');
  const props = at('(rule x (condition "A.');
  assert.equal(props.partial, "");
  // `"|" + name`: the list starts with an empty token, which the list box drops
  assert.equal(props.tokens[0], "");
  assert.ok(props.tokens.includes("NetClass") && props.tokens.includes("Via_Type") && props.tokens.includes("hasNetclass('x')"));
  assert.ok(!props.tokens.some((t) => t.includes("DEPRECATED")), "no DEPRECATED function");
  assert.ok(!props.tokens.includes("Net_Class") && !props.tokens.includes("Font"), "the properties hidden from the rules editor");
  assert.equal(props.tokens.length, 1 + vocab.properties.length + vocab.functions.filter((f) => !f.includes("DEPRECATED")).length);
  assert.equal(at('(rule x (condition "A.Net').partial, "Net");
  assert.deepEqual(at("(rule x (condition \"A.NetClass == '").tokens, ["", "HV", "power"]);
  assert.deepEqual(at("(rule x (condition \"A.NetClass != 'p").tokens, ["", "HV", "power"]);
  assert.equal(at("(rule x (condition \"A.NetClass != 'p").partial, "p");
  assert.deepEqual(at("(rule x (condition \"A.NetName=='").tokens, ["", "/CLK_N", "/CLK_P", "GND"]);
  assert.deepEqual(at("(rule x (condition \"A.Type == '").tokens, "Bitmap|Dimension|Footprint|Graphic|Group|Leader|Pad|Target|Text|Text Box|Track|Via|Zone".split("|"));
  assert.deepEqual(at("(rule x (condition \"A.Via_Type == '").tokens, ["Through", "Blind", "Buried", "Micro"]);
  assert.deepEqual(at("(rule x (condition \"A.Pad_Type == '").tokens, ["Through-hole", "SMD", "Edge connector", "NPTH, mechanical"]);
  assert.deepEqual(at("(rule x (condition \"A.Vertical_Justification == '").tokens, ["Top", "Center", "Bottom"]);
  assert.deepEqual(at("(rule x (condition \"A.Locked == '").tokens, [], "a property with no value list");
  assert.deepEqual(at("(rule x (condition \"A.NetClass == 'HV' && B.NetClass == '").tokens, ["", "HV", "power"]);
});

test("PANEL_SETUP_RULES: the scan starts at the line of the last (rule above the caret", () => {
  const text = '(version 1)\n(rule a\n  (condition "A.Type == \'Via\'"))\n(rule b (constraint ';
  assert.deepEqual(at(text).tokens, SETUP_CONSTRAINTS);
  // an unclosed string in an earlier rule does not leak into the next
  assert.deepEqual(at('(rule a (condition "A.x\n(rule b (').tokens, ["condition", "constraint", "layer", "severity"]);
  // the caret in the middle: only the text before it counts
  const mid = "(rule x (constraint ) (min 1mm))";
  assert.deepEqual(rulesCharAdded(mid, "(rule x (constraint ".length, vocab, names).tokens, SETUP_CONSTRAINTS);
});

test("DRC_RE_CUSTOM_RULE_PANEL: its own lists (max_error, no bridged_mask or solder ones), no strings", () => {
  assert.deepEqual(customRuleCharAdded("(", 1).tokens, ["condition", "constraint", "layer", "severity"]);
  const t = "   (constraint ";
  const c = customRuleCharAdded(t, t.length);
  assert.ok(c.tokens.includes("max_error") && !c.tokens.includes("bridged_mask") && !c.tokens.includes("solder_mask_expansion"));
  assert.equal(c.tokens.length, 30);
  assert.deepEqual(customRuleCharAdded("(constraint clearance (", 23).tokens, ["min", "max", "opt"]);
  assert.deepEqual(customRuleCharAdded("(layer ", 7).tokens, ["inner", "outer"]);
  assert.deepEqual(customRuleCharAdded("(severity ", 10).tokens, ["error", "warning", "ignore", "exclusion"]);
  const cond = '(condition "A.';
  assert.deepEqual(customRuleCharAdded(cond, cond.length), { tokens: [], partial: "", cancel: true, insert: null });
});

test("PANEL_DRC_RULE_EDITOR: condition, its quote, Blind/buried", () => {
  assert.deepEqual(ruleConditionCharAdded("(", 1, vocab, names).tokens, ["condition"]);
  assert.equal(ruleConditionCharAdded("(condition ", 11, vocab, names).insert, '"');
  const v = "(condition \"A.Via_Type == '";
  assert.deepEqual(ruleConditionCharAdded(v, v.length, vocab, names).tokens, ["Through", "Blind/buried", "Micro"]);
});

test("DoAutocomplete: *partial* case-blind, CmpNoCase order, and Scintilla's list shows only when an item begins with the word", () => {
  const tokens = at("(rule x (constraint ").tokens;
  const cl = doAutocomplete("cl", tokens);
  assert.ok(cl.show && cl.list);
  assert.deepEqual(cl.list.items, ["clearance", "courtyard_clearance", "edge_clearance", "hole_clearance", "physical_clearance", "physical_hole_clearance", "silk_clearance"]);
  assert.equal(cl.list.selected, 0);
  assert.equal(cl.list.lenEntered, 2);
  const all = doAutocomplete("", tokens);
  assert.ok(all.show && all.list && all.list.items.length === 33 && all.list.items[0] === "annular_width");
  // found inside items only: shown, then hidden at once (autoHide)
  assert.deepEqual(doAutocomplete("arance", tokens), { show: true, list: null });
  // nothing matches: not shown at all
  assert.deepEqual(doAutocomplete("zzz", tokens), { show: false });
  // case ignored, the exact case preferred
  const props = doAutocomplete("net", at('(rule x (condition "A.').tokens);
  assert.ok(props.show && props.list && props.list.items[props.list.selected] === "Net");
  const NC = doAutocomplete("NetC", at('(rule x (condition "A.').tokens);
  assert.ok(NC.show && NC.list && NC.list.items[NC.list.selected] === "NetClass" && NC.list.items.includes("hasNetclass('x')"));
  assert.ok(!all.list.items.includes(""), "the empty token is not an item");
});

test("Scintilla's binary search misses where CmpNoCase and its upper-case compare disagree about _", () => {
  const items = ["Pad_Type", "PadA"].sort(cmpNoCase); // "pad_type" < "pada": '_' before 'a'
  assert.deepEqual(items, ["Pad_Type", "PadA"]);
  assert.equal(autoCompSelect(items, "Pad_"), 0);
  // upper case: "PADA" < "PAD_" ('A' 0x41 before '_' 0x5F): the search goes left of "PadA" and finds nothing
  assert.equal(autoCompSelect(items, "PadA"), -1);
  // upper case: "PAD_" > "PADA" ('_' 0x5F after 'A' 0x41): the search goes right of the only "Pad_" item
  assert.equal(autoCompSelect(["Pad_Type", "PadA", "PadB"], "Pad_"), -1);
});

test("wxSplit, wxString::Matches, the <...> selection after Enter", () => {
  assert.deepEqual(wxSplit("|a|b", "|"), ["", "a", "b"]);
  assert.deepEqual(wxSplit("a\\|b|c|", "|"), ["a|b", "c", ""]);
  assert.deepEqual(wxSplit("", "|"), []);
  assert.ok(wxMatches("hasnetclass('x')", "*net*"));
  assert.ok(wxMatches("abc", "a?c") && wxMatches("abc", "*") && !wxMatches("abc", "*d*") && wxMatches("", "**"));
  assert.deepEqual(substitutionSelection("A.memberOf('<group>')", 4, 21), [12, 19]);
  assert.equal(substitutionSelection("A.memberOf('x')", 4, 15), null);
  assert.equal(substitutionSelection("<a>bc", 0, 5), null, "a < at the start of the range is not after it");
});
