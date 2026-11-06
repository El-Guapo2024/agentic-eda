//! `GET /api/fonts`: the font families the machine has, for the Font column of Symbol Properties' fields grid (`FIELDS_GRID_TABLE`'s `m_fontAttr`, a
//! `GRID_CELL_COMBOBOX` of `Fontconfig()->ListFonts( fontNames, .. )`, sorted, after "Default Font" and "KiCad Font", which the studio adds).
//!
//! KiCad asks fontconfig in process (`FONTCONFIG::ListFonts`, common/font/fontconfig.cpp). The studio runs `fc-list : family`, fontconfig's own lister, on the
//! machine the server is on, and keeps the first name of each family (fontconfig lists a family's names in its languages, `a,b,c`; KiCad picks the one of the
//! user's language, then the first). No `fc-list`: no families, and the column still takes a name typed in. `{ "ok": true, "families": [..] }`.

use serde_json::{json, Value};
use std::process::Command;

/// The families `fc-list : family` prints, one name each (the first of a line's comma-separated names), sorted and once.
pub(crate) fn families_of(listing: &str) -> Vec<String> {
    let mut out: Vec<String> = listing.lines().filter_map(|l| l.split(',').next()).map(|f| f.trim().replace("\\-", "-")).filter(|f| !f.is_empty()).collect();
    out.sort();
    out.dedup();
    out
}

pub(crate) fn fonts() -> Value {
    match Command::new("fc-list").args([":", "family"]).output() {
        Ok(o) if o.status.success() => json!({ "ok": true, "families": families_of(&String::from_utf8_lossy(&o.stdout)) }),
        _ => json!({ "ok": true, "families": Vec::<String>::new() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_familys_first_name_is_kept_once_and_the_list_is_sorted() {
        let listing = "DejaVu Sans\nIPAGothic,IPAゴシック\nDejaVu Sans\nFreeSans\n\n";
        assert_eq!(families_of(listing), ["DejaVu Sans", "FreeSans", "IPAGothic"]);
    }
}
