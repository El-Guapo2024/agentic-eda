//! Create Array in the Footprint Editor: grid and circular arrays of the pads, graphics and texts of a library footprint, with the
//! pads of the copies numbered by KiCad's schemes.
//!
//! Ports `ARRAY_TOOL::CreateArray` as it runs with `m_isFootprintEditor` (`pcbnew/tools/array_tool.cpp` at KiCad 8303b2ad): the dialog hands
//! it `enableArrayNumbering`, so the grid's and the circle's "Pad numbering" panels exist; the copies are made with
//! `FOOTPRINT::DuplicateItem` (a pad is copied with its number, a field is not copied at all); every item takes its point
//! (`TransformItem`: `Move` by the point's offset, then `Rotate` about its own new position when the circular array turns its items); and a pad that
//! `CanHaveNumber()` takes the next number of `ARRAY_PAD_NUMBER_PROVIDER` when the options number their items.
//!
//! * **Numbers follow the points.** KiCad walks the points from the last to the first, making the copies and leaving the selection itself on the
//!   last point; the provider is asked in the order of the *points* (the first copy is point 0), and within a point in the order of the selection. Here
//!   the selection keeps point 0 and the copies take the others, which are the same positions with the same numbers on them (the ids differ: a copy
//!   of KiCad's last point is the original, here it is the first -- same difference [`crate::array`] documents for the board).
//! * **Which numbers are free**: the numbers of the pads that are *not* selected are reserved (a selected pad gives its number up); with "From
//!   start value" nothing is reserved and the run starts where the entries say.
//! * **Arrange selection** puts the selected items on the points one after the other and numbers nothing (`ShouldArrangeSelection()` returns before
//!   the numbering).

use eda_model::units::Nm;
use super::array::shape_position;
use super::array_numbering::{NumberingAxes, PadNumberProvider, PadNumbering};
use super::library_editors::pad_can_have_number;
use super::pcb_transform::{transform_shape, transform_text, Xform};
use super::{ArrayGeometry, Board};
use eda_model::ir::{LibraryFootprint, LibraryPad, Point, Shape, Text};
use eda_model::CheckResult;
use std::collections::BTreeSet;

/// One selected item of the footprint, by its place in the footprint's lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Pad(usize),
    Graphic(usize),
    Text(usize),
}

/// A copy of an item before it is placed.
#[derive(Clone)]
enum Piece {
    Pad(LibraryPad),
    Graphic(Shape),
    Text(Text),
}

fn norm360(v: i64) -> i64 {
    v.rem_euclid(360_000)
}

impl Piece {
    fn of(fp: &LibraryFootprint, item: Item) -> Piece {
        match item {
            Item::Pad(i) => Piece::Pad(fp.pads[i].clone()),
            Item::Graphic(i) => Piece::Graphic(fp.graphics[i].clone()),
            Item::Text(i) => Piece::Text(fp.texts[i].clone()),
        }
    }

    /// `BOARD_ITEM::GetPosition()`: a pad's and a text's anchor, `EDA_SHAPE::getPosition` for a graphic.
    fn position(&self) -> Point {
        match self {
            Piece::Pad(p) => p.at,
            Piece::Graphic(s) => shape_position(s),
            Piece::Text(t) => t.at,
        }
    }

    /// `BOARD_ITEM::Move`.
    fn move_by(&mut self, dx: i64, dy: i64) {
        match self {
            Piece::Pad(p) => p.at = Point { x: p.at.x + Nm::from_raw_shim(dx), y: p.at.y + Nm::from_raw_shim(dy) },
            Piece::Graphic(s) => transform_shape(s, Xform::Move { dx: Nm::from_raw_shim(dx), dy: Nm::from_raw_shim(dy) }, 2),
            Piece::Text(t) => transform_text(t, Xform::Move { dx: Nm::from_raw_shim(dx), dy: Nm::from_raw_shim(dy) }, 2),
        }
    }

    /// `BOARD_ITEM::Rotate( GetPosition(), angle )` with `angle` in this crate's convention (clockwise on the screen): a pad turns about its
    /// own position, so only its orientation (KiCad's, counter-clockwise) changes.
    fn turn_about_itself(&mut self, angle: i64) {
        match self {
            Piece::Pad(p) => p.rot = norm360(p.rot as i64 - angle) as u32,
            Piece::Graphic(s) => {
                let pivot = shape_position(s);
                transform_shape(s, Xform::Rotate { pivot, angle }, 2);
            }
            Piece::Text(t) => {
                let pivot = t.at;
                transform_text(t, Xform::Rotate { pivot, angle }, 2);
            }
        }
    }

