//! Port of `PNS_LAYER_RANGE` (`pcbnew/router/pns_layerset.h`): a contiguous
//! span of copper layers, used both for an item's own layer span (a track
//! segment is one layer, a through-via spans every layer) and for queries
//! ("does this item touch that layer range").
//!
//! KiCad identifies layers by its own global `PCB_LAYER_ID` enum; this port
//! identifies them by position in the board's own copper-layer list
//! (`BoardRules::layers`, outer-first -- `["F.Cu", "B.Cu"]` on a two-layer
//! board), via [`LayerMap`]. `-1` plays the same "invalid/unset" role
//! `PNS_LAYER_RANGE`'s default constructor gives it.

use std::collections::HashMap;

pub const UNDEFINED: i32 = -1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayerRange {
    start: i32,
    end: i32,
}

impl Default for LayerRange {
    /// KiCad's `PNS_LAYER_RANGE()` default ctor: both ends `-1` (unset),
    /// not `0` -- an unset range must never accidentally overlap layer 0.
    fn default() -> Self {
        LayerRange { start: UNDEFINED, end: UNDEFINED }
    }
}

impl LayerRange {
    pub fn new(a: i32, b: i32) -> Self {
        if a > b {
            LayerRange { start: b, end: a }
        } else {
            LayerRange { start: a, end: b }
        }
    }

    /// A single-layer range -- the common case (a track segment, an SMD pad).
    pub fn single(layer: i32) -> Self {
        LayerRange { start: layer, end: layer }
    }

    pub fn start(&self) -> i32 {
        self.start
    }

    pub fn end(&self) -> i32 {
        self.end
    }

    pub fn is_multilayer(&self) -> bool {
        self.start != self.end
    }

    pub fn overlaps(&self, other: &LayerRange) -> bool {
        if self.start < 0 || self.end < 0 || other.start < 0 || other.end < 0 {
            return false;
        }
        self.end >= other.start && self.start <= other.end
    }

    pub fn overlaps_layer(&self, layer: i32) -> bool {
        if self.start < 0 || self.end < 0 || layer < 0 {
            return false;
        }
        layer >= self.start && layer <= self.end
    }

    pub fn merge(&mut self, other: &LayerRange) {
        if self.start < 0 || self.end < 0 {
            *self = *other;
            return;
        }
        if other.start < 0 {
            return;
        }
        self.start = self.start.min(other.start);
        self.end = self.end.max(other.end);
    }

    /// Every layer index this range spans, inclusive.
    pub fn iter(&self) -> impl Iterator<Item = i32> {
        self.start..=self.end
    }
}

/// Board copper-layer name <-> index, outer-first (`F.Cu` = 0), mirroring
/// how `BoardRules::layers`/`eda_drc::board::build` already name layers.
/// Every PNS item's [`LayerRange`] is expressed in this index space so
/// overlap tests are plain integer comparisons, same as KiCad's own
/// `PCB_LAYER_ID` integers.
#[derive(Debug, Clone)]
pub struct LayerMap {
    names: Vec<String>,
    index: HashMap<String, i32>,
}

impl LayerMap {
    pub fn new(names: &[String]) -> Self {
        let names = names.to_vec();
        let index = names.iter().enumerate().map(|(i, n)| (n.clone(), i as i32)).collect();
        LayerMap { names, index }
    }

    pub fn index_of(&self, name: &str) -> Option<i32> {
        self.index.get(name).copied()
    }

    pub fn name_of(&self, idx: i32) -> &str {
        self.names.get(idx as usize).map(String::as_str).unwrap_or("F.Cu")
    }

    pub fn count(&self) -> usize {
        self.names.len()
    }

    /// `LayerRange` spanning every copper layer -- a through via's range.
    pub fn all(&self) -> LayerRange {
        LayerRange::new(0, (self.names.len().max(1) - 1) as i32)
    }

    /// `LayerRange` for a `(from_layer, to_layer)` pair of names, as a
    /// `Via`'s IR carries it. Unknown names fall back to the full span
    /// rather than panicking -- a malformed layer name is a DRC/import
    /// concern, not something the router should crash over.
    pub fn range_of(&self, from: &str, to: &str) -> LayerRange {
        match (self.index_of(from), self.index_of(to)) {
            (Some(a), Some(b)) => LayerRange::new(a, b),
            _ => self.all(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_basic() {
        let a = LayerRange::single(0);
        let b = LayerRange::new(0, 1);
        assert!(a.overlaps(&b));
        let c = LayerRange::single(1);
        assert!(!a.overlaps(&c));
        assert!(b.overlaps(&c));
    }

    #[test]
    fn layer_map_round_trips() {
        let m = LayerMap::new(&["F.Cu".into(), "In1.Cu".into(), "B.Cu".into()]);
        assert_eq!(m.index_of("B.Cu"), Some(2));
        assert_eq!(m.name_of(1), "In1.Cu");
        assert_eq!(m.all(), LayerRange::new(0, 2));
        assert_eq!(m.range_of("F.Cu", "B.Cu"), LayerRange::new(0, 2));
    }
}
