//! Swap (Alt+S) of every kind of item, on the board and in the Footprint Editor: [`crate::Cmd::SwapItems`] and [`crate::fp_items::FpItemCmd::Swap`], one undo step each.
//!
//! Ports `EDIT_TOOL::Swap` (`pcbnew/tools/edit_tool_move_fct.cpp` at KiCad 8303b2ad; 25ec2335's differs only by `ReportFilteredLockedItems`, which the studio's tool
//! half does) down to what each item class does with it. The tool half -- which items (`FilterCollectorForMarkers`, `FilterCollectorForHierarchy`: a group's member
//! stands for its top group; `FilterCollectorForFreePads`: on the board a pad stands for its footprint, unless "Allow free pads" is on (UI-LEFT-3 R3-7: two pads of the
//! board then swap their copper's places, kept as pad edits); a straight track left out; `FilterCollectorForLockedItems`)
//! and their selection order (`GetItemsSortedBySelectionOrder`) -- is the caller's, as for the other edit tools (`web/studio/src/kicad-port/pcbSwap.ts`). Here, in
//! the order given:
//!
//! ```text
//! for( size_t i = 0; i < sorted.size() - 1; i++ )
//!     a = sorted[i], b = sorted[i + 1]
//!     two pads:          PAD::SwapShapePositions( a, b ), the layers swapped; continue
//!     aPos = a->GetPosition(), bPos = b->GetPosition(); a->SetPosition( bPos ); b->SetPosition( aPos );
//!     two footprints:    flipped both (about their new positions, TOP_BOTTOM) if on different sides, then the orientations swapped
//!     else, same type:   the layers swapped (`GetLayer` / `SetLayer`), unless they are groups
//! ```
//!
//! So each pair works on the positions the pair before it left. What `GetPosition` and `SetPosition` are differs by class, and is kept:
//!
//! | class | `GetPosition` | `SetPosition` |
//! |---|---|---|
//! | `FOOTPRINT`, `PCB_VIA`, `PCB_TEXT` (a footprint's text too), `PCB_FIELD`, `PCB_POINT`, `PCB_BARCODE`, `PCB_REFERENCE_IMAGE` | its position | moved there |
//! | `PCB_SHAPE`, `PCB_TEXTBOX` (`EDA_SHAPE::getPosition` / `setPosition`) | an arc's centre, a polygon's first corner, else its start (a circle's centre) | moved by the difference |
//! | `PCB_ARC` (`pcb_track.cpp::PCB_ARC::GetPosition`, `pcb_track.h`) | its centre (`CalcArcCenter`) | `m_Start = aPos`: only the start moves, the middle and the end stay |
//! | `PCB_DIMENSION_BASE` (`pcb_dimension.h`) | its start | `SetStart( aPos )`: only the start moves |
//! | `ZONE` (`zone.cpp::GetPosition`, `zone.h`) | its first corner | a no-op: a zone never moves |
//! | `PCB_TABLE` (`pcb_table.cpp`) | its first cell's position | moved by the difference |
//! | `PCB_GROUP` (`pcb_group.cpp`) | the centre of its bounding box | moved by the difference |
//! | `PAD` (`pad.cpp`) | its position (the hole) | moved there |
//! | `PCB_GENERATOR` (`pcb_generator.h`): a tuning pattern, a via stitching | `m_origin` (a via stitching: its outline's first corner) | `m_origin = aPos` |
//! | `PCB_GRID_ITEM`, `PCB_DRILL_MAP` (KiCad 25ec2335) | `m_pos`, `m_offset` | set there |
//!
//! `GetLayer` / `SetLayer` for the second rule: a via's is its drill's start (`PCB_VIA::GetLayer` / `SetLayer`, `Padstack().Drill().start`), a zone's is its one layer
//! or `UNDEFINED_LAYER` for a zone on several, which `ZONE::SetLayer` ignores (an empty `LSET`, see [`set_zone_layer`]), a shape leaves its net when it lands off copper
//! (`PCB_SHAPE::SetLayer`: `if( !IsOnCopperLayer() ) SetNetCode( -1 )`), a table's cells go with it (`PCB_TABLE::SetLayer`). The "same type" is KiCad's `KICAD_T`
//! ([`SwapType`]): every graphic shape is one `PCB_SHAPE_T`, a board text and a footprint's own text are both `PCB_TEXT_T`, each kind of dimension is a type of its own.
//!
//! A group's bounding box takes the drawn extent of everything below it (a footprint's with its texts, a track's width, a text's glyphs), which the studio's painter
//! works out (`itemBounds`); its centre is sent with the command (`centres`), and a group moved by a pair before is moved by the same difference.
//!
//! Differs:
//! - A pad's layer (`BOARD_ITEM::m_layer` of a `PAD`, swapped by the first rule) is neither drawn nor saved (a pad is on its layer set, `PAD::GetLayerSet`); the model has
//!   no such field, so two pads swap their shape positions only.
//! - A via: a through via (from the outermost copper layer to the other) stays one, as `PCB_VIA::SanitizeLayers` and the writer (`VIATYPE::THROUGH`: `F_Cu`, `B_Cu`) make it
//!   on the next load; a blind or buried via given a start below its end has the two in order (`SanitizeLayers`'s `std::swap`), and one whose start would be its end keeps
//!   its layers (KiCad would make a via on one layer).
//! - Not swapped (refused, the studio leaves them out): a table cell. KiCad's `PCB_TABLECELL` (a text box, `EDA_SHAPE::setPosition`) would leave its table's grid until
//!   something normalizes the table; the studio's table keeps its cells on the grid (`PCB_TABLE::Normalize`'s layout is all it holds, `eda_model::table`).
//!
//! The generators and KiCad 10's items (UI-LEFT-3 P3-33) as KiCad has them: a tuning pattern and a via stitching are both `PCB_GENERATOR_T` -- `GetPosition` is
//! `m_origin` (25ec2335 `PCB_VIA_STITCH::GetPosition`: its outline's first corner), `SetPosition` sets `m_origin` alone (the pattern's tracks, the stitching's outline
//! and vias stay; nothing regenerates them in the swap's commit), two of them swap layers (`PCB_TUNING_PATTERN::GetLayer` is its first track's, `SetLayer` sets its
//! tracks' too). 25ec2335's grid item: `m_pos`, no layer ("Grids don't have layers"); drill map: `m_offset` and its layer; drill chart: a table
//! (`PCB_GENERATED_TABLE`) of its own type, so a chart and a plain table do not swap layers.

