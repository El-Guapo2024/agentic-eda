//! The Extruded 3D Body of a footprint: KiCad's `EXTRUDED_3D_BODY` (`pcbnew/footprint.h` at 8303b2ad), a body the 3D viewer makes of the footprint's own outline when it has no
//! model of its own -- the outline (its courtyard, its fab or silkscreen drawings, or the box of its pads) raised from the board by `standoff` to `height`, in a material and a
//! colour -- written in the board and footprint files as a typed model, `(model (type extruded) (hide no) (overall_height ..) (body_pcb_gap ..) (layer ..) (material ..)
//! (color ..) (offset (xyz ..)) (scale (xyz ..)) (rotate (xyz ..)))` (`PCB_IO_KICAD_SEXPR::format( FOOTPRINT )`, "Save extruded 3D body info.", and `parseFOOTPRINT`'s
//! "Typed model (model (type extruded) ...)").
//!
//! The reader and writer of the files are `eda_kicad`'s (`crates/kicad/src/extruded_body_io.rs`); the outline the body is made of is the server's
//! (`crates/cli/src/extruded_body_api.rs`, `GetExtrusionOutline`), the drawing the 3D view's (`web/studio/src/components/viewer3d/extrudedBody.ts`). Where it is kept: a footprint on the
//! board has it in [`crate::ir::FootprintExtra::extruded_body`] (what its board file said) and, once the 3D Models tab has set it, [`crate::fp_edit::FootprintEdit::extruded_body`]; a
//! footprint of the project library in [`crate::ir::LibraryFootprint::extruded_body`]. All additive: a design without one reads and writes as before.

use crate::units::Nm;
use serde::{Deserialize, Serialize};

/// `EXTRUSION_MATERIAL`: how the body is lit (`GetMaterialProps`) and its colour when it has none of its own (`EXTRUDED_3D_BODY::GetDefaultColor`). The file's words are the
/// variants' (`matNames[] = { "plastic", "matte", "metal", "copper" }`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtrusionMaterial {
    #[default]
    Plastic,
    Matte,
    Metal,
    Copper,
}

impl ExtrusionMaterial {
    /// The word the file writes (`matNames`).
    pub fn token(self) -> &'static str {
        match self {
            ExtrusionMaterial::Plastic => "plastic",
            ExtrusionMaterial::Matte => "matte",
            ExtrusionMaterial::Metal => "metal",
            ExtrusionMaterial::Copper => "copper",
        }
    }

    /// `parseFOOTPRINT`'s `T_material`: `matte`, `metal`, `copper`, and anything else plastic.
    pub fn from_token(word: &str) -> ExtrusionMaterial {
        match word {
            "matte" => ExtrusionMaterial::Matte,
            "metal" => ExtrusionMaterial::Metal,
            "copper" => ExtrusionMaterial::Copper,
            _ => ExtrusionMaterial::Plastic,
        }
    }

    /// `EXTRUDED_3D_BODY::GetDefaultColor`: the colour of a body that has none of its own, `(r, g, b, a)`.
    pub fn default_color(self) -> [f64; 4] {
        match self {
            ExtrusionMaterial::Plastic => [0.2, 0.2, 0.2, 1.0],
            ExtrusionMaterial::Matte => [0.4, 0.4, 0.4, 1.0],
            ExtrusionMaterial::Metal => [0.7, 0.7, 0.7, 1.0],
            ExtrusionMaterial::Copper => [0.72, 0.45, 0.2, 1.0],
        }
    }
}

/// `m_layer`'s "Auto" (`UNDEFINED_LAYER`: the courtyard, else the fab drawings, else the silkscreen, else the pads' box) as the file writes it.
pub const LAYER_AUTO: &str = "auto";
/// `m_layer`'s "Pin bounding box" (`UNSELECTED_LAYER`) as the file writes it.
pub const LAYER_PAD_BBOX: &str = "pad_bbox";

