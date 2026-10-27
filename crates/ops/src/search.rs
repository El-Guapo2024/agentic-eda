//! Schematic Find / Find and Replace: the matching + replacing core of
//! eeschema's `SCH_FIND_REPLACE_TOOL` (`eeschema/tools/sch_find_replace_tool.cpp`)
//! over `EDA_ITEM::Matches` / `EDA_ITEM::Replace` (`common/eda_item.cpp`) and
//! the per-item overrides (`SCH_FIELD::Matches`/`Replace`,
//! `SCH_SYMBOL::Matches`, `SCH_LABEL_BASE::Matches`, `SCH_PIN::Matches`).
//!
//! What is ported:
//!  * the text matcher with `EDA_SEARCH_MATCH_MODE::PLAIN`, `WHOLEWORD`
//!    (word chars = alphanumeric or `_`) and `WILDCARD`
//!    (`wxString::Matches`: `*`, `?`, whole-string), honouring
//!    `matchCase` ([`matches_text`]);
//!  * the text replacer, including its whole-word handling and its
//!    case-insensitive search that copies the *original* text around
//!    each hit ([`replace_text`]);
//!  * the item walk and ordering of `SCH_FIND_REPLACE_TOOL::nextMatch`:
//!    all searchable items sorted by position (x, then y), then
//!    reversed for Find Previous ([`find_items`]);
//!  * the item rules: hidden fields only when `searchAllFields`; the
//!    Reference field is skipped when replacing unless `replaceReferences`
//!    and also matches when its *symbol* matches (`SCH_FIELD::Matches` calls
//!    `SCH_SYMBOL::Matches`); labels match on their text and, with
//!    `searchNetNames`, on their net name; pins only with `searchAllPins`
//!    (name/number) or `searchNetNames`, and are never replaceable.
//!
//! Scope limits (see `web/studio/PARITY-sch.md`): the regex and permissive
//! match modes are not ported (the eeschema dialog only exposes whole-word /
//! regex checkboxes; regex needs a regex engine this workspace does not
//! carry); our IR has no per-field visibility, so KiCad's template defaults
//! apply (Reference/Value visible; Footprint, Datasheet and user fields
//! hidden); no per-field position, so every field of a symbol sits at the
//! symbol's own position. A label's text and a field's text are matched
//! unescaped (`UnescapeString( GetText() )`), and a label is replaced with the
//! find and replace strings escaped (`EscapeString( .., CTX_NETNAME )`,
//! `SCH_LABEL_BASE::Replace`), r5 C5-M4.

use eda_model::ir::{Point, SchematicSection};
use eda_model::text_vars::unescape_string;
use eda_model::ConstraintModel;
use serde::{Deserialize, Serialize};

/// `EDA_SEARCH_MATCH_MODE` (minus `REGEX`/`PERMISSIVE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    #[default]
    Plain,
    WholeWord,
    Wildcard,
}

/// `EDA_SEARCH_DATA` + `SCH_SEARCH_DATA`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchData {
    #[serde(default)]
    pub find: String,
    #[serde(default)]
    pub replace: String,
    #[serde(default)]
    pub match_case: bool,
    #[serde(default)]
    pub mode: MatchMode,
    /// `searchAllFields`: "Search all fields" / hidden fields.
    #[serde(default)]
    pub search_hidden_fields: bool,
    /// `searchAllPins`.
    #[serde(default)]
    pub search_pins: bool,
    /// `searchNetNames`.
    #[serde(default)]
    pub search_net_names: bool,
    /// `replaceReferences`.
    #[serde(default)]
    pub replace_references: bool,
    /// `searchAndReplace`: makes non-replaceable items (pins, and the
    /// Reference unless `replaceReferences`) not match. KiCad's dialog never
    /// sets it (it is only read from and written to the config,
    /// `eda_draw_frame.cpp`, default false), so the dialog's Find, Find Next
    /// and highlight leave it off; our replace verb sets it so its walk
    /// skips what `SCH_FIELD::Replace` / `SCH_PIN::Replace` would not change.
    #[serde(default)]
    pub search_and_replace: bool,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `wxString::MakeUpper` per character, keeping one output char per input
