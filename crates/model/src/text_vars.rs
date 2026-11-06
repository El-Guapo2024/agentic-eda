//! Text variables: `${NAME}` in a text, a field or the title block, and what it stands for.
//!
//! The project's variables are the Text Variables page of Board Setup (`panel_text_variables.cpp`, [`crate::rules::RulesOverlay::text_variables`]); KiCad's own are
//! answered by the item the text is on, the board or sheet it is in, the title block and the project. This is the engine KiCad runs for every shown text and the
//! resolver of each kind of item, ported so the studio shows what kicad-cli plots: the studio's state carries each text's `shown` form beside its raw one.
//!
//! Ported from:
//!   common/common.cpp            `ExpandTextVars` (the `${..}` / `\${..}` / nested scanner), `ResolveTextVars` (the passes, to `m_ResolveTextRecursionDepth`)
//!   common/string_utils.cpp      `UnescapeString` (`{slash}`, `{dblquote}`, ... in a stored text)
//!   common/project.cpp           `PROJECT::TextVarResolver` (PROJECTNAME, CURRENT_DATE, ..., then the project's variables)
//!   common/title_block.cpp       `TITLE_BLOCK::TextVarResolver` (TITLE, ISSUE_DATE, REVISION, COMPANY, COMMENT1 .. 9)
//!   pcbnew/board.cpp             `BOARD::ResolveTextVar` (`REF:field`, FILENAME, FILEPATH, PROJECTNAME, the title block, the project)
//!   pcbnew/footprint.cpp         `FOOTPRINT::ResolveTextVar` (REFERENCE, VALUE, LAYER, FOOTPRINT_LIBRARY, FOOTPRINT_NAME, NET_NAME(n) ..., a field by name)
//!   pcbnew/pcb_text.cpp          `PCB_TEXT::GetShownText` (LAYER first, then the footprint it is in, then the board); `pcb_field.cpp` the same for a field
//!   eeschema/schematic.cpp       `SCHEMATIC::ResolveTextVar` (`#`, `##`, SHEETPATH, SHEETNAME, FILENAME, FILEPATH, PROJECTNAME, the title block, the project)
//!   eeschema/sch_sheet.cpp       `SCH_SHEET::ResolveTextVar` (the sheet's fields, its screen's title block, then its parent sheet up to the schematic)
//!   eeschema/sch_symbol.cpp      `SCH_SYMBOL::ResolveTextVar` (a field by name, FOOTPRINT_*, UNIT, SYMBOL_*, DNP, EXCLUDE_FROM_*, NET_NAME(n) ..., then the sheet)
//!   eeschema/sch_field.cpp, sch_text.cpp, sch_item.cpp   `GetShownText`, `ResolveText`: which resolvers an item's text goes through
//!
//! The `@{..}` math expressions (`common/text_eval`) are evaluated for their arithmetic after each pass, as 25ec2335's `ResolveTextVars` does
//! ([`evaluate_math`]), with the grammar's strings, units, the functions that do not read the clock and the version control's (`vcs..`), and `VCSHASH` /
//! `VCSSHORTHASH`: those ask the repository the project is in through a [`Vcs`] the caller sets for its thread ([`set_vcs`], as KiCad's text
//! evaluator reads `TEXT_EVAL_VCS`'s context path), and are left as written when none is set. Not ported: design variants. Simulation `OP` values are left
//! as written here and resolved by the studio, which holds the last run (`web/studio/src/kicad-port/sim/opVars.ts`).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::ir::TitleBlock;

// ------------------------------------------------------------------------------------------------ the version control

/// What `${VCSHASH}`, `${VCSSHORTHASH}` and the `@{vcs..}` functions ask of the repository the project is in (KiCad 25ec2335 `common/text_eval/text_eval_vcs.cpp`,
/// `common/git/project_git_utils.cpp::GetCurrentHash`). Each answer is empty (or 0, false) where KiCad's reader gives nothing: no repository, no commit, no
/// tag. A `path` is `"."` for the repository's HEAD, else a file, relative to the project's folder (`ResolveEffectivePath`).
pub trait Vcs {
    /// `ReadCommitHash`: HEAD's id for `"."`, else the last commit that changed the file (`GetFileCommit`), 40 hex digits.
    fn commit_hash(&self, path: &str) -> String;
    /// `ReadDescribeInfo`: the tag nearest HEAD among those matching `pattern` (`*` for none; annotated tags only, lightweight ones too with `any_tags`)
    /// and the number of commits walked before it.
    fn describe(&self, pattern: &str, any_tags: bool) -> (String, i64);
    /// `ReadIsDirty`: the index or the work tree differs from HEAD (untracked files counted with `include_untracked`).
    fn is_dirty(&self, include_untracked: bool) -> bool;
    /// `ReadCommitSignatureField`: the name or e-mail of the author or the committer of the commit [`Vcs::commit_hash`] names.
    fn signature(&self, path: &str, committer: bool, email: bool) -> String;
    /// `GetBranch`: the branch HEAD is on.
    fn branch(&self) -> String;
    /// `GetCommitTimestamp`: that commit's time, seconds since the epoch; 0 for none.
    fn commit_time(&self, path: &str) -> i64;
}

thread_local! {
    static VCS: RefCell<Option<Rc<dyn Vcs>>> = const { RefCell::new(None) };
}

/// The repository this thread's texts ask from now on (`TEXT_EVAL_VCS::SetContextPath`); `None` leaves the version control's variables and functions as
/// written. Returns the one set before.
pub fn set_vcs(vcs: Option<Rc<dyn Vcs>>) -> Option<Rc<dyn Vcs>> {
    VCS.with(|v| std::mem::replace(&mut *v.borrow_mut(), vcs))
}

/// `f` of the repository set for this thread, `None` when none is.
fn with_vcs<T>(f: impl FnOnce(&dyn Vcs) -> T) -> Option<T> {
    let vcs = VCS.with(|v| v.borrow().clone())?;
    Some(f(vcs.as_ref()))
}

/// `ERC_DRC` flag of `ExpandTextVars`: the report-only tokens (`ERC_WARNING ...`) are kept.
pub const FOR_ERC_DRC: u32 = 1;

/// `ADVANCED_CFG::m_ResolveTextRecursionDepth` (default 6): how deep nested variables and the passes of one text go.
pub const MAX_DEPTH: usize = 6;

pub(crate) const ESC_DOLLAR: &str = "<<<ESC_DOLLAR:";
pub(crate) const ESC_AT: &str = "<<<ESC_AT:";

/// A resolver is asked for one token (`NAME`, without the `${}`) and answers its text, or `None` when it is not one of its variables.
pub type Resolver<'a> = dyn Fn(&str) -> Option<String> + 'a;

fn chars_at(src: &[char], i: usize, marker: &str) -> bool {
    let m: Vec<char> = marker.chars().collect();
    i + m.len() <= src.len() && src[i..i + m.len()] == m[..]
}

/// Whether a text mentions a variable or an expression (`EDA_TEXT::HasTextVars`).
pub fn has_text_vars(s: &str) -> bool {
    s.contains("${") || s.contains("@{")
}

/// `UnescapeString`: `{slash}`, `{dblquote}`, `{brace}` ... in a stored text become the characters they stand for. A `{..}` right after `$`, `~`, `^` or
/// `_` is markup and stays, and so does an unknown name.
pub fn unescape_string(source: &str) -> String {
    let src: Vec<char> = source.chars().collect();
    if src.len() <= 2 {
        return source.to_string();
    }
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < src.len() {
        // The character before this one: what `prev` is in `UnescapeString`.
        let prev = if i == 0 { '\0' } else { src[i - 1] };
        let mut ch = src[i];
        if ch == '{' {
            let mut token = String::new();
            let mut depth = 1;
            let mut terminated = false;
            i += 1;
            while i < src.len() {
                ch = src[i];
                if ch == '{' {
                    depth += 1;
                } else if ch == '}' {
                    depth -= 1;
                }
                if depth <= 0 {
                    terminated = true;
                    break;
                }
                token.push(ch);
                i += 1;
            }
            if !terminated {
                out.push('{');
                out.push_str(&unescape_string(&token));
            } else if matches!(prev, '$' | '~' | '^' | '_') {
                out.push('{');
                out.push_str(&unescape_string(&token));
                out.push('}');
            } else {
                match token.as_str() {
                    "dblquote" => out.push('"'),
                    "quote" => out.push('\''),
                    "lt" => out.push('<'),
                    "gt" => out.push('>'),
                    "backslash" => out.push('\\'),
                    "slash" => out.push('/'),
                    "bar" => out.push('|'),
                    "comma" => out.push(','),
                    "colon" => out.push(':'),
                    "space" => out.push(' '),
                    "dollar" => out.push('$'),
                    "tab" => out.push('\t'),
                    "return" => out.push('\n'),
                    "brace" => out.push('{'),
                    other => {
                        out.push('{');
                        out.push_str(&unescape_string(other));
                        out.push('}');
                    }
                }
            }
        } else {
            out.push(ch);
        }
        i += 1;
    }
    out
}

/// Copy from `from` through the brace that closes the one just opened (the text after `<<<ESC_..:` is already inside one), returning the index after it.
fn copy_through_close(src: &[char], mut from: usize, out: &mut String) -> usize {
    let mut depth = 1;
    while from < src.len() && depth > 0 {
        match src[from] {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        out.push(src[from]);
        from += 1;
    }
    from
}

/// `ExpandTextVars(source, resolver, flags, depth)`: every `${TOKEN}` the resolver knows is replaced by its text; one it does not know stays as written. A
/// `\${..}` or `\@{..}` is kept as a literal (written with an escape marker until [`shown_text`] turns the markers back), a token that holds `${..}` itself
/// is expanded first (up to [`MAX_DEPTH`] deep), and an `@{..}` expression keeps its text with the variables inside it expanded.
pub fn expand_text_vars(source: &str, resolver: Option<&Resolver>, flags: u32, depth: usize) -> String {
    let src: Vec<char> = source.chars().collect();
    let n = src.len();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < n {
        // An escape marker written by an earlier pass: its contents are not touched.
        if chars_at(&src, i, ESC_DOLLAR) {
            out.push_str(ESC_DOLLAR);
            i = copy_through_close(&src, i + ESC_DOLLAR.len(), &mut out);
            continue;
        }
        if chars_at(&src, i, ESC_AT) {
            out.push_str(ESC_AT);
            i = copy_through_close(&src, i + ESC_AT.len(), &mut out);
            continue;
        }
        // `\${..}` and `\@{..}`: a literal.
        if src[i] == '\\' && i + 2 < n && (src[i + 1] == '$' || src[i + 1] == '@') && src[i + 2] == '{' {
            out.push_str(if src[i + 1] == '$' { ESC_DOLLAR } else { ESC_AT });
            i = copy_through_close(&src, i + 3, &mut out);
            continue;
        }
        if (src[i] == '$' || src[i] == '@') && i + 1 < n && src[i + 1] == '{' {
            let is_math = src[i] == '@';
            let mut token = String::new();
            let mut brace_depth = 1;
            let mut j = i + 2;
            while j < n {
                if chars_at(&src, j, ESC_DOLLAR) {
                    token.push_str(ESC_DOLLAR);
                    j = copy_through_close(&src, j + ESC_DOLLAR.len(), &mut token);
                    continue;
                }
                if chars_at(&src, j, ESC_AT) {
                    token.push_str(ESC_AT);
                    j = copy_through_close(&src, j + ESC_AT.len(), &mut token);
                    continue;
                }
                match src[j] {
                    '{' => {
                        brace_depth += 1;
                        token.push('{');
                    }
                    '}' => {
                        brace_depth -= 1;
                        if brace_depth == 0 {
                            break;
                        }
                        token.push('}');
                    }
                    c => token.push(c),
                }
                j += 1;
            }
            i = j + 1;
            if token.is_empty() {
                continue;
            }
            if has_text_vars(&token) && depth < MAX_DEPTH {
                token = expand_text_vars(&token, resolver, flags, depth + 1);
            }
            if is_math {
                out.push_str("@{");
                out.push_str(&token);
                out.push('}');
            } else if flags & FOR_ERC_DRC == 0 && ["ERC_WARNING", "ERC_ERROR", "DRC_WARNING", "DRC_ERROR"].iter().any(|p| token.starts_with(p)) {
                // Only shown during ERC and DRC.
            } else if let Some(resolved) = resolver.and_then(|r| r(&token)) {
                out.push_str(&resolved);
            } else {
                out.push_str("${");
                out.push_str(&token);
                out.push('}');
            }
            continue;
        }
        out.push(src[i]);
        i += 1;
    }
    out
}

/// `ResolveTextVars`: expand again while a variable is left, to `MAX_DEPTH` passes counted in `depth` (which the nested field lookups share), each pass
/// followed by the arithmetic of its `@{..}` expressions (KiCad 25ec2335: `text = evaluator.Evaluate( text )`, [`evaluate_math`]).
pub fn resolve_text_vars(source: &str, resolver: &Resolver, depth: &mut usize) -> String {
    let mut text = source.to_string();
    while has_text_vars(&text) && {
        *depth += 1;
        *depth <= MAX_DEPTH
    } {
        text = expand_text_vars(&text, Some(resolver), 0, 0);
        if text.contains("@{") {
            text = evaluate_math(&text);
        }
    }
    text
}

/// `EXPRESSION_EVALUATOR::Evaluate` (KiCad 25ec2335 `common/text_eval/text_eval_wrapper.cpp`, `text_eval_parser.cpp`, `text_eval.lemon`): each `@{..}` is
/// evaluated on its own (`evaluateWithPartialErrorRecovery`) and replaced by its value written as `VALUE_UTILS::ToString` writes it (a whole number without a
/// decimal point, else the shortest form). The grammar is `text_eval.lemon`'s: comparisons, then `+ -`, then `* / %`, then a sign, then `^` (right to
/// left); numbers with an SI prefix (`4k7`, `2.2u`, `1m`), a unit (`1in`, `10mil`, `2 mm`: in millimetres, `EDA_UNITS::MM`, the evaluator's default) and an
/// exponent; strings in `"` or `'` with their escapes; `+` joins when either side is a string, `==` and `!=` compare two strings; and the functions of
/// `EVAL_VISITOR::evaluateFunction` that give the same answer on every machine (the arithmetic ones, `upper`, `lower`, `concat`, `beforefirst` ...,
/// `replace`, `if`, `format`, `fixed`, `currency`, the E-series' `enearest`, `eup`, `edown`).
///
/// An expression that is not read, or whose evaluation fails (a division by zero, an unknown function, a string where a number is needed), is left as
/// written, as KiCad leaves it ("If processing had any evaluation errors, return original input unchanged"; the error goes to its error collector only).
/// **Differs**: the clock's (`today`, `now`, `dateformat`, `datestring`, `weekdayname`, `timeformat`), `random` and the version control's (`vcs..`) functions
/// are not ported and leave their expression as written; a number with a fraction is written by Rust's shortest form, `fmt::format( "{}" )`'s for most.
pub fn evaluate_math(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    // (a literal `\@{..}` is an escape marker by now, `<<<ESC_AT:..}`, which holds no `@{`)
    while let Some(at) = rest.find("@{") {
        out.push_str(&rest[..at]);
        let body = &rest[at + 2..];
        let mut depth = 1;
        let mut end = None;
        for (i, c) in body.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            out.push_str(&rest[at..]);
            return out;
        };
        match text_eval::evaluate(&body[..end]) {
            Some(v) => out.push_str(&v.to_text()),
            None => out.push_str(&rest[at..at + 2 + end + 1]),
        }
        rest = &body[end + 1..];
    }
    out.push_str(rest);
    out
}

