//! KiCad's netlist readers, as Import Netlist reads a file (`PCB_EDIT_FRAME::ReadNetlistFromFile`, `pcbnew/netlist_reader/netlist.cpp` at 8303b2ad):
//! `NETLIST_READER::GetNetlistReader` guesses the format from the file's lines (`GuessNetlistFileType`) and hands it to `KICAD_NETLIST_READER` (the
//! s-expression `.net` Eeschema writes, `kicad_netlist_reader.cpp`'s `KICAD_NETLIST_PARSER`) or `LEGACY_NETLIST_READER` (the `# EESchema Netlist`
//! format and OrCAD PCB2's `( {` one, `legacy_netlist_reader.cpp`). What they make is a `NETLIST` of `COMPONENT`s (`pcb_netlist.cpp`), here
//! [`Netlist`] and [`NetlistComponent`], which `BOARD_NETLIST_UPDATER` (`eda_ops::netlist_update`) lays onto the board.
//!
//! **The `.cmp` file is not read:** `CMP_READER` is only made when `GetNetlistReader` is given a footprint association file, and Import Netlist passes
//! `wxEmptyString` (8303b2ad reads none there), so a netlist's own `(footprint ..)` (or `$noname`) is what each component has.
//!
//! **Differs:** the s-expression file is read into a tree first (`crate::sexpr`) and walked list by list, where KiCad's parser reads token by token. The
//! file's order is kept (a `(nets ..)` read before `(components ..)` finds no component, as in KiCad), and so is what each list takes: a field's value is its
//! quoted string (`T_STRING`), a bare one is skipped; a list KiCad skips is skipped. A malformed file is refused with our reader's reason (`Error loading
//! netlist.` and the line), not the `DSNLEXER`'s own words. A KIID is kept as the file's text, lower-cased, a legacy 8-digit time stamp widened as
//! `KIID::KIID( const std::string& )` widens it; a text that is neither stays as it is where KiCad would make a random one (it matches nothing either way).

use crate::sexpr::{self, Sexpr};
use std::collections::{BTreeMap, BTreeSet};

/// `COMPONENT_NET`: a pin of a component and the net it is on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComponentNet {
    pub pin: String,
    pub net: String,
    pub pin_function: String,
    pub pin_type: String,
}

impl ComponentNet {
    /// `COMPONENT_NET::IsValid`: the pin has a name.
    pub fn is_valid(&self) -> bool {
        !self.pin.is_empty()
    }
}

/// `COMPONENT::UNIT_INFO`: a unit of a multi-unit symbol and the pins drawn on it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitInfo {
    pub name: String,
    pub pins: Vec<String>,
}

/// `COMPONENT_VARIANT`: what a design variant changes on a component. A flag is `Some` where the variant says it (`m_hasDnp` ..).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComponentVariant {
    pub name: String,
    pub dnp: Option<bool>,
    pub excluded_from_bom: Option<bool>,
    pub excluded_from_sim: Option<bool>,
    pub excluded_from_pos_files: Option<bool>,
    /// `m_fields`, an `ordered_map`: in the order first seen, a later value of the same name in its place.
    pub fields: Vec<(String, String)>,
}

/// `NETLIST_GROUP`: a schematic group and the symbols in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetlistGroup {
    pub name: String,
    pub uuid: String,
    /// The design block it was placed from (`LIB_ID`, formatted).
    pub lib_id: String,
    pub members: Vec<String>,
}

/// `COMPONENT`: one symbol of the schematic (all its units), as the netlist gives it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetlistComponent {
    /// `m_fpid`, formatted (`LIB_ID::Format`): `Lib:Name`, a bare `Name` for a legacy id, empty for none.
    pub fpid: String,
    pub reference: String,
    pub value: String,
    /// `m_path`: the uuids of the sheets down to the symbol's (`(sheetpath (tstamps ..))`).
    pub path: Vec<String>,
    /// `m_kiids`: the uuid of each of the symbol's units (`(tstamps ..)`).
    pub kiids: Vec<String>,
    /// `(libsource (part ..))` and `(lib ..)`.
    pub name: String,
    pub library: String,
    /// `m_properties`, a `std::map`: by name.
    pub properties: BTreeMap<String, String>,
    /// `m_fields`, an `ordered_map`.
    pub fields: Vec<(String, String)>,
    /// `(sheetpath (names ..))`.
    pub human_sheet_path: String,
    pub component_classes: BTreeSet<String>,
    pub duplicate_pads_are_jumpers: bool,
    pub jumper_pad_groups: Vec<BTreeSet<String>>,
    pub units: Vec<UnitInfo>,
    /// `m_variants`, a case-insensitive map: by name.
    pub variants: Vec<ComponentVariant>,
    pub nets: Vec<ComponentNet>,
    /// `m_footprintFilters`, from the component's `libpart` (`(footprints (fp ..))`) or the legacy file's filter section.
    pub footprint_filters: Vec<String>,
    pub pin_count: usize,
    /// `m_group`: the index of the group in [`Netlist::groups`] (`ApplyGroupMembership`).
    pub group: Option<usize>,
}

impl NetlistComponent {
    /// `COMPONENT::GetNet( aPinName )`: the net of the pin of that number, a stacked pin (`[1-3]`, `ExpandStackedPinNotation`) standing for each pad it
    /// lists; the empty net (no pin name) when there is none.
    pub fn net(&self, pin: &str) -> ComponentNet {
        for n in &self.nets {
            if n.pin == pin {
                return n.clone();
            }
            let (expanded, _) = eda_model::fp_units::expand_stacked_pin_notation(&n.pin);
            if expanded.iter().any(|p| p == pin) {
                return n.clone();
            }
        }
        ComponentNet::default()
    }

