//! The numbering half of Create Array in the Footprint Editor: how the pads of the new copies are numbered.
//!
//! Ports `ARRAY_AXIS` (`common/array_axis.cpp`), `AlphabeticFromIndex` (`common/increment.cpp`), the numbering of
//! `ARRAY_GRID_OPTIONS::GetItemNumber` / `ARRAY_CIRCULAR_OPTIONS::GetItemNumber` (`common/array_options.cpp`, in [`crate::array`]),
//! `ARRAY_PAD_NUMBER_PROVIDER` (`pcbnew/array_pad_number_provider.cpp`) and the dialog's own check of the axis entries
//! (`validateAxisOptions`, `pcbnew/dialogs/dialog_create_array.cpp`) at KiCad 8303b2ad.
//!
//! An axis numbers the points of the array along one direction: numerals (`0, 1, ... 9, 10`), hexadecimal, or one of two alphabets
//! (`A, B, ... Z, AA` -- the full 26 letters, or the 20 of ASME Y14.35M-1997 that leave out I, O, S, Q, X and Z). A grid can number its
//! points in one run (`1, 2, 3 ...`) or by coordinates (`A1, A2, ... B1 ...`: the primary axis for the column, the secondary for the
//! row). Where the numbers start is either "From start value" (the entries below) or "Use first free number" -- an artificial run of
//! numerals from 1 that skips the number of every pad that does not take part in the array.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::ArrayGeometry;

/// `ARRAY_AXIS::NUMBERING_TYPE`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumberingType {
    /// `NUMBERING_NUMERIC`: Arabic numerals, 0, 1, ... 9, 10, 11 ...
    #[default]
    Numeric,
    /// `NUMBERING_HEX`.
    Hex,
    /// `NUMBERING_ALPHA_NO_IOSQXZ`: the alphabet without I, O, S, Q, X and Z, which can be taken for numerals and are not used for the pins of
    /// a BGA (ASME Y14.35M-1997 sec. 5.2, formerly MIL-STD-100 sec. 406.5).
    AlphaNoIosqxz,
    /// `NUMBERING_ALPHA_FULL`: all 26 letters.
    AlphaFull,
}

impl NumberingType {
    /// `ARRAY_AXIS::TypeIsNumeric`.
    pub fn is_numeric(self) -> bool {
        matches!(self, NumberingType::Numeric | NumberingType::Hex)
    }

    /// `ARRAY_AXIS::GetAlphabet`.
    pub fn alphabet(self) -> &'static str {
        match self {
            NumberingType::Numeric => "0123456789",
            NumberingType::Hex => "0123456789ABCDEF",
            NumberingType::AlphaNoIosqxz => "ABCDEFGHJKLMNPRTUVWY",
            NumberingType::AlphaFull => "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        }
    }

    /// `schemeNonUnitColsStartAt0`: false for 0, 1 ... 9, 10; true for A, B ... Z, AA (where the column of tens starts at the first letter).
    fn non_unit_cols_start_at_0(self) -> bool {
        matches!(self, NumberingType::AlphaFull | NumberingType::AlphaNoIosqxz)
    }
}

/// `AlphabeticFromIndex`: the `n`-th name of `alphabet` counted the way a spreadsheet counts its columns, with `zero_based_non_unit_cols` the
/// way letters go (`Z`, then `AA`, not `BA`; 9, then 10, not 00).
///
/// `n` is KiCad's `size_t`: a negative index (a negative step run past zero) is read as the huge number it wraps to, as it is there. Where
/// the C++ reads `aAlphabet[-1]` (a name past 26 * 26 - 1 in the alphabets, which it leaves undefined) this takes the last letter.
pub fn alphabetic_from_index(n: u64, alphabet: &str, zero_based_non_unit_cols: bool) -> String {
    let letters: Vec<char> = alphabet.chars().collect();
    let radix = letters.len() as u64;
    let mut out: Vec<char> = Vec::new();
    let mut first_round = true;
    let mut n = n;
    loop {
        let mut m = (n % radix) as i64;
        if zero_based_non_unit_cols && !first_round {
            m -= 1; // start the "tens/hundreds/etc column" at "Ax", not "Bx"
        }
        if m < 0 {
            m += radix as i64;
        }
        out.push(letters[m as usize]);
        n /= radix;
        first_round = false;
        if n == 0 {
            break;
        }
    }
    out.iter().rev().collect()
}

/// `ARRAY_AXIS`: one axis of an array and the numbers along it.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayAxis {
    kind: NumberingType,
    offset: i32,
    /// Skip every `n` numbers: 2 numbers one side of a connector (1, 3, 5 ...).
    step: i32,
    /// Output lowercase letters (an alphabetic axis whose start was typed in lowercase).
    use_lowercase: bool,
}

