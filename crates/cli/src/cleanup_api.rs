//! HTTP handlers for "Cleanup Tracks & Vias..." (task item 1, GAPS.md
//! honorable mention): `POST /api/cleanup_tracks/{preview,apply}`.
//!
//! Stateless, same shape as `tune_api.rs`: every call reads `design.json`
//! fresh, computes `eda_connectivity::cleanup::compute_cleanup_filtered` (the real
//! `TRACKS_CLEANER` port, with the dialog's "Filter Items" box) against it, and `apply` commits the result
//! through the existing `Cmd::CommitRoute` (no new `Cmd` variant needed --
//! "remove these track/via ids, add these tracks" is exactly what a
//! cleanup pass does). `apply` recomputes rather than trusting a client-
//! held preview, so a board edited between preview and apply (another
//! browser tab, the CLI) can't apply stale removals.

use crate::board;
use eda_connectivity::cleanup::{compute_cleanup_filtered, CleanupChange, CleanupKind, CleanupOptions};
use eda_connectivity::cleanup_filter::CleanupFilter;
use serde_json::{json, Value};
use std::path::Path;

fn err(message: impl Into<String>) -> Value {
    json!({ "ok": false, "message": message.into() })
}

fn opts_of(req: &Value) -> CleanupOptions {
    let b = |key: &str| req.get(key).and_then(Value::as_bool).unwrap_or(false);
    CleanupOptions {
        delete_shorting: b("delete_shorting"),
        delete_redundant_vias: b("delete_redundant_vias"),
        delete_dangling_vias: b("delete_dangling_vias"),
        merge_segments: b("merge_segments"),
        delete_dangling_tracks: b("delete_dangling_tracks"),
        delete_tracks_in_pads: b("delete_tracks_in_pads"),
    }
}

/// The dialog's "Filter Items" box (`DIALOG_CLEANUP_TRACKS_AND_VIAS::doCleanup`'s `SetFilter`): a filter that is off has no key; `selected_ids` present --
/// even when empty -- is "Selected items only".
fn filter_of(req: &Value) -> CleanupFilter {
    let name = |key: &str| req.get(key).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
    CleanupFilter {
        net: name("filter_net"),
        netclass: name("filter_netclass"),
        layer: name("filter_layer"),
        selected: req.get("selected_ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()),
    }
}

fn kind_str(k: CleanupKind) -> &'static str {
    match k {
        CleanupKind::RedundantVia => "redundant_via",
        CleanupKind::ZeroLengthTrack => "zero_length_track",
        CleanupKind::DuplicateTrack => "duplicate_track",
        CleanupKind::ShortingTrack => "shorting_track",
        CleanupKind::ShortingVia => "shorting_via",
        CleanupKind::TrackInPad => "track_in_pad",
        CleanupKind::DanglingTrack => "dangling_track",
        CleanupKind::DanglingVia => "dangling_via",
        CleanupKind::MergedTracks => "merged_tracks",
    }
}

fn change_json(c: &CleanupChange) -> Value {
    json!({
        "kind": kind_str(c.kind),
        "label": c.kind.label(),
        "net": c.net,
        "remove_track_ids": c.remove_track_ids,
        "remove_via_ids": c.remove_via_ids,
    })
}

/// `POST /api/cleanup_tracks/preview`: read-only, never touches
/// `design.json` -- the dialog's "Build Changes" (`m_firstRun`).
pub fn preview(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    // A track is dangling or shorting against the pours as they were last filled (KiCad's fill, `crate::fill_api`); a zone with none is no copper.
    let copper = crate::fill_api::cached(dir).map(|k| k.copper(&design)).unwrap_or_default();
    let report = compute_cleanup_filtered(&design, &model, &copper, opts_of(&req), &filter_of(&req));
    json!({
        "ok": true,
        "changes": report.changes.iter().map(change_json).collect::<Vec<_>>(),
        "tracks_removed": report.remove_track_ids().len(),
        "vias_removed": report.remove_via_ids().len(),
        "tracks_added": report.add_tracks().len(),
    })
}

/// `POST /api/cleanup_tracks/apply`: same body as `preview`; commits
/// through `Cmd::CommitRoute` -- the dialog's "Update PCB".
pub fn apply(dir: &Path, body: &[u8]) -> Value {
    let req: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (_, design, model) = match board::load(dir) {
        Ok(v) => v,
        Err(e) => return err(board::reasons(&e)),
    };
    // A track is dangling or shorting against the pours as they were last filled (KiCad's fill, `crate::fill_api`); a zone with none is no copper.
    let copper = crate::fill_api::cached(dir).map(|k| k.copper(&design)).unwrap_or_default();
    let report = compute_cleanup_filtered(&design, &model, &copper, opts_of(&req), &filter_of(&req));
    if report.changes.is_empty() {
        return json!({ "ok": true, "message": "nothing to clean up", "changes": [] });
    }
    let changes_json: Vec<Value> = report.changes.iter().map(change_json).collect();
    let cmd = eda_ops::Cmd::CommitRoute {
        remove_track_ids: report.remove_track_ids(),
        remove_via_ids: report.remove_via_ids(),
        tracks: report.add_tracks(),
        vias: vec![],
    };
    match board::step(dir, cmd, true, "ui") {
        Ok(summary) => json!({ "ok": true, "message": summary, "changes": changes_json }),
        Err(e) => json!({ "ok": false, "message": board::reasons(&e) }),
    }
}