    /// `m_properties.count( aName ) > 0`.
    pub fn has_property(&self, name: &str) -> bool {
        self.properties.contains_key(name)
    }

    /// `COMPONENT::IsLibSource`.
    fn is_lib_source(&self, library: &str, name: &str) -> bool {
        library == self.library && name == self.name
    }

    /// The footprint path a footprint linked to this component has (`m_path` + `m_kiids.front()`, `COMPONENT::SetFootprint`), as `KIID_PATH::AsString` writes it.
    pub fn footprint_path(&self) -> String {
        path_string(self.path.iter().chain(self.kiids.first()))
    }
}

/// `KIID_PATH::AsString`: `/` before each uuid (empty for none).
pub fn path_string<'a>(steps: impl IntoIterator<Item = &'a String>) -> String {
    steps.into_iter().map(|s| format!("/{s}")).collect()
}

/// `NETLIST`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Netlist {
    pub components: Vec<NetlistComponent>,
    pub groups: Vec<NetlistGroup>,
    /// `(variants (variant (name ..) (description ..)))`: the design variants, in file order.
    pub variants: Vec<(String, String)>,
}

impl Netlist {
    /// `NETLIST::GetComponentByReference`: the first component of that reference.
    pub fn by_reference(&self, reference: &str) -> Option<&NetlistComponent> {
        self.components.iter().find(|c| c.reference == reference)
    }

    fn by_reference_mut(&mut self, reference: &str) -> Option<&mut NetlistComponent> {
        self.components.iter_mut().find(|c| c.reference == reference)
    }

    /// `NETLIST::GetComponentByPath`: the component whose sheet path is `path` without its last step and that has that step among its units' uuids.
    pub fn by_path(&self, path: &str) -> Option<&NetlistComponent> {
        let steps = path_steps(path);
        let (last, base) = steps.split_last()?;
        self.components.iter().find(|c| c.path == base && c.kiids.iter().any(|k| k == last))
    }

    /// `NETLIST::GetComponentByUuid`.
    fn index_by_uuid(&self, uuid: &str) -> Option<usize> {
        if uuid.is_empty() || uuid == "00000000-0000-0000-0000-000000000000" {
            return None;
        }
        self.components.iter().position(|c| c.kiids.iter().any(|k| k == uuid))
    }

    /// `NETLIST::GetGroupByUuid`.
    pub fn group_by_uuid(&self, uuid: &str) -> Option<&NetlistGroup> {
        self.groups.iter().find(|g| g.uuid == uuid)
    }

    /// `NETLIST::ApplyGroupMembership`.
    fn apply_group_membership(&mut self) {
        for gi in 0..self.groups.len() {
            for member in self.groups[gi].members.clone() {
                if let Some(ci) = self.index_by_uuid(&member) {
                    self.components[ci].group = Some(gi);
                }
            }
        }
    }

    /// `NETLIST::SortByFPID` (`ByFPID`: `ref.GetFPID() > cmp.GetFPID()`, a stable `ptr_vector::sort`): by footprint id, the greatest first.
    pub fn sorted_by_fpid(&self) -> Vec<&NetlistComponent> {
        let mut out: Vec<&NetlistComponent> = self.components.iter().collect();
        out.sort_by(|a, b| lib_id_cmp(&b.fpid, &a.fpid));
        out
    }
}

/// `LIB_ID::compare`: the library nickname, then the item name.
pub fn lib_id_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (la, na) = split_lib_id(a);
    let (lb, nb) = split_lib_id(b);
    la.cmp(lb).then_with(|| na.cmp(nb))
}

/// `Lib:Name` -> (`Lib`, `Name`); a bare name has no library (`LIB_ID::IsLegacy`).
pub fn split_lib_id(id: &str) -> (&str, &str) {
    id.split_once(':').unwrap_or(("", id))
}

/// `LIB_ID::Parse( aId, aFix = true )` then `Format`: the nickname is up to the first `:`, and every character of the item name `isLegalChar` refuses
/// (`:`, tab, newline, return, `\`, `<`, `>`, `"`) becomes `_` (`FixIllegalChars`). With the fix on the parse never fails.
pub fn fix_lib_id(id: &str) -> String {
    let (lib, item) = match id.find(':') {
        Some(i) => (Some(&id[..i]), &id[i + 1..]),
        None => (None, id),
    };
    let item: String = item.chars().map(|c| if matches!(c, ':' | '\t' | '\n' | '\r' | '\\' | '<' | '>' | '"') { '_' } else { c }).collect();
    match lib {
        Some(l) if !l.is_empty() => format!("{l}:{item}"),
        _ => item,
    }
}

/// `KIID::KIID( const std::string& )` as text: an 8-digit (or shorter) hex legacy time stamp fills the last four octets; anything else is the text, lower-cased
/// (`AsString` writes the canonical lower-case form).
pub fn kiid_text(text: &str) -> String {
    if !text.is_empty() && text.len() <= 8 && text.chars().all(|c| c.is_ascii_hexdigit()) {
        return format!("00000000-0000-0000-0000-0000{:0>8}", text.to_ascii_lowercase());
    }
    text.to_ascii_lowercase()
}

