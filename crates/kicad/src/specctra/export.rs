//! `SPECCTRA_DB::FromBOARD` (`pcbnew/specctra_import_export/specctra_export.cpp`): a board written as a Specctra DSN design.
//!
//! The board is [`BoardView`], the footprints of the back turned over to the front first (`FlipFOOTPRINTs`); the outline, with its cutouts, was built before that
//! (`BuiltBoardOutlines`). What `FromBOARD` makes is a [`Pcb`]: its boundary and keepouts, the layers, the planes of the zones, the images and padstacks of the footprints and
//! their pads, the placement, the nets with their classes, and the wiring of the tracks and vias the board has.
//!
//! The functions have the names of the C++ ones: `makePADSTACK`, `makeIMAGE`, `makeVia`, `fillBOUNDARY`, `exportNETCLASS`.

use eda_model::units::Nm;
use super::board::*;
use super::geom::*;
use super::model::*;
use eda_drc::outline::BoardOutline;
use eda_shape_poly_set::{CornerStrategy, ShapePolySet};

/// What `FromBOARD` takes besides the board: the board outline (`m_brd_outlines`, made before the footprints were flipped, in nanometres), the design settings it reads
/// (`m_HoleClearance`, `m_MaxError`), the Default net class and how a net finds its own.
pub struct Settings<'a> {
    pub pcb_name: String,
    pub host_version: String,
    pub outline: &'a BoardOutline,
    /// `BOARD_DESIGN_SETTINGS::m_HoleClearance`, nanometres.
    pub hole_clearance: i64,
    /// `BOARD_DESIGN_SETTINGS::m_MaxError`, nanometres.
    pub max_error: i64,
    /// `NET_SETTINGS::GetDefaultNetclass()`.
    pub default_class: ClassFacts,
    /// `NETINFO_ITEM::GetNetClass()` by net name: the effective class of a net.
    pub class_of_net: &'a dyn Fn(&str) -> ClassFacts,
}

/// The facts `FromBOARD` reads of a `NETCLASS`, nanometres.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassFacts {
    pub name: String,
    pub track_width: i64,
    pub clearance: i64,
    pub via_diameter: i64,
    pub via_drill: i64,
}

/// `scale( int )`: nanometres to the micrometres of the DSN file.
pub fn scale(nm: i64) -> f64 {
    nm as f64 / (1_000_000.0 / 1000.0)
}

/// `IU2um( int )`.
fn iu2um(nm: i64) -> f64 {
    nm as f64 * (1000.0 / 1_000_000.0)
}

/// `mapPt( VECTOR2I )`: the y axis of the DSN file points up.
fn map_pt(v: V) -> Pt {
    Pt::new(scale(v.x), -scale(v.y)).fixed()
}

/// `mapPt( pt, footprint )`: relative to the footprint, turned back by its orientation.
fn map_pt_fp(v: V, fp: &Fp) -> Pt {
    map_pt(rotate_point(v - fp.pos, -fp.orient))
}

fn fixed6(x: f64) -> String {
    format!("{x:.6}")
}

/// The reasons an export fails (`THROW_IO_ERROR` of `FromBOARD`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportError(pub String);

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `SPECCTRA_DB`'s state while it builds the file: the layer maps, the padstack set, the library under construction.
struct Db<'a> {
    st: &'a Settings<'a>,
    /// `m_layerIds`, indexed by the layer's place in the stack.
    layer_ids: Vec<String>,
    /// `m_pcbLayer2kicad`.
    kicad_layers: Vec<String>,
    /// `m_padstackset`, ordered by `PADSTACK::Compare`.
    padstacks: std::collections::BTreeMap<(String, String), Padstack>,
    pcb: Pcb,
}

