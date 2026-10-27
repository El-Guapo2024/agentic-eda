//! Builds a [`crate::node::Node`] from this project's own IR
//! (`eda_model::ir::Design` + `ConstraintModel`) -- the "NODE built from
//! the IR" the task's rules require, so the router works on the one JSON
//! source of truth rather than a separate board model.
//!
//! Reuses `eda_drc::board::build`, the exact same flattening DRC already
//! does (pads/tracks/vias to board-space shapes, net resolution, layer
//! names) -- so the router and the DRC engine agree on what a pad/track/via
//! *is*, geometrically, by construction. A copper zone is not
//! inserted as a `NODE` item: `PNS::ITEM::PnsKind` has no `ZONE_T` in real
//! KiCad either (confirmed while reading `pns_item.h`) -- the interactive
//! router routes straight through/over a zone's fill area, which gets
//! recomputed afterward, exactly like real KiCad. A keepout rule area is:
//! `PNS_KICAD_IFACE_BASE::syncZone` adds each triangle of its outline, on each
//! of its copper layers, as a `SOLID` of no net that is not routable
//! ([`add_keepouts`]).
//!
//! The world carries the board's rules ([`crate::rule_resolver::RuleResolver`],
//! `aWorld->SetRuleResolver( m_ruleResolver )`): the clearances the router keeps
//! are the DRC engine's, local clearances, net ties and custom rules included.

use eda_model::units::Nm;
use crate::arcs::ArcPts;
use crate::item::{Arc, Item, Net, Segment, Solid, Via};
use crate::layer::LayerMap;
use crate::node::Node;
use eda_drc::kimath::Shape;
use eda_model::ir::{Design, Point};
use eda_model::ConstraintModel;

/// The id/segment-index pair a `"trk_xxx#3"` flattened id decodes to, the
/// inverse of `eda_drc::board::build`'s own `format!("{}#{i}", t.id)`.
fn split_track_seg_id(flat_id: &str) -> Option<(String, usize)> {
    let (track_id, idx) = flat_id.rsplit_once('#')?;
    Some((track_id.to_string(), idx.parse().ok()?))
}

/// Build the router's world `Node` plus the [`LayerMap`] it was built
/// against (callers need the map to translate back to layer names when
/// emitting `Cmd`s).
pub fn build_node(design: &Design, model: &ConstraintModel) -> (Node, LayerMap) {
    build_node_with(design, model, false)
}

/// [`build_node`] for the length tuner, whose patterns are all in edit (`IN_EDIT`): the tracks of a tuning pattern are not locked in its world, since the
/// tuner is what replaces them (KiCad marks only the pattern being edited that way; every other pattern's members stay locked solids for its meanders --
/// here the tuner treats all of them alike, as it did before locks existed).
pub fn build_node_for_tuning(design: &Design, model: &ConstraintModel) -> (Node, LayerMap) {
    build_node_with(design, model, true)
}

fn build_node_with(design: &Design, model: &ConstraintModel, patterns_in_edit: bool) -> (Node, LayerMap) {
    let layers = LayerMap::new(&model.board.layers);
    let drc_board = eda_drc::board::build(design, model);
    let mut node = Node::new();
    // `aWorld->SetRuleResolver( m_ruleResolver )`: built first, so that each solid carries the number of its board item (`Solid::rule`)
    let names: Vec<String> = (0..layers.count() as i32).map(|i| layers.name_of(i).to_string()).collect();
    let mut rules = crate::rule_resolver::RuleResolver::new(design, model, &drc_board, names);
    // `IsFlashedOnLayer`: the layers the pads and vias that remove their unconnected layers keep (r3 R3-7)
    if eda_connectivity::flash::removes_any_layer(design, model) {
        rules.set_flashed(&eda_connectivity::flash::flashed_layers(design, model));
    }
    // one handle per net (`NET_HANDLE`), as the board has one `NETINFO_ITEM` per net: the rule resolver's cache knows a net by it
    let mut handles: std::collections::HashMap<String, Net> = std::collections::HashMap::new();
    let mut net_of = |name: &str| handles.entry(name.to_string()).or_insert_with(|| crate::item::net_of(name)).clone();
    let locked = locked_ids_with(design, !patterns_in_edit);
    node.set_npth(drc_board.pads.iter().filter(|p| p.kind == eda_model::PadKind::NonPlatedHole).map(|p| p.id.clone()).collect());

    for pad in &drc_board.pads {
        // A padstack: one solid per group of layers that shares a shape, as `PNS_KICAD_IFACE_BASE::syncPad` makes them (`PADSTACK::ForEachUniqueLayer`; the router treats
        // the pad as flashed on all of them and leaves the test of what is connected to the checks). A group whose layers are not next to each other (a custom stack's front
        // shape on the front and on a deeper layer) is a solid for each run of layers.
        if !pad.copper_by_layer.is_empty() {
            let net = if pad.kind == eda_model::PadKind::NonPlatedHole { crate::item::not_a_net() } else { pad.net.as_deref().map(&mut net_of).unwrap_or(None) };
            for (names, shape) in pad.copper_groups() {
                let mut idx: Vec<i32> = names.iter().filter_map(|n| layers.index_of(n)).collect();
                idx.sort_unstable();
                idx.dedup();
                let mut k = 0;
                while k < idx.len() {
                    let mut end = k;
                    while end + 1 < idx.len() && idx[end + 1] == idx[end] + 1 {
                        end += 1;
                    }
                    node.add(Item::Solid(Solid { net: net.clone(), layers: crate::layer::LayerRange::new(idx[k], idx[end]), pos: pad.center, shape: shape.clone(), source: pad.id.clone(), edge: false, rule: rules.rule_of(&pad.id), anchor_points: Vec::new() }));
                    k = end + 1;
                }
            }
            continue;
        }
        let layer_range = pad.layers.iter().filter_map(|n| layers.index_of(n)).fold(None, |acc: Option<crate::layer::LayerRange>, i| {
            let r = crate::layer::LayerRange::single(i);
            Some(match acc {
                Some(mut a) => {
                    a.merge(&r);
                    a
                }
                None => r,
            })
        });
        let layer_range = layer_range.unwrap_or_else(|| layers.all());
        let net = if pad.kind == eda_model::PadKind::NonPlatedHole { crate::item::not_a_net() } else { pad.net.as_deref().map(&mut net_of).unwrap_or(None) };
        node.add(Item::Solid(Solid { net, layers: layer_range, pos: pad.center, shape: pad.copper.clone(), source: pad.id.clone(), edge: false, rule: rules.rule_of(&pad.id), anchor_points: Vec::new() }));
    }

    // An arc track (`PCB_ARC`) is an `ARC` (`PNS_KICAD_IFACE_BASE::syncArc`): one item with the arc's three points, linked at its two ends. (A
    // full circle, whose ends are one point, has no arc to hold; it stays the chords of its polyline.)
    let circle_tracks: std::collections::HashMap<&str, &eda_model::ir::Track> = design.routing.iter().flat_map(|r| r.tracks.iter()).filter(|t| t.arc().is_some_and(|(s, _, e)| s == e)).map(|t| (t.id.as_str(), t)).collect();
    for t in &drc_board.tracks {
        let layer = layers.index_of(&t.layer).unwrap_or(0);
        let source_track = split_track_seg_id(&t.id);
        let net = t.net.as_deref().map(&mut net_of).unwrap_or(None);
        if let Some(mid) = t.arc_mid {
            match source_track.as_ref() {
                Some((id, _)) if circle_tracks.contains_key(id.as_str()) => {
                    let track = circle_tracks[id.as_str()];
                    for (i, w) in track.pts.windows(2).enumerate() {
                        node.add(Item::Segment(Segment { net: net.clone(), layer, a: w[0], b: w[1], width: t.width, source_track: Some((track.id.clone(), i)), locked: locked.contains(track.id.as_str()) }));
                    }
                }
                _ => {
                    let is_locked = source_track.as_ref().is_some_and(|(id, _)| locked.contains(id.as_str()));
                    node.add(Item::Arc(Arc::new(net, layer, t.width, ArcPts::new(t.a, mid, t.b), source_track.map(|(id, _)| id), is_locked)));
                }
            }
            continue;
        }
        let is_locked = source_track.as_ref().is_some_and(|(id, _)| locked.contains(id.as_str()));
        node.add(Item::Segment(Segment { net, layer, a: t.a, b: t.b, width: t.width, source_track, locked: is_locked }));
    }

    for v in &drc_board.vias {
        let layer_range = layers.range_of(&v.from_layer, &v.to_layer);
        node.add(Item::Via(Via { net: v.net.as_deref().map(&mut net_of).unwrap_or(None), layers: layer_range, pos: v.at, diameter: v.diameter, drill: v.drill, source_via: Some(v.id.clone()), locked: locked.contains(v.id.as_str()), is_virtual: false }));
    }

    add_board_outline(&mut node, &drc_board, &layers);
    add_margin(&mut node, design, &layers);
    add_copper_texts(&mut node, design, &layers, &mut rules);
    add_copper_graphics(&mut node, design, model, &layers, &mut rules);
    add_keepouts(&mut node, design, &layers, &mut rules);
    node.set_rule_resolver(rules);
    // `ROUTER::SyncWorld`: `m_world->FixupVirtualVias()`
    node.fixup_virtual_vias();

    (node, layers)
}

