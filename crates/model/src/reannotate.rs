//! Geographical Reannotate (`pcbnew.ReannotateTool.ShowReannotateDialog`): the footprints of the board renumbered by where they are.
//!
//! A port of `DIALOG_BOARD_REANNOTATE` (`pcbnew/dialogs/dialog_board_reannotate.cpp` at KiCad 8303b2ad): `BuildFootprintList` (each footprint's
//! location rounded to a grid, what to do with it -- update, leave it alone, excluded --, the front and back lists sorted in one of eight directions),
//! `BuildUnavailableRefsList` (the numbers of the footprints that keep their reference are not given out again), `BuildChangeArray` (prefix added
//! or removed, the start numbers, the numbering by prefix) and the duplicate check. The plan is data: the dialog's message window is composed from it
//! by the studio (`kicad-port/reannotate.ts`), and `Cmd::ReannotateBoard` (crates/ops) applies it.
//!
//! What stays as KiCad has it, so a board reannotates the same here:
//!  * the scope's own rule (selection, front, back) has the last word on a footprint, after the exclusion list and a locked footprint have been looked at:
//!    a footprint of a prefix in the exclusion list is updated anyway when it is in the scope of "Front", "Back" or "Selection" (the list works with
//!    "All");
//!  * a reference without a digit ("TP") is "unannotated; not updated" only with the scope "All"; in another scope it is numbered as its prefix
//!    ("TP1");
//!  * a start number (not zero) restarts the numbering of every prefix the plan knows; with the back start blank (zero) the back footprints' numbering
//!    continues each prefix of the front's, and a prefix the front has not got starts at 1.
//!
//! Where it goes further: the references of the design's parts that are not on the board (a symbol whose footprint is not placed yet) are taken like the
//! excluded footprints' -- their numbers are not given out -- because in the studio a part is its reference, and two parts cannot have one.
//!
//! Positions are micrometres; the sorting works in nanometres as KiCad's does (`RoundToGrid`, the grids of the mil lists are whole nanometres).

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeSet;

/// `MAXERROR`: how many duplicate references are told before the check gives up.
pub const MAX_ERRORS: usize = 10;
/// `VALIDPREFIX`: besides letters and digits, what a prefix may hold.
pub const VALID_PREFIX: &str = "_-+=/\\";

const SORT_Y_FIRST: u8 = 0b100;
const DESCENDING_FIRST: u8 = 0b010;
const DESCENDING_SECOND: u8 = 0b001;

/// `FrontDirectionsArray`: the eight radio buttons ("Top to bottom, left to right", ...), as the sort code each one means.
const FRONT_DIRECTIONS: [u8; 8] = [
    SORT_Y_FIRST,                                    // top to bottom, left to right
    SORT_Y_FIRST | DESCENDING_SECOND,                // top to bottom, right to left
    SORT_Y_FIRST | DESCENDING_FIRST,                 // bottom to top, left to right
    SORT_Y_FIRST | DESCENDING_FIRST | DESCENDING_SECOND, // bottom to top, right to left
    0,                                               // left to right, top to bottom
    DESCENDING_SECOND,                               // left to right, bottom to top
    DESCENDING_FIRST,                                // right to left, top to bottom
    DESCENDING_FIRST | DESCENDING_SECOND,            // right to left, bottom to top
];

/// `BackDirectionsArray`: the same buttons for the back, which is a mirror image (left and right swapped, "coordinates are from the top").
const BACK_DIRECTIONS: [u8; 8] = [
    SORT_Y_FIRST | DESCENDING_SECOND,
    SORT_Y_FIRST,
    SORT_Y_FIRST | DESCENDING_FIRST | DESCENDING_SECOND,
    SORT_Y_FIRST | DESCENDING_FIRST,
    DESCENDING_FIRST,
    DESCENDING_FIRST | DESCENDING_SECOND,
    0,
    DESCENDING_SECOND,
];

/// "Based on location of:" -- which position a footprint is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Location {
    /// The footprint's own position.
    #[default]
    Footprint,
    /// Where its Reference text is.
    Reference,
}

/// "Reannotation Scope".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    All,
    Front,
    Back,
    Selection,
}

