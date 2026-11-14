//! A via's and a pad's own teardrop parameters: `TEARDROP_PARAMETERS`, which every `BOARD_CONNECTED_ITEM` carries (`m_teardropParams`,
//! `pcbnew/board_connected_item.h` and `pcbnew/teardrop/teardrop_parameters.h` at KiCad 8303b2ad).
//!
//! KiCad 25ec2335's `TEARDROP_MANAGER::UpdateTeardrops` reads nothing else: `pad->GetTeardropParams()` and `via->GetTeardropParams()` decide whether an anchor gets
//! teardrops (`m_Enabled`) and how long and wide they are. Board Setup's `TEARDROP_PARAMETERS_LIST` only seeds them (the Edit Teardrops dialog writes its block into
//! every targeted item, `DIALOG_GLOBAL_EDIT_TEARDROPS::processItem`). In this model Board Setup's one block ([`crate::ir::TeardropSettings`]) stands for that dialog
//! and serves every anchor that has no parameters of its own; an anchor that has them -- read from a board file's `(teardrops ..)` or set in Pad Properties,
//! Track & Via Properties or the Properties panel -- is governed by them alone, as KiCad's is (`eda_connectivity::teardrop`).
//!
//! Where they live: a via's are its `(teardrops ..)` token ([`crate::via_extra::ViaExtra::tokens`]); a pad's are its edit
//! ([`crate::fp_edit::PadEdit::teardrops`]), else the token a board file gave it ([`crate::ir::PadMaskInfo::tokens`]). Read as
//! `PCB_IO_KICAD_SEXPR_PARSER::parseTEARDROP_PARAMETERS` reads them and written as `PCB_IO_KICAD_SEXPR::formatTeardropParameters` writes them ([`set_in_tokens`]
//! says where the writing differs).

use crate::units::Nm;
use serde::{Deserialize, Serialize};

/// `TEARDROP_PARAMETERS`. The field names are the file's tokens; `prefer_zone_connections` is `!m_TdOnPadsInZones` (`SetTeardropPreferZoneConnections`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeardropParams {
    /// `m_Enabled`.
    pub enabled: bool,
    /// `m_AllowUseTwoTracks`: "Allow teardrops to span 2 track segments".
    pub allow_two_segments: bool,
    /// `!m_TdOnPadsInZones`: "Prefer zone connection" (no teardrop on a pad a same-net zone already connects).
    pub prefer_zone_connections: bool,
    /// `m_BestLengthRatio`: the teardrop's length as a fraction of the anchor's size.
    pub best_length_ratio: f64,
    /// `m_TdMaxLen`, nm; 0 or less is no limit.
    #[serde(deserialize_with = "crate::units::de")]
    pub max_length: Nm,
    /// `m_BestWidthRatio`: the teardrop's width as a fraction of the anchor's size.
    pub best_width_ratio: f64,
    /// `m_TdMaxWidth`, nm; 0 or less is no limit.
    #[serde(deserialize_with = "crate::units::de")]
    pub max_width: Nm,
    /// `m_CurvedEdges`.
    pub curved_edges: bool,
    /// `m_WidthtoSizeFilterRatio`: a track at least this fraction of the anchor's size gets none.
    pub filter_ratio: f64,
}

impl Default for TeardropParams {
    /// `TEARDROP_PARAMETERS()`: disabled, 1 mm long and 2 mm wide at most, half the anchor long and as wide as it, 90% filter, straight edges, two segments
    /// allowed, zone connections preferred (`m_TdOnPadsInZones( false )`).
    fn default() -> Self {
        TeardropParams {
            enabled: false,
            allow_two_segments: true,
            prefer_zone_connections: true,
            best_length_ratio: 0.5,
            max_length: Nm::new(1_000_000),
            best_width_ratio: 1.0,
            max_width: Nm::new(2_000_000),
            curved_edges: false,
            filter_ratio: 0.9,
        }
    }
}

/// `FormatDouble2Str` (`common/string_utils.cpp`): `{:.10g}`, or sixteen decimals with the zeros taken off for a value of 0.0001 or less.
fn double_2_str(v: f64) -> String {
    if v != 0.0 && v.abs() <= 0.0001 {
        let s = format!("{v:.16}");
        return s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    crate::units::format_g10(v)
}

/// `KICAD_FORMAT::FormatBool`.
fn yes_no(v: bool) -> &'static str {
    if v {
        "yes"
    } else {
        "no"
    }
}

impl TeardropParams {
    /// `isDefaultTeardropParameters`: what KiCad's writer leaves out.
    pub fn is_default(&self) -> bool {
        *self == TeardropParams::default()
    }

