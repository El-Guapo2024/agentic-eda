//! Create Array in the Footprint Editor: grid and circular arrays of the items of a library footprint -- its pads, graphics, texts, dimensions, zones, text boxes,
//! points, images, barcodes, tables and groups -- with the pads of the copies numbered by KiCad's schemes.
//!
//! Ports `ARRAY_TOOL::CreateArray` as it runs with `m_isFootprintEditor` (`pcbnew/tools/array_tool.cpp` at KiCad 8303b2ad): the dialog hands
//! it `enableArrayNumbering`, so the grid's and the circle's "Pad numbering" panels exist; the copies are made with
//! `FOOTPRINT::DuplicateItem` (a pad is copied with its number, a field is not copied at all: "Fields cannot be duplicated, especially mandatory fields"), which keeps
//! every look of the item ([`crate::fp_copy::copy_within`]: the line style, the hatch, the text's looks, the lock; a group with everything below it, UI-LEFT-2 FF2);
//! every item takes its point (`TransformItem`: `Move` by the point's offset, then `Rotate` about its own new position when the circular array turns its items); and a
//! pad that `CanHaveNumber()` takes the next number of `ARRAY_PAD_NUMBER_PROVIDER` when the options number their items.
//!
//! * **Numbers follow the points.** KiCad walks the points from the last to the first, making the copies and leaving the selection itself on the
//!   last point; the provider is asked in the order of the *points* (the first copy is point 0), and within a point in the order of the selection. Here
//!   the selection keeps point 0 and the copies take the others, which are the same positions with the same numbers on them (the ids differ: a copy
//!   of KiCad's last point is the original, here it is the first -- same difference [`crate::array`] documents for the board).
//! * **Which numbers are free**: the numbers of the pads that are *not* selected are reserved (a selected pad gives its number up); with "From
//!   start value" nothing is reserved and the run starts where the entries say.
//! * **Arrange selection** puts the selected items on the points one after the other and numbers nothing (`ShouldArrangeSelection()` returns before
//!   the numbering).
//! * **A group** moves and turns as one (`PCB_GROUP::Move`, `Rotate`): its position is `PCB_GROUP::GetPosition`, the centre of its box -- here the box of its items'
//!   own positions (KiCad's is the box of what they draw; the two differ only for a circular array's offsets).

use eda_model::units::Nm;
use super::array::shape_position;
use super::array_numbering::{NumberingAxes, PadNumberProvider, PadNumbering};
use super::library_editors::pad_can_have_number;
use super::fp_copy::copy_within;
use super::fp_items::{transform_ids, FIELD_PREFIX};
use super::pcb_transform::Xform;
use super::{ArrayGeometry, Board};
use eda_model::groups::GroupTree;
use eda_model::ir::{LibraryFootprint, Point};
use eda_model::CheckResult;
use std::collections::BTreeSet;

/// `BOARD_ITEM::GetPosition()` of the leaf `id`: a pad's and a text's anchor, `EDA_SHAPE::getPosition` for a graphic, a dimension's start, a zone's first corner, a
/// text box's first corner, a point's, an image's, a barcode's and a table's position.
fn leaf_position(fp: &LibraryFootprint, id: &str) -> Option<Point> {
    if let Some(p) = fp.pads.iter().find(|p| p.id == id) {
        return Some(p.at);
    }
    if let Some(g) = fp.graphics.iter().find(|g| g.id() == id) {
        return Some(shape_position(g));
    }
    if let Some(t) = fp.texts.iter().find(|t| t.id == id) {
        return Some(t.at);
    }
    let it = &fp.items;
    it.dimensions
        .iter()
        .find(|d| d.id == id)
        .map(|d| d.start)
        .or_else(|| it.zones.iter().find(|z| z.id == id).and_then(|z| z.outline.first().copied()))
        .or_else(|| it.text_boxes.iter().find(|t| t.id == id).map(|t| t.pts.first().copied().unwrap_or(t.start)))
        .or_else(|| it.points.iter().find(|p| p.id == id).map(|p| p.at))
        .or_else(|| it.images.iter().find(|i| i.id == id).map(|i| i.at))
        .or_else(|| it.barcodes.iter().find(|b| b.id == id).map(|b| b.at))
        .or_else(|| it.tables.iter().find(|t| t.id == id).map(|t| t.at))
}

/// The leaves an item of the selection stands for: itself, or a group's items.
fn leaves_of(fp: &LibraryFootprint, id: &str) -> Vec<String> {
    let tree = GroupTree(&fp.items.groups);
    if tree.is_group(id) {
        tree.group_leaves(id).into_iter().filter(|l| !l.starts_with(FIELD_PREFIX)).collect()
    } else {
        vec![id.to_string()]
    }
}

/// `GetPosition()` of an item of the selection (a group's: the centre of its items' box, see the module doc).
fn position(fp: &LibraryFootprint, id: &str) -> Option<Point> {
    let pts: Vec<Point> = leaves_of(fp, id).iter().filter_map(|l| leaf_position(fp, l)).collect();
    if pts.len() == 1 || !GroupTree(&fp.items.groups).is_group(id) {
        return pts.first().copied();
    }
    let (x0, x1) = (pts.iter().map(|p| p.x).min()?, pts.iter().map(|p| p.x).max()?);
    let (y0, y1) = (pts.iter().map(|p| p.y).min()?, pts.iter().map(|p| p.y).max()?);
    Some(Point { x: Nm::new((x0.get() + x1.get()) / 2), y: Nm::new((y0.get() + y1.get()) / 2) })
}

