//! A board's reference images (`(image ..)`, KiCad's `PCB_REFERENCE_IMAGE`) read and written: `PCB_IO_KICAD_SEXPR_PARSER::parsePCB_REFERENCE_IMAGE` and
//! `PCB_IO_KICAD_SEXPR::format( const PCB_REFERENCE_IMAGE* )`, for [`eda_model::pcb_items::RefImage`].
//!
//! The pixels are the file inside `(data "..." "...")`, base64 in lines of 76 characters. They are kept as that text without its line breaks and written back as it was: KiCad
//! itself writes the bytes it read (`BITMAP_BASE::SaveImageData` writes `m_imageData` while the image is untouched), so a board that goes through the studio carries the same file.
//! A footprint's own `(image ..)` (a reference image inside a footprint, never drawn on the board) is part of the footprint (`FootprintExtra::tokens`).

use std::fmt::Write as _;

use eda_model::image_info::{self, SCALE_PPM};
use eda_model::ir::Point;
use eda_model::pcb_items::RefImage;

use crate::sexpr::{self, Sexpr};
use crate::{mm, mm_to_um, sexpr_str};

/// One `(image ..)` of a board; `None` when it has no position or its data is not an image of a kind known (KiCad stops reading the board at "Failed to read image data").
pub(crate) fn read(item: &[Sexpr]) -> Option<RefImage> {
    let at = sexpr::find(item, "at")?;
    let at = Point { x: mm_to_um(sexpr::num(at, 1)?), y: mm_to_um(sexpr::num(at, 2)?) };
    let data: String = sexpr::find(item, "data").map(|d| d.iter().skip(1).filter_map(Sexpr::text).collect()).unwrap_or_default();
    let data = image_info::compact(&data);
    image_info::header_of_base64(&data)?;
    // `refImage.SetImageScale( parseDouble() ); if( !std::isnormal( .. ) ) refImage.SetImageScale( 1.0 )`, and `SetImageScale` ignores a scale that is not positive.
    let scale = sexpr::find(item, "scale").and_then(|s| sexpr::num(s, 1)).filter(|v| v.is_normal() && *v > 0.0).unwrap_or(1.0);
    let layer = sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("F.Cu").to_string();
    Some(RefImage { id: String::new(), layer, at, scale_ppm: ((scale * SCALE_PPM as f64).round() as i64).max(1), data })
}

/// Every `(image ..)` of a board, with the uuid and the lock of each in `refs`.
pub(crate) fn read_all(root: &[Sexpr], refs: &mut Vec<crate::import_items::ItemRef>) -> Vec<RefImage> {
    let mut out = Vec::new();
    for item in sexpr::find_all(root, "image") {
        if let Some(img) = read(item) {
            out.push(img);
            refs.push(crate::import_items::ItemRef::of(item));
        }
    }
    out
}

/// One reference image as `format( const PCB_REFERENCE_IMAGE* )` writes it; returns its uuid.
pub(crate) fn write(out: &mut String, img: &RefImage, locked: bool) -> String {
    let uuid = crate::duid_for(&format!("refimage:{}", img.id), &img.id);
    write!(out, "\t(image\n\t\t(at {} {}) (layer {})", mm(img.at.x), mm(img.at.y), sexpr_str(&img.layer)).unwrap();
    if img.scale_ppm != SCALE_PPM {
        write!(out, " (scale {})", crate::sch_extras_io::scale_str(img.scale_ppm)).unwrap();
    }
    if locked {
        write!(out, " (locked yes)").unwrap();
    }
    writeln!(out, "\n{}\t\t(uuid \"{uuid}\")\n\t)", crate::sch_extras_io::data_str(&img.data, "\t\t")).unwrap();
    uuid
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_text(layer: &str, extra: &str) -> (String, String) {
        let data = image_info::base64_encode(&image_info::solid_png(60, 40, [20, 120, 220], 254));
        let lines: Vec<String> = data.as_bytes().chunks(76).map(|c| format!("\"{}\"", std::str::from_utf8(c).unwrap())).collect();
        (format!("(image (at 100 50) (layer \"{layer}\") {extra} (data {}) (uuid \"11111111-1111-1111-1111-111111111111\"))", lines.join("\n")), data)
    }

    #[test]
    fn an_image_is_read_as_parse_pcb_reference_image_reads_it() {
        let (text, data) = image_text("Cmts.User", "(scale 0.5) (locked yes)");
        let tree = sexpr::parse(&format!("(kicad_pcb {text})")).unwrap();
        let mut refs = Vec::new();
        let images = read_all(tree.as_list().unwrap(), &mut refs);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].layer, "Cmts.User");
        assert_eq!(images[0].at, Point { x: 100_000, y: 50_000 });
        assert_eq!(images[0].scale_ppm, 500_000);
        assert_eq!(images[0].data, data, "the data comes back as one text");
        assert_eq!(images[0].size_um(), (3_000, 2_000), "60 x 40 px at 254 ppi at half scale");
        assert!(refs[0].locked);
        assert_eq!(refs[0].uuid.as_deref(), Some("11111111-1111-1111-1111-111111111111"));
    }

    #[test]
    fn a_scale_that_is_not_a_positive_number_is_one_and_data_that_is_no_image_is_left_out() {
        for scale in ["(scale 0)", "(scale -2)", "(scale nan)", ""] {
            let (text, _) = image_text("F.SilkS", scale);
            let tree = sexpr::parse(&format!("(kicad_pcb {text})")).unwrap();
            let images = read_all(tree.as_list().unwrap(), &mut Vec::new());
            assert_eq!(images[0].scale_ppm, SCALE_PPM, "{scale}");
        }
        let tree = sexpr::parse("(kicad_pcb (image (at 1 1) (layer \"F.SilkS\") (data \"AAAAAAAAAAAAAAAAAAAAAAAA\")) (image (layer \"F.SilkS\")))").unwrap();
        assert!(read_all(tree.as_list().unwrap(), &mut Vec::new()).is_empty());
    }

    #[test]
    fn an_image_is_written_with_its_data_in_lines_of_76_characters_and_read_back_the_same() {
        let (text, _) = image_text("Dwgs.User", "(scale 2.25)");
        let tree = sexpr::parse(&format!("(kicad_pcb {text})")).unwrap();
        let img = read_all(tree.as_list().unwrap(), &mut Vec::new()).remove(0);
        let mut out = String::new();
        let uuid = write(&mut out, &img, true);
        assert!(out.starts_with("\t(image\n\t\t(at 100 50) (layer \"Dwgs.User\") (scale 2.25) (locked yes)\n"), "{out}");
        assert!(out.contains(&format!("(uuid \"{uuid}\")")));
        let data_lines: Vec<&str> = out.lines().filter(|l| l.trim_start().starts_with('"') && l.trim_end().ends_with('"')).collect();
        assert!(data_lines.len() >= 2 && data_lines.iter().all(|l| l.trim().len() <= 78), "{out}");
        let back = read_all(sexpr::parse(&format!("(kicad_pcb {out})")).unwrap().as_list().unwrap(), &mut Vec::new());
        assert_eq!(back, vec![RefImage { id: String::new(), ..img.clone() }]);
        // scale one is not written
        let mut unit = img;
        unit.scale_ppm = SCALE_PPM;
        let mut out = String::new();
        write(&mut out, &unit, false);
        assert!(!out.contains("(scale") && !out.contains("(locked"), "{out}");
    }
}
