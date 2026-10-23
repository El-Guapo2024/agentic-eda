//! A pad's padstack: the shape it has on each copper layer, and what becomes of the layers nothing connects to.
//!
//! KiCad's `PADSTACK` (`pcbnew/padstack.h`, `padstack.cpp`) holds a pad's copper geometry per layer: `MODE::NORMAL` is one shape on every layer (the
//! fields of [`Pad`] itself), `MODE::FRONT_INNER_BACK` a shape on the front, one on all the inner layers and one on the back, `MODE::CUSTOM` a shape on
//! each copper layer. Its `UNCONNECTED_LAYER_MODE` says which layers of a plated through-hole pad keep their copper when nothing is connected there
//! (`remove_unused_layers`, `keep_end_layers`; `PAD::FlashLayer`).
//!
//! The pad's own fields ([`Pad::shape`], [`Pad::size`], [`Pad::roundrect_ratio`] and [`PadShapeExt`]) are its front shape, the one `PADSTACK::ALL_LAYERS`
//! (`F_Cu`) holds, as seen on the footprint's own side. What the other layers have beyond it is [`PadStack`], a field of [`PadShapeExt`]: with
//! `PadStackMode::Normal` and `UnconnectedLayers::KeepAll` (the default) it is not written anywhere and a pad is what it always was.
//!
//! Layers are named as for a footprint on the top side (the footprint seen from above, like every coordinate of a pad): `F.Cu` is the footprint's own side,
//! `B.Cu` the other, `In1.Cu` ... the inner ones. A footprint on the bottom side has them the other way round on the board (`PADSTACK::FlipLayers`):
//! [`Pad::shape_groups`] takes the side and gives the shapes the board's layers have.

use crate::footprint::{Pad, PadCustom, PadKind, PadShape, PadShapeExt};
use crate::ir::{ChamferCorners, Side, Nm};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `PADSTACK::MODE`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadStackMode {
    /// One shape on every layer: the pad's own fields.
    #[default]
    Normal,
    /// `MODE::FRONT_INNER_BACK`: the pad's own shape on the front, [`PadStack::inner`] on every inner layer, [`PadStack::back`] on the back.
    FrontInnerBack,
    /// `MODE::CUSTOM`: a shape on each copper layer, [`PadStack::layers`] by name; a layer it does not name has the pad's own.
    Custom,
}

impl PadStackMode {
    pub fn is_normal(&self) -> bool {
        *self == PadStackMode::Normal
    }
}

/// `UNCONNECTED_LAYER_MODE`: which copper layers of a plated through-hole pad (or a via) keep their copper when nothing is connected to the pad on them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnconnectedLayers {
    /// All copper layers (`remove_unused_layers no`).
    #[default]
    KeepAll,
    /// Connected layers only (`remove_unused_layers yes`, `keep_end_layers no`).
    RemoveAll,
    /// Front, back and connected layers (`remove_unused_layers yes`, `keep_end_layers yes`).
    RemoveExceptStartAndEnd,
    /// Start and end layers only (a via's `start_end_only`).
    StartEndOnly,
}

impl UnconnectedLayers {
    pub fn is_keep_all(&self) -> bool {
        *self == UnconnectedLayers::KeepAll
    }

    /// `PAD::GetRemoveUnconnected`: anything but keeping every layer.
    pub fn removes(&self) -> bool {
        !self.is_keep_all()
    }

    /// `PAD::GetKeepTopBottom`.
    pub fn keeps_outer(&self) -> bool {
        *self == UnconnectedLayers::RemoveExceptStartAndEnd
    }

    /// The mode the two file tokens say, read in file order as `parsePAD` does: `(remove_unused_layers v)` is `SetRemoveUnconnected( v )` (`REMOVE_ALL` or
    /// `KEEP_ALL`) and `(keep_end_layers v)` is `SetKeepTopBottom( v )` (`REMOVE_EXCEPT_START_AND_END` or `REMOVE_ALL`), whatever came before it.
    pub fn after_remove_unused_layers(v: bool) -> UnconnectedLayers {
        if v {
            UnconnectedLayers::RemoveAll
        } else {
            UnconnectedLayers::KeepAll
        }
    }

