//! A uniform grid of cells over the board: the plain tree's spatial index.
//!
//! The plain tree answers every query sorted by item (see
//! [`crate::routing::tree`]), so its index's layout never shows, and it
//! need not be the Java's unbalanced [`crate::searchtree`], which contact
//! walks and the optimizer ask a great many small questions of. Each shape
//! is listed in every cell its bounding box meets; a query reports it from
//! the first cell they share, so once. Shapes spanning much of the board
//! (pours, big keepouts) wait in a list every query looks through.
//!
//! It finds what [`crate::searchtree::ShapeTree::overlaps`] would: every
//! shape whose bounds intersect the query's.

use crate::geometry::{IntBox, IntOctagon};

/// The cells along the board's longer side.
const CELLS_ALONG: i64 = 128;
/// A shape meeting more cells than this goes in the list of big ones.
const BIG_CELLS: usize = 1024;

#[derive(Debug, Clone, Copy)]
enum Place {
    /// Never found: its bounds are empty.
    Nowhere,
    Big,
    /// Listed in the cells `x0..=x1` by `y0..=y1`.
    Cells { x0: u32, y0: u32, x1: u32, y1: u32 },
}

#[derive(Debug, Clone)]
struct Slot<T> {
    bounds: IntOctagon,
    payload: T,
    place: Place,
}

/// A handle to a stored shape, stable until that shape is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotId(u32);

#[derive(Debug, Clone)]
pub struct CellGrid<T> {
    /// The lower left corner of cell 0, 0.
    x0: i64,
    y0: i64,
    cell: i64,
    nx: usize,
    ny: usize,
    /// Row by row, the shapes listed in each cell.
    cells: Vec<Vec<u32>>,
    big: Vec<u32>,
    slots: Vec<Option<Slot<T>>>,
    free: Vec<u32>,
    /// The shapes stored.
    live: usize,
}

impl<T: Copy> CellGrid<T> {
    /// A grid over `bounds`; shapes beyond it are listed in its border
    /// cells.
    pub fn new(bounds: &IntBox) -> Self {
        let w = (bounds.ur.x - bounds.ll.x).max(1);
        let h = (bounds.ur.y - bounds.ll.y).max(1);
        let cell = (w.max(h) + CELLS_ALONG - 1) / CELLS_ALONG;
        let (nx, ny) = ((w / cell + 1) as usize, (h / cell + 1) as usize);
        CellGrid { x0: bounds.ll.x, y0: bounds.ll.y, cell, nx, ny, cells: vec![Vec::new(); nx * ny], big: Vec::new(), slots: Vec::new(), free: Vec::new(), live: 0 }
    }

    fn column(&self, x: i64) -> u32 {
        x.saturating_sub(self.x0).div_euclid(self.cell).clamp(0, self.nx as i64 - 1) as u32
    }

    fn row(&self, y: i64) -> u32 {
        y.saturating_sub(self.y0).div_euclid(self.cell).clamp(0, self.ny as i64 - 1) as u32
    }

    fn cell_mut(&mut self, x: u32, y: u32) -> &mut Vec<u32> {
        &mut self.cells[y as usize * self.nx + x as usize]
    }

    pub fn insert(&mut self, bounds: IntOctagon, payload: T) -> SlotId {
        let place = if bounds.is_empty() {
            Place::Nowhere
        } else {
            let (x0, y0, x1, y1) = (self.column(bounds.left_x), self.row(bounds.bottom_y), self.column(bounds.right_x), self.row(bounds.top_y));
            if (x1 - x0 + 1) as usize * (y1 - y0 + 1) as usize > BIG_CELLS {
                Place::Big
            } else {
                Place::Cells { x0, y0, x1, y1 }
            }
        };
        let id = match self.free.pop() {
            Some(id) => id,
            None => {
                self.slots.push(None);
                (self.slots.len() - 1) as u32
            }
        };
        match place {
            Place::Nowhere => {}
            Place::Big => self.big.push(id),
            Place::Cells { x0, y0, x1, y1 } => {
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        self.cell_mut(x, y).push(id);
                    }
                }
            }
        }
        self.slots[id as usize] = Some(Slot { bounds, payload, place });
        self.live += 1;
        SlotId(id)
    }

    pub fn remove(&mut self, id: SlotId) {
        let slot = self.slots[id.0 as usize].take().expect("a stored shape");
        let unlist = |list: &mut Vec<u32>| {
            let k = list.iter().position(|&s| s == id.0).expect("a listed shape");
            list.swap_remove(k);
        };
        match slot.place {
            Place::Nowhere => {}
            Place::Big => unlist(&mut self.big),
            Place::Cells { x0, y0, x1, y1 } => {
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        unlist(self.cell_mut(x, y));
                    }
                }
            }
        }
        self.free.push(id.0);
        self.live -= 1;
    }

    pub fn payload(&self, id: SlotId) -> &T {
        &self.slots[id.0 as usize].as_ref().expect("a stored shape").payload
    }

    /// Onto `out`, in no particular order, the payloads `keep` accepts of
    /// every stored shape whose bounds intersect `shape`.
    pub fn overlaps(&self, shape: &IntOctagon, mut keep: impl FnMut(&T) -> bool, out: &mut Vec<T>) {
        if shape.is_empty() {
            return;
        }
        let (qx0, qy0, qx1, qy1) = (self.column(shape.left_x), self.row(shape.bottom_y), self.column(shape.right_x), self.row(shape.top_y));
        // A query over more cells than there are shapes (a long diagonal,
        // a small board) looks at the shapes instead.
        if (qx1 - qx0 + 1) as usize * (qy1 - qy0 + 1) as usize > self.live {
            for slot in self.slots.iter().flatten() {
                if slot.bounds.intersects(shape) && keep(&slot.payload) {
                    out.push(slot.payload);
                }
            }
            return;
        }
        for y in qy0..=qy1 {
            for x in qx0..=qx1 {
                for &s in &self.cells[y as usize * self.nx + x as usize] {
                    let slot = self.slots[s as usize].as_ref().expect("a listed shape");
                    let Place::Cells { x0, y0, .. } = slot.place else { unreachable!("a big shape listed in a cell") };
                    // Found from the first cell the two share, and only there.
                    if x == x0.max(qx0) && y == y0.max(qy0) && slot.bounds.intersects(shape) && keep(&slot.payload) {
                        out.push(slot.payload);
                    }
                }
            }
        }
        for &s in &self.big {
            let slot = self.slots[s as usize].as_ref().expect("a listed shape");
            if slot.bounds.intersects(shape) && keep(&slot.payload) {
                out.push(slot.payload);
            }
        }
    }
}