/// char so indexes stay aligned (multi-char uppercase forms are left as is).
fn upper(s: &str) -> Vec<char> {
    s.chars()
        .map(|c| {
            let mut u = c.to_uppercase();
            match (u.next(), u.next()) {
                (Some(x), None) => x,
                _ => c,
            }
        })
        .collect()
}

/// `wxString::Matches`: `*` any run (including empty), `?` any one
/// character, otherwise literal; the whole string must match.
pub fn wild_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || (p[pi] != '*' && p[pi] == t[ti])) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn find_from(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    if from > hay.len() || needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

/// `EscapeString( aSource, CTX_NETNAME )` (`common/string_utils.cpp`): `/` becomes `{slash}`, line breaks are dropped.
pub fn escape_netname(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '/' => out.push_str("{slash}"),
            '\n' | '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// `SCH_LABEL_BASE::Replace`: the find and replace strings escaped as a net name (`EscapeString( .., CTX_NETNAME )`), then `EDA_TEXT::Replace` on the stored
/// (escaped) text.
pub fn replace_label_text(stored: &str, data: &SearchData) -> Option<String> {
    let local = SearchData { find: escape_netname(&data.find), replace: escape_netname(&data.replace), ..data.clone() };
    replace_text(stored, &local)
}

/// `EDA_ITEM::Matches( aText, aSearchData )` (the replaceable-item guard is
/// the caller's job -- see [`find_items`]).
pub fn matches_text(text: &str, data: &SearchData) -> bool {
    if data.find.is_empty() {
        return false;
    }
    let (t, s): (Vec<char>, Vec<char>) = if data.match_case { (text.chars().collect(), data.find.chars().collect()) } else { (upper(text), upper(&data.find)) };
    match data.mode {
        MatchMode::Wildcard => wild_match(&s.iter().collect::<String>(), &t.iter().collect::<String>()),
        MatchMode::WholeWord => {
            let mut ii = 0usize;
            while ii < t.len() {
                let Some(next) = find_from(&t, &s, ii) else { return false };
                ii = next;
                let end = next + s.len();
                let start_ok = ii == 0 || !is_word_char(t[ii - 1]);
                let end_ok = end == t.len() || !is_word_char(t[end]);
                if start_ok && end_ok {
                    return true;
                }
                ii += 1;
            }
            false
        }
        MatchMode::Plain => find_from(&t, &s, 0).is_some(),
    }
}

/// `EDA_ITEM::Replace( aSearchData, aText )`: returns the new text when at
/// least one occurrence was replaced. Case-insensitive search runs on the
/// upper-cased text but the output copies the original characters between
/// hits, exactly like the source.
pub fn replace_text(text: &str, data: &SearchData) -> Option<String> {
    let orig: Vec<char> = text.chars().collect();
    let (t, s): (Vec<char>, Vec<char>) = if data.match_case { (orig.clone(), data.find.chars().collect()) } else { (upper(text), upper(&data.find)) };
    let mut result = String::new();
    let mut replaced = false;
    let mut ii = 0usize;
    while ii < t.len() {
        let Some(next) = find_from(&t, &s, ii) else {
            result.extend(&orig[ii..]);
            break;
        };
        if next > ii {
            result.extend(&orig[ii..next]);
        }
        ii = next;
        let end = next + s.len();
        let (start_ok, end_ok) = if data.mode == MatchMode::WholeWord { (ii == 0 || !is_word_char(t[ii - 1]), end == t.len() || !is_word_char(t[end])) } else { (true, true) };
        if start_ok && end_ok {
            result.push_str(&data.replace);
            replaced = true;
            ii = end;
        } else {
            result.push(orig[ii]);
            ii += 1;
        }
    }
    replaced.then_some(result)
}

// ---------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// A symbol field (`SCH_FIELD`): `name` is `Reference`/`Value`/...
    Field,
    /// A net label (`SCH_LABEL_BASE`): `id` is the label id.
    Label,
    /// Free text (`SCH_TEXT`): `id` is the text id.
    Text,
    /// A symbol pin (`SCH_PIN`): `name` is the pin number.
    Pin,
    /// A label's own field (`SCH_LABEL_BASE::m_fields`, `SchExtras::label_fields`): `id` is the label id, `name` the field's name.
    LabelField,
}

/// One match: what to select / pan to, and a stable key (`kind:id:name`)
/// the replace verb can be restricted by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hit {
    pub kind: ItemKind,
    pub id: String,
    pub name: String,
    pub at: Point,
    /// The text that matched (for the dialog's status line).
    pub text: String,
}

