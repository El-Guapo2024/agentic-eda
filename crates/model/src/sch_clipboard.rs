//! The schematic clipboard, as data: what a copy holds, and how a paste numbers the symbols it brings.
//!
//! KiCad puts a *fragment* on the clipboard -- `(lib_symbols ...)` and then the selected items, flat, with no `(kicad_sch ...)` around
//! them (`SCH_EDITOR_CONTROL::doCopy` -> `SCH_IO_KICAD_SEXPR::Format( SCH_SELECTION* ... )`) -- and reads it back with
//! `SCH_IO_KICAD_SEXPR_PARSER::ParseSchematic( aIsCopyableOnly )` (`SCH_EDITOR_CONTROL::Paste`). `eda-kicad` has that writer and reader
//! (`sch_clipboard.rs` there); this module is the part both it and the `PasteSch` verb of `eda-ops` share, and which needs neither:
//!
//! * [`SchFragment`]: the items of a fragment as IR types, in KiCad's own frame -- a symbol's `at` is its native origin, the way a
//!   `.kicad_sch` has it ([`crate::ir::SchematicSection::imported_from_kicad`] is set), and the library symbols it names.
//! * [`PasteMode`] and [`annotate_paste`]: the reference designators of the pasted symbols (`SCH_EDITOR_CONTROL::Paste` with
//!   `DIALOG_PASTE_SPECIAL`'s three options, `SCH_REFERENCE_LIST::ReannotateDuplicates`, `FindFirstUnusedReference`,
//!   `REFDES_TRACKER::areUnitsAvailable`).
//! * The anchors a paste is carried by ([`top_left_anchor`], [`closest_anchor`]) and the small geometry the conversions between KiCad's frame
//!   and a derived sheet's need.

use crate::ir::{LibrarySymbol, Point, SchematicSection, Um};
use crate::sch_extras::SchGraphicKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// `PASTE_MODE` (`dialog_paste_special.h`): what a paste does to the reference designators of the symbols it brings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteMode {
    /// "Assign unique reference designators to pasted symbols" -- the default (`annotation.automatic` is on): a reference nothing else in
    /// the design has is kept, any other takes the next free number.
    #[default]
    Unique,
    /// "Keep existing reference designators, even if they are duplicated". A design here names a part by its reference, so two symbols cannot
    /// share one: a reference that is already taken is numbered anew, exactly as in [`PasteMode::Unique`].
    Keep,
    /// "Clear reference designators on all pasted symbols". KiCad leaves them `R?`; here every pasted symbol takes the first free number at
    /// once (a design cannot hold two `R?`), counting up from 1.
    Remove,
}

/// What one copy holds, read back: the items of the fragment and the library symbols they name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchFragment {
    /// The items, in KiCad's frame: a symbol's `at` is its native origin (a `.kicad_sch`'s own convention), so `imported_from_kicad` is set.
    /// `symbols[i].id` is the reference the clipboard carried (`R12`, or `R?`); `user_fields` holds the extra properties of those symbols.
    pub section: SchematicSection,
    /// Every library symbol the fragment embeds (`(lib_symbols ...)`), by the `lib_id` the symbols name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lib_symbols: Vec<LibrarySymbol>,
    /// What the reader could not carry across, one line each ("2 symbols had no library definition ... and were left out").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl SchFragment {
    /// True when the fragment has nothing to paste.
    pub fn is_empty(&self) -> bool {
        let s = &self.section;
        s.symbols.is_empty()
            && s.power_symbols.is_empty()
            && s.wires.is_empty()
            && s.labels.is_empty()
            && s.texts.is_empty()
            && s.no_connects.is_empty()
            && s.bus_entries.is_empty()
            && s.junctions.is_empty()
            && s.lines.is_empty()
            && s.extras.graphics.is_empty()
            && s.sheets.is_empty()
    }

    /// The library symbol of the fragment for `lib_id`.
    pub fn lib_symbol(&self, lib_id: &str) -> Option<&LibrarySymbol> {
        self.lib_symbols.iter().find(|s| s.lib_id == lib_id)
    }
}