/// `VALUE_UTILS::ToString` of a number: `{:.0f}` when it is within 1e-10 of a whole number (below 1e15), else `fmt`'s shortest form.
fn format_number(v: f64) -> String {
    let rounded = v.round();
    if (v - rounded).abs() < 1e-10 && rounded.abs() < 1e15 {
        format!("{rounded:.0}")
    } else if v.is_nan() {
        "nan".into()
    } else {
        format!("{v}")
    }
}

/// The reader and the evaluator of one `@{..}` (KiCad 25ec2335 `KIEVAL_TEXT_TOKENIZER`, `text_eval.lemon`, `EVAL_VISITOR`).
mod text_eval {
    use super::format_number;

    /// `calc_parser::VALUE`: a number or a string.
    #[derive(Debug, Clone, PartialEq)]
    pub(super) enum Value {
        Num(f64),
        Str(String),
    }

    impl Value {
        /// `VALUE_UTILS::ToString`.
        pub(super) fn to_text(&self) -> String {
            match self {
                Value::Num(n) => format_number(*n),
                Value::Str(s) => s.clone(),
            }
        }
        /// `VALUE_UTILS::ToDouble`: a number, or a string that is one number and nothing else (`fast_float::from_chars`).
        fn num(&self) -> Option<f64> {
            match self {
                Value::Num(n) => Some(*n),
                Value::Str(s) => (!s.starts_with('+') && !s.is_empty()).then(|| s.parse::<f64>().ok()).flatten(),
            }
        }
        /// `VALUE_UTILS::ToChar`: the string's first character, a space for none.
        fn first_char(&self) -> char {
            self.to_text().chars().next().unwrap_or(' ')
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Tok {
        Num(f64),
        Str(String),
        Ident(String),
        DollarOpen,
        Close,
        Op(&'static str),
        Text,
    }

    /// `utf8_utils::CHARACTER_CLASSIFIER::is_alpha`: an ASCII letter or any code point past ASCII that is not white space.
    fn is_alpha(c: char) -> bool {
        c.is_ascii_alphabetic() || (c as u32 >= 0x80 && c != '\u{FFFD}' && !c.is_whitespace())
    }

    /// `SI_PREFIX_HANDLER::prefixes`.
    fn si_multiplier(c: char) -> Option<f64> {
        Some(match c {
            'a' => 1e-18,
            'f' => 1e-15,
            'p' => 1e-12,
            'n' => 1e-9,
            'u' | 'µ' | 'μ' => 1e-6,
            'm' => 1e-3,
            'R' | 'F' => 1.0,
            'k' | 'K' => 1e3,
            'M' => 1e6,
            'G' => 1e9,
            'T' => 1e12,
            'P' => 1e15,
            'E' => 1e18,
            _ => return None,
        })
    }

    /// `text_eval_units::UnitRegistry`'s table (`parseUnit`) with each unit's millimetres (`conversionToMM`); the evaluator's default unit is the millimetre,
    /// and a unit of time or angle is taken as it is.
    fn unit_to_mm(unit: &str) -> Option<f64> {
        Some(match unit {
            "ps/mm" | "ps/cm" | "ps/in" | "deg" | "ps" | "fs" | "°" | "mm" => 1.0,
            "thou" | "mil" | "mils" => 25.4 / 1000.0,
            "cm" => 10.0,
            "in" | "\"" => 25.4,
            "um" => 1.0 / 1000.0,
            _ => return None,
        })
    }

    struct Lexer {
        s: Vec<char>,
        i: usize,
    }

    impl Lexer {
        fn at(&self, k: usize) -> char {
            self.s.get(self.i + k).copied().unwrap_or('\0')
        }
        fn at_exponent(&self) -> bool {
            if !matches!(self.at(0), 'e' | 'E') {
                return false;
            }
            let k = if matches!(self.at(1), '+' | '-') { 2 } else { 1 };
            self.at(k).is_ascii_digit()
        }
        /// The letters (and quotes) from `from`: what a unit could be.
        fn unit_run(&self, from: usize) -> String {
            self.s[from..].iter().take_while(|&&c| is_alpha(c) || c == '"' || c == '\'').collect()
        }
        /// `canConsumePrefix`: R and F carry no scale, so they are RKM markers only when no letter follows.
        fn can_consume_prefix(&self, c: char) -> bool {
            si_multiplier(c).is_some() && (!matches!(c, 'R' | 'F') || !(is_alpha(self.at(1)) || self.at(1) == '_'))
        }
        /// `parse_number` in the expression context.
        fn number(&mut self) -> Option<f64> {
            let mut text = String::new();
            let mut multiplier = 1.0;
            let mut has_prefix = false;
            while self.at(0).is_ascii_digit() {
                text.push(self.at(0));
                self.i += 1;
            }
            if self.i < self.s.len() {
                let c = self.at(0);
                if c == '.' {
                    text.push('.');
                    self.i += 1;
                } else if self.at_exponent() {
                } else if is_alpha(c) {
                    // a unit is looked for before a prefix (unit strings are longer)
                    if unit_to_mm(&self.unit_run(self.i)).is_none() && self.can_consume_prefix(c) {
                        multiplier = si_multiplier(c)?;
                        has_prefix = true;
                        self.i += 1;
                    }
                }
                // RKM notation (3k3, 4R7) puts the multiplier where the decimal point goes
                if has_prefix && self.at(0).is_ascii_digit() {
                    text.push('.');
                }
            }
            while self.at(0).is_ascii_digit() {
                text.push(self.at(0));
                self.i += 1;
            }
            if self.at_exponent() {
                text.push(self.at(0));
                self.i += 1;
                if matches!(self.at(0), '+' | '-') {
                    text.push(self.at(0));
                    self.i += 1;
                }
                while self.at(0).is_ascii_digit() {
                    text.push(self.at(0));
                    self.i += 1;
                }
            }
            if !has_prefix && is_alpha(self.at(0)) && unit_to_mm(&self.unit_run(self.i)).is_none() && self.can_consume_prefix(self.at(0)) {
                multiplier = si_multiplier(self.at(0))?;
                self.i += 1;
            }
            let mut value = if text.is_empty() || text == "." { 0.0 } else { text.parse::<f64>().ok()? * multiplier };
            if !value.is_finite() {
                return None; // "Number out of range"
            }
            // a unit after it, white space allowed between
            let back = self.i;
            while self.at(0).is_whitespace() && self.i < self.s.len() {
                self.i += 1;
            }
            let unit = self.unit_run(self.i);
            match unit_to_mm(&unit) {
                Some(f) if !unit.is_empty() => {
                    value *= f;
                    self.i += unit.chars().count();
                }
                _ => self.i = back,
            }
            Some(value)
        }
        /// `parse_string_literal`.
        fn string(&mut self, quote: char) -> Option<String> {
            self.i += 1;
            let mut out = String::new();
            while self.i < self.s.len() && self.at(0) != quote {
                let c = self.at(0);
                if c == '\\' && self.i + 1 < self.s.len() {
                    let e = self.at(1);
                    self.i += 2;
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        '\\' => out.push('\\'),
                        '"' => out.push('"'),
                        '\'' => out.push('\''),
                        '0' => out.push('\0'),
                        'x' => {
                            let hex: String = self.s[self.i..].iter().take(2).take_while(|c| c.is_ascii_hexdigit()).collect();
                            self.i += hex.len();
                            if hex.is_empty() {
                                out.push_str("\\x");
                            } else {
                                out.push(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).unwrap_or('\u{FFFD}'));
                            }
                        }
                        other => {
                            out.push('\\');
                            out.push(other);
                        }
                    }
                } else if c == '\n' {
                    return None; // "Unterminated string literal"
                } else {
                    out.push(c);
                    self.i += 1;
                }
            }
            if self.at(0) != quote {
                return None; // "Missing closing quote in string literal"
            }
            self.i += 1;
            Some(out)
        }
        /// `get_next_token` inside `@{..}`; `None` at the end.
        fn next(&mut self) -> Option<Option<Tok>> {
            while self.i < self.s.len() && self.at(0).is_whitespace() {
                self.i += 1;
            }
            if self.i >= self.s.len() {
                return Some(None);
            }
            let (c, n) = (self.at(0), self.at(1));
            if c == '$' && n == '{' {
                self.i += 2;
                return Some(Some(Tok::DollarOpen));
            }
            if c == '}' {
                self.i += 1;
                return Some(Some(Tok::Close));
            }
            for two in ["<=", ">=", "==", "!="] {
                if self.s[self.i..].starts_with(&two.chars().collect::<Vec<_>>()) {
                    self.i += 2;
                    return Some(Some(Tok::Op(two)));
                }
            }
            if let Some(op) = ["(", ")", "+", "-", "*", "/", "%", "^", ",", "<", ">"].into_iter().find(|o| o.starts_with(c)) {
                self.i += 1;
                return Some(Some(Tok::Op(op)));
            }
            if c == '"' || c == '\'' {
                return Some(Some(Tok::Str(self.string(c)?)));
            }
            if c.is_ascii_digit() || (c == '.' && n.is_ascii_digit()) {
                return Some(Some(Tok::Num(self.number()?)));
            }
            if is_alpha(c) || c == '_' {
                let id: String = self.s[self.i..].iter().take_while(|&&c| is_alpha(c) || c.is_ascii_digit() || c == '_').collect();
                self.i += id.chars().count();
                return Some(Some(Tok::Ident(id)));
            }
            self.i = self.s.len();
            Some(Some(Tok::Text))
        }
    }

