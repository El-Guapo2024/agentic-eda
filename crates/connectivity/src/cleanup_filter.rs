//! The "Filter Items" half of "Cleanup Tracks & Vias..." and its locks.
//!
//! Ports the filter `DIALOG_CLEANUP_TRACKS_AND_VIAS::doCleanup` (`pcbnew/dialogs/dialog_cleanup_tracks_and_vias.cpp`) hands `TRACKS_CLEANER::SetFilter`
//! and the way every pass of `TRACKS_CLEANER` (`pcbnew/tracks_cleaner.cpp`) uses it: a track or via is a candidate for deletion or merging only if it is
//! not locked (`track->IsLocked()`) and the filter does not skip it (`filterItem( track )`).
//!
//! ```text
//! if( m_selectedItemsFilter )  skip an item that is neither selected nor in a selected group
//! if( m_netFilterOpt )         skip an item on another net
//! if( m_netclassFilterOpt )    skip an item whose effective net class does not contain the named one
//! if( m_layerFilterOpt )       skip an item on another layer (`GetLayer()`: a via's is the top layer of its span)
//! ```
//!
//! Items the filter skips are not removed or merged, but they still count as copper to everything else (they are what a dangling end is connected to, what a
//! merge may not swallow, what a duplicate is a duplicate of), exactly as in source, where the filter only gates the candidate.

use std::collections::BTreeSet;

use eda_model::ir::{Design, Via};
use eda_model::ConstraintModel;

/// The dialog's four filters; `None` is a filter that is off.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupFilter {
    /// "Filter items by net:" -- the net name.
    pub net: Option<String>,
    /// "Filter items by net class:" -- the class name (`Default` for the Default class).
    pub netclass: Option<String>,
    /// "Filter items by layer:" -- a copper layer.
    pub layer: Option<String>,
    /// "Selected items only" -- the ids of what is selected on the board (tracks, vias, groups...). A track or via passes when it is one of them or sits in a
    /// selected group, however deep (`while( group && !group->IsSelected() ) group = group->GetParentGroup()`).
    pub selected: Option<BTreeSet<String>>,
}

/// The name of the class that owns `net`: the Default class (`BoardRules::default_class`, "Default" unless a project names it otherwise) for a net no pattern claims.
fn class_name(model: &ConstraintModel, net: &str) -> String {
    if let Some(c) = model.board.class_of(net) {
        return c.name.clone();
    }
    model.board.default_class.as_ref().map(|c| c.name.clone()).unwrap_or_else(|| "Default".to_string())
}

impl CleanupFilter {
    pub fn is_empty(&self) -> bool {
        self.net.is_none() && self.netclass.is_none() && self.layer.is_none() && self.selected.is_none()
    }

    /// `aItem->IsSelected()`, or a group above it is.
    fn is_selected(&self, design: &Design, id: &str) -> bool {
        let Some(selected) = self.selected.as_ref() else { return true };
        if selected.contains(id) {
            return true;
        }
        let Some(dr) = design.drawings.as_ref() else { return false };
        let mut at = id.to_string();
        // a loop made by hand in a file ends the walk
        for _ in 0..64 {
            match dr.parent_group(&at) {
                Some(g) if selected.contains(&g.id) => return true,
                Some(g) => at = g.id.clone(),
                None => return false,
            }
        }
        false
    }

    /// The dialog's lambda: whether the filter skips an item with this id, net and layer.
    fn skips(&self, design: &Design, model: &ConstraintModel, id: &str, net: &str, layer: &str) -> bool {
        if !self.is_selected(design, id) {
            return true;
        }
        if self.net.as_deref().is_some_and(|n| n != net) {
            return true;
        }
        if self.netclass.as_deref().is_some_and(|c| c != class_name(model, net)) {
            return true;
        }
        self.layer.as_deref().is_some_and(|l| l != layer)
    }

