//! The zone fills: KiCad's, made by kicad-cli, and kept beside `design.json` as derived data.
//!
//! There is no zone filler in this workspace. A fill is what `kicad-cli pcb drc --refill-zones --save-board` stores in the `(filled_polygon ..)` of each zone of a copy
//! of the derived board (`eda_kicad_engine::refill_zones`: Fill All Zones, B, and the refill before a DRC), or what a `.kicad_pcb` carried when it was imported. The rings
//! are read back here and kept in `<board>/.kicad/zone_fills.json`, never in `design.json`, keyed by zone id with the revision of the design they were made at.
//!
//! Between fills the board shows the last fill, as KiCad does with auto-refill off: an edit to a track or a pad does not take the copper of a pour away. A zone *edited*
//! since its fill (its outline, its layers, its net, a setting: [`zone_signature`]) has no fill any more, as in KiCad where editing a zone empties it, and is drawn as an
//! outline; so is a zone that was never filled. Everything that reads zone copper (the canvas, the 3D view, live connectivity, the derived `.kicad_pcb`) reads it through
//! [`KicadFills::copper`] or [`KicadFills::current`], so a zone with no current fill is *no copper in that zone yet* everywhere at once.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use eda_clipper2::Point64;
use eda_drc::zone_copper::ZoneCopper;
use eda_model::ir::{Design, Zone};
use eda_shape_poly_set::ShapePolySet;
use serde::{Deserialize, Serialize};

/// One `(filled_polygon ..)` as KiCad stored it: the ring of an island with its holes bridged to its outline (`SHAPE_POLY_SET::Fracture`), in nanometres, and
/// whether KiCad marked it `(island yes)` (copper with no connection: Gerber plots give it no net).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KicadRing {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub island: bool,
    pub pts: Vec<[i64; 2]>,
}

/// The fill of one zone: the rings on each of its layers, and the fingerprint of the zone they were made for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ZoneEntry {
    /// [`zone_signature`] of the zone when it was filled.
    pub sig: String,
    /// Layer -> the rings KiCad stored for the zone on it. A layer that is not here has no fill.
    #[serde(default)]
    pub layers: BTreeMap<String, Vec<KicadRing>>,
}

/// What kicad-cli filled, for one design revision.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KicadFills {
    pub schema: u32,
    /// "kicad-cli 10.99.0"; "the .kicad_pcb that was imported" for the fills a file brought.
    pub engine: String,
    /// The design revision (`/api/version`) the run started from.
    #[serde(default)]
    pub revision: String,
    /// How long kicad-cli took, in milliseconds (whole numbers: a float does not survive the file exactly).
    #[serde(default)]
    pub millis: u64,
    /// Zone id -> its fill.
    #[serde(default)]
    pub zones: BTreeMap<String, ZoneEntry>,
}

impl KicadFills {
    pub const SCHEMA: u32 = 3;

    pub fn new(engine: String, revision: String, millis: u64, zones: BTreeMap<String, ZoneEntry>) -> Self {
        KicadFills { schema: Self::SCHEMA, engine, revision, millis, zones }
    }

    /// How long kicad-cli took, in seconds.
    pub fn seconds(&self) -> f64 {
        self.millis as f64 / 1000.0
    }

    /// The fill of `zone`, if it has one that is current: the zone has not been edited since it was filled.
    pub fn current(&self, zone: &Zone) -> Option<&ZoneEntry> {
        self.zones.get(&zone.id).filter(|e| fill_is_of(e, zone))
    }

    /// The rings of `zone` on `layer`: none for a zone with no current fill, and none for a layer KiCad left empty.
    pub fn rings(&self, zone: &Zone, layer: &str) -> &[KicadRing] {
        self.current(zone).and_then(|e| e.layers.get(layer)).map_or(&[], |r| r.as_slice())
    }

    /// The zones of `design` that have a fill that is current, and the zones that have one that is out of date (edited since).
    pub fn tally(&self, design: &Design) -> (usize, usize) {
        let zones = design.routing.as_ref().map_or(&[][..], |r| r.zones.as_slice());
        let fillable = zones.iter().filter(|z| fillable(z));
        let (mut current, mut stale) = (0, 0);
        for z in fillable {
            match self.zones.get(&z.id) {
                Some(e) if fill_is_of(e, z) => current += 1,
                Some(_) => stale += 1,
                None => {}
            }
        }
        (current, stale)
    }

