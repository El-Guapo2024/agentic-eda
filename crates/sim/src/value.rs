//! `SPICE_VALUE` and `SPICE_VALUE_FORMAT` (`eeschema/sim/spice_value.cpp`): a number with a SPICE unit prefix ("4k7" is not one, "4.7k"
//! is), parsed the way `sscanf( "%lf%7s" )` does and printed the way the simulator's dialogs, cursors and measurements print it.
//!
//! The studio's simulator panel has its own TypeScript port of the same functions (`web/studio/src/kicad-port/spiceValue.ts`); this one is
//! the server's, for the analysis commands it reads (`command.rs`) and the tests that hold both to KiCad's output.

/// `SPICE_VALUE::UNIT_PREFIX`: the power of ten of the prefix.
pub const PFX_FEMTO: i32 = -15;
pub const PFX_PICO: i32 = -12;
pub const PFX_NANO: i32 = -9;
pub const PFX_MICRO: i32 = -6;
pub const PFX_MILI: i32 = -3;
pub const PFX_NONE: i32 = 0;
pub const PFX_KILO: i32 = 3;
pub const PFX_MEGA: i32 = 6;
pub const PFX_GIGA: i32 = 9;
pub const PFX_TERA: i32 = 12;

/// `SPICE_VALUE`: `base * 10^prefix`, `spice_str` when it was read from a string with a unit after the number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpiceValue {
    pub base: f64,
    pub prefix: i32,
    pub spice_str: bool,
}

impl Default for SpiceValue {
    fn default() -> Self {
        SpiceValue { base: 0.0, prefix: PFX_NONE, spice_str: false }
    }
}

/// The C library's `strtod` on the longest prefix of `s` it accepts, and what is left (`sscanf( "%lf" )`). Leading blanks are skipped.
fn strtod(s: &str) -> Option<(f64, &str)> {
    let t = s.trim_start();
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut mantissa_digits = i - digits_start;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let f = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        mantissa_digits += i - f;
    }
    if mantissa_digits == 0 {
        // "inf" and "nan" are numbers to strtod too; nobody writes them in a SPICE value.
        return None;
    }
    let mut end = i;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let e = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > e {
            end = j;
        }
    }
    t[..end].parse::<f64>().ok().map(|v| (v, &t[end..]))
}

impl SpiceValue {
    /// `SPICE_VALUE( double aDouble, UNIT_PREFIX aPrefix )`: normalized.
    pub fn new(base: f64, prefix: i32) -> SpiceValue {
        let mut v = SpiceValue { base, prefix, spice_str: false };
        v.normalize();
        v
    }

    pub fn from_f64(x: f64) -> SpiceValue {
        SpiceValue::new(x, PFX_NONE)
    }

    /// `SPICE_VALUE( const wxString& )`. KiCad throws `KI_PARAM_ERROR` on a string `sscanf` reads no number from; that is `None` here. An
    /// empty string is zero, as in KiCad.
    pub fn parse(s: &str) -> Option<SpiceValue> {
        let mut v = SpiceValue::default();
        if s.is_empty() {
            return Some(v);
        }
        let (base, rest) = strtod(s)?;
        v.base = base;
        // "%7s": the next blank-delimited word, at most seven characters.
        let units: String = rest.trim_start().chars().take_while(|c| !c.is_whitespace()).take(7).collect::<String>().to_lowercase();
        if units.is_empty() {
            v.normalize();
            return Some(v);
        }
        v.spice_str = true;
        v.prefix = if units == "meg" {
            PFX_MEGA
        } else {
            match units.chars().next() {
                Some('f') => PFX_FEMTO,
                Some('p') => PFX_PICO,
                Some('n') => PFX_NANO,
                Some('u') => PFX_MICRO,
                Some('m') => PFX_MILI,
                Some('k') => PFX_KILO,
                Some('g') => PFX_GIGA,
                Some('t') => PFX_TERA,
                _ => PFX_NONE,
            }
        };
        v.normalize();
        Some(v)
    }

    /// `SPICE_VALUE::Normalize`: the prefix picked so that 1 <= |base| < 1000, within femto to tera.
    pub fn normalize(&mut self) {
        while self.base.abs() >= 1000.0 {
            if self.prefix == PFX_TERA {
                break;
            }
            self.base *= 0.001;
            self.prefix += 3;
        }
        while self.base != 0.0 && self.base.abs() < 1.000 {
            if self.prefix == PFX_FEMTO {
                break;
            }
            self.base *= 1000.0;
            self.prefix -= 3;
        }
    }

