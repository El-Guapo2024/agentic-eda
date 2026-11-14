//! The one regular-expression engine of the port, for what KiCad hands to `wxRegEx`: the net class patterns (`EDA_PATTERN_MATCH_REGEX` /
//! `EDA_PATTERN_MATCH_REGEX_ANCHORED`, common/eda_pattern_match.cpp, compiled with `wxRE_ADVANCED`; [`crate::net_pattern`]) and Find / Replace's "Regular
//! Expression" (`EDA_SEARCH_MATCH_MODE::REGEX`, common/eda_item.cpp `EDA_ITEM::Matches` / `Replace`, compiled with `wxRE_DEFAULT` or `wxRE_ICASE`;
//! `eda_ops::search`). No regex crate is taken (no new dependency): this is a port of the part of wxRegEx's syntax a net name or a schematic text is matched
//! with.
//!
//! The syntax: literals, `.` (any character), `[..]` classes (ranges, `^` negation, `[:alpha:]` `[:digit:]` `[:alnum:]` `[:upper:]` `[:lower:]` `[:space:]`
//! `[:punct:]` `[:xdigit:]`, the `\d \w \s` shorthands and their capitals), `( .. )` groups (numbered for a replacement's `\N`) and `(?: .. )` ones, `|`, `^`
//! and `$` (the text's ends), and `* + ? {m} {m,} {m,n}`, each lazy with a `?` after it; `\n`, `\t` and an escaped punctuation character stand for
//! themselves. [`Flavor::Find`] also takes `\b` and `\B` (a word's edge, and not one), and PCRE2's other forms (below). A pattern with anything else --
//! in the net class flavour a back reference, a look-ahead, a named group --, another letter escaped, a `{` that is no count, a second quantifier on one atom,
//! a class or range that does not close does not compile (`wxRegEx::Compile` returns false: a net class pattern then reads as a wildcard pattern only, a Find
//! matches nothing).
//!
//! Matching is by simulation of the machine the pattern compiles to (Thompson's construction, threads in priority order as Pike's), so no such pattern takes
//! longer than its length times the text's -- a pattern made to make a backtracking matcher blow up (`(a+)+$`) does not -- and a program of more than 4000
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
//! PCRE2's other forms (UI-LEFT-3 C3-11), for [`Flavor::Find`] and [`Flavor::Advanced`]: back references (`\\1`, `\\g{n}`, `\\g{-n}`, `\\k<name>`, `\\k'name'`,
//! `\\k{name}`, `(?P=name)`; an unset group matches nothing; `\\12` is a back reference when there are that many groups before it, else octal), look-aheads
//! and look-behinds (`(?=..)`, `(?!..)`, `(?<=..)`, `(?<!..)`, each branch of a look-behind of a fixed length, else the pattern does not compile), named groups
//! (`(?<name>..)`, `(?'name'..)`, `(?P<name>..)`; a name twice only under `(?J)`, where a back reference takes the first group of the name that is set), inline
//! options (`(?imnsxJU-imnsxJU)`, `(?^)`, scoped `(?i:..)`; an option set in a group lasts to its end, across its later branches), comments (`(?#..)`),
//! `\\Q..\\E` (in a class too; a quantifier after it takes its last character), Unicode properties (`\\p{..}`, `\\P{..}`, `\\pL`, `\\p{^..}`: the general
//! categories and their letters, `L&`, the scripts -- a bare name or `scx:` the script extensions, `sc:` the script --, the binary properties, `Any`, `Xan`,
//! `Xps`, `Xsp`, `Xwd`, `Xuc`; names matched loosely; `crate::wx_regex_unicode`, Unicode 15, where the box's PCRE2 10.42 has 14), atomic groups and possessive
//! quantifiers, `\\o{..}`, `\\0dd`, `\\cX`, `\\R`, `\\N{U+hh}`. A pattern with a back reference, a look-around or an atomic group is matched as PCRE2 matches, by
//! backtracking, and gives up (no match, `PCRE2_ERROR_MATCHLIMIT`) after PCRE2's `match_limit` of ten million steps; every other pattern keeps the machine
//! below. Checked case by case against the box's `libpcre2-32`. **Differs**: `\\X`, `\\K`, `\\G`, conditional groups, recursion and subroutine calls, branch
//! resets, `\\p{bc:..}`, the start-of-pattern settings (`(*UTF)` ..) and backtracking verbs do not compile here (no Find of KiCad's needs them; a pattern with one
//! matches nothing); the net class flavour keeps the syntax it had (Unicode classes, no new escapes), what the INACTIVE engine of `crate::net_pattern` is
//! tested against.

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
    /// A character, compared without regard to case when the pattern or an inline option (`(?i)`) says so.
    Char(char, bool),
    /// `.`: any character, a line feed too unless `(?-s)` took `PCRE2_DOTALL` off.
    Any(bool),
    Class { items: Vec<ClassItem>, negate: bool, icase: bool },
    /// `^` (`true`: at any line's start, `(?m)`) and `\A`.
    Bol(bool),
    /// `$` (`true`: before any line feed, `(?m)`) and `\Z`.
    Eol(bool),
    /// `\\z`: the text's very end (where `$` also holds before a final line feed).
    EndOnly,
    WordBoundary(bool),
    /// PCRE2's `[[:<:]]` (true) and `[[:>:]]` (false): a word starts, or ends, here ([`Flavor::Advanced`]).
    WordEdge(bool),
    Group(Box<Node>, usize),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Repeat { node: Box<Node>, min: usize, max: Option<usize>, greedy: bool },
    /// A back reference to group `n` (`\\1`, `\\g{..}`, `\\k<name>`, `(?P=name)`), compared without regard to case under `(?i)` (UI-LEFT-3 C3-11).
    Backref(usize, bool),
    /// A back reference by name, numbered once the whole pattern is read (a name may be used before its group).
    NamedBackref(String, bool),
    /// A back reference to a name several groups have (`(?J)`): the first of them that is set.
    BackrefAny(Vec<usize>, bool),
    /// `(?=..)`, `(?!..)`: the body matches here (or does not), nothing read.
    LookAhead { negate: bool, inner: Box<Node> },
    /// `(?<=..)`, `(?<!..)`: one of the branches, each of a fixed length (PCRE2: "lookbehind assertion is not fixed length"), ends here.
    LookBehind { negate: bool, branches: Vec<(usize, Node)> },
    /// `(?>..)` and a possessive quantifier (`a*+`): the body's first match, never tried again.
    Atomic(Box<Node>),
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
    /// `\\p{..}` (true) and `\\P{..}` (false): a Unicode property (UI-LEFT-3 C3-11).
    Prop(Prop, bool),
}

