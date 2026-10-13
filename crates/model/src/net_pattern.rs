//! The patterns that assign nets to net classes (Board Setup > Design Rules > Net Classes, "Netclass Assignments"): `EDA_COMBINED_MATCHER( pattern, CTX_NETCLASS )`.
//!
//! A net belongs to a pattern when *either* reading of the pattern matches the whole net name: as a regular expression (`EDA_PATTERN_MATCH_REGEX_ANCHORED`: `^` and `$` are
//! added when the pattern lacks them) or as a wildcard pattern (`EDA_PATTERN_MATCH_WILDCARD_ANCHORED`: `*` is any run of characters, `?` any one character, everything else
//! literal). So `USB_D.*`, `USB_D?` and `USB_D[PM]` all name the nets of a USB pair, and a pattern that is no regular expression (`USB_(`) still works as a wildcard pattern.
//!
//! The regular expressions are wxWidgets' advanced ones; this reads the part of them net names are matched with -- literals, `.`, `[..]` classes (ranges, `^` negation, `[:alpha:]` and
//! the like, the `\d \w \s` shorthands), `( .. )` and `(?: .. )` groups, `|`, and `* + ? {m} {m,} {m,n}` with an optional lazy `?` (which cannot change a match of the whole name).
//! Matching is by simulation of the machine the pattern compiles to, so no pattern can take longer than its length times the net name's. A pattern with anything else in it (a
//! back reference, a look-ahead, a word boundary) is no regular expression here, and matches as a wildcard pattern only -- as KiCad treats one wxRegEx cannot compile.
//!
//! Ported from common/eda_pattern_match.cpp (`EDA_COMBINED_MATCHER::EDA_COMBINED_MATCHER`, `EDA_PATTERN_MATCH_REGEX_ANCHORED::SetPattern`,
//! `EDA_PATTERN_MATCH_WILDCARD_ANCHORED::SetPattern`, `EDA_COMBINED_MATCHER::StartsWith`).

use crate::glob_match;

/// The most instructions a pattern may compile to: counted repeats are copied, so `(a{100}){100}` would be ten thousand. Over this, the pattern is no regular expression.
const MAX_PROGRAM: usize = 4_000;
/// The deepest nesting of groups a pattern may have.
const MAX_DEPTH: usize = 64;

/// Whether the whole of `net` matches `pattern` read as a regular expression or as a wildcard pattern: `EDA_COMBINED_MATCHER::StartsWith( net )` in the net class context.
pub fn pattern_matches(pattern: &str, net: &str) -> bool {
    glob_match(pattern, net) || regex_matches(pattern, net)
}

/// Whether the whole of `net` matches `pattern` read as an anchored regular expression. False when the pattern is no regular expression this reads.
pub fn regex_matches(pattern: &str, net: &str) -> bool {
    Regex::new(pattern).is_some_and(|r| r.is_match(net))
}

/// Whether `pattern` compiles as a regular expression (what a page can say about a pattern that does not: it only works as a wildcard pattern).
pub fn is_regex(pattern: &str) -> bool {
    Regex::new(pattern).is_some()
}

/// Whether `pattern` has a character a regular expression gives a meaning to besides `*` and `?` (which are wildcards too): a pattern without one reads the same either way.
pub fn has_regex_syntax(pattern: &str) -> bool {
    pattern.chars().any(|c| ".+()[]{}|^$\\".contains(c))
}

// ------------------------------------------------------------------------------------------------------------------------------------ the pattern

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Char(char),
    Any,
    Class { items: Vec<ClassItem>, negate: bool },
    Bol,
    Eol,
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Repeat { node: Box<Node>, min: usize, max: Option<usize> },
}

#[derive(Debug, Clone, PartialEq)]
enum ClassItem {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
    Posix(PosixClass),
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
}