    /// The expression's tokens, or `None` when the reader stops on an error.
    fn tokens(src: &str) -> Option<Vec<Tok>> {
        let mut lx = Lexer { s: src.chars().collect(), i: 0 };
        let mut out = Vec::new();
        while let Some(t) = lx.next()? {
            out.push(t);
        }
        Some(out)
    }

    struct Parser {
        t: Vec<Tok>,
        i: usize,
    }

    impl Parser {
        fn peek_op(&self, op: &str) -> bool {
            matches!(self.t.get(self.i), Some(Tok::Op(o)) if *o == op)
        }
        fn eat(&mut self, op: &str) -> bool {
            let yes = self.peek_op(op);
            if yes {
                self.i += 1;
            }
            yes
        }
        fn comparison(&mut self) -> Option<Value> {
            let mut l = self.sum()?;
            loop {
                let Some(op) = ["<=", ">=", "==", "!=", "<", ">"].into_iter().find(|o| self.eat(o)) else { return Some(l) };
                let r = self.sum()?;
                l = binop(&l, op, &r)?;
            }
        }
        fn sum(&mut self) -> Option<Value> {
            let mut l = self.term()?;
            loop {
                let Some(op) = ["+", "-"].into_iter().find(|o| self.eat(o)) else { return Some(l) };
                let r = self.term()?;
                l = binop(&l, op, &r)?;
            }
        }
        fn term(&mut self) -> Option<Value> {
            let mut l = self.unary()?;
            loop {
                let Some(op) = ["*", "/", "%"].into_iter().find(|o| self.eat(o)) else { return Some(l) };
                let r = self.unary()?;
                l = binop(&l, op, &r)?;
            }
        }
        /// `factor ::= MINUS factor [UMINUS]` (`0 - R`) and `PLUS factor` (R as it is); `^` binds tighter (`%right POWER` above `%right UMINUS`).
        fn unary(&mut self) -> Option<Value> {
            if self.eat("-") {
                let r = self.unary()?;
                return binop(&Value::Num(0.0), "-", &r);
            }
            if self.eat("+") {
                return self.unary();
            }
            self.power()
        }
        fn power(&mut self) -> Option<Value> {
            let base = self.atom()?;
            if self.eat("^") {
                let exp = self.unary()?;
                return binop(&base, "^", &exp);
            }
            Some(base)
        }
        fn atom(&mut self) -> Option<Value> {
            let t = self.t.get(self.i).cloned()?;
            self.i += 1;
            match t {
                Tok::Num(n) => Some(Value::Num(n)),
                Tok::Str(s) => Some(Value::Str(s)),
                Tok::Op("(") => {
                    let v = self.comparison()?;
                    self.eat(")").then_some(v)
                }
                // `function_call ::= IDENTIFIER LPAREN [arg_list] RPAREN`
                Tok::Ident(name) => {
                    if !self.eat("(") {
                        return None;
                    }
                    let mut args = Vec::new();
                    if !self.eat(")") {
                        loop {
                            args.push(self.comparison()?);
                            if self.eat(")") {
                                break;
                            }
                            if !self.eat(",") {
                                return None;
                            }
                        }
                    }
                    function(&name, &args)
                }
                // `variable ::= DOLLAR_OPEN IDENTIFIER CLOSE_BRACE`: a variable still here was not resolved ("Undefined variable")
                _ => None,
            }
        }
    }

    /// `EVAL_VISITOR::operator()`'s `BinOp`: `+` joins when either side is a string, `==` / `!=` compare two strings, else `VALUE_UTILS::ArithmeticOp`.
    fn binop(l: &Value, op: &str, r: &Value) -> Option<Value> {
        let string = |v: &Value| matches!(v, Value::Str(_));
        if op == "+" && (string(l) || string(r)) {
            return Some(Value::Str(l.to_text() + &r.to_text()));
        }
        if let (Value::Str(a), Value::Str(b), "==" | "!=") = (l, r, op) {
            return Some(Value::Num(if (a == b) == (op == "==") { 1.0 } else { 0.0 }));
        }
        let (a, b) = (l.num()?, r.num()?);
        let truth = |t: bool| Value::Num(if t { 1.0 } else { 0.0 });
        Some(match op {
            "+" => Value::Num(a + b),
            "-" => Value::Num(a - b),
            "*" => Value::Num(a * b),
            "/" if b == 0.0 => return None, // "Division by zero"
            "/" => Value::Num(a / b),
            "%" if b == 0.0 => return None, // "Modulo by zero"
            "%" => Value::Num(a % b),
            "^" => Value::Num(a.powf(b)),
            "<" => truth(a < b),
            ">" => truth(a > b),
            "<=" => truth(a <= b),
            ">=" => truth(a >= b),
            "==" => truth(a == b),
            "!=" => truth(a != b),
            _ => return None,
        })
    }

    /// `ESERIES_UTILS`: the E24 and E192 values of one decade (E3 to E24 and E48 to E96 take every n-th of them).
    const E24: [u16; 24] = [100, 110, 120, 130, 150, 160, 180, 200, 220, 240, 270, 300, 330, 360, 390, 430, 470, 510, 560, 620, 680, 750, 820, 910];
    const E192: [u16; 192] = [
        100, 101, 102, 104, 105, 106, 107, 109, 110, 111, 113, 114, 115, 117, 118, 120, 121, 123, 124, 126, 127, 129, 130, 132, 133, 135, 137, 138, 140, 142, 143, 145, 147, 149, 150, 152, 154, 156, 158, 160, 162, 164, 165,
        167, 169, 172, 174, 176, 178, 180, 182, 184, 187, 189, 191, 193, 196, 198, 200, 203, 205, 208, 210, 213, 215, 218, 221, 223, 226, 229, 232, 234, 237, 240, 243, 246, 249, 252, 255, 258, 261, 264, 267, 271, 274, 277,
        280, 284, 287, 291, 294, 298, 301, 305, 309, 312, 316, 320, 324, 328, 332, 336, 340, 344, 348, 352, 357, 361, 365, 370, 374, 379, 383, 388, 392, 397, 402, 407, 412, 417, 422, 427, 432, 437, 442, 448, 453, 459, 464,
        470, 475, 481, 487, 493, 499, 505, 511, 517, 523, 530, 536, 542, 549, 556, 562, 569, 576, 583, 590, 597, 604, 612, 619, 626, 634, 642, 649, 657, 665, 673, 681, 690, 698, 706, 715, 723, 732, 741, 750, 759, 768, 777,
        787, 796, 806, 816, 825, 835, 845, 856, 866, 876, 887, 898, 909, 920, 931, 942, 953, 965, 976, 988,
    ];

    /// `ESERIES_UTILS::FindNearest` / `FindUp` / `FindDown`: `how` is 0, 1 or -1.
    fn e_series(value: f64, series: &str, how: i32) -> Option<f64> {
        let n: usize = match series {
            "E3" | "e3" => 3,
            "E6" | "e6" => 6,
            "E12" | "e12" => 12,
            "E24" | "e24" => 24,
            "E48" | "e48" => 48,
            "E96" | "e96" => 96,
            "E192" | "e192" => 192,
            _ => return None,
        };
        if value <= 0.0 {
            return None;
        }
        let val = |i: usize| -> f64 { f64::from(if n <= 24 { E24[i * (24 / n)] } else { E192[i * (192 / n)] }) };
        let decade = value.log10().floor() as i32;
        let normalized = value / 10f64.powi(decade) * 100.0;
        let at = |v: f64, d: i32| v / 100.0 * 10f64.powi(d);
        match how {
            0 => {
                let (mut best, mut diff) = (100.0, f64::MAX);
                for i in 0..n {
                    let d = (normalized - val(i)).abs();
                    if d < diff {
                        (best, diff) = (val(i), d);
                    }
                }
                Some(at(best, decade))
            }
            1 => Some((0..n).map(val).find(|v| *v > normalized).map_or_else(|| at(val(0), decade + 1), |v| at(v, decade))),
            _ => Some((0..n).rev().map(val).find(|v| *v < normalized).map_or_else(|| at(val(n - 1), decade - 1), |v| at(v, decade))),
        }
    }

    /// `{:.{}f}`: a negative number of decimals is an error (`fmt` throws).
    fn fixed(v: f64, decimals: f64) -> Option<String> {
        let d = decimals as i64;
        (d >= 0).then(|| format!("{v:.*}", d as usize))
    }

    /// `wxString::Upper` / `Lower`: each character by itself.
    fn case(s: &str, upper: bool) -> String {
        s.chars()
            .map(|c| {
                let mut m: Vec<char> = if upper { c.to_uppercase().collect() } else { c.to_lowercase().collect() };
                if m.len() == 1 {
                    m.pop().unwrap_or(c)
                } else {
                    c
                }
            })
            .collect()
    }