/// `KIID_PATH::KIID_PATH( const wxString& )`: the steps of a `/`-separated path, empty steps left out, each a KIID.
pub fn path_steps(path: &str) -> Vec<String> {
    path.split('/').filter(|s| !s.is_empty()).map(kiid_text).collect()
}

/// `NETLIST_READER::NETLIST_FILE_T`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetlistFormat {
    Legacy,
    Kicad,
    Orcad,
}

/// `NETLIST_READER::GuessNetlistFileType`: the first line that matches one of the three patterns (tried in this order on each line) names the format.
/// `reLegacy` `(?i)#[ \t]+EESchema[ \t]+Netlist[ \t]+`, `reKicad` `[ ]*\(export(\s+\(version)?`, `reOrcad` `(?i)[ ]*\([ \t]+{+`, each found anywhere in the line.
pub fn guess_format(text: &str) -> Option<NetlistFormat> {
    for line in text.lines() {
        if matches_legacy(line) {
            return Some(NetlistFormat::Legacy);
        }
        if line.contains("(export") {
            return Some(NetlistFormat::Kicad);
        }
        if matches_orcad(line) {
            return Some(NetlistFormat::Orcad);
        }
    }
    None
}

fn blanks(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    i
}

fn matches_legacy(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let b = lower.as_bytes();
    (0..b.len()).filter(|&i| b[i] == b'#').any(|i| {
        let w = |at: usize, word: &str| -> Option<usize> {
            let s = blanks(b, at);
            (s > at && lower[s..].starts_with(word)).then_some(s + word.len())
        };
        w(i + 1, "eeschema").and_then(|e| w(e, "netlist")).is_some_and(|n| blanks(b, n) > n)
    })
}

fn matches_orcad(line: &str) -> bool {
    let b = line.as_bytes();
    (0..b.len()).filter(|&i| b[i] == b'(').any(|i| {
        let s = blanks(b, i + 1);
        s > i + 1 && s < b.len() && b[s] == b'{'
    })
}

/// `PCB_EDIT_FRAME::ReadNetlistFromFile`'s reading: the format guessed and the file read by its reader. `Err` is the message KiCad shows: `Cannot open
/// netlist file '%s'.` for a file of no format it knows, `Error loading netlist.\n%s` for a file its reader refuses.
pub fn read_netlist(text: &str, file_name: &str) -> Result<Netlist, String> {
    let Some(format) = guess_format(text) else {
        return Err(format!("Cannot open netlist file '{file_name}'."));
    };
    let read = match format {
        NetlistFormat::Kicad => read_kicad(text),
        NetlistFormat::Legacy | NetlistFormat::Orcad => read_legacy(text, file_name),
    };
    read.map_err(|e| format!("Error loading netlist.\n{e}"))
}

fn is_quoted(n: &Sexpr) -> bool {
    sexpr::is_quoted(n)
}

/// The text of the atom right after the tag (`NeedSYMBOLorNUMBER()`), empty when the list has none.
fn arg(list: &[Sexpr]) -> String {
    sexpr::txt(list, 1).unwrap_or_default().to_string()
}

/// `KICAD_NETLIST_PARSER::Parse`.
fn read_kicad(text: &str) -> Result<Netlist, String> {
    // the file is `(export ..)`; a file of several top-level lists is read as its first, which is the one that matters
    let start = text.find("(export").unwrap_or(0);
    let tree = sexpr::parse(&text[start..])?;
    let Some(top) = tree.as_list() else { return Err("not a netlist".into()) };
    let mut netlist = Netlist::default();
    let sections: Vec<&[Sexpr]> = if sexpr::tag(top) == Some("export") { top[1..].iter().filter_map(Sexpr::as_list).collect() } else { vec![top] };
    for section in sections {
        match sexpr::tag(section) {
            Some("components") => {
                for comp in sexpr::find_all(section, "comp") {
                    netlist.components.push(parse_component(comp));
                }
            }
            Some("groups") => {
                for g in sexpr::find_all(section, "group") {
                    netlist.groups.push(parse_group(g));
                }
            }
            Some("variants") => {
                for v in sexpr::find_all(section, "variant") {
                    // `parseVariant`: a variant with no name is none
                    let name = sexpr::find(v, "name").map(arg).unwrap_or_default();
                    let description = sexpr::find(v, "description").map(arg).unwrap_or_default();
                    if !name.is_empty() {
                        netlist.variants.push((name, description));
                    }
                }
            }
            Some("nets") => {
                for net in sexpr::find_all(section, "net") {
                    parse_net(net, &mut netlist);
                }
            }
            Some("libparts") => {
                for lp in sexpr::find_all(section, "libpart") {
                    parse_lib_part(lp, &mut netlist);
                }
            }
            _ => {}
        }
    }
    // "Go back and apply group information to the components"
    netlist.apply_group_membership();
    Ok(netlist)
}

