//! Netlist export -- port of `eeschema/netlist_exporters/netlist_exporter_xml.cpp`
//! (`NETLIST_EXPORTER_XML::makeRoot`, `makeDesignHeader`, `makeSymbols`,
//! `addSymbolFields`, `makeLibParts`, `makeLibraries`, `makeListOfNets`),
//! `netlist_exporter_kicad.cpp` (`NETLIST_EXPORTER_KICAD::Format`, the
//! `.net` s-expression flavour: the same `XNODE` tree, formatted by
//! `XNODE::Format` and prettified by `KICAD_FORMAT::Prettify`),
//! `netlist_exporter_base.cpp` (`findNextSymbol`: `#` references and
//! already-seen multi-unit references are skipped) and
//! `GetDefaultNetName` (`sch_pin.cpp`, for pins that no net names).
//!
//! The nets are the workspace's single netlist: `ConstraintModel::nets`,
//! which `board::load` has already overridden with `Design::nets` once the
//! schematic was hand-edited (PARITY-sch.md section 0). Nothing here
//! re-derives connectivity from geometry.
//!
//! Where the IR lacks what KiCad stores per symbol (a UUID, `exclude_from_*`
//! flags other than the library's, variants, groups, component classes,
//! text variables), the matching element is simply empty or absent, exactly
//! as KiCad writes it for a schematic that has none.

use std::collections::{BTreeMap, BTreeSet};

use eda_model::ir::{Design, SymbolInstance, TitleBlock};
use eda_model::{CheckResult, ConstraintModel, LibSymbol, Part, PinKind};

use crate::sch_plot::{sheet_list, SheetEntry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetlistFormat {
    /// `.net` -- `NETLIST_EXPORTER_KICAD` (`GNL_ALL | GNL_OPT_KICAD`).
    Kicad,
    /// `.xml` -- `NETLIST_EXPORTER_XML::WriteNetlist` (`GNL_ALL`).
    Xml,
}

impl NetlistFormat {
    pub fn extension(self) -> &'static str {
        match self {
            NetlistFormat::Kicad => "net",
            NetlistFormat::Xml => "xml",
        }
    }
}

/// Provenance for `makeDesignHeader` -- passed in so output is deterministic.
#[derive(Clone, Debug)]
pub struct NetlistMeta {
    /// `m_schematic->GetFileName()` -> `(source ...)`.
    pub source: String,
    /// `GetISO8601CurrentDateTime()`.
    pub date: String,
    /// `"Eeschema " + GetBuildVersion()` equivalent.
    pub tool: String,
    /// Project / root schematic base name (sheet file names derive from it).
    pub project: String,
    /// Title/date for a root sheet without a stored title block.
    pub fallback_title: String,
    /// Directory of the installed symbol libraries (`makeLibraries`'
    /// `GetFullURI`); `None` emits an empty `libraries` node.
    pub symbol_library_root: Option<std::path::PathBuf>,
}

// ------------------------------------------------------------------ XNODE

enum XChild {
    Node(XNode),
    Text(String),
}

/// `XNODE` (a `wxXmlNode` with ordered attributes and children).
struct XNode {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<XChild>,
}

impl XNode {
    /// `NETLIST_EXPORTER_XML::node`: text content only when non-empty.
    fn new(name: &str, text: &str) -> XNode {
        let mut n = XNode { name: name.to_string(), attrs: Vec::new(), children: Vec::new() };
        if !text.is_empty() {
            n.children.push(XChild::Text(text.to_string()));
        }
        n
    }
    fn attr(mut self, k: &str, v: &str) -> XNode {
        self.attrs.push((k.to_string(), v.to_string()));
        self
    }
    fn add(&mut self, c: XNode) {
        self.children.push(XChild::Node(c));
    }
    fn add_text(&mut self, t: &str) {
        self.children.push(XChild::Text(t.to_string()));
    }

    /// `OUTPUTFORMATTER::Quotes`.
    fn quote(s: &str) -> String {
        let mut r = String::from("\"");
        for c in s.chars() {
            match c {
                '\n' => r.push_str("\\n"),
                '\r' => r.push_str("\\r"),
                '\\' => r.push_str("\\\\"),
                '"' => r.push_str("\\\""),
                c => r.push(c),
            }
        }
        r.push('"');
        r
    }

    /// `XNODE::Format` for an element that has (`has_next`) / lacks a next sibling.
    fn format(&self, has_next: bool, out: &mut String) {
        out.push('(');
        out.push_str(&self.name);
        self.format_contents(out);
        out.push(')');
        if has_next {
            out.push('\n');
        }
    }