    /// `SPICE_VALUE::ToDouble`.
    pub fn to_f64(&self) -> f64 {
        let mut res = self.base;
        if self.prefix != PFX_NONE {
            res *= 10f64.powi(self.prefix);
        }
        res
    }

    /// `SPICE_VALUE::ToString()`: `%.3f` of the value with its trailing zeros stripped.
    pub fn to_plain_string(&self) -> String {
        let mut s = format!("{:.3}", self.to_f64());
        strip_zeros(&mut s);
        s
    }

    /// `SPICE_VALUE::ToSpiceString`: the base as `wxString::FromCDouble` writes a double (six significant digits, `%g`), its zeros stripped,
    /// then the SPICE prefix ("Meg" for mega).
    pub fn to_spice_string(&self) -> String {
        let mut s = format_g(self.base, 6);
        strip_zeros(&mut s);
        s.push_str(spice_prefix(self.prefix));
        s
    }

    /// `SPICE_VALUE::ToOrigString`.
    pub fn to_orig_string(&self) -> String {
        if self.spice_str {
            self.to_spice_string()
        } else {
            self.to_plain_string()
        }
    }

    /// `SPICE_VALUE::ToString( const SPICE_VALUE_FORMAT& )`: `precision` significant digits, in the range's prefix (or the normalized one when
    /// the range starts with '~'), then the range's unit. A range ending in a degree sign is an angle normalized to (-180, 180].
    pub fn to_string_fmt(&self, format: &SpiceValueFormat) -> String {
        let mut v = *self;
        let mut range = format.range.clone();
        if range.ends_with('°') {
            let mut deg = v.base * 10f64.powi(v.prefix);
            // EDA_ANGLE::Normalize180: into (-180, 180].
            while deg <= -180.0 {
                deg += 360.0;
            }
            while deg > 180.0 {
                deg -= 360.0;
            }
            return format!("{:.*}°", format.precision.max(0) as usize, deg);
        }
        if let Some(rest) = range.strip_prefix('~') {
            v.normalize();
            range = format!("{}{}", si_prefix(v.prefix), rest);
        } else {
            let range_prefix = parse_si_prefix(range.chars().next().unwrap_or(' '));
            v.base *= 10f64.powi(v.prefix - range_prefix);
            v.prefix = range_prefix;
        }
        let mut mantissa = v.base;
        let mut scale: i32 = 0;
        while mantissa.abs() >= 10.0 {
            mantissa *= 0.1;
            scale += 1;
        }
        while mantissa != 0.0 && mantissa.abs() < 1.0 {
            mantissa *= 10.0;
            scale -= 1;
        }
        let p = format.precision;
        mantissa = ki_round(mantissa * 10f64.powi(p - 1));
        mantissa *= 10f64.powi(scale - p + 1);
        let digits = (p - scale - 1).max(0) as usize;
        let mut res = format!("{mantissa:.digits$}");
        // "If we have an excessively long number, switch to scientific notation"
        if res.chars().count() as i64 > i64::from(p) + i64::from(scale) + 1 {
            res = format_g(mantissa, 6);
        }
        res + &range
    }

    fn combine(&self, other: &SpiceValue, f: impl Fn(f64, f64) -> f64) -> SpiceValue {
        let diff = self.prefix - other.prefix;
        let mut res = SpiceValue { spice_str: self.spice_str || other.spice_str, ..SpiceValue::default() };
        if diff > 0 {
            res.base = f(self.base * 10f64.powi(diff), other.base);
            res.prefix = other.prefix;
        } else if diff < 0 {
            res.base = f(self.base, other.base * 10f64.powi(-diff));
            res.prefix = self.prefix;
        } else {
            res.base = f(self.base, other.base);
            res.prefix = self.prefix;
        }
        res.normalize();
        res
    }

    /// `SPICE_VALUE::operator+`.
    pub fn add(&self, other: &SpiceValue) -> SpiceValue {
        self.combine(other, |a, b| a + b)
    }

    /// `SPICE_VALUE::operator-`.
    pub fn sub(&self, other: &SpiceValue) -> SpiceValue {
        self.combine(other, |a, b| a - b)
    }

    /// `SPICE_VALUE::operator*`.
    pub fn mul(&self, other: &SpiceValue) -> SpiceValue {
        let mut r = SpiceValue::new(self.base * other.base, self.prefix + other.prefix);
        r.spice_str = self.spice_str || other.spice_str;
        r
    }

