//! Load a real KiCad library footprint (a `.kicad_mod` file) into our own
//! `eda_model::Footprint`, so a part can name one directly --
//! `footprint: "Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12"` -- and
//! get every pad feature that footprint actually uses (slotted drills,
//! non-plated holes, a pad rotated on its own, repeated pad numbers,
//! roundrect ratio) instead of falling through to the built-in package
//! table, which only knows a handful of generic passive/small-IC shapes.
//!
//! This shares its pad grammar with the whole-board `.kicad_pcb` reader
//! (`import::parse_pad_geometry` -- a `.kicad_mod`'s `(pad ...)` nodes are
//! byte-for-byte the same grammar as a footprint's inside a board file) so
//! the two readers cannot drift apart on what a pad means.
//!
//! Lives in `eda-kicad`, not `eda-model`, because `eda-model` cannot depend
//! on the s-expression reader (or on `eda-kicad` at all -- the dependency
//! only goes the other way).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use eda_model::footprint::Model3d;
use eda_model::ir::{Side, Nm};
use eda_model::{ConstraintModel, Footprint};

use crate::import::{parse_pad_geometry, ImportNotes};
use crate::sexpr::{self, Sexpr};

/// Environment variable overriding where KiCad's own footprint libraries
/// (a directory of `<Library>.pretty/<Footprint>.kicad_mod` trees) live.
pub const LIBRARY_ROOT_ENV: &str = "EDA_KICAD_FOOTPRINTS";