/// One footprint's Extruded 3D Body (`EXTRUDED_3D_BODY`). Lengths in nanometres, the placement as the file has it (millimetres and degrees).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtrudedBody {
    /// `m_height`: the body's overall height above the board, the standoff included (`(overall_height ..)`). The file has a body only when it is above zero.
    #[serde(deserialize_with = "crate::units::de")]
    pub height: Nm,
    /// `m_standoff`: the gap between the board and the body's underside (`(body_pcb_gap ..)`).
    #[serde(default, skip_serializing_if = "is_zero_nm")]
    #[serde(deserialize_with = "crate::units::de")]
    pub standoff: Nm,
    /// `m_layer`: what the outline is made of -- [`LAYER_AUTO`], [`LAYER_PAD_BBOX`] or a layer's canonical name (`F.CrtYd`, `F.Fab`, `F.SilkS`; the 3D Models tab offers these).
    /// For a footprint on the board this is the layer of the footprint as its library has it, on the top side: KiCad's `FOOTPRINT::Flip` flips it (`FlipLayer`) with the
    /// footprint, and the board's reader and writer flip it for a footprint on the back ([`flip_layer`]).
    #[serde(default = "auto_layer", skip_serializing_if = "is_auto_layer")]
    pub layer: String,
    #[serde(default, skip_serializing_if = "is_plastic")]
    pub material: ExtrusionMaterial,
    /// `m_color` as `(r, g, b, a)` in 0..1; `None` is `COLOR4D::UNSPECIFIED`, the material's own colour ([`ExtrusionMaterial::default_color`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[f64; 4]>,
    /// `m_offset`, millimetres: x and y move the outline on the board (`ApplyExtrusionTransform`), z lifts the body.
    #[serde(default = "zero3", skip_serializing_if = "is_zero3")]
    pub offset: [f64; 3],
    /// `m_scale`: x and y scale the outline about the footprint's position, z the body's thickness.
    #[serde(default = "one3", skip_serializing_if = "is_one3")]
    pub scale: [f64; 3],
    /// `m_rotation`, degrees: z turns the outline about the footprint's position (x and y are kept, and drawn by nothing in KiCad).
    #[serde(default = "zero3", skip_serializing_if = "is_zero3")]
    pub rotate: [f64; 3],
    /// `m_show`: `(hide yes)` clears it.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub show: bool,
}

fn is_zero_nm(v: &Nm) -> bool {
    *v == Nm::ZERO
}
fn auto_layer() -> String {
    LAYER_AUTO.to_string()
}
fn is_auto_layer(l: &str) -> bool {
    l == LAYER_AUTO
}
fn is_plastic(m: &ExtrusionMaterial) -> bool {
    *m == ExtrusionMaterial::Plastic
}
fn zero3() -> [f64; 3] {
    [0.0; 3]
}
fn one3() -> [f64; 3] {
    [1.0; 3]
}
fn is_zero3(v: &[f64; 3]) -> bool {
    *v == [0.0; 3]
}
fn is_one3(v: &[f64; 3]) -> bool {
    *v == [1.0; 3]
}
fn yes() -> bool {
    true
}
fn is_yes(v: &bool) -> bool {
    *v
}

impl Default for ExtrudedBody {
    /// `EXTRUDED_3D_BODY()`: no height, no standoff, the layer `UNDEFINED_LAYER` (auto), no colour of its own, plastic, shown, the placement as it comes.
    fn default() -> Self {
        ExtrudedBody { height: Nm::ZERO, standoff: Nm::ZERO, layer: auto_layer(), material: ExtrusionMaterial::Plastic, color: None, offset: zero3(), scale: one3(), rotate: zero3(), show: true }
    }
}

impl ExtrudedBody {
    /// The colour the body is drawn in: its own, else its material's (`if( c == COLOR4D::UNSPECIFIED ) c = GetDefaultColor( body->m_material )`).
    pub fn drawn_color(&self) -> [f64; 4] {
        self.color.unwrap_or_else(|| self.material.default_color())
    }

    /// The same body on the other side of the board (`FOOTPRINT::Flip`: "Flip the extrusion source layer to match the new side", `if( m_layer != UNDEFINED_LAYER )`); the placement
    /// is the footprint's own and is not flipped.
    pub fn flipped(&self) -> ExtrudedBody {
        ExtrudedBody { layer: flip_layer(&self.layer), ..self.clone() }
    }
}

/// `BOARD::FlipLayer` for the layers a body names: a front layer's back twin and the other way round (`F.CrtYd` and `B.CrtYd`, `F.Fab` and `B.Fab`, ...); [`LAYER_AUTO`],
/// [`LAYER_PAD_BBOX`] and every other name as it is (an inner copper layer, which KiCad turns over the stackup, names no outline a body is made of).
pub fn flip_layer(layer: &str) -> String {
    if let Some(rest) = layer.strip_prefix("F.") {
        format!("B.{rest}")
    } else if let Some(rest) = layer.strip_prefix("B.") {
        format!("F.{rest}")
    } else {
        layer.to_string()
    }
}

/// What the 3D Models tab's Extruded 3D Body box sets on a footprint on the board (`PANEL_FP_PROPERTIES_3D_MODEL::TransferDataFromWindow`): the body, or none (`ClearExtrudedBody`,
/// the "Extruded 3D body" box off). Kept as [`crate::fp_edit::FootprintEdit::extruded_body`]; `None` there is the footprint's own body, from its board file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtrudedBodyEdit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<ExtrudedBody>,
}

/// Why the box's values are refused (`TransferDataFromWindow`'s messages, word for word, with its title "Extruded 3D Body").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyProblem {
    /// "Component height must be greater than zero."
    Height,
    /// "Standoff height must be less than the overall height."
    Standoff,
}