/// The Unicode properties PCRE2 10.42 reads in `\\p{..}` (`pcre2_ucptables.c`), from `crate::wx_regex_unicode`'s tables: a general category or a group of
/// them (`L`, `L&`), a script (its code points, `sc:`) or its script extensions (a bare name, `scx:`), a binary property, and PCRE2's own `Any`, `Xan`
/// (letters and numbers), `Xps` / `Xsp` (white space), `Xwd` (`Xan` and `_`) and `Xuc` (what a universal character name may be).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Prop {
    Any,
    /// The general categories whose two-letter names start with the letters given (`L` -> every `L?`; `Cn`: in none of the tables).
    Category(&'static str),
    /// `L&` / `LC`: `Lu`, `Ll`, `Lt`.
    CasedLetter,
    /// A script's code points (`sc:`) or its extensions (`scx:`, a bare name), by its index in `SCRIPTS`.
    Script(usize, bool),
    Binary(usize),
    Xan,
    Xsp,
    Xwd,
    Xuc,
}

/// PCRE2's loose name matching (10.40 and later): letters without case, spaces, hyphens and underscores ignored.
fn loose(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, ' ' | '-' | '_')).flat_map(char::to_lowercase).collect()
}

/// The property a `\\p{..}` name names, or `None` (the pattern does not compile: "unknown property name after \\P or \\p").
fn prop_named(name: &str) -> Option<Prop> {
    use crate::wx_regex_unicode::{BINARY, GENERAL_CATEGORIES, SCRIPTS};
    let (kind, value) = match name.split_once(':') {
        Some((k, v)) => (Some(loose(k)), loose(v)),
        None => (None, loose(name)),
    };
    let script = |v: &str, ext: bool| SCRIPTS.iter().position(|(long, short, _, _)| loose(long) == v || loose(short) == v).map(|i| Prop::Script(i, ext));
    match kind.as_deref() {
        Some("sc" | "script") => return script(&value, false),
        Some("scx" | "scriptextensions") => return script(&value, true),
        Some(_) => return None,
        None => {}
    }
    match value.as_str() {
        "any" => return Some(Prop::Any),
        "l&" | "lc" => return Some(Prop::CasedLetter),
        "xan" => return Some(Prop::Xan),
        "xps" | "xsp" => return Some(Prop::Xsp),
        "xwd" => return Some(Prop::Xwd),
        "xuc" => return Some(Prop::Xuc),
        "cn" => return Some(Prop::Category("Cn")),
        _ => {}
    }
    if let Some((g, _)) = GENERAL_CATEGORIES.iter().find(|(g, _)| loose(g) == value || (value.len() == 1 && g.to_lowercase().starts_with(&value))) {
        return Some(Prop::Category(if value.len() == 1 { &g[..1] } else { g }));
    }
    if let Some(i) = BINARY.iter().position(|(long, short, _)| loose(long) == value || loose(short) == value) {
        return Some(Prop::Binary(i));
    }
    script(&value, true)
}

fn in_ranges(ranges: &[(u32, u32)], c: u32) -> bool {
    ranges.binary_search_by(|&(lo, hi)| if hi < c { std::cmp::Ordering::Less } else if lo > c { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal }).is_ok()
}

/// Whether the character has the general category `g` (one or two letters; `Cn` is no category of the tables).
fn has_category(g: &str, c: u32) -> bool {
    use crate::wx_regex_unicode::GENERAL_CATEGORIES;
    if g == "Cn" || g == "C" {
        let assigned = GENERAL_CATEGORIES.iter().any(|(_, r)| in_ranges(r, c));
        if !assigned {
            return true;
        }
        if g == "Cn" {
            return false;
        }
    }
    GENERAL_CATEGORIES.iter().filter(|(name, _)| name.starts_with(g)).any(|(_, r)| in_ranges(r, c))
}

