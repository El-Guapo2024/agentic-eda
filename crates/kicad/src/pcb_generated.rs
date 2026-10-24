//! `(generated ..)`: the tuning patterns of a board (`PCB_TUNING_PATTERN`, a `PCB_GENERATOR`), written as `PCB_IO_KICAD_SEXPR::format( const
//! PCB_GENERATOR* )` writes them and read as `PCB_IO_KICAD_SEXPR_PARSER::parseGENERATOR` and `PCB_TUNING_PATTERN::SetProperties` read them.
//!
//! A generator is `(generated (uuid ..) (type tuning_pattern) (name "Tuning Pattern") (layer ..) [(locked yes)] <properties, in the order a
//! `std::map` keeps them> (members <uuid> ..))`. The properties of a tuning pattern are the ones `PCB_TUNING_PATTERN::GetProperties` lists, lengths in
//! millimetres: a number is written bare, a boolean as `yes` or `no`, a point as `(key (xy x y))`, a line as `(key (pts (xy x y) ..))`, a string quoted.
//! A pattern is not written without a member on the board (`"Some conditions appear to still be creating ghost tuning patterns. Don't save them."`),
//! and one read without any is dropped (`"Previous versions had bugs which could save ghost tuning patterns. Ignore them."`).
//!
//! Boards written by KiCad 9 have `last_tuning` (a string) where this has `last_tuning_length` (a number): the string is ignored on reading, the
//! length is recomputed the next time the pattern is tuned.

use crate::sexpr::{self, Sexpr};
use crate::{mm, mm_to_nm, sexpr_str};
use eda_model::ir::{Design, Point, Nm};
use eda_model::tuning::{PatternSide, TargetRange, TuningMode, TuningPattern, DELAY_UNCONSTRAINED_AS, LENGTH_UNCONSTRAINED_NM};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// A number the way `fmt::format( "{:.10g}", v )` writes it, which is how `format( const PCB_GENERATOR* )` writes every numeric property.
pub(crate) fn fmt_g10(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return "0".into();
    }
    // ten significant digits, d.ddddddddde<exp>
    let sci = format!("{:.9e}", v);
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let trim = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.to_string()
        }
    };
    if !(-4..10).contains(&exp) {
        // %g switches to exponent notation outside 1e-4 <= |v| < 1e10: "1.5e-05", "1e+10"
        let sign = if exp < 0 { '-' } else { '+' };
        return format!("{}e{}{:02}", trim(mant), sign, exp.abs());
    }
    let decimals = (9 - exp).max(0) as usize;
    trim(&format!("{:.*}", decimals, v))
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

/// A point as the file keeps it: `(key\n\t\t\t(xy x y)\n\t\t)`.
pub(crate) fn point_prop(out: &mut String, key: &str, p: Point) {
    writeln!(out, "\t\t({key}\n\t\t\t(xy {} {})\n\t\t)", mm(p.x), mm(p.y)).unwrap();
}

/// A line as the file keeps it: `(key (pts (xy ..) ..))`.
pub(crate) fn line_prop(out: &mut String, key: &str, pts: &[Point]) {
    let xy: Vec<String> = pts.iter().map(|p| format!("(xy {} {})", mm(p.x), mm(p.y))).collect();
    writeln!(out, "\t\t({key}\n\t\t\t(pts\n\t\t\t\t{}\n\t\t\t)\n\t\t)", xy.join(" ")).unwrap();
}

fn range_props(out: &mut String, key: &str, r: &TargetRange) {
    writeln!(out, "\t\t({key} {})", mm(r.opt)).unwrap();
    writeln!(out, "\t\t({key}_max {})", mm(r.max)).unwrap();
    writeln!(out, "\t\t({key}_min {})", mm(r.min)).unwrap();
}

/// A time as the file keeps it: `set_iu` writes the internal unit (an attosecond) over a million, as a length is written over a million nanometres -- picoseconds.
fn ps(attoseconds: i64) -> String {
    fmt_g10(attoseconds as f64 / 1_000_000.0)
}

/// `target_delay`, `target_delay_max`, `target_delay_min`: a delay target in picoseconds.
fn delay_props(out: &mut String, key: &str, r: &TargetRange) {
    writeln!(out, "\t\t({key} {})", ps(r.opt)).unwrap();
    writeln!(out, "\t\t({key}_max {})", ps(r.max)).unwrap();
    writeln!(out, "\t\t({key}_min {})", ps(r.min)).unwrap();
}

