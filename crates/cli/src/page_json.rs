//! The state JSON of a document's paper and title block (Page Settings, `common.Control.pageSettings`): the board's in
//! `GET /api/state` (`page`, `title_block`) and the schematic's in `GET /api/schematic` (`page`, `title_block`). `null` is
//! the default -- A4 landscape, an empty title block -- so a design that never opened the dialog reads exactly as before.

use eda_model::ir::TitleBlock;
use eda_model::page::PageSettings;
use serde_json::{json, Value};

/// `{ paper, portrait, user_size_um, size_um }`: the paper as the dialog edits it and its resolved width and height, µm.
pub(crate) fn page_json(page: &PageSettings) -> Value {
    json!({
        "paper": page.paper,
        "portrait": page.portrait,
        "user_size_um": page.user_size_um.map(|(w, h)| json!([w, h])),
        "size_um": page.size_um().ok().map(|(w, h)| json!([w, h])),
    })
}

pub(crate) fn title_block_json(tb: &TitleBlock) -> Value {
    json!({ "title": tb.title, "date": tb.date, "rev": tb.rev, "company": tb.company, "comments": tb.comments })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_standard_paper_reports_its_size_in_its_orientation() {
        let v = page_json(&PageSettings { paper: "A4".into(), portrait: true, user_size_um: None });
        assert_eq!(v["paper"], "A4");
        assert_eq!(v["portrait"], true);
        assert_eq!(v["size_um"], json!([210_000, 297_000]));
        assert_eq!(v["user_size_um"], Value::Null);
    }

    #[test]
    fn a_user_paper_reports_the_size_it_was_given() {
        let v = page_json(&PageSettings { paper: "User".into(), portrait: false, user_size_um: Some((300_000, 200_000)) });
        assert_eq!(v["size_um"], json!([300_000, 200_000]));
        assert_eq!(v["user_size_um"], json!([300_000, 200_000]));
    }

    #[test]
    fn the_title_block_keeps_every_field() {
        let v = title_block_json(&TitleBlock { title: "T".into(), date: "2026-10-01".into(), rev: "B".into(), company: "Co".into(), comments: vec!["one".into()], ..Default::default() });
        assert_eq!(v, json!({ "title": "T", "date": "2026-10-01", "rev": "B", "company": "Co", "comments": ["one"] }));
    }
}