/// Where KiCad's standard footprint libraries live: `LIBRARY_ROOT_ENV` if
/// set, else the installed KiCad's (see [`kicad_shared_dir`]).
pub fn default_footprint_library_root() -> PathBuf {
    if let Ok(p) = std::env::var(LIBRARY_ROOT_ENV) {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    kicad_shared_dir("footprints")
}

/// `sub` (`footprints`, `symbols`, `3dmodels`, `template`) of the installed KiCad's shared data: the macOS bundle's `SharedSupport`, else
/// `$KICAD_PATH` (what KiCad's Linux packages set, `/usr/share/kicad-nightly` for the nightly), `/usr/share/kicad-nightly`, `/usr/share/kicad`
/// or `/usr/local/share/kicad`, the first that has it. With none installed, the macOS path, which does not exist.
pub fn kicad_shared_dir(sub: &str) -> PathBuf {
    let mac = PathBuf::from("/Applications/KiCad/KiCad.app/Contents/SharedSupport");
    let env = std::env::var("KICAD_PATH").ok().filter(|p| !p.is_empty()).map(PathBuf::from);
    let candidates = [Some(mac.clone()), env, Some("/usr/share/kicad-nightly".into()), Some("/usr/share/kicad".into()), Some("/usr/local/share/kicad".into())];
    candidates.into_iter().flatten().map(|d| d.join(sub)).find(|d| d.is_dir()).unwrap_or_else(|| mac.join(sub))
}

/// The `.kicad_mod` file a library id (`"Connector_USB:USB_C_Receptacle_
/// HRO_TYPE-C-31-M-12"`) names, under `lib_root`. `None` for a name that is
/// not `"Library:Footprint"` shaped, or a file that does not exist --
/// either way, the caller falls back to the built-in table.
pub fn find_footprint_file(lib_root: &Path, lib_id: &str) -> Option<PathBuf> {
    let (lib, name) = lib_id.split_once(':')?;
    if lib.is_empty() || name.is_empty() {
        return None;
    }
    let path = lib_root.join(format!("{lib}.pretty")).join(format!("{name}.kicad_mod"));
    path.is_file().then_some(path)
}

/// Parse a `.kicad_mod` file's text into our `Footprint`: every pad (all
/// four hand-editable features, plus roundrect ratio), and a courtyard
/// from `F.CrtYd`/`B.CrtYd` graphics.
pub fn parse_footprint_file(text: &str, name: &str) -> Result<Footprint, String> {
    let mut tree = sexpr::parse(text).map_err(|e| format!("not a valid s-expression file: {e}"))?;
    // KiCad 10's bold text, whose thickness is the width before the bold factor: `parseEDA_TEXT` widens it back for a footprint file of version
    // 20260826 or later, the footprint's own `(version ..)` (`parseFOOTPRINT_unchecked`'s `m_requiredVersion`). See `bold_text`.
    crate::bold_text::normalize(&mut tree);
    // A file older than 20210606: its `~X~` overbars in KiCad's new `~{X}` notation, net names and texts, as KiCad's parser converts them (`overbar`).
    crate::overbar::normalize(&mut tree);
    // `(footprint ..)`, or KiCad 5's `(module ..)`: `parseFOOTPRINT_unchecked`'s `wxCHECK_MSG( CurTok() == T_module || CurTok() == T_footprint, ..`
    let root = tree.as_list().filter(|l| matches!(sexpr::tag(l), Some("footprint") | Some("module"))).ok_or("top-level form is not (footprint ...); not a KiCad footprint file")?;

    // A bare footprint file has no board placement, so its own `(at ...)`
    // (present when it was saved from a board, absent from a pristine
    // library file) is not our concern: pad rotations are already relative
    // to the footprint, which is exactly the frame this file's pads are
    // already in. Side is always top; the loader's caller mirrors it for a
    // bottom-side instance the same way it already does for the built-in
    // table.
    let mut notes = ImportNotes::default();
    let mut pads = Vec::new();
    for pad in sexpr::find_all(root, "pad") {
        if let Some(p) = parse_pad_geometry(pad, Side::Top, 0, &mut notes) {
            pads.push(p);
        }
    }
    if pads.is_empty() {
        return Err("footprint has no pads this reader could place (missing at/size?)".into());
    }

    // the courtyard as its graphics draw it, for the board to write the same items (`FpArt::courtyard`), and as the outlines the checks use
    let mut art = crate::fp_art_io::read_art(root, Side::Top, 0);
    art.courtyard = crate::fp_art_io::read_courtyard(root, Side::Top);
    let courtyard_outlines = crate::import::footprint_courtyard_outlines(root, Side::Top);
    Ok(Footprint { name: name.to_string(), pads, courtyard: courtyard_from(root), model: model_from(root), courtyard_outlines, models3d: models_from(root), art })
}

/// The `(model "...")` path, verbatim (KiCad writes it with its own
/// `${KICADn_3DMODEL_DIR}`-style variable already in place, which this
/// reader passes straight through rather than resolving -- resolving it
/// is `kicad-cli`'s job when it later exports this data, not this
/// import path's). A footprint can have more than one `(model ...)`
/// (rare, but real -- some parts ship separate models per variant); the
/// first one is enough for this app's purposes, same as how it already
/// only keeps one courtyard box rather than every graphic.
pub(crate) fn model_from(root: &[Sexpr]) -> Option<String> {
    sexpr::find(root, "model").and_then(|m| sexpr::txt(m, 1)).map(str::to_string)
}

/// Every `(model "path" (offset (xyz x y z)) (scale (xyz x y z)) (rotate (xyz x y z)) [(opacity v)] [(hide yes)])` of a footprint, in file order
/// (`PCB_IO_KICAD_SEXPR_PARSER::parse3DModel`). `offset` is millimetres; the legacy `(at (xyz ..))` form (KiCad before 5) is inches and is
/// converted, as the parser does. A form with no path is skipped; a missing offset, scale, rotation or opacity is KiCad's default (0, 1, 0, 1).
/// The placement is kept exactly as the file writes it, whatever side the footprint is on: [`Model3d::flipped_frame`] is the one place that
/// turns it into the studio's frame for a bottom-side footprint.
pub fn models_from(root: &[Sexpr]) -> Vec<Model3d> {
    let xyz = |form: Option<&[Sexpr]>, scale_to_mm: f64| -> Option<[f64; 3]> {
        let v = sexpr::find(form?, "xyz")?;
        Some([sexpr::num(v, 1)? * scale_to_mm, sexpr::num(v, 2)? * scale_to_mm, sexpr::num(v, 3)? * scale_to_mm])
    };
    sexpr::find_all(root, "model")
        .filter_map(|m| {
            let path = sexpr::txt(m, 1).filter(|p| !p.is_empty())?;
            let offset = xyz(sexpr::find(m, "offset"), 1.0).or_else(|| xyz(sexpr::find(m, "at"), 25.4)).unwrap_or([0.0; 3]);
            // `(hide)` is a bare token in old files and `(hide yes)` in new ones; `(hide no)` shows the model.
            let hidden = sexpr::find(m, "hide").is_some_and(|h| sexpr::txt(h, 1).map_or(true, |v| v != "no"));
            let bare_hide = m.iter().any(|e| e.text() == Some("hide"));
            Some(Model3d {
                path: path.to_string(),
                offset,
                scale: xyz(sexpr::find(m, "scale"), 1.0).unwrap_or([1.0; 3]),
                rotate: xyz(sexpr::find(m, "rotate"), 1.0).unwrap_or([0.0; 3]),
                opacity: sexpr::find(m, "opacity").and_then(|o| sexpr::num(o, 1)).unwrap_or(1.0),
                show: !(hidden || bare_hide),
            })
        })
        .collect()
}

/// The `(model ...)` entries of a footprint, `tabs` deep, as `PCB_IO_KICAD_SEXPR::format( FOOTPRINT )` writes them: the file name, `(hide yes)` for a model that is not shown,
/// `(opacity v)` with four decimals when it is not 1, then the offset, the scale and the rotation as `(offset (xyz x y z))` with `FormatDouble2Str`'s numbers. A model
/// with no file name is not written (`if( !bs3D->m_Filename.IsEmpty() )`). The placement is written as it is held: a bottom-side footprint's is turned by
/// [`Model3d::flipped_frame`] before this.
pub(crate) fn write_models(out: &mut String, models: &[Model3d], tabs: usize) {
    use std::fmt::Write as _;
    let t = "\t".repeat(tabs);
    for m in models.iter().filter(|m| !m.path.is_empty()) {
        writeln!(out, "{t}(model {}", crate::sexpr_str(&m.path)).unwrap();
        if !m.show {
            writeln!(out, "{t}\t(hide yes)").unwrap();
        }
        if m.opacity != 1.0 {
            writeln!(out, "{t}\t(opacity {:.4})", m.opacity).unwrap();
        }
        for (tag, v) in [("offset", m.offset), ("scale", m.scale), ("rotate", m.rotate)] {
            writeln!(out, "{t}\t({tag}\n{t}\t\t(xyz {} {} {})\n{t}\t)", crate::fmt_double_2_str(v[0]), crate::fmt_double_2_str(v[1]), crate::fmt_double_2_str(v[2])).unwrap();
        }
        writeln!(out, "{t})").unwrap();
    }
}

/// [`models_from`] for the text of a `.kicad_mod` file; empty when the text is no footprint.
pub fn footprint_models(text: &str) -> Vec<Model3d> {
    let Ok(tree) = sexpr::parse(text) else { return Vec::new() };
    tree.as_list().filter(|l| sexpr::tag(l) == Some("footprint") || sexpr::tag(l) == Some("module")).map(models_from).unwrap_or_default()
}

/// The `(attr ...)` flags of a `.kicad_mod` file's footprint, as the words the file writes (`smd`, `through_hole`, `board_only`,
/// `exclude_from_pos_files`, ...). KiCad's 3D viewer sorts models by them: `smd` is an SMD model, `through_hole` a through-hole one, and a footprint
/// with neither is a virtual one (`BOARD_ADAPTER::IsFootprintShown`).
pub fn footprint_attributes(text: &str) -> Vec<String> {
    let Ok(tree) = sexpr::parse(text) else { return Vec::new() };
    let Some(root) = tree.as_list().filter(|l| sexpr::tag(l) == Some("footprint") || sexpr::tag(l) == Some("module")) else { return Vec::new() };
    sexpr::find(root, "attr").map(|a| a.iter().skip(1).filter_map(|e| e.text()).map(str::to_string).collect()).unwrap_or_default()
}

/// A symmetric-about-origin courtyard half-extent enclosing every
/// `F.CrtYd`/`B.CrtYd` graphic. Real footprints are not always centred on
/// their own origin (KiCad has no such rule); `Footprint::courtyard` is,
/// so an off-centre courtyard is conservatively enclosed by the smallest
/// symmetric box that contains it, not represented exactly. `None` when
/// the footprint has no courtyard layer at all -- callers fall back to the
/// pad-bounding-box derivation the same as any other footprint with none.
pub(crate) fn courtyard_from(root: &[Sexpr]) -> Option<(Nm, Nm)> {
    let on_crtyd = |item: &[Sexpr]| matches!(sexpr::find(item, "layer").and_then(|l| sexpr::txt(l, 1)), Some("F.CrtYd") | Some("B.CrtYd"));
    let mm_to_nm = crate::import::mm_to_nm;
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut extend = |x: f64, y: f64| {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    };
    let mut seen = false;
    for line in sexpr::find_all(root, "fp_line").filter(|it| on_crtyd(it)) {
        for tag in ["start", "end"] {
            if let Some(p) = sexpr::find(line, tag) {
                if let (Some(x), Some(y)) = (sexpr::num(p, 1), sexpr::num(p, 2)) {
                    extend(x, y);
                    seen = true;
                }
            }
        }
    }
    for rect in sexpr::find_all(root, "fp_rect").filter(|it| on_crtyd(it)) {
        for tag in ["start", "end"] {
            if let Some(p) = sexpr::find(rect, tag) {
                if let (Some(x), Some(y)) = (sexpr::num(p, 1), sexpr::num(p, 2)) {
                    extend(x, y);
                    seen = true;
                }
            }
        }
    }
    for circle in sexpr::find_all(root, "fp_circle").filter(|it| on_crtyd(it)) {
        if let (Some(c), Some(e)) = (sexpr::find(circle, "center"), sexpr::find(circle, "end")) {
            if let (Some(cx), Some(cy), Some(ex), Some(ey)) = (sexpr::num(c, 1), sexpr::num(c, 2), sexpr::num(e, 1), sexpr::num(e, 2)) {
                let r = ((ex - cx).powi(2) + (ey - cy).powi(2)).sqrt();
                extend(cx - r, cy - r);
                extend(cx + r, cy + r);
                seen = true;
            }
        }
    }
    for poly in sexpr::find_all(root, "fp_poly").filter(|it| on_crtyd(it)) {
        if let Some(pts) = sexpr::find(poly, "pts") {
            for item in &pts[1..] {
                if let Some(xy) = item.as_list().filter(|l| sexpr::tag(l) == Some("xy")) {
                    if let (Some(x), Some(y)) = (sexpr::num(xy, 1), sexpr::num(xy, 2)) {
                        extend(x, y);
                        seen = true;
                    }
                }
            }
        }
    }
    if !seen {
        return None;
    }
    let half_w = mm_to_nm(x0.abs().max(x1.abs()));
    let half_h = mm_to_nm(y0.abs().max(y1.abs()));
    Some((half_w.max(Nm::new(1)), half_h.max(Nm::new(1))))
}

/// For every part naming a `"Library:Footprint"` footprint that is not
/// already in `model.footprints`, try to load it from `lib_root` and add
/// it there -- so `model.footprint_of` (explicit list, then built-in
/// table) resolves it without either of them changing. Best-effort: a
/// part naming a library footprint that cannot be found or parsed is left
/// alone, exactly as it would be today, to fail downstream with the same
/// "no resolvable footprint" a typo in a built-in name already produces.
/// Returns one human-readable warning per footprint that could not be
/// loaded, for the caller to print; an empty result means every named
/// library footprint (if any) resolved.
pub fn resolve_library_footprints(model: &mut ConstraintModel, lib_root: &Path) -> Vec<String> {
    resolve_library_footprints_with(model, |lib_id| find_footprint_file(lib_root, lib_id))
}

/// [`resolve_library_footprints`] with the way a `Library:Footprint` id finds its `.kicad_mod` file left to the caller -- the libraries of the board's
/// library tables, where a library's nickname and its folder are two things. `find` is asked only for ids the model needs.
pub fn resolve_library_footprints_with(model: &mut ConstraintModel, mut find: impl FnMut(&str) -> Option<PathBuf>) -> Vec<String> {
    let have: BTreeSet<String> = model.footprints.iter().map(|f| f.name.clone()).collect();
    let mut warnings = Vec::new();
    let mut wanted: Vec<String> = model.parts.iter().filter_map(|p| p.footprint.clone()).filter(|f| f.contains(':') && !have.contains(f)).collect();
    wanted.sort();
    wanted.dedup();
    for lib_id in wanted {
        let Some(path) = find(&lib_id) else { continue };
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse_footprint_file(&text, &lib_id) {
                Ok(fp) => model.footprints.push(fp),
                Err(e) => warnings.push(format!("{}: {e}", path.display())),
            },
            Err(e) => warnings.push(format!("{}: {e}", path.display())),
        }
    }
    // The built-in packages (`0603`, `SOIC-8`) have pads and no outline: their part gets the silkscreen and fab drawings of the KiCad footprint they stand for.
    crate::fp_art_io::attach_installed_art(model, &mut find);
    warnings
}

