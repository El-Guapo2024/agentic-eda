//! The board's plot settings: `(setup (pcbplotparams ..))` of the `.kicad_pcb`, KiCad's `PCB_PLOT_PARAMS` (`pcbnew/pcb_plot_params.cpp`).
//!
//! KiCad's Plot dialog plots with these settings (`DIALOG_PLOT::Plot` takes `m_editFrame->GetPlotSettings()`), and so does `kicad-cli pcb
//! export gerbers --board-plot-params` (`PCBNEW_JOBS_HANDLER::JobExportGerbers`: `plotOpts = boardPlotOptions`). An imported board keeps
//! its own block: the layer selection decoded by KiCad's rules (`PCB_PLOT_PARAMS_PARSER::Parse`'s `T_layerselection`, the legacy layer
//! numbers of a file older than 20240819 remapped as `remapLegacyLayerLSET` does), and every other item as the file wrote it. The two dash
//! ratios are [`crate::setup_pages::Formatting`]'s (Text & Graphics > Formatting edits them), so the writer puts the board's own in.
//! A board with no block of its own is plotted with `PCB_PLOT_PARAMS::PCB_PLOT_PARAMS`' defaults, written as KiCad 10.99's
//! `PCB_PLOT_PARAMS::Format` writes them ([`PlotParams::default_items`]).

use serde::{Deserialize, Serialize};

use crate::layers::{pcb_layer_id, user_layer_name, MAX_USER_LAYERS, TECH_LAYERS};

/// `Rescue` (`include/layer_ids.h`).
const RESCUE: u32 = 37;

/// `PCB_LAYER_ID_COUNT` (`include/layer_ids.h`).
const LAYER_COUNT: u32 = 128;

/// The board file version from which `layerselection` is in today's layer numbers (`PCB_PLOT_PARAMS_PARSER::Parse`: "The layers were
/// renumbered in 5e0abadb2342 .. this value is the one immediately after that happened").
pub const RENUMBERED_LAYERS_VERSION: i64 = 20240819;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PlotParams {
    /// `m_layerSelection`, by the layer names the board file writes, in `PCB_LAYER_ID` order.
    #[serde(default)]
    pub layer_selection: Vec<String>,
    /// `m_plotOnAllLayersSequence` ("Plot on All Layers"), the same way.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plot_on_all_layers: Vec<String>,
    /// Every other item of the block as the file wrote it, in its order (`(usegerberextensions yes)`, `(gerberprecision 5)`, ..), the two
    /// `dashed_line_*_ratio` items included as markers of where the board's ratios go.
    #[serde(default)]
    pub items: Vec<String>,
}

/// Every layer name a `PCB_LAYER_ID` has, with its number.
fn all_layers() -> Vec<(u32, String)> {
    let mut names: Vec<String> = vec!["F.Cu".into(), "B.Cu".into()];
    names.extend((1..=30).map(|n| format!("In{n}.Cu")));
    names.extend(TECH_LAYERS.iter().map(|s| s.to_string()));
    names.extend((1..=MAX_USER_LAYERS).map(user_layer_name));
    let mut v: Vec<(u32, String)> = names.into_iter().filter_map(|n| pcb_layer_id(&n).map(|id| (id, n))).collect();
    // `Rescue` (37) is no layer a board defines, but a selection read with `ParseHex` keeps its bit, and `FmtHex` writes it back.
    v.push((RESCUE, "Rescue".into()));
    v.sort();
    v
}

/// The bits of `BASE_SET::ParseHex` (the leading `0x` skipped; `_` ignored; the last nibble is bits 0..3).
fn parse_hex_bits(hex: &str, bit_count: u32) -> Vec<u32> {
    let mut bits = Vec::new();
    let mut nibble_ndx = 0u32;
    for c in hex.chars().rev() {
        if c == '_' {
            continue;
        }
        let Some(nibble) = c.to_digit(16) else { break };
        for b in 0..4 {
            let bit = nibble_ndx * 4 + b;
            if bit < bit_count && nibble & (1 << b) != 0 {
                bits.push(bit);
            }
        }
        nibble_ndx += 1;
        if nibble_ndx * 4 >= bit_count {
            break;
        }
    }
    bits
}

