//! A pad's padstack in the files: `(remove_unused_layers ..)`, `(keep_end_layers ..)` and `(padstack (mode ..) (layer ..) ..)`, read and written as
//! `PCB_IO_KICAD_SEXPR` does (`parsePAD`, `parsePadstack`, `format( const PAD* )`, `pcbnew/pcb_io/kicad_sexpr`). The model is `eda_model::padstack`.
//!
//! The file's frame is the board's: the pad's main fields hold the shape on `F.Cu`. The model's is the footprint's as seen from the top: its own shape is the
//! footprint's side. For a footprint on the top the two are the same; on the bottom `PADSTACK::FlipLayers` has put the footprint's front shape on `B.Cu`, so
//! reading takes the main fields to the model's *back* and the file's `B.Cu` layer to its own, and writing does the reverse (`Pad::file_main_shape`). The inner
//! layers of a custom stack flip with the board's number of copper layers (`::FlipLayer`). The x of every position in a layer is mirrored on the bottom, as
//! the main fields' are.

use eda_model::units::Nm;
use crate::pad_shape::{read_shape_node, write_custom_text, write_pad_shape};
use crate::sexpr::{self, Sexpr};
use crate::{mm, sexpr_str};
use eda_model::footprint::{Pad, PadShape};
use eda_model::ir::Side;
use eda_model::padstack::{role_of_layer, PadLayerShape, PadStack, PadStackMode, UnconnectedLayers};
use std::fmt::Write as _;

/// `parseMaybeAbsentBool( true )`: `(token yes)`, `(token no)` and a bare `(token)`, which is true.
fn maybe_absent_bool(item: &[Sexpr]) -> bool {
    !matches!(sexpr::txt(item, 1), Some("no") | Some("false"))
}

/// What `(remove_unused_layers ..)` and `(keep_end_layers ..)` of the pad `pad` say, read in the order they come (`parsePAD`).
pub(crate) fn read_unconnected(pad: &[Sexpr]) -> UnconnectedLayers {
    let mut mode = UnconnectedLayers::KeepAll;
    for item in pad.iter().skip(1).filter_map(Sexpr::as_list) {
        match sexpr::tag(item) {
            Some("remove_unused_layers") => mode = UnconnectedLayers::after_remove_unused_layers(maybe_absent_bool(item)),
            Some("keep_end_layers") => mode = UnconnectedLayers::after_keep_end_layers(maybe_absent_bool(item)),
            _ => {}
        }
    }
    mode
}

/// The tokens of a padstack layer's node that are its shape (`parsePadstack`): the others are kept as the file had them.
const LAYER_SHAPE_TOKENS: &[&str] = &["shape", "size", "offset", "rect_delta", "roundrect_rratio", "chamfer_ratio", "chamfer", "options", "primitives"];

/// One `(layer "X" (shape ..) (size ..) ..)` node: the shape of that layer. Negative sizes are zero (`PADSTACK::SetSize`), a missing shape a circle and a
/// missing size nothing (`SHAPE_PROPS()`).
fn read_layer_shape(layer: &[Sexpr], side: Side) -> PadLayerShape {
    let token = sexpr::find(layer, "shape").and_then(|s| sexpr::txt(s, 1)).unwrap_or("circle");
    let offset = sexpr::find(layer, "offset").and_then(|o| Some((crate::import::mm_to_nm(sexpr::num(o, 1)?), crate::import::mm_to_nm(sexpr::num(o, 2)?)))).map(|(x, y)| if side == Side::Bottom { (-x, y) } else { (x, y) });
    let read = read_shape_node(layer, token, offset.filter(|o| *o != (Nm::ZERO, Nm::ZERO)), side);
    let size = sexpr::find(layer, "size").and_then(|s| Some((crate::import::mm_to_nm(sexpr::num(s, 1)?).max(Nm::ZERO), crate::import::mm_to_nm(sexpr::num(s, 2)?).max(Nm::ZERO)))).unwrap_or((Nm::ZERO, Nm::ZERO));
    let tokens = layer.iter().skip(1).filter_map(Sexpr::as_list).filter(|l| !sexpr::tag(l).is_some_and(|t| LAYER_SHAPE_TOKENS.contains(&t))).map(|l| sexpr::to_text(&Sexpr::List(l.to_vec()))).collect();
    PadLayerShape {
        shape: read.shape,
        size,
        roundrect_ratio: read.roundrect_ratio,
        offset: read.ext.offset,
        trapezoid_delta: read.ext.trapezoid_delta,
        chamfer_ratio: read.ext.chamfer_ratio,
        chamfer_corners: read.ext.chamfer_corners,
        custom: read.ext.custom,
        tokens,
    }
}

