//! KiCad's text expressions, `@{..}`: what `EXPRESSION_EVALUATOR::Evaluate` makes of a text after its variables are expanded (`ResolveTextVars` runs it after every
//! pass, [`crate::text_vars::resolve_text_vars`]), for every shown text -- a table cell's on the board and on the sheet, a text's, a field's.
//!
//! Ported from (KiCad 8303b2ad):
//!   common/text_eval/text_eval_wrapper.cpp   `EXPRESSION_EVALUATOR::evaluateWithPartialErrorRecovery` (each `@{..}` of the text on its own, the braces matched; one that
//!                                            fails is left as written), `KIEVAL_TEXT_TOKENIZER` (numbers with an SI prefix or a unit, strings, identifiers, operators)
//!   common/text_eval/text_eval.lemon          the grammar: comparisons, then `+ -`, then `* / %`, then a sign, then `^` (right to left); a function call; a string
//!   common/text_eval/text_eval_parser.cpp     `EVAL_VISITOR` (`+` joins when either side is a string, `==` / `!=` compare two strings), `evaluateFunction`
//!   include/text_eval/text_eval_parser.h      `VALUE_UTILS::ToDouble`, `ToString` (a whole number without a decimal point), `ArithmeticOp`, `ToChar`
//!   include/text_eval/text_eval_units.h       the units a number may carry (`mm`, `cm`, `in`, `"`, `mil`, `thou`, `um`; `deg`, `°`, `ps`, `fs` and the
//!                                            `ps/..` ones unconverted), converted to millimetres, the evaluator's default units
//!
//! **Not ported** (an expression that uses one is left as written, as KiCad leaves one it cannot evaluate): the date and time functions (`today`, `now`,
//! `dateformat`, `datestring`, `weekdayname`, `timeformat`), `random`, the E-series functions (`enearest`, `eup`, `edown`), and the version-control ones
//! (`vcsidentifier` ...). A variable (`${..}`) inside an expression is the text's own variable, which `ExpandTextVars` has replaced before this runs.

/// A value of an expression (`calc_parser::Value`): a number or a string.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Num(f64),
    Str(String),
}

/// `VALUE_UTILS::ToString`: a number within 1e-10 of a whole one (below 1e15) without a decimal point, any other in its shortest form.
fn to_string(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Num(n) => {
            let rounded = n.round();
            if (n - rounded).abs() < 1e-10 && rounded.abs() < 1e15 {
                format!("{rounded:.0}")
            } else {
                format!("{n}")
            }
        }
    }
}

/// `VALUE_UTILS::ToDouble`: a number, or a string that is a number from end to end.
fn to_double(v: &Value) -> Result<f64, String> {
    match v {
        Value::Num(n) => Ok(*n),
        Value::Str(s) => s.parse::<f64>().ok().filter(|_| !s.is_empty() && !s.starts_with('+') && !s.contains(char::is_whitespace)).ok_or_else(|| format!("Cannot convert '{s}' to number")),
    }
}

/// `VALUE_UTILS::ToChar`: the first character of the value's text, a space for none.
fn to_char(v: &Value) -> char {
    to_string(v).chars().next().unwrap_or(' ')
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Op(&'static str),
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
        'k' | 'K' => 1e3,
        'M' => 1e6,
        'G' => 1e9,
        'T' => 1e12,
        'P' => 1e15,
        'E' => 1e18,
        _ => return None,
    })
}

/// `UnitRegistry::parseUnit` and `convertToEdaUnits( .., EDA_UNITS::MM )`: the factor a number with this unit is multiplied by, `None` for no unit.
fn unit_factor(unit: &str) -> Option<f64> {
    Some(match unit {
        "ps/mm" | "ps/cm" | "ps/in" | "deg" | "ps" | "fs" | "°" => 1.0,
        "thou" | "mil" => 25.4 / 1000.0,
        "mm" => 1.0,
        "cm" => 10.0,
        "in" | "\"" => 25.4,
        "um" => 1.0 / 1000.0,
        _ => return None,
    })
}

