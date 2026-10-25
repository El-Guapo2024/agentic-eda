//! The multichannel tool: Generate Placement Rule Areas, Repeat Layout and Apply Design Block Layout (GAPS.md items 24 and 32, package P24).
//!
//! A board with several identical channels is laid out once, and the layout is copied onto the others. KiCad's tool (`pcbnew/tools/multichannel_tool.cpp`) does it with
//! *placement rule areas*: a rule area (a zone) that names a sheet, a component class or a group (`ZONE::GetPlacementAreaSource`). The footprints it names are the
//! channel's; the area's outline holds the channel's routing and the other items that go with it. [`MultichannelCmd`] is the three verbs, one variant of [`crate::Cmd`]:
//! `{"op": "multichannel", "verb": "generate_rule_areas" | "repeat_layout" | "apply_design_block_layout", ...}`.
//!
//! * **Generate** (`MULTICHANNEL_TOOL::GeneratePotentialRuleAreas` and `AutogenerateRuleAreas`): [`Board::mc_potential_areas`] lists what the board offers -- a rule area per
//!   sheet, per component class and per group, with the footprints it would hold -- the dialog's three grids; [`MultichannelCmd::GenerateRuleAreas`] makes the areas that
//!   were ticked, each the rectified hull of the boxes of its footprints, and, when asked, replaces the ones that are already there and groups each area with its footprints.
//! * **Repeat Layout** (`MULTICHANNEL_TOOL::repeatLayout`, `CheckRACompatibility`, `RepeatLayout`, `copyRuleAreaContents`): [`Board::mc_compat`] matches the footprints of the
//!   reference area with those of every other area by topology ([`eda_model::topo_match`]) -- the dialog's table of targets and what is wrong with each; the verb copies the
//!   placement, the routing and the other items onto the targets that match, as `multichannel_copy` does.
//! * **Apply Design Block Layout** (`PCB_CONTROL::ApplyDesignBlockLayout`): the board layout of the design block a group is linked to is repeated onto the group's
//!   footprints, the block standing in as a reference area made of its items.
//!
//! The footprints' sheet, classes, group and box are `multichannel_facts`; the selection of what lies in an area is `multichannel_geom`.

use eda_model::units::Nm;
use super::multichannel_facts::{in_sheet_or_below, member_of_group, member_of_sheet_or_children, McBoard, McFootprint};
use super::Board;
use eda_model::ir::{Point, Zone, ZoneBorderStyle};
use eda_model::multichannel::{outline_centre, rule_area_outline, Box4, PlacementArea, PlacementSourceType};
use eda_model::topo_match::{Graph, MismatchReason};
use eda_model::CheckResult;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// `MULTICHANNEL_TOOL::GeneratePotentialRuleAreas`'s margin round the boxes of the footprints: 0.1 mm.
const AREA_MARGIN_NM: i64 = 100_000;

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// What a placement rule area is made from: the three kinds the dialog offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AreaKind {
    Sheet,
    ComponentClass,
    Group,
}

impl AreaKind {
    pub fn source_type(self) -> PlacementSourceType {
        match self {
            AreaKind::Sheet => PlacementSourceType::Sheetname,
            AreaKind::ComponentClass => PlacementSourceType::ComponentClass,
            AreaKind::Group => PlacementSourceType::Group,
        }
    }
}

/// One row of the Generate dialog that was ticked: the kind, and the sheet's path, the class's name or the group's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AreaPick {
    pub kind: AreaKind,
    pub key: String,
}

/// `REPEAT_LAYOUT_OPTIONS`, as the Repeat Layout dialog sets them. The defaults are the structure's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RepeatOptions {
    /// "Copy footprint placement".
    pub copy_placement: bool,
    /// "Copy routing".
    pub copy_routing: bool,
    /// "Restrict to routing connected within the area".
    pub connected_routing_only: bool,
    /// "Copy other items".
    pub copy_other_items: bool,
    /// "Group items with their target rule areas".
    pub group_items: bool,
    /// "Include locked items".
    pub include_locked_items: bool,
    /// "Anchor footprint": the reference of a footprint of the reference area, or none. The copy turns and moves so that the matching footprint of each target lands where
    /// that footprint of the reference area is, instead of lining the centres of the two areas up.
    pub anchor: Option<String>,
}

