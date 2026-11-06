//! A footprint's Extruded 3D Body in the board and footprint files: the typed model `(model (type extruded) ..)` that `PCB_IO_KICAD_SEXPR::format( FOOTPRINT )` writes ("Save extruded
//! 3D body info.") and `PCB_IO_KICAD_SEXPR_PARSER::parseFOOTPRINT` reads ("Typed model (model (type extruded) ...)"), `pcbnew/pcb_io/kicad_sexpr/` at 8303b2ad. The body itself is
//! [`eda_model::extruded_body`].
//!
//! The board's reader keeps it as [`eda_model::ir::FootprintExtra::extruded_body`] and the footprint library's reader as [`eda_model::ir::LibraryFootprint::extruded_body`]; both
//! writers write it after the footprint's embedded files and before its `(model "file" ..)` entries, where KiCad writes it. A footprint on the back has its body's layer flipped by
//! KiCad's `FOOTPRINT::Flip`; the board's reader and writer keep it in the footprint's top-side frame ([`eda_model::extruded_body::ExtrudedBody::flipped`]), so a round trip is
//! exact. KiCad 25ec2335 (the box's kicad-cli) adds the body's `(uuid ..)` (`SEXPR_BOARD_FILE_VERSION` 20260930, "UUID for extruded 3D body"): it is read past, and not
//! written, since 8303b2ad's parser refuses it.

use crate::sexpr::{self, Sexpr};
use eda_model::extruded_body::{ExtrudedBody, ExtrusionMaterial, LAYER_AUTO, LAYER_PAD_BBOX};
use eda_model::units::Nm;
use std::fmt::Write as _;

/// Whether a footprint's `(model ..)` form is a typed one -- `(model (type ..) ..)`, no file name -- and of the extruded type.
pub fn is_extruded(item: &[Sexpr]) -> bool {
    sexpr::tag(item) == Some("model") && item.get(1).and_then(Sexpr::as_list).is_some_and(|t| sexpr::tag(t) == Some("type") && sexpr::txt(t, 1) == Some("extruded"))
}

/// `parseFOOTPRINT`'s `case T_model:` for a typed model: the body `item` makes of `onto` (`EnsureExtrudedBody()`: a second typed model goes on setting the one body), or
/// `None` when `item` is no extruded typed model. Each form is read as the parser reads it: `(hide)`, `(hide yes)` and `(hide no)` (`parseMaybeAbsentBool( true )`),
/// `(overall_height mm)` and `(body_pcb_gap mm)` (`parseBoardUnits`), `(layer auto|pad_bbox|"name")` (a name that is no layer leaves the layer as it was: `if( layer >= 0 )`),
/// `(material ..)` (anything not `matte`, `metal` or `copper` is plastic), `(color unspecified)` or `(color r g b a)`, and `(offset|scale|rotate (xyz x y z))`. A form the
/// parser would refuse ("Expecting hide, overall_height, ...") is passed over here: the rest of the file still reads.
pub fn read(item: &[Sexpr], onto: Option<ExtrudedBody>) -> Option<ExtrudedBody> {
    if !is_extruded(item) {
        return None;
    }
    let mut body = onto.unwrap_or_default();
    for form in item.iter().skip(2).filter_map(Sexpr::as_list) {
        match sexpr::tag(form) {
            // `hide = parseMaybeAbsentBool( true )`: `(hide)` and `(hide yes)` hide it, `(hide no)` shows it
            Some("hide") => body.show = matches!(sexpr::txt(form, 1), Some("no") | Some("false")),
            Some("overall_height") => {
                if let Some(v) = sexpr::num(form, 1) {
                    body.height = eda_model::units::mm(v);
                }
            }
            Some("body_pcb_gap") => {
                if let Some(v) = sexpr::num(form, 1) {
                    body.standoff = eda_model::units::mm(v);
                }
            }
            Some("layer") => match sexpr::txt(form, 1) {
                Some(LAYER_AUTO) => body.layer = LAYER_AUTO.to_string(),
                Some(LAYER_PAD_BBOX) => body.layer = LAYER_PAD_BBOX.to_string(),
                Some(name) if eda_model::layers::pcb_layer_id(name).is_some() => body.layer = name.to_string(),
                _ => {}
            },
            Some("material") => body.material = ExtrusionMaterial::from_token(sexpr::txt(form, 1).unwrap_or("")),
            Some("color") => {
                body.color = match sexpr::txt(form, 1) {
                    Some("unspecified") | None => None,
                    Some(_) => Some([1, 2, 3, 4].map(|i| sexpr::num(form, i).unwrap_or(0.0))),
                }
            }
            Some(tag @ ("offset" | "scale" | "rotate")) => {
                if let Some(xyz) = sexpr::find(form, "xyz") {
                    let v = [1, 2, 3].map(|i| sexpr::num(xyz, i).unwrap_or(0.0));
                    match tag {
                        "offset" => body.offset = v,
                        "scale" => body.scale = v,
                        _ => body.rotate = v,
                    }
                }
            }
            _ => {}
        }
    }
    Some(body)
}

