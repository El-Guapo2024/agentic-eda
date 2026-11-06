//! What a library footprint says of itself beyond its items: Footprint Properties' Clearance Overrides tab (the footprint's clearance, solder mask and paste
//! margins and paste ratio), its Private Layers, its Net Ties (`net_tie_pad_groups`) and its jumper pad groups (UI-LEFT F4).
//!
//! KiCad keeps these on the `FOOTPRINT` (`SetLocalClearance`, `SetLocalSolderMaskMargin`, `SetLocalSolderPasteMargin`, `SetLocalSolderPasteMarginRatio`,
//! `SetPrivateLayers`, `AddNetTiePadGroup`, `SetDuplicatePadNumbersAreJumpers`, `JumperPadGroups()`), edits them in `DIALOG_FOOTPRINT_PROPERTIES_FP_EDITOR`
//! (pcbnew/dialogs/dialog_footprint_properties_fp_editor.cpp at KiCad 8303b2ad: `TransferDataToWindow`, `TransferDataFromWindow`) and writes them in
//! `PCB_IO_KICAD_SEXPR::format( const FOOTPRINT* )`: the margins, the ratio and the clearance before `(zone_connect ..)` and `(attr ..)`, the private layers, the
//! net-tie groups and the jumper settings after. Until UI-LEFT F4 the library reader kept them as the text of their forms
//! ([`crate::lib_keep::LibraryFootprintKeep::tokens`]); a `design.json` written before still has them there, and [`FootprintSettings::absorb_kept`] takes them
//! out of the tokens into these fields (the writer writes both, so such a footprint is written as it was until it is edited).

use serde::{Deserialize, Serialize};

use crate::ir::Nm;

fn is_false(b: &bool) -> bool {
    !*b
}

/// See the module doc.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FootprintSettings {
    /// `(clearance ..)`, nm (`FOOTPRINT::GetLocalClearance`): the copper clearance of every pad of the footprint that sets none of its own. `None` inherits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::units::de")]
    pub clearance: Option<Nm>,
    /// `(solder_mask_margin ..)`, nm (`GetLocalSolderMaskMargin`). `None` inherits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::units::de")]
    pub solder_mask_margin: Option<Nm>,
    /// `(solder_paste_margin ..)`, nm (`GetLocalSolderPasteMargin`). `None` inherits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::units::de")]
    pub solder_paste_margin: Option<Nm>,
    /// `(solder_paste_margin_ratio ..)` (`GetLocalSolderPasteMarginRatio`): a fraction of the pad's size, -0.05 is 5% smaller. `None` inherits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solder_paste_margin_ratio: Option<f64>,
    /// `(private_layers "User.1" ..)` (`GetPrivateLayers`): the user layers whose items only the Footprint Editor shows; canonical names, in `LSET::Seq()` order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub private_layers: Vec<String>,
    /// `(net_tie_pad_groups "1, 2" ..)` (`GetNetTiePadGroups`): each group the pad numbers a net tie lets short, as the dialog's grid row has them. A footprint with
    /// any is a net tie (`FOOTPRINT::IsNetTie`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub net_tie_pad_groups: Vec<String>,
    /// `(duplicate_pad_numbers_are_jumpers yes)` (`GetDuplicatePadNumbersAreJumpers`, the Pad Connections' "Pads with duplicate numbers are jumpers").
    #[serde(default, skip_serializing_if = "is_false")]
    pub duplicate_pad_numbers_are_jumpers: bool,
    /// `(jumper_pad_groups ("1" "2") ..)` (`JumperPadGroups()`): each group's pad numbers, sorted as KiCad's `std::set<wxString>` keeps them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jumper_pad_groups: Vec<Vec<String>>,
}

/// The tags of the forms [`FootprintSettings`] holds, as a file writes them (`solder_paste_ratio` is the older name of the ratio).
pub const SETTINGS_TAGS: &[&str] = &[
    "clearance",
    "solder_mask_margin",
    "solder_paste_margin",
    "solder_paste_margin_ratio",
    "solder_paste_ratio",
    "private_layers",
    "net_tie_pad_groups",
    "duplicate_pad_numbers_are_jumpers",
    "jumper_pad_groups",
];

impl FootprintSettings {
    pub fn is_empty(&self) -> bool {
        *self == FootprintSettings::default()
    }

    /// `FOOTPRINT::IsNetTie`: the footprint has a pad group a net tie lets short.
    pub fn is_net_tie(&self) -> bool {
        self.net_tie_pad_groups.iter().any(|g| !g.trim().is_empty())
    }

    /// Take the forms of [`SETTINGS_TAGS`] out of `tokens` (a footprint's kept forms, as an older reader left them) into these fields; a form that does not
    /// read (a number that is not one) is left in `tokens`. A field already set is not overwritten.
    pub fn absorb_kept(&mut self, tokens: &mut Vec<String>) {
        let mut rest = Vec::with_capacity(tokens.len());
        for t in tokens.drain(..) {
            if !self.absorb_one(&t) {
                rest.push(t);
            }
        }
        *tokens = rest;
    }

