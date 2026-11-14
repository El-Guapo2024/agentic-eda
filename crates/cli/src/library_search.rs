//! The installed KiCad libraries as the Symbol and Footprint Choosers list and search them
//! (`GET /api/library/entries|search|details`).
//!
//! `library_index.rs` lists library and item NAMES for the editors' trees. KiCad's choosers (`SYMBOL_CHOOSER_FRAME`,
//! `FOOTPRINT_CHOOSER_FRAME` over `LIB_TREE`) search more than names: the description, the keywords, the library nickname, the
//! default footprint -- each with a weight (`LIB_SYMBOL::cacheSearchTerms`, `FOOTPRINT_INFO::GetSearchTerms`) -- and show the
//! matches as a tree that is sorted best match first (`LIB_TREE_NODE::Compare`). The libraries are large (220 MB of symbols,
//! 179 MB of footprints), so:
//!
//!   * a library is indexed on demand, once ([`symbol_lib`], [`footprint_lib`]): one pass over its text
//!     (`eda_kicad::scan_symbols`, `eda_kicad::scan_footprint`) that keeps a few fields per item and the byte span of the item's
//!     form -- never its drawing -- and the index is cached against the library's modification time;
//!   * searching every library needs every index, so the first search starts a thread that indexes them all, library by library
//!     (the request loop never waits for it). Until it is done a search answers with what is indexed so far and says
//!     `"status": "indexing"` with the progress; the page asks again;
//!   * only the best matches leave the server (a cap of items, best first), grouped by library; a library's items are sent when
//!     it is expanded, as names and descriptions; one symbol's or footprint's drawing is sent when it is selected
//!     ([`details`]) -- never a whole library.
//!
//! The scoring is `EDA_COMBINED_MATCHER::ScoreTerms` and `LIB_TREE_NODE_ITEM::UpdateScore` (`common/eda_pattern_match.cpp`,
//! `common/lib_tree_model.cpp`): the query is split on blanks, every word must score against the item's terms (an exact term is
//! worth 8 times its weight, a match at the start of a term 2 times, a match anywhere once), and an item's score is 1 plus the sum.
//! A word is matched by the four matchers `EDA_COMBINED_MATCHER` builds in `CTX_LIBITEM`, each only when the word suits it ([`Matcher`]): a regular
//! expression (`/pattern/`, or `^...$`, on `eda_model::wx_regex`), the `*` / `?` wildcard form, the relational one (`voltage>3.3`, `resistance<10k`)
//! and the plain text.

use crate::library_index::{legal_library_name, libraries_in, modified, Kind, LibSource};
use eda_kicad::{FootprintSummary, SymbolSummary};
use eda_model::ir::{LibraryFootprint, LibrarySymbol};
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

// ------------------------------------------------------------------------------------------------ matching

/// One searchable text of an item with its weight (`SEARCH_TERM`), stored the way `ScoreTerms` normalizes it: lower case, trimmed,
/// at most 1000 characters.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Term {
    pub(crate) text: String,
    pub(crate) weight: u32,
}

pub(crate) fn term(text: &str, weight: u32) -> Term {
    let mut text = text.trim().to_lowercase();
    if text.chars().count() > 1000 {
        text = text.chars().take(1000).collect();
    }
    Term { text, weight }
}

/// One word of the query (`EDA_COMBINED_MATCHER` in `CTX_LIBITEM`, common/eda_pattern_match.cpp): the lower-case word (`UpdateSearchString` lowers every
/// word before it builds the matcher) and the matchers whose `SetPattern` took it, in KiCad's order -- `EDA_PATTERN_MATCH_REGEX`, `EDA_PATTERN_MATCH_WILDCARD`,
/// `EDA_PATTERN_MATCH_RELATIONAL`, `EDA_PATTERN_MATCH_SUBSTR` ("If any of the above matchers couldn't be created because the pattern syntax does not match,
/// the substring will try its best", and it always takes the word). [`Matcher::find`] is `EDA_COMBINED_MATCHER::Find`: the earliest place any of them finds.
///
/// The wildcard matcher compiles every word to a regular expression (each regex character escaped, `?` as `.`, `*` as `.*`), so for a word with neither
/// it finds what the substring finds, where the substring does: it is kept here only for a word that has one, as the leftmost match of its pieces.
#[derive(Debug, Clone)]
pub(crate) struct Matcher {
    pattern: String,
    /// The pieces between `*`, each a run of characters where `None` is `?`. Empty for a plain word.
    segments: Vec<Vec<Option<char>>>,
    /// Starts with `*`: the regex `.*x` matches from the first character on.
    leading_star: bool,
    /// `EDA_PATTERN_MATCH_REGEX`: a word written `/re/` (the closing `/` may be left off: "Requiring a '/' on the end means they get no feedback while
    /// they type") or `^re$`, compiled as `wxRegEx::Compile( .., wxRE_ADVANCED )` is -- `Flavor::Advanced`, wxWidgets 3.2's PCRE2 (UI-LEFT-2 LL4) -- and found anywhere in a term. `None` when the
    /// word is not written as one, or does not compile.
    regex: Option<eda_model::wx_regex::Regex>,
    /// `EDA_PATTERN_MATCH_RELATIONAL`: a word `key<op>value[unit]`.
    relational: Option<Relational>,
}

/// `EDA_PATTERN_MATCH_RELATIONAL`'s relations; `Any` is a word whose value is not typed yet (`voltage>`: "Matching on empty values keeps the match list
/// from going empty when the user types the relational operator character").
#[derive(Debug, Clone, Copy, PartialEq)]
enum Relation {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
    Any,
}

/// `EDA_PATTERN_MATCH_RELATIONAL`: the key, the relation and the value (its unit's multiplier applied).
#[derive(Debug, Clone)]
struct Relational {
    key: String,
    relation: Relation,
    value: f64,
}

/// `EDA_PATTERN_MATCH_RELATIONAL::m_units`, by the lower-case unit.
fn unit_multiplier(unit: &str) -> Option<f64> {
    Some(match unit {
        "p" => 1e-12,
        "n" => 1e-9,
        "u" => 1e-6,
        "m" => 1e-3,
        "" => 1.,
        "k" => 1e3,
        "meg" => 1e6,
        "g" => 1e9,
        "t" => 1e12,
        "ki" => 1024.,
        "mi" => 1_048_576.,
        "gi" => 1_073_741_824.,
        "ti" => 1_099_511_627_776.,
        _ => return None,
    })
}

/// `wxString::ToCDouble`: the whole text a number (the value's characters are `[-+]?[\d.]+` already).
fn to_c_double(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn relational_regexes() -> &'static (eda_model::wx_regex::Regex, eda_model::wx_regex::Regex) {
    use eda_model::wx_regex::{Flavor, Regex};
    static R: OnceLock<(Regex, Regex)> = OnceLock::new();
    R.get_or_init(|| {
        (
            // `wxRegEx( .., wxRE_ADVANCED )`: PCRE2's ASCII `\w` (UI-LEFT-2 LL4)
            Regex::compile(r"^(\w+)(<|<=|=|>=|>)([-+]?[\d.]*)(\w*)$", false, Flavor::Advanced).expect("SetPattern's regex compiles"),
            Regex::compile(r"(\w+)[=:]([-+]?[\d.]+)(\w*)", false, Flavor::Advanced).expect("FindOne's regex compiles"),
        )
    })
}

/// The text of group `g` of a match over `chars`.
fn group_text(chars: &[char], caps: &eda_model::wx_regex::Captures, g: usize) -> String {
    caps.group(g).map(|(a, b)| chars[a..b].iter().collect()).unwrap_or_default()
}

impl Relational {
    /// `EDA_PATTERN_MATCH_RELATIONAL::SetPattern`: `^(\w+)(<|<=|=|>=|>)([-+]?[\d.]*)(\w*)$`, the key lowered, an empty value matching any, a value that is
    /// no number or a unit not in the table refusing the word.
    fn new(word: &str) -> Option<Relational> {
        let chars: Vec<char> = word.chars().collect();
        let caps = relational_regexes().0.find_at(&chars, 0)?;
        let (key, rel, val, unit) = (group_text(&chars, &caps, 1), group_text(&chars, &caps, 2), group_text(&chars, &caps, 3), group_text(&chars, &caps, 4));
        let mut relation = match rel.as_str() {
            "<" => Relation::Lt,
            "<=" => Relation::Le,
            "=" => Relation::Eq,
            ">=" => Relation::Ge,
            ">" => Relation::Gt,
            _ => return None,
        };
        let mut value = 0.0;
        if val.is_empty() {
            relation = Relation::Any;
        } else {
            value = to_c_double(&val)?;
        }
        value *= unit_multiplier(&unit.to_lowercase())?;
        Some(Relational { key: key.to_lowercase(), relation, value })
    }

    /// `EDA_PATTERN_MATCH_RELATIONAL::FindOne`: the first `key=value[unit]` (or `key:value`) of one token, its key this one's, its value (scaled by its
    /// unit when the unit is known) in the relation. Where it starts in the token.
    fn find_one(&self, token: &str) -> Option<usize> {
        // `[=:]` must be there for the regex to match: the regex is only run on a token that has one
        if !token.contains(['=', ':']) {
            return None;
        }
        let chars: Vec<char> = token.chars().collect();
        let caps = relational_regexes().1.find_at(&chars, 0)?;
        let start = caps.group(0)?.0;
        if group_text(&chars, &caps, 1).to_lowercase() != self.key {
            return None;
        }
        let mut parsed = to_c_double(&group_text(&chars, &caps, 2))?;
        if let Some(m) = unit_multiplier(&group_text(&chars, &caps, 3).to_lowercase()) {
            parsed *= m;
        }
        let hit = match self.relation {
            Relation::Lt => parsed < self.value,
            Relation::Le => parsed <= self.value,
            Relation::Eq => parsed == self.value,
            Relation::Ge => parsed >= self.value,
            Relation::Gt => parsed > self.value,
            Relation::Any => true,
        };
        hit.then_some(start)
    }

    /// `EDA_PATTERN_MATCH_RELATIONAL::Find`: the text split on blanks (`wxTOKEN_STRTOK`), the first token that has a match; its place in the text.
    fn find(&self, text: &str) -> Option<usize> {
        let mut offset = 0usize;
        for token in text.split([' ', '\t', '\r', '\n']) {
            if !token.is_empty() {
                if let Some(at) = self.find_one(token) {
                    return Some(offset + at);
                }
            }
            offset += token.chars().count() + 1;
        }
        None
    }
}

/// `EDA_PATTERN_MATCH_REGEX::SetPattern`: a word written `^...$` is a regular expression as it is, one that starts with `/` is what follows it (less a
/// closing `/`); anything else is no regular expression ("For now regular expressions must be explicit"). `None` too when it does not compile.
fn explicit_regex(word: &str) -> Option<eda_model::wx_regex::Regex> {
    let pattern = if word.starts_with('^') && word.ends_with('$') {
        word
    } else if let Some(rest) = word.strip_prefix('/') {
        rest.strip_suffix('/').unwrap_or(rest)
    } else {
        return None;
    };
    eda_model::wx_regex::Regex::compile(pattern, false, eda_model::wx_regex::Flavor::Advanced)
}

