//! The items of a library footprint beyond its pads, graphics, texts and fields (UI-LEFT F2): the dimensions, the zones (a footprint's rule areas, keepouts and
//! non-copper zones), the text boxes, the points, the reference images, the barcodes, the tables and the groups a `.kicad_mod` holds.
//!
//! KiCad keeps all of them on the `FOOTPRINT` itself (`FOOTPRINT::GraphicalItems()` holds the dimensions, text boxes, images, barcodes and tables beside the shapes and
//! texts; `Zones()`, `Points()` and `Groups()` hold the rest), in the footprint's own frame like its pads, and the Footprint Editor draws and edits them with the board
//! editor's tools (`FOOTPRINT_EDIT_FRAME` runs `DRAWING_TOOL`, `EDIT_TOOL`, `PCB_GROUP_TOOL` and `ZONE_CREATE_HELPER` on its one footprint). Before this module the
//! footprint reader kept each of them as the text of its form ([`crate::lib_keep::LibraryFootprintKeep::tokens`]), written back where KiCad writes it and never drawn.
//!
//! The types are the board's ([`Dimension`], [`Zone`], [`TextBox`], [`PcbPoint`], [`RefImage`], [`Barcode`], [`Table`], [`Group`]): the fields mean the same, read in
//! the footprint's frame instead of the board's -- the frame of [`crate::ir::LibraryPad::at`], seen from the top, un-rotated. A zone of a footprint has no net
//! (`ZONE` in a footprint is a rule area or a zone on a non-copper layer, or copper that takes the net of the pad it touches when the footprint is placed, which this
//! editor does not model: its `net` is empty). One additive field of [`crate::ir::LibraryFootprint`] holds them all, `items`.
//!
//! A group's members are the ids of the footprint's items: a pad, a graphic, a text, any item of this module, or another group. The file names them by uuid, which the
//! reader turns into ids through the uuids it keeps ([`crate::lib_keep::LibraryFootprintKeep::uuids`]) and the writer turns back.

use crate::barcode::Barcode;
use crate::ir::{next_item_id, Dimension, Group, Zone};
use crate::pcb_items::{PcbPoint, RefImage, TextBox};
use crate::table::Table;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// See the module doc.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintItems {
    /// `(dimension ..)` of the footprint (`PCB_DIM_ALIGNED`, `PCB_DIM_ORTHOGONAL`, `PCB_DIM_RADIAL`, `PCB_DIM_LEADER`, `PCB_DIM_CENTER`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dimensions: Vec<Dimension>,
    /// `(zone ..)` of the footprint: rule areas (keepouts) and zones (`FOOTPRINT::Zones()`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<Zone>,
    /// `(fp_text_box ..)` (`PCB_TEXTBOX` with a parent footprint).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_boxes: Vec<TextBox>,
    /// `(point ..)` (`FOOTPRINT::Points()`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<PcbPoint>,
    /// `(image ..)` (`PCB_REFERENCE_IMAGE`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<RefImage>,
    /// `(barcode ..)` (`PCB_BARCODE`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub barcodes: Vec<Barcode>,
    /// `(table ..)` (`PCB_TABLE`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<Table>,
    /// `(group ..)` (`FOOTPRINT::Groups()`): see the module doc for what a member is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
}

impl FootprintItems {
    pub fn is_empty(&self) -> bool {
        *self == FootprintItems::default()
    }

    /// Every id of the items of this module, groups included.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.dimensions
            .iter()
            .map(|d| d.id.as_str())
            .chain(self.zones.iter().map(|z| z.id.as_str()))
            .chain(self.text_boxes.iter().map(|t| t.id.as_str()))
            .chain(self.points.iter().map(|p| p.id.as_str()))
            .chain(self.images.iter().map(|i| i.id.as_str()))
            .chain(self.barcodes.iter().map(|b| b.id.as_str()))
            .chain(self.tables.iter().map(|t| t.id.as_str()))
            .chain(self.groups.iter().map(|g| g.id.as_str()))
    }

    /// Whether `id` names one of the items of this module (a group too).
    pub fn has(&self, id: &str) -> bool {
        self.ids().any(|i| i == id)
    }

    /// Give every item without an id the deterministic one (`next_item_id`), kind by kind in an order that does not depend on the order of the lists; the groups last,
    /// since a group's id is made from its members'.
    pub fn assign_missing_ids(&mut self, existing: &mut BTreeSet<String>) {
        existing.extend(self.ids().filter(|s| !s.is_empty()).map(String::from));
        crate::pcb_items::assign_ids(&mut self.dimensions, "dim", existing, |d| &mut d.id, Dimension::id_seed, |d| (d.start, d.end));
        crate::pcb_items::assign_ids(&mut self.zones, "zone", existing, |z| &mut z.id, Zone::id_seed, |z| (z.layer.clone(), z.outline.first().copied()));
        crate::pcb_items::assign_ids(&mut self.text_boxes, "tbx", existing, |t| &mut t.id, TextBox::id_seed, |t| (t.text.clone(), t.start, t.end));
        crate::pcb_items::assign_ids(&mut self.points, "pnt", existing, |p| &mut p.id, PcbPoint::id_seed, |p| (p.layer.clone(), p.at, p.size_nm));
        crate::pcb_items::assign_ids(&mut self.images, "img", existing, |i| &mut i.id, RefImage::id_seed, |i| (i.layer.clone(), i.at, i.data.len() as i64));
        crate::pcb_items::assign_ids(&mut self.barcodes, "bar", existing, |b| &mut b.id, Barcode::id_seed, |b| (b.text.clone(), b.at));
        crate::pcb_items::assign_ids(&mut self.tables, "tbl", existing, |t| &mut t.id, Table::id_seed, |t| (t.layer.clone(), t.at));
        let mut order: Vec<usize> = (0..self.groups.len()).collect();
        order.sort_by_key(|&i| self.groups[i].id_seed());
        for i in order {
            if self.groups[i].id.is_empty() {
                let id = next_item_id("grp", &self.groups[i].id_seed(), existing);
                existing.insert(id.clone());
                self.groups[i].id = id;
            }
        }
    }

    /// Take `id` out of every group (an item that is gone is no member of anything), and drop a group left with no member (`PCB_GROUP` of no item is not written).
    pub fn forget_member(&mut self, id: &str) {
        for g in &mut self.groups {
            g.member_ids.retain(|m| m != id);
        }
        crate::groups::drop_empty_groups(&mut self.groups);
    }

    /// Take out of the groups every member that is not in `live` (the ids of the footprint's items) -- after a delete -- and drop the groups left empty.
    pub fn prune_groups(&mut self, live: &BTreeSet<String>) {
        let groups: BTreeSet<String> = self.groups.iter().map(|g| g.id.clone()).collect();
        for g in &mut self.groups {
            g.member_ids.retain(|m| live.contains(m) || groups.contains(m));
        }
        crate::groups::drop_empty_groups(&mut self.groups);
    }
}
