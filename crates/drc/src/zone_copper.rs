//! The copper of the board's zones, as the zones' fills hold it: one set of polygons per zone and layer, in nanometres.
//!
//! The fill is KiCad's (`kicad-cli pcb drc --refill-zones --save-board`, kept beside `design.json` by `eda_kicad::zone_fills`): this crate does not make one. What
//! reads zone copper (live connectivity through the pours, the studio's canvas and 3D view) is handed the fills as they were last made. A zone with no fill yet, or
//! one that was edited since it was filled, has none here -- no copper in that zone yet, as in KiCad -- and the studio draws it as an outline.

use eda_shape_poly_set::ShapePolySet;
use std::collections::HashMap;

/// Keyed by `(Zone::id, layer)`: a zone on several layers (`Zone::layers`) has a fill on each.
#[derive(Debug, Clone, Default)]
pub struct ZoneCopper {
    pub zones: HashMap<(String, String), ShapePolySet>,
}

impl ZoneCopper {
    /// The fill of `zone_id` on `layer` (`ZONE::GetFilledPolysList( layer )`); `None` when the zone has none there.
    pub fn get(&self, zone_id: &str, layer: &str) -> Option<&ShapePolySet> {
        self.zones.get(&(zone_id.to_string(), layer.to_string()))
    }

    /// The fills of `zone_id`, one per layer it has one on, in no particular order.
    pub fn layers_of<'a>(&'a self, zone_id: &'a str) -> impl Iterator<Item = (&'a str, &'a ShapePolySet)> + 'a {
        self.zones.iter().filter(move |((id, _), _)| id == zone_id).map(|((_, layer), fill)| (layer.as_str(), fill))
    }

    pub fn is_empty(&self) -> bool {
        self.zones.is_empty()
    }
}
