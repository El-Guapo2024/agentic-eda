//! SVG reader (step 2 of the Import Graphics port): a stub until the nanosvg-equivalent lands.

use crate::graphics_import::Imported;

/// Reads an SVG file into shapes in millimetres.
pub fn read_svg(_text: &str) -> Result<Imported, String> {
    Err("SVG import is not available yet".into())
}
