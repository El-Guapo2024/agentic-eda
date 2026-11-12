//! The one regular-expression engine of the port, for what KiCad hands to `wxRegEx`: the net class patterns (`EDA_PATTERN_MATCH_REGEX` /
//! `EDA_PATTERN_MATCH_REGEX_ANCHORED`, common/eda_pattern_match.cpp, compiled with `wxRE_ADVANCED`; [`crate::net_pattern`]) and Find / Replace's "Regular
//! Expression" (`EDA_SEARCH_MATCH_MODE::REGEX`, common/eda_item.cpp `EDA_ITEM::Matches` / `Replace`, compiled with `wxRE_DEFAULT` or `wxRE_ICASE`;
//! `eda_ops::search`). No regex crate is taken (no new dependency): this is a port of the part of wxRegEx's syntax a net name or a schematic text is matched
//! with.
//!
//! The syntax: literals, `.` (any character), `[..]` classes (ranges, `^` negation, `[:alpha:]` `[:digit:]` `[:alnum:]` `[:upper:]` `[:lower:]` `[:space:]`
//! `[:punct:]` `[:xdigit:]`, the `\d \w \s` shorthands and their capitals), `( .. )` groups (numbered for a replacement's `\N`) and `(?: .. )` ones, `|`, `^`
//! and `$` (the text's ends), and `* + ? {m} {m,} {m,n}`, each lazy with a `?` after it; `\n`, `\t` and an escaped punctuation character stand for
//! themselves. [`Flavor::Find`] also takes `\b` and `\B` (a word's edge, and not one). A pattern with anything else -- a back reference, a look-ahead, a
//! named group, another letter escaped, a `{` that is no count, a second quantifier on one atom, a class or range that does not close -- does not compile
//! (`wxRegEx::Compile` returns false: a net class pattern then reads as a wildcard pattern only, a Find matches nothing).
//!
//! Matching is by simulation of the machine the pattern compiles to (Thompson's construction, threads in priority order as Pike's), so no pattern takes longer
//! than its length times the text's -- a pattern made to make a backtracking matcher blow up (`(a+)+$`) does not -- and a program of more than 4000
//! instructions or groups nested deeper than 64 does not compile.
//!
//! What wxRegEx is (UI-LEFT-2 SS10): wxWidgets 3.2 compiles every pattern with PCRE2 (`src/common/regex.cpp` of 3.2.4, the version on this box, linked to
//! `libpcre2-32`; the old Henry Spencer library is gone since 3.1.6): `wx_regcomp` gives `PCRE2_UTF | PCRE2_ALT_BSUX | PCRE2_DOTALL`, plus `PCRE2_CASELESS`
//! for `wxRE_ICASE`, and `wxRE_EXTENDED` (KiCad's Find, flags `0` or `wxRE_ICASE`) changes nothing in the pattern, while `wxRE_ADVANCED` (the net class
//! patterns) only turns Tcl's `\m \M \y \Y` into PCRE's. So a match is PCRE's *leftmost-first* one, as this machine's (alternatives tried left to right, a
//! greedy repeat taking as many as it can before the rest is tried: `a|ab` on `ab` is `a`, which `libpcre2-32` answers too), not POSIX's leftmost-longest.
//! [`Flavor::Find`] reads the rest the way PCRE2 does without `PCRE2_UCP`: `\d \w \s`, the POSIX classes and a word's edge are ASCII's (`é` is no `\w`, as
//! in KiCad), the classes `[:blank:]` `[:cntrl:]` `[:print:]` `[:graph:]` `[:word:]` `[:ascii:]` are read, `\h \H \v \V` (horizontal and vertical space) and
//! `\N` (anything but a line feed), `\a \e \f \r` and `PCRE2_ALT_BSUX`'s `\xhh` (two hex digits, else the letter `x`), `\uhhhh` (four, else `u`) and `\U`
//! (the letter `U`), `\A`, `\z` and `\Z`; `$` (and `\Z`) also holds before a line feed that ends the text. A class name in capitals (`[:ALPHA:]`) and a letter
//! PCRE2 does not know escaped (`\T`, `\q`) do not compile, as `pcre2_compile` refuses them. Find's "not case-sensitive" upper-cases the pattern before it
//! compiles (`EDA_ITEM::Matches`, `eda_ops::search`), so these rules are what make `\d` read as `\D` there.
//!
//! **Differs**: PCRE2's back references, look-arounds, named groups, inline options, `\Q..\E`, `\p{..}`, `\X`, `\R`, `\K`, `\G`, `\c`, `\o{..}`
//! and atomic or possessive forms do not compile here (no Find of KiCad's needs them; a pattern with one matches nothing); the net class flavour keeps the
//! syntax it had (Unicode classes, no new escapes), what the INACTIVE engine of `crate::net_pattern` is tested against.

/// The most instructions a pattern may compile to (the position marks of its groups not counted): counted repeats are copied, so `(a{100}){100}` would be ten
/// thousand. Over this, the pattern does not compile.
const MAX_PROGRAM: usize = 4_000;
/// The deepest nesting of groups a pattern may have.
const MAX_DEPTH: usize = 64;

/// Which of KiCad's uses the pattern is for: the syntax differs in one point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// A net class pattern (`EDA_PATTERN_MATCH_REGEX`): a word boundary does not compile (`\b` is no boundary in wxWidgets' advanced syntax).
    NetClass,
    /// Find / Replace's Regular Expression: `\b` and `\B` are a word's edge and not one.
    Find,
    /// The library choosers' `EDA_PATTERN_MATCH_REGEX` (common/eda_pattern_match.cpp, `wxRE_ADVANCED`; UI-LEFT-2 LL4): what wxWidgets 3.2's `wxRegExImpl::Compile`
    /// hands PCRE2 for that flag -- [`Flavor::Find`]'s syntax, after `ConvertMetasyntax` (a leading `***:` director dropped, `***=` making the rest a literal, a
    /// leading embedded-options group `(?..)`) and `ConvertWordBoundaries` (Tcl's `\m` `\M` `\y` `\Y` as PCRE's `[[:<:]]` `[[:>:]]` `\b` `\B`), so `[[:<:]]` and
    /// `[[:>:]]` (a word's start and end) are read too. See [`advanced_metasyntax`] for the options read.
    Advanced,
}

