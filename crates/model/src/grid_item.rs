//! KiCad 10's grid items (file version 20260728, "Grid items"; `PCB_GRID_ITEM`, `pcbnew/pcb_grid_item.h` of KiCad 25ec2335 -- the 8303b2ad source the rest of the port reads
//! predates them): a sub-grid placed on the board, cartesian or polar, that the cursor, the router and the placement tools snap to where it covers the board instead of the
//! editor's own grid. It has no copper and no layer (`GetLayerSet` is every layer; the file writes none).
//!
//! The file has one as `(grid_item xy (at x y) (extent hx hy) (spacing dx dy) [(angle a)] (priority n) [(tick_interval n)] (affects (cursor yes) (routing yes) (placement yes))
//! [(locked yes)] (uuid ..))` -- for a polar grid `(extent r phi) (spacing dr dphi)`, the second numbers angles in degrees (`PCB_IO_KICAD_SEXPR::format( PCB_GRID_ITEM )`). The
//! board keeps them in [`crate::pcb_items::BoardItems::grid_items`].

use serde::{Deserialize, Serialize};

use crate::ir::{Point, Nm};

/// `PCB_GRID_TYPE`: the file's `xy` and `polar`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridKind {
    #[default]
    Cartesian,
    Polar,
}

impl GridKind {
    pub fn token(self) -> &'static str {
        match self {
            GridKind::Cartesian => "xy",
            GridKind::Polar => "polar",
        }
    }
}

fn d_true() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}
fn is_zero_u32(n: &u32) -> bool {
    *n == 0
}
fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

/// `PCB_GRID_AFFECTS`: what the grid takes over where it covers the board -- the cursor's snap, the router's frame, the placement tools' snap. All on by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridAffects {
    #[serde(default = "d_true", skip_serializing_if = "is_true")]
    pub cursor: bool,
    #[serde(default = "d_true", skip_serializing_if = "is_true")]
    pub routing: bool,
    #[serde(default = "d_true", skip_serializing_if = "is_true")]
    pub placement: bool,
}

impl Default for GridAffects {
    fn default() -> Self {
        GridAffects { cursor: true, routing: true, placement: true }
    }
}

impl GridAffects {
    pub fn is_default(&self) -> bool {
        *self == GridAffects::default()
    }
}

/// One `PCB_GRID_ITEM`.
///
/// `extent` is the half-extent, the distance from the centre to an edge (`m_extent`; for a polar grid `x` is the radius and `y` is unused), `spacing` the pitch (for a polar grid
/// `x` is the radial pitch). The angles are degrees: `angle` is the grid's orientation in KiCad's screen sense (`EDA_ORIENTATION`, in [0, 360)), `phi_extent_deg` and
/// `phi_spacing_deg` the polar grid's sweep and angular pitch (at least 0.001 degree each).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridItem {
    /// `grd_xxxxxxxxxxxx`, assigned by the board.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default)]
    pub kind: GridKind,
    pub at: Point,
    pub extent: Point,
    pub spacing: Point,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub angle: f64,
    #[serde(default = "d_phi_extent")]
    pub phi_extent_deg: f64,
    #[serde(default = "d_phi_spacing")]
    pub phi_spacing_deg: f64,
    /// `m_priority`: higher wins where grids overlap; at least 1 (0 is the editor's own grid).
    #[serde(default = "d_priority")]
    pub priority: u32,
    /// `m_tickInterval`: every Nth line is a major one; 0 for none.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub tick_interval: u32,
    #[serde(default, skip_serializing_if = "GridAffects::is_default")]
    pub affects: GridAffects,
    /// KiCad 10's `(custom_property "key" "value")` forms of the item (file version 20260831), kept as the file has them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_properties: Vec<(String, String)>,
}

fn d_phi_extent() -> f64 {
    360.0
}
fn d_phi_spacing() -> f64 {
    10.0
}
fn d_priority() -> u32 {
    1
}

