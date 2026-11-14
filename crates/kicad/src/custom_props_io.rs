//! The board items' custom properties in the board file (KiCad 25ec2335 `KICAD_FORMAT::FormatCustomProperties`, `common/io/kicad/kicad_io_utils.cpp`:
//! `(custom_property "name" "text")` for each, in name order, the last thing in a footprint's or a pad's form and after the `(uuid ..)` of the others; and the
//! parser's `T_custom_property` case of each item, `PCB_IO_KICAD_SEXPR_PARSER`). The design keeps them by item id (`DrawingsSection::custom_properties`).

use std::collections::{BTreeMap, HashMap};

use crate::sexpr::{self, Sexpr};

/// Each item's custom properties put into the board text `out`: in the form whose `(uuid "U")` is the item's (`uuid_of`: our id -> the uuid it was written
/// as), before that form's closing parenthesis.
pub(crate) fn inject(out: &mut String, props: &BTreeMap<String, BTreeMap<String, String>>, uuid_of: &HashMap<String, String>) {
    // the places, found first and filled from the last, so an insertion does not move the places before it
    let mut at: Vec<(usize, String)> = Vec::new();
    for (id, named) in props {
        let Some(uuid) = uuid_of.get(id) else { continue };
        let Some(start) = out.find(&format!("(uuid {})", crate::sexpr_str(uuid))) else { continue };
        let Some(close) = enclosing_close(out, start) else { continue };
        let text: String = named.iter().map(|(k, v)| format!(" (custom_property {} {})", crate::sexpr_str(k), crate::sexpr_str(v))).collect();
        at.push((close, text));
    }
    at.sort_by(|a, b| b.0.cmp(&a.0));
    for (close, text) in at {
        out.insert_str(close, &text);
    }
}

/// The byte offset of the `)` that closes the form holding the text at `from` (strings and their escapes skipped).
fn enclosing_close(text: &str, from: usize) -> Option<usize> {
    let mut depth = 0i64;
    let mut in_string = false;
    let mut escaped = false;
    for (i, c) in text[from..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return Some(from + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The custom properties the board `root` gives its items -- the board's own forms and the pads of its footprints -- by the id each item has in the design
/// (`ids_by_uuid`: the file's uuid -> our id). An item no id is known for keeps none.
pub(crate) fn read(root: &[Sexpr], ids_by_uuid: &BTreeMap<String, String>) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut take = |item: &[Sexpr]| {
        let named: BTreeMap<String, String> = sexpr::find_all(item, "custom_property").filter_map(|p| Some((sexpr::txt(p, 1)?.to_string(), sexpr::txt(p, 2).unwrap_or("").to_string()))).collect();
        if named.is_empty() {
            return;
        }
        if let Some(id) = crate::import_items::item_uuid(item).and_then(|u| ids_by_uuid.get(&u).cloned()) {
            out.entry(id).or_default().extend(named);
        }
    };
    for item in root.iter().skip(1).filter_map(Sexpr::as_list) {
        take(item);
        if matches!(sexpr::tag(item), Some("footprint") | Some("module")) {
            for pad in sexpr::find_all(item, "pad") {
                take(pad);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A footprint's properties go last in its form, a pad's in the pad's, a graphic's after its uuid; a name or a text with a quote is quoted as KiCad quotes it.
    #[test]
    fn the_properties_go_into_the_form_of_the_items_uuid() {
        let mut out = String::from("(kicad_pcb\n\t(footprint \"L:R\"\n\t\t(uuid \"f1\")\n\t\t(pad \"1\" smd rect (at 0 0) (size 1 1) (layers \"F.Cu\") (uuid \"p1\"))\n\t)\n\t(gr_line (start 0 0) (end 1 0) (layer \"F.SilkS\") (uuid \"s1\"))\n)\n");
        let props: BTreeMap<String, BTreeMap<String, String>> = [
            ("R1".to_string(), [("Supplier".to_string(), "ACME (\"best\")".to_string())].into_iter().collect()),
            ("R1.1".to_string(), [("Note".to_string(), "x".to_string())].into_iter().collect()),
            ("s1".to_string(), [("A".to_string(), "1".to_string()), ("B".to_string(), "".to_string())].into_iter().collect()),
        ]
        .into_iter()
        .collect();
        let uuid_of: HashMap<String, String> = [("R1", "f1"), ("R1.1", "p1"), ("s1", "s1")].into_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        inject(&mut out, &props, &uuid_of);
        assert!(out.contains("(uuid \"p1\") (custom_property \"Note\" \"x\"))"), "{out}");
        assert!(out.contains("(custom_property \"Supplier\" \"ACME (\\\"best\\\")\")"), "quoted as KiCad quotes: {out}");
        let fp_end = out.find("\t\t(pad").unwrap();
        assert!(out[fp_end..].find("(custom_property \"Supplier\"").unwrap() > out[fp_end..].find("(uuid \"p1\")").unwrap(), "the footprint's go last in its form: {out}");
        assert!(out.contains("(uuid \"s1\") (custom_property \"A\" \"1\") (custom_property \"B\" \"\"))"), "{out}");
        // and they read back by the ids of the uuids
        let tree = sexpr::parse(&out).unwrap();
        let ids: BTreeMap<String, String> = uuid_of.iter().map(|(a, b)| (b.clone(), a.clone())).collect();
        let back = read(tree.as_list().unwrap(), &ids);
        assert_eq!(back, props);
    }
}
