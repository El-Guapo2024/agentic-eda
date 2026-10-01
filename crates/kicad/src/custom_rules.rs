//! Ported from `pcbnew/drc/drc_rule_parser.cpp`: reads a `.kicad_dru`
//! custom-rule file's `(rule ...)` blocks into [`eda_model::CustomRule`]
//! (task item 4). This is a *structural* port -- it reads the same
//! `(rule NAME (constraint TYPE (min V) (max V) (opt V)) (layer L)
//! (severity S) (condition "..."))` shape `drc_rule_parser.cpp`'s
//! `DRC_RULES_PARSER::parseDRC_RULE` does -- not a byte-for-byte copy of
//! that recursive-descent parser, which also validates a much larger
//! grammar (every constraint type's own argument shape, `disallow`'s item
//! list, `assertion`'s own expression, legacy layer-set tokens, ...) this
//! crate has no use for yet. Evaluating the `condition` text this collects
//! is [`eda_drc::pcbexpr`]'s job, at DRC time, against a real item pair --
//! not this module's.
//!
//! Unlike `.kicad_pcb`/`.kicad_pro`, a `.kicad_dru` file is not one root
//! s-expression: it is `(version N)` followed by zero or more top-level
//! `(rule ...)` forms with nothing wrapping them. [`crate::sexpr::parse`]
//! only accepts a single root list, so this wraps the (comment-stripped)
//! text in one synthetic `(__root__ ...)` list before handing it to the
//! same tokenizer every other KiCad text format in this crate uses --
//! no changes to `sexpr.rs` needed, and every one of its "unknown tag is
//! just invisible" guarantees still hold.
//!
//! A `.kicad_dru` file also allows `#`-to-end-of-line comments (confirmed
//! directly against real KiCad QA-corpus rule files, including ones that
//! comment out a whole disabled rule this way), which `sexpr.rs`'s
//! tokenizer has no concept of -- see [`strip_comments`]. Getting this
//! wrong would silently *activate* a rule the file's author deliberately
//! disabled, which is worse than not parsing the file at all.

use eda_model::ir::Um;
use eda_model::CustomRule;

use crate::import::mm_to_um;
use crate::sexpr::{self, Sexpr};

