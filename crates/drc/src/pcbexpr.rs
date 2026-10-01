//! A subset of KiCad's PCBEXPR condition language (`pcbnew/pcbexpr_evaluator.cpp`,
//! `libeval`), just enough to evaluate the `(condition "...")` text a real
//! `.kicad_dru` rule uses to decide whether it applies to one specific pair
//! of board items (task item 4). This is an interpreter over the token
//! stream directly (no separate AST), which is enough for the grammar
//! subset below; it is not a general-purpose expression language.
//!
//! Grammar subset (standard precedence, `||` loosest):
//! ```text
//! expr       := or
//! or         := and ( '||' and )*
//! and        := unary ( '&&' unary )*
//! unary      := '!' unary | comparison
//! comparison := primary ( ('==' | '!=') primary )?
//! primary    := '(' expr ')' | string | ident [ '(' arg ')' ]
//! arg        := string
//! ```
//! `ident` is one of `A`/`B` followed by a known property (`A.NetClass`,
//! `B.Type`, ...) or a known boolean-returning function call
//! (`A.hasNetclass('x')`); a bare `A`/`B` with no property, a numeric
//! literal, a unit suffix (`20mm`, `90 deg`), and every property/function
//! this module does not recognize are all **unsupported** -- scanning or
//! evaluating one makes the whole condition evaluate to `false` rather
//! than guessing, so a rule this subset cannot fully understand is simply
//! never applied instead of being mis-applied. Measured against the real
//! condition strings in KiCad's own QA-corpus `.kicad_dru` files, this
//! subset (`NetClass`/`NetName`/`Type`/`Reference` properties,
//! `hasNetclass()`, string equality with `*` wildcards, `&&`/`||`/`!`,
//! parens) covers the majority of real-world conditions; the rest (numeric
//! comparisons with units, `insideArea`/`insideCourtyard`/`existsOnLayer`/
//! `isPlated`/`isCoupledDiffPair`, and anything needing a concept this
//! workspace's model has no field for) safely never match rather than
//! being approximated.
//!
//! `Type` is compared case-insensitively: real QA-corpus files use both
//! `A.Type == 'Pad'` and `A.Type == 'pad'`, and getting this subset's cheap
//! safety margin (a rule that cannot be fully understood never applies)
//! at the cost of occasionally under-matching on case would defeat the
//! point of porting this at all. Every other string property
//! (`NetClass`/`NetName`/`Reference`) stays case-sensitive and exact
//! (modulo `*`, via [`eda_model::glob_match`]) -- those are real identifiers
//! on the board, not a closed enum KiCad itself is lenient about.

use eda_model::glob_match;

/// Everything about one side of a pair (`A` or `B` in a condition) this
/// subset can answer a property/function call about. Built per call site
/// from whichever `Drc*` struct is at hand -- see
/// `providers::copper_clearance`'s `facts_of_*` helpers.
#[derive(Debug, Clone, Copy, Default)]
pub struct Facts<'a> {
    /// KiCad's own item-type name: `"Pad"`, `"Via"`, `"Track"`, `"Zone"`.
    pub item_type: &'a str,
    /// Resolved net class name (`"Default"` when the net has no explicit
    /// class, matching KiCad's own convention that every net belongs to
    /// at least the implicit default class).
    pub net_class: &'a str,
    pub net_name: &'a str,
    /// Footprint reference designator; empty for a track/via/zone, which
    /// have none (an empty string never matches a real reference pattern,
    /// a safe default rather than a special case).
    pub reference: &'a str,
    /// Every footprint whose courtyard contains this item's own
    /// representative point (a pad/via's centre, a track's midpoint) --
    /// `A.insideCourtyard('NAME')`/`B.insideCourtyard('NAME')`'s only data
    /// source (task item 4; `insideArea`, a zone/rule-area containment test,
    /// stays unsupported -- this model has no rule-area concept at all).
    /// Precomputed once per board by `providers::copper_clearance::
    /// CourtyardMembership` (and `providers::track_width`'s own copy of the
    /// same) rather than recomputed per pair -- see that type's doc comment.
    pub inside_courtyards: &'a [String],
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    LParen,
    RParen,
    And,
    Or,
    Not,
    Eq,
    Ne,
    Str(String),
    Ident(String),
}