impl BodyProblem {
    pub fn message(self) -> &'static str {
        match self {
            BodyProblem::Height => "Component height must be greater than zero.",
            BodyProblem::Standoff => "Standoff height must be less than the overall height.",
        }
    }
}

/// `TransferDataFromWindow`'s checks of a body the box sets: a height above zero, a negative standoff made zero (`if( standoff < 0.0 ) standoff = 0.0;`), and a standoff below the
/// height. The body as it is stored.
pub fn checked_body(body: &ExtrudedBody) -> Result<ExtrudedBody, BodyProblem> {
    if body.height <= Nm::ZERO {
        return Err(BodyProblem::Height);
    }
    let standoff = body.standoff.max(Nm::ZERO);
    if standoff >= body.height {
        return Err(BodyProblem::Standoff);
    }
    Ok(ExtrudedBody { standoff, ..body.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_starts_as_kicads_constructor_makes_one_and_its_colour_is_its_materials() {
        let b = ExtrudedBody::default();
        assert_eq!((b.height, b.standoff, b.layer.as_str(), b.material, b.color, b.scale, b.show), (Nm::ZERO, Nm::ZERO, "auto", ExtrusionMaterial::Plastic, None, [1.0; 3], true));
        for (m, c) in [(ExtrusionMaterial::Plastic, [0.2, 0.2, 0.2, 1.0]), (ExtrusionMaterial::Matte, [0.4, 0.4, 0.4, 1.0]), (ExtrusionMaterial::Metal, [0.7, 0.7, 0.7, 1.0]), (ExtrusionMaterial::Copper, [0.72, 0.45, 0.2, 1.0])] {
            assert_eq!(ExtrudedBody { material: m, ..b.clone() }.drawn_color(), c);
            assert_eq!(ExtrusionMaterial::from_token(m.token()), m);
        }
        assert_eq!(ExtrusionMaterial::from_token("wood"), ExtrusionMaterial::Plastic, "anything else is plastic");
        assert_eq!(ExtrudedBody { color: Some([1.0, 0.0, 0.0, 0.5]), ..b }.drawn_color(), [1.0, 0.0, 0.0, 0.5]);
    }

    #[test]
    fn a_flip_turns_the_layer_over_and_nothing_else() {
        assert_eq!((flip_layer("F.CrtYd"), flip_layer("B.Fab"), flip_layer("auto"), flip_layer("pad_bbox"), flip_layer("In1.Cu")), ("B.CrtYd".into(), "F.Fab".into(), "auto".into(), "pad_bbox".into(), "In1.Cu".into()));
        let b = ExtrudedBody { layer: "F.SilkS".into(), offset: [1.0, 2.0, 0.0], rotate: [0.0, 0.0, 30.0], ..Default::default() };
        let f = b.flipped();
        assert_eq!((f.layer.as_str(), f.offset, f.rotate), ("B.SilkS", [1.0, 2.0, 0.0], [0.0, 0.0, 30.0]));
        assert_eq!(f.flipped(), b);
    }

    #[test]
    fn the_box_refuses_what_transfer_data_from_window_refuses_in_its_words() {
        let mm = Nm::from_mm;
        let b = |h: f64, s: f64| ExtrudedBody { height: mm(h), standoff: mm(s), ..Default::default() };
        assert_eq!(checked_body(&b(0.0, 0.0)), Err(BodyProblem::Height));
        assert_eq!(BodyProblem::Height.message(), "Component height must be greater than zero.");
        assert_eq!(checked_body(&b(1.0, 1.0)), Err(BodyProblem::Standoff));
        assert_eq!(BodyProblem::Standoff.message(), "Standoff height must be less than the overall height.");
        assert_eq!(checked_body(&b(1.0, -0.5)).unwrap().standoff, Nm::ZERO, "a negative standoff is none");
        assert_eq!(checked_body(&b(2.0, 0.5)), Ok(b(2.0, 0.5)));
    }

    #[test]
    fn a_body_is_kept_in_the_design_with_only_what_differs_from_the_defaults() {
        let b = ExtrudedBody { height: Nm::from_mm(2.0), ..Default::default() };
        assert_eq!(serde_json::to_string(&b).unwrap(), r#"{"height":2000000}"#);
        let full = ExtrudedBody { standoff: Nm::from_mm(0.5), layer: "F.Fab".into(), material: ExtrusionMaterial::Metal, color: Some([0.1, 0.2, 0.3, 1.0]), offset: [1.0, 0.0, 0.0], scale: [1.0, 2.0, 1.0], rotate: [0.0, 0.0, 45.0], show: false, ..b };
        let text = serde_json::to_string(&full).unwrap();
        assert_eq!(serde_json::from_str::<ExtrudedBody>(&text).unwrap(), full);
        assert!(serde_json::from_str::<ExtrudedBody>(r#"{"height":1,"nope":2}"#).is_err(), "deny_unknown_fields");
    }
}
