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

/// A via's backdrill or tertiary drill as the Properties panel edits it (`PADSTACK::DRILL_PROPS`: `(backdrill (size ..) (layers start end))`); see [`ViaProps`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViaDrillProps {
    #[serde(deserialize_with = "crate::units::de")]
    pub size: crate::units::Nm,
    pub start: String,
    pub end: String,
}

/// `PAD_DRILL_POST_MACHINING_MODE`'s two modes a file writes (`NOT_POST_MACHINED` is written as nothing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostMachiningMode {
    Counterbore,
    Countersink,
}

/// `PADSTACK::POST_MACHINING_PROPS` as the panel edits it: `(front_post_machining counterbore (size ..) (depth ..) (angle ..))`, the angle in tenths of a degree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostMachiningProps {
    pub mode: PostMachiningMode,
    #[serde(default, deserialize_with = "crate::units::de")]
    pub size: crate::units::Nm,
    #[serde(default, deserialize_with = "crate::units::de")]
    pub depth: crate::units::Nm,
    #[serde(default)]
    pub angle_ddeg: i32,
}

/// What the Properties panel shows and sets of a via's padstack beyond its size and drill (`TRACK_VIA_DESC`, pcb_track.cpp at 8303b2ad): the tenting, covering and
/// plugging of each side (`has_solder_mask`, `has_covering`, `has_plugging`; `None` is "From board stackup"), the capping and filling of the hole, the backdrill and
/// tertiary drill, and the post-machining of each side. Read from and written to the via's tokens ([`ViaExtra::props`], [`ViaExtra::set_props`]) in the words
/// `PCB_IO_KICAD_SEXPR::format( const PCB_TRACK* )` writes, so the reader and writer that keep the tokens carry them as they always did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViaProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front_tenting: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back_tenting: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front_covering: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back_covering: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front_plugging: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back_plugging: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capping: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filling: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backdrill: Option<ViaDrillProps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tertiary_drill: Option<ViaDrillProps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front_post_machining: Option<PostMachiningProps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back_post_machining: Option<PostMachiningProps>,
}

/// The tags of the tokens [`ViaProps`] reads and writes.
const PROPS_TAGS: &[&str] = &["tenting", "capping", "covering", "plugging", "filling", "backdrill", "tertiary_drill", "front_post_machining", "back_post_machining"];

fn tag_of(token: &str) -> Option<&str> {
    token.strip_prefix('(')?.split(|c: char| c.is_whitespace() || c == '(' || c == ')').next()
}

/// `parseOptBool`: `yes`, `no` or `none`.
fn opt_bool(word: Option<&String>) -> Option<bool> {
    match word.map(String::as_str) {
        Some("yes") | Some("true") => Some(true),
        Some("no") | Some("false") => Some(false),
        _ => None,
    }
}

/// `KICAD_FORMAT::FormatOptBool`: `(key yes)`, `(key no)` or `(key none)`.
fn fmt_opt_bool(key: &str, v: Option<bool>) -> String {
    format!("({key} {})", match v {
        Some(true) => "yes",
        Some(false) => "no",
        None => "none",
    })
}

/// `formatInternalUnits`: millimetres, no trailing zeros.
fn fmt_mm(v: crate::units::Nm) -> String {
    let s = format!("{:.6}", v.as_f64() / 1_000_000.0);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_string() } else { s.to_string() }
}

/// `Quotew`: a layer name in double quotes.
fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

impl ViaExtra {
    /// `parseFrontBackOptBool` of the token `name`: `(name (front yes) (back none))`, or the legacy tenting words `(tenting front back)` / `(tenting none)`.
    fn front_back(&self, name: &str, legacy: bool) -> (Option<bool>, Option<bool>) {
        let Some(w) = self.token_words(name) else { return (None, None) };
        if legacy && w.first().is_some_and(|t| t != "(") {
            let (mut front, mut back) = (None, None);
            for t in w.iter().take_while(|t| *t != ")") {
                match t.as_str() {
                    "front" => front = Some(true),
                    "back" => back = Some(true),
                    "none" => (front, back) = (None, None),
                    _ => {}
                }
            }
            return (front, back);
        }
        (opt_bool(field(&w, "front").and_then(|f| f.first())), opt_bool(field(&w, "back").and_then(|f| f.first())))
    }