/// `format( FOOTPRINT )`'s "Save extruded 3D body info.": nothing for a body with no height (`body->m_height > 0`), else every form, the defaults included -- `(hide no)`
/// (`FormatBool`), the lengths as `formatInternalUnits` writes them, the layer quoted when it is a layer's name, the colour `unspecified` or its four numbers, and the
/// placement with `FormatDouble2Str` -- indented `tabs` deep, laid out as KiCad's prettifier lays out a footprint's `(model ..)`.
pub fn write(out: &mut String, body: &ExtrudedBody, tabs: usize) {
    if body.height <= Nm::ZERO {
        return;
    }
    let t = "\t".repeat(tabs);
    let d = crate::fmt_double_2_str;
    let layer = match body.layer.as_str() {
        LAYER_PAD_BBOX => LAYER_PAD_BBOX.to_string(),
        LAYER_AUTO => LAYER_AUTO.to_string(),
        name => crate::sexpr_str(name),
    };
    let color = match body.color {
        Some([r, g, b, a]) => format!("{} {} {} {}", d(r), d(g), d(b), d(a)),
        None => "unspecified".to_string(),
    };
    writeln!(out, "{t}(model").unwrap();
    writeln!(out, "{t}\t(type extruded)").unwrap();
    writeln!(out, "{t}\t(hide {})", if body.show { "no" } else { "yes" }).unwrap();
    writeln!(out, "{t}\t(overall_height {})", crate::mm(body.height)).unwrap();
    writeln!(out, "{t}\t(body_pcb_gap {})", crate::mm(body.standoff)).unwrap();
    writeln!(out, "{t}\t(layer {layer})").unwrap();
    writeln!(out, "{t}\t(material {})", body.material.token()).unwrap();
    writeln!(out, "{t}\t(color {color})").unwrap();
    for (tag, v) in [("offset", body.offset), ("scale", body.scale), ("rotate", body.rotate)] {
        writeln!(out, "{t}\t({tag}\n{t}\t\t(xyz {} {} {})\n{t}\t)", d(v[0]), d(v[1]), d(v[2])).unwrap();
    }
    writeln!(out, "{t})").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<Sexpr> {
        sexpr::parse(text).unwrap().as_list().unwrap().to_vec()
    }

    #[test]
    fn a_typed_model_is_read_as_parse_footprint_reads_it() {
        let b = read(&parse("(model (type extruded) (hide no) (overall_height 2.5) (body_pcb_gap 0.3) (layer \"F.Fab\") (material metal) (color 0.1 0.2 0.3 0.9) (offset (xyz 1 -2 0.5)) (scale (xyz 1 1.5 2)) (rotate (xyz 0 0 45)))"), None).unwrap();
        assert_eq!((b.height, b.standoff, b.layer.as_str(), b.material, b.color, b.show), (Nm::from_mm(2.5), Nm::from_mm(0.3), "F.Fab", ExtrusionMaterial::Metal, Some([0.1, 0.2, 0.3, 0.9]), true));
        assert_eq!((b.offset, b.scale, b.rotate), ([1.0, -2.0, 0.5], [1.0, 1.5, 2.0], [0.0, 0.0, 45.0]));
        // `parseMaybeAbsentBool( true )`, `auto` and `pad_bbox`, `unspecified`, anything else plastic, a name that is no layer leaving the layer as it was, KiCad 25ec2335's uuid
        let c = read(&parse("(model (type extruded) (uuid \"0f0f\") (hide) (overall_height 1) (layer pad_bbox) (layer \"Nowhere\") (material wood) (color unspecified))"), None).unwrap();
        assert_eq!((c.show, c.layer.as_str(), c.material, c.color), (false, "pad_bbox", ExtrusionMaterial::Plastic, None));
        assert_eq!(read(&parse("(model (type extruded) (hide yes) (overall_height 1) (layer auto))"), None).map(|b| (b.show, b.layer)), Some((false, "auto".to_string())));
        // a second typed model goes on setting the same body
        let d = read(&parse("(model (type extruded) (material copper))"), Some(b.clone())).unwrap();
        assert_eq!((d.height, d.material), (b.height, ExtrusionMaterial::Copper));
        // a model with a file name, or a type nobody reads, is no body
        assert!(read(&parse("(model \"a.step\" (offset (xyz 0 0 0)))"), None).is_none());
        assert!(read(&parse("(model (type sphere) (overall_height 1))"), None).is_none());
    }

    #[test]
    fn a_body_is_written_as_format_footprint_writes_it_and_read_back() {
        let b = ExtrudedBody { height: Nm::from_mm(2.0), standoff: Nm::from_mm(0.25), layer: "F.CrtYd".into(), ..Default::default() };
        let mut out = String::new();
        write(&mut out, &b, 2);
        let words: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(
            words,
            "(model (type extruded) (hide no) (overall_height 2) (body_pcb_gap 0.25) (layer \"F.CrtYd\") (material plastic) (color unspecified) (offset (xyz 0 0 0) ) (scale (xyz 1 1 1) ) (rotate (xyz 0 0 0) ) )"
        );
        assert!(out.starts_with("\t\t(model\n\t\t\t(type extruded)\n"), "{out}");
        assert_eq!(read(&parse(&out), None), Some(b.clone()));
        let full = ExtrudedBody { layer: "pad_bbox".into(), material: ExtrusionMaterial::Matte, color: Some([0.5, 0.25, 1.0, 0.75]), offset: [0.1, -0.2, 0.3], scale: [2.0, 1.0, 0.5], rotate: [0.0, 0.0, -30.0], show: false, ..b.clone() };
        let mut out = String::new();
        write(&mut out, &full, 1);
        assert!(out.contains("(layer pad_bbox)") && out.contains("(hide yes)") && out.contains("(color 0.5 0.25 1 0.75)") && out.contains("(xyz 0.1 -0.2 0.3)"), "{out}");
        assert_eq!(read(&parse(&out), None), Some(full));
        // `body && body->m_height > 0`: a body with no height is not written
        let mut none = String::new();
        write(&mut none, &ExtrudedBody::default(), 1);
        assert!(none.is_empty());
    }

    #[test]
    fn a_library_footprints_body_is_read_and_written_with_its_footprint() {
        let text = r#"(footprint "Box"
	(version 20260410)
	(generator "pcbnew")
	(layer "F.Cu")
	(attr smd)
	(fp_rect (start -1 -0.5) (end 1 0.5) (stroke (width 0.05) (type default)) (fill no) (layer "F.CrtYd"))
	(pad "1" smd rect (at -0.6 0) (size 0.4 0.6) (layers "F.Cu" "F.Mask" "F.Paste"))
	(pad "2" smd rect (at 0.6 0) (size 0.4 0.6) (layers "F.Cu" "F.Mask" "F.Paste"))
	(model (type extruded) (hide no) (overall_height 1.2) (body_pcb_gap 0) (layer auto) (material matte) (color unspecified) (offset (xyz 0 0 0)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 0)))
	(model "${KICAD10_3DMODEL_DIR}/X.3dshapes/X.step" (offset (xyz 0 0 0)) (scale (xyz 1 1 1)) (rotate (xyz 0 0 0)))
)"#;
        let parsed = crate::parse_library_footprint(text).unwrap().footprint;
        let body = parsed.extruded_body.clone().expect("the body is the footprint's");
        assert_eq!((body.height, body.layer.as_str(), body.material), (Nm::from_mm(1.2), "auto", ExtrusionMaterial::Matte));
        assert!(!parsed.kicad.tokens.iter().any(|t| t.contains("extruded")), "not kept as text too: {:?}", parsed.kicad.tokens);
        assert_eq!(parsed.models3d.len(), 1, "the file model is a model as before");
        let out = crate::export_kicad_mod(&parsed);
        assert_eq!(out.matches("(type extruded)").count(), 1, "{out}");
        assert!(out.find("(type extruded)").unwrap() < out.find("(model \"${KICAD10_3DMODEL_DIR}").unwrap(), "before the models: {out}");
        assert_eq!(crate::parse_library_footprint(&out).unwrap().footprint.extruded_body, Some(body));
    }
}