fn class_matches(items: &[ClassItem], negate: bool, c: char) -> bool {
    let hit = items.iter().any(|i| match *i {
        ClassItem::Range(a, b) => a <= c && c <= b,
        ClassItem::Digit(yes) => c.is_ascii_digit() == yes,
        ClassItem::Word(yes) => (c.is_alphanumeric() || c == '_') == yes,
        ClassItem::Space(yes) => c.is_whitespace() == yes,
        ClassItem::Posix(p) => match p {
            PosixClass::Alpha => c.is_alphabetic(),
            PosixClass::Digit => c.is_ascii_digit(),
            PosixClass::Alnum => c.is_alphanumeric(),
            PosixClass::Upper => c.is_uppercase(),
            PosixClass::Lower => c.is_lowercase(),
            PosixClass::Space => c.is_whitespace(),
            PosixClass::Punct => c.is_ascii_punctuation(),
            PosixClass::Xdigit => c.is_ascii_hexdigit(),
        },
    });
    hit != negate
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    depth: usize,
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
                // `{m}`, `{m,}`, `{m,n}`; a `{` that is no count is a literal in a wildcard reading only, so here it is an error (wxRegEx refuses it too).
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
        // A lazy quantifier matches the same names as a greedy one when the whole name must match.
        self.eat('?');
        if matches!(self.peek(), Some('*' | '+' | '{')) {
            return None; // a second quantifier on the same atom
        }
        Some(Node::Repeat { node: Box::new(atom), min, max })
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
                if self.eat('?') {
                    if !self.eat(':') {
                        return None; // look-ahead, named groups and flags are not read
                    }
                }
                let inner = self.alt()?;
                if !self.eat(')') {
                    return None;
                }
                self.depth -= 1;
                Some(inner)
            }
            ')' | '*' | '+' | '?' | '{' => None,
            '.' => Some(Node::Any),
            '^' => Some(Node::Bol),
            '$' => Some(Node::Eol),
            '[' => self.class(),
            '\\' => {
                let e = self.peek()?;
                self.pos += 1;
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
                        'n' => '\n',
                        't' => '\t',
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
    Split(usize, usize),
    Jmp(usize),
    Bol,
    Eol,
    Match,
}

/// A compiled pattern: matches a whole name.
pub struct Regex {
    program: Vec<Inst>,
    classes: Vec<(Vec<ClassItem>, bool)>,
}

impl Regex {
    /// Compile `pattern` as KiCad's anchored regular expression; `None` when it is not one this reads.
    pub fn new(pattern: &str) -> Option<Regex> {
        let mut p = Parser { chars: pattern.chars().collect(), pos: 0, depth: 0 };
        let ast = p.alt()?;
        if p.pos != p.chars.len() {
            return None; // an unmatched `)`
        }
        let mut r = Regex { program: Vec::new(), classes: Vec::new() };
        r.emit(&ast)?;
        r.program.push(Inst::Match);
        Some(r)
    }

    fn push(&mut self, i: Inst) -> Option<usize> {
        if self.program.len() >= MAX_PROGRAM {
            return None;
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
            Node::Seq(items) => {
                for i in items {
                    self.emit(i)?;
                }
            }
            Node::Alt(branches) => {
                // split L1, L2; L1: a; jmp end; L2: split ...
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
            Node::Repeat { node, min, max } => {
                for _ in 0..*min {
                    self.emit(node)?;
                }
                match max {
                    None => {
                        // L: split body, end; body; jmp L
                        let split = self.push(Inst::Split(0, 0))?;
                        self.emit(node)?;
                        self.push(Inst::Jmp(split))?;
                        let end = self.program.len();
                        self.program[split] = Inst::Split(split + 1, end);
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
                            self.program[s] = Inst::Split(s + 1, end);
                        }
                    }
                }
            }
        }
        Some(())
    }

    /// Whether the whole of `text` matches.
    pub fn is_match(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        let n = self.program.len();
        let mut current: Vec<usize> = Vec::new();
        let mut next: Vec<usize> = Vec::new();
        let mut seen = vec![usize::MAX; n];
        self.add(&mut current, 0, 0, chars.len(), &mut seen, 0);
        for (i, &c) in chars.iter().enumerate() {
            // `seen` is keyed by the step so a thread is added once per position.
            let step = i + 1;
            next.clear();
            for &pc in &current {
                let advance = match &self.program[pc] {
                    Inst::Char(x) => *x == c,
                    Inst::Any => true,
                    Inst::Class(k) => class_matches(&self.classes[*k].0, self.classes[*k].1, c),
                    _ => false,
                };
                if advance {
                    self.add(&mut next, pc + 1, step, chars.len(), &mut seen, step);
                }
            }
            std::mem::swap(&mut current, &mut next);
            if current.is_empty() {
                return false;
            }
        }
        current.iter().any(|&pc| matches!(self.program[pc], Inst::Match))
    }