// ---------------------------------------------------------------- reference designators

/// A reference designator split the way `SCH_REFERENCE::Split` does: the text before the trailing digits, and those digits as a number.
/// `R12` is `("R", Some(12))`; `R?` is `("R", None)`, as is `U1A` (no trailing digits: not annotated).
pub fn split_reference(reference: &str) -> (String, Option<u32>) {
    if let Some(prefix) = reference.strip_suffix('?') {
        return (prefix.to_string(), None);
    }
    // The run of digits and blanks at the end; `Split` needs it to end on a digit and to leave something in front of it.
    let mut start = reference.len();
    for (i, c) in reference.char_indices().rev() {
        if c.is_ascii_digit() || c <= ' ' {
            start = i;
        } else {
            break;
        }
    }
    let tail = &reference[start..];
    if start > 0 && tail.ends_with(|c: char| c.is_ascii_digit()) {
        if let Ok(n) = tail.trim().parse::<u32>() {
            return (reference[..start].to_string(), Some(n));
        }
    }
    (reference.to_string(), None)
}

/// `UTIL::GetRefDesUnannotated`: `R12` -> `R?`.
pub fn unannotated(reference: &str) -> String {
    let (prefix, _) = split_reference(reference);
    format!("{prefix}?")
}

/// True for a reference KiCad always re-annotates on paste (`SCH_REFERENCE::AlwaysAnnotate`): a power symbol's `#PWR..` and every other `#`-prefixed one.
pub fn always_annotated(reference: &str, power: bool) -> bool {
    power || reference.starts_with('#')
}

/// A symbol already in the design (any sheet), as the numbering sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsedRef {
    pub reference: String,
    pub unit: u32,
    pub lib_id: String,
    pub value: String,
}

/// One placed symbol of the fragment, before it has its final reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastedRef {
    /// The reference the clipboard carried.
    pub reference: String,
    pub unit: u32,
    /// How many units the library symbol has (more than one: its pasted units keep one number between them).
    pub unit_count: u32,
    pub lib_id: String,
    pub value: String,
    /// Where it sits (the annotation order: by reference prefix, then left to right, then top to bottom).
    pub at: (Um, Um),
    /// A power symbol (always numbered afresh).
    pub power: bool,
}

#[derive(Debug, Clone)]
struct Numbered {
    prefix: String,
    number: u32,
    unit: u32,
    lib_id: String,
    value: String,
}