    /// `SPICE_VALUE::operator/`.
    pub fn div(&self, other: &SpiceValue) -> SpiceValue {
        let mut r = SpiceValue::new(self.base / other.base, self.prefix - other.prefix);
        r.spice_str = self.spice_str || other.spice_str;
        r
    }
}

/// `KiROUND` of a double: half away from zero.
pub fn ki_round(x: f64) -> f64 {
    x.round()
}

/// `SPICE_VALUE::StripZeros`: trailing zeros after a decimal separator go, and the separator when nothing is left after it.
pub fn strip_zeros(s: &mut String) {
    if s.contains(',') || s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') || s.ends_with(',') {
            s.pop();
        }
    }
}

/// `spice_prefix()`: the prefix as SPICE writes it.
pub fn spice_prefix(p: i32) -> &'static str {
    match p {
        PFX_FEMTO => "f",
        PFX_PICO => "p",
        PFX_NANO => "n",
        PFX_MICRO => "u",
        PFX_MILI => "m",
        PFX_KILO => "k",
        PFX_MEGA => "Meg",
        PFX_GIGA => "G",
        PFX_TERA => "T",
        _ => "",
    }
}

/// `si_prefix()`: the prefix as the SI writes it (with KiCad's capital K).
pub fn si_prefix(p: i32) -> &'static str {
    match p {
        PFX_FEMTO => "f",
        PFX_PICO => "p",
        PFX_NANO => "n",
        PFX_MICRO => "u",
        PFX_MILI => "m",
        PFX_KILO => "K",
        PFX_MEGA => "M",
        PFX_GIGA => "G",
        PFX_TERA => "T",
        _ => "",
    }
}

/// `SPICE_VALUE::ParseSIPrefix`.
pub fn parse_si_prefix(c: char) -> i32 {
    match c {
        'f' => PFX_FEMTO,
        'p' => PFX_PICO,
        'n' => PFX_NANO,
        'u' => PFX_MICRO,
        'm' => PFX_MILI,
        'K' => PFX_KILO,
        'M' => PFX_MEGA,
        'G' => PFX_GIGA,
        'T' => PFX_TERA,
        _ => PFX_NONE,
    }
}

/// C's `%.<precision>g` (what `std::ostream` writes for a double by default, at precision 6): `precision` significant digits, the exponent
/// form when the exponent is below -4 or not below the precision, trailing zeros removed, a two-digit-at-least exponent.
pub fn format_g(x: f64, precision: usize) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let p = precision.max(1);
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    // The exponent of the value rounded to p significant digits.
    let sci = format!("{:.*e}", p - 1, x);
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if exp < -4 || exp >= p as i32 {
        let mut m = mant.to_string();
        if m.contains('.') {
            while m.ends_with('0') {
                m.pop();
            }
            if m.ends_with('.') {
                m.pop();
            }
        }
        format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        let mut f = format!("{x:.decimals$}");
        if f.contains('.') {
            while f.ends_with('0') {
                f.pop();
            }
            if f.ends_with('.') {
                f.pop();
            }
        }
        f
    }
}

/// `SPICE_VALUE_FORMAT`: significant digits and a range -- an SI prefix and a unit ("mV"), or '~' and a unit for the prefix that suits the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpiceValueFormat {
    pub precision: i32,
    pub range: String,
}

impl SpiceValueFormat {
    /// `SPICE_VALUE_FORMAT::FromString`: "3~V" is three digits, auto-ranged volts; an empty string is that default.
    pub fn from_string(s: &str) -> SpiceValueFormat {
        if s.is_empty() {
            return SpiceValueFormat { precision: 3, range: "~V".into() };
        }
        let mut chars = s.chars();
        let first = chars.next().unwrap_or('3');
        // wxString::ToLong of the first character: a digit, else 0.
        let precision = first.to_digit(10).map_or(0, |d| d as i32);
        SpiceValueFormat { precision, range: chars.collect() }
    }

    /// `SPICE_VALUE_FORMAT::ToString`.
    pub fn to_string_kicad(&self) -> String {
        format!("{}{}", self.precision.clamp(0, 9), self.range)
    }