/// `ExportBoardToSpecctraFile`: the board, a DSN design.
pub fn from_board(board: &mut BoardView, st: &Settings) -> Result<(Pcb, Vec<String>), ExportError> {
    let mut warnings = Vec::new();
    // `BuiltBoardOutlines`, before the footprints are flipped.
    if !st.outline.valid {
        warnings.push("Board outline is malformed. Run DRC for a full analysis.".to_string());
    }
    // `FlipFOOTPRINTs`: DSN images (= KiCad footprints and pads) must be presented from the top view.
    board.flip_footprints_to_top();

    // Not all boards are exportable. Check that all reference ids are unique, or the session file which comes back later cannot be imported.
    {
        let mut refs = std::collections::BTreeSet::new();
        for fp in &board.footprints {
            if fp.reference.is_empty() {
                return Err(ExportError(format!("Footprint with value of '{}' has an empty reference designator.", fp.value)));
            }
            if !refs.insert(fp.reference.clone()) {
                return Err(ExportError(format!("Multiple footprints have the reference designator '{}'.", fp.reference)));
            }
        }
    }

    let mut db = Db { st, layer_ids: Vec::new(), kicad_layers: Vec::new(), padstacks: Default::default(), pcb: Pcb::make(&st.host_version) };
    db.pcb.pcbname = st.pcb_name.clone();

    // ----<layer_descriptor>---- Specctra wants the top physical layer first, then going down to the bottom most physical layer in physical sequence.
    for layer in &board.copper {
        db.kicad_layers.push(layer.name.clone());
        db.layer_ids.push(layer.shown.clone());
    }
    {
        let structure = db.pcb.structure.as_mut().expect("MakePCB");
        for (ndx, layer) in board.copper.iter().enumerate() {
            let mut l = Layer::new(&db.layer_ids[ndx]);
            // Freerouter does not support type "mixed", only signal and power: "mixed" is remapped to "signal".
            l.layer_type = match layer.kind.as_str() {
                "power" => "power",
                "jumper" => "jumper",
                _ => "signal",
            };
            l.properties.push(Property { name: "index".into(), value: ndx.to_string() });
            structure.layers.push(l);
        }
    }

    // A space in a quoted token is NOT a terminator, true establishes this.
    db.pcb.parser.as_mut().expect("MakePCB").space_in_quoted_tokens = true;

    // ----<unit_descriptor> & <resolution_descriptor>---- tenths of micrometres, which is 100 nm resolution.
    db.pcb.unit = Some(UnitRes::unit(Unit::Um));
    db.pcb.resolution = Some(UnitRes::resolution(Unit::Um, 10));

    // ----<boundary_descriptor>----
    db.fill_boundary();

    // ----<rules>----
    {
        let default_clearance = st.default_class.clearance;
        let rules = &mut db.pcb.structure.as_mut().expect("MakePCB").rules.as_mut().expect("MakePCB").rules;
        rules.push(format!("(width {})", super::format::g6(scale(st.default_class.track_width))));
        rules.push(format!("(clearance {})", super::format::g6(scale(default_clearance))));
        // Pad to pad spacing on a single SMT part can be closer than our clearance. We don't want freerouter complaining about that, so a significantly smaller pad to pad
        // clearance is output.
        rules.push(format!("(clearance {} (type smd_smd))", super::format::g6(scale(default_clearance) / 4.0)));
    }

    // ----<zones (not keepout areas) become planes>----
    {
        let mut netless_zones = 0;
        for zone in &board.zones {
            if zone.rule_area || !zone.on_copper() {
                continue;
            }
            // Currently, only copper layers are exported: the layers the board has, in `PCB_LAYER_ID` order.
            for layer in zone.layers.iter().filter(|l| board.copper.iter().any(|c| &c.name == *l)) {
                let mut plane = Keepout::new("plane");
                plane.name = zone.net_name.clone();
                if plane.name.is_empty() {
                    // This is one of those no connection zones, netcode=0, and it has no name. Create a unique, bogus netname.
                    let bogus = format!("@:no_net_{netless_zones}");
                    netless_zones += 1;
                    db.pcb.network.as_mut().expect("MakePCB").nets.push(Net { net_id: bogus.clone(), ..Default::default() });
                    plane.name = bogus;
                }
                let layer_id = db.layer_id_of(board, layer);
                let (shape, windows) = zone_shape(zone, &layer_id, &layer_id);
                plane.shape = Some(shape);
                plane.windows = windows;
                db.pcb.structure.as_mut().expect("MakePCB").planes.push(plane);
            }
        }
    }

    // ----<zones flagged keepout areas become keepout>----
    for zone in &board.zones {
        if !zone.rule_area {
            continue;
        }
        let kind = keepout_kind(zone);
        for layer in zone.layers.iter().filter(|l| board.copper.iter().any(|c| &c.name == *l)) {
            let mut keepout = Keepout::new(kind);
            let layer_id = db.layer_id_of(board, layer);
            let (shape, windows) = zone_shape(zone, &layer_id, &layer_id);
            keepout.shape = Some(shape);
            keepout.windows = windows;
            db.pcb.structure.as_mut().expect("MakePCB").keepouts.push(keepout);
        }
    }

    // ----<build the images, components, and netlist>----
    db.images_components_and_nets(board);

    // The nets' classes in use, the Default class apart.
    let mut classes_in_use: std::collections::BTreeMap<String, ClassFacts> = std::collections::BTreeMap::new();
    for (_, name) in &board.nets {
        let class = (st.class_of_net)(name);
        if class.name == "Default" {
            continue;
        }
        classes_in_use.entry(class.name.clone()).or_insert(class);
    }

    // ----< output vias used in netclasses >----
    // The netclass vias are all the same kind of thru, blind or buried via, from the first layer of the stack to the last.
    let top_via_layer = 0usize;
    let bot_via_layer = board.copper_count() - 1;
    {
        // The via from the Default netclass first: the via container preserves the sequence of addition.
        let via = db.make_via(st.default_class.via_diameter, st.default_class.via_drill, top_via_layer, bot_via_layer);
        db.pcb.library.as_mut().expect("MakePCB").vias.push(via);
        for class in classes_in_use.values() {
            let via = db.make_via(class.via_diameter, class.via_drill, top_via_layer, bot_via_layer);
            db.pcb.library.as_mut().expect("MakePCB").lookup_via(via);
        }
    }

    // ----<create the wires from tracks>----
    db.wires(board);

    // ----<export the existing real BOARD instantiated vias>----
    db.vias(board);

    // ----<via_descriptor>---- the vias in the padstack list, by name only.
    {
        let names: Vec<String> = db.pcb.library.as_ref().expect("MakePCB").vias.iter().map(|v| v.padstack_id.clone()).collect();
        db.pcb.structure.as_mut().expect("MakePCB").via.as_mut().expect("MakePCB").padstacks.extend(names);
    }

    // ----<output NETCLASSs>----
    db.export_netclass(board, &st.default_class, top_via_layer, bot_via_layer);
    for class in classes_in_use.values() {
        db.export_netclass(board, class, top_via_layer, bot_via_layer);
    }

    Ok((db.pcb, warnings))
}

/// The type of keepout a rule area is: `keepout`, `via_keepout` or `wire_keepout` (the three Pcbnew knows).
fn keepout_kind(zone: &ZoneItem) -> &'static str {
    if zone.no_vias && zone.no_tracks {
        "keepout"
    } else if zone.no_vias {
        "via_keepout"
    } else if zone.no_tracks {
        "wire_keepout"
    } else {
        "keepout"
    }
}