    /// The copper of the zones of `design` as their current fills hold it, in nanometres: what live connectivity, the canvas and the 3D view read. A zone with no
    /// current fill is not in it.
    pub fn copper(&self, design: &Design) -> ZoneCopper {
        let mut out = ZoneCopper::default();
        for z in design.routing.as_ref().map_or(&[][..], |r| r.zones.as_slice()) {
            let Some(entry) = self.current(z) else { continue };
            for (layer, rings) in &entry.layers {
                let mut fill = ShapePolySet::default();
                for ring in rings {
                    fill.add_outline(ring.pts.iter().map(|p| Point64::new(p[0], p[1])).collect());
                }
                out.zones.insert((z.id.clone(), layer.clone()), fill);
            }
        }
        out
    }

    /// The current fills of the zones of `design` in nanometres, as KiCad holds them (`ZONE::GetFill( layer )`), by `(zone id, layer)`: what a via stitching's placement
    /// reads (`eda_drc::via_stitch`).
    pub fn fills_nm(&self, design: &Design) -> eda_drc::via_stitch::ZoneFillsNm {
        let mut out = eda_drc::via_stitch::ZoneFillsNm::new();
        for z in design.routing.as_ref().map_or(&[][..], |r| r.zones.as_slice()) {
            let Some(entry) = self.current(z) else { continue };
            for (layer, rings) in &entry.layers {
                let mut fill = ShapePolySet::default();
                for ring in rings {
                    fill.add_outline(ring.pts.iter().map(|p| Point64::new(p[0], p[1])).collect());
                }
                out.insert((z.id.clone(), layer.clone()), fill);
            }
        }
        out
    }

    /// How many zone layers have a fill, and how many rings those fills are.
    pub fn counts(&self) -> (usize, usize) {
        let layers = self.zones.values().map(|e| e.layers.values().filter(|r| !r.is_empty()).count()).sum();
        (layers, self.zones.values().flat_map(|e| e.layers.values()).map(Vec::len).sum())
    }

    /// Takes the fills of `ids` (all of them when `None`) from `fresh`, the fill kicad-cli just made, and keeps the others as they were: Fill Selected Zones.
    pub fn take_from(&mut self, fresh: KicadFills, ids: Option<&[String]>) {
        match ids {
            None => *self = fresh,
            Some(ids) => {
                for id in ids {
                    match fresh.zones.get(id) {
                        Some(entry) => {
                            self.zones.insert(id.clone(), entry.clone());
                        }
                        None => {
                            self.zones.remove(id);
                        }
                    }
                }
                self.engine = fresh.engine;
                self.revision = fresh.revision;
                self.millis = fresh.millis;
            }
        }
    }

    /// Takes the fills of `ids` away: Unfill Selected Zones.
    pub fn remove(&mut self, ids: &[String]) {
        for id in ids {
            self.zones.remove(id);
        }
    }
}

/// Whether kicad-cli's Fill All Zones fills `zone`: a zone of the board with an outline that is not a rule area. `ZONE_FILLER_TOOL::FillAllZones` walks `board()->Zones()`, which
/// leaves out the zones that belong to a footprint, so one of those has no fill to wait for (a fill a file stored for it is still read).
pub fn fillable(zone: &Zone) -> bool {
    !zone.is_rule_area && zone.outline.len() >= 3 && zone.parent_footprint.is_none()
}

/// Whether `entry` is the fill of `zone` as it is now: its fingerprint is the zone's. A fill kept by a cache of version 2 (written when the IR was micrometres,
/// `docs/NANOMETRES.md`) has the fingerprint of the zone in micrometres: a zone read from a design of that time, whose every length is a whole micrometre, is
/// matched against it with its lengths taken back to micrometres, so a board brought over keeps its fills (and its DRC) until it is edited or refilled.
fn fill_is_of(entry: &ZoneEntry, zone: &Zone) -> bool {
    entry.sig == zone_signature(zone) || micrometre_zone(zone).is_some_and(|z| entry.sig == zone_signature(&z))
}

/// `zone` with its lengths in micrometres (its area in square micrometres), as a design of version 1 held it; `None` when a length is not a whole micrometre.
fn micrometre_zone(zone: &Zone) -> Option<Zone> {
    let um = |v: i64| (v % 1000 == 0).then_some(v / 1000);
    let mut z = zone.clone();
    z.outline = zone.outline.iter().map(|p| Some(eda_model::ir::Point { x: um(p.x)?, y: um(p.y)? })).collect::<Option<_>>()?;
    for v in [&mut z.clearance, &mut z.min_thickness, &mut z.thermal_gap, &mut z.thermal_spoke_width, &mut z.hatch_thickness, &mut z.hatch_gap, &mut z.corner_radius] {
        *v = um(*v)?;
    }
    z.min_island_area = (zone.min_island_area % 1_000_000 == 0).then_some(zone.min_island_area / 1_000_000)?;
    Some(z)
}

// ----------------------------------------------------------------------------------------------------- the zone's fingerprint