    pub fn after_keep_end_layers(v: bool) -> UnconnectedLayers {
        if v {
            UnconnectedLayers::RemoveExceptStartAndEnd
        } else {
            UnconnectedLayers::RemoveAll
        }
    }

    /// The two settings of the Remove Unused Pads dialog: remove what is unconnected, keeping the outside layers or not (`SetRemoveUnconnected( true )`
    /// and `SetKeepTopBottom( keep )`).
    pub fn remove(keep_outside: bool) -> UnconnectedLayers {
        if keep_outside {
            UnconnectedLayers::RemoveExceptStartAndEnd
        } else {
            UnconnectedLayers::RemoveAll
        }
    }
}

/// `PAD::FlashLayer`'s verdict from the pad's own settings: the layers where it does not depend on what is connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flash {
    /// The pad's copper is on the layer.
    Always,
    /// It is not.
    Never,
    /// It is when a track, via or pad of the pad's net is connected to it on that layer (`CONNECTIVITY_DATA::IsConnectedOnLayer`).
    IfConnected,
}

/// One copper layer of a padstack: `PADSTACK::COPPER_LAYER_PROPS` (its `SHAPE_PROPS` and its custom shapes). The same fields a [`Pad`] has for its own shape, in
/// the pad's own frame (the footprint's as seen from the top, the pad's rotation not applied).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PadLayerShape {
    pub shape: PadShape,
    /// The shape's size before the pad's rotation; a custom pad's anchor's.
    #[serde(deserialize_with = "crate::units::de")]
    pub size: (Nm, Nm),
    /// `roundrect_rratio`, as [`Pad::roundrect_ratio`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roundrect_ratio: Option<f64>,
    /// `PAD::GetOffset( layer )`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::units::de")]
    pub offset: Option<(Nm, Nm)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::units::de")]
    pub trapezoid_delta: Option<(Nm, Nm)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chamfer_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "crate::ir::ChamferCorners::is_none")]
    pub chamfer_corners: ChamferCorners,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<PadCustom>,
    /// What the layer's node of the file says beyond its shape -- `(thermal_bridge_angle ..)`, `(thermal_gap ..)`, `(thermal_bridge_width ..)`, `(clearance ..)`,
    /// `(zone_connect ..)`, `(tenting ..)` -- as the file wrote it, in file order. The model does not read them; the writer gives them back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tokens: Vec<String>,
}

impl PadLayerShape {
    /// The shape a pad has on its own side: its fields.
    pub fn of(pad: &Pad) -> PadLayerShape {
        PadLayerShape {
            shape: pad.shape,
            size: pad.size,
            roundrect_ratio: pad.roundrect_ratio,
            offset: pad.ext.offset,
            trapezoid_delta: pad.ext.trapezoid_delta,
            chamfer_ratio: pad.ext.chamfer_ratio,
            chamfer_corners: pad.ext.chamfer_corners,
            custom: pad.ext.custom.clone(),
            tokens: Vec::new(),
        }
    }

    /// A shape of nothing but `shape` and `size`, which is what a layer node that says no more than that reads as (`SHAPE_PROPS()`: no ratio, no offset).
    pub fn plain(shape: PadShape, size: (Nm, Nm)) -> PadLayerShape {
        PadLayerShape { shape, size, roundrect_ratio: None, offset: None, trapezoid_delta: None, chamfer_ratio: None, chamfer_corners: ChamferCorners::default(), custom: None, tokens: Vec::new() }
    }

    /// Lay this shape on `pad`: the pad as it is on this layer. The pad's padstack is left out (the result is one shape for every layer).
    pub fn lay_on(&self, pad: &Pad) -> Pad {
        let mut p = pad.clone();
        p.shape = self.shape;
        p.size = self.size;
        p.roundrect_ratio = self.roundrect_ratio;
        p.ext = PadShapeExt {
            offset: self.offset,
            trapezoid_delta: self.trapezoid_delta,
            chamfer_ratio: self.chamfer_ratio,
            chamfer_corners: self.chamfer_corners,
            custom: self.custom.clone(),
            stack: PadStack { mode: PadStackMode::Normal, unconnected: pad.ext.stack.unconnected, ..Default::default() },
        };
        p
    }