/// `ConvertMetasyntax` and `ConvertWordBoundaries` of wxWidgets 3.2.4 (`src/common/regex.cpp`) for `wxRE_ADVANCED`: the pattern PCRE2 compiles and whether it is
/// caseless. A leading `***:` is dropped; `***=` makes the rest a literal (wxWidgets turns it into `\Q`: here every character that is no letter or digit is
/// escaped); a leading `(?opts)` of lower-case letters sets `i` (caseless) and `c` (case-sensitive), `s` (`PCRE2_DOTALL`, already on), `e` (extended syntax:
/// no word-boundary conversion) and `q` (the rest a literal). **Differs**: the other letters (`b` basic syntax, `m` `n` `p` `w` newline handling, `x` `t`
/// extended spacing, and any letter PCRE2 would take) make the pattern not compile here.
pub fn advanced_metasyntax(pattern: &str, icase: bool) -> Option<(String, bool)> {
    let literal = |rest: &str| -> String {
        let mut out = String::new();
        for c in rest.chars() {
            if !c.is_alphanumeric() {
                out.push('\\');
            }
            out.push(c);
        }
        out
    };
    let mut expr = pattern.to_string();
    let mut icase = icase;
    let mut advanced = true;
    if expr.chars().count() > 3 && expr.starts_with("***") {
        match expr[3..].chars().next() {
            Some(':') => expr = expr[4..].to_string(),
            Some('=') => return Some((literal(&expr[4..]), icase)),
            _ => {}
        }
    }
    if expr.starts_with("(?") {
        let body: String = expr[2..].chars().take_while(|c| c.is_ascii_lowercase()).collect();
        let after = 2 + body.len();
        if expr[after..].starts_with(')') {
            let mut quoted = false;
            for c in body.chars() {
                match c {
                    'i' => icase = true,
                    'c' => icase = false,
                    's' => {}
                    'e' => advanced = false,
                    'q' => quoted = true,
                    _ => return None,
                }
            }
            let rest = expr[after + 1..].to_string();
            if quoted {
                return Some((literal(&rest), icase));
            }
            expr = rest;
        }
    }
    if !advanced {
        return Some((expr, icase));
    }
    // `ConvertWordBoundaries`: an escaped pair is read as a pair, so `\\y` stays a backslash and a `y`
    let mut out = String::new();
    let mut it = expr.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            None => {
                out.push('\\');
                break;
            }
            Some('m') => out.push_str("[[:<:]]"),
            Some('M') => out.push_str("[[:>:]]"),
            Some('y') => out.push_str("\\b"),
            Some('Y') => out.push_str("\\B"),
            Some(e) => {
                out.push('\\');
                out.push(e);
            }
        }
    }
    Some((out, icase))
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Char(char),
    Any,
    Class { items: Vec<ClassItem>, negate: bool },
    Bol,
    Eol,
    /// `\\z`: the text's very end (where `$` also holds before a final line feed).
    EndOnly,
    WordBoundary(bool),
    /// PCRE2's `[[:<:]]` (true) and `[[:>:]]` (false): a word starts, or ends, here ([`Flavor::Advanced`]).
    WordEdge(bool),
    Group(Box<Node>, usize),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Repeat { node: Box<Node>, min: usize, max: Option<usize>, greedy: bool },
}

