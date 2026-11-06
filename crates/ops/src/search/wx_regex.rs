//! The regular expressions of Find / Replace's "Regular Expression" (`EDA_SEARCH_MATCH_MODE::REGEX`, `EDA_ITEM::Matches` / `EDA_ITEM::Replace`, common/eda_item.cpp at
//! 8303b2ad): KiCad compiles the search text with `wxRegEx::Compile( findString, matchCase ? 0 : wxRE_ICASE )` -- POSIX extended syntax (`wxRE_EXTENDED`, the default)
//! -- and asks `wxRegEx::Matches( text )` (a match anywhere in the text) and `wxRegEx::Replace( &text, replaceString )` (every match, left to right, none
//! overlapping; `\N` in the replacement is the Nth sub-expression, `&` and `\0` the whole match, `\&` a literal `&`).
//!
//! The workspace takes no regex crate (no new dependency), so this is a small backtracking matcher of that syntax: literals, `.`, bracket expressions with
//! ranges, negation and the POSIX classes (`[[:alpha:]]` ..), `^` and `$`, groups, `|`, and the quantifiers `*`, `+`, `?`, `{m}`, `{m,}`, `{m,n}`. **Differs**,
//! knowingly: the escapes `\d \w \s \D \W \S \b \B` of wxWidgets' advanced and PCRE flavours are taken too (a user of KiCad 9 and 10, whose wxWidgets uses
//! PCRE, writes them), a quantifier followed by `?` is lazy (PCRE), alternatives are tried left to right (POSIX takes the longest), and a search that is not
//! case-sensitive matches without regard to case (KiCad upper-cases the pattern first as it does the text, which turns `\d` into `\D`). A pattern that does
//! not compile matches nothing (`if( !regex.Compile( .. ) ) return false`).

#[derive(Debug, Clone)]
enum Node {
    Char(char),
    Any,
    Class(Class),
    Start,
    End,
    WordBoundary(bool),
    Group(Box<Node>, usize),
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    Repeat { node: Box<Node>, min: usize, max: Option<usize>, greedy: bool },
}

#[derive(Debug, Clone)]
struct Class {
    negated: bool,
    items: Vec<ClassItem>,
}

#[derive(Debug, Clone)]
enum ClassItem {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
    Posix(String),
}

/// A compiled expression.
#[derive(Debug, Clone)]
pub struct Regex {
    root: Node,
    groups: usize,
    icase: bool,
}

type Caps = Vec<Option<(usize, usize)>>;