/// `KICAD_NETLIST_PARSER::parseNet`: each node of the net is a pin of the component of its reference (one that is not there -- DNP, excluded from the
/// board -- is passed over), on the net of its name, `N-00000<code>` for a net with no name; a net whose code is not 1 or more puts no pin on it.
fn parse_net(net: &[Sexpr], netlist: &mut Netlist) {
    let (mut code, mut name) = (String::new(), String::new());
    let (mut reference, mut pin_number) = (String::new(), String::new());
    for item in net[1..].iter().filter_map(Sexpr::as_list) {
        match sexpr::tag(item) {
            Some("code") => code = arg(item),
            Some("name") => name = arg(item),
            Some("node") => {
                // "By default: no pin function or type." (the reference and the pin number carry over from the node before when a node has none)
                let (mut pin_function, mut pin_type) = (String::new(), String::new());
                for field in item[1..].iter().filter_map(Sexpr::as_list) {
                    match sexpr::tag(field) {
                        Some("ref") => reference = arg(field),
                        Some("pin") => pin_number = arg(field),
                        Some("pinfunction") => pin_function = arg(field),
                        Some("pintype") => pin_type = arg(field),
                        _ => {}
                    }
                }
                let code_value = strtol(&code);
                if let Some(component) = netlist.by_reference_mut(&reference) {
                    if code_value >= 1 {
                        if name.is_empty() {
                            name = format!("N-00000{code}");
                        }
                        component.nets.push(ComponentNet { pin: pin_number.clone(), net: name.clone(), pin_function, pin_type });
                    }
                }
            }
            _ => {}
        }
    }
}

/// `strtol( s, nullptr, 10 )`: the leading decimal number (after blanks, with a sign), 0 for none.
fn strtol(s: &str) -> i64 {
    let t = s.trim_start();
    let (sign, digits) = match t.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, t.strip_prefix('+').unwrap_or(t)),
    };
    let n: String = digits.chars().take_while(char::is_ascii_digit).collect();
    n.parse::<i64>().map(|v| sign * v).unwrap_or(0)
}

/// A flag of a variant's `(property (name ..) (value ..))`: "0" and "false" are off, "1" and "true" on, any other text on unless it is empty; no value is on.
fn variant_flag(value: Option<&str>) -> bool {
    match value {
        None => true,
        Some(v) => match v.to_lowercase().as_str() {
            "0" | "false" => false,
            "1" | "true" => true,
            _ => !v.is_empty(),
        },
    }
}

/// An `ordered_map` insert: a name seen before keeps its place and takes the new value.
fn ordered_insert(map: &mut Vec<(String, String)>, name: String, value: String) {
    match map.iter_mut().find(|(n, _)| *n == name) {
        Some(entry) => entry.1 = value,
        None => map.push((name, value)),
    }
}

/// `(fields (field (name N) "value") ..)`: each named field's last quoted string (`T_STRING`), empty for none.
fn parse_fields(list: &[Sexpr], out: &mut Vec<(String, String)>, value_list_too: bool) {
    for field in sexpr::find_all(list, "field") {
        let mut name = String::new();
        let mut value = String::new();
        for item in &field[1..] {
            match item {
                Sexpr::List(l) if sexpr::tag(l) == Some("name") => name = arg(l),
                // a variant's field may also give its value as `(value ..)`
                Sexpr::List(l) if value_list_too && sexpr::tag(l) == Some("value") => value = arg(l),
                a @ Sexpr::Atom(_) if is_quoted(a) => value = a.text().unwrap_or_default().to_string(),
                _ => {}
            }
        }
        if !name.is_empty() {
            ordered_insert(out, name, value);
        }
    }
}

/// `KICAD_NETLIST_PARSER::parseComponent`.
fn parse_component(comp: &[Sexpr]) -> NetlistComponent {
    let mut c = NetlistComponent::default();
    let mut footprint = String::new();
    for item in comp[1..].iter().filter_map(Sexpr::as_list) {
        match sexpr::tag(item) {
            Some("ref") => c.reference = arg(item),
            Some("value") => c.value = arg(item),
            Some("footprint") => footprint = arg(item),
            Some("libsource") => {
                if let Some(l) = sexpr::find(item, "lib") {
                    c.library = arg(l);
                }
                if let Some(p) = sexpr::find(item, "part") {
                    c.name = arg(p);
                }
            }
            Some("property") => {
                let name = sexpr::find(item, "name").map(arg).unwrap_or_default();
                let value = sexpr::find(item, "value").map(arg).unwrap_or_default();
                if !name.is_empty() {
                    c.properties.insert(name, value);
                }
            }
            Some("fields") => parse_fields(item, &mut c.fields, false),
            Some("sheetpath") => {
                if let Some(n) = sexpr::find(item, "names") {
                    c.human_sheet_path = arg(n);
                }
                if let Some(t) = sexpr::find(item, "tstamps") {
                    c.path = path_steps(&arg(t));
                }
            }
            Some("tstamps") => c.kiids = item[1..].iter().filter_map(Sexpr::text).map(kiid_text).collect(),
            Some("units") => {
                for unit in sexpr::find_all(item, "unit") {
                    let mut info = UnitInfo::default();
                    if let Some(n) = sexpr::find(unit, "name") {
                        info.name = arg(n);
                    }
                    if let Some(pins) = sexpr::find(unit, "pins") {
                        for pin in sexpr::find_all(pins, "pin") {
                            // "Parse pins in attribute style: (pin (num "1"))"
                            if let Some(num) = sexpr::find(pin, "num").map(arg).filter(|n| !n.is_empty()) {
                                info.pins.push(num);
                            }
                        }
                    }
                    c.units.push(info);
                }
            }
            Some("component_classes") => {
                for class in sexpr::find_all(item, "class") {
                    c.component_classes.insert(arg(class));
                }
            }
            Some("duplicate_pin_numbers_are_jumpers") => c.duplicate_pads_are_jumpers = arg(item) == "1",
            Some("jumper_pin_groups") => {
                for group in sexpr::find_all(item, "group") {
                    c.jumper_pad_groups.push(sexpr::find_all(group, "pin").map(arg).collect());
                }
            }
            Some("variants") => {
                for v in sexpr::find_all(item, "variant") {
                    let mut variant = ComponentVariant::default();
                    for field in v[1..].iter().filter_map(Sexpr::as_list) {
                        match sexpr::tag(field) {
                            Some("name") => variant.name = arg(field),
                            Some("property") => {
                                let name = sexpr::find(field, "name").map(arg).unwrap_or_default();
                                if name.is_empty() {
                                    continue;
                                }
                                let value = sexpr::find(field, "value").map(arg);
                                let flag = Some(variant_flag(value.as_deref()));
                                match name.to_lowercase().as_str() {
                                    "dnp" => variant.dnp = flag,
                                    "exclude_from_bom" => variant.excluded_from_bom = flag,
                                    "exclude_from_sim" => variant.excluded_from_sim = flag,
                                    "exclude_from_pos_files" => variant.excluded_from_pos_files = flag,
                                    _ => {}
                                }
                            }
                            Some("fields") => parse_fields(field, &mut variant.fields, true),
                            _ => {}
                        }
                    }
                    if !variant.name.is_empty() {
                        // `COMPONENT::AddVariant`: a name seen before (case-insensitively) keeps its first spelling and takes the new content
                        match c.variants.iter_mut().find(|v| v.name.to_lowercase() == variant.name.to_lowercase()) {
                            Some(old) => {
                                let name = old.name.clone();
                                *old = ComponentVariant { name, ..variant };
                            }
                            None => c.variants.push(variant),
                        }
                    }
                }
            }
            _ => {}
        }
    }
    c.variants.sort_by_key(|v| v.name.to_lowercase());
    if !footprint.is_empty() {
        c.fpid = fix_lib_id(&footprint);
    }
    c
}