/// `TransformItem` on the item `id` (its leaves) as it stands at `from`: moved by its point's offset, then turned about its new position.
fn transform_item(fp: &mut LibraryFootprint, id: &str, from: Point, geometry: &ArrayGeometry, index: i64) {
    let (offset, turn) = geometry.transform(index, from);
    let leaves: BTreeSet<String> = leaves_of(fp, id).into_iter().collect();
    if offset != (Point { x: Nm::ZERO, y: Nm::ZERO }) {
        transform_ids(fp, &leaves, Xform::Move { dx: offset.x, dy: offset.y });
    }
    if turn != 0 {
        transform_ids(fp, &leaves, Xform::Rotate { pivot: Point { x: from.x + offset.x, y: from.y + offset.y }, angle: turn });
    }
}

impl Board<'_> {
    /// See [`Cmd::CreateFootprintArray`](crate::Cmd::CreateFootprintArray) and the module doc.
    pub(crate) fn create_footprint_array(&mut self, footprint: &str, ids: &[String], geometry: &ArrayGeometry, arrange: bool, numbering: Option<&PadNumbering>) -> Result<(), Vec<CheckResult>> {
        let bad = |what: String| vec![CheckResult::fail("ops_bad_array", "array", what)];
        if ids.is_empty() {
            return Err(bad("no ids given".into()));
        }
        geometry.validate().map_err(bad)?;
        let axes = match numbering {
            Some(n) if !arrange => Some(n.resolve(matches!(geometry, ArrayGeometry::Circular { .. })).map_err(bad)?),
            _ => None,
        };
        let fp = self.library_footprint_mut(footprint)?;

        // The items the array works on, in the order given; an id that names nothing in the footprint is dropped, and so is a field ("Fields cannot be duplicated").
        let live: BTreeSet<String> = fp.pads.iter().map(|p| p.id.clone()).chain(fp.graphics.iter().map(|g| g.id().to_string())).chain(fp.texts.iter().map(|t| t.id.clone())).chain(fp.items.ids().map(String::from)).collect();
        let mut selection: Vec<String> = Vec::new();
        for id in ids {
            if live.contains(id) && !id.starts_with(FIELD_PREFIX) && !selection.contains(id) && position(fp, id).is_some() {
                selection.push(id.clone());
            }
        }
        if selection.is_empty() {
            return Err(vec![CheckResult::fail("ops_unknown_array", "array", "none of the given ids name an item of the footprint that an array copies")]);
        }

        if arrange {
            arrange_into_array(fp, &selection, geometry);
        } else {
            duplicate_into_array(fp, &selection, geometry, axes.as_ref());
        }
        fp.assign_missing_ids();
        Ok(())
    }
}

/// `ShouldArrangeSelection()`: the selected items, in order, onto the points. The first item starts the run where it is; every later one is put
/// at the first item's position (as the transform of point 0 left it: `item->SetPosition( firstItem->GetPosition() )`) and moved to its point.
fn arrange_into_array(fp: &mut LibraryFootprint, selection: &[String], geometry: &ArrayGeometry) {
    let size = geometry.size().max(0) as usize;
    let mut first: Option<Point> = None;
    for (index, id) in selection.iter().enumerate() {
        if index >= size {
            break;
        }
        let Some(mut here) = position(fp, id) else { continue };
        if let Some(origin) = first {
            let leaves: BTreeSet<String> = leaves_of(fp, id).into_iter().collect();
            transform_ids(fp, &leaves, Xform::Move { dx: origin.x - here.x, dy: origin.y - here.y });
            here = origin;
        }
        transform_item(fp, id, here, geometry, index as i64);
        if first.is_none() {
            first = position(fp, id);
        }
    }
}

/// The default branch: point 0 is the selection itself, every other point gets a copy of the whole selection, made from the items as they were selected (the
/// copies first, the selection moved to point 0 last); the pads are numbered point by point, in the order of the selection, when the options number their items.
fn duplicate_into_array(fp: &mut LibraryFootprint, selection: &[String], geometry: &ArrayGeometry, axes: Option<&NumberingAxes>) {
    let size = geometry.size().max(0);
    let selected_leaves: BTreeSet<String> = selection.iter().flat_map(|id| leaves_of(fp, id)).collect();
    // `unchangingPadNumbers`: the numbers of the pads that are not selected stay reserved; a selected pad gives its number up.
    let reserved: BTreeSet<String> = fp.pads.iter().filter(|p| !selected_leaves.contains(&p.id)).map(|p| p.number.clone()).collect();
    let mut provider = axes.map(|a| PadNumberProvider::new(reserved, geometry, a));
    let from: Vec<Point> = selection.iter().map(|id| position(fp, id).unwrap_or_default()).collect();
    // the items of each point, in the order of the selection: point 0 the selection itself
    let mut per_point: Vec<Vec<String>> = vec![selection.to_vec()];
    for index in 1..size {
        let map = copy_within(fp, selection);
        let copies: Vec<String> = selection.iter().map(|id| map.get(id).cloned().unwrap_or_default()).collect();
        for (k, id) in copies.iter().enumerate() {
            if !id.is_empty() {
                transform_item(fp, id, from[k], geometry, index);
            }
        }
        per_point.push(copies);
    }
    for (k, id) in selection.iter().enumerate() {
        transform_item(fp, id, from[k], geometry, 0);
    }
    if let Some(provider) = provider.as_mut() {
        for items in &per_point {
            for id in items.iter().filter(|id| !id.is_empty()) {
                for leaf in leaves_of(fp, id) {
                    if let Some(pad) = fp.pads.iter_mut().find(|p| p.id == leaf) {
                        if pad_can_have_number(pad) {
                            pad.number = provider.next_number();
                        }
                    }
                }
            }
        }
    }
}