    /// `XNODE::FormatContents`.
    fn format_contents(&self, out: &mut String) {
        for (k, v) in &self.attrs {
            out.push_str(&format!(" ({} {})", k, XNode::quote(v)));
        }
        let mut first = true;
        for (i, c) in self.children.iter().enumerate() {
            match c {
                XChild::Node(n) => {
                    if first {
                        out.push('\n');
                    }
                    n.format(i + 1 < self.children.len(), out);
                }
                XChild::Text(t) => {
                    out.push(' ');
                    out.push_str(&XNode::quote(t));
                }
            }
            first = false;
        }
    }

    /// `wxXmlDocument::Save` with 2-space indentation.
    fn write_xml(&self, indent: usize, out: &mut String) {
        fn esc(s: &str, attr: bool) -> String {
            let mut r = String::new();
            for c in s.chars() {
                match c {
                    '&' => r.push_str("&amp;"),
                    '<' => r.push_str("&lt;"),
                    '>' => r.push_str("&gt;"),
                    '"' if attr => r.push_str("&quot;"),
                    '\n' if attr => r.push_str("&#10;"),
                    '\r' if attr => r.push_str("&#13;"),
                    '\t' if attr => r.push_str("&#9;"),
                    c => r.push(c),
                }
            }
            r
        }
        let pad = "  ".repeat(indent);
        out.push_str(&pad);
        out.push('<');
        out.push_str(&self.name);
        for (k, v) in &self.attrs {
            out.push_str(&format!(" {}=\"{}\"", k, esc(v, true)));
        }
        if self.children.is_empty() {
            out.push_str("/>\n");
            return;
        }
        out.push('>');
        let text_only = self.children.iter().all(|c| matches!(c, XChild::Text(_)));
        if text_only {
            for c in &self.children {
                if let XChild::Text(t) = c {
                    out.push_str(&esc(t, false));
                }
            }
        } else {
            out.push('\n');
            for c in &self.children {
                match c {
                    XChild::Node(n) => n.write_xml(indent + 1, out),
                    XChild::Text(t) => {
                        out.push_str(&"  ".repeat(indent + 1));
                        out.push_str(&esc(t, false));
                        out.push('\n');
                    }
                }
            }
            out.push_str(&pad);
        }
        out.push_str(&format!("</{}>\n", self.name));
    }
}

/// `KICAD_FORMAT::Prettify` (`FORMAT_MODE::NORMAL`): tab indentation, short
/// lists on one line, a list that contains lists broken one per line.
pub fn prettify(source: &str) -> String {
    const INDENT: char = '\t';
    const WRAP_THRESHOLD: usize = 72;
    let chars: Vec<char> = source.chars().collect();
    let mut formatted = String::with_capacity(source.len());
    let mut list_depth = 0usize;
    let mut last_non_ws = '\0';
    let mut in_quote = false;
    let mut has_inserted_space = false;
    let mut in_multi_line_list = false;
    let mut column = 0usize;
    let mut backslashes = 0usize;
    let is_ws = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r');

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // next non-whitespace character
        let next = chars[i..].iter().copied().find(|c| !is_ws(*c)).unwrap_or('\0');
        if is_ws(c) && !in_quote {
            if !has_inserted_space && list_depth > 0 && last_non_ws != '(' && next != ')' && next != '(' {
                if column < WRAP_THRESHOLD {
                    formatted.push(' ');
                    column += 1;
                } else {
                    formatted.push('\n');
                    formatted.extend(std::iter::repeat(INDENT).take(list_depth));
                    column = list_depth;
                    in_multi_line_list = true;
                }
                has_inserted_space = true;
            }
        } else {
            has_inserted_space = false;
            if c == '(' && !in_quote {
                if formatted.is_empty() {
                    formatted.push('(');
                    column += 1;
                } else {
                    formatted.push('\n');
                    formatted.extend(std::iter::repeat(INDENT).take(list_depth));
                    formatted.push('(');
                    column = list_depth + 1;
                }
                list_depth += 1;
            } else if c == ')' && !in_quote {
                list_depth = list_depth.saturating_sub(1);
                if last_non_ws == ')' || in_multi_line_list {
                    formatted.push('\n');
                    formatted.extend(std::iter::repeat(INDENT).take(list_depth));
                    formatted.push(')');
                    column = list_depth + 1;
                    in_multi_line_list = false;
                } else {
                    formatted.push(')');
                    column += 1;
                }
            } else {
                // The output formatter escapes double-quotes (like \"); a '"'
                // preceded by an odd number of backslashes does not end the string.
                if c == '\\' {
                    backslashes += 1;
                } else if c == '"' && backslashes % 2 == 0 {
                    in_quote = !in_quote;
                }
                if c != '\\' {
                    backslashes = 0;
                }
                formatted.push(c);
                column += 1;
            }
            last_non_ws = c;
        }
        i += 1;
    }
    formatted.push('\n');
    formatted
}

