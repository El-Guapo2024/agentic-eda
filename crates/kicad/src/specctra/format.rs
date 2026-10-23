//! The text side of the Specctra writer: KiCad's `OUTPUTFORMATTER` as the DSN code uses it (`common/richio.cpp`), and the number formats the
//! DSN code prints with (`%.6g`, `FormatDouble2Str`'s `{:.10g}`).
//!
//! `Print( nestLevel, fmt, .. )` writes two spaces per nest level and then the text, and answers how many bytes it wrote; the wrapping code in
//! `PATH::Format`, `VIA::Format`, `NET::Format` and the others counts the line by those answers, so they are kept here too.

/// `OUTPUTFORMATTER`: the file text being built and the character `GetQuoteChar` wraps a name in.
#[derive(Debug, Clone)]
pub struct Out {
    buf: String,
    quote: char,
}

impl Default for Out {
    fn default() -> Self {
        Out::new('"')
    }
}

impl Out {
    /// A formatter writing to a string; `quote` is `SPECCTRA_DB::m_quote_char[0]` (`"` unless a file said otherwise with `string_quote`).
    pub fn new(quote: char) -> Self {
        Out { buf: String::new(), quote }
    }

    /// `OUTPUTFORMATTER::Print( nestLevel, "%s", text )`: `nest` levels of two spaces, then `text`. Returns the number of bytes written.
    pub fn print(&mut self, nest: i32, text: &str) -> usize {
        let indent = 2 * nest.max(0) as usize;
        for _ in 0..indent {
            self.buf.push(' ');
        }
        self.buf.push_str(text);
        indent + text.len()
    }

    /// `OUTPUTFORMATTER::GetQuoteChar( wrapee )` with the wrapee put between the quotes it asks for: `s` itself when it needs none.
    pub fn q(&self, wrapee: &str) -> String {
        if needs_quote(wrapee) {
            let mut s = String::with_capacity(wrapee.len() + 2);
            s.push(self.quote);
            s.push_str(wrapee);
            s.push(self.quote);
            s
        } else {
            wrapee.to_string()
        }
    }

    pub fn as_str(&self) -> &str {
        &self.buf
    }

    pub fn into_string(self) -> String {
        self.buf
    }

    /// `STRING_FORMATTER::StripUseless`: the text with its white space, parentheses and double quotes removed (what `makeHash` compares).
    pub fn stripped(&self) -> String {
        self.buf.chars().filter(|c| !c.is_whitespace() && *c != '(' && *c != ')' && *c != '"').collect()
    }
}

/// `OUTPUTFORMATTER::GetQuoteChar( wrapee, quote_char )`: whether a name has to be wrapped in quotes. A name that starts with `#` (a comment to the lexer), an empty one, one with
/// a tab, a space, a parenthesis, a `%`, a brace, or a `-` anywhere but first.
pub fn needs_quote(wrapee: &str) -> bool {
    if wrapee.starts_with('#') || wrapee.is_empty() {
        return true;
    }
    for (i, c) in wrapee.chars().enumerate() {
        if matches!(c, '\t' | ' ' | '(' | ')' | '%' | '{' | '}') {
            return true;
        }
        if i != 0 && c == '-' {
            return true;
        }
    }
    false
}

/// C's `printf( "%.{prec}g", x )`.
pub fn fmt_g(x: f64, prec: usize) -> String {
    let p = prec.max(1);
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    // The exponent the value has once rounded to `p` significant digits decides between the two notations.
    let e_form = format!("{:.*e}", p - 1, x);
    let (mantissa, exp) = e_form.split_once('e').expect("exponent form");
    let exp: i32 = exp.parse().expect("exponent");
    if exp < -4 || exp >= p as i32 {
        let mantissa = strip_zeros(mantissa);
        format!("{}e{}{:02}", mantissa, if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        strip_zeros(&format!("{:.*}", decimals, x))
    }
}

/// `%.6g`, the format of every coordinate, width and diameter of a DSN file.
pub fn g6(x: f64) -> String {
    fmt_g(x, 6)
}

fn strip_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let t = s.trim_end_matches('0');
    t.strip_suffix('.').unwrap_or(t).to_string()
}

/// `FormatDouble2Str` (`common/string_utils.cpp`): ten significant digits, and a value of 0.0001 or less in fixed notation with its trailing zeros dropped.
pub fn format_double2str(v: f64) -> String {
    if v != 0.0 && v.abs() <= 0.0001 {
        let mut buf = format!("{:.16}", v);
        while buf.ends_with('0') {
            buf.pop();
        }
        if buf.ends_with('.') {
            buf.pop();
        }
        buf
    } else {
        fmt_g(v, 10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g_formats_like_printf() {
        assert_eq!(g6(0.0), "0");
        assert_eq!(g6(1375.0), "1375");
        assert_eq!(g6(1374.5), "1374.5");
        assert_eq!(g6(250.951), "250.951");
        assert_eq!(g6(152.4), "152.4");
        assert_eq!(g6(-0.5), "-0.5");
        assert_eq!(g6(0.000123), "0.000123");
        assert_eq!(g6(0.00001), "1e-05");
        assert_eq!(g6(100000.0), "100000");
        assert_eq!(g6(1234567.0), "1.23457e+06");
        assert_eq!(g6(1e6), "1e+06");
        assert_eq!(g6(123456.7), "123457");
        assert_eq!(g6(-2800.95), "-2800.95");
        assert_eq!(g6(2778.0800000001), "2778.08");
        assert_eq!(g6(-0.0), "-0");
        assert_eq!(fmt_g(105605.887, 10), "105605.887");
        assert_eq!(fmt_g(0.1 + 0.2, 10), "0.3");
    }

    #[test]
    fn double2str_is_the_ten_digit_form() {
        assert_eq!(format_double2str(135.0), "135");
        assert_eq!(format_double2str(-39294.365), "-39294.365");
        assert_eq!(format_double2str(0.0), "0");
        assert_eq!(format_double2str(0.00005), "0.00005");
        assert_eq!(format_double2str(180.0000000001), "180");
    }

    #[test]
    fn names_are_quoted_when_the_lexer_could_not_read_them_bare() {
        let o = Out::default();
        assert_eq!(o.q("GND"), "GND");
        assert_eq!(o.q("+5V"), "+5V");
        assert_eq!(o.q("Net-(D7-Pad1)"), "\"Net-(D7-Pad1)\"");
        assert_eq!(o.q("-5V"), "-5V");
        assert_eq!(o.q("Via[0-1]_685.8:330.2_um"), "\"Via[0-1]_685.8:330.2_um\"");
        assert_eq!(o.q(""), "\"\"");
        assert_eq!(o.q("#1"), "\"#1\"");
        assert_eq!(o.q("a b"), "\"a b\"");
        assert_eq!(o.q("U1"), "U1");
        assert_eq!(o.q("100%"), "\"100%\"");
        assert_eq!(o.q("/CRESET"), "/CRESET");
        assert_eq!(o.q("tomu-fpga:USB-B"), "\"tomu-fpga:USB-B\"");
    }

    #[test]
    fn print_indents_two_spaces_a_level_and_counts_bytes() {
        let mut o = Out::default();
        assert_eq!(o.print(2, "(a"), 6);
        assert_eq!(o.print(0, ")\n"), 2);
        assert_eq!(o.as_str(), "    (a)\n");
        assert_eq!(Out::default().stripped(), "");
        let mut s = Out::default();
        s.print(1, "(shape (path F.Cu 1700\n  0 0))\n");
        assert_eq!(s.stripped(), "shapepathF.Cu170000");
    }
}