    /// `PCB_IO_KICAD_SEXPR_PARSER::parseTEARDROP_PARAMETERS` of a `(teardrops ..)` token: the item's parameters as they were (here the defaults) with `m_Enabled` and
    /// `m_AllowUseTwoTracks` off and `m_TdOnPadsInZones` on, then each sub-token. `None` when `token` is not a `(teardrops ..)`.
    pub fn parse(token: &str) -> Option<TeardropParams> {
        let words = crate::via_extra::words_of(token);
        if words.get(1).map(String::as_str) != Some("teardrops") {
            return None;
        }
        let mut p = TeardropParams { enabled: false, allow_two_segments: false, prefer_zone_connections: false, ..Default::default() };
        let mut i = 2;
        while i < words.len() {
            if words[i] != "(" {
                i += 1;
                continue;
            }
            let name = words.get(i + 1).map(String::as_str).unwrap_or("");
            // the sub-token's own words, up to its ")"
            let end = words[i + 2..].iter().position(|w| w == ")").map_or(words.len(), |e| i + 2 + e);
            let args = &words[i + 2..end];
            // `parseMaybeAbsentBool( aDefault )`: a bare `(name)` is `aDefault`.
            let maybe_bool = |default: bool| match args.first().map(String::as_str) {
                None => default,
                Some(w) => matches!(w, "yes" | "true"),
            };
            let num = || args.first().and_then(|w| w.parse::<f64>().ok());
            let mm = || num().map(|v| Nm::kiround(v * 1_000_000.0));
            match name {
                "enabled" => p.enabled = maybe_bool(true),
                "allow_two_segments" => p.allow_two_segments = maybe_bool(true),
                // `m_TdOnPadsInZones = !parseMaybeAbsentBool( false )`
                "prefer_zone_connections" => p.prefer_zone_connections = maybe_bool(false),
                "best_length_ratio" => p.best_length_ratio = num().unwrap_or(p.best_length_ratio),
                "max_length" => p.max_length = mm().unwrap_or(p.max_length),
                "best_width_ratio" => p.best_width_ratio = num().unwrap_or(p.best_width_ratio),
                "max_width" => p.max_width = mm().unwrap_or(p.max_width),
                // the legacy token: any curve point makes the edges curved
                "curve_points" => p.curved_edges = num().is_some_and(|n| n as i64 > 0),
                "curved_edges" => p.curved_edges = maybe_bool(true),
                "filter_ratio" => p.filter_ratio = num().unwrap_or(p.filter_ratio),
                _ => {}
            }
            i = end + 1;
        }
        Some(p)
    }

    /// The last `(teardrops ..)` among a file's kept tokens, read.
    pub fn from_tokens(tokens: &[String]) -> Option<TeardropParams> {
        tokens.iter().rev().find(|t| is_teardrops_token(t)).and_then(|t| TeardropParams::parse(t))
    }

    /// `PCB_IO_KICAD_SEXPR::formatTeardropParameters`.
    pub fn format(&self) -> String {
        format!(
            "(teardrops (best_length_ratio {}) (max_length {}) (best_width_ratio {}) (max_width {}) (curved_edges {}) (filter_ratio {}) (enabled {}) (allow_two_segments {}) (prefer_zone_connections {}))",
            double_2_str(self.best_length_ratio),
            crate::units::format_mm(self.max_length),
            double_2_str(self.best_width_ratio),
            crate::units::format_mm(self.max_width),
            yes_no(self.curved_edges),
            double_2_str(self.filter_ratio),
            yes_no(self.enabled),
            yes_no(self.allow_two_segments),
            yes_no(self.prefer_zone_connections),
        )
    }

    /// The teardrop generator's settings for an anchor these parameters govern: every target kind on (the item is the anchor), the sizes and filter these.
    pub fn as_settings(&self) -> crate::ir::TeardropSettings {
        crate::ir::TeardropSettings {
            enabled: self.enabled,
            target_vias: true,
            target_pth_pads: true,
            target_smd_pads: true,
            best_length_ratio: self.best_length_ratio,
            best_width_ratio: self.best_width_ratio,
            max_len_nm: self.max_length,
            max_width_nm: self.max_width,
            width_to_size_filter_ratio: self.filter_ratio,
            curved_edges: self.curved_edges,
            allow_two_segments: self.allow_two_segments,
            prefer_zone_connections: self.prefer_zone_connections,
            track_end: TeardropParams::default(),
            // (a non-round anchor these govern takes them too)
            rect_shape: Some(*self),
            round_shapes_only: false,
        }
    }