use super::Board;
use crate::fp_items::{field_layout_mut, field_name, live_ids, transform_ids};
use crate::pcb_transform::{FlipDirection, Xform};
use eda_model::groups::GroupTree;
use eda_model::ir::{tessellate_arc, DimensionKind, LibraryFootprint, Nm, Point, Shape, TRACK_ARC_SEGMENTS};
use eda_model::CheckResult;
use std::collections::{BTreeMap, BTreeSet};

fn fail(check: &str, what: &str, msg: impl Into<String>) -> Vec<CheckResult> {
    vec![CheckResult::fail(check, what, msg.into())]
}

/// An item's `KICAD_T`, as far as `EDIT_TOOL::Swap` asks (`a->Type() == b->Type()`, `PCB_PAD_T`, `PCB_FOOTPRINT_T`, `PCB_GROUP_T`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SwapType {
    Footprint,
    Pad,
    Via,
    Arc,
    Zone,
    Shape,
    Text,
    Field,
    DimAligned,
    DimOrthogonal,
    DimRadial,
    DimCenter,
    DimLeader,
    TextBox,
    Point,
    Image,
    Barcode,
    Table,
    Group,
    /// `PCB_GENERATOR_T`: a tuning pattern and a via stitching are one type (KiCad 25ec2335's `PCB_VIA_STITCH` is a `PCB_GENERATOR_POLY`).
    Generator,
    /// KiCad 25ec2335's `PCB_GRID_ITEM_T`.
    Grid,
    /// KiCad 25ec2335's `PCB_DRILL_MAP_T`.
    DrillMap,
    /// KiCad 25ec2335's `PCB_DRILL_CHART_T`: a table (`PCB_GENERATED_TABLE`), of a type of its own.
    DrillChart,
}

fn dimension_type(k: &DimensionKind) -> SwapType {
    match k {
        DimensionKind::Aligned { .. } => SwapType::DimAligned,
        DimensionKind::Orthogonal { .. } => SwapType::DimOrthogonal,
        DimensionKind::Radial { .. } => SwapType::DimRadial,
        DimensionKind::Center => SwapType::DimCenter,
        DimensionKind::Leader => SwapType::DimLeader,
    }
}

/// `EDA_SHAPE::getPosition`: an arc's centre (`getCenter`, `CalcArcCenter`), a polygon's first corner, the start of everything else (a circle's start is its centre).
fn shape_position(s: &Shape) -> Point {
    match s {
        Shape::Arc { start, mid, end, .. } => crate::sch_scene::arc_center_of(*start, *mid, *end),
        Shape::Circle { center, .. } => *center,
        other => other.points().first().copied().unwrap_or_default(),
    }
}

/// `PCB_SHAPE::GetPosition` of a text box (a rectangle, or a polygon when it is turned).
fn text_box_position(t: &eda_model::pcb_items::TextBox) -> Point {
    t.pts.first().copied().unwrap_or(t.start)
}

/// `ZONE::GetLayer`: the layer of a zone on exactly one, else `UNDEFINED_LAYER` (`None`).
fn zone_layer(z: &eda_model::ir::Zone) -> Option<String> {
    let set = z.layer_set();
    (set.len() == 1).then(|| set[0].to_string())
}

/// `ZONE::SetLayer( aLayer )`: `SetLayerSet( LSET( { aLayer } ) )`; `LSET`'s list constructor skips `UNDEFINED_LAYER` and `SetLayerSet` returns on an empty set, so a
/// zone given no layer stays as it is.
fn set_zone_layer(z: &mut eda_model::ir::Zone, layer: Option<&str>) {
    if let Some(l) = layer {
        z.set_layer_set(&[l]);
    }
}

fn delta(to: Point, from: Point) -> (Nm, Nm) {
    (to.x - from.x, to.y - from.y)
}

/// What [`swap`] needs of the place the items are in: the board, or the footprint open in the Footprint Editor.
pub(crate) trait SwapHost {
    fn swap_type(&self, id: &str) -> Result<SwapType, Vec<CheckResult>>;
    /// `GetPosition()` of an item that is not a group.
    fn swap_position(&mut self, id: &str, t: SwapType) -> Point;
    /// `SetPosition( at )`; `from` is the item's `GetPosition()` (a group's centre included).
    fn swap_set_position(&mut self, id: &str, t: SwapType, at: Point, from: Point) -> Result<(), Vec<CheckResult>>;
    /// `GetLayer()`; `None` is `UNDEFINED_LAYER`.
    fn swap_layer(&mut self, id: &str, t: SwapType) -> Option<String>;
    fn swap_set_layer(&mut self, id: &str, t: SwapType, layer: Option<&str>) -> Result<(), Vec<CheckResult>>;
    /// The footprints' part of a pair (after their positions): see [`Board::swap_footprint_sides`].
    fn swap_footprint_sides(&mut self, _a: &str, _b: &str) -> Result<(), Vec<CheckResult>> {
        Ok(())
    }
    /// `PAD::SwapShapePositions`.
    fn swap_pad_shapes(&mut self, _a: &str, _b: &str) -> Result<(), Vec<CheckResult>> {
        Ok(())
    }
}

