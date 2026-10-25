//! A board's barcodes (`(barcode ..)`, KiCad's `PCB_BARCODE`) read and written: `PCB_IO_KICAD_SEXPR_PARSER::parsePCB_BARCODE` and
//! `PCB_IO_KICAD_SEXPR::format( const PCB_BARCODE* )`, for [`eda_model::barcode::Barcode`].

use eda_model::units::Nm;
use std::fmt::Write as _;

use eda_model::barcode::{Barcode, BarcodeKind, EccLevel};
use eda_model::ir::Point;

use crate::sexpr::{self, Sexpr};
use crate::{fmt_mm_f, mm, mm_to_nm, sexpr_str};

/// `PCB_BARCODE`'s own defaults: 40 mm square, a QR code of error correction level L on `Dwgs.User`, the text printed.
const DEFAULT_SIZE_NM: i64 = 40_000_000;
/// The height of a `PCB_TEXT` that nothing sized (1.5 mm).
const DEFAULT_TEXT_HEIGHT_NM: i64 = 1_500_000;

fn yes(item: &[Sexpr], name: &str) -> Option<bool> {
    sexpr::find(item, name).map(|l| sexpr::txt(l, 1).map_or(true, |v| v != "no"))
}

/// One `(barcode ..)`; `None` when it has no position or a kind KiCad does not know.
pub(crate) fn read(item: &[Sexpr]) -> Option<Barcode> {
    let at = sexpr::find(item, "at")?;
    let (x, y) = (sexpr::num(at, 1)?, sexpr::num(at, 2)?);
    let angle = ((sexpr::num(at, 3).unwrap_or(0.0) * 1000.0).round() as i64).rem_euclid(360_000) as u32;
    let mut b = Barcode {
        id: String::new(),
        at: Point { x: mm_to_nm(x), y: mm_to_nm(y) },
        angle,
        layer: sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)).unwrap_or("Dwgs.User").to_string(),
        width_nm: Nm::from_raw_shim(DEFAULT_SIZE_NM),
        height_nm: Nm::from_raw_shim(DEFAULT_SIZE_NM),
        text: sexpr::find(item, "text").and_then(|t| sexpr::txt(t, 1)).unwrap_or("").to_string(),
        text_height_nm: Nm::from_raw_shim(DEFAULT_TEXT_HEIGHT_NM),
        kind: BarcodeKind::default(),
        ecc: EccLevel::default(),
        show_text: true,
        knockout: false,
        margin_x_nm: Nm::ZERO,
        margin_y_nm: Nm::ZERO,
    };
    if let Some(s) = sexpr::find(item, "size") {
        b.width_nm = sexpr::num(s, 1).map_or(b.width_nm, mm_to_nm);
        b.height_nm = sexpr::num(s, 2).map_or(b.height_nm, mm_to_nm);
    }
    if let Some(h) = sexpr::find(item, "text_height").and_then(|h| sexpr::num(h, 1)) {
        b.text_height_nm = mm_to_nm(h);
    }
    if let Some(t) = sexpr::find(item, "type") {
        b.kind = BarcodeKind::from_file_name(sexpr::txt(t, 1)?)?;
    }
    if let Some(e) = sexpr::find(item, "ecc_level").and_then(|e| sexpr::txt(e, 1)).and_then(EccLevel::from_file_name) {
        b.ecc = e;
    }
    // `(hide yes)` hides the text; the barcode itself is always drawn.
    b.show_text = !yes(item, "hide").unwrap_or(false);
    b.knockout = yes(item, "knockout").unwrap_or(false);
    if let Some(m) = sexpr::find(item, "margins") {
        b.margin_x_nm = sexpr::num(m, 1).map_or(Nm::ZERO, mm_to_nm);
        b.margin_y_nm = sexpr::num(m, 2).map_or(Nm::ZERO, mm_to_nm);
    }
    Some(b)
}

/// Every `(barcode ..)` of a board.
pub(crate) fn read_all(root: &[Sexpr], refs: &mut Vec<crate::import_items::ItemRef>) -> Vec<Barcode> {
    let mut out = Vec::new();
    for item in sexpr::find_all(root, "barcode") {
        if let Some(b) = read(item) {
            out.push(b);
            refs.push(crate::import_items::ItemRef::of(item));
        }
    }
    out
}

/// One barcode as `format( const PCB_BARCODE* )` writes it; returns its uuid.
pub(crate) fn write(out: &mut String, b: &Barcode, locked: bool) -> String {
    let uuid = crate::duid_for(&format!("barcode:{}", b.id), &b.id);
    write!(out, "\t(barcode").unwrap();
    if locked {
        write!(out, " (locked yes)").unwrap();
    }
    write!(out, " (at {} {} {})", mm(b.at.x), mm(b.at.y), fmt_mm_f(b.angle as f64 / 1000.0)).unwrap();
    write!(out, " (layer {})", sexpr_str(&b.layer)).unwrap();
    write!(out, " (size {} {})", mm(b.width_nm), mm(b.height_nm)).unwrap();
    write!(out, " (text {})", sexpr_str(&b.text)).unwrap();
    write!(out, " (text_height {})", mm(b.text_height_nm)).unwrap();
    write!(out, " (type {})", b.kind.file_name()).unwrap();
    if b.kind.has_error_correction() {
        write!(out, " (ecc_level {})", b.ecc.file_name()).unwrap();
    }
    write!(out, " (hide {})", if b.show_text { "no" } else { "yes" }).unwrap();
    write!(out, " (knockout {})", if b.knockout { "yes" } else { "no" }).unwrap();
    if b.margin_x_nm != Nm::ZERO || b.margin_y_nm != Nm::ZERO {
        write!(out, " (margins {} {})", mm(b.margin_x_nm), mm(b.margin_y_nm)).unwrap();
    }
    writeln!(out, " (uuid \"{uuid}\"))").unwrap();
    uuid
}
