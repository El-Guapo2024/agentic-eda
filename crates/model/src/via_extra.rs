//! What a via says beyond [`crate::ir::Via`]'s own fields.
//!
//! `Via` is a position, a drill, a diameter, two layers and a net. A `(via ..)` of a KiCad board says more, and kicad-cli judges all of it: the via's type (`blind`, `buried`,
//! `micro`: the microvia checks, the layers it spans), what becomes of the copper on the layers it does not connect (`remove_unused_layers`, `keep_end_layers`,
//! `start_end_only`: annular rings on the inner layers), `free`, the layers a zone flashes it on (`zone_layer_connections`), its tenting, capping, covering, plugging and
//! filling, its own padstack and its teardrop settings, and in the newest format a backdrill and post-machining. A board that came from a KiCad file keeps them here, one
//! [`ViaExtra`] per via that has any, in `DrawingsSection::via_extras` and keyed by the via's id, as [`crate::board_text::TextExtra`] is keyed by a text's: the type as a
//! word, the rest as the tokens the file had, verbatim and in order, so that a token a newer KiCad adds is carried along without anyone having read it. The importer reads them,
//! the writer writes them back, and a via nothing was read for is written as it always was. The overlay is additive: a design without it reads as "every via is a through via
//! with KiCad's defaults".

use crate::padstack::UnconnectedLayers;
use serde::{Deserialize, Serialize};

/// `VIATYPE`: the bare word after `(via` (a through via has none).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViaKind {
    #[default]
    Through,
    Blind,
    Buried,
    Micro,
}

impl ViaKind {
    /// The word the file writes before `(at ..)`, if any.
    pub fn file_word(self) -> Option<&'static str> {
        match self {
            ViaKind::Through => None,
            ViaKind::Blind => Some("blind"),
            ViaKind::Buried => Some("buried"),
            ViaKind::Micro => Some("micro"),
        }
    }

    pub fn from_file_word(word: &str) -> Option<ViaKind> {
        match word {
            "blind" => Some(ViaKind::Blind),
            "buried" => Some(ViaKind::Buried),
            "micro" => Some(ViaKind::Micro),
            _ => None,
        }
    }
}

fn is_through(k: &ViaKind) -> bool {
    *k == ViaKind::Through
}

/// The parts of one via's tokens that [`crate::ir::Via`] has no field for. See the module doc.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViaExtra {
    /// The id of the via it belongs to (`Via::id`).
    pub id: String,
    #[serde(default, skip_serializing_if = "is_through")]
    pub kind: ViaKind,
    /// The via's other tokens as the file wrote them (`(remove_unused_layers yes)`, `(tenting (front no) (back no))`, `(padstack ..)`, `free`, ..), in file order, without the
    /// position, size, drill, layers, net, uuid and lock, which the via itself holds or the writer makes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tokens: Vec<String>,
}

impl ViaExtra {
    /// Whether this says nothing a via without one would not (the importer keeps no entry for such a via).
    pub fn is_default(&self) -> bool {
        self.kind == ViaKind::Through && self.tokens.is_empty()
    }

    /// `PADSTACK::UnconnectedLayerMode` of the via, from the tokens read in file order as `parsePCB_VIA` reads them: `(remove_unused_layers yes)` is `SetRemoveUnconnected( true )`,
    /// `(keep_end_layers yes)` is `SetKeepStartEnd( true )` and `(start_end_only yes)` is `START_END_ONLY`; a `no` changes nothing (a via starts as `KEEP_ALL`).
    pub fn unconnected_layers(&self) -> UnconnectedLayers {
        let mut mode = UnconnectedLayers::KeepAll;
        for t in &self.tokens {
            let Some((name, value)) = flag_token(t) else { continue };
            match (name, value) {
                ("remove_unused_layers", true) => mode = UnconnectedLayers::RemoveAll,
                ("keep_end_layers", true) => mode = UnconnectedLayers::RemoveExceptStartAndEnd,
                ("start_end_only", true) => mode = UnconnectedLayers::StartEndOnly,
                _ => {}
            }
        }
        mode
    }

    /// `PADSTACK::SetUnconnectedLayerMode` on the tokens, in the words and the place `PCB_IO_KICAD_SEXPR::format( const PCB_TRACK* )` writes them: right after the layers, `(remove_unused_layers
    /// yes) (keep_end_layers no|yes)` or `(start_end_only yes)` and nothing for `KEEP_ALL`, and the `(zone_layer_connections ..)` list after `free` only for a via that removes (the writer prints it
    /// only then, empty when no layer is forced); every other token stays where it was.
    pub fn set_unconnected_layers(&mut self, mode: UnconnectedLayers) {
        let keep_zone_layers = self.tokens.iter().find(|t| t.starts_with("(zone_layer_connections")).cloned();
        self.tokens.retain(|t| !matches!(flag_token(t), Some(("remove_unused_layers" | "keep_end_layers" | "start_end_only", _))) && !t.starts_with("(zone_layer_connections"));
        let head: Vec<String> = match mode {
            UnconnectedLayers::KeepAll => vec![],
            UnconnectedLayers::RemoveAll => vec!["(remove_unused_layers yes)".into(), "(keep_end_layers no)".into()],
            UnconnectedLayers::RemoveExceptStartAndEnd => vec!["(remove_unused_layers yes)".into(), "(keep_end_layers yes)".into()],
            UnconnectedLayers::StartEndOnly => vec!["(start_end_only yes)".into()],
        };
        for (i, t) in head.iter().enumerate() {
            self.tokens.insert(i, t.clone());
        }
        if mode.removes() {
            let at = head.len() + usize::from(self.tokens.get(head.len()).is_some_and(|t| t == "free"));
            self.tokens.insert(at, keep_zone_layers.unwrap_or_else(|| "(zone_layer_connections)".to_string()));
        }
    }
}

