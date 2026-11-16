//! A quick look into a `.kicad_mod` file: what the Footprint Chooser's tree and search need to know about every footprint (its
//! description, tags and how many pads a schematic pin can reach) without reading the file into an s-expression tree.
//!
//! KiCad.app ships 155 footprint libraries, about 15,000 footprints and 179 MB. `FOOTPRINT_INFO` (`common/footprint_info.cpp`)
//! carries, per footprint, the fields the chooser scores a search against -- the library nickname, the name, `Lib:Name`, the
//! keywords (`tags`), the description (`descr`) -- and the pad count `FOOTPRINT::GetNumberedPadCount` gives the "Filter by pin
//! count" check. One pass over the text finds all of them: the depth-1 forms `(descr "..")`, `(tags "..")` and `(pad ..)`, and
//! inside a pad its `(layers ..)`.

use std::collections::BTreeSet;

use crate::symbol_scan::read_string;

/// What the chooser knows about one footprint file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FootprintSummary {
    pub description: String,
    /// `(tags "..")`: KiCad's keywords for a footprint, space separated.
    pub tags: String,
    /// `FOOTPRINT::GetNumberedPadCount`: the distinct pad numbers a schematic pin can name -- on copper, not an NPTH, written as digits or as at
    /// most two letters then digits (`1`, `42`, `A1`, `AB10`), so `MP`, `GND` and a pad with no number are not counted.
    pub numbered_pads: u32,
    /// The pads of the file, whatever they are (`FOOTPRINT::GetPadCount( INCLUDE_NPTH )`).
    pub pads: u32,
}

fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && is_ws(b[i]) {
        i += 1;
    }
    i
}

fn word_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && !is_ws(b[i]) && !matches!(b[i], b'(' | b')' | b'"') {
        i += 1;
    }
    i
}

fn string_end(b: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < b.len() && b[i] != b'"' {
        if b[i] == b'\\' {
            i += 1;
        }
        i += 1;
    }
    (i + 1).min(b.len())
}

/// `isElectricalPadNumber` of `FOOTPRINT::GetNumberedPadCount`: up to two letters, then at least one digit and nothing else.
pub fn is_numbered_pad(number: &str) -> bool {
    let letters = number.chars().take_while(|c| c.is_alphabetic()).count();
    if letters > 2 {
        return false;
    }
    let rest: Vec<char> = number.chars().skip(letters).collect();
    !rest.is_empty() && rest.iter().all(|c| c.is_ascii_digit())
}

/// A pad in the making: the `(pad "N" type ..)` the scan is inside.
struct PadScan {
    number: String,
    npth: bool,
    copper: bool,
}