/// `KICAD_NETLIST_PARSER::parseGroup`.
fn parse_group(g: &[Sexpr]) -> NetlistGroup {
    let mut out = NetlistGroup::default();
    for item in g[1..].iter().filter_map(Sexpr::as_list) {
        match sexpr::tag(item) {
            Some("name") => out.name = arg(item),
            Some("uuid") => out.uuid = kiid_text(&arg(item)),
            Some("lib_id") => out.lib_id = fix_lib_id(&arg(item)),
            Some("members") => {
                for m in sexpr::find_all(item, "member") {
                    out.members.push(sexpr::find(m, "uuid").map(|u| kiid_text(&arg(u))).unwrap_or_default());
                }
            }
            _ => {}
        }
    }
    out
}

/// `KICAD_NETLIST_PARSER::parseLibPartList`: the footprint filters and the pin count of a library part, given to every component of it (and of each of
/// its aliases).
fn parse_lib_part(lp: &[Sexpr], netlist: &mut Netlist) {
    let (mut lib, mut part) = (String::new(), String::new());
    let mut filters: Vec<String> = Vec::new();
    let mut aliases: Vec<String> = Vec::new();
    let mut pin_count = 0usize;
    for item in lp[1..].iter().filter_map(Sexpr::as_list) {
        match sexpr::tag(item) {
            Some("lib") => lib = arg(item),
            Some("part") => part = arg(item),
            // "Accept an empty (fp) sexpr.  We do write them out."
            Some("footprints") => filters.extend(sexpr::find_all(item, "fp").filter_map(|f| sexpr::txt(f, 1)).map(str::to_string)),
            Some("aliases") => aliases.extend(sexpr::find_all(item, "alias").map(arg)),
            Some("pins") => pin_count += sexpr::find_all(item, "pin").count(),
            _ => {}
        }
    }
    for c in netlist.components.iter_mut() {
        if c.is_lib_source(&lib, &part) || aliases.iter().any(|a| c.is_lib_source(&lib, a)) {
            c.footprint_filters = filters.clone();
            c.pin_count = pin_count;
        }
    }
}

/// `StrPurge`: blanks, tabs and line ends off both ends.
fn str_purge(line: &str) -> &str {
    line.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c' | '\x0b'))
}

/// `strtok( .., " ()\t\n" )`: the words of a line between those delimiters.
fn tokens(line: &str) -> Vec<&str> {
    line.split(|c: char| matches!(c, ' ' | '(' | ')' | '\t' | '\n')).filter(|w| !w.is_empty()).collect()
}

/// `LEGACY_NETLIST_READER::LoadNetlist`: a line opening a list goes a level down, one closing it a level up; at the second level a line is a component
/// (`loadComponent`), deeper it is a pin of the last one (`loadNet`). `{` starts a comment that runs to the next line holding a `}` -- **as KiCad reads
/// it**, a comment closed on its own line still hides the lines after it up to the next `}` --, and `{ Allowed footprints` before any component starts the
/// footprint filter section (`loadFootprintFilters`).
fn read_legacy(text: &str, source: &str) -> Result<Netlist, String> {
    let mut netlist = Netlist::default();
    let mut state: i32 = 0;
    let mut is_comment = false;
    let mut component: Option<usize> = None;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut i = 0usize;
    while i < lines.len() {
        let raw = lines[i];
        i += 1;
        let mut line = str_purge(raw);
        if is_comment {
            match line.find('}') {
                None => continue,
                Some(at) => line = &line[at..],
            }
            is_comment = false;
        }
        if line.starts_with('{') {
            is_comment = true;
            if state == 0 && line.len() >= 20 && line[..20].eq_ignore_ascii_case("{ Allowed footprints") {
                i = load_footprint_filters(&lines, i, &mut netlist, source)?;
                continue;
            }
            match line.find('}') {
                None => continue,
                Some(at) => line = &line[at..],
            }
        }
        if line.starts_with('(') {
            state += 1;
        }
        if line.starts_with(')') {
            state -= 1;
        }
        if state == 2 {
            component = Some(load_component(line, i, &mut netlist, source)?);
            continue;
        }
        if state >= 3 {
            let Some(ci) = component else { return Err(format!("a pin outside a symbol in '{source}' line {i}")) };
            load_net(line, i, &mut netlist.components[ci], source)?;
            state -= 1;
        }
    }
    Ok(netlist)
}