/// What reading a pad's padstack gives.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PadStackRead {
    pub stack: PadStack,
    /// The pad's own shape, when the stack says it was not the one the main fields hold: the footprint's side is the file's `B.Cu` on the bottom.
    pub own: Option<PadLayerShape>,
}

/// The padstack of the pad node `pad`: `main` is the shape its main fields hold (what `read_pad_shape` read, as a layer's), `side` the pad's footprint's side
/// and `copper_layers` the board's copper layers (a bottom-side custom stack has to know which layer each of them has become).
pub(crate) fn read_padstack(pad: &[Sexpr], main: &PadLayerShape, side: Side, copper_layers: &[String]) -> PadStackRead {
    let unconnected = read_unconnected(pad);
    let Some(node) = sexpr::find(pad, "padstack") else {
        return PadStackRead { stack: PadStack { unconnected, ..Default::default() }, own: None };
    };
    let mode = match sexpr::find(node, "mode").and_then(|m| sexpr::txt(m, 1)) {
        Some("front_inner_back") => PadStackMode::FrontInnerBack,
        Some("custom") => PadStackMode::Custom,
        _ => return PadStackRead { stack: PadStack { unconnected, ..Default::default() }, own: None },
    };
    // The layers the file names, in file order: a layer node of the file's own frame.
    let file_layers: Vec<(String, PadLayerShape)> = sexpr::find_all(node, "layer").filter_map(|l| Some((sexpr::txt(l, 1)?.to_string(), read_layer_shape(l, side)))).collect();
    let named = |name: &str| file_layers.iter().find(|(n, _)| n == name).map(|(_, s)| s.clone());
    // The file's front: the main fields, unless a `(layer "F.Cu" ..)` node says it again.
    let file_front = named("F.Cu").unwrap_or_else(|| main.clone());
    let n = copper_layers.len().max(2);
    let mut stack = PadStack { mode, unconnected, ..Default::default() };
    let mut own: Option<PadLayerShape> = None;
    match mode {
        PadStackMode::FrontInnerBack => {
            let inner = named("Inner");
            let back = named("B.Cu");
            if side == Side::Bottom {
                own = Some(back.clone().unwrap_or_else(|| file_front.clone()));
                stack.back = Some(file_front);
            } else {
                if file_front != *main {
                    own = Some(file_front);
                }
                stack.back = back;
            }
            stack.inner = inner;
        }
        PadStackMode::Custom => {
            // Every layer the board has (and every one the file names), in the model's names: the footprint's frame.
            let mut names: Vec<String> = copper_layers.to_vec();
            for (l, _) in &file_layers {
                if l != "F.Cu" && !names.contains(l) {
                    names.push(l.clone());
                }
            }
            for file_layer in names {
                if file_layer == "F.Cu" {
                    continue;
                }
                // A layer the file does not name has the front's shape (`PADSTACK::CopperLayer`'s `ALL_LAYERS` fallback).
                let shape = named(&file_layer).unwrap_or_else(|| file_front.clone());
                let role = role_of_layer(&file_layer, side, n);
                if role == "F.Cu" {
                    own = Some(shape);
                } else {
                    stack.layers.insert(role, shape);
                }
            }
            if side == Side::Bottom {
                // The file's F.Cu is the footprint's back.
                stack.layers.insert(role_of_layer("F.Cu", side, n), file_front);
            } else if file_front != *main {
                own = Some(file_front);
            }
        }
        PadStackMode::Normal => {}
    }
    PadStackRead { stack, own }
}