/// The ids of the tracks, arcs and vias the router treats as locked (`ITEM::IsLocked`, `MK_LOCKED`: `PNS_KICAD_IFACE_BASE::syncTrack`, `syncArc`, `syncVia`): the
/// ones the board has locked (`BOARD_ITEM::IsLocked`, [`eda_model::ir::DrawingsSection::locked_ids`]), and the tracks of a tuning pattern, which the router leaves
/// alone as long as the pattern is not being edited (`PCB_GENERATOR` members: "`!generator->HasFlag( IN_EDIT )`" marks them locked). A locked item is
/// neither shoved nor dragged -- the router goes round it as it does round a pad.
pub fn locked_ids(design: &Design) -> std::collections::HashSet<&str> {
    locked_ids_with(design, true)
}

/// [`locked_ids`], with or without the members of the tuning patterns (`!generator->HasFlag( IN_EDIT )`).
pub fn locked_ids_with(design: &Design, lock_pattern_members: bool) -> std::collections::HashSet<&str> {
    let Some(d) = design.drawings.as_ref() else { return std::collections::HashSet::new() };
    let members = d.tuning_patterns.iter().flat_map(|p| p.member_ids.iter().map(String::as_str)).filter(|_| lock_pattern_members);
    d.locked_ids.iter().map(String::as_str).chain(members).collect()
}

/// The board outline as router obstacles: `PNS_KICAD_IFACE_BASE::syncGraphicalItem` for each graphic on Edge.Cuts. Every
/// side of every Edge.Cuts item -- the outline polygon's, an arc's chords, a circle's or a rectangle's ring, a curve's flattening
/// (`MakeEffectiveShapes`) -- becomes a `SOLID` on every copper layer, with no net (it collides with everything, whatever the
/// net), not routable, and a zero-width shape (`SetWidth( 0 )` for an Edge.Cuts `SH_SEGMENT`/`SH_ARC`), so a track or via keeps the
/// board's copper-to-edge clearance from it (`Node::clearance_to`) and a shove cannot push copper past it. A cutout is an obstacle
/// like the outer edge: copper keeps the clearance from the hole's rim too.
fn add_board_outline(node: &mut Node, board: &eda_drc::board::DrcBoard, layers: &LayerMap) {
    add_edge_solids(node, board.edge_cuts.iter(), layers, "edge:", false);
}