    /// Put this shape in the pad's own fields (the front shape).
    pub fn put_on(&self, pad: &mut Pad) {
        pad.shape = self.shape;
        pad.size = self.size;
        pad.roundrect_ratio = self.roundrect_ratio;
        pad.ext.offset = self.offset;
        pad.ext.trapezoid_delta = self.trapezoid_delta;
        pad.ext.chamfer_ratio = self.chamfer_ratio;
        pad.ext.chamfer_corners = self.chamfer_corners;
        pad.ext.custom = self.custom.clone();
    }

    /// What belongs to one shape goes with it, as `apply_pad_edit` keeps a pad: a circle is square, a rounded or chamfered rectangle has a corner ratio, a
    /// trapezoid its delta, a chamfer its ratio and corners, a custom pad its shapes.
    pub fn normalize(&mut self) {
        if self.shape == PadShape::Circle || (self.shape == PadShape::Custom && self.custom.as_ref().is_none_or(|c| c.anchor == crate::footprint::PadAnchor::Circle)) {
            self.size.1 = self.size.0;
        }
        if matches!(self.shape, PadShape::RoundRect | PadShape::ChamferedRect) {
            if self.roundrect_ratio.is_none() {
                self.roundrect_ratio = Some(0.25);
            }
        } else {
            self.roundrect_ratio = None;
        }
        if self.shape == PadShape::ChamferedRect {
            if self.chamfer_ratio.is_none() {
                self.chamfer_ratio = Some(0.2);
            }
        } else {
            self.chamfer_ratio = None;
            self.chamfer_corners = ChamferCorners::default();
        }
        if self.shape != PadShape::Trapezoid {
            self.trapezoid_delta = None;
        }
        self.trapezoid_delta = self.trapezoid_delta.filter(|d| *d != (0, 0));
        self.offset = self.offset.filter(|o| *o != (0, 0));
        if self.shape == PadShape::Custom {
            if self.custom.is_none() {
                self.custom = Some(PadCustom::default());
            }
        } else {
            self.custom = None;
        }
    }

    /// The layer's bounding box in the pad's frame `(x0, y0, x1, y1)`, the pad's position the origin and its rotation not applied
    /// (`PAD::GetBoundingBox( layer )` of a pad at the origin).
    pub fn local_bbox(&self, pad: &Pad) -> (f64, f64, f64, f64) {
        self.lay_on(pad).local_bbox()
    }
}

/// `PADSTACK` without what the pad's own fields hold. See the module doc.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PadStack {
    #[serde(default, skip_serializing_if = "PadStackMode::is_normal")]
    pub mode: PadStackMode,
    /// `FrontInnerBack`: the shape on every inner layer. `None`: the pad's own (a layer the file does not give has the front's shape).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner: Option<PadLayerShape>,
    /// `FrontInnerBack`: the shape on the back, the footprint's other side. `None`: the pad's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back: Option<PadLayerShape>,
    /// `Custom`: the shape of each copper layer but the front, named as for a footprint on the top side (`In1.Cu`, `B.Cu`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub layers: BTreeMap<String, PadLayerShape>,
    /// `UNCONNECTED_LAYER_MODE`.
    #[serde(default, skip_serializing_if = "UnconnectedLayers::is_keep_all")]
    pub unconnected: UnconnectedLayers,
}

impl PadStack {
    /// Nothing beyond one shape for every layer and every layer kept.
    pub fn is_default(&self) -> bool {
        *self == PadStack::default()
    }

    /// Whether the layers can differ (`MODE` is not `NORMAL`).
    pub fn is_layered(&self) -> bool {
        self.mode != PadStackMode::Normal
    }
}

/// The copper layers that share one shape of a pad, and that shape.
#[derive(Debug, Clone, PartialEq)]
pub struct PadShapeGroup {
    /// The board's copper layers it is on, front to back.
    pub layers: Vec<String>,
    /// Which shape of the padstack: `F.Cu` for the pad's own, `Inner` for the inner layers of a front/inner/back stack, `B.Cu` for the back of one and, for a custom
    /// stack, the name of the layer (as for a footprint on the top side).
    pub role: String,
    pub shape: PadLayerShape,
}