fn tokenize(s: &str) -> Option<Vec<Tok>> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        match b[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            b')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            b'!' if b.get(i + 1) == Some(&b'=') => {
                out.push(Tok::Ne);
                i += 2;
            }
            b'!' => {
                out.push(Tok::Not);
                i += 1;
            }
            b'=' if b.get(i + 1) == Some(&b'=') => {
                out.push(Tok::Eq);
                i += 2;
            }
            b'&' if b.get(i + 1) == Some(&b'&') => {
                out.push(Tok::And);
                i += 2;
            }
            b'|' if b.get(i + 1) == Some(&b'|') => {
                out.push(Tok::Or);
                i += 2;
            }
            b'\'' => {
                let start = i + 1;
                let mut j = start;
                while j < b.len() && b[j] != b'\'' {
                    j += 1;
                }
                if j >= b.len() {
                    return None;
                }
                out.push(Tok::Str(s[start..j].to_string()));
                i = j + 1;
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                    i += 1;
                }
                out.push(Tok::Ident(s[start..i].to_string()));
            }
            // A bare `,` (function-arg separator beyond one string arg),
            // a number, a unit suffix, or anything else outside this
            // subset's grammar: bail out of the whole condition rather
            // than mis-tokenize it.
            _ => return None,
        }
    }
    Some(out)
}

/// A value this subset's property/function resolution can produce.
/// `Unknown` is the universal "this subset does not understand that" value
/// -- comparing it to anything, or using it directly as a boolean, is
/// always `false` (see the module doc comment). `TypeStr` is `Str`'s
/// sibling for the one property compared case-insensitively (`A.Type`/
/// `B.Type` -- see the module doc comment); kept as its own variant so
/// [`values_match`] only folds case for an actual `Type` comparison, never
/// for a `NetClass`/`NetName`/`Reference` one that happens to share a
/// string literal's spelling up to case.
enum Value {
    Str(String),
    TypeStr(String),
    Bool(bool),
    Unknown,
}

impl Value {
    fn as_bool(&self) -> bool {
        matches!(self, Value::Bool(true))
    }
}

struct Parser<'a, 'f> {
    toks: &'a [Tok],
    pos: usize,
    a: &'f Facts<'f>,
    b: &'f Facts<'f>,
}