fn prop_matches(p: Prop, ch: char) -> bool {
    use crate::wx_regex_unicode::{BINARY, SCRIPTS};
    let c = ch as u32;
    match p {
        Prop::Any => true,
        Prop::Category(g) => has_category(g, c),
        Prop::CasedLetter => ["Lu", "Ll", "Lt"].iter().any(|g| has_category(g, c)),
        Prop::Script(i, ext) => {
            let (_, _, own, extensions) = SCRIPTS[i];
            in_ranges(if ext { extensions.unwrap_or(own) } else { own }, c)
        }
        Prop::Binary(i) => in_ranges(BINARY[i].2, c),
        Prop::Xan => has_category("L", c) || has_category("N", c),
        Prop::Xsp => has_category("Z", c) || matches!(ch, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r'),
        Prop::Xwd => has_category("L", c) || has_category("N", c) || ch == '_',
        Prop::Xuc => matches!(ch, '$' | '@' | '`') || (c >= 0xa0 && !(0xd800..=0xdfff).contains(&c)),
    }
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
        ClassItem::Prop(p, yes) => prop_matches(p, c) == yes,
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

/// PCRE2's options a pattern can change inside itself (`(?imnsxJU)`, `(?^)`; UI-LEFT-3 C3-11): case, `^`/`$` at every line, `.` takes a line feed, extended
/// syntax (white space and `#` comments ignored), no numbered groups, duplicate names, quantifiers lazy by default.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Opts {
    icase: bool,
    multiline: bool,
    dotall: bool,
    extended: bool,
    no_auto_capture: bool,
    dup_names: bool,
    ungreedy: bool,
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    depth: usize,
    groups: usize,
    flavor: Flavor,
    opts: Opts,
    /// The options the pattern started with (`(?^)` goes back to them).
    start_opts: Opts,
    /// The named groups: name -> number.
    names: Vec<(String, usize)>,
    /// What the last atom was: `\\Q..\\E`'s characters (a quantifier takes the last of them), or an item that cannot be repeated (an option setting,
    /// a comment).
    last: AtomKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AtomKind {
    Plain,
    Quoted,
    Unrepeatable,
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

    fn starts(&self, s: &str) -> bool {
        let w: Vec<char> = s.chars().collect();
        self.chars[self.pos..].starts_with(&w)
    }

    /// `(?x)`: white space and `#` comments to the line's end are no part of the pattern (outside a class).
    fn skip_extended(&mut self) {
        if !self.opts.extended {
            return;
        }
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += 1;
            } else if c == '#' {
                while let Some(c) = self.peek() {
                    self.pos += 1;
                    if c == '\n' {
                        break;
                    }
                }
            } else if self.starts("(?#") {
                // a comment group
                while let Some(c) = self.peek() {
                    self.pos += 1;
                    if c == ')' {
                        break;
                    }
                }
            } else {
                break;
            }
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
        loop {
            self.skip_extended();
            match self.peek() {
                None | Some('|') | Some(')') => break,
                _ => {}
            }
            items.push(self.repeat()?);
        }
        Some(Node::Seq(items))
    }

    fn repeat(&mut self) -> Option<Node> {
        self.last = AtomKind::Plain;
        let atom = self.atom()?;
        let kind = self.last;
        self.skip_extended();
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
        // a `?` after the quantifier makes it lazy (greedy under `(?U)`), a `+` possessive (`(?>..)` of it) -- PCRE2's own (UI-LEFT-3 C3-11)
        let possessive = self.flavor.pcre() && self.eat('+');
        let greedy = if !possessive && self.eat('?') { self.opts.ungreedy } else { !self.opts.ungreedy || possessive };
        if matches!(self.peek(), Some('*' | '+' | '{')) {
            return None; // a second quantifier on the same atom
        }
        let wrap = |node: Node| {
            let r = Node::Repeat { node: Box::new(node), min, max, greedy };
            if possessive { Node::Atomic(Box::new(r)) } else { r }
        };
        match (kind, atom) {
            // "quantifier does not follow a repeatable item"
            (AtomKind::Unrepeatable, _) => None,
            // `\Qab\E+`: the quantifier takes the last quoted character
            (AtomKind::Quoted, Node::Seq(mut items)) => {
                let last = items.pop()?;
                items.push(wrap(last));
                Some(Node::Seq(items))
            }
            (_, atom) => Some(wrap(atom)),
        }
    }

    /// A group's name (`[A-Za-z_][A-Za-z0-9_]*`, at most 32 characters, PCRE2's `MAX_NAME_SIZE`) up to the closing character `close`.
    fn group_name(&mut self, close: char) -> Option<String> {
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c == close {
                self.pos += 1;
                let first = name.chars().next()?;
                return (!first.is_ascii_digit() && name.len() <= 32).then_some(name);
            }
            if !(c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            name.push(c);
            self.pos += 1;
        }
        None
    }

    /// The options of `(?imnsxJU-imnsxJU)` / `(?^..)` up to `)` or `:` (which is left to read); `None` for a letter PCRE2 does not take.
    fn options(&mut self) -> Option<Opts> {
        let mut o = self.opts;
        let mut on = true;
        if self.eat('^') {
            o = Opts { icase: self.start_opts.icase, multiline: false, dotall: self.start_opts.dotall, extended: false, no_auto_capture: false, dup_names: false, ungreedy: false };
        }
        loop {
            match self.peek()? {
                ')' | ':' => return Some(o),
                '-' if on => on = false,
                'i' => o.icase = on,
                'm' => o.multiline = on,
                's' => o.dotall = on,
                'x' => o.extended = on,
                'n' => o.no_auto_capture = on,
                'J' => o.dup_names = on,
                'U' => o.ungreedy = on,
                _ => return None,
            }
            self.pos += 1;
        }
    }

    /// A group's body up to its `)`, under the options it starts with; the options come back as they were after it.
    fn group_body(&mut self) -> Option<Node> {
        let saved = self.opts;
        let inner = self.alt()?;
        self.opts = saved;
        if !self.eat(')') {
            return None;
        }
        self.depth -= 1;
        Some(inner)
    }

    fn capture(&mut self, name: Option<String>) -> Option<Node> {
        self.groups += 1;
        let n = self.groups;
        if let Some(name) = name {
            if self.names.iter().any(|(k, _)| *k == name) && !self.opts.dup_names {
                return None; // "two named subpatterns have the same name"
            }
            self.names.push((name, n));
        }
        Some(Node::Group(Box::new(self.group_body()?), n))
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
                if !self.eat('?') {
                    return if self.opts.no_auto_capture && self.flavor.pcre() { self.group_body() } else { self.capture(None) };
                }
                if !self.flavor.pcre() {
                    // the net class flavour: only `(?:..)`
                    if !self.eat(':') {
                        return None;
                    }
                    return self.group_body();
                }
                // PCRE2's group forms (UI-LEFT-3 C3-11)
                match self.peek()? {
                    ':' => {
                        self.pos += 1;
                        self.group_body()
                    }
                    '#' => {
                        while self.peek()? != ')' {
                            self.pos += 1;
                        }
                        self.pos += 1;
                        self.depth -= 1;
                        self.last = AtomKind::Unrepeatable;
                        Some(Node::Seq(Vec::new()))
                    }
                    '=' | '!' => {
                        let negate = self.peek()? == '!';
                        self.pos += 1;
                        Some(Node::LookAhead { negate, inner: Box::new(self.group_body()?) })
                    }
                    '<' if matches!(self.chars.get(self.pos + 1), Some('=' | '!')) => {
                        let negate = self.chars[self.pos + 1] == '!';
                        self.pos += 2;
                        let inner = self.group_body()?;
                        let alternatives = match inner {
                            Node::Alt(b) => b,
                            other => vec![other],
                        };
                        let branches = alternatives.into_iter().map(|b| fixed_length(&b).map(|n| (n, b))).collect::<Option<Vec<_>>>()?;
                        Some(Node::LookBehind { negate, branches })
                    }
                    '>' => {
                        self.pos += 1;
                        Some(Node::Atomic(Box::new(self.group_body()?)))
                    }
                    '<' => {
                        self.pos += 1;
                        let name = self.group_name('>')?;
                        self.capture(Some(name))
                    }
                    '\'' => {
                        self.pos += 1;
                        let name = self.group_name('\'')?;
                        self.capture(Some(name))
                    }
                    'P' if self.chars.get(self.pos + 1) == Some(&'<') => {
                        self.pos += 2;
                        let name = self.group_name('>')?;
                        self.capture(Some(name))
                    }
                    'P' if self.chars.get(self.pos + 1) == Some(&'=') => {
                        self.pos += 2;
                        let name = self.group_name(')')?;
                        self.depth -= 1;
                        Some(Node::NamedBackref(name, self.opts.icase))
                    }
                    _ => {
                        let o = self.options()?;
                        if self.eat(')') {
                            // `(?i)`: the rest of the enclosing group
                            self.opts = o;
                            self.depth -= 1;
                            self.last = AtomKind::Unrepeatable;
                            return Some(Node::Seq(Vec::new()));
                        }
                        self.pos += 1; // ':'
                        let saved = self.opts;
                        self.opts = o;
                        let inner = self.group_body();
                        self.opts = saved;
                        inner
                    }
                }
            }
            ')' | '*' | '+' | '?' | '{' => None,
            '.' => Some(Node::Any(self.opts.dotall)),
            '^' => Some(Node::Bol(self.opts.multiline)),
            '$' => Some(Node::Eol(self.opts.multiline)),
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
                    'd' | 'D' | 'w' | 'W' | 's' | 'S' => Some(Node::Class { items: vec![shorthand(e)], negate: false, icase: self.opts.icase }),
                    'n' => Some(Node::Char('\n', self.opts.icase)),
                    't' => Some(Node::Char('\t', self.opts.icase)),
                    c if c.is_ascii_digit() || c == 'b' || c == 'B' || c == 'A' || c == 'Z' || c == '<' || c == '>' || c == 'y' || c == 'm' || c == 'M' => None, // back references, word boundaries
                    c if c.is_ascii_alphabetic() => None,
                    c => Some(Node::Char(c, self.opts.icase)),
                }
            }
            c => Some(Node::Char(c, self.opts.icase)),
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

    /// `{..}` after `\\p`, `\\o`, `\\g`, `\\k`: its text (a `\\p` and `\\P` may name a one-letter category without braces).
    fn braced(&mut self) -> Option<String> {
        if !self.eat('{') {
            return None;
        }
        let close = self.chars[self.pos..].iter().position(|&c| c == '}')? + self.pos;
        let text: String = self.chars[self.pos..close].iter().collect();
        self.pos = close + 1;
        Some(text)
    }

    /// `\\p{..}` / `\\P{..}` / `\\pL`: the class item ( `{^..}` negates).
    fn property(&mut self, yes: bool) -> Option<ClassItem> {
        let name = if self.peek() == Some('{') { self.braced()? } else { self.peek().map(|c| c.to_string()).inspect(|_| self.pos += 1)? };
        let (name, yes) = match name.strip_prefix('^') {
            Some(n) => (n.to_string(), !yes),
            None => (name, yes),
        };
        Some(ClassItem::Prop(prop_named(name.trim())?, yes))
    }

    /// `\\Q..\\E`: the characters up to `\\E` (or the pattern's end), each for itself.
    fn quoted(&mut self) -> Vec<char> {
        let mut out = Vec::new();
        while let Some(c) = self.peek() {
            if c == '\\' && self.chars.get(self.pos + 1) == Some(&'E') {
                self.pos += 2;
                break;
            }
            out.push(c);
            self.pos += 1;
        }
        out
    }

    /// `\\o{ddd}`, `\\0dd` and an octal escape: the character; `\\cX`: the control character.
    fn octal(&mut self, digits: &str) -> Option<char> {
        char::from_u32(u32::from_str_radix(digits, 8).ok()?)
    }

    /// One escaped character outside a class as PCRE2 reads it ([`Flavor::Find`], see the module doc); `None` for an escape that does not compile.
    fn pcre_escape(&mut self, e: char) -> Option<Node> {
        let icase = self.opts.icase;
        let class = |item: ClassItem| Some(Node::Class { items: vec![item], negate: false, icase });
        match e {
            'd' | 'D' | 'w' | 'W' | 's' | 'S' => class(shorthand(e)),
            'h' | 'H' => class(ClassItem::HSpace(e == 'h')),
            'v' | 'V' => class(ClassItem::VSpace(e == 'v')),
            'p' | 'P' => class(self.property(e == 'p')?),
            // `\\N`: any character but a line feed (`\\N{U+hh..}` a character)
            'N' if self.peek() == Some('{') => {
                let t = self.braced()?;
                let hex = t.strip_prefix("U+")?;
                Some(Node::Char(char::from_u32(u32::from_str_radix(hex, 16).ok()?)?, icase))
            }
            'N' => Some(Node::Class { items: vec![ClassItem::Range('\n', '\n')], negate: true, icase: false }),
            'b' => Some(Node::WordBoundary(true)),
            'B' => Some(Node::WordBoundary(false)),
            'A' => Some(Node::Bol(false)),
            'Z' => Some(Node::Eol(false)),
            'z' => Some(Node::EndOnly),
            'x' => Some(Node::Char(self.hex_char(2).unwrap_or('x'), icase)),
            'u' => Some(Node::Char(self.hex_char(4).unwrap_or('u'), icase)),
            'U' => Some(Node::Char('U', icase)),
            // UI-LEFT-3 C3-11: quoting, octal and control characters, `\\R`, back references by number and by name
            'Q' => {
                self.last = AtomKind::Quoted;
                Some(Node::Seq(self.quoted().into_iter().map(|c| Node::Char(c, icase)).collect()))
            }
            'E' => {
                self.last = AtomKind::Unrepeatable;
                Some(Node::Seq(Vec::new()))
            }
            'o' => Some(Node::Char(self.braced().and_then(|d| self.octal(&d))?, icase)),
            'c' => {
                let x = self.peek()?;
                self.pos += 1;
                if !x.is_ascii() || x.is_ascii_control() {
                    return None;
                }
                Some(Node::Char(char::from_u32((x.to_ascii_uppercase() as u32) ^ 0x40)?, icase))
            }
            'R' => {
                // `(?>\\r\\n|\\n|\\x0b|\\f|\\r|\\x85|\\x{2028}|\\x{2029})`
                let one = Node::Class { items: vec![ClassItem::VSpace(true)], negate: false, icase: false };
                Some(Node::Atomic(Box::new(Node::Alt(vec![Node::Seq(vec![Node::Char('\r', false), Node::Char('\n', false)]), one]))))
            }
            'g' => {
                let text = if self.peek() == Some('{') {
                    self.braced()?
                } else {
                    let start = self.pos;
                    if self.peek() == Some('-') || self.peek() == Some('+') {
                        self.pos += 1;
                    }
                    while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                        self.pos += 1;
                    }
                    self.chars[start..self.pos].iter().collect()
                };
                if let Ok(n) = text.parse::<i64>() {
                    let g = if text.starts_with('-') { self.groups as i64 + 1 + n } else { n };
                    if g <= 0 || text.starts_with('+') {
                        return None;
                    }
                    return Some(Node::Backref(g as usize, icase));
                }
                (!text.is_empty()).then(|| Node::NamedBackref(text, icase))
            }
            'k' => {
                let name = match self.peek()? {
                    '<' => {
                        self.pos += 1;
                        self.group_name('>')?
                    }
                    '\'' => {
                        self.pos += 1;
                        self.group_name('\'')?
                    }
                    '{' => {
                        self.pos += 1;
                        self.group_name('}')?
                    }
                    _ => return None,
                };
                Some(Node::NamedBackref(name, icase))
            }
            '0' => {
                // `\\0` and up to two more octal digits
                let start = self.pos;
                while self.pos < start + 2 && self.peek().is_some_and(|c| ('0'..='7').contains(&c)) {
                    self.pos += 1;
                }
                let digits: String = std::iter::once('0').chain(self.chars[start..self.pos].iter().copied()).collect();
                Some(Node::Char(self.octal(&digits)?, icase))
            }
            '1'..='9' => {
                // "the entire sequence is taken as a backreference" when it is less than 10, begins with 8 or 9, or there are that many groups before it;
                // else up to three octal digits
                let start = self.pos - 1;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.pos += 1;
                }
                let digits: String = self.chars[start..self.pos].iter().collect();
                let n: usize = digits.parse().ok()?;
                if n < 10 || e == '8' || e == '9' || n <= self.groups {
                    return Some(Node::Backref(n, icase));
                }
                self.pos = start;
                let mut end = start;
                while end < start + 3 && self.chars.get(end).is_some_and(|c| ('0'..='7').contains(c)) {
                    end += 1;
                }
                let oct: String = self.chars[start..end].iter().collect();
                self.pos = end;
                Some(Node::Char(self.octal(&oct)?, icase))
            }
            c => simple_escape(c).map(|c| Node::Char(c, icase)),
        }
    }

    fn class(&mut self) -> Option<Node> {
        let negate = self.eat('^');
        let mut items: Vec<ClassItem> = Vec::new();
        let mut first = true;
        // `\\Q..\\E` in a class: its characters, each for itself, still to be taken
        let mut pending: std::collections::VecDeque<char> = Default::default();
        loop {
            let c = match pending.pop_front() {
                Some(c) => {
                    first = false;
                    items.push(ClassItem::Range(c, c));
                    continue;
                }
                None => self.peek()?,
            };
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
                        'p' | 'P' if self.flavor.pcre() => {
                            items.push(self.property(e == 'p')?);
                            continue;
                        }
                        'Q' if self.flavor.pcre() => {
                            pending.extend(self.quoted());
                            continue;
                        }
                        'E' if self.flavor.pcre() => continue,
                        'n' => '\n',
                        't' => '\t',
                        // in a class PCRE2's `\b` is a backspace
                        'b' if self.flavor.pcre() => '\u{8}',
                        'x' if self.flavor.pcre() => self.hex_char(2).unwrap_or('x'),
                        'u' if self.flavor.pcre() => self.hex_char(4).unwrap_or('u'),
                        'U' if self.flavor.pcre() => 'U',
                        'o' if self.flavor.pcre() => self.braced().and_then(|d| self.octal(&d))?,
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
        Some(Node::Class { items, negate, icase: self.opts.icase })
    }
}