/// The polygon of a zone and its holes as the main shape and the windows of a plane or a keepout (`IterateWithHoles`): each ring closed by repeating its first point.
fn zone_shape(zone: &ZoneItem, layer_id: &str, window_layer: &str) -> (ShapeElem, Vec<Window>) {
    let mut main = PathElem::new(true);
    main.layer_id = layer_id.to_string();
    if let Some(outline) = zone.polys.first() {
        for p in outline {
            main.points.push(map_pt(*p));
        }
        if let Some(first) = outline.first() {
            main.points.push(map_pt(*first));
        }
    }
    let mut windows = Vec::new();
    for hole in zone.polys.iter().skip(1) {
        let mut cutout = PathElem::new(true);
        cutout.layer_id = window_layer.to_string();
        for p in hole {
            cutout.points.push(map_pt(*p));
        }
        if let Some(first) = hole.first() {
            cutout.points.push(map_pt(*first));
        }
        windows.push(Window { shape: Some(ShapeElem::Path(cutout)) });
    }
    (ShapeElem::Path(main), windows)
}

impl<'a> Db<'a> {
    /// `m_layerIds[ m_kicadLayer2pcb[ layer ] ]`.
    fn layer_id_of(&self, board: &BoardView, kicad_layer: &str) -> String {
        let _ = board;
        match self.kicad_layers.iter().position(|l| l == kicad_layer) {
            Some(i) => self.layer_ids[i].clone(),
            None => kicad_layer.to_string(),
        }
    }

    /// `fillBOUNDARY`: the outline as the boundary, its holes as keepouts on every signal layer.
    fn fill_boundary(&mut self) {
        let polys = &self.st.outline.polys;
        let mut boundary = Boundary::new(false);
        let mut hole_keepouts = Vec::new();
        for cnt in 0..polys.outline_count() {
            let mut path = PathElem::new(false);
            path.layer_id = "pcb".to_string();
            let outline = polys.outline(cnt);
            // `mapPt`: the outline is in nanometres, as KiCad builds it.
            let um = |p: &eda_clipper2::Point64| map_pt(V::new(p.x, p.y));
            for p in outline.iter() {
                path.points.push(um(p));
            }
            // Close polygon.
            if let Some(first) = outline.first() {
                path.points.push(um(first));
            }
            boundary.paths.push(path);

            // Generate holes as keepout.
            for ii in 0..polys.hole_count(cnt) {
                let mut keepout = Keepout::new("keepout");
                let mut poly_ko = PathElem::new(true);
                poly_ko.layer_id = "signal".to_string();
                let hole = polys.hole(cnt, ii);
                for p in hole.iter() {
                    poly_ko.points.push(um(p));
                }
                if let Some(first) = hole.first() {
                    poly_ko.points.push(um(first));
                }
                keepout.shape = Some(ShapeElem::Path(poly_ko));
                hole_keepouts.push(keepout);
            }
        }
        let structure = self.pcb.structure.as_mut().expect("MakePCB");
        structure.boundary = Some(boundary);
        structure.keepouts.extend(hole_keepouts);
    }

    /// `makeVia( aCopperDiameter, aDrillDiameter, aTopLayer, aBotLayer )`.
    fn make_via(&self, copper_diameter: i64, drill_diameter: i64, top_layer: usize, bot_layer: usize) -> Padstack {
        let mut padstack = Padstack::new();
        let dsn_diameter = scale(copper_diameter);
        for layer in top_layer..=bot_layer {
            let mut shape = ShapeEl::new(false);
            shape.shape = Some(ShapeElem::Circle(Circle { layer_id: self.layer_ids[layer].clone(), diameter: dsn_diameter, vertex: Pt::default() }));
            padstack.shapes.push(shape);
        }
        // The drill is encoded in the name for the import of the session.
        padstack.padstack_id = format!("Via[{top_layer}-{bot_layer}]_{}:{}_um", super::format::g6(dsn_diameter), super::format::g6(iu2um(drill_diameter)));
        if padstack.padstack_id.len() > 47 {
            padstack.padstack_id.truncate(47);
        }
        padstack
    }