/// `(name yes)` or `(name no)`: a file token that is one boolean setting. A bare `(name)` is `yes` (`parseMaybeAbsentBool( true )`).
fn flag_token(token: &str) -> Option<(&str, bool)> {
    let inner = token.strip_prefix('(')?.strip_suffix(')')?;
    let mut words = inner.split_whitespace();
    let name = words.next()?;
    let value = match words.next() {
        None => true,
        Some("yes") | Some("true") => true,
        Some("no") | Some("false") => false,
        Some(_) => return None,
    };
    words.next().is_none().then_some((name, value))
}

impl crate::ir::DrawingsSection {
    /// The extras of the via `id`, if it has any.
    pub fn via_extra(&self, id: &str) -> Option<&ViaExtra> {
        self.via_extras.iter().find(|e| e.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extra(tokens: &[&str]) -> ViaExtra {
        ViaExtra { id: "v".into(), kind: ViaKind::Through, tokens: tokens.iter().map(|t| t.to_string()).collect() }
    }

    #[test]
    fn the_unconnected_layer_mode_is_read_as_the_vias_parser_reads_the_tokens() {
        assert_eq!(extra(&[]).unconnected_layers(), UnconnectedLayers::KeepAll);
        assert_eq!(extra(&["(remove_unused_layers no)"]).unconnected_layers(), UnconnectedLayers::KeepAll);
        assert_eq!(extra(&["(remove_unused_layers yes)", "(keep_end_layers no)"]).unconnected_layers(), UnconnectedLayers::RemoveAll);
        assert_eq!(extra(&["(remove_unused_layers yes)", "(keep_end_layers yes)"]).unconnected_layers(), UnconnectedLayers::RemoveExceptStartAndEnd);
        assert_eq!(extra(&["(start_end_only yes)"]).unconnected_layers(), UnconnectedLayers::StartEndOnly);
        // A `yes` is never undone by a later `no`: the parser only acts on the true ones.
        assert_eq!(extra(&["(keep_end_layers yes)", "(remove_unused_layers no)"]).unconnected_layers(), UnconnectedLayers::RemoveExceptStartAndEnd);
        // A bare setting is a yes.
        assert_eq!(extra(&["(remove_unused_layers)"]).unconnected_layers(), UnconnectedLayers::RemoveAll);
    }

    #[test]
    fn the_mode_is_written_where_and_how_the_writer_of_kicad_puts_it() {
        let mut e = extra(&["free", "(tenting (front yes) (back no))"]);
        e.set_unconnected_layers(UnconnectedLayers::RemoveExceptStartAndEnd);
        assert_eq!(e.tokens, ["(remove_unused_layers yes)", "(keep_end_layers yes)", "free", "(zone_layer_connections)", "(tenting (front yes) (back no))"]);
        assert_eq!(e.unconnected_layers(), UnconnectedLayers::RemoveExceptStartAndEnd);
        e.set_unconnected_layers(UnconnectedLayers::RemoveAll);
        assert_eq!(e.tokens[..2], ["(remove_unused_layers yes)", "(keep_end_layers no)"]);
        assert_eq!(e.tokens.iter().filter(|t| t.starts_with("(zone_layer_connections")).count(), 1);
        e.set_unconnected_layers(UnconnectedLayers::StartEndOnly);
        assert_eq!(e.tokens[0], "(start_end_only yes)");
        e.set_unconnected_layers(UnconnectedLayers::KeepAll);
        assert_eq!(e.tokens, ["free", "(tenting (front yes) (back no))"]);
        // The layers a zone flashes the via on are kept while the via removes, and go when it stops removing.
        let mut z = extra(&["(remove_unused_layers yes)", "(keep_end_layers no)", "(zone_layer_connections \"F.Cu\" \"In1.Cu\")"]);
        z.set_unconnected_layers(UnconnectedLayers::RemoveExceptStartAndEnd);
        assert_eq!(z.tokens, ["(remove_unused_layers yes)", "(keep_end_layers yes)", "(zone_layer_connections \"F.Cu\" \"In1.Cu\")"]);
        z.set_unconnected_layers(UnconnectedLayers::KeepAll);
        assert!(z.tokens.is_empty() && z.is_default());
    }
}