/// The number of characters `n` always matches (PCRE2's lookbehind: "lookbehind assertion is not fixed length" when it has none), `None` when it varies.
fn fixed_length(n: &Node) -> Option<usize> {
    match n {
        Node::Char(..) | Node::Any(_) | Node::Class { .. } => Some(1),
        Node::Bol(_) | Node::Eol(_) | Node::EndOnly | Node::WordBoundary(_) | Node::WordEdge(_) | Node::LookAhead { .. } | Node::LookBehind { .. } => Some(0),
        Node::Group(inner, _) | Node::Atomic(inner) => fixed_length(inner),
        Node::Seq(items) => items.iter().map(fixed_length).sum(),
        Node::Alt(branches) => {
            let first = fixed_length(branches.first()?)?;
            branches.iter().all(|b| fixed_length(b) == Some(first)).then_some(first)
        }
        Node::Repeat { node, min, max, .. } => (Some(*min) == *max).then(|| fixed_length(node).map(|l| l * min)).flatten(),
        Node::Backref(..) | Node::NamedBackref(..) | Node::BackrefAny(..) => None,
    }
}

/// Whether the pattern needs the backtracking matcher (a back reference, a look-around or an atomic group): the machine of Thompson's construction cannot
/// tell those.
fn needs_backtracking(n: &Node) -> bool {
    match n {
        Node::Backref(..) | Node::NamedBackref(..) | Node::BackrefAny(..) | Node::LookAhead { .. } | Node::LookBehind { .. } | Node::Atomic(_) => true,
        Node::Group(inner, _) => needs_backtracking(inner),
        Node::Seq(items) | Node::Alt(items) => items.iter().any(needs_backtracking),
        Node::Repeat { node, .. } => needs_backtracking(node),
        _ => false,
    }
}