    /// Add the thread at `pc` and everything it reaches without reading a character, at position `pos`.
    fn add(&self, list: &mut Vec<usize>, pc: usize, pos: usize, len: usize, seen: &mut [usize], mark: usize) {
        if seen[pc] == mark {
            return;
        }
        seen[pc] = mark;
        match &self.program[pc] {
            Inst::Jmp(t) => self.add(list, *t, pos, len, seen, mark),
            Inst::Split(a, b) => {
                self.add(list, *a, pos, len, seen, mark);
                self.add(list, *b, pos, len, seen, mark);
            }
            Inst::Bol => {
                if pos == 0 {
                    self.add(list, pc + 1, pos, len, seen, mark);
                }
            }
            Inst::Eol => {
                if pos == len {
                    self.add(list, pc + 1, pos, len, seen, mark);
                }
            }
            _ => list.push(pc),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yes(pattern: &str, net: &str) {
        assert!(pattern_matches(pattern, net), "{pattern:?} should match {net:?}");
    }
    fn no(pattern: &str, net: &str) {
        assert!(!pattern_matches(pattern, net), "{pattern:?} should not match {net:?}");
    }

    #[test]
    fn a_wildcard_pattern_is_anchored_and_a_name_is_a_pattern() {
        yes("GND", "GND");
        no("GND", "GND2");
        no("GND", "AGND");
        yes("USB_*", "USB_DP");
        yes("USB_D?", "USB_DP");
        yes("USB_D?", "USB_D"); // as a regular expression the `?` makes the D optional: either reading will do
        no("USB_D?", "USB_DPM");
        yes("*", "anything");
        yes("/Sheet1/NET", "/Sheet1/NET");
        yes("A*B*C", "AxxBxxC");
        no("A*B*C", "AxxBxx");
    }

    #[test]
    fn a_regular_expression_is_anchored_too_and_either_reading_will_do() {
        yes("USB_D[PM]", "USB_DP");
        yes("USB_D[PM]", "USB_DM");
        no("USB_D[PM]", "USB_DX");
        no("USB_D[PM]", "USB_DPM");
        yes("USB_D.*", "USB_DP2");
        yes("^VBUS$", "VBUS");
        no("^VBUS$", "VBUS2");
        yes("(DP|DM)", "DP");
        no("(DP|DM)", "DPM");
        yes("D[0-9]+", "D15");
        no("D[0-9]+", "D");
        yes("D\\d{1,2}", "D7");
        yes("D\\d{1,2}", "D15");
        no("D\\d{1,2}", "D150");
        yes("A(?:B|C)D?", "AC");
        yes("A(?:B|C)D?", "ABD");
        yes("[[:alpha:]]+_[[:digit:]]", "MOSI_1");
        yes("[^A-Z]+", "abc_1");
        no("[^A-Z]+", "abC");
        // a regular expression and a wildcard pattern both read: `USB_*` as a regex is `USB` and any run of underscores, as a wildcard pattern it is every USB_ net
        yes("USB_*", "USB");
        yes("USB_*", "USB___");
        yes("USB_*", "USB_DP");
        // `.` is any character, so a pattern with a dot reads as a regex; the literal reading matches the dotted name too
        yes("V3.3", "V3.3");
        yes("V3.3", "V3x3");
    }

    #[test]
    fn a_pattern_that_is_no_regular_expression_is_still_a_wildcard_pattern() {
        assert!(!is_regex("USB_(") && !is_regex("USB_D[") && !is_regex("a**") && !is_regex("a\\1") && !is_regex("(?=a)") && !is_regex("a{3,1}") && !is_regex("x\\bX"));
        yes("USB_(", "USB_(");
        yes("USB_[*", "USB_[DP");
        no("USB_(", "USB_");
        assert!(is_regex("USB_D[PM]") && is_regex("(a|b)+c?") && is_regex("") && is_regex("a{2}") && is_regex("a{2,}") && is_regex("a+?"));
    }

    #[test]
    fn matching_does_not_blow_up_on_patterns_made_to_make_a_backtracking_matcher_blow_up() {
        let net = format!("{}!", "a".repeat(200));
        assert!(!pattern_matches("(a+)+$", &net));
        assert!(!pattern_matches("(a|aa)+b", &net));
        assert!(!pattern_matches("(a*)*b", &net));
        assert!(pattern_matches("(a*)*", &"a".repeat(200)));
        // a program that would have a hundred thousand instructions is no regular expression
        assert!(!is_regex("((((a{200}){200}){200}){200})"));
        assert!(!is_regex(&"(".repeat(100)));
        assert!(!is_regex(&format!("{}a{}", "(".repeat(100), ")".repeat(100))), "nested deeper than a pattern may be");
    }

    #[test]
    fn the_empty_net_and_the_empty_pattern() {
        assert!(pattern_matches("", ""));
        assert!(!pattern_matches("", "A"));
        assert!(pattern_matches("*", ""));
        assert!(pattern_matches("A*", "A"));
        assert!(has_regex_syntax("a.b") && has_regex_syntax("[ab]") && !has_regex_syntax("USB_*") && !has_regex_syntax("D?"));
    }

    #[test]
    fn counted_repeats_and_alternation_in_the_middle() {
        yes("(CLK|DAT)[0-3]", "CLK2");
        yes("(CLK|DAT)[0-3]", "DAT0");
        no("(CLK|DAT)[0-3]", "CLK4");
        yes("A{3}", "AAA");
        no("A{3}", "AA");
        yes("A{2,3}B", "AAAB");
        no("A{2,3}B", "AAAAB");
        yes("(AB){2}", "ABAB");
        yes("x|y|z", "y");
        no("x|y|z", "xy");
        yes("\\.", ".");
        no("\\.", "x");
        yes("[a\\]b]+", "a]b");
        yes("[-a]+", "a-");
        yes("[a-]+", "-a");
    }
}