/// ` (remove_unused_layers yes) (keep_end_layers no)`: a plated through-hole pad's mode as `format( const PAD* )` writes it, nothing for the other kinds of pad
/// and, with every layer kept, nothing at all (the token is `no` for KiCad and absent for the file of a pad that never had it: the same to KiCad).
pub(crate) fn unconnected_text(mode: UnconnectedLayers) -> String {
    match mode {
        UnconnectedLayers::KeepAll => String::new(),
        UnconnectedLayers::RemoveAll => " (remove_unused_layers yes) (keep_end_layers no)".to_string(),
        UnconnectedLayers::RemoveExceptStartAndEnd => " (remove_unused_layers yes) (keep_end_layers yes)".to_string(),
        // A pad has no `start_end_only`; it is a via's.
        UnconnectedLayers::StartEndOnly => " (remove_unused_layers yes) (keep_end_layers yes)".to_string(),
    }
}

/// The inner copper layers of the library's board-less file (`MAX_CU_LAYERS`): `F.Cu`, `In1.Cu` .. `In30.Cu`, `B.Cu`.
pub(crate) fn library_copper_layers() -> Vec<String> {
    let mut v = vec!["F.Cu".to_string()];
    v.extend((1..=30).map(|i| format!("In{i}.Cu")));
    v.push("B.Cu".to_string());
    v
}

/// One layer node's content, as `formatPadLayer` writes it.
fn layer_text(shape: &PadLayerShape, side: Side) -> String {
    let mut out = String::new();
    write!(out, "(shape {}) (size {} {})", crate::pad_shape::shape_token(shape.shape), mm(shape.size.0), mm(shape.size.1)).unwrap();
    let ext = eda_model::footprint::PadShapeExt {
        offset: shape.offset,
        trapezoid_delta: shape.trapezoid_delta,
        chamfer_ratio: shape.chamfer_ratio,
        chamfer_corners: shape.chamfer_corners,
        custom: shape.custom.clone(),
        stack: Default::default(),
    };
    let t = write_pad_shape(shape.shape, shape.roundrect_ratio, &ext, side);
    out.push_str(&t.delta);
    if let Some((ox, oy)) = t.offset {
        write!(out, " (offset {} {})", mm(ox), mm(oy)).unwrap();
    }
    out.push_str(&t.corners);
    if shape.shape == PadShape::Custom {
        // `(options (anchor ..))` and the primitives: the zone option is the pad's, not a layer's.
        out.push_str(&write_custom_text(&ext, side, false));
    }
    for token in &shape.tokens {
        write!(out, " {token}").unwrap();
    }
    out
}

/// The `(padstack ..)` form of `pad` on a footprint on `side`, with its leading space; nothing for a normal stack. `copper_layers` are the board's copper layers,
/// front to back, in the board's names (the library's file has all of them: [`library_copper_layers`]).
pub(crate) fn write_padstack(pad: &Pad, side: Side, copper_layers: &[String]) -> String {
    let stack = &pad.ext.stack;
    if !stack.is_layered() {
        return String::new();
    }
    let n = copper_layers.len().max(2);
    let mut out = String::new();
    match stack.mode {
        PadStackMode::FrontInnerBack => {
            out.push_str(" (padstack (mode front_inner_back)");
            // The inner layers, then the file's `B.Cu`, which is the footprint's own side on the bottom.
            let inner = pad.shape_for_role("In1.Cu");
            write!(out, " (layer \"Inner\" {})", layer_text(&inner, side)).unwrap();
            let back = if side == Side::Bottom { PadLayerShape::of(pad) } else { pad.shape_for_role("B.Cu") };
            write!(out, " (layer \"B.Cu\" {})", layer_text(&back, side)).unwrap();
        }
        PadStackMode::Custom => {
            out.push_str(" (padstack (mode custom)");
            for layer in copper_layers.iter().filter(|l| *l != "F.Cu") {
                let role = role_of_layer(layer, side, n);
                write!(out, " (layer {} {})", sexpr_str(layer), layer_text(&pad.shape_for_role(&role), side)).unwrap();
            }
        }
        PadStackMode::Normal => {}
    }
    out.push(')');
    out
}