// -------------------------------------------------------------- StrNumCmp

/// `StrNumCmp` (`common/string_utils.cpp`): digit runs compare numerically.
pub fn str_num_cmp(a: &str, b: &str, ignore_case: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let s1: Vec<char> = a.chars().collect();
    let s2: Vec<char> = b.chars().collect();
    let (mut i, mut j) = (0usize, 0usize);
    while i < s1.len() && j < s2.len() {
        let mut c1 = s1[i];
        let mut c2 = s2[j];
        if c1.is_ascii_digit() && c2.is_ascii_digit() {
            let (mut nb1, mut nb2) = (0u64, 0u64);
            while i < s1.len() && s1[i].is_ascii_digit() {
                nb1 = nb1.saturating_mul(10).saturating_add(s1[i] as u64 - '0' as u64);
                i += 1;
            }
            while j < s2.len() && s2[j].is_ascii_digit() {
                nb2 = nb2.saturating_mul(10).saturating_add(s2[j] as u64 - '0' as u64);
                j += 1;
            }
            if nb1 < nb2 {
                return Less;
            }
            if nb1 > nb2 {
                return Greater;
            }
            c1 = s1.get(i).copied().unwrap_or('\0');
            c2 = s2.get(j).copied().unwrap_or('\0');
        }
        // Any numerical comparisons to here are identical.
        if ignore_case {
            if c1 != c2 {
                let (u1, u2) = (c1.to_uppercase().next().unwrap_or(c1), c2.to_uppercase().next().unwrap_or(c2));
                if u1 != u2 {
                    return if u1 < u2 { Less } else { Greater };
                }
            }
        } else if c1 < c2 {
            return Less;
        } else if c1 > c2 {
            return Greater;
        }
        if i < s1.len() {
            i += 1;
        }
        if j < s2.len() {
            j += 1;
        }
    }
    if i >= s1.len() && j < s2.len() {
        Less
    } else if i < s1.len() && j >= s2.len() {
        Greater
    } else {
        Equal
    }
}

// -------------------------------------------------------------- pin metadata

/// What `SCH_PIN` knows about one pin number of a part.
#[derive(Clone, Debug)]
struct PinMeta {
    number: String,
    /// `GetShownName` (`~` / empty both mean "no name").
    name: String,
    /// `GetCanonicalElectricalTypeName`.
    etype: String,
    unit: u32,
    /// Library-frame position, for the "all pins stacked" test.
    at: (i64, i64),
}

fn etype_from_kind(k: PinKind) -> &'static str {
    match k {
        PinKind::Power | PinKind::Ground => "power_in",
        PinKind::Signal => "bidirectional",
        PinKind::Passive => "passive",
        PinKind::Nc => "no_connect",
    }
}

fn shown(name: &str) -> String {
    if name == "~" {
        String::new()
    } else {
        name.to_string()
    }
}

/// Pins of `part`, resolved against its library symbol when there is one:
/// the library's number/name/type win; a part pin the library lacks (or a
/// part with a synthetic symbol) falls back to the part's own
/// `PinKind` -- the same fallback `check_erc` uses.
fn pin_metas(part: Option<&Part>, lib: Option<&LibSymbol>) -> Vec<PinMeta> {
    let mut out = Vec::new();
    if let Some(l) = lib {
        for p in &l.pins {
            let kind_nc = part.and_then(|pt| pt.pins.iter().find(|q| q.number == p.number)).map(|q| q.kind == PinKind::Nc).unwrap_or(false);
            out.push(PinMeta {
                number: p.number.clone(),
                name: shown(&p.name),
                etype: if kind_nc { "no_connect".to_string() } else { p.electrical_type.clone() },
                unit: p.unit,
                at: ((p.at.x * 10000.0).round() as i64, (-p.at.y * 10000.0).round() as i64),
            });
        }
        if let Some(pt) = part {
            for q in &pt.pins {
                if !l.pins.iter().any(|p| p.number == q.number) {
                    out.push(PinMeta { number: q.number.clone(), name: shown(q.name.as_deref().unwrap_or("")), etype: etype_from_kind(q.kind).to_string(), unit: 0, at: (0, 0) });
                }
            }
        }
    } else if let Some(pt) = part {
        for q in &pt.pins {
            out.push(PinMeta { number: q.number.clone(), name: shown(q.name.as_deref().unwrap_or("")), etype: etype_from_kind(q.kind).to_string(), unit: 0, at: (0, 0) });
        }
    }
    out
}