/// The summary of `.kicad_mod` text, or `None` when it has no `(footprint ..)` form at its top.
pub fn scan_footprint(text: &str) -> Option<FootprintSummary> {
    let b = text.as_bytes();
    let n = b.len();
    let mut out = FootprintSummary::default();
    let mut numbered: BTreeSet<String> = BTreeSet::new();
    let mut pad: Option<PadScan> = None;
    let mut seen_footprint = false;
    let (mut i, mut depth) = (0usize, 0usize);
    while i < n {
        match b[i] {
            b'"' => i = string_end(b, i),
            b'(' => {
                let tag_end = word_end(b, i + 1);
                let tag = &b[i + 1..tag_end];
                let mut next = tag_end;
                match (depth, tag) {
                    (0, b"footprint") | (0, b"module") => seen_footprint = true,
                    (1, b"descr") => {
                        if let Some((s, after)) = read_string(text, skip_ws(b, tag_end)) {
                            out.description = s;
                            next = after;
                        }
                    }
                    (1, b"tags") => {
                        if let Some((s, after)) = read_string(text, skip_ws(b, tag_end)) {
                            out.tags = s;
                            next = after;
                        }
                    }
                    // KiCad 10 also writes the description as a field; the `descr` form wins when both are there.
                    (1, b"property") => {
                        if let Some((key, after_key)) = read_string(text, skip_ws(b, tag_end)) {
                            if key == "Description" && out.description.is_empty() {
                                if let Some((s, after)) = read_string(text, skip_ws(b, after_key)) {
                                    out.description = s;
                                    next = after;
                                }
                            }
                        }
                    }
                    (1, b"pad") => {
                        out.pads += 1;
                        let at = skip_ws(b, tag_end);
                        if let Some((number, after)) = read_string(text, at) {
                            let type_at = skip_ws(b, after);
                            let type_end = word_end(b, type_at);
                            pad = Some(PadScan { number, npth: &b[type_at..type_end] == b"np_thru_hole", copper: false });
                            next = type_end;
                        }
                    }
                    (2, b"layers") if pad.is_some() => {
                        let mut j = tag_end;
                        loop {
                            j = skip_ws(b, j);
                            match read_string(text, j) {
                                Some((layer, after)) => {
                                    if layer.ends_with(".Cu") {
                                        if let Some(p) = pad.as_mut() {
                                            p.copper = true;
                                        }
                                    }
                                    j = after;
                                }
                                None => break,
                            }
                        }
                        next = j;
                    }
                    _ => {}
                }
                depth += 1;
                i = next;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 1 {
                    if let Some(p) = pad.take() {
                        if p.copper && !p.npth && is_numbered_pad(&p.number) {
                            numbered.insert(p.number);
                        }
                    }
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    out.numbered_pads = numbered.len() as u32;
    seen_footprint.then_some(out)
}

// ------------------------------------------------------------------------------------------------ the parts cache's look

/// One pad of a footprint as the parts cache keeps it (`crate::footprint_scan::scan_footprint_detail`): what a fit or a pitch is judged by, nothing of
/// its padstack, net-tie or thermal details.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PadDetail {
    pub number: String,
    /// The file's pad type: `smd`, `thru_hole`, `np_thru_hole` or `connect`.
    pub pad_type: String,
    /// The file's shape token: `rect`, `roundrect`, `circle`, `oval`, `trapezoid`, `custom` (KiCad writes a chamfered rectangle as `roundrect` with a chamfer).
    pub shape: String,
    /// `(at x y [rot])`, nanometres and degrees, in the footprint's frame.
    pub x: i64,
    pub y: i64,
    pub rot: f64,
    /// `(size w h)`, nanometres, before the rotation.
    pub w: i64,
    pub h: i64,
    /// `(drill d)` or the width of `(drill oval w h)`, nanometres; 0 for none.
    pub drill: i64,
    /// One of its layers is copper (`F.Cu`, `B.Cu`, `*.Cu`, an inner layer).
    pub copper: bool,
}

/// An axis-aligned box in nanometres: `(x0, y0, x1, y1)`.
pub type BoxNm = (i64, i64, i64, i64);

/// Everything the parts cache keeps of a `.kicad_mod` file ([`scan_footprint_detail`]): the chooser's summary and what a fit, a pitch, a size or a 3D model
/// is judged by.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FootprintDetail {
    /// What the choosers know ([`scan_footprint`]'s answer, the same rules).
    pub summary: FootprintSummary,
    /// `(attr ..)`'s placement type: `smd`, `through_hole`, or `unspecified` (no `(attr)`, or one with neither).
    pub attr: String,
    pub exclude_from_bom: bool,
    pub exclude_from_pos: bool,
    /// Every `(pad ..)` in file order.
    pub pads: Vec<PadDetail>,
    /// The smallest distance between the centres of two numbered copper pads at different places, nanometres rounded to the micrometre; 0 for fewer than two.
    pub pitch_nm: i64,
    /// The box of the `F.CrtYd` graphics, and of the `B.CrtYd` ones (lines, rectangles, circles, arcs and polygons; an arc by its true extent).
    pub courtyard_front: Option<BoxNm>,
    pub courtyard_back: Option<BoxNm>,
    /// A courtyard has an arc (whose extent [`crate::footprint_lib`]'s box does not follow).
    pub courtyard_has_arc: bool,
    /// Every `(model "path" ..)`, in file order.
    pub models: Vec<String>,
}

impl FootprintDetail {
    /// The courtyard the parts cache keeps: the front's, else the back's.
    pub fn courtyard(&self) -> Option<BoxNm> {
        self.courtyard_front.or(self.courtyard_back)
    }

    pub fn count_of(&self, pad_type: &str) -> u32 {
        self.pads.iter().filter(|p| p.pad_type == pad_type).count() as u32
    }
}

/// The forms directly inside the top-level `(footprint ..)` of `.kicad_mod` text: each one's tag and byte range (parentheses included). `None` when the
/// text has no `(footprint ..)` (or `(module ..)`) at its top.
fn top_forms(text: &str) -> Option<Vec<(&str, std::ops::Range<usize>)>> {
    let b = text.as_bytes();
    let (mut i, mut depth) = (0usize, 0usize);
    let mut seen = false;
    let mut out = Vec::new();
    let mut open: Option<(usize, usize, usize)> = None; // start, tag start, tag end
    while i < b.len() {
        match b[i] {
            b'"' => i = string_end(b, i),
            b'(' => {
                let tag_end = word_end(b, i + 1);
                if depth == 0 {
                    if matches!(&b[i + 1..tag_end], b"footprint" | b"module") {
                        seen = true;
                    }
                } else if depth == 1 {
                    open = Some((i, i + 1, tag_end));
                }
                depth += 1;
                i = tag_end.max(i + 1);
            }
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 1 {
                    if let Some((start, t0, t1)) = open.take() {
                        out.push((&text[t0..t1], start..i + 1));
                    }
                }
                i += 1;
                if depth == 0 && seen {
                    break;
                }
            }
            _ => i += 1,
        }
    }
    seen.then_some(out)
}

fn grow(bx: &mut Option<BoxNm>, x: i64, y: i64) {
    *bx = Some(match *bx {
        None => (x, y, x, y),
        Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
    });
}

fn nm(mm: f64) -> i64 {
    eda_model::units::from_mm(mm).get()
}

/// The box of the arc from `s` through `m` to `e` (millimetres): its ends, and each axis extreme of its circle that the arc passes.
fn arc_box(s: (f64, f64), m: (f64, f64), e: (f64, f64), bx: &mut Option<BoxNm>) {
    for (x, y) in [s, m, e] {
        grow(bx, nm(x), nm(y));
    }
    // the circle through the three points
    let d = 2.0 * (s.0 * (m.1 - e.1) + m.0 * (e.1 - s.1) + e.0 * (s.1 - m.1));
    if d.abs() < 1e-12 {
        return;
    }
    let sq = |p: (f64, f64)| p.0 * p.0 + p.1 * p.1;
    let cx = (sq(s) * (m.1 - e.1) + sq(m) * (e.1 - s.1) + sq(e) * (s.1 - m.1)) / d;
    let cy = (sq(s) * (e.0 - m.0) + sq(m) * (s.0 - e.0) + sq(e) * (m.0 - s.0)) / d;
    let r = ((s.0 - cx).powi(2) + (s.1 - cy).powi(2)).sqrt();
    let ang = |p: (f64, f64)| (p.1 - cy).atan2(p.0 - cx);
    let (a0, am, a1) = (ang(s), ang(m), ang(e));
    // the sweep from a0 to a1 that passes am, counter-clockwise or clockwise
    let tau = std::f64::consts::TAU;
    let ccw = |from: f64, to: f64| (to - from).rem_euclid(tau);
    let (start, sweep) = if ccw(a0, am) <= ccw(a0, a1) { (a0, ccw(a0, a1)) } else { (a1, ccw(a1, a0)) };
    for k in 0..4 {
        let a = k as f64 * std::f64::consts::FRAC_PI_2;
        if ccw(start, a) <= sweep {
            grow(bx, nm(cx + r * a.cos()), nm(cy + r * a.sin()));
        }
    }
}

fn point(list: &[crate::sexpr::Sexpr], tag: &str) -> Option<(f64, f64)> {
    let p = crate::sexpr::find(list, tag)?;
    Some((crate::sexpr::num(p, 1)?, crate::sexpr::num(p, 2)?))
}

/// What the parts cache keeps of `.kicad_mod` text ([`FootprintDetail`]), or `None` when it has no `(footprint ..)` form at its top. The chooser's summary is
/// [`scan_footprint`]'s own; the rest reads only the forms it needs -- `(attr ..)`, each `(pad ..)`, the courtyard layers' graphics and the `(model ..)`s -- each
/// parsed alone, never the silkscreen, the fabrication drawing or the texts. The pad fields as `PCB_IO_KICAD_SEXPR_PARSER::parsePAD` reads them (`(at x y rot)`,
/// `(size w h)`, `(drill [oval] d ..)`), the attributes as `parseFOOTPRINT_unchecked`'s `T_attr`.
pub fn scan_footprint_detail(text: &str) -> Option<FootprintDetail> {
    use crate::sexpr::{find, find_all, num, parse, txt, Sexpr};
    let summary = scan_footprint(text)?;
    let forms = top_forms(text)?;
    let mut d = FootprintDetail { summary, attr: "unspecified".into(), ..Default::default() };
    for (tag, range) in forms {
        let form = &text[range];
        match tag {
            "attr" => {
                let Ok(Sexpr::List(l)) = parse(form) else { continue };
                for tok in l.iter().skip(1).filter_map(Sexpr::text) {
                    match tok {
                        "smd" => d.attr = "smd".into(),
                        "through_hole" => d.attr = "through_hole".into(),
                        "exclude_from_bom" => d.exclude_from_bom = true,
                        "exclude_from_pos_files" => d.exclude_from_pos = true,
                        _ => {}
                    }
                }
            }
            "pad" => {
                let Ok(Sexpr::List(l)) = parse(form) else { continue };
                let (x, y, rot) = find(&l, "at").map_or((0.0, 0.0, 0.0), |a| (num(a, 1).unwrap_or(0.0), num(a, 2).unwrap_or(0.0), num(a, 3).unwrap_or(0.0)));
                let (w, h) = find(&l, "size").map_or((0.0, 0.0), |s| (num(s, 1).unwrap_or(0.0), num(s, 2).unwrap_or(0.0)));
                let drill = find(&l, "drill").map_or(0.0, |dr| {
                    let toks: Vec<&str> = dr.iter().skip(1).filter_map(Sexpr::text).collect();
                    toks.iter().find_map(|t| t.parse::<f64>().ok()).unwrap_or(0.0)
                });
                let copper = find(&l, "layers").is_some_and(|ls| ls.iter().skip(1).filter_map(Sexpr::text).any(|n| n.ends_with(".Cu")));
                d.pads.push(PadDetail {
                    number: txt(&l, 1).unwrap_or("").to_string(),
                    pad_type: txt(&l, 2).unwrap_or("").to_string(),
                    shape: txt(&l, 3).unwrap_or("").to_string(),
                    x: nm(x),
                    y: nm(y),
                    rot,
                    w: nm(w),
                    h: nm(h),
                    drill: nm(drill),
                    copper,
                });
            }
            "fp_line" | "fp_rect" | "fp_circle" | "fp_arc" | "fp_poly" if form.contains("CrtYd") => {
                let Ok(Sexpr::List(l)) = parse(form) else { continue };
                let target = match find(&l, "layer").and_then(|ly| txt(ly, 1)) {
                    Some("F.CrtYd") => &mut d.courtyard_front,
                    Some("B.CrtYd") => &mut d.courtyard_back,
                    _ => continue,
                };
                match tag {
                    "fp_line" | "fp_rect" => {
                        for p in [point(&l, "start"), point(&l, "end")].into_iter().flatten() {
                            grow(target, nm(p.0), nm(p.1));
                        }
                    }
                    "fp_circle" => {
                        if let (Some(c), Some(e)) = (point(&l, "center"), point(&l, "end")) {
                            let r = ((e.0 - c.0).powi(2) + (e.1 - c.1).powi(2)).sqrt();
                            grow(target, nm(c.0 - r), nm(c.1 - r));
                            grow(target, nm(c.0 + r), nm(c.1 + r));
                        }
                    }
                    "fp_arc" => {
                        if let (Some(s), Some(m), Some(e)) = (point(&l, "start"), point(&l, "mid"), point(&l, "end")) {
                            arc_box(s, m, e, target);
                            d.courtyard_has_arc = true;
                        }
                    }
                    _ => {
                        if let Some(pts) = find(&l, "pts") {
                            for xy in find_all(pts, "xy") {
                                if let (Some(x), Some(y)) = (num(xy, 1), num(xy, 2)) {
                                    grow(target, nm(x), nm(y));
                                }
                            }
                            if find_all(pts, "arc").next().is_some() {
                                d.courtyard_has_arc = true;
                            }
                        }
                    }
                }
            }
            "model" => {
                let Ok(Sexpr::List(l)) = parse(form) else { continue };
                if let Some(path) = txt(&l, 1) {
                    d.models.push(path.to_string());
                }
            }
            _ => {}
        }
    }
    // the pitch: the nearest two numbered copper pads (a pad stacked on another is not a pitch)
    let centres: Vec<(i64, i64)> = d.pads.iter().filter(|p| p.copper && p.pad_type != "np_thru_hole" && is_numbered_pad(&p.number)).map(|p| (p.x, p.y)).collect();
    let mut best: Option<f64> = None;
    for (i, a) in centres.iter().enumerate() {
        for b in &centres[i + 1..] {
            let dist = (((a.0 - b.0) as f64).powi(2) + ((a.1 - b.1) as f64).powi(2)).sqrt();
            if dist >= 1000.0 && best.is_none_or(|v| dist < v) {
                best = Some(dist);
            }
        }
    }
    d.pitch_nm = best.map_or(0, |v| (v / 1000.0).round() as i64 * 1000);
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FP: &str = r##"(footprint "SOIC-8_3.9x4.9mm_P1.27mm" (version 20260206) (generator "kicad-footprint-generator") (layer "F.Cu")
        (descr "SOIC, 8 Pin (JEDEC MS-012AA, https://www.analog.com/media/en/package-pcb-resources/package/pkg_pdf/soic_narrow-r/r_8.pdf), generated with kicad-footprint-generator ipc_gullwing_generator.py")
        (tags "SOIC SO")
        (property "Reference" "REF**" (at 0 -3.4 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))
        (property "Value" "SOIC-8" (at 0 3.4 0) (layer "F.Fab"))
        (attr smd)
        (fp_line (start 0 0) (end 1 1) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
        (pad "1" smd roundrect (at -2.475 -1.905) (size 1.95 0.6) (layers "F.Cu" "F.Mask" "F.Paste") (roundrect_rratio 0.25))
        (pad "2" smd roundrect (at -2.475 -0.635) (size 1.95 0.6) (layers "F.Cu" "F.Mask" "F.Paste"))
        (pad "3" smd roundrect (at -2.475 0.635) (size 1.95 0.6) (layers "F.Cu" "F.Mask" "F.Paste"))
        (pad "4" smd roundrect (at -2.475 1.905) (size 1.95 0.6) (layers "F.Cu" "F.Mask" "F.Paste"))
        (pad "EP" smd rect (at 0 0) (size 2 2) (layers "F.Cu" "F.Mask"))
        (pad "" np_thru_hole circle (at 0 0) (size 1 1) (drill 1) (layers "*.Cu" "*.Mask"))
        (pad "5" np_thru_hole circle (at 1 1) (size 1 1) (drill 1) (layers "*.Cu" "*.Mask"))
        (pad "6" smd rect (at 2 2) (size 1 1) (layers "F.Mask"))
        (pad "A12" thru_hole circle (at 3 3) (size 1 1) (drill 0.5) (layers "*.Cu" "*.Mask"))
        (pad "ABC1" smd rect (at 3 3) (size 1 1) (layers "F.Cu"))
        (pad "1" smd roundrect (at 4 4) (size 1 1) (layers "B.Cu")))"##;

    #[test]
    fn the_description_tags_and_numbered_pads_are_read() {
        let s = scan_footprint(FP).unwrap();
        assert!(s.description.starts_with("SOIC, 8 Pin (JEDEC MS-012AA, https://"), "{}", s.description);
        assert_eq!(s.tags, "SOIC SO");
        assert_eq!(s.pads, 11, "every (pad ..) of the file");
        // 1 2 3 4 (copper SMD) and A12 (copper THT): EP is not a number, the NPTHs are holes, pad 6 is mask only, ABC1 has three letters
        assert_eq!(s.numbered_pads, 5);
    }

    #[test]
    fn the_numbered_pad_rule_is_kicads() {
        for yes in ["1", "42", "A1", "AB10", "b3"] {
            assert!(is_numbered_pad(yes), "{yes}");
        }
        for no in ["", "MP", "GND", "ABC1", "1A", "A", "A-1", "EP"] {
            assert!(!is_numbered_pad(no), "{no:?}");
        }
    }

    #[test]
    fn a_field_description_is_used_when_there_is_no_descr() {
        let s = scan_footprint(r#"(footprint "X" (property "Description" "From a field") (tags "a b"))"#).unwrap();
        assert_eq!((s.description.as_str(), s.tags.as_str()), ("From a field", "a b"));
        let both = scan_footprint(r#"(footprint "X" (descr "The descr") (property "Description" "From a field"))"#).unwrap();
        assert_eq!(both.description, "The descr");
    }

    #[test]
    fn something_that_is_not_a_footprint_is_none_and_junk_is_survived() {
        assert!(scan_footprint("").is_none());
        assert!(scan_footprint("(kicad_symbol_lib)").is_none());
        let cut = scan_footprint("(footprint \"X\" (descr \"unterminated").unwrap();
        assert_eq!(cut.description, "");
        assert!(scan_footprint("(footprint \"X\" (pad \"1\" smd rect (layers \"F.Cu").is_some());
    }

    /// The real libraries, when KiCad.app is installed.
    #[test]
    fn installed_footprints_scan() {
        let root = crate::default_footprint_library_root();
        let path = root.join("Package_SO.pretty").join("SOIC-8_3.9x4.9mm_P1.27mm.kicad_mod");
        let Ok(text) = std::fs::read_to_string(&path) else {
            eprintln!("no KiCad footprint libraries at {}; skipping", root.display());
            return;
        };
        let s = scan_footprint(&text).unwrap();
        assert_eq!(s.numbered_pads, 8, "an SOIC-8 has pads 1 to 8");
        assert!(s.description.contains("SOIC"), "{}", s.description);
        let hole = scan_footprint(&std::fs::read_to_string(root.join("MountingHole.pretty").join("MountingHole_3.2mm_M3.kicad_mod")).unwrap()).unwrap();
        assert_eq!((hole.pads, hole.numbered_pads), (1, 0), "a mounting hole is one NPTH, no pin");
        assert!(hole.tags.contains("mountinghole"), "{}", hole.tags);
    }

    #[test]
    fn the_detail_reads_pads_attributes_pitch_courtyard_and_models() {
        let d = scan_footprint_detail(FP).unwrap();
        assert_eq!(d.summary, scan_footprint(FP).unwrap(), "the chooser's summary is the summary's own");
        assert_eq!(d.attr, "smd");
        assert!(!d.exclude_from_bom && !d.exclude_from_pos);
        assert_eq!(d.pads.len(), 11);
        let p1 = &d.pads[0];
        assert_eq!((p1.number.as_str(), p1.pad_type.as_str(), p1.shape.as_str()), ("1", "smd", "roundrect"));
        assert_eq!((p1.x, p1.y, p1.w, p1.h, p1.drill), (-2_475_000, -1_905_000, 1_950_000, 600_000, 0));
        assert!(p1.copper);
        assert!(!d.pads[9].copper || d.pads[9].number == "ABC1");
        assert_eq!((d.count_of("smd"), d.count_of("thru_hole"), d.count_of("np_thru_hole")), (8, 1, 2));
        assert_eq!(d.pads[8].drill, 500_000);
        // pads 1..4 are 1.27 mm apart; pad "1" at (4, 4) on B.Cu is no nearer to anything
        assert_eq!(d.pitch_nm, 1_270_000);
        assert_eq!(d.courtyard(), None, "no courtyard drawn");
        assert!(d.models.is_empty());
    }

    #[test]
    fn the_detail_courtyard_is_the_fronts_box_an_arc_by_its_extent_and_models_are_listed() {
        let text = r#"(footprint "X" (attr through_hole exclude_from_pos_files exclude_from_bom)
            (fp_line (start -1 -2) (end 3 -2) (stroke (width 0.05) (type solid)) (layer "F.CrtYd"))
            (fp_rect (start -1 -2) (end 3 4) (layer "F.CrtYd"))
            (fp_circle (center 0 0) (end 9 0) (layer "B.CrtYd"))
            (fp_arc (start -1 4) (mid 1 6) (end 3 4) (layer "F.CrtYd"))
            (fp_line (start -50 -50) (end 50 50) (layer "F.SilkS"))
            (pad "1" thru_hole circle (at 0 0) (size 1.6 1.6) (drill 0.8) (layers "*.Cu" "*.Mask"))
            (pad "2" thru_hole oval (at 2.54 0 90) (size 1.6 2) (drill oval 0.8 1.2) (layers "*.Cu" "*.Mask"))
            (model "${KICAD10_3DMODEL_DIR}/X.3dshapes/X.wrl" (offset (xyz 0 0 0)))
            (model "/abs/Y.step"))"#;
        let d = scan_footprint_detail(text).unwrap();
        assert_eq!(d.attr, "through_hole");
        assert!(d.exclude_from_bom && d.exclude_from_pos);
        // the arc's top reaches y = 6 (its circle has centre (1, 4) and radius sqrt(4 + 0) = 2... through (1, 6))
        assert_eq!(d.courtyard_front, Some((-1_000_000, -2_000_000, 3_000_000, 6_000_000)));
        assert!(d.courtyard_has_arc);
        assert_eq!(d.courtyard_back, Some((-9_000_000, -9_000_000, 9_000_000, 9_000_000)));
        assert_eq!(d.courtyard(), d.courtyard_front, "the front wins");
        assert_eq!(d.pitch_nm, 2_540_000);
        assert_eq!((d.pads[1].rot, d.pads[1].drill), (90.0, 800_000));
        assert_eq!(d.models, ["${KICAD10_3DMODEL_DIR}/X.3dshapes/X.wrl", "/abs/Y.step"]);
        assert!(scan_footprint_detail("(kicad_symbol_lib)").is_none());
        assert_eq!(scan_footprint_detail("(footprint \"Y\")").unwrap().attr, "unspecified");
    }

    /// Slow tier (`EDA_SLOW_TESTS=1`): every installed footprint's detail agrees with the full reader's (`parse_library_footprint`): the description, the
    /// attributes, every pad's number, type, place, size and drill, the 3D models, and the courtyard box (`footprint_lib::courtyard_from`'s half extents, which
    /// take both sides and leave arcs out).
    #[test]
    fn slow_every_installed_footprint_detail_agrees_with_the_full_reader() {
        if std::env::var_os("EDA_SLOW_TESTS").is_none() {
            return;
        }
        let root = crate::default_footprint_library_root();
        let Ok(libs) = std::fs::read_dir(&root) else {
            eprintln!("no footprint libraries at {}; skipping", root.display());
            return;
        };
        let started = std::time::Instant::now();
        let (mut files, mut compared_courtyards, mut unparsed) = (0usize, 0usize, 0usize);
        let mut failures: Vec<String> = Vec::new();
        for lib in libs.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pretty")) {
            for file in std::fs::read_dir(&lib).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "kicad_mod")) {
                let text = std::fs::read_to_string(&file).unwrap();
                let Ok(full) = crate::parse_library_footprint(&text) else {
                    unparsed += 1;
                    continue;
                };
                let fp = full.footprint;
                files += 1;
                let d = scan_footprint_detail(&text).unwrap_or_else(|| panic!("{} scans", file.display()));
                let mut why: Vec<String> = Vec::new();
                if d.summary.description != fp.description {
                    why.push(format!("description {:?} vs {:?}", d.summary.description, fp.description));
                }
                if d.summary.tags != fp.keywords {
                    why.push(format!("tags {:?} vs {:?}", d.summary.tags, fp.keywords));
                }
                if (d.attr == "smd", d.attr == "through_hole", d.exclude_from_bom, d.exclude_from_pos) != (fp.attributes.smd, fp.attributes.through_hole, fp.attributes.exclude_from_bom, fp.attributes.exclude_from_position_files) {
                    why.push(format!("attributes {} {} {} vs {:?}", d.attr, d.exclude_from_bom, d.exclude_from_pos, fp.attributes));
                }
                let mut ours: Vec<(String, i64, i64, i64, i64)> = d.pads.iter().map(|p| (p.number.clone(), p.x, p.y, p.w, p.h)).collect();
                let mut theirs: Vec<(String, i64, i64, i64, i64)> = fp.pads.iter().map(|p| (p.number.clone(), p.at.x.get(), p.at.y.get(), p.size.0.get(), p.size.1.get())).collect();
                ours.sort();
                theirs.sort();
                if ours != theirs {
                    why.push(format!("pads {} vs {}", ours.len(), theirs.len()));
                }
                let kinds = |k: eda_model::footprint::PadKind| fp.pads.iter().filter(|p| p.kind == k).count() as u32;
                use eda_model::footprint::PadKind;
                if (d.count_of("smd") + d.count_of("connect"), d.count_of("thru_hole"), d.count_of("np_thru_hole")) != (kinds(PadKind::Smd), kinds(PadKind::ThroughHole), kinds(PadKind::NonPlatedHole)) {
                    why.push("pad types".into());
                }
                let mut drills: Vec<i64> = d.pads.iter().map(|p| p.drill).filter(|&v| v > 0).collect();
                let mut their_drills: Vec<i64> = fp.pads.iter().filter_map(|p| p.drill.map(|v| v.get()).or(p.drill_slot.map(|s| s.0.get()))).filter(|&v| v > 0).collect();
                drills.sort();
                their_drills.sort();
                if drills != their_drills {
                    why.push(format!("drills {drills:?} vs {their_drills:?}"));
                }
                let their_models: Vec<String> = if fp.models3d.is_empty() { fp.model.iter().cloned().collect() } else { fp.models3d.iter().map(|m| m.path.clone()).collect() };
                if d.models != their_models {
                    why.push(format!("models {:?} vs {their_models:?}", d.models));
                }
                if !d.courtyard_has_arc {
                    let mut both = d.courtyard_front;
                    if let Some((x0, y0, x1, y1)) = d.courtyard_back {
                        grow(&mut both, x0, y0);
                        grow(&mut both, x1, y1);
                    }
                    let half = both.map(|(x0, y0, x1, y1)| (x0.abs().max(x1.abs()).max(1), y0.abs().max(y1.abs()).max(1)));
                    let theirs = fp.courtyard.map(|(w, h)| (w.get(), h.get()));
                    if half != theirs {
                        why.push(format!("courtyard {half:?} vs {theirs:?}"));
                    }
                    compared_courtyards += 1;
                }
                if !why.is_empty() {
                    failures.push(format!("{}: {}", file.display(), why.join("; ")));
                }
            }
        }
        eprintln!("{files} footprints compared ({compared_courtyards} courtyards, {unparsed} the full reader refuses) in {:?}", started.elapsed());
        assert!(files > 10_000, "{files}");
        assert!(failures.is_empty(), "{} differ:\n{}", failures.len(), failures.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
    }
}
