//! Repeat Layout and Apply Design Block Layout: `MULTICHANNEL_TOOL::RepeatLayout` and `copyRuleAreaContents` (`pcbnew/tools/multichannel_tool.cpp`).
//!
//! The reference area's layout goes onto a target area in four steps, in KiCad's order, each a choice of the dialog:
//!
//! 1. **Routing** ("Copy routing"). What is in the target area -- the tracks, vias and copper shapes the area encloses (`findRoutingInRuleArea`: `A.enclosedByArea( area )`) -- goes,
//!    and the same of the reference area comes in, turned and moved to the target and on the nets of the target's pads (`fixupNet`). "Restrict to routing connected within the
//!    area" keeps to the nets of the footprints that were matched. Locked items stay unless "Include locked items" is on.
//! 2. **Other items** ("Copy other items"): the zones and the drawings the area encloses (`findOtherItemsInRuleArea`), the same way.
//! 3. **Placement** ("Copy footprint placement"): each footprint of the target takes the side, the place, the turn, the text fields and the 3D models of the footprint of
//!    the reference area that stands for it.
//! 4. The target area itself takes the shape of the reference area, turned and moved.
//!
//! The copy is turned and moved so that the target's footprint matching the anchor lands where the anchor is (`aOpts.m_anchorFp`), else so that the centres of the two areas
//! meet. A target that is a group (Apply Design Block Layout) takes the first matched pair as its anchor, keeps its own items, and the copies join the group.
//!
//! What is selected, copied and removed is worked out on the board as it was when the command began, so a target does not see what the one before it did.

use super::multichannel::{resolve_topology, RepeatOptions, RuleArea};
use super::multichannel_facts::McFootprint;
use super::multichannel_geom::{polygon_meets_box, Enclosure};
use super::pcb_transform::Xform;
use super::Board;
use eda_model::fp_edit::{self, FieldLayout, FootprintEdit};
use eda_model::ir::{next_item_id, Design, Dimension, FootprintInstance, Group, Point, Shape, Side, Text, Track, Nm, Via, Zone};
use eda_model::pcb_items::{PcbPoint, TextBox};
use eda_model::CheckResult;
use std::collections::{BTreeMap, BTreeSet};

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

// ----------------------------------------------------------------------------------------------------------------------------------- items

/// An item a rule area encloses, of any of the kinds the tool copies.
#[derive(Debug, Clone)]
pub(crate) enum Item {
    Track(Track),
    Via(Via),
    Zone(Zone),
    Shape(Shape, Option<eda_model::pcb_items::ShapeStyle>),
    Text(Text, Option<eda_model::board_text::TextExtra>),
    Dimension(Dimension),
    TextBox(TextBox),
    Point(PcbPoint),
}

impl Item {
    pub(crate) fn id(&self) -> &str {
        match self {
            Item::Track(t) => &t.id,
            Item::Via(v) => &v.id,
            Item::Zone(z) => &z.id,
            Item::Shape(s, _) => s.id(),
            Item::Text(t, _) => &t.id,
            Item::Dimension(d) => &d.id,
            Item::TextBox(t) => &t.id,
            Item::Point(p) => &p.id,
        }
    }

    /// `BOARD_ITEM::GetLayer()` as the tool tests it: a via's is its first layer, a zone's the first of its set.
    fn layer(&self) -> String {
        match self {
            Item::Track(t) => t.layer.clone(),
            Item::Via(v) => v.from_layer.clone(),
            Item::Zone(z) => z.layer.clone(),
            Item::Shape(s, _) => s.layer().to_string(),
            Item::Text(t, _) => t.layer.clone(),
            Item::Dimension(d) => d.layer.clone(),
            Item::TextBox(t) => t.layer.clone(),
            Item::Point(p) => p.layer.clone(),
        }
    }

    /// `BOARD_ITEM::GetLayerSet()`.
    fn layer_set(&self, copper: &[String]) -> Vec<String> {
        match self {
            Item::Zone(z) => z.layer_set().into_iter().map(String::from).collect(),
            Item::Via(v) => {
                let (a, b) = (copper.iter().position(|l| *l == v.from_layer), copper.iter().position(|l| *l == v.to_layer));
                match (a, b) {
                    (Some(a), Some(b)) => copper[a.min(b)..=a.max(b)].to_vec(),
                    _ => vec![v.from_layer.clone(), v.to_layer.clone()],
                }
            }
            other => vec![other.layer()],
        }
    }

    /// A connected item in the sense of `findRoutingInRuleArea`: a track, a via or a shape on copper.
    fn is_routing(&self) -> bool {
        match self {
            Item::Track(_) | Item::Via(_) => true,
            Item::Shape(s, _) => s.layer().ends_with(".Cu"),
            _ => false,
        }
    }