/// `LEGACY_NETLIST_READER::loadComponent`: `( /68183921-.. $noname R20 4.7K {Lib=R}` -- the time stamp, the footprint (`$noname` for none, which the
/// `.cmp` file would have given), the reference, the value and, optionally, the symbol's name.
fn load_component(line: &str, line_no: usize, netlist: &mut Netlist, source: &str) -> Result<usize, String> {
    let words = tokens(line);
    let err = |what: &str| format!("{what} in '{source}' line {line_no}: {line}");
    let stamp = words.first().ok_or_else(|| err("Cannot parse time stamp in symbol section of netlist."))?;
    let footprint = words.get(1).ok_or_else(|| err("Cannot parse footprint name in symbol section of netlist."))?;
    let reference = words.get(2).ok_or_else(|| err("Cannot parse reference designator in symbol section of netlist."))?;
    let value = words.get(3).ok_or_else(|| err("Cannot parse value in symbol section of netlist."))?;
    // "Read component name (fifth word) {Lib=C}": after the first `=`, before the last `}`
    let name = words.get(4).map(|w| {
        let after = w.split_once('=').map(|(_, r)| r).unwrap_or("");
        after.rsplit_once('}').map(|(l, _)| l).unwrap_or(after).to_string()
    });
    let footprint = if *footprint == "$noname" { String::new() } else { footprint.to_string() };
    netlist.components.push(NetlistComponent {
        // `fpid.SetLibItemName( footprintName )`: a legacy id, no library
        fpid: footprint,
        reference: reference.to_string(),
        value: value.to_string(),
        path: path_steps(stamp),
        name: name.unwrap_or_default(),
        ..Default::default()
    });
    Ok(netlist.components.len() - 1)
}

/// `LEGACY_NETLIST_READER::loadNet`: `(    1 /VCC )` -- the pin and its net, `?` for no net.
fn load_net(line: &str, line_no: usize, component: &mut NetlistComponent, source: &str) -> Result<(), String> {
    let words = tokens(line);
    let err = |what: &str| format!("{what} in '{source}' line {line_no}: {line}");
    let pin = words.first().ok_or_else(|| err("Cannot parse pin name in symbol net section of netlist."))?;
    let net = words.get(1).ok_or_else(|| err("Cannot parse net name in symbol net section of netlist."))?;
    // "? indicates no net connected to pin."
    let net = if net.starts_with('?') { String::new() } else { net.to_string() };
    component.nets.push(ComponentNet { pin: pin.to_string(), net, ..Default::default() });
    Ok(())
}