impl Matcher {
    pub(crate) fn new(word: &str) -> Matcher {
        let pattern = word.to_lowercase();
        let wild = pattern.contains(['*', '?']);
        let segments = if wild { pattern.split('*').filter(|s| !s.is_empty()).map(|s| s.chars().map(|c| (c != '?').then_some(c)).collect()).collect() } else { Vec::new() };
        let leading_star = wild && pattern.starts_with('*');
        let regex = explicit_regex(&pattern);
        let relational = Relational::new(&pattern);
        Matcher { pattern, segments, leading_star, regex, relational }
    }

    fn wild(&self) -> bool {
        self.pattern.contains(['*', '?'])
    }

    /// Where the word first matches in `text` (a position that is 0 only when it matches from the first character), if it does.
    /// `EDA_COMBINED_MATCHER::Find`: the earliest position over its matchers.
    fn find(&self, text: &str) -> Option<usize> {
        let regex = self.regex.as_ref().and_then(|re| {
            let chars: Vec<char> = text.chars().collect();
            re.find_at(&chars, 0).and_then(|caps| caps.group(0)).map(|(start, _)| start)
        });
        let wild = if self.wild() { self.find_wild(text) } else { None };
        let relational = self.relational.as_ref().and_then(|r| r.find(text));
        let plain = text.find(&self.pattern);
        [regex, wild, relational, plain].into_iter().flatten().min()
    }

    /// The leftmost match of the wildcard form: its pieces in order, each at its earliest place after the one before (a fixed-length
    /// piece found earliest ends earliest, so greedy is exact).
    fn find_wild(&self, text: &str) -> Option<usize> {
        if self.segments.is_empty() {
            return Some(0); // only stars: `.*` matches the empty start
        }
        let chars: Vec<char> = text.chars().collect();
        let mut from = 0usize;
        let mut first = None;
        for seg in &self.segments {
            let at = (from..=chars.len().checked_sub(seg.len())?).find(|&s| seg.iter().enumerate().all(|(k, c)| c.is_none_or(|c| chars[s + k] == c)))?;
            first.get_or_insert(at);
            from = at + seg.len();
        }
        Some(if self.leading_star { 0 } else { first.unwrap_or(0) })
    }

    /// `EDA_COMBINED_MATCHER::ScoreTerms`: a term that is the whole word (as typed, `GetPattern() == term.Text`) is worth 8 times its weight, one a matcher
    /// finds at its start 2 times, anywhere else once.
    pub(crate) fn score(&self, terms: &[Term]) -> u32 {
        let mut score = 0;
        for t in terms {
            if self.pattern == t.text {
                score += 8 * t.weight;
            } else if let Some(pos) = self.find(&t.text) {
                score += if pos == 0 { 2 } else { 1 } * t.weight;
            }
        }
        score
    }
}

/// The words of a query, at most 100 (`LIB_TREE_MODEL_ADAPTER::UpdateSearchString`).
pub(crate) fn parse_query(query: &str) -> Vec<Matcher> {
    query.split_whitespace().take(100).map(Matcher::new).collect()
}

/// `LIB_TREE_NODE_ITEM::UpdateScore`: 1 plus the score of every word, or 0 when one of them scores nothing.
pub(crate) fn score_item(matchers: &[Matcher], terms: &[Term]) -> u32 {
    let mut total = 1;
    for m in matchers {
        let s = m.score(terms);
        if s == 0 {
            return 0;
        }
        total += s;
    }
    total
}

/// `StrNumCmp( a, b, ignore case )`: runs of digits compare as numbers, anything else by upper-case character; the shorter of two equal
/// prefixes first.
pub(crate) fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c1), Some(c2)) if c1.is_ascii_digit() && c2.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut n: u128 = 0;
                    while let Some(d) = it.peek().and_then(|c| c.to_digit(10)) {
                        n = n.saturating_mul(10).saturating_add(d as u128);
                        it.next();
                    }
                    n
                };
                let (n1, n2) = (take(&mut a), take(&mut b));
                if n1 != n2 {
                    return n1.cmp(&n2);
                }
            }
            (Some(c1), Some(c2)) => {
                let (u1, u2) = (c1.to_uppercase().next().unwrap_or(c1), c2.to_uppercase().next().unwrap_or(c2));
                if u1 != u2 {
                    return u1.cmp(&u2);
                }
                a.next();
                b.next();
            }
        }
    }
}

/// `EDA_PATTERN_MATCH_WILDCARD_ANCHORED`: `*` and `?` over the WHOLE text, both lower case already.
pub(crate) fn wildcard_full(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) && p[pi] != '*' {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

// ------------------------------------------------------------------------------------------------ filters

/// What the footprint chooser narrows by (`FOOTPRINT_CHOOSER_FRAME::filterFootprint`) and the symbol chooser leaves out.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Filter {
    /// "Filter by pin count": only footprints with exactly this many numbered pads (0 or none: no filter).
    pub(crate) pins: Option<usize>,
    /// "Apply footprint filters": the symbol's `ki_fp_filters` patterns, lower case; a footprint passes when one matches its name (or
    /// `lib:name` for a pattern with a colon).
    pub(crate) fp_filters: Vec<String>,
    /// A power symbol is left out of the Symbol Chooser (`P` is their own tool).
    pub(crate) exclude_power: bool,
    /// Only power symbols: the chooser Place Power opens (`SYMBOL_LIBRARY_FILTER::FilterPowerSymbols( true )`, which `PANEL_SYMBOL_CHOOSER` turns into
    /// the tree filter `aNode.m_IsPower`).
    pub(crate) only_power: bool,
    /// The tree's shown columns (`LIB_TREE_MODEL_ADAPTER::GetShownColumns`, Select Columns...; UI-LEFT-2 LL4): the value of each of an item's chooser fields
    /// named here is a search term of weight 4 (`LIB_TREE_NODE::RebuildSearchTerms`). `None` is `loadColumnConfig`'s default, Item and Description.
    pub(crate) columns: Option<Vec<String>>,
}

impl Filter {
    /// The shown columns of the search's tree, sent as a tab-separated list (`cols`); an empty one leaves the default.
    pub(crate) fn with_columns(mut self, cols: &str) -> Filter {
        let list: Vec<String> = cols.split('\t').map(str::trim).filter(|c| !c.is_empty()).map(str::to_string).collect();
        if !list.is_empty() {
            self.columns = Some(list);
        }
        self
    }

    /// The shown columns (Item and Description by default).
    fn shown_columns(&self) -> Vec<&str> {
        match &self.columns {
            Some(c) => c.iter().map(String::as_str).collect(),
            None => vec!["Item", "Description"],
        }
    }

    pub(crate) fn parse(pins: &str, fp_filters: &str, power: &str) -> Filter {
        Filter {
            pins: pins.trim().parse::<usize>().ok().filter(|&n| n > 0),
            fp_filters: fp_filters.split_whitespace().map(str::to_lowercase).collect(),
            exclude_power: power == "exclude",
            only_power: power == "only",
            columns: None,
        }
    }

    /// Whether a symbol passes the power part of the filter.
    fn symbol_passes(&self, power: bool) -> bool {
        !(self.exclude_power && power) && !(self.only_power && !power)
    }

    fn footprint_passes(&self, lib: &str, name: &str, numbered_pads: u32) -> bool {
        if let Some(n) = self.pins {
            if numbered_pads as usize != n {
                return false;
            }
        }
        if !self.fp_filters.is_empty() {
            let (bare, qualified) = (name.to_lowercase(), format!("{}:{}", lib.to_lowercase(), name.to_lowercase()));
            return self.fp_filters.iter().any(|p| wildcard_full(p, if p.contains(':') { &qualified } else { &bare }));
        }
        true
    }
}

// ------------------------------------------------------------------------------------------------ indexes

/// One symbol library, indexed: its symbols (sorted by name, naturally), and the terms each is searched by.
pub(crate) struct SymbolLib {
    stamp: Option<SystemTime>,
    /// The library loaded with errors (a file of a folder library that does not read): KiCad's message, `eda_kicad::symbol_library_load_errors`.
    pub(crate) load_error: Option<String>,
    pub(crate) summaries: Vec<SymbolSummary>,
    terms: Vec<Vec<Term>>,
    by_name: HashMap<String, usize>,
}

impl SymbolLib {
    pub(crate) fn find(&self, name: &str) -> Option<&SymbolSummary> {
        self.by_name.get(name).map(|&i| &self.summaries[i])
    }
}

/// `LIB_SYMBOL::cacheSearchTerms` (`m_sourceSearchTerms`); the shown columns' fields are added per search ([`column_terms`]).
fn symbol_terms(lib: &str, s: &SymbolSummary) -> Vec<Term> {
    let mut terms = vec![term(lib, 4), term(&s.name, 8), term(&format!("{lib}:{}", s.name), 16)];
    terms.extend(s.keywords.split_whitespace().map(|k| term(k, 4)));
    terms.push(term(&s.keywords, 1));
    terms.push(term(&s.description, 1));
    if !s.footprint.is_empty() {
        terms.push(term(&s.footprint, 1));
    }
    terms
}

/// One chooser field of a symbol by name (`LIB_SYMBOL::GetChooserFields`, see [`chooser_fields`]): a mandatory field, a user field, or "Keywords".
fn chooser_field<'a>(s: &'a SymbolSummary, name: &str) -> Option<&'a str> {
    if let Some((_, v)) = s.user_fields.iter().find(|(k, _)| k == name) {
        return Some(v);
    }
    match name {
        "Reference" => Some(&s.reference),
        "Value" => Some(&s.value),
        "Footprint" => Some(&s.footprint),
        "Datasheet" => Some(&s.datasheet),
        "Description" => Some(&s.description),
        "Keywords" => Some(&s.keywords),
        _ => None,
    }
}

/// `LIB_TREE_NODE::RebuildSearchTerms`: the value of each of the symbol's chooser fields whose column is shown, weight 4 (a footprint has no chooser fields).
fn column_terms(s: &SymbolSummary, shown: &[&str]) -> Vec<Term> {
    shown.iter().filter_map(|name| chooser_field(s, name)).map(|v| term(v, 4)).collect()
}

/// One footprint library, indexed.
pub(crate) struct FootprintLib {
    stamp: Option<SystemTime>,
    pub(crate) names: Vec<String>,
    pub(crate) infos: Vec<FootprintSummary>,
    terms: Vec<Vec<Term>>,
}

/// `FOOTPRINT_INFO::GetSearchTerms`.
fn footprint_terms(lib: &str, name: &str, f: &FootprintSummary) -> Vec<Term> {
    let mut terms = vec![term(lib, 4), term(name, 8), term(&format!("{lib}:{name}"), 16)];
    terms.extend(f.tags.split_whitespace().map(|k| term(k, 4)));
    terms.push(term(&f.tags, 1));
    terms.push(term(&f.description, 1));
    terms
}

type SymbolCache = Mutex<HashMap<PathBuf, Arc<SymbolLib>>>;
type FootprintCache = Mutex<HashMap<PathBuf, Arc<FootprintLib>>>;

fn symbol_cache() -> &'static SymbolCache {
    static C: OnceLock<SymbolCache> = OnceLock::new();
    C.get_or_init(Default::default)
}

fn footprint_cache() -> &'static FootprintCache {
    static C: OnceLock<FootprintCache> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// The file (or folder library, KiCad 10's `.kicad_symdir`) of symbol library `lib`.
fn symbol_path<S: LibSource + ?Sized>(src: &S, lib: &str) -> Result<PathBuf, String> {
    src.lib_path(Kind::Symbol, lib)
}

/// The folder of footprint library `lib`.
fn footprint_dir<S: LibSource + ?Sized>(src: &S, lib: &str) -> Result<PathBuf, String> {
    src.lib_path(Kind::Footprint, lib)
}

