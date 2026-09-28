//! Route a board the parity harness dumped, pass after pass, and time it.
//!
//!     cargo run --release -p eda-freeroute --example route -- <dump.txt> [max passes]
//!
//! Prints each pass's items and results, and what is left unrouted.

use std::time::Instant;

use eda_freeroute::autoroute::batch::{autoroute_passes, pass_items};
use eda_freeroute::dump::read_board;
use eda_freeroute::model::ItemKind;
use eda_freeroute::routing::RoutingBoard;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: route <dump.txt> [max passes]");
    let max_passes: i32 = args.next().map_or(99, |a| a.parse().expect("a number of passes"));
    let text = std::fs::read_to_string(&path).expect("a readable dump");
    let board = read_board(&text).expect("a board");
    let mut rb = RoutingBoard::new(board);
    let start = Instant::now();
    for s in autoroute_passes(&mut rb, 1, max_passes) {
        println!("pass {}: {} items, {} routed, {} not routed ({:.2?} so far)", s.pass_no, s.items, s.routed, s.not_routed, start.elapsed());
    }
    let (mut traces, mut vias) = (0, 0);
    for i in 0..rb.board.items.len() {
        if !rb.is_on_board(i) {
            continue;
        }
        match rb.item(i).kind {
            ItemKind::Trace { .. } => traces += 1,
            ItemKind::Via { .. } => vias += 1,
            _ => {}
        }
    }
    println!("done in {:.2?}: {} traces, {} vias, {} items still unrouted", start.elapsed(), traces, vias, pass_items(&rb).len());
}