    /// `DATE_UTILS::DaysToYmd`: a day count from 1970-01-01 as (year, month, day).
    fn days_to_ymd(days: i64) -> (i64, i64, i64) {
        let leap = |y: i64| (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let in_year = |y: i64| if leap(y) { 366 } else { 365 };
        let in_month = |m: i64, y: i64| if m == 2 && leap(y) { 29 } else { [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][(m - 1) as usize] };
        let (mut year, mut rest) = (1970, days);
        if rest >= 0 {
            while rest >= in_year(year) {
                rest -= in_year(year);
                year += 1;
            }
        } else {
            while rest < 0 {
                year -= 1;
                rest += in_year(year);
            }
        }
        let mut month = 1;
        while month <= 12 && rest >= in_month(month, year) {
            rest -= in_month(month, year);
            month += 1;
        }
        (year, month, rest + 1)
    }

    /// `DATE_UTILS::FormatDate`: ISO (`2026-11-06`, also any format it does not know), US, EU, long, short, and the Chinese, Japanese and Korean forms.
    fn format_date(days: i64, format: &str) -> String {
        const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
        let (y, m, d) = days_to_ymd(days);
        let month = MONTHS[(m - 1) as usize];
        match format {
            "US" | "us" => format!("{m:02}/{d:02}/{y:04}"),
            "EU" | "european" => format!("{d:02}/{m:02}/{y:04}"),
            "long" => format!("{month} {d}, {y}"),
            "short" => format!("{} {d}, {y}", &month[..3]),
            "Chinese" | "chinese" | "CN" | "cn" | "中文" | "Japanese" | "japanese" | "JP" | "jp" | "日本語" => format!("{y}年{m:02}月{d:02}日"),
            "Korean" | "korean" | "KR" | "kr" | "한국어" => format!("{y}년 {m:02}월 {d:02}일"),
            _ => format!("{y:04}-{m:02}-{d:02}"),
        }
    }

    /// The version control's functions of `EVAL_VISITOR::evaluateFunction` (KiCad 25ec2335 `text_eval_parser.cpp`), over the repository [`super::set_vcs`]
    /// set; `None` when `name` is not one of them, or none is set (the expression is then left as written). An answer the repository does not have is
    /// "<unknown>" (`vcsResult`); a hash is cut to the length asked, from 4 to 40 characters (`GetCommitHash`).
    fn vcs_function(name: &str, args: &[Value]) -> Option<Value> {
        let argc = args.len();
        let text = |i: usize| args[i].to_text();
        let flag = |i: usize| args.get(i).and_then(Value::num).is_some_and(|n| n != 0.0);
        let known = |s: String| if s.is_empty() { "<unknown>".to_string() } else { s };
        let hash = |v: &dyn super::Vcs, path: &str, length: Option<f64>| {
            let n = length.map_or(40, |l| l as i64).clamp(4, 40) as usize;
            known(v.commit_hash(path).chars().take(n).collect())
        };
        let date = |v: &dyn super::Vcs, path: &str, format: String| match v.commit_time(path) {
            0 => known(String::new()),
            t => format_date(t.div_euclid(24 * 3600), &format),
        };
        let s = match (name, argc) {
            ("vcsidentifier", 0..=1) => super::with_vcs(|v| hash(v, ".", args.first().and_then(Value::num)))?,
            ("vcsnearestlabel", 0..=2) => {
                let pattern = if argc >= 1 { text(0) } else { String::new() };
                super::with_vcs(|v| known(v.describe(&pattern, flag(1)).0))?
            }
            ("vcslabeldistance", 0..=2) => {
                let pattern = if argc >= 1 { text(0) } else { String::new() };
                super::with_vcs(|v| v.describe(&pattern, flag(1)).1.to_string())?
            }
            ("vcsdirty", 0..=1) => super::with_vcs(|v| if v.is_dirty(flag(0)) { "1" } else { "0" }.to_string())?,
            ("vcsdirtysuffix", 0..=2) => {
                let suffix = if argc >= 1 { text(0) } else { "-dirty".to_string() };
                super::with_vcs(|v| if v.is_dirty(flag(1)) { suffix } else { String::new() })?
            }
            ("vcsauthor", 0) => super::with_vcs(|v| known(v.signature(".", false, false)))?,
            ("vcsauthoremail", 0) => super::with_vcs(|v| known(v.signature(".", false, true)))?,
            ("vcscommitter", 0) => super::with_vcs(|v| known(v.signature(".", true, false)))?,
            ("vcscommitteremail", 0) => super::with_vcs(|v| known(v.signature(".", true, true)))?,
            ("vcsbranch", 0) => super::with_vcs(|v| known(v.branch()))?,
            ("vcscommitdate", 0..=1) => {
                let format = if argc == 1 { text(0) } else { "ISO".to_string() };
                super::with_vcs(|v| date(v, ".", format))?
            }
            ("vcsfileidentifier", 1..=2) => super::with_vcs(|v| hash(v, &text(0), args.get(1).and_then(Value::num)))?,
            ("vcsfileauthor", 1) => super::with_vcs(|v| known(v.signature(&text(0), false, false)))?,
            ("vcsfileauthoremail", 1) => super::with_vcs(|v| known(v.signature(&text(0), false, true)))?,
            ("vcsfilecommitter", 1) => super::with_vcs(|v| known(v.signature(&text(0), true, false)))?,
            ("vcsfilecommitteremail", 1) => super::with_vcs(|v| known(v.signature(&text(0), true, true)))?,
            ("vcsfilecommitdate", 1..=2) => {
                let format = if argc == 2 { text(1) } else { "ISO".to_string() };
                super::with_vcs(|v| date(v, &text(0), format))?
            }
            _ => return None,
        };
        Some(Value::Str(s))
    }

    /// `EVAL_VISITOR::evaluateFunction`, but the clock's and `random`; the version control's when a repository is set ([`vcs_function`]).
    fn function(name: &str, args: &[Value]) -> Option<Value> {
        if name.starts_with("vcs") {
            return vcs_function(name, args);
        }
        let argc = args.len();
        let text = |i: usize| args[i].to_text();
        match name {
            "format" | "fixed" if argc >= 1 => {
                let decimals = if argc > 1 { args[1].num().unwrap_or(2.0) } else { 2.0 };
                return Some(Value::Str(fixed(args[0].num()?, decimals)?));
            }
            "currency" if argc >= 1 => {
                let amount = args[0].num()?;
                let symbol = if argc > 1 { text(1) } else { "$".into() };
                return Some(Value::Str(format!("{symbol}{amount:.2}")));
            }
            "upper" if argc == 1 => return Some(Value::Str(case(&text(0), true))),
            "lower" if argc == 1 => return Some(Value::Str(case(&text(0), false))),
            "concat" if argc >= 2 => return Some(Value::Str(args.iter().map(Value::to_text).collect())),
            // `wxString::BeforeFirst` / `BeforeLast` / `AfterFirst` / `AfterLast` of `ToChar`: the whole string, or none, where it has no such character
            "beforefirst" if argc == 2 => {
                let s = text(0);
                return Some(Value::Str(s.split_once(args[1].first_char()).map_or(s.clone(), |(a, _)| a.to_string())));
            }
            "beforelast" if argc == 2 => return Some(Value::Str(text(0).rsplit_once(args[1].first_char()).map_or(String::new(), |(a, _)| a.to_string()))),
            "afterfirst" if argc == 2 => return Some(Value::Str(text(0).split_once(args[1].first_char()).map_or(String::new(), |(_, b)| b.to_string()))),
            "afterlast" if argc == 2 => {
                let s = text(0);
                return Some(Value::Str(s.rsplit_once(args[1].first_char()).map_or(s.clone(), |(_, b)| b.to_string())));
            }
            "replace" if argc == 3 => {
                let (s, search) = (text(0), text(1));
                return Some(Value::Str(if search.is_empty() { s } else { s.replace(&search, &text(2)) }));
            }
            "if" if argc == 3 => return Some(if args[0].num()? != 0.0 { args[1].clone() } else { args[2].clone() }),
            "enearest" | "eup" | "edown" if (1..=2).contains(&argc) => {
                let series = if argc > 1 { text(1) } else { "E24".into() };
                let how = match name {
                    "enearest" => 0,
                    "eup" => 1,
                    _ => -1,
                };
                return e_series(args[0].num()?, &series, how).map(Value::Num);
            }
            _ => {}
        }
        // the arithmetic ones take numbers
        let n: Vec<f64> = args.iter().map(Value::num).collect::<Option<_>>()?;
        let v = match (name, argc) {
            ("abs", 1) => n[0].abs(),
            ("sum", c) if c >= 1 => n.iter().sum(),
            ("round", c) if c >= 1 => {
                let m = 10f64.powi(if c > 1 { n[1] as i32 } else { 0 });
                (n[0] * m).round() / m
            }
            ("sqrt", 1) if n[0] < 0.0 => return None, // "Square root of negative number"
            ("sqrt", 1) => n[0].sqrt(),
            ("pow", 2) => n[0].powf(n[1]),
            ("floor", 1) => n[0].floor(),
            ("ceil", 1) => n[0].ceil(),
            ("min", c) if c >= 1 => n.iter().copied().fold(f64::INFINITY, f64::min),
            ("max", c) if c >= 1 => n.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            ("avg", c) if c >= 1 => n.iter().sum::<f64>() / c as f64,
            ("shunt", 2) => {
                let sum = n[0] + n[1];
                if sum > 0.0 {
                    n[0] * n[1] / sum
                } else {
                    0.0
                }
            }
            ("db", 1) if n[0] <= 0.0 => return None,
            ("db", 1) => 10.0 * n[0].log10(),
            ("dbv", 1) if n[0] <= 0.0 => return None,
            ("dbv", 1) => 20.0 * n[0].log10(),
            ("fromdb", 1) => 10f64.powf(n[0] / 10.0),
            ("fromdbv", 1) => 10f64.powf(n[0] / 20.0),
            _ => return None, // "Unknown function"
        };
        Some(Value::Num(v))
    }

    /// One `@{..}`'s body: its value, or `None` on any error.
    pub(super) fn evaluate(body: &str) -> Option<Value> {
        let mut p = Parser { t: tokens(body)?, i: 0 };
        let v = p.comparison()?;
        (p.i == p.t.len()).then_some(v)
    }
}

/// What `GetShownText` makes of a stored text: its escapes undone, its variables resolved, and the escape markers of a literal `\${..}` turned back
/// (`FinalizeTextVarExpansion`). `depth` is the nesting of the field lookup this text is the answer to: 0 for the text of an item itself, which is
/// shown, so a marker becomes `${..}` (`FOR_CANVAS`, `FOR_GUI`); more for a field another text looks up, which every `ResolveTextVar` asks for in the
/// `INTERNAL` context at `aDepth + 1` (`FOOTPRINT::ResolveTextVar`, `SCH_SYMBOL::ResolveTextVar`), so a marker stays escaped, `\${..}`, and the
/// text that asked keeps it literal too.
pub fn shown_text(source: &str, resolver: &Resolver, depth: usize) -> String {
    let mut text = unescape_string(source);
    if has_text_vars(&text) {
        let mut d = depth;
        text = resolve_text_vars(&text, resolver, &mut d);
    }
    if depth == 0 {
        text.replace(ESC_DOLLAR, "${").replace(ESC_AT, "@{")
    } else {
        text.replace(ESC_DOLLAR, "\\${").replace(ESC_AT, "\\@{")
    }
}

// ------------------------------------------------------------------------------------------------ the project and the title block

/// `TITLE_BLOCK::GetCurrentDate` and friends: the time `${CURRENT_DATE}`, `${CURRENT_TIME_HH_MM_SS}` and `${CURRENT_TIME_LOCALE}` stand for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Clock {
    /// `YYYY-MM-DD` (`wxDateTime::FormatISODate`).
    pub date: String,
    /// `19h23m42s` (`%Hh%Mm%Ss`, a form fit for a file name).
    pub time_hms: String,
    /// The time in the locale's form (`FormatTime`); kicad-cli runs in the C locale, `HH:MM:SS`.
    pub time_locale: String,
}

impl Clock {
    /// The local time `YYYY-MM-DDTHH:MM:SS` names (what a browser reports; the server has no time zone of its own).
    pub fn from_local(iso: &str) -> Option<Clock> {
        let (date, time) = iso.split_once('T')?;
        let d: Vec<&str> = date.split('-').collect();
        let mut t: Vec<&str> = time.split(':').collect();
        // Seconds may carry a fraction or a zone ("05.123Z"): only the two digits are the time.
        if let Some(last) = t.get_mut(2) {
            *last = last.get(..2).unwrap_or(last);
        }
        let ok = d.len() == 3 && t.len() >= 3 && d.iter().chain(t.iter().take(3)).all(|p| !p.is_empty() && p.len() <= 4 && p.bytes().all(|b| b.is_ascii_digit()));
        ok.then(|| Clock { date: date.to_string(), time_hms: format!("{}h{}m{}s", t[0], t[1], t[2]), time_locale: format!("{}:{}:{}", t[0], t[1], t[2]) })
    }

    /// The system clock in UTC (`std` has no local time): the fallback when the caller does not say what its local time is.
    pub fn utc_now() -> Clock {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
        let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        let (h, mi, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
        Clock { date: format!("{y:04}-{m:02}-{d:02}"), time_hms: format!("{h:02}h{mi:02}m{s:02}s"), time_locale: format!("{h:02}:{mi:02}:{s:02}") }
    }
}

/// The project a board or a schematic belongs to (`PROJECT`): its name and its text variables.
#[derive(Debug, Clone, Default)]
pub struct Project {
    /// `PROJECT::GetProjectName`: the project file's stem.
    pub name: String,
    /// `PROJECT::GetTextVars`.
    pub variables: BTreeMap<String, String>,
    pub clock: Clock,
}

impl Project {
    /// `PROJECT::TextVarResolver`.
    pub fn resolve(&self, token: &str) -> Option<String> {
        match token {
            "PROJECTNAME" => Some(self.name.clone()),
            "CURRENT_DATE" => Some(self.clock.date.clone()),
            "CURRENT_TIME_HH_MM_SS" => Some(self.clock.time_hms.clone()),
            "CURRENT_TIME_LOCALE" => Some(self.clock.time_locale.clone()),
            // `PROJECT_GIT_UTILS::GetCurrentHash`: HEAD's id, "no hash" outside a repository; the short one its first 8 characters (`Left( 8 )`)
            "VCSHASH" | "VCSSHORTHASH" => with_vcs(|v| {
                let hash = v.commit_hash(".");
                let hash = if hash.is_empty() { "no hash".to_string() } else { hash };
                if token == "VCSSHORTHASH" {
                    hash.chars().take(8).collect()
                } else {
                    hash
                }
            }),
            _ => self.variables.get(token).cloned(),
        }
    }

    /// `ExpandTextVars(text, project)`: only the project's variables.
    pub fn expand(&self, text: &str) -> String {
        expand_text_vars(text, Some(&|t: &str| self.resolve(t)), 0, 0)
    }
}

/// A title block as the resolvers read it (`TITLE_BLOCK`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TitleBlockVars {
    pub title: String,
    pub date: String,
    pub revision: String,
    pub company: String,
    /// `COMMENT1` .. `COMMENT9`, in order.
    pub comments: Vec<String>,
}

impl TitleBlockVars {
    pub fn of(tb: &TitleBlock) -> TitleBlockVars {
        TitleBlockVars { title: tb.title.clone(), date: tb.date.clone(), revision: tb.rev.clone(), company: tb.company.clone(), comments: tb.comments.clone() }
    }