/// A fingerprint of everything about a zone that its fill is a fill *of*: the outline, the layers, the net and the settings (clearance, minimum width, connection,
/// thermal relief, island removal, hatch, smoothing). Editing any of them takes the fill away (KiCad unfills a zone that is edited); the name, the border style and where
/// the zone came from do not, and nor does the priority: the Zone Manager's OK changes the priorities and puts the zones' fills back (`DIALOG_ZONE_MANAGER::OnOk`), the
/// next fill takes them into account. The zone's own id is the key it is filed under.
pub fn zone_signature(z: &Zone) -> String {
    let text = format!(
        "{:?}|{}|{:?}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        z.outline,
        z.net,
        z.layer_set(),
        z.teardrop,
        z.clearance,
        z.min_thickness,
        z.thermal_gap,
        z.thermal_spoke_width,
        z.pad_connection,
        z.island_removal_mode,
        z.min_island_area,
        z.fill_mode,
        z.hatch_thickness,
        z.hatch_gap,
        z.hatch_orientation_mdeg,
        z.hatch_smoothing_level,
        (z.hatch_smoothing_value, z.hatch_hole_min_area, z.hatch_border_algorithm),
        z.smoothing,
        z.corner_radius,
        z.is_rule_area,
    );
    blake3::hash(text.as_bytes()).to_hex()[..16].to_string()
}

// ------------------------------------------------------------------------------------------------------------- the cache

/// Where the fills of the board in `board_dir` are kept: `<board>/.kicad/zone_fills.json`, a derived file like the rest of `.kicad/` (not under version control).
pub fn cache_path(board_dir: &Path) -> PathBuf {
    board_dir.join(".kicad").join("zone_fills.json")
}

/// The fills of the board in `board_dir`; `None` when it has none (or the file is not ours).
pub fn load(board_dir: &Path) -> Option<KicadFills> {
    let text = std::fs::read_to_string(cache_path(board_dir)).ok()?;
    // version 2 is the same file, its fingerprints taken over micrometres (`fill_is_of`)
    serde_json::from_str::<KicadFills>(&text).ok().filter(|k| k.schema == KicadFills::SCHEMA || k.schema == 2)
}

/// Keeps `fills` as the board's: written beside the final file and renamed over it, so a reader never sees half a file.
pub fn store(board_dir: &Path, fills: &KicadFills) -> Result<(), String> {
    let path = cache_path(board_dir);
    std::fs::create_dir_all(path.parent().unwrap_or(board_dir)).map_err(|e| format!("cannot create {}: {e}", board_dir.join(".kicad").display()))?;
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let text = serde_json::to_string(fills).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Forgets the board's fills: Unfill All Zones. The next B makes them again.
pub fn forget(board_dir: &Path) {
    let _ = std::fs::remove_file(cache_path(board_dir));
}

// --------------------------------------------------------------------------------------------- reading a refilled board

/// A `(filled_polygon ..)` of a zone, as the file has it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedFill {
    /// The `(uuid ..)` of the zone it is in.
    pub zone: String,
    pub layer: String,
    pub island: bool,
    pub pts: Vec<[i64; 2]>,
}

/// Every stored fill of every zone in a `.kicad_pcb` (the board's and the ones inside footprints), without building a tree of the file: a fill is a million points
/// on a big board. The points are in the board's frame: a file KiCad 10 saved (`(transform ..)` on its footprints, `fp_transform`) stores the fills of a footprint's own
/// zones in the footprint's frame, and they are mapped out of it here, as the importer maps the zones' outlines.
pub fn scan_zone_fills(text: &str) -> Vec<ScannedFill> {
    let mut out = Vec::new();
    let mut cur = Cursor { s: text.as_bytes(), text, i: 0 };
    // Walk the file's lists; a `(zone ..)` is read whole, a footprint is read for its zones, anything else is entered.
    while let Some(tag) = cur.next_list_tag() {
        match tag {
            "zone" => cur.read_zone(&mut out),
            "footprint" | "module" => cur.read_footprint(&mut out),
            _ => {}
        }
    }
    out
}

/// The fill of each zone of `design` as the scanned fills of a `.kicad_pcb` hold it, `ids` mapping the uuid a zone was written with in that file to its id. A zone
/// that is not in `ids` is not ours and is left out; the entry of a zone is made for the zone as it is in `design`.
pub fn entries_by_zone(design: &Design, scanned: Vec<ScannedFill>, ids: &HashMap<String, String>) -> BTreeMap<String, ZoneEntry> {
    let zones: HashMap<&str, &Zone> = design.routing.as_ref().map_or(&[][..], |r| r.zones.as_slice()).iter().map(|z| (z.id.as_str(), z)).collect();
    let mut out: BTreeMap<String, ZoneEntry> = BTreeMap::new();
    for f in scanned {
        let Some(id) = ids.get(&f.zone) else { continue };
        let Some(zone) = zones.get(id.as_str()) else { continue };
        out.entry(id.clone()).or_insert_with(|| ZoneEntry { sig: zone_signature(zone), layers: BTreeMap::new() }).layers.entry(f.layer).or_default().push(KicadRing { island: f.island, pts: f.pts });
    }
    out
}