    /// `makePADSTACK`.
    fn make_padstack(&self, board: &BoardView, pad: &Pad) -> Padstack {
        let mut padstack = Padstack::new();
        let copper_count = board.copper_count();
        let on_all_copper = board.copper.iter().all(|l| pad.on_layer(&l.name));
        let mut uniqifier = String::from("[");
        if on_all_copper {
            uniqifier.push('A');
        }
        let mut layer_names: Vec<String> = Vec::new();
        for layer in 0..copper_count {
            if on_all_copper || pad.on_layer(&self.kicad_layers[layer]) {
                layer_names.push(self.layer_ids[layer].clone());
                if !on_all_copper {
                    if layer == 0 {
                        uniqifier.push('T');
                    } else if layer == copper_count - 1 {
                        uniqifier.push('B');
                    } else {
                        uniqifier.push((b'0' + layer as u8) as char);
                    }
                }
            }
        }
        uniqifier.push(']');

        let pad_size = pad.size;
        let mut dsn_offset = Pt::default();
        if pad.offset.x != 0 || pad.offset.y != 0 {
            dsn_offset = map_pt(pad.offset);
            // Using () would cause padstack name to be quoted, and {} locks freerouter, so use [].
            uniqifier.push_str(&format!("[{},{}]", fixed6(dsn_offset.x), fixed6(dsn_offset.y)));
        }
        let add = |p: Pt| Pt::new(p.x + dsn_offset.x, p.y + dsn_offset.y);

        let polygon_on = |ps: &mut Padstack, layer: &str, pts: &[Pt]| {
            let mut shape = ShapeEl::new(false);
            let mut polygon = PathElem::new(true);
            polygon.layer_id = layer.to_string();
            polygon.points = pts.to_vec();
            shape.shape = Some(ShapeElem::Path(polygon));
            ps.shapes.push(shape);
        };

        match pad.shape {
            PadShapeKind::Circle => {
                let diameter = scale(pad_size.x);
                for layer in &layer_names {
                    let mut shape = ShapeEl::new(false);
                    shape.shape = Some(ShapeElem::Circle(Circle { layer_id: layer.clone(), diameter, vertex: dsn_offset }));
                    padstack.shapes.push(shape);
                }
                padstack.padstack_id = format!("Round{uniqifier}Pad_{}_um", fixed6(iu2um(pad_size.x)));
            }
            PadShapeKind::Rect => {
                let dx = scale(pad_size.x) / 2.0;
                let dy = scale(pad_size.y) / 2.0;
                let lower_left = add(Pt::new(-dx, -dy));
                let upper_right = add(Pt::new(dx, dy));
                for layer in &layer_names {
                    let mut rect = Rectangle { layer_id: layer.clone(), ..Default::default() };
                    rect.set_corners(lower_left, upper_right);
                    let mut shape = ShapeEl::new(false);
                    shape.shape = Some(ShapeElem::Rect(rect));
                    padstack.shapes.push(shape);
                }
                padstack.padstack_id = format!("Rect{uniqifier}Pad_{}x{}_um", fixed6(iu2um(pad_size.x)), fixed6(iu2um(pad_size.y)));
            }
            PadShapeKind::Oval => {
                let dx = scale(pad_size.x) / 2.0;
                let dy = scale(pad_size.y) / 2.0;
                let mut dr = dx - dy;
                let radius;
                let (pstart, pstop);
                if dr >= 0.0 {
                    // The oval is horizontal.
                    radius = dy;
                    pstart = Pt::new(-dr, 0.0);
                    pstop = Pt::new(dr, 0.0);
                } else {
                    // The oval is vertical.
                    radius = dx;
                    dr = -dr;
                    pstart = Pt::new(0.0, -dr);
                    pstop = Pt::new(0.0, dr);
                }
                let (pstart, pstop) = (add(pstart), add(pstop));
                for layer in &layer_names {
                    let mut path = PathElem::new(false);
                    path.layer_id = layer.clone();
                    path.points = vec![pstart, pstop];
                    path.aperture_width = 2.0 * radius;
                    let mut shape = ShapeEl::new(false);
                    shape.shape = Some(ShapeElem::Path(path));
                    padstack.shapes.push(shape);
                }
                padstack.padstack_id = format!("Oval{uniqifier}Pad_{}x{}_um", fixed6(iu2um(pad_size.x)), fixed6(iu2um(pad_size.y)));
            }
            PadShapeKind::Trapezoid => {
                let dx = scale(pad_size.x) / 2.0;
                let dy = scale(pad_size.y) / 2.0;
                let delta = pad.delta;
                let ddx = scale(delta.x) / 2.0;
                let ddy = scale(delta.y) / 2.0;
                // See class_pad_draw_functions.cpp which draws the trapezoid pad.
                let lower_left = add(Pt::new(-dx - ddy, -dy - ddx));
                let upper_left = add(Pt::new(-dx + ddy, dy + ddx));
                let upper_right = add(Pt::new(dx - ddy, dy - ddx));
                let lower_right = add(Pt::new(dx + ddy, -dy + ddx));
                for layer in &layer_names {
                    polygon_on(&mut padstack, layer, &[lower_left, upper_left, upper_right, lower_right]);
                }
                // This string _must_ be unique for a given physical shape.
                padstack.padstack_id = format!(
                    "Trapz{uniqifier}Pad_{}x{}_{}{}x{}{}_um",
                    fixed6(iu2um(pad_size.x)),
                    fixed6(iu2um(pad_size.y)),
                    if delta.x < 0 { "n" } else { "p" },
                    fixed6(iu2um(delta.x).abs()),
                    if delta.y < 0 { "n" } else { "p" },
                    fixed6(iu2um(delta.y).abs())
                );
            }
            PadShapeKind::ChamferedRect | PadShapeKind::RoundRect => {
                // Export the shape as a polygon, a round rect does not exist as a primitive. A slightly bigger shape is used because the round corners are approximated by
                // segments, giving the polygon a slightly smaller shape than the actual one.
                const CIRCLE_TO_SEGMENTS_COUNT: f64 = 36.0;
                let mut rradius = kiround(pad_size.x.min(pad_size.y) as f64 * pad.rratio);
                let correction_factor = (PI / CIRCLE_TO_SEGMENTS_COUNT).cos();
                let extra_clearance = kiround(rradius as f64 * (1.0 - correction_factor));
                let psize = V::new(pad_size.x + extra_clearance * 2, pad_size.y + extra_clearance * 2);
                rradius += extra_clearance;
                let do_chamfer = pad.shape == PadShapeKind::ChamferedRect;
                let poly = round_chamfered_rect_polygon(psize, rradius, pad.chamfer_ratio, if do_chamfer { pad.chamfer } else { 0 }, self.st.max_error);
                let mut pts: Vec<Pt> = poly.iter().map(|p| add(Pt::new(scale(p.x), scale(-p.y)))).collect();
                if let Some(first) = pts.first().copied() {
                    pts.push(first);
                }
                for layer in &layer_names {
                    polygon_on(&mut padstack, layer, &pts);
                }
                padstack.padstack_id = format!(
                    "RoundRect{uniqifier}Pad_{}x{}_{}_um_{}_{:X}",
                    fixed6(iu2um(pad_size.x)),
                    fixed6(iu2um(pad_size.y)),
                    fixed6(iu2um(rradius)),
                    fixed6(if do_chamfer { pad.chamfer_ratio } else { 0.0 }),
                    if do_chamfer { pad.chamfer } else { 0 }
                );
            }
            PadShapeKind::Custom => {
                // The exact shape can be a non convex polygon with holes, which routers handle badly: its convex hull is exported.
                let mut hull = custom_pad_hull(pad, self.st.max_error);
                if hull.first() != hull.last() {
                    if let Some(first) = hull.first().copied() {
                        hull.push(first);
                    }
                }
                let pts: Vec<Pt> = hull.iter().map(|p| add(Pt::new(scale(p.x), scale(-p.y)))).collect();
                for layer in &layer_names {
                    polygon_on(&mut padstack, layer, &pts);
                }
                // This string _must_ be unique for a given physical shape, so try to make it unique. `aPad->GetBoundingBox()` is the pad's on the board: the hull
                // turned by the pad's orientation, and its hole.
                let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
                let half_hole = V::new(pad.drill.x / 2, pad.drill.y / 2);
                for p in hull.iter().map(|p| rotate_point(*p, pad.orient)).chain([-half_hole, half_hole].into_iter().filter(|_| pad.drill.x > 0)) {
                    x0 = x0.min(p.x);
                    y0 = y0.min(p.y);
                    x1 = x1.max(p.x);
                    y1 = y1.max(p.y);
                }
                let (bw, bh) = if hull.is_empty() { (0, 0) } else { (x1 - x0, y1 - y0) };
                let mut hasher = blake3::Hasher::new();
                for p in &hull {
                    hasher.update(&p.x.to_le_bytes());
                    hasher.update(&p.y.to_le_bytes());
                }
                // KiCad's is the `HASH_128` of the merged polygon (`SHAPE_POLY_SET::GetHash`), which is not made here: this one is of the hull, in the same form.
                let hash = hasher.finalize().to_hex().to_ascii_uppercase();
                padstack.padstack_id = format!(
                    "Cust{uniqifier}Pad_{}x{}_{}x{}_{}_um_{}",
                    fixed6(iu2um(pad_size.x)),
                    fixed6(iu2um(pad_size.y)),
                    fixed6(iu2um(bw)),
                    fixed6(iu2um(bh)),
                    hull.len(),
                    &hash.as_str()[..32]
                );
            }
        }
        padstack
    }