/// `SCH_PIN::GetDefaultNetName` for a pin no net names (`Net-(...)`), or a
/// no-connect one (`unconnected-(...)`).
fn default_net_name(reference: &str, pin: &PinMeta, unconnected: bool, has_multiple: bool) -> String {
    let mut name = String::from(if unconnected { "unconnected-(" } else { "Net-(" });
    if reference.ends_with('?') {
        name.push_str(&format!("{}-Pad{})", crate::duid(&format!("sym:{reference}")), pin.number));
    } else if !pin.name.is_empty() && pin.name != pin.number {
        name.push_str(reference);
        name.push('-');
        name.push_str(&pin.name);
        if unconnected || has_multiple {
            name.push_str(&format!("-Pad{}", pin.number));
        }
        name.push(')');
    } else {
        name.push_str(reference);
        name.push_str(&format!("-Pad{})", pin.number));
    }
    name
}

// ----------------------------------------------------------------- the export

struct Comp<'a> {
    sym: &'a SymbolInstance,
    extra_units: Vec<&'a SymbolInstance>,
    part: Option<&'a Part>,
    lib: Option<LibSymbol>,
    lib_nick: String,
    lib_part: String,
}

/// Library nickname and symbol name of an instance (`GetLibId()`); a
/// synthetic `eda:<ref>` id keeps its own prefix, matching the exported
/// `.kicad_sch`.
fn lib_ident(sym: &SymbolInstance) -> (String, String) {
    let id = if sym.lib_id.is_empty() { format!("eda:{}", sym.id) } else { sym.lib_id.clone() };
    match id.split_once(':') {
        Some((n, p)) => (n.to_string(), p.to_string()),
        None => (String::new(), id),
    }
}

fn title_of(entry: &SheetEntry, meta: &NetlistMeta) -> TitleBlock {
    entry.section.title_block.clone().unwrap_or_else(|| TitleBlock {
        title: if entry.ids.is_empty() { meta.fallback_title.clone() } else { String::new() },
        date: String::new(),
        rev: String::new(),
        company: String::new(),
        comments: Vec::new(),
    })
}

/// `NETLIST_EXPORTER_XML::makeDesignHeader`.
fn make_design_header(sheets: &[SheetEntry], meta: &NetlistMeta) -> XNode {
    let mut d = XNode::new("design", "");
    d.add(XNode::new("source", &meta.source));
    d.add(XNode::new("date", &meta.date));
    d.add(XNode::new("tool", &meta.tool));
    for (i, sheet) in sheets.iter().enumerate() {
        let mut xs = XNode::new("sheet", "").attr("number", &(i + 1).to_string()).attr("name", &sheet.path_human_readable()).attr("tstamps", &sheet.path_as_string());
        let tb = title_of(sheet, meta);
        let mut xt = XNode::new("title_block", "");
        xt.add(XNode::new("title", &tb.title));
        xt.add(XNode::new("company", &tb.company));
        xt.add(XNode::new("rev", &tb.rev));
        xt.add(XNode::new("date", &tb.date));
        xt.add(XNode::new("source", &sheet.file));
        for n in 0..9 {
            xt.add(XNode::new("comment", "").attr("number", &(n + 1).to_string()).attr("value", tb.comments.get(n).map(String::as_str).unwrap_or("")));
        }
        xs.add(xt);
        d.add(xs);
    }
    d
}

