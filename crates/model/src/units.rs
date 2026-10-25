//! Lengths: integer nanometres, KiCad's board unit, and the exact conversions at the IR's boundaries.
//!
//! Every length in the IR is an [`Nm`], a whole number of nanometres, as KiCad's `pcbIUScale` holds it (`include/base_units.h`: `PCB_IU_PER_MM = 1e6`,
//! "Pcbnew IU is 1 nanometer"). The schematic holds nanometres too, but on KiCad's own schematic unit, 100 nm (`SCH_IU_PER_MM = 1e4`, "Schematic internal
//! units 1=100nm"): its reader rounds to that unit ([`sch_from_mm`]) and its writer writes it ([`sch_format_mm`]), so a schematic length is always what
//! KiCad would hold. See `docs/NANOMETRES.md`.
//!
//! The design file was micrometres up to its `schema` 1; [`de`] is what every length field of it reads through, and multiplies what it read by the
//! thread's [`LengthScale`] (1000 while an old document is read, 1 otherwise): see `crate::ir::Design`'s reader.

use serde::{Deserialize, Deserializer};
use std::cell::Cell;

/// A length: a whole number of nanometres (1 mm = 1_000_000 nm), KiCad's board unit. A type of its own so that a millimetre float, a micrometre literal or
/// a count cannot be taken for a length: build one with [`Nm::new`] (a count of nanometres), [`Nm::from_mm`] (KiCad's reader) or the [`mm`] / [`um`] /
/// [`mil`] helpers, and read it with [`Nm::get`] (the count) or [`Nm::mm`]. See `docs/NANOMETRES.md`, "The length type".
///
/// The arithmetic is the one lengths have: `Nm ± Nm`, `-Nm`, `Nm * i64`, `i64 * Nm`, `Nm / i64`, `Nm % Nm`, and `Nm / Nm`, a count (`i64`, truncated as
/// an integer division is). There is no `Nm * Nm`: an area is not a length ([`Nm::area`]). Comparisons, `Sum`, `Default` (zero), `Hash` are those of
/// the count. It is written as its count (`#[serde(transparent)]`, and `Debug` / `Display` print the bare integer), so a document and a message read
/// the same as when a length was an `i64`.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, Deserialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct Nm(i64);

impl Nm {
    /// No length.
    pub const ZERO: Nm = Nm(0);
    /// The largest length (a sentinel: "unbounded", the start of a minimum).
    pub const MAX: Nm = Nm(i64::MAX);
    /// The smallest (most negative) length.
    pub const MIN: Nm = Nm(i64::MIN);

    /// `nm` nanometres.
    #[inline]
    pub const fn new(nm: i64) -> Nm {
        Nm(nm)
    }

    /// The count of nanometres.
    #[inline]
    pub const fn get(self) -> i64 {
        self.0
    }

    /// Millimetres to nanometres as KiCad reads a board length: `KiROUND( std::clamp( mm * pcbIUScale.IU_PER_MM, -INT_LIMIT, INT_LIMIT ) )`
    /// (`PCB_IO_KICAD_SEXPR_PARSER::parseBoardUnits`).
    #[inline]
    pub fn from_mm(mm: f64) -> Nm {
        Nm(kiround((mm * PCB_IU_PER_MM).clamp(-INT_LIMIT, INT_LIMIT)))
    }

    /// A length computed in floating-point nanometres, rounded as KiCad's `KiROUND` (half away from zero; NaN is 0).
    #[inline]
    pub fn kiround(nm: f64) -> Nm {
        Nm(kiround(nm))
    }

    /// The length in millimetres.
    #[inline]
    pub fn mm(self) -> f64 {
        self.0 as f64 / PCB_IU_PER_MM
    }

    /// The length in micrometres, as a float (a message or a third-party format that counts micrometres).
    #[inline]
    pub fn um(self) -> f64 {
        self.0 as f64 / NM_PER_UM as f64
    }

    /// The length in mils, as a float.
    #[inline]
    pub fn mils(self) -> f64 {
        self.0 as f64 / NM_PER_MIL as f64
    }

    /// The count of nanometres as a float, for geometry done in floating point (a distance, an angle, a projection).
    #[inline]
    pub fn as_f64(self) -> f64 {
        self.0 as f64
    }