    /// `makeIMAGE`: the image of a footprint (flipped to the top), with its pins; the padstacks it needs go to the padstack set.
    fn make_image(&mut self, board: &BoardView, fp: &Fp) -> Image {
        let mut image = Image { image_id: fp.fpid.clone(), ..Default::default() };
        let mut pinmap: std::collections::BTreeMap<String, i32> = std::collections::BTreeMap::new();

        // From the pads, make an image using collated padstacks.
        for pad in &fp.pads {
            if is_round_keepout(pad) {
                // A through hole with no copper on its perimeter: a round keepout on every layer.
                let mut diameter = scale(pad.drill.x);
                let vertex = map_pt(fp.relative_position(pad.pos));
                diameter += scale(self.st.hole_clearance * 2);
                for layer in 0..board.copper_count() {
                    let mut keepout = Keepout::new("keepout");
                    keepout.shape = Some(ShapeElem::Circle(Circle { layer_id: self.layer_ids[layer].clone(), diameter, vertex }));
                    image.keepouts.push(keepout);
                }
            } else {
                // Pads not on copper layers (i.e. only on tech layers) are ignored because they create invalid pads in the dsn file for freerouter.
                if !pad.on_copper() {
                    continue;
                }
                let padstack = self.make_padstack(board, pad);
                let key = padstack.compare_key();
                let padstack_id = padstack.padstack_id.clone();
                self.padstacks.entry(key).or_insert(padstack);

                let mut pin = Pin { padstack_id, ..Default::default() };
                let pad_number = pad.number.clone();
                pin.pin_id = pad_number.clone();
                if !pad_number.is_empty() && !pinmap.contains_key(&pad_number) {
                    pinmap.insert(pad_number, 0);
                } else {
                    // The pad name is a duplicate within this footprint: "@1", "@2", etc. is appended to it.
                    let n = pinmap.entry(pad_number).or_insert(0);
                    *n += 1;
                    pin.pin_id.push_str(&format!("@{n}"));
                }
                pin.ki_net_code = pad.net_code;
                let angle = (pad.orient - fp.orient).normalize();
                pin.set_rotation(angle.0);
                pin.vertex = map_pt(fp.relative_position(pad.pos));
                image.pins.push(pin);
            }
        }

        // Specctra only supports the first outline, so the courtyard comes first.
        for chain in self.courtyard(fp) {
            let mut outline = ShapeEl::new(true);
            let mut path = PathElem::new(true);
            path.aperture_width = 0.0;
            path.layer_id = "signal".to_string();
            for corner in chain {
                path.points.push(map_pt_fp(corner, fp));
            }
            outline.shape = Some(ShapeElem::Path(path));
            image.outlines.push(outline);
        }

        // The footprint's shapes, converted to outlines.
        for graphic in &fp.gfx {
            if graphic.layer == "F.CrtYd" || graphic.layer == "B.CrtYd" {
                continue; // The courtyard is already handled above.
            }
            let width = scale(graphic.width);
            let mut outline = ShapeEl::new(true);
            let mut path = PathElem::new(false);
            path.layer_id = "signal".to_string();
            match &graphic.shape {
                GfxShape::Segment(a, b) => {
                    path.aperture_width = width;
                    path.points = vec![map_pt_fp(*a, fp), map_pt_fp(*b, fp)];
                }
                GfxShape::Circle(c, e) => {
                    // This is best done by 4 QARC's but freerouter does not yet support QARCs: line segments are used.
                    path.aperture_width = width;
                    // `GetRadius()`: at least 1.
                    let radius = kiround(((e.x - c.x) as f64).hypot((e.y - c.y) as f64)).max(1);
                    for p in arc_to_polyline_inside(*c, radius, Angle::ZERO, Angle(360.0), ARC_HIGH_DEF) {
                        path.points.push(map_pt_fp(p, fp));
                    }
                }
                GfxShape::Rect(a, b) => {
                    path.aperture_width = width;
                    let mut corner = *a;
                    path.points.push(map_pt_fp(corner, fp));
                    corner.x = b.x;
                    path.points.push(map_pt_fp(corner, fp));
                    corner.y = b.y;
                    path.points.push(map_pt_fp(corner, fp));
                    corner.x = a.x;
                    path.points.push(map_pt_fp(corner, fp));
                    path.points.push(map_pt_fp(*a, fp));
                }
                GfxShape::Arc(a, m, b) => {
                    // An arc is a polygon (a path with an aperture width is converted to a convex polygon by FreeRouting): `TransformShapeToPolygon( .., 0,
                    // ARC_HIGH_DEF, ERROR_INSIDE, false )`, its outline.
                    path.polygon = true;
                    path.aperture_width = 0.0;
                    for p in transform_arc_to_polygon(*a, *m, *b, graphic.width, ARC_HIGH_DEF, ErrorLoc::Inside) {
                        path.points.push(map_pt_fp(p, fp));
                    }
                }
                GfxShape::Poly(_) | GfxShape::Bezier(_) => continue,
            }
            outline.shape = Some(ShapeElem::Path(path));
            image.outlines.push(outline);
        }

        // The rule areas of the footprint.
        for zone in &fp.zones {
            if !zone.rule_area {
                continue;
            }
            // Image object coordinates are relative to the image, not absolute board coordinates: the zone is turned back by the footprint's orientation.
            let angle = (-fp.orient).normalize();
            let kind = keepout_kind(zone);
            // The holes go on `m_layerIds[ m_kicadLayer2pcb[ zone->GetLayer() ] ]`: the zone's layer when it has one, else `UNDEFINED_LAYER`, which the map
            // (`std::map::operator[]`) takes for layer 0 of the stack.
            let window_layer = if zone.layers.len() == 1 { self.layer_id_of(board, &zone.layers[0]) } else { self.layer_ids[0].clone() };
            // `layerset.CuStack()`: the copper layers of the zone from the top of the stack down.
            for layer in board.copper.iter().map(|c| &c.name).filter(|c| zone.layers.contains(c)) {
                let mut keepout = Keepout::new(kind);
                let layer_id = self.layer_id_of(board, layer);
                let local = |p: &V| map_pt(rotate_point_about(*p, fp.pos, angle) - fp.pos);
                let mut main = PathElem::new(true);
                main.layer_id = layer_id.clone();
                if let Some(outline) = zone.polys.first() {
                    for p in outline {
                        main.points.push(local(p));
                    }
                    if let Some(first) = outline.first() {
                        main.points.push(local(first));
                    }
                }
                keepout.shape = Some(ShapeElem::Path(main));
                for hole in zone.polys.iter().skip(1) {
                    let mut cutout = PathElem::new(true);
                    cutout.layer_id = window_layer.clone();
                    for p in hole {
                        cutout.points.push(local(p));
                    }
                    if let Some(first) = hole.first() {
                        cutout.points.push(local(first));
                    }
                    keepout.windows.push(Window { shape: Some(ShapeElem::Path(cutout)) });
                }
                image.keepouts.push(keepout);
            }
        }
        image
    }