    /// `BOARD_CONNECTED_ITEM::GetNetname()`, "" for none. A board shape's net is in its style (`ShapeStyle::net`), and only a
    /// shape on copper is connectable (`PCB_SHAPE::IsConnected`: "Only board-level copper shapes are connectable").
    fn net(&self) -> String {
        match self {
            Item::Track(t) => t.net.clone(),
            Item::Via(v) => v.net.clone(),
            Item::Zone(z) => z.net.clone(),
            Item::Shape(s, style) if s.layer().ends_with(".Cu") => style.as_ref().and_then(|st| st.net.clone()).unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// `BOARD_CONNECTED_ITEM::SetNetCode()`, "" for no net.
    fn set_net(&mut self, net: String) {
        match self {
            Item::Track(t) => t.net = net,
            Item::Via(v) => v.net = net,
            Item::Zone(z) if !z.is_rule_area => z.net = net,
            Item::Shape(s, style) if s.layer().ends_with(".Cu") => {
                let st = style.get_or_insert_with(|| eda_model::pcb_items::ShapeStyle::empty(s.id()));
                st.net = (!net.is_empty()).then_some(net);
            }
            _ => {}
        }
    }

    fn transform(&mut self, x: Xform, copper: usize, outer: &(Option<String>, Option<String>)) {
        use super::pcb_transform as t;
        match self {
            Item::Track(v) => t::transform_track(v, x, copper),
            Item::Via(v) => t::transform_via(v, x, copper, outer),
            Item::Zone(v) => t::transform_zone(v, x, copper),
            // A rounded rectangle turned off the quarter becomes the polygon of its rounded rectangle (`EDA_SHAPE::rotate`), its radius gone.
            Item::Shape(v, style) => {
                let radius = style.as_ref().map_or(eda_model::ir::Nm::ZERO, |st| st.corner_radius);
                if t::transform_rounded_shape(v, x, copper, radius) && radius > eda_model::ir::Nm::ZERO {
                    if let Some(st) = style.as_mut() {
                        st.corner_radius = eda_model::ir::Nm::ZERO;
                    }
                }
            }
            Item::Text(v, _) => t::transform_text(v, x, copper),
            Item::Dimension(v) => t::transform_dimension(v, x, copper),
            Item::TextBox(v) => super::pcb_items::transform_text_box(v, x, copper),
            Item::Point(v) => super::pcb_items::transform_point(v, x, copper),
        }
    }

    /// Is the item inside the outline (`enclosedByAreaFunc`)?
    fn enclosed_by(&self, e: &Enclosure) -> bool {
        use eda_model::ir::tessellate_arc;
        match self {
            Item::Track(t) => e.contains_stroke(&t.pts, t.width / 2),
            Item::Via(v) => e.contains_disc(v.at, v.diameter / 2),
            Item::Zone(z) => e.contains_ring(&z.outline, Nm::ZERO),
            Item::Shape(s, _) => {
                let half = s.stroke_width() / 2;
                match s {
                    Shape::Segment { start, end, .. } => e.contains_stroke(&[*start, *end], half),
                    Shape::Arc { start, mid, end, .. } => e.contains_stroke(&tessellate_arc(*start, *mid, *end, 24), half),
                    Shape::Rect { start, end, .. } => {
                        let (x0, x1, y0, y1) = (start.x.min(end.x), start.x.max(end.x), start.y.min(end.y), start.y.max(end.y));
                        e.contains_ring(&[Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }], half)
                    }
                    Shape::Circle { center, end, .. } => {
                        let r = Nm::kiround((end.x - center.x).as_f64().hypot((end.y - center.y).as_f64()));
                        e.contains_disc(*center, r + half)
                    }
                    Shape::Polygon { pts, .. } => e.contains_ring(pts, half),
                    Shape::Bezier { start, c1, c2, end, .. } => e.contains_stroke(&[*start, *c1, *c2, *end], half),
                }
            }
            Item::Text(t, _) => {
                let half_w = (t.content.chars().count().max(1) as i64 * t.size_nm * 6 / 10) / 2;
                e.contains_box((t.at.x - half_w, t.at.y - t.size_nm / 2, t.at.x + half_w, t.at.y + t.size_nm / 2))
            }
            Item::Dimension(d) => e.contains_stroke(&[d.start, d.end], d.stroke_width / 2),
            Item::TextBox(t) => {
                let [x0, y0, x1, y1] = t.bounds();
                e.contains_box((x0, y0, x1, y1))
            }
            Item::Point(p) => e.contains_point(p.at),
        }
    }
}

/// Every item of the board the outline encloses and that shares a layer with the area (`enclosedByAreaFunc`: "the item's layers meet the area's"), in the order the
/// board lists them. Teardrops (made by the Add Teardrops tool, not drawn) and the zones of footprints are not among them.
pub(crate) fn enclosed_items(design: &Design, outline: &[Point], area_layers: &[String], copper: &[String], skip_zone: Option<&str>) -> Vec<Item> {
    let e = Enclosure { outline };
    let overlaps = |it: &Item| it.layer_set(copper).iter().any(|l| area_layers.contains(l));
    let mut out: Vec<Item> = Vec::new();
    let mut take = |it: Item| {
        if overlaps(&it) && it.enclosed_by(&e) {
            out.push(it);
        }
    };
    if let Some(rt) = &design.routing {
        for t in &rt.tracks {
            take(Item::Track(t.clone()));
        }
        for v in &rt.vias {
            take(Item::Via(v.clone()));
        }
        for z in rt.zones.iter().filter(|z| !z.teardrop && z.parent_footprint.is_none() && Some(z.id.as_str()) != skip_zone) {
            take(Item::Zone(z.clone()));
        }
    }
    if let Some(dr) = &design.drawings {
        for s in &dr.shapes {
            take(Item::Shape(s.clone(), dr.board_items.style_of(s.id()).cloned()));
        }
        for t in &dr.texts {
            take(Item::Text(t.clone(), dr.text_extras.iter().find(|x| x.id == t.id).cloned()));
        }
        for d in &dr.dimensions {
            take(Item::Dimension(d.clone()));
        }
        for t in &dr.board_items.text_boxes {
            take(Item::TextBox(t.clone()));
        }
        for p in &dr.board_items.points {
            take(Item::Point(p.clone()));
        }
    }
    out
}

/// What the reference side of a copy holds: the items (their ids are the originals'), which of them are locked and which group each is in.
#[derive(Debug, Clone, Default)]
pub(crate) struct RefItems {
    pub items: Vec<Item>,
    pub locked: BTreeSet<String>,
    /// Item id -> the group it is in.
    pub group_of: BTreeMap<String, String>,
    /// Group id -> (name, design block).
    pub groups: BTreeMap<String, (String, String)>,
}

impl RefItems {
    /// The items a board's reference area encloses, with the locks and groups they carry.
    pub(crate) fn of_board(design: &Design, area: &RuleArea, copper: &[String]) -> RefItems {
        let items = enclosed_items(design, &area.outline, &area.layers, copper, Some(&area.zone_id));
        let mut r = RefItems { items, ..Default::default() };
        if let Some(dr) = &design.drawings {
            r.locked = dr.locked_ids.iter().cloned().collect();
            for it in &r.items {
                if let Some(g) = dr.parent_group(it.id()) {
                    r.group_of.insert(it.id().to_string(), g.id.clone());
                    r.groups.insert(g.id.clone(), (g.name.clone(), g.design_block.clone()));
                }
            }
        }
        r
    }
}

/// One side of a copy: a reference area, with the footprints it is made of.
pub(crate) struct RefSide<'a> {
    pub fps: &'a [McFootprint],
    pub outline: &'a [Point],
    pub layers: &'a [String],
    pub center: Point,
    pub items: &'a RefItems,
    /// `PLACEMENT_SOURCE_T::DESIGN_BLOCK`: the items are the block's, not found by the outline.
    pub design_block: bool,
}