/// The Reference, the Value and the user fields of a library footprint as `(property ..)` forms (`PCB_IO_KICAD_SEXPR::format( FOOTPRINT )` for a footprint of the library: its fields
/// are the root's own, so they are indented one tab less than on a board). The Reference and the Value are always written, with `CreateNewFootprint`'s layouts when the footprint
/// has none of its own; the user fields are always written, each with its layout (a field with none is hidden at the origin on the fabrication layer, `OnAddField`).
fn write_library_fields(out: &mut String, fp: &eda_model::ir::LibraryFootprint) {
    use eda_model::fp_edit::{default_library_reference_layout, default_library_value_layout, new_library_field_layout, LIBRARY_REFERENCE_TEXT};
    let one = |out: &mut String, name: &str, text: &str, l: &eda_model::fp_edit::FieldLayout| {
        let uuid = fp.kicad.uuids.get(&format!("field:{name}")).cloned().unwrap_or_else(|| crate::duid(&format!("fpfield:{}:{name}", fp.name)));
        let mut tmp = String::new();
        crate::fp_fields::write_property(&mut tmp, name, text, l, eda_model::ir::Side::Top, 0, &uuid);
        for line in tmp.lines() {
            out.push_str(line.strip_prefix('\t').unwrap_or(line));
            out.push('\n');
        }
    };
    // Always written: a `.kicad_mod` without them reads in KiCad as an empty Reference and Value at the origin. A footprint with no layout of its own takes
    // `CreateNewFootprint`'s (REF** on the silkscreen, the name on the fabrication layer).
    let mut l = fp.reference.clone().unwrap_or_else(default_library_reference_layout);
    l.visible = fp.reference_visible;
    one(out, "Reference", l.shown(LIBRARY_REFERENCE_TEXT), &l);
    let mut l = fp.value.clone().unwrap_or_else(default_library_value_layout);
    l.visible = fp.value_visible;
    one(out, "Value", l.shown(&fp.name), &l);
    // The Datasheet and the Description fields, when the footprint has them (else KiCad gives it its own, `default_library_datasheet_layout`).
    for (name, field) in [("Datasheet", &fp.datasheet), ("Description", &fp.description_field)] {
        if let Some(l) = field {
            one(out, name, l.text.as_deref().unwrap_or(""), l);
        }
    }
    for f in &fp.fields {
        let mut l = f.layout.clone().unwrap_or_else(new_library_field_layout);
        l.visible = f.visible;
        one(out, &f.name, &f.value, &l);
    }
}

/// Derive a standalone `.kicad_mod` library file from a project footprint-
/// library entry (GAPS.md #8 step 6) -- the write-side counterpart of
/// `parse_footprint_file` above, reusing this crate's own format helpers
/// (`crate::mm`/`sexpr_str`/`duid`/`fmt_mm_f`/`pad_file_angle`) so the two
/// never drift on what a number/string/uuid looks like on disk. A bare
/// library file, same convention `parse_footprint_file`'s own doc
/// describes: no `(at ...)` (no board placement), no reference/value/net
/// (a board instance's concern, not the definition's).
///
/// A pad's shape is written as `PCB_IO_KICAD_SEXPR::format( const PAD* )`
/// does ([`crate::pad_shape`]): a trapezoid's `rect_delta`, a chamfered
/// rectangle's `roundrect` with `chamfer_ratio` and `chamfer`, a custom pad's
/// `options` and `primitives`, the shape's offset inside `drill`.
///
/// Not a full inverse of `LibraryFootprint::from_engine_footprint`+editing:
/// per-pad thermal-gap/spoke-width overrides and the footprint's own
/// clearance/attribute-adjacent fields this session could not verify an exact
/// on-disk token for are left out rather than guessed at -- this writer only
/// emits sub-expressions this crate's own reader (`parse_pad_geometry`/
/// `courtyard_from`) or `crates/kicad/src/pcb.rs`'s existing, exercised board
/// writer already round-trips (`offset`/`clearance`/`layers`/
/// `roundrect_rratio`/`drill`). See PARITY-fpedit.md for the tracked gap.
pub fn export_kicad_mod(fp: &eda_model::ir::LibraryFootprint) -> String {
    export_kicad_mod_with(fp, &eda_model::embedded_files::EmbeddedFiles::default())
}