    fn opt_flag(&self, name: &str) -> Option<bool> {
        self.token_words(name).and_then(|w| opt_bool(w.first()))
    }

    fn post_machining_props(&self, front: bool) -> Option<PostMachiningProps> {
        let p = self.post_machining(front);
        let mode = match p.mode {
            2 => PostMachiningMode::Counterbore,
            3 => PostMachiningMode::Countersink,
            _ => return None,
        };
        Some(PostMachiningProps { mode, size: p.size, depth: p.depth, angle_ddeg: p.angle })
    }

    /// The via's [`ViaProps`], as `parsePCB_VIA` reads its tokens.
    pub fn props(&self) -> ViaProps {
        let (front_tenting, back_tenting) = self.front_back("tenting", true);
        let (front_covering, back_covering) = self.front_back("covering", false);
        let (front_plugging, back_plugging) = self.front_back("plugging", false);
        let drill = |d: Option<ViaBackdrill>| d.filter(|d| d.size > crate::units::Nm::ZERO).map(|d| ViaDrillProps { size: d.size, start: d.start, end: d.end });
        ViaProps {
            front_tenting,
            back_tenting,
            front_covering,
            back_covering,
            front_plugging,
            back_plugging,
            capping: self.opt_flag("capping"),
            filling: self.opt_flag("filling"),
            backdrill: drill(self.secondary_drill()),
            tertiary_drill: drill(self.tertiary_drill()),
            front_post_machining: self.post_machining_props(true),
            back_post_machining: self.post_machining_props(false),
        }
    }

