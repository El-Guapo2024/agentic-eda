//! A via's tokens beyond `Via`'s own fields, read and written: [`eda_model::via_extra::ViaExtra`].
//!
//! `PCB_IO_KICAD_SEXPR::format( const PCB_TRACK* )` writes a via as `(via [blind|buried|micro] (at ..) (size ..) (drill ..) [backdrill ..] (layers ..) [remove_unused_layers ..]
//! [keep_end_layers ..] [start_end_only ..] [locked] [free] [zone_layer_connections ..] [tenting ..] [capping ..] [covering ..] [plugging ..] [filling ..] [padstack ..]
//! [teardrops ..] (net ..) (uuid ..))`. The model holds the position, the size, the drill, the layers and the net; the type and everything else are kept as the file had them.

use eda_model::via_extra::{ViaExtra, ViaKind};

use crate::sexpr::{self, Sexpr};

/// The tokens of a via that the via itself holds or the writer makes.
const OWN: &[&str] = &["at", "size", "drill", "layers", "net", "uuid", "tstamp", "locked"];

/// What one `(via ..)` says beyond the position, size, drill, layers, net and lock; `id` is empty (the caller names the via once it has its id).
pub(crate) fn read(via: &[Sexpr]) -> ViaExtra {
    let mut extra = ViaExtra::default();
    for item in via.iter().skip(1) {
        match item {
            Sexpr::Atom(a) => {
                if let Some(kind) = ViaKind::from_file_word(a) {
                    extra.kind = kind;
                } else if a != "locked" {
                    extra.tokens.push(sexpr::atom_text(a));
                }
            }
            Sexpr::List(l) => {
                if !sexpr::tag(l).is_some_and(|t| OWN.contains(&t)) {
                    extra.tokens.push(sexpr::to_text(item));
                }
            }
        }
    }
    extra
}

/// The word before `(at ..)` and the tokens after `(layers ..)`: ` blind` and ` (remove_unused_layers yes) (tenting (front no) (back no))`.
pub(crate) fn words(extra: Option<&ViaExtra>) -> (String, String) {
    let Some(e) = extra else { return (String::new(), String::new()) };
    (e.kind.file_word().map(|w| format!(" {w}")).unwrap_or_default(), e.tokens.iter().map(|t| format!(" {t}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn via(text: &str) -> Vec<Sexpr> {
        sexpr::parse(text).unwrap().as_list().unwrap().to_vec()
    }

    #[test]
    fn a_vias_other_tokens_are_kept_as_the_file_had_them() {
        let e = read(&via(
            r#"(via blind (at 1 2) (size 0.6) (drill 0.3) (layers "F.Cu" "In2.Cu") (remove_unused_layers yes) (keep_end_layers no) (locked yes) free (zone_layer_connections "F.Cu" "In1.Cu")
               (tenting (front yes) (back no)) (capping no) (padstack (mode front_inner_back) (layer "Inner" (size 0.8))) (teardrops (best_length_ratio 0.5) (curved_edges no)) (net "GND") (uuid "x"))"#,
        ));
        assert_eq!(e.kind, ViaKind::Blind);
        assert_eq!(
            e.tokens,
            [
                "(remove_unused_layers yes)",
                "(keep_end_layers no)",
                "free",
                "(zone_layer_connections \"F.Cu\" \"In1.Cu\")",
                "(tenting (front yes) (back no))",
                "(capping no)",
                "(padstack (mode front_inner_back) (layer \"Inner\" (size 0.8)))",
                "(teardrops (best_length_ratio 0.5) (curved_edges no))",
            ]
        );
        let (before, after) = words(Some(&e));
        assert_eq!(before, " blind");
        assert!(after.starts_with(" (remove_unused_layers yes) (keep_end_layers no) free (zone_layer_connections") && after.ends_with("(curved_edges no))"), "{after}");
        // A plain via has nothing, a micro via is a word.
        assert!(read(&via(r#"(via (at 1 2) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1) (uuid "x"))"#)).is_default());
        assert_eq!(read(&via("(via micro (at 1 2))")).kind, ViaKind::Micro);
        assert_eq!(words(None), (String::new(), String::new()));
    }
}