/// `NETLIST_EXPORTER_XML::makeSymbols` (+ `addSymbolFields`).
fn make_symbols<'a>(sheets: &'a [SheetEntry], model: &'a ConstraintModel, opts_kicad: bool, meta: &NetlistMeta, comps_out: &mut Vec<Comp<'a>>) -> XNode {
    let mut xcomps = XNode::new("components", "");
    let mut refs_found: BTreeSet<String> = BTreeSet::new();
    for sheet in sheets {
        // ordered by StrNumCmp( ref ); on a repeated reference the lowest unit
        // is the primary and the rest are `extra_units` (KiCad keeps the lowest UUID).
        let mut by_ref: BTreeMap<String, Vec<&SymbolInstance>> = BTreeMap::new();
        for s in &sheet.section.symbols {
            by_ref.entry(s.id.clone()).or_default().push(s);
        }
        let mut refs: Vec<String> = by_ref.keys().cloned().collect();
        refs.sort_by(|a, b| str_num_cmp(a, b, false));
        for r in refs {
            let mut instances = by_ref.remove(&r).unwrap();
            instances.sort_by_key(|s| s.unit);
            let sym = instances[0];
            let extra: Vec<&SymbolInstance> = instances[1..].to_vec();
            // findNextSymbol: pseudo/virtual symbols are not in the netlist.
            if r.starts_with('#') {
                continue;
            }
            let part = model.part(&r);
            let lib = if sym.lib_id.is_empty() || eda_model::is_synthetic_lib_id(&sym.lib_id) { None } else { model.symbol_of(&sym.lib_id) };
            // A multi-unit reference is only emitted once across the whole hierarchy.
            if lib.as_ref().map(|l| l.unit_count > 1).unwrap_or(false) && !refs_found.insert(r.clone()) {
                continue;
            }
            // forBoard (GNL_OPT_KICAD): symbols excluded from the board are skipped.
            let on_board = lib.as_ref().map(|l| l.on_board).unwrap_or(true);
            let in_bom = lib.as_ref().map(|l| l.in_bom).unwrap_or(true);
            if opts_kicad && !on_board {
                continue;
            }
            let (nick, lpart) = lib_ident(sym);

            let mut xcomp = XNode::new("comp", "").attr("ref", &r);
            // addSymbolFields: single-unit path (and the lowest-unit-wins scavenger
            // for multi-unit, which with one IR field set per reference is the same value).
            let value = [sym.value.clone(), part.and_then(|p| p.value.clone()).unwrap_or_default()].into_iter().find(|v| !v.is_empty()).unwrap_or_default();
            let footprint = [sym.footprint.clone(), part.and_then(|p| p.footprint.clone()).unwrap_or_default()].into_iter().find(|v| !v.is_empty()).unwrap_or_default();
            let datasheet = [sym.datasheet.clone(), part.and_then(|p| p.datasheet.clone()).unwrap_or_default(), lib.as_ref().map(|l| l.datasheet.clone()).unwrap_or_default()].into_iter().find(|v| !v.is_empty()).unwrap_or_default();
            let description = lib.as_ref().map(|l| l.description.clone()).unwrap_or_default();
            // User fields: the part metadata the studio's BOM also carries.
            let mut user_fields: Vec<(String, String)> = Vec::new();
            if let Some(m) = part.and_then(|p| p.mpn.clone()).filter(|m| !m.is_empty()) {
                user_fields.push(("MPN".into(), m));
            }
            if let Some(l) = part.and_then(|p| p.lcsc.clone()).filter(|m| !m.is_empty()) {
                user_fields.push(("LCSC".into(), l));
            }
            xcomp.add(XNode::new("value", if value.is_empty() { "~" } else { &value }));
            if !footprint.is_empty() {
                xcomp.add(XNode::new("footprint", &footprint));
            }
            if !datasheet.is_empty() {
                xcomp.add(XNode::new("datasheet", &datasheet));
            }
            if !description.is_empty() {
                xcomp.add(XNode::new("description", &description));
            }
            let mut xfields = XNode::new("fields", "");
            for (k, v) in &user_fields {
                xfields.add(XNode::new("field", v).attr("name", k));
            }
            xfields.add(XNode::new("field", &footprint).attr("name", "Footprint"));
            xfields.add(XNode::new("field", &datasheet).attr("name", "Datasheet"));
            xfields.add(XNode::new("field", &description).attr("name", "Description"));
            xcomp.add(xfields);

            let xlib = XNode::new("libsource", "").attr("lib", &nick).attr("part", &lpart).attr("description", &description);
            xcomp.add(xlib);

            for (k, v) in &user_fields {
                xcomp.add(XNode::new("property", "").attr("name", k).attr("value", v));
            }
            // sheet.Last()->GetFields(): Sheetname / Sheetfile
            xcomp.add(XNode::new("property", "").attr("name", "Sheetname").attr("value", sheet.names.last().map(String::as_str).unwrap_or("")));
            xcomp.add(XNode::new("property", "").attr("name", "Sheetfile").attr("value", &sheet.file));
            if !in_bom {
                xcomp.add(XNode::new("property", "").attr("name", "exclude_from_bom"));
            }
            if !on_board {
                xcomp.add(XNode::new("property", "").attr("name", "exclude_from_board"));
            }
            xcomp.add(XNode::new("sheetpath", "").attr("names", &sheet.path_human_readable()).attr("tstamps", &sheet.path_as_string()));

            // tstamps: the extra units' UUIDs first, then the primary one; a space after
            // each extra one in the `.xml` flavour (wxXmlDocument::Save has no XNODE::Format).
            let mut xunits = XNode::new("tstamps", "");
            for e in &extra {
                let mut u = crate::duid(&format!("sym:{}:u{}", e.id, e.unit));
                if !opts_kicad {
                    u.push(' ');
                }
                xunits.add_text(&u);
            }
            xunits.add_text(&crate::duid(&format!("sym:{}", sym.id)));
            xcomp.add(xunits);

            // units: every unit slot of the library symbol with its pin numbers.
            let mut xunit_info = XNode::new("units", "");
            if let Some(l) = &lib {
                for u in 1..=l.unit_count.max(1) {
                    let mut pins: Vec<&eda_model::LibPin> = l.pins.iter().filter(|p| p.unit == 0 || p.unit == u).collect();
                    pins.sort_by_key(|p| ((p.at.x * 10000.0).round() as i64, (-p.at.y * 10000.0).round() as i64));
                    let mut seen = BTreeSet::new();
                    let mut xpins = XNode::new("pins", "");
                    for p in pins {
                        if !p.number.is_empty() && seen.insert(p.number.clone()) {
                            xpins.add(XNode::new("pin", "").attr("num", &p.number));
                        }
                    }
                    let mut xu = XNode::new("unit", "").attr("name", &unit_letter(u));
                    xu.add(xpins);
                    xunit_info.add(xu);
                }
            } else if let Some(pt) = part {
                // A synthetic (box) symbol has one unit holding every pin, in part order.
                let mut seen = BTreeSet::new();
                let mut xpins = XNode::new("pins", "");
                for p in &pt.pins {
                    if !p.number.is_empty() && seen.insert(p.number.clone()) {
                        xpins.add(XNode::new("pin", "").attr("num", &p.number));
                    }
                }
                let mut xu = XNode::new("unit", "").attr("name", &unit_letter(1));
                xu.add(xpins);
                xunit_info.add(xu);
            }
            xcomp.add(xunit_info);
            xcomps.add(xcomp);
            comps_out.push(Comp { sym, extra_units: extra, part, lib, lib_nick: nick, lib_part: lpart });
        }
    }
    let _ = meta;
    xcomps
}

