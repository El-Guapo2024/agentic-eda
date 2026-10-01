//! Port of `PNS::ITEM` and its concrete kinds (`pcbnew/router/pns_item.h`,
//! `pns_solid.h`, `pns_segment.h`, `pns_via.h`).
//!
//! KiCad models these as a polymorphic class hierarchy (`SOLID`/`SEGMENT`/
//! `VIA` all extend `ITEM`, `SEGMENT`/`VIA` further extend `LINKED_ITEM`).
//! Rust has no inheritance; [`Item`] is a closed enum instead, with the
//! shared accessors (`net`, `layers`, `shape`, `hull`) implemented as a
//! match over the three kinds. This is a representation change only -- the
//! fields and semantics of each kind are what KiCad's headers declare.
//!
//! Arcs (`ARC_T`) are out of scope: this project's IR represents every
//! track as a polyline of straight segments (`eda_model::ir::Track::pts`),
//! so no arc items are ever constructed. `DIFF_PAIR_T`/`HOLE_T` are also
//! out of scope (no diff-pair concept in this port; a pad/via's drilled
//! hole is not separately collision-tested here -- see the crate's
//! `README`/`PARITY.md` for the full fidelity-gap list).

use crate::hull;
use crate::layer::LayerRange;
use eda_drc::kimath::Shape;
use eda_model::ir::{Point, Um};
use std::rc::Rc;

/// `PNS::NET_HANDLE` (an opaque per-board net identifier). KiCad hides the
/// representation behind a `void*` owned by its `ROUTER_IFACE`; this port
/// just carries the net name, `Rc`-shared so cloning an item is cheap.
/// `None` is KiCad's `nullptr` net -- an unconnected item (a mounting hole,
/// a bare graphic) that collides with everything regardless of net.
pub type Net = Option<Rc<str>>;

pub fn net_of(name: &str) -> Net {
    if name.is_empty() {
        None
    } else {
        Some(Rc::from(name))
    }
}

/// Two items on the same (non-`None`) net never collide with each other --
/// `ITEM::Collide`'s net check. Two unconnected (`None`-net) items, or one
/// connected and one not, still collide: only a shared real net is "safe".
pub fn same_net(a: &Net, b: &Net) -> bool {
    matches!((a, b), (Some(x), Some(y)) if x == y)
}

pub type ItemId = u64;

/// `PNS::SOLID` -- a footprint pad (or, in KiCad, a graphic keepout; not
/// modeled here). Immovable (`m_movable = false` in KiCad's constructor):
/// the router never relocates a pad.
#[derive(Debug, Clone)]
pub struct Solid {
    pub net: Net,
    pub layers: LayerRange,
    pub pos: Point,
    pub shape: Shape,
    /// `"U1.3"` -- which footprint pad this came from, for mapping a
    /// collision/obstacle back to something the UI can name.
    pub source: String,
}

/// `PNS::SEGMENT` -- one straight copper segment of a track. A multi-point
/// `Track` in our IR becomes one `Segment` per `pts` window, exactly how
/// `eda_drc::board::build` already flattens tracks for DRC.
#[derive(Debug, Clone)]
pub struct Segment {
    pub net: Net,
    pub layer: i32,
    pub a: Point,
    pub b: Point,
    pub width: Um,
    /// The committed `Track::id` this segment belongs to, and its index
    /// within that track's `pts` -- `None` for a segment that only exists
    /// in an uncommitted preview line. Used to turn a NODE-level edit (the
    /// track got shoved) back into `DeleteTrack`/`AddTrack` verbs.
    pub source_track: Option<(String, usize)>,
    pub locked: bool,
}

impl Segment {
    pub fn seg(&self) -> eda_drc::kimath::Seg {
        eda_drc::kimath::Seg::new(self.a, self.b)
    }

    pub fn length(&self) -> f64 {
        let (dx, dy) = ((self.b.x - self.a.x) as f64, (self.b.y - self.a.y) as f64);
        (dx * dx + dy * dy).sqrt()
    }
}

/// `PNS::VIA`. This port only models round, single-diameter-per-layer
/// through vias (`STACK_MODE::NORMAL`, `VIATYPE::THROUGH`) -- the only kind
/// `eda_model::ir::Via` can express.
#[derive(Debug, Clone)]
pub struct Via {
    pub net: Net,
    pub layers: LayerRange,
    pub pos: Point,
    pub diameter: Um,
    pub drill: Um,
    pub source_via: Option<String>,
    pub locked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Solid,
    Segment,
    Via,
}

#[derive(Debug, Clone)]
pub enum Item {
    Solid(Solid),
    Segment(Segment),
    Via(Via),
}

impl Item {
    pub fn kind(&self) -> Kind {
        match self {
            Item::Solid(_) => Kind::Solid,
            Item::Segment(_) => Kind::Segment,
            Item::Via(_) => Kind::Via,
        }
    }

    pub fn net(&self) -> &Net {
        match self {
            Item::Solid(s) => &s.net,
            Item::Segment(s) => &s.net,
            Item::Via(v) => &v.net,
        }
    }

    pub fn layers(&self) -> LayerRange {
        match self {
            Item::Solid(s) => s.layers,
            Item::Segment(s) => LayerRange::single(s.layer),
            Item::Via(v) => v.layers,
        }
    }

    pub fn is_movable(&self) -> bool {
        match self {
            Item::Solid(_) => false,
            Item::Segment(s) => !s.locked,
            Item::Via(v) => !v.locked,
        }
    }

    /// `ITEM::Shape(layer)` -- the item's exact geometry, used for the real
    /// (non-hull) collision test. A via's circle is the same on every layer
    /// it spans (this port has no per-layer padstack), so `layer` is only
    /// consulted for a `Solid`'s compound shape in a fuller port; here it is
    /// accepted for interface symmetry with KiCad but every kind currently
    /// returns one shape regardless.
    pub fn shape(&self, _layer: i32) -> Shape {
        match self {
            Item::Solid(s) => s.shape.clone(),
            Item::Segment(s) => Shape::Stadium { a: s.a, b: s.b, r: s.width / 2 },
            Item::Via(v) => Shape::Circle { c: v.pos, r: v.diameter / 2 },
        }
    }

    /// `ITEM::Hull(clearance, walkaroundThickness, layer)` -- see
    /// `crate::hull` for the construction. Used only by WALKAROUND/
    /// OPTIMIZER (path shaping), never by the exact collision test.
    pub fn hull(&self, clearance: Um, walkaround_width: Um, layer: i32) -> Vec<Point> {
        hull::hull_of(&self.shape(layer), clearance, walkaround_width)
    }

    /// `ITEM::Anchor(n)`/`AnchorCount()` -- connection points a `JOINT` can
    /// form at. A pad/via has one (its centre); a segment has two (its
    /// ends).
    pub fn anchors(&self) -> Vec<Point> {
        match self {
            Item::Solid(s) => vec![s.pos],
            Item::Segment(s) => vec![s.a, s.b],
            Item::Via(v) => vec![v.pos],
        }
    }

    pub fn width(&self) -> Um {
        match self {
            Item::Solid(_) => 0,
            Item::Segment(s) => s.width,
            Item::Via(v) => v.diameter,
        }
    }
}