/// The named back references numbered (a name used before its group, or with `(?J)` the first group of that name); `None` for a name no group has.
fn resolve_names(n: &mut Node, names: &[(String, usize)]) -> Option<()> {
    match n {
        Node::NamedBackref(name, icase) => {
            let groups: Vec<usize> = names.iter().filter(|(k, _)| k == name).map(|(_, g)| *g).collect();
            *n = match groups.as_slice() {
                [] => return None,
                [g] => Node::Backref(*g, *icase),
                _ => Node::BackrefAny(groups, *icase),
            };
        }
        Node::Group(inner, _) | Node::Atomic(inner) | Node::LookAhead { inner, .. } => resolve_names(inner, names)?,
        Node::Repeat { node, .. } => resolve_names(node, names)?,
        Node::Seq(items) | Node::Alt(items) => items.iter_mut().try_for_each(|i| resolve_names(i, names))?,
        Node::LookBehind { branches, .. } => branches.iter_mut().try_for_each(|(_, b)| resolve_names(b, names))?,
        _ => {}
    }
    Some(())
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
    /// A character, and whether case is ignored.
    Char(char, bool),
    /// Any character; `false`: not a line feed.
    Any(bool),
    Class(usize),
    /// Two ways on, the first preferred.
    Split(usize, usize),
    Jmp(usize),
    /// `true`: also after a line feed (`(?m)`).
    Bol(bool),
    /// `true`: also before any line feed (`(?m)`).
    Eol(bool),
    EndOnly,
    WordBoundary(bool),
    WordEdge(bool),
    /// The text position at this point is kept in capture slot `n` (a group's start is `2 * g`, its end `2 * g + 1`).
    Save(usize),
    Match,
    // ---- the backtracking matcher's own (UI-LEFT-3 C3-11)
    /// Group `g`'s text again (an unset group matches nothing, as PCRE2's default).
    Backref(usize, bool),
    /// The first set group of these (a name several groups have).
    BackrefAny(Vec<usize>, bool),
    /// The body at the next instruction, ended by `SubMatch`, matches here (or does not); on at `end`.
    LookAhead { negate: bool, end: usize },
    /// Each branch `(length, first instruction)` tried `length` characters back; on at `end`.
    LookBehind { negate: bool, branches: Vec<(usize, usize)>, end: usize },
    /// The body at the next instruction, its first match kept; on at `end`.
    Atomic { end: usize },
    SubMatch,
    /// A loop's iteration starts: its position kept in loop slot `k`.
    LoopMark(usize),
    /// An iteration that read nothing leaves the loop (PCRE2 stops repeating a group that matched an empty string): on at `exit`.
    LoopCheck(usize, usize),
}

/// PCRE2's `match_limit` (10,000,000): a match that takes more steps than this fails (`PCRE2_ERROR_MATCHLIMIT`, which `wxRegEx::Matches` takes for no match).
const MATCH_LIMIT: u64 = 10_000_000;

