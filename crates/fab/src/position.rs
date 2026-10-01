//! KiCad's own footprint-position (`.pos`) writer.
//!
//! A port of `PLACE_FILE_EXPORTER::GenPositionData`
//! (`pcbnew/exporters/place_file_exporter.cpp`), CSV and ASCII forms.
//! Checked byte-for-byte against `kicad-cli pcb export pos --format csv
//! --units mm` / `--format ascii --units mm` run on the same board: header
//! row, column widths (ASCII -- computed from the data, not fixed), number
//! formatting, and both coordinate sign conventions (`PosY` negated,
//! `Rot` negated -- the same Y-flip and rotation-sign `eda_kicad::pcb`'s
//! own `.kicad_pcb` writer already uses, since this is reading the same
//! "plot-space" convention back out).
//!
//! This is deliberately a *second*, separate writer from [`crate::cpl_csv`]:
//! that one is JLCPCB's own CPL template (`Designator,Mid X,Mid Y,Layer,
//! Rotation`), a specific downstream consumer's columns this workspace
//! already produced and tested against real JLCPCB orders (see
//! `work/l1-order`). This one is KiCad's own native format, for a caller
//! that specifically wants that.

use crate::gerber::placed_pads_full;
use eda_model::ir::{Design, Side, Um};
use eda_model::{CheckResult, ConstraintModel, PadKind};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PosFormat {
    Csv,
    Ascii,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PosSide {
    Front,
    Back,
    Both,
}

#[derive(Clone, Copy)]
pub struct PosOptions {
    pub format: PosFormat,
    pub side: PosSide,
    /// `true` = millimetres, `false` = inches. KiCad/`kicad-cli` default
    /// to inches; this workspace is metric throughout, so `Default` below
    /// picks mm instead -- a deliberate, documented difference, not a
    /// missed port.
    pub units_mm: bool,
    pub smd_only: bool,
    pub exclude_fp_th: bool,
}

impl Default for PosOptions {
    fn default() -> Self {
        PosOptions { format: PosFormat::Csv, side: PosSide::Both, units_mm: true, smd_only: false, exclude_fp_th: false }
    }
}

struct Row {
    reference: String,
    value: String,
    package: String,
    x_um: Um,
    y_um: Um,
    rot_mdeg: i64,
    side: Side,
}

/// `GetFPID().GetLibItemName()`: the footprint's own name, library prefix
/// stripped -- what `kicad-cli` actually prints in the `Package` column
/// (checked directly: `Connector_USB:USB_C_Receptacle_...` prints as just
/// `USB_C_Receptacle_...`).
fn lib_item_name(raw: &str) -> &str {
    raw.rsplit(':').next().unwrap_or(raw)
}

fn rows(design: &Design, model: &ConstraintModel, opts: &PosOptions) -> Result<Vec<Row>, Vec<CheckResult>> {
    let Some(pl) = &design.placement else {
        return Err(vec![CheckResult::fail("fab.pos", "design", "design has no placement: nothing to place")]);
    };
    let mut out = Vec::new();
    for fp in &pl.footprints {
        if opts.side == PosSide::Front && fp.side == Side::Bottom {
            continue;
        }
        if opts.side == PosSide::Back && fp.side == Side::Top {
            continue;
        }
        let Some(part) = model.part(&fp.id) else {
            return Err(vec![CheckResult::fail("fab.pos", fp.id.clone(), "footprint has no matching part")]);
        };
        let Some(footprint) = model.footprint_of(part) else {
            return Err(vec![CheckResult::fail("fab.pos", fp.id.clone(), "part has no resolvable footprint geometry")]);
        };
        if opts.smd_only || opts.exclude_fp_th {
            let Some(pads) = placed_pads_full(model, part, fp) else {
                return Err(vec![CheckResult::fail("fab.pos", fp.id.clone(), "part has no resolvable footprint geometry")]);
            };
            let has_th = pads.iter().any(|p| p.kind != PadKind::Smd);
            if opts.smd_only && has_th {
                continue;
            }
            if opts.exclude_fp_th && has_th {
                continue;
            }
        }
        let lib_name = part.footprint.clone().unwrap_or_else(|| footprint.name.clone());
        out.push(Row {
            reference: fp.id.clone(),
            value: part.value.clone().unwrap_or_default(),
            package: lib_item_name(&lib_name).to_string(),
            x_um: fp.at.x,
            y_um: fp.at.y,
            rot_mdeg: fp.rot as i64,
            side: fp.side,
        });
    }
    // `sortFPlist`: top side first, then natural reference order within a side.
    out.sort_by(|a, b| (side_rank(a.side), natural_key(&a.reference)).cmp(&(side_rank(b.side), natural_key(&b.reference))));
    Ok(out)
}

/// `sortFPlist`'s `ref.m_Layer > tst.m_Layer`: KiCad's internal layer id
/// for `B_Cu` is *greater* than `F_Cu`'s, so despite that function's own
/// "top layer first" comment, a real `kicad-cli pcb export pos` sorts
/// bottom-side footprints before top-side ones -- checked directly (this
/// crate's own comparator test caught the opposite assumption).
fn side_rank(s: Side) -> u8 {
    if s == Side::Bottom {
        0
    } else {
        1
    }
}

/// `StrNumCmp`: alphabetic prefix, then the trailing number as a number
/// (`R2` before `R10`), not lexically.
fn natural_key(r: &str) -> (String, u64) {
    let split = r.find(|c: char| c.is_ascii_digit()).unwrap_or(r.len());
    let (alpha, num) = r.split_at(split);
    (alpha.to_string(), num.parse().unwrap_or(0))
}

fn conv_mm(um: Um, units_mm: bool) -> f64 {
    let mm = um as f64 / 1000.0;
    if units_mm {
        mm
    } else {
        mm / 25.4
    }
}

/// `-0.0 == 0.0` is true (IEEE 754), but `format!("{:.6}", -0.0)` still
/// prints the sign, so a part at Y=0 or with rotation 0 -- common, not an
/// edge case -- would print `-0.000000` once negated. A real KiCad
/// position file prints a plain `0.000000` there (checked directly), so
/// this clears the sign bit on an exact zero before formatting.
fn clear_negative_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

fn side_name(s: Side) -> &'static str {
    if s == Side::Top {
        "top"
    } else {
        "bottom"
    }
}