    /// The tracks and vias a cleanup must leave alone: the locked ones and the ones this filter skips.
    pub fn protected(&self, design: &Design, model: &ConstraintModel) -> Protected {
        let mut out = Protected::default();
        let Some(rt) = design.routing.as_ref() else { return out };
        for t in &rt.tracks {
            if is_locked(design, &t.id) || self.skips(design, model, &t.id, &t.net, &t.layer) {
                out.tracks.insert(t.id.clone());
            }
        }
        for v in &rt.vias {
            if is_locked(design, &v.id) || self.skips(design, model, &v.id, &v.net, via_top_layer(v, &model.board.layers)) {
                out.vias.insert(v.id.clone());
            }
        }
        out
    }
}

/// The tracks and vias a cleanup leaves alone, by id.
#[derive(Debug, Default, Clone)]
pub struct Protected {
    pub tracks: BTreeSet<String>,
    pub vias: BTreeSet<String>,
}

impl Protected {
    pub fn track(&self, id: &str) -> bool {
        self.tracks.contains(id)
    }
    pub fn via(&self, id: &str) -> bool {
        self.vias.contains(id)
    }
}

/// `PCB_VIA::GetLayer()`: the top layer of the via's span, whichever way round the span is written.
fn via_top_layer<'a>(v: &'a Via, copper: &[String]) -> &'a str {
    let at = |l: &str| copper.iter().position(|c| c == l).unwrap_or(usize::MAX);
    if at(&v.to_layer) < at(&v.from_layer) {
        &v.to_layer
    } else {
        &v.from_layer
    }
}