/// `::FlipLayer( aLayerId, aCopperLayersCount )` for a copper layer name: front and back swap and, from four copper layers up, the inner ones mirror through the
/// middle of the stack (`In1` <-> `In2` on four, `In1` <-> `In4` and `In2` <-> `In3` on six). Any other name is its own.
pub fn flip_copper_layer(layer: &str, copper_layers: usize) -> String {
    match layer {
        "F.Cu" => "B.Cu".to_string(),
        "B.Cu" => "F.Cu".to_string(),
        _ => {
            if copper_layers >= 4 {
                if let Some(n) = layer.strip_prefix("In").and_then(|r| r.strip_suffix(".Cu")).and_then(|d| d.parse::<i64>().ok()) {
                    let max_index = copper_layers as i64 - 3;
                    let flipped = (max_index - (n - 1)).clamp(0, max_index);
                    return format!("In{}.Cu", flipped + 1);
                }
            }
            layer.to_string()
        }
    }
}

/// The name a board layer has in a footprint's own frame: the board's for a footprint on the top, the flipped one for a footprint on the bottom.
pub fn role_of_layer(board_layer: &str, side: Side, copper_layers: usize) -> String {
    if side == Side::Bottom {
        flip_copper_layer(board_layer, copper_layers)
    } else {
        board_layer.to_string()
    }
}

/// `INNER` stands for every inner layer of a front/inner/back stack.
const INNER: &str = "Inner";

impl Pad {
    /// Whether the pad's copper differs from layer to layer (`PADSTACK::Mode() != NORMAL`).
    pub fn has_stack(&self) -> bool {
        self.ext.stack.is_layered()
    }

    /// The shape the pad has on the layer named `role` (as for a footprint on the top side): the pad's own for `F.Cu` and for every layer of a normal stack, else
    /// what the stack holds for it (`PADSTACK::CopperLayer( layer ) const` and its fallbacks: a layer the stack does not name has the pad's own).
    pub fn shape_for_role(&self, role: &str) -> PadLayerShape {
        let own = || PadLayerShape::of(self);
        let stack = &self.ext.stack;
        match stack.mode {
            PadStackMode::Normal => own(),
            PadStackMode::FrontInnerBack => match role {
                "F.Cu" => own(),
                "B.Cu" => stack.back.clone().unwrap_or_else(own),
                _ => stack.inner.clone().unwrap_or_else(own),
            },
            PadStackMode::Custom => {
                if role == "F.Cu" {
                    own()
                } else {
                    stack.layers.get(role).cloned().unwrap_or_else(own)
                }
            }
        }
    }

    /// The copper shapes of the pad on a footprint on `side`, each with the board's copper layers (`board_layers`, front to back) it is on: what
    /// `PADSTACK::ForEachUniqueLayer` and `EffectiveLayerFor` make of the stack. A surface-mount pad has its one layer (the first or last of `board_layers` by
    /// the side it is on), a normal stack one shape for every layer; a front/inner/back stack gives up to three (the inner layers, when the board has any, are one
    /// group), a custom one a group for each layer it names and one for all the rest.
    pub fn shape_groups(&self, side: Side, board_layers: &[String]) -> Vec<PadShapeGroup> {
        let (Some(first), Some(last)) = (board_layers.first(), board_layers.last()) else { return Vec::new() };
        if self.kind == PadKind::Smd {
            let layer = if self.on_back(side) { last } else { first };
            return vec![PadShapeGroup { layers: vec![layer.clone()], role: "F.Cu".into(), shape: PadLayerShape::of(self) }];
        }
        let n = board_layers.len();
        let mut groups: Vec<PadShapeGroup> = Vec::new();
        for layer in board_layers {
            let role = role_of_layer(layer, side, n);
            // The key of the shape this layer has: layers with the same key share a group.
            let key = match self.ext.stack.mode {
                PadStackMode::Normal => "F.Cu".to_string(),
                PadStackMode::FrontInnerBack => {
                    if role == "F.Cu" || role == "B.Cu" {
                        role.clone()
                    } else {
                        INNER.to_string()
                    }
                }
                PadStackMode::Custom => {
                    if role != "F.Cu" && self.ext.stack.layers.contains_key(&role) {
                        role.clone()
                    } else {
                        "F.Cu".to_string()
                    }
                }
            };
            match groups.iter_mut().find(|g| g.role == key) {
                Some(g) => g.layers.push(layer.clone()),
                None => {
                    let shape = match key.as_str() {
                        INNER => self.shape_for_role("In1.Cu"),
                        other => self.shape_for_role(other),
                    };
                    groups.push(PadShapeGroup { layers: vec![layer.clone()], role: key, shape });
                }
            }
        }
        groups
    }