/// `LIB_SYMBOL::LetterSubReference( unit, 'A' )`.
fn unit_letter(unit: u32) -> String {
    char::from_u32('A' as u32 + (unit.max(1) - 1) % 26).map(|c| c.to_string()).unwrap_or_default()
}

/// `NETLIST_EXPORTER_XML::makeLibParts`: one `libpart` per distinct library
/// symbol, ordered by lib id, with its pins sorted by number (case-insensitive
/// `StrNumCmp`) and the duplicates (multi-unit repeats, De Morgan) removed.
fn make_libparts(comps: &[Comp]) -> (XNode, BTreeSet<String>) {
    let mut xparts = XNode::new("libparts", "");
    let mut libraries: BTreeSet<String> = BTreeSet::new();
    let mut seen: BTreeMap<(String, String), (Option<&LibSymbol>, Vec<PinMeta>, &Comp)> = BTreeMap::new();
    for c in comps {
        seen.entry((c.lib_nick.clone(), c.lib_part.clone())).or_insert_with(|| (c.lib.as_ref(), pin_metas(c.part, c.lib.as_ref()), c));
    }
    for ((nick, part_name), (lib, metas, comp)) in &seen {
        if !nick.is_empty() {
            libraries.insert(nick.clone());
        }
        let mut xp = XNode::new("libpart", "").attr("lib", nick).attr("part", part_name);
        let description = lib.map(|l| l.description.clone()).unwrap_or_default();
        let datasheet = lib.map(|l| l.datasheet.clone()).unwrap_or_default();
        if !description.is_empty() {
            xp.add(XNode::new("description", &description));
        }
        if !datasheet.is_empty() {
            xp.add(XNode::new("docs", &datasheet));
        }
        let mut xf = XNode::new("fields", "");
        let prefix = lib.map(|l| l.reference_prefix.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| comp.sym.id.chars().take_while(|c| c.is_alphabetic()).collect());
        xf.add(XNode::new("field", &prefix).attr("name", "Reference"));
        xf.add(XNode::new("field", part_name).attr("name", "Value"));
        xf.add(XNode::new("field", "").attr("name", "Footprint"));
        xf.add(XNode::new("field", &datasheet).attr("name", "Datasheet"));
        xf.add(XNode::new("field", &description).attr("name", "Description"));
        xp.add(xf);
        let mut pins = metas.clone();
        pins.sort_by(|a, b| str_num_cmp(&a.number, &b.number, true));
        pins.dedup_by(|b, a| a.number == b.number);
        if !pins.is_empty() {
            let mut xpins = XNode::new("pins", "");
            for p in &pins {
                xpins.add(XNode::new("pin", "").attr("num", &p.number).attr("name", &p.name).attr("type", &p.etype));
            }
            xp.add(xpins);
        }
        xparts.add(xp);
    }
    (xparts, libraries)
}