impl Default for ArrayAxis {
    fn default() -> Self {
        ArrayAxis { kind: NumberingType::Numeric, offset: 0, step: 1, use_lowercase: false }
    }
}

impl ArrayAxis {
    pub fn new() -> Self {
        Self::default()
    }

    /// `SetAxisType`.
    pub fn set_axis_type(&mut self, kind: NumberingType) {
        self.kind = kind;
    }

    pub fn axis_type(&self) -> NumberingType {
        self.kind
    }

    /// `SetOffset( int )`: the start as an index (0 starts at `0` / `A`, 4 at `4` / `E`).
    pub fn set_offset(&mut self, offset: i32) {
        self.offset = offset;
    }

    pub fn offset(&self) -> i32 {
        self.offset
    }

    /// `SetStep`.
    pub fn set_step(&mut self, step: i32) {
        self.step = step;
    }

    /// `getNumberingOffset`: the index a numbering string such as `B` or `5` names in this axis's alphabet, `None` when it is empty or holds
    /// a character the alphabet lacks. For the alphabetic types a lowercase letter reads as its capital, and in `AA` the first letter counts
    /// from 1, so that it is index 26 and not 0 + 26.
    fn numbering_offset(&self, text: &str) -> Option<i32> {
        if text.is_empty() {
            return None;
        }
        let alphabet: Vec<char> = self.kind.alphabet().chars().collect();
        let radix = alphabet.len() as i32;
        let start0 = self.kind.non_unit_cols_start_at_0();
        let count = text.chars().count();
        let mut offset: i32 = 0;
        for (i, mut ch) in text.chars().enumerate() {
            if !self.kind.is_numeric() && ch.is_ascii_lowercase() {
                ch = ch.to_ascii_uppercase();
            }
            let mut index = alphabet.iter().position(|&c| c == ch)? as i32;
            // eg "AA" is actually index 27, not 26
            if start0 && i + 1 < count {
                index += 1;
            }
            offset = offset.checked_mul(radix)?.checked_add(index)?;
        }
        Some(offset)
    }

    /// `SetOffset( const wxString& )`: the start as typed. False, and nothing changed, when it does not decode in this axis's alphabet. A
    /// start typed in lowercase letters makes the whole axis print lowercase.
    pub fn set_offset_text(&mut self, text: &str) -> bool {
        let Some(offset) = self.numbering_offset(text) else { return false };
        self.use_lowercase = !self.kind.is_numeric() && text.chars().next().is_some_and(|c| c.is_ascii_lowercase());
        self.offset = offset;
        true
    }

    /// `GetItemNumber`: the name of the `n`-th point of the axis.
    pub fn item_number(&self, n: i64) -> String {
        // `n = m_offset + m_step * n;` on ints, and the result handed to a `size_t`
        let index = self.offset.wrapping_add(self.step.wrapping_mul(n as i32));
        let result = alphabetic_from_index(index as i64 as u64, self.kind.alphabet(), self.kind.non_unit_cols_start_at_0());
        if self.use_lowercase {
            result.to_lowercase()
        } else {
            result
        }
    }
}

fn d_start() -> String {
    "1".to_string()
}

fn d_step() -> i32 {
    1
}

/// One axis as the dialog has it: the choice of alphabet, the "Pad numbering start" entry as it was typed and the "Pad numbering increment".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisSpec {
    #[serde(default)]
    pub kind: NumberingType,
    #[serde(default = "d_start")]
    pub start: String,
    #[serde(default = "d_step")]
    pub step: i32,
}

impl Default for AxisSpec {
    fn default() -> Self {
        AxisSpec { kind: NumberingType::Numeric, start: d_start(), step: 1 }
    }
}

impl AxisSpec {
    /// `validateAxisOptions`: the axis these entries make, or the dialog's complaint about the start.
    fn axis(&self) -> Result<ArrayAxis, String> {
        let mut axis = ArrayAxis::new();
        axis.set_axis_type(self.kind);
        if !axis.set_offset_text(&self.start) {
            return Err(format!("Could not determine numbering start from '{}': expected value consistent with alphabet '{}'.", self.start, self.kind.alphabet()));
        }
        axis.set_step(self.step);
        Ok(axis)
    }
}