    /// `SPICE_VALUE_FORMAT::UpdateUnits`: the unit replaced, the prefix (or '~') kept.
    pub fn update_units(&mut self, units: &str) {
        let first = self.range.chars().next();
        match first {
            Some('~') => self.range = format!("~{units}"),
            Some(c) if parse_si_prefix(c) != PFX_NONE => self.range = format!("{c}{units}"),
            _ => self.range = units.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> SpiceValue {
        SpiceValue::parse(s).unwrap()
    }

    #[test]
    fn values_parse_with_their_spice_prefix() {
        assert_eq!(v("10k").to_f64(), 10_000.0);
        assert_eq!(v("4.7u").prefix, PFX_MICRO);
        assert!((v("4.7u").base - 4.7).abs() < 1e-12);
        assert_eq!(v("1Meg").to_f64(), 1e6);
        assert!((v("1MEG").to_f64() - 1e6).abs() < 1e-6);
        // 'M' is milli to SPICE: only "meg" is mega.
        assert!((v("1M").to_f64() - 1e-3).abs() < 1e-15);
        assert_eq!(v("5").to_f64(), 5.0);
        assert!(!v("5").spice_str && v("5V").spice_str);
        assert_eq!(v("5V").to_f64(), 5.0, "an unknown unit is no prefix");
        assert_eq!(v("1e3").to_f64(), 1000.0);
        assert_eq!(v("").to_f64(), 0.0);
        assert!(SpiceValue::parse("abc").is_none());
    }

    #[test]
    fn spice_strings_are_normalized_with_six_digits() {
        assert_eq!(v("1000").to_spice_string(), "1k");
        assert_eq!(v("0.001").to_spice_string(), "1m");
        assert_eq!(v("10u").to_spice_string(), "10u");
        assert_eq!(v("1500000").to_spice_string(), "1.5Meg");
        assert_eq!(v("0").to_spice_string(), "0");
        assert_eq!(SpiceValue::from_f64(1.0 / 3.0).to_spice_string(), "333.333m");
        assert_eq!(v("2.5").to_orig_string(), "2.5");
        assert_eq!(v("2.5k").to_orig_string(), "2.5k");
        assert_eq!(SpiceValue::from_f64(1234.5678).to_plain_string(), "1234.568");
    }

    #[test]
    fn formatted_values_keep_their_significant_digits() {
        let f = SpiceValueFormat::from_string("3~V");
        assert_eq!(SpiceValue::from_f64(4.99326).to_string_fmt(&f), "4.99V");
        assert_eq!(SpiceValue::from_f64(0.0031623).to_string_fmt(&f), "3.16mV");
        assert_eq!(SpiceValue::from_f64(12345.0).to_string_fmt(&f), "12.3KV");
        assert_eq!(SpiceValue::from_f64(0.0).to_string_fmt(&f), "0.00V");
        let fixed = SpiceValueFormat::from_string("4mV");
        assert_eq!(SpiceValue::from_f64(1.5).to_string_fmt(&fixed), "1500mV");
        let s = SpiceValueFormat::from_string("3~s");
        assert_eq!(SpiceValue::from_f64(1e-3).to_string_fmt(&s), "1.00ms");
        let deg = SpiceValueFormat::from_string("1°");
        assert_eq!(SpiceValue::from_f64(-190.0).to_string_fmt(&deg), "170.0°");
    }

    #[test]
    fn formats_read_write_and_change_units() {
        let mut f = SpiceValueFormat::from_string("");
        assert_eq!(f.to_string_kicad(), "3~V");
        f.update_units("A");
        assert_eq!(f.range, "~A");
        let mut g = SpiceValueFormat::from_string("2mV");
        g.update_units("s");
        assert_eq!(g.to_string_kicad(), "2ms");
        let mut h = SpiceValueFormat::from_string("5V");
        h.update_units("W");
        assert_eq!(h.range, "W");
    }

    #[test]
    fn arithmetic_works_across_prefixes() {
        let half = SpiceValue::from_f64(0.5).mul(&v("10k"));
        assert_eq!(half.to_spice_string(), "5k");
        assert_eq!(v("5m").sub(&v("0")).div(&SpiceValue::from_f64(50.0)).to_spice_string(), "100u");
        assert_eq!(v("1k").add(&v("500")).to_spice_string(), "1.5k");
    }

    #[test]
    fn percent_g_matches_c() {
        assert_eq!(format_g(1e-5, 6), "1e-05");
        assert_eq!(format_g(0.0001, 6), "0.0001");
        assert_eq!(format_g(123456.0, 6), "123456");
        assert_eq!(format_g(1234567.0, 6), "1.23457e+06");
        assert_eq!(format_g(4.7, 6), "4.7");
        assert_eq!(format_g(-2.5e-12, 6), "-2.5e-12");
    }
}
