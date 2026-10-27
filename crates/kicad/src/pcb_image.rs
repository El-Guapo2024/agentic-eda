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
use crate::{mm, mm_to_nm, sexpr_str};

/// One `(image ..)` of a board; `None` when it has no position or its data is not an image of a kind known (KiCad stops reading the board at "Failed to read image data").
pub(crate) fn read(item: &[Sexpr]) -> Option<RefImage> {
    let at = sexpr::find(item, "at")?;
    let at = Point { x: mm_to_nm(sexpr::num(at, 1)?), y: mm_to_nm(sexpr::num(at, 2)?) };
    let data: String = sexpr::find(item, "data").map(|d| d.iter().skip(1).filter_map(Sexpr::text).collect()).unwrap_or_default();
    let data = image_info::compact(&data);
    image_info::header_of_base64(&data)?;
    // `refImage.SetImageScale( parseDouble() ); if( !std::isnormal( .. ) ) refImage.SetImageScale( 1.0 )`, and `SetImageScale` ignores a scale that is not positive.
    let scale = sexpr::find(item, "scale").and_then(|s| sexpr::num(s, 1)).filter(|v| v.is_normal() && *v > 0.0).unwrap_or(1.0);
    let layer = sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("F.Cu").to_string();
    Some(RefImage { id: String::new(), layer, at, scale_ppm: ((scale * SCALE_PPM as f64).round() as i64).max(1), data })
}

/// The first board file version whose image scale is for the image's own definition (`SEXPR_BOARD_FILE_VERSION` 20260623, "Migrate reference image scale for PNG
/// pixel-density fix"). The board writer writes an older version (`pcb.rs`), so it writes the scale that version means ([`file_scale`]).
pub const FIRST_TRUE_PPI_SCALE: i64 = 20260623;

/// The scale an image has, as KiCad holds it, from the scale `scale_ppm` a file of `file_version` says: before [`FIRST_TRUE_PPI_SCALE`], the stored scale made up for a PNG's
/// definition read short (`BITMAP_BASE::GetLegacyPPI`), and `parsePCB_REFERENCE_IMAGE` scales it by `GetPPI() / legacyPPI` "to preserve the rendered size of existing boards".
pub(crate) fn scale_from_file(scale_ppm: i64, data: &str, file_version: i64) -> i64 {
    match image_info::header_of_base64(data) {
        Some(h) if file_version < FIRST_TRUE_PPI_SCALE && h.legacy_ppi > 0 && h.ppi != h.legacy_ppi => ((scale_ppm as f64 * h.ppi as f64 / h.legacy_ppi as f64).round() as i64).max(1),
        _ => scale_ppm,
    }
}

/// The `(scale ..)` to write for an image KiCad holds at `scale_ppm` into a file older than [`FIRST_TRUE_PPI_SCALE`], `None` for a scale of one: the inverse of
/// [`scale_from_file`], so that KiCad's rescaling gives the image back its size. The scale is written with the fewest decimals (up to four) that read back as `scale_ppm`, which
/// gives an older file's own scale back as it was (its `1` came in as 1.021277 and goes out as `1`), and otherwise in full, so that KiCad's product is the scale itself (a
/// newer file's `1` goes out as 0.9791666666666666 and is 1 again in KiCad, which writes no scale for it: `if( GetImageScale() != 1.0 )`).
pub(crate) fn file_scale(scale_ppm: i64, data: &str) -> Option<String> {
    let header = image_info::header_of_base64(data).filter(|h| h.legacy_ppi > 0 && h.ppi != h.legacy_ppi);
    let Some(h) = header else { return (scale_ppm != SCALE_PPM).then(|| crate::sch_extras_io::scale_str(scale_ppm)) };
    let (ppi, legacy) = (h.ppi as f64, h.legacy_ppi as f64);
    let x = scale_ppm as f64 / SCALE_PPM as f64 * legacy / ppi;
    for decimals in 0..=4 {
        let k = 10f64.powi(decimals);
        let v = (x * k).round() / k;
        if v > 0.0 && (v * ppi / legacy * SCALE_PPM as f64).round() as i64 == scale_ppm {
            return Some(crate::sch_extras_io::scale_str((v * SCALE_PPM as f64).round() as i64));
        }
    }
    Some(format!("{x}"))
}

/// Every `(image ..)` of a board, with the uuid and the lock of each in `refs`.
pub(crate) fn read_all(root: &[Sexpr], refs: &mut Vec<crate::import_items::ItemRef>) -> Vec<RefImage> {
    let mut out = Vec::new();
    let file_version = sexpr::find(root, "version").and_then(|v| sexpr::num(v, 1)).unwrap_or(0.0) as i64;
    for item in sexpr::find_all(root, "image") {
        if let Some(mut img) = read(item) {
            img.scale_ppm = scale_from_file(img.scale_ppm, &img.data, file_version);
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
    if let Some(scale) = file_scale(img.scale_ppm, &img.data) {
        write!(out, " (scale {scale})").unwrap();
    }
    if locked {
        write!(out, " (locked yes)").unwrap();
    }
    writeln!(out, "\n{}\t\t(uuid \"{uuid}\")\n\t)", crate::sch_extras_io::data_str(&img.data, "\t\t")).unwrap();
    uuid
}

#[cfg(test)]
mod tests {
    use eda_model::units::Nm;
    use super::*;

    /// F1 of round 2: a 96 dpi PNG (37.8 pixels per centimetre) was 94 dpi to KiCad before 20260623, and `parsePCB_REFERENCE_IMAGE` scales an older file's image by 96 / 94.
    /// The board is written as 20241229, so the scale written is the one that comes back as the image's.
    #[test]
    fn the_scale_of_a_png_follows_the_file_version_both_ways() {
        let data = image_info::base64_encode(&image_info::solid_png(40, 40, [0, 0, 0], 96));
        // an older file's 1 is KiCad's 96 / 94 and is written back as 1
        let old = scale_from_file(SCALE_PPM, &data, 20241229);
        assert_eq!(old, 1_021_277);
        assert_eq!(file_scale(old, &data).as_deref(), Some("1"));
        // a newer file's 1 is 1, written as what KiCad turns back into 1
        assert_eq!(scale_from_file(SCALE_PPM, &data, 20261017), SCALE_PPM);
        let text = file_scale(SCALE_PPM, &data).expect("a scale");
        let v: f64 = text.parse().unwrap();
        assert_eq!(v * 96.0 / 94.0, 1.0, "{text}");
        // an image whose definition KiCad always read right keeps its scale as it is
        let png300 = image_info::base64_encode(&image_info::solid_png(4, 4, [0, 0, 0], 300));
        assert_eq!(scale_from_file(SCALE_PPM, &png300, 20241229), SCALE_PPM);
        assert_eq!(file_scale(500_000, &png300).as_deref(), Some("0.5"));
        assert_eq!(file_scale(SCALE_PPM, &png300), None);
    }

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
        assert_eq!(images[0].at, Point { x: Nm::from_mm(100.0), y: Nm::from_mm(50.0) });
        assert_eq!(images[0].scale_ppm, 500_000);
        assert_eq!(images[0].data, data, "the data comes back as one text");
        assert_eq!(images[0].size_nm(), (Nm::from_mm(3.0), Nm::from_mm(2.0)), "60 x 40 px at 254 ppi at half scale");
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