#[derive(Debug, Clone, PartialEq)]
enum ClassItem {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
    Posix(PosixClass),
    /// PCRE2's `\h` / `\H` (horizontal white space) and `\v` / `\V` (vertical), Find's only.
    HSpace(bool),
    VSpace(bool),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PosixClass {
    Alpha,
    Digit,
    Alnum,
    Upper,
    Lower,
    Space,
    Punct,
    Xdigit,
    /// PCRE2's other classes (SS10): `[:blank:]` (space, tab), `[:cntrl:]`, `[:print:]`, `[:graph:]`, `[:word:]` (`\w`), `[:ascii:]`.
    Blank,
    Cntrl,
    Print,
    Graph,
    Word,
    Ascii,
}

/// Whether `c` is in the class item `i`. `ascii`: the classes PCRE2 reads without `PCRE2_UCP` ([`Flavor::Find`]: ASCII's letters, digits and space only);
/// else the Unicode ones the net class flavour has always used.
fn item_matches(i: &ClassItem, c: char, ascii: bool) -> bool {
    match *i {
        ClassItem::Range(a, b) => a <= c && c <= b,
        ClassItem::Digit(yes) => c.is_ascii_digit() == yes,
        ClassItem::Word(yes) => word_char(c, ascii) == yes,
        ClassItem::Space(yes) => space_char(c, ascii) == yes,
        ClassItem::HSpace(yes) => matches!(c, '\t' | ' ' | '\u{a0}' | '\u{1680}' | '\u{180e}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}') == yes,
        ClassItem::VSpace(yes) => matches!(c, '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}') == yes,
        ClassItem::Posix(p) => match p {
            PosixClass::Alpha => if ascii { c.is_ascii_alphabetic() } else { c.is_alphabetic() },
            PosixClass::Digit => c.is_ascii_digit(),
            PosixClass::Alnum => if ascii { c.is_ascii_alphanumeric() } else { c.is_alphanumeric() },
            PosixClass::Upper => if ascii { c.is_ascii_uppercase() } else { c.is_uppercase() },
            PosixClass::Lower => if ascii { c.is_ascii_lowercase() } else { c.is_lowercase() },
            PosixClass::Space => space_char(c, ascii),
            PosixClass::Punct => c.is_ascii_punctuation(),
            PosixClass::Xdigit => c.is_ascii_hexdigit(),
            PosixClass::Blank => c == ' ' || c == '\t',
            PosixClass::Cntrl => (c as u32) < 0x20 || c == '\u{7f}',
            PosixClass::Print => (' '..='~').contains(&c),
            PosixClass::Graph => ('!'..='~').contains(&c),
            PosixClass::Word => word_char(c, ascii),
            PosixClass::Ascii => c.is_ascii(),
        },
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `\w`: ASCII's letters, digits and `_` for PCRE2 (`ascii`), any letter or digit otherwise.
fn word_char(c: char, ascii: bool) -> bool {
    if ascii {
        c.is_ascii_alphanumeric() || c == '_'
    } else {
        is_word(c)
    }
}

/// `\s`: PCRE2's space, tab, line feed, vertical tab, form feed and carriage return (`ascii`), any white space otherwise.
fn space_char(c: char, ascii: bool) -> bool {
    if ascii {
        matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
    } else {
        c.is_whitespace()
    }
}

fn class_matches(items: &[ClassItem], negate: bool, c: char, icase: bool, ascii: bool) -> bool {
    let test = |c: char| items.iter().any(|i| item_matches(i, c, ascii));
    let hit = test(c) || (icase && (c.to_lowercase().any(test) || c.to_uppercase().any(test)));
    hit != negate
}

fn same_char(a: char, b: char, icase: bool) -> bool {
    a == b || (icase && a.to_lowercase().eq(b.to_lowercase()))
}

impl Flavor {
    /// The flavours that read PCRE2's syntax (see the module doc).
    fn pcre(self) -> bool {
        matches!(self, Flavor::Find | Flavor::Advanced)
    }
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    depth: usize,
    groups: usize,
    flavor: Flavor,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn alt(&mut self) -> Option<Node> {
        let mut branches = vec![self.seq()?];
        while self.eat('|') {
            branches.push(self.seq()?);
        }
        Some(if branches.len() == 1 { branches.pop().unwrap() } else { Node::Alt(branches) })
    }

    fn seq(&mut self) -> Option<Node> {
        let mut items = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            items.push(self.repeat()?);
        }
        Some(Node::Seq(items))
    }

    fn repeat(&mut self) -> Option<Node> {
        let atom = self.atom()?;
        let (min, max) = match self.peek() {
            Some('*') => (0, None),
            Some('+') => (1, None),
            Some('?') => (0, Some(1)),
            Some('{') => {
                // `{m}`, `{m,}`, `{m,n}`; a `{` that is no count is an error (wxRegEx refuses it too)
                let close = self.chars[self.pos..].iter().position(|&c| c == '}')? + self.pos;
                let body: String = self.chars[self.pos + 1..close].iter().collect();
                let (lo, hi) = match body.split_once(',') {
                    None => (body.parse::<usize>().ok()?, Some(body.parse::<usize>().ok()?)),
                    Some((lo, "")) => (lo.parse::<usize>().ok()?, None),
                    Some((lo, hi)) => (lo.parse::<usize>().ok()?, Some(hi.parse::<usize>().ok()?)),
                };
                if hi.is_some_and(|h| h < lo) || lo > 255 || hi.is_some_and(|h| h > 255) {
                    return None;
                }
                self.pos = close;
                (lo, hi)
            }
            _ => return Some(atom),
        };
        self.pos += 1;
        // a `?` after the quantifier makes it lazy: it takes as few as it can before the rest is tried
        let greedy = !self.eat('?');
        if matches!(self.peek(), Some('*' | '+' | '{')) {
            return None; // a second quantifier on the same atom
        }
        Some(Node::Repeat { node: Box::new(atom), min, max, greedy })
    }

    fn atom(&mut self) -> Option<Node> {
        let c = self.peek()?;
        self.pos += 1;
        match c {
            '(' => {
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return None;
                }
                let capture = if self.eat('?') {
                    if !self.eat(':') {
                        return None; // look-ahead, named groups and flags are not read
                    }
                    None
                } else {
                    self.groups += 1;
                    Some(self.groups)
                };
                let inner = self.alt()?;
                if !self.eat(')') {
                    return None;
                }
                self.depth -= 1;
                Some(match capture {
                    Some(n) => Node::Group(Box::new(inner), n),
                    None => inner,
                })
            }
            ')' | '*' | '+' | '?' | '{' => None,
            '.' => Some(Node::Any),
            '^' => Some(Node::Bol),
            '$' => Some(Node::Eol),
            '[' if self.flavor == Flavor::Advanced && self.chars[self.pos..].starts_with(&['[', ':', '<', ':', ']', ']']) => {
                self.pos += 6;
                Some(Node::WordEdge(true))
            }
            '[' if self.flavor == Flavor::Advanced && self.chars[self.pos..].starts_with(&['[', ':', '>', ':', ']', ']']) => {
                self.pos += 6;
                Some(Node::WordEdge(false))
            }
            '[' => self.class(),
            '\\' => {
                let e = self.peek()?;
                self.pos += 1;
                if self.flavor.pcre() {
                    return self.pcre_escape(e);
                }
                match e {
                    'd' | 'D' | 'w' | 'W' | 's' | 'S' => Some(Node::Class { items: vec![shorthand(e)], negate: false }),
                    'n' => Some(Node::Char('\n')),
                    't' => Some(Node::Char('\t')),
                    c if c.is_ascii_digit() || c == 'b' || c == 'B' || c == 'A' || c == 'Z' || c == '<' || c == '>' || c == 'y' || c == 'm' || c == 'M' => None, // back references, word boundaries
                    c if c.is_ascii_alphabetic() => None,
                    c => Some(Node::Char(c)),
                }
            }
            c => Some(Node::Char(c)),
        }
    }

    /// `n` hex digits at the parse position, read as one character (`PCRE2_ALT_BSUX`'s `\\xhh` and `\\uhhhh`); `None`, the position kept, when they are not there.
    fn hex_char(&mut self, n: usize) -> Option<char> {
        let digits: String = self.chars.get(self.pos..self.pos + n)?.iter().collect();
        if digits.len() != n || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let c = char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?;
        self.pos += n;
        Some(c)
    }

    /// One escaped character outside a class as PCRE2 reads it ([`Flavor::Find`], see the module doc); `None` for an escape that does not compile.
    fn pcre_escape(&mut self, e: char) -> Option<Node> {
        let class = |item: ClassItem| Some(Node::Class { items: vec![item], negate: false });
        match e {
            'd' | 'D' | 'w' | 'W' | 's' | 'S' => class(shorthand(e)),
            'h' | 'H' => class(ClassItem::HSpace(e == 'h')),
            'v' | 'V' => class(ClassItem::VSpace(e == 'v')),
            // `\N`: any character but a line feed
            'N' => Some(Node::Class { items: vec![ClassItem::Range('\n', '\n')], negate: true }),
            'b' => Some(Node::WordBoundary(true)),
            'B' => Some(Node::WordBoundary(false)),
            'A' => Some(Node::Bol),
            'Z' => Some(Node::Eol),
            'z' => Some(Node::EndOnly),
            'x' => Some(Node::Char(self.hex_char(2).unwrap_or('x'))),
            'u' => Some(Node::Char(self.hex_char(4).unwrap_or('u'))),
            'U' => Some(Node::Char('U')),
            c => simple_escape(c).map(Node::Char),
        }
    }

    fn class(&mut self) -> Option<Node> {
        let negate = self.eat('^');
        let mut items: Vec<ClassItem> = Vec::new();
        let mut first = true;
        loop {
            let c = self.peek()?;
            self.pos += 1;
            if c == ']' && !first {
                break;
            }
            first = false;
            let lo = match c {
                '[' if self.peek() == Some(':') => {
                    let close = (self.pos + 1..self.chars.len().saturating_sub(1)).find(|&i| self.chars[i] == ':' && self.chars[i + 1] == ']')?;
                    let name: String = self.chars[self.pos + 1..close].iter().collect();
                    self.pos = close + 2;
                    let p = match name.as_str() {
                        "alpha" => PosixClass::Alpha,
                        "digit" => PosixClass::Digit,
                        "alnum" => PosixClass::Alnum,
                        "upper" => PosixClass::Upper,
                        "lower" => PosixClass::Lower,
                        "space" => PosixClass::Space,
                        "punct" => PosixClass::Punct,
                        "xdigit" => PosixClass::Xdigit,
                        "blank" if self.flavor.pcre() => PosixClass::Blank,
                        "cntrl" if self.flavor.pcre() => PosixClass::Cntrl,
                        "print" if self.flavor.pcre() => PosixClass::Print,
                        "graph" if self.flavor.pcre() => PosixClass::Graph,
                        "word" if self.flavor.pcre() => PosixClass::Word,
                        "ascii" if self.flavor.pcre() => PosixClass::Ascii,
                        _ => return None,
                    };
                    items.push(ClassItem::Posix(p));
                    continue;
                }
                '\\' => {
                    let e = self.peek()?;
                    self.pos += 1;
                    match e {
                        'd' | 'D' | 'w' | 'W' | 's' | 'S' => {
                            items.push(shorthand(e));
                            continue;
                        }
                        'h' | 'H' | 'v' | 'V' if self.flavor.pcre() => {
                            items.push(if e.eq_ignore_ascii_case(&'h') { ClassItem::HSpace(e == 'h') } else { ClassItem::VSpace(e == 'v') });
                            continue;
                        }
                        'n' => '\n',
                        't' => '\t',
                        // in a class PCRE2's `\b` is a backspace
                        'b' if self.flavor.pcre() => '\u{8}',
                        'x' if self.flavor.pcre() => self.hex_char(2).unwrap_or('x'),
                        'u' if self.flavor.pcre() => self.hex_char(4).unwrap_or('u'),
                        'U' if self.flavor.pcre() => 'U',
                        e if self.flavor.pcre() && simple_escape(e).is_some() => simple_escape(e)?,
                        e if e.is_ascii_alphanumeric() => return None,
                        e => e,
                    }
                }
                c => c,
            };
            // a range `a-z` (a `-` that ends the class or starts it is a literal)
            if self.peek() == Some('-') && self.chars.get(self.pos + 1).is_some_and(|&n| n != ']') {
                self.pos += 1;
                let mut hi = self.peek()?;
                self.pos += 1;
                if hi == '\\' {
                    hi = self.peek()?;
                    self.pos += 1;
                }
                if hi < lo {
                    return None;
                }
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(ClassItem::Range(lo, lo));
            }
        }
        Some(Node::Class { items, negate })
    }
}

/// The escapes PCRE2 reads as one character: `\\a \\e \\f \\n \\r \\t`, and a character that is no letter or digit for itself; `None` for any other letter or digit.
fn simple_escape(c: char) -> Option<char> {
    match c {
        'a' => Some('\u{7}'),
        'e' => Some('\u{1b}'),
        'f' => Some('\u{c}'),
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        c if c.is_ascii_alphanumeric() => None,
        c => Some(c),
    }
}

fn shorthand(c: char) -> ClassItem {
    match c {
        'd' => ClassItem::Digit(true),
        'D' => ClassItem::Digit(false),
        'w' => ClassItem::Word(true),
        'W' => ClassItem::Word(false),
        's' => ClassItem::Space(true),
        _ => ClassItem::Space(false),
    }
}

// ------------------------------------------------------------------------------------------------------------------------------------ the machine

#[derive(Debug, Clone)]
enum Inst {
    Char(char),
    Any,
    Class(usize),
    /// Two ways on, the first preferred.
    Split(usize, usize),
    Jmp(usize),
    Bol,
    Eol,
    EndOnly,
    WordBoundary(bool),
    WordEdge(bool),
    /// The text position at this point is kept in capture slot `n` (a group's start is `2 * g`, its end `2 * g + 1`).
    Save(usize),
    Match,
}

/// A compiled pattern.
#[derive(Debug, Clone)]
pub struct Regex {
    program: Vec<Inst>,
    classes: Vec<(Vec<ClassItem>, bool)>,
    /// The groups numbered for `\N` (group 0 is the whole match).
    groups: usize,
    icase: bool,
    /// The classes are PCRE2's ASCII ones ([`Flavor::Find`]).
    ascii: bool,
    /// The instructions counted against [`MAX_PROGRAM`].
    counted: usize,
}

/// One match: where it starts and ends in the text's characters, and where each group did (`None` for a group that took no part).
#[derive(Debug, Clone, PartialEq)]
pub struct Captures {
    slots: Vec<Option<usize>>,
}

impl Captures {
    /// The span of group `g` (0: the whole match), in characters.
    pub fn group(&self, g: usize) -> Option<(usize, usize)> {
        Some((self.slots.get(2 * g).copied()??, self.slots.get(2 * g + 1).copied()??))
    }
}

impl Regex {
    /// `wxRegEx::Compile( pattern, icase ? wxRE_ICASE : .. )` for `flavor`; `None` when the pattern does not compile (see the module doc).
    pub fn compile(pattern: &str, icase: bool, flavor: Flavor) -> Option<Regex> {
        let (converted, icase) = if flavor == Flavor::Advanced { advanced_metasyntax(pattern, icase)? } else { (pattern.to_string(), icase) };
        let mut p = Parser { chars: converted.chars().collect(), pos: 0, depth: 0, groups: 0, flavor };
        let ast = p.alt()?;
        if p.pos != p.chars.len() {
            return None; // an unmatched `)`
        }
        let mut r = Regex { program: Vec::new(), classes: Vec::new(), groups: p.groups, icase, ascii: flavor.pcre(), counted: 0 };
        r.push(Inst::Save(0))?;
        r.emit(&ast)?;
        r.push(Inst::Save(1))?;
        r.push(Inst::Match)?;
        Some(r)
    }