/// `LEGACY_PCB_LAYER_ID` -> today's layer name (`s_legacyLayerIdMap`): F.Cu 0, In1..In30 1..30, B.Cu 31, then B/F Adhes, Paste, SilkS,
/// Mask, the user drawing layers, Edge.Cuts, Margin, the courtyards, the fab layers and User.1 .. User.9.
fn legacy_layer_name(id: u32) -> Option<String> {
    const TECH: [&str; 18] = ["B.Adhes", "F.Adhes", "B.Paste", "F.Paste", "B.SilkS", "F.SilkS", "B.Mask", "F.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "B.CrtYd", "F.CrtYd", "B.Fab", "F.Fab"];
    match id {
        0 => Some("F.Cu".into()),
        1..=30 => Some(format!("In{id}.Cu")),
        31 => Some("B.Cu".into()),
        32..=49 => Some(TECH[(id - 32) as usize].into()),
        50..=58 => Some(user_layer_name(id - 49)),
        _ => None,
    }
}

/// `LSET::AllCuMask()` | silkscreens: what a legacy integer `layerselection` reads as.
fn all_copper() -> Vec<String> {
    all_layers().into_iter().filter(|(_, n)| crate::layers::is_copper_layer_name(n)).map(|(_, n)| n).collect()
}

/// The layer names of a `layerselection` / `plot_on_all_layers_selection` value as `PCB_PLOT_PARAMS_PARSER::Parse` reads it from a file
/// of `file_version`, in `PCB_LAYER_ID` order. A number (the "pretty 3" legacy stack) is every copper layer and both silkscreens.
pub fn decode_layer_selection(value: &str, file_version: i64, number: bool) -> Vec<String> {
    let mut names: Vec<String> = if number {
        let mut v = all_copper();
        v.extend(["F.SilkS".to_string(), "B.SilkS".to_string()]);
        v
    } else {
        let hex = value.strip_prefix("0x").unwrap_or(value);
        if file_version < RENUMBERED_LAYERS_VERSION {
            parse_hex_bits(hex, 64).into_iter().filter_map(legacy_layer_name).collect()
        } else {
            let table = all_layers();
            parse_hex_bits(hex, LAYER_COUNT).into_iter().filter_map(|b| table.iter().find(|(id, _)| *id == b).map(|(_, n)| n.clone())).collect()
        }
    };
    names.sort_by_key(|n| layer_number(n).unwrap_or(u32::MAX));
    names.dedup();
    names
}

/// `pcb_layer_id`, and `Rescue`.
fn layer_number(name: &str) -> Option<u32> {
    if name == "Rescue" {
        Some(RESCUE)
    } else {
        pcb_layer_id(name)
    }
}

/// `BASE_SET::FmtHex` of the set of `layers` (128 bits, `_` between groups of eight nibbles), with KiCad's `0x` prefix.
pub fn encode_layer_selection(layers: &[String]) -> String {
    let mut bits = [false; LAYER_COUNT as usize];
    for l in layers {
        if let Some(id) = layer_number(l) {
            bits[id as usize] = true;
        }
    }
    let mut s = String::new();
    for nibble in 0..(LAYER_COUNT as usize / 4) {
        let mut ndx = 0u32;
        for b in 0..4 {
            if bits[nibble * 4 + b] {
                ndx |= 1 << b;
            }
        }
        if nibble != 0 && nibble % 8 == 0 {
            s.push('_');
        }
        s.push(std::char::from_digit(ndx, 16).unwrap());
    }
    format!("0x{}", s.chars().rev().collect::<String>())
}

/// The items `PCB_PLOT_PARAMS::Format` writes after the two layer sets: the only ones a board KiCad saves keeps.
pub const FORMAT_TOKENS: &[&str] = &[
    "disableapertmacros", "usegerberextensions", "usegerberattributes", "usegerberadvancedattributes", "creategerberjobfile", "gerberprecision",
    "dashed_line_dash_ratio", "dashed_line_gap_ratio", "svgprecision", "pngdpi", "pngantialias", "plotframeref", "mode", "useauxorigin",
    "pdf_front_fp_property_popups", "pdf_back_fp_property_popups", "pdf_metadata", "pdf_single_document", "dxfpolygonmode", "dxfimperialunits",
    "dxfusepcbnewfont", "psnegative", "psa4output", "plot_black_and_white", "sketchpadsonfab", "plotpadnumbers", "hidednponfab", "sketchdnponfab",
    "crossoutdnponfab", "subtractmaskfromsilk", "outputformat", "mirror", "drillshape", "scaleselection", "outputdirectory",
];

impl PlotParams {
    /// `PCB_PLOT_PARAMS::PCB_PLOT_PARAMS`' `m_layerSelection`: both silkscreens, masks and pastes, the board outline, and every copper layer.
    pub fn default_layer_selection() -> Vec<String> {
        let mut v = all_copper();
        v.extend(["F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "F.Paste", "B.Paste", "Edge.Cuts"].map(String::from));
        v.sort_by_key(|n| pcb_layer_id(n).unwrap_or(u32::MAX));
        v
    }

    /// The items `PCB_PLOT_PARAMS::Format` writes for a default `PCB_PLOT_PARAMS` after the two layer sets, in its order (what kicad-cli 10.99's
    /// `pcb upgrade` writes for a board with no block).
    pub fn default_items() -> Vec<String> {
        [
            "(disableapertmacros no)",
            "(usegerberextensions no)",
            "(usegerberattributes yes)",
            "(usegerberadvancedattributes yes)",
            "(creategerberjobfile yes)",
            "(dashed_line_dash_ratio 12)",
            "(dashed_line_gap_ratio 3)",
            "(svgprecision 4)",
            "(pngdpi 300)",
            "(pngantialias yes)",
            "(plotframeref no)",
            "(mode 1)",
            "(useauxorigin no)",
            "(pdf_front_fp_property_popups yes)",
            "(pdf_back_fp_property_popups yes)",
            "(pdf_metadata yes)",
            "(pdf_single_document no)",
            "(dxfpolygonmode yes)",
            "(dxfimperialunits yes)",
            "(dxfusepcbnewfont yes)",
            "(psnegative no)",
            "(psa4output no)",
            "(plot_black_and_white yes)",
            "(sketchpadsonfab no)",
            "(plotpadnumbers no)",
            "(hidednponfab no)",
            "(sketchdnponfab yes)",
            "(crossoutdnponfab yes)",
            "(subtractmaskfromsilk no)",
            "(outputformat 1)",
            "(mirror no)",
            "(drillshape 1)",
            "(scaleselection 1)",
            "(outputdirectory \"\")",
        ]
        .map(String::from)
        .to_vec()
    }

    /// The board's plot settings: an imported board's own block, else `PCB_PLOT_PARAMS`' defaults, read as KiCad reads them back
    /// ([`PlotParams::normalized`]: a board imported before the legacy tokens were dropped on import comes out the same).
    pub fn of_design(design: &crate::ir::Design) -> PlotParams {
        design.drawings.as_ref().and_then(|d| d.plot_params.clone()).map(PlotParams::normalized).unwrap_or_else(Self::kicad_default)
    }

    /// One item of the block as `PCB_PLOT_PARAMS_PARSER::Parse` reads it and `PCB_PLOT_PARAMS::Format` writes it back, in its place in
    /// the block. A legacy token `Format` no longer writes is applied and dropped, as a board KiCad saves loses it:
    /// - `(excludeedgelayer no)` is Edge.Cuts pushed on "Plot on All Layers" (`case T_excludeedgelayer: if( !parseBool() )
    ///   m_plotOnAllLayersSequence.push_back( Edge_Cuts )`); `yes` is nothing;
    /// - `(viasonmask ..)` is the board's via tenting, which the board reader applies (`case T_pcbplotparams`:
    ///   `GetLegacyPlotViaOnMaskLayer()`), so it is not kept here: written back after `(tenting ..)` it would undo the tenting on
    ///   every read;
    /// - `svguseinch`, `pscolor`, `plotinvisibletext`, the HPGL pen tokens and any unknown token (`default: skipCurrent()`) are read
    ///   and dropped;
    /// - `(gerberprecision n)` is clamped to 5..6 (`parseInt( gbrDefaultPrecision - 1, gbrDefaultPrecision )`), and 6, the default, is
    ///   not written (`if( m_gerberPrecision != gbrDefaultPrecision )`).
    pub fn read_item(&mut self, item: &str) {
        let body = item.trim().trim_start_matches('(').trim_end_matches(')');
        let mut words = body.split_whitespace();
        let Some(tag) = words.next() else { return };
        let value = words.next().unwrap_or("");
        match tag {
            "excludeedgelayer" => {
                if matches!(value, "no" | "false") && !self.plot_on_all_layers.iter().any(|l| l == "Edge.Cuts") {
                    self.plot_on_all_layers.push("Edge.Cuts".into());
                }
            }
            "gerberprecision" => {
                // `atoi`: a leading number, else 0.
                let digits: String = value.chars().enumerate().take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+'))).map(|(_, c)| c).collect();
                let n = digits.parse::<i64>().unwrap_or(0).clamp(5, 6);
                self.items.retain(|i| !i.starts_with("(gerberprecision"));
                if n != 6 {
                    self.items.push(format!("(gerberprecision {n})"));
                }
            }
            t if FORMAT_TOKENS.contains(&t) => self.items.push(item.to_string()),
            _ => {}
        }
    }

    /// The block as KiCad reads it back: every item through [`PlotParams::read_item`], in order.
    pub fn normalized(self) -> PlotParams {
        let mut p = PlotParams { layer_selection: self.layer_selection, plot_on_all_layers: self.plot_on_all_layers, items: Vec::new() };
        for item in &self.items {
            p.read_item(item);
        }
        p
    }

    /// The layers the selection names, of those the board has (its copper layers, and every non-copper layer, which a `.kicad_pcb` always defines;
    /// never `Rescue`, which `GetEnabledLayers` never has): the Plot dialog's checklist as `DIALOG_PLOT::init_Dialog` fills it from `GetLayerSelection()`.
    pub fn board_layers(&self, copper: &[String]) -> Vec<String> {
        self.layer_selection.iter().filter(|l| *l != "Rescue" && (!crate::layers::is_copper_layer_name(l) || copper.contains(l))).cloned().collect()
    }

    /// A default `PCB_PLOT_PARAMS`.
    pub fn kicad_default() -> PlotParams {
        PlotParams { layer_selection: Self::default_layer_selection(), plot_on_all_layers: Vec::new(), items: Self::default_items() }
    }

    /// The item whose tag is `key`, if the block has one.
    pub fn item(&self, key: &str) -> Option<&str> {
        let open = format!("({key} ");
        let bare = format!("({key})");
        self.items.iter().find(|i| i.starts_with(&open) || **i == bare).map(String::as_str)
    }

    /// Sets the item `key` to `(key value)`, in its place, or at the end when the block has none.
    pub fn set_item(&mut self, key: &str, value: &str) {
        let open = format!("({key} ");
        let text = format!("({key} {value})");
        match self.items.iter_mut().find(|i| i.starts_with(&open)) {
            Some(i) => *i = text,
            None => self.items.push(text),
        }
    }

    /// The value of a yes/no item (`KICAD_FORMAT::FormatBool`; the parser's `parseBool` also takes `true`/`false`).
    pub fn flag(&self, key: &str) -> Option<bool> {
        let item = self.item(key)?;
        let v = item.trim_start_matches('(').trim_end_matches(')').split_whitespace().nth(1)?;
        Some(matches!(v, "yes" | "true"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_selection_is_kicads_hex() {
        // What kicad-cli 10.99's `pcb upgrade` writes for a board with no `pcbplotparams`.
        assert_eq!(encode_layer_selection(&PlotParams::default_layer_selection()), "0x00000000_00000000_55555555_5755f5ff");
        assert_eq!(encode_layer_selection(&[]), "0x00000000_00000000_00000000_00000000");
        assert_eq!(decode_layer_selection("0x00000000_00000000_55555555_5755f5ff", 20241229, false), PlotParams::default_layer_selection());
    }

    #[test]
    fn the_rescue_bit_is_kept() {
        // r3 board-io F11: `video-v10`'s selection, bit 37 (Rescue) included, reads and writes back unchanged.
        let v = decode_layer_selection("0x00000000_00000000_000010fc_ffffffff", 20241229, false);
        assert!(v.contains(&"Rescue".to_string()));
        assert_eq!(encode_layer_selection(&v), "0x00000000_00000000_000010fc_ffffffff");
        assert!(!PlotParams { layer_selection: v, ..Default::default() }.board_layers(&[]).contains(&"Rescue".to_string()));
    }

    #[test]
    fn a_legacy_selection_is_remapped() {
        // KiCad 8's default (`0x00010fc_ffffffff` in the old numbers): every copper layer, F/B Paste, SilkS, Mask and Edge.Cuts.
        let v = decode_layer_selection("0x00010fc_ffffffff", 20240108, false);
        assert_eq!(v, PlotParams::default_layer_selection());
        // A number is every copper layer and both silkscreens.
        let n = decode_layer_selection("1", 20240108, true);
        assert!(n.contains(&"In30.Cu".to_string()) && n.contains(&"B.SilkS".to_string()) && !n.contains(&"Edge.Cuts".to_string()));
    }

    #[test]
    fn items_are_read_and_set_in_place() {
        let mut p = PlotParams::kicad_default();
        assert_eq!(p.flag("usegerberextensions"), Some(false));
        p.set_item("usegerberextensions", "yes");
        assert_eq!(p.flag("usegerberextensions"), Some(true));
        assert_eq!(p.items[1], "(usegerberextensions yes)");
        p.set_item("gerberprecision", "5");
        assert_eq!(p.items.last().unwrap(), "(gerberprecision 5)");
    }
}
