//! KiCad's expression language, its front half: the lexer and the grammar of `libeval_compiler` (`common/libeval_compiler/libeval_compiler.cpp`
//! `COMPILER::lexDefault`, `grammar.lemon` at KiCad 8303b2ad). It turns the text of a `.kicad_dru` condition, or of a Find by Properties query, into a
//! tree; what the tree means -- the items `A` and `B`, their properties, `A.hasNetclass('x')` -- is [`crate::pcbexpr`]'s.
//!
//! Ported as it is, quirks included (the studio's `kicad-port/libeval.ts` is the same port and is held to the same cases):
//!
//! * only the space is white space: a tab or a new line is "Unrecognized character";
//! * a unit (`mil` `mm` `in` `deg` `fs` `ps`) is looked for before an identifier, at every token start, and it is case sensitive;
//! * a `,` is a decimal separator inside a number (`1,5` is 1.5) and the separator of the arguments of a call between two;
//! * `||` binds tighter than `&&` (lemon gives a later `%left` line the higher precedence), so `a && b || c` is `a && (b || c)`;
//! * the comparisons and `.` do not chain (`%nonassoc`: `a < b < c` is a syntax error);
//! * an unterminated string is the rest of the text;
//! * an unrecognized character is reported and the parser is then given the end of the text, which may be a syntax error of its own.

/// `TOKEN_TYPE_T` of the tree: what a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Mul,
    Div,
    Add,
    Sub,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Equal,
    NotEqual,
    And,
    Or,
    Not,
    Number,
    Identifier,
    Str,
    StructRef,
    FuncCall,
    Unit,
    ArgList,
    Null,
}

/// `LIBEVAL::TREE_NODE`.
#[derive(Debug, Clone)]
pub struct Node {
    pub op: Op,
    /// `value.str`: a number's digits, an identifier's name, a string's text.
    pub text: Option<String>,
    /// A unit node's index in the resolver's list of units.
    pub idx: usize,
    pub leaf: [Option<Box<Node>>; 2],
    /// `srcPos`: `COMPILER::m_sourcePos` when the node was made (when the parser reduced it, with the next token already read).
    pub src_pos: usize,
}

impl Node {
    fn new(op: Op, text: Option<String>, idx: usize, src_pos: usize) -> Node {
        Node { op, text, idx, leaf: [None, None], src_pos }
    }
}

/// `UNIT_RESOLVER`: the units a number may carry, in the order they are looked for.
#[derive(Debug, Clone, Copy)]
pub struct UnitResolver {
    pub names: &'static [&'static str],
    /// `GetSupportedUnitsMessage`, said of a number that has none; empty when units are not asked for.
    pub message: &'static str,
    /// `Convert( text, unitId )`: the number a count of that unit is, in internal units.
    pub convert: fn(&str, usize) -> f64,
}

/// `PCBEXPR_UNIT_RESOLVER`'s units.
pub const PCB_UNIT_NAMES: [&str; 6] = ["mil", "mm", "in", "deg", "fs", "ps"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Value,
    Unit,
    Str,
    Ident,
    Comma,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    Not,
    Plus,
    Minus,
    Mult,
    Divide,
    Dot,
    LParen,
    RParen,
    Semicolon,
    Ends,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    text: String,
    idx: usize,
    /// Where the lexer stood before it skipped the blanks.
    before: usize,
}

fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

fn is_alnum(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

struct Lexer<'a> {
    text: Vec<char>,
    pos: usize,
    in_string: bool,
    units: &'a UnitResolver,
    /// The errors told so far: `(message, position)`.
    errors: Vec<(String, usize)>,
}

impl<'a> Lexer<'a> {
    fn new(text: &str, units: &'a UnitResolver) -> Self {
        Lexer { text: text.chars().collect(), pos: 0, in_string: false, units, errors: Vec::new() }
    }

    fn at(&self, i: usize) -> char {
        self.text.get(i).copied().unwrap_or('\0')
    }

