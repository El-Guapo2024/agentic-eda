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

/// `PADSTACK::DRILL_PROPS` of a via's backdrill (`SecondaryDrill`) or tertiary drill (`TertiaryDrill`), as `PCB_IO_KICAD_SEXPR_PARSER::parsePCB_VIA` reads `(backdrill (size ..)
/// (layers start end))` / `(tertiary_drill ..)`: a round drill of `size` (both axes), from the layer the drill starts on to the one it ends on, in the file's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViaBackdrill {
    pub size: crate::units::Nm,
    pub start: String,
    pub end: String,
}

/// `PADSTACK::POST_MACHINING_PROPS` as `parsePostMachining` reads `(front_post_machining counterbore|countersink (size ..) (depth ..) (angle ..))`: the mode as
/// `PAD_DRILL_POST_MACHINING_MODE` numbers it (0 `UNKNOWN` when the via has none, 2 `COUNTERBORE`, 3 `COUNTERSINK`), size and depth in nanometres, the angle in tenths of a degree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PostMachining {
    pub mode: i32,
    pub size: crate::units::Nm,
    pub depth: crate::units::Nm,
    pub angle: i32,
}

/// A file token split into its words and parentheses, a quoted word unquoted.
fn words_of(token: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = token.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == '(' || c == ')' {
            out.push(c.to_string());
            chars.next();
        } else if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut w = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        if let Some(n) = chars.next() {
                            w.push(n);
                        }
                    }
                    '"' => break,
                    c => w.push(c),
                }
            }
            out.push(w);
        } else {
            let mut w = String::new();
            while let Some(&c) = chars.peek() {
                if c == '(' || c == ')' || c.is_whitespace() {
                    break;
                }
                w.push(c);
                chars.next();
            }
            out.push(w);
        }
    }
    out
}

/// The words after `(name` in `words`, up to its `)` (a sub-list's words skipped).
fn field<'a>(words: &'a [String], name: &str) -> Option<&'a [String]> {
    let at = words.windows(2).position(|w| w[0] == "(" && w[1] == name)? + 2;
    let end = words[at..].iter().position(|w| w == "(" || w == ")").map_or(words.len(), |e| at + e);
    Some(&words[at..end])
}

fn board_units(word: Option<&String>) -> Option<crate::units::Nm> {
    word?.parse::<f64>().ok().map(|mm| crate::units::Nm::kiround(mm * 1_000_000.0))
}

impl ViaExtra {
    fn token_words(&self, name: &str) -> Option<Vec<String>> {
        let t = self.tokens.iter().rev().find(|t| t.strip_prefix('(').is_some_and(|r| r.split_whitespace().next() == Some(name)))?;
        Some(words_of(t)[2..].to_vec())
    }

    fn drill_props(&self, name: &str) -> Option<ViaBackdrill> {
        let w = self.token_words(name)?;
        let size = board_units(field(&w, "size")?.first())?;
        let layers = field(&w, "layers")?;
        Some(ViaBackdrill { size, start: layers.first()?.clone(), end: layers.get(1)?.clone() })
    }

    /// The via's backdrill (`Padstack().SecondaryDrill()`), if the file gave it one.
    pub fn secondary_drill(&self) -> Option<ViaBackdrill> {
        self.drill_props("backdrill")
    }

    /// The via's tertiary drill (`Padstack().TertiaryDrill()`), if the file gave it one.
    pub fn tertiary_drill(&self) -> Option<ViaBackdrill> {
        self.drill_props("tertiary_drill")
    }

    /// `Padstack().FrontPostMachining()` (`front`) or `BackPostMachining()`.
    pub fn post_machining(&self, front: bool) -> PostMachining {
        let Some(w) = self.token_words(if front { "front_post_machining" } else { "back_post_machining" }) else { return PostMachining::default() };
        let mode = match w.first().map(String::as_str) {
            Some("counterbore") => 2,
            Some("countersink") => 3,
            _ => return PostMachining::default(),
        };
        let num = |n: &str| field(&w, n).and_then(|f| board_units(f.first())).unwrap_or(crate::units::Nm::ZERO);
        let angle = field(&w, "angle").and_then(|f| f.first()?.parse::<f64>().ok()).map_or(0, |a| (a * 10.0).round() as i32);
        PostMachining { mode, size: num("size"), depth: num("depth"), angle }
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
    fn a_vias_backdrills_and_post_machining_are_read_as_parse_pcb_via_reads_them() {
        let e = extra(&["(backdrill (size 0.6) (layers \"B.Cu\" \"In2.Cu\"))", "(tertiary_drill (size 0.65) (layers \"F.Cu\" \"In1.Cu\"))", "(front_post_machining countersink (size 0.9) (angle 90))"]);
        assert_eq!(e.secondary_drill(), Some(ViaBackdrill { size: crate::units::Nm::new(600_000), start: "B.Cu".into(), end: "In2.Cu".into() }));
        assert_eq!(e.tertiary_drill().map(|d| (d.size, d.start)), Some((crate::units::Nm::new(650_000), "F.Cu".to_string())));
        assert_eq!(e.post_machining(true), PostMachining { mode: 3, size: crate::units::Nm::new(900_000), depth: crate::units::Nm::ZERO, angle: 900 });
        assert_eq!(e.post_machining(false), PostMachining::default());
        assert_eq!(extra(&[]).secondary_drill(), None);
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