/// The dialog's controls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReannotateOptions {
    /// "Footprint Order": which of the eight radio buttons, in the dialog's order (0 is "top to bottom, left to right", the first the dialog opens with).
    #[serde(default)]
    pub direction: u8,
    #[serde(default)]
    pub location: Location,
    /// "Round locations to": the grid, micrometres. Footprints a little out of line sort as if on the line.
    #[serde(default)]
    pub grid_um: i64,
    #[serde(default)]
    pub scope: Scope,
    /// "Exclude locked footprints".
    #[serde(default)]
    pub exclude_locked: bool,
    /// "Exclude references": prefixes (`R` is every `R1`, `R2` ...) or, ending in `*`, patterns for the whole reference (`R1*`); separated by commas or spaces.
    #[serde(default)]
    pub exclude: String,
    /// "Front reference start": 1 by default; 0 (blank) continues each prefix from where it is.
    #[serde(default = "one")]
    pub front_start: u32,
    /// "Back reference start": blank or 0 continues from the front.
    #[serde(default)]
    pub back_start: u32,
    #[serde(default)]
    pub front_prefix: String,
    #[serde(default)]
    pub back_prefix: String,
    #[serde(default)]
    pub remove_front_prefix: bool,
    #[serde(default)]
    pub remove_back_prefix: bool,
}

fn one() -> u32 {
    1
}

impl Default for ReannotateOptions {
    fn default() -> Self {
        ReannotateOptions {
            direction: 0,
            location: Location::default(),
            grid_um: 0,
            scope: Scope::default(),
            exclude_locked: false,
            exclude: String::new(),
            front_start: 1,
            back_start: 0,
            front_prefix: String::new(),
            back_prefix: String::new(),
            remove_front_prefix: false,
            remove_back_prefix: false,
        }
    }
}

/// `FilterPrefix`: a prefix keeps its letters and digits and what `VALIDPREFIX` allows.
pub fn sanitize_prefix(prefix: &str) -> String {
    prefix.chars().filter(|c| c.is_ascii_alphanumeric() || VALID_PREFIX.contains(*c)).collect()
}

/// One footprint of the board, as the dialog reads it.
#[derive(Debug, Clone)]
pub struct ReannotateInput {
    pub reference: String,
    /// The footprint's library id as the board has it; empty when it has none (the report says "unknown").
    pub fpid: String,
    /// The position it is sorted by (`footprint->GetPosition()` or `footprint->Reference().GetPosition()`), micrometres.
    pub x: i64,
    pub y: i64,
    /// `GetLayer() == F_Cu`.
    pub front: bool,
    pub locked: bool,
    pub selected: bool,
}

/// `ACTION_CODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Gets a new reference.
    Update,
    /// "(not updated)": its reference is empty.
    Empty,
    /// "(unannotated; not updated)": its reference has no number.
    Invalid,
    /// "(excluded)".
    Exclude,
}

/// A line of `LogFootprints`: the footprint's place in the order, where it is and where it is taken to be.
#[derive(Debug, Clone, Serialize)]
pub struct Listed {
    /// From 1.
    pub index: usize,
    pub reference: String,
    pub x: i64,
    pub y: i64,
    /// After the rounding to the grid, micrometres (a grid of mils is not whole micrometres).
    pub rounded_x: f64,
    pub rounded_y: f64,
}

/// `REFDES_CHANGE`: what becomes of one footprint's reference.
#[derive(Debug, Clone, Serialize)]
pub struct Change {
    /// The reference the footprint has now.
    pub old: String,
    /// The one it gets (the same when it is not updated).
    pub new: String,
    pub front: bool,
    pub action: Action,
}

/// A footprint whose reference is empty or has no number (`aBadRefDes`).
#[derive(Debug, Clone, Serialize)]
pub struct Bad {
    pub reference: String,
    pub fpid: String,
    pub x: i64,
    pub y: i64,
}

/// What a run of the dialog works out: the lists it logs, the change log and what is wrong.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Plan {
    /// `m_frontFootprints` and `m_backFootprints` in their sorted order; empty when no footprint is on that side.
    pub front: Vec<Listed>,
    pub back: Vec<Listed>,
    /// "Reference Designator Prefixes in Use": the prefixes in the order the numbering met them.
    pub prefixes: Vec<String>,
    /// The tokens of "Exclude references".
    pub excluded: Vec<String>,
    /// `m_changeArray`, in the order of the change log (by the old reference, numbers as numbers).
    pub changes: Vec<Change>,
    pub bad: Vec<Bad>,
    /// "Duplicate instances of R3", "Aborted: too many errors": the options give a reference twice. Empty when the plan can be applied.
    pub errors: Vec<String>,
}