impl<'a, 'f> Parser<'a, 'f> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn bump(&mut self) -> Option<&Tok> {
        let t = self.toks.get(self.pos);
        self.pos += 1;
        t
    }

    fn or(&mut self) -> Option<bool> {
        let mut v = self.and()?;
        while self.peek() == Some(&Tok::Or) {
            self.bump();
            let rhs = self.and()?;
            v = v || rhs;
        }
        Some(v)
    }

    fn and(&mut self) -> Option<bool> {
        let mut v = self.unary()?;
        while self.peek() == Some(&Tok::And) {
            self.bump();
            let rhs = self.unary()?;
            v = v && rhs;
        }
        Some(v)
    }

    fn unary(&mut self) -> Option<bool> {
        if self.peek() == Some(&Tok::Not) {
            self.bump();
            return Some(!self.unary()?);
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Option<bool> {
        let lhs = self.primary()?;
        match self.peek() {
            Some(Tok::Eq) => {
                self.bump();
                let rhs = self.primary()?;
                Some(values_match(&lhs, &rhs))
            }
            Some(Tok::Ne) => {
                self.bump();
                let rhs = self.primary()?;
                Some(!values_match(&lhs, &rhs))
            }
            _ => Some(lhs.as_bool()),
        }
    }

    fn primary(&mut self) -> Option<Value> {
        match self.bump()?.clone() {
            Tok::LParen => {
                let v = self.or()?;
                if self.bump() != Some(&Tok::RParen) {
                    return None;
                }
                Some(Value::Bool(v))
            }
            Tok::Str(s) => Some(Value::Str(s)),
            Tok::Ident(id) => {
                // A function call: `A.hasNetclass('x')`.
                if self.peek() == Some(&Tok::LParen) {
                    self.bump();
                    let arg = match self.bump()? {
                        Tok::Str(s) => s.clone(),
                        _ => return None,
                    };
                    if self.bump() != Some(&Tok::RParen) {
                        return None;
                    }
                    return Some(self.call(&id, &arg));
                }
                Some(self.property(&id))
            }
            _ => None,
        }
    }

    fn side(&self, id: &str) -> Option<&Facts<'f>> {
        if id.starts_with("A.") {
            Some(self.a)
        } else if id.starts_with("B.") {
            Some(self.b)
        } else {
            None
        }
    }

    fn property(&self, id: &str) -> Value {
        let Some(facts) = self.side(id) else { return Value::Unknown };
        let prop = &id[2..];
        match prop {
            "Type" => Value::TypeStr(facts.item_type.to_string()),
            "NetClass" => Value::Str(facts.net_class.to_string()),
            "NetName" | "Net" => Value::Str(facts.net_name.to_string()),
            "Reference" => Value::Str(facts.reference.to_string()),
            _ => Value::Unknown,
        }
    }

    fn call(&self, id: &str, arg: &str) -> Value {
        let Some(facts) = self.side(id) else { return Value::Unknown };
        let func = &id[2..];
        match func {
            "hasNetclass" => Value::Bool(facts.net_class == arg),
            "insideCourtyard" => Value::Bool(facts.inside_courtyards.iter().any(|fp_id| glob_match(arg, fp_id))),
            _ => Value::Unknown,
        }
    }
}

/// `Type`'s own case-insensitive equality (either side being a `TypeStr`
/// is enough -- a literal compared against `A.Type` is still a `Type`
/// comparison); every other string property compares exact modulo `*`
/// wildcards via [`glob_match`] -- see the module doc comment. A plain
/// `glob_match` with no `*` in either argument degenerates to exact
/// equality, so this is also correct for a property-vs-property
/// comparison with no literal at all (`A.NetClass == B.NetClass`).
fn values_match(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::TypeStr(x), Value::TypeStr(y)) => x.eq_ignore_ascii_case(y),
        (Value::TypeStr(x), Value::Str(y)) | (Value::Str(y), Value::TypeStr(x)) => x.eq_ignore_ascii_case(y),
        (Value::Str(x), Value::Str(y)) => glob_match(x, y) || glob_match(y, x),
        _ => false,
    }
}

/// A condition's token stream, tokenized once and reused across every pair
/// it is evaluated against -- see [`compile`]'s doc comment for why this
/// exists as its own type instead of just re-tokenizing the source string
/// per call.
pub struct CompiledCondition(Vec<Tok>);

/// Tokenize `condition` once, for a caller that will evaluate the same
/// `.kicad_dru` rule against many item pairs (every clearance-family
/// provider in `crate::providers::copper_clearance`, in practice). A
/// board's custom rules are few, but the pairs they are checked against are
/// not (thousands, on a real routed board) -- re-tokenizing and
/// re-(hand-)parsing the same condition string from scratch on every one of
/// those pairs measurably slowed real boards (`issue11814` in the QA
/// corpus, which has both a `.kicad_dru` and a lot of copper) enough to
/// blow the parity harness's own per-board watchdog. `None` means the same
/// as it would for [`matches`]: this condition is outside the subset's
/// grammar and so never matches anything.
pub fn compile(condition: &str) -> Option<CompiledCondition> {
    tokenize(condition).map(CompiledCondition)
}