    /// `TITLE_BLOCK::TextVarResolver`: the block's own variables, each value expanded with the project's. A token that comes back as the `${..}` it was is not resolved.
    pub fn resolve(&self, token: &str, project: Option<&Project>) -> Option<String> {
        let found = match token {
            "ISSUE_DATE" => Some(self.date.clone()),
            "CURRENT_DATE" => project.map(|p| p.clock.date.clone()).or_else(|| Some(Clock::utc_now().date)),
            "CURRENT_TIME_HH_MM_SS" => Some(project.map(|p| p.clock.time_hms.clone()).unwrap_or_else(|| Clock::utc_now().time_hms)),
            "CURRENT_TIME_LOCALE" => Some(project.map(|p| p.clock.time_locale.clone()).unwrap_or_else(|| Clock::utc_now().time_locale)),
            "REVISION" => Some(self.revision.clone()),
            "TITLE" => Some(self.title.clone()),
            "COMPANY" => Some(self.company.clone()),
            t if t.len() == 8 && t.starts_with("COMMENT") && matches!(t.as_bytes()[7], b'1'..=b'9') => Some(self.comments.get((t.as_bytes()[7] - b'1') as usize).cloned().unwrap_or_default()),
            _ => None,
        }?;
        // The date and the times are the clock's; every other value may name project variables.
        let value = if token.starts_with("CURRENT_") { found } else { project.map_or(found.clone(), |p| p.expand(&found)) };
        (value != format!("${{{token}}}")).then_some(value)
    }

    /// A value of the block as it is drawn: the project's variables in it expanded (`TITLE_BLOCK` values are expanded with the project).
    pub fn shown(&self, project: &Project) -> TitleBlockVars {
        let e = |s: &String| project.expand(s);
        TitleBlockVars { title: e(&self.title), date: e(&self.date), revision: e(&self.revision), company: e(&self.company), comments: self.comments.iter().map(e).collect() }
    }
}

// ------------------------------------------------------------------------------------------------ the board

/// One pad of a footprint, for `NET_NAME(n)`, `SHORT_NET_NAME(n)`, `NET_CLASS(n)` and `PIN_NAME(n)`.
#[derive(Debug, Clone, Default)]
pub struct PcbPad {
    pub number: String,
    /// `PAD::GetNetname`: the whole name of the net, empty for a pad on none.
    pub net: String,
    /// `PAD::GetNetClassName`.
    pub net_class: String,
    /// `PAD::GetPinFunction`: what the schematic calls the pin (empty when the board file does not say).
    pub pin_function: String,
}

/// A footprint as the resolvers read it (`FOOTPRINT`).
#[derive(Debug, Clone, Default)]
pub struct PcbFootprint {
    /// `FOOTPRINT::GetLayerName`: `F.Cu` or `B.Cu`.
    pub layer: String,
    /// `m_fpid`'s library nickname and item name.
    pub lib_nickname: String,
    pub lib_item_name: String,
    /// The fields, Reference and Value first.
    pub fields: Vec<PcbField>,
    pub pads: Vec<PcbPad>,
    /// `GetDNPForVariant`, `GetExcludedFromBOMForVariant`, `GetExcludedFromPosFilesForVariant`, `GetExcludedFromSimForVariant`: the footprint's attributes
    /// (`FP_DNP`, `FP_EXCLUDE_FROM_BOM`, `FP_EXCLUDE_FROM_POS_FILES`) and its symbol's "Exclude from simulation".
    pub dnp: bool,
    pub exclude_from_bom: bool,
    pub exclude_from_pos_files: bool,
    pub exclude_from_sim: bool,
}

/// A field of a footprint (`PCB_FIELD`): its name, its stored text and the layer it is on (what `${LAYER}` in it stands for).
#[derive(Debug, Clone, Default)]
pub struct PcbField {
    pub name: String,
    pub text: String,
    pub layer: String,
}

pub use crate::layers::standard_layer_name;

/// A board as the resolvers read it (`BOARD`).
#[derive(Debug, Clone, Default)]
pub struct PcbBoard {
    pub project: Project,
    /// The names the board gives its layers where they are not KiCad's (Board Setup > Board Editor Layers): the name the file writes -> the name the user gave.
    pub layer_names: BTreeMap<String, String>,
    /// `BOARD::GetFileName` as a name and as a path.
    pub file_name: String,
    pub file_path: String,
    pub title_block: Option<TitleBlockVars>,
    pub footprints: Vec<PcbFootprint>,
}

fn short_net_name(net: &str) -> &str {
    net.rsplit('/').next().unwrap_or(net)
}

impl PcbBoard {
    /// `BOARD::GetLayerName`: the name the user gave the layer, else KiCad's standard one -- what `${LAYER}` stands for.
    pub fn layer_name(&self, file_name: &str) -> String {
        self.layer_names.get(file_name).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| standard_layer_name(file_name))
    }

    /// `BOARD::ResolveTextVar`.
    pub fn resolve(&self, token: &str, depth: usize) -> Option<String> {
        if let Some((reference, remainder)) = token.split_once(':') {
            // No item has a uuid the text could name here; the reference designator it does name, case aside.
            return Some(match self.footprints.iter().find(|f| f.reference().eq_ignore_ascii_case(reference)) {
                Some(fp) => fp.resolve(self, remainder, depth + 1).unwrap_or_else(|| format!("<Unresolved: {}:{remainder}>", fp.reference())),
                None => format!("<Unknown reference: {reference}>"),
            });
        }
        match token {
            "FILENAME" => return Some(self.file_name.clone()),
            "FILEPATH" => return Some(self.file_path.clone()),
            // No design variants.
            "VARIANT" | "VARIANT_DESC" => return Some(String::new()),
            "PROJECTNAME" => return Some(self.project.name.clone()),
            _ => {}
        }
        if let Some(r) = self.title_block.as_ref().and_then(|tb| tb.resolve(token, Some(&self.project))) {
            return Some(r);
        }
        self.project.resolve(token)
    }
}

impl PcbFootprint {
    fn field(&self, name: &str) -> Option<&PcbField> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// The Reference field's stored text.
    pub fn reference(&self) -> &str {
        self.field("Reference").map_or("", |f| f.text.as_str())
    }

    /// `PCB_FIELD::GetShownText` of the field called `name`.
    pub fn field_shown(&self, board: &PcbBoard, name: &str, depth: usize) -> Option<String> {
        self.field(name).map(|f| self.shown(board, &f.text, &f.layer, depth))
    }

    /// `PCB_FIELD::GetShownText`: a stored text of this footprint (one of its fields, or one of its texts) on `layer`. `${LAYER}` is the text's own layer, then the
    /// footprint answers and then the board.
    pub fn shown(&self, board: &PcbBoard, raw: &str, layer: &str, depth: usize) -> String {
        shown_text(raw, &|t: &str| (t == "LAYER").then(|| board.layer_name(layer)).or_else(|| self.resolve(board, t, depth + 1)).or_else(|| board.resolve(t, depth + 1)), depth)
    }

    /// `FOOTPRINT::ResolveTextVar`.
    pub fn resolve(&self, board: &PcbBoard, token: &str, depth: usize) -> Option<String> {
        match token {
            "REFERENCE" => return self.field_shown(board, "Reference", depth + 1),
            "VALUE" => return self.field_shown(board, "Value", depth + 1),
            "LAYER" => return Some(board.layer_name(&self.layer)),
            "FOOTPRINT_LIBRARY" => return Some(self.lib_nickname.clone()),
            "FOOTPRINT_NAME" => return Some(self.lib_item_name.clone()),
            // KiCad 25ec2335 `FOOTPRINT::ResolveTextVar`: the flags, by the words the symbol's resolver uses (a footprint is never excluded from the board)
            "EXCLUDE_FROM_BOM" => return Some(if self.exclude_from_bom { "Excluded from BOM" } else { "" }.to_string()),
            "EXCLUDE_FROM_POS_FILES" => return Some(if self.exclude_from_pos_files { "Excluded from position files" } else { "" }.to_string()),
            "EXCLUDE_FROM_BOARD" => return Some(String::new()),
            "EXCLUDE_FROM_SIM" => return Some(if self.exclude_from_sim { "Excluded from simulation" } else { "" }.to_string()),
            "DNP" => return Some(if self.dnp { "DNP" } else { "" }.to_string()),
            _ => {}
        }
        for prefix in ["SHORT_NET_NAME(", "NET_NAME(", "NET_CLASS(", "PIN_NAME("] {
            if token.starts_with(prefix) {
                let number = token[prefix.len()..].rsplit_once(')').map_or("", |(n, _)| n);
                if let Some(pad) = self.pads.iter().find(|p| p.number == number) {
                    return Some(match prefix {
                        "SHORT_NET_NAME(" => short_net_name(&pad.net).to_string(),
                        "NET_NAME(" => pad.net.clone(),
                        "NET_CLASS(" => pad.net_class.clone(),
                        _ => pad.pin_function.clone(),
                    });
                }
                // No such pad: the board may know the name.
                return board.resolve(token, depth + 1);
            }
        }
        if let Some(shown) = self.field_shown(board, token, depth + 1) {
            return Some(shown);
        }
        board.resolve(token, depth + 1)
    }
}

/// `PCB_TEXT::GetShownText`: a text of the board (`footprint` is the one it belongs to, none for a free text) on `layer`.
pub fn pcb_text_shown(raw: &str, layer: &str, footprint: Option<&PcbFootprint>, board: &PcbBoard) -> String {
    shown_text(
        raw,
        &|t: &str| {
            if t == "LAYER" {
                return Some(board.layer_name(layer));
            }
            footprint.and_then(|fp| fp.resolve(board, t, 1)).or_else(|| board.resolve(t, 1))
        },
        0,
    )
}

// ------------------------------------------------------------------------------------------------ the schematic

/// One pin of a symbol, for `NET_NAME(n)`, `SHORT_NET_NAME(n)`, `NET_CLASS(n)`, `PIN_NAME(n)`, `PIN_BASE_NAME(n)`, `PIN_ALT_LIST(n)` and `REFERENCE(n)`.
#[derive(Debug, Clone, Default)]
pub struct SchPin {
    pub number: String,
    /// `SCH_PIN::GetBaseName`: the library pin's own name.
    pub name: String,
    /// `SCH_PIN::GetAlt`: the alternate function the placed pin is set to (`SCH_PIN::SetAlt`), empty for none.
    pub alt: String,
    /// `SCH_PIN::GetAlternates`: the names of the pin's alternate functions, in the map's order (by name).
    pub alternates: Vec<String>,
    /// The net the pin is on, empty when it is on none.
    pub net: String,
    pub net_class: String,
    /// The unit of the symbol the pin belongs to (0 for a pin every unit shares).
    pub unit: u32,
}

/// A symbol (or a power symbol, or a sheet) as the resolvers read it.
#[derive(Debug, Clone, Default)]
pub struct SchSymbol {
    /// The fields by name with their stored texts. `Reference` first.
    pub fields: Vec<(String, String)>,
    /// `SCH_SYMBOL::GetRef(path, true)`, the reference with its unit letter, and without it.
    pub reference: String,
    pub short_reference: String,
    /// `SubReference(unit)`: `A`, `B` ...
    pub unit_letter: String,
    pub lib_nickname: String,
    pub lib_item_name: String,
    /// The Footprint field's text (`Lib:Name`).
    pub footprint: String,
    pub description: String,
    pub keywords: String,
    pub dnp: bool,
    pub exclude_from_bom: bool,
    pub exclude_from_board: bool,
    pub exclude_from_sim: bool,
    pub pins: Vec<SchPin>,
    /// The units' display names of the symbol's library symbol (`LIB_SYMBOL::GetUnitDisplayNames`), which [`SchSymbol::unit_letter_of`] gives in place of
    /// a letter (KiCad 25ec2335 `SCH_SYMBOL::SubReference`, r6 B6-H1).
    pub unit_names: std::collections::BTreeMap<u32, String>,
    /// The project's unit notation (`SCHEMATIC_SETTINGS::m_SubpartIdSeparator`, `m_SubpartFirstId`), which [`SchSymbol::unit_letter_of`] names a unit by (r7 H5).
    pub subpart: crate::annotate::SubpartIds,
}

/// One sheet of the path from the root down to the sheet in view (`SCH_SHEET_PATH`): the placement's fields and the title block of the screen it shows.
#[derive(Debug, Clone, Default)]
pub struct SchSheetLevel {
    /// The placement's fields (`Sheetname`, `Sheetfile`, the user's), empty for the root.
    pub fields: Vec<(String, String)>,
    pub title_block: Option<TitleBlockVars>,
}