    /// What Board Setup's block gives an anchor that has no parameters of its own (`DIALOG_GLOBAL_EDIT_TEARDROPS::processItem` copies the block into the item):
    /// its enable, sizes, filter, curved edges, two segments and zone preference.
    pub fn of_settings(s: &crate::ir::TeardropSettings) -> TeardropParams {
        TeardropParams {
            enabled: s.enabled,
            allow_two_segments: s.allow_two_segments,
            prefer_zone_connections: s.prefer_zone_connections,
            best_length_ratio: s.best_length_ratio,
            best_width_ratio: s.best_width_ratio,
            max_length: s.max_len_nm,
            max_width: s.max_width_nm,
            curved_edges: s.curved_edges,
            filter_ratio: s.width_to_size_filter_ratio,
        }
    }
}

/// Whether a file token is a `(teardrops ..)`.
pub fn is_teardrops_token(token: &str) -> bool {
    token.strip_prefix('(').is_some_and(|r| r.split(|c: char| c.is_whitespace() || c == '(' || c == ')').next() == Some("teardrops"))
}

/// `tokens` with the `(teardrops ..)` token replaced by `params` (in its place, else at the end), or taken out when `params` is `None`.
///
/// Differs: KiCad's writer leaves out parameters that are its defaults (`isDefaultTeardropParameters`), since an item without the token reads back as those
/// defaults. Here an item without parameters of its own is served by Board Setup's block, so parameters set on the item are written whatever they are: a via
/// whose teardrops were switched off stays off when the board's are on.
pub fn set_in_tokens(tokens: &mut Vec<String>, params: Option<&TeardropParams>) {
    let at = tokens.iter().position(|t| is_teardrops_token(t));
    tokens.retain(|t| !is_teardrops_token(t));
    if let Some(p) = params {
        tokens.insert(at.unwrap_or(tokens.len()).min(tokens.len()), p.format());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_teardrops_token_is_read_as_parse_teardrop_parameters_reads_it() {
        let p = TeardropParams::parse("(teardrops (best_length_ratio 0.5) (max_length 1) (best_width_ratio 1) (max_width 2) (curved_edges no) (filter_ratio 0.9) (enabled yes) (allow_two_segments no) (prefer_zone_connections yes))").unwrap();
        assert_eq!(p, TeardropParams { enabled: true, allow_two_segments: false, ..Default::default() });
        // The parser starts with the item disabled, one segment, and pads in zones taken: a token that names none of them leaves them so.
        let q = TeardropParams::parse("(teardrops (best_length_ratio 0.7) (max_length 0.5))").unwrap();
        assert_eq!((q.enabled, q.allow_two_segments, q.prefer_zone_connections, q.best_length_ratio, q.max_length), (false, false, false, 0.7, Nm::new(500_000)));
        // `parseMaybeAbsentBool`: a bare `(enabled)` is yes, a bare `(prefer_zone_connections)` is no; the legacy curve points.
        let r = TeardropParams::parse("(teardrops (enabled) (prefer_zone_connections) (curve_points 5))").unwrap();
        assert_eq!((r.enabled, r.prefer_zone_connections, r.curved_edges), (true, false, true));
        assert_eq!(TeardropParams::parse("(tenting (front yes))"), None);
    }

    #[test]
    fn the_parameters_are_written_as_format_teardrop_parameters_writes_them_and_read_back() {
        let p = TeardropParams { enabled: true, allow_two_segments: false, best_length_ratio: 0.8, max_length: Nm::new(1_500_000), max_width: Nm::new(1_250_000), filter_ratio: 0.75, ..Default::default() };
        let t = p.format();
        assert_eq!(t, "(teardrops (best_length_ratio 0.8) (max_length 1.5) (best_width_ratio 1) (max_width 1.25) (curved_edges no) (filter_ratio 0.75) (enabled yes) (allow_two_segments no) (prefer_zone_connections yes))");
        assert_eq!(TeardropParams::parse(&t), Some(p));
        // In the tokens: in the place the old one had, gone when the defaults are set.
        let mut tokens = vec!["(tenting (front yes) (back no))".to_string(), "(teardrops (enabled yes))".to_string(), "(die_length 0.25)".to_string()];
        set_in_tokens(&mut tokens, Some(&p));
        assert_eq!(tokens[1], t);
        assert_eq!(TeardropParams::from_tokens(&tokens), Some(p));
        // KiCad's defaults are still the item's own (an item without them is Board Setup's); `None` takes them away.
        set_in_tokens(&mut tokens, Some(&TeardropParams::default()));
        assert_eq!(TeardropParams::from_tokens(&tokens), Some(TeardropParams::default()));
        set_in_tokens(&mut tokens, None);
        assert_eq!(tokens, ["(tenting (front yes) (back no))", "(die_length 0.25)"]);
    }
}