/// Evaluate `condition` (a `.kicad_dru` rule's raw PCBEXPR text) for one
/// specific `(a, b)` pair. `None` (unparseable, or outside this subset's
/// grammar) is treated as "does not match" by [`matches`] -- never as
/// "matches unconditionally".
fn eval(condition: &str, a: &Facts, b: &Facts) -> Option<bool> {
    let toks = tokenize(condition)?;
    eval_toks(&toks, a, b)
}

fn eval_toks(toks: &[Tok], a: &Facts, b: &Facts) -> Option<bool> {
    let mut p = Parser { toks, pos: 0, a, b };
    let v = p.or()?;
    if p.pos != p.toks.len() {
        return None; // trailing tokens: outside this subset's grammar
    }
    Some(v)
}

/// Whether a rule's `condition` matches `(a, b)` in **either** order.
/// KiCad evaluates a rule's condition against a canonically-ordered pair
/// (sorted by internal item UUID, which this port does not reproduce), so
/// a condition written asymmetrically (`"A.Type == 'Via' && B.Type ==
/// 'Track'"`) is meant to match a via/track pair regardless of which one
/// our own, arbitrary pairing happened to label `a`/`b`. Trying both
/// orders and matching on either is the conservative, order-independent
/// reading of "this rule is about an unordered pair of items like these" --
/// see this module's doc comment on the alternative (mis-ordering vs. a
/// real KiCad run) being strictly worse. `None`/unconditional
/// (`condition.is_none()`) always matches, same as a `.kicad_dru` rule
/// with no `(condition ...)` at all.
pub fn matches(condition: Option<&str>, a: &Facts, b: &Facts) -> bool {
    match condition {
        None => true,
        Some(c) => eval(c, a, b).unwrap_or(false) || eval(c, b, a).unwrap_or(false),
    }
}