impl Hit {
    pub fn key(&self) -> String {
        hit_key(self.kind, &self.id, &self.name)
    }
}

pub fn hit_key(kind: ItemKind, id: &str, name: &str) -> String {
    let k = match kind {
        ItemKind::Field => "field",
        ItemKind::Label => "label",
        ItemKind::Text => "text",
        ItemKind::Pin => "pin",
        ItemKind::LabelField => "label_field",
    };
    format!("{k}:{id}:{name}")
}

/// `SCH_FIELD::IsVisible` under KiCad's own template defaults (see the
/// module doc): the two fields every symbol shows by default.
fn field_visible(name: &str) -> bool {
    matches!(name, "Reference" | "Value")
}

/// `(name, text)` of every field of the symbol `id`, in KiCad's field
/// order (Reference, Value, Footprint, Datasheet, then user fields).
fn symbol_fields(sch: &SchematicSection, model: &ConstraintModel, id: &str) -> Vec<(String, String)> {
    let sym = sch.symbols.iter().find(|s| s.id == id).expect("symbol exists");
    let value = if sym.value.is_empty() { model.part(id).and_then(|p| p.value.clone()).unwrap_or_default() } else { sym.value.clone() };
    let mut out = vec![("Reference".to_string(), id.to_string()), ("Value".to_string(), value), ("Footprint".to_string(), sym.footprint.clone()), ("Datasheet".to_string(), sym.datasheet.clone())];
    if let Some(user) = sch.user_fields.get(id) {
        out.extend(user.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    out
}

/// `SCH_SYMBOL::Matches`: any non-Reference field matches (hidden ones only
/// with `searchAllFields`; a field's text unescaped, as `SCH_FIELD::Matches`
/// reads it), then -- "for completeness" -- any non-field draw item of the
/// library symbol (`GetLibSymbolRef()->GetDrawItems()`, every unit and body
/// style): a body text (`SCH_TEXT::Matches` on its text) or, with
/// `searchAllPins`, a pin's name or number (`SCH_PIN::Matches`; a library
/// pin has no connection, so `searchNetNames` adds nothing), r5 C5-L1.
/// `searchMetadata` (library name/description/keywords) is not exposed by
/// the schematic dialog and not ported.
fn symbol_matches(sch: &SchematicSection, model: &ConstraintModel, id: &str, data: &SearchData) -> bool {
    if symbol_fields(sch, model, id).into_iter().filter(|(n, _)| n != "Reference").any(|(n, text)| (field_visible(&n) || data.search_hidden_fields) && matches_text(&unescape_string(&text), data)) {
        return true;
    }
    let Some(sym) = sch.symbols.iter().find(|s| s.id == id) else { return false };
    let Some(lib) = model.part(id).and_then(|part| model.real_symbol_of(&sym.lib_id, part)) else { return false };
    let body_text = lib.graphics.iter().any(|g| matches!(g, eda_model::symbol::SymbolGraphic::Text { text, .. } if matches_text(text, data)));
    body_text || (data.search_pins && lib.pins.iter().any(|p| matches_text(&p.name, data) || matches_text(&p.number, data)))
}

/// Unit letter suffix (`SCH_SYMBOL::SubReference`): 1 -> "A".
fn unit_letter(unit: u32) -> String {
    char::from_u32('A' as u32 + unit.saturating_sub(1)).map(|c| c.to_string()).unwrap_or_default()
}

fn net_of_pin<'a>(model: &'a ConstraintModel, pin_ref: &str) -> Option<&'a str> {
    model.nets.iter().find(|n| n.pins.iter().any(|p| p == pin_ref)).map(|n| n.name.as_str())
}