    /// `FOOTPRINT::GetCourtyard( F_CrtYd )` and the back one, put together and simplified: the contours (outline, then holes, of each polygon) the courtyard is.
    fn courtyard(&self, fp: &Fp) -> Vec<Vec<V>> {
        let max_error = 5000i64; // pcbIUScale.mmToIU( 0.005 )
        let chaining_epsilon = Nm::from_mm(0.02); // pcbIUScale.mmToIU( 0.02 )
        let side = |layer: &str| -> ShapePolySet {
            let shapes: Vec<eda_model::ir::Shape> = fp.gfx.iter().filter(|g| g.layer == layer).map(Gfx::to_ir).collect();
            if shapes.is_empty() {
                return ShapePolySet::new();
            }
            let (mut polys, ok, _) = eda_drc::outline::convert_outline_to_polygon(&shapes, max_error as f64, chaining_epsilon, true);
            if !ok {
                return ShapePolySet::new();
            }
            // Touching courtyards, or courtyards at the clearance distance, are legal: the allowed deviation of an arc.
            polys.inflate(-max_error, CornerStrategy::ChamferAcuteCorners, max_error as i32, false);
            polys
        };
        let mut crt_yd = side("F.CrtYd");
        let back = side("B.CrtYd");
        for i in 0..back.outline_count() {
            crt_yd.add_polygon(back.polygon(i).clone());
        }
        crt_yd.simplify();
        let mut out = Vec::new();
        for i in 0..crt_yd.outline_count() {
            for chain in crt_yd.polygon(i).iter() {
                out.push(chain.iter().map(|p| V::new(p.x, p.y)).collect());
            }
        }
        out
    }