    fn absorb_one(&mut self, token: &str) -> bool {
        let Some(inner) = token.trim().strip_prefix('(').and_then(|s| s.strip_suffix(')')) else { return false };
        let (tag, body) = inner.split_once(char::is_whitespace).unwrap_or((inner, ""));
        let body = body.trim();
        let number = || body.parse::<f64>().ok();
        match tag {
            "clearance" | "solder_mask_margin" | "solder_paste_margin" => {
                let Some(v) = number() else { return false };
                let slot = match tag {
                    "clearance" => &mut self.clearance,
                    "solder_mask_margin" => &mut self.solder_mask_margin,
                    _ => &mut self.solder_paste_margin,
                };
                slot.get_or_insert(Nm::from_mm(v));
                true
            }
            "solder_paste_margin_ratio" | "solder_paste_ratio" => {
                let Some(v) = number() else { return false };
                self.solder_paste_margin_ratio.get_or_insert(v);
                true
            }
            "private_layers" => {
                if self.private_layers.is_empty() {
                    self.private_layers = quoted_words(body);
                }
                true
            }
            "net_tie_pad_groups" => {
                if self.net_tie_pad_groups.is_empty() {
                    self.net_tie_pad_groups = quoted_words(body);
                }
                true
            }
            "duplicate_pad_numbers_are_jumpers" => {
                self.duplicate_pad_numbers_are_jumpers |= body != "no";
                true
            }
            "jumper_pad_groups" => {
                if self.jumper_pad_groups.is_empty() {
                    // `( "1" "2" ) ( "3" "4" )`: one list per group
                    let mut groups = Vec::new();
                    let mut rest = body;
                    while let Some(open) = rest.find('(') {
                        let Some(close) = rest[open..].find(')') else { break };
                        groups.push(sorted_set(quoted_words(&rest[open + 1..open + close])));
                        rest = &rest[open + close + 1..];
                    }
                    self.jumper_pad_groups = groups;
                }
                true
            }
            _ => false,
        }
    }

    /// `DIALOG_FOOTPRINT_PROPERTIES_FP_EDITOR::TransferDataFromWindow`'s jumper groups: each grid row is cut at commas, spaces, tabs and line ends (`wxStringTokenizer(
    /// .., ", \t\r\n", wxTOKEN_STRTOK )`), every number must be a pad of the footprint ("Pad '%s' in jumper pad group %d does not exist in this footprint.") and the group
    /// is a `std::set` (sorted, no repeats). A row with no number is an empty group, as KiCad's `emplace_back` leaves it.
    pub fn jumper_groups_of_rows(rows: &[String], pad_numbers: &std::collections::BTreeSet<String>) -> Result<Vec<Vec<String>>, String> {
        let mut out = Vec::new();
        for (ii, row) in rows.iter().enumerate() {
            let mut group = Vec::new();
            for token in row.split([',', ' ', '\t', '\r', '\n']).filter(|t| !t.is_empty()) {
                if !pad_numbers.contains(token) {
                    return Err(format!("Pad '{token}' in jumper pad group {} does not exist in this footprint.", ii + 1));
                }
                group.push(token.to_string());
            }
            out.push(sorted_set(group));
        }
        Ok(out)
    }

    /// `TransferDataToWindow`'s jumper rows: each group's numbers joined with ", ".
    pub fn jumper_rows(&self) -> Vec<String> {
        self.jumper_pad_groups.iter().map(|g| g.join(", ")).collect()
    }
}

/// `std::set<wxString>`: sorted (by `wxString::operator<`, a code-point compare), no repeats.
fn sorted_set(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

/// The quoted strings (and bare words) of a form's body, unescaped as KiCad's lexer reads them.
fn quoted_words(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = body.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut s = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        if let Some(n) = chars.next() {
                            s.push(n);
                        }
                    }
                    '"' => break,
                    _ => s.push(c),
                }
            }
            out.push(s);
        } else {
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                s.push(c);
                chars.next();
            }
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kept_forms_of_an_older_design_are_taken_into_the_settings() {
        let mut tokens: Vec<String> = [
            "(solder_mask_margin 0.05)",
            "(solder_paste_ratio -0.1)",
            "(clearance 0.2)",
            "(net_tie_pad_groups \"1, 2\" \"3,4\")",
            "(private_layers \"User.1\" \"User.2\")",
            "(duplicate_pad_numbers_are_jumpers yes)",
            "(jumper_pad_groups (\"2\" \"1\") (\"3\" \"4\"))",
            "(stackup (layer \"F.Cu\"))",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        let mut s = FootprintSettings::default();
        s.absorb_kept(&mut tokens);
        assert_eq!(tokens, vec!["(stackup (layer \"F.Cu\"))".to_string()], "what the settings do not hold stays kept");
        assert_eq!(s.solder_mask_margin, Some(Nm::new(50_000)));
        assert_eq!(s.solder_paste_margin_ratio, Some(-0.1));
        assert_eq!(s.clearance, Some(Nm::new(200_000)));
        assert_eq!(s.net_tie_pad_groups, vec!["1, 2".to_string(), "3,4".to_string()]);
        assert!(s.is_net_tie());
        assert_eq!(s.private_layers, vec!["User.1".to_string(), "User.2".to_string()]);
        assert!(s.duplicate_pad_numbers_are_jumpers);
        assert_eq!(s.jumper_pad_groups, vec![vec!["1".to_string(), "2".to_string()], vec!["3".to_string(), "4".to_string()]]);
    }

    #[test]
    fn jumper_rows_are_cut_and_checked_as_the_dialog_does() {
        let pads: std::collections::BTreeSet<String> = ["1", "2", "3"].into_iter().map(str::to_string).collect();
        let groups = FootprintSettings::jumper_groups_of_rows(&["2, 1".to_string(), "3\t3".to_string()], &pads).unwrap();
        assert_eq!(groups, vec![vec!["1".to_string(), "2".to_string()], vec!["3".to_string()]]);
        let err = FootprintSettings::jumper_groups_of_rows(&["1 2".to_string(), "1, 9".to_string()], &pads).unwrap_err();
        assert_eq!(err, "Pad '9' in jumper pad group 2 does not exist in this footprint.");
        let s = FootprintSettings { jumper_pad_groups: groups, ..Default::default() };
        assert_eq!(s.jumper_rows(), vec!["1, 2".to_string(), "3".to_string()]);
    }
}