/// The fills a `.kicad_pcb` carried, for the zones of the design imported from it: the zones are found by the uuid they had in the file (`Zone::uuid`). A zone that the
/// importer made of several outlines gets the fills of the file's zone on its first.
pub fn entries_of_imported_file(design: &Design, scanned: Vec<ScannedFill>) -> BTreeMap<String, ZoneEntry> {
    let mut by_uuid: HashMap<String, String> = HashMap::new();
    for z in design.routing.as_ref().map_or(&[][..], |r| r.zones.as_slice()) {
        if !z.uuid.is_empty() && !z.id.is_empty() {
            by_uuid.entry(z.uuid.clone()).or_insert_with(|| z.id.clone());
        }
    }
    entries_by_zone(design, scanned, &by_uuid)
}

struct Cursor<'a> {
    s: &'a [u8],
    text: &'a str,
    i: usize,
}

impl<'a> Cursor<'a> {
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    /// A quoted string (its text, escapes left as they are) or a bare atom, from the cursor.
    fn atom(&mut self) -> Option<&'a str> {
        self.skip_ws();
        let start = self.i;
        match self.s.get(self.i)? {
            b'"' => {
                self.i += 1;
                let from = self.i;
                while self.i < self.s.len() && self.s[self.i] != b'"' {
                    self.i += if self.s[self.i] == b'\\' { 2 } else { 1 };
                }
                let to = self.i.min(self.s.len());
                self.i = (self.i + 1).min(self.s.len());
                Some(&self.text[from..to])
            }
            b'(' | b')' => None,
            _ => {
                while self.i < self.s.len() && !self.s[self.i].is_ascii_whitespace() && self.s[self.i] != b'(' && self.s[self.i] != b')' {
                    self.i += 1;
                }
                Some(&self.text[start..self.i])
            }
        }
    }

    /// Moves to the next `(` outside a string, past it and its tag, and returns the tag. `None` at the end of the file.
    fn next_list_tag(&mut self) -> Option<&'a str> {
        while self.i < self.s.len() {
            match self.s[self.i] {
                b'"' => {
                    self.atom();
                }
                b'(' => {
                    self.i += 1;
                    return Some(self.atom().unwrap_or(""));
                }
                _ => self.i += 1,
            }
        }
        None
    }

    /// From inside a list, past its closing paren.
    fn skip_list(&mut self) {
        let mut depth = 1usize;
        while self.i < self.s.len() && depth > 0 {
            match self.s[self.i] {
                b'"' => {
                    self.atom();
                    continue;
                }
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            self.i += 1;
        }
    }

    /// Past a `(footprint` and up to its closing paren: the fills of its zones, each point taken out of the footprint's frame when the footprint has a `(transform ..)`
    /// (`fp_transform::apply_transform`; the transform precedes the zones in the file, as `PCB_IO_KICAD_SEXPR::format( const FOOTPRINT* )` writes it).
    fn read_footprint(&mut self, out: &mut Vec<ScannedFill>) {
        let mut transform: Option<([f64; 2], f64, [f64; 2])> = None;
        loop {
            self.skip_ws();
            match self.s.get(self.i) {
                None => return,
                Some(b')') => {
                    self.i += 1;
                    return;
                }
                Some(b'(') => {
                    self.i += 1;
                    match self.atom().unwrap_or("") {
                        "transform" => transform = Some(self.read_transform()),
                        "zone" => {
                            let first = out.len();
                            self.read_zone(out);
                            if let Some((origin, angle, scale)) = transform {
                                for fill in &mut out[first..] {
                                    for p in &mut fill.pts {
                                        let (x, y) = crate::fp_transform::apply_transform((p[0] as f64, p[1] as f64), (origin[0] * 1e6, origin[1] * 1e6), angle, (scale[0], scale[1]));
                                        *p = [x.round() as i64, y.round() as i64];
                                    }
                                }
                            }
                        }
                        _ => self.skip_list(),
                    }
                }
                Some(_) => {
                    self.atom();
                }
            }
        }
    }

    /// Past `(transform` and up to its closing paren: `(translate x y) (rotate a) (scale sx sy)` as the translation in millimetres, the angle in degrees and the scale.
    /// What the file leaves out is what `read_transform` of the importer takes it for: no translation, no rotation, scale 1 (a zero or negative scale is read as 1).
    fn read_transform(&mut self) -> ([f64; 2], f64, [f64; 2]) {
        let (mut origin, mut angle, mut scale) = ([0.0, 0.0], 0.0, [1.0, 1.0]);
        loop {
            self.skip_ws();
            match self.s.get(self.i) {
                None => break,
                Some(b')') => {
                    self.i += 1;
                    break;
                }
                Some(b'(') => {
                    self.i += 1;
                    let tag = self.atom().unwrap_or("");
                    let nums = self.numbers();
                    match (tag, nums.as_slice()) {
                        ("translate", [x, y, ..]) => origin = [*x, *y],
                        ("rotate", [a, ..]) => angle = *a,
                        ("scale", [sx, sy, ..]) => scale = [sx, sy].map(|v| if v.is_finite() && *v > 0.0 { *v } else { 1.0 }),
                        _ => {}
                    }
                }
                Some(_) => {
                    self.atom();
                }
            }
        }
        (origin, angle, scale)
    }

    /// The numbers of a list from the cursor to its closing paren (past it); anything that is not a number is left out.
    fn numbers(&mut self) -> Vec<f64> {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            match self.s.get(self.i) {
                None => return out,
                Some(b')') => {
                    self.i += 1;
                    return out;
                }
                Some(b'(') => {
                    self.i += 1;
                    self.skip_list();
                }
                Some(_) => {
                    if let Some(v) = self.atom().and_then(|a| a.parse::<f64>().ok()) {
                        out.push(v);
                    }
                }
            }
        }
    }

    /// Past a `(zone` and up to its closing paren: the zone's uuid and each `(filled_polygon ..)`.
    fn read_zone(&mut self, out: &mut Vec<ScannedFill>) {
        let mut uuid = String::new();
        let first = out.len();
        loop {
            self.skip_ws();
            match self.s.get(self.i) {
                None => return,
                Some(b')') => {
                    self.i += 1;
                    break;
                }
                Some(b'(') => {
                    self.i += 1;
                    match self.atom().unwrap_or("") {
                        // `(uuid ..)`, and `(tstamp ..)` before KiCad 8.
                        "uuid" | "tstamp" => {
                            if uuid.is_empty() {
                                uuid = self.atom().unwrap_or("").to_string();
                            }
                            self.skip_list();
                        }
                        "filled_polygon" => {
                            if let Some(f) = self.read_filled_polygon() {
                                out.push(f);
                            }
                        }
                        _ => self.skip_list(),
                    }
                }
                Some(_) => {
                    self.atom();
                }
            }
        }
        for f in &mut out[first..] {
            f.zone = uuid.clone();
        }
    }

    /// Past `(filled_polygon` and up to its closing paren: `(layer "F.Cu") [(island yes)] (pts (xy x y) ..)`.
    fn read_filled_polygon(&mut self) -> Option<ScannedFill> {
        let (mut layer, mut island, mut pts) = (String::new(), false, Vec::new());
        loop {
            self.skip_ws();
            match self.s.get(self.i) {
                None => return None,
                Some(b')') => {
                    self.i += 1;
                    break;
                }
                Some(b'(') => {
                    self.i += 1;
                    match self.atom().unwrap_or("") {
                        "layer" => {
                            layer = self.atom().unwrap_or("").to_string();
                            self.skip_list();
                        }
                        // `FormatBool( "island", true )` writes `(island yes)`; older files a bare `(island)`.
                        "island" => {
                            island = self.atom().is_none_or(|v| v != "no");
                            self.skip_list();
                        }
                        "pts" => pts = self.read_pts(),
                        _ => self.skip_list(),
                    }
                }
                Some(_) => {
                    self.atom();
                }
            }
        }
        (!layer.is_empty() && pts.len() >= 3).then_some(ScannedFill { zone: String::new(), layer, island, pts })
    }

    /// Past `(pts` and up to its closing paren: the `(xy x y)` points, in nanometres. An `(arc ..)` contributes its three points.
    fn read_pts(&mut self) -> Vec<[i64; 2]> {
        let mut pts = Vec::new();
        loop {
            self.skip_ws();
            match self.s.get(self.i) {
                None => return pts,
                Some(b')') => {
                    self.i += 1;
                    return pts;
                }
                Some(b'(') => {
                    self.i += 1;
                    match self.atom().unwrap_or("") {
                        "xy" => {
                            if let Some(p) = self.xy() {
                                pts.push(p);
                            }
                        }
                        "arc" => {
                            // `(arc (start x y) (mid x y) (end x y))`
                            loop {
                                self.skip_ws();
                                match self.s.get(self.i) {
                                    None => return pts,
                                    Some(b')') => {
                                        self.i += 1;
                                        break;
                                    }
                                    Some(b'(') => {
                                        self.i += 1;
                                        self.atom();
                                        if let Some(p) = self.xy() {
                                            pts.push(p);
                                        }
                                    }
                                    Some(_) => {
                                        self.atom();
                                    }
                                }
                            }
                        }
                        _ => self.skip_list(),
                    }
                }
                Some(_) => {
                    self.atom();
                }
            }
        }
    }

    /// `x y)` of an `(xy x y)`, past the closing paren.
    fn xy(&mut self) -> Option<[i64; 2]> {
        let x = self.atom().and_then(mm_to_nm);
        let y = self.atom().and_then(mm_to_nm);
        self.skip_list();
        Some([x?, y?])
    }
}