/// The `(generated ..)` blocks of the board's tuning patterns: `written` says which uuids each track of the design was written as, `uuid_of` names a
/// pattern, `locked` says whether it is.
pub(crate) fn write_generated(out: &mut String, patterns: &[TuningPattern], written: &BTreeMap<String, Vec<String>>, uuid_of: &dyn Fn(&TuningPattern) -> String, locked: &dyn Fn(&str) -> bool) {
    let mut sorted: Vec<&TuningPattern> = patterns.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    for p in sorted {
        let mut members: Vec<String> = p.member_ids.iter().flat_map(|m| written.get(m).cloned().unwrap_or_default()).collect();
        if members.is_empty() {
            continue; // a ghost
        }
        members.sort();
        members.dedup();
        writeln!(out, "\t(generated\n\t\t(uuid \"{}\")\n\t\t(type tuning_pattern)\n\t\t(name \"Tuning Pattern\")\n\t\t(layer {})", uuid_of(p), sexpr_str(&p.layer)).unwrap();
        if locked(&p.id) {
            writeln!(out, "\t\t(locked yes)").unwrap();
        }
        if !p.base_line.is_empty() {
            line_prop(out, "base_line", &p.base_line);
        }
        if !p.base_line_coupled.is_empty() {
            line_prop(out, "base_line_coupled", &p.base_line_coupled);
        }
        writeln!(out, "\t\t(corner_radius_percent {})", p.corner_radius_percent).unwrap();
        point_prop(out, "end", p.end);
        writeln!(out, "\t\t(initial_side {})", sexpr_str(p.initial_side.token())).unwrap();
        writeln!(out, "\t\t(is_time_domain {})", yes_no(p.is_time_domain)).unwrap();
        writeln!(out, "\t\t(last_diff_pair_gap {})", mm(p.last_diff_pair_gap)).unwrap();
        writeln!(out, "\t\t(last_netname {})", sexpr_str(&p.last_netname)).unwrap();
        writeln!(out, "\t\t(last_status {})", sexpr_str(if p.last_status.is_empty() { "tuned" } else { &p.last_status })).unwrap();
        writeln!(out, "\t\t(last_track_width {})", mm(p.last_track_width)).unwrap();
        writeln!(out, "\t\t(last_tuning_length {})", fmt_g10(p.last_tuning_length as f64 / 1_000_000.0)).unwrap();
        writeln!(out, "\t\t(max_amplitude {})", mm(p.max_amplitude)).unwrap();
        writeln!(out, "\t\t(min_amplitude {})", mm(p.min_amplitude)).unwrap();
        writeln!(out, "\t\t(min_spacing {})", mm(p.spacing)).unwrap();
        point_prop(out, "origin", p.origin);
        writeln!(out, "\t\t(override_custom_rules {})", yes_no(p.override_custom_rules)).unwrap();
        writeln!(out, "\t\t(rounded {})", yes_no(p.rounded)).unwrap();
        writeln!(out, "\t\t(single_sided {})", yes_no(p.single_sided)).unwrap();
        // the delay target of the time domain, in picoseconds (KiCad's "unconstrained" is `DELAY_UNCONSTRAINED`, a million of them)
        delay_props(out, "target_delay", &p.target_delay);
        range_props(out, "target_length", &p.target_length);
        range_props(out, "target_skew", &p.target_skew);
        writeln!(out, "\t\t(tuning_mode {})", sexpr_str(p.mode.token())).unwrap();
        write!(out, "\t\t(members").unwrap();
        for m in &members {
            write!(out, " \"{m}\"").unwrap();
        }
        writeln!(out, ")\n\t)").unwrap();
    }
}

// ---------------------------------------------------------------------------------------------------------------------------- reading

/// A number property: `(key 1.5)`.
pub(crate) fn prop_num(list: &[Sexpr], key: &str) -> Option<f64> {
    sexpr::find(list, key).and_then(|v| sexpr::num(v, 1))
}

/// A length property in micrometres.
fn prop_nm(list: &[Sexpr], key: &str) -> Option<Nm> {
    prop_num(list, key).map(mm_to_nm)
}

pub(crate) fn prop_str<'a>(list: &'a [Sexpr], key: &str) -> Option<&'a str> {
    sexpr::find(list, key).and_then(|v| sexpr::txt(v, 1))
}

fn prop_bool(list: &[Sexpr], key: &str) -> Option<bool> {
    prop_str(list, key).map(|v| v == "yes" || v == "true")
}

pub(crate) fn prop_point(list: &[Sexpr], key: &str) -> Option<Point> {
    let v = sexpr::find(list, key)?;
    let xy = sexpr::find(v, "xy")?;
    Some(Point { x: mm_to_nm(sexpr::num(xy, 1)?), y: mm_to_nm(sexpr::num(xy, 2)?) })
}