/// All matching items, in `SCH_FIND_REPLACE_TOOL::nextMatch` order
/// (ascending x, then y; stable for ties). Reverse the result for Find
/// Previous.
pub fn find_items(sch: &SchematicSection, model: &ConstraintModel, data: &SearchData) -> Vec<Hit> {
    let mut hits: Vec<Hit> = Vec::new();
    if data.find.is_empty() {
        return hits;
    }
    let mut seen_ids: Vec<&str> = Vec::new();
    for sym in &sch.symbols {
        // Fields are part-wide: one set of items per reference, at the first
        // placed unit's position.
        if seen_ids.contains(&sym.id.as_str()) {
            continue;
        }
        seen_ids.push(&sym.id);
        let multi_unit = sch.symbols.iter().filter(|s| s.id == sym.id).count() > 1;
        for (name, text) in symbol_fields(sch, model, &sym.id) {
            if !field_visible(&name) && !data.search_hidden_fields {
                continue;
            }
            let matched = if name == "Reference" {
                // SCH_FIELD::Matches: skipped when replacing unless
                // replaceReferences; otherwise matches if the parent symbol
                // matches, or the reference (with its unit letter) does.
                if data.search_and_replace && !data.replace_references {
                    false
                } else {
                    symbol_matches(sch, model, &sym.id, data) || matches_text(&text, data) || (multi_unit && matches_text(&format!("{}{}", text, unit_letter(sym.unit)), data))
                }
            } else {
                matches_text(&unescape_string(&text), data)
            };
            if matched {
                hits.push(Hit { kind: ItemKind::Field, id: sym.id.clone(), name, at: sym.at, text });
            }
        }
        // Pins (never replaceable -> never match in replace mode).
        if !data.search_and_replace && (data.search_pins || data.search_net_names) {
            if let Some(part) = model.part(&sym.id) {
                for pin in &part.pins {
                    let pin_ref = format!("{}.{}", sym.id, pin.number);
                    let pin_name = pin.name.clone().unwrap_or_default();
                    let by_text = data.search_pins && (matches_text(&pin_name, data) || matches_text(&pin.number, data));
                    let by_net = data.search_net_names && net_of_pin(model, &pin_ref).is_some_and(|n| matches_text(n, data));
                    if by_text || by_net {
                        hits.push(Hit { kind: ItemKind::Pin, id: sym.id.clone(), name: pin.number.clone(), at: sym.at, text: pin_name });
                    }
                }
            }
        }
    }
    for l in &sch.labels {
        // SCH_LABEL_BASE::Matches: its text unescaped (`UnescapeString( GetText() )`: `{slash}` is `/`), or (searchNetNames) its net
        // name -- which for our labels is the same string, so the text test already covers both.
        let shown = unescape_string(&l.net);
        if matches_text(&shown, data) {
            hits.push(Hit { kind: ItemKind::Label, id: l.id.clone(), name: String::new(), at: l.at, text: shown });
        }
        // `nextMatch`'s `addItem`: a label's fields are items of their own (`for( SCH_FIELD& field : label->GetFields() )`), each matched by
        // `SCH_FIELD::Matches` -- a hidden one only with `searchAllFields`, its text unescaped (r5 C5-L2). They sit at the label here (no field
        // position is kept for the sort).
        for f in sch.extras.label_fields.get(&l.id).into_iter().flatten() {
            if !f.visible && !data.search_hidden_fields {
                continue;
            }
            let text = unescape_string(&f.text);
            if matches_text(&text, data) {
                hits.push(Hit { kind: ItemKind::LabelField, id: l.id.clone(), name: f.name.clone(), at: l.at, text });
            }
        }
    }
    for t in &sch.texts {
        if matches_text(&t.content, data) {
            hits.push(Hit { kind: ItemKind::Text, id: t.id.clone(), name: String::new(), at: t.at, text: t.content.clone() });
        }
    }
    hits.sort_by(|a, b| (a.at.x, a.at.y).cmp(&(b.at.x, b.at.y)));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(find: &str) -> SearchData {
        SearchData { find: find.into(), ..Default::default() }
    }

    #[test]
    fn plain_match_is_case_insensitive_by_default() {
        assert!(matches_text("Resistor 10k", &d("RESISTOR")));
        let mut s = d("RESISTOR");
        s.match_case = true;
        assert!(!matches_text("Resistor 10k", &s));
        assert!(matches_text("RESISTOR", &s));
    }

    #[test]
    fn whole_word_respects_word_boundaries() {
        let mut s = d("R1");
        s.mode = MatchMode::WholeWord;
        assert!(matches_text("R1", &s));
        assert!(matches_text("net R1 here", &s));
        assert!(!matches_text("R10", &s), "R10 contains R1 but not as a whole word");
        assert!(!matches_text("XR1", &s));
        assert!(matches_text("R1_x R1", &s), "second occurrence is a whole word");
        assert!(!matches_text("R1_x", &s), "underscore is a word char");
    }

    #[test]
    fn wildcard_matches_whole_string_with_star_and_question() {
        let mut s = d("R*");
        s.mode = MatchMode::Wildcard;
        assert!(matches_text("R10", &s));
        assert!(!matches_text("C10", &s));
        s.find = "?1".into();
        assert!(matches_text("R1", &s));
        assert!(!matches_text("R10", &s), "wxString::Matches anchors the whole string");
        s.find = "*k*".into();
        assert!(matches_text("10k 1%", &s));
        s.find = "*".into();
        assert!(matches_text("anything", &s));
    }

    #[test]
    fn replace_plain_keeps_original_case_around_hits() {
        let mut s = d("res");
        s.replace = "R".into();
        assert_eq!(replace_text("Resistor RES", &s).as_deref(), Some("Ristor R"));
        s.match_case = true;
        assert_eq!(replace_text("Resistor RES", &s), None, "lowercase 'res' occurs nowhere case-sensitively");
        s.find = "Res".into();
        assert_eq!(replace_text("Resistor RES", &s).as_deref(), Some("Ristor RES"));
        assert_eq!(replace_text("none", &d("zzz")), None);
    }

    #[test]
    fn replace_whole_word_only_touches_whole_words() {
        let mut s = d("R1");
        s.mode = MatchMode::WholeWord;
        s.replace = "R9".into();
        assert_eq!(replace_text("R1 R10 R1", &s).as_deref(), Some("R9 R10 R9"));
        assert_eq!(replace_text("R10", &s), None);
    }

    #[test]
    fn wild_match_basics() {
        assert!(wild_match("a*c", "abbbc"));
        assert!(wild_match("a*c", "ac"));
        assert!(!wild_match("a*c", "acd"));
        assert!(wild_match("*a*b*", "xxaxxbxx"));
        assert!(!wild_match("a?c", "ac"));
    }

    use eda_model::ir::{LabelKind, NetLabel, SymbolInstance};
    use eda_model::sch_extras::LabelField;
    use eda_model::units::mm;

    fn label(id: &str, net: &str, x: f64) -> NetLabel {
        NetLabel { id: id.into(), net: net.into(), at: Point { x: mm(x), y: mm(0.0) }, kind: LabelKind::Local }
    }

    #[test]
    fn a_label_matches_on_its_unescaped_text() {
        // SCH_LABEL_BASE::Matches: `SCH_ITEM::Matches( UnescapeString( GetText() ), aSearchData )`
        let sch = SchematicSection { labels: vec![label("l1", "SDA{slash}SCL", 1.0)], ..Default::default() };
        let m = ConstraintModel::default();
        let hits = find_items(&sch, &m, &d("SDA/SCL"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "SDA/SCL");
        assert!(find_items(&sch, &m, &d("slash")).is_empty(), "the escape is not text");
        // SCH_LABEL_BASE::Replace: both strings escaped as a net name, then replaced on the stored text
        let mut r = d("/");
        r.replace = "_".into();
        assert_eq!(replace_label_text("SDA{slash}SCL", &r).as_deref(), Some("SDA_SCL"));
        let mut r = d("_");
        r.replace = "/".into();
        assert_eq!(replace_label_text("A_B", &r).as_deref(), Some("A{slash}B"));
    }

    #[test]
    fn a_labels_shown_field_is_found() {
        let mut sch = SchematicSection { labels: vec![label("l1", "VBUS", 1.0)], ..Default::default() };
        sch.extras.label_fields.insert("l1".into(), vec![LabelField { name: "Netclass".into(), text: "HV".into(), visible: true, italic: false }, LabelField { name: "Note".into(), text: "HVX".into(), visible: false, italic: false }]);
        let m = ConstraintModel::default();
        let hits = find_items(&sch, &m, &d("HV"));
        assert_eq!(hits.iter().map(|h| (h.kind, h.name.as_str())).collect::<Vec<_>>(), vec![(ItemKind::LabelField, "Netclass")], "the hidden field only with searchAllFields");
        let mut all = d("HV");
        all.search_hidden_fields = true;
        assert_eq!(find_items(&sch, &m, &all).len(), 2);
        assert_eq!(hits[0].key(), "label_field:l1:Netclass");
    }

    #[test]
    fn a_symbols_body_text_makes_its_reference_match() {
        // SCH_SYMBOL::Matches: "Search non-field lib draw items (pins, graphical text) for completeness."
        let mut lib = eda_model::symbol::builtin("Device:R").unwrap();
        lib.lib_id = "T:OPA".into();
        lib.graphics.push(eda_model::symbol::SymbolGraphic::Text { unit: 0, text: "OPA".into(), at: eda_model::symbol::SPoint { x: 0.0, y: 0.0 }, angle_deg: 0.0, size_mm: 1.27 });
        let pin_name = lib.pins[0].number.clone();
        let mut m = ConstraintModel::default();
        m.symbols.push(lib);
        m.parts.push(serde_json::from_value(serde_json::json!({ "reference": "U1" })).unwrap());
        let sym = SymbolInstance { id: "U1".into(), at: Point { x: mm(1.0), y: mm(0.0) }, rot: 0, mirrored: false, mirror_y: false, lib_id: "T:OPA".into(), unit: 1, value: "x".into(), footprint: String::new(), datasheet: String::new(), dnp: false, exclude_from_bom: false, exclude_from_board: false, exclude_from_sim: false };
        let sch = SchematicSection { symbols: vec![sym], ..Default::default() };
        let hits = find_items(&sch, &m, &d("OPA"));
        assert_eq!(hits.iter().map(|h| (h.kind, h.name.as_str())).collect::<Vec<_>>(), vec![(ItemKind::Field, "Reference")]);
        // a library pin's number, only with searchAllPins
        let mut whole = d(&pin_name);
        whole.mode = MatchMode::WholeWord;
        assert!(find_items(&sch, &m, &whole).iter().all(|h| h.name != "Reference"));
        whole.search_pins = true;
        assert!(find_items(&sch, &m, &whole).iter().any(|h| h.name == "Reference"));
    }
}