/// A compiled pattern.
#[derive(Debug, Clone)]
pub struct Regex {
    program: Vec<Inst>,
    classes: Vec<(Vec<ClassItem>, bool, bool)>,
    /// The groups numbered for `\N` (group 0 is the whole match).
    groups: usize,
    /// The classes are PCRE2's ASCII ones ([`Flavor::Find`]).
    ascii: bool,
    /// The instructions counted against [`MAX_PROGRAM`].
    counted: usize,
    /// The pattern has a back reference, a look-around or an atomic group: matched by backtracking, as PCRE2 matches, within [`MATCH_LIMIT`].
    backtrack: bool,
    loops: usize,
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

/// What the backtracking matcher undoes when it goes back.
enum Job {
    Try(usize, usize),
    Slot(usize, Option<usize>),
    Loop(usize, usize),
}

struct LimitReached;

impl Regex {
    /// `wxRegEx::Compile( pattern, icase ? wxRE_ICASE : .. )` for `flavor`; `None` when the pattern does not compile (see the module doc).
    pub fn compile(pattern: &str, icase: bool, flavor: Flavor) -> Option<Regex> {
        let (converted, icase) = if flavor == Flavor::Advanced { advanced_metasyntax(pattern, icase)? } else { (pattern.to_string(), icase) };
        let opts = Opts { icase, multiline: false, dotall: true, extended: false, no_auto_capture: false, dup_names: false, ungreedy: false };
        let mut p = Parser { chars: converted.chars().collect(), pos: 0, depth: 0, groups: 0, flavor, opts, start_opts: opts, names: Vec::new(), last: AtomKind::Plain };
        let mut ast = p.alt()?;
        if p.pos != p.chars.len() {
            return None; // an unmatched `)`
        }
        resolve_names(&mut ast, &p.names)?;
        let backtrack = needs_backtracking(&ast);
        // "reference to non-existent subpattern"
        if backtrack && !backrefs_exist(&ast, p.groups) {
            return None;
        }
        let mut r = Regex { program: Vec::new(), classes: Vec::new(), groups: p.groups, ascii: flavor.pcre(), counted: 0, backtrack, loops: 0 };
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

    /// A body ending in `SubMatch` (a look-around's, an atomic group's).
    fn emit_sub(&mut self, n: &Node) -> Option<()> {
        self.emit(n)?;
        self.push(Inst::SubMatch)?;
        Some(())
    }

    fn emit(&mut self, n: &Node) -> Option<()> {
        match n {
            Node::Char(c, icase) => {
                self.push(Inst::Char(*c, *icase))?;
            }
            Node::Any(dotall) => {
                self.push(Inst::Any(*dotall))?;
            }
            Node::Class { items, negate, icase } => {
                self.classes.push((items.clone(), *negate, *icase));
                let idx = self.classes.len() - 1;
                self.push(Inst::Class(idx))?;
            }
            Node::Bol(m) => {
                self.push(Inst::Bol(*m))?;
            }
            Node::Eol(m) => {
                self.push(Inst::Eol(*m))?;
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
                        // L: split body, end; body; jmp L -- the backtracking matcher also stops at an iteration that read nothing
                        let at = self.push(Inst::Split(0, 0))?;
                        let guard = self.backtrack.then(|| {
                            self.loops += 1;
                            self.loops - 1
                        });
                        if let Some(k) = guard {
                            self.push(Inst::LoopMark(k))?;
                        }
                        self.emit(node)?;
                        let check = match guard {
                            Some(k) => Some(self.push(Inst::LoopCheck(k, 0))?),
                            None => None,
                        };
                        self.push(Inst::Jmp(at))?;
                        let end = self.program.len();
                        self.program[at] = split(at + 1, end);
                        if let (Some(c), Some(k)) = (check, guard) {
                            self.program[c] = Inst::LoopCheck(k, end);
                        }
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
            Node::Backref(g, icase) => {
                self.push(Inst::Backref(*g, *icase))?;
            }
            Node::BackrefAny(gs, icase) => {
                self.push(Inst::BackrefAny(gs.clone(), *icase))?;
            }
            Node::NamedBackref(..) => return None,
            Node::LookAhead { negate, inner } => {
                let at = self.push(Inst::LookAhead { negate: *negate, end: 0 })?;
                self.emit_sub(inner)?;
                let end = self.program.len();
                self.program[at] = Inst::LookAhead { negate: *negate, end };
            }
            Node::LookBehind { negate, branches } => {
                let at = self.push(Inst::LookBehind { negate: *negate, branches: Vec::new(), end: 0 })?;
                let mut starts = Vec::new();
                for (len, b) in branches {
                    starts.push((*len, self.program.len()));
                    self.emit_sub(b)?;
                }
                let end = self.program.len();
                self.program[at] = Inst::LookBehind { negate: *negate, branches: starts, end };
            }
            Node::Atomic(inner) => {
                let at = self.push(Inst::Atomic { end: 0 })?;
                self.emit_sub(inner)?;
                let end = self.program.len();
                self.program[at] = Inst::Atomic { end };
            }
        }
        Some(())
    }

    /// Whether `inst` holds at `pos` without reading (an anchor or a boundary).
    fn holds(&self, inst: &Inst, pos: usize, chars: &[char]) -> bool {
        match inst {
            Inst::Bol(m) => pos == 0 || (*m && chars[pos - 1] == '\n'),
            // PCRE2's `$` (no `PCRE2_DOLLAR_ENDONLY`): the end, or before a line feed that ends the text; under `(?m)` before any line feed
            Inst::Eol(m) => pos == chars.len() || (pos + 1 == chars.len() && chars[pos] == '\n') || (*m && chars[pos] == '\n'),
            Inst::EndOnly => pos == chars.len(),
            Inst::WordBoundary(want) => {
                let before = pos > 0 && word_char(chars[pos - 1], self.ascii);
                let after = pos < chars.len() && word_char(chars[pos], self.ascii);
                (before != after) == *want
            }
            // `[[:<:]]`: a word character follows and none precedes; `[[:>:]]`: the other way round
            Inst::WordEdge(start) => {
                let before = pos > 0 && word_char(chars[pos - 1], self.ascii);
                let after = pos < chars.len() && word_char(chars[pos], self.ascii);
                (after && !before && *start) || (before && !after && !*start)
            }
            _ => false,
        }
    }

    /// Whether the instruction reads the character `c`.
    fn reads(&self, inst: &Inst, c: char) -> bool {
        match inst {
            Inst::Char(x, icase) => same_char(*x, c, *icase),
            Inst::Any(dotall) => *dotall || c != '\n',
            Inst::Class(k) => {
                let (items, negate, icase) = &self.classes[*k];
                class_matches(items, *negate, c, *icase, self.ascii)
            }
            _ => false,
        }
    }

    /// Whether some match starts at the first character of `text`: the machine runs from position 0 and accepts as soon as one of its threads reaches the match
    /// state, wherever that is (`wxRegEx::Matches` plus `GetMatch`'s start, which `EDA_COMBINED_MATCHER::StartsWith` requires to be 0) -- the net class question.
    pub fn matches_from_start(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        if self.backtrack {
            let mut budget = MATCH_LIMIT;
            let mut slots = vec![None; 2 * (self.groups + 1)];
            return matches!(self.backtrack_from(0, 0, &mut slots, &mut vec![usize::MAX; self.loops], &chars, &mut budget), Ok(Some(_)));
        }
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
        if self.backtrack {
            // `pcre2_match`: each start in turn, the first that matches; over the match limit, no match
            let mut budget = MATCH_LIMIT;
            for start in from..=chars.len() {
                let mut slots = vec![None; width];
                match self.backtrack_from(0, start, &mut slots, &mut vec![usize::MAX; self.loops], chars, &mut budget) {
                    Ok(Some(_)) => return Some(Captures { slots }),
                    Ok(None) => {}
                    Err(LimitReached) => return None,
                }
            }
            return None;
        }
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

    /// The backtracking matcher (UI-LEFT-3 C3-11), PCRE2's way: from instruction `pc` at `pos`, the alternatives in order, each choice undone when the rest fails;
    /// the end of the first path that reaches `Match` or `SubMatch`, with the slots it set.
    fn backtrack_from(&self, pc: usize, pos: usize, slots: &mut Vec<Option<usize>>, loops: &mut Vec<usize>, chars: &[char], budget: &mut u64) -> Result<Option<usize>, LimitReached> {
        let mut stack = vec![Job::Try(pc, pos)];
        while let Some(job) = stack.pop() {
            let (mut pc, mut pos) = match job {
                Job::Slot(n, v) => {
                    slots[n] = v;
                    continue;
                }
                Job::Loop(k, v) => {
                    loops[k] = v;
                    continue;
                }
                Job::Try(pc, pos) => (pc, pos),
            };
            loop {
                *budget = budget.checked_sub(1).ok_or(LimitReached)?;
                let inst = &self.program[pc];
                match inst {
                    Inst::Char(..) | Inst::Any(_) | Inst::Class(_) => {
                        if pos < chars.len() && self.reads(inst, chars[pos]) {
                            pc += 1;
                            pos += 1;
                        } else {
                            break;
                        }
                    }
                    Inst::Bol(_) | Inst::Eol(_) | Inst::EndOnly | Inst::WordBoundary(_) | Inst::WordEdge(_) => {
                        if self.holds(inst, pos, chars) {
                            pc += 1;
                        } else {
                            break;
                        }
                    }
                    Inst::Split(a, b) => {
                        stack.push(Job::Try(*b, pos));
                        pc = *a;
                    }
                    Inst::Jmp(t) => pc = *t,
                    Inst::Save(n) => {
                        stack.push(Job::Slot(*n, slots[*n]));
                        slots[*n] = Some(pos);
                        pc += 1;
                    }
                    Inst::LoopMark(k) => {
                        stack.push(Job::Loop(*k, loops[*k]));
                        loops[*k] = pos;
                        pc += 1;
                    }
                    Inst::LoopCheck(k, exit) => pc = if loops[*k] == pos { *exit } else { pc + 1 },
                    Inst::Match | Inst::SubMatch => return Ok(Some(pos)),
                    Inst::Backref(..) | Inst::BackrefAny(..) => {
                        let (gs, icase) = match inst {
                            Inst::Backref(g, i) => (std::slice::from_ref(g), *i),
                            Inst::BackrefAny(gs, i) => (gs.as_slice(), *i),
                            _ => unreachable!(),
                        };
                        let set = gs.iter().find(|g| slots.get(2 * **g).copied().flatten().is_some() && slots.get(2 * **g + 1).copied().flatten().is_some());
                        let span = set.map(|g| (slots[2 * g].unwrap_or(0), slots[2 * g + 1].unwrap_or(0)));
                        match span {
                            Some((a, b)) if b >= a && pos + (b - a) <= chars.len() && (0..b - a).all(|i| same_char(chars[a + i], chars[pos + i], icase)) => {
                                pos += b - a;
                                pc += 1;
                            }
                            _ => break,
                        }
                    }
                    Inst::LookAhead { negate, end } => {
                        let mut inner = slots.clone();
                        let hit = self.backtrack_from(pc + 1, pos, &mut inner, &mut loops.clone(), chars, budget)?.is_some();
                        if hit == *negate {
                            break;
                        }
                        // a positive assertion keeps the groups it set
                        if hit {
                            self.take_slots(slots, inner, &mut stack);
                        }
                        pc = *end;
                    }
                    Inst::LookBehind { negate, branches, end } => {
                        let mut found = None;
                        for (len, start) in branches {
                            if *len > pos {
                                continue;
                            }
                            let mut inner = slots.clone();
                            if self.backtrack_from(*start, pos - len, &mut inner, &mut loops.clone(), chars, budget)? == Some(pos) {
                                found = Some(inner);
                                break;
                            }
                        }
                        if found.is_some() == *negate {
                            break;
                        }
                        if let Some(inner) = found {
                            self.take_slots(slots, inner, &mut stack);
                        }
                        pc = *end;
                    }
                    Inst::Atomic { end } => {
                        let mut inner = slots.clone();
                        match self.backtrack_from(pc + 1, pos, &mut inner, &mut loops.clone(), chars, budget)? {
                            Some(e) => {
                                self.take_slots(slots, inner, &mut stack);
                                pos = e;
                                pc = *end;
                            }
                            None => break,
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// The slots a sub-match set, taken over so that going back undoes them.
    fn take_slots(&self, slots: &mut [Option<usize>], inner: Vec<Option<usize>>, stack: &mut Vec<Job>) {
        for (n, v) in inner.into_iter().enumerate() {
            if slots[n] != v {
                stack.push(Job::Slot(n, slots[n]));
                slots[n] = v;
            }
        }
    }

    /// Every thread of `current` that reads the character at `i`, moved on past it (in priority order), at the next position's stamp.
    fn step(&self, current: &[Thread], i: usize, chars: &[char], seen: &mut [usize], keep: bool) -> Vec<Thread> {
        let c = chars[i];
        let mut next: Vec<Thread> = Vec::new();
        for t in current {
            if self.reads(&self.program[t.pc], c) {
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
        let inst = &self.program[t.pc];
        match inst {
            Inst::Jmp(to) => self.add(list, Thread { pc: *to, slots: t.slots }, pos, chars, seen, keep),
            Inst::Split(a, b) => {
                self.add(list, Thread { pc: *a, slots: t.slots.clone() }, pos, chars, seen, keep);
                self.add(list, Thread { pc: *b, slots: t.slots }, pos, chars, seen, keep);
            }
            Inst::Bol(_) | Inst::Eol(_) | Inst::EndOnly | Inst::WordBoundary(_) | Inst::WordEdge(_) => {
                if self.holds(inst, pos, chars) {
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

/// Every back reference names a group the pattern has.
fn backrefs_exist(n: &Node, groups: usize) -> bool {
    match n {
        Node::Backref(g, _) => *g >= 1 && *g <= groups,
        Node::BackrefAny(gs, _) => gs.iter().all(|g| *g >= 1 && *g <= groups),
        Node::Group(inner, _) | Node::Atomic(inner) | Node::LookAhead { inner, .. } => backrefs_exist(inner, groups),
        Node::Repeat { node, .. } => backrefs_exist(node, groups),
        Node::Seq(items) | Node::Alt(items) => items.iter().all(|i| backrefs_exist(i, groups)),
        Node::LookBehind { branches, .. } => branches.iter().all(|(_, b)| backrefs_exist(b, groups)),
        _ => true,
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
        for bad in ["(a", "*a", "[b-a]", "x{", "a\\1", "(?<=a+)b", "\\q"] {
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
        // **Differs** (the module doc): PCRE2 compiles these, this port does not
        for unread in ["\\X41", "\\K", "(?|a)", "(?(?=a)a|b)", "(?R)", "(*UTF)a"] {
            assert!(Regex::compile(unread, false, Flavor::Find).is_none(), "{unread:?}");
        }
        // the net class flavour keeps its own syntax
        assert!(Regex::compile("[[:blank:]]", false, Flavor::NetClass).is_none() && Regex::compile("\\w", false, Flavor::NetClass).unwrap().is_match("é"));
    }

    /// UI-LEFT-3 C3-11: back references, look-arounds, named groups, inline options, `\\Q..\\E`, `\\p{..}`, atomic groups and possessive quantifiers,
    /// `\\o`, `\\c`, `\\R`: each case's answer is what the box's `libpcre2-32` (10.42, wxWidgets 3.2's flags `PCRE2_UTF | PCRE2_ALT_BSUX |
    /// PCRE2_DOTALL`) gave for it -- `None`: it does not compile; `Some(None)`: no match; else the match's span in characters.
    #[test]
    fn pcre2s_other_forms_match_as_libpcre2_does() {
        let cases: &[(&str, &str, Option<Option<(usize, usize)>>)] = &[
        ("(a)\\1", "xaay", Some(Some((1, 3)))),
        ("(a)\\1", "xaby", Some(None)),
        ("(\\w+) \\1", "say hello hello", Some(Some((4, 15)))),
        ("(?i)(a)\\1", "aA", Some(Some((0, 2)))),
        ("(a)|b\\1", "b", Some(None)),
        ("(?<n>ab)\\k<n>", "abab", Some(Some((0, 4)))),
        ("(?'n'ab)\\k'n'", "abab", Some(Some((0, 4)))),
        ("(?P<n>ab)(?P=n)", "abab", Some(Some((0, 4)))),
        ("(?<n>a)\\k{n}", "aa", Some(Some((0, 2)))),
        ("(?<n>a)\\g{n}", "aa", Some(Some((0, 2)))),
        ("(a)(b)\\g{-1}", "abb", Some(Some((0, 3)))),
        ("(a)\\g1", "aa", Some(Some((0, 2)))),
        ("(a)\\g{1}", "aa", Some(Some((0, 2)))),
        ("a(?=b)", "acab", Some(Some((2, 3)))),
        ("a(?!b)", "abac", Some(Some((2, 3)))),
        ("(?<=a)b", "cbab", Some(Some((3, 4)))),
        ("(?<!a)b", "abcb", Some(Some((3, 4)))),
        ("(?<=ab|c)d", "xcd", Some(Some((2, 3)))),
        ("(?<=a(b))c", "abc", Some(Some((2, 3)))),
        ("\\Qa.b*\\E+", "a.b**", Some(Some((0, 5)))),
        ("[\\Q]\\E]", "]", Some(Some((0, 1)))),
        ("\\Qa.b", "xa.b", Some(Some((1, 4)))),
        ("(?i)abc", "xABC", Some(Some((1, 4)))),
        ("a(?i)bc", "aBC", Some(Some((0, 3)))),
        ("a(?i)bc", "ABC", Some(None)),
        ("(?i:a)b", "Ab", Some(Some((0, 2)))),
        ("(?i:a)b", "AB", Some(None)),
        ("(a(?i)b|c)", "C", Some(Some((0, 1)))),
        ("(?m)^b$", "a\nb\nc", Some(Some((2, 3)))),
        ("^b$", "a\nb\nc", Some(None)),
        ("(?s-s)a.b", "a\nb", Some(None)),
        ("(?x) a b c # comment", "abc", Some(Some((0, 3)))),
        ("(?x)a[ ]b", "a b", Some(Some((0, 3)))),
        ("(?n)(a)(?<x>b)\\k<x>", "abb", Some(Some((0, 3)))),
        ("(?U)a+", "aaa", Some(Some((0, 1)))),
        ("(?U)a+?", "aaa", Some(Some((0, 3)))),
        ("(?^i)A", "a", Some(Some((0, 1)))),
        ("(?i)(?^)A", "a", Some(None)),
        ("\\p{Lu}+", "abCDe", Some(Some((2, 4)))),
        ("\\P{L}", "ab1", Some(Some((2, 3)))),
        ("\\pL+", "12ab", Some(Some((2, 4)))),
        ("\\p{Greek}+", "a\u{03b1}\u{03b2}\u{03b3}b", Some(Some((1, 4)))),
        ("\\p{L&}", "1A", Some(Some((1, 2)))),
        ("\\p{Xan}+", "--a1--", Some(Some((2, 4)))),
        ("\\p{Nd}", "x\u{0663}", Some(Some((1, 2)))),
        ("[\\p{Lu}\\d]+", "xA1B2y", Some(Some((1, 5)))),
        ("[^\\p{L}]", "ab-", Some(Some((2, 3)))),
        ("\\p{^Lu}", "AbC", Some(Some((1, 2)))),
        ("\\p{Han}", "a\u{4e2d}", Some(Some((1, 2)))),
        ("\\p{sc:Greek}", "\u{0345}\u{03b1}", Some(Some((1, 2)))),
        ("\\p{scx:Greek}", "\u{0345}\u{03b1}", Some(Some((0, 1)))),
        ("\\p{Alphabetic}+", "12ab", Some(Some((2, 4)))),
        ("\\p{White_Space}", "a\u{2003}b", Some(Some((1, 2)))),
        ("(?>a+)b", "aaab", Some(Some((0, 4)))),
        ("(?>a+)a", "aaa", Some(None)),
        ("a++b", "aab", Some(Some((0, 3)))),
        ("a*+a", "aaa", Some(None)),
        ("a?+", "a", Some(Some((0, 1)))),
        ("\\o{101}", "A", Some(Some((0, 1)))),
        ("\\101", "A", Some(Some((0, 1)))),
        ("\\cA", "\u{0001}", Some(Some((0, 1)))),
        ("\\R", "a\r\nb", Some(Some((1, 3)))),
        ("(a*)*b", "b", Some(Some((0, 1)))),
        ("(a|b)*?c", "abc", Some(Some((0, 3)))),
        ("(?<=\\d{3})x", "123x", Some(Some((3, 4)))),
        ("x(?=(\\d))\\1", "x1", Some(Some((0, 2)))),
        ("(?<=a+)b", "ab", None),
        ("\\k<nope>", "a", None),
        ("(a)\\2", "aa", None),
        ("(?<n>a)(?<n>b)", "ab", None),
        ("(?J)(?<n>a)|(?<n>b)\\k<n>", "bb", Some(Some((0, 2)))),
        ];
        for (p, t, want) in cases {
            let got = Regex::compile(p, false, Flavor::Find).map(|r| r.find_at(&t.chars().collect::<Vec<_>>(), 0).and_then(|c| c.group(0)));
            assert_eq!(got, *want, "{p:?} on {t:?}");
        }
        // the choosers' flavour reads them too
        assert!(Regex::compile("(?<n>R)\\d\\k<n>", false, Flavor::Advanced).unwrap().is_match("R1R"));
        // a back reference's groups, and a replacement with them
        assert_eq!(find("(?<a>\\w)(?<b>\\w)").replace_all("ab cd", "\\2\\1").as_deref(), Some("ba dc"));
        // the match limit: a pattern that backtracks without end gives up (`PCRE2_ERROR_MATCHLIMIT` is no match), quickly
        let long = format!("{}!", "a".repeat(60));
        let t0 = std::time::Instant::now();
        assert!(!find("(?:(a+)+)\\1$").is_match(&long));
        assert!(t0.elapsed().as_secs() < 30);
        // the net class flavour keeps its syntax
        assert!(Regex::compile("(a)\\1", false, Flavor::NetClass).is_none() && Regex::compile("\\p{L}", false, Flavor::NetClass).is_none());
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
        assert!(adv("(?m)a").is_none(), "a leading option wxWidgets' ConvertMetasyntax does not take");
        // the net class flavour keeps its own rules
        assert!(Regex::compile(r"\yres", false, Flavor::NetClass).is_none());
    }
}