    fn push(&mut self, i: Inst) -> Option<usize> {
        if !matches!(i, Inst::Save(_)) {
            if self.counted >= MAX_PROGRAM {
                return None;
            }
            self.counted += 1;
        }
        self.program.push(i);
        Some(self.program.len() - 1)
    }

    fn emit(&mut self, n: &Node) -> Option<()> {
        match n {
            Node::Char(c) => {
                self.push(Inst::Char(*c))?;
            }
            Node::Any => {
                self.push(Inst::Any)?;
            }
            Node::Class { items, negate } => {
                self.classes.push((items.clone(), *negate));
                let idx = self.classes.len() - 1;
                self.push(Inst::Class(idx))?;
            }
            Node::Bol => {
                self.push(Inst::Bol)?;
            }
            Node::Eol => {
                self.push(Inst::Eol)?;
            }
            Node::EndOnly => {
                self.push(Inst::EndOnly)?;
            }
            Node::WordBoundary(yes) => {
                self.push(Inst::WordBoundary(*yes))?;
            }
            Node::WordEdge(start) => {
                self.push(Inst::WordEdge(*start))?;
            }
            Node::Group(inner, g) => {
                self.push(Inst::Save(2 * g))?;
                self.emit(inner)?;
                self.push(Inst::Save(2 * g + 1))?;
            }
            Node::Seq(items) => {
                for i in items {
                    self.emit(i)?;
                }
            }
            Node::Alt(branches) => {
                // split L1, L2; L1: a; jmp end; L2: split ... -- the earlier branch preferred
                let mut jumps = Vec::new();
                for (k, b) in branches.iter().enumerate() {
                    if k + 1 < branches.len() {
                        let split = self.push(Inst::Split(0, 0))?;
                        self.emit(b)?;
                        jumps.push(self.push(Inst::Jmp(0))?);
                        let next = self.program.len();
                        self.program[split] = Inst::Split(split + 1, next);
                    } else {
                        self.emit(b)?;
                    }
                }
                let end = self.program.len();
                for j in jumps {
                    self.program[j] = Inst::Jmp(end);
                }
            }
            Node::Repeat { node, min, max, greedy } => {
                let split = |body: usize, out: usize| if *greedy { Inst::Split(body, out) } else { Inst::Split(out, body) };
                for _ in 0..*min {
                    self.emit(node)?;
                }
                match max {
                    None => {
                        // L: split body, end; body; jmp L
                        let at = self.push(Inst::Split(0, 0))?;
                        self.emit(node)?;
                        self.push(Inst::Jmp(at))?;
                        let end = self.program.len();
                        self.program[at] = split(at + 1, end);
                    }
                    Some(max) => {
                        // (max - min) optional copies, each skipping the rest of them
                        let mut splits = Vec::new();
                        for _ in *min..*max {
                            splits.push(self.push(Inst::Split(0, 0))?);
                            self.emit(node)?;
                        }
                        let end = self.program.len();
                        for s in splits {
                            self.program[s] = split(s + 1, end);
                        }
                    }
                }
            }
        }
        Some(())
    }