/// `EDA_ANGLE::Normalize`: into [0, 360).
pub fn normalize_deg(a: f64) -> f64 {
    let mut v = a;
    while v < -0.0 {
        v += 360.0;
    }
    while v >= 360.0 {
        v -= 360.0;
    }
    v
}

impl GridItem {
    /// `PCB_GRID_ITEM::PCB_GRID_ITEM`: a cartesian grid 10 mm square (5 mm each way) with a 1 mm pitch, a full turn and 10 degrees for a polar one, priority 1, affecting everything.
    pub fn new(at: Point) -> GridItem {
        GridItem {
            id: String::new(),
            kind: GridKind::Cartesian,
            at,
            extent: Point { x: Nm::new(5_000_000), y: Nm::new(5_000_000) },
            spacing: Point { x: Nm::new(1_000_000), y: Nm::new(1_000_000) },
            angle: 0.0,
            phi_extent_deg: 360.0,
            phi_spacing_deg: 10.0,
            priority: 1,
            tick_interval: 0,
            affects: GridAffects::default(),
            custom_properties: Vec::new(),
        }
    }

    /// What the item's setters make of the values (`SetExtent`: magnitudes; `SetSpacing`: at least 1 nm; `SetPhi*Degrees`: magnitudes of at least 0.001 degree;
    /// `SetAssignedPriority`: 0 is 1; `SetOrientation`: normalised).
    pub fn normalized(mut self) -> GridItem {
        self.extent = Point { x: self.extent.x.abs(), y: self.extent.y.abs() };
        self.spacing = Point { x: self.spacing.x.max(Nm::new(1)), y: self.spacing.y.max(Nm::new(1)) };
        self.phi_extent_deg = self.phi_extent_deg.abs().max(0.001);
        self.phi_spacing_deg = self.phi_spacing_deg.abs().max(0.001);
        if self.priority == 0 {
            self.priority = 1;
        }
        self.angle = normalize_deg(self.angle);
        self
    }

    pub fn id_seed(&self) -> String {
        format!("grid:{}:{}:{}:{}", self.kind.token(), self.at.x, self.at.y, self.extent.x)
    }

    /// `PCB_GRID_ITEM::buildOutlineWorld`'s box (`GetBoundingBox`): the rectangle or the wedge turned by the orientation, `[x0, y0, x1, y1]`.
    pub fn bounds(&self) -> [Nm; 4] {
        let pts = self.outline();
        let mut b = [Nm::MAX, Nm::MAX, Nm::MIN, Nm::MIN];
        for p in pts {
            b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
        }
        b
    }

    /// `PCB_GRID_ITEM::buildOutlineWorld`: the four corners of a cartesian grid, the arc of a polar one in 32 segments (with the apex first for a wedge short of a turn), each
    /// point turned by the orientation (`RotatePoint`) about the centre.
    pub fn outline(&self) -> Vec<Point> {
        let (s, c) = self.angle.to_radians().sin_cos();
        // `RotatePoint( local, angle )`: x' = x cos + y sin, y' = y cos - x sin.
        let world = |x: f64, y: f64| Point { x: self.at.x + Nm::kiround(x * c + y * s), y: self.at.y + Nm::kiround(y * c - x * s) };
        match self.kind {
            GridKind::Cartesian => {
                let (ex, ey) = (self.extent.x.as_f64(), self.extent.y.as_f64());
                vec![world(-ex, -ey), world(ex, -ey), world(ex, ey), world(-ex, ey)]
            }
            GridKind::Polar => {
                let r = self.extent.x.as_f64();
                let phi_max = self.phi_extent_deg.to_radians();
                let mut out = Vec::new();
                if phi_max + 1e-9 < 2.0 * std::f64::consts::PI {
                    out.push(world(0.0, 0.0));
                }
                for i in 0..=32 {
                    let phi = phi_max * i as f64 / 32.0;
                    out.push(world((r * phi.cos()).round(), (r * phi.sin()).round()));
                }
                out
            }
        }
    }