impl Default for RepeatOptions {
    fn default() -> Self {
        RepeatOptions { copy_placement: true, copy_routing: true, connected_routing_only: false, copy_other_items: true, group_items: false, include_locked_items: true, anchor: None }
    }
}

/// One edit of the multichannel tool. On the wire `{"op": "multichannel", "verb": "<verb>", ...}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum MultichannelCmd {
    /// `pcbnew.Multichannel.generatePlacementRuleAreas` after the dialog's OK: a rule area for each row ticked. `replace_existing` is "Replace existing placement rule areas", `group_items`
    /// "Group footprints with their placement rule areas" (the dialog turns it off for the Groups tab).
    GenerateRuleAreas {
        areas: Vec<AreaPick>,
        #[serde(default)]
        replace_existing: bool,
        #[serde(default)]
        group_items: bool,
    },
    /// `pcbnew.Multichannel.repeatLayout` after the dialog's OK: the layout of the rule area `reference` (a zone id) copied onto the rule areas `targets` (zone ids) that match it
    /// (one that does not is left alone), as `options` say.
    RepeatLayout {
        reference: String,
        targets: Vec<String>,
        #[serde(default)]
        options: RepeatOptions,
    },
    /// `pcbnew.InteractiveDrawing.applyDesignBlockLayout`: the board layout of the design block `block` -- `text` is its `.kicad_pcb` -- repeated onto the footprints of the group
    /// `group` (which is linked to the block).
    ApplyDesignBlockLayout { group: String, block: String, text: String },
}

impl MultichannelCmd {
    /// The ids the command names, for the journal and the activity feed.
    pub fn ids(&self) -> Vec<&str> {
        match self {
            MultichannelCmd::GenerateRuleAreas { areas, .. } => areas.iter().map(|a| a.key.as_str()).collect(),
            MultichannelCmd::RepeatLayout { reference, targets, .. } => std::iter::once(reference.as_str()).chain(targets.iter().map(String::as_str)).collect(),
            MultichannelCmd::ApplyDesignBlockLayout { group, .. } => vec![group.as_str()],
        }
    }

    /// The one-line description the CLI journal and the activity feed show.
    pub fn describe(&self) -> String {
        match self {
            MultichannelCmd::GenerateRuleAreas { areas, replace_existing, group_items } => {
                format!("multichannel generate {} area(s){}{}", areas.len(), if *replace_existing { " --replace" } else { "" }, if *group_items { " --group" } else { "" })
            }
            MultichannelCmd::RepeatLayout { reference, targets, options } => format!(
                "multichannel repeat {reference} -> {}{}{}{}{}",
                targets.join(","),
                if options.copy_placement { "" } else { " --no-placement" },
                if options.copy_routing { "" } else { " --no-routing" },
                if options.copy_other_items { "" } else { " --no-other-items" },
                options.anchor.as_deref().map(|a| format!(" --anchor {a}")).unwrap_or_default()
            ),
            MultichannelCmd::ApplyDesignBlockLayout { group, block, text } => format!("multichannel apply-design-block {block} -> {group} ({} bytes of KiCad text)", text.len()),
        }
    }

    pub fn kind(&self) -> &'static str {
        "multichannel"
    }
}

// --------------------------------------------------------------------------------------------------------------------------------- rule areas

/// A placement rule area of the board (`RULE_AREA`): the zone, the footprints it holds and where its centre is.
#[derive(Debug, Clone)]
pub(crate) struct RuleArea {
    pub zone_id: String,
    pub name: String,
    pub outline: Vec<Point>,
    pub layers: Vec<String>,
    pub center: Point,
    /// The footprints it holds: indexes into the board's footprints.
    pub members: Vec<usize>,
}

/// What the Generate dialog lists: the rule area a sheet, a class or a group would make.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PotentialArea {
    pub kind: AreaKind,
    /// The sheet's path (`/ch1/`), the class's name or the group's name.
    pub key: String,
    /// The sheet's file, the dialog's "Sheet Name" column (it holds `m_sheetfile`); empty for a class or a group.
    pub file: String,
    /// The footprints it would hold, by reference.
    pub components: Vec<String>,
}

/// The sources the Placement tab of Rule Area Properties offers (`DIALOG_RULE_AREA_PROPERTIES::TransferDataToWindow`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlacementSources {
    pub sheets: Vec<String>,
    pub classes: Vec<String>,
    pub groups: Vec<String>,
}