/// `(key (pts (xy x y) .. [(arc (start ..) (mid ..) (end ..))]))`: the points of a line, an arc by its ends.
pub(crate) fn prop_line(list: &[Sexpr], key: &str) -> Vec<Point> {
    let Some(v) = sexpr::find(list, key) else { return Vec::new() };
    let Some(pts) = sexpr::find(v, "pts") else { return Vec::new() };
    let mut out = Vec::new();
    for item in pts.iter().skip(1).filter_map(Sexpr::as_list) {
        match sexpr::tag(item) {
            Some("xy") => {
                if let (Some(x), Some(y)) = (sexpr::num(item, 1), sexpr::num(item, 2)) {
                    out.push(Point { x: mm_to_nm(x), y: mm_to_nm(y) });
                }
            }
            Some("arc") => {
                for part in ["start", "end"] {
                    if let Some(p) = sexpr::find(item, part).and_then(|s| Some(Point { x: mm_to_nm(sexpr::num(s, 1)?), y: mm_to_nm(sexpr::num(s, 2)?) })) {
                        out.push(p);
                    }
                }
            }
            _ => {}
        }
    }
    out.dedup();
    out
}

fn prop_range(list: &[Sexpr], key: &str, default: TargetRange) -> TargetRange {
    // `SetTargetLength( val )` gives a target its default tolerance, then the file's own minimum and maximum replace it
    let opt = prop_nm(list, key).unwrap_or(default.opt);
    let mut r = if opt == LENGTH_UNCONSTRAINED_NM { TargetRange::unconstrained() } else { TargetRange::around(opt) };
    if let Some(min) = prop_nm(list, &format!("{key}_min")) {
        r.min = min;
    }
    if let Some(max) = prop_nm(list, &format!("{key}_max")) {
        r.max = max;
    }
    r
}

/// A time in the file, picoseconds, as attoseconds.
fn prop_delay(list: &[Sexpr], key: &str) -> Option<i64> {
    prop_num(list, key).map(|v| (v * 1_000_000.0).round() as i64)
}

/// `SetTargetLengthDelay( val )` and then the file's own minimum and maximum, as for a length.
fn prop_delay_range(list: &[Sexpr], key: &str) -> TargetRange {
    let opt = prop_delay(list, key).unwrap_or(DELAY_UNCONSTRAINED_AS);
    let mut r = if opt == DELAY_UNCONSTRAINED_AS { TargetRange::delay_unconstrained() } else { TargetRange::delay_around(opt) };
    if let Some(min) = prop_delay(list, &format!("{key}_min")) {
        r.min = min;
    }
    if let Some(max) = prop_delay(list, &format!("{key}_max")) {
        r.max = max;
    }
    r
}

/// The tuning patterns of the board file `root`, their members named by the file's uuid: `track_ids` maps the uuid of a track the importer made to
/// its id in the design. A generator of another kind, and a pattern none of whose members is a track of the board, is left out.
pub(crate) fn import_patterns(root: &[Sexpr], track_ids: &BTreeMap<String, String>) -> Vec<TuningPattern> {
    let mut out = Vec::new();
    for g in sexpr::find_all(root, "generated") {
        if prop_str(g, "type") != Some("tuning_pattern") {
            continue;
        }
        let members: Vec<String> = sexpr::find(g, "members")
            .map(|m| m.iter().skip(1).filter_map(Sexpr::text).filter_map(|u| track_ids.get(u).cloned()).collect())
            .unwrap_or_default();
        if members.is_empty() {
            continue; // "Ignore them"
        }
        let mode = prop_str(g, "tuning_mode").and_then(TuningMode::from_token).unwrap_or_default();
        let origin = prop_point(g, "origin").unwrap_or_default();
        let end = prop_point(g, "end").unwrap_or(origin);
        let mut skew = TargetRange { min: -100_000, opt: 0, max: 100_000 };
        skew = prop_range(g, "target_skew", skew);
        out.push(TuningPattern {
            id: String::new(),
            mode,
            layer: prop_str(g, "layer").unwrap_or("F.Cu").to_string(),
            origin,
            end,
            initial_side: prop_str(g, "initial_side").and_then(PatternSide::from_token).unwrap_or(PatternSide::Left),
            min_amplitude: prop_nm(g, "min_amplitude").unwrap_or(200_000),
            max_amplitude: prop_nm(g, "max_amplitude").unwrap_or(1_000_000),
            spacing: prop_nm(g, "min_spacing").unwrap_or(600_000),
            corner_radius_percent: prop_num(g, "corner_radius_percent").map(|v| v.round().clamp(0.0, 100.0) as u32).unwrap_or(80),
            rounded: prop_bool(g, "rounded").unwrap_or(true),
            single_sided: prop_bool(g, "single_sided").unwrap_or(false),
            override_custom_rules: prop_bool(g, "override_custom_rules").unwrap_or(false),
            target_length: prop_range(g, "target_length", TargetRange::unconstrained()),
            target_skew: skew,
            is_time_domain: prop_bool(g, "is_time_domain").unwrap_or(false),
            target_delay: prop_delay_range(g, "target_delay"),
            // (the board file keeps no skew delay)
            target_skew_delay: TargetRange { min: -100_000, opt: 0, max: 100_000 },
            base_line: prop_line(g, "base_line"),
            base_line_coupled: prop_line(g, "base_line_coupled"),
            last_track_width: prop_nm(g, "last_track_width").unwrap_or(0),
            last_diff_pair_gap: prop_nm(g, "last_diff_pair_gap").unwrap_or(0),
            last_tuning_length: prop_num(g, "last_tuning_length").map(|mm| (mm * 1_000_000.0).round() as i64).unwrap_or(0),
            last_tuning_delay: 0,
            last_netname: prop_str(g, "last_netname").unwrap_or("").to_string(),
            last_status: prop_str(g, "last_status").unwrap_or("tuned").to_string(),
            member_ids: members,
        });
    }
    out
}