/// The target of a copy.
pub(crate) struct TargetSide<'a> {
    /// The zone of the target area, `None` for the group an Apply Design Block Layout works on (its area is a temporary one that is not on the board).
    pub zone: Option<&'a str>,
    pub outline: &'a [Point],
    pub layers: &'a [String],
    pub center: Point,
    /// `PLACEMENT_SOURCE_T::GROUP_PLACEMENT`.
    pub group: bool,
}

/// What a copy to one target did, for the grouping that follows.
#[derive(Debug, Default, Clone)]
pub(crate) struct Outcome {
    /// `m_affectedItems` that still exist: the zone and the footprints.
    pub affected: Vec<String>,
    /// `m_groupableItems`: the zone, the copies and the footprints.
    pub groupable: Vec<String>,
}

/// The turn and the move a copy makes: `rot` clockwise millidegrees about the origin, then `disp`.
#[derive(Debug, Clone, Copy)]
struct Placing {
    rot: i64,
    disp: Point,
}

impl Placing {
    fn point(self, p: Point) -> Point {
        let r = if self.rot == 0 { p } else { super::multichannel_facts::rotate_about_origin(p, self.rot) };
        Point { x: r.x + self.disp.x, y: r.y + self.disp.y }
    }

    fn apply(self, it: &mut Item, copper: usize, outer: &(Option<String>, Option<String>)) {
        if self.rot != 0 {
            it.transform(Xform::Rotate { pivot: Point { x: Nm::ZERO, y: Nm::ZERO }, angle: self.rot }, copper, outer);
        }
        if self.disp.x != Nm::ZERO || self.disp.y != Nm::ZERO {
            it.transform(Xform::Move { dx: self.disp.x, dy: self.disp.y }, copper, outer);
        }
    }
}

fn norm360(a: i64) -> i64 {
    a.rem_euclid(360_000)
}