/// The reasons a target does not match, as the dialog lists them, and the pairs when it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetCompat {
    /// The target's zone id and name.
    pub id: String,
    pub name: String,
    pub ok: bool,
    /// "OK", or the first reason (`RULE_AREA_COMPAT_DATA::m_errorMsg`).
    pub message: String,
    pub reasons: Vec<String>,
    /// The footprints of the target, by reference.
    pub components: Vec<String>,
    /// The footprint of the target that stands for each footprint of the reference area: (reference's, target's).
    pub pairs: Vec<(String, String)>,
}

/// What the Repeat Layout dialog shows of a reference area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepeatPlan {
    pub id: String,
    pub name: String,
    /// The footprints of the reference area, sorted: the choices of "Anchor footprint".
    pub components: Vec<String>,
    /// Every other placement rule area, the ones that match first and each in order of name.
    pub targets: Vec<TargetCompat>,
}

/// `FormatComponentList`: the references, sorted without regard to case, ten to a line.
fn format_component_list(refs: &[String]) -> String {
    let mut sorted: Vec<&String> = refs.iter().collect();
    sorted.sort_by_key(|r| r.to_lowercase());
    if sorted.is_empty() {
        return "(none)".to_string();
    }
    sorted.chunks(10).map(|line| line.iter().map(|r| r.as_str()).collect::<Vec<_>>().join(", ")).collect::<Vec<_>>().join("\n")
}

/// `resolveConnectionTopology`: the matcher run on the footprints of two areas, and what it says.
pub(crate) struct Resolution {
    pub ok: bool,
    pub message: String,
    pub reasons: Vec<String>,
    /// (index in the reference list, index in the target list).
    pub pairs: Vec<(usize, usize)>,
}

pub(crate) fn resolve_topology(ref_fps: &[McFootprint], ref_members: &[usize], tgt_fps: &[McFootprint], tgt_members: &[usize], codes: &BTreeMap<String, i64>) -> Resolution {
    let topo = |fps: &[McFootprint], members: &[usize]| -> Vec<eda_model::topo_match::TopoFootprint> {
        members
            .iter()
            .map(|&i| {
                let f = &fps[i];
                eda_model::topo_match::TopoFootprint {
                    reference: f.reference.clone(),
                    fpid: f.fpid.clone(),
                    pads: f.pads.iter().map(|(number, net)| (number.clone(), codes.get(net).copied().unwrap_or(0))).collect(),
                    symbol: f.symbol.clone(),
                }
            })
            .collect()
    };
    let names: std::collections::HashMap<i64, String> = codes.iter().map(|(n, c)| (*c, n.clone())).collect();
    let (ra, rb) = (topo(ref_fps, ref_members), topo(tgt_fps, tgt_members));
    let ga = Graph::build(&ra, &rb, &names);
    let gb = Graph::build(&rb, &ra, &names);
    match ga.find_isomorphism(&gb) {
        Ok(pairs) => Resolution { ok: true, message: "OK".to_string(), reasons: Vec::new(), pairs: pairs.into_iter().map(|(r, t)| (ref_members[r], tgt_members[t])).collect() },
        Err(found) => {
            let mut reasons: Vec<String> = found.iter().filter(|r: &&MismatchReason| !r.reason.is_empty()).map(reason_text).collect();
            if reasons.is_empty() {
                reasons.push("Topology mismatch".to_string());
            }
            if ref_members.len() != tgt_members.len() {
                let refs = |fps: &[McFootprint], m: &[usize]| m.iter().map(|&i| fps[i].reference.clone()).collect::<Vec<_>>();
                let (rr, tr) = (refs(ref_fps, ref_members), refs(tgt_fps, tgt_members));
                reasons.push(format!("Reference area total components: {}", rr.len()));
                reasons.push(format!("Reference area components:\n{}", format_component_list(&rr)));
                reasons.push(format!("Target area total components: {}", tr.len()));
                reasons.push(format!("Target area components:\n{}", format_component_list(&tr)));
            }
            Resolution { ok: false, message: reasons[0].clone(), reasons, pairs: Vec::new() }
        }
    }
}

