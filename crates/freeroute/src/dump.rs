//! Reading a board from the dumps `parity/MazeParity.java` writes: the
//! board as FreeRouting itself loaded it from a Specctra file -- layers,
//! rules, router settings and every item with its raw shapes. The parity
//! tests check the port against FreeRouting on boards read this way, and it
//! doubles as a way to route any board FreeRouting can read.
//!
//! One record per line, a keyword then its fields; see the Java for the
//! full list. Records this reader does not know are skipped, so the same
//! dump can carry the search trace the tests compare.

use std::collections::BTreeMap;

use crate::board::{AreaShape, PadShape};
use crate::geometry::{Circle, Direction, IntBox, IntOctagon, IntPoint, Line, PolygonShape, Polyline, PolylineArea, PolylineShape, Simplex, TileShape};
use crate::model::{AreaKind, AutorouteSettings, Board, FixedState, Item, ItemKind, Layer, Net, NetClass, Padstack, Rules, ViaInfo};
use crate::rules::ClearanceMatrix;

/// The words of one record, read in turn.
pub struct Words<'a> {
    words: Vec<&'a str>,
    next: usize,
}

impl<'a> Words<'a> {
    pub fn new(words: &[&'a str]) -> Self {
        Words { words: words.to_vec(), next: 0 }
    }

    pub fn word(&mut self) -> Result<&'a str, String> {
        let w = self.words.get(self.next).copied().ok_or_else(|| format!("record too short: {}", self.words.join(" ")))?;
        self.next += 1;
        Ok(w)
    }

    pub fn rest(&self) -> &[&'a str] {
        &self.words[self.next.min(self.words.len())..]
    }

    pub fn int(&mut self) -> Result<i64, String> {
        let w = self.word()?;
        w.parse().map_err(|_| format!("not an integer: {w}"))
    }

    pub fn float(&mut self) -> Result<f64, String> {
        let w = self.word()?;
        w.parse().map_err(|_| format!("not a number: {w}"))
    }

    pub fn ints(&mut self, n: usize) -> Result<Vec<i64>, String> {
        (0..n).map(|_| self.int()).collect()
    }

    /// `n` lines, each `ax ay bx by`.
    pub fn lines(&mut self, n: usize) -> Result<Vec<Line>, String> {
        (0..n)
            .map(|_| {
                let v = self.ints(4)?;
                Ok(Line::new(IntPoint::new(v[0], v[1]), IntPoint::new(v[2], v[3])))
            })
            .collect()
    }

    /// A convex shape of the given kind: "box", "octagon" or "simplex".
    pub fn tile(&mut self, kind: &str) -> Result<Option<TileShape>, String> {
        Ok(Some(match kind {
            "box" => {
                let n = self.ints(4)?;
                TileShape::Box(IntBox::new(n[0], n[1], n[2], n[3]))
            }
            "octagon" => {
                let n = self.ints(8)?;
                TileShape::Octagon(IntOctagon::new(n[0], n[1], n[2], n[3], n[4], n[5], n[6], n[7]))
            }
            "simplex" => {
                let k = self.int()? as usize;
                TileShape::Simplex(Simplex::new(self.lines(k)?))
            }
            _ => return Ok(None),
        }))
    }

    /// A pad: a circle, or one of the convex shapes.
    pub fn pad(&mut self) -> Result<PadShape, String> {
        let kind = self.word()?;
        if kind == "circle" {
            let n = self.ints(3)?;
            return Ok(PadShape::Circle(Circle::new(IntPoint::new(n[0], n[1]), n[2])));
        }
        match self.tile(kind)? {
            Some(TileShape::Box(b)) => Ok(PadShape::Box(b)),
            Some(TileShape::Octagon(o)) => Ok(PadShape::Octagon(o)),
            Some(TileShape::Simplex(s)) => Ok(PadShape::Polygon(s)),
            None => Err(format!("unknown pad shape {kind}")),
        }
    }

    /// A polygon by the corners FreeRouting keeps, taken as they are: its
    /// constructor has already cleaned them, and doing so again need not
    /// leave them unchanged.
    pub fn polygon(&mut self) -> Result<PolygonShape, String> {
        let k = self.int()? as usize;
        let n = self.ints(2 * k)?;
        Ok(PolygonShape { corners: (0..k).map(|i| IntPoint::new(n[2 * i], n[2 * i + 1])).collect() })
    }

    fn part(&mut self) -> Result<PolylineShape, String> {
        match self.word()? {
            "polygon" => Ok(PolylineShape::Polygon(self.polygon()?)),
            kind => self.tile(kind)?.map(PolylineShape::Tile).ok_or_else(|| format!("unknown area part {kind}")),
        }
    }

