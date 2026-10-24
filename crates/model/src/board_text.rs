//! What a board text has beyond [`crate::ir::Text`]'s own fields.
//!
//! `Text` is a square-sized, horizontally justified line of stroke text: its size, its stroke width, its horizontal justification and its mirror. KiCad's
//! `PCB_TEXT` (`EDA_TEXT::Format`'s `(effects (font ..) (justify ..))`) says more, and the rest decides how large the text is and where it sits, which is what
//! kicad-cli's clearance, short-circuit and silkscreen checks measure: a glyph width that is not the height (`(size 2.032 1.524)`), a vertical justification
//! (`(justify left bottom)` puts the text above its anchor, not centred on it), bold and italic, a knockout, the line spacing of a text of several lines, a font face and
//! an automatic stroke width. A board that came from a KiCad file keeps them here, one [`TextExtra`] per text that has any, in `DrawingsSection::text_extras` and keyed
//! by the text's id, as `footprint_extras` is keyed by a footprint's reference: the importer reads them, the writer writes them back, and a text nothing was read for is
//! written as it always was. The overlay is additive: a design without it reads as "every text is a square-sized, vertically centred, regular line with a stroke width".

use crate::ir::{TextVAlign, Nm};
use serde::{Deserialize, Serialize};

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

/// The parts of one board text's `(effects ..)` that [`crate::ir::Text`] has no field for. See the module doc.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextExtra {
    /// The id of the text it belongs to (`Text::id`).
    pub id: String,
    /// The glyph width, nm, when it is not the height (`(size HEIGHT WIDTH)`; `Text::size_nm` is the height).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(alias = "width_um", deserialize_with = "crate::units::de")]
    pub width_nm: Option<Nm>,
    /// The file had no `(thickness ..)`: KiCad draws the text with its automatic stroke width (`GetPenSizeForNormal`, or `GetPenSizeForBold` for a bold one), and the writer
    /// leaves the token out. `Text::stroke_width` then holds that width.
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_thickness: bool,
    /// Vertical justification (`justify top|bottom`); centred when absent.
    #[serde(default)]
    pub valign: TextVAlign,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    /// `(layer "F.Cu" knockout)`: the text is cut out of a filled area (`PCB_TEXT::IsKnockout`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub knockout: bool,
    /// `(line_spacing N)` in thousandths, for a text of several lines; 0 is KiCad's 1.0, and 1000 is the same.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub line_spacing_permille: i64,
    /// `(face "..")`: the outline font the text is set in. Stroke text, the only kind drawn here, has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
}

impl TextExtra {
    /// Whether this says nothing a text without one would not (the importer keeps no entry for such a text).
    pub fn is_default(&self) -> bool {
        let TextExtra { id: _, width_nm, auto_thickness, valign, bold, italic, knockout, line_spacing_permille, face } = self;
        width_nm.is_none() && !auto_thickness && *valign == TextVAlign::Center && !bold && !italic && !knockout && (*line_spacing_permille == 0 || *line_spacing_permille == 1000) && face.is_none()
    }
}

impl crate::ir::DrawingsSection {
    /// The extras of the text `id`, if it has any.
    pub fn text_extra(&self, id: &str) -> Option<&TextExtra> {
        self.text_extras.iter().find(|e| e.id == id)
    }
}