/// `BOARD_ITEM::IsLocked`: the item is locked, or a group above it is.
fn is_locked(design: &Design, id: &str) -> bool {
    let Some(dr) = design.drawings.as_ref() else { return false };
    let mut at = id.to_string();
    // a loop made by hand in a file ends the walk
    for _ in 0..64 {
        if dr.locked_ids.iter().any(|l| *l == at) {
            return true;
        }
        match dr.parent_group(&at) {
            Some(g) => at = g.id.clone(),
            None => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::{compute_cleanup_filtered, CleanupKind, CleanupOptions, CleanupReport};
    use eda_drc::zone_copper::ZoneCopper;
    use crate::tests_support::{pad_center, two_pad_model};
    use eda_model::ir::{DrawingsSection, Group, Point, Track, Via};
    use eda_model::{Net, NetClass};

    fn track(id: &str, net: &str, layer: &str, width: i64, pts: Vec<Point>) -> Track {
        Track { id: id.into(), net: net.into(), pins: vec![], layer: layer.into(), width, pts, arc_mid_offset: None }
    }

    fn via(id: &str, net: &str, at: Point) -> Via {
        Via { id: id.into(), net: net.into(), at, drill: 300, diameter: 600, from_layer: "F.Cu".into(), to_layer: "B.Cu".into() }
    }

    fn class(name: &str, nets: &[&str]) -> NetClass {
        NetClass {
            name: name.into(),
            nets: nets.iter().map(|n| n.to_string()).collect(),
            track_width: None,
            clearance: None,
            via_diameter: None,
            via_drill: None,
            microvia_diameter: None,
            microvia_drill: None,
            diff_pair_width: None,
            diff_pair_gap: None,
            diff_pair_via_gap: None,
            priority: 0,
        }
    }

    fn only(ids: &[&str]) -> CleanupFilter {
        CleanupFilter { selected: Some(ids.iter().map(|s| s.to_string()).collect()), ..Default::default() }
    }

    /// Two zero-length tracks on two nets and two layers, and two pairs of vias on top of each other, one pair per net: every pass finds work on both.
    fn two_of_everything() -> (Design, ConstraintModel) {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let b = pad_center(&design, &model, "R2", "1");
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(track("z1", "N1", "F.Cu", 200, vec![a, a]));
        rt.tracks.push(track("z2", "N2", "B.Cu", 200, vec![b, b]));
        rt.vias.push(via("v1", "N1", a));
        rt.vias.push(via("v2", "N1", a));
        rt.vias.push(via("w1", "N2", b));
        rt.vias.push(via("w2", "N2", b));
        (design, model)
    }

    fn cleanup(design: &Design, model: &ConstraintModel, opts: CleanupOptions, filter: &CleanupFilter) -> CleanupReport {
        compute_cleanup_filtered(design, model, &ZoneCopper::default(), opts, filter)
    }

    fn nulls_and_vias(design: &Design, model: &ConstraintModel, filter: &CleanupFilter) -> (Vec<String>, Vec<String>) {
        let report = cleanup(design, model, CleanupOptions { merge_segments: true, delete_redundant_vias: true, ..Default::default() }, filter);
        (report.remove_track_ids(), report.remove_via_ids())
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_filter_removes_what_it_always_did() {
        let (design, model) = two_of_everything();
        assert_eq!(nulls_and_vias(&design, &model, &CleanupFilter::default()), (ids(&["z1", "z2"]), ids(&["v2", "w2"])));
    }

    #[test]
    fn the_net_filter_leaves_the_other_nets_alone() {
        let (design, model) = two_of_everything();
        let f = CleanupFilter { net: Some("N2".into()), ..Default::default() };
        assert_eq!(nulls_and_vias(&design, &model, &f), (ids(&["z2"]), ids(&["w2"])));
    }

    #[test]
    fn the_layer_filter_takes_a_vias_first_layer() {
        let (design, model) = two_of_everything();
        // z1 is on F.Cu, z2 on B.Cu; every via starts on F.Cu (`PCB_VIA::GetLayer()` is the top layer of the pair)
        let f = CleanupFilter { layer: Some("B.Cu".into()), ..Default::default() };
        assert_eq!(nulls_and_vias(&design, &model, &f), (ids(&["z2"]), vec![]));
        let f = CleanupFilter { layer: Some("F.Cu".into()), ..Default::default() };
        assert_eq!(nulls_and_vias(&design, &model, &f), (ids(&["z1"]), ids(&["v2", "w2"])));
    }

    #[test]
    fn a_via_written_bottom_to_top_is_on_its_top_layer() {
        let (mut design, model) = two_of_everything();
        for v in design.routing.as_mut().unwrap().vias.iter_mut() {
            std::mem::swap(&mut v.from_layer, &mut v.to_layer);
        }
        let f = CleanupFilter { layer: Some("F.Cu".into()), ..Default::default() };
        assert_eq!(nulls_and_vias(&design, &model, &f), (ids(&["z1"]), ids(&["v2", "w2"])));
        let f = CleanupFilter { layer: Some("B.Cu".into()), ..Default::default() };
        assert_eq!(nulls_and_vias(&design, &model, &f), (ids(&["z2"]), vec![]));
    }

    #[test]
    fn the_net_class_filter_names_the_class_that_owns_the_net() {
        let (design, mut model) = two_of_everything();
        model.board.net_classes.push(class("Power", &["N2"]));
        let f = CleanupFilter { netclass: Some("Power".into()), ..Default::default() };
        assert_eq!(nulls_and_vias(&design, &model, &f), (ids(&["z2"]), ids(&["w2"])));
        // N1 is in no class of its own: the Default class owns it
        let f = CleanupFilter { netclass: Some("Default".into()), ..Default::default() };
        assert_eq!(nulls_and_vias(&design, &model, &f), (ids(&["z1"]), ids(&["v2"])));
    }

    #[test]
    fn selected_items_only_takes_the_selection_and_the_members_of_a_selected_group() {
        let (mut design, model) = two_of_everything();
        // w2 is selected, w1 is not: w2 is the candidate and w1, a twin it collides with, stays
        assert_eq!(nulls_and_vias(&design, &model, &only(&["z1", "w2"])), (ids(&["z1"]), ids(&["w2"])));
        assert_eq!(nulls_and_vias(&design, &model, &only(&[])), (vec![], vec![]), "nothing selected, nothing to clean");
        // a selected group stands for its members, a group inside a group too
        let dr = design.drawings.get_or_insert_with(DrawingsSection::default);
        dr.groups.push(Group { id: "inner".into(), name: String::new(), member_ids: ids(&["z2", "v2"]), design_block: String::new() });
        dr.groups.push(Group { id: "outer".into(), name: String::new(), member_ids: ids(&["inner"]), design_block: String::new() });
        assert_eq!(nulls_and_vias(&design, &model, &only(&["outer"])), (ids(&["z2"]), ids(&["v2"])));
        assert_eq!(nulls_and_vias(&design, &model, &only(&["inner"])), (ids(&["z2"]), ids(&["v2"])));
    }

    #[test]
    fn the_filters_combine_with_and() {
        let (design, model) = two_of_everything();
        // N1's track is on F.Cu and its vias start on F.Cu: none of them is on B.Cu
        let f = CleanupFilter { net: Some("N1".into()), layer: Some("B.Cu".into()), ..Default::default() };
        assert_eq!(nulls_and_vias(&design, &model, &f), (vec![], vec![]));
        let f = CleanupFilter { net: Some("N1".into()), layer: Some("F.Cu".into()), selected: Some(ids(&["z1"]).into_iter().collect()), ..Default::default() };
        assert_eq!(nulls_and_vias(&design, &model, &f), (ids(&["z1"]), vec![]));
    }

    #[test]
    fn a_locked_item_is_never_removed_and_a_lock_on_a_group_holds_its_members() {
        let (mut design, model) = two_of_everything();
        let dr = design.drawings.get_or_insert_with(DrawingsSection::default);
        dr.locked_ids.push("z1".into());
        dr.groups.push(Group { id: "g".into(), name: String::new(), member_ids: ids(&["w2"]), design_block: String::new() });
        dr.locked_ids.push("g".into());
        // z1 is locked; w2 is in a locked group, so w1 -- its twin -- is the one that goes
        assert_eq!(nulls_and_vias(&design, &model, &CleanupFilter::default()), (ids(&["z2"]), ids(&["v2", "w1"])));
    }

    #[test]
    fn a_via_on_top_of_a_protected_one_is_still_removed_whichever_comes_first() {
        // source never flags a protected via as processed, so the candidate collides with it
        let (design, model) = two_of_everything();
        assert_eq!(nulls_and_vias(&design, &model, &only(&["v2"])).1, ids(&["v2"]));
        assert_eq!(nulls_and_vias(&design, &model, &only(&["v1"])).1, ids(&["v1"]));
    }

    #[test]
    fn dangling_tracks_and_vias_the_filter_skips_stay() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let up = |p: Point| Point { x: p.x, y: p.y + 10_000 };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(track("stub1", "N1", "F.Cu", 200, vec![a, up(a)]));
        rt.tracks.push(track("stub2", "N1", "F.Cu", 200, vec![b, up(b)]));
        let opts = CleanupOptions { delete_dangling_tracks: true, ..Default::default() };
        assert_eq!(cleanup(&design, &model, opts, &CleanupFilter::default()).remove_track_ids(), ids(&["stub1", "stub2"]));
        assert_eq!(cleanup(&design, &model, opts, &only(&["stub2"])).remove_track_ids(), ids(&["stub2"]));
        let f = CleanupFilter { layer: Some("B.Cu".into()), ..Default::default() };
        assert!(cleanup(&design, &model, opts, &f).changes.is_empty());
        // a locked stub stays too
        design.drawings.get_or_insert_with(DrawingsSection::default).locked_ids.push("stub1".into());
        assert_eq!(cleanup(&design, &model, opts, &CleanupFilter::default()).remove_track_ids(), ids(&["stub2"]));
    }

    #[test]
    fn a_dangling_via_the_filter_skips_stays() {
        let (mut design, model) = two_pad_model();
        let a = pad_center(&design, &model, "R1", "1");
        let via_at = Point { x: a.x, y: a.y + 3_000 };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(track("t1", "N1", "F.Cu", 200, vec![a, via_at]));
        rt.vias.push(via("v1", "N1", via_at));
        let opts = CleanupOptions { delete_dangling_vias: true, ..Default::default() };
        assert_eq!(cleanup(&design, &model, opts, &CleanupFilter::default()).remove_via_ids(), ids(&["v1"]));
        assert!(cleanup(&design, &model, opts, &only(&["t1"])).changes.is_empty(), "the via is not selected");
        assert_eq!(cleanup(&design, &model, opts, &only(&["v1"])).remove_via_ids(), ids(&["v1"]));
    }

    #[test]
    fn tracks_in_pads_and_shorting_tracks_follow_the_filter() {
        let (mut design, mut model) = two_pad_model();
        // R2's pad gets its own net, so a track from R1 to R2 and a via on R2's pad touch another net
        model.nets[0].pins = vec!["R1.1".into()];
        model.nets.push(Net { name: "N2".into(), pins: vec!["R2.1".into()] });
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(track("bridge", "N1", "F.Cu", 200, vec![a, b]));
        rt.vias.push(via("sv", "N1", b));
        rt.tracks.push(track("inpad1", "N1", "F.Cu", 50, vec![a, Point { x: a.x + 5, y: a.y }]));
        rt.tracks.push(track("inpad2", "N2", "F.Cu", 50, vec![b, Point { x: b.x + 5, y: b.y }]));

        let shorts = CleanupOptions { delete_shorting: true, ..Default::default() };
        let all = cleanup(&design, &model, shorts, &CleanupFilter::default());
        assert!(all.remove_track_ids().contains(&"bridge".to_string()) && all.remove_via_ids() == ids(&["sv"]), "{:?}", all.changes.iter().map(|c| c.kind).collect::<Vec<_>>());
        let one = cleanup(&design, &model, shorts, &only(&["sv"]));
        assert_eq!((one.remove_track_ids(), one.remove_via_ids()), (vec![], ids(&["sv"])));
        let one = cleanup(&design, &model, shorts, &only(&["bridge"]));
        assert_eq!((one.remove_track_ids(), one.remove_via_ids()), (ids(&["bridge"]), vec![]));

        let in_pads = CleanupOptions { delete_tracks_in_pads: true, ..Default::default() };
        let all = cleanup(&design, &model, in_pads, &CleanupFilter::default());
        assert!(all.changes.iter().filter(|c| matches!(c.kind, CleanupKind::TrackInPad)).count() >= 2);
        let one = cleanup(&design, &model, in_pads, &CleanupFilter { net: Some("N2".into()), ..Default::default() });
        assert_eq!(one.remove_track_ids(), ids(&["inpad2"]));
    }

    #[test]
    fn two_collinear_tracks_merge_only_when_both_are_free_to_change() {
        let (mut design, model) = two_pad_model();
        let (a, b) = (pad_center(&design, &model, "R1", "1"), pad_center(&design, &model, "R2", "1"));
        let mid = Point { x: (a.x + b.x) / 2, y: a.y };
        let rt = design.routing.as_mut().unwrap();
        rt.tracks.push(track("t1", "N1", "F.Cu", 200, vec![a, mid]));
        rt.tracks.push(track("t2", "N1", "F.Cu", 200, vec![mid, b]));
        let opts = CleanupOptions { merge_segments: true, ..Default::default() };
        let merges = |f: &CleanupFilter| cleanup(&design, &model, opts, f).changes.iter().filter(|c| matches!(c.kind, CleanupKind::MergedTracks)).count();
        assert_eq!(merges(&CleanupFilter::default()), 1);
        assert_eq!(merges(&only(&["t1", "t2"])), 1);
        assert_eq!(merges(&only(&["t1"])), 0, "t2 is skipped, so the pair is not merged");
        assert_eq!(merges(&only(&["t2"])), 0);
        let mut locked = design.clone();
        locked.drawings.get_or_insert_with(DrawingsSection::default).locked_ids.push("t2".into());
        let report = cleanup(&locked, &model, opts, &CleanupFilter::default());
        assert!(report.changes.iter().all(|c| !matches!(c.kind, CleanupKind::MergedTracks)), "a locked track is never merged");
    }

    #[test]
    fn an_empty_filter_is_empty() {
        assert!(CleanupFilter::default().is_empty());
        assert!(!only(&[]).is_empty());
        assert!(!CleanupFilter { net: Some("N1".into()), ..Default::default() }.is_empty());
    }
}