    /// An area: a circle, a polygon, a convex shape, or a border with holes.
    /// `None` for a kind the port cannot read.
    pub fn area(&mut self) -> Result<Option<AreaShape>, String> {
        Ok(match self.word()? {
            "circle" => {
                let n = self.ints(3)?;
                Some(AreaShape::Circle(Circle::new(IntPoint::new(n[0], n[1]), n[2])))
            }
            "polygon" => Some(AreaShape::Polygon(self.polygon()?)),
            "holes" => {
                let k = self.int()? as usize;
                let border = self.part()?;
                let holes = (0..k).map(|_| self.part()).collect::<Result<Vec<_>, _>>()?;
                Some(AreaShape::WithHoles(PolylineArea { border, holes }))
            }
            kind => self.tile(kind)?.map(AreaShape::Tile),
        })
    }
}

/// An item's `it` record.
struct Head {
    id: u32,
    kind: String,
    first_layer: i32,
    last_layer: i32,
    clearance_class: i32,
    fixed: FixedState,
    component: i32,
    nets: Vec<i32>,
}

/// Item data gathered from records that follow the item's `it` record.
#[derive(Default)]
struct Parts {
    center: Option<IntPoint>,
    pads: BTreeMap<usize, PadShape>,
    padstack: Option<usize>,
    trace: Option<(i32, i64, Vec<Line>)>,
    area: Option<(i32, AreaShape)>,
    conduction_obstacle: Option<bool>,
    outline: Option<(i64, bool, Vec<Vec<Line>>)>,
    neckdown: Vec<(i32, i64)>,
    exits: Vec<(i32, Direction)>,
}