impl Plan {
    /// `BuildFootprintList` returned true: nothing is wrong with the new references.
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// The references that change, `old -> new`.
    pub fn renames(&self) -> std::collections::BTreeMap<String, String> {
        self.changes.iter().filter(|c| c.action == Action::Update && c.old != c.new).map(|c| (c.old.clone(), c.new.clone())).collect()
    }
}

/// `StrNumCmp`: strings compared with the runs of digits as numbers.
pub fn str_num_cmp(a: &str, b: &str, ignore_case: bool) -> Ordering {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let (mut c1, mut c2) = (Some(a[i]), Some(b[j]));
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (mut n1, mut n2) = (0u128, 0u128);
            while i < a.len() && a[i].is_ascii_digit() {
                n1 = n1.saturating_mul(10).saturating_add(a[i] as u128 - '0' as u128);
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                n2 = n2.saturating_mul(10).saturating_add(b[j] as u128 - '0' as u128);
                j += 1;
            }
            match n1.cmp(&n2) {
                Ordering::Equal => {}
                other => return other,
            }
            c1 = None;
            c2 = None;
        }
        // any numerical comparison to here is identical
        if let (Some(x), Some(y)) = (c1, c2) {
            let order = if ignore_case { x.to_lowercase().cmp(y.to_lowercase()) } else { x.cmp(&y) };
            if order != Ordering::Equal {
                return order;
            }
            i += 1;
            j += 1;
        }
    }
    (i < a.len()).cmp(&(j < b.len()))
}

/// `wxString::Matches`: `*` is any run of characters, `?` one; case counts.
pub fn wild_matches(mask: &str, text: &str) -> bool {
    fn go(mask: &[char], text: &[char]) -> bool {
        match mask.split_first() {
            None => text.is_empty(),
            Some(('*', rest)) => (0..=text.len()).any(|skip| go(rest, &text[skip..])),
            Some(('?', rest)) => !text.is_empty() && go(rest, &text[1..]),
            Some((c, rest)) => text.first() == Some(c) && go(rest, &text[1..]),
        }
    }
    go(&mask.chars().collect::<Vec<_>>(), &text.chars().collect::<Vec<_>>())
}

/// `DIALOG_BOARD_REANNOTATE::RoundToGrid` (the remainder takes the sign of the coordinate, as C++'s `%` does).
pub fn round_to_grid(coord: i64, grid: i64) -> i64 {
    // `MINGRID` is 1000 nm
    let grid = if grid == 0 { 1000 } else { grid };
    let rounder = coord % grid;
    let mut coord = coord - rounder;
    if rounder.abs() > grid / 2 {
        coord += if coord < 0 { -grid } else { grid };
    }
    coord
}

/// `UTIL::GetRefDesNumber`: the number from the first digit on, `-1` when what follows the first digit is not a number (`R12A`) or there is no digit.
pub fn ref_des_number(reference: &str) -> i64 {
    match reference.find(|c: char| c.is_ascii_digit()) {
        Some(at) => reference[at..].parse::<i64>().unwrap_or(-1),
        None => -1,
    }
}

/// The part of a reference before its first digit; the whole reference when it has none (`substr( 0, firstnum )` with `npos`).
fn prefix_of(reference: &str) -> &str {
    match reference.find(|c: char| c.is_ascii_digit()) {
        Some(at) => &reference[..at],
        None => reference,
    }
}

/// `REFDES_INFO`.
struct Info {
    reference: String,
    fpid: String,
    x: i64,
    y: i64,
    rounded_x: i64,
    rounded_y: i64,
    front: bool,
    action: Action,
    prefix: String,
}

/// `REFDES_PREFIX_INFO`.
struct PrefixInfo {
    prefix: String,
    last_used: i64,
    unavailable: BTreeSet<i64>,
}

/// `GetOrBuildRefDesInfo`.
fn info_of<'a>(infos: &'a mut Vec<PrefixInfo>, prefix: &str, start: i64) -> &'a mut PrefixInfo {
    if let Some(at) = infos.iter().position(|i| i.prefix == prefix) {
        return &mut infos[at];
    }
    infos.push(PrefixInfo { prefix: prefix.to_string(), last_used: (start - 1).max(0), unavailable: BTreeSet::new() });
    infos.last_mut().expect("just pushed")
}