/// The index of symbol library `lib` of a source, built if it is not cached (or the file changed since): one read of the file and one pass
/// over its text -- a folder library's text being the one its files make (`eda_kicad::read_symbol_library_text`), which [`installed_symbol`]
/// reads again to cut a symbol out by the span the index keeps.
pub(crate) fn symbol_lib<S: LibSource + ?Sized>(src: &S, lib: &str) -> Result<Arc<SymbolLib>, String> {
    let path = symbol_path(src, lib)?;
    let stamp = Kind::Symbol.modified(&path);
    if stamp.is_none() {
        return Err(format!("no library {lib:?} is installed"));
    }
    if let Some(hit) = lock(symbol_cache()).get(&path) {
        if hit.stamp == stamp {
            return Ok(hit.clone());
        }
    }
    let (text, _) = eda_kicad::read_symbol_library(&path)?;
    let mut summaries = eda_kicad::scan_symbols(&text);
    eda_kicad::inherit_symbol_summaries(&mut summaries);
    // what KiCad's parser refuses is not in the library (UI-LEFT-3 L3-6, `crate::library_verdict`), and the library says so
    let (refused, load_error) = crate::library_verdict::symbol_refusals(&path, true);
    summaries.retain(|s| !refused.contains(&s.name));
    summaries.sort_by(|a, b| natural_cmp(&a.name, &b.name).then_with(|| a.name.cmp(&b.name)));
    summaries.dedup_by(|b, a| a.name == b.name);
    let terms = summaries.iter().map(|s| symbol_terms(lib, s)).collect();
    let by_name = summaries.iter().enumerate().map(|(i, s)| (s.name.clone(), i)).collect();
    let built = Arc::new(SymbolLib { stamp, load_error, summaries, terms, by_name });
    lock(symbol_cache()).insert(path, built.clone());
    Ok(built)
}

/// The cached index of a symbol library, if there is a fresh one.
fn cached_symbol_lib<S: LibSource + ?Sized>(src: &S, lib: &str) -> Option<Arc<SymbolLib>> {
    let path = symbol_path(src, lib).ok()?;
    let hit = lock(symbol_cache()).get(&path).cloned()?;
    (hit.stamp == Kind::Symbol.modified(&path)).then_some(hit)
}

/// The index of footprint library `lib` of a source: every `.kicad_mod` of the `.pretty` directory read once and scanned.
pub(crate) fn footprint_lib<S: LibSource + ?Sized>(src: &S, lib: &str) -> Result<Arc<FootprintLib>, String> {
    let dir = footprint_dir(src, lib)?;
    let stamp = modified(&dir);
    if stamp.is_none() {
        return Err(format!("no library {lib:?} is installed"));
    }
    if let Some(hit) = lock(footprint_cache()).get(&dir) {
        if hit.stamp == stamp {
            return Ok(hit.clone());
        }
    }
    let mut files: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("kicad_mod"))
        .filter_map(|p| Some((p.file_stem()?.to_str()?.to_string(), p)))
        .collect();
    files.sort_by(|a, b| natural_cmp(&a.0, &b.0).then_with(|| a.0.cmp(&b.0)));
    // `FP_CACHE::Load`: a file the reader refuses -- this crate's or KiCad's parser (`crate::library_verdict`, UI-LEFT-3 L3-6) -- is not in the library, so the
    // choosers neither list nor find it
    let (refused, _) = crate::library_verdict::footprint_refusals(&dir, true);
    files.retain(|(name, _)| !refused.contains(name));
    let mut names = Vec::with_capacity(files.len());
    let mut infos = Vec::with_capacity(files.len());
    for (name, path) in files {
        let info = std::fs::read_to_string(&path).ok().and_then(|t| eda_kicad::scan_footprint(&t)).unwrap_or_default();
        names.push(name);
        infos.push(info);
    }
    let terms = names.iter().zip(&infos).map(|(n, i)| footprint_terms(lib, n, i)).collect();
    let built = Arc::new(FootprintLib { stamp, names, infos, terms });
    lock(footprint_cache()).insert(dir, built.clone());
    Ok(built)
}

fn cached_footprint_lib<S: LibSource + ?Sized>(src: &S, lib: &str) -> Option<Arc<FootprintLib>> {
    let dir = footprint_dir(src, lib).ok()?;
    let hit = lock(footprint_cache()).get(&dir).cloned()?;
    (hit.stamp == modified(&dir)).then_some(hit)
}

/// Index library `lib` of `kind` of a source (a no-op when it is indexed already).
fn index_lib<S: LibSource + ?Sized>(kind: Kind, src: &S, lib: &str) -> Result<(), String> {
    match kind {
        Kind::Symbol => symbol_lib(src, lib).map(|_| ()),
        Kind::Footprint => footprint_lib(src, lib).map(|_| ()),
    }
}

/// Index every library of `kind` of a source, one after the other.
pub(crate) fn index_all<S: LibSource + ?Sized>(kind: Kind, src: &S) {
    for lib in libraries_in(kind, src).iter() {
        let _ = index_lib(kind, src, &lib.name);
        std::thread::yield_now();
    }
}

type Builds = Mutex<HashMap<(Kind, String), Arc<AtomicBool>>>;

fn builds() -> &'static Builds {
    static B: OnceLock<Builds> = OnceLock::new();
    B.get_or_init(Default::default)
}

/// Start the thread that indexes every library of `kind` of a source, unless one is running or has run for this very set of libraries; the flag it
/// returns is set when it is done.
fn ensure_background_index<S: LibSource + ?Sized>(kind: Kind, src: &S) -> Arc<AtomicBool> {
    let key = (kind, src.fingerprint(kind));
    let mut all = lock(builds());
    if let Some(done) = all.get(&key) {
        return done.clone();
    }
    let done = Arc::new(AtomicBool::new(false));
    all.insert(key.clone(), done.clone());
    drop(all);
    let (owned, flag) = (src.boxed(), done.clone());
    let spawned = std::thread::Builder::new().name("library-search-index".into()).spawn(move || {
        index_all(kind, &*owned);
        flag.store(true, AtomicOrdering::Release);
    });
    if spawned.is_err() {
        // No thread: the search indexes what it needs inline instead.
        done.store(true, AtomicOrdering::Release);
    }
    done
}

/// The symbol libraries read in the background as the board is opened, so the first Place Power or Place Symbol finds them indexed rather than waiting for
/// every library ("Searching... n of 222", UI-LEFT-2 SS12). KiCad 25ec2335 `eeschema/eeschema.cpp::IFACE::PreloadLibraries`: when a project is opened the
/// symbol library adapter starts loading every library on a background job ("Loading Symbol Libraries", `adapter->AsyncLoad()`), and the chooser later shows
/// what is loaded. The same thread the first search would start ([`ensure_background_index`]), started by `board serve`; the table is read on that thread
/// too, so serving starts at once.
pub(crate) fn preload_symbol_libraries(dir: &Path) {
    let dir = dir.to_path_buf();
    let _ = std::thread::Builder::new().name("library-preload".into()).spawn(move || {
        let libs = crate::library_tables::libs(&dir, Kind::Symbol);
        ensure_background_index(Kind::Symbol, &*libs);
    });
}

// ------------------------------------------------------------------------------------------------ library descriptions

type Descriptions = Mutex<HashMap<PathBuf, (Option<SystemTime>, Arc<HashMap<String, String>>)>>;

/// `(lib (name "X") ... (descr "Y"))` of KiCad's `sym-lib-table` / `fp-lib-table`, one library to a line.
pub(crate) fn parse_lib_table(text: &str) -> HashMap<String, String> {
    let quoted = |line: &str, key: &str| -> Option<String> {
        let at = line.find(key)? + key.len();
        let rest = &line[at..];
        Some(rest[..rest.find('"')?].to_string())
    };
    text.lines().filter_map(|l| Some((quoted(l, "(name \"")?, quoted(l, "(descr \"")?))).collect()
}

/// The library descriptions the installed tables carry (the Description column of a library row), from `<root>/../template/` (an install's
/// `PATHS::GetStockTemplatesPath`), else from `<root>/` itself, where the libraries' own repositories (kicad-symbols, kicad-footprints) keep the table
/// their install copies to the template folder -- a checkout of them named by `KICAD_PATH`.
pub(crate) fn library_descriptions(kind: Kind, root: &Path) -> Arc<HashMap<String, String>> {
    static CACHE: OnceLock<Descriptions> = OnceLock::new();
    let file = if kind == Kind::Symbol { "sym-lib-table" } else { "fp-lib-table" };
    let Some(installed) = root.parent().map(|p| p.join("template").join(file)) else { return Arc::default() };
    let path = if !installed.is_file() && root.join(file).is_file() { root.join(file) } else { installed };
    let stamp = modified(&path);
    let cache = CACHE.get_or_init(Default::default);
    if let Some((s, hit)) = lock(cache).get(&path) {
        if *s == stamp {
            return hit.clone();
        }
    }
    let parsed = Arc::new(std::fs::read_to_string(&path).map(|t| parse_lib_table(&t)).unwrap_or_default());
    lock(cache).insert(path, (stamp, parsed.clone()));
    parsed
}

// ------------------------------------------------------------------------------------------------ the answers

/// `LIB_SYMBOL::GetChooserFields` (`cacheChooserFields`): every field of the symbol by name (each is shown in the chooser, `m_showInChooser` is never cleared
/// at 8303b2ad), and "Keywords" -- the symbol's keywords unless a field of that name is its own: the values the library tree's extra columns show
/// (`LIB_TREE_MODEL_ADAPTER::GetValue`; Select Columns, UI-LEFT A2). The Description column shows `description`.
fn chooser_fields(s: &SymbolSummary) -> serde_json::Map<String, Value> {
    let mut m = serde_json::Map::new();
    for (k, v) in [("Reference", &s.reference), ("Value", &s.value), ("Footprint", &s.footprint), ("Datasheet", &s.datasheet), ("Description", &s.description)] {
        m.insert(k.to_string(), json!(v));
    }
    for (k, v) in &s.user_fields {
        m.insert(k.clone(), json!(v));
    }
    if !m.contains_key("Keywords") {
        m.insert("Keywords".to_string(), json!(s.keywords));
    }
    m
}

fn symbol_item(s: &SymbolSummary, score: Option<u32>) -> Value {
    let mut v = json!({ "name": s.name, "description": s.description, "units": s.unit_count(), "pins": s.pin_count(), "power": s.power, "fields": chooser_fields(s) });
    if let Some(score) = score {
        v["score"] = json!(score);
    }
    v
}

fn footprint_item(name: &str, f: &FootprintSummary, score: Option<u32>) -> Value {
    let mut v = json!({ "name": name, "description": f.description, "pads": f.numbered_pads });
    if let Some(score) = score {
        v["score"] = json!(score);
    }
    v
}

