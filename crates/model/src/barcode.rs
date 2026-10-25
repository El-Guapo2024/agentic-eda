//! A barcode on the board (KiCad's `PCB_BARCODE`, `(barcode ..)` in a `.kicad_pcb`): a QR code, a Data Matrix or a one-dimensional code made of copper, silkscreen or any
//! other layer.
//!
//! This is the data half only. A board file that has barcodes keeps them (`DrawingsSection::barcodes`: read by the importer, written back by the exporter as KiCad
//! writes them), and kicad-cli judges them as it judges the original: a barcode's visual separation from the board edge (`clearance`: "barcode visual separation") and its
//! copper against the nets round it are DRC findings that vanished from a re-export that dropped them. There is no editor for them yet: the canvas does not draw them and
//! no verb places or edits one (GAPS.md, item 24).

use crate::ir::{Millideg, Point, Nm};
use serde::{Deserialize, Serialize};

/// `BARCODE_T`: the `(type ..)` of a barcode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarcodeKind {
    Code39,
    Code128,
    DataMatrix,
    /// What `PCB_BARCODE` is until the file says otherwise.
    #[default]
    Qr,
    MicroQr,
}

impl BarcodeKind {
    /// The word the file writes (`PCB_IO_KICAD_SEXPR::format( const PCB_BARCODE* )`).
    pub fn file_name(self) -> &'static str {
        match self {
            BarcodeKind::Code39 => "code39",
            BarcodeKind::Code128 => "code128",
            BarcodeKind::DataMatrix => "datamatrix",
            BarcodeKind::Qr => "qr",
            BarcodeKind::MicroQr => "microqr",
        }
    }

    /// The kind a file's word stands for (`parsePCB_BARCODE` takes `data_matrix`, `qrcode` and `micro_qr` too).
    pub fn from_file_name(name: &str) -> Option<BarcodeKind> {
        Some(match name {
            "code39" => BarcodeKind::Code39,
            "code128" => BarcodeKind::Code128,
            "datamatrix" | "data_matrix" => BarcodeKind::DataMatrix,
            "qr" | "qrcode" => BarcodeKind::Qr,
            "microqr" | "micro_qr" => BarcodeKind::MicroQr,
            _ => return None,
        })
    }

    /// Whether the code has an error correction level (the two QR kinds).
    pub fn has_error_correction(self) -> bool {
        matches!(self, BarcodeKind::Qr | BarcodeKind::MicroQr)
    }
}

/// `BARCODE_ECC_T`: how much of a QR code may be lost and still be read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EccLevel {
    #[default]
    L,
    M,
    Q,
    H,
}

impl EccLevel {
    pub fn file_name(self) -> &'static str {
        match self {
            EccLevel::L => "L",
            EccLevel::M => "M",
            EccLevel::Q => "Q",
            EccLevel::H => "H",
        }
    }

    /// The level a file's letter stands for, in either case.
    pub fn from_file_name(name: &str) -> Option<EccLevel> {
        Some(match name {
            "L" | "l" => EccLevel::L,
            "M" | "m" => EccLevel::M,
            "Q" | "q" => EccLevel::Q,
            "H" | "h" => EccLevel::H,
            _ => return None,
        })
    }
}

fn d_true() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &Nm) -> bool {
    *n == Nm::ZERO
}

/// One barcode, as `PCB_BARCODE` holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Barcode {
    /// Stable id (`bar_xxxxxxxxxxxx`), made when the barcode is read, as every drawing's is.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// The barcode's centre.
    pub at: Point,
    /// The angle in the file's sense (counter-clockwise on the screen), millidegrees.
    #[serde(default)]
    pub angle: Millideg,
    pub layer: String,
    /// The size of the symbol, nm (`(size W H)`).
    #[serde(alias = "width_um", deserialize_with = "crate::units::de")]
    pub width_nm: Nm,
    #[serde(alias = "height_um", deserialize_with = "crate::units::de")]
    pub height_nm: Nm,
    /// What the barcode encodes.
    pub text: String,
    /// The height of the text printed with a one-dimensional code, nm (`(text_height H)`).
    #[serde(alias = "text_height_um", deserialize_with = "crate::units::de")]
    pub text_height_nm: Nm,
    #[serde(default)]
    pub kind: BarcodeKind,
    /// A QR code's error correction level (`(ecc_level ..)`; the file has none for the other kinds).
    #[serde(default)]
    pub ecc: EccLevel,
    /// The text is printed (`(hide no)`, the default).
    #[serde(default = "d_true", skip_serializing_if = "is_true")]
    pub show_text: bool,
    /// The barcode is cut out of the copper or silkscreen round it (`(knockout yes)`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub knockout: bool,
    /// `(margins X Y)`: the quiet zone round the symbol, nm.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[serde(alias = "margin_x_um", deserialize_with = "crate::units::de")]
    pub margin_x_nm: Nm,
    #[serde(default, skip_serializing_if = "is_zero")]
    #[serde(alias = "margin_y_um", deserialize_with = "crate::units::de")]
    pub margin_y_nm: Nm,
}

impl Barcode {
    pub(crate) fn id_seed(&self) -> String {
        format!("{}|{},{}|{}|{}", self.text, self.at.x, self.at.y, self.angle, self.layer)
    }
}