/// `FootprintCompare`: is `a` before `b` for the sort code.
fn before(a: &Info, b: &Info, code: u8) -> bool {
    let (mut x0, mut x1, mut y0, mut y1) = (a.rounded_x, b.rounded_x, a.rounded_y, b.rounded_y);
    // sorting by Y then X: swap X and Y
    if code & SORT_Y_FIRST != 0 {
        std::mem::swap(&mut x0, &mut y0);
        std::mem::swap(&mut x1, &mut y1);
    }
    // descending is the same compare with the directions swapped
    if code & DESCENDING_FIRST != 0 {
        std::mem::swap(&mut x0, &mut x1);
    }
    if code & DESCENDING_SECOND != 0 {
        std::mem::swap(&mut y0, &mut y1);
    }
    (x0, y0) < (x1, y1)
}

fn listed(list: &[Info]) -> Vec<Listed> {
    list.iter()
        .enumerate()
        .map(|(i, f)| Listed { index: i + 1, reference: f.reference.clone(), x: f.x / 1000, y: f.y / 1000, rounded_x: f.rounded_x as f64 / 1000.0, rounded_y: f.rounded_y as f64 / 1000.0 })
        .collect()
}

/// `BuildChangeArray` for one side.
fn build_change_array(list: &[Info], start: u32, prefix: &str, remove_prefix: bool, infos: &mut Vec<PrefixInfo>, changes: &mut Vec<Change>, bad: &mut Vec<Bad>) {
    let prefix_len = prefix.len();
    let have_prefix = prefix_len != 0;
    let add_prefix = have_prefix && !remove_prefix;
    let remove_prefix = remove_prefix && have_prefix;
    // a start number restarts every prefix the plan knows
    if start != 0 {
        for info in infos.iter_mut() {
            info.last_used = start as i64 - 1;
        }
    }
    for f in list {
        let mut change = Change { old: f.reference.clone(), new: f.reference.clone(), front: f.front, action: f.action };
        if matches!(change.action, Action::Empty | Action::Invalid) {
            changes.push(change);
            bad.push(Bad { reference: f.reference.clone(), fpid: f.fpid.clone(), x: f.x / 1000, y: f.y / 1000 });
            continue;
        }
        if change.action == Action::Update {
            let mut pfx = f.prefix.clone();
            let present = pfx.starts_with(prefix);
            if add_prefix && !present {
                pfx.insert_str(0, prefix);
            }
            if remove_prefix && present {
                pfx.drain(..prefix_len);
            }
            let info = info_of(infos, &pfx, start as i64);
            let mut number = info.last_used + 1;
            while info.unavailable.contains(&number) {
                number += 1;
            }
            change.new = format!("{}{}", info.prefix, number);
            info.last_used = number;
        }
        changes.push(change);
    }
}