/// The patterns of an imported design: [`import_patterns`] with the tracks the importer made (`refs.tracks[i]` is the file uuid of the design's `i`th track).
pub(crate) fn attach_patterns(design: &mut Design, root: &[Sexpr], refs: &crate::import_items::Refs) {
    let Some(rt) = &design.routing else { return };
    let ids: BTreeMap<String, String> = refs.tracks.iter().zip(&rt.tracks).filter_map(|(r, t)| Some((r.uuid.clone()?, t.id.clone()))).collect();
    let patterns = import_patterns(root, &ids);
    if patterns.is_empty() {
        return;
    }
    let dr = design.drawings.get_or_insert_with(Default::default);
    dr.tuning_patterns = patterns;
    dr.assign_missing_ids();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> Point {
        Point { x, y }
    }

    fn pattern() -> TuningPattern {
        TuningPattern {
            id: "tun_1".into(),
            mode: TuningMode::DiffPair,
            layer: "F.Cu".into(),
            origin: p(115_571_000, 50_140_000),
            end: p(123_137_000, 50_140_000),
            initial_side: PatternSide::Right,
            min_amplitude: 200_000,
            max_amplitude: 1_500_000,
            spacing: 1_000_000,
            corner_radius_percent: 100,
            rounded: false,
            single_sided: true,
            override_custom_rules: true,
            target_length: TargetRange { min: 99_900_000, opt: 100_000_000, max: 100_100_000 },
            target_skew: TargetRange { min: -100_000, opt: 0, max: 100_000 },
            is_time_domain: false,
            target_delay: TargetRange::delay_unconstrained(),
            target_skew_delay: TargetRange { min: -100_000, opt: 0, max: 100_000 },
            base_line: vec![p(115_571_000, 50_140_000), p(123_137_000, 50_140_000)],
            base_line_coupled: vec![p(115_571_000, 50_540_000), p(123_137_000, 50_540_000)],
            last_track_width: 200_000,
            last_diff_pair_gap: 180_000,
            last_tuning_length: 99_987_654,
            last_tuning_delay: 0,
            last_netname: "USB_P".into(),
            last_status: "too_short".into(),
            member_ids: vec!["trk_a".into(), "trk_b".into()],
        }
    }

    fn written() -> BTreeMap<String, Vec<String>> {
        [("trk_a".to_string(), vec!["bbbb".to_string()]), ("trk_b".to_string(), vec!["aaaa".to_string(), "cccc".to_string()])].into()
    }

    #[test]
    fn a_pattern_is_written_with_the_properties_kicad_lists_in_its_order() {
        let mut out = String::new();
        write_generated(&mut out, &[pattern()], &written(), &|_| "uuid-1".into(), &|_| false);
        assert!(out.starts_with("\t(generated\n\t\t(uuid \"uuid-1\")\n\t\t(type tuning_pattern)\n\t\t(name \"Tuning Pattern\")\n\t\t(layer \"F.Cu\")\n"), "{out}");
        let keys: Vec<&str> = out.lines().filter(|l| l.starts_with("\t\t(")).map(|l| l.trim_start_matches("\t\t(").split(|c| c == ' ' || c == '\n').next().unwrap()).collect();
        let want = [
            "uuid", "type", "name", "layer", "base_line", "base_line_coupled", "corner_radius_percent", "end", "initial_side", "is_time_domain", "last_diff_pair_gap", "last_netname", "last_status", "last_track_width", "last_tuning_length", "max_amplitude", "min_amplitude",
            "min_spacing", "origin", "override_custom_rules", "rounded", "single_sided", "target_delay", "target_delay_max", "target_delay_min", "target_length", "target_length_max", "target_length_min", "target_skew", "target_skew_max", "target_skew_min", "tuning_mode", "members",
        ];
        assert_eq!(keys, want, "{out}");
        assert!(out.contains("(members \"aaaa\" \"bbbb\" \"cccc\")"), "sorted uuids of both tracks: {out}");
        assert!(out.contains("(last_tuning_length 99.987654)") && out.contains("(target_length_min 99.9)") && out.contains("(target_skew_min -0.1)") && out.contains("(min_spacing 1)"), "{out}");
        assert!(out.contains("(rounded no)") && out.contains("(single_sided yes)") && out.contains("(tuning_mode \"diff_pair\")") && out.contains("(initial_side \"right\")"), "{out}");
        // locked, and without any member on the board nothing is written
        let mut locked = String::new();
        write_generated(&mut locked, &[pattern()], &written(), &|_| "u".into(), &|id| id == "tun_1");
        assert!(locked.contains("(layer \"F.Cu\")\n\t\t(locked yes)\n"), "{locked}");
        let mut ghost = String::new();
        write_generated(&mut ghost, &[pattern()], &BTreeMap::new(), &|_| "u".into(), &|_| false);
        assert!(ghost.is_empty());
    }

    #[test]
    fn a_pattern_written_is_a_pattern_read() {
        let mut out = String::new();
        write_generated(&mut out, &[pattern()], &written(), &|_| "uuid-1".into(), &|_| false);
        let tree = sexpr::parse(&format!("(kicad_pcb\n{out})")).unwrap();
        let root = tree.as_list().unwrap();
        let ids: BTreeMap<String, String> = [("aaaa", "trk_b1"), ("bbbb", "trk_a"), ("cccc", "trk_b2")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let back = import_patterns(root, &ids);
        assert_eq!(back.len(), 1);
        let mut want = pattern();
        want.id = String::new();
        want.member_ids = vec!["trk_b1".into(), "trk_a".into(), "trk_b2".into()];
        // the order the file lists the members in is the order of their uuids
        assert_eq!(back[0], want);
    }

    #[test]
    fn a_pattern_of_kicad_9_is_read_and_its_unknown_properties_are_left() {
        let text = r#"(kicad_pcb (generated (id 4f22a815-3048-42b3-86fa-eb71720d35ae) (type tuning_pattern) (name "Tuning Pattern") (layer "F.Cu")
            (base_line (pts (xy 115.571487 50.14) (xy 123.13735 50.14))) (corner_radius_percent 100) (end (xy 123.13735 50.14)) (initial_side "left")
            (last_diff_pair_gap 0.18) (last_netname "") (last_status "too_short") (last_track_width 0.2) (last_tuning "75.9800 mm (too short)")
            (max_amplitude 2) (min_spacing 0.6) (origin (xy 115.571487 50.14)) (override_custom_rules no) (rounded yes) (single_sided no)
            (target_length 100) (target_length_max 100.1) (target_length_min 99.9) (target_skew 0) (target_skew_max 0.1) (target_skew_min -0.1)
            (tuning_mode "single") (members 0376a7f9 0461661b gone)))"#;
        let tree = sexpr::parse(text).unwrap();
        let ids: BTreeMap<String, String> = [("0376a7f9", "t1"), ("0461661b", "t2")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let back = import_patterns(tree.as_list().unwrap(), &ids);
        assert_eq!(back.len(), 1);
        let t = &back[0];
        assert_eq!((t.mode, t.layer.as_str(), t.max_amplitude, t.min_amplitude, t.spacing, t.corner_radius_percent), (TuningMode::Single, "F.Cu", 2_000_000, 200_000, 600_000, 100), "min_amplitude takes MEANDER_SETTINGS' default");
        assert_eq!((t.target_length, t.last_status.as_str(), t.base_line.len(), t.member_ids.len()), (TargetRange { min: 99_900_000, opt: 100_000_000, max: 100_100_000 }, "too_short", 2, 2));
        assert_eq!((t.origin, t.end), (p(115_571_487, 50_140_000), p(123_137_350, 50_140_000)));
        // a pattern with no member of the board is a ghost
        assert!(import_patterns(tree.as_list().unwrap(), &BTreeMap::new()).is_empty());
    }
}