    /// The images and placement of the footprints, and the nets with their pins.
    fn images_components_and_nets(&mut self, board: &BoardView) {
        let highest_net_code = board.nets.iter().map(|(c, _)| *c).max().unwrap_or(0).max(0) as usize;
        // m_nets[netcode] with the pins each net has.
        let mut net_pins: Vec<Vec<PinRef>> = vec![Vec::new(); highest_net_code + 1];

        for fp in &board.footprints {
            let mut image = self.make_image(board, fp);
            let component_id = fp.reference.clone();

            // A net list entry for all the actual pins in the current footprint. The fabricated pin names ("@1") make the pins unique within a footprint.
            for pin in &image.pins {
                let netcode = pin.ki_net_code;
                if netcode > 0 && (netcode as usize) < net_pins.len() {
                    net_pins[netcode as usize].push(PinRef { component_id: component_id.clone(), pin_id: pin.pin_id.clone() });
                }
            }

            let registered = {
                let library = self.pcb.library.as_mut().expect("MakePCB");
                let idx = library.lookup_image(std::mem::take(&mut image));
                library.images[idx].image_id()
            };
            let mut place = Place::new();
            place.rotation = fp.orient.0;
            place.set_vertex(map_pt(fp.pos));
            place.component_id = component_id;
            place.part_number = fp.value.clone();
            // The footprint is flipped from the bottom side: the side is back.
            if fp.flag {
                let angle = (Angle(180.0) - fp.orient).normalize();
                place.rotation = angle.0;
                place.side = Side::Back;
            }
            self.pcb.placement.as_mut().expect("MakePCB").lookup_component(&registered).places.push(place);
        }

        // The padstack set goes to the library, in the order the set keeps them.
        let padstacks: Vec<Padstack> = std::mem::take(&mut self.padstacks).into_values().collect();
        self.pcb.library.as_mut().expect("MakePCB").padstacks = padstacks;

        // The nets with pins go to the network, in the order of their codes.
        for code in 1..net_pins.len() {
            if net_pins[code].is_empty() {
                continue;
            }
            let name = board.net_name(code as i32).to_string();
            self.pcb.network.as_mut().expect("MakePCB").nets.push(Net { net_id: name, pins: std::mem::take(&mut net_pins[code]), ..Default::default() });
        }
    }

    /// The wires of the tracks: consecutive tracks of one net, width and layer that join end to start are one wire.
    fn wires(&mut self, board: &BoardView) {
        let mut path: Option<usize> = None; // index of the current wire
        let mut old_netcode = -1i32;
        let mut old_width = -1i64;
        let mut old_layer = String::new();
        let mut wires: Vec<Wire> = Vec::new();
        for track in &board.tracks {
            let (start, end, width, layer, net_code, locked) = match track {
                TrackItem::Segment { start, end, width, layer, net_code, locked } | TrackItem::Arc { start, end, width, layer, net_code, locked } => (*start, *end, *width, layer, *net_code, *locked),
                TrackItem::Via { .. } => continue,
            };
            if net_code == 0 {
                continue;
            }
            let continues = path.is_some_and(|i| matches!(&wires[i].shape, Some(ShapeElem::Path(p)) if p.points.last() == Some(&map_pt(start))));
            if old_netcode != net_code || old_width != width || old_layer != *layer || !continues {
                old_width = width;
                old_layer = layer.clone();
                old_netcode = net_code;
                let mut wire = Wire::new();
                wire.net_id = board.net_name(net_code).to_string();
                // Tracks with the fix property are not returned in session files.
                wire.wire_type = Some(if locked { WireType::Fix } else { WireType::Route });
                let mut p = PathElem::new(false);
                p.layer_id = self.layer_id_of(board, layer);
                p.aperture_width = scale(width);
                p.points.push(map_pt(start));
                wire.shape = Some(ShapeElem::Path(p));
                wires.push(wire);
                path = Some(wires.len() - 1);
            }
            if let Some(i) = path {
                if let Some(ShapeElem::Path(p)) = &mut wires[i].shape {
                    p.points.push(map_pt(end));
                }
            }
        }
        self.pcb.wiring.as_mut().expect("MakePCB").wires = wires;
    }

    /// The vias the board has, once for each size and drill.
    fn vias(&mut self, board: &BoardView) {
        for track in &board.tracks {
            let TrackItem::Via { pos, size, drill, top, bottom, net_code, locked } = track else { continue };
            if *net_code == 0 {
                continue;
            }
            let index_of = |layer: &str| self.kicad_layers.iter().position(|l| l == layer).unwrap_or(0);
            let (mut top_layer, mut bot_layer) = (index_of(top), index_of(bottom));
            if top_layer > bot_layer {
                std::mem::swap(&mut top_layer, &mut bot_layer);
            }
            let padstack = self.make_via(*size, *drill, top_layer, bot_layer);
            let idx = self.pcb.library.as_mut().expect("MakePCB").lookup_via(padstack);
            let registered_id = self.pcb.library.as_ref().expect("MakePCB").vias[idx].padstack_id.clone();
            let mut via = WireVia::new();
            via.padstack_id = registered_id;
            via.vertexes.push(map_pt(*pos));
            via.net_id = board.net_name(*net_code).to_string();
            // Vias with the fix property are not returned in session files.
            via.via_type = Some(if *locked { WireType::Fix } else { WireType::Route });
            self.pcb.wiring.as_mut().expect("MakePCB").wire_vias.push(via);
        }
    }

    /// `exportNETCLASS`.
    fn export_netclass(&mut self, board: &BoardView, class: &ClassFacts, top_via_layer: usize, bot_via_layer: usize) {
        let mut clazz = Class { class_id: class.name.clone(), ..Default::default() };
        for (code, name) in &board.nets {
            let _ = code;
            if (self.st.class_of_net)(name).name == clazz.class_id {
                clazz.net_ids.push(name.clone());
            }
        }
        let mut rules = Rule::new("rule");
        rules.rules.push(format!("(width {})", super::format::g6(scale(class.track_width))));
        rules.rules.push(format!("(clearance {})", super::format::g6(scale(class.clearance))));
        clazz.rules = Some(rules);
        // Freerouter creates a class named 'default' anyway, and if we try to use that we end up with two 'default' via rules so something else is the name of the default class.
        if class.name == "Default" {
            clazz.class_id = "kicad_default".to_string();
        }
        // The easiest way to get the via name is to create a temporary via and grab its name.
        let via = self.make_via(class.via_diameter, class.via_drill, top_via_layer, bot_via_layer);
        clazz.circuit.push(format!("(use_via \"{}\")", via.padstack_id));
        self.pcb.network.as_mut().expect("MakePCB").classes.push(clazz);
    }
}