/// The solids of edge graphics (Edge.Cuts, Margin) as [`add_board_outline`] describes them, their sources `<prefix><n>`.
/// `with_width`: the shape keeps half its width round its line (Margin); an Edge.Cuts one is given none (`SetWidth( 0 )`).
fn add_edge_solids<'a>(node: &mut Node, shapes: impl Iterator<Item = &'a eda_model::ir::Shape>, layers: &LayerMap, prefix: &str, with_width: bool) {
    let mut i = 0usize;
    for shape in shapes {
        // KiCad keeps an Edge.Cuts arc a `SHAPE_ARC`; the chords here lie up to the flattening error inside it, so copper on the arc's
        // outer side (a cutout's rim, a notch) could come that much nearer the real arc than the clearance. A flattened side is
        // therefore given the error as its radius: the band holds the true curve, and the clearance is kept from all of it.
        let curved = matches!(shape, eda_model::ir::Shape::Arc { .. } | eda_model::ir::Shape::Circle { .. } | eda_model::ir::Shape::Bezier { .. });
        let band = if curved { eda_model::outline::MAX_ERROR_NM } else { Nm::ZERO } + if with_width { shape.stroke_width() / 2 } else { Nm::ZERO };
        for chain in eda_model::outline::shape_chains(shape, eda_model::outline::MAX_ERROR_NM.as_f64()) {
            let n = chain.pts.len();
            let sides = if chain.closed { n } else { n.saturating_sub(1) };
            for k in 0..sides {
                let (a, b) = (chain.pts[k], chain.pts[(k + 1) % n]);
                let index = i;
                i += 1;
                if a == b {
                    continue;
                }
                let mid = Point { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
                node.add(Item::Solid(Solid { net: crate::item::not_a_net(), layers: layers.all(), pos: mid, shape: Shape::Stadium { a, b, r: band }, source: format!("{prefix}{index}"), edge: true, rule: 0, anchor_points: Vec::new() }));
            }
        }
    }
}

/// The board's graphics on the Margin layer as board-edge obstacles (r3 R3-6): `syncGraphicalItem`'s `aItem->GetLayer() == Edge_Cuts ||
/// aItem->GetLayer() == Margin` -- a `SOLID` on every copper layer, not routable -- and `isEdge`'s `parent->IsOnLayer( Edge_Cuts ) ||
/// parent->IsOnLayer( Margin )`, so copper keeps the copper-to-edge clearance from it (`CT_EDGE_CLEARANCE`), as KiCad's DRC does
/// (`drc_test_provider_edge_clearance.cpp`: `LSET( { Edge_Cuts, Margin } )`). The shape keeps its width (`SetWidth( 0 )` is for Edge.Cuts only), so
/// the clearance is kept from its stroke's edge, as kicad-cli measures it. **Differs:** a footprint's own Margin graphics are not read; to the rules
/// the solid is the board edge's item (on Edge.Cuts).
fn add_margin(node: &mut Node, design: &Design, layers: &LayerMap) {
    let Some(d) = design.drawings.as_ref() else { return };
    add_edge_solids(node, d.shapes.iter().filter(|s| s.layer() == "Margin"), layers, "margin:", true);
}

/// `PNS_KICAD_IFACE_BASE::syncZone` for each keepout rule area (`!aZone->GetIsRuleArea() || !aZone->HasKeepoutParametersSet()` returns at
/// once): every triangle of the outline with its holes (`poly->CacheTriangulation()`, [`keepout_triangles`]), on **each** copper layer of the
/// area's set (`LSET layers = aZone->GetLayerSet(); for( PCB_LAYER_ID layer : LAYER_RANGE( F_Cu, B_Cu, .. ) ) { if( !layers[ layer ] ) continue;
/// .. }`: a keepout on F.Cu and B.Cu, or on `*.Cu`, is an obstacle on every one of them), a `SOLID` with no net (`SetNet( nullptr )`, here
/// [`crate::item::not_a_net`]), its parent the zone, not routable. What it keeps out, and how (`collideSimple`'s `IsKeepout`: an exact
/// boundary), is [`crate::rule_resolver::Keepout`]'s and [`Node::item_collision`]'s. An outline that cannot be triangulated
/// (`!poly->IsTriangulationUpToDate()`) is left out of the world, and the router says so: "%s is malformed." (with
/// `GetItemDescription( .., true )`), "This zone cannot be handled by the router.\nPlease verify it is not a self-intersecting polygon."
/// ([`crate::rule_resolver::RuleResolver::warnings`]; KiCad shows it in a dialog).
fn add_keepouts(node: &mut Node, design: &Design, layers: &LayerMap, rules: &mut crate::rule_resolver::RuleResolver) {
    let Some(rt) = design.routing.as_ref() else { return };
    for z in rt.zones.iter().filter(|z| z.is_rule_area && (z.keepout_tracks || z.keepout_vias || z.keepout_pads || z.keepout_copper_pour || z.keepout_footprints)) {
        let mut on: Vec<i32> = z.copper_layers().into_iter().filter_map(|l| layers.index_of(l)).collect();
        on.sort_unstable();
        on.dedup();
        if on.is_empty() || z.outline.len() < 3 {
            continue;
        }
        let Some(pieces) = keepout_triangles(&z.outline, &z.holes) else {
            rules.warn(format!("{} is malformed.\nThis zone cannot be handled by the router.\nPlease verify it is not a self-intersecting polygon.", rule_area_description(z, layers)));
            continue;
        };
        let rule = rules.rule_of(&format!("{}{}", Solid::KEEPOUT_SOURCE, z.id));
        let mut k = 0usize;
        for &layer in &on {
            for pts in &pieces {
                let n = pts.len() as i64;
                let pos = Point { x: pts.iter().map(|p| p.x).sum::<Nm>() / n, y: pts.iter().map(|p| p.y).sum::<Nm>() / n };
                node.add(Item::Solid(Solid { net: crate::item::not_a_net(), layers: crate::layer::LayerRange::single(layer), pos, shape: Shape::Polygon { pts: pts.clone() }, source: format!("{}{}#{k}", Solid::KEEPOUT_SOURCE, z.id), edge: false, rule, anchor_points: Vec::new() }));
                k += 1;
            }
        }
    }
}

/// `SHAPE_POLY_SET::CacheTriangulation` of a rule area's outline and its holes: the triangles of `TesselatePolygon` ([`crate::triangulation::tesselate`])
/// of the outline, which leave the holes open -- a route may run in a keepout's cutout. An outline with holes is first fractured (`flatSet.Fracture()`,
/// [`eda_shape_poly_set::ShapePolySet::fracture`]: each hole joined to the outline by a bridge) and the one chain that makes is cut into ears. `None`
/// when the ear cutting does not finish (`!IsTriangulationUpToDate()`). **Differs:** KiCad first tries `TesselatePolygon( const POLYGON& )`, whose
/// `eliminateHoles` bridges each hole to the outline itself, and fractures only when that fails; the triangles differ, the area they cover does not.
fn keepout_triangles(outline: &[Point], holes: &[Vec<Point>]) -> Option<Vec<Vec<Point>>> {
    let holes: Vec<&Vec<Point>> = holes.iter().filter(|h| h.len() >= 3).collect();
    let chains: Vec<Vec<Point>> = if holes.is_empty() {
        vec![outline.to_vec()]
    } else {
        use eda_shape_poly_set::ShapePolySet;
        let to64 = |c: &[Point]| c.iter().map(|p| eda_clipper2::Point64::new(p.x.get(), p.y.get())).collect::<Vec<_>>();
        let mut set = ShapePolySet::default();
        let o = set.add_outline(to64(outline));
        for h in holes {
            set.add_hole(to64(h), Some(o));
        }
        set.fracture(true);
        set.polys.iter().filter_map(|poly| poly.first()).map(|c| c.iter().map(|q| Point { x: Nm::new(q.x), y: Nm::new(q.y) }).collect()).collect()
    };
    let mut out = Vec::new();
    for c in chains {
        out.extend(crate::triangulation::tesselate(&c)?.into_iter().map(|t| t.to_vec()));
    }
    Some(out)
}

/// `ZONE::GetItemDescription( .., true )` of a rule area: "Rule Area on %s" or "Rule area '%s' on %s", its layers in the board's order ("on %s",
/// "on %s and %s", "on %s, %s and %s", "on %s, %s and %zu more").
fn rule_area_description(z: &eda_model::ir::Zone, layers: &LayerMap) -> String {
    let mut set: Vec<&str> = z.layer_set();
    // `m_layerSet.Seq()`: the copper layers in stack order, then the rest
    set.sort_by_key(|l| (layers.index_of(l).unwrap_or(i32::MAX), l.to_string()));
    let layer_desc = match set.as_slice() {
        [a] => format!("on {a}"),
        [a, b] => format!("on {a} and {b}"),
        [a, b, c] => format!("on {a}, {b} and {c}"),
        [a, b, rest @ ..] => format!("on {a}, {b} and {} more", rest.len()),
        [] => String::new(),
    };
    if z.name.is_empty() {
        format!("Rule Area {layer_desc}")
    } else {
        format!("Rule area '{}' {layer_desc}", z.name)
    }
}

/// `GetMaxError()` of a board this IR does not say otherwise of: `ARC_HIGH_DEF`, 0.005 mm.
const TEXT_MAX_ERROR_NM: eda_model::ir::Nm = Nm::new(5_000);

/// The board's copper text as router obstacles: `PNS_KICAD_IFACE_BASE::syncTextItem` for each `PCB_TEXT` on a copper layer
/// (`syncGraphicalItem`'s `PCB_TEXT_T` case) -- a `SOLID` on that one layer, with no net, not routable ([`Solid::is_text`]), whose shape is
/// `TransformShapeToPolygon( .., 0, GetMaxError(), ERROR_OUTSIDE )`: the text's bounding hull (`buildBoundingHull`), the box of its glyphs
/// as drawn (each stroke and its pen, and `aMaxError` more for `ERROR_OUTSIDE`) in the text's own frame, turned by its draw rotation about its
/// position. A track keeps the ordinary copper clearance from it, as from a pad of no net. The texts: the board's imported copper texts
/// (`DrawingsSection::copper_texts`, with their layout) and a plain [`eda_model::ir::Text`] put on a copper layer in the studio. A hidden text
/// is not imported as copper (`IsVisible()`). Not here: a footprint's own copper texts (KiCad syncs its reference, value, fields and texts on
/// copper too), a table (`PCB_TABLE`), a knockout text's margin, and a text box (`gr_text_box` is not read by the importer).
fn add_copper_texts(node: &mut Node, design: &Design, layers: &LayerMap, rules: &mut crate::rule_resolver::RuleResolver) {
    let Some(d) = design.drawings.as_ref() else { return };
    let mut add = |id: &str, layer: &str, at: Point, rot_deg: f64, segs: &[eda_drc::kimath::Seg], r: eda_model::ir::Nm| {
        let (Some(layer), false) = (layers.index_of(layer), segs.is_empty()) else { return };
        let hull = text_bounding_hull(segs, r + TEXT_MAX_ERROR_NM, at, rot_deg);
        let source = format!("{}{id}", Solid::TEXT_SOURCE);
        let rule = rules.add_text(&source, layer);
        node.add(Item::Solid(Solid { net: crate::item::not_a_net(), layers: crate::layer::LayerRange::single(layer), pos: at, shape: Shape::Polygon { pts: hull }, source, edge: false, rule, anchor_points: Vec::new() }));
    };
    for (i, t) in d.copper_texts.iter().enumerate() {
        if let Some(Shape::Strokes { segs, r }) = eda_drc::board::footprint_text_shape(t) {
            add(&format!("copper{i}"), &t.layer, t.at, eda_drc::board::footprint_text_draw_rotation(t), &segs, r);
        }
    }
    for t in d.texts.iter().filter(|t| t.layer.ends_with(".Cu")) {
        // laid out as the silkscreen check lays out a plain text (`eda_drc::board::build`): square glyphs, vertically centred
        let size = (t.size_nm, t.size_nm);
        let pen = eda_drc::stroke_font::effective_pen_width(t.stroke_width, size, false);
        let halign = match t.justify {
            eda_model::ir::TextJustify::Left => -1,
            eda_model::ir::TextJustify::Center => 0,
            eda_model::ir::TextJustify::Right => 1,
        };
        let rot = (t.angle as f64 / 1000.0).rem_euclid(360.0);
        let segs = eda_drc::stroke_font::kicad_text_segments(&eda_drc::stroke_font::TextLayout { text: &t.content, pos: t.at, size, thickness: t.stroke_width, angle_deg: rot, mirror: t.mirror, halign, valign: 0 });
        add(&t.id, &t.layer, t.at, rot, &segs, pen / 2);
    }
}

/// `PCB_SHAPE::GetConnectionPoints()`: where a route joins a graphic on copper -- a filled shape's centre (`IsSolidFill()`: `GetCenter()`), a
/// circle's four quadrant points (`KIGEOM::GetCircleKeyPoints( .., false )`), an arc's middle and ends, a line's or a curve's ends, a polygon's
/// corners, a rectangle's four corners.
fn connection_points(shape: &eda_model::ir::Shape) -> Vec<Point> {
    use eda_model::ir::Shape as S;
    let bbox_centre = |pts: &[Point]| {
        let (x0, x1) = (pts.iter().map(|p| p.x).min().unwrap_or(Nm::ZERO), pts.iter().map(|p| p.x).max().unwrap_or(Nm::ZERO));
        let (y0, y1) = (pts.iter().map(|p| p.y).min().unwrap_or(Nm::ZERO), pts.iter().map(|p| p.y).max().unwrap_or(Nm::ZERO));
        Point { x: (x0 + x1) / 2, y: (y0 + y1) / 2 }
    };
    match shape {
        S::Circle { center, filled: true, .. } => vec![*center],
        S::Rect { start, end, filled: true, .. } => vec![bbox_centre(&[*start, *end])],
        S::Polygon { pts, filled: true, .. } if pts.len() >= 3 => vec![bbox_centre(pts)],
        S::Circle { center, end, .. } => {
            let r = Nm::kiround(((end.x - center.x).as_f64()).hypot((end.y - center.y).as_f64()));
            vec![Point { x: center.x, y: center.y + r }, Point { x: center.x + r, y: center.y }, Point { x: center.x, y: center.y - r }, Point { x: center.x - r, y: center.y }]
        }
        S::Arc { start, mid, end, .. } => vec![*mid, *start, *end],
        S::Segment { start, end, .. } | S::Bezier { start, end, .. } => vec![*start, *end],
        S::Polygon { pts, .. } => pts.clone(),
        S::Rect { start, end, .. } => vec![*start, Point { x: end.x, y: start.y }, *end, Point { x: start.x, y: end.y }],
    }
}

/// The graphics on copper as router obstacles (r3 R3-2), as `PNS_KICAD_IFACE_BASE::SyncWorld` adds them from `m_board->Drawings()` and each
/// footprint's `GraphicalItems()`:
/// - a shape (`PCB_SHAPE_T`, `syncGraphicalItem`): one `SOLID` per effective shape (`MakeEffectiveShapes`, [`eda_drc::board::effective_shapes`]: a
///   filled outline and its stroked edge, a line, an arc's chords) on its layer, of the shape's net (`SetNet( aItem->GetNet() )`: a board shape's
///   net, none for a footprint's), routable, with the shape's connection points as its anchors ([`connection_points`]): a route of another net (or
///   of none) keeps the clearance from it, one of its net may start or end on it. A footprint's shape is its footprint's, so the bridge of a net tie
///   is excused where it meets a pad of the route's net ([`crate::rule_resolver::RuleResolver::is_net_tie_exclusion`]);
/// - a text box (`PCB_TEXTBOX_T`, `syncGraphicalItem` of it as a `PCB_SHAPE`): its rectangle's border at the stroke's width (with the border off
///   too), routable, of no net -- its text is not an obstacle;
/// - a dimension (`syncDimension`): its lines at their width, and its text when it shows, not routable, of no net;
/// - a barcode (`syncBarcode`: `GetBoundingHull`): the box of its symbol turned by its angle, not routable, of no net;
/// - a table (`syncTextItem` of a `PCB_TABLE`: its cells' `TransformShapeToPolygon` with half the border's width, simplified to `Outline( 0 )`):
///   the table's box grown by half its widest rule, not routable, of no net;
/// - a footprint's own text on copper (`syncTextItem` of its reference, value, fields and texts): its bounding hull, as a board text's.
///
/// **Differs:** a dimension's and a barcode's text is the box of its glyphs, not the glyphs (`syncDimension` adds each glyph outline); a
/// barcode's printed text is left out of its hull; a rounded rectangle on copper is its square corners; the line style (dashes) of a shape is
/// not read; an arc or a circle's ring is its chords with the flattening error (5 um) added to the stroke, where KiCad has one `SHAPE_ARC`.
/// A hidden board text and a dimension whose text is hidden are not in the model (the importer leaves them out, so the board kicad-cli checks
/// has none either); a footprint's hidden text is a hidden field, which `syncTextItem` leaves out too.
fn add_copper_graphics(node: &mut Node, design: &Design, model: &ConstraintModel, layers: &LayerMap, rules: &mut crate::rule_resolver::RuleResolver) {
    use eda_drc::rule_item::Class;
    let copper = |l: &str| layers.index_of(l).filter(|_| l.ends_with(".Cu"));
    let add_shape = |node: &mut Node, rules: &mut crate::rule_resolver::RuleResolver, id: &str, shape: &eda_model::ir::Shape, net: Option<&str>, footprint: Option<&str>| {
        let Some(layer) = copper(shape.layer()) else { return };
        let source = format!("{}{id}", Solid::GRAPHIC_SOURCE);
        let rule = rules.add_graphic(&source, layer, net, footprint, Class::Shape);
        let anchors = connection_points(shape);
        let pos = anchors.first().copied().unwrap_or(Point { x: Nm::ZERO, y: Nm::ZERO });
        let net = net.and_then(crate::item::net_of);
        // KiCad keeps an arc and an unfilled circle's ring one `SHAPE_ARC` (`makeEffectiveShapes`); its chords here lie up to the flattening error
        // inside it, so, as for an Edge.Cuts arc ([`add_edge_solids`]), the stroke is given that error more: the band holds the true curve (r4 R4-6)
        let curved = matches!(shape, eda_model::ir::Shape::Arc { .. } | eda_model::ir::Shape::Circle { .. });
        for (k, piece) in eda_drc::board::effective_shapes(shape).into_iter().enumerate() {
            let piece = match piece {
                Shape::Strokes { segs, r } if curved => Shape::Strokes { segs, r: r + eda_model::outline::MAX_ERROR_NM },
                piece => piece,
            };
            node.add(Item::Solid(Solid { net: net.clone(), layers: crate::layer::LayerRange::single(layer), pos, shape: piece, source: format!("{source}#{k}"), edge: false, rule, anchor_points: anchors.clone() }));
        }
    };
    let add_obstacle = |node: &mut Node, rules: &mut crate::rule_resolver::RuleResolver, id: &str, layer: i32, pieces: Vec<Shape>, class: Class| {
        let source = format!("{}{id}", Solid::OBSTACLE_SOURCE);
        let rule = rules.add_graphic(&source, layer, None, None, class);
        for (k, piece) in pieces.into_iter().enumerate() {
            let (x0, y0, x1, y1) = piece.bbox(Nm::ZERO);
            let pos = Point { x: (x0 + x1) / 2, y: (y0 + y1) / 2 };
            node.add(Item::Solid(Solid { net: crate::item::not_a_net(), layers: crate::layer::LayerRange::single(layer), pos, shape: piece, source: format!("{source}#{k}"), edge: false, rule, anchor_points: Vec::new() }));
        }
    };
    if let Some(d) = design.drawings.as_ref() {
        for (i, shape) in d.shapes.iter().enumerate() {
            let id = if shape.id().is_empty() { format!("shape{i}") } else { shape.id().to_string() };
            add_shape(node, rules, &id, shape, d.board_items.net_of(shape.id()), None);
        }
        // a text box: `syncGraphicalItem( static_cast<PCB_SHAPE*>( gitem ) )`, its rectangle (`PCB_SHAPE` part); not filled, its border at the
        // stroke's width whether the border is drawn or not (`makeEffectiveShapes` takes `GetEffectiveWidth()`, the stroke's; `m_borderEnabled` is
        // read only by the text box's own drawing and polygon code, r4 R4-6)
        for (i, tb) in d.board_items.text_boxes.iter().enumerate() {
            if copper(&tb.layer).is_none() {
                continue;
            }
            let corners = if tb.pts.len() == 4 { tb.pts.clone() } else { vec![tb.start, Point { x: tb.end.x, y: tb.start.y }, tb.end, Point { x: tb.start.x, y: tb.end.y }] };
            let id = if tb.id.is_empty() { format!("textbox{i}") } else { tb.id.clone() };
            let shape = eda_model::ir::Shape::Polygon { id: id.clone(), layer: tb.layer.clone(), stroke_width: tb.stroke_width_nm.max(Nm::ZERO), filled: false, pts: corners };
            add_shape(node, rules, &id, &shape, None, None);
        }
        for (i, dim) in d.dimensions.iter().enumerate() {
            let Some(layer) = copper(&dim.layer) else { continue };
            let g = eda_connectivity::dimension::compute_dimension_geometry(dim);
            let r = dim.stroke_width / 2 + TEXT_MAX_ERROR_NM;
            let mut pieces: Vec<Shape> = g.lines.iter().filter(|(a, b)| a != b).map(|&(a, b)| Shape::Stadium { a, b, r }).collect();
            if !g.text.is_empty() {
                let size = (dim.text_size_nm, dim.text_size_nm);
                let thickness = dim.text_thickness_nm.unwrap_or_else(|| Nm::kiround(dim.text_size_nm.as_f64() * 0.15));
                let rot = (g.text_angle as f64 / 1000.0).rem_euclid(360.0);
                let segs = eda_drc::stroke_font::kicad_text_segments(&eda_drc::stroke_font::TextLayout { text: &g.text, pos: g.text_at, size, thickness, angle_deg: rot, mirror: false, halign: 0, valign: 0 });
                if !segs.is_empty() {
                    pieces.push(Shape::Polygon { pts: text_bounding_hull(&segs, thickness / 2 + TEXT_MAX_ERROR_NM, g.text_at, rot) });
                }
            }
            let id = if dim.id.is_empty() { format!("dimension{i}") } else { dim.id.clone() };
            add_obstacle(node, rules, &id, layer, pieces, Class::Dimension);
        }
        for (i, b) in d.barcodes.iter().enumerate() {
            let Some(layer) = copper(&b.layer) else { continue };
            let (hw, hh) = (b.width_nm.as_f64() / 2.0, b.height_nm.as_f64() / 2.0);
            // `RotatePoint( corner, m_pos, m_angle )`: counter-clockwise on the screen, y down
            let (sin, cos) = (b.angle as f64 / 1000.0).to_radians().sin_cos();
            let pts = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)].iter().map(|&(x, y)| Point { x: b.at.x + Nm::kiround(x * cos + y * sin), y: b.at.y + Nm::kiround(-x * sin + y * cos) }).collect();
            let id = if b.id.is_empty() { format!("barcode{i}") } else { b.id.clone() };
            add_obstacle(node, rules, &id, layer, vec![Shape::Polygon { pts }], Class::Shape);
        }
        for (i, t) in d.tables.iter().enumerate() {
            let Some(layer) = copper(&t.layer) else { continue };
            let (w, h) = t.local_size();
            let corners: Vec<Point> = [(Nm::ZERO, Nm::ZERO), (w, Nm::ZERO), (w, h), (Nm::ZERO, h)].iter().map(|&c| t.to_world(c)).collect();
            // `if( StrokeColumns() || StrokeRows() ) gap = std::max( gap, .. GetSeparatorsStroke().GetWidth() / 2 ); if( StrokeExternal() || StrokeHeaderSeparator() ) ..`
            let sep = if t.separators.rows || t.separators.cols { t.separators.stroke.width_nm / 2 } else { Nm::ZERO };
            let border = if t.border.external || t.border.header { t.border.stroke.width_nm / 2 } else { Nm::ZERO };
            let gap = sep.max(border);
            let ring: Vec<eda_drc::kimath::Seg> = (0..4).map(|k| eda_drc::kimath::Seg::new(corners[k], corners[(k + 1) % 4])).collect();
            let shape = if gap > Nm::ZERO { Shape::Compound { parts: vec![Shape::Polygon { pts: corners.clone() }, Shape::Strokes { segs: ring, r: gap }] } } else { Shape::Polygon { pts: corners } };
            let id = if t.id.is_empty() { format!("table{i}") } else { t.id.clone() };
            add_obstacle(node, rules, &id, layer, vec![shape], Class::Shape);
        }
    }
    for (i, (fp, piece)) in eda_drc::fp_art::footprint_copper_of(design, model).into_iter().enumerate() {
        match piece {
            eda_drc::fp_art::FpCopper::Shape(shape) => add_shape(node, rules, &format!("{fp}:{i}"), &shape, None, Some(&fp)),
            eda_drc::fp_art::FpCopper::Text(t) => {
                let (Some(layer), Some(Shape::Strokes { segs, r })) = (copper(&t.layer), eda_drc::board::footprint_text_shape(&t)) else { continue };
                if segs.is_empty() {
                    continue;
                }
                let hull = text_bounding_hull(&segs, r + TEXT_MAX_ERROR_NM, t.at, eda_drc::board::footprint_text_draw_rotation(&t));
                let source = format!("{}{fp}:{i}", Solid::TEXT_SOURCE);
                let rule = rules.add_text(&source, layer);
                node.add(Item::Solid(Solid { net: crate::item::not_a_net(), layers: crate::layer::LayerRange::single(layer), pos: t.at, shape: Shape::Polygon { pts: hull }, source, edge: false, rule, anchor_points: Vec::new() }));
            }
        }
    }
}