/// `GET /api/library/entries?kind=&lib=`: the items of one library with their descriptions, in the tree's order (names sorted naturally),
/// narrowed by the footprint chooser's filters. `{"lib", "description"?, "entries": [..]}` or `{"error"}`.
pub(crate) fn entries_in<S: LibSource + ?Sized>(kind: Kind, src: &S, lib: &str, filter: &Filter) -> Value {
    let mut v = match kind {
        Kind::Symbol => match symbol_lib(src, lib) {
            Ok(idx) => {
                let mut v = json!({ "lib": lib, "entries": idx.summaries.iter().filter(|s| filter.symbol_passes(s.power)).map(|s| symbol_item(s, None)).collect::<Vec<_>>() });
                // read with errors: the symbols that read are listed, and KiCad's message goes with them (`LIB_STATUS::error`)
                if let Some(e) = &idx.load_error {
                    v["load_error"] = json!(e);
                }
                v
            }
            Err(e) => return json!({ "error": e }),
        },
        Kind::Footprint => match footprint_lib(src, lib) {
            Ok(idx) => json!({
                "lib": lib,
                "entries": idx.names.iter().zip(&idx.infos).filter(|(n, i)| filter.footprint_passes(lib, n, i.numbered_pads)).map(|(n, i)| footprint_item(n, i, None)).collect::<Vec<_>>(),
            }),
            Err(e) => return json!({ "error": e }),
        },
    };
    if let Some(d) = src.describe(kind, lib) {
        v["description"] = json!(d);
    }
    v
}

pub(crate) fn entries(dir: &Path, kind: Kind, lib: &str, filter: &Filter) -> Value {
    entries_in(kind, &*crate::library_tables::libs(dir, kind), lib, filter)
}

/// The most items one search sends.
pub(crate) const SEARCH_LIMIT: usize = 400;

struct Hit {
    lib: usize,
    item: usize,
    score: u32,
}

/// `GET /api/library/search?kind=&q=`: the items of every (indexed) library that match, best first, grouped by library
/// (`LIB_TREE_NODE::Compare`: a library sorts by the best score of its items, then by name). `filter` narrows (the footprint chooser's pin
/// count and footprint filters, the symbol chooser's power symbols); `limit` caps the items sent.
///
/// While the thread that indexes every library has not finished the answer is `"status": "indexing"` with `indexed` of `total` libraries done
/// and the matches among those; an empty query only starts that thread. `matches` counts every match, `truncated` says the items sent are
/// the best `limit` of them. With the power chooser's filter (`only_power`) the answer also names the libraries read so far that hold a power symbol
/// (`power_libraries`, even for an empty query): the libraries its tree shows.
pub(crate) fn search_in<S: LibSource + ?Sized>(kind: Kind, src: &S, query: &str, filter: &Filter, limit: usize) -> Value {
    let finished = ensure_background_index(kind, src);
    let libs = libraries_in(kind, src);
    let matchers = parse_query(query);
    let shown = filter.shown_columns();
    // Nothing installed is nothing to wait for.
    let done = finished.load(AtomicOrdering::Acquire) || libs.is_empty();

    let mut hits: Vec<Hit> = Vec::new();
    let mut indexed_count = 0usize;
    let mut sym_libs: Vec<Option<Arc<SymbolLib>>> = Vec::new();
    let mut fp_libs: Vec<Option<Arc<FootprintLib>>> = Vec::new();
    let mut power_libraries: Vec<String> = Vec::new();
    for (li, info) in libs.iter().enumerate() {
        // A library the thread has not reached is skipped while it runs; once it is done, one it lacks (changed on disk since) is indexed here. The
        // cache is asked once per library: whether a folder library changed is a look at each of its files (`Kind::modified`).
        let have = match kind {
            Kind::Symbol => {
                let idx = cached_symbol_lib(src, &info.name).or_else(|| done.then(|| symbol_lib(src, &info.name).ok()).flatten());
                let have = idx.is_some();
                sym_libs.push(idx);
                have
            }
            Kind::Footprint => {
                let idx = cached_footprint_lib(src, &info.name).or_else(|| done.then(|| footprint_lib(src, &info.name).ok()).flatten());
                let have = idx.is_some();
                fp_libs.push(idx);
                have
            }
        };
        if have {
            indexed_count += 1;
        }
        // The power chooser's tree has only the libraries that hold a power symbol (`LIB_TREE_MODEL_ADAPTER` hides a library whose items the
        // filter all hides): which of the libraries read so far do.
        if kind == Kind::Symbol && filter.only_power && sym_libs[li].as_ref().is_some_and(|idx| idx.summaries.iter().any(|s| s.power)) {
            power_libraries.push(info.name.clone());
        }
        if matchers.is_empty() {
            continue;
        }
        match kind {
            Kind::Symbol => {
                if let Some(idx) = &sym_libs[li] {
                    for (i, (s, terms)) in idx.summaries.iter().zip(&idx.terms).enumerate() {
                        if !filter.symbol_passes(s.power) {
                            continue;
                        }
                        let mut terms = terms.clone();
                        terms.extend(column_terms(s, &shown));
                        let score = score_item(&matchers, &terms);
                        if score > 0 {
                            hits.push(Hit { lib: li, item: i, score });
                        }
                    }
                }
            }
            Kind::Footprint => {
                if let Some(idx) = &fp_libs[li] {
                    for (i, ((n, f), terms)) in idx.names.iter().zip(&idx.infos).zip(&idx.terms).enumerate() {
                        if !filter.footprint_passes(&info.name, n, f.numbered_pads) {
                            continue;
                        }
                        let score = score_item(&matchers, terms);
                        if score > 0 {
                            hits.push(Hit { lib: li, item: i, score });
                        }
                    }
                }
            }
        }
    }

    let matches = hits.len();
    // The best `limit`, ties by library then name (the libraries and their items are already in name order).
    hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.lib.cmp(&b.lib)).then(a.item.cmp(&b.item)));
    let truncated = hits.len() > limit;
    hits.truncate(limit);

    // Group by library: the one with the best item first, ties by name order; an item list best first.
    let mut order: Vec<usize> = Vec::new();
    let mut groups: HashMap<usize, Vec<&Hit>> = HashMap::new();
    for h in &hits {
        groups.entry(h.lib).or_insert_with(|| {
            order.push(h.lib);
            Vec::new()
        });
        groups.get_mut(&h.lib).expect("just inserted").push(h);
    }
    let libraries: Vec<Value> = order
        .iter()
        .map(|&li| {
            let name = &libs[li].name;
            let items: Vec<Value> = groups[&li]
                .iter()
                .map(|h| match kind {
                    Kind::Symbol => symbol_item(&sym_libs[li].as_ref().expect("a hit has an index").summaries[h.item], Some(h.score)),
                    Kind::Footprint => {
                        let idx = fp_libs[li].as_ref().expect("a hit has an index");
                        footprint_item(&idx.names[h.item], &idx.infos[h.item], Some(h.score))
                    }
                })
                .collect();
            let mut lib = json!({ "name": name, "score": groups[&li][0].score, "items": items });
            if let Some(d) = src.describe(kind, name) {
                lib["description"] = json!(d);
            }
            lib
        })
        .collect();

    let mut answer = json!({
        "status": if done { "ready" } else { "indexing" },
        "indexed": indexed_count,
        "total": libs.len(),
        "matches": matches,
        "truncated": truncated,
        "libraries": libraries,
    });
    if filter.only_power {
        answer["power_libraries"] = json!(power_libraries);
    }
    // The libraries read so far that loaded with errors (`LIBRARY_MANAGER_ADAPTER::GetLibraryLoadErrors`): the chooser says so, as KiCad's status bar does.
    let load_errors: Vec<Value> = libs
        .iter()
        .zip(&sym_libs)
        .filter_map(|(info, idx)| idx.as_ref()?.load_error.as_ref().map(|e| json!({ "lib": info.name, "error": e })))
        .collect();
    if !load_errors.is_empty() {
        answer["load_errors"] = json!(load_errors);
    }
    answer
}

/// `GET /api/library/load_errors?kind=symbol`: every symbol library of the tables that loads with errors, with KiCad's message (`GetLibraryLoadErrors`, which the
/// frames put in the status bar's warnings after the libraries load: `statusBar->AddWarningMessages( "load", .. )`). A library is read for this once until it
/// changes (`eda_kicad::symbol_library_load_errors`), and the index's reads answer it too. Footprint libraries: none (a `.pretty` file that does not read is
/// listed by its name, as KiCad lists it).
pub(crate) fn load_errors_in<S: LibSource + ?Sized>(kind: Kind, src: &S) -> Value {
    if kind != Kind::Symbol {
        return json!({ "kind": "footprint", "errors": [] });
    }
    let errors: Vec<Value> = libraries_in(kind, src)
        .iter()
        .filter_map(|info| {
            let path = symbol_path(src, &info.name).ok()?;
            crate::library_verdict::symbol_refusals(&path, false).1.map(|e| json!({ "lib": info.name, "path": path.display().to_string(), "error": e }))
        })
        .collect();
    json!({ "kind": "symbol", "errors": errors })
}

pub(crate) fn load_errors(dir: &Path, kind: Kind) -> Value {
    load_errors_in(kind, &*crate::library_tables::libs(dir, kind))
}

/// The most entries the Symbol Chooser's footprint selector lists (`FOOTPRINT_SELECT_WIDGET( .., int aMaxItems = 400 )`).
pub(crate) const SELECT_LIMIT: usize = 400;

/// `GET /api/library/fp_select?pins=&fp_filters=&zero=`: the footprints the Symbol Chooser's footprint selector lists for the selected symbol
/// (`PANEL_SYMBOL_CHOOSER::populateFootprintSelector` -> `FOOTPRINT_SELECT_WIDGET::UpdateList` -> pcbnew's `filterFootprints`, `KIFACE_FILTER_FOOTPRINTS`): every
/// footprint of every library with exactly the symbol's pin count of numbered pads (`GetUniquePadCount( DO_NOT_INCLUDE_NPTH )`) and a name one of its
/// footprint filters matches whole (`EDA_PATTERN_MATCH_WILDCARD_ANCHORED`, `Lib:Name` for a filter with a colon), as `Lib:Name`, at most `limit`. With
/// `zero` (`m_zero_filter`: the symbol has no footprint filters) and nothing to filter by (no pins either) the list is empty -- the selector then offers the
/// default footprint alone. The libraries in the trees' order, each one's footprints by name. `filterFootprints` waits for every library to load
/// (`BlockUntilLoaded`); here the answer says `"status": "indexing"` until the index thread has read them, with the footprints of those read, and the page
/// asks again.
pub(crate) fn footprint_select_in<S: LibSource + ?Sized>(src: &S, filter: &Filter, zero_filters: bool, limit: usize) -> Value {
    let has_filters = filter.pins.is_some() || !filter.fp_filters.is_empty();
    if zero_filters && !has_filters {
        return json!({ "status": "ready", "footprints": [] });
    }
    let finished = ensure_background_index(Kind::Footprint, src);
    let libs = libraries_in(Kind::Footprint, src);
    let done = finished.load(AtomicOrdering::Acquire) || libs.is_empty();
    let mut out: Vec<String> = Vec::new();
    let (mut indexed, mut skipped) = (0usize, false);
    'libs: for info in libs.iter() {
        let Some(idx) = cached_footprint_lib(src, &info.name).or_else(|| done.then(|| footprint_lib(src, &info.name).ok()).flatten()) else {
            // not read yet (once the thread is done, a library that cannot be read is no library)
            skipped |= !done;
            continue;
        };
        indexed += 1;
        for (n, f) in idx.names.iter().zip(&idx.infos) {
            if filter.footprint_passes(&info.name, n, f.numbered_pads) {
                out.push(format!("{}:{n}", info.name));
                if out.len() >= limit {
                    break 'libs;
                }
            }
        }
    }
    // every library up to the last one listed read (by the thread, or by an earlier request): the list is the one the finished index makes
    let ready = done || !skipped;
    json!({ "status": if ready { "ready" } else { "indexing" }, "indexed": indexed, "total": libs.len(), "footprints": out })
}