fn is_unit_char(c: char) -> bool {
    c.is_alphabetic() || c == '"' || c == '\'' || c == '°'
}

/// `KIEVAL_TEXT_TOKENIZER` in its expression context, over the inside of one `@{..}`.
fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        let next = c.get(i + 1).copied();
        let two = |a: char, b: char| ch == a && next == Some(b);
        if let Some(op) = [("<=", two('<', '=')), (">=", two('>', '=')), ("==", two('=', '=')), ("!=", two('!', '='))].iter().find(|(_, m)| *m).map(|(o, _)| *o) {
            out.push(Tok::Op(op));
            i += 2;
            continue;
        }
        if let Some(op) = ["(", ")", "+", "-", "*", "/", "%", "^", ",", "<", ">"].into_iter().find(|o| o.starts_with(ch)) {
            out.push(Tok::Op(op));
            i += 1;
            continue;
        }
        if ch == '"' || ch == '\'' {
            // `parse_string_literal`: the escapes `\n`, `\t`, `\r`, `\\`, the quote, `\xHH`; another `\` stays
            let quote = ch;
            let mut text = String::new();
            i += 1;
            let mut closed = false;
            while i < c.len() {
                let d = c[i];
                if d == quote {
                    closed = true;
                    i += 1;
                    break;
                }
                if d == '\\' && i + 1 < c.len() {
                    let e = c[i + 1];
                    i += 2;
                    match e {
                        'n' => text.push('\n'),
                        't' => text.push('\t'),
                        'r' => text.push('\r'),
                        '\\' => text.push('\\'),
                        '"' => text.push('"'),
                        '\'' => text.push('\''),
                        'x' => {
                            let hex: String = c[i..].iter().take(2).take_while(|h| h.is_ascii_hexdigit()).collect();
                            i += hex.len();
                            match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                                Some(v) if !hex.is_empty() => text.push(v),
                                _ if hex.is_empty() => text.push_str("\\x"),
                                _ => text.push('\u{FFFD}'),
                            }
                        }
                        other => {
                            text.push('\\');
                            text.push(other);
                        }
                    }
                    continue;
                }
                if d == '\n' {
                    return Err("Unterminated string literal".into());
                }
                text.push(d);
                i += 1;
            }
            if !closed {
                return Err("Missing closing quote in string literal".into());
            }
            out.push(Tok::Str(text));
            continue;
        }
        if ch.is_ascii_digit() || (ch == '.' && next.is_some_and(|n| n.is_ascii_digit())) {
            // `parse_number`: the digits, a decimal point or an SI prefix (unless a unit starts there), more digits, an exponent, then a unit
            let mut text = String::new();
            let mut multiplier = 1.0;
            while i < c.len() && c[i].is_ascii_digit() {
                text.push(c[i]);
                i += 1;
            }
            let unit_at = |from: usize| -> String { c[from..].iter().take_while(|u| is_unit_char(**u)).collect() };
            if i < c.len() {
                let d = c[i];
                if d == '.' {
                    text.push('.');
                    i += 1;
                } else if d.is_alphabetic() {
                    let unit = unit_at(i);
                    if unit_factor(&unit).is_none() {
                        if let Some(m) = si_multiplier(d) {
                            multiplier = m;
                            i += 1;
                        }
                    }
                } else if let Some(m) = si_multiplier(d) {
                    multiplier = m;
                    i += 1;
                }
            }
            while i < c.len() && c[i].is_ascii_digit() {
                text.push(c[i]);
                i += 1;
            }
            if i < c.len() && (c[i] == 'e' || c[i] == 'E') && c.get(i + 1).is_some_and(|n| *n == '+' || *n == '-' || n.is_ascii_digit()) {
                text.push(c[i]);
                i += 1;
                if c[i] == '+' || c[i] == '-' {
                    text.push(c[i]);
                    i += 1;
                }
                if !c.get(i).is_some_and(char::is_ascii_digit) {
                    return Err("Invalid scientific notation: missing exponent digits".into());
                }
                while i < c.len() && c[i].is_ascii_digit() {
                    text.push(c[i]);
                    i += 1;
                }
            }
            if i < c.len() && multiplier == 1.0 {
                let d = c[i];
                if d.is_alphabetic() {
                    if unit_factor(&unit_at(i)).is_none() {
                        if let Some(m) = si_multiplier(d) {
                            multiplier = m;
                            i += 1;
                        }
                    }
                } else if let Some(m) = si_multiplier(d) {
                    multiplier = m;
                    i += 1;
                }
            }
            let mut value = if text.is_empty() || text == "." { 0.0 } else { text.parse::<f64>().map_err(|_| format!("Invalid number format: {text}"))? * multiplier };
            // a unit, after any white space; none, and the white space is the next token's
            let mut j = i;
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            let unit = unit_at(j);
            if let Some(f) = unit_factor(&unit) {
                value *= f;
                i = j + unit.chars().count();
            }
            out.push(Tok::Num(value));
            continue;
        }
        if ch.is_alphabetic() || ch == '_' {
            let mut name = String::new();
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                name.push(c[i]);
                i += 1;
            }
            out.push(Tok::Ident(name));
            continue;
        }
        return Err(format!("Unexpected character '{ch}'"));
    }
    Ok(out)
}