/// Same as [`matches`], but against an already-[`compile`]d condition --
/// the hot-path version every real DRC call site uses (see `compile`'s doc
/// comment for why re-tokenizing per pair was a real cost, not a
/// micro-optimization).
pub fn matches_compiled(condition: Option<&CompiledCondition>, a: &Facts, b: &Facts) -> bool {
    match condition {
        None => true,
        Some(c) => eval_toks(&c.0, a, b).unwrap_or(false) || eval_toks(&c.0, b, a).unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad<'a>(net_class: &'a str, net_name: &'a str, reference: &'a str) -> Facts<'a> {
        Facts { item_type: "Pad", net_class, net_name, reference, inside_courtyards: &[] }
    }

    fn via<'a>(net_class: &'a str, net_name: &'a str) -> Facts<'a> {
        Facts { item_type: "Via", net_class, net_name, reference: "", inside_courtyards: &[] }
    }

    #[test]
    fn same_netclass() {
        let a = pad("power", "VIN", "C1");
        let b = pad("power", "VOUT", "C2");
        assert!(matches(Some("A.NetClass == B.NetClass"), &a, &b));
        let c = pad("signal", "SIG", "R1");
        assert!(!matches(Some("A.NetClass == B.NetClass"), &a, &c));
    }

    #[test]
    fn compiled_matches_agree_with_uncompiled() {
        let a = pad("power", "VIN", "C1");
        let b = pad("power", "VOUT", "C2");
        let cond = "A.NetClass == B.NetClass";
        let compiled = compile(cond).expect("valid condition compiles");
        assert_eq!(matches(Some(cond), &a, &b), matches_compiled(Some(&compiled), &a, &b));
        let c = pad("signal", "SIG", "R1");
        assert_eq!(matches(Some(cond), &a, &c), matches_compiled(Some(&compiled), &a, &c));
        assert!(!matches_compiled(Some(&compiled), &a, &c));
    }

    #[test]
    fn has_netclass_and_not() {
        let a = pad("BI", "N1", "J1");
        let b = pad("signal", "N2", "R1");
        let cond = Some("A.hasNetclass('BI') && !B.hasNetclass('BI')");
        assert!(matches(cond, &a, &b));
        // Order independence (see `matches`'s doc comment): this condition
        // was written assuming A is the BI-class item, but still matches
        // when our own call site happens to pass (signal, BI) instead.
        assert!(matches(cond, &b, &a));
        // Two items of the *same* class never match either way.
        let c = pad("BI", "N3", "J2");
        assert!(!matches(cond, &a, &c));
        assert!(!matches(cond, &c, &a));
    }

    /// GAPS.md #3's `issue11814` tail: a real `.kicad_dru` rule
    /// (`A.insideCourtyard('U4') || A.insideCourtyard('CW*')`) loosens
    /// clearance for anything inside a tight WSON/connector courtyard.
    /// Before `inside_courtyards` existed this function was unconditionally
    /// `Value::Unknown` (unsupported), so the rule could never match and
    /// every such pair fell back to the board's stricter default clearance.
    #[test]
    fn inside_courtyard_matches_by_name_or_glob() {
        let near_u4 = Facts { inside_courtyards: &["U4".to_string()], ..pad("Default", "GND", "") };
        let elsewhere = Facts { inside_courtyards: &[], ..pad("Default", "GND", "") };
        assert!(matches(Some("A.insideCourtyard('U4')"), &near_u4, &elsewhere));
        assert!(!matches(Some("A.insideCourtyard('U4')"), &elsewhere, &elsewhere));

        let near_cw2 = Facts { inside_courtyards: &["CW2".to_string()], ..pad("Default", "GND", "") };
        assert!(matches(Some("A.insideCourtyard('CW*')"), &near_cw2, &elsewhere), "the real rule's glob side ('CW*') must match a specific courtyard name");
        assert!(!matches(Some("A.insideCourtyard('CW*')"), &near_u4, &elsewhere), "U4 must not satisfy a pattern it doesn't match");
    }

    #[test]
    fn type_is_case_insensitive() {
        let a = pad("Default", "N1", "P1");
        let b = via("Default", "N2");
        assert!(matches(Some("A.Type == 'pad' && B.Type == 'via'"), &a, &b));
        assert!(matches(Some("A.Type =='Pad' && B.Type =='Via'"), &a, &b));
    }

    #[test]
    fn reference_wildcard() {
        let a = pad("Default", "N1", "TP3");
        let b = pad("Default", "N2", "TP9");
        assert!(matches(Some("A.Reference =='TP*' && B.Reference == 'TP*'"), &a, &b));
        let c = pad("Default", "N3", "R1");
        assert!(!matches(Some("A.Reference =='TP*' && B.Reference == 'TP*'"), &a, &c));
    }

    #[test]
    fn unconditional_rule_always_matches() {
        let a = pad("Default", "N1", "P1");
        let b = via("Default", "N2");
        assert!(matches(None, &a, &b));
    }

    #[test]
    fn unsupported_syntax_never_matches() {
        let a = pad("Default", "N1", "P1");
        let b = via("Default", "N2");
        // A numeric/unit comparison this subset does not parse.
        assert!(!matches(Some("A.Position_X < 20mm"), &a, &b));
        // A function this subset does not implement.
        assert!(!matches(Some("A.insideArea('Zone1')"), &a, &b));
    }

    #[test]
    fn order_independence_for_asymmetric_conditions() {
        let via_item = via("Default", "N1");
        let track = pad("Default", "N2", ""); // stand-in with item_type overridden below
        let track = Facts { item_type: "Track", ..track };
        // Written assuming A=via, B=track -- must still match when our own
        // call site happens to pass (track, via) instead.
        assert!(matches(Some("A.Type == 'Via' && B.Type == 'Track'"), &track, &via_item));
        assert!(matches(Some("A.Type == 'Via' && B.Type == 'Track'"), &via_item, &track));
    }
}