pub(crate) fn footprint_select(dir: &Path, filter: &Filter, zero_filters: bool) -> Value {
    footprint_select_in(&*crate::library_tables::libs(dir, Kind::Footprint), filter, zero_filters, SELECT_LIMIT)
}

pub(crate) fn search(dir: &Path, kind: Kind, query: &str, filter: &Filter, limit: usize) -> Value {
    search_in(kind, &*crate::library_tables::libs(dir, kind), query, filter, limit)
}

/// A symbol of the installed libraries, cut out of its library file and parsed alone: the editable definition (units, body styles, keywords,
/// footprint filters) named `Lib:Name`, and what the index knows of it (default footprint, datasheet, value).
pub(crate) fn installed_symbol<S: LibSource + ?Sized>(src: &S, lib_id: &str) -> Option<(LibrarySymbol, SymbolSummary)> {
    read_installed_symbol(src, lib_id).ok().flatten()
}

/// [`installed_symbol`], telling a symbol the libraries do not have (`Ok(None)`: not a `Lib:Name`, no such library, no such symbol in it) from one whose
/// library is there and cannot be read, or whose text is there and cannot be parsed (`Err`, why). The Symbol Editor must not open the second as a blank
/// symbol: `SYMBOL_EDIT_FRAME::LoadSymbol` says "Error loading symbol %s from library '%s'." and returns with the editor as it was. A symbol whose text the
/// index could not take -- a folder library's `Name.kicad_sym` that does not parse (the folder reader skips it), a `(symbol "Name"` form of a file the scan
/// lost -- is the second kind too.
pub(crate) fn read_installed_symbol<S: LibSource + ?Sized>(src: &S, lib_id: &str) -> Result<Option<(LibrarySymbol, SymbolSummary)>, String> {
    let Some((lib, name)) = lib_id.split_once(':') else { return Ok(None) };
    if !legal_library_name(name) {
        return Ok(None);
    }
    let Ok(path) = symbol_path(src, lib) else { return Ok(None) };
    if Kind::Symbol.modified(&path).is_none() {
        return Ok(None); // no library at the row's path
    }
    let idx = symbol_lib(src, lib)?;
    let Some(summary) = idx.find(name).cloned() else {
        // not in the index: a symbol the library does not have -- or one whose text is there and could not be read into it
        if path.is_dir() {
            let file = path.join(format!("{name}.kicad_sym"));
            if file.is_file() {
                let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
                return Err(match eda_kicad::parse_library_symbols(&text) {
                    Err(e) => format!("{}: {e}", file.display()),
                    Ok(_) => format!("{}: the symbol {name:?} is not in it", file.display()),
                });
            }
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            if text.contains(&format!("(symbol \"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))) {
                return Err(format!("{}: the symbol {name:?} cannot be read", path.display()));
            }
        }
        return Ok(None);
    };
    let text = eda_kicad::read_symbol_library_text(&path)?;
    let Some(mut sym) = eda_kicad::parse_symbol_checked(&text, &idx.summaries, name).map_err(|e| format!("{}: {e}", path.display()))? else { return Ok(None) };
    sym.lib_id = lib_id.to_string();
    sym.assign_missing_ids();
    Ok(Some((sym, summary)))
}

/// `GET /api/library/details?kind=&id=Lib:Name`: what the chooser's preview and description pane show for the selected item -- its fields and
/// its drawing, loaded now and only this one. `{"id", ..., "symbol" | "footprint": <editable definition>}` or `{"error"}`.
pub(crate) fn details_in<S: LibSource + ?Sized>(kind: Kind, src: &S, id: &str) -> Value {
    match kind {
        Kind::Symbol => match installed_symbol(src, id) {
            Some((sym, s)) => json!({
                "id": id,
                "description": s.description,
                "keywords": s.keywords,
                "reference": s.reference,
                "value": s.value,
                "footprint": s.footprint,
                "datasheet": s.datasheet,
                "fp_filters": s.fp_filters,
                "units": s.unit_count(),
                "pins": s.pin_count(),
                "power": s.power,
                "extends": s.extends,
                "alternate_body_style": s.alternate_body_style,
                "symbol": sym,
            }),
            None => json!({ "error": format!("no symbol {id:?} in the installed libraries") }),
        },
        Kind::Footprint => {
            let Some((lib, name)) = id.split_once(':') else { return json!({ "error": format!("{id:?} is not a Lib:Name") }) };
            let (Some(fp), Ok(idx)) = (installed_footprint(src, id), footprint_lib(src, lib)) else { return json!({ "error": format!("no footprint {id:?} in the installed libraries") }) };
            let at = idx.names.iter().position(|n| n == name);
            let info = at.map(|i| idx.infos[i].clone()).unwrap_or_default();
            json!({ "id": id, "description": info.description, "tags": info.tags, "pads": info.pads, "numbered_pads": info.numbered_pads, "footprint": fp })
        }
    }
}

pub(crate) fn details(dir: &Path, kind: Kind, id: &str) -> Value {
    details_in(kind, &*crate::library_tables::libs(dir, kind), id)
}

/// A footprint of the installed libraries (`Lib:Name` -> `<root>/Lib.pretty/Name.kicad_mod`) in the editable type, every graphic, text, attribute
/// and pad layer included, named `Lib:Name`.
pub(crate) fn installed_footprint<S: LibSource + ?Sized>(src: &S, name: &str) -> Option<LibraryFootprint> {
    read_installed_footprint(src, name).ok().flatten()
}

/// [`installed_footprint`], telling a footprint the libraries do not have (`Ok(None)`: not a `Lib:Name`, no such library, no `Name.kicad_mod` in its
/// folder) from one whose file is there and cannot be read or parsed (`Err`, why). The Footprint Editor must not open the second as a blank footprint:
/// `PCB_BASE_FRAME::LoadFootprint` gives nothing on the `IO_ERROR`, and `FOOTPRINT_EDIT_FRAME::LoadFootprintFromLibrary` then returns with the editor as it was.
pub(crate) fn read_installed_footprint<S: LibSource + ?Sized>(src: &S, name: &str) -> Result<Option<LibraryFootprint>, String> {
    let Some((lib, item)) = name.split_once(':') else { return Ok(None) };
    if !legal_library_name(item) || item.is_empty() {
        return Ok(None);
    }
    let Ok(dir) = footprint_dir(src, lib) else { return Ok(None) };
    let file = dir.join(format!("{item}.kicad_mod"));
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", file.display())),
    };
    let mut fp = eda_kicad::parse_library_footprint(&text).map_err(|e| format!("{}: {e}", file.display()))?.footprint;
    fp.name = name.to_string();
    fp.assign_missing_ids();
    Ok(Some(fp))
}