/// `EDIT_TOOL::Swap`'s loop over `ids` (in selection order), see the module doc. `centres` holds each group's `GetPosition()`.
pub(crate) fn swap<H: SwapHost>(host: &mut H, ids: &[String], centres: &BTreeMap<String, Point>) -> Result<(), Vec<CheckResult>> {
    // `if( selection.Size() < 2 ) return 0;`
    if ids.len() < 2 {
        return Err(fail("ops_swap", "swap_items", "swapping needs at least two items"));
    }
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id.as_str()) {
            return Err(fail("ops_swap_self", id, "an item cannot be swapped with itself"));
        }
    }
    let types: Vec<SwapType> = ids.iter().map(|id| host.swap_type(id)).collect::<Result<_, _>>()?;
    let mut group_at: BTreeMap<&str, Point> = BTreeMap::new();
    for (id, t) in ids.iter().zip(&types) {
        if *t == SwapType::Group {
            let c = centres.get(id).ok_or_else(|| fail("ops_swap", id, "a group's position (the centre of its bounding box) must be sent in `centres`"))?;
            group_at.insert(id.as_str(), *c);
        }
    }
    for i in 0..ids.len() - 1 {
        let (a, b) = (ids[i].as_str(), ids[i + 1].as_str());
        let (ta, tb) = (types[i], types[i + 1]);
        // "Pads may have a copper shape offset from the anchor/hole, so swap visible shape centers rather than anchor positions."
        if ta == SwapType::Pad && tb == SwapType::Pad {
            host.swap_pad_shapes(a, b)?;
            continue;
        }
        let pos = |host: &mut H, id: &str, t: SwapType| if t == SwapType::Group { group_at[id] } else { host.swap_position(id, t) };
        let (a_pos, b_pos) = (pos(host, a, ta), pos(host, b, tb));
        host.swap_set_position(a, ta, b_pos, a_pos)?;
        host.swap_set_position(b, tb, a_pos, b_pos)?;
        for (id, t, at) in [(a, ta, b_pos), (b, tb, a_pos)] {
            if t == SwapType::Group {
                group_at.insert(id, at);
            }
        }
        if ta == SwapType::Footprint && tb == SwapType::Footprint {
            host.swap_footprint_sides(a, b)?;
        } else if ta == tb && ta != SwapType::Group {
            // "We can also do a layer swap safely for two objects of the same type, except groups which don't support layer swaps."
            let (la, lb) = (host.swap_layer(a, ta), host.swap_layer(b, tb));
            host.swap_set_layer(a, ta, lb.as_deref())?;
            host.swap_set_layer(b, tb, la.as_deref())?;
        }
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------------------- the board

impl Board<'_> {
    /// `Cmd::SwapItems`.
    pub(crate) fn swap_items(&mut self, ids: &[String], centres: &BTreeMap<String, Point>) -> Result<(), Vec<CheckResult>> {
        swap(self, ids, centres)
    }

    /// `PCB_VIA::SetLayer`, with what the next load makes of it (see the module doc).
    fn set_via_start(&mut self, id: &str, layer: &str) {
        let copper = &self.model.board.layers;
        let rank = |l: &str| copper.iter().position(|c| c == l);
        let (first, last) = (copper.first().cloned(), copper.last().cloned());
        let Some(v) = self.design.routing.as_mut().and_then(|rt| rt.vias.iter_mut().find(|v| v.id == id)) else { return };
        if Some(&v.from_layer) == first.as_ref() && Some(&v.to_layer) == last.as_ref() {
            return;
        }
        if layer == v.to_layer {
            return;
        }
        v.from_layer = layer.to_string();
        if let (Some(f), Some(t)) = (rank(&v.from_layer), rank(&v.to_layer)) {
            if f > t {
                std::mem::swap(&mut v.from_layer, &mut v.to_layer);
            }
        }
    }

    /// A footprint's own text's layer as the board shows it, and the footprint's side.
    fn fp_text_layer(&self, id: &str) -> Option<String> {
        let (reference, k) = self.resolve_fp_text(id)?;
        let pose = self.pose_of(&reference)?;
        let fp = self.unedited_footprint(self.model.part(&reference)?)?;
        Some(eda_model::fp_art::layer_on_side(&fp.art.texts.get(k)?.layout.layer, pose.side))
    }
}