/// [`export_kicad_mod`] for a footprint that embeds files: `(embedded_fonts ..)` and `(embedded_files ..)` with the data (`PCB_IO_KICAD_SEXPR::format( const FOOTPRINT* )` for a
/// library, where `aWriteData` is true), before the models as KiCad writes them. The fonts a footprint does not embed are dropped (`ClearEmbeddedFonts`).
pub fn export_kicad_mod_with(fp: &eda_model::ir::LibraryFootprint, embedded: &eda_model::embedded_files::EmbeddedFiles) -> String {
    use eda_model::ir::Shape;
    use std::fmt::Write as _;

    let mut out = String::new();
    writeln!(out, "(footprint {}", crate::sexpr_str(&fp.name)).unwrap();
    writeln!(out, "\t(version 20240108)").unwrap();
    writeln!(out, "\t(generator \"eda\")").unwrap();
    writeln!(out, "\t(generator_version \"1.0\")").unwrap();
    writeln!(out, "\t(layer \"F.Cu\")").unwrap();
    if !fp.description.is_empty() {
        writeln!(out, "\t(descr {})", crate::sexpr_str(&fp.description)).unwrap();
    }
    if !fp.keywords.is_empty() {
        writeln!(out, "\t(tags {})", crate::sexpr_str(&fp.keywords)).unwrap();
    }
    write_library_fields(&mut out, fp);
    if let Some(c) = fp.zone_connection {
        writeln!(out, "\t(zone_connect {})", crate::zone_connection_to_file(c)).unwrap();
    }
    let mut attrs = Vec::new();
    if fp.attributes.smd {
        attrs.push("smd");
    }
    if fp.attributes.through_hole {
        attrs.push("through_hole");
    }
    if fp.attributes.board_only {
        attrs.push("board_only");
    }
    if fp.attributes.exclude_from_position_files {
        attrs.push("exclude_from_pos_files");
    }
    if fp.attributes.exclude_from_bom {
        attrs.push("exclude_from_bom");
    }
    if fp.attributes.allow_missing_courtyard {
        attrs.push("allow_missing_courtyard");
    }
    if fp.attributes.dnp {
        attrs.push("dnp");
    }
    // `FP_ALLOW_SOLDERMASK_BRIDGES`, written `allow_soldermask_bridges` (`format( FOOTPRINT )`; the reader takes `allow_solder_mask_bridges` too)
    if fp.attributes.allow_soldermask_bridges {
        attrs.push("allow_soldermask_bridges");
    }
    if !attrs.is_empty() {
        writeln!(out, "\t(attr {})", attrs.join(" ")).unwrap();
    }
    // The footprint's own kept forms that are no items (margins, clearance, net-tie groups, private layers, jumpers), after the attributes as KiCad writes them.
    for t in fp.kicad.tokens.iter().filter(|t| !is_kept_item(t)) {
        writeln!(out, "\t{t}").unwrap();
    }

    let fp_fill = |filled: bool| if filled { "yes" } else { "no" };
    for g in &fp.graphics {
        let uuid = fp.kicad.uuids.get(g.id()).cloned().unwrap_or_else(|| crate::duid(&format!("fpgraphic:{}", g.id())));
        let layer = crate::sexpr_str(g.layer());
        // the line style the file had (`LibraryFootprintKeep::stroke_types`), else KiCad's `solid`
        let solid = fp.kicad.stroke_types.get(g.id()).map(String::as_str).unwrap_or("solid");
        match g {
            Shape::Segment { stroke_width, start, end, .. } => writeln!(
                out,
                "\t(fp_line (start {} {}) (end {} {}) (stroke (width {}) (type {solid})) (layer {layer}) (uuid \"{uuid}\"))",
                crate::mm(start.x), crate::mm(start.y), crate::mm(end.x), crate::mm(end.y), crate::mm((*stroke_width).max(Nm::ZERO))
            ),
            Shape::Arc { stroke_width, start, mid, end, .. } => writeln!(
                out,
                "\t(fp_arc (start {} {}) (mid {} {}) (end {} {}) (stroke (width {}) (type {solid})) (layer {layer}) (uuid \"{uuid}\"))",
                crate::mm(start.x), crate::mm(start.y), crate::mm(mid.x), crate::mm(mid.y), crate::mm(end.x), crate::mm(end.y), crate::mm((*stroke_width).max(Nm::ZERO))
            ),
            Shape::Rect { stroke_width, filled, start, end, .. } => writeln!(
                out,
                "\t(fp_rect (start {} {}) (end {} {}) (stroke (width {}) (type {solid})) (fill {}) (layer {layer}) (uuid \"{uuid}\"))",
                crate::mm(start.x), crate::mm(start.y), crate::mm(end.x), crate::mm(end.y), crate::mm((*stroke_width).max(Nm::ZERO)), fp_fill(*filled)
            ),
            Shape::Circle { stroke_width, filled, center, end, .. } => writeln!(
                out,
                "\t(fp_circle (center {} {}) (end {} {}) (stroke (width {}) (type {solid})) (fill {}) (layer {layer}) (uuid \"{uuid}\"))",
                crate::mm(center.x), crate::mm(center.y), crate::mm(end.x), crate::mm(end.y), crate::mm((*stroke_width).max(Nm::ZERO)), fp_fill(*filled)
            ),
            Shape::Polygon { stroke_width, filled, pts, .. } => {
                write!(out, "\t(fp_poly (pts").unwrap();
                for p in pts {
                    write!(out, " (xy {} {})", crate::mm(p.x), crate::mm(p.y)).unwrap();
                }
                writeln!(out, ") (stroke (width {}) (type {solid})) (fill {}) (layer {layer}) (uuid \"{uuid}\"))", crate::mm((*stroke_width).max(Nm::ZERO)), fp_fill(*filled))
            }
            Shape::Bezier { stroke_width, start, c1, c2, end, .. } => writeln!(
                out,
                "\t(fp_curve (pts (xy {} {}) (xy {} {}) (xy {} {}) (xy {} {})) (stroke (width {}) (type {solid})) (layer {layer}) (uuid \"{uuid}\"))",
                crate::mm(start.x), crate::mm(start.y), crate::mm(c1.x), crate::mm(c1.y), crate::mm(c2.x), crate::mm(c2.y), crate::mm(end.x), crate::mm(end.y), crate::mm((*stroke_width).max(Nm::ZERO))
            ),
        }
        .unwrap();
    }

    for t in &fp.texts {
        let uuid = fp.kicad.uuids.get(&t.id).cloned().unwrap_or_else(|| crate::duid(&format!("fptext:{}", t.id)));
        write_fp_text(&mut out, t, &uuid, fp.kicad.text_extra(&t.id), !fp.kicad.unlocked_texts.contains(&t.id));
    }

    for pad in &fp.pads {
        // The engine's twin of the pad has the shape whole (trapezoid delta, chamfer, primitives, offset): written as KiCad does.
        let engine = pad.to_engine_pad();
        let shape = crate::pad_shape::shape_token(engine.shape);
        let shape_text = crate::pad_shape::write_pad_shape(engine.shape, engine.roundrect_ratio, &engine.ext, eda_model::ir::Side::Top);
        let kind = match pad.kind {
            // an edge connector pad is an SMD pad with its own attribute (`PAD_ATTRIB::CONN`)
            eda_model::PadKind::Smd if pad.kicad.connector => "connect",
            eda_model::PadKind::Smd => "smd",
            eda_model::PadKind::ThroughHole => "thru_hole",
            eda_model::PadKind::NonPlatedHole => "np_thru_hole",
        };
        let uuid = fp.kicad.uuids.get(&pad.id).cloned().unwrap_or_else(|| crate::duid(&format!("fppad:{}:{}:{}:{}", pad.id, pad.number, pad.at.x, pad.at.y)));
        let angle_deg = crate::pad_file_angle(eda_model::ir::Side::Top, 0, pad.rot);
        write!(out, "\t(pad {} {kind} {shape} (at {} {} {angle_deg})", crate::sexpr_str(&pad.number), crate::mm(pad.at.x), crate::mm(pad.at.y)).unwrap();
        write!(out, " (size {} {})", crate::mm(pad.size.0), crate::mm(pad.size.1)).unwrap();
        write!(out, "{}{}", shape_text.delta, shape_text.corners).unwrap();
        let offset = shape_text.offset.map(|(ox, oy)| format!(" (offset {} {})", crate::mm(ox), crate::mm(oy))).unwrap_or_default();
        match pad.kind {
            eda_model::PadKind::ThroughHole | eda_model::PadKind::NonPlatedHole => match (pad.drill, pad.drill_slot) {
                (Some(d), _) => {
                    write!(out, " (drill {}{offset})", crate::mm(d)).unwrap();
                }
                (None, Some((w, h))) => {
                    write!(out, " (drill oval {} {}{offset})", crate::mm(w), crate::mm(h)).unwrap();
                }
                (None, None) => {
                    // Footprint::validate would have caught this on the way in; an empty footprint being exported mid-edit (validation not yet
                    // enforced on save) just emits no drill rather than panicking.
                    if !offset.is_empty() {
                        write!(out, " (drill{offset})").unwrap();
                    }
                }
            },
            eda_model::PadKind::Smd => {
                if !offset.is_empty() {
                    write!(out, " (drill{offset})").unwrap();
                }
            }
        }
        let layers = if pad.layers.is_empty() {
            match pad.kind {
                eda_model::PadKind::Smd => vec!["F.Cu".to_string(), "F.Paste".to_string(), "F.Mask".to_string()],
                eda_model::PadKind::ThroughHole | eda_model::PadKind::NonPlatedHole => vec!["*.Cu".to_string(), "*.Mask".to_string()],
            }
        } else {
            pad.layers.clone()
        };
        write!(out, " (layers").unwrap();
        for l in &layers {
            write!(out, " {}", crate::sexpr_str(l)).unwrap();
        }
        write!(out, ")").unwrap();
        // `(remove_unused_layers ..) (keep_end_layers ..)` of a plated through-hole pad, after its layers.
        if pad.kind == eda_model::PadKind::ThroughHole {
            out.push_str(&crate::padstack_io::unconnected_text(engine.ext.stack.unconnected));
        }
        // `(property pad_prop_..)` (the fabrication property), then the margins, as `format( const PAD* )` writes them
        if let Some(p) = &pad.kicad.property {
            write!(out, " (property {p})").unwrap();
        }
        if let Some(m) = pad.kicad.solder_mask_margin {
            write!(out, " (solder_mask_margin {})", crate::mm(m)).unwrap();
        }
        if let Some(m) = pad.kicad.solder_paste_margin {
            write!(out, " (solder_paste_margin {})", crate::mm(m)).unwrap();
        }
        if let Some(r) = pad.kicad.solder_paste_margin_ratio {
            write!(out, " (solder_paste_margin_ratio {})", crate::fmt_double_2_str(r)).unwrap();
        }
        if let Some(c) = pad.clearance_override {
            write!(out, " (clearance {})", crate::mm(c)).unwrap();
        }
        // `PCB_IO_KICAD_SEXPR::format( const PAD* )`: zone_connect, thermal_bridge_width, thermal_bridge_angle (only when it is
        // not the shape's default: 45 for a circle, 90 otherwise), thermal_gap.
        if let Some(c) = pad.zone_connection {
            write!(out, " (zone_connect {})", crate::zone_connection_to_file(c)).unwrap();
        }
        if let Some(w) = pad.thermal_spoke_width_override {
            write!(out, " (thermal_bridge_width {})", crate::mm(w)).unwrap();
        }
        if let Some(a) = pad.thermal_spoke_angle_mdeg {
            let default_mdeg = engine.default_spoke_angle_mdeg() as eda_model::ir::Millideg;
            if a != default_mdeg {
                write!(out, " (thermal_bridge_angle {})", crate::fmt_mm_f(a as f64 / 1000.0)).unwrap();
            }
        }
        if let Some(g) = pad.thermal_gap_override {
            write!(out, " (thermal_gap {})", crate::mm(g)).unwrap();
        }
        out.push_str(&shape_text.custom);
        // the pad's other kept forms (pin-to-die length, teardrops, tenting, ...)
        for t in &pad.kicad.tokens {
            write!(out, " {t}").unwrap();
        }
        write!(out, " (uuid \"{uuid}\")").unwrap();
        // The other layers' shapes, after the uuid (a library file has no board: every copper layer KiCad's file can name, `MAX_CU_LAYERS`).
        out.push_str(&crate::padstack_io::write_padstack(&engine, eda_model::ir::Side::Top, &crate::padstack_io::library_copper_layers()));
        writeln!(out, ")").unwrap();
    }

    // The kept items the editor does not draw (zones, points, dimensions, images, text boxes, tables, barcodes), after the pads as KiCad writes them.
    for t in fp.kicad.tokens.iter().filter(|t| is_kept_item(t)) {
        writeln!(out, "\t{t}").unwrap();
    }

    // The embedded files, before the models as KiCad writes them.
    crate::embedded::write_owner(&mut out, embedded, 1, false);

    // The 3D models: the list, else the one path of an older entry, placed as it comes.
    let models: Vec<Model3d> = if fp.models3d.is_empty() { fp.model.iter().map(Model3d::identity).collect() } else { fp.models3d.clone() };
    write_models(&mut out, &models, 1);
    writeln!(out, ")").unwrap();
    out
}

