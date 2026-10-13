//! The tuning-pattern verb ([`Cmd::CommitTuning`]) and the upkeep that keeps a pattern honest when its tracks leave the board.
//!
//! A tuning pattern (`PCB_TUNING_PATTERN`, `crates/model/src/tuning.rs`) is a generator: the tracks of its meander are its members. What the
//! router's tuner makes for a pattern -- the tracks it put in place of the line it tuned, the pattern's settings and the baseline it started
//! from -- arrives as one command, so a new pattern, a pattern re-tuned and a pattern removed (the baseline put back) are each one undo step
//! (`GENERATOR_TOOL::GenEditAction`: `EditStart`, `Update`, `EditFinish` in one `BOARD_COMMIT`).
//!
//! A track of a pattern that is deleted by another verb leaves the pattern's members, and a pattern that is left with none is not kept
//! (`PCB_IO_KICAD_SEXPR::format( PCB_GENERATOR )` does not write a tuning pattern without members; "ghost tuning patterns").

use super::Board;
use eda_model::ir::Track;
use eda_model::tuning::TuningPattern;
use eda_model::CheckResult;
use std::collections::BTreeSet;

fn bad(subject: &str, why: &str) -> Vec<CheckResult> {
    vec![CheckResult::fail("ops_bad_tuning", subject, why)]
}

impl Board<'_> {
    /// `Cmd::CommitTuning`: take `remove_track_ids` and the pattern `remove_pattern` off the board, add `tracks`, and add `pattern` with the
    /// tracks of `member_indices` (indices into `tracks`) as its members. A pattern with the id of the one removed takes its place; one without
    /// an id gets one.
    pub(crate) fn commit_tuning(&mut self, pattern: Option<&TuningPattern>, remove_pattern: Option<&str>, remove_track_ids: &[String], mut tracks: Vec<Track>, member_indices: &[usize]) -> Result<(), Vec<CheckResult>> {
        for t in &tracks {
            self.known_net(&t.net)?;
            self.known_layer(&t.layer)?;
            if t.width <= 0 || t.pts.len() < 2 {
                return Err(bad(&t.net, "a track of a tuning needs a positive width and at least two points"));
            }
        }
        if let Some(i) = member_indices.iter().find(|&&i| i >= tracks.len()) {
            return Err(bad("member_indices", &format!("member {i} is not one of the {} tracks", tracks.len())));
        }
        if let Some(p) = pattern {
            self.known_layer(&p.layer)?;
            if member_indices.is_empty() {
                return Err(bad(&p.id, "a tuning pattern needs at least one track of meander to be one"));
            }
        }
        if let Some(id) = remove_pattern {
            if !self.design.drawings.as_ref().is_some_and(|d| d.tuning_patterns.iter().any(|t| t.id == id)) {
                return Err(bad(id, "no tuning pattern with this id"));
            }
        }

        if !remove_track_ids.is_empty() {
            if let Some(rt) = self.design.routing.as_mut() {
                rt.tracks.retain(|t| !remove_track_ids.iter().any(|id| id == &t.id));
            }
        }
        if let Some(id) = remove_pattern {
            if let Some(dr) = self.design.drawings.as_mut() {
                dr.tuning_patterns.retain(|t| t.id != id);
            }
        }

        let mut member_ids: Vec<String> = Vec::new();
        if !tracks.is_empty() {
            for t in &mut tracks {
                t.id.clear();
            }
            let rt = self.routing_mut();
            let first = rt.tracks.len();
            rt.tracks.append(&mut tracks);
            rt.assign_missing_ids();
            member_ids = member_indices.iter().map(|&i| rt.tracks[first + i].id.clone()).collect();
            self.sort_routing();
        }

        if let Some(p) = pattern {
            let mut record = p.clone();
            record.member_ids = member_ids;
            let dr = self.drawings_mut();
            dr.tuning_patterns.push(record);
            dr.assign_missing_ids();
        }
        Ok(())
    }

    /// The ids of every track of the board, when there are patterns to keep honest ([`Board::tidy_tuning_patterns`]); `None` without any.
    pub(crate) fn tuning_universe(&self) -> Option<BTreeSet<String>> {
        let dr = self.design.drawings.as_ref()?;
        (!dr.tuning_patterns.is_empty()).then(|| self.design.routing.iter().flat_map(|r| r.tracks.iter().map(|t| t.id.clone())).collect())
    }

    /// Takes out of every pattern the members that were tracks of the board before the command and are not now, and drops a pattern that is left
    /// with none.
    pub(crate) fn tidy_tuning_patterns(&mut self, before: Option<BTreeSet<String>>) {
        let Some(before) = before else { return };
        let now: BTreeSet<String> = self.design.routing.iter().flat_map(|r| r.tracks.iter().map(|t| t.id.clone())).collect();
        let gone: BTreeSet<&String> = before.difference(&now).collect();
        if gone.is_empty() {
            return;
        }
        if let Some(dr) = self.design.drawings.as_mut() {
            for p in dr.tuning_patterns.iter_mut() {
                p.member_ids.retain(|m| !gone.contains(m));
            }
            dr.tuning_patterns.retain(|p| !p.member_ids.is_empty());
        }
    }
}