impl SwapHost for Board<'_> {
    fn swap_type(&self, id: &str) -> Result<SwapType, Vec<CheckResult>> {
        if self.pose_of(id).is_some() {
            return Ok(SwapType::Footprint);
        }
        // a pad on its own: the studio sends one with "Allow free pads" (`FilterCollectorForFreePads` keeps it, UI-LEFT-3 R3-7)
        if self.pad_of(id).is_some() {
            return Ok(SwapType::Pad);
        }
        if let Some(rt) = &self.design.routing {
            if let Some(t) = rt.tracks.iter().find(|t| t.id == id) {
                // `if( item->Type() == PCB_TRACE_T ) aCollector.Remove( item );`
                return match t.arc() {
                    Some(_) => Ok(SwapType::Arc),
                    None => Err(fail("ops_swap", id, "a straight track is not swapped (EDIT_TOOL::Swap leaves it out)")),
                };
            }
            if rt.vias.iter().any(|v| v.id == id) {
                return Ok(SwapType::Via);
            }
            if rt.zones.iter().any(|z| z.id == id) {
                return Ok(SwapType::Zone);
            }
        }
        if self.resolve_field(id).is_some() {
            return Ok(SwapType::Field);
        }
        if self.resolve_fp_text(id).is_some() {
            return Ok(SwapType::Text);
        }
        if let Some(dr) = &self.design.drawings {
            let bi = &dr.board_items;
            if dr.shapes.iter().any(|s| s.id() == id) {
                return Ok(SwapType::Shape);
            }
            if dr.texts.iter().any(|t| t.id == id) {
                return Ok(SwapType::Text);
            }
            if let Some(d) = dr.dimensions.iter().find(|d| d.id == id) {
                return Ok(dimension_type(&d.kind));
            }
            if bi.text_boxes.iter().any(|t| t.id == id) {
                return Ok(SwapType::TextBox);
            }
            if bi.points.iter().any(|p| p.id == id) {
                return Ok(SwapType::Point);
            }
            if bi.images.iter().any(|i| i.id == id) {
                return Ok(SwapType::Image);
            }
            if dr.barcodes.iter().any(|b| b.id == id) {
                return Ok(SwapType::Barcode);
            }
            if dr.tables.iter().any(|t| t.id == id) {
                // a drill chart is a table of a type of its own (`PCB_DRILL_CHART_T`)
                return Ok(if bi.drill_charts.iter().any(|c| c.table_id == id) { SwapType::DrillChart } else { SwapType::Table });
            }
            if dr.is_group(id) {
                return Ok(SwapType::Group);
            }
            if dr.tuning_patterns.iter().any(|p| p.id == id) || bi.via_stitches.iter().any(|v| v.id == id) {
                return Ok(SwapType::Generator);
            }
            if bi.grid_items.iter().any(|g| g.id == id) {
                return Ok(SwapType::Grid);
            }
            if bi.drill_maps.iter().any(|m| m.id == id) {
                return Ok(SwapType::DrillMap);
            }
            if dr.tables.iter().any(|t| crate::pcb_swap::has_cell(t, id)) {
                return Err(fail("ops_swap", id, "a table cell is not swapped here (the studio keeps a table's cells on its grid, as PCB_TABLE::Normalize lays them)"));
            }
        }
        Err(fail("ops_unknown_item", id, "no footprint, via, arc, zone, shape, text, field, dimension, text box, point, image, barcode, table, group, tuning pattern, via stitching, grid, drill map or drill chart with this id"))
    }

    fn swap_position(&mut self, id: &str, t: SwapType) -> Point {
        let rt = self.design.routing.as_ref();
        let dr = self.design.drawings.as_ref();
        let found = match t {
            SwapType::Footprint => self.pose_of(id).map(|f| f.at),
            // `PAD::GetPosition`: the pad's position (its hole), on the board
            SwapType::Pad => self.pad_of(id).and_then(|(r, _, _, pad)| self.pose_of(&r).map(|fp| eda_model::footprint::to_board(fp, pad.at))),
            SwapType::Via => rt.and_then(|rt| rt.vias.iter().find(|v| v.id == id)).map(|v| v.at),
            SwapType::Arc => rt.and_then(|rt| rt.tracks.iter().find(|t| t.id == id)).and_then(|t| t.arc()).map(|(s, m, e)| crate::sch_scene::arc_center_of(s, m, e)),
            SwapType::Zone => rt.and_then(|rt| rt.zones.iter().find(|z| z.id == id)).and_then(|z| z.outline.first().copied()),
            SwapType::Field => self.resolve_field(id).and_then(|(r, n)| Some(self.field_layout(&r, &n)?.board_position(self.pose_of(&r)?))),
            SwapType::Text if self.resolve_fp_text(id).is_some() => {
                let (r, k) = self.resolve_fp_text(id).expect("checked");
                let pose = self.pose_of(&r).cloned();
                let fp = self.model.part(&r).and_then(|p| self.unedited_footprint(p));
                fp.zip(pose).and_then(|(fp, pose)| Some(fp.art.texts.get(k)?.layout.board_position(&pose)))
            }
            SwapType::Text => dr.and_then(|d| d.texts.iter().find(|t| t.id == id)).map(|t| t.at),
            SwapType::Shape => dr.and_then(|d| d.shapes.iter().find(|s| s.id() == id)).map(shape_position),
            SwapType::DimAligned | SwapType::DimOrthogonal | SwapType::DimRadial | SwapType::DimCenter | SwapType::DimLeader => dr.and_then(|d| d.dimensions.iter().find(|x| x.id == id)).map(|d| d.start),
            SwapType::TextBox => dr.and_then(|d| d.board_items.text_boxes.iter().find(|x| x.id == id)).map(text_box_position),
            SwapType::Point => dr.and_then(|d| d.board_items.points.iter().find(|x| x.id == id)).map(|p| p.at),
            SwapType::Image => dr.and_then(|d| d.board_items.images.iter().find(|x| x.id == id)).map(|i| i.at),
            SwapType::Barcode => dr.and_then(|d| d.barcodes.iter().find(|x| x.id == id)).map(|b| b.at),
            SwapType::Table | SwapType::DrillChart => dr.and_then(|d| d.tables.iter().find(|x| x.id == id)).map(|t| t.at),
            // `PCB_GENERATOR::GetPosition`: `m_origin`; a via stitching's (25ec2335 `PCB_VIA_STITCH::GetPosition`) is its outline's first corner, the origin when it has none
            SwapType::Generator => dr.and_then(|d| {
                d.tuning_patterns.iter().find(|p| p.id == id).map(|p| p.origin).or_else(|| d.board_items.via_stitches.iter().find(|v| v.id == id).map(|v| v.outline.first().copied().unwrap_or(v.origin)))
            }),
            // 25ec2335 `PCB_GRID_ITEM::GetPosition`: `m_pos`; `PCB_DRILL_MAP::GetPosition`: `m_offset`
            SwapType::Grid => dr.and_then(|d| d.board_items.grid_items.iter().find(|g| g.id == id)).map(|g| g.at),
            SwapType::DrillMap => dr.and_then(|d| d.board_items.drill_maps.iter().find(|m| m.id == id)).map(|m| m.offset),
            SwapType::Group => None,
        };
        found.unwrap_or_default()
    }

    fn swap_set_position(&mut self, id: &str, t: SwapType, at: Point, from: Point) -> Result<(), Vec<CheckResult>> {
        match t {
            // `ZONE::SetPosition( const VECTOR2I& aPos ) override {}`
            SwapType::Zone => Ok(()),
            // `PCB_ARC::SetPosition`: `m_Start = aPos;`, the middle and the end where they were.
            SwapType::Arc => {
                let tr = self.design.routing.as_mut().and_then(|rt| rt.tracks.iter_mut().find(|t| t.id == id)).ok_or_else(|| fail("ops_unknown_item", id, "no track with this id"))?;
                let (_, mid, end) = tr.arc().ok_or_else(|| fail("ops_swap", id, "the track is no arc"))?;
                tr.pts = tessellate_arc(at, mid, end, TRACK_ARC_SEGMENTS);
                tr.arc_mid_offset = Some(Point { x: mid.x - at.x, y: mid.y - at.y });
                Ok(())
            }
            // `PCB_DIMENSION_BASE::SetPosition`: `SetStart( aPos )`.
            SwapType::DimAligned | SwapType::DimOrthogonal | SwapType::DimRadial | SwapType::DimCenter | SwapType::DimLeader => {
                let d = self.drawings_mut().dimensions.iter_mut().find(|d| d.id == id).ok_or_else(|| fail("ops_unknown_item", id, "no dimension with this id"))?;
                d.start = at;
                Ok(())
            }
            // `PCB_GENERATOR::SetPosition`: `m_origin = aPos`, nothing else -- the pattern's tracks and the stitching's outline and vias stay where they are.
            SwapType::Generator => {
                let dr = self.drawings_mut();
                if let Some(p) = dr.tuning_patterns.iter_mut().find(|p| p.id == id) {
                    p.origin = at;
                } else if let Some(v) = dr.board_items.via_stitches.iter_mut().find(|v| v.id == id) {
                    v.origin = at;
                }
                Ok(())
            }
            SwapType::Grid => {
                if let Some(g) = self.drawings_mut().board_items.grid_items.iter_mut().find(|g| g.id == id) {
                    g.at = at;
                }
                Ok(())
            }
            SwapType::DrillMap => {
                if let Some(m) = self.drawings_mut().board_items.drill_maps.iter_mut().find(|m| m.id == id) {
                    m.offset = at;
                }
                Ok(())
            }
            _ => {
                let (dx, dy) = delta(at, from);
                if dx == Nm::ZERO && dy == Nm::ZERO {
                    return Ok(());
                }
                self.move_items(&[id.to_string()], dx, dy)
            }
        }
    }

    /// `PAD::SwapShapePositions( a, b )` of two pads on the board ("Allow free pads"): each pad moves by what takes its copper's centre (`ShapePos`: the position
    /// and the offset turned with the pad) to the other's, as one `move_items` each.
    fn swap_pad_shapes(&mut self, a: &str, b: &str) -> Result<(), Vec<CheckResult>> {
        let shape_pos = |board: &Self, id: &str| -> Result<Point, Vec<CheckResult>> {
            let (r, _, _, pad) = board.pad_of(id).ok_or_else(|| fail("ops_unknown_item", id, "no pad with this id"))?;
            let fp = board.pose_of(&r).ok_or_else(|| fail("ops_unknown_item", id, "no pad with this id"))?;
            let off = pad.ext.offset.map(|(x, y)| crate::pad_edit::rotate(Point { x, y }, pad.rot as i64)).unwrap_or_default();
            Ok(eda_model::footprint::to_board(fp, (pad.at.0 + off.x, pad.at.1 + off.y)))
        };
        let (sa, sb) = (shape_pos(self, a)?, shape_pos(self, b)?);
        let (dx, dy) = delta(sb, sa);
        if dx != Nm::ZERO || dy != Nm::ZERO {
            self.move_items(&[a.to_string()], dx, dy)?;
            self.move_items(&[b.to_string()], -dx, -dy)?;
        }
        Ok(())
    }

    fn swap_layer(&mut self, id: &str, t: SwapType) -> Option<String> {
        let rt = self.design.routing.as_ref();
        let dr = self.design.drawings.as_ref();
        match t {
            SwapType::Via => rt?.vias.iter().find(|v| v.id == id).map(|v| v.from_layer.clone()),
            SwapType::Arc => rt?.tracks.iter().find(|t| t.id == id).map(|t| t.layer.clone()),
            SwapType::Zone => rt?.zones.iter().find(|z| z.id == id).and_then(zone_layer),
            SwapType::Field => self.resolve_field(id).and_then(|(r, n)| self.field_layout(&r, &n)).map(|l| l.layer),
            SwapType::Text if self.resolve_fp_text(id).is_some() => self.fp_text_layer(id),
            SwapType::Text => dr?.texts.iter().find(|t| t.id == id).map(|t| t.layer.clone()),
            SwapType::Shape => dr?.shapes.iter().find(|s| s.id() == id).map(|s| s.layer().to_string()),
            SwapType::DimAligned | SwapType::DimOrthogonal | SwapType::DimRadial | SwapType::DimCenter | SwapType::DimLeader => dr?.dimensions.iter().find(|d| d.id == id).map(|d| d.layer.clone()),
            SwapType::TextBox => dr?.board_items.text_boxes.iter().find(|x| x.id == id).map(|x| x.layer.clone()),
            SwapType::Point => dr?.board_items.points.iter().find(|x| x.id == id).map(|x| x.layer.clone()),
            SwapType::Image => dr?.board_items.images.iter().find(|x| x.id == id).map(|x| x.layer.clone()),
            SwapType::Barcode => dr?.barcodes.iter().find(|x| x.id == id).map(|x| x.layer.clone()),
            SwapType::Table | SwapType::DrillChart => dr?.tables.iter().find(|x| x.id == id).map(|x| x.layer.clone()),
            // `PCB_TUNING_PATTERN::GetLayer`: its first track's layer, else its own; a via stitching's own (`BOARD_ITEM::m_layer`)
            SwapType::Generator => {
                if let Some(p) = dr?.tuning_patterns.iter().find(|p| p.id == id) {
                    let tracks = rt.map(|rt| rt.tracks.as_slice()).unwrap_or_default();
                    return Some(p.member_ids.iter().find_map(|m| tracks.iter().find(|t| &t.id == m)).map_or_else(|| p.layer.clone(), |t| t.layer.clone()));
                }
                dr?.board_items.via_stitches.iter().find(|v| v.id == id).map(|v| v.layer.clone())
            }
            SwapType::DrillMap => dr?.board_items.drill_maps.iter().find(|m| m.id == id).map(|m| m.layer.clone()),
            // 25ec2335 `PCB_GRID_ITEM::GetLayer`: "Grids don't have layers": `UNDEFINED_LAYER`
            SwapType::Grid | SwapType::Footprint | SwapType::Pad | SwapType::Group => None,
        }
    }

    fn swap_set_layer(&mut self, id: &str, t: SwapType, layer: Option<&str>) -> Result<(), Vec<CheckResult>> {
        if t == SwapType::Zone {
            if let Some(z) = self.design.routing.as_mut().and_then(|rt| rt.zones.iter_mut().find(|z| z.id == id)) {
                set_zone_layer(z, layer);
            }
            return Ok(());
        }
        // Every other kind has a layer: `None` only comes from a zone, which is no other kind's pair.
        let Some(layer) = layer else { return Ok(()) };
        match t {
            SwapType::Via => self.set_via_start(id, layer),
            SwapType::Arc => {
                if let Some(tr) = self.design.routing.as_mut().and_then(|rt| rt.tracks.iter_mut().find(|t| t.id == id)) {
                    tr.layer = layer.to_string();
                }
            }
            SwapType::Field => {
                let (r, n) = self.resolve_field(id).ok_or_else(|| fail("ops_unknown_item", id, "no field with this id"))?;
                let mut l = self.field_layout(&r, &n).expect("the field was resolved");
                l.layer = layer.to_string();
                self.store_layout(&r, &n, l);
            }
            SwapType::Text if self.resolve_fp_text(id).is_some() => {
                let (r, k) = self.resolve_fp_text(id).expect("checked");
                // kept as for a top-side footprint (`fp_art::layer_on_side`)
                self.with_own_copy(&r, |fp, pose| {
                    if let Some(t) = fp.art.texts.get_mut(k) {
                        t.layout.layer = eda_model::fp_art::layer_on_side(layer, pose.side);
                    }
                    Ok(())
                })?;
            }
            SwapType::Text => {
                if let Some(t) = self.drawings_mut().texts.iter_mut().find(|t| t.id == id) {
                    t.layer = layer.to_string();
                }
            }
            SwapType::Shape => {
                let dr = self.drawings_mut();
                if let Some(s) = dr.shapes.iter_mut().find(|s| s.id() == id) {
                    s.set_layer(layer.to_string());
                }
                // `PCB_SHAPE::SetLayer`: `if( !IsOnCopperLayer() ) SetNetCode( -1 );`
                if !eda_model::layers::is_copper_layer_name(layer) {
                    dr.board_items.set_net(id, None);
                }
            }
            SwapType::DimAligned | SwapType::DimOrthogonal | SwapType::DimRadial | SwapType::DimCenter | SwapType::DimLeader => {
                if let Some(d) = self.drawings_mut().dimensions.iter_mut().find(|d| d.id == id) {
                    d.layer = layer.to_string();
                }
            }
            SwapType::TextBox => {
                if let Some(x) = self.drawings_mut().board_items.text_boxes.iter_mut().find(|x| x.id == id) {
                    x.layer = layer.to_string();
                }
            }
            SwapType::Point => {
                if let Some(x) = self.drawings_mut().board_items.points.iter_mut().find(|x| x.id == id) {
                    x.layer = layer.to_string();
                }
            }
            SwapType::Image => {
                if let Some(x) = self.drawings_mut().board_items.images.iter_mut().find(|x| x.id == id) {
                    x.layer = layer.to_string();
                }
            }
            SwapType::Barcode => {
                if let Some(x) = self.drawings_mut().barcodes.iter_mut().find(|x| x.id == id) {
                    x.layer = layer.to_string();
                }
            }
            // `PCB_TABLE::SetLayer`: the table and each of its cells.
            SwapType::Table | SwapType::DrillChart => {
                if let Some(x) = self.drawings_mut().tables.iter_mut().find(|x| x.id == id) {
                    x.layer = layer.to_string();
                }
            }
            // `PCB_TUNING_PATTERN::SetLayer`: its own and each of its tracks'; a via stitching's own alone (`PCB_GENERATOR::SetLayer`)
            SwapType::Generator => {
                let members = self.design.drawings.as_ref().and_then(|d| d.tuning_patterns.iter().find(|p| p.id == id)).map(|p| p.member_ids.clone());
                let dr = self.drawings_mut();
                if let Some(p) = dr.tuning_patterns.iter_mut().find(|p| p.id == id) {
                    p.layer = layer.to_string();
                } else if let Some(v) = dr.board_items.via_stitches.iter_mut().find(|v| v.id == id) {
                    v.layer = layer.to_string();
                }
                if let (Some(members), Some(rt)) = (members, self.design.routing.as_mut()) {
                    for t in rt.tracks.iter_mut().filter(|t| members.contains(&t.id)) {
                        t.layer = layer.to_string();
                    }
                }
            }
            SwapType::DrillMap => {
                if let Some(m) = self.drawings_mut().board_items.drill_maps.iter_mut().find(|m| m.id == id) {
                    m.layer = layer.to_string();
                }
            }
            SwapType::Zone | SwapType::Footprint | SwapType::Pad | SwapType::Group | SwapType::Grid => {}
        }
        Ok(())
    }

    /// The footprints' part of a pair, their positions swapped already:
    ///
    /// ```text
    /// EDA_ANGLE aAngle = aFP->GetOrientation(), bAngle = bFP->GetOrientation();
    /// if( aFP->IsFlipped() != bFP->IsFlipped() ) { aFP->Flip( aPos, TOP_BOTTOM ); bFP->Flip( bPos, TOP_BOTTOM ); }
    /// std::swap( aAngle, bAngle ); aFP->SetOrientation( aAngle ); bFP->SetOrientation( bAngle );
    /// ```
    ///
    /// Each footprint turns over about its own (new) position, its fields and user-layer drawings with it ([`Board::flip_items`], `FOOTPRINT::Flip`), then takes the
    /// other's orientation.
    fn swap_footprint_sides(&mut self, a: &str, b: &str) -> Result<(), Vec<CheckResult>> {
        let (fa, fb) = (self.require_placed(a)?, self.require_placed(b)?);
        if fa.side != fb.side {
            self.flip_items(&[a.to_string()], fa.at, FlipDirection::TopBottom)?;
            self.flip_items(&[b.to_string()], fb.at, FlipDirection::TopBottom)?;
        }
        let (at_a, at_b) = (self.require_placed(a)?.at, self.require_placed(b)?.at);
        self.set_pose_exact(a, at_a, fb.rot)?;
        self.set_pose_exact(b, at_b, fa.rot)?;
        Ok(())
    }
}