/// The grammar of `text_eval.lemon` over the tokens of one expression.
struct Parser {
    toks: Vec<Tok>,
    at: usize,
}

impl Parser {
    fn peek_op(&self, op: &str) -> bool {
        matches!(self.toks.get(self.at), Some(Tok::Op(o)) if *o == op)
    }
    fn eat_op(&mut self, op: &str) -> bool {
        let yes = self.peek_op(op);
        if yes {
            self.at += 1;
        }
        yes
    }
    fn expression(&mut self) -> Result<Value, String> {
        let mut l = self.sum()?;
        loop {
            let Some(op) = ["<=", ">=", "==", "!=", "<", ">"].into_iter().find(|o| self.peek_op(o)) else { return Ok(l) };
            self.at += 1;
            let r = self.sum()?;
            // `==` and `!=` of two strings compare the strings; any other comparison is of numbers
            let b = match (op, &l, &r) {
                ("==", Value::Str(a), Value::Str(b)) => a == b,
                ("!=", Value::Str(a), Value::Str(b)) => a != b,
                _ => {
                    let (a, b) = (to_double(&l)?, to_double(&r)?);
                    match op {
                        "<=" => a <= b,
                        ">=" => a >= b,
                        "==" => a == b,
                        "!=" => a != b,
                        "<" => a < b,
                        _ => a > b,
                    }
                }
            };
            l = Value::Num(if b { 1.0 } else { 0.0 });
        }
    }
    fn sum(&mut self) -> Result<Value, String> {
        let mut l = self.term()?;
        loop {
            if self.eat_op("+") {
                let r = self.term()?;
                // "If either operand is a string, concatenate"
                l = if matches!(l, Value::Str(_)) || matches!(r, Value::Str(_)) { Value::Str(to_string(&l) + &to_string(&r)) } else { Value::Num(to_double(&l)? + to_double(&r)?) };
            } else if self.eat_op("-") {
                let r = self.term()?;
                l = Value::Num(to_double(&l)? - to_double(&r)?);
            } else {
                return Ok(l);
            }
        }
    }
    fn term(&mut self) -> Result<Value, String> {
        let mut l = self.unary()?;
        loop {
            if self.eat_op("*") {
                l = Value::Num(to_double(&l)? * to_double(&self.unary()?)?);
            } else if self.eat_op("/") {
                let r = to_double(&self.unary()?)?;
                if r == 0.0 {
                    return Err("Division by zero".into());
                }
                l = Value::Num(to_double(&l)? / r);
            } else if self.eat_op("%") {
                let r = to_double(&self.unary()?)?;
                if r == 0.0 {
                    return Err("Modulo by zero".into());
                }
                l = Value::Num(to_double(&l)? % r);
            } else {
                return Ok(l);
            }
        }
    }
    fn unary(&mut self) -> Result<Value, String> {
        if self.eat_op("-") {
            // `NODE::CreateBinOpRaw( NODE::CreateNumberRaw( 0.0 ), '-', R )`
            return Ok(Value::Num(0.0 - to_double(&self.unary()?)?));
        }
        if self.eat_op("+") {
            return self.unary();
        }
        self.power()
    }
    fn power(&mut self) -> Result<Value, String> {
        let base = self.atom()?;
        if self.eat_op("^") {
            let exp = self.unary()?;
            return Ok(Value::Num(to_double(&base)?.powf(to_double(&exp)?)));
        }
        Ok(base)
    }
    fn atom(&mut self) -> Result<Value, String> {
        match self.toks.get(self.at).cloned() {
            Some(Tok::Num(n)) => {
                self.at += 1;
                Ok(Value::Num(n))
            }
            Some(Tok::Str(s)) => {
                self.at += 1;
                Ok(Value::Str(s))
            }
            Some(Tok::Op("(")) => {
                self.at += 1;
                let v = self.expression()?;
                if self.eat_op(")") {
                    Ok(v)
                } else {
                    Err("syntax error".into())
                }
            }
            Some(Tok::Ident(name)) => {
                self.at += 1;
                // `function_call ::= IDENTIFIER LPAREN [arg_list] RPAREN`; a bare name is no factor of the grammar
                if !self.eat_op("(") {
                    return Err(format!("syntax error at '{name}'"));
                }
                let mut args = Vec::new();
                if !self.eat_op(")") {
                    loop {
                        args.push(self.expression()?);
                        if self.eat_op(",") {
                            continue;
                        }
                        if self.eat_op(")") {
                            break;
                        }
                        return Err("syntax error".into());
                    }
                }
                function(&name, &args)
            }
            _ => Err("syntax error".into()),
        }
    }
}