fn same_prefix(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// `REFDES_TRACKER::areUnitsAvailable`: every unit in `required` is free on a number that other symbols hold -- none of them is another
/// library symbol or value, and none already has that unit.
fn units_available(lib_id: &str, value: &str, holders: &[&Numbered], required: &[u32]) -> bool {
    required.iter().all(|unit| holders.iter().all(|h| h.lib_id == lib_id && h.value == value && h.unit != *unit))
}

/// `SCH_REFERENCE_LIST::FindFirstUnusedReference` + `REFDES_TRACKER::GetNextRefDesForUnits`: the first number from `min` that no symbol of the
/// same prefix holds -- or that holds only symbols with the same library symbol and value, none of them with a unit in `required_units`.
fn first_unused(prefix: &str, lib_id: &str, value: &str, min: u32, required_units: &[u32], taken: &[Numbered]) -> u32 {
    let mut candidate = min;
    loop {
        let holders: Vec<&Numbered> = taken.iter().filter(|t| same_prefix(&t.prefix, prefix) && t.number == candidate).collect();
        if holders.is_empty() || (!required_units.is_empty() && units_available(lib_id, value, &holders, required_units)) {
            return candidate;
        }
        candidate += 1;
    }
}

/// The reference text of `prefix` + `number` (`SCH_REFERENCE::formatRefStr`: a power symbol's number carries a leading zero, `#PWR01`).
fn format_reference(prefix: &str, number: u32, power: bool) -> String {
    if power {
        format!("{prefix}0{number}")
    } else {
        format!("{prefix}{number}")
    }
}

/// The reference every pasted symbol ends up with, in the order of `pasted` (`SCH_EDITOR_CONTROL::Paste`).
///
/// `used` is every symbol of the design, on every sheet (`hierarchy.GetSymbols( existingRefs, SYMBOL_FILTER_ALL )`), power symbols included.
///
/// * [`PasteMode::Unique`]: all pasted symbols go through `ReannotateDuplicates` -- they are numbered by prefix and then left to right, and each
///   takes the first number from its own that nothing else holds (its own, when it is free). The units of a multi-unit symbol that were pasted
///   together share one number, and a unit may join a number already used by the same part when that unit is still free there.
/// * [`PasteMode::Keep`]: a reference not yet in the design stays; one already taken is numbered as in `Unique`. A power symbol is always
///   numbered afresh (`AlwaysAnnotate`), from 1.
/// * [`PasteMode::Remove`]: every pasted symbol is numbered from 1.
pub fn annotate_paste(used: &[UsedRef], pasted: &[PastedRef], mode: PasteMode) -> Vec<String> {
    let mut taken: Vec<Numbered> = used
        .iter()
        .filter_map(|u| {
            let (prefix, number) = split_reference(&u.reference);
            Some(Numbered { prefix, number: number?, unit: u.unit, lib_id: u.lib_id.clone(), value: u.value.clone() })
        })
        .collect();
    let mut taken_exact: BTreeSet<(String, u32)> = used.iter().map(|u| (u.reference.clone(), u.unit)).collect();

    let mut out: Vec<Option<String>> = vec![None; pasted.len()];

    // `Keep`: a pasted symbol keeps its reference when that is a real number nothing else in the design (or earlier in the paste) has.
    if mode == PasteMode::Keep {
        for (i, p) in pasted.iter().enumerate() {
            let (prefix, number) = split_reference(&p.reference);
            let Some(number) = number else { continue };
            if always_annotated(&p.reference, p.power) || taken_exact.contains(&(p.reference.clone(), p.unit)) {
                continue;
            }
            taken_exact.insert((p.reference.clone(), p.unit));
            taken.push(Numbered { prefix, number, unit: p.unit, lib_id: p.lib_id.clone(), value: p.value.clone() });
            out[i] = Some(p.reference.clone());
        }
    }

    // The order the numbers are handed out in (`SortByXCoordinate`: reference prefix, then x, then y).
    let mut order: Vec<usize> = (0..pasted.len()).filter(|&i| out[i].is_none()).collect();
    order.sort_by(|&a, &b| {
        let (pa, pb) = (split_reference(&pasted[a].reference).0.to_ascii_lowercase(), split_reference(&pasted[b].reference).0.to_ascii_lowercase());
        (pa, pasted[a].at.0, pasted[a].at.1, a).cmp(&(pb, pasted[b].at.0, pasted[b].at.1, b))
    });

    // `lockedSymbols`: the pasted units of one multi-unit reference are numbered together. `Remove` clears the references first, so
    // units of different symbols can no longer be told apart by their old one -- but they still were pasted as one reference.
    let group_of = |i: usize| -> Vec<usize> {
        let p = &pasted[i];
        if p.unit_count <= 1 || p.reference.ends_with('?') {
            return vec![i];
        }
        (0..pasted.len()).filter(|&j| pasted[j].reference == p.reference && pasted[j].unit_count > 1 && pasted[j].lib_id == p.lib_id && pasted[j].value == p.value).collect()
    };

    let mut min_ref = 1u32;
    let mut current_prefix: Option<String> = None;
    for &i in &order {
        if out[i].is_some() {
            continue;
        }
        let p = &pasted[i];
        let (prefix, number) = split_reference(&p.reference);
        if current_prefix.as_ref().is_none_or(|c| !same_prefix(c, &prefix)) {
            current_prefix = Some(prefix.clone());
            min_ref = 1;
        }
        // `aStartAtCurrent`: a pasted number that is free stays; `Unique` starts from it. A power symbol is numbered from 1 unless it is
        // the plain duplicate pass. `Remove` and a symbol that lost its number count up from the running minimum.
        let from_current = mode == PasteMode::Unique || (mode == PasteMode::Keep && !always_annotated(&p.reference, p.power));
        if from_current {
            if let Some(n) = number.filter(|n| *n > 0) {
                min_ref = n;
            }
        } else {
            min_ref = 1;
        }
        let group = group_of(i);
        let units: Vec<u32> = if p.unit_count > 1 {
            let mut u: Vec<u32> = group.iter().map(|&j| pasted[j].unit).collect();
            u.sort_unstable();
            u.dedup();
            u
        } else {
            Vec::new()
        };
        let n = first_unused(&prefix, &p.lib_id, &p.value, min_ref, &units, &taken);
        for &j in &group {
            if out[j].is_some() {
                continue;
            }
            out[j] = Some(format_reference(&prefix, n, pasted[j].power));
            taken.push(Numbered { prefix: prefix.clone(), number: n, unit: pasted[j].unit, lib_id: pasted[j].lib_id.clone(), value: pasted[j].value.clone() });
        }
    }
    out.into_iter().zip(pasted).map(|(o, p)| o.unwrap_or_else(|| p.reference.clone())).collect()
}

// ---------------------------------------------------------------- geometry

/// Where a point of a library symbol (mm, KiCad's library frame: +y up) lands relative to the symbol's origin once the symbol is placed with
/// `angle_deg` (the `(at x y angle)` of the file), `mirrored` (`(mirror y)`: x negated) and `mirror_y` (`(mirror x)`: y negated) -- the sheet
/// frame, mm, +y down. `sch_symbol.cpp`'s `SetOrientation` composition, the same one `eda_kicad::transform_local_point` has.
pub fn place_offset_mm(angle_deg: f64, mirrored: bool, mirror_y: bool, local: (f64, f64)) -> (f64, f64) {
    let snap = |v: f64| if (v - v.round()).abs() < 1e-12 { v.round() } else { v };
    let (x, y) = (local.0, -local.1);
    let theta = (-angle_deg).to_radians();
    let (c, s) = (snap(theta.cos()), snap(theta.sin()));
    let (mut rx, mut ry) = (x * c - y * s, x * s + y * c);
    if mirrored {
        rx = -rx;
    }
    if mirror_y {
        ry = -ry;
    }
    (rx, ry)
}

/// The same, in micrometres.
pub fn place_offset_um(angle_deg: f64, mirrored: bool, mirror_y: bool, local_mm: (f64, f64)) -> (Um, Um) {
    let (x, y) = place_offset_mm(angle_deg, mirrored, mirror_y, local_mm);
    ((x * 1000.0).round() as Um, (y * 1000.0).round() as Um)
}

fn shift(p: &mut Point, dx: Um, dy: Um) {
    p.x += dx;
    p.y += dy;
}

/// Move one drawn graphic by `(dx, dy)`.
pub fn translate_graphic(shape: &mut SchGraphicKind, dx: Um, dy: Um) {
    match shape {
        SchGraphicKind::Rectangle { start, end, .. } | SchGraphicKind::TextBox { start, end, .. } => {
            shift(start, dx, dy);
            shift(end, dx, dy);
        }
        SchGraphicKind::Circle { center, .. } => shift(center, dx, dy),
        SchGraphicKind::Arc { start, mid, end } => {
            shift(start, dx, dy);
            shift(mid, dx, dy);
            shift(end, dx, dy);
        }
        SchGraphicKind::Bezier { start, c1, c2, end } => {
            shift(start, dx, dy);
            shift(c1, dx, dy);
            shift(c2, dx, dy);
            shift(end, dx, dy);
        }
        SchGraphicKind::Polygon { pts } | SchGraphicKind::RuleArea { pts, .. } => pts.iter_mut().for_each(|p| shift(p, dx, dy)),
        SchGraphicKind::Directive { at, .. } => shift(at, dx, dy),
    }
}

/// Move every item of `section` by `(dx, dy)`.
pub fn translate_section(section: &mut SchematicSection, dx: Um, dy: Um) {
    for s in &mut section.symbols {
        shift(&mut s.at, dx, dy);
    }
    for p in &mut section.power_symbols {
        shift(&mut p.at, dx, dy);
    }
    for w in &mut section.wires {
        w.pts.iter_mut().for_each(|p| shift(p, dx, dy));
    }
    for l in &mut section.labels {
        shift(&mut l.at, dx, dy);
    }
    for t in &mut section.texts {
        shift(&mut t.at, dx, dy);
    }
    for n in &mut section.no_connects {
        shift(&mut n.at, dx, dy);
    }
    for b in &mut section.bus_entries {
        shift(&mut b.at, dx, dy);
    }
    for j in &mut section.junctions {
        shift(&mut j.at, dx, dy);
    }
    for l in &mut section.lines {
        l.pts.iter_mut().for_each(|p| shift(p, dx, dy));
    }
    for g in &mut section.extras.graphics {
        translate_graphic(&mut g.shape, dx, dy);
    }
    for s in &mut section.sheets {
        shift(&mut s.at, dx, dy);
        for pin in &mut s.pins {
            shift(&mut pin.at, dx, dy);
        }
    }
}

/// `SCH_ITEM::GetPosition` of every item of `section`, with whether `IsConnectable()` holds for it, in the order KiCad walks a selection (the
/// symbols first, then the rest; the order only matters between items at the very same point).
fn positions(section: &SchematicSection) -> Vec<(Point, bool)> {
    let mut out: Vec<(Point, bool)> = Vec::new();
    out.extend(section.symbols.iter().map(|s| (s.at, true)));
    out.extend(section.power_symbols.iter().map(|s| (s.at, true)));
    out.extend(section.wires.iter().filter_map(|w| w.pts.first().map(|p| (*p, true))));
    out.extend(section.junctions.iter().map(|j| (j.at, true)));
    out.extend(section.no_connects.iter().map(|n| (n.at, true)));
    out.extend(section.bus_entries.iter().map(|b| (b.at, true)));
    out.extend(section.labels.iter().map(|l| (l.at, true)));
    out.extend(section.sheets.iter().map(|s| (s.at, true)));
    out.extend(section.texts.iter().map(|t| (t.at, false)));
    out.extend(section.lines.iter().filter_map(|l| l.pts.first().map(|p| (*p, false))));
    for g in &section.extras.graphics {
        let (p, connectable) = match &g.shape {
            SchGraphicKind::Rectangle { start, .. } | SchGraphicKind::TextBox { start, .. } | SchGraphicKind::Arc { start, .. } | SchGraphicKind::Bezier { start, .. } => (*start, false),
            SchGraphicKind::Circle { center, .. } => (*center, false),
            SchGraphicKind::Polygon { pts } | SchGraphicKind::RuleArea { pts, .. } => match pts.first() {
                Some(p) => (*p, false),
                None => continue,
            },
            SchGraphicKind::Directive { at, .. } => (*at, true),
        };
        out.push((p, connectable));
    }
    out
}

/// The point a paste is carried by (`SCH_SELECTION::GetTopLeftItem` -> `GetPosition`): the leftmost, then the topmost, of the connectable
/// items -- wires, symbols, labels, junctions... -- else of all of them.
pub fn top_left_anchor(section: &SchematicSection) -> Option<Point> {
    let all = positions(section);
    let pick = |filter: &dyn Fn(bool) -> bool| all.iter().filter(|(_, c)| filter(*c)).map(|(p, _)| *p).fold(None, |best: Option<Point>, p| match best {
        Some(b) if (b.x, b.y) <= (p.x, p.y) => Some(b),
        _ => Some(p),
    });
    pick(&|c| c).or_else(|| pick(&|_| true))
}

/// The point Duplicate carries the copy by: the connection point nearest the cursor (the pins and ends of the connectable items, and a symbol's
/// origin), else, with none, the nearest of the items' own points -- `SCH_EDITOR_CONTROL::Paste`'s `aEvent.IsAction( &ACTIONS::duplicate )` block.
/// `connection_points` are the extra points the caller knows (a symbol's pins in the fragment's frame).
pub fn closest_anchor(section: &SchematicSection, connection_points: &[Point], cursor: Point) -> Option<Point> {
    let dist = |p: &Point| (((p.x - cursor.x) as f64).powi(2) + ((p.y - cursor.y) as f64).powi(2)).sqrt();
    let best = |pts: Vec<Point>| pts.into_iter().min_by(|a, b| dist(a).partial_cmp(&dist(b)).unwrap_or(std::cmp::Ordering::Equal));
    let mut connectable: Vec<Point> = connection_points.to_vec();
    connectable.extend(section.symbols.iter().map(|s| s.at));
    connectable.extend(section.power_symbols.iter().map(|s| s.at));
    for w in &section.wires {
        connectable.extend(w.pts.iter().copied());
    }
    connectable.extend(section.junctions.iter().map(|j| j.at));
    connectable.extend(section.no_connects.iter().map(|n| n.at));
    connectable.extend(section.bus_entries.iter().flat_map(|b| [b.at, Point { x: b.at.x + b.size.x, y: b.at.y + b.size.y }]));
    connectable.extend(section.labels.iter().map(|l| l.at));
    connectable.extend(section.sheets.iter().map(|s| s.at));
    if let Some(p) = best(connectable) {
        return Some(p);
    }
    let mut others: Vec<Point> = section.texts.iter().map(|t| t.at).collect();
    for l in &section.lines {
        others.extend(l.pts.iter().copied());
    }
    for g in &section.extras.graphics {
        match &g.shape {
            SchGraphicKind::Rectangle { start, end, .. } | SchGraphicKind::TextBox { start, end, .. } => others.extend([*start, *end, Point { x: start.x, y: end.y }, Point { x: end.x, y: start.y }]),
            SchGraphicKind::Circle { center, .. } => others.push(*center),
            SchGraphicKind::Arc { start, end, .. } => others.extend([*start, *end]),
            SchGraphicKind::Bezier { start, end, .. } => others.extend([*start, *end]),
            SchGraphicKind::Polygon { pts } | SchGraphicKind::RuleArea { pts, .. } => others.extend(pts.iter().copied()),
            SchGraphicKind::Directive { at, .. } => others.push(*at),
        }
    }
    best(others)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn used(reference: &str, unit: u32, lib: &str, value: &str) -> UsedRef {
        UsedRef { reference: reference.into(), unit, lib_id: lib.into(), value: value.into() }
    }
    fn pasted(reference: &str, x: Um) -> PastedRef {
        PastedRef { reference: reference.into(), unit: 1, unit_count: 1, lib_id: "Device:R".into(), value: "10k".into(), at: (x, 0), power: false }
    }

    #[test]
    fn references_split_at_their_trailing_digits() {
        assert_eq!(split_reference("R12"), ("R".to_string(), Some(12)));
        assert_eq!(split_reference("R?"), ("R".to_string(), None));
        assert_eq!(split_reference("#PWR012"), ("#PWR".to_string(), Some(12)));
        assert_eq!(split_reference("U1A"), ("U1A".to_string(), None));
        assert_eq!(split_reference("TP"), ("TP".to_string(), None));
        assert_eq!(unannotated("C7"), "C?");
        assert!(always_annotated("#PWR01", false) && always_annotated("X1", true) && !always_annotated("R1", false));
    }

    #[test]
    fn a_free_reference_stays_and_a_taken_one_takes_the_next_free_number_from_its_own() {
        let used = vec![used("R1", 1, "Device:R", "10k"), used("R2", 1, "Device:R", "10k"), used("R4", 1, "Device:R", "10k")];
        // R9 is free: it stays. R2 is taken: R3 is the first free number from 2. R4 is taken: R5.
        let out = annotate_paste(&used, &[pasted("R9", 0), pasted("R2", 1), pasted("R4", 2)], PasteMode::Unique);
        assert_eq!(out, vec!["R9", "R3", "R5"]);
    }

    #[test]
    fn pasted_symbols_never_share_a_number_with_each_other() {
        let used = vec![used("C1", 1, "Device:C", "1u")];
        let two = vec![
            PastedRef { reference: "C1".into(), lib_id: "Device:C".into(), value: "1u".into(), ..pasted("C1", 0) },
            PastedRef { reference: "C1".into(), lib_id: "Device:C".into(), value: "1u".into(), ..pasted("C1", 5) },
        ];
        // The same reference pasted twice (two units of nothing): each is its own symbol, so each takes its own number.
        let out = annotate_paste(&used, &two, PasteMode::Unique);
        let mut sorted = out.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), 2, "{out:?}");
        assert!(!out.contains(&"C1".to_string()));
    }

    #[test]
    fn the_numbers_go_out_left_to_right_within_a_prefix() {
        let used = vec![used("R1", 1, "Device:R", "10k")];
        // Both are R1 (taken). The one on the left is numbered first.
        let out = annotate_paste(&used, &[pasted("R1", 900), pasted("R1", 100)], PasteMode::Unique);
        assert_eq!(out, vec!["R3", "R2"]);
    }

    #[test]
    fn keep_keeps_a_free_reference_and_renumbers_only_a_taken_one() {
        let used = vec![used("R1", 1, "Device:R", "10k")];
        let out = annotate_paste(&used, &[pasted("R7", 0), pasted("R1", 1)], PasteMode::Keep);
        assert_eq!(out, vec!["R7", "R2"]);
    }

    #[test]
    fn remove_numbers_every_pasted_symbol_from_one() {
        let used = vec![used("R1", 1, "Device:R", "10k"), used("R3", 1, "Device:R", "10k")];
        let out = annotate_paste(&used, &[pasted("R7", 0), pasted("R8", 1), pasted("R9", 2)], PasteMode::Remove);
        assert_eq!(out, vec!["R2", "R4", "R5"]);
    }

    #[test]
    fn power_symbols_are_numbered_afresh_with_a_leading_zero() {
        let used = vec![used("#PWR01", 1, "power:GND", "GND"), used("#PWR02", 1, "power:GND", "GND")];
        let p = |r: &str, x| PastedRef { reference: r.into(), lib_id: "power:GND".into(), value: "GND".into(), power: true, ..pasted(r, x) };
        assert_eq!(annotate_paste(&used, &[p("#PWR01", 0)], PasteMode::Unique), vec!["#PWR03"]);
        assert_eq!(annotate_paste(&used, &[p("#PWR05", 0)], PasteMode::Keep), vec!["#PWR03"], "a power symbol is numbered anew even when Keep");
        assert_eq!(annotate_paste(&[], &[p("#PWR05", 0)], PasteMode::Unique), vec!["#PWR05"]);
    }

    #[test]
    fn the_units_of_one_part_pasted_together_keep_one_number() {
        let unit = |u: u32, x: Um| PastedRef { reference: "U1".into(), unit: u, unit_count: 4, lib_id: "Amp:Quad".into(), value: "LM324".into(), at: (x, 0), power: false };
        let used = vec![used("U1", 1, "Amp:Quad", "LM324"), used("U1", 2, "Amp:Quad", "LM324")];
        // Units 1 and 2 of U1 are taken: pasting them again numbers both as U2.
        let out = annotate_paste(&used, &[unit(1, 0), unit(2, 10)], PasteMode::Unique);
        assert_eq!(out, vec!["U2", "U2"]);
        // A unit the part does not have yet joins it.
        let out = annotate_paste(&used, &[unit(3, 0)], PasteMode::Unique);
        assert_eq!(out, vec!["U1"]);
        // ... unless the part there is another one.
        let other = vec![UsedRef { value: "TL074".into(), ..used[0].clone() }];
        assert_eq!(annotate_paste(&other, &[unit(3, 0)], PasteMode::Unique), vec!["U2"]);
    }

    #[test]
    fn an_unannotated_reference_gets_a_number_in_every_mode() {
        let used = vec![used("R1", 1, "Device:R", "10k")];
        for mode in [PasteMode::Unique, PasteMode::Keep, PasteMode::Remove] {
            assert_eq!(annotate_paste(&used, &[pasted("R?", 0)], mode), vec!["R2"], "{mode:?}");
        }
    }

    #[test]
    fn orientation_matches_kicads_matrices() {
        // A point 2 mm up in the library frame of an unturned symbol is 2 mm up (y down: -2) on the sheet.
        assert_eq!(place_offset_mm(0.0, false, false, (0.0, 2.0)), (0.0, -2.0));
        // 90 degrees (the file's angle): counter-clockwise on screen, so "up" becomes "left".
        assert_eq!(place_offset_mm(90.0, false, false, (0.0, 2.0)), (-2.0, 0.0));
        assert_eq!(place_offset_mm(180.0, false, false, (1.0, 2.0)), (-1.0, 2.0));
        // `(mirror y)` negates x, `(mirror x)` negates y -- after the turn.
        assert_eq!(place_offset_mm(0.0, true, false, (1.0, 2.0)), (-1.0, -2.0));
        assert_eq!(place_offset_mm(0.0, false, true, (1.0, 2.0)), (1.0, 2.0));
        assert_eq!(place_offset_um(0.0, false, false, (1.27, 0.0)), (1270, 0));
    }

    #[test]
    fn the_anchor_is_the_leftmost_then_topmost_connectable_item() {
        use crate::ir::{NetLabel, SchematicText, Wire};
        let mut s = SchematicSection::default();
        s.texts.push(SchematicText { id: String::new(), content: "far left".into(), at: Point { x: -9_000, y: 0 }, angle: 0, size_um: 1270 });
        s.wires.push(Wire { id: String::new(), net: String::new(), pins: vec![], pts: vec![Point { x: 5_000, y: 7_000 }, Point { x: 9_000, y: 7_000 }], bus: false });
        s.labels.push(NetLabel { id: String::new(), net: "A".into(), at: Point { x: 5_000, y: 2_000 }, kind: Default::default() });
        // The text is further left but not connectable: the label is the topmost of the two at x = 5000.
        assert_eq!(top_left_anchor(&s), Some(Point { x: 5_000, y: 2_000 }));
        let only_text = SchematicSection { texts: s.texts.clone(), ..Default::default() };
        assert_eq!(top_left_anchor(&only_text), Some(Point { x: -9_000, y: 0 }));
        assert_eq!(top_left_anchor(&SchematicSection::default()), None);
        // The nearest connection point to a cursor: the label, not the far wire end.
        assert_eq!(closest_anchor(&s, &[], Point { x: 5_100, y: 2_100 }), Some(Point { x: 5_000, y: 2_000 }));
    }
}