struct Parser<'a> {
    p: &'a [char],
    i: usize,
    groups: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.p.get(self.i).copied()
    }

    fn alt(&mut self) -> Result<Node, String> {
        let mut alts = vec![self.concat()?];
        while self.peek() == Some('|') {
            self.i += 1;
            alts.push(self.concat()?);
        }
        Ok(if alts.len() == 1 { alts.pop().expect("one") } else { Node::Alt(alts) })
    }

    fn concat(&mut self) -> Result<Node, String> {
        let mut out = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom()?;
            out.push(self.quantified(atom)?);
        }
        Ok(Node::Concat(out))
    }

    fn number(&mut self) -> Option<usize> {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        (self.i > start).then(|| self.p[start..self.i].iter().collect::<String>().parse().ok()).flatten()
    }

    fn quantified(&mut self, atom: Node) -> Result<Node, String> {
        let mut node = atom;
        loop {
            let (min, max) = match self.peek() {
                Some('*') => (0, None),
                Some('+') => (1, None),
                Some('?') => (0, Some(1)),
                Some('{') => {
                    // `{m}`, `{m,}`, `{m,n}`; anything else is a literal brace
                    let save = self.i;
                    self.i += 1;
                    let Some(m) = self.number() else {
                        self.i = save;
                        return Ok(node);
                    };
                    let max = if self.peek() == Some(',') {
                        self.i += 1;
                        self.number()
                    } else {
                        Some(m)
                    };
                    if self.peek() != Some('}') || max.is_some_and(|x| x < m) {
                        self.i = save;
                        return Ok(node);
                    }
                    (m, max)
                }
                _ => return Ok(node),
            };
            self.i += 1;
            let greedy = if self.peek() == Some('?') {
                self.i += 1;
                false
            } else {
                true
            };
            if matches!(node, Node::Start | Node::End) {
                return Err("nothing to repeat".into());
            }
            node = Node::Repeat { node: Box::new(node), min, max, greedy };
        }
    }

    fn atom(&mut self) -> Result<Node, String> {
        let c = self.peek().ok_or("unexpected end")?;
        self.i += 1;
        Ok(match c {
            '(' => {
                self.groups += 1;
                let n = self.groups;
                let inner = self.alt()?;
                if self.peek() != Some(')') {
                    return Err("unmatched (".into());
                }
                self.i += 1;
                Node::Group(Box::new(inner), n)
            }
            '[' => Node::Class(self.class()?),
            '.' => Node::Any,
            '^' => Node::Start,
            '$' => Node::End,
            '*' | '+' | '?' => return Err("nothing to repeat".into()),
            '\\' => {
                let e = self.peek().ok_or("trailing backslash")?;
                self.i += 1;
                match e {
                    'd' => Node::Class(Class { negated: false, items: vec![ClassItem::Digit(true)] }),
                    'D' => Node::Class(Class { negated: false, items: vec![ClassItem::Digit(false)] }),
                    'w' => Node::Class(Class { negated: false, items: vec![ClassItem::Word(true)] }),
                    'W' => Node::Class(Class { negated: false, items: vec![ClassItem::Word(false)] }),
                    's' => Node::Class(Class { negated: false, items: vec![ClassItem::Space(true)] }),
                    'S' => Node::Class(Class { negated: false, items: vec![ClassItem::Space(false)] }),
                    'b' => Node::WordBoundary(true),
                    'B' => Node::WordBoundary(false),
                    't' => Node::Char('\t'),
                    'n' => Node::Char('\n'),
                    other => Node::Char(other),
                }
            }
            other => Node::Char(other),
        })
    }

    fn class(&mut self) -> Result<Class, String> {
        let mut negated = false;
        if self.peek() == Some('^') {
            negated = true;
            self.i += 1;
        }
        let mut items = Vec::new();
        let mut first = true;
        loop {
            let c = self.peek().ok_or("unmatched [")?;
            if c == ']' && !first {
                self.i += 1;
                break;
            }
            first = false;
            // `[:alpha:]` and the other POSIX classes
            if c == '[' && self.p.get(self.i + 1) == Some(&':') {
                let rest: String = self.p[self.i + 2..].iter().collect();
                if let Some(end) = rest.find(":]") {
                    items.push(ClassItem::Posix(rest[..end].to_string()));
                    self.i += 2 + end + 2;
                    continue;
                }
            }
            self.i += 1;
            let lo = if c == '\\' {
                let e = self.peek().ok_or("trailing backslash")?;
                self.i += 1;
                match e {
                    'd' => {
                        items.push(ClassItem::Digit(true));
                        continue;
                    }
                    'w' => {
                        items.push(ClassItem::Word(true));
                        continue;
                    }
                    's' => {
                        items.push(ClassItem::Space(true));
                        continue;
                    }
                    't' => '\t',
                    'n' => '\n',
                    other => other,
                }
            } else {
                c
            };
            if self.peek() == Some('-') && self.p.get(self.i + 1).is_some_and(|&n| n != ']') {
                self.i += 1;
                let hi = self.peek().ok_or("unmatched [")?;
                self.i += 1;
                if hi < lo {
                    return Err("invalid range".into());
                }
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(ClassItem::Range(lo, lo));
            }
        }
        Ok(Class { negated, items })
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn posix(name: &str, c: char) -> bool {
    match name {
        "alpha" => c.is_alphabetic(),
        "digit" => c.is_ascii_digit(),
        "alnum" => c.is_alphanumeric(),
        "upper" => c.is_uppercase(),
        "lower" => c.is_lowercase(),
        "space" => c.is_whitespace(),
        "blank" => c == ' ' || c == '\t',
        "punct" => c.is_ascii_punctuation(),
        "xdigit" => c.is_ascii_hexdigit(),
        "cntrl" => c.is_control(),
        "print" => !c.is_control(),
        "graph" => !c.is_control() && !c.is_whitespace(),
        _ => false,
    }
}

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

impl Class {
    fn has(&self, c: char, icase: bool) -> bool {
        let test = |c: char| {
            self.items.iter().any(|it| match it {
                ClassItem::Range(lo, hi) => (*lo..=*hi).contains(&c),
                ClassItem::Digit(on) => c.is_ascii_digit() == *on,
                ClassItem::Word(on) => is_word(c) == *on,
                ClassItem::Space(on) => c.is_whitespace() == *on,
                ClassItem::Posix(n) => posix(n, c),
            })
        };
        let hit = test(c) || (icase && (c.to_lowercase().any(&test) || c.to_uppercase().any(&test)));
        hit != self.negated
    }
}

impl Regex {
    /// `wxRegEx::Compile( pattern, icase ? wxRE_ICASE : 0 )`.
    pub fn new(pattern: &str, icase: bool) -> Result<Regex, String> {
        let chars: Vec<char> = pattern.chars().collect();
        let mut p = Parser { p: &chars, i: 0, groups: 0 };
        let root = p.alt()?;
        if p.i != chars.len() {
            return Err("unmatched )".into());
        }
        Ok(Regex { root, groups: p.groups, icase })
    }

    fn node(&self, n: &Node, t: &[char], i: usize, caps: &mut Caps, k: &mut dyn FnMut(usize, &mut Caps) -> bool) -> bool {
        match n {
            Node::Char(c) => i < t.len() && (t[i] == *c || (self.icase && fold(t[i]) == fold(*c))) && k(i + 1, caps),
            Node::Any => i < t.len() && t[i] != '\n' && k(i + 1, caps),
            Node::Class(cl) => i < t.len() && cl.has(t[i], self.icase) && k(i + 1, caps),
            Node::Start => i == 0 && k(i, caps),
            Node::End => i == t.len() && k(i, caps),
            Node::WordBoundary(want) => {
                let before = i > 0 && is_word(t[i - 1]);
                let after = i < t.len() && is_word(t[i]);
                (before != after) == *want && k(i, caps)
            }
            Node::Group(inner, g) => {
                let g = *g;
                self.node(inner, t, i, caps, &mut |j, caps: &mut Caps| {
                    let prev = caps[g];
                    caps[g] = Some((i, j));
                    if k(j, caps) {
                        return true;
                    }
                    caps[g] = prev;
                    false
                })
            }
            Node::Concat(list) => self.seq(list, t, i, caps, k),
            Node::Alt(alts) => alts.iter().any(|a| {
                let saved = caps.clone();
                if self.node(a, t, i, caps, k) {
                    return true;
                }
                *caps = saved;
                false
            }),
            Node::Repeat { node, min, max, greedy } => self.repeat(node, *min, *max, *greedy, 0, t, i, caps, k),
        }
    }

    fn seq(&self, list: &[Node], t: &[char], i: usize, caps: &mut Caps, k: &mut dyn FnMut(usize, &mut Caps) -> bool) -> bool {
        match list.split_first() {
            None => k(i, caps),
            Some((first, rest)) => self.node(first, t, i, caps, &mut |j, caps: &mut Caps| self.seq(rest, t, j, caps, k)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn repeat(&self, n: &Node, min: usize, max: Option<usize>, greedy: bool, count: usize, t: &[char], i: usize, caps: &mut Caps, k: &mut dyn FnMut(usize, &mut Caps) -> bool) -> bool {
        let more = max.is_none_or(|m| count < m);
        let again = |caps: &mut Caps, k: &mut dyn FnMut(usize, &mut Caps) -> bool| {
            more && self.node(n, t, i, caps, &mut |j, caps: &mut Caps| {
                // an empty pass is no progress: it may not loop
                if j == i && count >= min {
                    return false;
                }
                self.repeat(n, min, max, greedy, count + 1, t, j, caps, k)
            })
        };
        if greedy {
            if again(caps, k) {
                return true;
            }
            count >= min && k(i, caps)
        } else {
            if count >= min && k(i, caps) {
                return true;
            }
            again(caps, k)
        }
    }

    /// The first match at or after `from` as (start, end, captures) -- `^` holds at the text's start only (`wxRE_NOTBOL` after the first).
    fn find_at(&self, t: &[char], from: usize) -> Option<(usize, usize, Caps)> {
        for start in from..=t.len() {
            let mut caps: Caps = vec![None; self.groups + 1];
            let mut end = None;
            if self.node(&self.root, t, start, &mut caps, &mut |j, _| {
                end = Some(j);
                true
            }) {
                let e = end.expect("set on a match");
                caps[0] = Some((start, e));
                return Some((start, e, caps));
            }
        }
        None
    }

    /// `wxRegEx::Matches( text )`: is there a match anywhere in the text?
    pub fn is_match(&self, text: &str) -> bool {
        let t: Vec<char> = text.chars().collect();
        self.find_at(&t, 0).is_some()
    }

    /// `wxRegEx::Replace( &text, replacement )`: every match replaced, left to right (an empty match moves on by one character); `None` when nothing matched.
    pub fn replace_all(&self, text: &str, replacement: &str) -> Option<String> {
        let t: Vec<char> = text.chars().collect();
        let r: Vec<char> = replacement.chars().collect();
        let mut out = String::new();
        let mut at = 0;
        let mut n = 0;
        while at <= t.len() {
            let Some((s, e, caps)) = self.find_at(&t, at) else { break };
            out.extend(&t[at..s]);
            let mut k = 0;
            while k < r.len() {
                match r[k] {
                    '\\' if k + 1 < r.len() && r[k + 1].is_ascii_digit() => {
                        let g = r[k + 1].to_digit(10).unwrap_or(0) as usize;
                        if let Some(Some((a, b))) = caps.get(g) {
                            out.extend(&t[*a..*b]);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, t: &str) -> bool {
        Regex::new(p, false).unwrap().is_match(t)
    }

    #[test]
    fn extended_syntax_matches_anywhere_in_the_text() {
        assert!(m("R[0-9]+", "R12"));
        assert!(m("^R1$", "R1") && !m("^R1$", "R12"));
        assert!(m("(SDA|SCL)_[0-9]", "I2C_SCL_1"));
        assert!(m("a{2,3}", "baab") && !m("a{3}", "baab"));
        assert!(m("[[:upper:]]{3}", "xxVCCx"));
        assert!(m("[^0-9]$", "R1x") && !m("[^0-9]$", "R12"));
        assert!(m("\\d+\\.\\d", "v3.3") && m("\\bGND\\b", "A GND B") && !m("\\bGND\\b", "AGNDB"));
        assert!(m("x{", "x{") && m("a.c", "abc") && !m("a.c", "ac"));
        assert!(Regex::new("(a", false).is_err() && Regex::new("*a", false).is_err() && Regex::new("[b-a]", false).is_err());
    }

    #[test]
    fn a_search_that_is_not_case_sensitive_folds_the_case() {
        assert!(Regex::new("vcc", true).unwrap().is_match("+VCC"));
        assert!(!Regex::new("vcc", false).unwrap().is_match("+VCC"));
        assert!(Regex::new("[a-c]x", true).unwrap().is_match("BX"));
    }

    #[test]
    fn replace_takes_every_match_and_the_sub_expressions() {
        let re = Regex::new("([A-Z]+)([0-9]+)", false).unwrap();
        assert_eq!(re.replace_all("R1 C22", "\\2\\1").as_deref(), Some("1R 22C"));
        assert_eq!(re.replace_all("R1", "<&>").as_deref(), Some("<R1>"));
        assert_eq!(re.replace_all("R1", "\\&").as_deref(), Some("&"));
        assert_eq!(re.replace_all("none", "x"), None);
        assert_eq!(Regex::new("x*", false).unwrap().replace_all("ab", "-").as_deref(), Some("-a-b-"));
        assert_eq!(Regex::new("^S", false).unwrap().replace_all("SS", "x").as_deref(), Some("xS"), "^ only at the start");
        // lazy and greedy
        assert_eq!(Regex::new("<.+>", false).unwrap().replace_all("<a><b>", "x").as_deref(), Some("x"));
        assert_eq!(Regex::new("<.+?>", false).unwrap().replace_all("<a><b>", "x").as_deref(), Some("xx"));
    }
}