    /// Whether some match starts at the first character of `text`: the machine runs from position 0 and accepts as soon as one of its threads reaches the match
    /// state, wherever that is (`wxRegEx::Matches` plus `GetMatch`'s start, which `EDA_COMBINED_MATCHER::StartsWith` requires to be 0) -- the net class question.
    pub fn matches_from_start(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        let mut current: Vec<Thread> = Vec::new();
        let mut seen = vec![usize::MAX; self.program.len()];
        self.add(&mut current, Thread { pc: 0, slots: Vec::new() }, 0, &chars, &mut seen, false);
        for i in 0..=chars.len() {
            if current.iter().any(|t| matches!(self.program[t.pc], Inst::Match)) {
                return true;
            }
            if i == chars.len() || current.is_empty() {
                return false;
            }
            current = self.step(&current, i, &chars, &mut seen, false);
        }
        false
    }

    /// The leftmost-first match at or after character `from` of `chars` (`^` holds at the text's start only), with its groups.
    pub fn find_at(&self, chars: &[char], from: usize) -> Option<Captures> {
        let width = 2 * (self.groups + 1);
        let mut current: Vec<Thread> = Vec::new();
        let mut seen = vec![usize::MAX; self.program.len()];
        let mut best: Option<Vec<Option<usize>>> = None;
        for i in from..=chars.len() {
            // a new thread starts here while no match has been found: a match that starts earlier is preferred (`addthread` of the start at each step)
            if best.is_none() {
                self.add(&mut current, Thread { pc: 0, slots: vec![None; width] }, i, chars, &mut seen, true);
            }
            // the highest-priority thread at the match state is the match so far; the threads after it are of lower priority and stop
            if let Some(k) = current.iter().position(|t| matches!(self.program[t.pc], Inst::Match)) {
                best = Some(current[k].slots.clone());
                current.truncate(k);
            }
            if i == chars.len() || (current.is_empty() && best.is_some()) {
                break;
            }
            current = self.step(&current, i, chars, &mut seen, true);
        }
        best.map(|slots| Captures { slots })
    }