/// Strip `#`-to-end-of-line comments, respecting `"..."` quoting (a `#`
/// inside a quoted condition string -- e.g. a reference pattern -- is data,
/// not a comment starter). Mirrors `sexpr.rs`'s own quoting rules
/// (`\"`/`\\` escapes) just enough to track whether we are inside a string;
/// it does not need to *decode* the string, only to not misread its quotes.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => {
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                }
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '#' => {
                for c2 in chars.by_ref() {
                    if c2 == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// A `.kicad_dru` length value: a bare number (KiCad's own default unit,
/// mm) or a number immediately followed by a unit suffix with no space
/// (`"0.2mm"`, `"6.35mm"`, `"4mil"`, `"0.01in"` -- confirmed against real
/// QA-corpus rule files, all of which happen to use `mm`, but the other two
/// are real KiCad units worth not mis-parsing as a bare number). An
/// unrecognized suffix (angles like `"90 deg"` tokenize as a *separate*
/// atom, not part of this one, so they never reach this function; a
/// genuinely unknown unit does) returns `None` rather than guessing.
fn parse_length(s: &str) -> Option<Um> {
    let s = s.trim();
    let split_at = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split_at);
    let n: f64 = num.parse().ok()?;
    let mm = match unit {
        "" | "mm" => n,
        "mil" => n * 0.0254,
        "in" => n * 25.4,
        _ => return None,
    };
    Some(mm_to_um(mm))
}

/// `(constraint TYPE (min V) (max V) (opt V))` -- `TYPE` is always present
/// (`drc_rule_parser.cpp` rejects a constraint with none); the three bound
/// sub-lists are each optional, and `TYPE` alone with no bound at all is a
/// valid KiCad rule for a handful of constraint types this crate does not
/// evaluate (`disallow`, `assertion`), parsed here without error same as
/// anything else not acted on.
fn parse_constraint(c: &[Sexpr]) -> (String, Option<Um>, Option<Um>, Option<Um>) {
    let ty = sexpr::txt(c, 1).unwrap_or("").to_string();
    let bound = |key: &str| sexpr::find(c, key).and_then(|b| sexpr::txt(b, 1)).and_then(parse_length);
    (ty, bound("min"), bound("max"), bound("opt"))
}

fn parse_rule(r: &[Sexpr]) -> Option<CustomRule> {
    let name = sexpr::txt(r, 1)?.to_string();
    let (constraint_type, min, max, opt) = sexpr::find(r, "constraint").map(parse_constraint).unwrap_or_default();
    let layer = sexpr::find(r, "layer").and_then(|l| sexpr::txt(l, 1)).map(String::from);
    let severity = sexpr::find(r, "severity").and_then(|l| sexpr::txt(l, 1)).map(String::from);
    let condition = sexpr::find(r, "condition").and_then(|l| sexpr::txt(l, 1)).map(String::from);
    Some(CustomRule { name, constraint_type, min, max, opt, layer, severity, condition })
}

/// Parse every `(rule ...)` block in a `.kicad_dru` file's text. Returns an
/// empty `Vec` for anything this cannot make sense of (unparseable
/// s-expression text, no `(rule ...)` forms at all) -- same "absent/invalid
/// input is a no-op, not an error" convention as [`crate::import::
/// parse_project_net_classes`], since a `.kicad_dru` file is optional and a
/// missing or malformed one should never block an otherwise-good board
/// from importing.
pub fn parse_custom_rules(text: &str) -> Vec<CustomRule> {
    let wrapped = format!("(__root__ {})", strip_comments(text));
    let Ok(tree) = sexpr::parse(&wrapped) else { return Vec::new() };
    let Some(root) = tree.as_list() else { return Vec::new() };
    sexpr::find_all(root, "rule").filter_map(parse_rule).collect()
}

/// Merge a sidecar `<board>.kicad_dru` file into an already-imported board
/// (task item 4), keeping its raw text alongside the parsed rules (see
/// `eda_model::BoardRules::custom_rules_text`'s doc comment for why both).
/// A caller with a real project directory should call this after
/// [`crate::import_kicad_pcb`], same pattern as
/// [`crate::merge_project_net_classes`]/[`crate::merge_project_rule_severities`]
/// -- except the custom-rule file is its own sibling file, not a section
/// inside `.kicad_pro`, so the caller reads `<board>.kicad_dru` rather than
/// `<board>.kicad_pro`.
pub fn merge_custom_rules(model: &mut eda_model::ConstraintModel, dru_text: &str) {
    model.board.custom_rules = parse_custom_rules(dru_text);
    model.board.custom_rules_text = Some(dru_text.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_simple_clearance_rule() {
        let text = "(version 1)\n(rule ReduceClearanceSameClass\n   (constraint clearance (min 0.2mm))\n   (condition \"A.NetClass == B.NetClass\")\n)\n";
        let rules = parse_custom_rules(text);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "ReduceClearanceSameClass");
        assert_eq!(rules[0].constraint_type, "clearance");
        assert_eq!(rules[0].min, Some(200));
        assert_eq!(rules[0].condition.as_deref(), Some("A.NetClass == B.NetClass"));
    }

    #[test]
    fn parses_a_quoted_name_and_severity_and_layer() {
        let text = "(version 1)\n(rule \"Distance between 1mm test points\"\n\t(constraint courtyard_clearance (min 1.54mm))\n\t(layer \"F.Cu\")\n\t(severity warning)\n\t(condition \"A.Reference =='TP*'\")\n)\n";
        let rules = parse_custom_rules(text);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "Distance between 1mm test points");
        assert_eq!(rules[0].layer.as_deref(), Some("F.Cu"));
        assert_eq!(rules[0].severity.as_deref(), Some("warning"));
    }

    #[test]
    fn multiple_rules_in_one_file_all_parse() {
        let text = "(version 1)\n(rule a (constraint clearance (min 0.1mm)))\n(rule b (constraint hole_to_hole (min 0.254mm)))\n";
        let rules = parse_custom_rules(text);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].name, "a");
        assert_eq!(rules[1].name, "b");
        assert_eq!(rules[1].constraint_type, "hole_to_hole");
    }

    /// A whole rule commented out line-by-line with a leading `#` (a real
    /// pattern found in the KiCad QA corpus, `issue2512.kicad_dru`) must
    /// not be parsed as live -- the naive "wrap the whole file and parse"
    /// approach would otherwise see `#(rule ...` as the literal start of a
    /// real `(rule ...)` form.
    #[test]
    fn hash_commented_out_rule_is_not_parsed() {
        let text = "(version 1)\n#(rule mine\n#\t(condition \"A.NetName == 'GND'\")\n#\t(constraint clearance (min 2mm)))\n\n(rule real (constraint clearance (min 0.1mm)))\n";
        let rules = parse_custom_rules(text);
        assert_eq!(rules.len(), 1, "{rules:#?}");
        assert_eq!(rules[0].name, "real");
    }

    #[test]
    fn a_hash_inside_a_quoted_condition_is_not_a_comment() {
        let text = "(version 1)\n(rule r (constraint clearance (min 0.1mm)) (condition \"A.Reference == 'R#1'\"))\n";
        let rules = parse_custom_rules(text);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].condition.as_deref(), Some("A.Reference == 'R#1'"));
    }

    #[test]
    fn empty_or_garbage_text_is_an_empty_no_op() {
        assert!(parse_custom_rules("").is_empty());
        assert!(parse_custom_rules("not an s-expression at all })(").is_empty());
    }

    #[test]
    fn length_units() {
        assert_eq!(parse_length("0.2mm"), Some(200));
        assert_eq!(parse_length("1mm"), Some(1000));
        assert_eq!(parse_length("20"), Some(20_000), "bare number defaults to mm");
        assert_eq!(parse_length("1in"), Some(25_400));
        assert_eq!(parse_length("4mil"), Some(102));
        assert_eq!(parse_length("1furlong"), None, "unrecognized unit must not be guessed at");
    }
}