// ------------------------------------------------------------------------------------------------- the Footprint Editor

/// The footprint open in the Footprint Editor as [`swap`]'s host. KiCad's Footprint Editor runs the same `EDIT_TOOL::Swap` with `m_isFootprintEditor`: a pad stays a pad
/// (`FilterCollectorForFreePads`) and nothing is locked out (`FilterCollectorForLockedItems` only works in the board editor), so two pads swap as pads.
pub(crate) struct FpSwap<'a>(pub(crate) &'a mut LibraryFootprint);

impl FpSwap<'_> {
    fn move_by(&mut self, id: &str, t: SwapType, dx: Nm, dy: Nm) {
        let ids: BTreeSet<String> = if t == SwapType::Group { GroupTree(&self.0.items.groups).group_leaves(id).into_iter().collect() } else { BTreeSet::from([id.to_string()]) };
        transform_ids(self.0, &ids, Xform::Move { dx, dy });
    }
}

impl SwapHost for FpSwap<'_> {
    fn swap_type(&self, id: &str) -> Result<SwapType, Vec<CheckResult>> {
        let fp = &*self.0;
        let it = &fp.items;
        let t = if fp.pads.iter().any(|p| p.id == id) {
            SwapType::Pad
        } else if fp.graphics.iter().any(|g| g.id() == id) {
            SwapType::Shape
        } else if fp.texts.iter().any(|t| t.id == id) {
            SwapType::Text
        } else if field_name(id).is_some() && live_ids(fp).contains(id) {
            SwapType::Field
        } else if let Some(d) = it.dimensions.iter().find(|d| d.id == id) {
            dimension_type(&d.kind)
        } else if it.zones.iter().any(|z| z.id == id) {
            SwapType::Zone
        } else if it.text_boxes.iter().any(|x| x.id == id) {
            SwapType::TextBox
        } else if it.points.iter().any(|x| x.id == id) {
            SwapType::Point
        } else if it.images.iter().any(|x| x.id == id) {
            SwapType::Image
        } else if it.barcodes.iter().any(|x| x.id == id) {
            SwapType::Barcode
        } else if it.tables.iter().any(|x| x.id == id) {
            SwapType::Table
        } else if it.groups.iter().any(|g| g.id == id) {
            SwapType::Group
        } else {
            return Err(fail("ops_unknown_item", id, "no item of this footprint has this id"));
        };
        Ok(t)
    }

    fn swap_position(&mut self, id: &str, t: SwapType) -> Point {
        let fp = &mut *self.0;
        let found = match t {
            SwapType::Pad => fp.pads.iter().find(|p| p.id == id).map(|p| p.at),
            SwapType::Shape => fp.graphics.iter().find(|g| g.id() == id).map(shape_position),
            SwapType::Text => fp.texts.iter().find(|t| t.id == id).map(|t| t.at),
            SwapType::Field => field_name(id).and_then(|n| field_layout_mut(fp, n)).map(|l| l.at),
            SwapType::DimAligned | SwapType::DimOrthogonal | SwapType::DimRadial | SwapType::DimCenter | SwapType::DimLeader => fp.items.dimensions.iter().find(|d| d.id == id).map(|d| d.start),
            SwapType::Zone => fp.items.zones.iter().find(|z| z.id == id).and_then(|z| z.outline.first().copied()),
            SwapType::TextBox => fp.items.text_boxes.iter().find(|x| x.id == id).map(text_box_position),
            SwapType::Point => fp.items.points.iter().find(|x| x.id == id).map(|x| x.at),
            SwapType::Image => fp.items.images.iter().find(|x| x.id == id).map(|x| x.at),
            SwapType::Barcode => fp.items.barcodes.iter().find(|x| x.id == id).map(|x| x.at),
            SwapType::Table => fp.items.tables.iter().find(|x| x.id == id).map(|x| x.at),
            SwapType::Footprint | SwapType::Via | SwapType::Arc | SwapType::Group | SwapType::Generator | SwapType::Grid | SwapType::DrillMap | SwapType::DrillChart => None,
        };
        found.unwrap_or_default()
    }

    fn swap_set_position(&mut self, id: &str, t: SwapType, at: Point, from: Point) -> Result<(), Vec<CheckResult>> {
        match t {
            SwapType::Zone => {}
            SwapType::DimAligned | SwapType::DimOrthogonal | SwapType::DimRadial | SwapType::DimCenter | SwapType::DimLeader => {
                if let Some(d) = self.0.items.dimensions.iter_mut().find(|d| d.id == id) {
                    d.start = at;
                }
            }
            _ => {
                let (dx, dy) = delta(at, from);
                if dx != Nm::ZERO || dy != Nm::ZERO {
                    self.move_by(id, t, dx, dy);
                }
            }
        }
        Ok(())
    }

    fn swap_layer(&mut self, id: &str, t: SwapType) -> Option<String> {
        let fp = &mut *self.0;
        let it = &fp.items;
        match t {
            SwapType::Shape => fp.graphics.iter().find(|g| g.id() == id).map(|g| g.layer().to_string()),
            SwapType::Text => fp.texts.iter().find(|t| t.id == id).map(|t| t.layer.clone()),
            SwapType::Field => field_name(id).and_then(|n| field_layout_mut(fp, n)).map(|l| l.layer.clone()),
            SwapType::DimAligned | SwapType::DimOrthogonal | SwapType::DimRadial | SwapType::DimCenter | SwapType::DimLeader => it.dimensions.iter().find(|d| d.id == id).map(|d| d.layer.clone()),
            SwapType::Zone => it.zones.iter().find(|z| z.id == id).and_then(zone_layer),
            SwapType::TextBox => it.text_boxes.iter().find(|x| x.id == id).map(|x| x.layer.clone()),
            SwapType::Point => it.points.iter().find(|x| x.id == id).map(|x| x.layer.clone()),
            SwapType::Image => it.images.iter().find(|x| x.id == id).map(|x| x.layer.clone()),
            SwapType::Barcode => it.barcodes.iter().find(|x| x.id == id).map(|x| x.layer.clone()),
            SwapType::Table => it.tables.iter().find(|x| x.id == id).map(|x| x.layer.clone()),
            SwapType::Pad | SwapType::Footprint | SwapType::Via | SwapType::Arc | SwapType::Group | SwapType::Generator | SwapType::Grid | SwapType::DrillMap | SwapType::DrillChart => None,
        }
    }

    fn swap_set_layer(&mut self, id: &str, t: SwapType, layer: Option<&str>) -> Result<(), Vec<CheckResult>> {
        let fp = &mut *self.0;
        if t == SwapType::Zone {
            if let Some(z) = fp.items.zones.iter_mut().find(|z| z.id == id) {
                set_zone_layer(z, layer);
            }
            return Ok(());
        }
        let Some(layer) = layer else { return Ok(()) };
        let l = layer.to_string();
        let it = &mut fp.items;
        match t {
            SwapType::Shape => {
                if let Some(g) = fp.graphics.iter_mut().find(|g| g.id() == id) {
                    g.set_layer(l);
                }
            }
            SwapType::Text => {
                if let Some(x) = fp.texts.iter_mut().find(|t| t.id == id) {
                    x.layer = l;
                }
            }
            SwapType::Field => {
                if let Some(x) = field_name(id).and_then(|n| field_layout_mut(fp, n)) {
                    x.layer = l;
                }
            }
            SwapType::DimAligned | SwapType::DimOrthogonal | SwapType::DimRadial | SwapType::DimCenter | SwapType::DimLeader => {
                if let Some(x) = it.dimensions.iter_mut().find(|d| d.id == id) {
                    x.layer = l;
                }
            }
            SwapType::TextBox => {
                if let Some(x) = it.text_boxes.iter_mut().find(|x| x.id == id) {
                    x.layer = l;
                }
            }
            SwapType::Point => {
                if let Some(x) = it.points.iter_mut().find(|x| x.id == id) {
                    x.layer = l;
                }
            }
            SwapType::Image => {
                if let Some(x) = it.images.iter_mut().find(|x| x.id == id) {
                    x.layer = l;
                }
            }
            SwapType::Barcode => {
                if let Some(x) = it.barcodes.iter_mut().find(|x| x.id == id) {
                    x.layer = l;
                }
            }
            SwapType::Table => {
                if let Some(x) = it.tables.iter_mut().find(|x| x.id == id) {
                    x.layer = l;
                }
            }
            SwapType::Zone | SwapType::Pad | SwapType::Footprint | SwapType::Via | SwapType::Arc | SwapType::Group | SwapType::Generator | SwapType::Grid | SwapType::DrillMap | SwapType::DrillChart => {}
        }
        Ok(())
    }

    /// `PAD::SwapShapePositions`: each pad's anchor goes where the other's copper is drawn, less its own offset turned with it, so the two shapes trade places:
    ///
    /// ```text
    /// lhsShapePos = aLhs->ShapePos( ALL_LAYERS ); rhsShapePos = aRhs->ShapePos( ALL_LAYERS );
    /// lhsOffset = aLhs->GetOffset( ALL_LAYERS ); RotatePoint( lhsOffset, aLhs->GetOrientation() );   (and the same for rhs)
    /// aLhs->SetPosition( rhsShapePos - lhsOffset ); aRhs->SetPosition( lhsShapePos - rhsOffset );
    /// ```
    fn swap_pad_shapes(&mut self, a: &str, b: &str) -> Result<(), Vec<CheckResult>> {
        let pads = &mut self.0.pads;
        let ia = pads.iter().position(|p| p.id == a).ok_or_else(|| fail("ops_unknown_item", a, "no pad with this id"))?;
        let ib = pads.iter().position(|p| p.id == b).ok_or_else(|| fail("ops_unknown_item", b, "no pad with this id"))?;
        // `ShapePos` and the offset turned as `Pad::rot` runs (crate::pad_edit's pad frame).
        let off = |i: usize| crate::pad_edit::rotate(pads[i].offset, pads[i].rot as i64);
        let (oa, ob) = (off(ia), off(ib));
        let (sa, sb) = (Point { x: pads[ia].at.x + oa.x, y: pads[ia].at.y + oa.y }, Point { x: pads[ib].at.x + ob.x, y: pads[ib].at.y + ob.y });
        pads[ia].at = Point { x: sb.x - oa.x, y: sb.y - oa.y };
        pads[ib].at = Point { x: sa.x - ob.x, y: sa.y - ob.y };
        Ok(())
    }
}

/// Whether `id` names one of `table`'s cells (the studio's `TABLE:ROW:COL`).
pub(crate) fn has_cell(table: &eda_model::table::Table, id: &str) -> bool {
    id.strip_prefix(table.id.as_str()).and_then(|rest| rest.strip_prefix(':')).is_some_and(|rc| rc.split(':').count() == 2 && rc.split(':').all(|n| n.parse::<u32>().is_ok()))
}
