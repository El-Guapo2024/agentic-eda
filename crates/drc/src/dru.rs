//! A `.kicad_dru` custom rules file: `DRC_RULES_PARSER` (`pcbnew/drc/drc_rule_parser.cpp`) over the s-expression lexer of `DSNLEXER` (`common/dsnlexer.cpp`) and
//! the keywords of `common/drc_rules.keywords`, at KiCad 8303b2ad. It turns the text into [`Rule`]s for the [`crate::constraints::RuleEngine`].
//!
//! Ported as it is, quirks included. With no reporter, which is how `DRC_ENGINE::loadRules` and so the Clearance and Constraints Resolution commands run the parser,
//! the first error ends the parse (`reportError` throws): a file with one mistake in it gives no explicit rules at all, only the implicit ones, and the report says
//! "could not compile custom design rules". The same goes for a condition that parses but names an item, a property or a function KiCad does not know
//! (`compileRules` throws on the first error any condition tells).
//!
//! The lexer: a line whose first non-blank character is `#` is a comment; `(` and `)` are tokens; `"..."` is a string with the escapes `\\`, `\"`, `\n`, `\r`, `\t`,
//! `\a`, `\b`, `\f`, `\v`, `\x..` and `\ooo` and may not span lines; anything else runs to the next blank or parenthesis and is a number, a keyword or a symbol.
//! Keywords are case sensitive.

use crate::constraints::{disallow, CType, Constraint, Rule, Severity, ZoneConnection};
use crate::pcbexpr::{self, Condition, Env, Unit};
use std::collections::BTreeSet;
use std::rc::Rc;

/// `DRC_RULE_FILE_VERSION`.
pub const DRC_RULE_FILE_VERSION: i64 = 20200610;

/// A `PARSE_ERROR`: what the rules file got wrong and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub offset: usize,
    /// For the error `compileRules` throws: the rule whose condition it is, and the condition.
    pub rule: String,
    pub source: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line > 0 {
            write!(f, "{} (line {}, offset {})", self.message, self.line, self.offset)
        } else if !self.rule.is_empty() {
            write!(f, "{} in rule '{}': {}", self.message, self.rule, self.source)
        } else {
            write!(f, "{}", self.message)
        }
    }
}

/// `drc_rules.keywords`.
const KEYWORDS: &[&str] = &[
    "annular_width", "assertion", "assign_component_class", "board_edge", "bridged_mask", "blind_via", "buried_via", "clearance", "condition", "connection_width", "constraint",
    "courtyard_clearance", "creepage", "diff_pair_gap", "diff_pair_uncoupled", "disallow", "edge_clearance", "error", "exclusion", "footprint", "graphic", "hole", "hole_clearance",
    "hole_size", "hole_to_hole", "ignore", "inner", "layer", "length", "max", "mechanical_clearance", "mechanical_hole_clearance", "micro_via", "min", "min_resolved_spokes", "none",
    "npth", "opt", "outer", "pad", "pth", "physical_clearance", "physical_hole_clearance", "rule", "severity", "silk_clearance", "skew", "solder_mask_expansion",
    "solder_paste_abs_margin", "solder_paste_rel_margin", "solder_mask_sliver", "solid", "text", "text_height", "text_thickness", "thermal_reliefs", "thermal_relief_gap",
    "thermal_spoke_width", "through_via", "track", "track_angle", "track_width", "track_segment_length", "version", "via", "via_count", "via_dangling", "via_diameter", "warning",
    "within_diff_pairs", "zone", "zone_connection",
];

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Left,
    Right,
    Number,
    Str,
    /// A keyword of `drc_rules.keywords`.
    Kw(&'static str),
    Symbol,
    Eof,
}

struct Lexer {
    lines: Vec<Vec<char>>,
    li: usize,
    col: usize,
    fresh: bool,
    tok: Tok,
    text: String,
    /// `CurSeparator()`: the blanks before the token.
    sep: String,
    line_no: usize,
    offset: usize,
}

fn is_space(c: char) -> bool {
    (c as u32) <= (' ' as u32)
}

fn is_sep(c: char) -> bool {
    is_space(c) || c == '(' || c == ')'
}