/// A coordinate in mm as the file writes it (`12.345678`, six decimals at most) to nanometres, exactly.
pub fn mm_to_nm(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let (neg, digits) = match b.first()? {
        b'-' => (true, &b[1..]),
        b'+' => (false, &b[1..]),
        _ => (false, b),
    };
    let dot = digits.iter().position(|&c| c == b'.');
    let (int_part, frac_part) = match dot {
        Some(d) => (&digits[..d], &digits[d + 1..]),
        None => (digits, &digits[digits.len()..]),
    };
    if int_part.iter().chain(frac_part).any(|c| !c.is_ascii_digit()) || frac_part.len() > 6 || (int_part.is_empty() && frac_part.is_empty()) {
        // An exponent, or more decimals than a nanometre: the slow way.
        return s.parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| (v * 1_000_000.0).round() as i64);
    }
    let int = int_part.iter().fold(0i64, |a, c| a * 10 + (c - b'0') as i64);
    let frac = frac_part.iter().fold(0i64, |a, c| a * 10 + (c - b'0') as i64) * 10i64.pow((6 - frac_part.len()) as u32);
    let v = int * 1_000_000 + frac;
    Some(if neg { -v } else { v })
}

/// Nanometres as KiCad writes a coordinate: millimetres with the decimals that are needed, none when it is whole.
pub fn fmt_nm(v: i64) -> String {
    let a = v.unsigned_abs();
    let (int, frac) = (a / 1_000_000, a % 1_000_000);
    let mut s = String::new();
    if v < 0 {
        s.push('-');
    }
    s.push_str(&int.to_string());
    if frac != 0 {
        s.push('.');
        s.push_str(format!("{frac:06}").trim_end_matches('0'));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::ir::{Point, PlacementSection, Provenance, RoutingSection};

    #[test]
    fn coordinates_go_between_millimetres_and_nanometres_exactly() {
        assert_eq!(mm_to_nm("12.345678"), Some(12_345_678));
        assert_eq!(mm_to_nm("-0.5"), Some(-500_000));
        assert_eq!(mm_to_nm("3"), Some(3_000_000));
        assert_eq!(mm_to_nm("-0.000001"), Some(-1));
        assert_eq!(mm_to_nm("1e-3"), Some(1_000), "an exponent still reads");
        assert_eq!(mm_to_nm("0.1234567"), Some(123_457), "more decimals than a nanometre are rounded");
        assert_eq!(mm_to_nm("abc"), None);
        for v in [0, 1, -1, 999_999, 1_000_000, -1_500_000, 12_345_678, -987_654_321] {
            assert_eq!(mm_to_nm(&fmt_nm(v)), Some(v), "{v}");
        }
        assert_eq!(fmt_nm(12_345_000), "12.345");
        assert_eq!(fmt_nm(-500_000), "-0.5");
        assert_eq!(fmt_nm(0), "0");
    }

    const BOARD: &str = r#"(kicad_pcb (version 20241229)
	(footprint "x" (layer "F.Cu")
		(zone (net 0) (net_name "") (layers "F.Cu") (uuid "fp-zone") (hatch edge 0.5) (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1)))
			(filled_polygon (layer "F.Cu") (pts (xy 0 0) (xy 1 0) (xy 1 1)))
		)
	)
	(gr_text "(zone with a quote \" and a (paren" (at 0 0) (layer "F.SilkS") (uuid "t"))
	(zone (net 1) (net_name "GND") (layers "F&B.Cu") (uuid "z1") (name "a(b")
		(polygon (pts (xy 0 0) (xy 10 0) (xy 10 10)))
		(filled_polygon (layer "F.Cu") (pts (xy 0.5 0.5) (xy 9.25 0.5) (xy 9.25 -9.75) (xy 0.5 9.5)))
		(filled_polygon (layer "B.Cu") (island yes) (pts (xy 1 1) (xy 2 1) (xy 2 2)))
		(filled_polygon (layer "B.Cu") (island) (pts (xy 3 3) (xy 4 3) (xy 4 4)))
		(filled_polygon (layer "B.Cu") (pts (xy 5 5)))
	)
	(zone (net 2) (layer "B.Cu") (tstamp "z2") (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))))
)"#;

    fn zone(id: &str, uuid: &str) -> Zone {
        Zone { id: id.into(), uuid: uuid.into(), net: "GND".into(), layer: "F.Cu".into(), outline: vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 10_000, y: 10_000 }], ..Default::default() }
    }

    fn design(zones: Vec<Zone>) -> Design {
        Design {
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "0".into(), intent_hash: "x".into(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![], footprints: vec![], modules: vec![] }),
            routing: Some(RoutingSection { tracks: vec![], vias: vec![], zones, track_width_presets: vec![], via_presets: vec![], teardrop_settings: Default::default() }),
            drawings: None,
            footprint_library: None,
            sheet_contents: None,
            bus_aliases: vec![],
            symbol_library: None,
        }
    }

    #[test]
    fn the_stored_fills_of_every_zone_are_scanned_and_nothing_in_a_string_is_taken_for_a_zone() {
        let got = scan_zone_fills(BOARD);
        assert_eq!(got.len(), 4, "the single point is no polygon, a zone with no fill has none, a string is no zone: {got:?}");
        assert_eq!((got[0].zone.as_str(), got[0].layer.as_str(), got[0].island), ("fp-zone", "F.Cu", false), "the zone inside a footprint");
        assert_eq!(got[1].zone, "z1");
        assert_eq!(got[1].pts, vec![[500_000, 500_000], [9_250_000, 500_000], [9_250_000, -9_750_000], [500_000, 9_500_000]]);
        assert_eq!((got[2].layer.as_str(), got[2].island), ("B.Cu", true), "(island yes)");
        assert!(got[3].island, "a bare (island) is one too");
    }

    #[test]
    fn the_fills_of_a_footprints_zones_come_out_of_the_footprints_frame_when_it_has_a_transform() {
        // A KiCad 10 file: the footprint is placed with `(transform ..)` and its zones are stored in its own frame (`fp_transform`). A quarter turn takes (x, y) to (y, -x).
        let fill = r#"(filled_polygon (layer "F.Cu") (pts (xy 1 0) (xy 2 0) (xy 2 1.5)))"#;
        let board = format!(
            r#"(kicad_pcb (version 20260901)
	(footprint "turned" (layer "F.Cu") (transform (translate 100 50) (rotate 90) (scale 1 1)) (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu"))
		(zone (uuid "turned-zone") (layers "F.Cu") (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))) {fill}))
	(footprint "plain" (layer "F.Cu") (at 7 7 0)
		(zone (uuid "plain-zone") (layers "F.Cu") (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))) {fill}))
	(footprint "scaled" (layer "F.Cu") (transform (translate 10 20) (scale 2 3))
		(zone (uuid "scaled-zone") (layers "F.Cu") (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))) {fill}))
	(footprint "slanted" (layer "F.Cu") (transform (translate 0 0) (rotate 30) (scale 1 1))
		(zone (uuid "slanted-zone") (layers "F.Cu") (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))) (filled_polygon (layer "F.Cu") (pts (xy 2 0) (xy 3 0) (xy 3 1)))))
	(zone (uuid "board-zone") (layers "F.Cu") (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))) {fill})
)"#
        );
        let got: BTreeMap<String, Vec<[i64; 2]>> = scan_zone_fills(&board).into_iter().map(|f| (f.zone, f.pts)).collect();
        assert_eq!(got.len(), 5, "{got:?}");
        assert_eq!(got["turned-zone"], vec![[100_000_000, 49_000_000], [100_000_000, 48_000_000], [101_500_000, 48_000_000]], "turned a quarter, then moved");
        assert_eq!(got["plain-zone"], vec![[1_000_000, 0], [2_000_000, 0], [2_000_000, 1_500_000]], "no transform: the points are the board's, as an older file has them");
        assert_eq!(got["scaled-zone"], vec![[12_000_000, 20_000_000], [14_000_000, 20_000_000], [14_000_000, 24_500_000]], "scaled, then moved");
        assert_eq!(got["board-zone"], vec![[1_000_000, 0], [2_000_000, 0], [2_000_000, 1_500_000]]);
        // Any other angle is the rotation `RotatePoint` makes: x' = y sin + x cos, y' = y cos - x sin.
        assert_eq!(got["slanted-zone"], vec![[1_732_051, -1_000_000], [2_598_076, -1_500_000], [3_098_076, -633_975]]);
    }

    #[test]
    fn a_zone_is_filled_by_id_and_its_fill_is_current_until_the_zone_itself_is_edited() {
        let d = design(vec![zone("zone-a", "z1")]);
        let entries = entries_of_imported_file(&d, scan_zone_fills(BOARD));
        assert_eq!(entries.keys().collect::<Vec<_>>(), ["zone-a"], "found by the uuid it had in the file; the others are not in the design");
        let fills = KicadFills::new("kicad-cli 10.99.0".into(), "r".into(), 1_500, entries);
        assert_eq!(fills.counts(), (2, 3));
        let z = &d.routing.as_ref().unwrap().zones[0];
        assert_eq!(fills.rings(z, "F.Cu").len(), 1);
        assert_eq!(fills.rings(z, "B.Cu").len(), 2);
        assert!(fills.rings(z, "In1.Cu").is_empty());
        assert_eq!(fills.tally(&d), (1, 0));

        // The copper in micrometres, a polygon per ring.
        let copper = fills.copper(&d);
        let f = copper.get("zone-a", "F.Cu").expect("a fill");
        assert_eq!(f.polys.len(), 1);
        assert_eq!(f.polys[0][0][1], Point64::new(9_250_000, 500_000));

        // An edit elsewhere on the board changes nothing about the zone; an edit to the zone itself empties it.
        let mut moved = z.clone();
        moved.outline[2] = Point { x: 11_000, y: 10_000 };
        assert!(fills.current(&moved).is_none() && fills.rings(&moved, "F.Cu").is_empty());
        let d2 = design(vec![moved.clone()]);
        assert_eq!(fills.tally(&d2), (0, 1), "one zone out of date");
        assert!(fills.copper(&d2).is_empty(), "no copper in that zone yet");
        let mut renamed = z.clone();
        renamed.name = "GND pour".into();
        renamed.uuid = "other".into();
        assert!(fills.current(&renamed).is_some(), "a name is no part of what a fill is a fill of");
        let mut thicker = z.clone();
        thicker.min_thickness += 10;
        assert!(fills.current(&thicker).is_none(), "a setting is");
        let mut other_net = z.clone();
        other_net.net = "VCC".into();
        assert!(fills.current(&other_net).is_none());
        let mut raised = z.clone();
        raised.priority += 1;
        assert!(fills.current(&raised).is_some(), "the Zone Manager's OK changes priorities and keeps the fills; the next fill takes them into account");
        // A zone drawn after the fill has none.
        let d3 = design(vec![z.clone(), zone("zone-b", "")]);
        assert_eq!(fills.tally(&d3), (1, 0));
    }

    #[test]
    fn fill_selected_zones_takes_only_those_from_the_fresh_run_and_unfill_selected_takes_them_away() {
        let ring = |n: i64| vec![KicadRing { island: false, pts: vec![[0, 0], [n, 0], [n, n]] }];
        let entry = |sig: &str, n: i64| ZoneEntry { sig: sig.into(), layers: BTreeMap::from([("F.Cu".to_string(), ring(n))]) };
        let mut old = KicadFills::new("old".into(), "r1".into(), 1, BTreeMap::from([("a".to_string(), entry("s", 1)), ("b".to_string(), entry("s", 2))]));
        let fresh = KicadFills::new("fresh".into(), "r2".into(), 2, BTreeMap::from([("a".to_string(), entry("s", 10)), ("b".to_string(), entry("s", 20)), ("c".to_string(), entry("s", 30))]));
        old.take_from(fresh.clone(), Some(&["b".to_string(), "c".to_string()]));
        assert_eq!(old.zones["a"].layers["F.Cu"][0].pts[1][0], 1, "a keeps the fill it had");
        assert_eq!(old.zones["b"].layers["F.Cu"][0].pts[1][0], 20);
        assert!(old.zones.contains_key("c"));
        assert_eq!((old.engine.as_str(), old.revision.as_str()), ("fresh", "r2"));
        old.remove(&["a".to_string()]);
        assert_eq!(old.zones.keys().collect::<Vec<_>>(), ["b", "c"]);
        old.take_from(fresh, None);
        assert_eq!(old.zones.len(), 3, "all of them: Fill All Zones");
    }

    #[test]
    fn the_cache_is_a_file_beside_the_design_that_is_written_whole_and_forgotten_on_request() {
        let dir = std::env::temp_dir().join(format!("eda-zone-fills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load(&dir).is_none());
        let zones = BTreeMap::from([("z".to_string(), ZoneEntry { sig: "abc".into(), layers: BTreeMap::from([("F.Cu".to_string(), vec![KicadRing { island: true, pts: vec![[0, 0], [1_000, 0], [1_000, 1_000]] }])]) })]);
        let fills = KicadFills::new("kicad-cli 10.99.0".into(), "rev".into(), 2_500, zones);
        store(&dir, &fills).unwrap();
        assert_eq!(load(&dir), Some(fills));
        assert!(cache_path(&dir).ends_with(".kicad/zone_fills.json"));
        std::fs::write(cache_path(&dir), "not json").unwrap();
        assert!(load(&dir).is_none(), "a file that is not ours is no fill");
        forget(&dir);
        assert!(!cache_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