/// Read the board from a dump. Fails on a record it cannot make sense of,
/// or an area of a shape the port cannot read.
pub fn read_board(text: &str) -> Result<Board, String> {
    let mut bounds = None;
    let mut layers: Vec<Layer> = Vec::new();
    let mut host_cad = false;
    let mut area_section = 50_000.0;
    let mut min_trace_half_width = 0;
    let mut default_via_diameter = 0.0;
    let mut pin_edge_to_turn_dist = -1.0;
    let mut board_max_trace_half_width = 1000;
    let mut max_trace_half_width = 100;
    let mut pull_tight_accuracy = 500;
    let mut id_max = 0;
    let mut classes = 0usize;
    let mut cm: Vec<(usize, usize, usize, i64)> = Vec::new();
    let mut cmax: Vec<(usize, usize, i64)> = Vec::new();
    let mut padstacks = Vec::new();
    let mut via_infos: BTreeMap<usize, ViaInfo> = BTreeMap::new();
    let mut via_rules: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut net_classes: BTreeMap<usize, NetClass> = BTreeMap::new();
    let mut nets = BTreeMap::new();
    let mut settings = None;
    let mut layer_costs: BTreeMap<usize, (bool, f64, f64, f64)> = BTreeMap::new();
    let mut heads: Vec<Head> = Vec::new();
    let mut parts: BTreeMap<u32, Parts> = BTreeMap::new();
    let mut last_outline: Option<u32> = None;

    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let Some((&key, rest)) = f.split_first() else { continue };
        let mut w = Words::new(rest);
        match key {
            "board" => {
                let n = w.ints(4)?;
                bounds = Some(IntBox::new(n[0], n[1], n[2], n[3]));
            }
            "layer" => {
                let index = w.int()? as usize;
                let is_signal = w.int()? != 0;
                let name = w.rest().join(" ");
                if index != layers.len() {
                    return Err(format!("layer {index} out of order"));
                }
                layers.push(Layer { name, is_signal });
            }
            "host_cad" => host_cad = w.int()? != 0,
            "area_section" => area_section = w.float()?,
            "min_trace_half_width" => min_trace_half_width = w.int()?,
            "default_via_diameter" => default_via_diameter = w.float()?,
            "pin_edge_to_turn_dist" => pin_edge_to_turn_dist = w.float()?,
            "trace_half_widths" => {
                let n = w.ints(4)?;
                min_trace_half_width = n[0];
                board_max_trace_half_width = n[1];
                max_trace_half_width = n[3];
            }
            "pull_tight_accuracy" => pull_tight_accuracy = w.int()? as i32,
            "id_generator" => id_max = w.int()? as u32,
            "pin_neckdown" => {
                let n = w.ints(3)?;
                parts.entry(n[0] as u32).or_default().neckdown.push((n[1] as i32, n[2]));
            }
            "pin_exit" => {
                let n = w.ints(4)?;
                parts.entry(n[0] as u32).or_default().exits.push((n[1] as i32, Direction::of(n[2], n[3])));
            }
            "classes" => classes = w.int()? as usize,
            "cm" => {
                let n = w.ints(4)?;
                cm.push((n[0] as usize, n[1] as usize, n[2] as usize, n[3]));
            }
            "cmax" => {
                let n = w.ints(3)?;
                cmax.push((n[0] as usize, n[1] as usize, n[2]));
            }
            "padstack" => {
                let n = w.ints(3)?;
                let max_width = w.rest().iter().map(|s| s.parse::<f64>().map(|v| (v >= 0.0).then_some(v))).collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
                padstacks.push(Padstack { no: n[0] as usize, from_layer: n[1] as i32, to_layer: n[2] as i32, max_width });
            }
            "viainfo" => {
                let n = w.ints(4)?;
                via_infos.insert(n[0] as usize, ViaInfo { padstack: n[1] as usize, clearance_class: n[2] as i32, attach_smd_allowed: n[3] != 0 });
            }
            "viarule" => {
                let index = w.int()? as usize;
                let vias = w.rest().iter().map(|s| s.parse::<usize>()).collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
                via_rules.insert(index, vias);
            }
            "netclass" => {
                let n = w.ints(3)?;
                let l = layers.len();
                let active = w.ints(l)?.into_iter().map(|v| v != 0).collect();
                let half = w.ints(l)?;
                let shove_fixed = w.rest().first().is_some_and(|s| *s == "1");
                net_classes.insert(
                    n[0] as usize,
                    NetClass { trace_clearance_class: n[1] as i32, via_rule: n[2] as usize, active_layers: active, trace_half_width: half, shove_fixed },
                );
            }
            "net" => {
                let n = w.ints(3)?;
                nets.insert(n[0] as i32, Net { no: n[0] as i32, class: n[1] as usize, contains_plane: n[2] != 0 });
            }
            "settings" => {
                let n = w.ints(6)?;
                settings = Some(n);
            }
            "layer_costs" => {
                let l = w.int()? as usize;
                let active = w.int()? != 0;
                layer_costs.insert(l, (active, w.float()?, w.float()?, w.float()?));
            }
            "it" => {
                let id = w.int()? as u32;
                let kind = w.word()?.to_string();
                let n = w.ints(5)?;
                let fixed = FixedState::from_ordinal(n[3] as u32).ok_or("bad fixed state")?;
                let nets = w.rest().iter().map(|s| s.parse::<i32>()).collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
                heads.push(Head { id, kind, first_layer: n[0] as i32, last_layer: n[1] as i32, clearance_class: n[2] as i32, fixed, component: n[4] as i32, nets });
            }
            "center" => {
                let n = w.ints(3)?;
                parts.entry(n[0] as u32).or_default().center = Some(IntPoint::new(n[1], n[2]));
            }
            "pad" => {
                let n = w.ints(4)?;
                let shape = w.pad()?;
                parts.entry(n[0] as u32).or_default().pads.insert(n[1] as usize, shape);
            }
            "via_padstack" => {
                let n = w.ints(2)?;
                parts.entry(n[0] as u32).or_default().padstack = Some(n[1] as usize);
            }
            "trace" => {
                let n = w.ints(5)?;
                let lines = w.lines(n[4] as usize)?;
                parts.entry(n[0] as u32).or_default().trace = Some((n[1] as i32, n[2], lines));
            }
            "area" => {
                let n = w.ints(3)?;
                let shape = w.area()?.ok_or_else(|| format!("area {} of a shape the port cannot read", n[0]))?;
                parts.entry(n[0] as u32).or_default().area = Some((n[1] as i32, shape));
            }
            "conduction" => {
                let n = w.ints(2)?;
                parts.entry(n[0] as u32).or_default().conduction_obstacle = Some(n[1] != 0);
            }
            "outline" => {
                let n = w.ints(5)?;
                parts.entry(n[0] as u32).or_default().outline = Some((n[1], n[4] != 0, Vec::new()));
                last_outline = Some(n[0] as u32);
            }
            "outline_shape" => {
                let k = w.int()? as usize;
                let lines = w.lines(k)?;
                let id = last_outline.ok_or("outline_shape before outline")?;
                if let Some((_, _, shapes)) = parts.entry(id).or_default().outline.as_mut() {
                    shapes.push(lines);
                }
            }
            _ => {}
        }
    }

    let layer_count = layers.len();
    let mut clearance = ClearanceMatrix::new(classes.max(1), layer_count);
    for &(i, j, l, v) in &cm {
        clearance.set(i, j, l, v);
    }
    let mut max_clearance = vec![vec![0; layer_count]; classes.max(1)];
    for &(i, l, v) in &cmax {
        max_clearance[i][l] = v;
    }
    let s = settings.ok_or("no settings record")?;
    let mut settings = AutorouteSettings {
        vias_allowed: s[0] != 0,
        via_costs: s[1] as i32,
        plane_via_costs: s[2] as i32,
        start_ripup_costs: s[3] as i32,
        with_fanout: s[4] != 0,
        automatic_neckdown: s[5] != 0,
        layer_active: vec![false; layer_count],
        trace_costs: vec![(1.0, 1.0); layer_count],
        preferred_direction_costs: vec![1.0; layer_count],
    };
    for (l, (active, h, v, p)) in layer_costs {
        settings.layer_active[l] = active;
        settings.trace_costs[l] = (h, v);
        settings.preferred_direction_costs[l] = p;
    }
    let rules = Rules {
        clearance,
        max_clearance,
        padstacks,
        via_infos: dense(via_infos, "via info")?,
        via_rules: dense(via_rules, "via rule")?,
        net_classes: dense(net_classes, "net class")?,
        nets,
        min_trace_half_width,
        default_via_diameter,
        pin_edge_to_turn_dist,
        board_max_trace_half_width,
        max_trace_half_width,
        pull_tight_accuracy,
    };

    let mut items = Vec::with_capacity(heads.len());
    for Head { id, kind, first_layer, last_layer, clearance_class, fixed, component, nets } in heads {
        let p = parts.remove(&id).unwrap_or_default();
        let pads = || -> Vec<Option<PadShape>> {
            let n = (last_layer - first_layer + 1).max(0) as usize;
            (0..n).map(|i| p.pads.get(&i).cloned()).collect()
        };
        let kind = match kind.as_str() {
            "pin" => {
                let n = (last_layer - first_layer + 1).max(0) as usize;
                let mut neckdown = vec![1; n];
                for &(l, v) in &p.neckdown {
                    neckdown[(l - first_layer) as usize] = v;
                }
                let mut exits = vec![Vec::new(); n];
                for &(l, d) in &p.exits {
                    exits[(l - first_layer) as usize].push(d);
                }
                ItemKind::Pin { center: p.center.ok_or(format!("pin {id} has no center"))?, pads: pads(), neckdown, exits }
            }
            "via" => ItemKind::Via {
                center: p.center.ok_or(format!("via {id} has no center"))?,
                padstack: p.padstack.ok_or(format!("via {id} has no padstack"))?,
                pads: pads(),
            },
            "trace" => {
                let (layer, half_width, lines) = p.trace.clone().ok_or(format!("trace {id} has no polyline"))?;
                ItemKind::Trace { layer, half_width, polyline: Polyline { lines } }
            }
            "keepout" | "via_keepout" | "component_keepout" | "conduction" => {
                let (layer, shape) = p.area.clone().ok_or(format!("area {id} has no shape"))?;
                let area_kind = match kind.as_str() {
                    "keepout" => AreaKind::Keepout,
                    "via_keepout" => AreaKind::ViaKeepout,
                    "component_keepout" => AreaKind::ComponentKeepout,
                    _ => AreaKind::Conduction { is_obstacle: p.conduction_obstacle.ok_or(format!("pour {id} without its obstacle flag"))? },
                };
                ItemKind::Area { kind: area_kind, layer, shape }
            }
            "outline" => {
                let (half_width, keepout_outside, shapes) = p.outline.clone().ok_or(format!("outline {id} has no shapes"))?;
                ItemKind::Outline { half_width, shapes, keepout_outside }
            }
            "component_outline" => ItemKind::ComponentOutline,
            other => return Err(format!("item {id} of a kind the port does not know: {other}")),
        };
        items.push(Item { id, kind, first_layer, last_layer, clearance_class, fixed, component, nets });
    }

    Ok(Board { bounds: bounds.ok_or("no board record")?, layers, rules, settings, items, host_cad, area_section, id_max })
}

/// The values of a map keyed 0, 1, 2, ... in key order; an error naming the
/// first key missing.
fn dense<T>(m: BTreeMap<usize, T>, what: &str) -> Result<Vec<T>, String> {
    m.into_iter().enumerate().map(|(k, (i, x))| if k == i { Ok(x) } else { Err(format!("{what} {k} missing")) }).collect()
}
