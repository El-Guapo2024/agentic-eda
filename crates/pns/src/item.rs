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
//! An arc track (`PCB_ARC`) is an [`Arc`] (`ARC_T`, `pns_arc.h`): one item with the three points of the arc, its hull
//! ([`crate::arcs::arc_hull`]) and its collisions, linked at its two ends like a segment. `DIFF_PAIR_T`/`HOLE_T` are out
//! of scope (a pair is a struct of two lines here; a pad/via's drilled hole is not separately collision-tested -- see the
//! crate's `PARITY.md` for the full fidelity-gap list).

use crate::arcs::{self, ArcPts};
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

/// `PNS::SOLID` -- a footprint pad, or a segment of the board outline (a
/// graphic on Edge.Cuts, see [`Solid::edge`]). Immovable (`m_movable = false`
/// in KiCad's constructor): the router never relocates a pad or the edge.
#[derive(Debug, Clone)]
pub struct Solid {
    pub net: Net,
    pub layers: LayerRange,
    pub pos: Point,
    pub shape: Shape,
    /// `"U1.3"` -- which footprint pad this came from, for mapping a
    /// collision/obstacle back to something the UI can name. `"edge:3"`
    /// for the outline's fourth segment.
    pub source: String,
    /// `isEdge( item )` (`pns_kicad_iface.cpp`): a graphic on Edge.Cuts,
    /// which `PNS_KICAD_IFACE_BASE::syncGraphicalItem` adds to the world as a
    /// `SOLID` on every copper layer, not routable, with no net and a
    /// zero-width shape. It is not copper (`isCopper`), so the only clearance
    /// it asks for is the board's copper-to-edge clearance
    /// (`CT_EDGE_CLEARANCE`, [`crate::node::Node::clearance_to`]), whatever
    /// net class the other item is in. It has no anchor: nothing joins to it,
    /// nothing starts a route on it.
    pub edge: bool,
}

impl Solid {
    /// The `source` prefix of a board text on copper ([`crate::from_ir`]'s `syncTextItem`).
    pub const TEXT_SOURCE: &'static str = "text:";

    /// A board text on copper (`syncTextItem`): an obstacle with the ordinary copper clearance, not routable.
    pub fn is_text(&self) -> bool {
        self.source.starts_with(Self::TEXT_SOURCE)
    }

    /// `ITEM::IsRoutable()`: a pad is; the board outline (`syncGraphicalItem`) and a copper text (`syncTextItem`: `SetRoutable( false )`) are
    /// not -- nothing joins to them, nothing starts or ends a route on them, nothing picks them.
    pub fn routable(&self) -> bool {
        !self.edge && !self.is_text()
    }
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

/// `PNS::ARC` -- one arc of copper, a `PCB_ARC` of the board: the track from `arc.start` through `arc.mid` to `arc.end`. It links at
/// its two ends exactly as a [`Segment`] does (`ARC::Anchor`), a line assembled through it holds its polyline
/// ([`crate::line::Line::arc_runs`]), and it is hulled by `ArcHull` ([`crate::arcs::arc_hull`]).
#[derive(Debug, Clone)]
pub struct Arc {
    pub net: Net,
    pub layer: i32,
    pub width: Um,
    pub arc: ArcPts,
    /// The committed `Track::id` of the IR arc track this came from -- `None` for an arc that only exists in an uncommitted preview line.
    pub source_track: Option<String>,
    pub locked: bool,
    poly: Rc<Vec<Point>>,
}

impl Arc {
    pub fn new(net: Net, layer: i32, width: Um, arc: ArcPts, source_track: Option<String>, locked: bool) -> Arc {
        let poly = Rc::new(arc.polyline());
        Arc { net, layer, width, arc, source_track, locked, poly }
    }

    /// The arc's points as a line holds them (`SHAPE_LINE_CHAIN::Append( SHAPE_ARC )`), from `start` to `end`.
    pub fn polyline(&self) -> &[Point] {
        &self.poly
    }

    pub fn start(&self) -> Point {
        self.arc.start
    }

    pub fn end(&self) -> Point {
        self.arc.end
    }

    /// The end that is not `p`.
    pub fn other_end(&self, p: Point) -> Point {
        if self.arc.start == p {
            self.arc.end
        } else {
            self.arc.start
        }
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
    /// `ITEM::m_isVirtual` (`VVIA`, `NODE::FixupVirtualVias`): a via the router puts where tracks meet with no pad or via -- three or more segments,
    /// a change of width, the ends of a locked segment -- so that the meeting point is a joint with a via in it. It has no hole (`HasHole()` false),
    /// collides with nothing (`NODE::Collide`, `QueryColliding`: "virtual items cannot collide"), is not drawn and not committed; what it is for is
    /// the dragger, which drags it as a via when a segment is taken hold of at such an end (`DRAGGER::checkVirtualVia`), and the joints, which end a line
    /// there.
    pub is_virtual: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Solid,
    Segment,
    Arc,
    Via,
}

#[derive(Debug, Clone)]
pub enum Item {
    Solid(Solid),
    Segment(Segment),
    Arc(Arc),
    Via(Via),
}

impl Item {
    /// `ITEM::IsVirtual`: a virtual via ([`Via::is_virtual`]).
    pub fn is_virtual(&self) -> bool {
        matches!(self, Item::Via(v) if v.is_virtual)
    }