/// `PCB_TEXT::buildBoundingHull`: the glyph strokes turned back by the draw rotation about `at` (`poly.Rotate( -GetDrawRotation(), GetDrawPos() )`),
/// their box grown by `r` (half the pen, and the error), its four corners turned forward again (`RotatePoint( corner, GetDrawPos(), GetDrawRotation() )`).
/// KiCad's rotation is counter-clockwise on screen, y down.
fn text_bounding_hull(segs: &[eda_drc::kimath::Seg], r: eda_model::ir::Nm, at: Point, rot_deg: f64) -> Vec<Point> {
    let (sin, cos) = rot_deg.to_radians().sin_cos();
    // `RotatePoint( p, centre, angle )` with y down: x' = x cos + y sin, y' = -x sin + y cos (relative to the centre)
    let turn = |p: (f64, f64), s: f64| (p.0 * cos + p.1 * s, -p.0 * s + p.1 * cos);
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in segs.iter().flat_map(|s| [s.a, s.b]) {
        let q = turn(((p.x - at.x).as_f64(), (p.y - at.y).as_f64()), -sin);
        (x0, y0, x1, y1) = (x0.min(q.0), y0.min(q.1), x1.max(q.0), y1.max(q.1));
    }
    let r = r.as_f64();
    [(x0 - r, y0 - r), (x1 + r, y0 - r), (x1 + r, y1 + r), (x0 - r, y1 + r)]
        .into_iter()
        .map(|c| {
            let q = turn(c, sin);
            Point { x: at.x + Nm::kiround(q.0), y: at.y + Nm::kiround(q.1) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use eda_model::units::mm;
    use super::*;
    use crate::item::net_of;
    use eda_model::ir::{Design, FootprintInstance, LabelSide, PlacementSection, Point, Provenance, RoutingSection, Side, Track};
    use eda_model::{ConstraintModel, Part, Pin, PinKind};

    fn two_pad_board() -> (Design, ConstraintModel) {
        let part_a = Part { reference: "U1".into(), mpn: None, lcsc: None, value: None, package: Some("0603".into()), footprint: Some("0603".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }, Pin { number: "2".into(), name: None, kind: PinKind::Passive }], body_nm: None, symbol: None, datasheet: None, edge: None };
        let model = ConstraintModel { parts: vec![part_a.clone()], ..Default::default() };
        let fp = FootprintInstance { id: "U1".into(), at: Point { x: Nm::ZERO, y: Nm::ZERO }, rot: 0, side: Side::Top, label: LabelSide::Above };
        let design = Design {
            footprint_library: None, sheet_contents: None, bus_aliases: vec![], symbol_library: None,
            schema: eda_model::ir::SCHEMA_VERSION,
            provenance: Provenance { engine_version: "test".into(), intent_hash: String::new(), seed: 0, stage_hashes: vec![] },
            schematic: None,
            nets: None,
            placement: Some(PlacementSection { outline: vec![], footprints: vec![fp], modules: vec![] }),
            routing: Some(RoutingSection { tracks: vec![Track { id: "trk1".into(), net: "N1".into(), pins: vec![], layer: "F.Cu".into(), width: Nm::new(200), pts: vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::new(1000), y: Nm::ZERO }, Point { x: Nm::new(1000), y: Nm::new(1000) }], arc_mid_offset: None }], vias: vec![], zones: vec![], track_width_presets: vec![], via_presets: vec![], diff_pair_presets: vec![], teardrop_settings: Default::default() }),
            drawings: None,
        };
        (design, model)
    }

    /// A through-hole pad whose copper differs from layer to layer, on a four-layer board.
    fn padstack_board(side: Side) -> (Design, ConstraintModel) {
        use eda_model::padstack::{PadLayerShape, PadStack, PadStackMode};
        use eda_model::{BoardRules, Footprint, Pad, PadKind, PadShape};
        let part = Part { reference: "J1".into(), mpn: None, lcsc: None, value: None, package: Some("TH1".into()), footprint: Some("TH1".into()), pins: vec![Pin { number: "1".into(), name: None, kind: PinKind::Passive }], body_nm: None, symbol: None, datasheet: None, edge: None };
        let mut pad = Pad { number: "1".into(), at: (Nm::ZERO, Nm::ZERO), size: (Nm::new(1600), Nm::new(1600)), shape: PadShape::Circle, kind: PadKind::ThroughHole, drill: Some(Nm::new(800)), drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext: Default::default() };
        pad.ext.stack = PadStack {
            mode: PadStackMode::FrontInnerBack,
            inner: Some(PadLayerShape::plain(PadShape::Rect, (Nm::new(1200), Nm::new(1200)))),
            back: Some(PadLayerShape::plain(PadShape::Oval, (Nm::new(2400), Nm::new(1000)))),
            ..Default::default()
        };
        let model = ConstraintModel {
            parts: vec![part],
            footprints: vec![Footprint { name: "TH1".into(), pads: vec![pad], courtyard: Some((Nm::new(2000), Nm::new(2000))), model: None, courtyard_outlines: vec![], models3d: vec![], art: Default::default() }],
            board: BoardRules { layers: vec!["F.Cu".into(), "In1.Cu".into(), "In2.Cu".into(), "B.Cu".into()], ..Default::default() },
            ..Default::default()
        };
        let (mut design, _) = two_pad_board();
        design.placement = Some(PlacementSection { outline: vec![], footprints: vec![FootprintInstance { id: "J1".into(), at: Point { x: mm(0.01), y: mm(0.01) }, rot: 0, side, label: LabelSide::Above }], modules: vec![] });
        design.routing = None;
        (design, model)
    }

    /// `(first layer, last layer, bounding box)` of each solid of the board's one pad, in layer order.
    fn pad_solids(node: &Node) -> Vec<(i32, i32, (Nm, Nm, Nm, Nm))> {
        let mut v: Vec<(i32, i32, (Nm, Nm, Nm, Nm))> = node.iter().filter_map(|(_, it)| match it { Item::Solid(s) if !s.edge => Some((s.layers.start(), s.layers.end(), s.shape.bbox(Nm::ZERO))), _ => None }).collect();
        v.sort();
        v
    }

    #[test]
    fn a_padstack_is_a_solid_for_each_group_of_layers_that_shares_a_shape() {
        let (design, model) = padstack_board(Side::Top);
        let (node, _) = build_node(&design, &model);
        // Front: the circle of 1.6 mm; the two inner layers one rectangle of 1.2 mm; the back the 2.4 x 1.0 mm oval.
        assert_eq!(pad_solids(&node), vec![(0, 0, (Nm::new(9200), Nm::new(9200), Nm::new(10_800), Nm::new(10_800))), (1, 2, (Nm::new(9400), Nm::new(9400), Nm::new(10_600), Nm::new(10_600))), (3, 3, (Nm::new(8800), Nm::new(9500), Nm::new(11_200), Nm::new(10_500)))]);
        // A footprint on the bottom has them the other way round: the oval on the board's front, the circle on its back.
        let (design, model) = padstack_board(Side::Bottom);
        let (node, _) = build_node(&design, &model);
        assert_eq!(pad_solids(&node), vec![(0, 0, (Nm::new(8800), Nm::new(9500), Nm::new(11_200), Nm::new(10_500))), (1, 2, (Nm::new(9400), Nm::new(9400), Nm::new(10_600), Nm::new(10_600))), (3, 3, (Nm::new(9200), Nm::new(9200), Nm::new(10_800), Nm::new(10_800)))]);
    }

    /// `syncTextItem`: a board text on copper is a solid of no net on its layer, the box of its glyphs turned with the text; nothing joins to
    /// it, and a route keeps the copper clearance from it.
    #[test]
    fn a_copper_text_is_an_obstacle_on_its_layer() {
        use eda_model::ir::{DrawingsSection, FootprintText};
        let (mut design, model) = two_pad_board();
        let text = |angle: i64, layer: &str| FootprintText { text: "AB".into(), layer: layer.into(), at: Point { x: mm(0.02), y: mm(0.02) }, angle_file_mdeg: angle, size: (Nm::new(1000), Nm::new(1000)), thickness: Nm::new(150), halign: 0, valign: 0, mirror: false, keep_upright: false, bold: false };
        design.drawings = Some(DrawingsSection { copper_texts: vec![text(0, "F.Cu"), text(90_000, "B.Cu"), text(0, "F.SilkS")], ..Default::default() });
        let (node, layers) = build_node(&design, &model);
        let texts: Vec<&Solid> = node.iter().filter_map(|(_, it)| match it { Item::Solid(s) if s.is_text() => Some(s), _ => None }).collect();
        assert_eq!(texts.len(), 2, "the two on copper, not the one on silk");
        let (front, back) = (texts.iter().find(|s| s.layers == crate::layer::LayerRange::single(layers.index_of("F.Cu").unwrap())).unwrap(), texts.iter().find(|s| s.layers == crate::layer::LayerRange::single(layers.index_of("B.Cu").unwrap())).unwrap());
        assert!(front.net == crate::item::not_a_net() && !front.routable() && node.get(node.iter().find(|(_, it)| matches!(it, Item::Solid(s) if s.is_text())).unwrap().0).unwrap().anchors().is_empty());
        let Shape::Polygon { pts } = &front.shape else { panic!("a polygon: {:?}", front.shape) };
        assert_eq!(pts.len(), 4);
        let (x0, y0, x1, y1) = front.shape.bbox(Nm::ZERO);
        // two glyphs 1 mm high, centred on (20, 20) mm: wider than high, round the anchor, the pen and the error outside the glyphs
        assert!(x1 - x0 > y1 - y0 && x0 < mm(0.02) && x1 > mm(0.02) && y0 < Nm::new(19_500) && y1 > Nm::new(20_500), "{:?}", (x0, y0, x1, y1));
        // the text turned by 90 degrees: its box is turned too
        let (bx0, by0, bx1, by1) = back.shape.bbox(Nm::ZERO);
        assert!((bx1 - bx0 - (y1 - y0)).abs() <= Nm::new(2) && (by1 - by0 - (x1 - x0)).abs() <= Nm::new(2), "{:?} vs {:?}", (bx0, by0, bx1, by1), (x0, y0, x1, y1));
        // a track through it collides; one that keeps the clearance does not
        let rules = model.board.clone();
        let through = Shape::Stadium { a: Point { x: mm(0.015), y: mm(0.02) }, b: Point { x: mm(0.025), y: mm(0.02) }, r: Nm::new(100) };
        assert!(node.first_colliding(&through, &net_of("N1"), front.layers, &rules, &[]).is_some());
        let clear = Shape::Stadium { a: Point { x: mm(0.015), y: y1 + Nm::new(100) + rules.clearance }, b: Point { x: mm(0.025), y: y1 + Nm::new(100) + rules.clearance }, r: Nm::new(100) };
        assert!(node.first_colliding(&clear, &net_of("N1"), front.layers, &rules, &[]).is_none());
    }

    fn keepout_board(zone: eda_model::ir::Zone) -> (Design, ConstraintModel) {
        let (mut design, mut model) = two_pad_board();
        model.board.layers = vec!["F.Cu".into(), "In1.Cu".into(), "In2.Cu".into(), "B.Cu".into()];
        design.routing.as_mut().unwrap().zones.push(zone);
        (design, model)
    }

    fn keepout_zone(layers: &[&str], outline: Vec<Point>, holes: Vec<Vec<Point>>) -> eda_model::ir::Zone {
        let mut z = eda_model::ir::Zone { id: "ko".into(), is_rule_area: true, keepout_tracks: true, outline, holes, ..Default::default() };
        z.set_layer_set(layers);
        z
    }

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
        vec![Point { x: mm(x0), y: mm(y0) }, Point { x: mm(x1), y: mm(y0) }, Point { x: mm(x1), y: mm(y1) }, Point { x: mm(x0), y: mm(y1) }]
    }

    /// The layers the keepout triangles of the world are on.
    fn keepout_layers(node: &Node) -> Vec<i32> {
        let mut v: Vec<i32> = node.iter().filter_map(|(_, it)| match it { Item::Solid(s) if s.is_keepout() => Some(s.layers.start()), _ => None }).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// R3-1 (`syncZone`: `for( PCB_LAYER_ID layer : LAYER_RANGE( F_Cu, B_Cu, .. ) ) { if( !layers[ layer ] ) continue; .. }`): a rule area on
    /// F.Cu and B.Cu is an obstacle on both, and one on every copper layer (`*.Cu`) on all four.
    #[test]
    fn a_keepout_is_an_obstacle_on_every_copper_layer_of_its_set() {
        let (design, model) = keepout_board(keepout_zone(&["F.Cu", "B.Cu"], square(10.0, 10.0, 20.0, 20.0), vec![]));
        let (node, _) = build_node(&design, &model);
        assert_eq!(keepout_layers(&node), vec![0, 3]);
        let (design, model) = keepout_board(keepout_zone(&["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"], square(10.0, 10.0, 20.0, 20.0), vec![]));
        let (node, _) = build_node(&design, &model);
        assert_eq!(keepout_layers(&node), vec![0, 1, 2, 3]);
        let (design, model) = keepout_board(keepout_zone(&["B.Cu"], square(10.0, 10.0, 20.0, 20.0), vec![]));
        let (node, _) = build_node(&design, &model);
        assert_eq!(keepout_layers(&node), vec![3]);
    }

    /// R3-5 (`CacheTriangulation` of the outline with its holes): no triangle of a keepout covers its hole, and together they cover the rest.
    #[test]
    fn a_keepouts_hole_is_left_open() {
        let (design, model) = keepout_board(keepout_zone(&["F.Cu"], square(10.0, 10.0, 20.0, 20.0), vec![square(13.0, 13.0, 17.0, 17.0)]));
        let (node, _) = build_node(&design, &model);
        let tris: Vec<&Shape> = node.iter().filter_map(|(_, it)| match it { Item::Solid(s) if s.is_keepout() => Some(&s.shape), _ => None }).collect();
        let at = |x: f64, y: f64| Shape::Circle { c: Point { x: mm(x), y: mm(y) }, r: Nm::ZERO };
        assert!(tris.iter().all(|t| t.collides(&at(15.0, 15.0), Nm::ZERO).is_none()), "the hole's middle is open");
        assert!(tris.iter().all(|t| t.collides(&at(13.5, 16.5), Nm::ZERO).is_none()), "and its corner");
        for (x, y) in [(11.0, 11.0), (15.0, 11.5), (18.5, 15.0), (11.5, 18.5)] {
            assert!(tris.iter().any(|t| t.collides(&at(x, y), Nm::ZERO).is_some()), "({x}, {y}) is in the area");
        }
        let area: f64 = tris.iter().map(|t| match t { Shape::Polygon { pts } => ((pts[1].x - pts[0].x).as_f64() * (pts[2].y - pts[0].y).as_f64() - (pts[2].x - pts[0].x).as_f64() * (pts[1].y - pts[0].y).as_f64()).abs() / 2.0, _ => 0.0 }).sum();
        assert!((area - 84e12).abs() < 1e9, "100 mm2 less the 16 mm2 hole: {area}");
    }

    /// R3-9 (`syncZone`: `if( !poly->IsTriangulationUpToDate() ) { .. "%s is malformed." .. return false; }`): a self-intersecting outline is
    /// left out of the world, and the router says so.
    #[test]
    fn a_malformed_keepout_is_left_out_with_a_warning() {
        let bow_tie = vec![Point { x: mm(10.0), y: mm(10.0) }, Point { x: mm(20.0), y: mm(20.0) }, Point { x: mm(20.0), y: mm(10.0) }, Point { x: mm(10.0), y: mm(20.0) }];
        assert!(crate::triangulation::tesselate(&bow_tie).is_none(), "the ear cutting does not finish a bow tie");
        let mut z = keepout_zone(&["F.Cu", "B.Cu"], bow_tie, vec![]);
        z.name = "ANT".into();
        let (design, model) = keepout_board(z);
        let (node, _) = build_node(&design, &model);
        assert!(keepout_layers(&node).is_empty());
        let warnings = node.rule_resolver().unwrap().warnings();
        assert_eq!(warnings, ["Rule area 'ANT' on F.Cu and B.Cu is malformed.\nThis zone cannot be handled by the router.\nPlease verify it is not a self-intersecting polygon."]);
    }

    /// The graphics' solids of a world, by the id of their graphic.
    fn graphic_solids<'a>(node: &'a Node, id: &str) -> Vec<&'a Solid> {
        let (shape, other) = (format!("{}{id}#", Solid::GRAPHIC_SOURCE), format!("{}{id}#", Solid::OBSTACLE_SOURCE));
        node.iter().filter_map(|(_, it)| match it { Item::Solid(s) if s.source.starts_with(&shape) || s.source.starts_with(&other) => Some(s), _ => None }).collect()
    }

    /// R3-2 (`SyncWorld`'s `m_board->Drawings()` -> `syncGraphicalItem`, `syncBarcode`, `syncTextItem` of a table, `syncGraphicalItem` of a text box):
    /// each graphic on copper is a solid on its layer; a shape has its net and its connection points, and a route of its net joins it there;
    /// one on silk is nothing to the router.
    #[test]
    fn copper_graphics_are_solids_on_their_layer() {
        use eda_model::ir::{DrawingsSection, Shape as S};
        let (mut design, model) = two_pad_board();
        let pt = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        let mut d = DrawingsSection::default();
        d.shapes.push(S::Polygon { id: "shp_gnd".into(), layer: "F.Cu".into(), stroke_width: Nm::ZERO, filled: true, pts: vec![pt(10.0, 10.0), pt(14.0, 10.0), pt(14.0, 12.0), pt(10.0, 12.0)] });
        d.board_items.set_net("shp_gnd", Some("GND".into()));
        d.shapes.push(S::Segment { id: "shp_line".into(), layer: "B.Cu".into(), stroke_width: mm(0.5), filled: false, start: pt(20.0, 10.0), end: pt(25.0, 10.0) });
        d.shapes.push(S::Segment { id: "shp_silk".into(), layer: "F.SilkS".into(), stroke_width: mm(0.5), filled: false, start: pt(20.0, 20.0), end: pt(25.0, 20.0) });
        d.board_items.text_boxes.push(eda_model::pcb_items::TextBox { id: "tbx_1".into(), ..eda_model::pcb_items::TextBox::new("F.Cu", pt(30.0, 10.0), pt(34.0, 12.0), mm(1.0), mm(0.1)) });
        d.tables.push(eda_model::table::Table { id: "tbl_1".into(), ..eda_model::table::Table::new_grid("F.Cu", pt(40.0, 10.0), 2, 2, mm(5.0), mm(2.0), mm(0.2)) });
        d.barcodes.push(eda_model::barcode::Barcode { id: "bar_1".into(), at: pt(60.0, 10.0), angle: 90_000, layer: "F.Cu".into(), width_nm: mm(4.0), height_nm: mm(2.0), text: "X".into(), text_height_nm: mm(1.0), kind: Default::default(), ecc: Default::default(), show_text: false, knockout: false, margin_x_nm: Nm::ZERO, margin_y_nm: Nm::ZERO });
        design.drawings = Some(d);
        let (node, layers) = build_node(&design, &model);
        let (f, b) = (layers.index_of("F.Cu").unwrap(), layers.index_of("B.Cu").unwrap());
        let gnd = graphic_solids(&node, "shp_gnd");
        assert_eq!(gnd.len(), 1, "a filled polygon with no stroke: its fill");
        assert!(gnd[0].net.as_deref() == Some("GND") && gnd[0].routable() && gnd[0].layers == crate::layer::LayerRange::single(f));
        // `GetConnectionPoints`: a filled shape's centre, which is a joint of GND
        assert_eq!(gnd[0].anchor_points, vec![pt(12.0, 11.0)]);
        assert!(node.joint_at(pt(12.0, 11.0), &net_of("GND")).is_some());
        let line = graphic_solids(&node, "shp_line");
        assert!(line.len() == 1 && line[0].layers == crate::layer::LayerRange::single(b) && line[0].net.is_none() && line[0].anchor_points == vec![pt(20.0, 10.0), pt(25.0, 10.0)]);
        assert!(graphic_solids(&node, "shp_silk").is_empty());
        // a SIG track across the GND polygon collides; across the line on the front it does not, on the back it does
        let rules = model.board.clone();
        let across = |y: f64| Shape::Stadium { a: pt(9.0, y), b: pt(26.0, y), r: mm(0.1) };
        assert!(node.first_colliding(&across(11.0), &net_of("SIG"), crate::layer::LayerRange::single(f), &rules, &[]).is_some());
        assert!(node.first_colliding(&across(10.0), &net_of("GND"), crate::layer::LayerRange::single(b), &rules, &[]).is_some());
        assert!(node.first_colliding(&across(11.0), &net_of("GND"), crate::layer::LayerRange::single(f), &rules, &[]).is_none(), "its own net does not collide with it");
        // the text box's border, the table's box and the barcode's turned box
        let tbx = graphic_solids(&node, "tbx_1");
        assert!(!tbx.is_empty() && tbx.iter().all(|s| s.routable() && s.layers.start() == f));
        let tbl = graphic_solids(&node, "tbl_1");
        assert_eq!(tbl.len(), 1);
        let (x0, y0, x1, y1) = tbl[0].shape.bbox(Nm::ZERO);
        assert_eq!((x0, y0, x1, y1), (mm(40.0) - mm(0.1), mm(10.0) - mm(0.1), mm(50.0) + mm(0.1), mm(14.0) + mm(0.1)), "two 5 mm columns, two 2 mm rows, half the 0.2 mm border round them");
        assert!(!tbl[0].routable());
        let bar = graphic_solids(&node, "bar_1");
        let (x0, y0, x1, y1) = bar[0].shape.bbox(Nm::ZERO);
        assert_eq!((x1 - x0, y1 - y0), (mm(2.0), mm(4.0)), "4 x 2 mm turned by 90 degrees");
    }

    /// r4 R4-6: a text box with its border off keeps its stroke's width (`makeEffectiveShapes` takes `GetEffectiveWidth()`), and an arc on copper
    /// is its chords with the flattening error added to its stroke, so the band holds the true `SHAPE_ARC`.
    #[test]
    fn a_borderless_text_box_keeps_its_stroke_and_an_arc_its_flattening_error() {
        use eda_model::ir::{DrawingsSection, Shape as S};
        let (mut design, model) = two_pad_board();
        let pt = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        let mut d = DrawingsSection::default();
        d.board_items.text_boxes.push(eda_model::pcb_items::TextBox { id: "tbx_1".into(), border: false, ..eda_model::pcb_items::TextBox::new("F.Cu", pt(30.0, 10.0), pt(34.0, 12.0), mm(1.0), mm(0.1)) });
        d.shapes.push(S::Arc { id: "shp_arc".into(), layer: "F.Cu".into(), stroke_width: mm(0.2), filled: false, start: pt(40.0, 10.0), mid: pt(45.0, 5.0), end: pt(50.0, 10.0) });
        design.drawings = Some(d);
        let (node, _) = build_node(&design, &model);
        let radii = |id: &str| graphic_solids(&node, id).iter().map(|s| match &s.shape { Shape::Strokes { r, .. } => *r, other => panic!("{other:?}") }).collect::<Vec<_>>();
        assert_eq!(radii("tbx_1"), vec![mm(0.05)], "half the 0.1 mm stroke, the border off");
        assert_eq!(radii("shp_arc"), vec![mm(0.1) + Nm::new(5_000)], "half the 0.2 mm stroke and ARC_HIGH_DEF");
    }

    /// R3-2: a footprint's own copper shape (`footprint->GraphicalItems()`) is a solid of its footprint, so a net tie's bridge is in the tie
    /// (`IsInNetTie`), and a footprint's copper text is an obstacle (`syncTextItem` of its texts).
    #[test]
    fn a_footprints_copper_shape_is_of_its_footprint() {
        use eda_model::ir::{DrawingsSection, FootprintExtra, FootprintGraphic, FootprintText, Shape as S};
        let (mut design, model) = two_pad_board();
        let pt = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        let bridge = FootprintGraphic { shape: S::Segment { id: String::new(), layer: "F.Cu".into(), stroke_width: mm(0.3), filled: false, start: pt(0.0, 0.0), end: pt(1.0, 0.0) }, solder_mask_margin: None, pad_number: None, net: String::new(), pin_type: String::new() };
        let text = FootprintText { text: "NT".into(), layer: "F.Cu".into(), at: pt(0.0, 3.0), angle_file_mdeg: 0, size: (mm(1.0), mm(1.0)), thickness: mm(0.15), halign: 0, valign: 0, mirror: false, keep_upright: false, bold: false };
        design.drawings = Some(DrawingsSection { footprint_extras: vec![FootprintExtra { id: "U1".into(), net_tie_pad_groups: vec!["1,2".into()], graphics: vec![bridge], texts: vec![text], ..Default::default() }], ..Default::default() });
        let (node, _) = build_node(&design, &model);
        let shapes: Vec<&Item> = node.iter().filter_map(|(_, it)| match it { Item::Solid(s) if s.is_graphic() => Some(it), _ => None }).collect();
        assert_eq!(shapes.len(), 1, "{:?}", node.iter().filter(|(_, it)| matches!(it, Item::Solid(_))).map(|(_, it)| it).collect::<Vec<_>>());
        assert!(node.rule_resolver().unwrap().is_in_net_tie(shapes[0]), "the bridge is the net tie's");
        assert_eq!(node.iter().filter(|(_, it)| matches!(it, Item::Solid(s) if s.is_text())).count(), 1, "the footprint's copper text");
    }

    /// R3-6 (`syncGraphicalItem`: `Edge_Cuts || Margin` -> every copper layer, not routable; `isEdge`: `IsOnLayer( Margin )`): a line on Margin is
    /// a board edge to the router, kept at the copper-to-edge clearance.
    #[test]
    fn a_margin_line_is_an_edge() {
        use eda_model::ir::{DrawingsSection, Shape as S};
        let (mut design, model) = two_pad_board();
        let pt = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        design.drawings = Some(DrawingsSection { shapes: vec![S::Segment { id: "shp_m".into(), layer: "Margin".into(), stroke_width: mm(0.1), filled: false, start: pt(10.0, 0.0), end: pt(10.0, 20.0) }], ..Default::default() });
        let (node, layers) = build_node(&design, &model);
        let margin: Vec<&Solid> = node.iter().filter_map(|(_, it)| match it { Item::Solid(s) if s.source.starts_with("margin:") => Some(s), _ => None }).collect();
        assert_eq!(margin.len(), 1);
        assert!(margin[0].edge && !margin[0].routable() && margin[0].layers == layers.all());
        let rules = model.board.clone();
        let edge = eda_drc::constraints::edge_clearance_min(&rules);
        let near = Shape::Stadium { a: pt(5.0, 5.0), b: Point { x: mm(10.0) - edge + Nm::new(10_000), y: mm(5.0) }, r: Nm::new(5_000) };
        assert!(node.first_colliding(&near, &net_of("SIG"), crate::layer::LayerRange::single(0), &rules, &[]).is_some(), "within the edge clearance of the Margin line");
    }

    #[test]
    fn builds_a_node_with_a_pad_and_a_two_segment_track() {
        let (design, model) = two_pad_board();
        let (node, layers) = build_node(&design, &model);
        assert!(layers.index_of("F.Cu").is_some());
        let segs = node.iter().filter(|(_, it)| matches!(it, Item::Segment(_))).count();
        assert_eq!(segs, 2, "a 3-point track must flatten into 2 segments");
        let pads = node.iter().filter(|(_, it)| matches!(it, Item::Solid(_))).count();
        assert!(pads >= 1);
    }
}