    /// `TOKENIZER::MatchAhead`: the text has `m` here, followed by the end or by a character `stop` accepts.
    fn match_ahead(&self, m: &str, stop: impl Fn(char) -> bool) -> bool {
        let m: Vec<char> = m.chars().collect();
        let remaining = self.text.len().saturating_sub(self.pos);
        if remaining < m.len() {
            return false;
        }
        if self.text[self.pos..self.pos + m.len()] == m[..] {
            return remaining == m.len() || stop(self.text[self.pos + m.len()]);
        }
        false
    }

    /// `TOKENIZER::GetString`: up to the next quote that is not escaped, the closing quote skipped.
    fn get_string(&mut self) -> String {
        let mut out = String::new();
        while self.pos < self.text.len() && self.text[self.pos] != '\'' {
            if self.text[self.pos] == '\\' && self.pos + 1 < self.text.len() && self.text[self.pos + 1] == '\'' {
                self.pos += 1;
            }
            out.push(self.text[self.pos]);
            self.pos += 1;
        }
        self.pos += 1;
        out
    }

    fn resolve_units(&mut self) -> Option<usize> {
        for (id, name) in self.units.names.iter().enumerate() {
            if self.match_ahead(name, |c| !is_alnum(c)) {
                self.pos += name.chars().count();
                return Some(id);
            }
        }
        None
    }

    fn next(&mut self) -> Token {
        let before = self.pos;
        loop {
            if self.in_string {
                self.in_string = false;
                let text = self.get_string();
                return Token { kind: Kind::Str, text, idx: 0, before };
            }
            if let Some(t) = self.lex_default(before) {
                return t;
            }
        }
    }

    fn lex_default(&mut self, before: usize) -> Option<Token> {
        let tok = |kind: Kind, text: String, idx: usize| Token { kind, text, idx, before };
        if self.pos >= self.text.len() {
            return Some(tok(Kind::Ends, String::new(), 0));
        }
        let mut ch = self.at(self.pos);
        while ch == ' ' {
            self.pos += 1;
            ch = self.at(self.pos);
        }
        if ch == '\0' {
            return Some(tok(Kind::Ends, String::new(), 0));
        }
        let is_decimal = |c: char| c == '.' || c == ',';
        if is_digit(ch) {
            let mut current = String::new();
            let mut have_separator = false;
            loop {
                if is_decimal(ch) && have_separator {
                    break;
                }
                current.push(ch);
                if is_decimal(ch) {
                    have_separator = true;
                }
                self.pos += 1;
                ch = self.at(self.pos);
                if !(is_digit(ch) || is_decimal(ch)) {
                    break;
                }
            }
            return Some(tok(Kind::Value, current.replace(',', "."), 0));
        }
        if let Some(unit) = self.resolve_units() {
            return Some(tok(Kind::Unit, String::new(), unit));
        }
        if ch == '\'' {
            self.in_string = true;
            self.pos += 1;
            return None;
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            let mut end = self.pos;
            while end < self.text.len() && (is_alnum(self.text[end]) || self.text[end] == '_') {
                end += 1;
            }
            let name: String = self.text[self.pos..end].iter().collect();
            self.pos = end;
            return Some(tok(Kind::Ident, name, 0));
        }
        let two: [(&str, Kind, char); 6] = [("==", Kind::Eq, '='), ("!=", Kind::Ne, '='), ("<=", Kind::Le, '='), (">=", Kind::Ge, '='), ("&&", Kind::And, '&'), ("||", Kind::Or, '|')];
        for (text, kind, not_followed_by) in two {
            if self.match_ahead(text, |c| c != not_followed_by) {
                self.pos += 2;
                return Some(tok(kind, String::new(), 0));
            }
        }
        let single = match ch {
            '+' => Some(Kind::Plus),
            '!' => Some(Kind::Not),
            '-' => Some(Kind::Minus),
            '*' => Some(Kind::Mult),
            '/' => Some(Kind::Divide),
            '<' => Some(Kind::Lt),
            '>' => Some(Kind::Gt),
            '(' => Some(Kind::LParen),
            ')' => Some(Kind::RParen),
            ';' => Some(Kind::Semicolon),
            '.' => Some(Kind::Dot),
            ',' => Some(Kind::Comma),
            _ => None,
        };
        if let Some(kind) = single {
            self.pos += 1;
            return Some(tok(kind, String::new(), 0));
        }
        // Anything else: the error, then the token that ends the text (`retval.token` stays `G_ENDS`).
        if self.match_ahead("${", |c| c != '{') {
            self.errors.push(("Unresolved text variable reference".to_string(), before + 2));
        } else {
            self.errors.push((format!("Unrecognized character '{ch}'"), before));
        }
        self.pos += 1;
        Some(tok(Kind::Ends, String::new(), 0))
    }
}