    /// Every thread of `current` that reads the character at `i`, moved on past it (in priority order), at the next position's stamp.
    fn step(&self, current: &[Thread], i: usize, chars: &[char], seen: &mut [usize], keep: bool) -> Vec<Thread> {
        let c = chars[i];
        let mut next: Vec<Thread> = Vec::new();
        for t in current {
            let advance = match &self.program[t.pc] {
                Inst::Char(x) => same_char(*x, c, self.icase),
                Inst::Any => true,
                Inst::Class(k) => class_matches(&self.classes[*k].0, self.classes[*k].1, c, self.icase, self.ascii),
                _ => false,
            };
            if advance {
                self.add(&mut next, Thread { pc: t.pc + 1, slots: t.slots.clone() }, i + 1, chars, seen, keep);
            }
        }
        next
    }

    /// Add the thread and everything it reaches without reading a character, at position `pos` (each instruction once per position: `seen` is stamped with it,
    /// the first thread to reach one being the preferred). `keep` records the groups' positions.
    fn add(&self, list: &mut Vec<Thread>, t: Thread, pos: usize, chars: &[char], seen: &mut [usize], keep: bool) {
        if seen[t.pc] == pos {
            return;
        }
        seen[t.pc] = pos;
        match &self.program[t.pc] {
            Inst::Jmp(to) => self.add(list, Thread { pc: *to, slots: t.slots }, pos, chars, seen, keep),
            Inst::Split(a, b) => {
                self.add(list, Thread { pc: *a, slots: t.slots.clone() }, pos, chars, seen, keep);
                self.add(list, Thread { pc: *b, slots: t.slots }, pos, chars, seen, keep);
            }
            Inst::Bol => {
                if pos == 0 {
                    self.add(list, Thread { pc: t.pc + 1, slots: t.slots }, pos, chars, seen, keep);
                }
            }
            // PCRE2's `$` (no `PCRE2_DOLLAR_ENDONLY`, no `PCRE2_MULTILINE`): the end, or before a line feed that ends the text
            Inst::Eol => {
                if pos == chars.len() || (pos + 1 == chars.len() && chars[pos] == '\n') {
                    self.add(list, Thread { pc: t.pc + 1, slots: t.slots }, pos, chars, seen, keep);
                }
            }
            Inst::EndOnly => {
                if pos == chars.len() {
                    self.add(list, Thread { pc: t.pc + 1, slots: t.slots }, pos, chars, seen, keep);
                }
            }
            Inst::WordBoundary(want) => {
                let before = pos > 0 && word_char(chars[pos - 1], self.ascii);
                let after = pos < chars.len() && word_char(chars[pos], self.ascii);
                if (before != after) == *want {
                    self.add(list, Thread { pc: t.pc + 1, slots: t.slots }, pos, chars, seen, keep);
                }
            }
            // `[[:<:]]`: a word character follows and none precedes; `[[:>:]]`: the other way round
            Inst::WordEdge(start) => {
                let before = pos > 0 && word_char(chars[pos - 1], self.ascii);
                let after = pos < chars.len() && word_char(chars[pos], self.ascii);
                if (after && !before && *start) || (before && !after && !*start) {
                    self.add(list, Thread { pc: t.pc + 1, slots: t.slots }, pos, chars, seen, keep);
                }
            }
            Inst::Save(n) => {
                let mut slots = t.slots;
                if keep {
                    slots[*n] = Some(pos);
                }
                self.add(list, Thread { pc: t.pc + 1, slots }, pos, chars, seen, keep);
            }
            _ => list.push(t),
        }
    }