/// `BuildFootprintList` and what follows it in `ReannotateBoard`: the plan for `footprints` under `opts`. `elsewhere` are the references of the design's parts that are not
/// on the board; their numbers are not given out.
pub fn plan(footprints: &[ReannotateInput], elsewhere: &[String], opts: &ReannotateOptions) -> Plan {
    let grid = if opts.grid_um <= 0 { 1000 } else { opts.grid_um * 1000 };
    let excluded: Vec<String> = opts.exclude.split([',', ' ', '\t', '\r', '\n']).filter(|t| !t.is_empty()).map(str::to_string).collect();
    let (mut front, mut back): (Vec<Info>, Vec<Info>) = (Vec::new(), Vec::new());
    for f in footprints {
        let reference = f.reference.clone();
        let mut action = if reference.is_empty() {
            Action::Empty
        } else if !reference.contains(|c: char| c.is_ascii_digit()) {
            Action::Invalid
        } else {
            Action::Update
        };
        let prefix = prefix_of(&reference).to_string();
        for pattern in &excluded {
            // an exclusion that ends in `*` is a pattern for the whole reference, any other is a prefix
            if pattern.ends_with('*') {
                if wild_matches(pattern, &reference) {
                    action = Action::Exclude;
                    break;
                }
            } else if *pattern == prefix {
                action = Action::Exclude;
                break;
            }
        }
        // the scope has the last word
        if f.locked && opts.exclude_locked {
            action = Action::Exclude;
        } else if opts.scope == Scope::Selection {
            action = if f.selected { Action::Update } else { Action::Exclude };
        } else if opts.scope == Scope::Front {
            action = if f.front { Action::Update } else { Action::Exclude };
        } else if opts.scope == Scope::Back {
            action = if f.front { Action::Exclude } else { Action::Update };
        }
        let (x, y) = (f.x * 1000, f.y * 1000);
        let info = Info { reference, fpid: f.fpid.clone(), x, y, rounded_x: round_to_grid(x, grid), rounded_y: round_to_grid(y, grid), front: f.front, action, prefix };
        (if f.front { &mut front } else { &mut back }).push(info);
    }

    // equal places keep the references' own order, so the same board always numbers the same way
    front.sort_by(|a, b| str_num_cmp(&a.reference, &b.reference, true));
    back.sort_by(|a, b| str_num_cmp(&a.reference, &b.reference, true));
    let code = if (opts.direction as usize) < FRONT_DIRECTIONS.len() { opts.direction as usize } else { 0 };
    let order = |code: u8| move |a: &Info, b: &Info| if before(a, b, code) { Ordering::Less } else if before(b, a, code) { Ordering::Greater } else { Ordering::Equal };
    front.sort_by(order(FRONT_DIRECTIONS[code]));
    back.sort_by(order(BACK_DIRECTIONS[code]));

    let mut out = Plan { excluded, ..Plan::default() };
    let mut infos: Vec<PrefixInfo> = Vec::new();

    // `BuildUnavailableRefsList`: the numbers of the footprints that keep their reference
    for f in front.iter().chain(back.iter()).filter(|f| f.action == Action::Exclude) {
        info_of(&mut infos, &f.prefix, 1).unavailable.insert(ref_des_number(&f.reference));
    }
    for reference in elsewhere {
        info_of(&mut infos, prefix_of(reference), 1).unavailable.insert(ref_des_number(reference));
    }

    if !front.is_empty() {
        out.front = listed(&front);
        build_change_array(&front, opts.front_start, &sanitize_prefix(&opts.front_prefix), opts.remove_front_prefix, &mut infos, &mut out.changes, &mut out.bad);
    }
    if !back.is_empty() {
        out.back = listed(&back);
        build_change_array(&back, opts.back_start, &sanitize_prefix(&opts.back_prefix), opts.remove_back_prefix, &mut infos, &mut out.changes, &mut out.bad);
    }
    out.changes.sort_by(|a, b| str_num_cmp(&a.old, &b.old, false));
    out.prefixes = infos.into_iter().map(|i| i.prefix).collect();

    // two footprints with one reference: the plan cannot be applied
    let mut error_count = 0usize;
    'scan: for i in 0..out.changes.len() {
        if !matches!(out.changes[i].action, Action::Empty | Action::Invalid) {
            for j in i + 1..out.changes.len() {
                if out.changes[i].new == out.changes[j].new {
                    out.errors.push(format!("Duplicate instances of {}", out.changes[j].new));
                    let told = error_count;
                    error_count += 1;
                    if told > MAX_ERRORS {
                        out.errors.push("Aborted: too many errors".to_string());
                        break;
                    }
                }
            }
        }
        if error_count > MAX_ERRORS {
            break 'scan;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(reference: &str, x: i64, y: i64, front: bool) -> ReannotateInput {
        ReannotateInput { reference: reference.into(), fpid: "Lib:FP".into(), x, y, front, locked: false, selected: false }
    }

    fn news(plan: &Plan) -> Vec<(String, String)> {
        plan.changes.iter().map(|c| (c.old.clone(), c.new.clone())).collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn a_coordinate_rounds_to_the_grid_with_the_sign_of_c_remainders() {
        assert_eq!(round_to_grid(1500, 1000), 1000, "exactly half goes down");
        assert_eq!(round_to_grid(1501, 1000), 2000);
        assert_eq!(round_to_grid(-1500, 1000), -1000);
        assert_eq!(round_to_grid(-1501, 1000), -2000);
        assert_eq!(round_to_grid(299, 1000), 0);
        assert_eq!(round_to_grid(7, 0), 0, "no grid is the minimum grid, 1000 nm");
        assert_eq!(round_to_grid(1_270_000 * 3 + 700_000, 1_270_000), 1_270_000 * 4, "a 50 mil grid");
        assert_eq!(round_to_grid(1_270_000 * 3 + 600_000, 1_270_000), 1_270_000 * 3);
    }

    #[test]
    fn numbers_in_strings_compare_as_numbers() {
        assert_eq!(str_num_cmp("R2", "R10", false), Ordering::Less);
        assert_eq!(str_num_cmp("R10", "R2", false), Ordering::Greater);
        assert_eq!(str_num_cmp("R2", "R2", false), Ordering::Equal);
        assert_eq!(str_num_cmp("R", "R1", false), Ordering::Less, "the shorter one first");
        assert_eq!(str_num_cmp("C9", "R1", false), Ordering::Less);
        assert_eq!(str_num_cmp("r1", "R1", true), Ordering::Equal);
        assert_eq!(str_num_cmp("r1", "R1", false), Ordering::Greater, "lower case is after upper case when case counts");
    }

    #[test]
    fn wildcards_match_the_whole_reference_and_case_counts() {
        assert!(wild_matches("R1*", "R1"));
        assert!(wild_matches("R1*", "R10"));
        assert!(!wild_matches("R1*", "R2"));
        assert!(wild_matches("*", ""));
        assert!(wild_matches("R?*", "R5"));
        assert!(!wild_matches("R?*", "R"));
        assert!(!wild_matches("r*", "R1"));
        assert!(wild_matches("*K", "10K"));
    }

    #[test]
    fn a_reference_number_is_what_follows_the_first_digit_when_that_is_a_number() {
        assert_eq!(ref_des_number("R12"), 12);
        assert_eq!(ref_des_number("R12A"), -1);
        assert_eq!(ref_des_number("TP"), -1);
        assert_eq!(ref_des_number("D4"), 4);
        assert_eq!(prefix_of("R12"), "R");
        assert_eq!(prefix_of("TP"), "TP");
        assert_eq!(prefix_of("F_R12"), "F_R");
    }

    #[test]
    fn the_eight_directions_number_a_two_by_two_grid_in_their_order() {
        // a 2 x 2 block at 10 mm pitch: A at the top left, B top right, C bottom left, D bottom right
        let block = |f: bool| vec![fp("R4", 0, 0, f), fp("R3", 10_000, 0, f), fp("R2", 0, 10_000, f), fp("R1", 10_000, 10_000, f)];
        let order = |direction: u8, front: bool| -> Vec<String> {
            let opts = ReannotateOptions { direction, ..Default::default() };
            let p = plan(&block(front), &[], &opts);
            // the references the places get, in the order of the places: top left, top right, bottom left, bottom right
            ["R4", "R3", "R2", "R1"].iter().map(|old| p.changes.iter().find(|c| c.old == *old).unwrap().new.clone()).collect()
        };
        // front: top to bottom, left to right
        assert_eq!(order(0, true), ["R1", "R2", "R3", "R4"]);
        assert_eq!(order(1, true), ["R2", "R1", "R4", "R3"], "top to bottom, right to left");
        assert_eq!(order(2, true), ["R3", "R4", "R1", "R2"], "bottom to top, left to right");
        assert_eq!(order(3, true), ["R4", "R3", "R2", "R1"], "bottom to top, right to left");
        assert_eq!(order(4, true), ["R1", "R3", "R2", "R4"], "left to right, top to bottom");
        assert_eq!(order(5, true), ["R2", "R4", "R1", "R3"], "left to right, bottom to top");
        assert_eq!(order(6, true), ["R3", "R1", "R4", "R2"], "right to left, top to bottom");
        assert_eq!(order(7, true), ["R4", "R2", "R3", "R1"], "right to left, bottom to top");
        // back: left and right are swapped, the board seen from below
        assert_eq!(order(0, false), ["R2", "R1", "R4", "R3"]);
        assert_eq!(order(1, false), ["R1", "R2", "R3", "R4"]);
        assert_eq!(order(4, false), ["R3", "R1", "R4", "R2"]);
    }

    #[test]
    fn a_footprint_a_little_out_of_line_sorts_with_its_row_when_the_grid_is_coarse() {
        // R2 is 0.4 mm lower than R1, a 1 mm grid puts them on one row, so the left one is first
        let parts = vec![fp("R9", 0, 400, true), fp("R8", 5_000, 0, true), fp("R7", 10_000, 100, true)];
        let p = plan(&parts, &[], &ReannotateOptions { grid_um: 1_000, ..Default::default() });
        assert_eq!(news(&p), pairs(&[("R7", "R3"), ("R8", "R2"), ("R9", "R1")]));
        // with no grid but the minimum the lowest row goes last
        let p = plan(&parts, &[], &ReannotateOptions::default());
        assert_eq!(news(&p), pairs(&[("R7", "R2"), ("R8", "R1"), ("R9", "R3")]));
    }

    #[test]
    fn each_prefix_counts_on_from_the_start_number_and_the_back_continues_from_the_front() {
        let parts = vec![fp("R5", 0, 0, true), fp("C1", 5_000, 0, true), fp("R2", 10_000, 0, true), fp("R8", 0, 0, false), fp("C3", 5_000, 0, false)];
        let p = plan(&parts, &[], &ReannotateOptions { front_start: 10, ..Default::default() });
        assert_eq!(news(&p), pairs(&[("C1", "C10"), ("C3", "C11"), ("R2", "R11"), ("R5", "R10"), ("R8", "R12")]));
        assert_eq!(p.prefixes, ["R", "C"]);
        // the back starts over at 100
        let p = plan(&parts, &[], &ReannotateOptions { back_start: 100, ..Default::default() });
        assert_eq!(news(&p), pairs(&[("C1", "C1"), ("C3", "C100"), ("R2", "R2"), ("R5", "R1"), ("R8", "R100")]));
        // a blank front start leaves every prefix to start after zero again
        let p = plan(&parts, &[], &ReannotateOptions { front_start: 0, ..Default::default() });
        assert_eq!(news(&p).iter().find(|(o, _)| o == "R5").unwrap().1, "R1");
    }

    #[test]
    fn the_numbers_of_the_excluded_footprints_are_not_given_out_again() {
        let mut locked = fp("R2", 0, 0, true);
        locked.locked = true;
        let parts = vec![fp("R9", 5_000, 0, true), locked, fp("R7", 10_000, 0, true)];
        let p = plan(&parts, &[], &ReannotateOptions { exclude_locked: true, ..Default::default() });
        assert_eq!(news(&p), pairs(&[("R2", "R2"), ("R7", "R3"), ("R9", "R1")]));
        assert_eq!(p.changes.iter().find(|c| c.old == "R2").unwrap().action, Action::Exclude);
        // a part that is not on the board keeps its number too
        let p = plan(&[fp("R9", 0, 0, true), fp("R7", 5_000, 0, true)], &["R1".to_string(), "R3".to_string()], &ReannotateOptions::default());
        assert_eq!(news(&p), pairs(&[("R7", "R4"), ("R9", "R2")]));
    }

    #[test]
    fn the_exclusion_list_takes_prefixes_and_patterns_and_the_scope_has_the_last_word() {
        let parts = vec![fp("R5", 0, 0, true), fp("RN1", 5_000, 0, true), fp("C7", 10_000, 0, true), fp("C70", 15_000, 0, true)];
        // `R` is the prefix R, not RN; `C7*` is C7 and C70
        let p = plan(&parts, &[], &ReannotateOptions { exclude: "R, C7*".into(), ..Default::default() });
        let acts: Vec<(String, Action)> = p.changes.iter().map(|c| (c.old.clone(), c.action)).collect();
        assert_eq!(acts, [("C7".to_string(), Action::Exclude), ("C70".to_string(), Action::Exclude), ("R5".to_string(), Action::Exclude), ("RN1".to_string(), Action::Update)]);
        assert_eq!(p.excluded, ["R", "C7*"]);
        // as KiCad has it, the scope decides after the list: with "Front" the excluded prefix is updated
        let p = plan(&parts, &[], &ReannotateOptions { exclude: "R".into(), scope: Scope::Front, ..Default::default() });
        assert!(p.changes.iter().all(|c| c.action == Action::Update));
    }

    #[test]
    fn the_scope_names_the_footprints_to_update_and_the_others_keep_their_references() {
        let mut selected = fp("R4", 0, 0, true);
        selected.selected = true;
        let parts = vec![selected, fp("R2", 5_000, 0, true), fp("R6", 0, 0, false)];
        let acts = |scope: Scope| -> Vec<(String, Action)> { plan(&parts, &[], &ReannotateOptions { scope, ..Default::default() }).changes.iter().map(|c| (c.old.clone(), c.action)).collect() };
        assert_eq!(acts(Scope::Selection), [("R2".to_string(), Action::Exclude), ("R4".to_string(), Action::Update), ("R6".to_string(), Action::Exclude)]);
        assert_eq!(acts(Scope::Back), [("R2".to_string(), Action::Exclude), ("R4".to_string(), Action::Exclude), ("R6".to_string(), Action::Update)]);
        // only R4 is updated, and R2 and R6 keep their numbers: R4 gets the first free one, R1
        let p = plan(&parts, &[], &ReannotateOptions { scope: Scope::Selection, ..Default::default() });
        assert_eq!(p.renames().into_iter().collect::<Vec<_>>(), [("R4".to_string(), "R1".to_string())]);
    }

    #[test]
    fn a_prefix_is_added_to_a_side_once_and_taken_off_again() {
        let parts = vec![fp("R1", 0, 0, true), fp("F_R9", 5_000, 0, true), fp("R3", 0, 0, false)];
        let p = plan(&parts, &[], &ReannotateOptions { front_prefix: "F_".into(), back_prefix: "B_".into(), ..Default::default() });
        assert_eq!(news(&p), pairs(&[("F_R9", "F_R2"), ("R1", "F_R1"), ("R3", "B_R1")]), "F_R9 has the prefix already; the back's own prefix starts at 1");
        let p = plan(&parts, &[], &ReannotateOptions { front_prefix: "F_".into(), remove_front_prefix: true, ..Default::default() });
        assert_eq!(news(&p), pairs(&[("F_R9", "R2"), ("R1", "R1"), ("R3", "R3")]));
        assert_eq!(sanitize_prefix("F-_ x!+"), "F-_x+", "a prefix is letters, digits and _-+=/\\");
    }

    #[test]
    fn a_reference_without_a_number_is_told_not_updated_in_the_scope_all_and_numbered_in_another() {
        let parts = vec![fp("TP", 0, 0, true), fp("R3", 5_000, 0, true)];
        let p = plan(&parts, &[], &ReannotateOptions::default());
        assert_eq!(news(&p), pairs(&[("R3", "R1"), ("TP", "TP")]));
        assert_eq!(p.bad.len(), 1);
        assert_eq!((p.bad[0].reference.as_str(), p.bad[0].fpid.as_str()), ("TP", "Lib:FP"));
        assert_eq!(p.changes.iter().find(|c| c.old == "TP").unwrap().action, Action::Invalid);
        let p = plan(&parts, &[], &ReannotateOptions { scope: Scope::Front, ..Default::default() });
        assert_eq!(news(&p), pairs(&[("R3", "R1"), ("TP", "TP1")]));
        assert!(p.bad.is_empty());
    }

    #[test]
    fn the_change_log_is_in_the_order_of_the_old_references() {
        let parts = vec![fp("R10", 0, 0, true), fp("C2", 1_000, 0, true), fp("R9", 2_000, 0, true), fp("C10", 3_000, 0, true)];
        let p = plan(&parts, &[], &ReannotateOptions::default());
        assert_eq!(p.changes.iter().map(|c| c.old.as_str()).collect::<Vec<_>>(), ["C2", "C10", "R9", "R10"]);
        assert_eq!(p.front.len(), 4);
        assert_eq!(p.front[0].index, 1);
        assert!(p.back.is_empty(), "no footprint on the back, no list");
    }

    #[test]
    fn two_footprints_that_keep_one_reference_are_an_error_and_the_check_gives_up_after_a_while() {
        let parts = vec![fp("R3", 0, 0, true), fp("R3", 5_000, 0, true), fp("R4", 10_000, 0, true)];
        let p = plan(&parts, &[], &ReannotateOptions { exclude: "R3*".into(), scope: Scope::All, ..Default::default() });
        assert!(!p.ok());
        assert_eq!(p.errors, ["Duplicate instances of R3"]);
        // fifteen of them
        let many: Vec<ReannotateInput> = (0..15).map(|i| fp("R3", i * 1000, 0, true)).collect();
        let p = plan(&many, &[], &ReannotateOptions { exclude: "R".into(), ..Default::default() });
        assert!(p.errors.iter().any(|e| e == "Aborted: too many errors"));
        assert!(p.errors.len() > MAX_ERRORS);
    }

    #[test]
    fn an_empty_board_has_nothing_to_do() {
        let p = plan(&[], &[], &ReannotateOptions::default());
        assert!(p.ok() && p.changes.is_empty() && p.front.is_empty() && p.back.is_empty());
    }

    #[test]
    fn the_options_read_back_from_json_with_their_defaults() {
        let o: ReannotateOptions = serde_json::from_str(r#"{"direction":2,"grid_um":1270,"scope":"selection","exclude":"R","front_start":5}"#).unwrap();
        assert_eq!((o.direction, o.grid_um, o.scope, o.front_start, o.back_start), (2, 1270, Scope::Selection, 5, 0));
        let o: ReannotateOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(o, ReannotateOptions::default());
        assert_eq!(o.front_start, 1, "the dialog opens with front reference start 1");
    }
}