/// `ARC_HIGH_DEF`: `pcbIUScale.mmToIU( 0.005 )`.
const ARC_HIGH_DEF: i64 = 5000;
use std::f64::consts::PI;

/// `isRoundKeepout`: a copper-less through hole which needs to be made into a round keepout.
fn is_round_keepout(pad: &Pad) -> bool {
    if pad.shape == PadShapeKind::Circle {
        if pad.drill.x >= pad.size.x {
            return true;
        }
        if !pad.on_copper() {
            return true;
        }
    }
    false
}

/// `TransformShapeToPolygon( aBuffer, UNDEFINED_LAYER, 0, aError, ERROR_INSIDE )` of a custom pad's basic shape (`EDA_SHAPE::TransformShapeToPolygon`), its outlines.
/// A circle's or a ring's, a rectangle's (with its edges drawn as stadiums when it has a width or no fill), an arc's, a segment's, a polygon's (filled and grown by
/// half its width with round corners, or its edges as stadiums), a Bezier's flattened edges. A rounded rectangle's corner radius is not read here: none is rounded.
fn shape_polygons(g: &Gfx, error: i64) -> Vec<Vec<V>> {
    let loc = ErrorLoc::Inside;
    let width = g.width;
    let solid = g.filled;
    let mut out: Vec<Vec<V>> = Vec::new();
    let edges = |pts: &[V], out: &mut Vec<Vec<V>>| {
        for i in 0..pts.len() {
            out.push(oval_polygon(pts[i], pts[(i + 1) % pts.len()], width, error, loc));
        }
    };
    match &g.shape {
        GfxShape::Circle(c, e) => {
            let r = kiround(((e.x - c.x) as f64).hypot((e.y - c.y) as f64)).max(1);
            let inner = r - width / 2;
            if solid || inner <= 0 {
                out.push(circle_polygon(*c, r + width / 2, error, loc));
            } else {
                // `TransformRingToPolygon`: the outer circle and the hole.
                out.push(circle_polygon(*c, inner + width, error, loc));
                out.push(circle_polygon(*c, inner, error, ErrorLoc::Outside));
            }
        }
        GfxShape::Rect(a, b) => {
            // `GetRectCorners`.
            let pts = [*a, V::new(b.x, a.y), *b, V::new(a.x, b.y)];
            if solid {
                out.push(pts.to_vec());
            }
            if width > 0 || !solid {
                edges(&pts, &mut out);
            }
        }
        GfxShape::Arc(a, m, b) => out.push(transform_arc_to_polygon(*a, *m, *b, width, error, loc)),
        GfxShape::Segment(a, b) => out.push(oval_polygon(*a, *b, width, error, loc)),
        GfxShape::Poly(pts) if pts.is_empty() => {}
        GfxShape::Poly(pts) => {
            if solid {
                let mut tmp = ShapePolySet::from_outline(pts.iter().map(|p| eda_clipper2::Point64::new(p.x, p.y)).collect());
                if width > 0 {
                    tmp.inflate(width / 2, CornerStrategy::RoundAllCorners, error as i32, false);
                }
                for i in 0..tmp.outline_count() {
                    out.push(tmp.outline(i).iter().map(|p| V::new(p.x, p.y)).collect());
                }
            } else {
                edges(pts, &mut out);
            }
        }
        GfxShape::Bezier([a, c1, c2, b]) => {
            let p = |v: &V| eda_model::ir::Point { x: Nm::new(v.x), y: Nm::new(v.y) };
            let poly: Vec<V> = eda_model::bezier::bezier_polyline(p(a), p(c1), p(c2), p(b), Nm::new(error)).into_iter().map(|q| V::new(q.x.get(), q.y.get())).collect();
            for w in poly.windows(2) {
                out.push(oval_polygon(w[0], w[1], width, error, loc));
            }
        }
    }
    out
}

/// `BuildConvexHull( polygonal_shape, pad_shape )` of `PAD::MergePrimitivesAsPolygon`, in the pad's own frame: the hull of the anchor (a rectangle, or a circle at the
/// board's `m_MaxError`) and of the outlines of the basic shapes. The hull of their union is the hull of their corners: a corner the union drops is inside it, and a
/// point where two edges cross is on an edge.
fn custom_pad_hull(pad: &Pad, max_error: i64) -> Vec<V> {
    let mut pts: Vec<V> = Vec::new();
    // The anchor pad is always at 0,0.
    if pad.anchor_rect {
        // `SHAPE_RECT( -padSize.x / 2, -padSize.y / 2, padSize.x, padSize.y ).Outline()`.
        let (x0, y0) = (-pad.size.x / 2, -pad.size.y / 2);
        pts.extend([V::new(x0, y0), V::new(x0 + pad.size.x, y0), V::new(x0 + pad.size.x, y0 + pad.size.y), V::new(x0, y0 + pad.size.y)]);
    } else {
        pts.extend(circle_polygon(V::new(0, 0), pad.size.x / 2, max_error, ErrorLoc::Inside));
    }
    for g in &pad.prims {
        for poly in shape_polygons(g, max_error) {
            pts.extend(poly);
        }
    }
    build_convex_hull(&pts)
}