/// `NETLIST_EXPORTER_XML::makeLibraries`.
fn make_libraries(libs: &BTreeSet<String>, root: Option<&std::path::Path>) -> XNode {
    let mut xl = XNode::new("libraries", "");
    for nick in libs {
        let uri = root.and_then(|r| crate::find_symbol_library_file(r, nick));
        if let Some(path) = uri {
            let mut x = XNode::new("library", "").attr("logical", nick);
            x.add(XNode::new("uri", &path.to_string_lossy()));
            xl.add(x);
        }
    }
    xl
}

struct NetNode {
    reference: String,
    number: String,
    name: String,
    etype: String,
    at: (i64, i64),
}

struct NetRecord {
    name: String,
    class: String,
    has_no_connect: bool,
    nodes: Vec<NetNode>,
}

/// `NETLIST_EXPORTER_XML::makeListOfNets`.
fn make_list_of_nets(model: &ConstraintModel, comps: &[Comp], sheets: &[SheetEntry]) -> XNode {
    // pin metadata per reference, and which units are placed
    let mut metas: BTreeMap<String, Vec<PinMeta>> = BTreeMap::new();
    let mut placed_units: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    for c in comps {
        metas.entry(c.sym.id.clone()).or_insert_with(|| pin_metas(c.part, c.lib.as_ref()));
        let u = placed_units.entry(c.sym.id.clone()).or_default();
        u.insert(c.sym.unit);
        for e in &c.extra_units {
            u.insert(e.unit);
        }
    }
    let lookup = |reference: &str, number: &str| -> Option<&PinMeta> { metas.get(reference).and_then(|v| v.iter().find(|p| p.number == number)) };
    let class_of = |net: &str| model.board.class_of(net).map(|c| c.name.clone()).unwrap_or_else(|| "Default".to_string());

    let mut nets: Vec<NetRecord> = Vec::new();
    let mut used: BTreeSet<(String, String)> = BTreeSet::new();
    // no-connect flags: `SCH_NO_CONNECT` on a pin ("REF.PIN")
    let mut nc_pins: BTreeSet<String> = BTreeSet::new();
    for s in sheets {
        for nc in &s.section.no_connects {
            if !nc.pin.is_empty() {
                nc_pins.insert(nc.pin.clone());
            }
        }
    }
    for net in &model.nets {
        if net.pins.is_empty() {
            continue; // `subgraphs.empty()`
        }
        let mut rec = NetRecord { name: net.name.clone(), class: class_of(&net.name), has_no_connect: false, nodes: Vec::new() };
        for pr in &net.pins {
            let Some((r, n)) = pr.rsplit_once('.') else { continue };
            used.insert((r.to_string(), n.to_string()));
            if nc_pins.contains(pr) {
                rec.has_no_connect = true;
            }
            let meta = lookup(r, n);
            rec.nodes.push(NetNode { reference: r.to_string(), number: n.to_string(), name: meta.map(|m| m.name.clone()).unwrap_or_default(), etype: meta.map(|m| m.etype.clone()).unwrap_or_else(|| "passive".to_string()), at: meta.map(|m| m.at).unwrap_or((0, 0)) });
        }
        nets.push(rec);
    }
    // Every placed pin that no net names is its own one-node net, named by
    // `GetDefaultNetName` (`unconnected-(...)` for a no-connect one).
    for c in comps {
        let r = &c.sym.id;
        let Some(ms) = metas.get(r) else { continue };
        let units = placed_units.get(r).cloned().unwrap_or_default();
        let mut done: BTreeSet<String> = BTreeSet::new();
        for p in ms {
            if !(p.unit == 0 || units.contains(&p.unit)) || used.contains(&(r.clone(), p.number.clone())) || !done.insert(p.number.clone()) {
                continue;
            }
            let unconnected = p.etype == "no_connect" || nc_pins.contains(&format!("{}.{}", r, p.number));
            let has_multiple = ms.iter().any(|q| q.name == p.name && q.number != p.number && (q.etype == "no_connect") == unconnected);
            let name = default_net_name(r, p, unconnected, has_multiple);
            nets.push(NetRecord { name, class: "Default".to_string(), has_no_connect: unconnected, nodes: vec![NetNode { reference: r.clone(), number: p.number.clone(), name: p.name.clone(), etype: p.etype.clone(), at: p.at }] });
        }
    }

    // Netlist ordering: net name (StrNumCmp), then ref des, then pin number.
    nets.sort_by(|a, b| str_num_cmp(&a.name, &b.name, false));
    let mut xnets = XNode::new("nets", "");
    for (i, rec) in nets.iter_mut().enumerate() {
        rec.nodes.sort_by(|a, b| if a.reference == b.reference { a.number.cmp(&b.number) } else { a.reference.cmp(&b.reference) });
        // Some duplicates can exist (multi-unit parts with duplicated pins): remove them.
        rec.nodes.dedup_by(|b, a| a.reference == b.reference && a.number == b.number);
        // Nets with only one pin are implicitly taken to be stacked.
        let all_stacked = rec.nodes.len() <= 1 || rec.nodes[1..].iter().all(|n| n.reference == rec.nodes[0].reference && n.at == rec.nodes[0].at && n.name == rec.nodes[0].name);
        let mut xnet: Option<XNode> = None;
        for n in &rec.nodes {
            // Skip power symbols and virtual symbols
            if n.reference.starts_with('#') {
                continue;
            }
            let x = xnet.get_or_insert_with(|| XNode::new("net", "").attr("code", &(i + 1).to_string()).attr("name", &rec.name).attr("class", &rec.class));
            let mut node = XNode::new("node", "").attr("ref", &n.reference).attr("pin", &n.number);
            let full = if n.name.is_empty() { n.number.clone() } else { format!("{}_{}", n.name, n.number) };
            if !n.name.is_empty() {
                node = node.attr("pinfunction", &full);
            }
            let mut ty = n.etype.clone();
            if rec.has_no_connect && (rec.nodes.len() == 1 || all_stacked) {
                ty.push_str("+no_connect");
            }
            node = node.attr("pintype", &ty);
            x.add(node);
        }
        if let Some(x) = xnet {
            xnets.add(x);
        }
    }
    xnets
}

