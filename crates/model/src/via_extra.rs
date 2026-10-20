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
}

impl crate::ir::DrawingsSection {
    /// The extras of the via `id`, if it has any.
    pub fn via_extra(&self, id: &str) -> Option<&ViaExtra> {
        self.via_extras.iter().find(|e| e.id == id)
    }
}
