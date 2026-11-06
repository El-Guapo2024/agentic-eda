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
//! **Differs**, knowingly, from wxRegEx (POSIX): a match is the *leftmost-first* one (alternatives tried left to right, a greedy repeat taking as many as it
//! can before the rest is tried, as Perl and PCRE do), where POSIX takes the *leftmost-longest*. Whether a text matches, and where the first match starts, are
//! the same either way; what can differ is the span of a match when alternatives of different lengths match at one start -- `a|ab` on `ab` is `a` here and
//! `ab` in POSIX -- which shows only in Replace's span with alternations (a net class pattern only asks whether a match starts at the name's first character,
//! `EDA_COMBINED_MATCHER::StartsWith`). A search that is not case-sensitive folds the case of the text and the pattern alike (KiCad upper-cases both before
//! compiling with `wxRE_ICASE`, which turns `\d` into `\D`).

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
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Char(char),
    Any,
    Class { items: Vec<ClassItem>, negate: bool },
    Bol,
    Eol,
    WordBoundary(bool),
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

fn item_matches(i: &ClassItem, c: char) -> bool {
    match *i {
        ClassItem::Range(a, b) => a <= c && c <= b,
        ClassItem::Digit(yes) => c.is_ascii_digit() == yes,
        ClassItem::Word(yes) => is_word(c) == yes,
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
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn class_matches(items: &[ClassItem], negate: bool, c: char, icase: bool) -> bool {
    let test = |c: char| items.iter().any(|i| item_matches(i, c));
    let hit = test(c) || (icase && (c.to_lowercase().any(test) || c.to_uppercase().any(test)));
    hit != negate
}

fn same_char(a: char, b: char, icase: bool) -> bool {
    a == b || (icase && a.to_lowercase().eq(b.to_lowercase()))
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
            '[' => self.class(),
            '\\' => {
                let e = self.peek()?;
                self.pos += 1;
                match e {
                    'd' | 'D' | 'w' | 'W' | 's' | 'S' => Some(Node::Class { items: vec![shorthand(e)], negate: false }),
                    'n' => Some(Node::Char('\n')),
                    't' => Some(Node::Char('\t')),
                    'b' if self.flavor == Flavor::Find => Some(Node::WordBoundary(true)),
                    'B' if self.flavor == Flavor::Find => Some(Node::WordBoundary(false)),
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
    /// Two ways on, the first preferred.
    Split(usize, usize),
    Jmp(usize),
    Bol,
    Eol,
    WordBoundary(bool),
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
        let mut p = Parser { chars: pattern.chars().collect(), pos: 0, depth: 0, groups: 0, flavor };
        let ast = p.alt()?;
        if p.pos != p.chars.len() {
            return None; // an unmatched `)`
        }
        let mut r = Regex { program: Vec::new(), classes: Vec::new(), groups: p.groups, icase, counted: 0 };
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
            Node::WordBoundary(yes) => {
                self.push(Inst::WordBoundary(*yes))?;
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
                Inst::Class(k) => class_matches(&self.classes[*k].0, self.classes[*k].1, c, self.icase),
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
            Inst::Eol => {
                if pos == chars.len() {
                    self.add(list, Thread { pc: t.pc + 1, slots: t.slots }, pos, chars, seen, keep);
                }
            }
            Inst::WordBoundary(want) => {
                let before = pos > 0 && is_word(chars[pos - 1]);
                let after = pos < chars.len() && is_word(chars[pos]);
                if (before != after) == *want {
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
        // the Differs of the module doc: leftmost-first takes the first alternative that matches, POSIX the longest
        assert_eq!(find("a|ab").replace_all("ab", "x").as_deref(), Some("xb"));
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
}
