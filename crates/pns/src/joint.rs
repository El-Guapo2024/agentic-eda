//! Port of `PNS::JOINT` (`pcbnew/router/pns_joint.h`): a 2D point on a given
//! net that links together the items touching it. `NODE` hashes joints by
//! `(pos, net)` -- see that file's `HASH_TAG` -- so connectivity queries
//! ("what's the next segment after this one?", "assemble the whole line
//! through this point") are O(1) lookups instead of a board-wide scan.
//!
//! Traversal logic (`NextSegment`, `IsLineCorner`, ...) needs to inspect
//! the *kind* of each linked item, which means looking items up by id --
//! so, unlike KiCad's `JOINT` (which holds live `ITEM*` pointers and can
//! answer those questions itself), this port keeps `Joint` as plain data
//! (just the linked [`ItemId`]s) and implements the traversal methods on
//! [`crate::node::Node`], which owns the item table they need to consult.

use crate::item::{ItemId, Net};
use crate::layer::LayerRange;
use eda_model::ir::Point;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JointKey {
    /// `eda_model::ir::Point` doesn't derive `Hash`, so the coordinates are
    /// stored as a plain tuple here instead of widening that shared type.
    pos: (eda_model::ir::Um, eda_model::ir::Um),
    /// `None` for an unconnected (no-net) point; a net name otherwise.
    /// Stored as the owned string (not `Rc<str>`) so the key type needs no
    /// lifetime and is cheap enough to build per-query at this project's
    /// board sizes.
    net: Option<String>,
}

impl JointKey {
    pub fn new(pos: Point, net: &Net) -> Self {
        JointKey { pos: (pos.x, pos.y), net: net.as_ref().map(|n| n.to_string()) }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Joint {
    pub layers: LayerRange,
    pub links: Vec<ItemId>,
}

impl Joint {
    pub fn link(&mut self, id: ItemId) {
        if !self.links.contains(&id) {
            self.links.push(id);
        }
    }

    /// Returns `true` if the joint has no more links (the caller should
    /// drop it) -- `JOINT::Unlink`'s return value.
    pub fn unlink(&mut self, id: ItemId) -> bool {
        self.links.retain(|&x| x != id);
        self.links.is_empty()
    }
}