/// The precedence of a binary operator, its node and whether it chains: the order of the `%left` / `%nonassoc` lines of `grammar.lemon`, later is tighter.
fn binary(kind: Kind) -> Option<(u8, Op, bool)> {
    Some(match kind {
        Kind::Comma => (2, Op::ArgList, false),
        Kind::And => (3, Op::And, false),
        Kind::Or => (4, Op::Or, false),
        Kind::Lt => (6, Op::Less, true),
        Kind::Gt => (6, Op::Greater, true),
        Kind::Le => (6, Op::LessEqual, true),
        Kind::Ge => (6, Op::GreaterEqual, true),
        Kind::Eq => (6, Op::Equal, true),
        Kind::Ne => (6, Op::NotEqual, true),
        Kind::Plus => (8, Op::Add, false),
        Kind::Minus => (8, Op::Sub, false),
        Kind::Divide => (9, Op::Div, false),
        Kind::Mult => (9, Op::Mul, false),
        Kind::Dot => (10, Op::StructRef, true),
        _ => return None,
    })
}

struct Syntax;

struct Parser<'a> {
    lexer: Lexer<'a>,
    look: Token,
}

impl<'a> Parser<'a> {
    fn node(&self, op: Op, text: Option<String>, idx: usize) -> Node {
        // `newNode` is called when the parser reduces, with the next token already read.
        Node::new(op, text, idx, self.look.before)
    }

    fn advance(&mut self) -> Token {
        let next = self.lexer.next();
        std::mem::replace(&mut self.look, next)
    }

    fn statement(&mut self) -> Result<Option<Node>, Syntax> {
        if self.look.kind == Kind::Ends {
            return Ok(None);
        }
        let root = self.expr(0)?;
        if self.look.kind != Kind::Ends {
            return Err(Syntax);
        }
        Ok(Some(root))
    }