    /// `self / other` as a float: how many of `other` make `self`.
    #[inline]
    pub fn ratio(self, other: Nm) -> f64 {
        self.0 as f64 / other.0 as f64
    }

    /// The area of a `self` by `other` rectangle, in nm^2, exactly (`i128`: two board coordinates multiply past `i64` near 3 m).
    #[inline]
    pub fn area(self, other: Nm) -> i128 {
        self.0 as i128 * other.0 as i128
    }

    /// The length scaled by a float factor, rounded as `KiROUND` (a percentage of a width, half a gap computed in floating point).
    #[inline]
    pub fn scale(self, k: f64) -> Nm {
        Nm(kiround(self.0 as f64 * k))
    }

    #[inline]
    pub const fn abs(self) -> Nm {
        Nm(self.0.abs())
    }

    /// -1, 0 or 1.
    #[inline]
    pub const fn signum(self) -> i64 {
        self.0.signum()
    }

    #[inline]
    pub const fn is_positive(self) -> bool {
        self.0 > 0
    }

    #[inline]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// The remainder of `self / step`, never negative (a coordinate's offset from a grid of `step`).
    #[inline]
    pub const fn rem_euclid(self, step: Nm) -> Nm {
        Nm(self.0.rem_euclid(step.0))
    }

    /// How many whole `step`s fit below `self`, rounded towards minus infinity (a coordinate's grid index).
    #[inline]
    pub const fn div_euclid(self, step: Nm) -> i64 {
        self.0.div_euclid(step.0)
    }

    #[inline]
    pub const fn checked_add(self, o: Nm) -> Option<Nm> {
        match self.0.checked_add(o.0) {
            Some(v) => Some(Nm(v)),
            None => None,
        }
    }

    #[inline]
    pub const fn checked_sub(self, o: Nm) -> Option<Nm> {
        match self.0.checked_sub(o.0) {
            Some(v) => Some(Nm(v)),
            None => None,
        }
    }

    #[inline]
    pub const fn saturating_add(self, o: Nm) -> Nm {
        Nm(self.0.saturating_add(o.0))
    }

    #[inline]
    pub const fn saturating_sub(self, o: Nm) -> Nm {
        Nm(self.0.saturating_sub(o.0))
    }

    /// The length on KiCad's schematic grid: rounded to 100 nm, half away from zero ([`sch_round`]).
    #[inline]
    pub fn sch_round(self) -> Nm {
        sch_round(self)
    }

    /// Wave-1 shim (`docs/NANOMETRES.md`, "The length type"): the same as [`Nm::get`], at a place outside `crates/model` where a length still meets code
    /// that holds lengths as bare `i64`. Wave 2 replaces each with the typed form.
    #[doc(hidden)]
    #[inline]
    pub const fn raw_shim(self) -> i64 {
        self.0
    }

    /// Wave-1 shim: the same as [`Nm::new`], where a bare `i64` (a literal, a value from code not converted yet) becomes a length outside
    /// `crates/model`. Wave 2 replaces each with the typed form ([`mm`], [`Nm::from_mm`], a typed constant).
    #[doc(hidden)]
    #[inline]
    pub const fn from_raw_shim(nm: i64) -> Nm {
        Nm(nm)
    }
}

impl std::fmt::Debug for Nm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.0, f)
    }
}

impl std::fmt::Display for Nm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// The count, as `serde_json::Value::from(i64)` makes it (what `json!` writes for a length too).
impl From<Nm> for serde_json::Value {
    fn from(v: Nm) -> serde_json::Value {
        serde_json::Value::from(v.0)
    }
}

impl std::str::FromStr for Nm {
    type Err = std::num::ParseIntError;
    /// A count of nanometres, as an `i64` parses.
    fn from_str(s: &str) -> Result<Nm, Self::Err> {
        s.parse().map(Nm)
    }
}

impl std::ops::Add for Nm {
    type Output = Nm;
    #[inline]
    fn add(self, o: Nm) -> Nm {
        Nm(self.0 + o.0)
    }
}

impl std::ops::Sub for Nm {
    type Output = Nm;
    #[inline]
    fn sub(self, o: Nm) -> Nm {
        Nm(self.0 - o.0)
    }
}