/// `wxString::BeforeFirst` and the rest: the text before or after the first or last `c` (the whole text, or nothing, when there is none, as wx gives them).
fn split_at(s: &str, c: char, last: bool, before: bool) -> String {
    let at = if last { s.rfind(c) } else { s.find(c) };
    match (at, before) {
        (Some(i), true) => s[..i].to_string(),
        (Some(i), false) => s[i + c.len_utf8()..].to_string(),
        // `BeforeFirst` / `BeforeLast` of a text without it: the whole text for BeforeFirst, nothing for BeforeLast; `AfterFirst` nothing, `AfterLast` the whole text
        (None, true) => if last { String::new() } else { s.to_string() },
        (None, false) => if last { s.to_string() } else { String::new() },
    }
}

/// `{:.{}f}`: `value` with `decimals` digits after the point (a negative count is none).
fn fixed(value: f64, decimals: f64) -> String {
    format!("{value:.*}", decimals.max(0.0) as usize)
}

/// `EVAL_VISITOR::evaluateFunction`, less what the module doc lists as not ported.
fn function(name: &str, args: &[Value]) -> Result<Value, String> {
    let argc = args.len();
    let num = |i: usize| to_double(&args[i]);
    let s = |i: usize| to_string(&args[i]);
    let decimals = || if argc > 1 { to_double(&args[1]).unwrap_or(2.0) } else { 2.0 };
    match (name, argc) {
        ("format" | "fixed", 1..) => return Ok(Value::Str(fixed(num(0)?, decimals()))),
        ("currency", 1..) => return Ok(Value::Str(format!("{}{:.2}", if argc > 1 { s(1) } else { "$".into() }, num(0)?))),
        ("upper", 1) => return Ok(Value::Str(s(0).to_ascii_uppercase())),
        ("lower", 1) => return Ok(Value::Str(s(0).to_ascii_lowercase())),
        ("concat", 2..) => return Ok(Value::Str(args.iter().map(to_string).collect())),
        ("beforefirst", 2) => return Ok(Value::Str(split_at(&s(0), to_char(&args[1]), false, true))),
        ("beforelast", 2) => return Ok(Value::Str(split_at(&s(0), to_char(&args[1]), true, true))),
        ("afterfirst", 2) => return Ok(Value::Str(split_at(&s(0), to_char(&args[1]), false, false))),
        ("afterlast", 2) => return Ok(Value::Str(split_at(&s(0), to_char(&args[1]), true, false))),
        ("replace", 3) => {
            let (text, search) = (s(0), s(1));
            return Ok(Value::Str(if search.is_empty() { text } else { text.replace(&search, &s(2)) }));
        }
        // "Convert only the condition to a number"
        ("if", 3) => return Ok(if num(0)? != 0.0 { args[1].clone() } else { args[2].clone() }),
        _ => {}
    }
    let n: Vec<f64> = args.iter().map(to_double).collect::<Result<_, _>>()?;
    Ok(Value::Num(match (name, argc) {
        ("abs", 1) => n[0].abs(),
        ("sum", 1..) => n.iter().sum(),
        ("round", 1..) => {
            let m = 10f64.powi(if argc > 1 { n[1] as i32 } else { 0 });
            (n[0] * m).round() / m
        }
        ("sqrt", 1) if n[0] < 0.0 => return Err("Square root of negative number".into()),
        ("sqrt", 1) => n[0].sqrt(),
        ("pow", 2) => n[0].powf(n[1]),
        ("floor", 1) => n[0].floor(),
        ("ceil", 1) => n[0].ceil(),
        ("min", 1..) => n.iter().copied().fold(f64::INFINITY, f64::min),
        ("max", 1..) => n.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        ("avg", 1..) => n.iter().sum::<f64>() / argc as f64,
        ("shunt", 2) => {
            let sum = n[0] + n[1];
            if sum > 0.0 {
                n[0] * n[1] / sum
            } else {
                0.0
            }
        }
        ("db", 1) if n[0] <= 0.0 => return Err("db() argument must be positive".into()),
        ("db", 1) => 10.0 * n[0].log10(),
        ("dbv", 1) if n[0] <= 0.0 => return Err("dbv() argument must be positive".into()),
        ("dbv", 1) => 20.0 * n[0].log10(),
        ("fromdb", 1) => 10f64.powf(n[0] / 10.0),
        ("fromdbv", 1) => 10f64.powf(n[0] / 20.0),
        _ => return Err(format!("Unknown function: {name} with {argc} arguments")),
    }))
}