/// The nets of the pads of `members`, "" for a pad on none (`refc` and `targc`: the net codes of the matched footprints' pads).
fn pad_net_set(fps: &[McFootprint], members: &[usize]) -> BTreeSet<String> {
    members.iter().flat_map(|&i| fps[i].pads.iter().map(|(_, n)| n.clone())).collect()
}

impl Board<'_> {
    /// `fixupNet`: the net of the pad that stands for the first pad of the reference footprints on `net`, in the target.
    fn mc_fix_net(ref_fps: &[McFootprint], tgt_fps: &[McFootprint], pairs: &[(usize, usize)], net: &str) -> String {
        let mut ordered: Vec<&(usize, usize)> = pairs.iter().collect();
        ordered.sort_by(|a, b| ref_fps[a.0].reference.cmp(&ref_fps[b.0].reference));
        for &&(r, t) in &ordered {
            for (number, pad_net) in &ref_fps[r].pads {
                if pad_net == net {
                    if let Some((_, target_net)) = tgt_fps[t].pads.iter().find(|(n, _)| n == number) {
                        return target_net.clone();
                    }
                }
            }
        }
        net.to_string()
    }

    /// `copyRuleAreaContents`: see the module doc. `orig` is the board as it was before the command; the edits are made to `self.design`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mc_copy_contents(&mut self, orig: &Design, r: &RefSide, t: &TargetSide, tgt_fps: &[McFootprint], pairs: &[(usize, usize)], opts: &RepeatOptions, anchor: Option<usize>) -> Outcome {
        let copper_layers: Vec<String> = self.model.board.layers.clone();
        let copper = copper_layers.len();
        let outer = (copper_layers.first().cloned(), copper_layers.last().cloned());
        let mut outcome = Outcome::default();

        // The turn and the move. Anchored: the target's footprint that stands for the anchor ends where the anchor is, after the turn. Else the centres of the two areas meet.
        let mut placing = Placing { rot: 0, disp: Point { x: t.center.x - r.center.x, y: t.center.y - r.center.y } };
        if let Some(a) = anchor {
            let target_anchor = pairs.iter().find(|(rf, _)| r.fps[*rf].reference == r.fps[a].reference).map(|(_, tf)| *tf).or_else(|| pairs.first().map(|(_, tf)| *tf));
            if let Some(tf) = target_anchor {
                let (anchor_pose, target_pose) = (&r.fps[a].pose, &tgt_fps[tf].pose);
                let rot = norm360(target_pose.rot as i64 - anchor_pose.rot as i64);
                let moved = super::multichannel_facts::rotate_about_origin(anchor_pose.at, rot);
                placing = Placing { rot, disp: Point { x: target_pose.at.x - moved.x, y: target_pose.at.y - moved.y } };
            }
        }

        let ref_nets = pad_net_set(r.fps, &pairs.iter().map(|p| p.0).collect::<Vec<_>>());
        let target_nets = pad_net_set(tgt_fps, &pairs.iter().map(|p| p.1).collect::<Vec<_>>());
        let ref_layers: BTreeSet<&str> = r.layers.iter().map(String::as_str).collect();
        let target_layers: BTreeSet<&str> = t.layers.iter().map(String::as_str).collect();
        let locked_now: BTreeSet<String> = orig.drawings.iter().flat_map(|d| d.locked_ids.iter().cloned()).collect();

        // What leaves the target: its routing, then its other items (a group's temporary area is not on the board, so it finds nothing).
        let mut gone: BTreeSet<String> = BTreeSet::new();
        if t.zone.is_some() {
            for it in enclosed_items(orig, t.outline, t.layers, &copper_layers, t.zone) {
                let wanted = if it.is_routing() {
                    opts.copy_routing && !(locked_now.contains(it.id()) && !opts.include_locked_items) && !(opts.connected_routing_only && !target_nets.contains(&it.net())) && target_layers.contains(it.layer().as_str())
                } else {
                    // `if( item->Type() == PCB_ZONE_T )`: every layer of the zone is in the target area; else the item's layer is
                    opts.copy_other_items
                        && !(locked_now.contains(it.id()) && !opts.include_locked_items)
                        && match &it {
                            Item::Zone(z) => z.layer_set().iter().all(|l| target_layers.contains(l)),
                            other => target_layers.contains(other.layer().as_str()),
                        }
                };
                if wanted {
                    gone.insert(it.id().to_string());
                }
            }
        }

        // What comes in: the reference area's routing and other items on the layers both areas have, on the target's nets, turned and moved.
        let mut incoming: Vec<(Item, String)> = Vec::new();
        for it in &r.items.items {
            let locked = r.items.locked.contains(it.id());
            if locked && !opts.include_locked_items {
                continue;
            }
            let layer_ok = match it {
                Item::Zone(z) => z.layer_set().iter().all(|l| ref_layers.contains(l) && target_layers.contains(l)),
                other => ref_layers.contains(other.layer().as_str()) && target_layers.contains(other.layer().as_str()),
            };
            if !layer_ok {
                continue;
            }
            if it.is_routing() {
                if !opts.copy_routing || (opts.connected_routing_only && !ref_nets.contains(&it.net())) {
                    continue;
                }
            } else if !opts.copy_other_items {
                continue;
            }
            // A design block's zones are copied with the other items, and its pads and nets are its own
            let mut copy = it.clone();
            if !matches!(&copy, Item::Zone(z) if z.is_rule_area) {
                let net = copy.net();
                // `fixupNet( item, copied, ... )` runs on every copied connected item (a track, a via, a copper shape); a zone's net is fixed only when it has one.
                if !net.is_empty() || copy.is_routing() {
                    let fixed = Self::mc_fix_net(r.fps, tgt_fps, pairs, &net);
                    copy.set_net(fixed);
                }
            }
            if let Item::Track(tr) = &mut copy {
                tr.pins.clear();
            }
            placing.apply(&mut copy, copper, &outer);
            incoming.push((copy, it.id().to_string()));
        }

        // Land it.
        let preserve_groups = !t.group;
        {
            let mut taken = self.mc_all_ids();
            let mut new_groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
            let mut minted: Vec<(String, String)> = Vec::new();
            self.mc_remove_items(&gone);
            for (n, (mut it, source)) in incoming.into_iter().enumerate() {
                let new_id = self.mc_land(&mut it, n, &mut taken);
                minted.push((source.clone(), new_id.clone()));
                outcome.groupable.push(new_id.clone());
                if r.items.locked.contains(&source) {
                    let dr = self.drawings_mut();
                    dr.locked_ids.push(new_id.clone());
                    dr.locked_ids.sort();
                    dr.locked_ids.dedup();
                }
                if preserve_groups {
                    if let Some(g) = r.items.group_of.get(&source) {
                        new_groups.entry(g.clone()).or_default().push(new_id);
                    }
                }
            }
            if !new_groups.is_empty() {
                for (g, members) in new_groups {
                    let (name, design_block) = r.items.groups.get(&g).cloned().unwrap_or_default();
                    // A group of one is no group (`GROUP_TOOL::RemoveFromGroup`): the copy of a group that had only one item in the area stays loose.
                    if members.len() < 2 && design_block.is_empty() {
                        continue;
                    }
                    let dr = self.drawings_mut();
                    dr.groups.push(Group { id: String::new(), name, member_ids: members, design_block });
                    eda_model::groups::GroupHost::assign_group_ids(dr);
                }
            }
        }

        // The footprints.
        if opts.copy_placement {
            for &(rf, tf) in pairs {
                let (refp, tgt) = (&r.fps[rf], &tgt_fps[tf]);
                let layer = if refp.pose.side == Side::Top { "F.Cu" } else { "B.Cu" };
                if !ref_layers.contains(layer) || !target_layers.contains(layer) {
                    continue;
                }
                // "For regular Rule Area repeat, ignore source footprints outside the reference area."
                if !r.design_block && !polygon_meets_box(r.outline, refp.bbox) {
                    continue;
                }
                if tgt.locked && !opts.include_locked_items {
                    continue;
                }
                self.mc_place_like(refp, tgt, &placing);
                outcome.affected.push(tgt.reference.clone());
                outcome.groupable.push(tgt.reference.clone());
            }
        }

        // The target's own area takes the reference area's shape.
        if let Some(zone) = t.zone {
            if !r.outline.is_empty() {
                let outline: Vec<Point> = r.outline.iter().map(|p| placing.point(*p)).collect();
                if let Some(rt) = self.design.routing.as_mut() {
                    if let Some(z) = rt.zones.iter_mut().find(|z| z.id == zone) {
                        z.outline = outline;
                    }
                }
            }
            outcome.affected.push(zone.to_string());
            outcome.groupable.push(zone.to_string());
        }
        outcome
    }

    /// Every id an item of the board is listed under (the ones a new item must not take).
    fn mc_all_ids(&self) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = BTreeSet::new();
        if let Some(rt) = &self.design.routing {
            out.extend(rt.tracks.iter().map(|t| t.id.clone()));
            out.extend(rt.vias.iter().map(|v| v.id.clone()));
            out.extend(rt.zones.iter().map(|z| z.id.clone()));
        }
        if let Some(dr) = &self.design.drawings {
            out.extend(dr.shapes.iter().map(|s| s.id().to_string()));
            out.extend(dr.texts.iter().map(|t| t.id.clone()));
            out.extend(dr.dimensions.iter().map(|d| d.id.clone()));
            out.extend(dr.groups.iter().map(|g| g.id.clone()));
            out.extend(dr.tables.iter().map(|t| t.id.clone()));
            out.extend(dr.barcodes.iter().map(|b| b.id.clone()));
            out.extend(dr.board_items.ids().map(String::from));
        }
        out
    }

    /// Take the items with these ids off the board.
    fn mc_remove_items(&mut self, ids: &BTreeSet<String>) {
        if ids.is_empty() {
            return;
        }
        if let Some(rt) = self.design.routing.as_mut() {
            rt.tracks.retain(|t| !ids.contains(&t.id));
            rt.vias.retain(|v| !ids.contains(&v.id));
            rt.zones.retain(|z| !ids.contains(&z.id));
        }
        if let Some(dr) = self.design.drawings.as_mut() {
            dr.shapes.retain(|s| !ids.contains(s.id()));
            dr.texts.retain(|t| !ids.contains(&t.id));
            dr.text_extras.retain(|x| !ids.contains(&x.id));
            dr.dimensions.retain(|d| !ids.contains(&d.id));
            dr.board_items.text_boxes.retain(|t| !ids.contains(&t.id));
            dr.board_items.points.retain(|p| !ids.contains(&p.id));
            dr.locked_ids.retain(|l| !ids.contains(l));
        }
    }

    /// Put an item on the board under a new id; the id.
    fn mc_land(&mut self, it: &mut Item, n: usize, taken: &mut BTreeSet<String>) -> String {
        let mut mint = |prefix: &str, seed: String| -> String {
            let id = next_item_id(prefix, &format!("repeat{n}|{seed}"), taken);
            taken.insert(id.clone());
            id
        };
        match it {
            Item::Track(t) => {
                t.id = mint("trk", format!("{}|{}|{:?}", t.net, t.layer, t.pts.first()));
                let id = t.id.clone();
                self.routing_mut().tracks.push(t.clone());
                self.sort_routing();
                id
            }
            Item::Via(v) => {
                v.id = mint("via", format!("{}|{},{}", v.net, v.at.x, v.at.y));
                let id = v.id.clone();
                self.routing_mut().vias.push(v.clone());
                self.sort_routing();
                id
            }
            Item::Zone(z) => {
                z.id = mint("zone", format!("{}|{}|{:?}", z.net, z.layer, z.outline.first()));
                z.uuid.clear();
                let id = z.id.clone();
                self.routing_mut().zones.push(z.clone());
                id
            }
            Item::Shape(s, style) => {
                let id = mint("shp", format!("{}|{:?}", s.layer(), s.points().first()));
                s.set_id(id.clone());
                let style = style.clone();
                let dr = self.drawings_mut();
                dr.shapes.push(s.clone());
                if style.is_some() {
                    dr.board_items.set_style(&id, style.as_ref());
                }
                id
            }
            Item::Text(t, extra) => {
                t.id = mint("txt", format!("{}|{},{}|{}", t.content, t.at.x, t.at.y, t.layer));
                let id = t.id.clone();
                let dr = self.drawings_mut();
                dr.texts.push(t.clone());
                if let Some(x) = extra {
                    let mut x = x.clone();
                    x.id = id.clone();
                    dr.text_extras.push(x);
                }
                id
            }
            Item::Dimension(d) => {
                d.id = mint("dim", format!("{},{}|{},{}", d.start.x, d.start.y, d.end.x, d.end.y));
                let id = d.id.clone();
                self.drawings_mut().dimensions.push(d.clone());
                id
            }
            Item::TextBox(b) => {
                b.id = mint("tbx", format!("{}|{},{}", b.text, b.start.x, b.start.y));
                let id = b.id.clone();
                self.drawings_mut().board_items.text_boxes.push(b.clone());
                id
            }
            Item::Point(p) => {
                p.id = mint("pnt", format!("{},{}", p.at.x, p.at.y));
                let id = p.id.clone();
                self.drawings_mut().board_items.points.push(p.clone());
                id
            }
        }
    }

    /// The placement of one footprint (`copyRuleAreaContents`' `if( aOpts.m_copyPlacement )` block): the layer, orientation and position of the reference's, turned and moved; its
    /// fields' layers, visibility, attributes and places; its 3D models.
    fn mc_place_like(&mut self, refp: &McFootprint, tgt: &McFootprint, placing: &Placing) {
        let at = placing.point(refp.pose.at);
        let rot = norm360(refp.pose.rot as i64 + placing.rot) as u32;
        let pose = FootprintInstance { id: tgt.reference.clone(), at, rot, side: refp.pose.side, label: refp.pose.label };
        if let Some(fps) = self.design.placement.as_mut().map(|p| &mut p.footprints) {
            if let Some(f) = fps.iter_mut().find(|f| f.id == tgt.reference) {
                *f = pose.clone();
            }
        }
        // The Reference, the Value and the user fields: the layout of the reference's, in the footprint's own frame, which a turn of both leaves as it is.
        let part = self.model.part(&tgt.reference).cloned();
        let footprint = part.as_ref().and_then(|p| self.model.footprint_of(p));
        let current = self.design.footprint_edit(&tgt.reference).cloned();
        let mut edit = current.clone().unwrap_or_else(|| FootprintEdit::new(&tgt.reference));
        let keep_text = |own: &Option<FieldLayout>| own.as_ref().and_then(|l| l.text.clone());
        if let (Some(layout), Some(fp)) = (&refp.reference_layout, footprint.as_ref()) {
            let mut wanted = layout.clone();
            wanted.text = keep_text(&edit.reference);
            let now = edit.reference.clone().unwrap_or_else(|| fp_edit::default_reference_layout(&pose, fp));
            if wanted != now {
                edit.reference = Some(wanted);
            }
        }
        if let Some(layout) = &refp.value_layout {
            let mut wanted = layout.clone();
            wanted.text = keep_text(&edit.value);
            let now = edit.value.clone().unwrap_or_else(|| fp_edit::default_value_layout(&pose));
            if wanted != now {
                edit.value = Some(wanted);
            }
        }
        for rf in &refp.user_fields {
            if let Some(tf) = edit.fields.iter_mut().find(|u| u.name == rf.name) {
                let mut wanted = rf.layout.clone();
                wanted.text = tf.layout.text.clone();
                tf.layout = wanted;
            }
        }
        // `targetFP->Models() = refFP->Models()`
        let target_models = edit.models3d.clone().unwrap_or_else(|| footprint.as_ref().map(|f| f.models3d.clone()).unwrap_or_default());
        if refp.models3d != target_models {
            edit.models3d = Some(refp.models3d.clone());
        }
        let new_edit = (!edit.is_empty()).then_some(edit);
        if new_edit != current {
            let dr = self.drawings_mut();
            dr.footprint_edits.retain(|e| e.id != tgt.reference);
            if let Some(e) = new_edit {
                dr.footprint_edits.push(e);
                dr.footprint_edits.sort_by(|a, b| a.id.cmp(&b.id));
            }
        }
    }

    /// `MULTICHANNEL_TOOL::RepeatLayout( aEvent, aRefZone )`: the reference area's layout onto each target that matches.
    pub(crate) fn repeat_layout(&mut self, reference: &str, targets: &[String], opts: &RepeatOptions) -> Result<(), Vec<CheckResult>> {
        if targets.is_empty() {
            return Err(fail("ops_no_targets", reference, "no target rule area was chosen"));
        }
        let orig = self.design.clone();
        let mc = self.mc_board();
        let areas = self.mc_rule_areas(&mc);
        let Some(refa) = areas.iter().find(|a| a.zone_id == reference) else {
            return Err(fail("ops_not_placement_area", reference, "this is not a rule area with placement enabled"));
        };
        if refa.members.is_empty() {
            return Err(fail("ops_empty_area", reference, "the reference rule area holds no footprint"));
        }
        let anchor = match opts.anchor.as_deref().filter(|a| !a.is_empty()) {
            Some(a) => Some(refa.members.iter().copied().find(|&i| mc.fps[i].reference == a).ok_or_else(|| fail("ops_unknown_anchor", a, "the anchor footprint is not in the reference rule area"))?),
            None => None,
        };
        let copper: Vec<String> = self.model.board.layers.clone();
        let items = RefItems::of_board(&orig, refa, &copper);
        let ref_side = RefSide { fps: &mc.fps, outline: &refa.outline, layers: &refa.layers, center: refa.center, items: &items, design_block: false };

        let mut outcomes: Vec<Outcome> = Vec::new();
        let mut first_problem: Option<String> = None;
        for id in targets {
            if id == reference {
                continue;
            }
            let Some(target) = areas.iter().find(|a| a.zone_id == *id) else {
                return Err(fail("ops_not_placement_area", id, "this is not a rule area with placement enabled"));
            };
            let res = resolve_topology(&mc.fps, &refa.members, &mc.fps, &target.members, &mc.codes);
            if !res.ok {
                first_problem.get_or_insert_with(|| format!("{}: {}", if target.name.is_empty() { &target.zone_id } else { &target.name }, res.message));
                continue;
            }
            let tgt_side = TargetSide { zone: Some(&target.zone_id), outline: &target.outline, layers: &target.layers, center: target.center, group: false };
            outcomes.push(self.mc_copy_contents(&orig, &ref_side, &tgt_side, &mc.fps, &res.pairs, opts, anchor));
        }
        if outcomes.is_empty() {
            return Err(fail("ops_topology_mismatch", reference, format!("Rule Area topologies do not match: {}", first_problem.unwrap_or_else(|| "no target".to_string()))));
        }
        // `if( m_areas.m_options.m_groupItems )`: each target's zone, copies and footprints in a group of their own.
        if opts.group_items {
            for out in &outcomes {
                if out.groupable.len() < 2 {
                    continue;
                }
                let dr = self.drawings_mut();
                // `pruneExistingGroups( commit, m_affectedItems )`: the `< 2` rule on the groups it pruned, no other
                let affected = eda_model::groups::groups_holding(&dr.groups, &out.affected);
                for g in dr.groups.iter_mut() {
                    g.member_ids.retain(|m| !out.affected.contains(m));
                }
                eda_model::groups::prune_affected_groups(&mut dr.groups, &affected);
                let mut seen: BTreeSet<&String> = BTreeSet::new();
                let members: Vec<String> = out.groupable.iter().filter(|m| seen.insert(*m)).cloned().collect();
                // `group->AddItem( item )`: "Items can only be in one group at a time", so a copy leaves the group cloned for it
                // (`fixupParentGroup`); a group that is left empty is not written (`format( const PCB_GROUP* )`: `if( memberIds.empty() ) return;`).
                for g in dr.groups.iter_mut() {
                    g.member_ids.retain(|m| !members.contains(m));
                }
                dr.groups.retain(|g| !g.member_ids.is_empty());
                dr.groups.push(Group { id: String::new(), name: String::new(), member_ids: members, design_block: String::new() });
                eda_model::groups::GroupHost::assign_group_ids(dr);
            }
        }
        Ok(())
    }

    /// `PCB_CONTROL::ApplyDesignBlockLayout`: see the module doc and `multichannel.rs`.
    pub(crate) fn apply_design_block_layout(&mut self, group_id: &str, block: &str, text: &str) -> Result<(), Vec<CheckResult>> {
        let Some(group) = self.design.drawings.as_ref().and_then(|d| d.group(group_id)).cloned() else {
            return Err(fail("ops_unknown_group", group_id, "no group with this id"));
        };
        if group.design_block.is_empty() {
            return Err(fail("ops_no_design_block_link", group_id, "the group is not linked to a design block"));
        }
        let clip = eda_kicad::parse_pcb_clipboard(text).map_err(|e| fail("ops_bad_clipboard", block, e.iter().map(|c| c.hint.clone().unwrap_or_else(|| c.check.clone())).collect::<Vec<_>>().join("; ")))?;
        if clip.footprints.is_empty() {
            return Err(fail("ops_empty_design_block", block, "Design block placement failed - no footprints were placed."));
        }
        let orig = self.design.clone();
        let mc = self.mc_board();
        // the group's footprints (`group->GetItems()` of type footprint), none locked
        let mut members: Vec<usize> = Vec::new();
        for m in &group.member_ids {
            if let Some(i) = mc.index_of(m) {
                if mc.fps[i].locked {
                    return Err(fail("ops_locked_footprint", m, format!("Footprint {m} is locked and cannot be placed.")));
                }
                members.push(i);
            }
        }
        if members.is_empty() {
            return Err(fail("ops_empty_group", group_id, "Selected group contains no footprints to place."));
        }
        // the block's footprints and items as the reference side
        let block_fps: Vec<McFootprint> = clip.footprints.iter().map(super::multichannel_facts::block_footprint).collect();
        let ref_members: Vec<usize> = (0..block_fps.len()).collect();
        let mut codes = mc.codes.clone();
        let mut next_code = codes.values().copied().max().unwrap_or(0);
        for f in &block_fps {
            for (_, net) in &f.pads {
                if !net.is_empty() && !codes.contains_key(net) {
                    next_code += 1;
                    codes.insert(net.clone(), next_code);
                }
            }
        }
        let res = resolve_topology(&block_fps, &ref_members, &mc.fps, &members, &codes);
        if !res.ok {
            let shown: Vec<String> = res.reasons.iter().take(3).cloned().collect();
            return Err(fail("ops_topology_mismatch", group_id, format!("Rule Area topologies do not match: {}", shown.join("; "))));
        }
        let copper: Vec<String> = self.model.board.layers.clone();
        let items = super::multichannel_facts::block_items(&clip);
        // The block's area is all the copper layers (`LSET::AllCuMask()`): only what is on copper comes across.
        let ref_side = RefSide { fps: &block_fps, outline: &[], layers: &copper, center: Point { x: Nm::ZERO, y: Nm::ZERO }, items: &items, design_block: true };
        let tgt_side = TargetSide { zone: None, outline: &[], layers: &copper, center: Point { x: Nm::ZERO, y: Nm::ZERO }, group: true };
        let opts = RepeatOptions { copy_routing: true, connected_routing_only: false, copy_placement: true, copy_other_items: true, group_items: false, include_locked_items: true, anchor: None };
        // "If no anchor is provided, pick the first matched pair": the group stays where it is.
        let anchor = res.pairs.first().map(|(r, _)| *r);
        let outcome = self.mc_copy_contents(&orig, &ref_side, &tgt_side, &mc.fps, &res.pairs, &opts, anchor);
        // the copies and the footprints join the group
        let dr = self.drawings_mut();
        if let Some(g) = dr.groups.iter_mut().find(|g| g.id == group_id) {
            for id in outcome.groupable {
                if !g.member_ids.contains(&id) {
                    g.member_ids.push(id);
                }
            }
        }
        Ok(())
    }
}