    /// `PCB_GRID_ITEM::Rotate`: the centre turned about `centre` by `deg` (positive counter-clockwise on the screen, `RotatePoint`), the orientation by the same.
    pub fn rotate(&mut self, centre: Point, deg: f64) {
        let (s, c) = deg.to_radians().sin_cos();
        let (dx, dy) = ((self.at.x - centre.x).as_f64(), (self.at.y - centre.y).as_f64());
        self.at = Point { x: centre.x + Nm::kiround(dx * c + dy * s), y: centre.y + Nm::kiround(dy * c - dx * s) };
        self.angle = normalize_deg(self.angle + deg);
    }

    /// `PCB_GRID_ITEM::Flip`: the centre mirrored about `centre`'s x (left-right) or y (top-bottom), the orientation 180 minus it or its negative.
    pub fn flip(&mut self, centre: Point, left_right: bool) {
        if left_right {
            self.at.x = centre.x - (self.at.x - centre.x);
            self.angle = normalize_deg(180.0 - self.angle);
        } else {
            self.at.y = centre.y - (self.at.y - centre.y);
            self.angle = normalize_deg(-self.angle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setters_keep_what_kicad_keeps() {
        let g = GridItem { extent: Point { x: Nm::new(-3_000_000), y: Nm::new(2_000_000) }, spacing: Point { x: Nm::ZERO, y: Nm::new(-5000) }, phi_extent_deg: -0.0, phi_spacing_deg: 15.0, priority: 0, angle: -90.0, ..GridItem::new(Point { x: Nm::ZERO, y: Nm::ZERO }) }.normalized();
        assert_eq!((g.extent, g.spacing, g.priority, g.angle), (Point { x: Nm::new(3_000_000), y: Nm::new(2_000_000) }, Point { x: Nm::new(1), y: Nm::new(1) }, 1, 270.0));
        assert_eq!((g.phi_extent_deg, g.phi_spacing_deg), (0.001, 15.0));
    }

    #[test]
    fn a_turn_and_a_flip_move_the_centre_and_the_orientation_as_pcb_grid_item_does() {
        let mut g = GridItem::new(Point { x: Nm::new(10_000_000), y: Nm::ZERO });
        g.rotate(Point { x: Nm::ZERO, y: Nm::ZERO }, 90.0);
        assert_eq!((g.at, g.angle), (Point { x: Nm::ZERO, y: Nm::new(-10_000_000) }, 90.0), "counter-clockwise on the screen is up");
        g.flip(Point { x: Nm::ZERO, y: Nm::ZERO }, true);
        assert_eq!(g.angle, 90.0, "180 - 90");
        g.flip(Point { x: Nm::ZERO, y: Nm::ZERO }, false);
        assert_eq!((g.at, g.angle), (Point { x: Nm::ZERO, y: Nm::new(10_000_000) }, 270.0));
    }

    #[test]
    fn the_outline_is_the_box_or_the_wedge() {
        let g = GridItem::new(Point { x: Nm::ZERO, y: Nm::ZERO });
        assert_eq!(g.bounds(), [Nm::new(-5_000_000), Nm::new(-5_000_000), Nm::new(5_000_000), Nm::new(5_000_000)]);
        let wedge = GridItem { kind: GridKind::Polar, extent: Point { x: Nm::new(4_000_000), y: Nm::ZERO }, phi_extent_deg: 90.0, ..GridItem::new(Point { x: Nm::ZERO, y: Nm::ZERO }) };
        assert_eq!(wedge.outline().len(), 34, "the apex and 33 arc points");
        assert_eq!(wedge.bounds(), [Nm::ZERO, Nm::ZERO, Nm::new(4_000_000), Nm::new(4_000_000)]);
    }
}