#[cfg(test)]
mod tests {
    use eda_model::units::Nm;
    use super::*;
    use eda_model::footprint::{PadAnchor, PadCustom, PadKind, PadPrimitive, PadShapeExt};
    use eda_model::ir::{Point, Shape};

    fn node(text: &str) -> Vec<Sexpr> {
        sexpr::parse(text).unwrap().as_list().unwrap().to_vec()
    }

    fn layers(n: usize) -> Vec<String> {
        let mut v = vec!["F.Cu".to_string()];
        v.extend((1..=n.saturating_sub(2)).map(|i| format!("In{i}.Cu")));
        v.push("B.Cu".to_string());
        v
    }

    /// The pad as `parse_pad_geometry_in` makes it, on a footprint at `side` of a board with `n` copper layers.
    fn pad_of(text: &str, side: Side, n: usize) -> Pad {
        let mut notes = crate::import::ImportNotes::default();
        crate::import::parse_pad_geometry_in(&node(text), side, 0, &mut notes, false, &layers(n)).expect("a pad")
    }

    const FRONT_INNER_BACK: &str = "(pad \"1\" thru_hole oval (at 0 0) (size 3.048 1.524) (drill 0.762) (layers \"*.Cu\" \"*.Mask\") (remove_unused_layers no) \
        (padstack (mode front_inner_back) (layer \"Inner\" (shape oval) (size 1.27 1.27)) (layer \"B.Cu\" (shape oval) (size 2.54 1.016))) (uuid \"43d32944-eb49-4135-9a55-229ae4e9215e\"))";

    #[test]
    fn the_unconnected_layer_tokens_are_read_in_file_order_as_parse_pad_does() {
        let mode = |t: &str| read_unconnected(&node(&format!("(pad \"1\" thru_hole circle (at 0 0) (size 1 1) {t})")));
        assert_eq!(mode(""), UnconnectedLayers::KeepAll);
        assert_eq!(mode("(remove_unused_layers no)"), UnconnectedLayers::KeepAll);
        assert_eq!(mode("(remove_unused_layers yes)"), UnconnectedLayers::RemoveAll);
        assert_eq!(mode("(remove_unused_layers)"), UnconnectedLayers::RemoveAll, "an absent value is yes");
        assert_eq!(mode("(remove_unused_layers yes) (keep_end_layers no)"), UnconnectedLayers::RemoveAll);
        assert_eq!(mode("(remove_unused_layers yes) (keep_end_layers yes)"), UnconnectedLayers::RemoveExceptStartAndEnd);
        // `SetKeepTopBottom( true )` is a mode of its own, whatever came before: a `keep_end_layers` that follows a `no` still removes.
        assert_eq!(mode("(remove_unused_layers no) (keep_end_layers yes)"), UnconnectedLayers::RemoveExceptStartAndEnd);
        assert_eq!(mode("(keep_end_layers yes) (remove_unused_layers no)"), UnconnectedLayers::KeepAll);
        assert_eq!(unconnected_text(UnconnectedLayers::KeepAll), "");
        assert_eq!(unconnected_text(UnconnectedLayers::RemoveAll), " (remove_unused_layers yes) (keep_end_layers no)");
        assert_eq!(unconnected_text(UnconnectedLayers::RemoveExceptStartAndEnd), " (remove_unused_layers yes) (keep_end_layers yes)");
    }

    #[test]
    fn a_front_inner_back_padstack_is_read_as_its_three_shapes() {
        let pad = pad_of(FRONT_INNER_BACK, Side::Top, 4);
        assert_eq!((pad.shape, pad.size), (PadShape::Oval, (Nm::from_mm(3.048), Nm::from_mm(1.524))));
        let stack = &pad.ext.stack;
        assert_eq!(stack.mode, PadStackMode::FrontInnerBack);
        assert_eq!(stack.inner, Some(PadLayerShape::plain(PadShape::Oval, (Nm::from_mm(1.27), Nm::from_mm(1.27)))));
        assert_eq!(stack.back, Some(PadLayerShape::plain(PadShape::Oval, (Nm::from_mm(2.54), Nm::from_mm(1.016)))));
        assert_eq!(stack.unconnected, UnconnectedLayers::KeepAll);
        // What the board's layers have: the front, the two inner layers together, the back.
        let groups = pad.shape_groups(Side::Top, &layers(4));
        assert_eq!(groups.iter().map(|g| (g.layers.clone(), g.shape.size)).collect::<Vec<_>>(), vec![(vec!["F.Cu".to_string()], (Nm::from_mm(3.048), Nm::from_mm(1.524))), (vec!["In1.Cu".to_string(), "In2.Cu".to_string()], (Nm::from_mm(1.27), Nm::from_mm(1.27))), (vec!["B.Cu".to_string()], (Nm::from_mm(2.54), Nm::from_mm(1.016)))]);
    }