    /// `wxRegEx::Matches( text )`: is there a match anywhere in the text?
    pub fn is_match(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        self.find_at(&chars, 0).is_some()
    }

    /// `wxRegEx::Replace( &text, replacement )`: every match replaced, left to right (an empty match moves on by one character); `\N` in the replacement is group
    /// N, `&` and `\0` the whole match, `\&` and `\\` the character itself. `None` when nothing matched.
    pub fn replace_all(&self, text: &str, replacement: &str) -> Option<String> {
        let t: Vec<char> = text.chars().collect();
        let r: Vec<char> = replacement.chars().collect();
        let mut out = String::new();
        let mut at = 0;
        let mut n = 0;
        while at <= t.len() {
            let Some(caps) = self.find_at(&t, at) else { break };
            let (s, e) = caps.group(0).expect("a match has a span");
            out.extend(&t[at..s]);
            let mut k = 0;
            while k < r.len() {
                match r[k] {
                    '\\' if k + 1 < r.len() && r[k + 1].is_ascii_digit() => {
                        let g = r[k + 1].to_digit(10).unwrap_or(0) as usize;
                        if let Some((a, b)) = caps.group(g) {
                            out.extend(&t[a..b]);
                        }
                        k += 2;
                    }
                    '\\' if k + 1 < r.len() => {
                        out.push(r[k + 1]);
                        k += 2;
                    }
                    '&' => {
                        out.extend(&t[s..e]);
                        k += 1;
                    }
                    c => {
                        out.push(c);
                        k += 1;
                    }
                }
            }
            n += 1;
            if e == s {
                if s < t.len() {
                    out.push(t[s]);
                }
                at = s + 1;
            } else {
                at = e;
            }
        }
        if n == 0 {
            return None;
        }
        if at < t.len() {
            out.extend(&t[at..]);
        }
        Some(out)
    }
}

#[derive(Debug, Clone)]
struct Thread {
    pc: usize,
    slots: Vec<Option<usize>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(p: &str) -> Regex {
        Regex::compile(p, false, Flavor::Find).unwrap_or_else(|| panic!("{p:?} compiles"))
    }

    #[test]
    fn the_extended_syntax_matches_anywhere_in_the_text() {
        assert!(find("R[0-9]+").is_match("R12"));
        assert!(find("^R1$").is_match("R1") && !find("^R1$").is_match("R12"));
        assert!(find("(SDA|SCL)_[0-9]").is_match("I2C_SCL_1"));
        assert!(find("a{2,3}").is_match("baab") && !find("a{3}").is_match("baab"));
        assert!(find("[[:upper:]]{3}").is_match("xxVCCx"));
        assert!(find("[^0-9]$").is_match("R1x") && !find("[^0-9]$").is_match("R12"));
        assert!(find("\\d+\\.\\d").is_match("v3.3") && find("\\bGND\\b").is_match("A GND B") && !find("\\bGND\\b").is_match("AGNDB"));
        assert!(find("a.c").is_match("abc") && !find("a.c").is_match("ac"));
        for bad in ["(a", "*a", "[b-a]", "x{", "a\\1", "(?=a)", "\\q"] {
            assert!(Regex::compile(bad, false, Flavor::Find).is_none(), "{bad:?}");
        }
        // a word boundary is Find's only: a net class pattern with one is no regular expression
        assert!(Regex::compile("x\\bX", false, Flavor::NetClass).is_none());
    }

    #[test]
    fn a_search_that_is_not_case_sensitive_folds_the_case() {
        assert!(Regex::compile("vcc", true, Flavor::Find).unwrap().is_match("+VCC"));
        assert!(!find("vcc").is_match("+VCC"));
        assert!(Regex::compile("[a-c]x", true, Flavor::Find).unwrap().is_match("BX"));
    }

    #[test]
    fn replace_takes_every_match_and_the_groups_leftmost_first() {
        let re = find("([A-Z]+)([0-9]+)");
        assert_eq!(re.replace_all("R1 C22", "\\2\\1").as_deref(), Some("1R 22C"));
        assert_eq!(re.replace_all("R1", "<&>").as_deref(), Some("<R1>"));
        assert_eq!(re.replace_all("R1", "\\&").as_deref(), Some("&"));
        assert_eq!(re.replace_all("none", "x"), None);
        assert_eq!(find("x*").replace_all("ab", "-").as_deref(), Some("-a-b-"));
        assert_eq!(find("^S").replace_all("SS", "x").as_deref(), Some("xS"), "^ only at the start");
        assert_eq!(find("<.+>").replace_all("<a><b>", "x").as_deref(), Some("x"), "greedy");
        assert_eq!(find("<.+?>").replace_all("<a><b>", "x").as_deref(), Some("xx"), "lazy");
        assert_eq!(find("(?:A|B)(\\d)").replace_all("B7", "\\1").as_deref(), Some("7"), "a (?: ) group is not numbered");
        // leftmost-first, as PCRE2 (wxWidgets 3.2's engine) takes the first alternative that matches: `libpcre2-32` gives `a|ab` on `ab` the span 0..1
        assert_eq!(find("a|ab").replace_all("ab", "x").as_deref(), Some("xb"));
        assert_eq!(find("(a|ab)(c|bcd)").find_at(&"abcd".chars().collect::<Vec<_>>(), 0).and_then(|c| c.group(0)), Some((0, 4)));
    }