/// A footprint of the installed libraries as the board's model has footprints (`eda_model::Footprint`: pads, graphics, courtyard outlines and 3D models, as the
/// board reader keeps them), named `Lib:Name`: the library copy an exchange puts on a part, and what the board's is compared with.
pub(crate) fn installed_engine_footprint<S: LibSource + ?Sized>(src: &S, name: &str) -> Option<eda_model::Footprint> {
    let (lib, item) = name.split_once(':')?;
    if !legal_library_name(item) || item.is_empty() {
        return None;
    }
    let file = footprint_dir(src, lib).ok()?.join(format!("{item}.kicad_mod"));
    let text = std::fs::read_to_string(file).ok()?;
    eda_kicad::parse_footprint_file(&text, name).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eda-library-search-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn matcher_score(query: &str, terms: &[(&str, u32)]) -> u32 {
        let terms: Vec<Term> = terms.iter().map(|(t, w)| term(t, *w)).collect();
        score_item(&parse_query(query), &terms)
    }

    #[test]
    fn a_term_scores_8_times_its_weight_when_exact_2_times_at_its_start_and_once_inside() {
        // the weights of a symbol's terms: name 8, library 4, keyword 4, description 1
        assert_eq!(matcher_score("lm358", &[("LM358", 8)]), 1 + 8 * 8, "the whole term");
        assert_eq!(matcher_score("lm3", &[("LM358", 8)]), 1 + 2 * 8, "its start");
        assert_eq!(matcher_score("358", &[("LM358", 8)]), 1 + 8, "inside it");
        assert_eq!(matcher_score("nope", &[("LM358", 8)]), 0);
        // an item is the sum over its terms
        assert_eq!(matcher_score("opamp", &[("Amplifier_Operational", 4), ("LM358", 8), ("dual opamp", 4), ("opamp", 4), ("Dual Operational Amplifier, opamp", 1)]), 1 + 4 + 8 * 4 + 1);
    }

    #[test]
    fn every_word_of_the_query_must_score_and_the_scores_add() {
        let terms = [("LM358", 8), ("Low-Power, Dual Operational Amplifier", 1), ("dual opamp", 4), ("dual", 4), ("opamp", 4)];
        // "lm358": the exact name, 8 * 8. "dual": inside the description (1), at the start of "dual opamp" (2 * 4) and the whole of "dual" (8 * 4).
        assert_eq!(matcher_score("lm358 dual", &terms), 1 + 8 * 8 + (1 + 2 * 4 + 8 * 4));
        assert_eq!(matcher_score("lm358 nothinglikeit", &terms), 0, "one word that scores nothing hides the item");
        assert_eq!(matcher_score("", &terms), 1, "no query: every item shows");
        assert_eq!(matcher_score("   ", &terms), 1);
        assert_eq!(matcher_score("LM358", &terms), matcher_score("lm358", &terms), "case does not matter");
    }

    #[test]
    fn a_word_may_carry_wildcards_and_nothing_else_is_special() {
        let terms = [("SOIC-8_3.9x4.9mm_P1.27mm", 8)];
        assert!(matcher_score("soic*3.9x4.9", &terms) > 0);
        assert!(matcher_score("so?c", &terms) > 0);
        assert!(matcher_score("*p1.27", &terms) > 1, "a leading star matches from the start");
        assert_eq!(matcher_score("soic*qfn", &terms), 0);
        assert_eq!(matcher_score("p1*soic", &terms), 0, "the pieces must come in order");
        assert_eq!(matcher_score("soic*3.9x4.9mm_p1.27mm", &terms), 1 + 2 * 8);
        assert_eq!(matcher_score("voltage>3.3", &[("a voltage>3.3 b", 1)]), 1 + 1, "a relational word is found as its text too");
    }

    /// `EDA_PATTERN_MATCH_REGEX` (the same cases as `libChooser.test.ts`).
    #[test]
    fn a_word_written_slash_re_slash_or_caret_re_dollar_is_a_regular_expression_found_anywhere_in_a_term() {
        let t = [("R_0603_1608Metric", 8), ("Resistor_SMD", 4), ("Resistor_SMD:R_0603_1608Metric", 16)];
        assert_eq!(matcher_score("/^R_0603/", &t), 1 + 2 * 8, "the word is lowered first; ^ starts only the name");
        assert_eq!(matcher_score("/^r_0603", &t), 1 + 2 * 8, "the closing slash may be left off");
        assert_eq!(matcher_score("/0603/", &t), 1 + 8 + 16, "found inside the name and inside Lib:Name");
        assert_eq!(matcher_score("^r_0603.*$", &t), 1 + 2 * 8, "^...$ is a regular expression as it is");
        assert_eq!(matcher_score("/^r_0805/", &t), 0);
        assert_eq!(matcher_score("/(/", &t), 0, "a pattern that does not compile is no matcher, and its text is nowhere");
        assert_eq!(matcher_score("/smd$/", &t), 1 + 4, "the end of a term");
        assert_eq!(matcher_score("/r_0[68]0[35]/", &t), 1 + 2 * 8 + 16, "classes");
    }

    /// `EDA_PATTERN_MATCH_RELATIONAL` (the same cases as `libChooser.test.ts`).
    #[test]
    fn a_relational_word_matches_a_key_value_of_the_terms_words() {
        let r = [("resistance=4.7k 1% 0603", 1)];
        assert_eq!(matcher_score("resistance<10k", &r), 1 + 2, "4.7k is below 10k, at the start of the term");
        assert_eq!(matcher_score("resistance<10k", &[("resistance=22k", 1)]), 0, "22k is not");
        assert_eq!(matcher_score("resistance>=4700", &r), 1 + 2, "a unit scales the value: 4.7k is 4700");
        assert_eq!(matcher_score("resistance=4.7k", &r), 1 + 2);
        assert_eq!(matcher_score("resistance>4.7k", &r), 0);
        assert_eq!(matcher_score("voltage>3.3", &[("vin voltage=5v", 1)]), 1 + 1, "a unit not in the table leaves the value as it is; found past the first word");
        assert_eq!(matcher_score("voltage>", &[("voltage:1", 4)]), 1 + 2 * 4, "no value yet matches any value (ANY)");
        assert_eq!(matcher_score("current<1", &[("voltage=0.5", 1)]), 0, "another key");
        assert_eq!(matcher_score("size>=1ki", &[("size=1024", 1)]), 1 + 2, "ki is 1024");
        assert_eq!(matcher_score("c=100n", &[("c=100n", 4)]), 1 + 8 * 4, "the whole term is still 8 times");
        assert_eq!(matcher_score("voltage>3.3v", &[("voltage=5", 1)]), 0, "a unit not in the table makes the word no relational one");
        // SetPattern's parse: the relation, the value and the unit
        let rel = Relational::new("resistance<=10k").unwrap();
        assert_eq!((rel.key.as_str(), rel.relation, rel.value), ("resistance", Relation::Le, 10_000.0));
        assert_eq!(Relational::new("v>").map(|r| r.relation), Some(Relation::Any));
        assert!(Relational::new("v>1.2.3").is_none() && Relational::new("v>x").is_none() && Relational::new("lm358").is_none());
        assert_eq!(Relational::new("f>=2meg").map(|r| r.value), Some(2e6));
    }

    #[test]
    fn full_wildcards_anchor_at_both_ends() {
        assert!(wildcard_full("soic*3.9x4.9mm*p1.27mm*", "soic-8_3.9x4.9mm_p1.27mm"));
        assert!(wildcard_full("dip*w7.62mm*", "dip-8_w7.62mm"));
        assert!(!wildcard_full("dip*w7.62mm", "dip-8_w7.62mm_socket"));
        assert!(wildcard_full("so?c", "soic"));
        assert!(!wildcard_full("so?c", "soc"));
        assert!(wildcard_full("*", ""));
        assert!(wildcard_full("a*b*c", "axxbyyc") && !wildcard_full("a*b*c", "axxbyy"));
    }

    #[test]
    fn natural_order_reads_numbers_as_numbers_and_ignores_case() {
        let mut names = vec!["R_0805", "r_0603", "R_10", "R_9", "Conn_02x03", "Conn_01x10", "Conn_01x2"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["Conn_01x2", "Conn_01x10", "Conn_02x03", "R_9", "R_10", "r_0603", "R_0805"]);
        assert_eq!(natural_cmp("abc", "ABC"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "ab"), Ordering::Less, "the shorter first");
    }

    #[test]
    fn library_tables_give_the_library_descriptions() {
        let table = "(sym_lib_table\n\t(version 7)\n\t(lib (name \"Device\") (type \"KiCad\") (uri \"${KICAD10_SYMBOL_DIR}/Device.kicad_sym\") (options \"\") (descr \"Device symbols\"))\n\t(lib (name \"4xxx\") (type \"KiCad\") (uri \"x\") (options \"\") (descr \"4xxx series symbols\"))\n)\n";
        let d = parse_lib_table(table);
        assert_eq!(d.get("Device").map(String::as_str), Some("Device symbols"));
        assert_eq!(d.len(), 2);
    }

    const AMPS: &str = r##"(kicad_symbol_lib (version 20251024)
        (symbol "LM358" (property "Reference" "U") (property "Value" "LM358") (property "Footprint" "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm")
            (property "Datasheet" "http://ti.com/lm358.pdf") (property "Description" "Low-Power, Dual Operational Amplifier, DIP-8/SOIC-8") (property "ki_keywords" "dual opamp") (property "ki_fp_filters" "SOIC*3.9x4.9mm* DIP*W7.62mm*") (property "MPN" "LM358DR")
            (symbol "LM358_1_1" (pin input line (at 0 0 0) (length 1) (name "+") (number "3")) (pin input line (at 0 0 0) (length 1) (name "-") (number "2")) (pin output line (at 0 0 0) (length 1) (name "~") (number "1")))
            (symbol "LM358_2_1" (pin input line (at 0 0 0) (length 1) (name "+") (number "5")) (pin input line (at 0 0 0) (length 1) (name "-") (number "6")) (pin output line (at 0 0 0) (length 1) (name "~") (number "7")))
            (symbol "LM358_3_1" (pin power_in line (at 0 0 0) (length 1) (name "V+") (number "8")) (pin power_in line (at 0 0 0) (length 1) (name "V-") (number "4"))))
        (symbol "LM2904" (extends "LM358") (property "Value" "LM2904") (property "Description" "") (property "ki_keywords" "dual opamp low power"))
        (symbol "TL072" (property "Reference" "U") (property "Value" "TL072") (property "Description" "Dual low-noise JFET-input operational amplifier") (property "ki_keywords" "dual opamp jfet")
            (symbol "TL072_1_1" (pin input line (at 0 0 0) (length 1) (name "+") (number "3")))))"##;

    const DEVICE: &str = r##"(kicad_symbol_lib (version 20251024)
        (symbol "R" (property "Reference" "R") (property "Value" "R") (property "Description" "Resistor") (property "ki_keywords" "R res resistor")
            (symbol "R_1_1" (pin passive line (at 0 0 0) (length 1) (name "~") (number "1")) (pin passive line (at 0 0 0) (length 1) (name "~") (number "2"))))
        (symbol "R_Small" (property "Reference" "R") (property "Value" "R_Small") (property "Description" "Resistor, small symbol") (property "ki_keywords" "R resistor")
            (symbol "R_Small_1_1" (pin passive line (at 0 0 0) (length 1) (name "~") (number "1"))))
        (symbol "Opamp_Model" (property "Value" "Opamp_Model") (property "Description" "Behavioural op-amp model")
            (symbol "Opamp_Model_1_1" (pin input line (at 0 0 0) (length 1) (name "+") (number "1"))))
        (symbol "GND" (power) (property "Reference" "#PWR") (property "Value" "GND") (property "Description" "Power symbol: GND")
            (symbol "GND_1_1" (pin power_in line (at 0 0 0) (length 0) (name "GND") (number "1")))))"##;

    fn symbol_root(tag: &str) -> PathBuf {
        let root = temp_root(tag);
        std::fs::write(root.join("Amplifier_Operational.kicad_sym"), AMPS).unwrap();
        std::fs::write(root.join("Device.kicad_sym"), DEVICE).unwrap();
        root
    }

    fn names_of(v: &Value) -> Vec<String> {
        v["libraries"].as_array().unwrap().iter().flat_map(|l| l["items"].as_array().unwrap().iter().map(move |i| format!("{}:{}", l["name"].as_str().unwrap(), i["name"].as_str().unwrap()))).collect()
    }

    #[test]
    fn a_library_is_indexed_once_and_its_entries_carry_descriptions_in_natural_order() {
        let root = symbol_root("entries");
        let first = symbol_lib(&root, "Amplifier_Operational").unwrap();
        let again = symbol_lib(&root, "Amplifier_Operational").unwrap();
        assert!(Arc::ptr_eq(&first, &again), "the second ask is served from the cache");
        let e = entries_in(Kind::Symbol, &root, "Amplifier_Operational", &Filter::default());
        let names: Vec<&str> = e["entries"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["LM358", "LM2904", "TL072"], "natural order: 358 before 2904");
        assert_eq!(e["entries"][0]["description"], "Low-Power, Dual Operational Amplifier, DIP-8/SOIC-8");
        assert_eq!((e["entries"][0]["units"].clone(), e["entries"][0]["pins"].clone()), (json!(3), json!(8)));
        assert_eq!(e["entries"][1]["description"], "Low-Power, Dual Operational Amplifier, DIP-8/SOIC-8", "a derived symbol with no description of its own shows its base's");
        // the chooser fields the tree's extra columns show (`GetChooserFields`): every field, and the keywords as "Keywords"
        assert_eq!(e["entries"][0]["fields"]["Value"], "LM358");
        assert_eq!(e["entries"][0]["fields"]["Datasheet"], "http://ti.com/lm358.pdf");
        assert_eq!(e["entries"][0]["fields"]["Keywords"], "dual opamp");
        assert!(entries_in(Kind::Symbol, &root, "../x", &Filter::default())["error"].is_string());
        assert!(entries_in(Kind::Symbol, &root, "Missing", &Filter::default())["error"].is_string());
        // power symbols are left out on request
        let d = entries_in(Kind::Symbol, &root, "Device", &Filter { exclude_power: true, ..Default::default() });
        assert_eq!(d["entries"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap()).collect::<Vec<_>>(), ["Opamp_Model", "R", "R_Small"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_changed_library_is_indexed_again() {
        let root = temp_root("stale");
        let file = root.join("L.kicad_sym");
        std::fs::write(&file, r#"(kicad_symbol_lib (symbol "A" (property "Description" "first")))"#).unwrap();
        assert_eq!(symbol_lib(&root, "L").unwrap().summaries[0].description, "first");
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(&file, r#"(kicad_symbol_lib (symbol "A" (property "Description" "second")) (symbol "B"))"#).unwrap();
        let fresh = symbol_lib(&root, "L").unwrap();
        assert_eq!((fresh.summaries[0].description.as_str(), fresh.summaries.len()), ("second", 2));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A KiCad 10 folder library (`Amplifier_Operational.kicad_symdir/`, a file per symbol): the chooser indexes the symbols of all its files, a derived
    /// symbol takes its base's units, pins and description from the base's file, and the one picked is cut out of the folder's text and opened.
    #[test]
    fn a_folder_library_is_indexed_searched_and_opened_like_a_file() {
        let root = temp_root("symdir");
        let dir = root.join("Amplifier_Operational.kicad_symdir");
        std::fs::create_dir_all(&dir).unwrap();
        for sym in eda_kicad::scan_symbols(AMPS) {
            std::fs::write(dir.join(format!("{}.kicad_sym", sym.name)), format!("(kicad_symbol_lib (version 20251024)\n{}\n)\n", eda_kicad::symbol_text(AMPS, &sym))).unwrap();
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3, "a file per symbol");
        let e = entries_in(Kind::Symbol, &root, "Amplifier_Operational", &Filter::default());
        let names: Vec<&str> = e["entries"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["LM358", "LM2904", "TL072"], "{e}");
        assert_eq!((e["entries"][1]["units"].clone(), e["entries"][1]["pins"].clone()), (json!(3), json!(8)), "LM2904 has the units and pins of LM358, another file's");
        assert_eq!(e["entries"][1]["description"], "Low-Power, Dual Operational Amplifier, DIP-8/SOIC-8");
        index_all(Kind::Symbol, &root);
        assert_eq!(names_of(&search_in(Kind::Symbol, &root, "LM358", &Filter::default(), 50))[0], "Amplifier_Operational:LM358");
        let d = details_in(Kind::Symbol, &root, "Amplifier_Operational:LM2904");
        assert_eq!(d["symbol"]["pins"].as_array().map(Vec::len), Some(8), "{d}");
        assert_eq!(d["symbol"]["lib_id"], "Amplifier_Operational:LM2904");
        // a file of the folder changes: the index is built again
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(dir.join("TL072.kicad_sym"), r#"(kicad_symbol_lib (symbol "TL072" (property "Description" "changed")))"#).unwrap();
        assert_eq!(symbol_lib(&root, "Amplifier_Operational").unwrap().find("TL072").unwrap().description, "changed");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn search_finds_by_name_description_and_keyword_and_ranks_the_name_first() {
        let root = symbol_root("search");
        index_all(Kind::Symbol, &root);
        let by_name = search_in(Kind::Symbol, &root, "LM358", &Filter::default(), 50);
        assert_eq!(names_of(&by_name)[0], "Amplifier_Operational:LM358", "{by_name}");
        assert_eq!(names_of(&by_name).len(), 1, "no other symbol has that name, in its text or its keywords: {by_name}");
        // a derived symbol is found by its own keywords, and by the description it takes from its base
        let low = names_of(&search_in(Kind::Symbol, &root, "low power", &Filter::default(), 50));
        assert_eq!(low[0], "Amplifier_Operational:LM2904", "{low:?}");
        assert!(names_of(&search_in(Kind::Symbol, &root, "operational", &Filter::default(), 50)).contains(&"Amplifier_Operational:LM2904".to_string()));

        // description and keywords, in two libraries: 'opamp' is a keyword of the amplifiers and Device's model is only an op-amp by description
        let op = search_in(Kind::Symbol, &root, "op-amp", &Filter::default(), 50);
        assert_eq!(names_of(&op), ["Device:Opamp_Model"], "{op}");
        let opamp = search_in(Kind::Symbol, &root, "opamp", &Filter::default(), 50);
        let n = names_of(&opamp);
        assert!(n.contains(&"Amplifier_Operational:LM358".to_string()) && n.contains(&"Amplifier_Operational:TL072".to_string()), "{n:?}");
        assert!(n.contains(&"Device:Opamp_Model".to_string()), "the name of Device's model contains it too: {n:?}");
        // A keyword that is exactly the word (4 * 8 + 1 on top of 1) outscores a name that starts with it (Device:Opamp_Model: 16 + 16 on top of 1) while the
        // Value column is not shown (Item and Description, `loadColumnConfig`): the library with the best item first.
        assert_eq!((opamp["libraries"][0]["name"].clone(), opamp["libraries"][0]["score"].clone()), (json!("Amplifier_Operational"), json!(34)), "{opamp}");
        assert_eq!((opamp["libraries"][1]["name"].clone(), opamp["libraries"][1]["score"].clone()), (json!("Device"), json!(33)));
        // shown, the Value column's field is a term of weight 4 (`RebuildSearchTerms`; UI-LEFT-2 LL4): Opamp_Model's value starts with the word, 2 * 4 more
        let with_value = search_in(Kind::Symbol, &root, "opamp", &Filter::default().with_columns("Item\tDescription\tValue"), 50);
        assert_eq!((with_value["libraries"][0]["name"].clone(), with_value["libraries"][0]["score"].clone()), (json!("Device"), json!(41)), "{with_value}");
        // a user field in a shown column is searched; not shown, it is not
        let mpn = |cols: &str| names_of(&search_in(Kind::Symbol, &root, "lm358dr", &Filter::default().with_columns(cols), 50));
        assert!(mpn("Item\tMPN").contains(&"Amplifier_Operational:LM358".to_string()), "{:?}", mpn("Item\tMPN"));
        assert!(mpn("Item\tDescription").is_empty());
        let amps = opamp["libraries"][0]["items"].as_array().unwrap();
        assert_eq!(amps.iter().map(|i| i["name"].as_str().unwrap()).collect::<Vec<_>>(), ["LM358", "LM2904", "TL072"], "equal scores keep the library's own (natural) order");

        // a word of the description, case does not matter; both words must be found
        assert_eq!(names_of(&search_in(Kind::Symbol, &root, "JFET noise", &Filter::default(), 50)), ["Amplifier_Operational:TL072"]);
        assert!(names_of(&search_in(Kind::Symbol, &root, "jfet resistor", &Filter::default(), 50)).is_empty());
        // the library nickname is searched too, and the whole Lib:Name
        let by_lib = names_of(&search_in(Kind::Symbol, &root, "device", &Filter::default(), 50));
        assert!(by_lib.iter().all(|n| n.starts_with("Device:")) && by_lib.len() >= 3, "{by_lib:?}");
        assert_eq!(names_of(&search_in(Kind::Symbol, &root, "device:r_small", &Filter::default(), 50))[0], "Device:R_Small");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn search_leaves_power_symbols_out_on_request_and_caps_the_items_it_sends() {
        let root = symbol_root("cap");
        index_all(Kind::Symbol, &root);
        assert!(names_of(&search_in(Kind::Symbol, &root, "gnd", &Filter::default(), 50)).contains(&"Device:GND".to_string()));
        assert!(names_of(&search_in(Kind::Symbol, &root, "gnd", &Filter { exclude_power: true, ..Default::default() }, 50)).is_empty());
        let capped = search_in(Kind::Symbol, &root, "r", &Filter::default(), 2);
        assert_eq!(names_of(&capped).len(), 2);
        assert_eq!(capped["truncated"], true);
        assert!(capped["matches"].as_u64().unwrap() > 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Place Power's chooser (`FilterPowerSymbols( true )`): only power symbols, and its tree only the libraries that hold one.
    #[test]
    fn the_power_filter_keeps_only_power_symbols_and_names_the_libraries_that_hold_one() {
        let root = symbol_root("only_power");
        index_all(Kind::Symbol, &root);
        let only = Filter::parse("", "", "only");
        assert!(only.only_power && !only.exclude_power);
        let d = entries_in(Kind::Symbol, &root, "Device", &only);
        let names: Vec<&str> = d["entries"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["GND"]);
        assert_eq!(entries_in(Kind::Symbol, &root, "Amplifier_Operational", &only)["entries"].as_array().unwrap().len(), 0);
        assert_eq!(names_of(&search_in(Kind::Symbol, &root, "gnd", &only, 50)), ["Device:GND"]);
        assert!(names_of(&search_in(Kind::Symbol, &root, "lm358", &only, 50)).is_empty());
        // an empty query still names the libraries the tree shows; without the filter there is no such list
        let empty = search_in(Kind::Symbol, &root, "", &only, 50);
        assert_eq!(empty["power_libraries"], json!(["Device"]), "{empty}");
        assert!(search_in(Kind::Symbol, &root, "", &Filter::default(), 50).get("power_libraries").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_search_before_the_index_is_done_says_so_and_answers_with_what_is_indexed() {
        let root = symbol_root("progress");
        // nothing indexed and the thread not run: the answer is partial and honest. (The thread may win the race; either way the counts agree.)
        let first = search_in(Kind::Symbol, &root, "lm358", &Filter::default(), 50);
        assert_eq!(first["total"], 2);
        assert!(first["indexed"].as_u64().unwrap() <= 2);
        if first["status"] == "indexing" {
            assert!(first["indexed"].as_u64().unwrap() < 2 || first["libraries"].as_array().unwrap().len() <= 1);
        }
        // run to the end: ready, everything indexed
        index_all(Kind::Symbol, &root);
        let mut last = json!(null);
        for _ in 0..200 {
            last = search_in(Kind::Symbol, &root, "lm358", &Filter::default(), 50);
            if last["status"] == "ready" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!((last["status"].clone(), last["indexed"].clone(), last["total"].clone()), (json!("ready"), json!(2), json!(2)), "{last}");
        assert_eq!(names_of(&last)[0], "Amplifier_Operational:LM358");
        // an empty query only warms the index
        let warm = search_in(Kind::Symbol, &root, "", &Filter::default(), 50);
        assert_eq!(warm["libraries"].as_array().unwrap().len(), 0);
        assert_eq!(warm["matches"], 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn without_kicad_installed_there_is_nothing_to_search_and_nothing_goes_wrong() {
        let nowhere = std::env::temp_dir().join("eda-library-search-does-not-exist");
        for kind in [Kind::Symbol, Kind::Footprint] {
            let r = search_in(kind, &nowhere, "lm358", &Filter::default(), 50);
            assert_eq!((r["status"].clone(), r["total"].clone(), r["matches"].clone()), (json!("ready"), json!(0), json!(0)), "{r}");
            assert!(entries_in(kind, &nowhere, "Device", &Filter::default())["error"].is_string());
            assert!(details_in(kind, &nowhere, "Device:R")["error"].is_string());
        }
        assert!(library_descriptions(Kind::Symbol, &nowhere).is_empty());
    }

    #[test]
    fn details_cut_one_symbol_out_of_its_library_with_its_units_and_default_footprint() {
        let root = symbol_root("details");
        let d = details_in(Kind::Symbol, &root, "Amplifier_Operational:LM358");
        assert_eq!(d["id"], "Amplifier_Operational:LM358");
        assert_eq!(d["footprint"], "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm");
        assert_eq!((d["units"].clone(), d["pins"].clone()), (json!(3), json!(8)));
        assert_eq!(d["fp_filters"], "SOIC*3.9x4.9mm* DIP*W7.62mm*");
        assert_eq!(d["symbol"]["lib_id"], "Amplifier_Operational:LM358");
        assert_eq!(d["symbol"]["unit_count"], 3);
        assert_eq!(d["symbol"]["pins"].as_array().unwrap().len(), 8);
        // a derived symbol is drawn like its base
        let dd = details_in(Kind::Symbol, &root, "Amplifier_Operational:LM2904");
        assert_eq!(dd["symbol"]["pins"].as_array().unwrap().len(), 8);
        assert_eq!(dd["extends"], "LM358");
        assert!(details_in(Kind::Symbol, &root, "Amplifier_Operational:Nope")["error"].is_string());
        assert!(details_in(Kind::Symbol, &root, "Amplifier_Operational:../x")["error"].is_string());
        assert!(details_in(Kind::Symbol, &root, "nolib")["error"].is_string());
        let _ = std::fs::remove_dir_all(&root);
    }

    fn footprint_root(tag: &str) -> PathBuf {
        let root = temp_root(tag);
        let write = |lib: &str, name: &str, descr: &str, tags: &str, pads: &[&str]| {
            let dir = root.join(format!("{lib}.pretty"));
            std::fs::create_dir_all(&dir).unwrap();
            let pads: String = pads.iter().map(|n| format!("(pad \"{n}\" smd rect (at 0 0) (size 1 1) (layers \"F.Cu\" \"F.Mask\"))")).collect();
            std::fs::write(dir.join(format!("{name}.kicad_mod")), format!("(footprint \"{name}\" (descr \"{descr}\") (tags \"{tags}\") {pads} (fp_line (start 0 0) (end 1 1) (stroke (width 0.1) (type solid)) (layer \"F.SilkS\")))")).unwrap();
        };
        write("Package_SO", "SOIC-8_3.9x4.9mm_P1.27mm", "SOIC, 8 Pin", "SOIC SO", &["1", "2", "3", "4", "5", "6", "7", "8"]);
        write("Package_SO", "SOIC-16_3.9x9.9mm_P1.27mm", "SOIC, 16 Pin", "SOIC SO", &["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16"]);
        write("Package_DIP", "DIP-8_W7.62mm", "8-lead dip package, row spacing 7.62 mm (300 mils)", "THT DIP DIL PDIP 2.54mm 7.62mm 300mil", &["1", "2", "3", "4", "5", "6", "7", "8"]);
        write("MountingHole", "MountingHole_3.2mm_M3", "Mounting Hole 3.2mm, M3, no annular", "mountinghole M3", &[]);
        root
    }

    #[test]
    fn footprints_are_found_by_description_and_tags_and_narrowed_by_pin_count_and_footprint_filters() {
        let root = footprint_root("fp");
        index_all(Kind::Footprint, &root);
        let hole = search_in(Kind::Footprint, &root, "mounting hole", &Filter::default(), 50);
        assert_eq!(names_of(&hole), ["MountingHole:MountingHole_3.2mm_M3"], "{hole}");
        let tags = search_in(Kind::Footprint, &root, "pdip", &Filter::default(), 50);
        assert_eq!(names_of(&tags), ["Package_DIP:DIP-8_W7.62mm"], "a tag");
        let soic = names_of(&search_in(Kind::Footprint, &root, "soic", &Filter::default(), 50));
        assert_eq!(soic.len(), 2);
        // "Filter by pin count (8)"
        let eight = Filter::parse("8", "", "");
        assert_eq!(eight.pins, Some(8));
        assert_eq!(names_of(&search_in(Kind::Footprint, &root, "soic", &eight, 50)), ["Package_SO:SOIC-8_3.9x4.9mm_P1.27mm"]);
        assert_eq!(names_of(&search_in(Kind::Footprint, &root, "", &eight, 50)).len(), 0, "an empty query lists nothing; the tree asks for entries");
        // "Apply footprint filters (SOIC*3.9x4.9mm* DIP*W7.62mm*)"
        let both = Filter::parse("8", "SOIC*3.9x4.9mm* DIP*W7.62mm*", "");
        let e8 = entries_in(Kind::Footprint, &root, "Package_SO", &both);
        assert_eq!(e8["entries"].as_array().unwrap().len(), 1);
        let filtered = Filter::parse("", "dip*w7.62mm*", "");
        assert!(entries_in(Kind::Footprint, &root, "Package_SO", &filtered)["entries"].as_array().unwrap().is_empty());
        assert_eq!(entries_in(Kind::Footprint, &root, "Package_DIP", &filtered)["entries"].as_array().unwrap().len(), 1);
        // a pattern with a colon is matched against Lib:Name
        let qualified = Filter::parse("", "package_so:soic*", "");
        assert_eq!(entries_in(Kind::Footprint, &root, "Package_SO", &qualified)["entries"].as_array().unwrap().len(), 2);
        assert!(entries_in(Kind::Footprint, &root, "Package_DIP", &qualified)["entries"].as_array().unwrap().is_empty());
        // pin counts are the numbered pads
        let e = entries_in(Kind::Footprint, &root, "Package_SO", &Filter::default());
        assert_eq!((e["entries"][0]["name"].clone(), e["entries"][0]["pads"].clone()), (json!("SOIC-8_3.9x4.9mm_P1.27mm"), json!(8)));
        assert_eq!(e["entries"][1]["pads"], 16);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `FOOTPRINT_SELECT_WIDGET::UpdateList` -> pcbnew's `filterFootprints`: the symbol's pin count and footprint filters, every library, `Lib:Name`, capped;
    /// nothing at all for a symbol with no filters and no pins (`zero_filters`).
    #[test]
    fn the_footprint_selector_lists_the_footprints_of_the_symbols_pins_and_filters() {
        let root = footprint_root("fpselect");
        index_all(Kind::Footprint, &root);
        let list = |pins: &str, filters: &str, zero: bool, limit: usize| -> Vec<String> {
            let v = footprint_select_in(&root, &Filter::parse(pins, filters, ""), zero, limit);
            assert_eq!(v["status"], "ready", "{v}");
            v["footprints"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).collect()
        };
        assert_eq!(list("8", "SOIC*3.9x4.9mm* DIP*W7.62mm*", false, 400), ["Package_DIP:DIP-8_W7.62mm", "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm"], "the libraries in the trees' order");
        assert_eq!(list("8", "", true, 400), ["Package_DIP:DIP-8_W7.62mm", "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm"], "no filters: the pin count alone");
        assert_eq!(list("16", "", true, 400), ["Package_SO:SOIC-16_3.9x9.9mm_P1.27mm"]);
        assert!(list("", "", true, 400).is_empty(), "no filters and no pins: nothing (`m_zero_filter`)");
        assert_eq!(list("", "", false, 400).len(), 4, "zero_filters off: everything");
        assert_eq!(list("", "soic*", false, 400), ["Package_SO:SOIC-8_3.9x4.9mm_P1.27mm", "Package_SO:SOIC-16_3.9x9.9mm_P1.27mm"], "a filter alone; anchored, case folded; names in natural order");
        assert_eq!(list("", "package_dip:*", false, 400), ["Package_DIP:DIP-8_W7.62mm"], "a filter with a colon is matched against Lib:Name");
        assert_eq!(list("8", "", true, 1).len(), 1, "at most the limit");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// KiCad 25ec2335's `SCH_IO_KICAD_SEXPR_LIB_CACHE::Load`: a folder library with a file that does not read is listed with what reads, and its load error goes
    /// with the entries, the search's answer and the load-errors list (`GetLibraryLoadErrors`).
    #[test]
    fn a_folder_library_with_a_broken_file_lists_what_reads_and_says_it_loaded_with_errors() {
        let root = temp_root("symdir-broken");
        let dir = root.join("Amplifier_Operational.kicad_symdir");
        std::fs::create_dir_all(&dir).unwrap();
        for sym in eda_kicad::scan_symbols(AMPS) {
            std::fs::write(dir.join(format!("{}.kicad_sym", sym.name)), format!("(kicad_symbol_lib (version 20251024)\n{}\n)\n", eda_kicad::symbol_text(AMPS, &sym))).unwrap();
        }
        std::fs::write(dir.join("Zbroken.kicad_sym"), "(kicad_symbol_lib (version 20251024)\n\t(symbol \"Z\" (pin").unwrap();
        std::fs::write(root.join("Device.kicad_sym"), DEVICE).unwrap();
        let e = entries_in(Kind::Symbol, &root, "Amplifier_Operational", &Filter::default());
        assert_eq!(e["entries"].as_array().unwrap().len(), 3, "the good files are read: {e}");
        let message = e["load_error"].as_str().expect("the entries say the library loaded with errors");
        assert!(message.starts_with(&format!("Unable to read file '{}'\nExpecting ')'", dir.join("Zbroken.kicad_sym").display())), "{message}");
        assert!(message.ends_with("The library cannot be saved until these errors are fixed manually."));
        assert!(entries_in(Kind::Symbol, &root, "Device", &Filter::default()).get("load_error").is_none(), "a library that reads has none");
        index_all(Kind::Symbol, &root);
        let found = search_in(Kind::Symbol, &root, "lm358", &Filter::default(), 50);
        assert_eq!(found["load_errors"], json!([{ "lib": "Amplifier_Operational", "error": message }]), "{found}");
        let all = load_errors_in(Kind::Symbol, &root);
        assert_eq!(all["errors"].as_array().unwrap().len(), 1);
        assert_eq!(all["errors"][0]["lib"], "Amplifier_Operational");
        // the file fixed: the next read has no error
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::remove_file(dir.join("Zbroken.kicad_sym")).unwrap();
        assert!(entries_in(Kind::Symbol, &root, "Amplifier_Operational", &Filter::default()).get("load_error").is_none());
        assert!(load_errors_in(Kind::Symbol, &root)["errors"].as_array().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_footprints_details_carry_its_drawing() {
        let root = footprint_root("fpdetails");
        let d = details_in(Kind::Footprint, &root, "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm");
        assert_eq!(d["id"], "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm");
        assert_eq!(d["numbered_pads"], 8);
        assert_eq!(d["footprint"]["name"], "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm");
        assert_eq!(d["footprint"]["pads"].as_array().unwrap().len(), 8);
        assert!(!d["footprint"]["graphics"].as_array().unwrap().is_empty());
        assert!(details_in(Kind::Footprint, &root, "Package_SO:../../x")["error"].is_string());
        assert!(details_in(Kind::Footprint, &root, "Package_SO:Nope")["error"].is_string());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// KiCad.app's own libraries, when installed: the search the brief names ("LM358") finds the symbol, with its description, its three units and
    /// its default footprint, and the footprint chooser's search finds a mounting hole.
    #[test]
    fn the_installed_libraries_answer_lm358_and_a_mounting_hole() {
        let (sym_root, fp_root) = (Kind::Symbol.root(), Kind::Footprint.root());
        if !sym_root.is_dir() || !fp_root.is_dir() {
            eprintln!("no KiCad libraries installed; skipping");
            return;
        }
        let started = std::time::Instant::now();
        index_all(Kind::Symbol, &sym_root);
        eprintln!("indexed every symbol library in {:?}", started.elapsed());
        let r = search_in(Kind::Symbol, &sym_root, "LM358", &Filter { exclude_power: true, ..Default::default() }, SEARCH_LIMIT);
        let n = names_of(&r);
        assert_eq!(n[0], "Amplifier_Operational:LM358", "the exact name comes first: {n:?}");
        let lm = &r["libraries"][0]["items"][0];
        assert_eq!((lm["units"].clone(), lm["pins"].clone()), (json!(3), json!(8)));
        assert!(lm["description"].as_str().unwrap().to_lowercase().contains("operational amplifier"));
        // a description-only search: "dual operational amplifier" finds more than the name
        let by_description = names_of(&search_in(Kind::Symbol, &sym_root, "dual operational amplifier", &Filter::default(), SEARCH_LIMIT));
        assert!(by_description.contains(&"Amplifier_Operational:LM358".to_string()) && by_description.len() > 5, "{} matches", by_description.len());
        let d = details_in(Kind::Symbol, &sym_root, "Amplifier_Operational:LM358");
        assert_eq!(d["symbol"]["pins"].as_array().unwrap().len(), 8);

        index_all(Kind::Footprint, &fp_root);
        let hole = names_of(&search_in(Kind::Footprint, &fp_root, "MountingHole 3.2mm M3", &Filter::default(), 50));
        assert!(hole.contains(&"MountingHole:MountingHole_3.2mm_M3".to_string()), "{hole:?}");
        let soic8 = Filter::parse("8", "soic*3.9x4.9mm*p1.27mm*", "");
        let e = entries_in(Kind::Footprint, &fp_root, "Package_SO", &soic8);
        assert!(e["entries"].as_array().unwrap().iter().any(|i| i["name"] == "SOIC-8_3.9x4.9mm_P1.27mm"), "{e}");
        assert!(e["entries"].as_array().unwrap().iter().all(|i| i["pads"] == 8));
        // the libraries come with the descriptions of KiCad's tables
        assert!(!library_descriptions(Kind::Symbol, &sym_root).is_empty());
    }
}