/// How `resolveConnectionTopology` words a reason: `ref -> candidate: reason`, `ref: reason` or the reason alone.
fn reason_text(r: &MismatchReason) -> String {
    match (r.reference.is_empty(), r.candidate.is_empty()) {
        (false, false) => format!("{} -> {}: {}", r.reference, r.candidate, r.reason),
        (false, true) => format!("{}: {}", r.reference, r.reason),
        _ => r.reason.clone(),
    }
}

impl Board<'_> {
    /// `findComponentsInRuleArea` for a zone: the footprints the rule area's source names, found by the rule KiCad compiles for it (`memberOfSheetOrChildren`,
    /// `hasComponentClass`, `memberOfGroup`). A source that would not compile in a rule (a quote in it) or is empty finds nothing.
    pub(crate) fn mc_components_of(mc: &McBoard, placement: &PlacementArea) -> Vec<usize> {
        let source = placement.source.as_str();
        if source.is_empty() || source.contains('\'') {
            return Vec::new();
        }
        mc.fps
            .iter()
            .enumerate()
            .filter(|(_, f)| match placement.source_type {
                PlacementSourceType::Sheetname => member_of_sheet_or_children(&f.sheetname, source),
                PlacementSourceType::ComponentClass => f.classes.contains(source),
                PlacementSourceType::Group => member_of_group(&f.groups_up, source),
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// `FindExistingRuleAreas`: the rule areas that have placement turned on.
    pub(crate) fn mc_rule_areas(&self, mc: &McBoard) -> Vec<RuleArea> {
        let Some(rt) = self.design.routing.as_ref() else { return Vec::new() };
        rt.zones
            .iter()
            .filter(|z| z.is_rule_area && z.placement.enabled && z.parent_footprint.is_none())
            .map(|z| RuleArea {
                zone_id: z.id.clone(),
                name: z.name.clone(),
                outline: z.outline.clone(),
                layers: z.layer_set().into_iter().map(String::from).collect(),
                center: outline_centre(&z.outline).unwrap_or_default(),
                members: Self::mc_components_of(mc, &z.placement),
            })
            .collect()
    }

    /// `GeneratePotentialRuleAreas`: a row for each sheet (with its file), each component class and each named group the footprints are in, with the footprints each would hold.
    pub fn mc_potential_areas(&self) -> Vec<PotentialArea> {
        let mc = self.mc_board();
        self.mc_potential_areas_of(&mc)
    }

    pub(crate) fn mc_potential_areas_of(&self, mc: &McBoard) -> Vec<PotentialArea> {
        let sheets: BTreeSet<(String, String)> = mc.fps.iter().map(|f| (f.sheetname.clone(), f.sheetfile.clone())).collect();
        let classes: BTreeSet<String> = mc.fps.iter().flat_map(|f| f.classes.iter().cloned()).collect();
        let groups: BTreeSet<String> = mc.fps.iter().filter_map(|f| f.groups_up.first().cloned()).filter(|g| !g.is_empty()).collect();
        let names = |idx: Vec<usize>| -> Vec<String> { idx.into_iter().map(|i| mc.fps[i].reference.clone()).collect() };
        let mut out: Vec<PotentialArea> = Vec::new();
        for (path, file) in sheets {
            let members = mc.fps.iter().enumerate().filter(|(_, f)| in_sheet_or_below(&f.sheetname, &path)).map(|(i, _)| i).collect();
            out.push(PotentialArea { kind: AreaKind::Sheet, key: path, file, components: names(members) });
        }
        for class in classes {
            let members = mc.fps.iter().enumerate().filter(|(_, f)| f.classes.contains(&class)).map(|(i, _)| i).collect();
            out.push(PotentialArea { kind: AreaKind::ComponentClass, key: class, file: String::new(), components: names(members) });
        }
        for group in groups {
            // `queryComponentsInGroup`: the footprints directly in a group of that name (every group of that name)
            let members = mc.fps.iter().enumerate().filter(|(_, f)| f.group.as_ref().is_some_and(|_| f.groups_up.first() == Some(&group))).map(|(i, _)| i).collect();
            out.push(PotentialArea { kind: AreaKind::Group, key: group, file: String::new(), components: names(members) });
        }
        out
    }

    /// The Placement tab's choices (`DIALOG_RULE_AREA_PROPERTIES::TransferDataToWindow`): the sheet of every footprint (the empty one included), the component classes
    /// and the names of the groups the footprints are in.
    pub fn mc_sources(&self) -> PlacementSources {
        let mc = self.mc_board();
        let sheets: BTreeSet<String> = mc.fps.iter().map(|f| f.sheetname.clone()).collect();
        let classes: BTreeSet<String> = mc.fps.iter().flat_map(|f| f.classes.iter().cloned()).collect();
        let groups: BTreeSet<String> = mc.fps.iter().filter_map(|f| f.groups_up.first().cloned()).filter(|g| !g.is_empty()).collect();
        PlacementSources { sheets: sheets.into_iter().collect(), classes: classes.into_iter().collect(), groups: groups.into_iter().collect() }
    }

    /// `CheckRACompatibility`: the reference area (a zone id) matched against every other placement rule area. `Err` when the zone is not a placement rule area.
    pub fn mc_compat(&self, reference: &str) -> Result<RepeatPlan, Vec<CheckResult>> {
        let mc = self.mc_board();
        let areas = self.mc_rule_areas(&mc);
        let Some(refa) = areas.iter().find(|a| a.zone_id == reference) else {
            return Err(fail("ops_not_placement_area", reference, "this is not a rule area with placement enabled"));
        };
        let mut targets: Vec<TargetCompat> = Vec::new();
        for t in areas.iter().filter(|a| a.zone_id != reference) {
            let res = resolve_topology(&mc.fps, &refa.members, &mc.fps, &t.members, &mc.codes);
            targets.push(TargetCompat {
                id: t.zone_id.clone(),
                name: t.name.clone(),
                ok: res.ok,
                message: res.message,
                reasons: res.reasons,
                components: t.members.iter().map(|&i| mc.fps[i].reference.clone()).collect(),
                pairs: res.pairs.iter().map(|&(r, g)| (mc.fps[r].reference.clone(), mc.fps[g].reference.clone())).collect(),
            });
        }
        // the dialog's order: the ones that match first, each in order of name
        targets.sort_by(|a, b| (!a.ok, &a.name, &a.id).cmp(&(!b.ok, &b.name, &b.id)));
        let mut components: Vec<String> = refa.members.iter().map(|&i| mc.fps[i].reference.clone()).collect();
        components.sort();
        Ok(RepeatPlan { id: refa.zone_id.clone(), name: refa.name.clone(), components, targets })
    }

    // ------------------------------------------------------------------------------------------------------------------------- the verbs

    /// Apply one [`MultichannelCmd`].
    pub(crate) fn apply_multichannel(&mut self, cmd: &MultichannelCmd) -> Result<(), Vec<CheckResult>> {
        match cmd {
            MultichannelCmd::GenerateRuleAreas { areas, replace_existing, group_items } => self.generate_rule_areas(areas, *replace_existing, *group_items),
            MultichannelCmd::RepeatLayout { reference, targets, options } => self.repeat_layout(reference, targets, options),
            MultichannelCmd::ApplyDesignBlockLayout { group, block, text } => self.apply_design_block_layout(group, block, text),
        }
    }

    /// `MULTICHANNEL_TOOL::AutogenerateRuleAreas`, after the dialog: see the module doc.
    fn generate_rule_areas(&mut self, picks: &[AreaPick], replace_existing: bool, group_items: bool) -> Result<(), Vec<CheckResult>> {
        let mc = self.mc_board();
        let potential = self.mc_potential_areas_of(&mc);
        let existing = self.mc_rule_areas(&mc);
        let copper: Vec<String> = self.model.board.layers.clone();
        if copper.is_empty() {
            return Err(fail("ops_no_copper_layers", "multichannel", "the board has no copper layers to make a rule area on"));
        }
        // Which rows were ticked, and which of them are there already (a placement rule area that holds exactly the same footprints).
        struct Row<'a> {
            area: &'a PotentialArea,
            members: Vec<usize>,
            old_zone: Option<String>,
        }
        let mut rows: Vec<Row> = Vec::new();
        for p in &potential {
            if !picks.iter().any(|k| k.kind == p.kind && k.key == p.key) {
                continue;
            }
            let members: Vec<usize> = p.components.iter().filter_map(|r| mc.index_of(r)).collect();
            let set: BTreeSet<usize> = members.iter().copied().collect();
            let old_zone = existing.iter().find(|a| !a.members.is_empty() && a.members.iter().copied().collect::<BTreeSet<usize>>() == set).map(|a| a.zone_id.clone());
            rows.push(Row { area: p, members, old_zone });
        }
        if picks.is_empty() {
            return Err(fail("ops_no_rule_areas", "multichannel", "no sheet, component class or group was chosen"));
        }
        if rows.is_empty() {
            return Err(fail("ops_unknown_area", "multichannel", "none of the sheets, component classes or groups chosen is on the board"));
        }
        let mut made: Vec<(Zone, Vec<usize>, bool)> = Vec::new();
        let mut removed: BTreeSet<String> = BTreeSet::new();
        for row in &rows {
            if row.old_zone.is_some() && !replace_existing {
                continue;
            }
            if row.members.is_empty() {
                continue;
            }
            // Groups are the user's explicit list of what belongs together: the area is made from everything in the group, not only its footprints.
            let boxes: Vec<Box4> = if row.area.kind == AreaKind::Group {
                let items = self.mc_group_item_boxes(&mc, &row.area.key);
                if items.is_empty() {
                    continue;
                }
                items
            } else {
                row.members.iter().map(|&i| mc.fps[i].bbox).collect()
            };
            let outline = rule_area_outline(&boxes, Nm::from_raw_shim(AREA_MARGIN_NM));
            if outline.len() < 3 {
                continue;
            }
            let mut zone = Zone {
                name: format!("auto-placement-area-{}", row.area.key),
                is_rule_area: true,
                outline,
                border_style: ZoneBorderStyle::None,
                placement: PlacementArea { enabled: true, source_type: row.area.kind.source_type(), source: row.area.key.clone() },
                ..Default::default()
            };
            zone.set_layer_set(&copper);
            if let Some(old) = &row.old_zone {
                removed.insert(old.clone());
            }
            made.push((zone, row.members.clone(), row.old_zone.is_some()));
        }
        if made.is_empty() {
            return Err(fail("ops_no_rule_areas", "multichannel", "nothing to make: the areas chosen exist already (turn on Replace existing placement rule areas), hold no footprint or have no extent"));
        }
        // Land the zones. The areas that are replaced go first: a zone's id is made from its net, layer and outline, so the area made again over the same footprints has the id of
        // the one it replaces, and is new all the same.
        self.routing_mut().zones.retain(|z| !removed.contains(&z.id));
        let before: BTreeSet<String> = self.design.routing.iter().flat_map(|r| r.zones.iter().map(|z| z.id.clone())).collect();
        {
            let rt = self.routing_mut();
            for (z, _, _) in &made {
                rt.zones.push(z.clone());
            }
            rt.assign_missing_ids();
        }
        // The ids the new zones got, in the order they were made (a zone with the same net, layer and outline as another gets a suffix).
        let new_ids: Vec<String> = self.design.routing.iter().flat_map(|r| r.zones.iter()).filter(|z| !before.contains(&z.id)).map(|z| z.id.clone()).collect();
        if group_items {
            // `if( m_areas.m_options.m_groupItems )`: every area made gets a group of its zone and its footprints, after the footprints have left the groups they were in.
            let zones_in_order: Vec<(String, &Vec<usize>)> = made.iter().zip(&new_ids).map(|((_, members, _), id)| (id.clone(), members)).collect();
            for (zone_id, members) in zones_in_order {
                if members.is_empty() {
                    continue;
                }
                let refs: Vec<String> = members.iter().map(|&i| mc.fps[i].reference.clone()).collect();
                let dr = self.drawings_mut();
                // `pruneExistingGroups`: only the groups that lost a member are tested against the `< 2` rule
                let mut gone = refs.clone();
                gone.push(zone_id.clone());
                let affected = eda_model::groups::groups_holding(&dr.groups, &gone);
                for g in dr.groups.iter_mut() {
                    g.member_ids.retain(|m| !refs.contains(m) && *m != zone_id);
                }
                eda_model::groups::prune_affected_groups(&mut dr.groups, &affected);
                let mut ids = vec![zone_id];
                ids.extend(refs);
                dr.groups.push(eda_model::ir::Group { id: String::new(), name: String::new(), member_ids: ids, design_block: String::new() });
                eda_model::groups::GroupHost::assign_group_ids(dr);
            }
        }
        Ok(())
    }

    /// `queryBoardItemsInGroup` and their boxes: every item directly in a group of that name -- a footprint by `GetBoundingBox( false )`, the rest by their own box.
    fn mc_group_item_boxes(&self, mc: &McBoard, name: &str) -> Vec<Box4> {
        let Some(dr) = self.design.drawings.as_ref() else { return Vec::new() };
        let mut boxes: Vec<Box4> = Vec::new();
        for g in dr.groups.iter().filter(|g| g.name == name) {
            for m in &g.member_ids {
                if let Some(i) = mc.index_of(m) {
                    boxes.push(mc.fps[i].bbox);
                } else if let Some(b) = self.mc_item_box(m) {
                    boxes.push(b);
                }
            }
        }
        boxes
    }

    /// The box of a board item other than a footprint (`BOARD_ITEM::GetBoundingBox`), a group's the box of what it holds.
    pub(crate) fn mc_item_box(&self, id: &str) -> Option<Box4> {
        let union = |a: Option<Box4>, b: Box4| -> Option<Box4> { Some(a.map_or(b, |a| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))) };
        let around = |pts: &[Point], r: i64| -> Option<Box4> { eda_model::multichannel::polygon_box(pts).map(|b| (b.0 - Nm::from_raw_shim(r), b.1 - Nm::from_raw_shim(r), b.2 + Nm::from_raw_shim(r), b.3 + Nm::from_raw_shim(r))) };
        if let Some(rt) = &self.design.routing {
            if let Some(t) = rt.tracks.iter().find(|t| t.id == id) {
                return around(&t.pts, (t.width / 2).raw_shim());
            }
            if let Some(v) = rt.vias.iter().find(|v| v.id == id) {
                return Some((v.at.x - v.diameter / 2, v.at.y - v.diameter / 2, v.at.x + v.diameter / 2, v.at.y + v.diameter / 2));
            }
            if let Some(z) = rt.zones.iter().find(|z| z.id == id) {
                return around(&z.outline, 0);
            }
        }
        let dr = self.design.drawings.as_ref()?;
        if let Some(s) = dr.shapes.iter().find(|s| s.id() == id) {
            let pts = match s {
                eda_model::ir::Shape::Circle { center, end, .. } => {
                    let r = (((end.x - center.x).raw_shim() as f64).hypot((end.y - center.y).raw_shim() as f64)).round() as i64;
                    vec![Point { x: center.x - Nm::from_raw_shim(r), y: center.y - Nm::from_raw_shim(r) }, Point { x: center.x + Nm::from_raw_shim(r), y: center.y + Nm::from_raw_shim(r) }]
                }
                eda_model::ir::Shape::Arc { start, mid, end, .. } => eda_model::ir::tessellate_arc(*start, *mid, *end, 16),
                other => other.points(),
            };
            return around(&pts, (s.stroke_width() / 2).raw_shim());
        }
        if let Some(t) = dr.texts.iter().find(|t| t.id == id) {
            let half = (t.content.chars().count().max(1) as i64 * t.size_nm * 6 / 10) / 2;
            return Some((t.at.x - half, t.at.y - t.size_nm / 2, t.at.x + half, t.at.y + t.size_nm / 2));
        }
        if let Some(d) = dr.dimensions.iter().find(|d| d.id == id) {
            return around(&[d.start, d.end], (d.stroke_width / 2).raw_shim());
        }
        if let Some(b) = dr.board_items.text_boxes.iter().find(|b| b.id == id) {
            let [x0, y0, x1, y1] = b.bounds();
            return Some((x0, y0, x1, y1));
        }
        if let Some(b) = dr.board_items.points.iter().find(|b| b.id == id) {
            let [x0, y0, x1, y1] = b.bounds();
            return Some((x0, y0, x1, y1));
        }
        if let Some(g) = dr.group(id) {
            let mut acc: Option<Box4> = None;
            for m in &g.member_ids {
                let member = self.pose_of(m).and_then(|_| self.mc_board_box_of(m)).or_else(|| self.mc_item_box(m));
                if let Some(b) = member {
                    acc = union(acc, b);
                }
            }
            return acc;
        }
        None
    }

    /// The box of one footprint (`GetBoundingBox( false )`).
    fn mc_board_box_of(&self, reference: &str) -> Option<Box4> {
        let mc = self.mc_board();
        mc.index_of(reference).map(|i| mc.fps[i].bbox)
    }
}