/// `NETLIST_EXPORTER_XML::makeRoot` + the per-format writer.
pub fn export_netlist(design: &Design, model: &ConstraintModel, format: NetlistFormat, meta: &NetlistMeta) -> Result<String, Vec<CheckResult>> {
    if design.schematic.is_none() {
        return Err(vec![CheckResult::fail("netlist.no_schematic", "design", "design has no schematic section to export a netlist from")]);
    }
    let sheets = sheet_list(design, &meta.project);
    let opts_kicad = format == NetlistFormat::Kicad;

    let mut root = XNode::new("export", "").attr("version", "E");
    root.add(make_design_header(&sheets, meta));
    let mut comps: Vec<Comp> = Vec::new();
    let xcomps = make_symbols(&sheets, model, opts_kicad, meta, &mut comps);
    root.add(xcomps);
    if opts_kicad {
        // makeGroups / makeVariants: the IR has neither.
        root.add(XNode::new("groups", ""));
        root.add(XNode::new("variants", ""));
    }
    let (libparts, libs) = make_libparts(&comps);
    root.add(libparts);
    root.add(make_libraries(&libs, meta.symbol_library_root.as_deref()));
    root.add(make_list_of_nets(model, &comps, &sheets));

    Ok(match format {
        NetlistFormat::Kicad => {
            let mut raw = String::new();
            root.format(false, &mut raw);
            prettify(&raw)
        }
        NetlistFormat::Xml => {
            let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
            root.write_xml(0, &mut s);
            s
        }
    })
}

#[cfg(test)]
#[path = "netlist_tests.rs"]
mod tests;
