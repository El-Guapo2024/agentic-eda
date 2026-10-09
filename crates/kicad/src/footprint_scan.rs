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
}