/// `&a - &b`, `a + &b`, ...: the by-reference forms `i64` has, for iterators of lengths (`xs.iter().max().unwrap() - xs.iter().min().unwrap()`).
macro_rules! nm_ref_ops {
    ($($tr:ident $f:ident),*) => {$(
        impl std::ops::$tr<&Nm> for Nm {
            type Output = Nm;
            #[inline]
            fn $f(self, o: &Nm) -> Nm {
                std::ops::$tr::$f(self, *o)
            }
        }
        impl std::ops::$tr<Nm> for &Nm {
            type Output = Nm;
            #[inline]
            fn $f(self, o: Nm) -> Nm {
                std::ops::$tr::$f(*self, o)
            }
        }
        impl std::ops::$tr<&Nm> for &Nm {
            type Output = Nm;
            #[inline]
            fn $f(self, o: &Nm) -> Nm {
                std::ops::$tr::$f(*self, *o)
            }
        }
    )*};
}
nm_ref_ops!(Add add, Sub sub);

impl std::ops::Neg for &Nm {
    type Output = Nm;
    #[inline]
    fn neg(self) -> Nm {
        Nm(-self.0)
    }
}

impl std::ops::Div<i64> for &Nm {
    type Output = Nm;
    #[inline]
    fn div(self, k: i64) -> Nm {
        Nm(self.0 / k)
    }
}

impl std::ops::Mul<i64> for &Nm {
    type Output = Nm;
    #[inline]
    fn mul(self, k: i64) -> Nm {
        Nm(self.0 * k)
    }
}

impl std::ops::Neg for Nm {
    type Output = Nm;
    #[inline]
    fn neg(self) -> Nm {
        Nm(-self.0)
    }
}

impl std::ops::Mul<i64> for Nm {
    type Output = Nm;
    #[inline]
    fn mul(self, k: i64) -> Nm {
        Nm(self.0 * k)
    }
}

impl std::ops::Mul<Nm> for i64 {
    type Output = Nm;
    #[inline]
    fn mul(self, v: Nm) -> Nm {
        Nm(self * v.0)
    }
}

impl std::ops::Div<i64> for Nm {
    type Output = Nm;
    #[inline]
    fn div(self, k: i64) -> Nm {
        Nm(self.0 / k)
    }
}

/// How many `o` in `self`, truncated towards zero as an integer division.
impl std::ops::Div for Nm {
    type Output = i64;
    #[inline]
    fn div(self, o: Nm) -> i64 {
        self.0 / o.0
    }
}

impl std::ops::Rem for Nm {
    type Output = Nm;
    #[inline]
    fn rem(self, o: Nm) -> Nm {
        Nm(self.0 % o.0)
    }
}

impl std::ops::AddAssign for Nm {
    #[inline]
    fn add_assign(&mut self, o: Nm) {
        self.0 += o.0;
    }
}

impl std::ops::SubAssign for Nm {
    #[inline]
    fn sub_assign(&mut self, o: Nm) {
        self.0 -= o.0;
    }
}

impl std::ops::MulAssign<i64> for Nm {
    #[inline]
    fn mul_assign(&mut self, k: i64) {
        self.0 *= k;
    }
}

impl std::ops::DivAssign<i64> for Nm {
    #[inline]
    fn div_assign(&mut self, k: i64) {
        self.0 /= k;
    }
}

impl std::iter::Sum for Nm {
    fn sum<I: Iterator<Item = Nm>>(it: I) -> Nm {
        Nm(it.map(|v| v.0).sum())
    }
}

impl<'a> std::iter::Sum<&'a Nm> for Nm {
    fn sum<I: Iterator<Item = &'a Nm>>(it: I) -> Nm {
        Nm(it.map(|v| v.0).sum())
    }
}

/// `x` millimetres, rounded half away from zero (no clamp): a length typed by a person, computed in floating point, or written in a test.
pub fn mm(x: f64) -> Nm {
    Nm(kiround(x * PCB_IU_PER_MM))
}

/// `x` micrometres, rounded half away from zero.
pub fn um(x: f64) -> Nm {
    Nm(kiround(x * NM_PER_UM as f64))
}

/// `x` mils (thousandths of an inch), rounded half away from zero.
pub fn mil(x: f64) -> Nm {
    Nm(kiround(x * NM_PER_MIL as f64))
}

