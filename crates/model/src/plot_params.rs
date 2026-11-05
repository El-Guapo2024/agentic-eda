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

    /// The Plot dialog's checklist stored back as the layer selection (`DIALOG_PLOT::applyPlotSettings`): the `checked` layers, plus every
    /// copper layer the board does not have (`selectedLayers | ( LSET::AllCuMask() & ~GetEnabledLayers() )`: "If someone enables more
    /// copper layers they will be selected by default"), in `PCB_LAYER_ID` order. A layer the checklist does not list (one the board has
    /// not enabled, `Rescue`) is not kept, as `m_layerList` is the board's enabled layers.
    pub fn set_dialog_layer_selection(&mut self, checked: &[String], board_copper: &[String]) {
        let mut v: Vec<String> = checked.iter().filter(|l| layer_number(l).is_some() && *l != "Rescue").cloned().collect();
        v.extend(all_copper().into_iter().filter(|l| !board_copper.contains(l)));
        v.sort_by_key(|n| layer_number(n).unwrap_or(u32::MAX));
        v.dedup();
        self.layer_selection = v;
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

    /// The value of a number item as `PCB_PLOT_PARAMS_PARSER::parseInt( aMin, aMax )` reads it (`atoi`, then clamped to the token's range,
    /// [`option_kind`]): `(drillshape 7)` is 2.
    pub fn int(&self, key: &str) -> Option<i64> {
        let item = self.item(key)?;
        let v = item.trim_start_matches('(').trim_end_matches(')').split_whitespace().nth(1)?;
        let digits: String = v.chars().enumerate().take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+'))).map(|(_, c)| c).collect();
        let n = digits.parse::<i64>().unwrap_or(0);
        Some(match option_kind(key) {
            Some(OptionKind::Int(lo, hi)) => n.clamp(lo, hi),
            _ => n,
        })
    }

    /// `m_outputDirectory`: the `(outputdirectory "..")` item's text (`NeedSYMBOLorNUMBER`, so an unquoted `plots/` too), "" when the block has none.
    pub fn output_directory(&self) -> String {
        let Some(item) = self.item("outputdirectory") else { return String::new() };
        let body = item.trim().strip_prefix("(outputdirectory").unwrap_or("").trim().strip_suffix(')').unwrap_or("").trim();
        match body.strip_prefix('"').and_then(|b| b.strip_suffix('"')) {
            Some(quoted) => {
                // The DSN lexer's escapes, as `OUTPUTFORMATTER::Quotew` writes them.
                let mut out = String::new();
                let mut chars = quoted.chars();
                while let Some(c) = chars.next() {
                    if c == '\\' {
                        match chars.next() {
                            Some('n') => out.push('\n'),
                            Some('r') => out.push('\r'),
                            Some(o) => out.push(o),
                            None => {}
                        }
                    } else {
                        out.push(c);
                    }
                }
                out
            }
            None => body.to_string(),
        }
    }

    /// Sets the item `key` in `PCB_PLOT_PARAMS::Format`'s place for it ([`FORMAT_TOKENS`]' order) when the block has none yet, so a board
    /// written back lists its items as KiCad writes them.
    fn set_item_in_order(&mut self, key: &str, text: String) {
        let open = format!("({key} ");
        if let Some(i) = self.items.iter_mut().find(|i| i.starts_with(&open)) {
            *i = text;
            return;
        }
        let rank = |k: &str| FORMAT_TOKENS.iter().position(|t| *t == k);
        let mine = rank(key);
        let at = self.items.iter().position(|i| {
            let tag = i.trim_start_matches('(').split(|c: char| c.is_whitespace() || c == ')').next().unwrap_or("");
            matches!((rank(tag), mine), (Some(theirs), Some(mine)) if theirs > mine)
        });
        match at {
            Some(i) => self.items.insert(i, text),
            None => self.items.push(text),
        }
    }

    /// One of the Plot dialog's controls stored back as `DIALOG_PLOT::applyPlotSettings` stores it (and the Generate Drill Files dialog's
    /// output folder, `DIALOG_GENDRILL::updateConfig`): `key` is the token `PCB_PLOT_PARAMS::Format` writes for it, `value` its text --
    /// `yes`/`no` for a switch, a number for a choice (clamped to the token's range as the parser clamps it), the folder as typed for
    /// `outputdirectory` (backslashes made forward slashes: `dirStr.Replace( wxT( "\\" ), wxT( "/" ) )`). `gerberprecision` 6 is no item at
    /// all (`Format` writes it only when it is not `gbrDefaultPrecision`). Refused: a token `Format` does not write, the layer sets (their own
    /// fields) and the dash ratios (Text & Graphics > Formatting's), and a value that is none of its kind's.
    pub fn set_option(&mut self, key: &str, value: &str) -> Result<(), String> {
        let kind = option_kind(key).ok_or_else(|| format!("the plot settings have no option {key:?}"))?;
        match kind {
            OptionKind::Bool => {
                let v = match value {
                    "yes" | "true" => "yes",
                    "no" | "false" => "no",
                    _ => return Err(format!("{key} is yes or no, not {value:?}")),
                };
                self.set_item_in_order(key, format!("({key} {v})"));
            }
            OptionKind::Int(lo, hi) => {
                let n: i64 = value.trim().parse().map_err(|_| format!("{key} is a number, not {value:?}"))?;
                let n = n.clamp(lo, hi);
                if key == "gerberprecision" && n == 6 {
                    self.items.retain(|i| !i.starts_with("(gerberprecision"));
                } else {
                    self.set_item_in_order(key, format!("({key} {n})"));
                }
            }
            OptionKind::Text => {
                let dir = value.replace('\\', "/");
                let quoted = format!("\"{}\"", dir.replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r"));
                self.set_item_in_order(key, format!("({key} {quoted})"));
            }
        }
        Ok(())
    }
}

impl PlotParams {
    /// Every control the dialogs store ([`option_kind`]), as the block says it or as `PCB_PLOT_PARAMS::PCB_PLOT_PARAMS` starts it when the
    /// block has no item for it: `yes`/`no`, a number (clamped as the parser clamps it), the output folder's text.
    pub fn dialog_values(&self) -> std::collections::BTreeMap<String, String> {
        let defaults = PlotParams::kicad_default();
        let mut out = std::collections::BTreeMap::new();
        for token in FORMAT_TOKENS.iter().copied().chain(["gerberprecision"]) {
            let Some(kind) = option_kind(token) else { continue };
            let value = match kind {
                OptionKind::Bool => {
                    let on = self.flag(token).or_else(|| defaults.flag(token)).unwrap_or(false);
                    (if on { "yes" } else { "no" }).to_string()
                }
                OptionKind::Int(..) => self.int(token).or_else(|| defaults.int(token)).unwrap_or(if token == "gerberprecision" { 6 } else { 0 }).to_string(),
                OptionKind::Text => self.output_directory(),
            };
            out.insert(token.to_string(), value);
        }
        out
    }
}

/// What a token of `PCB_PLOT_PARAMS::Format` holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionKind {
    /// `KICAD_FORMAT::FormatBool`.
    Bool,
    /// `parseInt( min, max )`.
    Int(i64, i64),
    /// `outputdirectory`.
    Text,
}