    fn expr(&mut self, min_prec: u8) -> Result<Node, Syntax> {
        let mut left = self.unary()?;
        loop {
            let Some((prec, op, nonassoc)) = binary(self.look.kind) else { break };
            if prec < min_prec {
                break;
            }
            self.advance();
            // The right operand of `.` is a primary (nothing binds tighter); of the others whatever binds tighter than the operator.
            let right = self.expr(prec + 1)?;
            let mut n = self.node(op, None, 0);
            n.leaf = [Some(Box::new(left)), Some(Box::new(right))];
            left = n;
            if nonassoc {
                if let Some((again, _, _)) = binary(self.look.kind) {
                    if again == prec {
                        return Err(Syntax);
                    }
                }
            }
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Node, Syntax> {
        match self.look.kind {
            Kind::Not => {
                self.advance();
                // `G_BOOL_NOT nt` is %right at the precedence of NOT (7): what binds tighter belongs to its operand.
                let operand = self.expr(8)?;
                let mut n = self.node(Op::Not, None, 0);
                n.leaf[0] = Some(Box::new(operand));
                Ok(n)
            }
            Kind::Plus => {
                self.advance();
                self.expr(9)
            }
            Kind::Minus => {
                self.advance();
                // `G_MINUS nt` has the precedence of MINUS (8, left): only * and / and . go into its operand. It is 0 - operand.
                let operand = self.expr(9)?;
                let zero = self.node(Op::Number, None, 0);
                let mut n = self.node(Op::Sub, None, 0);
                n.leaf = [Some(Box::new(zero)), Some(Box::new(operand))];
                Ok(n)
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Node, Syntax> {
        let t = self.look.clone();
        match t.kind {
            Kind::Value => {
                self.advance();
                let mut n = self.node(Op::Number, Some(t.text), 0);
                if self.look.kind == Kind::Unit {
                    let u = self.advance();
                    n.leaf[0] = Some(Box::new(self.node(Op::Unit, None, u.idx)));
                }
                Ok(n)
            }
            Kind::Str => {
                self.advance();
                Ok(self.node(Op::Str, Some(t.text), 0))
            }
            Kind::Ident => {
                self.advance();
                if self.look.kind == Kind::LParen {
                    self.advance();
                    let mut call = self.node(Op::FuncCall, None, 0);
                    let name = self.node(Op::Identifier, Some(t.text), 0);
                    let args = if self.look.kind == Kind::RParen {
                        self.node(Op::Null, None, 0)
                    } else {
                        let a = self.expr(0)?;
                        if self.look.kind != Kind::RParen {
                            return Err(Syntax);
                        }
                        a
                    };
                    self.advance();
                    call.leaf = [Some(Box::new(name)), Some(Box::new(args))];
                    return Ok(call);
                }
                Ok(self.node(Op::Identifier, Some(t.text), 0))
            }
            Kind::LParen => {
                self.advance();
                let inner = self.expr(0)?;
                if self.look.kind != Kind::RParen {
                    return Err(Syntax);
                }
                self.advance();
                Ok(inner)
            }
            _ => Err(Syntax),
        }
    }
}

/// What `COMPILER::Compile` makes of a text before it generates code.
#[derive(Debug, Clone)]
pub struct Parsed {
    /// The tree; `None` for a text that is empty or only blanks, and for one that did not parse.
    pub root: Option<Node>,
    /// The errors told, in order: `(message, position)`. Any error makes `COMPILER::Compile` answer false.
    pub errors: Vec<(String, usize)>,
}

/// Lex and parse `text`. A text that is empty (or only blanks) has no tree and no error: it is true.
pub fn parse(text: &str, units: &UnitResolver) -> Parsed {
    if text.is_empty() {
        return Parsed { root: None, errors: Vec::new() };
    }
    let mut lexer = Lexer::new(text, units);
    let first = lexer.next();
    let mut parser = Parser { lexer, look: first };
    let root = match parser.statement() {
        Ok(root) => root,
        Err(Syntax) => {
            let pos = parser.look.before;
            parser.lexer.errors.push(("Syntax error".to_string(), pos));
            None
        }
    };
    let errors = std::mem::take(&mut parser.lexer.errors);
    Parsed { root: if errors.is_empty() { root } else { None }, errors }
}

/// `squashParamList`: the arguments of a call in the order the text has them.
pub fn squash_params(root: Option<&Node>) -> Vec<&Node> {
    let mut args: Vec<&Node> = Vec::new();
    let Some(root) = root else { return args };
    if root.op != Op::ArgList && root.op != Op::Null {
        args.push(root);
    } else {
        let mut n: Option<&Node> = Some(root);
        while let Some(node) = n {
            if let Some(l1) = node.leaf[1].as_deref() {
                args.push(l1);
            }
            n = node.leaf[0].as_deref();
            if !matches!(n, Some(m) if m.op == Op::ArgList) {
                break;
            }
        }
        if let Some(rest) = n {
            args.push(rest);
        }
    }
    args.reverse();
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(_: &str, _: usize) -> f64 {
        0.0
    }
    const UNITS: UnitResolver = UnitResolver { names: &PCB_UNIT_NAMES, message: "must be mm, in, mil, deg, fs, or ps", convert: conv };

    fn errs(text: &str) -> Vec<String> {
        parse(text, &UNITS).errors.into_iter().map(|e| e.0).collect()
    }

    /// The shape of the tree as a string, for the precedence tests.
    fn shape(n: &Node) -> String {
        let l = |i: usize| n.leaf[i].as_deref().map(shape).unwrap_or_default();
        match n.op {
            Op::Number => n.text.clone().unwrap_or_else(|| "0".into()),
            Op::Identifier | Op::Str => n.text.clone().unwrap_or_default(),
            Op::Not => format!("!({})", l(0)),
            Op::StructRef => format!("{}.{}", l(0), l(1)),
            Op::FuncCall => format!("{}({})", l(0), l(1)),
            Op::Unit => "unit".into(),
            Op::Null => String::new(),
            op => format!("({op:?} {} {})", l(0), l(1)),
        }
    }

    fn tree(text: &str) -> String {
        shape(parse(text, &UNITS).root.as_ref().expect("a tree"))
    }

    #[test]
    fn or_binds_tighter_than_and() {
        assert_eq!(tree("a && b || c"), "(And a (Or b c))");
        assert_eq!(tree("a || b && c"), "(And (Or a b) c)");
    }

    #[test]
    fn not_binds_looser_than_plus_and_tighter_than_comparisons() {
        assert_eq!(tree("!a == b"), "(Equal !(a) b)");
        assert_eq!(tree("!a + b"), "!((Add a b))");
        assert_eq!(tree("-a * b"), "(Sub 0 (Mul a b))");
        assert_eq!(tree("-a + b"), "(Add (Sub 0 a) b)");
    }

    #[test]
    fn comparisons_and_the_dot_do_not_chain() {
        assert_eq!(errs("1 < 2 < 3"), ["Syntax error"]);
        assert_eq!(errs("1 == 1 != 0"), ["Syntax error"]);
        assert_eq!(errs("A.x.y"), ["Syntax error"]);
        assert!(errs("1 < 2 && 2 < 3").is_empty());
        assert!(errs("(1 < 2) < 3").is_empty());
    }

    #[test]
    fn a_struct_ref_and_a_call() {
        assert_eq!(tree("A.hasNetclass('x') && A.Type == 'Pad'"), "(And A.hasNetclass(x) (Equal A.Type Pad))");
        assert_eq!(tree("A.f('a', 'b')"), "A.f((ArgList a b))");
        // `1, 2` is one number with a decimal separator and then another number: KiCad's lexer reads the comma into the first.
        assert_eq!(errs("A.f(1, 2)"), ["Syntax error"]);
        assert_eq!(tree("A.f()"), "A.f()");
    }

    #[test]
    fn numbers_and_units() {
        let p = parse("1,5 mm", &UNITS);
        let root = p.root.expect("a tree");
        assert_eq!(root.text.as_deref(), Some("1.5"));
        assert_eq!(root.leaf[0].as_ref().map(|u| u.idx), Some(1));
        // a unit is case sensitive and is read before an identifier
        assert_eq!(errs("1MM"), ["Syntax error"]);
        assert_eq!(tree("A.x == 90deg"), "(Equal A.x 90)");
    }

    #[test]
    fn errors_and_where_the_parser_goes_on() {
        assert_eq!(errs("1\t== 1"), ["Unrecognized character '\t'"]);
        assert_eq!(errs("A.x == %"), ["Unrecognized character '%'", "Syntax error"]);
        assert_eq!(errs("A.x == 1 %"), ["Unrecognized character '%'"]);
        assert_eq!(errs("1 == 1 ${X}"), ["Unresolved text variable reference"]);
        assert_eq!(errs("1 == ${X}"), ["Unresolved text variable reference", "Syntax error"], "the parser is then given the end of an unfinished text");
        for text in ["(", ")", "1 +", "== 1", "1 1", "f(", "A.x ==", "()"] {
            assert_eq!(errs(text), ["Syntax error"], "{text}");
        }
        assert!(parse("", &UNITS).root.is_none() && parse("   ", &UNITS).errors.is_empty());
    }

    #[test]
    fn strings() {
        assert_eq!(tree("'it\\'s'"), "it's");
        assert_eq!(tree("'abc"), "abc");
    }

    #[test]
    fn squashed_params_run_in_the_order_of_the_text() {
        let p = parse("f('x', 'y', 'z')", &UNITS).root.unwrap();
        let args = squash_params(p.leaf[1].as_deref());
        let texts: Vec<_> = args.iter().map(|n| n.text.clone().unwrap()).collect();
        assert_eq!(texts, ["x", "y", "z"]);
    }
}