    pub fn kind(&self) -> Kind {
        match self {
            Item::Solid(_) => Kind::Solid,
            Item::Segment(_) => Kind::Segment,
            Item::Arc(_) => Kind::Arc,
            Item::Via(_) => Kind::Via,
        }
    }

    pub fn net(&self) -> &Net {
        match self {
            Item::Solid(s) => &s.net,
            Item::Segment(s) => &s.net,
            Item::Arc(a) => &a.net,
            Item::Via(v) => &v.net,
        }
    }

    pub fn layers(&self) -> LayerRange {
        match self {
            Item::Solid(s) => s.layers,
            Item::Segment(s) => LayerRange::single(s.layer),
            Item::Arc(a) => LayerRange::single(a.layer),
            Item::Via(v) => v.layers,
        }
    }

    pub fn is_movable(&self) -> bool {
        match self {
            Item::Solid(_) => false,
            Item::Segment(s) => !s.locked,
            Item::Arc(a) => !a.locked,
            Item::Via(v) => !v.locked,
        }
    }

    /// `ITEM::IsLocked` (`MK_LOCKED`): a locked track, arc or via is neither dragged nor shoved. A pad never moves, locked or not.
    pub fn is_locked(&self) -> bool {
        match self {
            Item::Solid(_) => false,
            Item::Segment(s) => s.locked,
            Item::Arc(a) => a.locked,
            Item::Via(v) => v.locked,
        }
    }

    /// A segment or an arc: `ITEM::SEGMENT_T | ITEM::ARC_T`, the two things a track is made of (`LINKED_ITEM`s a line is assembled from).
    pub fn is_track(&self) -> bool {
        matches!(self, Item::Segment(_) | Item::Arc(_))
    }

    /// The IR track a segment or an arc came from.
    pub fn source_track_id(&self) -> Option<&str> {
        match self {
            Item::Segment(s) => s.source_track.as_ref().map(|(t, _)| t.as_str()),
            Item::Arc(a) => a.source_track.as_deref(),
            _ => None,
        }
    }

    /// The two ends a track item links at (`LINKED_ITEM::Anchor( 0 )` / `( 1 )`); `None` for a pad or a via.
    pub fn track_ends(&self) -> Option<(Point, Point)> {
        match self {
            Item::Segment(s) => Some((s.a, s.b)),
            Item::Arc(a) => Some((a.arc.start, a.arc.end)),
            _ => None,
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
            Item::Arc(a) => arcs::arc_shape(&a.poly, a.width),
            Item::Via(v) => Shape::Circle { c: v.pos, r: v.diameter / 2 },
        }
    }

    /// `ITEM::Hull(clearance, walkaroundThickness, layer)` -- see
    /// `crate::hull` for the construction. Used only by WALKAROUND/
    /// OPTIMIZER (path shaping), never by the exact collision test.
    pub fn hull(&self, clearance: Um, walkaround_width: Um, layer: i32) -> Vec<Point> {
        let clearance = clearance + hull::HULL_ROUNDING_GUARD;
        match self {
            // `SEGMENT::Hull` -> `SegmentHull`, `VIA::Hull`, `SOLID::Hull`.
            Item::Segment(s) => hull::segment_hull(s.a, s.b, s.width, clearance, walkaround_width),
            Item::Arc(a) => arcs::arc_hull(&a.arc, a.width, clearance, walkaround_width),
            Item::Via(v) => hull::make_clockwise(hull::via_hull(v.pos, v.diameter, clearance, walkaround_width)),
            Item::Solid(_) => hull::primitive_hull(&self.shape(layer), clearance, walkaround_width),
        }
    }

    /// `ITEM::Anchor(n)`/`AnchorCount()` -- connection points a `JOINT` can
    /// form at. A pad/via has one (its centre); a segment has two (its
    /// ends).
    pub fn anchors(&self) -> Vec<Point> {
        match self {
            Item::Solid(s) if !s.routable() => Vec::new(),
            Item::Solid(s) => vec![s.pos],
            Item::Segment(s) => vec![s.a, s.b],
            Item::Arc(a) => vec![a.arc.start, a.arc.end],
            Item::Via(v) => vec![v.pos],
        }
    }

    pub fn width(&self) -> Um {
        match self {
            Item::Solid(_) => 0,
            Item::Segment(s) => s.width,
            Item::Arc(a) => a.width,
            Item::Via(v) => v.diameter,
        }
    }
}