/// `PCB_IU_PER_MM` (`include/base_units.h`): the board's internal unit is a nanometre.
pub const PCB_IU_PER_MM: f64 = 1e6;
/// `SCH_IU_PER_MM`: the schematic's internal unit is 100 nm.
pub const SCH_IU_PER_MM: f64 = 1e4;
/// Nanometres in a millimetre.
pub const NM_PER_MM: i64 = 1_000_000;
/// Nanometres in a micrometre.
pub const NM_PER_UM: i64 = 1_000;
/// `pcbIUScale.IU_PER_MILS`: nanometres in a thousandth of an inch.
pub const NM_PER_MIL: i64 = 25_400;
/// Nanometres in an inch.
pub const NM_PER_INCH: i64 = 25_400_000;
/// Nanometres in KiCad's schematic internal unit.
pub const SCH_IU_NM: i64 = 100;

/// `INT_LIMIT` of `pcb_io_kicad_sexpr_parser.cpp`: `std::numeric_limits<int>::max() - 10`, the largest board unit the reader keeps.
pub const INT_LIMIT: f64 = (i32::MAX - 10) as f64;

/// `KiROUND` (`libs/kimath/include/math/util.h`) to a 64-bit result: `std::llround`, half away from zero.
pub fn kiround(v: f64) -> i64 {
    if v.is_nan() {
        0
    } else {
        v.round() as i64
    }
}

/// Millimetres to nanometres as KiCad reads a board length: `KiROUND( std::clamp( mm * pcbIUScale.IU_PER_MM, -INT_LIMIT, INT_LIMIT ) )`
/// (`PCB_IO_KICAD_SEXPR_PARSER::parseBoardUnits`).
pub fn from_mm(mm: f64) -> Nm {
    Nm::from_mm(mm)
}

/// Millimetres to nanometres as KiCad reads a schematic length: `KiROUND( std::clamp( mm * schIUScale.IU_PER_MM, -int_limit, int_limit ) )` with
/// `int_limit = INT_MAX * 0.7071` (`SCH_IO_KICAD_SEXPR_PARSER::parseInternalUnits`), in KiCad's 100 nm unit, then in nanometres.
pub fn sch_from_mm(mm: f64) -> Nm {
    let int_limit = i32::MAX as f64 * 0.7071;
    Nm(kiround((mm * SCH_IU_PER_MM).clamp(-int_limit, int_limit)) * SCH_IU_NM)
}

/// A length on KiCad's schematic grid: `nm` rounded to 100 nm, half away from zero (`KiROUND( nm / 100 ) * 100`).
pub fn sch_round(nm: Nm) -> Nm {
    let nm = nm.0;
    let q = nm.abs() / SCH_IU_NM + i64::from(nm.abs() % SCH_IU_NM >= SCH_IU_NM / 2);
    Nm(nm.signum() * q * SCH_IU_NM)
}

/// Nanometres to millimetres.
pub fn to_mm(nm: Nm) -> f64 {
    nm.mm()
}

/// Nanometres to micrometres, as a float (a message or a third-party format that counts micrometres).
pub fn to_um(nm: Nm) -> f64 {
    nm.um()
}

/// Nanometres to mils, as a float.
pub fn to_mils(nm: Nm) -> f64 {
    nm.mils()
}


/// A length as KiCad writes a board one: `EDA_UNIT_UTILS::FormatInternalUnits( pcbIUScale, aValue )` (`common/eda_units.cpp`), millimetres with ten
/// significant digits (`{:.10g}`), or, at or below 0.0001 mm, ten decimals with the trailing zeros taken off. Below 10 000 mm a nanometre count has at
/// most ten significant digits as millimetres, so it is written exactly, from the integer.
pub fn format_mm(nm: Nm) -> String {
    format_iu(nm.0, 6)
}

/// A length as KiCad writes a schematic one: `FormatInternalUnits( schIUScale, aValue )` of the length in KiCad's 100 nm unit ([`sch_round`]), so at most
/// four decimals of a millimetre.
pub fn sch_format_mm(nm: Nm) -> String {
    format_iu(sch_round(nm).0 / SCH_IU_NM, 4)
}