    #[test]
    fn a_padstack_is_written_as_it_was_read_on_either_side_of_the_board() {
        for side in [Side::Top, Side::Bottom] {
            let pad = pad_of(FRONT_INNER_BACK, side, 4);
            // Back to a pad form: the main fields (the file's F.Cu), the unconnected tokens, the padstack.
            let main = pad.file_main_shape(side);
            let ext = PadShapeExt { offset: main.offset, trapezoid_delta: main.trapezoid_delta, chamfer_ratio: main.chamfer_ratio, chamfer_corners: main.chamfer_corners, custom: main.custom.clone(), stack: Default::default() };
            let t = write_pad_shape(main.shape, main.roundrect_ratio, &ext, side);
            let text = format!(
                "(pad \"1\" thru_hole {} (at 0 0) (size {} {}){} (drill 0.762) (layers \"*.Cu\" \"*.Mask\"){}{})",
                crate::pad_shape::shape_token(main.shape),
                mm(main.size.0),
                mm(main.size.1),
                t.corners,
                unconnected_text(pad.ext.stack.unconnected),
                write_padstack(&pad, side, &layers(4))
            );
            let again = pad_of(&text, side, 4);
            assert_eq!(again, pad, "{side:?}: {text}");
        }
        // On the bottom the file's `F.Cu` is the footprint's back: the main fields of the file hold what the model calls its back.
        let bottom = pad_of(FRONT_INNER_BACK, Side::Bottom, 4);
        assert_eq!((bottom.shape, bottom.size), (PadShape::Oval, (Nm::from_mm(2.54), Nm::from_mm(1.016))), "the file's B.Cu is the footprint's own side");
        assert_eq!(bottom.ext.stack.back.as_ref().map(|s| s.size), Some((Nm::from_mm(3.048), Nm::from_mm(1.524))), "the file's main fields are its back");
        assert_eq!(bottom.ext.stack.inner.as_ref().map(|s| s.size), Some((Nm::from_mm(1.27), Nm::from_mm(1.27))));
    }

    #[test]
    fn a_custom_padstack_reads_every_layer_the_file_names_and_clamps_a_negative_size() {
        // `padstacks_complex.kicad_pcb`'s first pad: the front is listed in the stack too (older files), In2.Cu has a negative height.
        let text = "(pad \"3\" thru_hole rect (at 0 0) (size 0.762 0.762) (drill 0.762) (layers \"*.Cu\" \"*.Mask\") (remove_unused_layers no) \
            (padstack (mode custom) (layer \"F.Cu\" (shape rect) (size 0.762 0.762)) (layer \"In1.Cu\" (shape rect) (size 1.016 1.016)) (layer \"In2.Cu\" (shape rect) (size 1.27 -1.27)) (layer \"B.Cu\" (shape rect) (size 1.524 1.524))))";
        let pad = pad_of(text, Side::Top, 4);
        let stack = &pad.ext.stack;
        assert_eq!(stack.mode, PadStackMode::Custom);
        assert_eq!(stack.layers.keys().map(String::as_str).collect::<Vec<_>>(), vec!["B.Cu", "In1.Cu", "In2.Cu"], "the front is the pad's own");
        assert_eq!(stack.layers["In2.Cu"].size, (Nm::from_mm(1.27), Nm::ZERO), "`PADSTACK::SetSize` takes a negative size to zero");
        assert_eq!(stack.layers["B.Cu"].size, (Nm::from_mm(1.524), Nm::from_mm(1.524)));
        let groups = pad.shape_groups(Side::Top, &layers(4));
        assert_eq!(groups.len(), 4, "four layers, four shapes");
        // Written back, the layers in the board's order and without the front, as `format( const PAD* )` does.
        let written = write_padstack(&pad, Side::Top, &layers(4));
        assert!(written.starts_with(" (padstack (mode custom) (layer \"In1.Cu\" (shape rect) (size 1.016 1.016)) (layer \"In2.Cu\" (shape rect) (size 1.27 0)) (layer \"B.Cu\" (shape rect) (size 1.524 1.524)))"), "{written}");
    }