    /// What the file's main fields (`(shape ..) (size ..) (rect_delta ..) (options ..)` and the rest of the pad's own form) hold for a footprint on `side`: the pad's
    /// front shape on the top; on the bottom the front of the board is the footprint's *other* side (`PADSTACK::FlipLayers`), so it is the shape the back has.
    pub fn file_main_shape(&self, side: Side) -> PadLayerShape {
        if side == Side::Bottom && self.has_stack() {
            self.shape_for_role("B.Cu")
        } else {
            PadLayerShape::of(self)
        }
    }

    /// Every distinct copper shape of the pad, its own first (the pad's `ForEachUniqueLayer`): a pad of a normal stack has one.
    pub fn unique_shapes(&self) -> Vec<PadLayerShape> {
        let mut out = vec![PadLayerShape::of(self)];
        match self.ext.stack.mode {
            PadStackMode::Normal => {}
            PadStackMode::FrontInnerBack => {
                out.push(self.shape_for_role("In1.Cu"));
                out.push(self.shape_for_role("B.Cu"));
            }
            PadStackMode::Custom => out.extend(self.ext.stack.layers.values().cloned()),
        }
        out
    }

    /// `PAD::FlashLayer( layer )` from the pad's own settings (`first` and `last` the board's outer copper layers): the layers a plated through-hole pad keeps
    /// its copper on whatever is connected, the ones it never has it on, and the ones where it has it when something is connected to it there.
    /// A pad that is not a plated through-hole pad always has its copper (`UnconnectedLayers` is about those).
    pub fn flash(&self, layer: &str, first: &str, last: &str) -> Flash {
        if self.kind != PadKind::ThroughHole {
            return Flash::Always;
        }
        let outer = layer == first || layer == last;
        match self.ext.stack.unconnected {
            UnconnectedLayers::KeepAll => Flash::Always,
            UnconnectedLayers::StartEndOnly => {
                if outer {
                    Flash::Always
                } else {
                    Flash::Never
                }
            }
            UnconnectedLayers::RemoveExceptStartAndEnd if outer => Flash::Always,
            UnconnectedLayers::RemoveExceptStartAndEnd | UnconnectedLayers::RemoveAll => Flash::IfConnected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::footprint::PadKind;

    fn layers(n: usize) -> Vec<String> {
        let mut v = vec!["F.Cu".to_string()];
        for i in 1..=n.saturating_sub(2) {
            v.push(format!("In{i}.Cu"));
        }
        v.push("B.Cu".to_string());
        v
    }

    fn pad(kind: PadKind) -> Pad {
        Pad { number: "1".into(), at: (0, 0), size: (1_600_000, 1_600_000), shape: PadShape::Circle, kind, drill: Some(800_000), drill_slot: None, rot: 0, roundrect_ratio: None, opposite_side: false, ext: Default::default() }
    }

    fn oval() -> PadLayerShape {
        PadLayerShape::plain(PadShape::Oval, (2_000_000, 1_000_000))
    }

    fn rect() -> PadLayerShape {
        PadLayerShape::plain(PadShape::Rect, (1_200_000, 1_200_000))
    }

    fn groups(p: &Pad, side: Side, n: usize) -> Vec<(Vec<String>, PadShape)> {
        p.shape_groups(side, &layers(n)).into_iter().map(|g| (g.layers, g.shape.shape)).collect()
    }

    #[test]
    fn a_normal_stack_is_one_shape_on_every_layer_and_a_surface_mount_pad_has_its_side() {
        let th = pad(PadKind::ThroughHole);
        assert_eq!(groups(&th, Side::Top, 4), vec![(layers(4), PadShape::Circle)]);
        let mut smd = pad(PadKind::Smd);
        assert_eq!(groups(&smd, Side::Top, 4), vec![(vec!["F.Cu".into()], PadShape::Circle)]);
        assert_eq!(groups(&smd, Side::Bottom, 4), vec![(vec!["B.Cu".into()], PadShape::Circle)]);
        smd.opposite_side = true;
        assert_eq!(groups(&smd, Side::Top, 4), vec![(vec!["B.Cu".into()], PadShape::Circle)]);
    }

    #[test]
    fn a_front_inner_back_stack_gives_the_front_the_inner_layers_and_the_back_their_own_shapes() {
        let mut p = pad(PadKind::ThroughHole);
        p.ext.stack = PadStack { mode: PadStackMode::FrontInnerBack, inner: Some(rect()), back: Some(oval()), ..Default::default() };
        assert_eq!(
            groups(&p, Side::Top, 4),
            vec![(vec!["F.Cu".to_string()], PadShape::Circle), (vec!["In1.Cu".to_string(), "In2.Cu".to_string()], PadShape::Rect), (vec!["B.Cu".to_string()], PadShape::Oval)]
        );
        // A two-layer board has no inner group.
        assert_eq!(groups(&p, Side::Top, 2), vec![(vec!["F.Cu".to_string()], PadShape::Circle), (vec!["B.Cu".to_string()], PadShape::Oval)]);
        // A footprint on the bottom has its front on B.Cu and its back on F.Cu: the board's layers read the stack the other way round.
        assert_eq!(
            groups(&p, Side::Bottom, 4),
            vec![(vec!["F.Cu".to_string()], PadShape::Oval), (vec!["In1.Cu".to_string(), "In2.Cu".to_string()], PadShape::Rect), (vec!["B.Cu".to_string()], PadShape::Circle)]
        );
        // A part of the stack the file left out has the pad's own shape.
        p.ext.stack.back = None;
        assert_eq!(groups(&p, Side::Top, 2), vec![(layers(2), PadShape::Circle)].into_iter().flat_map(|(l, s)| l.into_iter().map(move |l| (vec![l], s))).collect::<Vec<_>>());
    }

    #[test]
    fn a_custom_stack_has_a_group_for_each_layer_it_names_and_one_for_the_rest() {
        let mut p = pad(PadKind::ThroughHole);
        p.ext.stack = PadStack { mode: PadStackMode::Custom, layers: [("In2.Cu".to_string(), rect()), ("B.Cu".to_string(), oval())].into(), ..Default::default() };
        assert_eq!(
            groups(&p, Side::Top, 4),
            vec![(vec!["F.Cu".to_string(), "In1.Cu".to_string()], PadShape::Circle), (vec!["In2.Cu".to_string()], PadShape::Rect), (vec!["B.Cu".to_string()], PadShape::Oval)]
        );
        // On the bottom the names flip: the stack's In2 is the board's In1 on four layers, its B.Cu the board's F.Cu.
        assert_eq!(
            groups(&p, Side::Bottom, 4),
            vec![(vec!["F.Cu".to_string()], PadShape::Oval), (vec!["In1.Cu".to_string()], PadShape::Rect), (vec!["In2.Cu".to_string(), "B.Cu".to_string()], PadShape::Circle)]
        );
    }

    #[test]
    fn the_file_main_shape_is_the_front_on_top_and_the_back_on_the_bottom() {
        let mut p = pad(PadKind::ThroughHole);
        p.ext.stack = PadStack { mode: PadStackMode::FrontInnerBack, back: Some(oval()), ..Default::default() };
        assert_eq!(p.file_main_shape(Side::Top).shape, PadShape::Circle);
        assert_eq!(p.file_main_shape(Side::Bottom).shape, PadShape::Oval);
        // No stack, no swap.
        assert_eq!(pad(PadKind::ThroughHole).file_main_shape(Side::Bottom).shape, PadShape::Circle);
    }

    #[test]
    fn flip_copper_layer_mirrors_the_inner_layers_through_the_middle_of_the_stack() {
        assert_eq!(flip_copper_layer("F.Cu", 4), "B.Cu");
        assert_eq!(flip_copper_layer("In1.Cu", 4), "In2.Cu");
        assert_eq!(flip_copper_layer("In1.Cu", 6), "In4.Cu");
        assert_eq!(flip_copper_layer("In2.Cu", 6), "In3.Cu");
        assert_eq!(flip_copper_layer("In1.Cu", 2), "In1.Cu");
        assert_eq!(flip_copper_layer("F.Mask", 4), "F.Mask");
    }

    #[test]
    fn flashing_follows_the_unconnected_layer_mode_of_a_plated_through_hole_pad() {
        let mut p = pad(PadKind::ThroughHole);
        let f = |p: &Pad, l: &str| p.flash(l, "F.Cu", "B.Cu");
        assert_eq!((f(&p, "In1.Cu"), f(&p, "F.Cu")), (Flash::Always, Flash::Always));
        p.ext.stack.unconnected = UnconnectedLayers::RemoveAll;
        assert_eq!((f(&p, "In1.Cu"), f(&p, "F.Cu"), f(&p, "B.Cu")), (Flash::IfConnected, Flash::IfConnected, Flash::IfConnected));
        p.ext.stack.unconnected = UnconnectedLayers::RemoveExceptStartAndEnd;
        assert_eq!((f(&p, "In1.Cu"), f(&p, "F.Cu"), f(&p, "B.Cu")), (Flash::IfConnected, Flash::Always, Flash::Always));
        p.ext.stack.unconnected = UnconnectedLayers::StartEndOnly;
        assert_eq!((f(&p, "In1.Cu"), f(&p, "F.Cu")), (Flash::Never, Flash::Always));
        // A surface-mount pad and a non-plated hole are not what the mode is about.
        let mut smd = pad(PadKind::Smd);
        smd.ext.stack.unconnected = UnconnectedLayers::RemoveAll;
        assert_eq!(f(&smd, "F.Cu"), Flash::Always);
    }

    #[test]
    fn an_unconnected_mode_follows_the_file_tokens_in_the_order_they_come() {
        use UnconnectedLayers as U;
        assert_eq!(U::after_remove_unused_layers(true), U::RemoveAll);
        assert_eq!(U::after_remove_unused_layers(false), U::KeepAll);
        assert_eq!(U::after_keep_end_layers(true), U::RemoveExceptStartAndEnd);
        assert_eq!(U::after_keep_end_layers(false), U::RemoveAll);
        assert_eq!(U::remove(true), U::RemoveExceptStartAndEnd);
        assert_eq!(U::remove(false), U::RemoveAll);
        assert!(U::RemoveAll.removes() && !U::KeepAll.removes() && U::RemoveExceptStartAndEnd.keeps_outer() && !U::RemoveAll.keeps_outer());
    }

    #[test]
    fn a_layer_shape_lays_on_a_pad_and_normalizes_as_a_pad_edit_does() {
        let p = pad(PadKind::ThroughHole);
        let mut s = PadLayerShape::plain(PadShape::RoundRect, (2_000_000, 1_000_000));
        s.normalize();
        assert_eq!(s.roundrect_ratio, Some(0.25));
        let laid = s.lay_on(&p);
        assert_eq!((laid.shape, laid.size, laid.has_stack()), (PadShape::RoundRect, (2_000_000, 1_000_000), false));
        let mut c = PadLayerShape::plain(PadShape::Circle, (1_000_000, 600_000));
        c.normalize();
        assert_eq!(c.size, (1_000_000, 1_000_000), "a circle is square");
        let mut t = PadLayerShape::plain(PadShape::Rect, (1_000_000, 600_000));
        t.trapezoid_delta = Some((0, 100_000));
        t.chamfer_ratio = Some(0.3);
        t.normalize();
        assert_eq!((t.trapezoid_delta, t.chamfer_ratio), (None, None));
    }

    #[test]
    fn a_default_stack_is_default_and_is_not_serialized() {
        let p = pad(PadKind::ThroughHole);
        assert!(p.ext.stack.is_default() && p.ext.is_default());
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("stack"), "{json}");
        let mut q = p.clone();
        q.ext.stack = PadStack { mode: PadStackMode::FrontInnerBack, back: Some(oval()), unconnected: UnconnectedLayers::RemoveAll, ..Default::default() };
        let back: Pad = serde_json::from_str(&serde_json::to_string(&q).unwrap()).unwrap();
        assert_eq!(back, q);
    }
}