/// The schematic as the resolvers read it (`SCHEMATIC`, with the path of the sheet in view).
#[derive(Debug, Clone, Default)]
pub struct SchSheet {
    pub project: Project,
    /// The root first, the sheet in view last.
    pub levels: Vec<SchSheetLevel>,
    /// `SCH_SHEET_PATH::GetPageNumber` of the sheet in view, and `Root().CountSheets()`.
    pub page: String,
    pub page_count: usize,
    /// `PathHumanReadable`: `/` for the root, `/Power/` for a sheet in it.
    pub sheet_path: String,
    /// The last sheet's name (empty for the root).
    pub sheet_name: String,
    pub file_name: String,
    pub file_path: String,
    /// Every symbol of the schematic, for `${REF:field}`.
    pub symbols: Vec<SchSymbol>,
}

impl SchSheet {
    /// `SCH_SHEET::ResolveTextVar` for the level `at` of the path, and so for each one above it, then `SCHEMATIC::ResolveTextVar`.
    pub fn resolve_level(&self, at: usize, token: &str, depth: usize) -> Option<String> {
        if token.contains(':') {
            return self.resolve_cross_reference(token, depth + 1);
        }
        if let Some(level) = self.levels.get(at) {
            for (name, text) in &level.fields {
                // A sheet's two mandatory fields are named in capitals (`GetCanonicalName().Upper()`): `${SHEETNAME}`, `${SHEETFILE}`; a user's field by its own name.
                let key = if name == "Sheetname" || name == "Sheetfile" { name.to_uppercase() } else { name.clone() };
                if token == key {
                    return Some(self.field_shown(at, None, text, depth + 1));
                }
            }
            if let Some(r) = level.title_block.as_ref().and_then(|tb| tb.resolve(token, Some(&self.project))) {
                return Some(r);
            }
        }
        match token {
            "#" => return Some(self.page.clone()),
            "##" => return Some(self.page_count.to_string()),
            "SHEETPATH" => return Some(self.sheet_path.clone()),
            "EXCLUDE_FROM_BOM" | "EXCLUDE_FROM_BOARD" | "EXCLUDE_FROM_SIM" | "DNP" => return Some(String::new()),
            _ => {}
        }
        if at > 0 {
            return self.resolve_level(at - 1, token, depth + 1);
        }
        self.resolve_schematic(token)
    }

    /// `SCHEMATIC::ResolveTextVar`.
    pub fn resolve_schematic(&self, token: &str) -> Option<String> {
        match token {
            "#" => return Some(self.page.clone()),
            "##" => return Some(self.page_count.to_string()),
            "SHEETPATH" => return Some(self.sheet_path.clone()),
            "SHEETNAME" => return Some(self.sheet_name.clone()),
            "FILENAME" => return Some(self.file_name.clone()),
            "FILEPATH" => return Some(self.file_path.clone()),
            "PROJECTNAME" => return Some(self.project.name.clone()),
            "VARIANT" | "VARIANTNAME" | "VARIANT_DESC" => return Some(String::new()),
            _ => {}
        }
        // The path has been taken back to the root by now (`SCH_SHEET::ResolveTextVar` pops one sheet at a time), so it is the root screen's title block.
        if let Some(r) = self.levels.first().and_then(|l| l.title_block.as_ref()).and_then(|tb| tb.resolve(token, Some(&self.project))) {
            return Some(r);
        }
        self.project.resolve(token)
    }

    /// `SCHEMATIC::ResolveCrossReference`: `REF:FIELD`, by reference designator (a multi-unit symbol's `U1` names `U1A`).
    fn resolve_cross_reference(&self, token: &str, depth: usize) -> Option<String> {
        let (reference, remainder) = token.split_once(':')?;
        let field = remainder.split_once(':').map_or(remainder, |(f, _variant)| f);
        let found = self.symbols.iter().find(|s| s.short_reference == reference || s.reference == reference).or_else(|| {
            self.symbols.iter().filter(|s| s.reference.len() == reference.len() + 1 && s.reference.starts_with(reference) && s.reference.ends_with(|c: char| c.is_ascii_uppercase())).last()
        });
        Some(match found {
            Some(sym) => self.symbol_resolve(sym, field, depth + 1).unwrap_or_else(|| format!("<Unresolved: {}:{field}>", sym.short_reference)),
            None => format!("<Unresolved: {reference}>"),
        })
    }

    /// `SCH_SYMBOL::ResolveTextVar`: the sheet the symbol is on is asked last.
    pub fn symbol_resolve(&self, sym: &SchSymbol, token: &str, depth: usize) -> Option<String> {
        let at = self.levels.len().saturating_sub(1);
        if token.contains(':') {
            return self.resolve_cross_reference(token, depth + 1);
        }
        for (name, text) in &sym.fields {
            // A field whose text is just a reference to itself ends the lookup, whatever the token: it comes back as it is (`SCH_SYMBOL::ResolveTextVar`).
            if text.replace(' ', "").eq_ignore_ascii_case(&format!("${{{name}}}")) {
                return Some(token.to_string());
            }
            if token.eq_ignore_ascii_case(name) {
                return Some(if name == "Reference" { sym.reference.clone() } else { self.field_shown(at, Some(sym), text, depth + 1) });
            }
        }
        match token {
            "SIM.DEVICE" | "SIM.TYPE" | "SIM.PINS" | "SIM.PARAMS" | "SIM.LIBRARY" | "SIM.NAME" => return Some(String::new()),
            "FOOTPRINT_LIBRARY" => return Some(sym.footprint.split(':').next().filter(|_| !sym.footprint.is_empty()).unwrap_or("").to_string()),
            "FOOTPRINT_NAME" => return Some(sym.footprint.split(':').nth(1).unwrap_or("").to_string()),
            "UNIT" => return Some(sym.unit_letter.clone()),
            "SHORT_REFERENCE" => return Some(sym.short_reference.clone()),
            "SYMBOL_LIBRARY" => return Some(sym.lib_nickname.clone()),
            "SYMBOL_NAME" => return Some(sym.lib_item_name.clone()),
            "SYMBOL_DESCRIPTION" => return Some(sym.description.clone()),
            "SYMBOL_KEYWORDS" => return Some(sym.keywords.clone()),
            "EXCLUDE_FROM_BOM" => return Some(if sym.exclude_from_bom { "Excluded from BOM" } else { "" }.to_string()),
            "EXCLUDE_FROM_BOARD" => return Some(if sym.exclude_from_board { "Excluded from board" } else { "" }.to_string()),
            "EXCLUDE_FROM_SIM" => return Some(if sym.exclude_from_sim { "Excluded from simulation" } else { "" }.to_string()),
            "DNP" => return Some(if sym.dnp { "DNP" } else { "" }.to_string()),
            _ => {}
        }
        // KiCad 25ec2335 `SCH_SYMBOL::ResolveTextVar`: `PIN_NAME` is the alternate the pin is set to, else its name; `PIN_BASE_NAME` its own name;
        // `PIN_ALT_LIST` its alternates' names, joined by ", "
        for prefix in ["SHORT_NET_NAME(", "NET_NAME(", "NET_CLASS(", "PIN_NAME(", "PIN_BASE_NAME(", "PIN_ALT_LIST(", "REFERENCE(", "SHORT_REFERENCE(", "UNIT("] {
            if token.starts_with(prefix) {
                let number = token[prefix.len()..].rsplit_once(')').map_or("", |(n, _)| n);
                let Some(pin) = sym.pins.iter().find(|p| p.number == number) else { return Some(format!("<Unresolved: pin {number}>")) };
                let unit_letter = |p: &SchPin| if p.unit > 0 { sym.unit_letter_of(p.unit) } else { String::new() };
                return Some(match prefix {
                    "REFERENCE(" => format!("{}{}", sym.short_reference, unit_letter(pin)),
                    "SHORT_REFERENCE(" => sym.short_reference.clone(),
                    "UNIT(" => unit_letter(pin),
                    "PIN_NAME(" => if pin.alt.is_empty() { pin.name.clone() } else { pin.alt.clone() },
                    "PIN_BASE_NAME(" => pin.name.clone(),
                    "PIN_ALT_LIST(" => pin.alternates.join(", "),
                    "SHORT_NET_NAME(" => {
                        if pin.net.is_empty() || pin.net.to_lowercase().starts_with("unconnected") {
                            "NC".to_string()
                        } else {
                            short_net_name(&pin.net).to_string()
                        }
                    }
                    "NET_NAME(" => pin.net.clone(),
                    _ => pin.net_class.clone(),
                });
            }
        }
        // The sheet the symbol is on.
        self.resolve_level(at, token, depth + 1)
    }

    /// `SCH_FIELD::GetShownText`, `SCH_ITEM::ResolveText`: a stored text of the item (the symbol `sym`, or the sheet at `level` when none) with its variables resolved.
    pub fn field_shown(&self, level: usize, sym: Option<&SchSymbol>, raw: &str, depth: usize) -> String {
        let resolver = |t: &str| match sym {
            Some(s) => self.symbol_resolve(s, t, depth + 1),
            None => self.resolve_level(level, t, depth + 1),
        };
        shown_text(raw, &resolver, depth)
    }

    /// `SCH_TEXT::GetShownText`: a free text on the sheet in view.
    pub fn text_shown(&self, raw: &str) -> String {
        self.field_shown(self.levels.len().saturating_sub(1), None, raw, 0)
    }
}

impl SchSymbol {
    /// `SubReference( unit, false )` for another unit than this symbol's own, as `${REFERENCE(pin)}` and `${UNIT(pin)}` ask for it (KiCad 25ec2335
    /// `SCH_SYMBOL::ResolveTextVar`: `SubReference( pinUnit, false )`): the unit's display name, else `A` for 1, `B` for 2 ... in the project's notation
    /// (`SCHEMATIC_SETTINGS::SubReference`), with no separator.
    pub fn unit_letter_of(&self, unit: u32) -> String {
        if unit == 0 {
            return String::new();
        }
        if let Some(name) = self.unit_names.get(&unit) {
            return name.clone();
        }
        self.subpart.sub_reference(unit, false)
    }
}