/// The numbering of a footprint editor array: what `DIALOG_CREATE_ARRAY` hands `ARRAY_OPTIONS` when pads are to be renumbered
/// (`SetShouldNumber`). Left out of the command, the copies keep the number of the pad they were made from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PadNumbering {
    /// "From start value" (`SetNumberingStartIsSpecified( true )`); false is "Use first free number": numerals from 1 that skip the numbers
    /// of the pads outside the array.
    #[serde(default)]
    pub start_specified: bool,
    /// A grid's "Coordinate (A1, A2, ... B1, ...)" scheme (`m_2dArrayNumbering`); false is "Continuous (1, 2, 3...)". A circular array has one axis.
    #[serde(default)]
    pub two_d: bool,
    /// "Primary axis numbering" and its start and increment (a circular array's only axis).
    #[serde(default)]
    pub primary: AxisSpec,
    /// "Secondary axis numbering": the rows of a coordinate scheme.
    #[serde(default)]
    pub secondary: AxisSpec,
}

impl Default for PadNumbering {
    fn default() -> Self {
        PadNumbering { start_specified: false, two_d: false, primary: AxisSpec::default(), secondary: AxisSpec::default() }
    }
}

/// [`PadNumbering`] made into the axes `ARRAY_OPTIONS` works with.
#[derive(Debug, Clone, PartialEq)]
pub struct NumberingAxes {
    pub start_specified: bool,
    pub two_d: bool,
    pub primary: ArrayAxis,
    pub secondary: ArrayAxis,
}

impl PadNumbering {
    /// `TransferDataFromWindow`'s half for the numbering: with "Use first free number" an artificial linear numeric scheme from 1; with a
    /// start value each axis in use must decode (`validateAxisOptions`) -- every complaint is returned, one to a line, like the dialog's
    /// message box. `circular` arrays have the one axis.
    pub fn resolve(&self, circular: bool) -> Result<NumberingAxes, String> {
        if !self.start_specified {
            let mut primary = ArrayAxis::new();
            primary.set_axis_type(NumberingType::Numeric);
            primary.set_offset(1); // start at "1"
            return Ok(NumberingAxes { start_specified: false, two_d: false, primary, secondary: ArrayAxis::new() });
        }
        let two_d = !circular && self.two_d;
        let mut errors: Vec<String> = Vec::new();
        let primary = self.primary.axis().map_err(|e| errors.push(e)).ok();
        let secondary = if two_d { self.secondary.axis().map_err(|e| errors.push(e)).ok() } else { Some(ArrayAxis::new()) };
        match (primary, secondary) {
            (Some(primary), Some(secondary)) if errors.is_empty() => Ok(NumberingAxes { start_specified: true, two_d, primary, secondary }),
            _ => Err(errors.join("\n")),
        }
    }
}

/// `ARRAY_PAD_NUMBER_PROVIDER`: hands out the numbers of an array in order, none of which is the number of a pad that stays where it is.
pub struct PadNumberProvider<'a> {
    geometry: &'a ArrayGeometry,
    axes: &'a NumberingAxes,
    existing: BTreeSet<String>,
    index: i64,
}

impl<'a> PadNumberProvider<'a> {
    /// `existing` are the numbers of the pads outside the array; with a start value they are not looked at ("we don't look at existing
    /// names, so it's just an empty reserved set").
    pub fn new(existing: BTreeSet<String>, geometry: &'a ArrayGeometry, axes: &'a NumberingAxes) -> Self {
        let existing = if axes.start_specified { BTreeSet::new() } else { existing };
        PadNumberProvider { geometry, axes, existing, index: 0 }
    }