fn csv_field(s: &str) -> String {
    format!("\"{s}\"")
}

fn gen_csv(rows: &[Row], opts: &PosOptions) -> String {
    let mut out = String::from("Ref,Val,Package,PosX,PosY,Rot,Side\n");
    for r in rows {
        let (x, y) = (conv_mm(r.x_um, opts.units_mm), clear_negative_zero(-conv_mm(r.y_um, opts.units_mm)));
        let rot = clear_negative_zero(-(r.rot_mdeg as f64) / 1000.0);
        out.push_str(&format!("{},{},{},{:.6},{:.6},{:.6},{}\n", csv_field(&r.reference), csv_field(&r.value), csv_field(&r.package), x, y, rot, side_name(r.side)));
    }
    out
}

fn gen_ascii(rows: &[Row], opts: &PosOptions, meta_date: &str, generator_version: &str) -> String {
    let len_ref = rows.iter().map(|r| r.reference.len()).max().unwrap_or(0).max(8);
    let len_val = rows.iter().map(|r| r.value.len()).max().unwrap_or(0).max(8);
    let len_pkg = rows.iter().map(|r| r.package.len()).max().unwrap_or(0).max(16);
    let underscored = |s: &str| s.replace(' ', "_");

    let mut out = String::new();
    out.push_str(&format!("### Footprint positions - created on {meta_date} ###\n"));
    out.push_str(&format!("### Printed by eda-fab version {generator_version}\n"));
    out.push_str(if opts.units_mm { "## Unit = mm, Angle = deg.\n" } else { "## Unit = inches, Angle = deg.\n" });
    out.push_str("## Side : ");
    out.push_str(match opts.side {
        PosSide::Front => "top",
        PosSide::Back => "bottom",
        PosSide::Both => "All",
    });
    out.push('\n');
    out.push_str(&format!("{:<len_ref$}  {:<len_val$}  {:<len_pkg$}  {:>9}  {:>9}  {:>8}  {}\n", "# Ref", "Val", "Package", "PosX", "PosY", "Rot", "Side"));
    for r in rows {
        let (x, y) = (conv_mm(r.x_um, opts.units_mm), clear_negative_zero(-conv_mm(r.y_um, opts.units_mm)));
        let rot = clear_negative_zero(-(r.rot_mdeg as f64) / 1000.0);
        out.push_str(&format!(
            "{:<len_ref$}  {:<len_val$}  {:<len_pkg$}  {:>9.4}  {:>9.4}  {:>8.4}  {}\n",
            underscored(&r.reference),
            underscored(&r.value),
            underscored(&r.package),
            x,
            y,
            rot,
            side_name(r.side)
        ));
    }
    out.push_str("## End\n");
    out
}

pub struct PosMeta {
    pub date: String,
    pub generator_version: String,
}

/// Generate the position-file text. The caller decides the filename
/// (KiCad's own convention: `<title>.pos`, or `<title>-top.pos`/
/// `<title>-bottom.pos`/`<title>-all.pos` -- `PLACE_FILE_EXPORTER::
/// DecorateFilename` -- when `opts.side` is not `Both`, or front and back
/// are written as separate files).
pub fn write_pos(design: &Design, model: &ConstraintModel, meta: &PosMeta, opts: PosOptions) -> Result<String, Vec<CheckResult>> {
    let rows = rows(design, model, &opts)?;
    Ok(match opts.format {
        PosFormat::Csv => gen_csv(&rows, &opts),
        PosFormat::Ascii => gen_ascii(&rows, &opts, &meta.date, &meta.generator_version),
    })
}