/// `FormatInternalUnits` of `iu` internal units of `10^-decimals` mm each.
fn format_iu(iu: i64, decimals: u32) -> String {
    let scale = 10i64.pow(decimals);
    let a = iu.unsigned_abs();
    // `{:.10g}` keeps every digit while the whole has at most 10 - decimals digits; KiCad's own lengths never come near.
    if a < 10u64.pow(10) {
        let (int, frac) = (a / scale as u64, a % scale as u64);
        let mut s = String::new();
        if iu < 0 {
            s.push('-');
        }
        s.push_str(&int.to_string());
        if frac != 0 {
            s.push('.');
            s.push_str(format!("{frac:0width$}", width = decimals as usize).trim_end_matches('0'));
        }
        return s;
    }
    format_g10(iu as f64 / scale as f64)
}

/// C's `%.10g` (fmt's `{:.10g}`): ten significant digits, the trailing zeros taken off, an exponent below 1e-5 or from 1e10.
pub fn format_g10(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    if !v.is_finite() {
        return format!("{v}");
    }
    let sci = format!("{v:.9e}");
    let (mantissa, exp) = sci.split_once('e').expect("an exponent");
    let exp: i32 = exp.parse().expect("an integer exponent");
    if !(-5..10).contains(&exp) {
        let m = mantissa.trim_end_matches('0').trim_end_matches('.');
        return format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    let s = format!("{v:.*}", (9 - exp).max(0) as usize);
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

// ------------------------------------------------------------------------------------------------------------------- reading old documents

thread_local! {
    static SCALE: Cell<i64> = const { Cell::new(1) };
}

/// The factor every length read through [`de`] is multiplied by on this thread, while the guard lives: 1000 while a micrometre document (a `design.json`
/// or an intent of `schema` 1) is read, 1 otherwise. Guards nest; dropping one puts the previous factor back.
pub struct LengthScale {
    previous: i64,
}

impl LengthScale {
    pub fn enter(factor: i64) -> LengthScale {
        LengthScale { previous: SCALE.with(|s| s.replace(factor)) }
    }

    /// The factor in force on this thread.
    pub fn current() -> i64 {
        SCALE.with(Cell::get)
    }
}

impl Drop for LengthScale {
    fn drop(&mut self) {
        SCALE.with(|s| s.set(self.previous));
    }
}

/// A value made of lengths, which a document of an older unit multiplies.
pub trait Lengths {
    fn scale_lengths(&mut self, k: i64);
}

/// An area (nm^2, read through [`de_area`]) or another integer that scales with the unit.
impl Lengths for i64 {
    fn scale_lengths(&mut self, k: i64) {
        *self *= k;
    }
}

impl Lengths for Nm {
    fn scale_lengths(&mut self, k: i64) {
        *self *= k;
    }
}

impl<T: Lengths> Lengths for Option<T> {
    fn scale_lengths(&mut self, k: i64) {
        if let Some(v) = self {
            v.scale_lengths(k);
        }
    }
}

impl<T: Lengths> Lengths for Vec<T> {
    fn scale_lengths(&mut self, k: i64) {
        for v in self {
            v.scale_lengths(k);
        }
    }
}

impl<T: Lengths, const N: usize> Lengths for [T; N] {
    fn scale_lengths(&mut self, k: i64) {
        for v in self {
            v.scale_lengths(k);
        }
    }
}

impl<A: Lengths, B: Lengths> Lengths for (A, B) {
    fn scale_lengths(&mut self, k: i64) {
        self.0.scale_lengths(k);
        self.1.scale_lengths(k);
    }
}

impl<A: Lengths, B: Lengths, C: Lengths, D: Lengths> Lengths for (A, B, C, D) {
    fn scale_lengths(&mut self, k: i64) {
        self.0.scale_lengths(k);
        self.1.scale_lengths(k);
        self.2.scale_lengths(k);
        self.3.scale_lengths(k);
    }
}

impl<K: Ord, T: Lengths> Lengths for std::collections::BTreeMap<K, T> {
    fn scale_lengths(&mut self, k: i64) {
        for v in self.values_mut() {
            v.scale_lengths(k);
        }
    }
}

/// `#[serde(deserialize_with = "crate::units::de")]`: a length field (or an `Option`, `Vec`, tuple or array of lengths), multiplied by the thread's
/// [`LengthScale`] once read.
pub fn de<'de, D: Deserializer<'de>, T: Deserialize<'de> + Lengths>(d: D) -> Result<T, D::Error> {
    let mut v = T::deserialize(d)?;
    let k = LengthScale::current();
    if k != 1 {
        v.scale_lengths(k);
    }
    Ok(v)
}

/// `#[serde(deserialize_with = "crate::units::de_width")]`: a width whose negative values are a flag, not a length (a table's stroke of -1, "none"), so
/// only a width of zero or more is multiplied.
pub fn de_width<'de, D: Deserializer<'de>>(d: D) -> Result<Nm, D::Error> {
    let v = Nm::deserialize(d)?;
    Ok(if v >= Nm::ZERO { v * LengthScale::current() } else { v })
}

/// `#[serde(deserialize_with = "crate::units::de_area")]`: an area (nm^2), multiplied by the square of the thread's [`LengthScale`].
pub fn de_area<'de, D: Deserializer<'de>, T: Deserialize<'de> + Lengths>(d: D) -> Result<T, D::Error> {
    let mut v = T::deserialize(d)?;
    let k = LengthScale::current();
    if k != 1 {
        v.scale_lengths(k * k);
    }
    Ok(v)
}