/// `LEGACY_NETLIST_READER::loadFootprintFilters`: `$component R1`, its filters a line each, `$endlist`; `$endfootprintlist` ends the section. Returns the
/// index of the line after it.
fn load_footprint_filters(lines: &[&str], mut i: usize, netlist: &mut Netlist, source: &str) -> Result<usize, String> {
    let mut filters: Vec<String> = Vec::new();
    let mut component: Option<usize> = None;
    while i < lines.len() {
        let line = lines[i].trim_end_matches(['\n', '\r']);
        i += 1;
        let starts = |p: &str, n: usize| line.len() >= n && line[..n].eq_ignore_ascii_case(&p[..n]);
        if starts("$endlist", 8) {
            if let Some(ci) = component.take() {
                netlist.components[ci].footprint_filters = std::mem::take(&mut filters);
            }
            filters.clear();
            continue;
        }
        // `strncasecmp( line, "$endfootprintlist", 4 )`: only `$end` is compared
        if starts("$endfootprintlist", 4) {
            return Ok(i);
        }
        if starts("$component", 10) {
            let reference = line.get(11..).unwrap_or("").trim();
            match netlist.components.iter().position(|c| c.reference == reference) {
                Some(ci) => component = Some(ci),
                None => return Err(format!("Cannot find symbol {reference} in footprint filter section of netlist. ('{source}' line {i})")),
            }
        } else {
            filters.push(line.get(1..).unwrap_or("").trim().to_string());
        }
    }
    Ok(i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qa(rel: &str) -> Option<String> {
        let root = std::env::var("KICAD_SRC_DIR").unwrap_or_else(|_| format!("{}/src/kicad-src-8303b2ad", std::env::var("HOME").unwrap_or_default()));
        std::fs::read_to_string(format!("{root}/qa/data/{rel}")).ok()
    }

    /// The three formats are told apart as `GuessNetlistFileType` tells them; a file of none is refused in KiCad's words.
    #[test]
    fn the_format_is_guessed_from_the_lines() {
        assert_eq!(guess_format("(export (version \"E\")\n"), Some(NetlistFormat::Kicad));
        assert_eq!(guess_format("# EESchema Netlist Version 1.1 created  15/5/2008\n("), Some(NetlistFormat::Legacy));
        assert_eq!(guess_format("#\teeschema  NETLIST \t\n"), Some(NetlistFormat::Legacy));
        assert_eq!(guess_format("( { Netliste generee par Cvpcb\n"), Some(NetlistFormat::Orcad));
        assert_eq!(guess_format("*PADS-PCB*\n*PART*\n"), None);
        assert_eq!(read_netlist("*PADS-PCB*\n", "a.net").unwrap_err(), "Cannot open netlist file 'a.net'.");
        // `# EESchema Netlist` needs a blank after the last word
        assert_eq!(guess_format("# EESchema Netlist\n"), None);
    }

    /// A component of an s-expression netlist with what `parseComponent` keeps, and its pins' nets (`parseNet`), on KiCad's own QA file.
    #[test]
    fn a_kicad_netlist_gives_components_and_their_nets() {
        let Some(text) = qa("eeschema/netlists/jumpers/jumpers.net") else { return eprintln!("skipped: no KiCad QA data") };
        let n = read_netlist(&text, "jumpers.net").unwrap();
        let r1 = n.by_reference("R1").unwrap();
        assert_eq!(r1.fpid, "Resistor_SMD:R_0603_1608Metric");
        assert_eq!(r1.value, "22");
        assert_eq!((r1.library.as_str(), r1.name.as_str()), ("Device", "R"));
        assert_eq!(r1.kiids, ["4c671539-9120-429d-9b52-8f805a6b0fad"]);
        assert!(r1.path.is_empty());
        assert_eq!(r1.footprint_path(), "/4c671539-9120-429d-9b52-8f805a6b0fad");
        assert_eq!(r1.human_sheet_path, "/");
        assert_eq!(r1.properties.get("Sheetfile").map(String::as_str), Some("jumpers.kicad_sch"));
        assert_eq!(r1.properties.get("ki_fp_filters").map(String::as_str), Some("R_*"));
        // `(field (name "Datasheet"))` has no string: an empty value
        assert_eq!(r1.fields, [("Footprint".to_string(), "Resistor_SMD:R_0603_1608Metric".to_string()), ("Datasheet".into(), "".into()), ("Description".into(), "Resistor".into())]);
        assert_eq!(r1.footprint_filters, ["R_*"]);
        assert_eq!(r1.pin_count, 2);
        assert!(r1.net("1").is_valid() && r1.net("2").is_valid());
        assert!(!r1.net("3").is_valid());
        let jumpers = n.components.iter().find(|c| c.duplicate_pads_are_jumpers).expect("a component with duplicate pins as jumpers");
        assert!(!jumpers.reference.is_empty());
        let matrix = n.components.iter().find(|c| !c.jumper_pad_groups.is_empty()).unwrap();
        assert_eq!(matrix.jumper_pad_groups, [BTreeSet::from(["1".to_string(), "3".to_string()]), BTreeSet::from(["2".to_string(), "4".to_string()])]);
    }

    /// A version D netlist (Eeschema 4): bare values, `(tstamp ..)` which 8303b2ad does not read (`T_tstamps` only), the filters from the libpart.
    #[test]
    fn a_version_d_netlist_reads_as_kicad_reads_it() {
        let Some(text) = qa("eeschema/netlists/legacy_power4/legacy_power4.net") else { return eprintln!("skipped: no KiCad QA data") };
        let n = read_netlist(&text, "legacy_power4.net").unwrap();
        assert_eq!(n.components.len(), 2);
        let r1 = n.by_reference("R1").unwrap();
        assert_eq!(r1.value, "R");
        assert_eq!(r1.fpid, "");
        assert!(r1.kiids.is_empty(), "`(tstamp ..)` is skipped");
        assert_eq!(r1.footprint_filters, ["R_*", "R_*"]);
        assert_eq!(r1.net("1").net, "VCC");
        assert_eq!(r1.net("2").net, "GND");
    }

    /// Every s-expression netlist of KiCad's QA reads, and every node of its nets that names a component lands on that component.
    #[test]
    fn every_qa_netlist_reads() {
        let root = format!("{}/src/kicad-src-8303b2ad/qa/data/eeschema/netlists", std::env::var("HOME").unwrap_or_default());
        let Ok(dirs) = std::fs::read_dir(&root) else { return eprintln!("skipped: no KiCad QA data") };
        let mut read = 0;
        for d in dirs.flatten() {
            let name = d.file_name().to_string_lossy().to_string();
            let Ok(text) = std::fs::read_to_string(d.path().join(format!("{name}.net"))) else { continue };
            let n = read_netlist(&text, &name).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!n.components.is_empty(), "{name}");
            // a node of a net carries the component's reference: count them against the file's nodes of known references
            let nodes = text.matches("(node").count();
            let pins: usize = n.components.iter().map(|c| c.nets.len()).sum();
            assert!(pins <= nodes && pins > 0, "{name}: {pins} pins of {nodes} nodes");
            read += 1;
        }
        assert!(read >= 20, "{read} netlists read");
    }

    /// The legacy format (`# EESchema Netlist Version 1.1`): a component per line, its pins below it, `?` for no net, `$noname` for no footprint, the filters
    /// section at the end -- KiCad's own QA demo.
    #[test]
    fn a_legacy_netlist_reads() {
        let Some(text) = qa("pcbnew/plugins/legacy_demos/pic_programmer/pic_programmer.net") else { return eprintln!("skipped: no KiCad QA data") };
        let n = read_netlist(&text, "pic_programmer.net").unwrap();
        assert!(n.components.len() > 40, "{}", n.components.len());
        let c = &n.components[0];
        assert!(!c.reference.is_empty() && !c.value.is_empty());
        assert_eq!(c.path.len(), 1, "the time stamp is the path: {:?}", c.path);
        assert!(n.components.iter().all(|c| !c.fpid.contains(':')), "a legacy footprint has no library");
        assert!(n.components.iter().any(|c| !c.footprint_filters.is_empty()), "the filters section gives filters");
        assert!(n.components.iter().all(|c| !c.nets.is_empty()));
        // a pin on no net (`?`) has an empty net name
        let text2 = "# EESchema Netlist Version 1.1 created  today\n(\n ( /4A5F3F2B $noname  R1 10K {Lib=R}\n  (    1 /VCC )\n  (    2 ? )\n )\n)\n*\n";
        let n2 = read_netlist(text2, "t.net").unwrap();
        let r1 = n2.by_reference("R1").unwrap();
        assert_eq!((r1.fpid.as_str(), r1.value.as_str(), r1.name.as_str()), ("", "10K", "R"));
        assert_eq!(r1.path, ["00000000-0000-0000-0000-00004a5f3f2b"]);
        assert_eq!(r1.net("1").net, "/VCC");
        assert!(r1.net("2").is_valid() && r1.net("2").net.is_empty());
    }

    /// OrCAD PCB2's `( {` format goes through the legacy reader, as in KiCad.
    #[test]
    fn an_orcad_netlist_reads_with_the_legacy_reader() {
        let Some(text) = qa("pcbnew/plugins/legacy_demos/test_xil_95108/carte_test.net") else { return eprintln!("skipped: no KiCad QA data") };
        let n = read_netlist(&text, "carte_test.net").unwrap();
        assert!(!n.components.is_empty());
    }

    /// A net with no name takes `N-00000<code>`; a net whose code is 0 puts no pin on it; a node of a reference the netlist does not have is passed over;
    /// a stacked pin stands for each pad it lists; an illegal character of a footprint name is `_`.
    #[test]
    fn nets_and_pins_follow_kicads_rules() {
        let text = r#"(export (version "E")
          (components
            (comp (ref "U1") (value "X") (footprint "Lib:A<B") (tstamps "AB12"))
            (comp (ref "R1") (value "1k") (footprint "Lib:R") (tstamps "4c671539-9120-429d-9b52-8f805a6b0fad")))
          (nets
            (net (code "1") (name "") (node (ref "U1") (pin "1")) (node (ref "NOPE") (pin "1")))
            (net (code "0") (name "Z") (node (ref "U1") (pin "2")))
            (net (code "3") (name "/S") (node (ref "U1") (pin "[3-5]") (pinfunction "D") (pintype "bidirectional")) (node (ref "R1") (pin "1")))))"#;
        let n = read_netlist(text, "t.net").unwrap();
        let u1 = n.by_reference("U1").unwrap();
        assert_eq!(u1.fpid, "Lib:A_B");
        assert_eq!(u1.kiids, ["00000000-0000-0000-0000-00000000ab12"]);
        assert_eq!(u1.net("1").net, "N-000001");
        assert!(!u1.net("2").is_valid());
        let s = u1.net("4");
        assert_eq!((s.net.as_str(), s.pin.as_str(), s.pin_function.as_str(), s.pin_type.as_str()), ("/S", "[3-5]", "D", "bidirectional"));
        assert_eq!(n.by_path("/4c671539-9120-429d-9b52-8f805a6b0fad").map(|c| c.reference.as_str()), Some("R1"));
        assert!(n.by_path("/x/4c671539-9120-429d-9b52-8f805a6b0fad").is_none());
    }

    /// Groups (`parseGroup`) and their members (`ApplyGroupMembership`), design variants and a component's variant flags and fields.
    #[test]
    fn groups_and_variants_are_read() {
        let text = r#"(export (version "E")
          (components
            (comp (ref "R1") (value "1k") (tstamps "4c671539-9120-429d-9b52-8f805a6b0fad")
              (variants (variant (name "Lite") (property (name "dnp") (value "1")) (property (name "exclude_from_bom")) (fields (field (name "Value") "2k"))))))
          (groups (group (name "G") (uuid "7B1488BE-4C43-4004-94FC-E4A26DDA8F5B") (lib_id "Blocks:B") (members (member (uuid "4c671539-9120-429d-9b52-8f805a6b0fad")))))
          (variants (variant (name "Lite") (description "No LEDs"))))"#;
        let n = read_netlist(text, "t.net").unwrap();
        assert_eq!(n.groups[0].uuid, "7b1488be-4c43-4004-94fc-e4a26dda8f5b");
        assert_eq!(n.groups[0].lib_id, "Blocks:B");
        let r1 = n.by_reference("R1").unwrap();
        assert_eq!(r1.group, Some(0));
        assert_eq!(n.variants, [("Lite".to_string(), "No LEDs".to_string())]);
        let v = &r1.variants[0];
        assert_eq!((v.dnp, v.excluded_from_bom, v.excluded_from_sim), (Some(true), Some(true), None));
        assert_eq!(v.fields, [("Value".to_string(), "2k".to_string())]);
    }
}