// ------------------------------------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    fn project(vars: &[(&str, &str)]) -> Project {
        Project { name: "demo".into(), variables: vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), clock: Clock { date: "2026-10-10".into(), time_hms: "21h30m05s".into(), time_locale: "21:30:05".into() } }
    }

    fn expand(src: &str, p: &Project) -> String {
        shown_text(src, &|t: &str| p.resolve(t), 0)
    }

    #[test]
    fn a_known_variable_is_replaced_and_an_unknown_one_stays_as_written() {
        let p = project(&[("REV", "B"), ("Designer name", "Ada")]);
        assert_eq!(expand("Rev ${REV} by ${Designer name}", &p), "Rev B by Ada");
        assert_eq!(expand("${NOPE} and ${REV}", &p), "${NOPE} and B");
        assert_eq!(expand("no variables here, $ and { and }", &p), "no variables here, $ and { and }");
        assert_eq!(expand("${PROJECTNAME} ${CURRENT_DATE}", &p), "demo 2026-10-10");
        assert_eq!(expand("${VCSHASH}", &p), "${VCSHASH}", "the project's git hash is not known here");
    }

    #[test]
    fn a_backslash_keeps_a_variable_literal_and_nested_variables_expand_inside_out() {
        let p = project(&[("A", "x"), ("B", "${A}y"), ("NAME", "REV"), ("REV", "B")]);
        assert_eq!(expand("\\${A} is ${A}", &p), "${A} is x", "a backslash before the dollar is shown as a plain dollar and not expanded");
        assert_eq!(expand("\\@{1+1}", &p), "@{1+1}");
        assert_eq!(expand("${B}", &p), "xy", "a value that names another variable is expanded on the next pass");
        assert_eq!(expand("${${NAME}}", &p), "B", "the inner reference is expanded before the outer is looked up");
        assert_eq!(expand("${A", &p), "x", "an unterminated reference is read to the end of the text");
        assert_eq!(expand("${NOPE", &p), "${NOPE}", "and one that is not known is closed with the brace it lacks");
        assert_eq!(expand("${}", &p), "", "an empty reference is dropped");
        assert_eq!(expand("@{${A}+1}", &p), "@{x+1}", "an expression keeps its text, with the variables in it expanded");
    }

    /// `EXPRESSION_EVALUATOR::Evaluate`'s arithmetic, as KiCad 25ec2335's `ResolveTextVars` runs it after each pass: QA `issue23962`'s label `Out@{(${#}-2)*8+0}`
    /// on page 3 is `Out8`, `issue24044`'s `Ch@{(${#}-2)*2+1}` on page 2 is `Ch1`.
    #[test]
    fn a_math_expression_is_evaluated_once_its_variables_are_expanded() {
        let p = project(&[("N", "3"), ("A", "x")]);
        assert_eq!(expand("Out@{(${N}-2)*8+0}", &p), "Out8");
        assert_eq!(expand("@{7/2} @{2^3^2} @{-2^2} @{7%4} @{1+2*3} @{(1+2)*3} @{2>1} @{0.1+0.2}", &p), "3.5 512 -4 3 7 9 1 0.30000000000000004");
        assert_eq!(expand("@{1/0}", &p), "@{1/0}", "an error is left as written");
        assert_eq!(evaluate_math("a @{2*3} b @{x} c"), "a 6 b @{x} c");
    }

    /// UI-LEFT S22: the rest of `text_eval.lemon` and `EVAL_VISITOR::evaluateFunction` (KiCad 25ec2335): strings, `+` joining them, string comparisons,
    /// SI prefixes and RKM notation, units in millimetres, and the functions -- each expression evaluated on its own, one that fails left as written.
    #[test]
    fn the_grammars_strings_units_and_functions_are_evaluated_as_kicad_does() {
        let e = evaluate_math;
        // strings
        assert_eq!(e(r#"@{"Ch" + 3} @{'a' + "b"} @{"x" == "x"} @{"x" != 'x'} @{"a\tb"}"#), "Ch3 ab 1 0 a\tb");
        assert_eq!(e(r#"@{upper("net_a")}/@{lower('LED')} @{concat("U", 1, "A")}"#), "NET_A/led U1A");
        assert_eq!(e(r#"@{beforefirst("a.b.c", ".")} @{beforelast("a.b.c", ".")} @{afterfirst("a.b.c", ".")} @{afterlast("a.b.c", ".")}"#), "a a.b b.c c");
        assert_eq!(e(r#"[@{beforefirst("abc", ".")}] [@{afterfirst("abc", ".")}] [@{beforelast("abc", ".")}] [@{afterlast("abc", ".")}]"#), "[abc] [] [] [abc]", "wxString's answers where the character is not there");
        assert_eq!(e(r#"@{replace("R_1_2", "_", "-")} @{if(2 > 1, "yes", "no")} @{if(0, 1, 2)}"#), "R-1-2 yes 2");
        // numbers: SI prefixes, RKM, exponents and units (millimetres)
        assert_eq!(e("@{4k7} @{2.2u*1e6} @{1m*1000} @{3E3} @{4R7} @{1in} @{100mil} @{2 mm + 1cm} @{1.5M/1k}"), "4700 2.2 1 3000 4.7 25.4 2.54 12 1500");
        // the arithmetic functions
        assert_eq!(e("@{abs(-3)} @{sum(1,2,3)} @{round(2.345, 2)} @{round(2.5)} @{sqrt(16)} @{pow(2, 10)} @{floor(-1.5)} @{ceil(1.2)} @{min(3, 1, 2)} @{max(3, 1, 2)} @{avg(1, 2)}"), "3 6 2.35 3 4 1024 -2 2 1 3 1.5");
        assert_eq!(e("@{shunt(10, 10)} @{db(100)} @{dbv(10)} @{fromdb(20)} @{fromdbv(20)}"), "5 20 20 100 10");
        assert_eq!(e("@{format(3.14159)} @{fixed(2, 3)} @{currency(5)} @{currency(5, \"EUR \")}"), "3.14 2.000 $5.00 EUR 5.00");
        assert_eq!(e("@{enearest(4800)} @{eup(4700)} @{edown(4700)} @{enearest(1234, \"E96\")}"), "4700 5100 4300 1240");
        // what fails is left as written, the rest of the text evaluated
        assert_eq!(e("@{sqrt(-1)} @{nope(1)} @{\"a\" - 1} @{1+} @{upper()} ok @{1+1}"), "@{sqrt(-1)} @{nope(1)} @{\"a\" - 1} @{1+} @{upper()} ok 2");
        // a variable left unresolved is no value
        assert_eq!(e("@{${X}+1}"), "@{${X}+1}");
    }

    /// A repository that answers what a test says (`TEXT_EVAL_VCS`' readers).
    struct FakeVcs {
        hash: String,
    }

    impl Vcs for FakeVcs {
        fn commit_hash(&self, path: &str) -> String {
            if path == "." { self.hash.clone() } else if path == "board.kicad_sch" { "f".repeat(40) } else { String::new() }
        }
        fn describe(&self, pattern: &str, any_tags: bool) -> (String, i64) {
            match (pattern, any_tags) {
                ("", false) | ("v*", false) => ("v1.2".into(), 3),
                (_, true) => ("light".into(), 0),
                _ => (String::new(), 0),
            }
        }
        fn is_dirty(&self, include_untracked: bool) -> bool {
            include_untracked
        }
        fn signature(&self, _path: &str, committer: bool, email: bool) -> String {
            match (committer, email) {
                (false, false) => "Ada".into(),
                (false, true) => "ada@x".into(),
                (true, false) => "Bob".into(),
                (true, true) => String::new(),
            }
        }
        fn branch(&self) -> String {
            "main".into()
        }
        fn commit_time(&self, path: &str) -> i64 {
            if path == "." { 1_762_387_200 } else { 0 }
        }
    }

    /// KiCad 25ec2335 `text_eval_parser.cpp`'s version control functions and `PROJECT::TextVarResolver`'s `VCSHASH` / `VCSSHORTHASH`, over the repository
    /// the thread's texts are set to; with none set they stay as written.
    #[test]
    fn the_version_control_functions_ask_the_repository_set_for_the_thread() {
        let p = project(&[]);
        assert_eq!(expand("${VCSHASH} @{vcsbranch()}", &p), "${VCSHASH} @{vcsbranch()}", "no repository set: left as written");
        let before = set_vcs(Some(Rc::new(FakeVcs { hash: "0123456789abcdef0123456789abcdef01234567".into() })));
        assert_eq!(expand("${VCSHASH}", &p), "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(expand("${VCSSHORTHASH}", &p), "01234567");
        assert_eq!(expand("@{vcsidentifier()} @{vcsidentifier(7)} @{vcsidentifier(2)}", &p), "0123456789abcdef0123456789abcdef01234567 0123456 0123", "the length clamped to 4 .. 40");
        assert_eq!(expand("@{vcsnearestlabel()}-@{vcslabeldistance()} @{vcsnearestlabel(\"x*\")} @{vcsnearestlabel(\"\", 1)}", &p), "v1.2-3 <unknown> light");
        assert_eq!(expand("@{vcsdirty()} @{vcsdirty(1)} [@{vcsdirtysuffix()}] @{vcsdirtysuffix(\"+\", 1)}", &p), "0 1 [] +");
        assert_eq!(expand("@{vcsauthor()} @{vcsauthoremail()} @{vcscommitter()} @{vcscommitteremail()} @{vcsbranch()}", &p), "Ada ada@x Bob <unknown> main");
        assert_eq!(expand("@{vcscommitdate()} @{vcscommitdate(\"US\")} @{vcscommitdate(\"long\")} @{vcscommitdate(\"short\")}", &p), "2025-11-06 11/06/2025 November 6, 2025 Nov 6, 2025");
        assert_eq!(expand("@{vcsfileidentifier(\"board.kicad_sch\", 8)} @{vcsfileidentifier(\"nope\")} @{vcsfilecommitdate(\"nope\")} @{vcsfileauthor(\"board.kicad_sch\")}", &p), "ffffffff <unknown> <unknown> Ada");
        assert_eq!(expand("@{vcsauthor(1)}", &p), "@{vcsauthor(1)}", "the wrong number of arguments is no function");
        set_vcs(Some(Rc::new(FakeVcs { hash: String::new() })));
        assert_eq!(expand("${VCSHASH} ${VCSSHORTHASH}", &p), "no hash no hash", "outside a repository");
        set_vcs(before);
    }

    #[test]
    fn nesting_is_bounded_and_a_loop_ends() {
        let p = project(&[("A", "${B}"), ("B", "${A}")]);
        let shown = expand("${A}", &p);
        assert!(shown == "${A}" || shown == "${B}", "a loop is cut at the recursion depth, not followed forever: {shown}");
        let mut depth = 0;
        assert_eq!(resolve_text_vars("${NOPE}", &|_| None, &mut depth), "${NOPE}");
        assert_eq!(depth, MAX_DEPTH + 1, "an unresolved variable costs the passes up to the limit");
    }

    #[test]
    fn the_escapes_of_a_stored_text_are_undone_before_the_variables() {
        assert_eq!(unescape_string("a{slash}b{dblquote}c{brace}d{tab}e{space}f"), "a/b\"c{d\te f");
        assert_eq!(unescape_string("${REV}"), "${REV}", "a brace after a dollar is a variable's");
        assert_eq!(unescape_string("~{OVER}"), "~{OVER}", "an overbar is markup");
        assert_eq!(unescape_string("{unknown}"), "{unknown}");
        assert_eq!(unescape_string("{unterminated"), "{unterminated");
        assert_eq!(unescape_string("ab"), "ab");
        let p = project(&[("REV", "B")]);
        assert_eq!(expand("R{slash}${REV}", &p), "R/B");
    }

    #[test]
    fn the_title_block_answers_its_own_names_with_the_projects_variables_in_them() {
        let p = project(&[("TEAM", "Boards Inc")]);
        let tb = TitleBlockVars { title: "Main board".into(), date: "2026-01-02".into(), revision: "C".into(), company: "${TEAM}".into(), comments: vec!["first".into(), "".into(), "third".into()] };
        let ask = |t: &str| tb.resolve(t, Some(&p));
        assert_eq!(ask("TITLE").as_deref(), Some("Main board"));
        assert_eq!(ask("ISSUE_DATE").as_deref(), Some("2026-01-02"));
        assert_eq!(ask("REVISION").as_deref(), Some("C"));
        assert_eq!(ask("COMPANY").as_deref(), Some("Boards Inc"), "the project's variables are expanded in a title block value");
        assert_eq!(ask("COMMENT1").as_deref(), Some("first"));
        assert_eq!(ask("COMMENT2").as_deref(), Some(""));
        assert_eq!(ask("COMMENT3").as_deref(), Some("third"));
        assert_eq!(ask("COMMENT9").as_deref(), Some(""), "a comment the block lacks is empty, not unresolved");
        assert_eq!(ask("COMMENT0"), None);
        assert_eq!(ask("COMMENTS"), None);
        assert_eq!(ask("CURRENT_DATE").as_deref(), Some("2026-10-10"));
        assert_eq!(tb.shown(&p).company, "Boards Inc");
    }

    fn board() -> PcbBoard {
        let fp = |reference: &str, value: &str, layer: &str, extra: &[(&str, &str)]| PcbFootprint {
            layer: layer.into(),
            lib_nickname: "Resistor_SMD".into(),
            lib_item_name: "R_0603".into(),
            fields: [("Reference", reference, "F.SilkS"), ("Value", value, "F.Fab")].iter().copied().chain(extra.iter().map(|(n, t)| (*n, *t, "F.Fab"))).map(|(n, t, l)| PcbField { name: n.into(), text: t.into(), layer: l.into() }).collect(),
            pads: vec![
                PcbPad { number: "1".into(), net: "/Power/VCC".into(), net_class: "power".into(), pin_function: "".into() },
                PcbPad { number: "2".into(), net: "GND".into(), net_class: "Default".into(), pin_function: "".into() },
            ],
            dnp: reference == "R2",
            exclude_from_pos_files: reference == "R2",
            ..Default::default()
        };
        PcbBoard {
            project: project(&[("REV", "B")]),
            layer_names: Default::default(),
            file_name: "demo.kicad_pcb".into(),
            file_path: "/work/demo/.kicad/demo.kicad_pcb".into(),
            title_block: Some(TitleBlockVars { title: "Demo".into(), revision: "C".into(), ..Default::default() }),
            footprints: vec![fp("R1", "10k", "F.Cu", &[("MPN", "ERJ-3 ${VALUE}")]), fp("R2", "${REV}-22k", "B.Cu", &[])],
        }
    }

    #[test]
    fn a_board_text_sees_layer_the_title_block_the_project_and_other_footprints() {
        let b = board();
        let shown = |s: &str| pcb_text_shown(s, "F.SilkS", None, &b);
        assert_eq!(shown("${LAYER}"), "F.Silkscreen", "KiCad's name for the layer, not the one the file writes");
        assert_eq!(shown("${TITLE} ${REVISION} ${REV} ${FILENAME} ${PROJECTNAME}"), "Demo C B demo.kicad_pcb demo");
        assert_eq!(shown("${R1:VALUE} / ${r2:Value}"), "10k / B-22k", "a reference designator names a footprint, case aside, and a value is expanded in turn");
        assert_eq!(shown("${R9:VALUE}"), "<Unknown reference: R9>");
        assert_eq!(shown("${R1:NOFIELD}"), "<Unresolved: R1:NOFIELD>");
        assert_eq!(shown("${R1:NET_NAME(1)} ${R1:SHORT_NET_NAME(1)} ${R1:NET_CLASS(1)}"), "/Power/VCC VCC power");
        assert_eq!(shown("${REFERENCE}"), "${REFERENCE}", "a free text has no footprint to ask");
        assert_eq!(shown("${UNKNOWN}"), "${UNKNOWN}");
        assert_eq!(shown("\\${REV}"), "${REV}");
    }

    #[test]
    fn a_footprints_text_sees_its_own_fields_first() {
        let b = board();
        let r1 = &b.footprints[0];
        let shown = |s: &str, layer: &str| pcb_text_shown(s, layer, Some(r1), &b);
        assert_eq!(shown("${REFERENCE} ${VALUE}", "F.Fab"), "R1 10k");
        assert_eq!(shown("${LAYER}", "F.Fab"), "F.Fab", "a text's own layer wins over its footprint's");
        assert_eq!(shown("${LAYER}", "F.CrtYd"), "F.Courtyard");
        assert_eq!(shown("${FOOTPRINT_LIBRARY}:${FOOTPRINT_NAME}", "F.Fab"), "Resistor_SMD:R_0603");
        assert_eq!(shown("${MPN}", "F.Fab"), "ERJ-3 10k", "a user field, whose own text names the footprint's Value");
        assert_eq!(shown("${NET_NAME(2)} ${SHORT_NET_NAME(1)} ${NET_CLASS(1)}", "F.Fab"), "GND VCC power");
        // KiCad 25ec2335 `FOOTPRINT::ResolveTextVar`'s flags
        let r2 = &b.footprints[1];
        assert_eq!(r2.shown(&b, "[${DNP}] [${EXCLUDE_FROM_POS_FILES}] [${EXCLUDE_FROM_BOM}] [${EXCLUDE_FROM_BOARD}]", "B.Fab", 0), "[DNP] [Excluded from position files] [] []");
        assert_eq!(b.footprints[0].shown(&b, "[${DNP}]", "F.Fab", 0), "[]");
        assert_eq!(shown("${NET_NAME(9)}", "F.Fab"), "${NET_NAME(9)}", "no such pad: unresolved");
        assert_eq!(shown("${REV} ${TITLE}", "F.Fab"), "B Demo", "then the board and the project");
        // The field of a footprint, shown: its Value names a project variable.
        let r2 = &b.footprints[1];
        assert_eq!(r2.field_shown(&b, "Value", 0).as_deref(), Some("B-22k"));
        assert_eq!(r2.shown(&b, "${LAYER} ${REFERENCE}", "B.SilkS", 0), "B.Silkscreen R2", "the text's own layer, then the footprint's fields");
    }

    #[test]
    fn a_layer_the_user_renamed_shows_the_name_the_user_gave_it() {
        let mut b = board();
        assert_eq!(pcb_text_shown("${LAYER}", "In1.Cu", None, &b), "In1.Cu");
        assert_eq!(pcb_text_shown("${LAYER}", "User.7", None, &b), "User.7");
        b.layer_names.insert("F.Cu".into(), "Top".into());
        b.layer_names.insert("User.7".into(), "Assembly notes".into());
        b.layer_names.insert("F.SilkS".into(), String::new());
        assert_eq!(pcb_text_shown("${LAYER}", "User.7", None, &b), "Assembly notes");
        assert_eq!(pcb_text_shown("${LAYER}", "F.SilkS", None, &b), "F.Silkscreen", "an empty name clears the user's name (`SetLayerName`)");
        assert_eq!(b.footprints[0].shown(&b, "${LAYER}", "F.Fab", 0), "F.Fab");
        assert_eq!(pcb_text_shown("${R1:LAYER}", "F.SilkS", None, &b), "Top", "a footprint's own layer is its copper side, by the board's name for it");
        assert_eq!(standard_layer_name("Edge.Cuts"), "Edge.Cuts");
    }

    #[test]
    fn a_field_that_names_itself_ends_at_the_depth_limit() {
        let mut b = board();
        b.footprints[0].fields[1].text = "${VALUE}".into();
        let shown = b.footprints[0].field_shown(&b, "Value", 0).unwrap();
        assert!(shown.contains("VALUE"), "{shown}");
    }

    fn sheet() -> SchSheet {
        let sym = |reference: &str, value: &str| SchSymbol {
            fields: vec![("Reference".into(), reference.into()), ("Value".into(), value.into()), ("MPN".into(), "ERJ-${Value}".into())],
            reference: format!("{reference}A"),
            short_reference: reference.into(),
            unit_letter: "A".into(),
            lib_nickname: "Device".into(),
            lib_item_name: "R".into(),
            footprint: "Resistor_SMD:R_0603".into(),
            description: "Resistor".into(),
            dnp: reference == "R2",
            pins: vec![SchPin { number: "1".into(), name: "~".into(), net: "/VCC".into(), net_class: "power".into(), unit: 1, ..Default::default() }, SchPin { number: "2".into(), name: "~".into(), net: "".into(), net_class: "Default".into(), unit: 1, ..Default::default() }],
            ..Default::default()
        };
        SchSheet {
            project: project(&[("REV", "B")]),
            levels: vec![
                SchSheetLevel { fields: vec![], title_block: Some(TitleBlockVars { title: "Root".into(), company: "Acme".into(), ..Default::default() }) },
                SchSheetLevel { fields: vec![("Sheetname".into(), "Power".into()), ("Sheetfile".into(), "power.kicad_sch".into())], title_block: Some(TitleBlockVars { title: "Power stage".into(), ..Default::default() }) },
            ],
            page: "2".into(),
            page_count: 3,
            sheet_path: "/Power/".into(),
            sheet_name: "Power".into(),
            file_name: "demo.kicad_sch".into(),
            file_path: "/work/demo/.kicad/demo.kicad_sch".into(),
            symbols: vec![sym("R1", "10k"), sym("R2", "22k")],
        }
    }

    #[test]
    fn a_sheet_text_sees_its_sheets_fields_title_block_and_the_schematic() {
        let s = sheet();
        assert_eq!(s.text_shown("${#}/${##} ${SHEETPATH} ${SHEETNAME}"), "2/3 /Power/ Power");
        assert_eq!(s.text_shown("${TITLE}"), "Power stage", "the title block of the screen in view");
        assert_eq!(s.text_shown("${COMPANY}"), "", "a title block answers its own names, empty or not");
        assert_eq!(s.text_shown("${SHEETNAME} ${SHEETFILE}"), "Power power.kicad_sch", "a sheet's mandatory fields are named in capitals");
        assert_eq!(s.text_shown("${Sheetname}"), "${Sheetname}");
        assert_eq!(s.text_shown("${FILENAME} ${PROJECTNAME} ${REV}"), "demo.kicad_sch demo B");
        assert_eq!(s.text_shown("${R1:Value}"), "10k", "a cross reference by designator");
        assert_eq!(s.text_shown("${R1:MPN}"), "ERJ-10k", "a field whose text names another of the symbol's fields");
        assert_eq!(s.text_shown("${R9:Value}"), "<Unresolved: R9>");
        assert_eq!(s.text_shown("${R2:Nope}"), "<Unresolved: R2:Nope>");
        assert_eq!(s.text_shown("${DNP}"), "", "no symbol, no flag");
    }

    #[test]
    fn a_symbols_field_sees_the_symbol_first() {
        let s = sheet();
        let r1 = &s.symbols[0];
        let shown = |raw: &str, sym: &SchSymbol| s.field_shown(1, Some(sym), raw, 0);
        assert_eq!(shown("${Reference} ${REFERENCE} ${Value} ${value}", r1), "R1A R1A 10k 10k", "field names are not case sensitive");
        assert_eq!(shown("${SHORT_REFERENCE}${UNIT}", r1), "R1A");
        assert_eq!(shown("${MPN}", r1), "ERJ-10k");
        assert_eq!(shown("${FOOTPRINT_LIBRARY} ${FOOTPRINT_NAME} ${SYMBOL_LIBRARY} ${SYMBOL_NAME} ${SYMBOL_DESCRIPTION}", r1), "Resistor_SMD R_0603 Device R Resistor");
        assert_eq!(shown("[${DNP}] [${EXCLUDE_FROM_BOM}]", r1), "[] []");
        assert_eq!(shown("[${DNP}]", &s.symbols[1]), "[DNP]");
        assert_eq!(shown("${NET_NAME(1)} ${SHORT_NET_NAME(1)} ${NET_CLASS(1)} ${PIN_NAME(1)} ${SHORT_NET_NAME(2)} ${NET_NAME(2)}", r1), "/VCC VCC power ~ NC ");
        // KiCad 25ec2335: a pin set to an alternate is named by it; its own name and its alternates are asked for apart
        let mut alt = r1.clone();
        alt.pins[0] = SchPin { name: "PA0".into(), alt: "TIM2_CH1".into(), alternates: vec!["ADC_IN0".into(), "TIM2_CH1".into()], ..alt.pins[0].clone() };
        assert_eq!(shown("${PIN_NAME(1)} ${PIN_BASE_NAME(1)} [${PIN_ALT_LIST(1)}] [${PIN_ALT_LIST(2)}]", &alt), "TIM2_CH1 PA0 [ADC_IN0, TIM2_CH1] []");
        assert_eq!(shown("${NET_NAME(7)}", r1), "<Unresolved: pin 7>");
        assert_eq!(shown("${REFERENCE(2)} ${UNIT(2)}", r1), "R1A A");
        assert_eq!(shown("${SIM.DEVICE}.", r1), ".");
        assert_eq!(shown("${TITLE} ${REV} ${#}", r1), "Power stage B 2", "then the sheet, its title block and the project");
        let selfish = SchSymbol { fields: vec![("Footprint".into(), "${Footprint}".into())], ..Default::default() };
        assert_eq!(shown("${Value}|${Footprint}", &selfish), "Value|Footprint", "a field that only names itself ends the lookup: the token comes back as the word it is");
    }

    #[test]
    fn the_clock_reads_a_browsers_local_time_and_names_the_utc_day_otherwise() {
        let c = Clock::from_local("2026-10-10T21:30:05").unwrap();
        assert_eq!((c.date.as_str(), c.time_hms.as_str(), c.time_locale.as_str()), ("2026-10-10", "21h30m05s", "21:30:05"));
        assert_eq!(Clock::from_local("2026-10-10T21:30:05.123Z").unwrap().time_locale, "21:30:05");
        assert!(Clock::from_local("yesterday").is_none() && Clock::from_local("2026-10-10").is_none() && Clock::from_local("x-y-zTa:b:c").is_none());
        let now = Clock::utc_now();
        assert_eq!(now.date.len(), 10);
        assert!(now.date.starts_with("20"), "{}", now.date);
    }

    #[test]
    fn unit_letters_count_like_a_spreadsheet() {
        let s = SchSymbol::default();
        assert_eq!((s.unit_letter_of(1), s.unit_letter_of(2), s.unit_letter_of(26), s.unit_letter_of(27), s.unit_letter_of(0)), ("A".into(), "B".into(), "Z".into(), "AA".into(), String::new()));
        // a unit's display name in place of its letter (r6 B6-H1)
        let named = SchSymbol { unit_names: [(2, "Eqep2".to_string())].into(), ..Default::default() };
        assert_eq!((named.unit_letter_of(1), named.unit_letter_of(2)), ("A".into(), "Eqep2".into()));
    }
}