    /// The via's tokens with `props` in place of what they said of those settings, in the words `format( const PCB_TRACK* )` writes: the drills and the post-machining
    /// first (KiCad writes them before the layers, which the via's own tokens follow here), then the others where KiCad writes them, before a `(padstack ..)` or
    /// `(teardrops ..)` token and after everything else. A side KiCad would write as `none` with the other side unset is not written at all (`has_value()` on neither).
    pub fn set_props(&mut self, props: &ViaProps) {
        self.tokens.retain(|t| !tag_of(t).is_some_and(|tag| PROPS_TAGS.contains(&tag)));
        let mut head = Vec::new();
        let drill = |name: &str, d: &ViaDrillProps| format!("({name} (size {}) (layers {} {}))", fmt_mm(d.size), quoted(&d.start), quoted(&d.end));
        if let Some(d) = props.backdrill.as_ref().filter(|d| d.size > crate::units::Nm::ZERO) {
            head.push(drill("backdrill", d));
        }
        if let Some(d) = props.tertiary_drill.as_ref().filter(|d| d.size > crate::units::Nm::ZERO) {
            head.push(drill("tertiary_drill", d));
        }
        let pm = |name: &str, p: &PostMachiningProps| {
            let mut t = format!("({name} {}", match p.mode {
                PostMachiningMode::Counterbore => "counterbore",
                PostMachiningMode::Countersink => "countersink",
            });
            if p.size > crate::units::Nm::ZERO {
                t.push_str(&format!(" (size {})", fmt_mm(p.size)));
            }
            if p.depth > crate::units::Nm::ZERO {
                t.push_str(&format!(" (depth {})", fmt_mm(p.depth)));
            }
            if p.angle_ddeg > 0 {
                let a = format!("{:.4}", p.angle_ddeg as f64 / 10.0);
                t.push_str(&format!(" (angle {})", a.trim_end_matches('0').trim_end_matches('.')));
            }
            t.push(')');
            t
        };
        if let Some(p) = &props.front_post_machining {
            head.push(pm("front_post_machining", p));
        }
        if let Some(p) = &props.back_post_machining {
            head.push(pm("back_post_machining", p));
        }
        let mut tail = Vec::new();
        let pair = |name: &str, front: Option<bool>, back: Option<bool>| (front.is_some() || back.is_some()).then(|| format!("({name} {} {})", fmt_opt_bool("front", front), fmt_opt_bool("back", back)));
        tail.extend(pair("tenting", props.front_tenting, props.back_tenting));
        if props.capping.is_some() {
            tail.push(fmt_opt_bool("capping", props.capping));
        }
        tail.extend(pair("covering", props.front_covering, props.back_covering));
        tail.extend(pair("plugging", props.front_plugging, props.back_plugging));
        if props.filling.is_some() {
            tail.push(fmt_opt_bool("filling", props.filling));
        }
        let at = self.tokens.iter().position(|t| matches!(tag_of(t), Some("padstack") | Some("teardrops"))).unwrap_or(self.tokens.len());
        self.tokens.splice(at..at, tail);
        self.tokens.splice(0..0, head);
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
    fn a_vias_props_are_read_as_parse_pcb_via_reads_them_and_written_as_format_writes_them() {
        let e = extra(&["(remove_unused_layers yes)", "(keep_end_layers no)", "(tenting (front yes) (back no))", "(capping yes)", "(covering (front no) (back none))", "(filling no)", "(teardrops (best_length_ratio 0.5))"]);
        let p = e.props();
        assert_eq!((p.front_tenting, p.back_tenting, p.capping, p.front_covering, p.back_covering, p.filling, p.front_plugging), (Some(true), Some(false), Some(true), Some(false), None, Some(false), None));
        // The legacy tenting words: `front back` tents both sides, `none` neither.
        assert_eq!(extra(&["(tenting front back)"]).props().back_tenting, Some(true));
        assert_eq!(extra(&["(tenting none)"]).props().front_tenting, None);
        // Written back: the drills and post-machining first, the others before the teardrops, in `format`'s order; an unchanged set leaves the tokens as they were.
        let mut w = e.clone();
        w.set_props(&p);
        assert_eq!(w.tokens, ["(remove_unused_layers yes)", "(keep_end_layers no)", "(tenting (front yes) (back no))", "(capping yes)", "(covering (front no) (back none))", "(filling no)", "(teardrops (best_length_ratio 0.5))"]);
        let mut q = p.clone();
        q.front_tenting = None;
        q.back_tenting = None;
        q.front_plugging = Some(true);
        q.backdrill = Some(ViaDrillProps { size: crate::units::Nm::new(450_000), start: "B.Cu".into(), end: "In2.Cu".into() });
        q.front_post_machining = Some(PostMachiningProps { mode: PostMachiningMode::Countersink, size: crate::units::Nm::new(900_000), depth: crate::units::Nm::ZERO, angle_ddeg: 900 });
        w.set_props(&q);
        assert_eq!(
            w.tokens,
            [
                "(backdrill (size 0.45) (layers \"B.Cu\" \"In2.Cu\"))",
                "(front_post_machining countersink (size 0.9) (angle 90))",
                "(remove_unused_layers yes)",
                "(keep_end_layers no)",
                "(capping yes)",
                "(covering (front no) (back none))",
                "(plugging (front yes) (back none))",
                "(filling no)",
                "(teardrops (best_length_ratio 0.5))",
            ]
        );
        assert_eq!(w.props(), q, "what is written reads back the same");
        assert_eq!(w.unconnected_layers(), UnconnectedLayers::RemoveAll, "the other tokens stay");
        // Nothing set leaves nothing behind.
        let mut z = extra(&["(tenting (front yes) (back yes))"]);
        z.set_props(&ViaProps::default());
        assert!(z.is_default());
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