    #[test]
    fn a_layers_other_tokens_and_custom_shapes_are_kept_and_a_custom_layer_has_no_zone_option() {
        let text = "(pad \"1\" thru_hole circle (at 0 0) (size 2 2) (drill 1) (layers \"*.Cu\") (remove_unused_layers yes) (keep_end_layers yes) \
            (padstack (mode front_inner_back) (layer \"Inner\" (shape circle) (size 1.5 1.5) (thermal_gap 0.3) (clearance 0.2) (zone_connect 2)) \
            (layer \"B.Cu\" (shape custom) (size 1 1) (options (anchor rect)) (primitives (gr_line (start 0 0) (end 1 0) (width 0.2)) (gr_poly (pts (xy 0 0) (xy 1 0) (xy 1 1)) (width 0) (fill yes))))))";
        let pad = pad_of(text, Side::Top, 2);
        assert_eq!(pad.ext.stack.unconnected, UnconnectedLayers::RemoveExceptStartAndEnd);
        let inner = pad.ext.stack.inner.as_ref().unwrap();
        assert_eq!(inner.tokens, vec!["(thermal_gap 0.3)".to_string(), "(clearance 0.2)".to_string(), "(zone_connect 2)".to_string()]);
        let back = pad.ext.stack.back.as_ref().unwrap();
        let custom = back.custom.as_ref().unwrap();
        assert_eq!((custom.anchor, custom.primitives.len()), (PadAnchor::Rect, 2));
        let written = write_padstack(&pad, Side::Top, &layers(2));
        assert!(written.contains("(layer \"Inner\" (shape circle) (size 1.5 1.5) (thermal_gap 0.3) (clearance 0.2) (zone_connect 2))"), "{written}");
        assert!(written.contains("(layer \"B.Cu\" (shape custom) (size 1 1) (options (anchor rect)) (primitives (gr_line (start 0 0) (end 1 0) (width 0.2)) (gr_poly"), "{written}");
        assert!(!written.contains("(clearance outline)") && !written.contains("convexhull"), "a layer has no zone option: {written}");
        let again = {
            let mut notes = crate::import::ImportNotes::default();
            let t = format!("(pad \"1\" thru_hole circle (at 0 0) (size 2 2) (drill 1) (layers \"*.Cu\"){}{})", unconnected_text(pad.ext.stack.unconnected), written);
            crate::import::parse_pad_geometry_in(&node(&t), Side::Top, 0, &mut notes, false, &layers(2)).unwrap()
        };
        assert_eq!(again.ext.stack, pad.ext.stack);
        let _ = (PadKind::Smd, PadPrimitive { shape: Shape::Segment { id: String::new(), layer: String::new(), stroke_width: Nm::ZERO, filled: false, start: Point { x: Nm::ZERO, y: Nm::ZERO }, end: Point { x: Nm::ZERO, y: Nm::ZERO } }, proxy: false }, PadCustom::default());
    }

    #[test]
    fn a_normal_pad_has_no_padstack_and_the_library_has_every_layer_a_file_can_name() {
        let pad = pad_of("(pad \"1\" thru_hole circle (at 0 0) (size 1 1) (drill 0.5) (layers \"*.Cu\"))", Side::Top, 2);
        assert!(pad.ext.stack.is_default());
        assert_eq!(write_padstack(&pad, Side::Top, &layers(2)), "");
        let all = library_copper_layers();
        assert_eq!((all.len(), all[1].as_str(), all[30].as_str(), all[31].as_str()), (32, "In1.Cu", "In30.Cu", "B.Cu"));
    }
}