    /// `GetNextPadNumber`: the first number from the current point on that is not reserved.
    pub fn next_number(&mut self) -> String {
        loop {
            let number = self.geometry.item_number(self.index, self.axes);
            self.index += 1;
            if !self.existing.contains(&number) {
                return number;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use eda_model::units::um;
    use eda_model::units::Nm;
    use super::*;

    fn axis(kind: NumberingType, start: &str, step: i32) -> ArrayAxis {
        let mut a = ArrayAxis::new();
        a.set_axis_type(kind);
        assert!(a.set_offset_text(start), "{start:?} decodes in {kind:?}");
        a.set_step(step);
        a
    }

    fn run(a: &ArrayAxis, n: i64) -> Vec<String> {
        (0..n).map(|i| a.item_number(i)).collect()
    }

    #[test]
    fn numerals_count_from_the_start_by_the_step() {
        assert_eq!(run(&axis(NumberingType::Numeric, "1", 1), 12), ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12"]);
        assert_eq!(run(&axis(NumberingType::Numeric, "0", 1), 3), ["0", "1", "2"]);
        // one side of a connector: the odd pins
        assert_eq!(run(&axis(NumberingType::Numeric, "1", 2), 4), ["1", "3", "5", "7"]);
        assert_eq!(run(&axis(NumberingType::Numeric, "20", -5), 4), ["20", "15", "10", "5"]);
    }

    #[test]
    fn hexadecimal_runs_through_f_and_on_to_10() {
        assert_eq!(run(&axis(NumberingType::Hex, "E", 1), 4), ["E", "F", "10", "11"]);
        assert_eq!(axis(NumberingType::Hex, "1F", 1).item_number(0), "1F");
        let mut hex = ArrayAxis::new();
        hex.set_axis_type(NumberingType::Hex);
        assert!(!hex.set_offset_text("a"), "numeric types do not read lowercase: only the capitals are in the alphabet");
        assert!(!hex.set_offset_text("G"));
    }

    #[test]
    fn the_full_alphabet_goes_z_aa_and_the_tens_column_starts_at_a() {
        let a = axis(NumberingType::AlphaFull, "A", 1);
        assert_eq!(a.item_number(0), "A");
        assert_eq!(a.item_number(25), "Z");
        assert_eq!(a.item_number(26), "AA", "Z, then AA: the column of tens starts at the first letter");
        assert_eq!(a.item_number(27), "AB");
        assert_eq!(a.item_number(51), "AZ");
        assert_eq!(a.item_number(52), "BA");
        assert_eq!(a.item_number(675), "YZ");
    }

    #[test]
    fn the_bga_alphabet_leaves_out_i_o_s_q_x_and_z() {
        let a = axis(NumberingType::AlphaNoIosqxz, "A", 1);
        let first: String = (0..20).map(|i| a.item_number(i)).collect();
        assert_eq!(first, "ABCDEFGHJKLMNPRTUVWY");
        assert_eq!(a.item_number(20), "AA");
        assert_eq!(a.item_number(21), "AB");
        assert_eq!(a.item_number(40), "BA");
        for c in ['I', 'O', 'S', 'Q', 'X', 'Z'] {
            assert!(!first.contains(c));
        }
    }

    #[test]
    fn a_start_is_read_in_the_alphabet_it_belongs_to() {
        // "AA" is index 26, not 0 * 26 + 0: the first letter of a longer name counts from 1
        let mut a = ArrayAxis::new();
        a.set_axis_type(NumberingType::AlphaFull);
        assert!(a.set_offset_text("AA"));
        assert_eq!(a.offset(), 26);
        assert!(a.set_offset_text("BC"));
        assert_eq!(a.offset(), 54, "B counts as 2 in the column of tens: 2 * 26 + C (2)");
        assert_eq!(a.item_number(0), "BC");
        assert!(!a.set_offset_text(""), "an empty start is no start");
        assert!(!a.set_offset_text("A1"), "a numeral is not in the alphabet");
        a.set_axis_type(NumberingType::AlphaNoIosqxz);
        assert!(!a.set_offset_text("I"), "I is not in the BGA alphabet");
        assert!(a.set_offset_text("J"));
        assert_eq!(a.offset(), 8);
    }

    #[test]
    fn a_start_in_lowercase_prints_lowercase_and_reads_the_same() {
        let a = axis(NumberingType::AlphaFull, "b", 1);
        assert_eq!(run(&a, 3), ["b", "c", "d"]);
        assert_eq!(a.item_number(24), "z");
        assert_eq!(a.item_number(25), "aa", "z, then aa");
        let a = axis(NumberingType::AlphaFull, "B", 1);
        assert_eq!(a.item_number(0), "B");
        // the choice is made by the first letter of the start
        let mut mixed = ArrayAxis::new();
        mixed.set_axis_type(NumberingType::AlphaFull);
        assert!(mixed.set_offset_text("aB"));
        assert_eq!(mixed.item_number(0), "ab");
    }

    #[test]
    fn a_negative_index_wraps_as_a_size_t_does() {
        // KiCad hands the int to a `size_t`: counting down past zero gives the digits of 2^64 - 1
        let a = axis(NumberingType::Numeric, "1", -1);
        assert_eq!(a.item_number(0), "1");
        assert_eq!(a.item_number(1), "0");
        assert_eq!(a.item_number(2), "18446744073709551615");
    }

    #[test]
    fn first_free_number_skips_every_pad_outside_the_array() {
        let g = ArrayGeometry::Grid { nx: 4, ny: 1, dx: um(1.0), dy: um(1.0), offset_x: Nm::ZERO, offset_y: Nm::ZERO, centred: false, stagger: 0, stagger_rows: true, horizontal_then_vertical: true, reverse_alternate: false };
        let axes = PadNumbering::default().resolve(false).unwrap();
        let reserved: BTreeSet<String> = ["1", "3"].iter().map(|s| s.to_string()).collect();
        let mut p = PadNumberProvider::new(reserved.clone(), &g, &axes);
        assert_eq!((0..4).map(|_| p.next_number()).collect::<Vec<_>>(), ["2", "4", "5", "6"]);
        // with a start value the pads outside are not looked at
        let specified = PadNumbering { start_specified: true, ..PadNumbering::default() }.resolve(false).unwrap();
        let mut p = PadNumberProvider::new(reserved, &g, &specified);
        assert_eq!((0..4).map(|_| p.next_number()).collect::<Vec<_>>(), ["1", "2", "3", "4"]);
    }

    #[test]
    fn a_grid_numbers_by_coordinates_in_the_order_it_is_walked() {
        let g = ArrayGeometry::Grid { nx: 3, ny: 2, dx: um(1.0), dy: um(1.0), offset_x: Nm::ZERO, offset_y: Nm::ZERO, centred: false, stagger: 0, stagger_rows: true, horizontal_then_vertical: true, reverse_alternate: false };
        let numbering = PadNumbering {
            start_specified: true,
            two_d: true,
            primary: AxisSpec { kind: NumberingType::AlphaFull, start: "A".into(), step: 1 },
            secondary: AxisSpec { kind: NumberingType::Numeric, start: "1".into(), step: 1 },
        };
        let axes = numbering.resolve(false).unwrap();
        let names: Vec<String> = (0..6).map(|n| g.item_number(n, &axes)).collect();
        assert_eq!(names, ["A1", "B1", "C1", "A2", "B2", "C2"]);
        // one number per point, in one run, when the scheme is continuous
        let linear = PadNumbering { start_specified: true, two_d: false, primary: AxisSpec { kind: NumberingType::Numeric, start: "10".into(), step: 1 }, ..PadNumbering::default() }.resolve(false).unwrap();
        assert_eq!((0..6).map(|n| g.item_number(n, &linear)).collect::<Vec<_>>(), ["10", "11", "12", "13", "14", "15"]);
    }

    #[test]
    fn the_serpentine_and_the_walking_direction_decide_which_point_is_which() {
        let mut g = ArrayGeometry::Grid { nx: 3, ny: 2, dx: um(1.0), dy: um(1.0), offset_x: Nm::ZERO, offset_y: Nm::ZERO, centred: false, stagger: 0, stagger_rows: true, horizontal_then_vertical: true, reverse_alternate: true };
        let numbering = PadNumbering { start_specified: true, two_d: true, primary: AxisSpec { kind: NumberingType::AlphaFull, start: "A".into(), step: 1 }, secondary: AxisSpec::default() };
        let axes = numbering.resolve(false).unwrap();
        // the second row runs back: point 3 is the third column
        assert_eq!((0..6).map(|n| g.item_number(n, &axes)).collect::<Vec<_>>(), ["A1", "B1", "C1", "C2", "B2", "A2"]);
        if let ArrayGeometry::Grid { horizontal_then_vertical, reverse_alternate, .. } = &mut g {
            *horizontal_then_vertical = false;
            *reverse_alternate = false;
        }
        // vertical first: `getGridCoords` walks an axis of ny, so the "x" of a point is the row it is in
        assert_eq!((0..6).map(|n| g.item_number(n, &axes)).collect::<Vec<_>>(), ["A1", "B1", "A2", "B2", "A3", "B3"]);
    }

    #[test]
    fn the_entries_are_judged_like_the_dialog_does() {
        let bad = PadNumbering { start_specified: true, two_d: true, primary: AxisSpec { kind: NumberingType::AlphaNoIosqxz, start: "I".into(), step: 1 }, secondary: AxisSpec { kind: NumberingType::Hex, start: "Q".into(), step: 1 } };
        let e = bad.resolve(false).unwrap_err();
        assert_eq!(
            e,
            "Could not determine numbering start from 'I': expected value consistent with alphabet 'ABCDEFGHJKLMNPRTUVWY'.\nCould not determine numbering start from 'Q': expected value consistent with alphabet '0123456789ABCDEF'."
        );
        // a circular array has the one axis: the secondary is not looked at
        let circular = PadNumbering { start_specified: true, two_d: true, primary: AxisSpec { kind: NumberingType::Numeric, start: "1".into(), step: 1 }, secondary: AxisSpec { kind: NumberingType::Hex, start: "Q".into(), step: 1 } };
        assert!(circular.resolve(true).is_ok());
        // "Use first free number" ignores the entries altogether
        let free = PadNumbering { start_specified: false, ..bad };
        assert!(free.resolve(false).is_ok());
    }
}