/// One expression (the inside of an `@{..}`) to its text, or the reason it cannot be read.
pub fn evaluate_expression(expr: &str) -> Result<String, String> {
    let mut p = Parser { toks: tokenize(expr)?, at: 0 };
    let v = p.expression()?;
    if p.at != p.toks.len() {
        return Err("syntax error".into());
    }
    if let Value::Num(n) = v {
        if !n.is_finite() {
            return Err("Number out of range".into());
        }
    }
    Ok(to_string(&v))
}

/// `EXPRESSION_EVALUATOR::Evaluate`: every `@{..}` of `text` (its braces matched, inner ones included) replaced by its value, an expression that cannot be
/// evaluated left as written (`evaluateWithPartialErrorRecovery`), an `@{` with no closing brace left too.
pub fn evaluate(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("@{") {
        out.push_str(&rest[..at]);
        let body = &rest[at + 2..];
        let mut depth = 1;
        let mut end = None;
        for (i, ch) in body.char_indices() {
            match ch {
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
            // a malformed one is skipped: the text after its `@{` is looked through again
            out.push_str("@{");
            rest = body;
            continue;
        };
        match evaluate_expression(&body[..end]) {
            Ok(v) => out.push_str(&v),
            Err(_) => out.push_str(&rest[at..at + 2 + end + 1]),
        }
        rest = &body[end + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_comes_out_as_kicad_writes_a_number() {
        assert_eq!(evaluate("@{1+2*3}"), "7");
        assert_eq!(evaluate("@{(1+2)*3} and @{7/2}"), "9 and 3.5");
        assert_eq!(evaluate("@{2^3^2}"), "512", "power is right to left");
        assert_eq!(evaluate("@{-2^2}"), "-4", "a sign binds below the power");
        assert_eq!(evaluate("@{10 % 4} @{3 > 2} @{1 == 2}"), "2 1 0");
        assert_eq!(evaluate("@{0.1+0.2}"), "0.30000000000000004");
        assert_eq!(evaluate("@{1/0}"), "@{1/0}", "an error leaves the expression as written");
        assert_eq!(evaluate("@{x} and @{1+1}"), "@{x} and 2", "a bare name is no expression");
    }

    #[test]
    fn strings_join_compare_and_go_through_the_string_functions() {
        assert_eq!(evaluate(r#"@{"R" + 5}"#), "R5");
        assert_eq!(evaluate(r#"@{'a' == 'a'} @{"a" != "b"}"#), "1 1");
        assert_eq!(evaluate(r#"@{upper("abc")} @{lower('ABC')} @{concat("a", 1, "b")}"#), "ABC abc a1b");
        assert_eq!(evaluate(r#"@{beforefirst("a.b.c", ".")} @{afterlast("a.b.c", ".")} @{replace("a-b", "-", "+")}"#), "a c a+b");
        assert_eq!(evaluate(r#"@{if(2 > 1, "yes", "no")} @{if(0, 1, 2)}"#), "yes 2");
        assert_eq!(evaluate(r#"@{"3" * 2}"#), "6", "a string that is a number is one for arithmetic");
        assert_eq!(evaluate(r#"@{"x" * 2}"#), r#"@{"x" * 2}"#);
    }

    #[test]
    fn the_math_and_number_functions() {
        assert_eq!(evaluate("@{round(3.14159, 2)} @{round(2.5)} @{floor(2.7)} @{ceil(2.1)} @{abs(-3)}"), "3.14 3 2 3 3");
        assert_eq!(evaluate("@{format(2)} @{fixed(3.14159, 3)} @{currency(5)} @{currency(5, \"EUR \")}"), "2.00 3.142 $5.00 EUR 5.00");
        assert_eq!(evaluate("@{sum(1, 2, 3)} @{avg(1, 2, 3)} @{min(4, 2, 8)} @{max(4, 2, 8)} @{sqrt(16)} @{pow(2, 10)}"), "6 2 2 8 4 1024");
        assert_eq!(evaluate("@{shunt(100, 100)} @{db(100)} @{dbv(10)} @{fromdb(20)}"), "50 20 20 100");
        assert_eq!(evaluate("@{today()}"), "@{today()}", "the date functions are not ported");
    }

    #[test]
    fn a_number_may_carry_an_si_prefix_or_a_unit() {
        assert_eq!(evaluate("@{10k} @{100n * 1G} @{1in} @{2 mm} @{100mil}"), "10000 100 25.4 2 2.54");
        // `4k7`: the digits after a prefix join the ones before it (`parse_number`), as KiCad reads it
        assert_eq!(evaluate("@{4k7}"), "47000");
        assert_eq!(evaluate("@{1e3 + 1}"), "1001");
    }

    #[test]
    fn nested_braces_and_unclosed_ones() {
        assert_eq!(evaluate("a @{ if(1, \"{x}\", 2) } b"), "a {x} b");
        assert_eq!(evaluate("@{1+"), "@{1+");
        assert_eq!(evaluate("no expression"), "no expression");
    }
}