/// Whether a kept footprint form (`LibraryFootprintKeep::tokens`) is an item of the footprint (a zone, a point, ...) rather than one of its settings.
fn is_kept_item(token: &str) -> bool {
    let tag = token.trim_start_matches('(').split(|c: char| c.is_whitespace() || c == ')').next().unwrap_or("");
    matches!(tag, "zone" | "point" | "dimension" | "image" | "fp_text_box" | "table" | "barcode" | "group" | "generated")
}

/// `fp_text user "..."`, the footprint-local counterpart of `pcb.rs`'s
/// `write_text`'s `gr_text` -- same field shape (KiCad's `EDA_TEXT::Format`
/// is shared by both), different owning tag and an added type token.
/// The text's `(effects ..)` is `EDA_TEXT::Format`'s with what the file said beyond [`eda_model::ir::Text`] (`extra`: bold, italic, a vertical justification, a glyph
/// width, an automatic pen, a font), and `(unlocked yes)` a text that turns with the footprint (`keep_upright` false).
fn write_fp_text(out: &mut String, text: &eda_model::ir::Text, uuid: &str, extra: Option<&eda_model::board_text::TextExtra>, keep_upright: bool) {
    use std::fmt::Write as _;
    let angle_deg = crate::fmt_mm_f(text.angle as f64 / 1000.0);
    let effects = crate::board_text_io::effects(text, extra);
    let knockout = if extra.is_some_and(|e| e.knockout) { " knockout" } else { "" };
    let unlocked = if keep_upright { "" } else { " (unlocked yes)" };
    writeln!(
        out,
        "\t(fp_text user {} (at {} {} {angle_deg}){unlocked} (layer {}{knockout}) (uuid \"{uuid}\")\n\t\t{effects}\n\t)",
        crate::sexpr_str(&text.content),
        crate::mm(text.at.x),
        crate::mm(text.at.y),
        crate::sexpr_str(text.layer.as_str()),
    )
    .unwrap();
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;
    use eda_model::PadKind;

    /// r6-board-io F5: a KiCad 5 library file is `(module ..)`, which `parseFOOTPRINT_unchecked` takes as it takes `(footprint ..)`.
    #[test]
    fn a_kicad5_module_file_is_a_footprint() {
        let fp = parse_footprint_file("(module R (layer F.Cu) (tedit 5B307E4C) (pad 1 smd rect (at 0 0) (size 1 1) (layers F.Cu F.Paste F.Mask)))", "old:R").expect("a (module ..) file reads");
        assert_eq!(fp.pads.len(), 1);
        assert_eq!(fp.name, "old:R");
        assert!(parse_footprint_file("(kicad_pcb (version 1))", "x:y").is_err(), "a board is still not a footprint");
    }

    fn usb_c_fixture() -> String {
        // A trimmed stand-in for the real HRO USB-C receptacle: one signal
        // pad, one non-plated mounting hole, one slotted shield pad, and an
        // asymmetric F.CrtYd -- every feature this loader exists for, small
        // enough to keep inline.
        r#"(footprint "USB_C_TEST" (version 20260206) (generator "pcbnew") (layer "F.Cu")
            (fp_line (start -5 -5) (end -5 4) (stroke (width 0.05) (type solid)) (layer "F.CrtYd") (uuid "c1"))
            (fp_line (start -5 -5) (end 5 -5) (stroke (width 0.05) (type solid)) (layer "F.CrtYd") (uuid "c2"))
            (fp_line (start -5 4) (end 5 4) (stroke (width 0.05) (type solid)) (layer "F.CrtYd") (uuid "c3"))
            (fp_line (start 5 -5) (end 5 4) (stroke (width 0.05) (type solid)) (layer "F.CrtYd") (uuid "c4"))
            (pad "" np_thru_hole circle (at -2.89 -2.6) (size 0.65 0.65) (drill 0.65) (layers "*.Cu" "*.Mask") (uuid "n1"))
            (pad "A1" smd roundrect (at -3.25 -4.045) (size 0.6 1.45) (layers "F.Cu" "F.Mask" "F.Paste") (roundrect_rratio 0.25) (uuid "p1"))
            (pad "SH" thru_hole oval (at -4.32 -3.13) (size 1 2.1) (drill oval 0.6 1.7) (layers "*.Cu" "*.Mask") (uuid "s1"))
            (pad "SH" thru_hole oval (at 4.32 -3.13) (size 1 2.1) (drill oval 0.6 1.7) (layers "*.Cu" "*.Mask") (uuid "s2"))
        )"#
        .to_string()
    }

    #[test]
    fn parses_every_pad_feature() {
        let fp = parse_footprint_file(&usb_c_fixture(), "Connector_USB:USB_C_TEST").expect("parses");
        assert_eq!(fp.pads.len(), 4);

        let npth = fp.pads.iter().find(|p| p.kind == PadKind::NonPlatedHole).expect("the mounting hole");
        assert_eq!(npth.number, "");
        assert_eq!(npth.drill, Some(mm(0.65)));

        let shields: Vec<_> = fp.pads.iter().filter(|p| p.number == "SH").collect();
        assert_eq!(shields.len(), 2, "both shield pads must survive under the shared number");
        assert_eq!(shields[0].drill_slot, Some((mm(0.6), mm(1.7))));
        assert_eq!(shields[0].drill, None);

        let signal = fp.pads.iter().find(|p| p.number == "A1").unwrap();
        assert_eq!(signal.roundrect_ratio, Some(0.25));

        // Asymmetric F.CrtYd (y: -5..4) conservatively enclosed by a
        // symmetric half-extent of 5mm either way.
        let (hw, hh) = fp.courtyard.expect("courtyard from F.CrtYd");
        assert_eq!(hw, mm(5.0));
        assert_eq!(hh, mm(5.0));

        assert!(fp.validate().is_empty(), "{:?}", fp.validate());
    }

    /// `parseEDA_TEXT`: `if( m_requiredVersion < 20260826 ) aText->MigrateLegacyBoldStrokeWidth();`, and a footprint file's own `(version ..)`
    /// sets that version (`parseFOOTPRINT_unchecked`'s `m_requiredVersion = std::max( m_requiredVersion, this_version )`). A 20261017 file's bold
    /// `(thickness 0.125)` is the width before the bold factor, so every reader here -- which has the older meaning -- must see 0.2 mm.
    #[test]
    fn a_kicad_11_footprint_files_bold_text_keeps_its_width() {
        let text = r#"(footprint "X" (version 20261017) (generator "pcbnew") (layer "F.Cu")
            (fp_text user "X" (at 0 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.125) (bold yes))))
            (fp_text user "Y" (at 0 2) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.125))))
            (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu" "F.Paste" "F.Mask")))"#;
        let fp = parse_footprint_file(text, "X").expect("parses");
        let by = |t: &str| fp.art.texts.iter().find(|x| x.text == t).expect("the text");
        assert_eq!((by("X").layout.thickness, by("X").layout.bold), (mm(0.2), true), "a bold text of a 20260826+ file keeps the width KiCad drew");
        assert_eq!(by("Y").layout.thickness, mm(0.125), "a text that is not bold is left as it is");
    }

    #[test]
    fn find_footprint_file_needs_lib_colon_name() {
        let dir = std::env::temp_dir().join("eda_kicad_footprint_lib_test");
        std::fs::create_dir_all(dir.join("Connector_USB.pretty")).unwrap();
        std::fs::write(dir.join("Connector_USB.pretty").join("Foo.kicad_mod"), usb_c_fixture()).unwrap();
        assert!(find_footprint_file(&dir, "Connector_USB:Foo").is_some());
        assert!(find_footprint_file(&dir, "Connector_USB:Missing").is_none());
        assert!(find_footprint_file(&dir, "not_a_lib_id").is_none());
    }

    #[test]
    fn resolve_library_footprints_adds_only_what_is_missing() {
        let dir = std::env::temp_dir().join("eda_kicad_footprint_lib_resolve_test");
        std::fs::create_dir_all(dir.join("Connector_USB.pretty")).unwrap();
        std::fs::write(dir.join("Connector_USB.pretty").join("Foo.kicad_mod"), usb_c_fixture()).unwrap();

        let part = |r: &str, footprint: &str| eda_model::Part {
            reference: r.into(),
            mpn: None,
            lcsc: None,
            value: None,
            package: None,
            footprint: Some(footprint.into()),
            pins: vec![],
            body_nm: None, symbol: None, datasheet: None,
            edge: None,
        };
        let mut model = ConstraintModel {
            parts: vec![part("J1", "Connector_USB:Foo"), part("R1", "0603"), part("J2", "Connector_USB:DoesNotExist")],
            ..Default::default()
        };
        let warnings = resolve_library_footprints(&mut model, &dir);
        assert!(warnings.is_empty(), "a missing library file is a silent fallback to the built-in table, not a warning: {warnings:?}");
        assert_eq!(model.footprints.len(), 1);
        assert_eq!(model.footprints[0].name, "Connector_USB:Foo");

        // Calling it again must not duplicate the now-present footprint.
        let warnings2 = resolve_library_footprints(&mut model, &dir);
        assert!(warnings2.is_empty());
        assert_eq!(model.footprints.len(), 1);
    }

    /// The two real footprints this feature exists for, straight from a
    /// real KiCad install -- not `#[ignore]`d (no kicad-cli needed, just
    /// the app's bundled library files), but skipped gracefully when
    /// they're not there, the same way the kicad-cli tests skip.
    #[test]
    fn loads_the_real_usb_c_and_button_footprints() {
        let root = default_footprint_library_root();
        let Some(usb_c) = find_footprint_file(&root, "Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12") else {
            eprintln!("KiCad footprint libraries not found at {}; skipping", root.display());
            return;
        };
        let text = std::fs::read_to_string(&usb_c).unwrap();
        let fp = parse_footprint_file(&text, "Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12").expect("parses");
        assert!(fp.validate().is_empty(), "{:?}", fp.validate());
        assert_eq!(fp.pads.iter().filter(|p| p.kind == PadKind::NonPlatedHole).count(), 2, "the two locating pegs");
        let shield: Vec<_> = fp.pads.iter().filter(|p| p.number == "SH").collect();
        assert_eq!(shield.len(), 4, "the shield's four physical pads, one pin");
        assert!(shield.iter().all(|p| p.drill_slot.is_some()), "the shield pads are slotted");
        assert!(fp.courtyard.is_some());

        let button_path = find_footprint_file(&root, "Button_Switch_SMD:SW_SPST_B3U-1000P").expect("KiCad ships this footprint alongside the connector's");
        let text = std::fs::read_to_string(&button_path).unwrap();
        let button = parse_footprint_file(&text, "Button_Switch_SMD:SW_SPST_B3U-1000P").expect("parses");
        assert!(button.validate().is_empty());
        assert_eq!(button.pads.len(), 2);
    }

    // -------------------------------------------------- export_kicad_mod

    fn lib_pad(number: &str, x: eda_model::ir::Nm, y: eda_model::ir::Nm, shape: eda_model::ir::LibraryPadShape, kind: PadKind) -> eda_model::ir::LibraryPad {
        eda_model::ir::LibraryPad {
            id: format!("pad_{number}"),
            number: number.into(),
            at: eda_model::ir::Point { x, y },
            offset: eda_model::ir::Point { x: Nm::ZERO, y: Nm::ZERO },
            size: (mm(1.0), mm(1.0)),
            shape,
            kind,
            drill: if kind == PadKind::Smd { None } else { Some(mm(0.6)) },
            drill_slot: None,
            rot: 0,
            roundrect_ratio: Some(0.25),
            trapezoid_delta: None,
            chamfer_ratio: None,
            chamfer_corners: eda_model::ir::ChamferCorners::default(),
            custom: None,
            layers: vec![],
            clearance_override: None,
            thermal_gap_override: None,
            thermal_spoke_width_override: None,
            zone_connection: None,
            thermal_spoke_angle_mdeg: None,
            stack: Default::default(),
            kicad: Default::default(),
        }
    }

    #[test]
    fn export_kicad_mod_round_trips_through_this_crates_own_reader() {
        let fp = eda_model::ir::LibraryFootprint {
            name: "Test:RoundTrip".into(),
            description: "a test footprint".into(),
            keywords: "test".into(),
            attributes: eda_model::ir::FootprintAttributes { smd: true, ..Default::default() },
            pads: vec![
                lib_pad("1", mm(-1.0), Nm::ZERO, eda_model::ir::LibraryPadShape::RoundRect, PadKind::Smd),
                lib_pad("2", mm(1.0), Nm::ZERO, eda_model::ir::LibraryPadShape::Circle, PadKind::Smd),
                lib_pad("3", Nm::ZERO, mm(1.5), eda_model::ir::LibraryPadShape::Oval, PadKind::ThroughHole),
            ],
            graphics: vec![eda_model::ir::Shape::Rect { id: "g1".into(), layer: "F.CrtYd".into(), stroke_width: mm(0.05), filled: false, start: eda_model::ir::Point { x: mm(-2.0), y: mm(-2.0) }, end: eda_model::ir::Point { x: mm(2.0), y: mm(2.0) } }],
            texts: vec![],
            fields: vec![],
            reference_visible: true,
            value_visible: true,
            reference: None,
            value: None,
            courtyard: None,
            model: Some("${KICAD10_3DMODEL_DIR}/Test.3dshapes/RoundTrip.step".into()),
            models3d: vec![],
            anchor: eda_model::ir::Point { x: Nm::ZERO, y: Nm::ZERO },
            published: false,
            zone_connection: None,
            pad_edit: None,
            datasheet: None,
            description_field: None,
            kicad: Default::default(),
        };

        let text = export_kicad_mod(&fp);
        let parsed = parse_footprint_file(&text, "Test:RoundTrip").expect("the exported file must parse back through this crate's own reader");
        assert_eq!(parsed.pads.len(), 3);
        assert!(parsed.validate().is_empty(), "{:?}", parsed.validate());

        let by_num = |n: &str| parsed.pads.iter().find(|p| p.number == n).unwrap();
        assert_eq!(by_num("1").shape, eda_model::PadShape::RoundRect);
        assert_eq!(by_num("1").at, (mm(-1.0), Nm::ZERO));
        assert_eq!(by_num("2").shape, eda_model::PadShape::Circle);
        assert_eq!(by_num("3").kind, PadKind::ThroughHole);
        assert_eq!(by_num("3").drill, Some(mm(0.6)));
        assert!(parsed.courtyard.is_some(), "the exported F.CrtYd rect must be picked back up as the courtyard");
        assert_eq!(parsed.model.as_deref(), Some("${KICAD10_3DMODEL_DIR}/Test.3dshapes/RoundTrip.step"));
    }

    /// The fields grid of Footprint Properties: the Reference and the Value with their places, the user fields with theirs -- written as KiCad 8 writes a footprint's
    /// properties, and read back as the same fields. A footprint that says nothing about its Reference and Value is written without them, as before.
    #[test]
    fn export_kicad_mod_writes_the_fields_with_their_layouts_and_reads_them_back() {
        use eda_model::fp_edit::{new_library_field_layout, FieldLayout};
        use eda_model::ir::{FootprintField, Point};
        // E1 (r2-editors-exports): a footprint with no layouts of its own writes `CreateNewFootprint`'s Reference and Value, not nothing (KiCad would read both empty).
        let plain = export_kicad_mod(&eda_model::ir::LibraryFootprint::new_empty("Test:Plain"));
        assert!(plain.contains("(property \"Reference\" \"REF**\" (at 0 -0.5 0) (layer \"F.SilkS\")"), "{plain}");
        assert!(plain.contains("(property \"Value\" \"Test:Plain\" (at 0 1 0) (layer \"F.Fab\")"), "{plain}");

        let mut reference = FieldLayout::new(Point { x: mm(0.5), y: mm(-2.5) }, "F.SilkS");
        reference.size = (mm(1.2), mm(1.0));
        reference.thickness = mm(0.18);
        reference.text = Some("J**".into());
        let mut value = FieldLayout::new(Point { x: mm(-0.3), y: mm(2.2) }, "F.Fab");
        value.italic = true;
        value.angle = 90_000;
        let mut mpn = new_library_field_layout();
        mpn.at = Point { x: mm(1.5), y: mm(3.0) };
        mpn.layer = "F.Fab".into();
        mpn.bold = true;
        mpn.halign = -1;
        let fp = eda_model::ir::LibraryFootprint {
            reference: Some(reference.clone()),
            value: Some(value.clone()),
            value_visible: false,
            fields: vec![
                FootprintField { name: "MPN".into(), value: "ABC-1".into(), visible: true, layout: Some(FieldLayout { visible: true, ..mpn.clone() }) },
                FootprintField { name: "Vendor".into(), value: "ACME \"Inc\"".into(), visible: false, layout: None },
            ],
            ..eda_model::ir::LibraryFootprint::new_empty("Test:Fields")
        };
        let text = export_kicad_mod(&fp);
        assert!(text.contains("(property \"Reference\" \"J**\" (at 0.5 -2.5 0) (layer \"F.SilkS\")"), "{text}");
        assert!(text.contains("(property \"Value\" \"Test:Fields\" (at -0.3 2.2 90) (layer \"F.Fab\") (hide yes)"), "{text}");
        assert!(text.contains("(property \"MPN\" \"ABC-1\" (at 1.5 3 0) (unlocked yes) (layer \"F.Fab\")") || text.contains("(property \"MPN\" \"ABC-1\" (at 1.5 3 0) (layer \"F.Fab\")"), "{text}");
        assert!(text.contains("(property \"Vendor\" \"ACME \\\"Inc\\\"\" (at 0 0 0)") && text.contains("(hide yes)"), "a field with no layout is hidden at the origin: {text}");
        assert!(text.contains("(justify left)") && text.contains("bold"), "{text}");

        let back = crate::parse_library_footprint(&text).expect("the editor's reader").footprint;
        assert_eq!(back.reference.as_ref().map(|l| (l.at, l.size, l.thickness, l.layer.as_str(), l.text.as_deref())), Some((reference.at, reference.size, reference.thickness, "F.SilkS", Some("J**"))));
        assert!(back.reference_visible && !back.value_visible, "the Value is hidden");
        let v = back.value.as_ref().expect("the Value's layout");
        assert_eq!((v.at, v.italic, v.angle, v.text.as_deref(), v.visible), (value.at, true, 90_000, None, false));
        assert_eq!(back.fields.len(), 2);
        let m = &back.fields[0];
        assert_eq!((m.name.as_str(), m.value.as_str(), m.visible), ("MPN", "ABC-1", true));
        let ml = m.layout.as_ref().expect("a layout");
        assert_eq!((ml.at, ml.layer.as_str(), ml.bold, ml.halign, ml.visible), (Point { x: mm(1.5), y: mm(3.0) }, "F.Fab", true, -1, true));
        assert_eq!((back.fields[1].name.as_str(), back.fields[1].value.as_str(), back.fields[1].visible), ("Vendor", "ACME \"Inc\"", false));
        // and a file read and written again says the same
        assert_eq!(export_kicad_mod(&back), text, "the fields survive a read and a write");
        // KiCad's own reader takes it too
        let engine = parse_footprint_file(&text, "Test:Fields");
        assert!(engine.is_ok() || engine.unwrap_err().contains("no pad"), "the engine's reader does not trip on the properties");
    }

    #[test]
    fn export_kicad_mod_writes_trapezoid_chamfered_and_custom_pads_as_kicad_does_and_reads_them_back() {
        use eda_model::footprint::{CustomZoneMode, PadAnchor, PadCustom, PadPrimitive};
        use eda_model::ir::{ChamferCorners, Point, Shape};
        let mut trap = lib_pad("1", Nm::ZERO, Nm::ZERO, eda_model::ir::LibraryPadShape::Trapezoid, PadKind::Smd);
        trap.trapezoid_delta = Some((mm(0.2), Nm::ZERO));
        trap.roundrect_ratio = None;
        let mut chf = lib_pad("2", mm(2.0), Nm::ZERO, eda_model::ir::LibraryPadShape::ChamferedRect, PadKind::Smd);
        chf.chamfer_ratio = Some(0.3);
        chf.chamfer_corners = ChamferCorners { top_left: true, bottom_right: true, ..Default::default() };
        chf.roundrect_ratio = Some(0.1);
        let mut custom = lib_pad("3", mm(4.0), Nm::ZERO, eda_model::ir::LibraryPadShape::Custom, PadKind::Smd);
        custom.roundrect_ratio = None;
        custom.custom = Some(PadCustom {
            anchor: PadAnchor::Rect,
            zone: CustomZoneMode::ConvexHull,
            primitives: vec![PadPrimitive { shape: Shape::Polygon { id: String::new(), layer: String::new(), stroke_width: Nm::ZERO, filled: true, pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(1.5), y: Nm::ZERO }, Point { x: Nm::ZERO, y: mm(1.5) }] }, proxy: false }],
        });
        let fp = eda_model::ir::LibraryFootprint { name: "Test:Shapes".into(), pads: vec![trap, chf, custom], ..eda_model::ir::LibraryFootprint::new_empty("Test:Shapes") };

        let text = export_kicad_mod(&fp);
        assert!(text.contains("trapezoid (at 0 0 0) (size 1 1) (rect_delta 0.2 0)"), "{text}");
        assert!(text.contains("(roundrect_rratio 0.1) (chamfer_ratio 0.3) (chamfer top_left bottom_right)"), "{text}");
        assert!(text.contains("(options (clearance convexhull) (anchor rect)) (primitives (gr_poly (pts (xy 0 0) (xy 1.5 0) (xy 0 1.5)) (width 0) (fill yes)))"), "{text}");
        let parsed = parse_footprint_file(&text, "Test:Shapes").expect("a footprint with these pads loads");
        assert!(parsed.validate().is_empty());
        let by_num = |n: &str| parsed.pads.iter().find(|p| p.number == n).unwrap();
        assert_eq!((by_num("1").shape, by_num("1").ext.trapezoid_delta), (eda_model::PadShape::Trapezoid, Some((mm(0.2), Nm::ZERO))));
        assert_eq!((by_num("2").shape, by_num("2").roundrect_ratio, by_num("2").ext.chamfer_ratio), (eda_model::PadShape::ChamferedRect, Some(0.1), Some(0.3)));
        assert_eq!(by_num("3").shape, eda_model::PadShape::Custom);
        assert_eq!(by_num("3").ext.custom.as_ref().map(|c| (c.anchor, c.zone, c.primitives.len())), Some((PadAnchor::Rect, CustomZoneMode::ConvexHull, 1)));
        // And the editor's own reader gets the same library pads back.
        let back = crate::parse_library_footprint(&text).expect("the editor's reader").footprint;
        assert_eq!(back.pads.iter().map(|p| p.shape).collect::<Vec<_>>(), [eda_model::ir::LibraryPadShape::Trapezoid, eda_model::ir::LibraryPadShape::ChamferedRect, eda_model::ir::LibraryPadShape::Custom]);
        assert_eq!(back.pads[2].custom, fp.pads[2].custom);
        assert_eq!((back.pads[1].chamfer_ratio, back.pads[1].roundrect_ratio, back.pads[1].chamfer_corners), (Some(0.3), Some(0.1), fp.pads[1].chamfer_corners));
    }
    /// `(model ...)` lines come with their placement (`parse3DModel`): the offset in millimetres (a legacy `(at (xyz ..))` in inches), the scale, the rotation in
    /// degrees, the opacity and whether the model is hidden, in file order -- and the footprint keeps all of them, not only the first path.
    #[test]
    fn every_model_line_is_read_with_its_placement() {
        let text = r#"(footprint "Test:X" (layer "F.Cu") (attr smd exclude_from_pos_files)
            (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu"))
            (model "${KICAD10_3DMODEL_DIR}/A.3dshapes/a.step" (offset (xyz 1 -2 0.5)) (scale (xyz 1 1 1)) (rotate (xyz 90 0 180)))
            (model "b.wrl" (at (xyz 0.1 0 0)) (hide yes) (opacity 0.5))
            (model "c.step" (hide))
            (model "d.step" (scale (xyz 0.5 0.5 0.5)) (hide no))
            (model ""))"#;
        let models = footprint_models(text);
        assert_eq!(models.len(), 4, "a model with no path is no model: {models:?}");
        assert_eq!(models[0], Model3d { path: "${KICAD10_3DMODEL_DIR}/A.3dshapes/a.step".into(), offset: [1.0, -2.0, 0.5], scale: [1.0; 3], rotate: [90.0, 0.0, 180.0], opacity: 1.0, show: true });
        assert_eq!(models[1].offset, [2.54, 0.0, 0.0], "a legacy (at (xyz ..)) is in inches");
        assert!(!models[1].show && models[1].opacity == 0.5, "{:?}", models[1]);
        assert!(!models[2].show, "a bare (hide) hides it, like (hide yes)");
        assert!(models[3].show && models[3].scale == [0.5; 3], "(hide no) shows it: {:?}", models[3]);
        assert_eq!(footprint_attributes(text), vec!["smd".to_string(), "exclude_from_pos_files".to_string()]);
        assert!(footprint_models("not a footprint").is_empty() && footprint_attributes("(kicad_pcb)").is_empty());
        let parsed = parse_footprint_file(text, "Test:X").expect("a loadable footprint");
        assert_eq!(parsed.models3d.len(), 4);
        assert_eq!(parsed.model.as_deref(), Some("${KICAD10_3DMODEL_DIR}/A.3dshapes/a.step"), "`model` stays the first path");
    }

    /// The Footprint Editor's footprint keeps every model with its placement: the file it writes has them all, in order, and its own reader and the library
    /// importer give the same list back; an older entry that has only the path is written, and read, as the one model it was.
    #[test]
    fn a_library_footprint_exports_its_whole_list_of_models_and_reads_it_back() {
        let list = vec![
            Model3d { path: "${KICAD10_3DMODEL_DIR}/One.3dshapes/one.step".into(), offset: [1.0, 2.0, 3.0], scale: [1.0, 1.5, 1.0], rotate: [90.0, 0.0, -45.0], opacity: 1.0, show: true },
            Model3d { path: "two.wrl".into(), offset: [0.0, 0.0, 0.0], scale: [1.0, 1.0, 1.0], rotate: [0.0, 0.0, 0.0], opacity: 0.25, show: false },
        ];
        let mut fp = eda_model::ir::LibraryFootprint::new_empty("Test:Two");
        fp.pads.push(lib_pad("1", Nm::ZERO, Nm::ZERO, eda_model::ir::LibraryPadShape::Rect, PadKind::Smd));
        fp.models3d = list.clone();
        fp.model = Some(list[0].path.clone());
        let text = export_kicad_mod(&fp);
        assert_eq!(text.matches("(model ").count(), 2, "{text}");
        assert_eq!(footprint_models(&text), list, "the file's models, in order, with their placement");
        // the engine footprint the same file gives, and the Footprint Editor's own reader
        let engine = parse_footprint_file(&text, "Test:Two").expect("a loadable footprint");
        assert_eq!((engine.models3d.clone(), engine.model.clone()), (list.clone(), Some(list[0].path.clone())));
        let editor = crate::parse_library_footprint(&text).unwrap().footprint;
        assert_eq!((editor.models3d.clone(), editor.model.clone()), (list.clone(), Some(list[0].path.clone())));
        // and what the editor makes of a footprint the board can use keeps them: the engine's footprint is the same list
        assert_eq!(editor.to_engine_footprint().models3d, list);
        assert_eq!(eda_model::ir::LibraryFootprint::from_engine_footprint(&engine).models3d, list);

        // an older entry: the one path
        let mut old = eda_model::ir::LibraryFootprint::new_empty("Test:One");
        old.pads.push(lib_pad("1", Nm::ZERO, Nm::ZERO, eda_model::ir::LibraryPadShape::Rect, PadKind::Smd));
        old.model = Some("old.step".into());
        assert_eq!(footprint_models(&export_kicad_mod(&old)), vec![Model3d::identity("old.step")]);
        assert_eq!(old.to_engine_footprint().model.as_deref(), Some("old.step"));
        // an entry with no model writes none
        old.model = None;
        assert!(!export_kicad_mod(&old).contains("(model "));
    }

    /// `PCB_IO_KICAD_SEXPR::format( FOOTPRINT )`'s model entries: the hide flag, the opacity with four decimals, then the offset, the scale and the rotation in `FormatDouble2Str`'s
    /// numbers, in that order; a model with no name is not written. What is written reads back as the list it was written from.
    #[test]
    fn the_models_are_written_as_kicad_writes_them_and_read_back() {
        let list = vec![
            Model3d { path: "${KICAD10_3DMODEL_DIR}/A.3dshapes/a.step".into(), offset: [1.27, -2.0, 0.5], scale: [1.0, 1.0, 1.5], rotate: [90.0, 0.0, -30.5], opacity: 0.65, show: true },
            Model3d { path: "b.wrl".into(), offset: [0.0, 0.00001, -0.0], scale: [0.00002, 1.0, 1.0], rotate: [0.0, 0.0, 180.0], opacity: 1.0, show: false },
            Model3d::identity(""),
        ];
        let mut out = String::new();
        write_models(&mut out, &list, 2);
        let want = "\t\t(model \"${KICAD10_3DMODEL_DIR}/A.3dshapes/a.step\"\n\t\t\t(opacity 0.6500)\n\t\t\t(offset\n\t\t\t\t(xyz 1.27 -2 0.5)\n\t\t\t)\n\t\t\t(scale\n\t\t\t\t(xyz 1 1 1.5)\n\t\t\t)\n\t\t\t(rotate\n\t\t\t\t(xyz 90 0 -30.5)\n\t\t\t)\n\t\t)\n\t\t(model \"b.wrl\"\n\t\t\t(hide yes)\n\t\t\t(offset\n\t\t\t\t(xyz 0 0.00001 0)\n\t\t\t)\n\t\t\t(scale\n\t\t\t\t(xyz 0.00002 1 1)\n\t\t\t)\n\t\t\t(rotate\n\t\t\t\t(xyz 0 0 180)\n\t\t\t)\n\t\t)\n";
        assert_eq!(out, want);
        // and read as a footprint's models
        let back = footprint_models(&format!("(footprint \"T:X\"\n{out})"));
        assert_eq!(back.len(), 2);
        assert_eq!(back[0], list[0]);
        assert_eq!(back[1], Model3d { offset: [0.0, 0.00001, 0.0], ..list[1].clone() }, "a negative zero comes back as zero");
    }
}
