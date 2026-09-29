//! Copper pours on the grid: the plane a pour can still be, once the
//! board's copper has carved it.
//!
//! The flood is deliberately *conservative*: a cell joins the plane only
//! if the net could legally put a track there, and a real filler lays
//! copper into gaps thinner than a track. Under-reporting reach fails a
//! board that might have been fine; over-reporting would pass a board with
//! an isolated pad, which is the failure this module exists to make
//! impossible.

use crate::grid::{Grid, Occ};
use eda_model::Pour;
use std::collections::VecDeque;

/// Cells on `layer` the poured net may occupy, as a flood-filled component
/// map. `None` means the cell is not pourable at all.
fn components(grid: &Grid, layer: u8, net_id: u32) -> (Vec<i32>, usize) {
    let w = grid.cells_x;
    let h = grid.cells_y;
    let idx = |x: i64, y: i64| (y * w + x) as usize;
    let mut comp = vec![-1i32; (w * h) as usize];
    let mut n = 0usize;
    for sy in 0..h {
        for sx in 0..w {
            if comp[idx(sx, sy)] != -1 {
                continue;
            }
            if !grid.in_outline(sx, sy) || !grid.passable_as_id(sx, sy, layer, net_id, Occ::Track) {
                continue;
            }
            let id = n as i32;
            n += 1;
            let mut q = VecDeque::from([(sx, sy)]);
            comp[idx(sx, sy)] = id;
            while let Some((x, y)) = q.pop_front() {
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w || ny >= h {
                        continue;
                    }
                    let i = idx(nx, ny);
                    if comp[i] != -1 {
                        continue;
                    }
                    if !grid.in_outline(nx, ny) || !grid.passable_as_id(nx, ny, layer, net_id, Occ::Track) {
                        continue;
                    }
                    comp[i] = id;
                    q.push_back((nx, ny));
                }
            }
        }
    }
    (comp, n)
}

/// The plane's body on `layer`: the cells of its largest pourable
/// component, row by row. Empty where nothing on the layer is pourable.
pub fn body_cells(grid: &Grid, spec: &Pour, layer: u8) -> Vec<(i64, i64)> {
    let net_id = grid.net_id_of(&spec.net);
    let (comp, ncomp) = components(grid, layer, net_id);
    let mut size = vec![0usize; ncomp];
    for &c in &comp {
        if c >= 0 {
            size[c as usize] += 1;
        }
    }
    let Some(body) = (0..ncomp).max_by_key(|&i| size[i]).map(|i| i as i32) else { return Vec::new() };
    let w = grid.cells_x;
    (0..comp.len()).filter(|&i| comp[i] == body).map(|i| (i as i64 % w, i as i64 / w)).collect()
}