/// A document whose derived reader [`read_versioned`] wraps: `Self::derived` is the `#[serde(remote = "Self")]` reader of the type.
pub trait Versioned<'de>: Sized {
    fn derived<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error>;
}

/// Reads a whole document of type `T` whose `schema` field says its unit: below `current` (or absent) the document is micrometres, and every length is
/// read x1000 (the thread's [`LengthScale`] while `T::derived` runs). Returns what was read and the version the document said (`None`: no `schema`).
///
/// The document is not buffered when `schema` is its first key, which is where our writers put it (it is the first field of the types that have one).
/// Otherwise, with `buffer`, the entries before it are held as `serde_json::Value`s until it is found (a document built through a `serde_json::Value`,
/// whose keys are sorted, has it after `drawings` and `placement`), and handed to the derived reader with the rest once the unit is known; a JSON
/// value reads into every field type the same way the text does. Without `buffer` (a YAML intent, whose numbers a `serde_json::Value` would fix as
/// numbers before the reader says it wants a string), a first key that is not `schema` makes the document version 1.
pub fn read_versioned<'de, D: Deserializer<'de>, T: Versioned<'de>>(d: D, current: u32, buffer: bool) -> Result<(T, Option<u64>), D::Error> {
    use serde::de::value::{StringDeserializer, U64Deserializer};
    use serde::de::{DeserializeSeed, Error, MapAccess, Visitor};
    use std::marker::PhantomData;

    /// The document again: the entries held before `schema`, `schema`, a key read but not its value (`first`), then the rest of the map.
    struct Replay<A> {
        held: std::vec::IntoIter<(String, serde_json::Value)>,
        value: Option<serde_json::Value>,
        schema: Option<u64>,
        schema_key: bool,
        first: Option<String>,
        map: Option<A>,
    }
    impl<'de, A: MapAccess<'de>> MapAccess<'de> for Replay<A> {
        type Error = A::Error;
        fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, A::Error> {
            if let Some((k, v)) = self.held.next() {
                self.value = Some(v);
                return seed.deserialize(StringDeserializer::<A::Error>::new(k)).map(Some);
            }
            if self.schema_key {
                self.schema_key = false;
                return seed.deserialize(StringDeserializer::<A::Error>::new("schema".into())).map(Some);
            }
            if let Some(k) = self.first.take() {
                return seed.deserialize(StringDeserializer::<A::Error>::new(k)).map(Some);
            }
            match &mut self.map {
                Some(m) => m.next_key_seed(seed),
                None => Ok(None),
            }
        }
        fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, A::Error> {
            if let Some(v) = self.value.take() {
                return seed.deserialize(v).map_err(A::Error::custom);
            }
            if let Some(n) = self.schema.take() {
                return seed.deserialize(U64Deserializer::<A::Error>::new(n));
            }
            match &mut self.map {
                Some(m) => m.next_value_seed(seed),
                None => Err(A::Error::custom("a value without a key")),
            }
        }
    }
    struct ReplayDe<A>(Replay<A>);
    impl<'de, A: MapAccess<'de>> Deserializer<'de> for ReplayDe<A> {
        type Error = A::Error;
        fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, A::Error> {
            v.visit_map(self.0)
        }
        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map struct enum identifier ignored_any
        }
    }
    struct Peek<T>(u32, bool, PhantomData<T>);
    impl<'de, T: Versioned<'de>> Visitor<'de> for Peek<T> {
        type Value = (T, Option<u64>);
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a document")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut held = Vec::new();
            let mut schema = None;
            let mut pending_key = None;
            let mut ended = false;
            loop {
                match map.next_key::<String>()? {
                    Some(k) if k == "schema" => {
                        schema = Some(map.next_value::<u64>()?);
                        break;
                    }
                    Some(k) if self.1 => held.push((k.clone(), map.next_value::<serde_json::Value>()?)),
                    Some(k) => {
                        pending_key = Some(k);
                        break;
                    }
                    None => {
                        ended = true;
                        break;
                    }
                }
            }
            let _scale = LengthScale::enter(if schema.unwrap_or(0) < self.0 as u64 { NM_PER_UM } else { 1 });
            // in the unbuffered case a key may have been read but not its value: it goes first, its value read from the map as usual
            let replay = Replay { held: held.into_iter(), value: None, schema, schema_key: schema.is_some(), first: pending_key, map: (!ended).then_some(map) };
            let t = T::derived(ReplayDe(replay))?;
            Ok((t, schema))
        }
    }
    d.deserialize_map(Peek::<T>(current, buffer, PhantomData))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_millimetres_as_kicad_does() {
        assert_eq!(from_mm(0.3048), Nm::new(304_800));
        assert_eq!(from_mm(-0.0000005), Nm::new(-1), "half away from zero");
        assert_eq!(from_mm(1.27), Nm::new(1_270_000));
        assert_eq!(from_mm(1e9), Nm::new(INT_LIMIT as i64), "clamped to INT_LIMIT");
        assert_eq!(sch_from_mm(1.23456), Nm::new(1_234_600), "the schematic's unit is 100 nm");
        assert_eq!(sch_from_mm(-0.00005), Nm::new(-100));
        assert_eq!(sch_round(Nm::new(150)), Nm::new(200));
        assert_eq!(sch_round(Nm::new(-150)), Nm::new(-200));
        assert_eq!(sch_round(Nm::new(149)), Nm::new(100));
    }

    #[test]
    fn writes_millimetres_as_kicad_does() {
        assert_eq!(format_mm(Nm::new(304_800)), "0.3048");
        assert_eq!(format_mm(Nm::new(1)), "0.000001");
        assert_eq!(format_mm(Nm::new(-1_500_000)), "-1.5");
        assert_eq!(format_mm(Nm::ZERO), "0");
        assert_eq!(format_mm(Nm::new(12_345_678)), "12.345678");
        assert_eq!(format_mm(Nm::new(100_000)), "0.1");
        assert_eq!(sch_format_mm(Nm::new(1_234_560)), "1.2346");
        assert_eq!(sch_format_mm(Nm::new(2_540_000)), "2.54");
        // past 10 000 mm, `{:.10g}` keeps ten significant digits
        assert_eq!(format_mm(Nm::new(12_345_678_912_345)), "12345678.91");
        assert_eq!(format_g10(0.1 + 0.2), "0.3");
        assert_eq!(format_g10(1e-7), "1e-07");
        assert_eq!(format_g10(123456789012.0), "1.23456789e+11");
    }

    #[derive(Deserialize)]
    struct Probe {
        #[serde(deserialize_with = "de")]
        a: Nm,
        #[serde(default, deserialize_with = "de")]
        b: Option<(Nm, Nm)>,
        #[serde(deserialize_with = "de_area")]
        area: i64,
        n: i64,
    }

    #[test]
    fn a_scale_multiplies_lengths_and_only_lengths() {
        let text = r#"{"a": 5, "b": [1, -2], "area": 3, "n": 7}"#;
        let p: Probe = serde_json::from_str(text).unwrap();
        assert_eq!((p.a, p.b, p.area, p.n), (Nm::new(5), Some((Nm::new(1), Nm::new(-2))), 3, 7));
        let p: Probe = {
            let _k = LengthScale::enter(1000);
            serde_json::from_str(text).unwrap()
        };
        assert_eq!((p.a, p.b, p.area, p.n), (Nm::new(5000), Some((Nm::new(1000), Nm::new(-2000))), 3_000_000, 7));
        assert_eq!(LengthScale::current(), 1, "the guard puts the scale back");
    }
}
