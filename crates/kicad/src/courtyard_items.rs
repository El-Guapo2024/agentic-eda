//! A footprint's courtyard graphics as the board file wrote them (`FootprintExtra::courtyard_items`: `(fp_line ..)`, `(fp_rect ..)` .. on `F.CrtYd` / `B.CrtYd`,
//! written back verbatim): their layer read, and the two properties a global edit sets on a `PCB_SHAPE` -- `SetLayer` and the stroke's width (`STROKE_PARAMS::SetWidth`)
//! -- set in the text (`DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS::processItem`, `PCB_SHAPE::StyleFromSettings` at KiCad 8303b2ad; UI-LEFT-2 PP23).

use crate::sexpr::{self, Sexpr};
use eda_model::units::Nm;

/// The layer of a courtyard item (`(layer "F.CrtYd")`).
pub fn courtyard_item_layer(item: &str) -> Option<String> {
    let Ok(Sexpr::List(l)) = sexpr::parse(item) else { return None };
    sexpr::find(&l, "layer").and_then(|x| sexpr::txt(x, 1)).map(str::to_string)
}

/// The item with its layer and its line width set, each when given: `(layer ..)` replaced; the width in `(stroke (width ..))`, or in the older `(width ..)` of a file
/// before strokes (`PCB_IO_KICAD_SEXPR_PARSER::parsePCB_SHAPE` reads both), one added to the stroke when it had none. `None` when the text is not a list.
pub fn restyle_courtyard_item(item: &str, layer: Option<&str>, width: Option<Nm>) -> Option<String> {
    let Ok(Sexpr::List(mut l)) = sexpr::parse(item) else { return None };
    if let Some(layer) = layer {
        let node = Sexpr::List(vec![Sexpr::Atom("layer".into()), sexpr::quoted_atom(layer)]);
        match l.iter().position(|x| x.as_list().is_some_and(|x| sexpr::tag(x) == Some("layer"))) {
            Some(i) => l[i] = node,
            None => l.push(node),
        }
    }
    if let Some(w) = width {
        let width_node = Sexpr::List(vec![Sexpr::Atom("width".into()), Sexpr::Atom(crate::mm(w))]);
        if let Some(i) = l.iter().position(|x| x.as_list().is_some_and(|x| sexpr::tag(x) == Some("stroke"))) {
            if let Sexpr::List(stroke) = &mut l[i] {
                match stroke.iter().position(|x| x.as_list().is_some_and(|x| sexpr::tag(x) == Some("width"))) {
                    Some(j) => stroke[j] = width_node,
                    None => stroke.insert(1, width_node),
                }
            }
        } else if let Some(i) = l.iter().position(|x| x.as_list().is_some_and(|x| sexpr::tag(x) == Some("width"))) {
            l[i] = width_node;
        } else {
            l.push(Sexpr::List(vec![Sexpr::Atom("stroke".into()), width_node, Sexpr::List(vec![Sexpr::Atom("type".into()), Sexpr::Atom("solid".into())])]));
        }
    }
    Some(sexpr::to_text(&Sexpr::List(l)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_model::units::mm;

    #[test]
    fn the_layer_and_the_stroke_width_are_set_in_the_text() {
        let item = r#"(fp_rect (start -1 -1) (end 1 1) (stroke (width 0.05) (type solid)) (fill none) (layer "F.CrtYd") (uuid "a"))"#;
        assert_eq!(courtyard_item_layer(item).as_deref(), Some("F.CrtYd"));
        let out = restyle_courtyard_item(item, Some("B.CrtYd"), Some(mm(0.1))).unwrap();
        assert_eq!(out, r#"(fp_rect (start -1 -1) (end 1 1) (stroke (width 0.1) (type solid)) (fill none) (layer "B.CrtYd") (uuid "a"))"#);
        assert_eq!(restyle_courtyard_item(item, None, None).unwrap(), item, "nothing asked, the text as it was");
    }

    #[test]
    fn an_older_width_is_set_where_it_is() {
        let item = r#"(fp_line (start 0 0) (end 1 0) (layer "F.CrtYd") (width 0.05))"#;
        assert_eq!(restyle_courtyard_item(item, None, Some(mm(0.12))).unwrap(), r#"(fp_line (start 0 0) (end 1 0) (layer "F.CrtYd") (width 0.12))"#);
    }
}