    /// `TransformItem`: `GetTransform( aIndex, aItem.GetPosition() )`, `Move( offset )`, `Rotate( GetPosition(), rotation )`.
    fn transform(&mut self, geometry: &ArrayGeometry, index: i64) {
        let (offset, turn) = geometry.transform(index, self.position());
        if offset != (Point { x: Nm::ZERO, y: Nm::ZERO }) {
            self.move_by(offset.x.raw_shim(), offset.y.raw_shim());
        }
        if turn != 0 {
            self.turn_about_itself(turn);
        }
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

        // The items the array works on, in the order given; an id that names nothing in the footprint is dropped.
        let mut selection: Vec<Item> = Vec::new();
        for id in ids {
            let item = fp
                .pads
                .iter()
                .position(|p| &p.id == id)
                .map(Item::Pad)
                .or_else(|| fp.graphics.iter().position(|g| g.id() == id).map(Item::Graphic))
                .or_else(|| fp.texts.iter().position(|t| &t.id == id).map(Item::Text));
            if let Some(item) = item {
                if !selection.contains(&item) {
                    selection.push(item);
                }
            }
        }
        if selection.is_empty() {
            return Err(vec![CheckResult::fail("ops_unknown_array", "array", "none of the given ids name a pad, graphic or text of the footprint")]);
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
fn arrange_into_array(fp: &mut LibraryFootprint, selection: &[Item], geometry: &ArrayGeometry) {
    let size = geometry.size().max(0) as usize;
    let mut first: Option<Point> = None;
    for (index, item) in selection.iter().enumerate() {
        if index >= size {
            break;
        }
        let mut piece = Piece::of(fp, *item);
        if let Some(origin) = first {
            let here = piece.position();
            piece.move_by((origin.x - here.x).raw_shim(), (origin.y - here.y).raw_shim());
        }
        piece.transform(geometry, index as i64);
        if first.is_none() {
            first = Some(piece.position());
        }
        match (*item, piece) {
            (Item::Pad(i), Piece::Pad(p)) => fp.pads[i] = p,
            (Item::Graphic(i), Piece::Graphic(s)) => fp.graphics[i] = s,
            (Item::Text(i), Piece::Text(t)) => fp.texts[i] = t,
            _ => unreachable!("a piece is made from its own item"),
        }
    }
}

/// The default branch: point 0 is the selection itself, every other point gets a copy of the whole selection; the pads are numbered point by
/// point, in the order of the selection, when the options number their items.
fn duplicate_into_array(fp: &mut LibraryFootprint, selection: &[Item], geometry: &ArrayGeometry, axes: Option<&NumberingAxes>) {
    let size = geometry.size().max(0);
    // `unchangingPadNumbers`: the numbers of the pads that are not selected stay reserved; a selected pad gives its number up.
    let reserved: BTreeSet<String> = fp.pads.iter().enumerate().filter(|(i, _)| !selection.contains(&Item::Pad(*i))).map(|(_, p)| p.number.clone()).collect();
    let mut provider = axes.map(|a| PadNumberProvider::new(reserved, geometry, a));
    // Every copy is made from the item as it was selected.
    let sources: Vec<Piece> = selection.iter().map(|i| Piece::of(fp, *i)).collect();

    let mut added: Vec<Piece> = Vec::new();
    for index in 0..size {
        for (item, source) in selection.iter().zip(&sources) {
            let mut piece = source.clone();
            piece.transform(geometry, index);
            if let (Some(provider), Piece::Pad(pad)) = (provider.as_mut(), &mut piece) {
                if pad_can_have_number(pad) {
                    pad.number = provider.next_number();
                }
            }
            if index == 0 {
                match (*item, piece) {
                    (Item::Pad(i), Piece::Pad(p)) => fp.pads[i] = p,
                    (Item::Graphic(i), Piece::Graphic(s)) => fp.graphics[i] = s,
                    (Item::Text(i), Piece::Text(t)) => fp.texts[i] = t,
                    _ => unreachable!("a piece is made from its own item"),
                }
            } else {
                // `ResetUuidDirect()`: the copy is a new item; its id is made with the rest
                match &mut piece {
                    Piece::Pad(p) => p.id.clear(),
                    Piece::Graphic(s) => s.set_id(String::new()),
                    Piece::Text(t) => t.id.clear(),
                }
                added.push(piece);
            }
        }
    }
    for piece in added {
        match piece {
            Piece::Pad(p) => fp.pads.push(p),
            Piece::Graphic(s) => fp.graphics.push(s),
            Piece::Text(t) => fp.texts.push(t),
        }
    }
}