/// The kind of each token the dialogs store ([`PlotParams::set_option`]); `None` for the layer sets, the dash ratios and anything `Format`
/// does not write. The ranges are the parser's: `gerberprecision` `gbrDefaultPrecision - 1 .. gbrDefaultPrecision`, `svgprecision`
/// `SVG_PRECISION_MIN .. SVG_PRECISION_MAX`, `pngdpi` `MIN_PNG_DPI .. MAX_PNG_DPI`, `outputformat` `PLOT_FORMAT::FIRST_FORMAT .. LAST_FORMAT`
/// (HPGL 0, Gerber 1, PostScript 2, DXF 3, PDF 4, SVG 5, PNG 6), `drillshape` 0..2 (`DRILL_MARKS`), `scaleselection` 0..4, `mode` 1..2.
pub fn option_kind(key: &str) -> Option<OptionKind> {
    Some(match key {
        "gerberprecision" => OptionKind::Int(5, 6),
        "svgprecision" => OptionKind::Int(3, 6),
        "pngdpi" => OptionKind::Int(72, 2400),
        "outputformat" => OptionKind::Int(0, 6),
        "drillshape" => OptionKind::Int(0, 2),
        "scaleselection" => OptionKind::Int(0, 4),
        "mode" => OptionKind::Int(1, 2),
        "outputdirectory" => OptionKind::Text,
        "dashed_line_dash_ratio" | "dashed_line_gap_ratio" => return None,
        k if FORMAT_TOKENS.contains(&k) => OptionKind::Bool,
        _ => return None,
    })
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

    /// The Plot dialog's controls stored back (`DIALOG_PLOT::applyPlotSettings`): each in `Format`'s place, the numbers clamped as the parser
    /// clamps them, the folder quoted as `Quotew` quotes it and read back.
    #[test]
    fn dialog_options_are_stored_in_formats_place_and_read_back() {
        let mut p = PlotParams::kicad_default();
        p.set_option("drillshape", "2").unwrap();
        assert_eq!(p.int("drillshape"), Some(2));
        p.set_option("scaleselection", "9").unwrap();
        assert_eq!(p.item("scaleselection"), Some("(scaleselection 4)"), "clamped to 0..4");
        p.set_option("outputformat", "4").unwrap();
        assert_eq!(p.int("outputformat"), Some(4));
        p.set_option("plotframeref", "yes").unwrap();
        assert_eq!(p.flag("plotframeref"), Some(true));
        assert!(p.set_option("plotframeref", "maybe").is_err());
        assert!(p.set_option("layerselection", "0x0").is_err(), "the layer sets are their own fields");
        assert!(p.set_option("dashed_line_dash_ratio", "3").is_err(), "the dash ratios are Formatting's");
        assert!(p.set_option("viasonmask", "yes").is_err(), "a token Format no longer writes");
        p.set_option("outputdirectory", "plots\\gerber \"a\"/").unwrap();
        assert_eq!(p.item("outputdirectory"), Some("(outputdirectory \"plots/gerber \\\"a\\\"/\")"));
        assert_eq!(p.output_directory(), "plots/gerber \"a\"/");
        assert_eq!(PlotParams { items: vec!["(outputdirectory plots/)".into()], ..Default::default() }.output_directory(), "plots/", "a bare symbol too");
        // The default precision is no item; 5 is written.
        p.set_option("gerberprecision", "5").unwrap();
        assert_eq!(p.item("gerberprecision"), Some("(gerberprecision 5)"));
        let at = |p: &PlotParams, t: &str| p.items.iter().position(|i| i.starts_with(&format!("({t} "))).unwrap();
        assert!(at(&p, "creategerberjobfile") < at(&p, "gerberprecision") && at(&p, "gerberprecision") < at(&p, "dashed_line_dash_ratio"), "in Format's place: {:?}", p.items);
        p.set_option("gerberprecision", "6").unwrap();
        assert!(p.item("gerberprecision").is_none());
        // A block that lacks an item (an older board) gets it in its place, not at the end.
        let mut old = PlotParams { items: vec!["(usegerberextensions no)".into(), "(outputdirectory \"\")".into()], ..Default::default() };
        old.set_option("mirror", "yes").unwrap();
        assert_eq!(old.items, vec!["(usegerberextensions no)", "(mirror yes)", "(outputdirectory \"\")"]);
        // What the dialog starts from: the block's own, the defaults for what it lacks.
        let v = old.dialog_values();
        assert_eq!((v["mirror"].as_str(), v["usegerberextensions"].as_str(), v["drillshape"].as_str(), v["gerberprecision"].as_str(), v["sketchdnponfab"].as_str()), ("yes", "no", "1", "6", "yes"));
        assert!(!v.contains_key("dashed_line_dash_ratio") && v.contains_key("useauxorigin"));
    }
}

/// Export IPC-2581's BOM columns as the dialog keeps them in the project (`PROJECT_FILE::m_IP2581Bom`, `board.ipc2581.*` of the `.kicad_pro`:
/// `id`, `mpn`, `mfg`, `distpn`, `dist`, `bom_rev`; `DIALOG_EXPORT_2581::TransferDataFromWindow`). Each column is a footprint field's name, empty
/// for the choice's first item ("Generate unique", "Omit", "N/A"). Stored in `design.json` ([`crate::ir::DrawingsSection::ipc2581_bom`]) as
/// every project setting of the board is; KiCad's `schRevision` (the root schematic's title block revision, which `eeschema_config.cpp`
/// copies into the project) is not stored: it is read from the schematic when the dialog opens.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ipc2581Bom {
    /// "Internal ID:" (`id`): empty is "Generate unique".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// "Manufacturer P/N:" (`mpn`): empty is "Omit".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mpn: String,
    /// "Manufacturer:" (`mfg`): empty is "N/A".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mfg: String,
    /// "Distributor P/N:" (`distpn`): empty is "Omit".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dist_pn: String,
    /// "Distributor:" (`dist`), free text: empty is "N/A".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dist: String,
    /// "BOM revision:" (`bom_rev`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bom_rev: String,
}