    #[test]
    fn a_match_starts_at_the_leftmost_place_and_no_pattern_blows_up() {
        let c = find("b+").find_at(&"aabbb".chars().collect::<Vec<_>>(), 0).unwrap();
        assert_eq!(c.group(0), Some((2, 5)));
        let long: String = format!("{}!", "a".repeat(400));
        assert!(!find("(a+)+$").is_match(&long));
        assert!(!find("(a|aa)+b").is_match(&long));
        assert!(Regex::compile("((((a{200}){200}){200}){200})", false, Flavor::Find).is_none());
    }

    /// UI-LEFT-2 SS10: what `pcre2_compile` / `pcre2_match` (32-bit, `PCRE2_UTF | PCRE2_ALT_BSUX | PCRE2_DOTALL`, wxWidgets 3.2.4's `wx_regcomp`) answer on this
    /// box, each asked of the system's `libpcre2-32` before it was written down here.
    #[test]
    fn the_syntax_is_pcre2s_as_wx_regex_compiles_it() {
        let ok = |p: &str, t: &str| find(p).is_match(t);
        // the four POSIX classes that did not compile, and PCRE2's two others
        assert!(ok("[[:blank:]]", "a b") && !ok("[[:blank:]]", "ab"));
        assert!(ok("[[:cntrl:]]", "a\tb") && !ok("[[:cntrl:]]", "ab"));
        assert!(ok("^[[:print:]]+$", "x y") && !ok("[[:print:]]", "\t"));
        assert!(ok("^[[:graph:]]$", "x") && !ok("[[:graph:]]", " "));
        assert!(ok("[[:word:]]", "_") && ok("[[:ascii:]]", "x") && !ok("[[:ascii:]]", "é"));
        // a class name in capitals is none (what Find's upper-casing makes of `[:alpha:]`)
        assert!(Regex::compile("[[:ALPHA:]]", true, Flavor::Find).is_none());
        // ASCII classes, no `PCRE2_UCP`
        assert!(!ok("\\w", "é") && !ok("[[:alpha:]]", "é") && ok("\\W", "é"));
        // the escapes
        assert!(ok("\\x41", "A") && ok("\\u0041", "A") && ok("\\U", "U") && ok("\\xg", "xg") && ok("\\h", " ") && !ok("\\h", "\n") && ok("\\v", "\n"));
        assert!(ok("a\\N", "ab") && !ok("a\\N", "a\n") && ok("\\Ax", "x") && ok("x\\z", "x") && !ok("x\\z", "x\n") && ok("x\\Z", "x\n") && ok("x$", "x\n"));
        assert!(ok("[\\x41-\\x43]", "B") && ok("[\\h]", "\t"));
        for bad in ["\\T", "\\q"] {
            assert!(Regex::compile(bad, false, Flavor::Find).is_none(), "{bad:?}");
        }
        // **Differs** (the module doc): PCRE2 compiles these, this machine does not
        for unread in ["\\X41", "\\Q.\\E", "(?i)a", "(?=a)"] {
            assert!(Regex::compile(unread, false, Flavor::Find).is_none(), "{unread:?}");
        }
        // the net class flavour keeps its own syntax
        assert!(Regex::compile("[[:blank:]]", false, Flavor::NetClass).is_none() && Regex::compile("\\w", false, Flavor::NetClass).unwrap().is_match("é"));
    }

    /// The choosers' `wxRE_ADVANCED` (UI-LEFT-2 LL4): PCRE2's syntax with wxWidgets 3.2's `ConvertMetasyntax` and `ConvertWordBoundaries` in front.
    #[test]
    fn the_advanced_flavour_is_pcre2_after_wxwidgets_converts_the_tcl_metasyntax() {
        let adv = |p: &str| Regex::compile(p, false, Flavor::Advanced);
        // PCRE2's syntax: ASCII classes, `\b`, `\h`, `[:word:]`
        assert!(adv(r"\bres").unwrap().is_match("a res"));
        assert!(!adv(r"\bres").unwrap().is_match("ares"));
        assert!(!adv(r"\w").unwrap().is_match("é"));
        assert!(adv(r"[[:word:]]+").unwrap().is_match("x_1"));
        // Tcl's word boundaries: `\y` `\Y` are `\b` `\B`, `\m` `\M` a word's start and end
        assert!(adv(r"\yres\y").unwrap().is_match("a res b") && !adv(r"\yres\y").unwrap().is_match("ares"));
        assert!(adv(r"\Yes").unwrap().is_match("res") && !adv(r"\Yes").unwrap().is_match("es"));
        let m = adv(r"\mre").unwrap();
        assert!(m.is_match("x re") && !m.is_match("are"));
        let e = adv(r"or\M").unwrap();
        assert!(e.is_match("resistor") && !e.is_match("orb"));
        assert!(adv("[[:<:]]a").unwrap().is_match("b a") && !adv("[[:<:]]a").unwrap().is_match("ba"));
        // an escaped backslash is a pair: `\\y` is a backslash and a y
        assert!(adv(r"\\y").unwrap().is_match(r"a\y"));
        // the directors and the leading options
        assert!(adv("***:abc").unwrap().is_match("xabcx"));
        let lit = adv("***=a.c").unwrap();
        assert!(lit.is_match("a.c") && !lit.is_match("abc"));
        assert!(adv("(?i)ABC").unwrap().is_match("xabc"));
        assert!(!Regex::compile("(?c)abc", true, Flavor::Advanced).unwrap().is_match("ABC"));
        assert!(adv("(?q)a+").unwrap().is_match("a+") && !adv("(?q)a+").unwrap().is_match("aa"));
        assert!(adv("(?e)\\y").is_none(), "extended syntax: no conversion, and PCRE2 refuses the letter y escaped");
        assert!(adv("(?m)a").is_none(), "an option this port does not read");
        // the net class flavour keeps its own rules
        assert!(Regex::compile(r"\yres", false, Flavor::NetClass).is_none());
    }
}