/// `isNumber` of the lexer: `[-+]?[0-9]*\.?[0-9]+([eE][-+]?[0-9]+)?` the whole token.
fn is_number(t: &str) -> bool {
    let c: Vec<char> = t.chars().collect();
    let (mut i, mut saw) = (0, false);
    if i < c.len() && (c[i] == '-' || c[i] == '+') {
        i += 1;
    }
    while i < c.len() && c[i].is_ascii_digit() {
        i += 1;
        saw = true;
    }
    if i < c.len() && c[i] == '.' {
        i += 1;
        while i < c.len() && c[i].is_ascii_digit() {
            i += 1;
            saw = true;
        }
    }
    if saw && i < c.len() && (c[i] == 'e' || c[i] == 'E') {
        i += 1;
        saw = false;
        if i < c.len() && (c[i] == '-' || c[i] == '+') {
            i += 1;
        }
        while i < c.len() && c[i].is_ascii_digit() {
            i += 1;
            saw = true;
        }
    }
    saw && i == c.len()
}

impl Lexer {
    fn new(text: &str) -> Lexer {
        // `STRING_LINE_READER`: lines keep their end of line.
        let mut lines: Vec<Vec<char>> = Vec::new();
        let mut cur: Vec<char> = Vec::new();
        for ch in text.chars() {
            cur.push(ch);
            if ch == '\n' {
                lines.push(std::mem::take(&mut cur));
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        Lexer { lines, li: 0, col: 0, fresh: true, tok: Tok::Left, text: String::new(), sep: String::new(), line_no: 0, offset: 0 }
    }

    fn err(&self, message: &str, offset_extra: usize) -> ParseError {
        ParseError { message: message.to_string(), line: self.line_no.max(1), offset: self.offset + offset_extra, rule: String::new(), source: String::new() }
    }

    /// `DSNLEXER::NextTok` in `!specctraMode`.
    fn next(&mut self) -> Result<Tok, ParseError> {
        self.sep.clear();
        if self.tok == Tok::Eof {
            return Ok(Tok::Eof);
        }
        loop {
            if self.li >= self.lines.len() {
                self.tok = Tok::Eof;
                self.text.clear();
                return Ok(Tok::Eof);
            }
            let line = &self.lines[self.li];
            // skip leading blanks
            while self.col < line.len() && is_space(line[self.col]) {
                self.sep.push(line[self.col]);
                self.col += 1;
            }
            if self.col >= line.len() {
                self.li += 1;
                self.col = 0;
                self.fresh = true;
                continue;
            }
            // If the first non-blank character is #, this line is a comment; comments cannot follow any other token on the same line.
            if self.fresh && line[self.col] == '#' {
                self.li += 1;
                self.col = 0;
                continue;
            }
            self.fresh = false;
            break;
        }
        let line = &self.lines[self.li];
        let start = self.col;
        self.line_no = self.li + 1;
        self.offset = start;
        let c = line[start];
        if c == '(' {
            self.col += 1;
            self.text = "(".to_string();
            self.tok = Tok::Left;
            return Ok(Tok::Left);
        }
        if c == ')' {
            self.col += 1;
            self.text = ")".to_string();
            self.tok = Tok::Right;
            return Ok(Tok::Right);
        }
        if c == '"' {
            let mut out = String::new();
            let mut i = start + 1;
            loop {
                if i >= line.len() {
                    let e = ParseError { message: "Unterminated delimited string".to_string(), line: self.line_no, offset: start + out.chars().count() + 1, rule: String::new(), source: String::new() };
                    return Err(e);
                }
                match line[i] {
                    '\\' => {
                        i += 1;
                        if i >= line.len() {
                            continue;
                        }
                        let e = line[i];
                        i += 1;
                        match e {
                            '"' | '\\' => out.push(e),
                            'a' => out.push('\x07'),
                            'b' => out.push('\x08'),
                            'f' => out.push('\x0c'),
                            'n' => out.push('\n'),
                            'r' => out.push('\r'),
                            't' => out.push('\t'),
                            'v' => out.push('\x0b'),
                            'x' => {
                                let mut hex = String::new();
                                while hex.len() < 2 && i < line.len() && line[i].is_ascii_hexdigit() {
                                    hex.push(line[i]);
                                    i += 1;
                                }
                                if hex.is_empty() {
                                    out.push('x');
                                } else {
                                    out.push(u8::from_str_radix(&hex, 16).unwrap_or(0) as char);
                                }
                            }
                            _ => {
                                i -= 1;
                                let mut oct = String::new();
                                while oct.len() < 3 && i < line.len() && ('0'..='7').contains(&line[i]) {
                                    oct.push(line[i]);
                                    i += 1;
                                }
                                if oct.is_empty() {
                                    out.push('\\');
                                } else {
                                    out.push(u8::from_str_radix(&oct, 8).unwrap_or(0) as char);
                                }
                            }
                        }
                    }
                    '"' => {
                        i += 1;
                        break;
                    }
                    ch => {
                        out.push(ch);
                        i += 1;
                    }
                }
            }
            self.col = i;
            self.text = out;
            self.tok = Tok::Str;
            return Ok(Tok::Str);
        }
        let mut end = start;
        while end < line.len() && !is_sep(line[end]) {
            end += 1;
        }
        let word: String = line[start..end].iter().collect();
        self.col = end;
        self.tok = if is_number(&word) {
            Tok::Number
        } else if let Some(k) = KEYWORDS.iter().find(|k| **k == word) {
            Tok::Kw(k)
        } else {
            Tok::Symbol
        };
        self.text = word;
        Ok(self.tok.clone())
    }
}

/// `IsSymbol`: a symbol, a string or a keyword.
fn is_symbol(t: &Tok) -> bool {
    matches!(t, Tok::Symbol | Tok::Str | Tok::Kw(_))
}

struct Parser {
    lx: Lexer,
    env: Rc<Env>,
}

/// What a rules file holds.
pub struct Parsed {
    pub rules: Vec<Rule>,
    pub version: i64,
    /// The file asks for a newer version than this one knows (`m_tooRecent`).
    pub too_recent: bool,
}

/// `DRC_RULES_PARSER::Parse` with no reporter: the rules of `text`, or the first error.
pub fn parse(text: &str, env: &Rc<Env>) -> Result<Parsed, ParseError> {
    let mut p = Parser { lx: Lexer::new(text), env: env.clone() };
    p.parse()
}

impl Parser {
    fn next(&mut self) -> Result<Tok, ParseError> {
        self.lx.next()
    }

    fn tok(&self) -> &Tok {
        &self.lx.tok
    }

    /// `reportError`: with no reporter it throws. The message is cut at the `|` and the halves joined, as `"ERROR: %s%s"` does.
    fn error<T>(&self, message: &str) -> Result<T, ParseError> {
        self.error_at(message, 0)
    }

    fn error_at<T>(&self, message: &str, offset: usize) -> Result<T, ParseError> {
        let (first, rest) = message.split_once('|').unwrap_or((message, ""));
        Err(self.lx.err(&format!("ERROR: {first}{rest}"), offset))
    }

    /// `checkUnresolvedTextVariable`.
    fn check_unresolved(&self) -> Result<bool, ParseError> {
        if let Some(pos) = self.lx.text.find("${") {
            return self.error_at("Unresolved text variable", self.lx.text[..pos].chars().count());
        }
        Ok(false)
    }

    /// `expected`.
    fn expected<T>(&self, expected: &str) -> Result<T, ParseError> {
        let msg = if self.lx.text.starts_with("${") { "Unresolved text variable.".to_string() } else { format!("Unrecognized item '{}'.| Expected {expected}.", self.lx.text) };
        self.error(&msg)
    }

    /// `parseExpression`: the tokens up to the closing parenthesis, joined with the blanks that were between them.
    fn parse_expression(&mut self) -> Result<String, ParseError> {
        let mut expr = String::new();
        let mut depth = 1;
        loop {
            let t = self.next()?;
            if t == Tok::Eof {
                break;
            }
            if t == Tok::Left {
                depth += 1;
            }
            if t == Tok::Right {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            if !expr.is_empty() {
                expr.push_str(&self.lx.sep);
            }
            self.check_unresolved()?;
            expr.push_str(&self.lx.text);
        }
        Ok(expr)
    }

    fn parse(&mut self) -> Result<Parsed, ParseError> {
        let mut have_version = false;
        let mut out = Parsed { rules: Vec::new(), version: 0, too_recent: false };
        self.next()?;
        while *self.tok() != Tok::Eof {
            if self.check_unresolved()? {
                self.next()?;
                continue;
            }
            if *self.tok() != Tok::Left {
                self.error::<()>("Missing '('.")?;
            }
            self.next()?;
            if !have_version && *self.tok() != Tok::Kw("version") {
                self.error::<()>("Missing version statement.")?;
            }
            match self.tok().clone() {
                Tok::Kw("version") => {
                    have_version = true;
                    let t = self.next()?;
                    if t == Tok::Right {
                        self.error::<()>("Missing version number.")?;
                    } else if t == Tok::Number {
                        out.version = self.lx.text.split(|c: char| !(c.is_ascii_digit() || c == '-' || c == '+')).next().and_then(|s| s.parse().ok()).unwrap_or(0);
                        out.too_recent = out.version > DRC_RULE_FILE_VERSION;
                        if self.next()? != Tok::Right {
                            self.error::<()>("Missing ')'.")?;
                        }
                    } else {
                        self.expected::<()>("version number")?;
                    }
                }
                Tok::Kw("rule") => {
                    let r = self.parse_rule()?;
                    out.rules.push(r);
                }
                Tok::Eof => self.error::<()>("Incomplete statement.")?,
                _ => self.expected::<()>("rule or version")?,
            }
            self.next()?;
        }
        // `compileRules`: a condition that tells an error is a parse error, after the whole file is read.
        for r in &mut out.rules {
            if let Some(c) = &mut r.condition {
                if !c.expression().is_empty() {
                    c.compile(&self.env);
                    if !c.compile_errors().is_empty() {
                        return Err(ParseError { message: "Parse error".to_string(), line: 0, offset: 0, rule: r.name.clone(), source: c.expression().to_string() });
                    }
                }
            }
        }
        Ok(out)
    }

    fn parse_rule(&mut self) -> Result<Rule, ParseError> {
        let mut rule = Rule::new("");
        let t = self.next()?;
        if !is_symbol(&t) {
            self.error::<()>("Missing rule name.")?;
        }
        self.check_unresolved()?;
        rule.name = self.lx.text.clone();
        let mut layer_set = false;
        loop {
            let t = self.next()?;
            if t == Tok::Right || t == Tok::Eof {
                break;
            }
            if self.check_unresolved()? {
                continue;
            }
            if t != Tok::Left {
                self.error::<()>("Missing '('.")?;
            }
            match self.next()? {
                Tok::Kw("constraint") => self.parse_constraint(&mut rule)?,
                Tok::Kw("condition") => {
                    let t = self.next()?;
                    if t == Tok::Right {
                        self.error::<()>("Missing condition expression.")?;
                    } else if is_symbol(&t) {
                        self.check_unresolved()?;
                        let mut cond = Condition::new(&self.lx.text);
                        if !cond.compile(&self.env) {
                            let m = format!("Could not parse expression '{}'.", self.lx.text);
                            self.error::<()>(&m)?;
                        }
                        rule.condition = Some(cond);
                        if self.next()? != Tok::Right {
                            self.error::<()>("Missing ')'.")?;
                        }
                    } else {
                        self.expected::<()>("quoted expression")?;
                    }
                }
                Tok::Kw("layer") => {
                    if layer_set {
                        self.error::<()>("'layer' keyword already present.")?;
                    }
                    let (set, source) = self.parse_layer()?;
                    rule.layers = Some(set);
                    rule.layer_source = source;
                    layer_set = true;
                }
                Tok::Kw("severity") => rule.severity = self.parse_severity()?,
                Tok::Eof => {
                    self.error::<()>("Incomplete statement.")?;
                }
                _ => self.expected::<()>("constraint, condition, or disallow")?,
            }
        }
        if *self.tok() != Tok::Right {
            self.error::<()>("Missing ')'.")?;
        }
        Ok(rule)
    }

    /// `GetCurStrAsToken`: the keyword a quoted string spells, else the string stays a symbol.
    fn current_as_keyword(&self) -> Tok {
        match KEYWORDS.iter().find(|k| **k == self.lx.text) {
            Some(k) => Tok::Kw(k),
            None => Tok::Symbol,
        }
    }

    fn parse_constraint(&mut self, rule: &mut Rule) -> Result<(), ParseError> {
        const TYPES: &str = "assertion, clearance, hole_clearance, edge_clearance, physical_clearance, physical_hole_clearance, courtyard_clearance, silk_clearance, hole_size, hole_to_hole, track_width, track_angle, track_segment_length, annular_width, disallow, zone_connection, thermal_relief_gap, thermal_spoke_width, min_resolved_spokes, solder_mask_expansion, solder_mask_sliver, solder_paste_abs_margin, solder_paste_rel_margin, length, skew, via_count, via_dangling, via_diameter, diff_pair_gap, diff_pair_uncoupled or bridged_mask";
        let mut t = self.next()?;
        if self.check_unresolved()? {
            return Ok(());
        }
        // The deprecated names.
        t = match t {
            Tok::Kw("mechanical_clearance") => Tok::Kw("physical_clearance"),
            Tok::Kw("mechanical_hole_clearance") => Tok::Kw("physical_hole_clearance"),
            Tok::Kw("hole") => Tok::Kw("hole_size"),
            other => other,
        };
        if t == Tok::Right || t == Tok::Eof {
            return self.error("Missing constraint type.|  Expected assertion, clearance, hole_clearance, edge_clearance, physical_clearance, physical_hole_clearance, courtyard_clearance, silk_clearance, hole_size, hole_to_hole, track_width, track_angle, track_segment_length, annular_width, disallow, zone_connection, thermal_relief_gap, thermal_spoke_width, min_resolved_spokes, solder_mask_expansion, solder_paste_abs_margin, solder_paste_rel_margin, length, skew, via_count, via_dangling, via_diameter, diff_pair_gap or diff_pair_uncoupled.");
        }
        let ctype = match &t {
            Tok::Kw(k) => match *k {
                "diff_pair_uncoupled" => Some(CType::MaxUncoupled),
                other => CType::from_token(other).filter(|c| !matches!(c, CType::Null | CType::MaxUncoupled)),
            },
            _ => None,
        };
        let Some(ctype) = ctype else {
            return self.expected(TYPES);
        };
        if rule.find_constraint(ctype).is_some() {
            let m = format!("Rule already has a '{}' constraint.", self.lx.text);
            self.error::<()>(&m)?;
        }
        let mut c = Constraint::new(ctype);
        let unitless = matches!(ctype, CType::ViaCount | CType::MinResolvedSpokes | CType::TrackAngle | CType::ViaDangling | CType::BridgedMask);
        let allows_time = matches!(ctype, CType::Length | CType::Skew);

        if ctype == CType::Disallow {
            loop {
                let mut t = self.next()?;
                if t == Tok::Right {
                    break;
                }
                if t == Tok::Str {
                    t = self.current_as_keyword();
                }
                match t {
                    Tok::Kw("track") => c.disallow |= disallow::TRACKS,
                    Tok::Kw("via") => c.disallow |= disallow::THROUGH_VIAS | disallow::BLIND_VIAS | disallow::BURIED_VIAS | disallow::MICRO_VIAS,
                    Tok::Kw("through_via") => c.disallow |= disallow::THROUGH_VIAS,
                    Tok::Kw("blind_via") => c.disallow |= disallow::BLIND_VIAS,
                    Tok::Kw("buried_via") => c.disallow |= disallow::BURIED_VIAS,
                    Tok::Kw("micro_via") => c.disallow |= disallow::MICRO_VIAS,
                    Tok::Kw("pad") => c.disallow |= disallow::PADS,
                    Tok::Kw("zone") => c.disallow |= disallow::ZONES,
                    Tok::Kw("text") => c.disallow |= disallow::TEXTS,
                    Tok::Kw("graphic") => c.disallow |= disallow::GRAPHICS,
                    Tok::Kw("hole") => c.disallow |= disallow::HOLES,
                    Tok::Kw("footprint") => c.disallow |= disallow::FOOTPRINTS,
                    Tok::Eof => return self.error("Missing ')'."),
                    _ => return self.expected("track, via, through_via, blind_via, micro_via, buried_via, pad, zone, text, graphic, hole, or footprint."),
                }
            }
            rule.constraints.push(c);
            return Ok(());
        }
        if ctype == CType::ZoneConnection {
            let mut t = self.next()?;
            if t == Tok::Str {
                t = self.current_as_keyword();
            }
            c.zone_connection = match t {
                Tok::Kw("solid") => ZoneConnection::Full,
                Tok::Kw("thermal_reliefs") => ZoneConnection::Thermal,
                Tok::Kw("none") => ZoneConnection::None,
                Tok::Eof => return self.error("Missing ')'."),
                _ => return self.expected("solid, thermal_reliefs or none."),
            };
            if self.next()? != Tok::Right {
                self.error::<()>("Missing ')'.")?;
            }
            rule.constraints.push(c);
            return Ok(());
        }
        if ctype == CType::MinResolvedSpokes {
            // No min/max/opt here: it would suggest an optimal number of spokes could be asked for.
            let t = self.next()?;
            if t == Tok::Number {
                c.value.min = Some(self.lx.text.split(|ch: char| !(ch.is_ascii_digit() || ch == '-' || ch == '+')).next().and_then(|s| s.parse().ok()).unwrap_or(0));
                if self.next()? != Tok::Right {
                    self.error::<()>("Missing ')'.")?;
                }
            } else {
                self.expected::<()>("number")?;
            }
            rule.constraints.push(c);
            return Ok(());
        }
        if ctype == CType::Assertion {
            let t = self.next()?;
            if t == Tok::Right {
                self.error::<()>("Missing assertion expression.")?;
            }
            if is_symbol(&t) {
                // (The assertion is compiled for its code only: whatever it told is not an error of the file.)
                let mut cond = Condition::new(&self.lx.text);
                cond.compile(&self.env);
                c.test = Some(cond);
                if self.next()? != Tok::Right {
                    self.error::<()>("Missing ')'.")?;
                }
            } else {
                self.expected::<()>("quoted expression")?;
            }
            rule.constraints.push(c);
            return Ok(());
        }

        // (min ..) (opt ..) (max ..) (within_diff_pairs)
        #[derive(PartialEq, Clone, Copy)]
        enum UnitsType {
            Unitless,
            Distance,
            Time,
        }
        let mut units_type = UnitsType::Unitless;
        loop {
            let t = self.next()?;
            if t == Tok::Right || t == Tok::Eof {
                if t == Tok::Eof {
                    self.error::<()>("Incomplete statement.")?;
                }
                break;
            }
            if t != Tok::Left {
                self.error::<()>("Missing '('.")?;
            }
            match self.next()? {
                Tok::Kw("within_diff_pairs") => {
                    if ctype == CType::Skew {
                        c.options |= OPT_SKEW_WITHIN_DIFF_PAIRS;
                    } else {
                        self.error::<()>("within_diff_pairs option invalid for constraint type.")?;
                    }
                    if self.next()? != Tok::Right {
                        self.error::<()>("Missing ')'.")?;
                    }
                }
                Tok::Kw(which @ ("min" | "max" | "opt")) => {
                    let offset = self.lx.offset + which.chars().count();
                    let expr = self.parse_expression()?;
                    if expr.is_empty() {
                        self.error::<()>(&format!("Missing {which} value."))?;
                        continue;
                    }
                    let (value, units) = self.parse_value_with_units(offset, &expr, unitless)?;
                    let ut = match units {
                        Unit::Mm | Unit::Mil | Unit::In => UnitsType::Distance,
                        Unit::Fs | Unit::Ps => UnitsType::Time,
                        Unit::Deg | Unit::Unscaled => UnitsType::Unitless,
                    };
                    if !allows_time && ut == UnitsType::Time {
                        self.error::<()>("Time based units not allowed for constraint type.")?;
                    }
                    if (c.value.min.is_some() || c.value.max.is_some() || c.value.opt.is_some()) && units_type != ut {
                        self.error::<()>("Mixed units for constraint values.")?;
                    }
                    units_type = ut;
                    match which {
                        "min" => c.value.min = Some(value),
                        "max" => c.value.max = Some(value),
                        _ => c.value.opt = Some(value),
                    }
                    if allows_time {
                        if ut == UnitsType::Time {
                            c.options |= OPT_TIME_DOMAIN;
                            c.options &= !OPT_SPACE_DOMAIN;
                        } else {
                            c.options |= OPT_SPACE_DOMAIN;
                            c.options &= !OPT_TIME_DOMAIN;
                        }
                    }
                }
                Tok::Eof => return self.error("Incomplete statement."),
                _ => self.expected::<()>("min, max, opt, or within_diff_pairs")?,
            }
        }
        rule.constraints.push(c);
        Ok(())
    }

    /// `parseValueWithUnits`: the expression of a `(min ..)` evaluated to internal units, with the units it ended up in. Any error the compiler tells throws.
    fn parse_value_with_units(&self, offset: usize, expr: &str, unitless: bool) -> Result<(i64, Unit), ParseError> {
        match pcbexpr::evaluate_number(expr, unitless) {
            Ok(v) => Ok(v),
            Err(e) => {
                let (first, rest) = e.message.split_once('|').unwrap_or((&e.message, ""));
                Err(ParseError { message: format!("ERROR: {first}{rest}"), line: self.lx.line_no.max(1), offset: self.lx.offset + offset, rule: String::new(), source: String::new() })
            }
        }
    }

    /// `parseLayer`: `outer`, `inner` or a layer name (a pattern over the names of the layers).
    fn parse_layer(&mut self) -> Result<(BTreeSet<String>, String), ParseError> {
        let t = self.next()?;
        let (set, source);
        if t == Tok::Right {
            return self.error("Missing layer name or type.");
        } else if t == Tok::Kw("outer") {
            source = "outer".to_string();
            set = ["F.Cu", "B.Cu"].iter().map(|s| s.to_string()).collect();
        } else if t == Tok::Kw("inner") {
            source = "inner".to_string();
            set = (1..=30).map(|n| format!("In{n}.Cu")).collect();
        } else {
            let name = self.lx.text.clone();
            let mask = self.env.layer_mask(&name);
            let mut s: BTreeSet<String> = mask.iter().cloned().collect();
            let src = if s.is_empty() { String::new() } else { name.clone() };
            if s.is_empty() {
                if !self.check_unresolved()? {
                    let m = format!("Unrecognized layer '{name}'.");
                    self.error::<()>(&m)?;
                }
                s.insert("Rescue".to_string());
            }
            set = s;
            source = src;
        }
        if self.next()? != Tok::Right {
            self.error::<()>("Missing ')'.")?;
        }
        Ok((set, source))
    }

    fn parse_severity(&mut self) -> Result<Severity, ParseError> {
        let t = self.next()?;
        if t == Tok::Right || t == Tok::Eof {
            return self.error("Missing severity name.");
        }
        let sev = match t {
            Tok::Kw("ignore") => Severity::Ignore,
            Tok::Kw("warning") => Severity::Warning,
            Tok::Kw("error") => Severity::Error,
            Tok::Kw("exclusion") => Severity::Exclusion,
            _ => return self.expected("ignore, warning, error, or exclusion"),
        };
        if self.next()? != Tok::Right {
            self.error::<()>("Missing ')'.")?;
        }
        Ok(sev)
    }
}

/// `DRC_CONSTRAINT::OPTIONS`.
pub const OPT_SKEW_WITHIN_DIFF_PAIRS: u8 = 1;
pub const OPT_SPACE_DOMAIN: u8 = 2;
pub const OPT_TIME_DOMAIN: u8 = 4;

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Rc<Env> {
        let mut layers: Vec<String> = vec!["F.Cu".into(), "B.Cu".into()];
        layers.extend((1..=30).map(|n| format!("In{n}.Cu")));
        layers.extend(["F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Edge.Cuts", "F.CrtYd", "B.CrtYd"].map(String::from));
        let refs: Vec<&str> = layers.iter().map(String::as_str).collect();
        let mut e = Env::simple(&refs);
        e.layer_entries.push(("F.Silkscreen".into(), "F.SilkS".into()));
        Rc::new(e)
    }

    fn rules(text: &str) -> Vec<Rule> {
        parse(text, &env()).unwrap_or_else(|e| panic!("{e}\n{text}")).rules
    }

    fn err(text: &str) -> String {
        parse(text, &env()).err().unwrap_or_else(|| panic!("no error for {text}")).message
    }

    #[test]
    fn a_rule_with_a_condition_a_layer_and_a_constraint() {
        let r = rules("(version 1)\n# a comment\n(rule \"HV\"\n\t(condition \"A.NetClass == 'HV'\")\n\t(layer outer)\n\t(constraint clearance (min 0.5mm) (opt 0.6mm))\n)\n");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "HV");
        assert_eq!(r[0].layer_source, "outer");
        assert_eq!(r[0].layers.as_ref().unwrap().len(), 2);
        assert_eq!(r[0].condition.as_ref().unwrap().expression(), "A.NetClass == 'HV'");
        let c = &r[0].constraints[0];
        assert_eq!((c.ctype, c.value.min, c.value.opt, c.value.max), (CType::Clearance, Some(500_000), Some(600_000), None));
    }

    #[test]
    fn values_are_expressions_in_internal_units() {
        let r = rules("(version 1)(rule x (constraint track_width (min 4mil) (max 0.2mm + 0.1mm)))");
        let c = &r[0].constraints[0];
        assert_eq!((c.value.min, c.value.max), (Some(101_600), Some(300_000)));
        // a count has no units; a time does
        let r = rules("(version 1)(rule x (constraint via_count (max 3)))(rule y (constraint skew (max 100ps)))");
        assert_eq!(r[0].constraints[0].value.max, Some(3));
        assert_eq!(r[1].constraints[0].value.max, Some(100_000_000));
        assert_ne!(r[1].constraints[0].options & OPT_TIME_DOMAIN, 0);
    }

    #[test]
    fn disallow_zone_connection_assertion_and_spokes() {
        let r = rules("(version 1)(rule k (constraint disallow track via \"pad\"))(rule z (constraint zone_connection thermal_reliefs))(rule a (constraint assertion \"A.Width > 0.1mm\"))(rule s (constraint min_resolved_spokes 3))");
        assert_eq!(r[0].constraints[0].disallow, disallow::TRACKS | disallow::THROUGH_VIAS | disallow::BLIND_VIAS | disallow::BURIED_VIAS | disallow::MICRO_VIAS | disallow::PADS);
        assert_eq!(r[1].constraints[0].zone_connection, ZoneConnection::Thermal);
        assert_eq!(r[2].constraints[0].test.as_ref().unwrap().expression(), "A.Width > 0.1mm");
        assert_eq!(r[3].constraints[0].value.min, Some(3));
    }

    #[test]
    fn the_first_error_is_the_error() {
        assert_eq!(err("(rule x)"), "ERROR: Missing version statement.");
        assert_eq!(err("(version 1)(rule x (constraint clearance (min 1)))"), "ERROR: Missing units for '1' (must be mm, in, mil, deg, fs, or ps)");
        assert_eq!(err("(version 1)(rule x (constraint nonsense))"), format!("ERROR: Unrecognized item 'nonsense'. Expected assertion, clearance, hole_clearance, edge_clearance, physical_clearance, physical_hole_clearance, courtyard_clearance, silk_clearance, hole_size, hole_to_hole, track_width, track_angle, track_segment_length, annular_width, disallow, zone_connection, thermal_relief_gap, thermal_spoke_width, min_resolved_spokes, solder_mask_expansion, solder_mask_sliver, solder_paste_abs_margin, solder_paste_rel_margin, length, skew, via_count, via_dangling, via_diameter, diff_pair_gap, diff_pair_uncoupled or bridged_mask."));
        assert_eq!(err("(version 1)(rule x (constraint clearance (min 1mm)) (constraint clearance (min 2mm)))"), "ERROR: Rule already has a 'clearance' constraint.");
        assert_eq!(err("(version 1)(rule x (constraint clearance (min 1mm) (max 5)))"), "ERROR: Missing units for '5' (must be mm, in, mil, deg, fs, or ps)");
        assert_eq!(err("(version 1)(rule x (layer nowhere))"), "ERROR: Unrecognized layer 'nowhere'.");
        assert_eq!(err("(version 1)(rule x (condition \"A.Width >\"))"), "ERROR: Could not parse expression 'A.Width >'.");
        assert_eq!(err("(version 1)(rule x (severity loud))"), "ERROR: Unrecognized item 'loud'. Expected ignore, warning, error, or exclusion.");
        assert_eq!(err("(version 1)(rule x (constraint skew (max 1mm) (max 5ps)))"), "ERROR: Mixed units for constraint values.");
        assert_eq!(err("(version 1)(rule x (constraint clearance (min 1ps)))"), "ERROR: Time based units not allowed for constraint type.");
        assert_eq!(err("(version 1)(rule x (constraint clearance (min 1mm))"), "ERROR: Missing ')'.");
        assert_eq!(err("(version 1)(rule \"x"), "Unterminated delimited string");
    }

    #[test]
    fn a_condition_the_compiler_tells_an_error_on_is_refused_after_the_whole_file_is_read() {
        let e = parse("(version 1)(rule x (condition \"A.Nope == 'y'\") (constraint clearance (min 1mm)))", &env()).err().unwrap();
        assert_eq!((e.message.as_str(), e.rule.as_str()), ("Parse error", "x"));
    }

    #[test]
    fn the_rules_of_kicads_own_test_data_parse_or_say_why_not() {
        let dir = std::path::Path::new("/Users/juanantonioluera/ws/kicad-src-8303b2ad/qa/data/pcbnew");
        if !dir.exists() {
            return;
        }
        let mut files = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "kicad_dru") {
                    files.push(p);
                }
            }
        }
        files.sort();
        assert!(files.len() >= 30, "{}", files.len());
        let mut bad = Vec::new();
        for f in &files {
            let text = std::fs::read_to_string(f).unwrap();
            if let Err(e) = parse(&text, &env()) {
                bad.push(format!("{}: {e}", f.file_name().unwrap().to_string_lossy()));
            }
        }
        eprintln!("{} files, {} refused:\n{}", files.len(), bad.len(), bad.join("\n"));
    }
}
